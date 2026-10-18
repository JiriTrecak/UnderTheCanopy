//! The ability encounter (the editor's ability stage, docs/editor.md): an ability cast on
//! test targets with the real simulation, as the old spell studio's stage did. A fixture
//! around the game, not a second implementation: the content gains preview copies of the
//! caster and target units, the ability is free and ready at once, the units stand on a
//! flat sandbox map, and a [`Take`] casts, steps, records what happened and seeks by
//! replaying from the start.

use crate::{
    Action, MatchSetup, Queued, Slot, SlotKind, World,
    abilities::{AbilityEvent, EventKind, library::AbilityId},
    action::{CastTarget, PlainPoint},
    combat::ScriptedHit,
    entity::EntityId,
    math,
    state::FactKind,
};
use canopy_content::{
    Content, Id,
    abilities::{
        Ability, AbilityBinding, AbilityCaster, AbilityCommand, Activation, Amount, Control,
        Delivery, Effect, StatusLifetime, TargetingKind,
    },
    conditions::Relation as Side,
    game::{Definition, Kind},
    map::{DRY_WATER_HEIGHT, MAP_HALO, SavedScene, TerrainData, UtcMap, Version, grid::Grid},
    placement::{Aggression, Camp, Owner, Placement, Point},
};
use std::sync::Arc;

/// Most targets an encounter stands up.
pub const MAX_TARGETS: u32 = 8;
/// The caster's binding of the ability.
pub const BINDING: &str = "encounter";
/// The preview copies' definition ids.
pub const CASTER: &str = "unit.encounter.caster";
pub const TARGET: &str = "unit.encounter.target";
/// Cells per side of the sandbox map.
pub const SIZE: u32 = 256;
/// Where the caster stands (cells); the targets stand east of it.
pub const ORIGIN: [f64; 2] = [120.0, 128.0];
/// The caster's mana pool.
pub const MANA: u32 = 1000;
/// The longest take: a minute.
const LONGEST: u32 = 2400;

/// Who the targets are to the caster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Relation {
    /// The caster's own units.
    Ally,
    /// Another player's units.
    Enemy,
    /// No targets: the caster alone (`"self"`).
    #[serde(rename = "self")]
    Caster,
    /// Neutral creeps guarding a camp.
    Neutral,
}

impl Relation {
    pub const ALL: [Relation; 4] = [Self::Ally, Self::Enemy, Self::Caster, Self::Neutral];

    pub fn name(self) -> &'static str {
        match self {
            Self::Ally => "Ally",
            Self::Enemy => "Enemy",
            Self::Caster => "Self",
            Self::Neutral => "Neutral camp",
        }
    }
}

/// How an encounter is set up.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// The unit that casts (a unit definition id); it binds the ability on Q.
    pub caster: String,
    /// The unit every target is.
    pub target: String,
    /// 0 to [`MAX_TARGETS`].
    pub targets: u32,
    /// Cells between the first target and the others around it, 1 to 8.
    pub spacing: f64,
    /// Cells from the caster to the first target, 1 to 30.
    pub distance: f64,
    pub relation: Relation,
    /// The ability's rank, from 1.
    pub rank: u32,
    /// Health at the start, percent of the unit's maximum (1 to 100).
    pub caster_health: u32,
    pub target_health: u32,
    /// The caster's mana at the start, 0 to [`MANA`].
    pub mana: u32,
    /// Units keep their weapons, move and fight; otherwise only the ability acts.
    pub combat: bool,
    /// The targets start as corpses.
    pub fallen: bool,
    pub seed: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            caster: String::new(),
            target: String::new(),
            targets: 1,
            spacing: 3.0,
            distance: 8.0,
            relation: Relation::Enemy,
            rank: 1,
            caster_health: 100,
            target_health: 100,
            mana: 200,
            combat: false,
            fallen: false,
            seed: 1,
        }
    }
}

/// A scene independent of the ability (old `encounterPresets`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    relation: Relation,
    targets: u32,
    fallen: bool,
}

pub const PRESETS: [Preset; 8] = [
    Preset::new("enemy-single", "Single enemy", Relation::Enemy, 1),
    Preset::new("enemy-group", "Few enemies", Relation::Enemy, 5),
    Preset::new("ally-single", "Single ally", Relation::Ally, 1),
    Preset::new("ally-group", "Few allies", Relation::Ally, 5),
    Preset {
        fallen: true,
        ..Preset::new("fallen-allies", "Fallen allies", Relation::Ally, 6)
    },
    Preset::new("neutral-camp", "Neutral camp", Relation::Neutral, 5),
    Preset::new("empty", "Empty ground", Relation::Enemy, 0),
    Preset::new("self", "Caster only", Relation::Caster, 0),
];

impl Preset {
    const fn new(id: &'static str, name: &'static str, relation: Relation, targets: u32) -> Self {
        Self {
            id,
            name,
            relation,
            targets,
            fallen: false,
        }
    }

    /// `settings` with this scene: its relation, targets and corpses, the old studio's
    /// distance and spacing, and wounded allies (so heals show).
    pub fn apply(&self, settings: &Settings) -> Settings {
        Settings {
            relation: self.relation,
            targets: self.targets,
            fallen: self.fallen,
            distance: 8.0,
            spacing: 3.0,
            target_health: if self.relation == Relation::Ally {
                50
            } else {
                100
            },
            ..settings.clone()
        }
    }

    /// The preset `settings` match, if any.
    pub fn of(settings: &Settings) -> Option<&'static Preset> {
        PRESETS.iter().find(|p| {
            p.relation == settings.relation
                && p.targets == settings.targets
                && p.fallen == settings.fallen
        })
    }

    pub fn find(id: &str) -> Option<&'static Preset> {
        PRESETS.iter().find(|p| p.id == id)
    }
}

/// The unit definitions that bind `ability`, in id order.
pub fn binders<'a>(content: &'a Content, ability: &str) -> Vec<&'a Definition> {
    content
        .definitions
        .iter()
        .filter(|d| {
            d.behaviors
                .abilities
                .as_ref()
                .is_some_and(|a| a.bindings.iter().any(|b| b.ability.as_str() == ability))
        })
        .collect()
}

/// The units an encounter can stand up (units with a body), in id order.
pub fn units(content: &Content) -> Vec<&Definition> {
    content
        .definitions
        .iter()
        .filter(|d| d.kind == Kind::Unit && d.body.is_some())
        .filter(|d| d.id.as_str() != CASTER && d.id.as_str() != TARGET)
        .collect()
}

/// Settings that suit `ability`: a unit that binds it casts, the default race's first
/// soldier stands as the target, and the scene follows the ability's aim (enemies for
/// spells that may hit enemies, allies for the rest, the caster alone for self spells,
/// a group for areas and auras).
pub fn suggest(content: &Content, ability: &str) -> Settings {
    let mut settings = Settings::default();
    let units = units(content);
    let soldier = content
        .rules
        .races
        .get(&content.rules.default_race)
        .into_iter()
        .flat_map(|r| &r.roster)
        .filter_map(|id| content.definition(id.as_str()))
        .find(|d| {
            d.kind == Kind::Unit
                && !d.is_hero()
                && d.body.is_some()
                && d.behaviors.combat.is_some()
                && d.behaviors.work.is_none()
        })
        .or_else(|| units.first().copied());
    let caster = binders(content, ability)
        .into_iter()
        .find(|d| d.kind == Kind::Unit && d.body.is_some())
        .or(soldier);
    settings.caster = caster.map(|d| d.id.to_string()).unwrap_or_default();
    settings.target = soldier
        .or(caster)
        .map(|d| d.id.to_string())
        .unwrap_or_default();
    let Some(a) = content.abilities.ability(ability) else {
        return settings;
    };
    let area = a.targeting.radius.is_some() || a.aura.is_some();
    let preset = if a.aura.is_some() || a.activation == Activation::Passive {
        if a.aura.is_some() {
            "ally-group"
        } else {
            "enemy-single"
        }
    } else if a.targeting.kind == TargetingKind::Caster {
        "self"
    } else if a.targeting.relations.contains(&Side::Enemy) {
        if area { "enemy-group" } else { "enemy-single" }
    } else if area {
        "ally-group"
    } else {
        "ally-single"
    };
    if let Some(p) = Preset::find(preset) {
        settings = p.apply(&settings);
    }
    // Passive weapon abilities show only in a fight.
    settings.combat = a.activation == Activation::Passive && a.aura.is_none();
    settings
}

/// An ability's encounter: the content with the preview units, the sandbox map and the
/// match setup, ready to start takes from.
#[derive(Debug, Clone)]
pub struct Fixture {
    pub content: Arc<Content>,
    pub map: UtcMap,
    pub setup: MatchSetup,
    pub settings: Settings,
    /// The ability as the take casts it (free, no cooldown, no autocast).
    pub ability: Ability,
    /// The rank cast (clamped to the ability's ranks).
    pub rank: u32,
    /// Ticks a take runs before it stops.
    pub duration: u32,
}

/// A copy of `d` as `id` without what an encounter does not stage: no training,
/// levels, items or abilities of its own; its weapons only when `combat`; camp defence
/// only in a camp. It is never a hero (the old studio's default).
fn preview(d: &Definition, id: &str, combat: bool, camp: bool) -> Result<Definition, String> {
    let mut d = d.clone();
    d.id = Id::new(id)?;
    d.hero = None;
    d.creation = None;
    d.requires = None;
    d.upgrade = None;
    d.supply_cost = None;
    let b = &mut d.behaviors;
    b.progression = None;
    b.inventory = None;
    b.abilities = None;
    b.revival = None;
    b.research = None;
    b.production = None;
    b.work = None;
    b.storage = None;
    if !combat {
        b.combat = None;
    }
    if !camp {
        b.camp_defense = None;
    }
    Ok(d)
}

/// A grid of `count` equal little-endian floats.
fn floats(value: f32, count: usize) -> Grid {
    Grid(
        std::iter::repeat_n(value.to_le_bytes(), count)
            .flatten()
            .collect(),
    )
}

/// A flat, dry sandbox map of [`SIZE`] cells covered in grass, with `entities` and
/// `camps`.
pub fn sandbox(entities: Vec<Placement>, camps: Vec<Camp>) -> UtcMap {
    let span = (SIZE + 2 * MAP_HALO) as usize;
    let vertices = (span + 1) * (span + 1);
    UtcMap {
        v: Version,
        size: SIZE,
        name: "Ability encounter".into(),
        biome: None,
        description: None,
        mission: None,
        sandbox: true,
        initial_exploration: None,
        stamps: Vec::new(),
        player_starts: Vec::new(),
        entities,
        camps,
        landscape: None,
        playable: None,
        authoring: SavedScene {
            terrain: TerrainData {
                version: Version,
                size: SIZE,
                texture_grid: None,
                heights: floats(2.0, vertices),
                water_heights: floats(DRY_WATER_HEIGHT as f32, vertices),
                water_flow: Grid(
                    std::iter::repeat_n([128u8, 128, 0, 0], span * span)
                        .flatten()
                        .collect(),
                ),
                water_profiles: Vec::new(),
                grass: floats(1.0, vertices),
                rock: floats(0.0, vertices),
                paint: Vec::new(),
                shade: None,
            },
            version: Version,
            objects: Vec::new(),
        },
    }
}

/// The value of `amount` at `rank` (0 when the rank lacks it).
fn at(ability: &Ability, rank: u32, amount: &Amount) -> u32 {
    ability
        .ranks
        .get(rank.saturating_sub(1) as usize)
        .and_then(|r| amount.at(r))
        .unwrap_or(0)
        .clamp(0, i64::from(u32::MAX)) as u32
}

/// Ticks a take of `ability` at `rank` runs (old studio): the cast, its delivery and the
/// longest lasting effect, then a margin, at most a minute.
pub fn duration(ability: &Ability, rank: u32) -> u32 {
    let value = |a: &Amount| at(ability, rank, a);
    let cast = &ability.cast;
    let channel = cast
        .channel
        .as_ref()
        .map_or(0, |c| value(&c.waves) * value(&c.interval_ticks));
    let range = value(&ability.targeting.range);
    let travel = |length: f64, speed: f64| (length / speed.max(0.1) * 40.0).ceil() as u32;
    let delivery = match &ability.delivery {
        Some(Delivery::Projectile { speed }) if ability.targeting.kind != TargetingKind::Caster => {
            travel(f64::from(range), *speed)
        }
        Some(Delivery::Line { speed, length, .. }) => travel(*length, *speed),
        Some(Delivery::Chain {
            bounces,
            interval_ticks,
            ..
        }) => value(bounces) * interval_ticks,
        Some(Delivery::Swarm {
            duration_ticks,
            return_timeout_ticks,
            ..
        }) => value(duration_ticks) + return_timeout_ticks,
        Some(Delivery::Charge { max_ticks, .. }) => *max_ticks,
        _ => 0,
    };
    let lasting = ability
        .all_effects()
        .into_iter()
        .map(|e| {
            if let Some(s) = e.status() {
                return match s.lifetime {
                    StatusLifetime::UntilDeath => 1200,
                    _ => value(&s.amount),
                };
            }
            match e.effect() {
                Some(Effect::Summon(s)) => s.duration_ticks.as_ref().map_or(1200, value),
                Some(Effect::Resurrect(r)) => r.duration_ticks.as_ref().map_or(0, value),
                Some(Effect::Revive(r)) => value(&r.amount) + r.placement_wait_ticks,
                Some(e @ (Effect::Split(_) | Effect::Contain(_) | Effect::Vision(_))) => {
                    value(&e.amount())
                }
                _ => 0,
            }
        })
        .chain(
            ability
                .persistent
                .as_ref()
                .map(|p| value(&p.duration_ticks)),
        )
        .max()
        .unwrap_or(0);
    let floor = if ability.activation == Activation::Passive || ability.weapon_cast.is_some() {
        400
    } else {
        200
    };
    let body = cast.prepare_ticks + channel + delivery + cast.recover_ticks + lasting.max(80);
    (80 + body.max(floor)).min(LONGEST)
}

impl Fixture {
    /// The encounter of ability `ability` of `content` with `settings`. The content is
    /// the caller's (the editor's checked content, unsaved edits included); the fixture
    /// keeps its own copy.
    pub fn new(content: &Content, ability: &str, settings: &Settings) -> Result<Self, String> {
        let index = content
            .abilities
            .abilities
            .binary_search_by(|a| a.id.as_str().cmp(ability))
            .map_err(|_| format!("no ability {ability}"))?;
        let unit = |id: &str, role: &str| -> Result<&Definition, String> {
            content
                .definition(id)
                .filter(|d| d.kind == Kind::Unit && d.body.is_some())
                .ok_or_else(|| format!("the {role} must be a unit with a body, not {id:?}"))
        };
        let caster = unit(&settings.caster, "caster")?;
        let relation = settings.relation;
        let targets = if relation == Relation::Caster {
            0
        } else {
            settings.targets.min(MAX_TARGETS)
        };
        let target = if targets > 0 {
            Some(unit(&settings.target, "target")?)
        } else {
            None
        };
        let mut content = content.clone();
        let spell = &mut content.abilities.abilities[index];
        spell.cast.cost.amount = Amount::Fixed(0);
        spell.cast.cooldown.ticks = Amount::Fixed(0);
        if let Some(a) = &mut spell.autocast {
            a.enabled_by_default = false;
        }
        let ability = spell.clone();
        let rank = settings.rank.clamp(1, ability.ranks.len().max(1) as u32);
        let mut copy = preview(
            caster,
            CASTER,
            settings.combat || ability.weapon_cast.is_some(),
            false,
        )?;
        copy.behaviors.abilities = Some(AbilityCaster {
            max_mana: MANA,
            mana_regen_per_second: 0.0,
            bindings: vec![AbilityBinding {
                id: BINDING.into(),
                ability: ability.id.clone(),
                initial_rank: rank,
                controls: vec![Control::Player, Control::Ai, Control::Scenario],
                command: Some(AbilityCommand {
                    hotkey: "Q".into(),
                    column: 1,
                    icon: None,
                }),
                learning: None,
                ai: None,
            }],
        });
        content.put_definition(copy);
        let camp = relation == Relation::Neutral;
        if let Some(target) = target {
            content.put_definition(preview(target, TARGET, settings.combat || camp, camp)?);
        }
        // The caster faces east, towards the first target; the others stand on a circle
        // around it, each facing the caster.
        let [cx, cz] = ORIGIN;
        let distance = settings.distance.clamp(1.0, 30.0);
        let spacing = settings.spacing.clamp(1.0, 8.0);
        let owner = match relation {
            Relation::Enemy => Owner::Player(1),
            Relation::Neutral => Owner::Nobody,
            Relation::Ally | Relation::Caster => Owner::Player(0),
        };
        let place = |id: String, definition: &str, x: f64, z: f64, rotation: f64, owner| {
            Ok::<_, String>(Placement {
                activation: None,
                id,
                map_knowledge: None,
                definition: Id::new(definition)?,
                // Units stand on navigation cell centres.
                position: Point {
                    x: x.round(),
                    y: z.round(),
                    surface: None,
                },
                rotation,
                owner,
                initial_state: None,
                appearance: None,
            })
        };
        let mut entities = vec![place(
            "caster".into(),
            CASTER,
            cx,
            cz,
            90.0,
            Owner::Player(0),
        )?];
        for i in 0..targets {
            let angle = f64::from(i.saturating_sub(1)) * std::f64::consts::TAU
                / f64::from((targets - 1).max(1));
            let (dx, dz) = if i == 0 {
                (0.0, 0.0)
            } else {
                (math::cos(angle) * spacing, math::sin(angle) * spacing)
            };
            let (x, z) = (cx + distance + dx, cz + dz);
            let facing = math::atan2(cx - x, cz - z).to_degrees().rem_euclid(360.0);
            entities.push(place(format!("target-{i}"), TARGET, x, z, facing, owner)?);
        }
        let camps = if camp && targets > 0 {
            vec![Camp {
                id: "encounter-camp".into(),
                map_knowledge: None,
                members: (0..targets).map(|i| format!("target-{i}")).collect(),
                home: Point {
                    x: (cx + distance).round(),
                    y: cz,
                    surface: None,
                },
                aggro_range: 8,
                leash: 18,
                aggression: Aggression::Players,
                loot_pool: None,
                fixed_drops: None,
                legendary: None,
            }]
        } else {
            Vec::new()
        };
        let slot = |player: u8| Slot {
            player,
            kind: SlotKind::Human,
            name: None,
            team: None,
            hero: None,
            race: None,
        };
        Ok(Self {
            content: Arc::new(content),
            map: sandbox(entities, camps),
            setup: MatchSetup {
                seed: settings.seed,
                slots: vec![slot(0), slot(1)],
            },
            settings: Settings {
                targets,
                ..settings.clone()
            },
            duration: duration(&ability, rank),
            ability,
            rank,
        })
    }

    /// A fresh world of the encounter: units at their health, the caster's mana, and
    /// targets that start as corpses already fallen.
    pub fn world(&self) -> Result<World, String> {
        let mut world =
            World::new(self.content.clone(), &self.map, self.setup.clone()).map_err(|e| e.0)?;
        let share = |max: u32, percent: u32| (max * percent.clamp(1, 100)).div_ceil(100).max(1);
        let ids: Vec<(EntityId, bool)> = world
            .state()
            .entities
            .iter()
            .filter_map(|e| Some((e.id, e.placement.as_deref()? == "caster")))
            .collect();
        for (id, caster) in ids {
            let max = {
                let state = world.state();
                let e = state.entities.get(id).expect("listed");
                crate::stats::Stats::of(world.catalog(), e, crate::stats::research(state, e.owner))
                    .max_hp
            };
            let state = world.state_mut();
            let e = state.entities.get_mut(id).expect("listed");
            if caster {
                e.hp = Some(share(max, self.settings.caster_health));
                if let Some(c) = &mut e.caster {
                    c.mana = self.settings.mana.min(MANA);
                }
            } else {
                e.hp = Some(share(max, self.settings.target_health));
                if self.settings.fallen {
                    state.combat.scripted.push(ScriptedHit {
                        target: id,
                        source: None,
                        amount: max.max(1) * 10,
                    });
                }
            }
        }
        if self.settings.fallen {
            // The next combat resolution lets them fall.
            world.step();
        }
        Ok(world)
    }
}

/// Where a take's cast is aimed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Aim {
    /// The caster itself (self spells).
    Caster,
    Unit {
        entity: u32,
    },
    Point {
        x: u32,
        y: u32,
    },
}

/// What the user did to a running take, replayed at its tick when seeking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intervention {
    /// The caster is stopped (the Stop command): a cast before its release is cancelled.
    Interrupt,
}

/// A refusal the simulation reported (a cast out of range, an invalid target).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub tick: u32,
    pub message: String,
}

/// One run of the encounter: a fresh world, the cast at its first tick, the user's
/// interventions, and every ability event since. Ticks are counted from the take's start.
pub struct Take {
    fixture: Arc<Fixture>,
    world: World,
    /// The world's tick the take started at.
    start: u32,
    caster: EntityId,
    aim: Option<Aim>,
    interventions: Vec<(u32, Intervention)>,
    events: Vec<AbilityEvent>,
    refusals: Vec<Refusal>,
    seen_event: u32,
    seen_fact: u32,
    /// Commands sent so far (sequence numbers).
    seq: u32,
    /// The ability's index in the simulation's library (events name it so).
    ability: Option<AbilityId>,
}

impl Take {
    /// Starts a take: a fresh world and, unless `aim` is `None` (passives, or just
    /// watching), the cast.
    pub fn new(fixture: Arc<Fixture>, aim: Option<Aim>) -> Result<Self, String> {
        let world = fixture.world()?;
        let caster = world
            .state()
            .entities
            .iter()
            .find(|e| e.placement.as_deref() == Some("caster"))
            .map(|e| e.id)
            .ok_or("the caster could not be placed")?;
        let seen_event = world.events().all().last().map_or(0, |e| e.id);
        let seen_fact = world.state().facts.last().map_or(0, |f| f.id);
        let ability = world.catalog().spells.find(fixture.ability.id.as_str());
        let mut take = Self {
            ability,
            start: world.tick(),
            fixture,
            world,
            caster,
            aim,
            interventions: Vec::new(),
            events: Vec::new(),
            refusals: Vec::new(),
            seen_event,
            seen_fact,
            seq: 0,
        };
        if let Some(aim) = aim {
            let target = match aim {
                Aim::Caster => CastTarget::Unit { entity: caster.0 },
                Aim::Unit { entity } => CastTarget::Unit { entity },
                Aim::Point { x, y } => CastTarget::Point {
                    position: PlainPoint { x, y },
                },
            };
            take.command(Action::CastAbility {
                actor: caster.0,
                binding: BINDING.into(),
                target,
            })?;
        }
        Ok(take)
    }

    fn command(&mut self, action: Action) -> Result<(), String> {
        self.seq += 1;
        self.world
            .enqueue(Queued {
                tick: self.world.tick(),
                player: 0,
                seq: self.seq,
                action,
            })
            .map_err(|e| e.to_string())
    }

    pub fn fixture(&self) -> &Arc<Fixture> {
        &self.fixture
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    /// The caster's entity.
    pub fn caster(&self) -> EntityId {
        self.caster
    }

    pub fn aim(&self) -> Option<Aim> {
        self.aim
    }

    /// The aim at the first target as the ability aims: the caster for self spells, the
    /// first target's cell for point spells (or the cell `distance` east of the caster
    /// when there is none), the first target itself for unit spells (the caster when it
    /// stands no more). `None` for passives, which are not cast.
    pub fn first_aim(&self) -> Option<Aim> {
        let fixture = &self.fixture;
        if fixture.ability.activation == Activation::Passive {
            return None;
        }
        let first = fixture.map.entities.iter().find(|e| e.id == "target-0");
        Some(match fixture.ability.targeting.kind {
            TargetingKind::Caster => Aim::Caster,
            TargetingKind::Point => {
                let [x, z] = ORIGIN;
                let (px, pz) = first.map_or((x + fixture.settings.distance, z), |e| {
                    (e.position.x, e.position.y)
                });
                Aim::Point {
                    x: px.round().max(0.0) as u32,
                    y: pz.round().max(0.0) as u32,
                }
            }
            TargetingKind::Unit => Aim::Unit {
                entity: self
                    .world
                    .state()
                    .entities
                    .iter()
                    .find(|e| e.placement.as_deref() == Some("target-0"))
                    .map_or(self.caster.0, |e| e.id.0),
            },
        })
    }

    /// Ticks since the take started.
    pub fn tick(&self) -> u32 {
        self.world.tick() - self.start
    }

    /// The take has run its course.
    pub fn done(&self) -> bool {
        self.tick() >= self.fixture.duration
    }

    /// Ability events since the start, their ticks counted from it.
    pub fn events(&self) -> &[AbilityEvent] {
        &self.events
    }

    /// What the simulation refused the player (the encounter's side) since the start.
    pub fn refusals(&self) -> &[Refusal] {
        &self.refusals
    }

    pub fn interventions(&self) -> &[(u32, Intervention)] {
        &self.interventions
    }

    /// The desync fingerprint of the world as it stands.
    pub fn fingerprint(&self) -> u32 {
        self.world.fingerprint()
    }

    /// Simulates one tick, applying the interventions due.
    pub fn step(&mut self) {
        let now = self.tick();
        let due: Vec<Intervention> = self
            .interventions
            .iter()
            .filter(|(t, _)| *t == now)
            .map(|(_, i)| *i)
            .collect();
        for intervention in due {
            self.apply(intervention);
        }
        self.world.step();
        self.collect();
    }

    fn apply(&mut self, intervention: Intervention) {
        match intervention {
            Intervention::Interrupt => {
                let caster = self.caster.0;
                // Refused only when the caster is gone; nothing to stop then.
                let _ = self.command(Action::Stop {
                    actors: vec![caster],
                });
            }
        }
    }

    /// Takes the new events and refusals.
    fn collect(&mut self) {
        let start = self.start;
        let seen = self.seen_event;
        for e in self.world.events().all().iter().filter(|e| e.id > seen) {
            let mut e = e.clone();
            e.tick = e.tick.saturating_sub(start);
            self.events.push(e);
        }
        self.seen_event = self.world.events().all().last().map_or(seen, |e| e.id);
        let seen = self.seen_fact;
        for f in self.world.state().facts.iter().filter(|f| f.id > seen) {
            if f.kind == FactKind::Error && f.owner == Owner::Player(0) {
                self.refusals.push(Refusal {
                    tick: f.tick.saturating_sub(start),
                    message: f.message.clone(),
                });
            }
        }
        self.seen_fact = self.world.state().facts.last().map_or(seen, |f| f.id);
    }

    /// Records `intervention` at the current tick (dropping later ones: the future is
    /// rewritten) and applies it before the next step.
    pub fn intervene(&mut self, intervention: Intervention) {
        let now = self.tick();
        self.interventions.retain(|(t, _)| *t < now);
        self.interventions.push((now, intervention));
    }

    /// Runs to the take's end.
    pub fn finish(&mut self) {
        while !self.done() {
            self.step();
        }
    }

    /// A fresh take with this one's cast and interventions, at its start: replaying it
    /// gives the same world tick for tick.
    pub fn restart(&self) -> Result<Self, String> {
        Self::replay(self.fixture.clone(), self.aim, self.interventions.clone())
    }

    /// A take of `fixture` with `aim` and `interventions` at their ticks, at its start
    /// (a take again after the fixture changed, such as an edit to the ability).
    pub fn replay(
        fixture: Arc<Fixture>,
        aim: Option<Aim>,
        interventions: Vec<(u32, Intervention)>,
    ) -> Result<Self, String> {
        let mut take = Self::new(fixture, aim)?;
        take.interventions = interventions;
        Ok(take)
    }

    /// The take at `tick` (clamped to its duration): forward by stepping, backward by
    /// replaying from a fresh world with the same cast and interventions, so the result
    /// is the one stepping straight there gives.
    pub fn seek(&mut self, tick: u32) -> Result<(), String> {
        let tick = tick.min(self.fixture.duration);
        if tick < self.tick() {
            *self = self.restart()?;
        }
        while self.tick() < tick {
            self.step();
        }
        Ok(())
    }

    /// The cast's course as spans of ticks, from the events (the timeline's lanes).
    pub fn timeline(&self) -> Timeline {
        timeline(&self.fixture, self.ability, &self.events)
    }
}

/// A stretch of a take on one lane.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub label: String,
    pub start: u32,
    pub end: u32,
    pub kind: SpanKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    Turn,
    Prepare,
    Channel,
    Recover,
    Cancelled,
    Impact,
    Status,
}

/// A take's lanes: the caster's animation, the impacts, the statuses on units.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Timeline {
    pub animation: Vec<Span>,
    pub impacts: Vec<Span>,
    pub statuses: Vec<Span>,
}

/// Event kinds the impacts lane shows.
fn impact(kind: EventKind) -> bool {
    !matches!(
        kind,
        EventKind::Accepted
            | EventKind::Finished
            | EventKind::StatusApplied
            | EventKind::WaveStarted
            | EventKind::Interval
    )
}

/// An event's name as the content's presentation names it (`statusApplied`).
pub fn event_name(kind: EventKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The lanes of a take of `fixture` with `events`; `id` is the ability's index in the
/// simulation's library.
pub fn timeline(fixture: &Fixture, id: Option<AbilityId>, events: &[AbilityEvent]) -> Timeline {
    let ability = &fixture.ability;
    let duration = fixture.duration;
    let animations = &ability.presentation.animations;
    let mut t = Timeline::default();
    let ours = |e: &&AbilityEvent| Some(e.ability) == id;
    let first = |kind: EventKind| events.iter().filter(ours).find(|e| e.kind == kind);
    if ability.activation == Activation::Passive {
        t.animation.push(Span {
            label: if ability.aura.is_some() {
                "Aura active".into()
            } else {
                "Passive".into()
            },
            start: 0,
            end: duration,
            kind: SpanKind::Channel,
        });
    } else if let Some(accepted) = first(EventKind::Accepted) {
        let prepare = ability.cast.prepare_ticks;
        let channel = ability.cast.channel.as_ref().map_or(0, |c| {
            at(ability, fixture.rank, &c.waves) * at(ability, fixture.rank, &c.interval_ticks)
        });
        let cancelled = first(EventKind::Cancelled).map(|e| e.tick);
        let release = first(EventKind::Released)
            .map(|e| e.tick)
            .unwrap_or_else(|| accepted.tick + prepare);
        let finished = first(EventKind::Finished)
            .map(|e| e.tick)
            .unwrap_or(release + channel + ability.cast.recover_ticks);
        let begun = release.saturating_sub(prepare).max(accepted.tick);
        let mut spans = Vec::new();
        if begun > accepted.tick {
            spans.push(("Turn".to_string(), accepted.tick, begun, SpanKind::Turn));
        }
        spans.push((
            format!("Prepare · {}", animations.prepare),
            begun,
            release,
            SpanKind::Prepare,
        ));
        if channel > 0 {
            spans.push((
                format!(
                    "Channel · {}",
                    animations.channel.as_deref().unwrap_or(&animations.prepare)
                ),
                release,
                release + channel,
                SpanKind::Channel,
            ));
        }
        spans.push((
            format!("Recover · {}", animations.recover),
            release + channel,
            finished.max(release + channel),
            SpanKind::Recover,
        ));
        for (label, start, mut end, kind) in spans {
            if let Some(c) = cancelled {
                if start >= c {
                    continue;
                }
                end = end.min(c);
            }
            t.animation.push(Span {
                label,
                start,
                end: end.max(start + 1),
                kind,
            });
        }
        if let Some(c) = cancelled {
            let reason = first(EventKind::Cancelled)
                .and_then(|e| e.reason.clone())
                .unwrap_or_default();
            t.animation.push(Span {
                label: format!("Cancelled {reason}").trim().to_string(),
                start: c,
                end: c + 1,
                kind: SpanKind::Cancelled,
            });
        }
    }
    for e in events.iter().filter(|e| impact(e.kind)) {
        let name = event_name(e.kind);
        let label = match e.amount {
            Some(amount) => format!("{name} · {amount}"),
            None => name,
        };
        let length = match e.kind {
            EventKind::Projectile => e.duration_ticks.unwrap_or(1).max(1),
            _ => 1,
        };
        t.impacts.push(Span {
            label,
            start: e.tick,
            end: (e.tick + length).min(duration.max(e.tick + 1)),
            kind: if e.kind == EventKind::Cancelled {
                SpanKind::Cancelled
            } else {
                SpanKind::Impact
            },
        });
    }
    for e in events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::StatusApplied | EventKind::Summoned))
    {
        // A status ends at its duration, a dispel or its holder's death, whichever is first.
        let ended = events
            .iter()
            .filter(|o| o.tick >= e.tick && o.target == e.target && e.target.is_some())
            .find(|o| matches!(o.kind, EventKind::Dispelled | EventKind::Death))
            .map(|o| o.tick);
        let length = match (e.duration_ticks, e.until_death) {
            (_, true) | (None, _) => duration.saturating_sub(e.tick),
            (Some(d), false) => d,
        };
        let end = (e.tick + length.max(1))
            .min(ended.unwrap_or(u32::MAX))
            .min(duration);
        let label = match e.kind {
            EventKind::Summoned => format!("{} summoned", e.amount.unwrap_or(1)),
            _ => format!(
                "{} · unit {}",
                e.status.as_deref().unwrap_or("status"),
                e.target.map_or(0, |t| t.0)
            ),
        };
        t.statuses.push(Span {
            label,
            start: e.tick,
            end: end.max(e.tick + 1),
            kind: SpanKind::Status,
        });
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sandbox_is_a_valid_map() {
        let map = sandbox(Vec::new(), Vec::new());
        let value = serde_json::to_value(&map).unwrap();
        let parsed = UtcMap::from_value(value, "encounter").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(parsed.size, SIZE);
        assert!(parsed.sandbox);
    }

    #[test]
    fn presets_set_the_scene_and_are_recognised() {
        let base = Settings {
            caster: "unit.a".into(),
            target: "unit.b".into(),
            distance: 20.0,
            ..Settings::default()
        };
        for preset in &PRESETS {
            let s = preset.apply(&base);
            assert_eq!(Preset::of(&s).map(|p| p.id), Some(preset.id));
            assert_eq!((s.caster.as_str(), s.distance), ("unit.a", 8.0));
        }
        let fallen = Preset::find("fallen-allies").unwrap().apply(&base);
        assert!(fallen.fallen && fallen.relation == Relation::Ally);
        let custom = Settings {
            targets: 3,
            ..Preset::find("enemy-single").unwrap().apply(&base)
        };
        assert!(Preset::of(&custom).is_none());
    }
}
