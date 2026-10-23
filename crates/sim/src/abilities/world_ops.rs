//! Operations on the world (old `worldEffects.ts`, `splitForms.ts`, `containment.ts`,
//! `ownership.ts`, the spell half of `revival.ts`): teleport, vision, conversion,
//! containment and its release, sacrifice, linked split forms and hero returns.

use super::{
    Ctx, Extra, Mark,
    actor::{self, Actor},
    cast::{self, Aim},
    events::EventKind,
    library::{self, AbilityId},
    program::{self, Recipient},
    state::{Containment, HeroReturn, Split, Summoned, UNTIL_DEATH},
    status_ops::Apply,
};
use crate::{
    entity::{EntityId, Milli, Place},
    orders::{discard_navigation, release_combat},
    spatial::Spot,
    state::State,
    stats::Stats,
};
use canopy_content::{
    abilities::{AllLost, Anchor, ContainLifetime, Effect, Operation, SupplyPolicy, Teleport},
    placement::Owner,
};

fn alive(e: &crate::entity::Entity) -> bool {
    actor::alive(e)
}

fn held(e: &crate::entity::Entity) -> bool {
    e.unit
        .as_ref()
        .is_some_and(|u| u.contained.is_some() || u.garrison.is_some() || u.release.is_some())
}

fn split_member(e: &crate::entity::Entity) -> bool {
    e.links
        .as_ref()
        .and_then(|l| l.summoned.as_ref())
        .is_some_and(|s| s.split.is_some())
}

/// Supply of an owner now.
pub fn supply(ctx: &mut Ctx, state: &State, owner: Owner) -> crate::economy::supply::Supply {
    let catalog = ctx.senses.catalog;
    let index = ctx.economy.index(state, catalog);
    crate::economy::supply::colony(state, catalog, index, owner)
}

fn sacrifice_reason(
    ctx: &Ctx,
    state: &State,
    source: EntityId,
    target: EntityId,
) -> Option<&'static str> {
    let (Some(s), Some(t)) = (state.entities.get(source), state.entities.get(target)) else {
        return Some("message.sacrifice-requires-an-owned-living-non");
    };
    let bad = !alive(s)
        || s.unit.is_none()
        || s.owner != t.owner
        || (s.owner == Owner::Nobody
            && (actor::camp(s).is_none() || actor::camp(s) != actor::camp(t)))
        || s.id == t.id
        || !alive(t)
        || t.unit.is_none()
        || t.fallen
        || hero(ctx, t)
        || split_member(t)
        || held(t);
    bad.then_some("message.sacrifice-requires-an-owned-living-non")
}

fn hero(ctx: &Ctx, e: &crate::entity::Entity) -> bool {
    ctx.catalog().get(e.definition).is_hero()
}

/// Where a teleported unit lands (old `landing`).
#[allow(clippy::too_many_arguments)]
fn landing(
    ctx: &Ctx,
    state: &State,
    id: EntityId,
    op: &Teleport,
    radius: i64,
    aim: Milli,
    aim_surface: Option<&str>,
    origin: Milli,
) -> Option<Spot> {
    let e = state.entities.get(id)?;
    let destination = if op.destination == Anchor::Caster {
        origin
    } else {
        aim
    };
    let offset = if op.preserve_offset {
        (e.position.x - origin.x, e.position.y - origin.y)
    } else {
        (0, 0)
    };
    let x = super::cell_round(destination.x + offset.0);
    let y = super::cell_round(destination.y + offset.1);
    let air = super::forms::airborne(ctx.catalog(), e);
    let deck = if air {
        None
    } else {
        aim_surface
            .filter(|_| op.destination != Anchor::Caster)
            .and_then(|s| ctx.spatial.deck_id(s))
    };
    let body = ctx.spatial.body_of(e);
    ctx.spatial.nearest(
        Spot::new(x, y, deck),
        radius as i32,
        Some(id),
        Some(&body),
        None,
        None,
    )
}

/// Whether a containment carrier could take `victim` (old `SpellContainments.reason`).
fn contain_reason(
    ctx: &Ctx,
    state: &State,
    host: EntityId,
    victim: EntityId,
    capacity: u32,
) -> Option<&'static str> {
    let h = state.entities.get(host);
    if h.is_none_or(|h| h.unit.is_none() || !alive(h) || held(h)) {
        return Some("message.carrier-is-unavailable");
    }
    let v = state.entities.get(victim);
    if v.is_none_or(|v| {
        v.unit.is_none() || !alive(v) || v.id == host || hero(ctx, v) || split_member(v) || held(v)
    }) {
        return Some("message.choose-an-available-non-hero-unit");
    }
    if !occupants(state, victim).is_empty() {
        return Some("message.cannot-contain-another-occupied-carrier");
    }
    let held_units = occupants(state, host);
    let mut capacity = capacity;
    for id in &held_units {
        let record = contained(state, *id).expect("held");
        if let Some(op) = contain_op(ctx, record.ability, &record.operation) {
            capacity = capacity.min(op.capacity);
        }
    }
    (held_units.len() as u32 >= capacity).then_some("message.carrier-is-full")
}

fn contained(state: &State, id: EntityId) -> Option<&Containment> {
    state.entities.get(id)?.links.as_ref()?.containment.as_ref()
}

fn contain_op<'a>(
    ctx: &Ctx<'a>,
    ability: AbilityId,
    name: &str,
) -> Option<&'a canopy_content::abilities::Contain> {
    library::all_effects(ctx.ability(ability)).find_map(|e| match e {
        Effect::Contain(c) if c.id == name => Some(c),
        _ => None,
    })
}

fn split_op<'a>(
    ctx: &Ctx<'a>,
    ability: AbilityId,
    name: &str,
) -> Option<&'a canopy_content::abilities::Split> {
    library::all_effects(ctx.ability(ability)).find_map(|e| match e {
        Effect::Split(s) if s.id == name => Some(s),
        _ => None,
    })
}

/// Units held by a spell carrier, in id order.
pub fn occupants(state: &State, host: EntityId) -> Vec<EntityId> {
    state
        .entities
        .iter()
        .filter(|e| {
            e.unit.is_some()
                && e.links
                    .as_ref()
                    .and_then(|l| l.containment.as_ref())
                    .is_some_and(|c| c.host == host)
        })
        .map(|e| e.id)
        .collect()
}

/// Whether a living unit is held inside `host` by a containing ability.
pub fn hosting(state: &State, host: EntityId) -> bool {
    state.entities.iter().any(|e| {
        alive(e)
            && e.links
                .as_ref()
                .and_then(|l| l.containment.as_ref())
                .is_some_and(|c| c.host == host)
    })
}

/// Why a unit cannot change hands (old `UnitOwnership.reason`).
fn convert_reason(
    ctx: &mut Ctx,
    state: &State,
    id: EntityId,
    owner: Owner,
    policy: SupplyPolicy,
    camp: Option<&str>,
) -> Option<String> {
    let e = state.entities.get(id)?;
    if e.unit.is_none() || !alive(e) || e.fallen || hero(ctx, e) {
        return Some("message.choose-a-living-non-hero-unit".into());
    }
    if owner != Owner::Nobody
        && ctx
            .catalog()
            .get(e.definition)
            .behaviors
            .player_control
            .is_none()
    {
        return Some("message.unit-does-not-support-player-control".into());
    }
    if split_member(e) || held(e) {
        return Some("message.unit-is-unavailable".into());
    }
    if e.owner == owner && (owner != Owner::Nobody || actor::camp(e) == camp) {
        return Some("message.unit-already-belongs-to-this-controller".into());
    }
    let cost = match policy {
        SupplyPolicy::Require => ctx.catalog().get(e.definition).supply_cost.unwrap_or(0),
        SupplyPolicy::AllowOverCap => 0,
    };
    let s = supply(ctx, state, owner);
    crate::economy::supply::admission(&s, cost, None)
}

/// Only the first operation is checked before the costs (old `validateAim`).
pub fn validate_aim(
    ctx: &mut Ctx,
    state: &State,
    caster: EntityId,
    aim: Aim,
    ability: AbilityId,
    rank: u8,
) -> Result<(), &'static str> {
    let spell = ctx.ability(ability);
    let Some(c) = state.entities.get(caster) else {
        return Ok(());
    };
    let point = match aim {
        Aim::Unit(id) => match state.entities.get(id) {
            Some(t) => t.position,
            None => return Ok(()),
        },
        Aim::Point(p) => p,
    };
    let Some(first) = spell.on_release.first() else {
        return Ok(());
    };
    let caster_actor = actor::of(&ctx.senses, state, c);
    let candidates: Vec<&Effect> = match first {
        Operation::Effect(e) => vec![e],
        Operation::Branch(b) => b
            .then
            .iter()
            .take(1)
            .chain(b.otherwise.iter().take(1))
            .collect(),
    };
    for op in candidates {
        if let Effect::Found(f) = op {
            // A colony founds; a neutral (a race's unit placed for nobody) never does.
            if c.owner == Owner::Nobody {
                return Err("message.only-a-colony-can-found");
            }
            let at = if f.target == Anchor::Caster {
                c.position
            } else {
                point
            };
            let catalog = ctx.senses.catalog;
            let def = catalog.find(f.building.as_str());
            let index = ctx.economy.index(state, catalog);
            let ground = crate::economy::construction::Ground {
                state,
                catalog,
                spatial: ctx.spatial,
                index,
            };
            if def.is_none_or(|d| {
                crate::economy::construction::found_site(&ground, c.owner, d, at).is_none()
            }) {
                return Err("message.no-room-to-found-here");
            }
            continue;
        }
        // Raising or consuming the dead needs a corpse to work on: an empty field refuses
        // the cast before its costs, as Warcraft's Resurrection does.
        let corpses = match op {
            Effect::Resurrect(r) => {
                let q = &r.corpses;
                Some((&q.radius, &q.relations, q.filter.as_ref(), q.order))
            }
            Effect::Summon(s) => s
                .corpses
                .as_ref()
                .map(|q| (&q.radius, &q.relations, q.filter.as_ref(), q.order)),
            _ => None,
        };
        if let Some((radius, relations, filter, order)) = corpses {
            let views = super::corpses::visible(ctx, state, &caster_actor);
            let radius = library::value(spell, rank, radius);
            if super::corpses::select(&views, radius, relations, filter, order, point).is_empty() {
                return Err("message.no-corpses-nearby");
            }
            continue;
        }
        if op.query().is_some() || op.target() == Anchor::Point {
            continue;
        }
        let target = if op.target() == Anchor::Caster {
            Some(caster_actor.clone())
        } else {
            match aim {
                Aim::Unit(id) => actor::get(&ctx.senses, state, id),
                Aim::Point(_) => None,
            }
        };
        let Some(target) = target else {
            continue;
        };
        if !actor::matches(&target, op.filter()) {
            continue;
        }
        if let Operation::Branch(b) = first {
            let side = actor::relation(&ctx.senses, &caster_actor, &target);
            let arm = if actor::condition(&b.condition, side, &caster_actor, &target) {
                &b.then
            } else {
                &b.otherwise
            };
            if !arm.iter().any(|e| std::ptr::eq(e, op)) {
                continue;
            }
        }
        if let Effect::Split(s) = op {
            split_reason(ctx, state, caster, s, rank)?;
        }
        if let Effect::Teleport(t) = op
            && op.target() == Anchor::Caster
        {
            let radius = library::value(spell, rank, &t.amount);
            if landing(ctx, state, caster, t, radius, point, None, c.position).is_none() {
                return Err("message.no-clear-arrival-position");
            }
        }
        if op.target() != Anchor::Target {
            continue;
        }
        match op {
            Effect::Sacrifice(_) => {
                if let Some(r) = sacrifice_reason(ctx, state, caster, target.id) {
                    return Err(r);
                }
            }
            Effect::Contain(k) => {
                if let Some(r) = contain_reason(ctx, state, caster, target.id, k.capacity) {
                    return Err(r);
                }
            }
            Effect::Convert(k) => {
                let camp = actor::camp(c).map(str::to_string);
                if let Some(r) =
                    convert_reason(ctx, state, target.id, c.owner, k.supply, camp.as_deref())
                {
                    return Err(leak(r));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Rejection messages are static in the cast path; dynamic supply messages are interned.
fn leak(message: String) -> &'static str {
    match message.as_str() {
        "message.choose-a-living-non-hero-unit" => "message.choose-a-living-non-hero-unit",
        "message.unit-does-not-support-player-control" => {
            "message.unit-does-not-support-player-control"
        }
        "message.unit-is-unavailable" => "message.unit-is-unavailable",
        "message.unit-already-belongs-to-this-controller" => {
            "message.unit-already-belongs-to-this-controller"
        }
        "message.unit-limit-reached" => "message.unit-limit-reached",
        _ => "message.not-enough-supply-short",
    }
}

/// Applies a world operation to one recipient (old `SpellWorldEffects.apply`).
pub fn apply(ctx: &mut Ctx, state: &mut State, a: &Apply) -> i64 {
    let target = match a.target {
        Recipient::Unit(t) => Some(t.id),
        Recipient::Point(_) => None,
    };
    let point = match a.target {
        Recipient::Unit(t) => t.position,
        Recipient::Point(p) => *p,
    };
    match a.effect {
        Effect::Split(s) => {
            let Some(parent) = state.entities.get(a.source) else {
                return 0;
            };
            if Some(a.source) != target || parent.owner != a.owner {
                return 0;
            }
            enter_split(ctx, state, a.source, a.ability, a.rank, a.cast, s)
        }
        Effect::Vision(v) => {
            let spell = ctx.ability(a.ability);
            let radius = library::value(spell, a.rank, &v.radius).max(0) as u32;
            let amount = a.amount.max(0) as u32;
            let surface = match a.target {
                Recipient::Unit(t) => t.surface.clone(),
                Recipient::Point(_) => None,
            };
            let vision = crate::vision::SpellVision {
                id: 0,
                owner: a.owner,
                point,
                surface,
                elevation: 0,
                radius: radius * 1000,
                ignore_terrain: v.ignore_terrain,
                detect_invisible: v.detect_invisible,
                expires: state.tick + amount,
                caster: v.ends_with_caster.then_some(a.source),
            };
            match crate::vision::add_spell_vision(state, vision) {
                Some(_) => i64::from(amount),
                None => 0,
            }
        }
        Effect::Contain(k) => {
            let Some(t) = target else {
                return 0;
            };
            let host_ok = state
                .entities
                .get(a.source)
                .is_some_and(|h| h.owner == a.owner);
            if !host_ok || state.entities.get(t).is_none() {
                return 0;
            }
            hold(ctx, state, a.source, t, a.ability, a.rank, a.cast, k)
        }
        Effect::ReleaseContained(_) => target.map_or(0, |t| release_hosted(ctx, state, t)),
        Effect::Found(f) => {
            if a.owner == Owner::Nobody {
                return 0;
            }
            let catalog = ctx.senses.catalog;
            let Some(def) = catalog.find(f.building.as_str()) else {
                return 0;
            };
            let mut ectx = ctx.economy.ctx(state, catalog, ctx.spatial);
            let Some(site) =
                crate::economy::construction::found_site(&ectx.ground(), a.owner, def, point)
            else {
                return 0;
            };
            crate::economy::construction::found(&mut ectx, a.owner, site);
            if f.consume {
                crate::economy::work::remove(&mut ectx, a.source, false);
            }
            1
        }
        Effect::Sacrifice(_) => {
            let Some(t) = target else {
                return 0;
            };
            let ok = state
                .entities
                .get(a.source)
                .is_some_and(|c| c.owner == a.owner)
                && sacrifice_reason(ctx, state, a.source, t).is_none();
            if !ok {
                return 0;
            }
            state.entities.get_mut(t).expect("victim").hp = Some(0);
            crate::combat::deaths::die(ctx, state, t);
            1
        }
        Effect::Convert(k) => {
            let Some(t) = target else {
                return 0;
            };
            let camp = a.origin.camp.clone();
            i64::from(transfer(ctx, state, t, a.owner, k.supply, camp.as_deref()))
        }
        Effect::Teleport(op) => {
            let Some(t) = target else {
                return 0;
            };
            let Some(e) = state.entities.get(t) else {
                return 0;
            };
            if e.unit.is_none() || !alive(e) || held(e) {
                return 0;
            }
            let (aim, aim_surface) = match a.aim {
                program::Aim::Unit(u) => (u.position, u.surface.clone()),
                program::Aim::Point(p) => (*p, None),
            };
            let Some(to) = landing(
                ctx,
                state,
                t,
                op,
                a.amount,
                aim,
                aim_surface.as_deref(),
                a.origin.position,
            ) else {
                return 0;
            };
            if t != a.source {
                cast::cancel(ctx, state, t, "Displaced");
            }
            crate::economy::interrupt(state, t, None);
            let surface = to.deck.map(|d| ctx.spatial.deck_name(d));
            let e = state.entities.get_mut(t).expect("unit");
            let u = e.unit.as_mut().expect("unit");
            discard_navigation(u);
            u.target = None;
            u.idle = None;
            u.work.pending_move = None;
            u.combat.attack = None;
            u.combat.pursuit = None;
            u.last_moved_tick = None;
            e.position = Milli {
                x: to.x * 1000,
                y: to.y * 1000,
            };
            e.surface = surface;
            ctx.spatial.update_body(e);
            1
        }
        Effect::Pull(op) => {
            let Some(t) = target else {
                return 0;
            };
            pull(ctx, state, a, op, t)
        }
        _ => 0,
    }
}

/// Drags `t` one step toward the pull's anchor over ground it could stand on, never
/// closer than the minimum distance; returns 1 when it moved.
fn pull(
    ctx: &mut Ctx,
    state: &mut State,
    a: &Apply,
    op: &canopy_content::abilities::Pull,
    t: EntityId,
) -> i64 {
    let Some(e) = state.entities.get(t) else {
        return 0;
    };
    if e.unit.is_none() || !alive(e) || held(e) || t == a.source {
        return 0;
    }
    let anchor = match (op.toward, a.aim) {
        (Anchor::Caster, _) => state
            .entities
            .get(a.source)
            .map_or(a.origin.position, |c| c.position),
        (Anchor::Target, program::Aim::Unit(u)) => {
            state.entities.get(u.id).map_or(u.position, |x| x.position)
        }
        (_, aim) => aim.position(),
    };
    let dx = f64::from(anchor.x - e.position.x);
    let dy = f64::from(anchor.y - e.position.y);
    let distance = (dx * dx + dy * dy).sqrt();
    let keep = op.min_distance * 1000.0;
    if distance <= keep || distance <= 0.0 {
        return 0;
    }
    // Hundredths of a world unit per application; positions are thousandths.
    let step = (a.amount.max(0) as f64 * 10.0).min(distance - keep);
    let to = Milli {
        x: e.position.x + crate::terrain::round_half_up(dx * step / distance) as i32,
        y: e.position.y + crate::terrain::round_half_up(dy * step / distance) as i32,
    };
    if to == e.position {
        return 0;
    }
    let body = ctx.spatial.body_of(e);
    let spot = Spot::new(
        super::cell_round(to.x),
        super::cell_round(to.y),
        ctx.spatial.deck_of(e),
    );
    // Ground only: bodies crowd together in a vortex.
    if !ctx
        .spatial
        .free(spot, Some(t), Some(&body), Some(&|_| false))
    {
        return 0;
    }
    let e = state.entities.get_mut(t).expect("unit");
    let u = e.unit.as_mut().expect("unit");
    discard_navigation(u);
    u.combat.attack = None;
    u.last_moved_tick = None;
    e.position = to;
    ctx.spatial.update_body(e);
    1
}

/// Holds `victim` inside `host` (old `SpellContainments.hold`).
#[allow(clippy::too_many_arguments)]
fn hold(
    ctx: &mut Ctx,
    state: &mut State,
    host: EntityId,
    victim: EntityId,
    ability: AbilityId,
    rank: u8,
    cast_id: u32,
    op: &canopy_content::abilities::Contain,
) -> i64 {
    if contain_reason(ctx, state, host, victim, op.capacity).is_some() {
        return 0;
    }
    cast::cancel(ctx, state, victim, "Contained");
    {
        let mut ectx = ctx.economy.ctx(state, ctx.senses.catalog, ctx.spatial);
        crate::economy::work::detach_unit(&mut ectx, victim);
    }
    let now = state.tick;
    let spell = ctx.ability(ability);
    let duration = if op.lifetime == ContainLifetime::UntilDeath {
        UNTIL_DEATH - now
    } else {
        library::value(spell, rank, &op.amount).max(0) as u32
    };
    let (host_position, host_surface, host_owner) = {
        let h = state.entities.get(host).expect("host");
        (anchor_cell(h), h.surface.clone(), h.owner)
    };
    let e = state.entities.get_mut(victim).expect("victim");
    let old = e.unit.take().expect("unit");
    let mut fresh = crate::entity::Unit {
        camp: old.camp.clone(),
        contained: Some(host),
        ..Default::default()
    };
    fresh.work.cargo = old.work.cargo.clone();
    fresh.combat.cooldown = old.combat.cooldown;
    if let Some(mut c) = old.combat.charge.clone() {
        c.target = None;
        fresh.combat.charge = Some(c);
    }
    e.unit = Some(Box::new(fresh));
    super::links(e).containment = Some(Containment {
        host,
        owner: host_owner,
        ability,
        operation: op.id.clone(),
        rank,
        cast: cast_id,
        started: now,
        expires: now.saturating_add(duration),
        next_tick: now.saturating_add(op.digestion.as_ref().map_or(duration, |d| d.interval_ticks)),
    });
    e.statuses.retain(|s| !s.aura);
    e.position = host_position;
    e.surface = host_surface;
    super::clamp_pools(ctx, state, victim);
    refresh_lent_sight(ctx, state, host);
    i64::from(duration)
}

/// Lends a carrier's sight to the owners of passengers held with `shareVision`.
pub fn refresh_lent_sight(ctx: &Ctx, state: &mut State, host: EntityId) {
    let mut owners: Vec<Owner> = Vec::new();
    let tick = state.tick;
    let host_ok = state.entities.get(host).is_some_and(|h| {
        alive(h)
            && h.unit
                .as_ref()
                .is_some_and(|u| u.contained.is_none() && u.release.is_none())
    });
    if host_ok {
        for id in occupants(state, host) {
            let e = state.entities.get(id).expect("occupant");
            let record = e
                .links
                .as_ref()
                .and_then(|l| l.containment.as_ref())
                .expect("held");
            let shares = contain_op(ctx, record.ability, &record.operation)
                .is_some_and(|op| op.share_vision == Some(true));
            let host_owner = state.entities.get(host).map(|h| h.owner);
            if shares
                && alive(e)
                && record.expires > tick
                && host_owner == Some(record.owner)
                && !owners.contains(&e.owner)
            {
                owners.push(e.owner);
            }
        }
    }
    if let Some(h) = state.entities.get_mut(host) {
        if owners.is_empty() && h.links.is_none() {
            return;
        }
        super::links(h).lent_sight = owners;
        super::tidy(h);
    }
}

/// Lets a held unit out near `at` (old `GameContext.release`).
pub fn release_unit(
    ctx: &mut Ctx,
    state: &mut State,
    id: EntityId,
    at: Milli,
    surface: Option<String>,
) {
    let Some(e) = state.entities.get_mut(id) else {
        return;
    };
    let Some(u) = e.unit.as_mut() else {
        return;
    };
    release_combat(u);
    u.garrison = None;
    u.detour = None;
    u.idle = None;
    u.segment = None;
    u.contained = None;
    u.work.job = None;
    u.work.employment = None;
    u.order = None;
    discard_navigation(u);
    u.release = Some(Place {
        x: super::cell_round(at.x),
        y: super::cell_round(at.y),
        surface,
    });
    crate::garrisons::settle(ctx.spatial, e);
}

fn release(ctx: &mut Ctx, state: &mut State, victim: EntityId, record: &Containment) {
    if let Some(h) = state.entities.get(record.host) {
        let (p, s) = (anchor_cell(h), h.surface.clone());
        let e = state.entities.get_mut(victim).expect("victim");
        e.position = p;
        e.surface = s;
    }
    if let Some(e) = state.entities.get_mut(victim)
        && let Some(l) = &mut e.links
    {
        l.containment = None;
        super::tidy(e);
    }
    let (at, surface) = {
        let e = state.entities.get(victim).expect("victim");
        (e.position, e.surface.clone())
    };
    release_unit(ctx, state, victim, at, surface);
    let carrier = actor::get(&ctx.senses, state, record.host);
    let Some(v) = actor::get(&ctx.senses, state, victim) else {
        return;
    };
    containment_event(
        ctx,
        state,
        carrier,
        &v,
        record,
        EventKind::ReleasedContained,
        0,
    );
    refresh_lent_sight(ctx, state, record.host);
}

fn containment_event(
    ctx: &mut Ctx,
    state: &State,
    carrier: Option<Actor>,
    victim: &Actor,
    record: &Containment,
    kind: EventKind,
    amount: i64,
) {
    let carrier = carrier.unwrap_or_else(|| {
        actor::placeholder(
            record.host,
            record.owner,
            victim.definition,
            victim.position,
        )
    });
    let viewers: Vec<Owner> = super::owners(ctx)
        .iter()
        .copied()
        .filter(|&o| {
            o == victim.owner
                || o == record.owner
                || super::visible_point(ctx, state, o, victim.position, None)
        })
        .collect();
    super::emit(
        ctx,
        state,
        &carrier,
        &Mark::of(victim),
        record.cast,
        record.ability,
        kind,
        Extra::amount(amount),
        Some(viewers),
    );
}

/// Releases everything a carrier holds (old `releaseHosted`).
pub fn release_hosted(ctx: &mut Ctx, state: &mut State, host: EntityId) -> i64 {
    let mut count = 0;
    for id in occupants(state, host) {
        let record = contained(state, id).cloned().expect("held");
        release(ctx, state, id, &record);
        count += 1;
    }
    count
}

/// Containment expiry, carriers lost and digestion (old `SpellContainments.tick`).
pub fn tick_containment(ctx: &mut Ctx, state: &mut State) {
    let tick = state.tick;
    let held: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|e| e.unit.is_some() && e.links.as_ref().is_some_and(|l| l.containment.is_some()))
        .map(|e| e.id)
        .collect();
    let mut hosts: Vec<EntityId> = Vec::new();
    for id in held {
        let Some(record) = contained(state, id).cloned() else {
            continue;
        };
        hosts.push(record.host);
        let op = contain_op(ctx, record.ability, &record.operation);
        let host_ok = state.entities.get(record.host).is_some_and(|h| {
            alive(h)
                && h.owner == record.owner
                && h.unit
                    .as_ref()
                    .is_some_and(|u| u.contained.is_none() && u.release.is_none())
        });
        if !host_ok || op.is_none() || record.expires <= tick {
            release(ctx, state, id, &record);
            continue;
        }
        let op = op.expect("contain");
        let (p, s) = {
            let h = state.entities.get(record.host).expect("host");
            (anchor_cell(h), h.surface.clone())
        };
        let e = state.entities.get_mut(id).expect("victim");
        e.position = p;
        e.surface = s;
        if let Some(digestion) = &op.digestion
            && record.next_tick <= tick
        {
            if let Some(c) = state
                .entities
                .get_mut(id)
                .and_then(|e| e.links.as_mut())
                .and_then(|l| l.containment.as_mut())
            {
                c.next_tick += digestion.interval_ticks;
            }
            let spell = ctx.ability(record.ability);
            let damage = library::value(spell, record.rank, &digestion.damage).max(0) as u32;
            let kind = ctx.catalog().spells.damage_type(&digestion.damage_type);
            let carrier = actor::get(&ctx.senses, state, record.host);
            let victim = actor::get(&ctx.senses, state, id);
            let dealt = super::status_ops::spell_damage(
                ctx,
                state,
                record.host,
                record.owner,
                id,
                damage,
                kind,
            );
            if let Some(v) = victim {
                containment_event(
                    ctx,
                    state,
                    carrier,
                    &v,
                    &record,
                    EventKind::Digested,
                    i64::from(dealt),
                );
            }
        }
    }
    hosts.sort_unstable();
    hosts.dedup();
    for host in hosts {
        refresh_lent_sight(ctx, state, host);
    }
}

// ----- split forms --------------------------------------------------------------------

/// Member placements of a split (old `plan`).
fn split_plan(
    ctx: &Ctx,
    state: &State,
    parent: EntityId,
    op: &canopy_content::abilities::Split,
    rank: u8,
) -> Option<Vec<(crate::catalog::Def, Spot)>> {
    let p = state.entities.get(parent)?;
    let deck = p.surface.as_deref().and_then(|s| ctx.spatial.deck_id(s));
    let mut out: Vec<(crate::catalog::Def, Spot)> = Vec::new();
    let n = op.members.len();
    for (index, member) in op.members.iter().enumerate() {
        let def = ctx.catalog().find(library::reference(member, rank))?;
        let profile = ctx.spatial.profiles.get(def);
        let body = profile.body;
        let angle = index as f64 * std::f64::consts::PI * 2.0 / n as f64;
        let r = f64::from(op.radius);
        let origin = Spot::new(
            super::cell_round(p.position.x)
                + crate::terrain::round_half_up(crate::math::cos(angle) * r) as i32,
            super::cell_round(p.position.y)
                + crate::terrain::round_half_up(crate::math::sin(angle) * r) as i32,
            deck,
        );
        let spatial = &*ctx.spatial;
        let accept = |s: Spot| {
            out.iter().all(|(other, at)| {
                let ob = spatial.profiles.get(*other).body;
                if ob.air != body.air {
                    return true;
                }
                let (dx, dy) = (f64::from(s.x - at.x), f64::from(s.y - at.y));
                dx * dx + dy * dy > (body.radius + ob.radius) * (body.radius + ob.radius)
            })
        };
        let at = spatial.nearest(
            origin,
            op.radius as i32,
            Some(parent),
            Some(&body),
            Some(&accept),
            None,
        )?;
        out.push((def, at));
    }
    Some(out)
}

/// Why the unit cannot split now (old `SpellSplitForms.reason`).
fn split_reason(
    ctx: &mut Ctx,
    state: &State,
    parent: EntityId,
    op: &canopy_content::abilities::Split,
    rank: u8,
) -> Result<(), &'static str> {
    if !occupants(state, parent).is_empty() {
        return Err("message.release-contained-units-before-entering-a");
    }
    let Some(p) = state.entities.get(parent) else {
        return Err("message.unit-cannot-enter-a-linked-form");
    };
    if p.unit.is_none()
        || !alive(p)
        || p.links.as_ref().is_some_and(|l| l.split.is_some())
        || split_member(p)
        || held(p)
    {
        return Err("message.unit-cannot-enter-a-linked-form");
    }
    let s = supply(ctx, state, p.owner);
    if s.units + s.queued_units + op.members.len() as u32 > s.unit_limit {
        return Err("message.unit-limit-reached");
    }
    if split_plan(ctx, state, parent, op, rank).is_none() {
        return Err("message.not-enough-clear-space-for-every");
    }
    Ok(())
}

/// Replaces the unit with its linked members (old `SpellSplitForms.enter`).
fn enter_split(
    ctx: &mut Ctx,
    state: &mut State,
    parent: EntityId,
    ability: AbilityId,
    rank: u8,
    cast_id: u32,
    op: &canopy_content::abilities::Split,
) -> i64 {
    if split_reason(ctx, state, parent, op, rank).is_err() {
        return 0;
    }
    let plan = split_plan(ctx, state, parent, op, rank).expect("plan");
    let now = state.tick;
    let spell = ctx.ability(ability);
    let expires = now + library::value(spell, rank, &op.amount).max(0) as u32;
    let (owner, rotation, camp) = {
        let p = state.entities.get(parent).expect("parent");
        (p.owner, p.rotation, actor::camp(p).map(str::to_string))
    };
    let mut members = Vec::new();
    for (def, at) in plan {
        let id = super::summons::spawn(ctx, state, def, owner, at, rotation);
        let e = state.entities.get_mut(id).expect("member");
        if let Some(u) = &mut e.unit {
            u.camp = camp.clone().map(|id| crate::camps::Member {
                id,
                returning: false,
            });
        }
        super::links(e).summoned = Some(Summoned {
            source: parent,
            ability,
            rank,
            cast: cast_id,
            started: now,
            expires,
            split: Some(op.id.clone()),
            link: None,
            reanimated_from: None,
        });
        members.push(id);
    }
    {
        let mut ectx = ctx.economy.ctx(state, ctx.senses.catalog, ctx.spatial);
        crate::economy::work::detach_unit(&mut ectx, parent);
    }
    let first = members[0];
    let e = state.entities.get_mut(parent).expect("parent");
    let old = e.unit.take().expect("unit");
    let mut fresh = crate::entity::Unit {
        camp: old.camp.clone(),
        contained: Some(first),
        ..Default::default()
    };
    fresh.work.cargo = old.work.cargo.clone();
    fresh.combat.cooldown = old.combat.cooldown;
    e.unit = Some(Box::new(fresh));
    e.statuses.clear();
    if let Some(q) = &mut e.equipment {
        q.statuses.clear();
    }
    let record = Split {
        owner,
        camp,
        ability,
        operation: op.id.clone(),
        rank,
        cast: cast_id,
        started: now,
        expires,
        members: members.clone(),
    };
    super::links(e).split = Some(record.clone());
    follow(state, parent, first);
    super::clamp_pools(ctx, state, parent);
    split_event(
        ctx,
        state,
        parent,
        &record,
        EventKind::SplitStarted,
        members.clone(),
        true,
    );
    members.len() as i64
}

/// The cell a held body follows: the old game copied the anchor's whole-cell position, not
/// its precise point between cells.
fn anchor_cell(anchor: &crate::entity::Entity) -> Milli {
    let (x, y) = anchor.position.cell();
    crate::items::inventory::cell_point(x, y)
}

fn follow(state: &mut State, parent: EntityId, anchor: EntityId) {
    let Some(a) = state.entities.get(anchor) else {
        return;
    };
    let (p, s) = (anchor_cell(a), a.surface.clone());
    let e = state.entities.get_mut(parent).expect("parent");
    e.position = p;
    e.surface = s;
    if let Some(u) = &mut e.unit {
        u.contained = Some(anchor);
    }
}

fn split_event(
    ctx: &mut Ctx,
    state: &State,
    parent: EntityId,
    record: &Split,
    kind: EventKind,
    spawned: Vec<EntityId>,
    success: bool,
) {
    let Some(a) = actor::get(&ctx.senses, state, parent) else {
        return;
    };
    let viewers: Vec<Owner> = super::owners(ctx)
        .iter()
        .copied()
        .filter(|&o| o == record.owner || super::visible_point(ctx, state, o, a.position, None))
        .collect();
    let extra = Extra {
        spawned,
        amount: Some(i64::from(success)),
        duration_ticks: (kind == EventKind::SplitStarted).then(|| record.expires - record.started),
        ..Extra::default()
    };
    super::emit(
        ctx,
        state,
        &a,
        &Mark::of(&a),
        record.cast,
        record.ability,
        kind,
        extra,
        Some(viewers),
    );
}

fn living_members(
    state: &State,
    parent: EntityId,
    s: &Split,
    excluding: Option<EntityId>,
) -> Vec<EntityId> {
    s.members
        .iter()
        .copied()
        .filter(|&id| {
            Some(id) != excluding
                && state.entities.get(id).is_some_and(|e| {
                    alive(e)
                        && e.owner == s.owner
                        && actor::camp(e).map(str::to_string) == s.camp
                        && e.links
                            .as_ref()
                            .and_then(|l| l.summoned.as_ref())
                            .is_some_and(|m| m.source == parent && m.cast == s.cast)
                })
        })
        .collect()
}

fn cleanup(
    ctx: &mut Ctx,
    state: &mut State,
    parent: EntityId,
    s: &Split,
    excluding: Option<EntityId>,
) {
    if let Some(e) = state.entities.get_mut(parent) {
        if let Some(l) = &mut e.links {
            l.split = None;
        }
        if let Some(u) = &mut e.unit {
            u.contained = None;
        }
        super::tidy(e);
    }
    for &id in &s.members {
        if Some(id) == excluding {
            continue;
        }
        let ours = state.entities.get(id).is_some_and(|e| {
            e.links
                .as_ref()
                .and_then(|l| l.summoned.as_ref())
                .is_some_and(|m| m.source == parent && m.cast == s.cast)
        });
        if ours {
            let mut ectx = ctx.economy.ctx(state, ctx.senses.catalog, ctx.spatial);
            crate::economy::work::remove(&mut ectx, id, false);
        }
    }
}

fn finish_split(
    ctx: &mut Ctx,
    state: &mut State,
    parent: EntityId,
    s: &Split,
    success: bool,
    op: &canopy_content::abilities::Split,
    excluding: Option<EntityId>,
) {
    cleanup(ctx, state, parent, s, excluding);
    if !success {
        state.entities.get_mut(parent).expect("parent").hp = Some(0);
        split_event(
            ctx,
            state,
            parent,
            s,
            EventKind::SplitEnded,
            Vec::new(),
            false,
        );
        crate::combat::deaths::die(ctx, state, parent);
        return;
    }
    let spell = ctx.ability(s.ability);
    let e = state.entities.get(parent).expect("parent");
    let stats = Stats::of(ctx.catalog(), e, crate::stats::research(state, e.owner));
    let health = op
        .return_health_permille
        .as_ref()
        .map(|v| library::value(spell, s.rank, v));
    let mana = op
        .return_mana_permille
        .as_ref()
        .map(|v| library::value(spell, s.rank, v));
    let e = state.entities.get_mut(parent).expect("parent");
    if let Some(h) = health {
        e.hp = Some(((i64::from(stats.max_hp) * h / 1000).max(1)) as u32);
    }
    if let (Some(c), Some(m)) = (&mut e.caster, mana) {
        c.mana = (i64::from(stats.max_mana) * m / 1000).max(0) as u32;
    }
    let (at, surface) = (e.position, e.surface.clone());
    release_unit(ctx, state, parent, at, surface);
    super::clamp_pools(ctx, state, parent);
    split_event(
        ctx,
        state,
        parent,
        s,
        EventKind::SplitEnded,
        Vec::new(),
        true,
    );
}

/// Keeps a split parent with its members (old `reconcile`).
fn reconcile_split(
    ctx: &mut Ctx,
    state: &mut State,
    parent: EntityId,
    excluding: Option<EntityId>,
) {
    let Some(s) = state
        .entities
        .get(parent)
        .and_then(|e| e.links.as_ref())
        .and_then(|l| l.split.clone())
    else {
        return;
    };
    let op = split_op(ctx, s.ability, &s.operation);
    let e = state.entities.get(parent).expect("parent");
    let valid = alive(e) && e.owner == s.owner && actor::camp(e).map(str::to_string) == s.camp;
    let Some(op) = op.filter(|_| valid) else {
        cleanup(ctx, state, parent, &s, excluding);
        if state.entities.get(parent).is_some_and(alive) {
            let (at, surface) = {
                let e = state.entities.get(parent).expect("parent");
                (e.position, e.surface.clone())
            };
            release_unit(ctx, state, parent, at, surface);
        }
        return;
    };
    let members = living_members(state, parent, &s, excluding);
    if let Some(&first) = members.first() {
        follow(state, parent, first);
    }
    if members.is_empty() || s.expires <= state.tick {
        let success = !members.is_empty() || op.on_all_lost == AllLost::Return;
        finish_split(ctx, state, parent, &s, success, op, excluding);
    }
}

/// A split parent or member leaves the world (old `SpellSplitForms.removing`).
pub fn removing(ctx: &mut Ctx, state: &mut State, id: EntityId) {
    let Some(e) = state.entities.get(id) else {
        return;
    };
    if let Some(s) = e.links.as_ref().and_then(|l| l.split.clone()) {
        cleanup(ctx, state, id, &s, Some(id));
        return;
    }
    let member = e
        .links
        .as_ref()
        .and_then(|l| l.summoned.as_ref())
        .filter(|m| m.split.is_some())
        .map(|m| (m.source, m.cast));
    if let Some((parent, cast_id)) = member
        && state
            .entities
            .get(parent)
            .and_then(|p| p.links.as_ref())
            .and_then(|l| l.split.as_ref())
            .is_some_and(|s| s.cast == cast_id)
    {
        reconcile_split(ctx, state, parent, Some(id));
    }
}

/// Split forms each tick (old `SpellSplitForms.tick`).
pub fn tick_splits(ctx: &mut Ctx, state: &mut State) {
    let parents: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|e| e.unit.is_some() && e.links.as_ref().is_some_and(|l| l.split.is_some()))
        .map(|e| e.id)
        .collect();
    for id in parents {
        reconcile_split(ctx, state, id, None);
    }
}

// ----- conversion --------------------------------------------------------------------

/// Permanently transfers a unit (old `UnitOwnership.transfer`).
pub fn transfer(
    ctx: &mut Ctx,
    state: &mut State,
    id: EntityId,
    owner: Owner,
    policy: SupplyPolicy,
    camp: Option<&str>,
) -> bool {
    if convert_reason(ctx, state, id, owner, policy, camp).is_some() {
        return false;
    }
    release_hosted(ctx, state, id);
    cast::cancel(ctx, state, id, "message.owner-changed");
    {
        let mut ectx = ctx.economy.ctx(state, ctx.senses.catalog, ctx.spatial);
        crate::economy::work::detach_unit(&mut ectx, id);
    }
    let e = state.entities.get_mut(id).expect("unit");
    e.owner = owner;
    let old = e.unit.take().expect("unit");
    let mut fresh = crate::entity::Unit {
        camp: if owner == Owner::Nobody {
            camp.map(|id| crate::camps::Member {
                id: id.to_string(),
                returning: false,
            })
        } else {
            None
        },
        segment: old.segment.clone(),
        ..Default::default()
    };
    fresh.work.cargo = old.work.cargo.clone();
    fresh.combat.cooldown = old.combat.cooldown;
    fresh.flight = old.flight;
    if let Some(mut c) = old.combat.charge.clone() {
        c.target = None;
        fresh.combat.charge = Some(c);
    }
    e.unit = Some(Box::new(fresh));
    e.statuses.retain(|s| !s.aura);
    super::clamp_pools(ctx, state, id);
    if let Some(e) = state.entities.get(id) {
        ctx.spatial.update_body(e);
    }
    true
}

// ----- hero returns ------------------------------------------------------------------

/// Schedules the fallen holder hero's return (old `Revival.schedule`); returns the delay.
pub fn schedule_revive(
    ctx: &mut Ctx,
    state: &mut State,
    caster: &Actor,
    ability: AbilityId,
    rank: u8,
    cast_id: u32,
    op: &canopy_content::abilities::Revive,
) -> u32 {
    let Some(hero) = state.entities.get(caster.id) else {
        return 0;
    };
    let queued = state.entities.iter().any(|e| {
        e.revival
            .as_ref()
            .is_some_and(|r| r.queue.iter().any(|q| q.hero == caster.id))
    });
    if !hero.fallen
        || !ctx.catalog().get(hero.definition).is_hero()
        || hero.owner != caster.owner
        || hero
            .links
            .as_ref()
            .is_some_and(|l| l.spell_return.is_some())
        || queued
    {
        return 0;
    }
    let spell = ctx.ability(ability);
    let delay = library::value(spell, rank, &op.amount).max(0) as u32;
    let now = state.tick;
    let e = state.entities.get_mut(caster.id).expect("hero");
    super::links(e).spell_return = Some(HeroReturn {
        ability,
        operation: op.id.clone(),
        rank,
        cast: cast_id,
        owner: caster.owner,
        started: now,
        due: now + delay,
        deadline: now + delay + op.placement_wait_ticks,
    });
    delay
}

/// Whether a fallen hero is returning through an ability.
pub fn returning(e: &crate::entity::Entity) -> bool {
    e.links.as_ref().is_some_and(|l| l.spell_return.is_some())
}

/// Hero returns due this tick (the ability half of old `Revival.tick`).
pub fn tick_returns(ctx: &mut Ctx, state: &mut State) {
    let heroes: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|e| e.unit.is_some() && returning(e))
        .map(|e| e.id)
        .collect();
    for id in heroes {
        let e = state.entities.get(id).expect("hero");
        let record = e
            .links
            .as_ref()
            .and_then(|l| l.spell_return.clone())
            .expect("return");
        let op = library::all_effects(ctx.ability(record.ability)).find_map(|x| match x {
            Effect::Revive(r) if r.id == record.operation => Some(r),
            _ => None,
        });
        let spell = ctx.ability(record.ability);
        let valid = e.fallen
            && e.owner == record.owner
            && op.is_some()
            && library::parameters(spell, record.rank).is_some();
        if !valid {
            clear_return(state, id);
            revival_outcome(ctx, state, id, &record, false);
            continue;
        }
        let op = op.expect("revive");
        if state.tick < record.due {
            continue;
        }
        let owner = e.owner;
        let s = supply(ctx, state, owner);
        let location = if s.units < s.unit_limit {
            let e = state.entities.get(id).expect("hero");
            let deck = e.surface.as_deref().and_then(|d| ctx.spatial.deck_id(d));
            let body = ctx.spatial.body_of(e);
            ctx.spatial.nearest(
                Spot::new(
                    super::cell_round(e.position.x),
                    super::cell_round(e.position.y),
                    deck,
                ),
                op.placement_radius as i32,
                Some(id),
                Some(&body),
                None,
                None,
            )
        } else {
            None
        };
        match location {
            Some(spot) => {
                let health = library::value(spell, record.rank, &op.health_permille);
                let mana = library::value(spell, record.rank, &op.mana_permille);
                {
                    let mut ectx = ctx.economy.ctx(state, ctx.senses.catalog, ctx.spatial);
                    crate::economy::revival::restore(&mut ectx, id, spot);
                }
                restore_pools(ctx.catalog(), state, id, health, mana);
                clear_return(state, id);
                revival_outcome(ctx, state, id, &record, true);
            }
            None if state.tick >= record.deadline => {
                clear_return(state, id);
                revival_outcome(ctx, state, id, &record, false);
            }
            None => {}
        }
    }
}

/// Health and mana of a returned hero as fractions of the maximum (old `returnAt`).
/// Pools of a returning hero (the end of old `returnAt`): health and mana at the given
/// shares, casting and regeneration restarted. Building revival returns them full.
pub fn restore_pools(
    catalog: &crate::catalog::Catalog,
    state: &mut State,
    id: EntityId,
    health_permille: i64,
    mana_permille: i64,
) {
    let Some(e) = state.entities.get(id) else {
        return;
    };
    let stats = Stats::of(catalog, e, crate::stats::research(state, e.owner));
    let e = state.entities.get_mut(id).expect("hero");
    e.hp = Some(((i64::from(stats.max_hp) * health_permille / 1000).max(1)) as u32);
    if let Some(c) = &mut e.caster {
        c.mana = (i64::from(stats.max_mana) * mana_permille / 1000).max(0) as u32;
        c.pending = None;
        c.weapon_order = None;
        c.regeneration = 0;
    }
    if let Some(u) = &mut e.unit {
        u.regeneration = 0;
    }
}

fn clear_return(state: &mut State, id: EntityId) {
    if let Some(e) = state.entities.get_mut(id) {
        if let Some(l) = &mut e.links {
            l.spell_return = None;
        }
        super::tidy(e);
    }
}

fn revival_outcome(ctx: &mut Ctx, state: &State, id: EntityId, record: &HeroReturn, success: bool) {
    let Some(a) = actor::get(&ctx.senses, state, id) else {
        return;
    };
    let viewers: Vec<Owner> = super::owners(ctx)
        .iter()
        .copied()
        .filter(|&o| o == a.owner || super::visible_point(ctx, state, o, a.position, Some(&a)))
        .collect();
    let kind = if success {
        EventKind::Revived
    } else {
        EventKind::RevivalCancelled
    };
    super::emit(
        ctx,
        state,
        &a,
        &Mark::of(&a),
        record.cast,
        record.ability,
        kind,
        Extra::default(),
        Some(viewers),
    );
}
