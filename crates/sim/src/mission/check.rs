//! The mission check (docs/campaign.md "Checking a mission"): what cost the campaign builds
//! days, found without playing. A mission map is loaded headless and judged by the
//! simulation's own navigation (terrain, scenery blockers and buildings, body by body):
//!
//! - units, placed or scripted, that cannot walk out of where they stand;
//! - regions waves march from or to whose centre lies in a building or on closed ground;
//! - routes the real pathfinder cannot walk: from player 1 to every region, and from each
//!   wave's sources to its target, with the narrowest width along each, and a flag where
//!   player 1's large units ([`LARGE_RADIUS`]) no longer fit;
//! - scripted neutral buildings (crates, rubble) that would spawn in a narrow corridor and
//!   wall it: they are no attack-move target;
//! - every scene played skipped with its cast in place: no script error, cinematic mode
//!   off at its end (unless it won or lost the mission), and nobody left where they cannot
//!   step.
//!
//! Buildings count as each side meets them: player 1 breaks the enemy's and the neutral
//! ones (gates, rubble, crates), so its routes and every unit's room are judged without
//! them; waves attack-move through player 1's side but are walled by their own and the
//! neutral buildings.
//!
//! Each finding is one sentence that names the map, the placement or region and the
//! problem. Nothing here knows any content id.

use super::{
    CAMPAIGN_PLAYER, MissionEffect, Script, cinema::ScenePlay, effect, sandbox, state::Variable,
    systems,
};
use crate::{
    MatchSetup, Slot, SlotKind, World,
    campaign::CampaignStart,
    catalog::{Catalog, Def},
    economy::sites,
    entity::{EntityId, Milli},
    spatial::{Body, BodyRecord, Fixed, Profiles, Spatial, Spot},
    state::State,
    terrain::{Terrain, footprint::footprint_cells},
};
use canopy_content::{
    Content,
    game::Kind,
    map::{Region, Team, UtcMap, WaveSource, WaveTarget},
    placement::{Activation, Owner, Placement},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// A large unit's body radius, cells: a route of player 1's it cannot walk is flagged.
pub const LARGE_RADIUS: f64 = 3.0;
/// Cells a unit must be able to walk to from where it stands, or it is shut in.
pub const POCKET_CELLS: usize = 300;
/// The narrowest corridor (cells across) a scripted neutral building may spawn in.
pub const CORRIDOR_WIDTH: u32 = 13;
/// Ticks a skipped scene may take to end.
const SCENE_TICKS: u32 = 200;
/// Cells around a route's ends left out of its width: where units gather, not the way.
const END_CELLS: i32 = 4;
/// How far (cells) a route's end may move off a closed point onto open ground.
const SNAP_CELLS: i32 = 12;
/// The variable the dry run sets once the cast stands: spawns from then on pass.
const CAST: &str = "canopy_check_cast";
/// Appended to a scene's dry run. `on_tick` does nothing, so the scene alone plays; a
/// spawn of a cast member already standing passes; an objective the scene ends that no
/// earlier scene began here begins first (the objectives start afresh for each scene).
const DRY_RUN: &str = r#"
-- The mission check's dry run.
do
  local spawn = mission.spawn
  local begin = mission.begin_objective
  local complete = mission.complete_objective
  local fail = mission.fail_objective
  local begun = {}
  mission.spawn = function(id)
    if mission.get("canopy_check_cast") then return end
    return spawn(id)
  end
  mission.begin_objective = function(id, ...)
    begun[id] = true
    return begin(id, ...)
  end
  mission.complete_objective = function(id)
    if not begun[id] then begun[id] = true; begin(id) end
    return complete(id)
  end
  mission.fail_objective = function(id)
    if not begun[id] then begun[id] = true; begin(id) end
    return fail(id)
  end
end
function on_tick() end
"#;

/// A route the check walked.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub from: String,
    pub to: String,
    /// The body radius walked, cells: the largest of the units that take the route.
    pub radius: f64,
    /// The real pathfinder found a way.
    pub reached: bool,
    /// A large unit ([`LARGE_RADIUS`]) finds a way too (always so for waves: they are
    /// walked with their own units' bodies).
    pub large: bool,
    /// Cells across at the narrowest point of the widest way, and that point; `None`
    /// when the ground does not connect at all.
    pub width: Option<(u32, (i32, i32))>,
    /// A wave's way that its own side's buildings close until they fall (a station's
    /// gate): the script must hold the waves until then.
    pub walled: bool,
}

/// What the check found.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// What fails the check: one sentence each, naming the map.
    pub findings: Vec<String>,
    /// What is flagged but may be meant: places player 1's large units cannot enter (a
    /// secret behind a gap in the trees).
    pub warnings: Vec<String>,
    pub routes: Vec<Route>,
    /// Scripted neutral buildings and the width of the corridor each spawns in.
    pub spawns: Vec<(String, u32)>,
    /// The scenes played.
    pub scenes: Vec<String>,
}

/// Checks the mission of `map` (named `name` in findings) against `content`.
pub fn check(content: &Arc<Content>, map: &UtcMap, name: &str) -> Report {
    let _span = tracing::info_span!("mission check").entered();
    let mut report = Report::default();
    let Some(mission) = &map.mission else {
        return report;
    };
    let world = match campaign_world(content, map) {
        Ok(world) => world,
        Err(e) => {
            report
                .findings
                .push(format!("{name}: the mission does not start: {e}."));
            return report;
        }
    };
    let catalog = Catalog::new(content.clone());
    let placements = match crate::spawn::placements(map, &catalog, world.terrain(), world.setup()) {
        Ok(p) => p.list,
        Err(e) => {
            report.findings.push(format!("{name}: {e}"));
            return report;
        }
    };
    let terrain = world.terrain().clone();
    let profiles = world.spatial().profiles.clone();
    // Player 1's side: player 1 and the mission's allies.
    let allies: BTreeSet<Owner> = mission
        .players
        .iter()
        .filter(|p| p.team == Team::Ally)
        .map(|p| Owner::Player(p.player.saturating_sub(1)))
        .chain([CAMPAIGN_PLAYER])
        .collect();
    let scales = crate::world::appearance_scales(map, &terrain);
    let models = Models {
        state: world.state(),
        terrain: &terrain,
        catalog: &catalog,
        scales: &scales,
    };
    let mut check = Check {
        name,
        catalog: &catalog,
        profiles: &profiles,
        regions: &mission.regions,
        allies: &allies,
        findings: &mut report.findings,
        warnings: &mut report.warnings,
    };
    // Every building as placed.
    let mut all = models.spatial(|_| true);
    let clear = clearance(&all, &terrain);
    check.wave_regions(map, &mut all);
    report.spawns = check.corridors(&placements, &clear);
    // Waves attack-move through player 1's side; their own side's buildings may wall them
    // in until they fall (a gate the script waits for), which the routes only note.
    let mut waves = models.spatial(|o| o == Owner::Nobody);
    let mut walled = models.spatial(|o| !allies.contains(&o));
    let clear = clearance(&waves, &terrain);
    report.routes = check.wave_routes(map, &placements, &mut waves, &mut walled, &clear);
    // Player 1 breaks the rest.
    let mut open = models.spatial(|o| allies.contains(&o));
    let clear = clearance(&open, &terrain);
    check.stranded(&placements, &mut open);
    report
        .routes
        .extend(check.player_routes(map, &placements, &mut open, &clear));
    report
        .routes
        .sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));
    report.routes.dedup();
    report.scenes = match sandbox::scene_names(&mission.script) {
        Ok(names) => names,
        Err(e) => {
            report
                .findings
                .push(format!("{name}: the script does not load: {e}."));
            Vec::new()
        }
    };
    // Each dry run starts its own match: they run side by side, reported in scene order.
    let runs: Vec<Vec<String>> = std::thread::scope(|scope| {
        let runs: Vec<_> = report
            .scenes
            .iter()
            .map(|scene| scope.spawn(move || dry_run(content, map, name, scene)))
            .collect();
        runs.into_iter()
            .map(|run| {
                run.join()
                    .unwrap_or_else(|_| vec![format!("{name}: a dry run panicked.")])
            })
            .collect()
    });
    let mut seen: BTreeSet<String> = report.findings.iter().cloned().collect();
    for findings in runs {
        for finding in findings {
            if seen.insert(finding.clone()) {
                report.findings.push(finding);
            }
        }
    }
    report
}

/// The mission as the campaign starts it: player 1 the campaign's player, nothing locked.
fn campaign_world(content: &Arc<Content>, map: &UtcMap) -> Result<World, String> {
    let setup = MatchSetup {
        seed: 1,
        slots: vec![Slot {
            difficulty: Default::default(),
            player: 0,
            kind: SlotKind::Human,
            name: None,
            team: None,
            hero: None,
            race: None,
        }],
    };
    let start = CampaignStart {
        player: 0,
        locked: BTreeSet::new(),
        heroes: Vec::new(),
        hero_level_cap: None,
        variables: BTreeMap::new(),
        difficulty: Default::default(),
    };
    World::new_campaign(content.clone(), map, setup, &start).map_err(|e| e.to_string())
}

/// What spatial models are built from: the mission's first state.
struct Models<'a> {
    state: &'a State,
    terrain: &'a Arc<Terrain>,
    catalog: &'a Catalog,
    scales: &'a BTreeMap<String, f64>,
}

impl Models<'_> {
    /// The spatial model with only the buildings whose owner `keep` holds (resources and
    /// units stay).
    fn spatial(&self, keep: impl Fn(Owner) -> bool) -> Spatial {
        let mut state = self.state.clone();
        let gone: Vec<EntityId> = state
            .entities
            .iter()
            .filter(|e| self.catalog.kind(e.definition) == Kind::Building && !keep(e.owner))
            .map(|e| e.id)
            .collect();
        for id in gone {
            state.entities.remove(id);
        }
        Spatial::new(
            self.terrain.clone(),
            self.catalog,
            &state,
            self.scales.clone(),
        )
    }
}

/// A point in fixed point.
fn fixed(x: f64, y: f64) -> Fixed {
    Fixed::on(
        (x * 1000.0).round() as i32,
        (y * 1000.0).round() as i32,
        None,
    )
}

/// The cell holding a point.
fn cell(x: f64, y: f64) -> Spot {
    let p = fixed(x, y);
    let (cx, cy) = Milli { x: p.x, y: p.y }.cell();
    Spot::new(cx, cy, None)
}

fn describe(p: &Placement) -> String {
    let scripted = if p.activation == Some(Activation::Script) {
        ", a scripted spawn,"
    } else {
        ""
    };
    format!(
        "`{}` ({}{scripted} at {},{})",
        p.id,
        p.definition,
        p.position.x.round(),
        p.position.y.round()
    )
}

fn nobody(_: &BodyRecord) -> bool {
    false
}

/// Open ground near `spot` where `body` fits, units disregarded.
fn open(spatial: &Spatial, spot: Spot, body: &Body, reach: i32) -> Option<Spot> {
    spatial.nearest(spot, reach, None, Some(body), None, Some(&nobody))
}

/// Where the nearest ground `body` fits on lies, for a finding: ` (its body fits at x,y)`.
fn fits_near(spatial: &Spatial, spot: Spot, body: &Body) -> String {
    open(spatial, spot, body, SNAP_CELLS).map_or(String::new(), |s| {
        format!(" (the nearest ground its body fits on is {},{})", s.x, s.y)
    })
}

/// A route's width for a finding.
fn narrowest(width: Option<(u32, (i32, i32))>) -> String {
    match width {
        Some((w, (x, y))) => format!("its widest way narrows to {w} cells at {x},{y}"),
        None => "the ground does not connect at all".into(),
    }
}

/// The cell of `seen` inside region `r` nearest its centre.
fn inside(r: &Region, seen: &[bool], size: i32) -> Option<Spot> {
    let reach = r.radius.floor() as i32;
    let (cx, cy) = (r.x as i32, r.y as i32);
    let mut best: Option<(i64, Spot)> = None;
    for y in (cy - reach).max(0)..=(cy + reach).min(size - 1) {
        for x in (cx - reach).max(0)..=(cx + reach).min(size - 1) {
            let (dx, dy) = (i64::from(x - cx), i64::from(y - cy));
            let d = dx * dx + dy * dy;
            if f64::from(d as i32) <= r.radius * r.radius
                && seen[(y * size + x) as usize]
                && best.is_none_or(|(b, _)| d < b)
            {
                best = Some((d, Spot::new(x, y, None)));
            }
        }
    }
    best.map(|(_, s)| s)
}

/// A route's end: a name and the open cell units stand on there.
#[derive(Clone)]
struct End {
    name: String,
    spot: Option<Spot>,
}

/// The static half of the check.
struct Check<'a> {
    name: &'a str,
    catalog: &'a Catalog,
    profiles: &'a Profiles,
    regions: &'a [Region],
    allies: &'a BTreeSet<Owner>,
    findings: &'a mut Vec<String>,
    warnings: &'a mut Vec<String>,
}

impl Check<'_> {
    fn find(&mut self, message: String) {
        self.findings.push(format!("{}: {message}", self.name));
    }

    fn warn(&mut self, message: String) {
        self.warnings.push(format!("{}: {message}", self.name));
    }

    fn def(&self, p: &Placement) -> Option<Def> {
        self.catalog.find(p.definition.as_str())
    }

    /// The ground body of a placement that walks.
    fn walker(&self, p: &Placement) -> Option<Body> {
        let profile = self.profiles.get(self.def(p)?);
        (profile.kind == Kind::Unit && profile.motion.is_some() && !profile.body.air)
            .then_some(profile.body)
    }

    fn region(&self, id: &str) -> Option<&Region> {
        self.regions.iter().find(|r| r.id == id)
    }

    /// Units that cannot walk out of where they stand.
    fn stranded(&mut self, placements: &[Placement], spatial: &mut Spatial) {
        for p in placements {
            let Some(body) = self.walker(p) else {
                continue;
            };
            // Where its routes start, as the movement system starts them: a body wedged
            // against a blocker steps clear first.
            let Some(start) = spatial.route_start(fixed(p.position.x, p.position.y), &body) else {
                self.find(format!(
                    "{} cannot take a single step (terrain, scenery blockers or buildings hold its body): it will never move{}.",
                    describe(p),
                    fits_near(spatial, cell(p.position.x, p.position.y), &body)
                ));
                continue;
            };
            let room = spatial.room(start, &body, POCKET_CELLS);
            if room < POCKET_CELLS {
                self.find(format!(
                    "{} can walk to only {room} cells: terrain, scenery blockers or buildings shut it in.",
                    describe(p)
                ));
            }
        }
    }

    /// The regions waves come from or march to, by id.
    fn wave_region_ids(map: &UtcMap) -> BTreeSet<&str> {
        let mut ids = BTreeSet::new();
        for line in map
            .mission
            .iter()
            .flat_map(|m| &m.players)
            .flat_map(|p| &p.ai.waves)
        {
            if let WaveSource::Region(id) = &line.from {
                ids.insert(id.as_str());
            }
            if let WaveTarget::Region(id) = &line.target {
                ids.insert(id.as_str());
            }
        }
        ids
    }

    /// Wave regions whose centre lies in a building or on ground nobody walks: the AI
    /// sends its waves to the centre.
    fn wave_regions(&mut self, map: &UtcMap, spatial: &mut Spatial) {
        let body = self.profiles.navigation_body;
        for id in Self::wave_region_ids(map) {
            let Some(r) = self.region(id) else {
                self.find(format!(
                    "waves use region `{id}`, which the mission does not have."
                ));
                continue;
            };
            let spot = Spot::new(r.x as i32, r.y as i32, None);
            let Some(node) = spatial.node(spot) else {
                continue;
            };
            if spatial.building_at(node) {
                self.find(format!(
                    "region `{}`, which waves use, has its centre ({},{}) inside a building's footprint: waves find no route to it.",
                    r.id, r.x, r.y
                ));
            } else if !spatial.unit_walkable(spot, &body) {
                self.find(format!(
                    "region `{}`, which waves use, has its centre ({},{}) on ground nobody walks (a cliff, deep water or a scenery blocker).",
                    r.id, r.x, r.y
                ));
            }
        }
    }

    fn region_end(&self, spatial: &Spatial, id: &str, body: &Body) -> End {
        let name = format!("region `{id}`");
        let spot = self.region(id).and_then(|r| {
            let centre = Spot::new(r.x as i32, r.y as i32, None);
            open(
                spatial,
                centre,
                body,
                (r.radius as i32).clamp(1, SNAP_CELLS),
            )
        });
        End { name, spot }
    }

    /// Player 1's start, else its starting army (where its largest walker stands), and
    /// the body its routes are walked with: its smallest walker's, so a place counts as
    /// reached when any of its units gets there.
    fn player_one(
        &self,
        map: &UtcMap,
        placements: &[Placement],
        spatial: &Spatial,
    ) -> Option<(End, Body)> {
        let army: Vec<(&Placement, Body)> = placements
            .iter()
            .filter(|p| p.owner == CAMPAIGN_PLAYER && p.activation.is_none())
            .filter_map(|p| Some((p, self.walker(p)?)))
            .collect();
        let largest = army
            .iter()
            .max_by(|a, b| a.1.radius.total_cmp(&b.1.radius))
            .copied();
        let body = army
            .iter()
            .map(|&(_, b)| b)
            .min_by(|a, b| a.radius.total_cmp(&b.radius))
            .unwrap_or(self.profiles.navigation_body);
        let end = match map.player_starts.iter().find(|s| s.player == 1) {
            Some(s) => End {
                name: "player 1's start".into(),
                spot: open(spatial, cell(s.x, s.z), &body, 2 * SNAP_CELLS),
            },
            None => {
                let (p, _) = largest?;
                End {
                    name: format!("player 1's `{}`", p.id),
                    spot: open(spatial, cell(p.position.x, p.position.y), &body, SNAP_CELLS),
                }
            }
        };
        Some((end, body))
    }

    /// Player 1 to every region. A region counts as reached when player 1's units can
    /// stand anywhere inside it (scripts ask `in_region`): its smallest walker for the way
    /// there, a large unit's body ([`LARGE_RADIUS`]) for the flag.
    fn player_routes(
        &mut self,
        map: &UtcMap,
        placements: &[Placement],
        spatial: &mut Spatial,
        clear: &[u16],
    ) -> Vec<Route> {
        let Some((origin, body)) = self.player_one(map, placements, spatial) else {
            return Vec::new();
        };
        let big = Body {
            radius: body.radius.max(LARGE_RADIUS),
            ..body
        };
        let size = spatial.size() as i32;
        // Every cell a body walks to from player 1, by the route search's own steps.
        let walks = |spatial: &mut Spatial, body: &Body| {
            let mut seen = vec![false; (size * size) as usize];
            let start = origin
                .spot
                .and_then(|s| open(spatial, s, body, SNAP_CELLS))
                .and_then(|s| spatial.node(s));
            if let Some(start) = start {
                for cell in spatial.reach(start, body, usize::MAX) {
                    seen[cell as usize] = true;
                }
            }
            seen
        };
        let small = walks(spatial, &body);
        let large = walks(spatial, &big);
        let mut routes = Vec::new();
        for r in self.regions {
            let target = End {
                name: format!("region `{}`", r.id),
                spot: inside(r, &small, size)
                    .or_else(|| self.region_end(spatial, &r.id, &body).spot),
            };
            let mut route = self.walk(spatial, clear, &origin, &target, &body);
            route.large = route.reached && inside(r, &large, size).is_some();
            if route.reached && !route.large {
                self.warn(format!(
                    "{} cannot be entered from {} by a large unit (radius {LARGE_RADIUS}): {}.",
                    target.name,
                    origin.name,
                    narrowest(route.width)
                ));
            }
            routes.push(route);
        }
        routes
    }

    /// Each wave line's sources to its target.
    fn wave_routes(
        &mut self,
        map: &UtcMap,
        placements: &[Placement],
        spatial: &mut Spatial,
        walled: &mut Spatial,
        clear: &[u16],
    ) -> Vec<Route> {
        let mut routes = Vec::new();
        let one = self
            .player_one(map, placements, spatial)
            .map(|(end, _)| end);
        for player in map.mission.iter().flat_map(|m| &m.players) {
            let owner = Owner::Player(player.player.saturating_sub(1));
            for (index, line) in player.ai.waves.iter().enumerate() {
                let units: Vec<&str> = line
                    .units
                    .iter()
                    .map(|u| u.unit.as_str())
                    .chain(line.grow.iter().map(|g| g.unit.as_str()))
                    .collect();
                let Some(radius) = units
                    .iter()
                    .filter_map(|id| self.catalog.find(id))
                    .map(|d| self.profiles.get(d))
                    .filter(|p| !p.body.air)
                    .map(|p| p.body.radius)
                    .reduce(f64::max)
                else {
                    continue;
                };
                let body = Body {
                    radius,
                    ..self.profiles.navigation_body
                };
                let label = format!("player {}'s wave line {}", player.player, index + 1);
                let sources: Vec<End> = match &line.from {
                    WaveSource::Region(id) => vec![self.region_end(walled, id, &body)],
                    WaveSource::Trained => placements
                        .iter()
                        .filter(|p| p.owner == owner)
                        .filter_map(|p| {
                            let def = self.def(p)?;
                            let trains = self.catalog.get(def).behaviors.production.as_ref()?;
                            if !units
                                .iter()
                                .any(|u| trains.outputs.iter().any(|o| o.as_str() == *u))
                            {
                                return None;
                            }
                            let at = fixed(p.position.x, p.position.y);
                            let door = sites::entrance(
                                self.catalog,
                                def,
                                Milli { x: at.x, y: at.y },
                                (p.rotation * 1000.0).round() as i32,
                            );
                            let (x, y) = door.cell();
                            Some(End {
                                name: format!("`{}`", p.id),
                                spot: open(walled, Spot::new(x, y, None), &body, SNAP_CELLS),
                            })
                        })
                        .collect(),
                };
                if sources.is_empty() {
                    self.find(format!(
                        "{label} is trained, but no building of player {}'s trains its units: it sends nothing.",
                        player.player
                    ));
                }
                let target = match &line.target {
                    WaveTarget::Region(id) => self.region_end(walled, id, &body),
                    // Enemies march on player 1; allies on the enemies' bases, which are
                    // the map's to place.
                    WaveTarget::Base | WaveTarget::Nearest => {
                        match (&one, self.allies.contains(&owner)) {
                            (Some(origin), false) => origin.clone(),
                            _ => continue,
                        }
                    }
                };
                for source in &sources {
                    let mut route = self.walk(spatial, clear, source, &target, &body);
                    route.from = format!("{label} from {}", route.from);
                    if let (true, Some(a), Some(b)) = (route.reached, source.spot, target.spot)
                        && let (Some(a), Some(b)) = (walled.node(a), walled.node(b))
                    {
                        route.walled = walled
                            .find_path(a, b, None, i64::MAX, &body, None)
                            .is_none();
                    }
                    routes.push(route);
                }
            }
        }
        routes
    }

    /// Walks a route with the real pathfinder and measures its width.
    fn walk(
        &mut self,
        spatial: &mut Spatial,
        clear: &[u16],
        from: &End,
        to: &End,
        body: &Body,
    ) -> Route {
        let mut route = Route {
            from: from.name.clone(),
            to: to.name.clone(),
            radius: body.radius,
            reached: false,
            large: true,
            width: None,
            walled: false,
        };
        let (Some(a), Some(b)) = (from.spot, to.spot) else {
            let missing = if from.spot.is_none() { from } else { to };
            self.find(format!(
                "{} has no open ground for a unit of radius {} near it, so the route from {} to {} cannot start or end there.",
                missing.name, body.radius, from.name, to.name
            ));
            return route;
        };
        let (Some(start), Some(goal)) = (spatial.node(a), spatial.node(b)) else {
            return route;
        };
        route.width = widest(spatial, clear, a, b);
        route.reached = spatial
            .find_path(start, goal, None, i64::MAX, body, None)
            .is_some();
        if !route.reached {
            self.find(format!(
                "{} cannot be reached from {} by a unit of radius {}: the pathfinder finds no way ({}).",
                to.name,
                from.name,
                body.radius,
                narrowest(route.width)
            ));
        }
        route
    }

    /// Scripted neutral buildings and the corridor each spawns in; a finding where it is
    /// narrower than [`CORRIDOR_WIDTH`].
    fn corridors(&mut self, placements: &[Placement], clear: &[u16]) -> Vec<(String, u32)> {
        let size = (clear.len() as f64).sqrt() as i64;
        let mut spawns = Vec::new();
        for p in placements {
            if p.activation != Some(Activation::Script) || p.owner != Owner::Nobody {
                continue;
            }
            let Some(def) = self.def(p) else {
                continue;
            };
            let profile = self.profiles.get(def);
            if profile.kind != Kind::Building {
                continue;
            }
            let bounds = footprint_cells(p.position.x, p.position.y, profile.footprint, p.rotation);
            // The corridor's width across the deepest point of the footprint.
            let deepest = bounds
                .cells()
                .filter(|&(x, y)| x >= 0 && y >= 0 && x < size && y < size)
                .map(|(x, y)| clear[(y * size + x) as usize])
                .max()
                .unwrap_or(0);
            let width = (2 * u32::from(deepest)).saturating_sub(1);
            spawns.push((p.id.clone(), width));
            if width < CORRIDOR_WIDTH {
                self.find(format!(
                    "{} spawns in a corridor {width} cells wide (under {CORRIDOR_WIDTH}): neutral buildings are no attack-move target, so it walls the way.",
                    describe(p)
                ));
            }
        }
        spawns
    }
}

/// Per ground cell, the Chebyshev distance to the nearest cell no unit stands on (closed
/// terrain, scenery, buildings, resources) or the map's edge; 0 on such a cell.
fn clearance(spatial: &Spatial, terrain: &Terrain) -> Vec<u16> {
    let size = terrain.size() as i32;
    let mut out = vec![0u16; (size * size) as usize];
    let mut frontier = Vec::new();
    for y in 0..size {
        for x in 0..size {
            let c = (y * size + x) as usize;
            if spatial.walkable(c as u32) {
                out[c] = (x + 1).min(y + 1).min(size - x).min(size - y) as u16;
            } else {
                frontier.push((x, y));
            }
        }
    }
    let mut distance = 0u16;
    while !frontier.is_empty() {
        distance += 1;
        let mut next = Vec::new();
        for (x, y) in frontier {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= size || ny >= size {
                        continue;
                    }
                    let c = (ny * size + nx) as usize;
                    if out[c] > distance {
                        out[c] = distance;
                        next.push((nx, ny));
                    }
                }
            }
        }
        frontier = next;
    }
    out
}

/// The widest way over open ground from around `a` to around `b` ([`END_CELLS`]), never
/// crossing a step too steep: the width at its narrowest point (`2c - 1` cells for
/// clearance `c`) and that point.
fn widest(spatial: &Spatial, clear: &[u16], a: Spot, b: Spot) -> Option<(u32, (i32, i32))> {
    let size = spatial.size() as i32;
    let steep = spatial.terrain().steep_edges();
    let index = |x: i32, y: i32| (y * size + x) as usize;
    let around = |s: Spot| {
        let mut cells = Vec::new();
        for y in (s.y - END_CELLS).max(0)..=(s.y + END_CELLS).min(size - 1) {
            for x in (s.x - END_CELLS).max(0)..=(s.x + END_CELLS).min(size - 1) {
                if clear[index(x, y)] > 0 {
                    cells.push(index(x, y));
                }
            }
        }
        cells
    };
    // A bottleneck search: cells taken in falling order of the narrowest point on their
    // best way from `a`.
    let top = clear.iter().copied().max().unwrap_or(0);
    let mut best = vec![0u16; clear.len()];
    let mut pinch = vec![u32::MAX; clear.len()];
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); usize::from(top) + 1];
    // Where units gather counts as wide as any: the start's own room, the goal's.
    for c in around(a) {
        best[c] = top;
        buckets[usize::from(top)].push(c);
    }
    let goals = around(b);
    let goal: BTreeSet<usize> = goals.iter().copied().collect();
    let mut level = usize::from(top);
    while level > 0 {
        let Some(c) = buckets[level].pop() else {
            level -= 1;
            continue;
        };
        if usize::from(best[c]) != level {
            continue;
        }
        let (x, y) = (c as i32 % size, c as i32 / size);
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= size || ny >= size {
                continue;
            }
            if steep.entered(nx as usize, ny as usize, dx, dy) {
                continue;
            }
            let next = index(nx, ny);
            let own = if goal.contains(&next) && clear[next] > 0 {
                best[c]
            } else {
                clear[next]
            };
            let through = best[c].min(own);
            if through > best[next] {
                best[next] = through;
                pinch[next] = if own < best[c] { next as u32 } else { pinch[c] };
                buckets[usize::from(through)].push(next);
            }
        }
    }
    let end = goals
        .into_iter()
        .max_by_key(|&c| (best[c], std::cmp::Reverse(c)))?;
    if best[end] == 0 {
        return None;
    }
    let at = match pinch[end] {
        u32::MAX => end as u32,
        p => p,
    };
    Some((
        2 * u32::from(best[end]) - 1,
        (at as i32 % size, at as i32 / size),
    ))
}

/// The variables the script names: `mission.get("<key>")` and `mission.set("<key>", …)`.
fn named_variables(script: &str) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for call in ["mission.get(\"", "mission.set(\""] {
        let mut rest = script;
        while let Some(at) = rest.find(call) {
            rest = &rest[at + call.len()..];
            if let Some(end) = rest.find('"') {
                keys.insert(rest[..end].to_string());
                rest = &rest[end..];
            }
        }
    }
    keys
}

/// Plays scene `scene` skipped, after `on_start` (and whatever scene it plays, skipped
/// too), with every scripted placement spawned and the objectives afresh. A variable the
/// script reads by name that nothing has set yet stands in as player 1's lead unit's x
/// (names ending in `x`), its y (ending in `y`), or true: what triggers set before they
/// play a scene (where it happens, whether a side quest was done).
fn dry_run(content: &Arc<Content>, map: &UtcMap, name: &str, scene: &str) -> Vec<String> {
    let _span = tracing::info_span!("scene dry run").entered();
    let mut findings = Vec::new();
    let mut map = map.clone();
    let Some(mission) = map.mission.as_mut() else {
        return findings;
    };
    mission.script.push_str(DRY_RUN);
    let keys = named_variables(&mission.script);
    let mut world = match campaign_world(content, &map) {
        Ok(world) => world,
        Err(e) => {
            findings.push(format!(
                "{name}: the mission does not start for a dry run of scene `{scene}`: {e}."
            ));
            return findings;
        }
    };
    world.step();
    let playing = |world: &World| {
        world
            .state()
            .mission
            .as_ref()
            .and_then(|m| m.cinema.scene.as_ref())
            .map(|s| s.name.clone())
    };
    let error = |world: &World| world.state().mission.as_ref().and_then(|m| m.error.clone());
    let mut ticks = 0;
    while let Some(opening) = playing(&world) {
        if ticks == SCENE_TICKS {
            findings.push(format!(
                "{name}: scene `{opening}`, played by on_start, does not end when skipped."
            ));
            return findings;
        }
        super::skip(world.state_mut(), CAMPAIGN_PLAYER);
        world.step();
        ticks += 1;
    }
    if let Some(e) = error(&world) {
        findings.push(format!("{name}: on_start stops the script: {e}"));
        return findings;
    }
    if world.state().outcome.is_some() {
        return findings;
    }
    // The cast: every scripted placement stands, as it would by the time any scene plays.
    let catalog = Catalog::new(content.clone());
    let script: Arc<Script> = world.mission().expect("a mission").script.clone();
    let live = systems::Live { catalog: &catalog };
    let spawned = world
        .state()
        .mission
        .as_ref()
        .map(|m| m.spawned.clone())
        .unwrap_or_default();
    for p in script.placements() {
        if p.activation != Some(Activation::Script) || spawned.contains(&p.id) {
            continue;
        }
        let spawn = MissionEffect::Spawn {
            placement: p.id.clone(),
            camp: script.camp_of(&p.id).map(|c| c.id.clone()),
        };
        effect::apply(&spawn, world.state_mut(), &catalog, &script, &live);
        if let Some(m) = world.state_mut().mission.as_mut() {
            m.spawned.push(p.id.clone());
        }
    }
    world.step();
    // Player 1's lead: its largest walker.
    let lead = {
        let spatial = world.spatial();
        world
            .state()
            .entities
            .iter()
            .filter(|e| e.owner == CAMPAIGN_PLAYER && e.unit.is_some())
            .max_by(|a, b| {
                let r = |e: &crate::entity::Entity| spatial.profiles.get(e.definition).body.radius;
                r(a).total_cmp(&r(b)).then(b.id.cmp(&a.id))
            })
            .map_or(Milli::default(), |e| e.position)
    };
    let before: BTreeMap<String, Milli> = world
        .state()
        .entities
        .iter()
        .filter_map(|e| Some((e.placement.clone()?, e.position)))
        .collect();
    let tick = world.tick();
    if let Some(m) = world.state_mut().mission.as_mut() {
        for key in &keys {
            if m.variables.contains_key(key) {
                continue;
            }
            let value = if key.ends_with('x') {
                Variable::Number(f64::from(lead.x) / 1000.0)
            } else if key.ends_with('y') {
                Variable::Number(f64::from(lead.y) / 1000.0)
            } else {
                Variable::Boolean(true)
            };
            m.variables.insert(key.clone(), value);
        }
        m.variables.insert(CAST.into(), Variable::Boolean(true));
        m.objective_states.clear();
        m.cinema.scene = Some(ScenePlay {
            name: scene.to_string(),
            done: 0,
            since: tick,
            skippable: true,
            skipping: true,
            moves: Vec::new(),
        });
    }
    let mut ticks = 0;
    while playing(&world).is_some() && error(&world).is_none() && world.state().outcome.is_none() {
        if ticks == SCENE_TICKS {
            findings.push(format!(
                "{name}: scene `{scene}` does not end when skipped."
            ));
            return findings;
        }
        world.step();
        ticks += 1;
    }
    if let Some(e) = error(&world) {
        findings.push(format!(
            "{name}: scene `{scene}`, skipped with its cast in place, stops the script: {e}"
        ));
        return findings;
    }
    let state = world.state();
    if state.outcome.is_none() && state.mission.as_ref().is_some_and(|m| m.cinema.on) {
        findings.push(format!(
            "{name}: scene `{scene}` ends with cinematic mode on: the battle stays held and the player's commands refused."
        ));
    }
    // Whoever the scene put somewhere must be able to walk on from there.
    let spatial = world.spatial();
    for e in state.entities.iter() {
        let (Some(placement), Some(_)) = (e.placement.as_ref(), e.unit.as_ref()) else {
            continue;
        };
        if before.get(placement) == Some(&e.position) || e.hp.is_some_and(|hp| hp == 0) {
            continue;
        }
        let profile = spatial.profiles.get(e.definition);
        if profile.motion.is_none() || profile.body.air {
            continue;
        }
        let (x, y) = e.position.cell();
        let at = Fixed::on(e.position.x, e.position.y, None);
        if spatial.route_start(at, &profile.body).is_none() {
            findings.push(format!(
                "{name}: scene `{scene}` leaves `{placement}` at {x},{y}, where it cannot take a single step: it will never move again{}.",
                fits_near(spatial, Spot::new(x, y, None), &profile.body)
            ));
        }
    }
    findings
}
