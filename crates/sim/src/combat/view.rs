//! What views show of combat: each seen body's stats, swing, charge and lookout; the
//! viewer's projectiles in flight, corpses in sight and fallen heroes.

use super::{Corpse, Missile, Shell, senses::Senses};
use crate::{
    entity::{Entity, EntityId},
    state::State,
    stats::Stats,
    vision::{Sight, VISIBLE},
};
use canopy_content::placement::Owner;

/// A swing as a viewer sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttackView {
    /// Hidden from other owners unless they see the target too.
    pub target: Option<EntityId>,
    pub started: u32,
    pub impact: u32,
    pub ends: u32,
    pub released: bool,
}

/// A body's combat state in a view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombatView {
    pub stats: Stats,
    /// Whether the viewer's side fights it unprompted (enemies, hostile camps).
    pub hostile: bool,
    pub attack: Option<AttackView>,
    pub cooldown: u32,
    pub charging: bool,
    /// The strongest slow on it, permille.
    pub slow: u32,
    /// The lookout it stands in.
    pub garrison: Option<EntityId>,
    /// Owner only: what it engages, and the target of its attack order.
    pub target: Option<EntityId>,
    pub commanded_target: Option<EntityId>,
    /// Owner only.
    pub experience: Option<u32>,
}

impl CombatView {
    /// `viewer` is `None` for observers; `private` when the viewer owns the entity (or
    /// observes).
    pub fn of(
        senses: &Senses,
        state: &State,
        e: &Entity,
        viewer: Option<Owner>,
        private: bool,
    ) -> Option<Self> {
        e.hp?;
        let sight = senses.sight(state);
        let unit = e.unit.as_ref();
        let attack = unit
            .and_then(|u| u.combat.attack.as_ref())
            .map(|a| AttackView {
                target: (private
                    || viewer.is_some_and(|v| {
                        state
                            .entities
                            .get(a.target)
                            .is_some_and(|t| sight.visible(v, t))
                    }))
                .then_some(a.target),
                started: a.started,
                impact: a.impact,
                ends: a.ends,
                released: a.released,
            });
        let hostile = viewer.is_some_and(|v| {
            senses.opponents(
                v,
                None,
                e.owner,
                unit.and_then(|u| u.camp.as_ref()).map(|c| c.id.as_str()),
            )
        });
        Some(Self {
            stats: Stats::of(senses.catalog, e, crate::stats::research(state, e.owner)),
            hostile,
            attack,
            cooldown: unit.map_or(0, |u| u.combat.cooldown),
            charging: super::charge::charging(e, state.tick),
            slow: unit.map_or(0, |u| u.combat.slow()),
            garrison: unit.and_then(|u| u.garrison.as_ref()).map(|g| g.building),
            target: if private {
                unit.and_then(|u| u.target)
            } else {
                None
            },
            commanded_target: if private {
                unit.and_then(|u| match u.order {
                    Some(crate::orders::Order::Attack { target, .. }) => Some(target),
                    _ => None,
                })
            } else {
                None
            },
            experience: if private {
                e.progression.as_ref().map(|p| p.experience)
            } else {
                None
            },
        })
    }
}

/// The viewer's share of battle state outside entities.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Battle {
    /// Missiles the viewer saw launched whose destination it sees now.
    pub missiles: Vec<Missile>,
    /// Shells the viewer saw launched.
    pub shells: Vec<Shell>,
    /// Corpses on cells in sight.
    pub corpses: Vec<Corpse>,
    /// The viewer's fallen heroes (everyone's for an observer).
    pub fallen: Vec<EntityId>,
}

impl Battle {
    pub fn of(senses: &Senses, state: &State, viewer: Option<Owner>) -> Self {
        let sight: Sight = senses.sight(state);
        let cells = viewer.and_then(|v| sight.cells(v));
        let in_sight = |x: i32, y: i32, surface: Option<&str>| {
            let Some(cells) = cells else {
                return viewer.is_none();
            };
            let deck = surface.and_then(|s| senses.terrain.deck_id(s));
            let node = match senses.terrain.surfaces() {
                Some(s) => s.node_at(i64::from(x), i64::from(y), deck),
                None => {
                    let size = senses.terrain.size() as i32;
                    (x >= 0 && y >= 0 && x < size && y < size).then(|| (y * size + x) as u32)
                }
            };
            node.is_some_and(|n| cells.get(n) == VISIBLE)
        };
        let tick = state.tick;
        Self {
            missiles: state
                .combat
                .missiles
                .iter()
                .filter(|m| {
                    viewer.is_none_or(|v| {
                        let (x, y) = m.destination.position.cell();
                        m.viewers.contains(&v) && in_sight(x, y, m.destination.surface.as_deref())
                    })
                })
                .cloned()
                .collect(),
            shells: state
                .combat
                .shells
                .iter()
                .filter(|s| viewer.is_none_or(|v| s.viewers.contains(&v)))
                .cloned()
                .collect(),
            corpses: state
                .combat
                .corpses
                .iter()
                .filter(|c| c.expires > tick)
                .filter(|c| in_sight(c.position.x, c.position.y, c.position.surface.as_deref()))
                .cloned()
                .collect(),
            fallen: state
                .entities
                .iter()
                .filter(|e| e.fallen && viewer.is_none_or(|v| e.owner == v))
                .map(|e| e.id)
                .collect(),
        }
    }
}
