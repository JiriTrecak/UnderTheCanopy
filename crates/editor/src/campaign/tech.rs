//! The Tech view (U) of a campaign's mission map (docs/campaign.md "Tech"): the campaign
//! race's whole tree drawn from the content, laid out by hall tier. The race's hall
//! upgrade line runs across the top, one column per tier; every other building stands in
//! the column of the first hall tier it needs, carrying the units it trains first and its
//! research (chains in a row). Each node shows its state in the mission shown and a click
//! moves it through Locked, New and Blocked (an earlier unlock between Available and
//! Blocked), writing the step's `tech`. What needs a blocked or locked building is
//! hatched; something allowed that cannot be reached is a contradiction, in red.
//! Positions are computed from the content every frame, never saved.

use super::{MissionLink, StepKind, Tech, TechState, mission_numbers, steps, text};
use crate::document::Context;
use crate::icons::{self, Icon};
use crate::library::Kind as Editor;
use crate::tabs::Edit;
use crate::theme::*;
use canopy_content::Content;
use canopy_content::game::Kind;
use egui::{Align2, Color32, FontId, Id, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use std::collections::{BTreeMap, BTreeSet};

/// A column's width, the gap between columns, and the margin round the tree.
const COLUMN: f32 = 252.0;
const GAP: f32 = 64.0;
const MARGIN: f32 = 28.0;
const HEADER: f32 = 62.0;
const ROW: f32 = 38.0;
const CARD_GAP: f32 = 16.0;
const CHIP: f32 = 28.0;
/// Room above the cards for the tier names.
const TITLE: f32 = 36.0;

pub const NEW: Color32 = Color32::from_rgb(0x34, 0xd3, 0x99);
pub const BLOCKED: Color32 = Color32::from_rgb(0xfb, 0x92, 0x3c);
pub const HALL: Color32 = Color32::from_rgb(0xe9, 0xc4, 0x6a);

/// A unit or building of the race as the tree needs it.
#[derive(Debug, Clone, Default)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub building: bool,
    pub hero: bool,
    /// Display order, then roster order.
    pub order: (i64, usize),
    pub requires: Vec<String>,
    /// A building's upgrade.
    pub upgrade: Option<String>,
    pub trains: Vec<String>,
    pub researches: Vec<String>,
    /// What a worker puts up.
    pub builds: Vec<String>,
}

/// A research as the tree needs it.
#[derive(Debug, Clone, Default)]
pub struct Research {
    pub name: String,
    pub icon: String,
    pub requires: Vec<String>,
    pub follows: Option<String>,
}

/// What the tree is made from: a race's units and buildings (and the units they train),
/// its research, its first hall and what its starting setup places.
#[derive(Debug, Clone, Default)]
pub struct Source {
    pub items: Vec<Item>,
    pub research: BTreeMap<String, Research>,
    pub fort: String,
    pub placed: Vec<String>,
}

impl Source {
    /// `race`'s tree in `content` (empty for a race it does not have).
    pub fn of(content: &Content, race: &str) -> Self {
        let Some(profile) = content.rules.race(race) else {
            return Source::default();
        };
        let roster: Vec<String> = profile
            .race
            .roster
            .iter()
            .map(|id| id.to_string())
            .collect();
        let mut ids = roster.clone();
        // Units the roster's buildings train, and upgrades, though the roster omits them.
        for id in &roster {
            if let Some(d) = content.definition(id) {
                let more = d
                    .behaviors
                    .production
                    .iter()
                    .filter(|p| p.mode == canopy_content::game::ProductionMode::Queued)
                    .flat_map(|p| p.outputs.iter())
                    .chain(d.upgrade.iter().map(|u| &u.target));
                for out in more {
                    if !ids.iter().any(|i| i == out.as_str()) {
                        ids.push(out.to_string());
                    }
                }
            }
        }
        let strings = |list: &[canopy_content::Id]| list.iter().map(|i| i.to_string()).collect();
        let items = ids
            .iter()
            .filter_map(|id| content.definition(id))
            .filter(|d| matches!(d.kind, Kind::Unit | Kind::Building))
            .map(|d| Item {
                id: d.id.to_string(),
                name: d.name.clone(),
                icon: d.icon.to_string(),
                building: d.kind == Kind::Building,
                hero: d.is_hero(),
                order: (
                    d.display_order.unwrap_or(i64::MAX),
                    roster
                        .iter()
                        .position(|r| r == d.id.as_str())
                        .unwrap_or(usize::MAX),
                ),
                requires: d.requires.as_deref().map(strings).unwrap_or_default(),
                upgrade: d.upgrade.as_ref().map(|u| u.target.to_string()),
                // What the race's tech holds (`canopy_content::campaign::race_tech`):
                // queued training, not what a building makes by itself.
                trains: d
                    .behaviors
                    .production
                    .as_ref()
                    .filter(|p| p.mode == canopy_content::game::ProductionMode::Queued)
                    .map(|p| strings(&p.outputs))
                    .unwrap_or_default(),
                researches: d
                    .behaviors
                    .research
                    .as_ref()
                    .map(|r| strings(&r.outputs))
                    .unwrap_or_default(),
                builds: d
                    .behaviors
                    .work
                    .as_ref()
                    .map(|w| strings(&w.builds))
                    .unwrap_or_default(),
            })
            .collect::<Vec<_>>();
        let research = items
            .iter()
            .flat_map(|i| i.researches.iter())
            .filter_map(|id| {
                let r = content.rules.research.get(id.as_str())?;
                Some((
                    id.clone(),
                    Research {
                        name: r.name.clone(),
                        icon: r.icon.to_string(),
                        requires: r.requires.as_deref().map(strings).unwrap_or_default(),
                        follows: r.follows.as_ref().map(|f| f.to_string()),
                    },
                ))
            })
            .collect();
        let setup = profile.starting_setup;
        Source {
            items,
            research,
            fort: setup.fort.to_string(),
            placed: std::iter::once(setup.fort.to_string())
                .chain(setup.units.iter().map(|u| u.definition.to_string()))
                .collect(),
        }
    }

    fn item(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    fn requires(&self, id: &str) -> Vec<String> {
        match (self.item(id), self.research.get(id)) {
            (Some(i), _) => i.requires.clone(),
            (None, Some(r)) => r.requires.clone(),
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Hall,
    Building,
    Unit,
    Hero,
    Research,
}

/// One thing a mission's tech can allow: a building, a unit or a research.
#[derive(Debug, Clone)]
pub struct Node {
    pub id: String,
    pub name: String,
    /// Its picture in the pack (`<icon>/image.png`).
    pub icon: Option<String>,
    pub kind: NodeKind,
    /// The hall tier it needs (0: the first hall).
    pub tier: usize,
    /// Buildings and research it requires.
    pub requires: Vec<String>,
    /// Where it comes from: the buildings that train or research it, the building it is
    /// an upgrade of (a hall: the hall before it).
    pub made_at: Vec<String>,
    /// Workers of the race can put it up.
    pub built: bool,
    /// What it puts up, when it is a worker.
    pub builds: Vec<String>,
    /// In graph units (points at zoom 1).
    pub rect: Rect,
    /// The card it stands in.
    pub card: usize,
}

/// A building's card: its header and the units and research it carries.
#[derive(Debug, Clone)]
pub struct Card {
    pub node: usize,
    pub rect: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// The hall line, and a building's upgrade.
    Upgrade,
    /// A building that requires another building.
    Requires,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub kind: EdgeKind,
}

/// A research chain's links (by node), drawn in one row with arrows between.
#[derive(Debug, Clone)]
pub struct Chain(pub Vec<usize>);

/// The race's tree, laid out.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub cards: Vec<Card>,
    pub edges: Vec<Edge>,
    pub chains: Vec<Chain>,
    /// The hall line: one node per tier.
    pub halls: Vec<usize>,
    /// What the race's starting setup places (they stand without tech).
    pub present: BTreeSet<String>,
    pub size: egui::Vec2,
}

fn picture(icon: &str) -> Option<String> {
    (!icon.is_empty()).then(|| format!("{icon}/image.png"))
}

/// The hall tier `id` needs: the highest hall among what it requires (through buildings
/// and research) and an upgrade's source.
fn tier_of(
    id: &str,
    source: &Source,
    halls: &[String],
    upgraded_from: &BTreeMap<String, String>,
    tiers: &mut BTreeMap<String, usize>,
    depth: usize,
) -> usize {
    if let Some(t) = tiers.get(id) {
        return *t;
    }
    if let Some(t) = halls.iter().position(|h| h == id) {
        tiers.insert(id.to_string(), t);
        return t;
    }
    if depth > 24 {
        return 0;
    }
    let mut needs = source.requires(id);
    needs.extend(upgraded_from.get(id).cloned());
    let tier = needs
        .iter()
        .map(|need| tier_of(need, source, halls, upgraded_from, tiers, depth + 1))
        .max()
        .unwrap_or(0);
    tiers.insert(id.to_string(), tier);
    tier
}

impl Graph {
    /// The tree of `source`; `keep` says which nodes show (all, or only a mission's
    /// changes). A building's card shows while it or anything on it is kept; the halls
    /// always do.
    pub fn build(source: &Source, keep: &dyn Fn(&str) -> bool) -> Self {
        let buildings: Vec<&Item> = source.items.iter().filter(|i| i.building).collect();
        // The hall line: the setup's fort, then its upgrades.
        let mut halls: Vec<String> = Vec::new();
        let mut hall = source.item(&source.fort).map(|i| i.id.clone());
        while let Some(h) = hall.take() {
            if halls.contains(&h) {
                break;
            }
            hall = source.item(&h).and_then(|i| i.upgrade.clone());
            halls.push(h);
        }
        let built: BTreeSet<&str> = source
            .items
            .iter()
            .flat_map(|i| i.builds.iter().map(String::as_str))
            .collect();
        let mut upgraded_from: BTreeMap<String, String> = BTreeMap::new();
        for b in &buildings {
            if let Some(target) = &b.upgrade {
                upgraded_from.entry(target.clone()).or_insert(b.id.clone());
            }
        }
        // Who trains and researches what: the hall line first, then by order.
        let mut producers: Vec<&Item> = halls.iter().filter_map(|h| source.item(h)).collect();
        let mut rest: Vec<&Item> = buildings
            .iter()
            .copied()
            .filter(|b| !halls.contains(&b.id))
            .collect();
        rest.sort_by(|a, b| (a.order, &a.name).cmp(&(b.order, &b.name)));
        producers.extend(rest);
        let mut trained_at: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        let mut researched_at: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for b in &producers {
            for out in &b.trains {
                trained_at.entry(out).or_default().push(b.id.clone());
            }
            for out in &b.researches {
                researched_at.entry(out).or_default().push(b.id.clone());
            }
        }
        let mut tiers = BTreeMap::new();
        let mut tier = |id: &str| tier_of(id, source, &halls, &upgraded_from, &mut tiers, 0);

        let mut graph = Graph {
            present: source.placed.iter().cloned().collect(),
            ..Graph::default()
        };
        // Each building's card: the building, the units it is the first to train, its
        // research in chains (each level after the one it follows).
        type Contents = (usize, Vec<(String, NodeKind)>, Vec<Vec<String>>);
        let mut cards: Vec<Contents> = Vec::new();
        let mut trained = BTreeSet::new();
        for b in &producers {
            let kind = if halls.contains(&b.id) {
                NodeKind::Hall
            } else {
                NodeKind::Building
            };
            let mut units = Vec::new();
            for out in &b.trains {
                let Some(u) = source.item(out).filter(|u| !u.building) else {
                    continue;
                };
                if !trained.insert(out.clone()) || !keep(out) {
                    continue;
                }
                let kind = if u.hero {
                    NodeKind::Hero
                } else {
                    NodeKind::Unit
                };
                units.push((out.clone(), kind));
            }
            let follows = |id: &str| source.research.get(id).and_then(|r| r.follows.clone());
            let mut chains: Vec<Vec<String>> = Vec::new();
            for out in &b.researches {
                if follows(out).is_some_and(|f| b.researches.contains(&f)) {
                    continue;
                }
                let mut chain = vec![out.clone()];
                while let Some(next) = b.researches.iter().find(|o| {
                    follows(o).as_deref() == chain.last().map(String::as_str) && !chain.contains(o)
                }) {
                    chain.push(next.clone());
                }
                chain.retain(|id| keep(id));
                if !chain.is_empty() {
                    chains.push(chain);
                }
            }
            if kind != NodeKind::Hall && !keep(&b.id) && units.is_empty() && chains.is_empty() {
                continue;
            }
            let node = graph.nodes.len();
            let made_at = match kind {
                // A hall comes from the one before it.
                NodeKind::Hall => halls
                    .iter()
                    .position(|h| *h == b.id)
                    .filter(|&i| i > 0)
                    .map(|i| halls[i - 1].clone())
                    .into_iter()
                    .collect(),
                _ => upgraded_from.get(&b.id).cloned().into_iter().collect(),
            };
            graph.nodes.push(Node {
                id: b.id.clone(),
                name: b.name.clone(),
                icon: picture(&b.icon),
                kind,
                tier: tier(&b.id),
                requires: b.requires.clone(),
                made_at,
                built: built.contains(b.id.as_str()),
                builds: Vec::new(),
                rect: Rect::NOTHING,
                card: cards.len(),
            });
            cards.push((node, units, chains));
        }
        graph.halls = halls
            .iter()
            .filter_map(|h| graph.nodes.iter().position(|n| n.id == *h))
            .collect();
        // Columns: one per hall tier; the halls across the top, aligned.
        let columns = graph.halls.len().max(1);
        let x = |tier: usize| MARGIN + tier.min(columns - 1) as f32 * (COLUMN + GAP);
        let height = |rows: usize| HEADER + rows as f32 * ROW + if rows > 0 { 8.0 } else { 0.0 };
        let top = cards
            .iter()
            .filter(|(n, _, _)| graph.nodes[*n].kind == NodeKind::Hall)
            .map(|(_, u, c)| height(u.len() + c.len()))
            .fold(HEADER, f32::max);
        let mut bottoms = vec![MARGIN + TITLE + top + CARD_GAP * 2.0; columns];
        for (node, units, chains) in cards {
            let column = graph.nodes[node].tier.min(columns - 1);
            let hall = graph.nodes[node].kind == NodeKind::Hall;
            let h = height(units.len() + chains.len());
            let y = if hall {
                MARGIN + TITLE
            } else {
                bottoms[column]
            };
            let rect = Rect::from_min_size(pos2(x(column), y), vec2(COLUMN, h));
            if !hall {
                bottoms[column] += h + CARD_GAP;
            }
            let card = graph.cards.len();
            graph.nodes[node].rect = Rect::from_min_size(rect.min, vec2(COLUMN, HEADER));
            graph.cards.push(Card { node, rect });
            let mut row = rect.min.y + HEADER;
            for (unit, kind) in units {
                let item = source.item(&unit);
                graph.nodes.push(Node {
                    name: item.map_or(unit.clone(), |i| i.name.clone()),
                    icon: item.and_then(|i| picture(&i.icon)),
                    kind,
                    tier: tier(&unit),
                    requires: source.requires(&unit),
                    made_at: trained_at.get(unit.as_str()).cloned().unwrap_or_default(),
                    built: false,
                    builds: item.map(|i| i.builds.clone()).unwrap_or_default(),
                    rect: Rect::from_min_size(
                        pos2(rect.min.x + 8.0, row),
                        vec2(COLUMN - 16.0, ROW - 4.0),
                    ),
                    card,
                    id: unit,
                });
                row += ROW;
            }
            for chain in chains {
                let single = chain.len() == 1;
                let mut links = Vec::new();
                for (k, research) in chain.into_iter().enumerate() {
                    let r = source.research.get(&research);
                    let at = pos2(rect.min.x + 8.0 + k as f32 * (CHIP + 18.0), row);
                    let size = if single {
                        vec2(COLUMN - 16.0, ROW - 4.0)
                    } else {
                        vec2(CHIP + 6.0, ROW - 4.0)
                    };
                    links.push(graph.nodes.len());
                    graph.nodes.push(Node {
                        name: r.map_or(research.clone(), |r| r.name.clone()),
                        icon: r.and_then(|r| picture(&r.icon)),
                        kind: NodeKind::Research,
                        tier: tier(&research),
                        requires: source.requires(&research),
                        made_at: researched_at
                            .get(research.as_str())
                            .cloned()
                            .unwrap_or_default(),
                        built: false,
                        builds: Vec::new(),
                        rect: Rect::from_min_size(at, size),
                        card,
                        id: research,
                    });
                }
                graph.chains.push(Chain(links));
                row += ROW;
            }
        }
        // Edges between cards: the hall line, upgrades, requirements on other buildings
        // (a hall needed shows as the column instead).
        let card_of = |id: &str, graph: &Graph| {
            graph
                .nodes
                .iter()
                .position(|n| n.id == id && matches!(n.kind, NodeKind::Hall | NodeKind::Building))
        };
        for to in 0..graph.nodes.len() {
            let n = &graph.nodes[to];
            if !matches!(n.kind, NodeKind::Hall | NodeKind::Building) {
                continue;
            }
            let (made_at, requires) = (n.made_at.clone(), n.requires.clone());
            let upgrades: Vec<usize> = made_at.iter().filter_map(|m| card_of(m, &graph)).collect();
            let needs: Vec<usize> = requires
                .iter()
                .filter(|r| !halls.contains(r))
                .filter_map(|r| card_of(r, &graph))
                .collect();
            for from in upgrades {
                graph.edges.push(Edge {
                    from,
                    to,
                    kind: EdgeKind::Upgrade,
                });
            }
            for from in needs {
                graph.edges.push(Edge {
                    from,
                    to,
                    kind: EdgeKind::Requires,
                });
            }
        }
        let right = graph.cards.iter().map(|c| c.rect.max.x).fold(0.0, f32::max);
        let bottom = graph.cards.iter().map(|c| c.rect.max.y).fold(0.0, f32::max);
        graph.size = vec2(right + MARGIN, bottom + MARGIN);
        graph
    }

    fn find(&self, id: &str) -> impl Iterator<Item = &Node> {
        self.nodes.iter().filter(move |n| n.id == id)
    }

    fn name(&self, id: &str) -> String {
        self.find(id)
            .next()
            .map_or_else(|| id.to_string(), |n| n.name.clone())
    }
}

/// Whether each node can be had in a mission: whether what it needs is there, and what
/// is reachable (allowed with what it needs there, or placed by the starting setup).
#[derive(Debug, Default)]
pub struct Reach {
    /// By id: what it needs is there, or why not.
    pub needs: BTreeMap<String, Result<(), String>>,
    pub reached: BTreeSet<String>,
}

impl Reach {
    /// Works out reachability for `graph`'s nodes under `tech`.
    pub fn of(graph: &Graph, tech: &Tech) -> Self {
        let ids: BTreeSet<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
        let needs_of = |n: &Node, reached: &BTreeSet<String>| -> Result<(), String> {
            // Requirements outside the race's tree (neutral, other races) are not tech
            // this view knows about.
            for need in &n.requires {
                if ids.contains(need.as_str()) && !reached.contains(need) {
                    return Err(format!("needs {}", graph.name(need)));
                }
            }
            let from_source = n.made_at.iter().any(|m| reached.contains(m));
            let worker = || {
                graph
                    .nodes
                    .iter()
                    .any(|w| reached.contains(&w.id) && w.builds.contains(&n.id))
            };
            let ok = match n.kind {
                NodeKind::Hall | NodeKind::Building => from_source || (n.built && worker()),
                _ => from_source,
            };
            if ok {
                return Ok(());
            }
            let sources: Vec<String> = n.made_at.iter().map(|m| graph.name(m)).collect();
            Err(match n.kind {
                NodeKind::Research => {
                    format!("nothing here researches it ({})", sources.join(", "))
                }
                NodeKind::Unit | NodeKind::Hero => {
                    format!("nothing here trains it ({})", sources.join(", "))
                }
                _ if n.built && sources.is_empty() => "no worker here can build it".into(),
                _ if n.built => format!(
                    "no worker here can build it, nor is {} here",
                    sources.join(", ")
                ),
                _ => format!("needs {}", sources.join(", ")),
            })
        };
        let mut reached: BTreeSet<String> = graph.present.clone();
        // Grow what is reachable until nothing more joins.
        loop {
            let joined: Vec<String> = graph
                .nodes
                .iter()
                .filter(|n| !reached.contains(&n.id) && tech.state(&n.id).allowed())
                .filter(|n| needs_of(n, &reached).is_ok())
                .map(|n| n.id.clone())
                .collect();
            if joined.is_empty() {
                break;
            }
            reached.extend(joined);
        }
        let needs = graph
            .nodes
            .iter()
            .map(|n| (n.id.clone(), needs_of(n, &reached)))
            .collect();
        Reach { needs, reached }
    }

    /// What is allowed although what it needs is not there: (id, why).
    pub fn contradictions(&self, tech: &Tech) -> Vec<(String, String)> {
        self.needs
            .iter()
            .filter(|(id, _)| tech.state(id).allowed())
            .filter_map(|(id, needs)| needs.clone().err().map(|why| (id.clone(), why)))
            .collect()
    }
}

/// The Tech view's state in a map's session.
#[derive(Debug, Clone)]
pub struct View {
    /// The tree's offset on screen and its zoom.
    pub pan: egui::Vec2,
    pub zoom: f32,
    /// The mission shown (a step index) when it is not the map's own.
    pub mission: Option<usize>,
    /// Only what the mission changes (and what contradicts).
    pub changes_only: bool,
    /// Fit the tree to the view on the next frame.
    pub fit: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            mission: None,
            changes_only: false,
            fit: true,
        }
    }
}

fn state_color(state: TechState) -> Color32 {
    match state {
        TechState::New => NEW,
        TechState::Blocked => BLOCKED,
        TechState::Available => SOFT,
        TechState::Locked => FAINT,
    }
}

/// Diagonal hatching over `rect`.
fn hatch(painter: &egui::Painter, rect: Rect, color: Color32) {
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let step = 7.0;
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        painter.line_segment(
            [pos2(x, rect.bottom()), pos2(x + rect.height(), rect.top())],
            Stroke::new(1.0, color),
        );
        x += step;
    }
}

/// A small glyph for a state: a lock, a check, a plus, a slash.
fn glyph(painter: &egui::Painter, center: Pos2, size: f32, state: TechState) {
    let color = state_color(state);
    let s = size / 2.0;
    let stroke = Stroke::new((size / 9.0).max(1.2), color);
    match state {
        TechState::Locked => {
            let body = Rect::from_center_size(center + vec2(0.0, s * 0.3), vec2(s * 1.4, s * 1.0));
            painter.rect_stroke(body, 2.0, stroke, egui::StrokeKind::Middle);
            let r = s * 0.42;
            let top = center + vec2(0.0, s * 0.3 - s * 0.5);
            let points: Vec<Pos2> = (0..=10)
                .map(|k| {
                    let a = std::f32::consts::PI * (1.0 + k as f32 / 10.0);
                    top + vec2(a.cos() * r, a.sin() * r)
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
        }
        TechState::Available => {
            painter.add(egui::Shape::line(
                vec![
                    center + vec2(-s * 0.6, 0.0),
                    center + vec2(-s * 0.15, s * 0.45),
                    center + vec2(s * 0.65, -s * 0.5),
                ],
                stroke,
            ));
        }
        TechState::New => {
            painter.line_segment(
                [center - vec2(s * 0.6, 0.0), center + vec2(s * 0.6, 0.0)],
                stroke,
            );
            painter.line_segment(
                [center - vec2(0.0, s * 0.6), center + vec2(0.0, s * 0.6)],
                stroke,
            );
        }
        TechState::Blocked => {
            painter.circle_stroke(center, s * 0.7, stroke);
            painter.line_segment(
                [
                    center + vec2(-s * 0.5, s * 0.5),
                    center + vec2(s * 0.5, -s * 0.5),
                ],
                stroke,
            );
        }
    }
}

/// A state's pill: its glyph and name.
fn pill(painter: &egui::Painter, right_center: Pos2, state: TechState, zoom: f32) -> Rect {
    let color = state_color(state);
    let font = FontId::proportional(10.5 * zoom);
    let galley = painter.layout_no_wrap(state.label().to_string(), font, color);
    let h = 18.0 * zoom;
    let w = galley.size().x + h + 8.0 * zoom;
    let rect = Rect::from_min_size(right_center - vec2(w, h / 2.0), vec2(w, h));
    let fill = match state {
        TechState::New => NEW.gamma_multiply(0.16),
        TechState::Blocked => BLOCKED.gamma_multiply(0.16),
        _ => SELECTED,
    };
    painter.rect_filled(rect, h / 2.0, fill);
    glyph(
        painter,
        pos2(rect.left() + h / 2.0 + 2.0 * zoom, rect.center().y),
        h * 0.62,
        state,
    );
    painter.galley(
        pos2(
            rect.left() + h + 2.0 * zoom,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
    rect
}

/// What the view did this frame.
#[derive(Default)]
pub struct Out {
    pub edits: Vec<Edit>,
}

/// The Tech view over `rect` for the mission map in `session` (its campaign `link`).
pub fn show(
    ui: &mut Ui,
    rect: Rect,
    session: &mut crate::map::Session,
    link: &MissionLink,
    content: Option<&Content>,
) -> Out {
    let mut out = Out::default();
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0, MAIN);
    // A faint dot grid, so the canvas reads as one.
    let view = &mut session.tech;
    let Some(content) = content else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "The tree shows once the package passes the check",
            FontId::proportional(13.0),
            MUTED,
        );
        return out;
    };
    let missions: Vec<(usize, usize, String)> = steps(&link.value)
        .iter()
        .zip(mission_numbers(&link.value))
        .enumerate()
        .filter(|(_, (s, _))| StepKind::of(s) == Some(StepKind::Mission))
        .map(|(i, (s, n))| (i, n.unwrap_or(0), text(s, "title").to_string()))
        .collect();
    let Some(own) = link.step else {
        return out;
    };
    let shown = view
        .mission
        .filter(|m| missions.iter().any(|(i, _, _)| i == m))
        .unwrap_or(own);
    let tech = Tech::at(&link.value, shown);
    let race = link.race();
    let source = Source::of(content, race);
    let full = Graph::build(&source, &|_| true);
    let reach = Reach::of(&full, &tech);
    let contradictions: BTreeMap<String, String> =
        reach.contradictions(&tech).into_iter().collect();
    let graph = if view.changes_only {
        Graph::build(&source, &|id| {
            matches!(tech.state(id), TechState::New | TechState::Blocked)
                || contradictions.contains_key(id)
        })
    } else {
        full.clone()
    };
    if graph.nodes.is_empty() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            format!("The campaign's race ({race}) has no tree in the content"),
            FontId::proportional(13.0),
            MUTED,
        );
        return out;
    }

    // The canvas: drag pans, scroll zooms round the pointer, double-click fits.
    let top_space = 112.0;
    let canvas = Rect::from_min_max(pos2(rect.left(), rect.top() + top_space), rect.max);
    let background = ui.interact(
        canvas,
        Id::new(("tech-canvas", &session.root)),
        Sense::click_and_drag(),
    );
    if view.fit || background.double_clicked() {
        let zoom = ((canvas.width() - 24.0) / graph.size.x)
            .min((canvas.height() - 24.0) / graph.size.y)
            .clamp(0.45, 1.25);
        view.zoom = zoom;
        view.pan = vec2(
            (canvas.width() - graph.size.x * zoom) / 2.0,
            ((canvas.height() - graph.size.y * zoom) / 2.0).max(0.0),
        );
        view.fit = false;
    }
    if background.dragged() {
        view.pan += background.drag_delta();
    }
    if let Some(pointer) = ui.input(|i| i.pointer.hover_pos())
        && canvas.contains(pointer)
    {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let before = view.zoom;
            view.zoom = (view.zoom * (1.0 + scroll * 0.0015)).clamp(0.35, 2.0);
            let at = pointer - canvas.min - view.pan;
            view.pan -= at * (view.zoom / before - 1.0);
        }
    }
    let zoom = view.zoom;
    let origin = canvas.min + view.pan;
    let to_screen = |r: Rect| Rect::from_min_size(origin + r.min.to_vec2() * zoom, r.size() * zoom);
    let painter = ui.painter_at(canvas);
    // Dots every 24 points.
    let spacing = 24.0 * zoom;
    if spacing > 8.0 {
        let start = pos2(
            canvas.left() + (origin.x - canvas.left()).rem_euclid(spacing),
            canvas.top() + (origin.y - canvas.top()).rem_euclid(spacing),
        );
        let mut y = start.y;
        while y < canvas.bottom() {
            let mut x = start.x;
            while x < canvas.right() {
                painter.circle_filled(pos2(x, y), 0.9, Color32::from_gray(38));
                x += spacing;
            }
            y += spacing;
        }
    }
    // The tier columns: a band behind each, its hall's tier at its head.
    for (tier, &hall) in graph.halls.iter().enumerate() {
        let x = MARGIN + tier as f32 * (COLUMN + GAP) - 12.0;
        let band = to_screen(Rect::from_min_size(
            pos2(x, MARGIN - 4.0),
            vec2(COLUMN + 24.0, graph.size.y - MARGIN),
        ));
        painter.rect_filled(band, 12.0 * zoom, Color32::from_white_alpha(4));
        painter.text(
            band.left_top() + vec2(14.0, 10.0) * zoom,
            Align2::LEFT_TOP,
            format!(
                "TIER {}  ·  {}",
                tier + 1,
                graph.nodes[hall].name.to_uppercase()
            ),
            FontId::proportional(10.0 * zoom),
            FAINT,
        );
    }
    // Edges under the cards.
    for edge in &graph.edges {
        let (a, b) = (
            to_screen(graph.nodes[edge.from].rect),
            to_screen(graph.nodes[edge.to].rect),
        );
        let (from, to) = if (b.center().x - a.center().x).abs() < 4.0 {
            (a.center_bottom(), b.center_top())
        } else if b.center().x > a.center().x {
            (a.right_center(), b.left_center())
        } else {
            (a.left_center(), b.right_center())
        };
        let color = match edge.kind {
            EdgeKind::Upgrade => HALL.gamma_multiply(0.75),
            EdgeKind::Requires => Color32::from_gray(96),
        };
        let horizontal = (to.x - from.x).abs() > 4.0;
        let bend = if horizontal {
            vec2((to.x - from.x) * 0.5, 0.0)
        } else {
            vec2(0.0, (to.y - from.y) * 0.5)
        };
        let curve = egui::epaint::CubicBezierShape::from_points_stroke(
            [from, from + bend, to - bend, to],
            false,
            Color32::TRANSPARENT,
            Stroke::new(1.6 * zoom.max(0.7), color),
        );
        painter.add(curve);
        // The arrow head at its end.
        let dir = (to - (to - bend * 0.2)).normalized();
        let side = vec2(-dir.y, dir.x);
        let head = 6.0 * zoom;
        painter.add(egui::Shape::convex_polygon(
            vec![
                to,
                to - dir * head + side * head * 0.55,
                to - dir * head - side * head * 0.55,
            ],
            color,
            Stroke::NONE,
        ));
    }
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let mut clicked: Option<(String, TechState)> = None;
    // The cards.
    for card in &graph.cards {
        let n = &graph.nodes[card.node];
        let r = to_screen(card.rect);
        if !r.intersects(canvas) {
            continue;
        }
        let hall = n.kind == NodeKind::Hall;
        painter.rect(
            r,
            10.0 * zoom,
            RAISED,
            Stroke::new(
                1.0,
                if hall {
                    HALL.gamma_multiply(0.45)
                } else {
                    BORDER
                },
            ),
            egui::StrokeKind::Inside,
        );
        if hall {
            painter.rect_filled(
                Rect::from_min_size(
                    r.min + vec2(10.0 * zoom, 0.0),
                    vec2(r.width() - 20.0 * zoom, 2.0 * zoom),
                ),
                1.0,
                HALL,
            );
        }
    }
    // Every node: its picture, name and state; hatched where what it needs is missing,
    // red where it is allowed anyway.
    let palette = &mut session.blocks.palette;
    for (index, n) in graph.nodes.iter().enumerate() {
        let r = to_screen(n.rect);
        if !r.intersects(canvas) {
            continue;
        }
        let state = tech.state(&n.id);
        let needs = reach.needs.get(&n.id).cloned().unwrap_or(Ok(()));
        let contradiction = contradictions.get(&n.id);
        let present = graph.present.contains(&n.id);
        let header = matches!(n.kind, NodeKind::Hall | NodeKind::Building);
        let chained = n.kind == NodeKind::Research && n.rect.width() < COLUMN * 0.5;
        let response = ui.interact(
            r,
            Id::new(("tech-node", &session.root, index)),
            Sense::click(),
        );
        let hovered = response.hovered();
        let icon_size = if header { 40.0 } else { CHIP } * zoom;
        let icon_rect = Rect::from_min_size(
            pos2(
                r.left() + if header { 12.0 } else { 3.0 } * zoom,
                r.center().y - icon_size / 2.0,
            ),
            vec2(icon_size, icon_size),
        );
        if hovered && !header {
            painter.rect_filled(r, 6.0 * zoom, SELECTED);
        } else if hovered {
            painter.rect_stroke(
                r.shrink(1.0),
                10.0 * zoom,
                Stroke::new(1.0, CONTROL),
                egui::StrokeKind::Inside,
            );
        }
        let dim = state == TechState::Locked && !present;
        let texture = n.icon.as_deref().and_then(|e| palette.icon(ui.ctx(), e));
        match texture {
            Some(texture) => {
                let tint = if dim {
                    Color32::from_gray(90)
                } else if state == TechState::Blocked {
                    Color32::from_gray(150)
                } else {
                    Color32::WHITE
                };
                egui::Image::from_texture((texture, icon_rect.size()))
                    .corner_radius((6.0 * zoom) as u8)
                    .tint(tint)
                    .paint_at(ui, icon_rect);
            }
            None => {
                painter.rect_filled(icon_rect, 6.0 * zoom, SELECTED);
                icons::paint(
                    &painter,
                    icons::square(icon_rect.center(), icon_size * 0.55),
                    match n.kind {
                        NodeKind::Research => Icon::Abilities,
                        NodeKind::Unit | NodeKind::Hero => Icon::Units,
                        _ => Icon::Buildings,
                    },
                    FAINT,
                );
            }
        }
        // The state on the picture: a ring in its colour, a glyph in its corner.
        let ring = match state {
            TechState::New => Some(NEW),
            TechState::Blocked => Some(BLOCKED),
            _ => None,
        };
        if let Some(color) = ring {
            painter.rect_stroke(
                icon_rect.expand(1.5 * zoom),
                7.0 * zoom,
                Stroke::new(2.0 * zoom, color),
                egui::StrokeKind::Outside,
            );
        }
        if chained || !header {
            let badge = Rect::from_center_size(
                icon_rect.right_bottom() - vec2(2.0, 2.0) * zoom,
                vec2(13.0, 13.0) * zoom,
            );
            painter.circle_filled(badge.center(), 7.0 * zoom, RAISED);
            glyph(&painter, badge.center(), 10.0 * zoom, state);
        }
        if needs.is_err() {
            hatch(&painter, icon_rect, Color32::from_black_alpha(150));
            if !chained {
                hatch(&painter, r.shrink(1.0), Color32::from_white_alpha(14));
            }
        }
        if contradiction.is_some() {
            painter.rect_stroke(
                if chained {
                    icon_rect.expand(3.0 * zoom)
                } else {
                    r.shrink(0.5)
                },
                if header || chained { 8.0 } else { 6.0 } * zoom,
                Stroke::new(2.0 * zoom.max(0.75), ERROR),
                egui::StrokeKind::Outside,
            );
        }
        // Its name (a chain's links show theirs on hover), and the state as a pill.
        if !chained {
            let text_left = icon_rect.right() + 10.0 * zoom;
            let name_color = if dim { MUTED } else { BODY };
            let (name_size, sub) = if header {
                (
                    13.5 * zoom,
                    Some(match n.kind {
                        NodeKind::Hall => format!("HALL · TIER {}", n.tier + 1),
                        _ if n.made_at.is_empty() => "BUILDING".into(),
                        _ => "UPGRADE".into(),
                    }),
                )
            } else {
                (
                    12.0 * zoom,
                    (n.kind == NodeKind::Hero).then(|| "HERO".to_string()),
                )
            };
            let pill_rect = if header || zoom > 0.6 {
                Some(pill(
                    &painter,
                    pos2(r.right() - 10.0 * zoom, r.center().y),
                    state,
                    zoom * if header { 1.0 } else { 0.92 },
                ))
            } else {
                None
            };
            let width = pill_rect.map_or(r.right(), |p| p.left()) - text_left - 6.0 * zoom;
            let mut job = egui::text::LayoutJob::simple_singleline(
                n.name.clone(),
                FontId::proportional(name_size),
                name_color,
            );
            job.wrap.max_width = width.max(10.0);
            job.wrap.max_rows = 1;
            let galley = painter.layout_job(job);
            let y = match &sub {
                Some(_) => r.center().y - galley.size().y + 2.0 * zoom,
                None => r.center().y - galley.size().y / 2.0,
            };
            painter.galley(pos2(text_left, y), galley, name_color);
            if let Some(sub) = sub {
                painter.text(
                    pos2(text_left, r.center().y + 4.0 * zoom),
                    Align2::LEFT_TOP,
                    sub,
                    FontId::proportional(9.0 * zoom),
                    if n.kind == NodeKind::Hall {
                        HALL
                    } else {
                        FAINT
                    },
                );
            }
            // Something needing a higher tier than its card says so.
            let card_tier = graph.nodes[graph.cards[n.card].node].tier;
            if !header && n.tier > card_tier && zoom > 0.6 {
                let hall = graph
                    .halls
                    .get(n.tier)
                    .map(|h| graph.nodes[*h].name.clone())
                    .unwrap_or_default();
                let at = pos2(text_left, r.bottom() - 1.0);
                painter.text(
                    at,
                    Align2::LEFT_BOTTOM,
                    format!("needs {hall}"),
                    FontId::proportional(9.0 * zoom),
                    FAINT,
                );
            }
        }
        // A chain's arrow to its next link.
        if chained && let Some(chain) = graph.chains.iter().find(|c| c.0.contains(&index)) {
            let k = chain.0.iter().position(|i| *i == index).unwrap_or(0);
            if k + 1 < chain.0.len() {
                let a = icon_rect.right_center() + vec2(4.0 * zoom, 0.0);
                let b = a + vec2(10.0 * zoom, 0.0);
                painter.arrow(a, b - a, Stroke::new(1.4 * zoom.max(0.7), CONTROL));
            } else {
                // The chain's name after its last link.
                let first = &graph.nodes[chain.0[0]].name;
                let base = first.trim_end_matches(|c: char| {
                    c == 'I' || c == 'V' || c == 'X' || c.is_ascii_digit() || c == ' '
                });
                let label = format!(
                    "{} ×{}",
                    if base.is_empty() { first } else { base },
                    chain.0.len()
                );
                let x = icon_rect.right() + 10.0 * zoom;
                let card = to_screen(graph.cards[n.card].rect);
                let mut job = egui::text::LayoutJob::simple_singleline(
                    label,
                    FontId::proportional(11.5 * zoom),
                    SOFT,
                );
                job.wrap.max_width = (card.right() - x - 8.0 * zoom).max(10.0);
                job.wrap.max_rows = 1;
                let galley = painter.layout_job(job);
                painter.galley(pos2(x, r.center().y - galley.size().y / 2.0), galley, SOFT);
            }
        }
        let before = tech.before.get(&n.id).and_then(|step| {
            missions
                .iter()
                .find(|(i, _, _)| i == step)
                .map(|(_, number, title)| format!("Unlocked in mission {number} · {title}"))
        });
        let response = response.on_hover_ui(|ui| {
            ui.set_max_width(280.0);
            ui.label(egui::RichText::new(&n.name).color(TEXT).strong());
            ui.label(
                egui::RichText::new(&n.id)
                    .color(FAINT)
                    .size(10.5)
                    .monospace(),
            );
            ui.label(egui::RichText::new(state.label()).color(state_color(state)));
            if let Some(before) = &before {
                ui.label(egui::RichText::new(before).color(MUTED).size(11.0));
            }
            if present {
                ui.label(
                    egui::RichText::new("Placed by the race's starting setup")
                        .color(MUTED)
                        .size(11.0),
                );
            }
            if let Err(why) = &needs {
                ui.label(
                    egui::RichText::new(format!("Unreachable: {why}"))
                        .color(MUTED)
                        .size(11.0),
                );
            }
            if let Some(why) = contradiction {
                ui.label(
                    egui::RichText::new(format!("Contradiction: allowed, but {why}"))
                        .color(ERROR)
                        .size(11.0),
                );
            }
            ui.label(
                egui::RichText::new("Click: next state · right-click: choose")
                    .color(FAINT)
                    .size(10.5),
            );
        });
        if response.clicked() {
            clicked = Some((n.id.clone(), tech.next(&n.id)));
        }
        response.context_menu(|ui| {
            ui.label(egui::RichText::new(&n.name).color(MUTED).size(11.0));
            for choice in tech.choices(&n.id) {
                if ui
                    .selectable_label(state == *choice, choice.label())
                    .clicked()
                {
                    clicked = Some((n.id.clone(), *choice));
                    ui.close();
                }
            }
        });
        let _ = pointer;
    }
    if let Some((id, to)) = clicked
        && to != tech.state(&id)
    {
        let (new, blocked) = tech.with(&id, to);
        let name = graph
            .find(&id)
            .next()
            .map_or(id.clone(), |n| n.name.clone());
        out.edits.push(Edit {
            label: format!("Tech: {name} {}", to.label().to_lowercase()),
            context: Context {
                editor: Editor::Maps,
                document: session.path("map.json"),
                pointer: None,
            },
            commands: vec![super::set_tech(&link.document(), shown, new, blocked)],
            merge: None,
        });
    }

    // The strip of the campaign's missions, under the view bar.
    let strip = Rect::from_min_size(rect.min + vec2(16.0, 58.0), vec2(rect.width() - 32.0, 46.0));
    egui::Area::new(Id::new(("tech-strip", &session.root)))
        .order(egui::Order::Foreground)
        .fixed_pos(strip.min)
        .constrain_to(rect)
        .show(ui.ctx(), |ui| {
            ui.set_max_width(strip.width());
            crate::world::strip_frame().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(crate::widgets::eyebrow("Tech in"));
                    ui.add_space(4.0);
                    egui::ScrollArea::horizontal()
                        .id_salt(("tech-missions", &session.root))
                        .max_width((strip.width() - 330.0).max(120.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (step, number, title) in &missions {
                                    let t = Tech::at(&link.value, *step);
                                    if mission_chip(
                                        ui,
                                        *number,
                                        title,
                                        *step == shown,
                                        *step == own,
                                        &t,
                                    ) {
                                        session.tech.mission = (*step != own).then_some(*step);
                                    }
                                }
                            });
                        });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .selectable_label(session.tech.changes_only, "Changes only")
                            .on_hover_text(
                                "Only what this mission makes new or blocks, and what contradicts",
                            )
                            .clicked()
                        {
                            session.tech.changes_only = !session.tech.changes_only;
                            session.tech.fit = true;
                        }
                        if ui
                            .button("Fit")
                            .on_hover_text("Fit the tree to the view (double-click)")
                            .clicked()
                        {
                            session.tech.fit = true;
                        }
                        let counts = |s: TechState| {
                            full.nodes.iter().filter(|n| tech.state(&n.id) == s).count()
                        };
                        ui.label(
                            egui::RichText::new(format!(
                                "{} new · {} blocked · {} available · {} locked",
                                counts(TechState::New),
                                counts(TechState::Blocked),
                                counts(TechState::Available),
                                counts(TechState::Locked)
                            ))
                            .size(11.0)
                            .color(MUTED),
                        );
                        if !contradictions.is_empty() {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} contradictions",
                                    contradictions.len()
                                ))
                                .size(11.0)
                                .color(ERROR),
                            );
                        }
                    });
                });
            });
        });
    legend(ui, rect, &session.root);
    out
}

/// One mission in the strip: its number and title, what it changes; the one shown is
/// selected, the map's own marked.
fn mission_chip(ui: &mut Ui, number: usize, title: &str, on: bool, own: bool, tech: &Tech) -> bool {
    let font = FontId::proportional(12.0);
    let label = format!("{number}  {title}");
    let galley = ui
        .painter()
        .layout_no_wrap(label, font, if on { TEXT } else { MUTED });
    let marks = (!tech.new.is_empty()) as usize + (!tech.blocked.is_empty()) as usize;
    let size = vec2(galley.size().x + 22.0 + marks as f32 * 16.0, 30.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    if on {
        painter.rect(
            rect,
            7,
            SELECTED,
            Stroke::new(1.0, SOFT),
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() {
        painter.rect_filled(rect, 7, RAISED);
    }
    painter.galley(
        pos2(rect.left() + 11.0, rect.center().y - galley.size().y / 2.0),
        galley,
        TEXT,
    );
    let mut x = rect.right() - 12.0 - marks as f32 * 16.0 + 8.0;
    for (has, color) in [
        (!tech.new.is_empty(), NEW),
        (!tech.blocked.is_empty(), BLOCKED),
    ] {
        if has {
            painter.circle_filled(pos2(x + 4.0, rect.center().y), 3.5, color);
            x += 16.0;
        }
    }
    if own {
        painter.rect_filled(
            Rect::from_min_size(
                pos2(rect.left() + 10.0, rect.bottom() - 3.0),
                vec2(rect.width() - 20.0, 2.0),
            ),
            1.0,
            HALL,
        );
    }
    response
        .on_hover_text(if own {
            format!("{} (this map's mission)", tech.summary())
        } else {
            tech.summary()
        })
        .clicked()
}

/// The legend in the view's bottom-left corner.
fn legend(ui: &mut Ui, rect: Rect, root: &str) {
    egui::Area::new(Id::new(("tech-legend", root)))
        .order(egui::Order::Foreground)
        .pivot(Align2::LEFT_BOTTOM)
        .fixed_pos(rect.left_bottom() + vec2(16.0, -16.0))
        .show(ui.ctx(), |ui| {
            crate::world::strip_frame().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    for state in [
                        TechState::New,
                        TechState::Blocked,
                        TechState::Available,
                        TechState::Locked,
                    ] {
                        let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                        glyph(ui.painter(), r.center(), 12.0, state);
                        ui.label(
                            egui::RichText::new(state.label())
                                .size(11.0)
                                .color(state_color(state)),
                        );
                    }
                    let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    ui.painter().rect_filled(r, 3, SELECTED);
                    hatch(ui.painter(), r, Color32::from_gray(140));
                    ui.label(egui::RichText::new("Unreachable").size(11.0).color(MUTED));
                    let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    ui.painter().rect_stroke(
                        r.shrink(2.0),
                        3,
                        Stroke::new(2.0, ERROR),
                        egui::StrokeKind::Inside,
                    );
                    ui.label(egui::RichText::new("Contradiction").size(11.0).color(MUTED));
                });
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(id: &str, building: bool) -> Item {
        Item {
            id: id.into(),
            name: id.into(),
            building,
            ..Item::default()
        }
    }

    /// A race of a hall line (hall → keep), a worker the hall trains, a barracks that
    /// trains a soldier, a lab that needs the keep and researches a two-level chain, and
    /// a tower that upgrades to a spire needing the keep.
    fn source() -> Source {
        let mut hall = item("b.hall", true);
        hall.upgrade = Some("b.keep".into());
        hall.trains = vec!["u.worker".into()];
        let mut keep = item("b.keep", true);
        keep.trains = vec!["u.worker".into()];
        let mut worker = item("u.worker", false);
        worker.builds = ["b.hall", "b.barracks", "b.lab", "b.tower"]
            .map(String::from)
            .to_vec();
        let mut barracks = item("b.barracks", true);
        barracks.trains = vec!["u.soldier".into()];
        let mut lab = item("b.lab", true);
        lab.requires = vec!["b.keep".into()];
        lab.researches = vec!["r.one".into(), "r.two".into()];
        let mut tower = item("b.tower", true);
        tower.upgrade = Some("b.spire".into());
        let mut spire = item("b.spire", true);
        spire.requires = vec!["b.keep".into()];
        let research = |follows: Option<&str>| Research {
            name: "Plating".into(),
            follows: follows.map(String::from),
            requires: follows.map(String::from).into_iter().collect(),
            ..Research::default()
        };
        Source {
            items: vec![
                hall,
                keep,
                worker,
                barracks,
                item("u.soldier", false),
                lab,
                tower,
                spire,
            ],
            research: [
                ("r.one".to_string(), research(None)),
                ("r.two".to_string(), research(Some("r.one"))),
            ]
            .into(),
            fort: "b.hall".into(),
            placed: vec!["b.hall".into(), "u.worker".into()],
        }
    }

    fn tech(new: &[&str], blocked: &[&str]) -> Tech {
        Tech::at(
            &json!({"steps": [{"kind": "mission", "id": "m", "map": "m",
                "tech": {"new": new, "blocked": blocked}}]}),
            0,
        )
    }

    #[test]
    fn halls_head_their_columns_and_buildings_stand_in_the_tier_they_need() {
        let graph = Graph::build(&source(), &|_| true);
        let names: Vec<&str> = graph
            .halls
            .iter()
            .map(|h| graph.nodes[*h].id.as_str())
            .collect();
        assert_eq!(names, ["b.hall", "b.keep"]);
        let column = |id: &str| {
            let n = graph.find(id).next().unwrap();
            ((graph.cards[n.card].rect.min.x - MARGIN) / (COLUMN + GAP)).round() as usize
        };
        assert_eq!(column("b.barracks"), 0);
        assert_eq!(column("b.lab"), 1);
        assert_eq!(column("b.spire"), 1);
        // The worker stands under the first hall that trains it; the chain in one row.
        assert_eq!(graph.find("u.worker").count(), 1);
        let worker = graph.find("u.worker").next().unwrap();
        assert_eq!(graph.nodes[graph.cards[worker.card].node].id, "b.hall");
        assert_eq!(graph.chains.len(), 1);
        assert_eq!(graph.chains[0].0.len(), 2);
        assert!(graph.edges.iter().any(|e| e.kind == EdgeKind::Upgrade
            && graph.nodes[e.from].id == "b.tower"
            && graph.nodes[e.to].id == "b.spire"));
        // The same tree laid out again lands in the same places.
        let again = Graph::build(&source(), &|_| true);
        let rects = |g: &Graph| {
            g.nodes
                .iter()
                .map(|n| (n.id.clone(), n.rect))
                .collect::<Vec<_>>()
        };
        assert_eq!(rects(&graph), rects(&again));
    }

    #[test]
    fn a_blocked_building_leaves_what_needs_it_unreachable_and_allowing_that_contradicts() {
        let graph = Graph::build(&source(), &|_| true);
        // The barracks and its soldier allowed: reachable through the placed hall and
        // worker.
        let fine = tech(&["b.barracks", "u.soldier"], &[]);
        let reach = Reach::of(&graph, &fine);
        assert!(reach.reached.contains("u.soldier"));
        assert!(reach.contradictions(&fine).is_empty());
        // The barracks blocked: the soldier, allowed, cannot be had.
        let blocked = tech(&["u.soldier"], &["b.barracks"]);
        let reach = Reach::of(&graph, &blocked);
        assert!(reach.needs["u.soldier"].is_err());
        let found: Vec<String> = reach
            .contradictions(&blocked)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(found, ["u.soldier"]);
        // The lab's research needs the keep, still locked: unreachable, but no
        // contradiction while the research is locked too.
        let none = tech(&[], &[]);
        let reach = Reach::of(&graph, &none);
        assert!(reach.needs["r.one"].is_err());
        assert!(reach.contradictions(&none).is_empty());
        // The second level needs the first.
        let chain = tech(&["b.keep", "b.lab", "r.two"], &[]);
        let reach = Reach::of(&graph, &chain);
        assert_eq!(reach.contradictions(&chain).len(), 1);
    }

    #[test]
    fn changes_only_keeps_the_halls_and_the_cards_of_what_changed() {
        let changed = ["u.soldier"];
        let graph = Graph::build(&source(), &|id| changed.contains(&id));
        let cards: Vec<&str> = graph
            .cards
            .iter()
            .map(|c| graph.nodes[c.node].id.as_str())
            .collect();
        assert_eq!(cards, ["b.hall", "b.keep", "b.barracks"]);
        assert!(graph.find("u.worker").next().is_none());
    }
}
