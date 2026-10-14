//! What views show of abilities and items (old `describeActor` ability fields,
//! `observedEvents`, `observedDeliveries`, `observedHeroReturns`).

use super::{
    Events, actor,
    events::AbilityEvent,
    library::AbilityId,
    state::{Phase, SeekerPhase},
    statuses,
};
use crate::{
    catalog::{Catalog, Def},
    combat::Senses,
    entity::{Entity, EntityId, Milli},
    items::{ItemRuntime, ItemStatusKind},
    state::State,
};
use canopy_content::{abilities::Polarity, placement::Owner};

/// A spell status as a viewer sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusView {
    pub ability: AbilityId,
    pub status: String,
    pub polarity: Polarity,
    pub source: EntityId,
    pub started: u32,
    pub expires: u32,
    pub stacks: Option<u32>,
    pub shield: Option<u32>,
    pub aura: bool,
}

/// A cast in progress.
#[derive(Debug, Clone, PartialEq)]
pub struct CastingView {
    pub ability: AbilityId,
    pub start_tick: u32,
    pub resolve_tick: u32,
    pub finish_tick: u32,
    pub channel_end: Option<u32>,
    pub phase: Phase,
}

/// Owner-only ability state.
#[derive(Debug, Clone, PartialEq)]
pub struct CasterView {
    pub mana: u32,
    pub ranks: Vec<u8>,
    pub cooldowns: Vec<(AbilityId, u32)>,
    /// Autocast per binding (with the ability's default where not set).
    pub autocast: Vec<bool>,
    /// Abilities with a running persistent instance (toggles on).
    pub active: Vec<AbilityId>,
    pub weapon_order: Option<(AbilityId, EntityId)>,
}

/// Owner-only item state.
#[derive(Debug, Clone, PartialEq)]
pub struct InventoryView {
    pub slots: Vec<Option<Def>>,
    pub runtime: Vec<Option<ItemRuntime>>,
}

/// An item status as a viewer sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemStatusView {
    pub item: Def,
    pub kind: ItemStatusKind,
    pub expires: u32,
    pub shield: Option<u32>,
}

/// A unit's abilities, statuses and items in a view.
#[derive(Debug, Clone, PartialEq)]
pub struct MagicView {
    pub statuses: Vec<StatusView>,
    pub item_statuses: Vec<ItemStatusView>,
    /// Cosmetic opacity from concealment and forms (1 = opaque).
    pub opacity: f64,
    /// The newest form's model and scale.
    pub form: Option<(Option<String>, f64)>,
    /// Flight height in centimetres while airborne.
    pub flight: Option<i32>,
    pub summoned: bool,
    pub casting: Option<CastingView>,
    pub max_mana: u32,
    /// Owner only.
    pub caster: Option<CasterView>,
    pub inventory: Option<InventoryView>,
    pub summon_origin: Option<(EntityId, AbilityId)>,
    pub returning: Option<(AbilityId, u32)>,
}

impl MagicView {
    pub fn of(senses: &Senses, state: &State, e: &Entity, private: bool) -> Option<Self> {
        e.unit.as_ref()?;
        let catalog: &Catalog = senses.catalog;
        let tick = state.tick;
        let stats = crate::stats::Stats::of(catalog, e, crate::stats::research(state, e.owner));
        let form = super::forms::appearance(catalog, e);
        let opacity =
            statuses::concealment_opacity(catalog, e, tick) * form.as_ref().map_or(1.0, |f| f.2);
        let pending = e.caster.as_ref().and_then(|c| c.pending.as_ref());
        let caster = e.caster.as_ref().filter(|_| private).map(|c| {
            let policy = catalog.spells.caster(e.definition);
            CasterView {
                mana: c.mana,
                ranks: c.ranks.clone(),
                cooldowns: c.cooldowns.iter().map(|(&a, &t)| (a, t)).collect(),
                autocast: policy.map_or_else(Vec::new, |p| {
                    p.bindings
                        .iter()
                        .enumerate()
                        .map(|(i, b)| {
                            c.autocast.get(&(i as u8)).copied().unwrap_or_else(|| {
                                catalog.content.abilities.abilities[usize::from(b.ability.0)]
                                    .autocast
                                    .as_ref()
                                    .is_some_and(|a| a.enabled_by_default)
                            })
                        })
                        .collect()
                }),
                active: state
                    .abilities
                    .instances
                    .iter()
                    .filter(|i| i.source == e.id)
                    .map(|i| i.ability)
                    .collect(),
                weapon_order: c.weapon_order.as_ref().map(|o| (o.ability, o.target)),
            }
        });
        Some(Self {
            statuses: e
                .statuses
                .iter()
                .map(|s| {
                    let d = catalog.spells.status(s.key);
                    StatusView {
                        ability: s.key.ability,
                        status: d.name.clone(),
                        polarity: d.polarity,
                        source: s.source,
                        started: s.started,
                        expires: s.expires,
                        stacks: s.stacks,
                        shield: s.shield,
                        aura: s.aura,
                    }
                })
                .collect(),
            item_statuses: e.equipment.as_ref().map_or_else(Vec::new, |q| {
                q.statuses
                    .iter()
                    .map(|s| ItemStatusView {
                        item: s.item,
                        kind: s.kind,
                        expires: s.expires,
                        shield: s.shield,
                    })
                    .collect()
            }),
            opacity,
            form: form.map(|(asset, scale, _)| (asset, scale)),
            flight: super::forms::flight_cm(catalog, e),
            summoned: actor::summoned(e),
            casting: pending
                .filter(|p| p.start_tick <= tick)
                .map(|p| CastingView {
                    ability: p.ability,
                    start_tick: p.start_tick,
                    resolve_tick: p.release_tick,
                    finish_tick: p.finish_tick,
                    channel_end: p.channel.as_ref().map(|c| c.end_tick),
                    phase: p.phase,
                }),
            max_mana: stats.max_mana,
            caster,
            inventory: e
                .equipment
                .as_ref()
                .filter(|q| private && !q.slots.is_empty())
                .map(|q| InventoryView {
                    slots: q.slots.clone(),
                    runtime: q.runtime.clone(),
                }),
            summon_origin: e
                .links
                .as_ref()
                .and_then(|l| l.summoned.as_ref())
                .filter(|_| private)
                .map(|s| (s.source, s.ability)),
            returning: e
                .links
                .as_ref()
                .and_then(|l| l.spell_return.as_ref())
                .filter(|_| private)
                .map(|r| (r.ability, r.due)),
        })
    }
}

/// A delivery in flight as a viewer sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct DeliveryView {
    pub cast: u32,
    pub ability: AbilityId,
    /// Milli-cells and height in world units.
    pub position: (i32, i32, f64),
    /// Unit direction of travel (x, y, height).
    pub direction: (f64, f64, f64),
}

/// The viewer's share of ability state outside entities.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Spellcraft {
    /// Events of the last few seconds the viewer may see.
    pub events: Vec<AbilityEvent>,
    /// Deliveries in flight on cells in sight (chains are drawn by their impacts).
    pub deliveries: Vec<DeliveryView>,
    /// Weapon missiles carrying an enhancement.
    pub enhanced_missiles: Vec<u32>,
    /// Fallen heroes returning by an ability: (hero, ability, started, due).
    pub returns: Vec<(EntityId, AbilityId, u32, u32)>,
}

impl Spellcraft {
    pub fn of(senses: &Senses, state: &State, events: &Events, viewer: Option<Owner>) -> Self {
        let sight = senses.sight(state);
        let size = senses.terrain.size() as i32;
        let in_sight = |p: Milli| {
            let (x, y) = (super::cell_round(p.x), super::cell_round(p.y));
            match viewer {
                None => true,
                Some(o) => {
                    x >= 0
                        && y >= 0
                        && x < size
                        && y < size
                        && sight.in_sight(o, &[(y * size + x) as u32])
                }
            }
        };
        let catalog = senses.catalog;
        let deliveries = state
            .abilities
            .deliveries
            .iter()
            .filter(|d| {
                d.swarm
                    .as_ref()
                    .is_none_or(|s| s.phase != SeekerPhase::Waiting)
            })
            .filter(|d| {
                !matches!(
                    catalog.content.abilities.abilities[usize::from(d.ability.0)].delivery,
                    Some(canopy_content::abilities::Delivery::Chain { .. })
                )
            })
            .filter(|d| in_sight(d.position.milli()))
            .map(|d| {
                let (dx, dy, dh) = (
                    d.point.xf() - d.position.xf(),
                    d.point.yf() - d.position.yf(),
                    d.point.hf() - d.position.hf(),
                );
                let n = (dx * dx + dy * dy + dh * dh).sqrt();
                let n = if n == 0.0 { 1.0 } else { n };
                let m = d.position.milli();
                DeliveryView {
                    cast: d.cast,
                    ability: d.ability,
                    position: (m.x, m.y, d.position.hf()),
                    direction: (dx / n, dy / n, dh / n),
                }
            })
            .collect();
        let returns = state
            .entities
            .iter()
            .filter(|e| e.fallen)
            .filter_map(|e| {
                let r = e.links.as_ref()?.spell_return.as_ref()?;
                (viewer.is_none_or(|v| v == e.owner) || in_sight(e.position))
                    .then_some((e.id, r.ability, r.started, r.due))
            })
            .collect();
        let enhanced_missiles = state
            .combat
            .missiles
            .iter()
            .filter(|m| m.enhancement.is_some() && !m.resolved)
            .filter(|m| viewer.is_none_or(|v| m.viewers.contains(&v)))
            .map(|m| m.id)
            .collect();
        Self {
            events: events.observed(viewer, state.tick),
            deliveries,
            enhanced_missiles,
            returns,
        }
    }
}
