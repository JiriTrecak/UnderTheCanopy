//! The ability runtime (old `src/sim/abilities/*`): casting, statuses, triggers,
//! deliveries, persistent effects, auras and the sixteen operations, interpreting the
//! published ability library deterministically.
//!
//! **Saved state** lives on entities ([`Caster`] for units with abilities, [`Status`] list,
//! [`Links`] for summons, split forms, containment, hero returns and trigger bookkeeping,
//! `Unit::flight` and `Unit::taunt`) and in [`AbilityWorld`] (deliveries, persistent
//! instances, the lifecycle reaction queue and pending combat events). Temporary sight
//! sources are vision's (`State::knowledge.spells`).
//!
//! **Tick** (docs/simulation.md): step 2 runs [`step::lifecycle`] (statuses, world effects,
//! summon lifetimes, auras, weapon orders, casts and mana), cast orders, autocast and
//! taunts; step 6 the second cast-order pass; step 8 [`step::resolve`] (reactions,
//! persistent pulses, deliveries, releases, channels) and taunts.
//!
//! **Events** ([`events`]) are cosmetic and never saved.

pub mod actor;
pub mod ai;
pub mod cast;
pub mod corpses;
pub mod deliveries;
pub mod events;
pub mod forms;
pub mod library;
pub mod orders;
pub mod persistent;
pub mod program;
pub mod reactions;
pub mod sight;
pub mod state;
pub mod status_ops;
pub mod statuses;
pub mod step;
pub mod summons;
pub mod taunt;
pub mod view;
pub mod weapons;
pub mod world_ops;

pub use events::{AbilityEvent, EventKind, Events};
pub use forms::Flight;
pub use library::{AbilityId, Spells, StatusKey};
pub use state::{AbilityWorld, Caster, Links, Status};
pub use taunt::Taunt;

use crate::{
    catalog::Catalog,
    combat::Senses,
    entity::{Entity, EntityId, Milli},
    spatial::Spatial,
    state::State,
};
use actor::Actor;
use canopy_content::placement::Owner;
use events::EventPoint;

/// Everything ability code borrows for a pass besides the state.
pub struct Ctx<'a> {
    pub senses: Senses<'a>,
    pub spatial: &'a mut Spatial,
    pub events: &'a mut Events,
    pub economy: &'a mut crate::economy::Economy,
    pub limits: crate::heroes::Limits,
    /// Trigger programs running: the hits they cause start no hit or damage reactions.
    pub suppress: u32,
}

impl<'a> Ctx<'a> {
    pub fn catalog(&self) -> &'a Catalog {
        self.senses.catalog
    }

    pub fn ability(&self, id: AbilityId) -> &'a canopy_content::abilities::Ability {
        &self.senses.catalog.content.abilities.abilities[usize::from(id.0)]
    }
}

/// Gives a newly created entity its ability state and item slots (old `context.create`).
pub fn equip_new(catalog: &Catalog, e: &mut Entity) {
    let d = catalog.get(e.definition);
    if let Some(policy) = catalog.spells.caster(e.definition) {
        let max_mana = crate::stats::Stats::of(catalog, e, &[]).max_mana;
        e.caster = Some(Box::new(Caster {
            mana: max_mana,
            ranks: policy.bindings.iter().map(|b| b.initial_rank).collect(),
            ..Caster::default()
        }));
    }
    if let Some(inventory) = &d.behaviors.inventory {
        let q = crate::items::equipment(e);
        q.slots = vec![None; inventory.slots as usize];
    }
}

/// The links block, created on demand.
pub fn links(e: &mut Entity) -> &mut Links {
    e.links.get_or_insert_with(Default::default)
}

/// Drops an empty links block.
pub fn tidy(e: &mut Entity) {
    if e.links.as_ref().is_some_and(|l| l.is_empty()) {
        e.links = None;
    }
}

/// What an event lands on: an entity or a point.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    pub id: Option<EntityId>,
    pub position: Milli,
    pub surface: Option<String>,
    /// Absolute height in world units when known.
    pub height: Option<f64>,
}

impl Mark {
    pub fn point(position: Milli) -> Self {
        Self {
            id: None,
            position,
            surface: None,
            height: None,
        }
    }

    pub fn of(a: &Actor) -> Self {
        Self {
            id: Some(a.id),
            position: a.position,
            surface: a.surface.clone(),
            height: None,
        }
    }
}

/// Optional fields of an event.
#[derive(Debug, Clone, Default)]
pub struct Extra {
    pub amount: Option<i64>,
    pub status: Option<String>,
    pub reason: Option<String>,
    pub duration_ticks: Option<u32>,
    pub radius: Option<f64>,
    pub until_death: bool,
    pub spawned: Vec<EntityId>,
    pub origin: Option<EventPoint>,
}

impl Extra {
    pub fn amount(amount: i64) -> Self {
        Self {
            amount: Some(amount),
            ..Self::default()
        }
    }

    pub fn reason(reason: &str) -> Self {
        Self {
            reason: Some(reason.to_string()),
            ..Self::default()
        }
    }
}

/// The ground or deck height at a milli point, world units.
pub fn ground_height(ctx: &Ctx, p: Milli, surface: Option<&str>) -> f64 {
    let size = ctx.spatial.size() as i32 - 1;
    let deck = surface.and_then(|s| ctx.spatial.deck_id(s));
    let clamp = |v: i32| f64::from(v.clamp(0, size * 1000)) / 1000.0;
    ctx.spatial.height(clamp(p.x), clamp(p.y), deck)
}

/// An actor's absolute height.
pub fn actor_height(ctx: &Ctx, a: &Actor) -> f64 {
    ground_height(ctx, a.position, a.surface.as_deref()) + f64::from(a.elevation) / 100.0
}

fn point_of(ctx: &Ctx, state: &State, m: &Mark) -> EventPoint {
    let height = m
        .height
        .unwrap_or_else(|| match m.id.and_then(|id| state.entities.get(id)) {
            Some(e) => {
                let a = actor::of(&ctx.senses, state, e);
                actor_height(ctx, &a)
            }
            None => ground_height(ctx, m.position, m.surface.as_deref()),
        });
    EventPoint {
        x: m.position.x,
        y: m.position.y,
        height,
    }
}

/// Owners in the match, in slot order (old `viewers()`).
pub fn owners<'a>(ctx: &Ctx<'a>) -> &'a [Owner] {
    ctx.senses.teams.owners()
}

/// Records an event (old `emit`). Without an `audience` it is seen by owners who see the
/// caster and the target (or the target point).
#[allow(clippy::too_many_arguments)]
pub fn emit(
    ctx: &mut Ctx,
    state: &State,
    caster: &Actor,
    target: &Mark,
    cast: u32,
    ability: AbilityId,
    kind: EventKind,
    extra: Extra,
    audience: Option<Vec<Owner>>,
) {
    let tick = state.tick;
    let viewers = match audience {
        Some(v) => v,
        None => {
            let spell = ctx.ability(ability);
            let live_caster = state
                .entities
                .get(caster.id)
                .map(|e| actor::of(&ctx.senses, state, e));
            owners(ctx)
                .iter()
                .copied()
                .filter(|&owner| {
                    (target.id.is_none() && owner == caster.owner && !spell.targeting.visible)
                        || live_caster.as_ref().is_some_and(|c| {
                            visible(ctx, state, owner, c.id, Some(c))
                                && match target.id {
                                    Some(t) => {
                                        state.entities.get(t).is_some()
                                            && visible(ctx, state, owner, t, Some(c))
                                    }
                                    None => {
                                        visible_point(ctx, state, owner, target.position, Some(c))
                                    }
                                }
                        })
                })
                .collect()
        }
    };
    // The caster's own cast in progress gives point radii and channel timings (old `emit`).
    let pending = state
        .entities
        .get(caster.id)
        .and_then(|e| e.caster.as_ref())
        .and_then(|c| c.pending.as_ref());
    let spell = ctx.ability(ability);
    let mut radius = match (&spell.targeting.radius, pending) {
        (Some(r), Some(p)) => Some(library::value(spell, p.rank, r) as f64),
        _ => None,
    };
    let mut duration = pending
        .and_then(|p| p.channel.as_ref())
        .and_then(|c| match kind {
            EventKind::WaveStarted => Some(c.next_wave_tick.saturating_sub(tick)),
            EventKind::Released => Some(c.end_tick.saturating_sub(tick)),
            _ => None,
        });
    if extra.radius.is_some() {
        radius = extra.radius;
    }
    if extra.duration_ticks.is_some() {
        duration = extra.duration_ticks;
    }
    let origin = extra.origin.unwrap_or_else(|| EventPoint {
        x: caster.position.x,
        y: caster.position.y,
        height: actor_height(ctx, caster),
    });
    let point = point_of(ctx, state, target);
    ctx.events.push(AbilityEvent {
        id: 0,
        cast,
        tick,
        ability,
        kind,
        caster: caster.id,
        target: target.id,
        origin,
        point,
        viewers,
        amount: extra.amount,
        status: extra.status,
        reason: extra.reason,
        duration_ticks: duration,
        radius,
        until_death: extra.until_death,
        spawned: extra.spawned,
    });
}

/// Whether `owner` sees entity `target` for ability purposes (old `gameHost.visible`):
/// neutral casters see within [`crate::camps::CASTER_SIGHT`] cells over the terrain.
pub fn visible(
    ctx: &Ctx,
    state: &State,
    owner: Owner,
    target: EntityId,
    caster: Option<&Actor>,
) -> bool {
    let Some(entity) = state.entities.get(target) else {
        return false;
    };
    let sight = ctx.senses.sight(state);
    let observer = caster.and_then(|c| state.entities.get(c.id));
    if owner == Owner::Nobody {
        let Some(caster) = caster else {
            return false;
        };
        let r = i64::from(crate::camps::CASTER_SIGHT) * 1000;
        if actor::distance2(caster.position, entity.position) > r * r {
            return false;
        }
        if !sight.detects(owner, entity, observer) {
            return false;
        }
        let eye = eye_of(ctx, caster);
        let to = sight.sight_point(entity);
        return if forms::airborne(ctx.catalog(), entity) {
            ctx.senses.terrain.shot_clear(eye, to)
        } else {
            ctx.senses.terrain.visible(eye, to)
        };
    }
    sight.detects(owner, entity, observer) && sight.visible(owner, entity)
}

fn eye_of(ctx: &Ctx, a: &Actor) -> crate::terrain::SurfacePoint {
    crate::terrain::SurfacePoint {
        x: a.x(),
        y: a.y(),
        deck: a
            .surface
            .as_deref()
            .and_then(|s| ctx.senses.terrain.deck_id(s)),
        elevation: f64::from(a.elevation) / 100.0,
    }
}

/// Whether `owner` currently sees the cell at `p` (old `visiblePoint`).
pub fn visible_point(
    ctx: &Ctx,
    state: &State,
    owner: Owner,
    p: Milli,
    caster: Option<&Actor>,
) -> bool {
    if owner == Owner::Nobody {
        let Some(caster) = caster else {
            return false;
        };
        let r = i64::from(crate::camps::CASTER_SIGHT) * 1000;
        if actor::distance2(caster.position, p) > r * r {
            return false;
        }
        let to = crate::terrain::SurfacePoint {
            x: f64::from(p.x) / 1000.0,
            y: f64::from(p.y) / 1000.0,
            deck: None,
            elevation: 0.0,
        };
        return ctx.senses.terrain.visible(eye_of(ctx, caster), to);
    }
    let size = ctx.spatial.size() as i32;
    let (x, y) = (cell_round(p.x), cell_round(p.y));
    if x < 0 || y < 0 || x >= size || y >= size {
        return false;
    }
    ctx.senses
        .sight(state)
        .in_sight(owner, &[(y * size + x) as u32])
}

/// `Math.round` of a milli coordinate to a cell.
pub fn cell_round(v: i32) -> i32 {
    (v + 500).div_euclid(1000)
}

/// Clamps health and mana to the current maximums (old `clampPools`), after landing a
/// lapsed flight form and dropping a swing of a lapsed combat form.
pub fn clamp_pools(ctx: &mut Ctx, state: &mut State, id: EntityId) {
    forms::reconcile_flight(ctx, state, id);
    forms::reconcile_weapon(ctx.catalog(), state, id);
    let Some(e) = state.entities.get(id) else {
        return;
    };
    let stats = crate::stats::Stats::of(ctx.catalog(), e, crate::stats::research(state, e.owner));
    let e = state.entities.get_mut(id).expect("entity");
    clamp_to(e, &stats);
}

/// Clamps pools to `stats` without the flight and weapon reconciliation.
pub fn clamp_to(e: &mut Entity, stats: &crate::stats::Stats) {
    if let Some(hp) = &mut e.hp {
        *hp = (*hp).min(stats.max_hp);
    }
    if let Some(c) = &mut e.caster {
        let limit = stats.max_mana.saturating_sub(c.escrow());
        c.mana = c.mana.min(limit);
    }
}
