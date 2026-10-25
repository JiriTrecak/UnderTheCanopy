//! Deliveries (old `AbilityRuntime.launch`/`deliver`, `swarm.ts`, `charge.ts`): homing
//! projectiles, chains, lines, charges and swarm seekers. Positions in flight step on the
//! old 1/1024 cell grid ([`Fine`]).

use super::{
    Ctx, Extra, Mark,
    actor::{self, Actor, Side},
    cast,
    events::{EventKind, EventPoint},
    library::{self, AbilityId},
    program::{self, Aim, Cargo, Ops, Run},
    state::{Delivery as Shot, Fine, MAX_DELIVERIES, Pending, Seeker, SeekerPhase},
};
use crate::{
    catalog::Catalog,
    entity::{Entity, EntityId, Milli},
    spatial::Fixed,
    state::State,
};
use canopy_content::abilities::{Ability, Delivery};

/// Whether an ability's delivery is a charge.
pub fn is_charge(catalog: &Catalog, ability: AbilityId) -> bool {
    matches!(
        catalog.content.abilities.abilities[usize::from(ability.0)].delivery,
        Some(Delivery::Charge { .. })
    )
}

/// Whether the unit is charging through an ability (old `charging`).
pub fn charging(catalog: &Catalog, state: &State, id: EntityId) -> bool {
    state
        .abilities
        .deliveries
        .iter()
        .any(|d| d.source == id && is_charge(catalog, d.ability))
}

fn fine_of(ctx: &Ctx, a: &Actor) -> Fine {
    Fine::of(a.position, super::actor_height(ctx, a))
}

fn event_point(p: &Fine) -> EventPoint {
    let m = p.milli();
    EventPoint {
        x: m.x,
        y: m.y,
        height: p.hf(),
    }
}

/// Launches the delivery of a released cast (old `launch`).
pub fn launch(ctx: &mut Ctx, state: &mut State, caster: &Actor, p: &Pending) {
    if state.abilities.deliveries.len() >= MAX_DELIVERIES {
        return;
    }
    let spell = ctx.ability(p.ability);
    let Some(delivery) = spell.delivery.clone() else {
        return;
    };
    let mark = cast::mark_of(ctx, state, p, caster);
    let origin = fine_of(ctx, caster);
    let target_height = match mark.id.and_then(|id| actor::get(&ctx.senses, state, id)) {
        Some(t) => super::actor_height(ctx, &t),
        None => super::ground_height(ctx, mark.position, None),
    };
    let mut point = Fine::of(mark.position, target_height);
    if let Delivery::Line { length, .. } = &delivery {
        let (dx, dy) = (point.xf() - origin.xf(), point.yf() - origin.yf());
        let len = crate::math::hypot(dx, dy);
        let len = if len == 0.0 { 1.0 } else { len };
        let (x, y) = (
            origin.xf() + dx / len * length,
            origin.yf() + dy / len * length,
        );
        let at = Milli {
            x: crate::terrain::round_half_up(x * 1000.0) as i32,
            y: crate::terrain::round_half_up(y * 1000.0) as i32,
        };
        point = Fine {
            x: (x * super::state::FINE).round() as i64,
            y: (y * super::state::FINE).round() as i64,
            height: (super::ground_height(ctx, at, None) * super::state::FINE).round() as i64,
        };
    }
    let source = state
        .entities
        .get(caster.id)
        .map(|e| Box::new(actor::source(&ctx.senses, state, e)));
    let tick = state.tick;
    let shot = Shot {
        cast: p.id,
        ability: p.ability,
        rank: p.rank,
        source: caster.id,
        owner: caster.owner,
        target: mark.id,
        origin,
        position: origin,
        point,
        started: tick,
        next_tick: tick + 1,
        hit: Vec::new(),
        power: 1000,
        swarm: None,
        source_context: source,
    };
    if let Delivery::Swarm {
        count,
        duration_ticks,
        launch_interval_ticks,
        ..
    } = &delivery
    {
        let count = library::value(spell, p.rank, count).max(0) as u32;
        let duration = library::value(spell, p.rank, duration_ticks).max(0) as u32;
        // A swarm is admitted whole; a partial one would change its balance.
        if state.abilities.deliveries.len() + count as usize > MAX_DELIVERIES {
            return;
        }
        for index in 0..count {
            let cast = cast::next_cast(state);
            let mut seeker = shot.clone();
            seeker.cast = cast;
            seeker.target = None;
            seeker.swarm = Some(Seeker {
                group: p.id,
                index,
                launch_tick: shot.started + index * launch_interval_ticks,
                expires: shot.started + duration,
                phase: SeekerPhase::Waiting,
                hits: 0,
                health: 0,
                mana: 0,
            });
            state.abilities.deliveries.push(seeker);
        }
        return;
    }
    state.abilities.deliveries.push(shot);
    let duration = match &delivery {
        Delivery::Chain { .. } => 1,
        Delivery::Projectile { speed }
        | Delivery::Line { speed, .. }
        | Delivery::Charge { speed, .. } => {
            let distance = crate::math::hypot(point.xf() - origin.xf(), point.yf() - origin.yf());
            ((distance * 40.0 / speed).ceil() as u32).max(1)
        }
        Delivery::Swarm { .. } => 1,
    };
    let radius = match &delivery {
        Delivery::Line { width, .. } => *width,
        _ => 0.0,
    };
    let target = Mark {
        id: mark.id,
        position: point.milli(),
        surface: None,
        height: Some(point.hf()),
    };
    let extra = Extra {
        duration_ticks: Some(duration),
        radius: Some(radius),
        ..Extra::default()
    };
    super::emit(
        ctx,
        state,
        caster,
        &target,
        p.id,
        p.ability,
        EventKind::Projectile,
        extra,
        None,
    );
}

/// Where the entities with health stand, in id order, for [`candidates_among`] while
/// none moves, appears or goes.
#[derive(Debug, Default)]
pub struct Standing {
    /// The entity id counter and count it was taken at, while it holds.
    taken: Option<(u32, usize)>,
    at: Vec<(EntityId, Milli)>,
}

impl Standing {
    /// The positions, taken again when stale or `forget`-ed.
    pub fn of(&mut self, state: &State) -> &[(EntityId, Milli)] {
        let now = (state.counters.entity, state.entities.len());
        if self.taken != Some(now) {
            self.at.clear();
            self.at.extend(
                state
                    .entities
                    .iter()
                    .filter(|e| e.hp.is_some())
                    .map(|e| (e.id, e.position)),
            );
            self.taken = Some(now);
        }
        &self.at
    }

    /// After something that may have moved an entity.
    pub fn forget(&mut self) {
        self.taken = None;
    }
}

/// Eligible units around a point by the ability's targeting (old `candidates`), in id
/// order.
pub fn candidates(
    ctx: &Ctx,
    state: &State,
    caster: &Actor,
    spell: &Ability,
    point: (f64, f64),
    radius: f64,
) -> Vec<Actor> {
    candidates_of(
        ctx,
        state,
        caster,
        spell,
        point,
        radius,
        state.entities.iter(),
    )
}

/// [`candidates`] among the entities `standing` holds (current: nothing moved since),
/// leaving out those `leave` names (before their snapshots are taken).
#[allow(clippy::too_many_arguments)]
pub fn candidates_among(
    ctx: &Ctx,
    state: &State,
    caster: &Actor,
    spell: &Ability,
    point: (f64, f64),
    radius: f64,
    standing: &[(EntityId, Milli)],
    leave: impl Fn(EntityId) -> bool,
) -> Vec<Actor> {
    let r2 = radius * radius;
    let near = standing
        .iter()
        .filter(|(_, p)| {
            let (dx, dy) = (
                f64::from(p.x) / 1000.0 - point.0,
                f64::from(p.y) / 1000.0 - point.1,
            );
            dx * dx + dy * dy <= r2
        })
        .filter(|&&(id, _)| !leave(id))
        .filter_map(|&(id, _)| state.entities.get(id));
    candidates_of(ctx, state, caster, spell, point, radius, near)
}

fn candidates_of<'s>(
    ctx: &Ctx,
    state: &'s State,
    caster: &Actor,
    spell: &Ability,
    point: (f64, f64),
    radius: f64,
    entities: impl Iterator<Item = &'s Entity>,
) -> Vec<Actor> {
    let r2 = radius * radius;
    entities
        .filter(|e| e.hp.is_some() && actor::alive(e))
        .filter(|e| {
            let (dx, dy) = (
                f64::from(e.position.x) / 1000.0 - point.0,
                f64::from(e.position.y) / 1000.0 - point.1,
            );
            dx * dx + dy * dy <= r2
        })
        // The tests below that read only the entity, before taking its snapshot.
        .filter(|e| {
            let side = actor::relation_of(
                &ctx.senses,
                caster.id,
                caster.owner,
                caster.camp.as_deref(),
                e.id,
                e.owner,
                actor::camp(e),
            );
            actor::targetable(e)
                && (e.unit.is_some() || spell.targeting.include_buildings == Some(true))
                && (spell.targeting.allow_self || e.id != caster.id)
                && side.is(&spell.targeting.relations)
        })
        .map(|e| actor::of(&ctx.senses, state, e))
        .filter(|t| {
            let side = actor::relation(&ctx.senses, caster, t);
            t.alive
                && cast::matches_target(spell, t, caster, side)
                && cast::accepts(spell, t, side)
                && (t.unit || spell.targeting.include_buildings == Some(true))
                && t.targetable
                && (spell.targeting.allow_self || t.id != caster.id)
                && side.is(&spell.targeting.relations)
        })
        .collect()
}

/// Whether a delivery target still qualifies (relation, filters, immunity).
fn eligible(ctx: &Ctx, spell: &Ability, caster: &Actor, t: &Actor) -> bool {
    let side = actor::relation(&ctx.senses, caster, t);
    t.alive
        && t.targetable
        && cast::matches_target(spell, t, caster, side)
        && cast::accepts(spell, t, side)
        && side.is(&spell.targeting.relations)
}

fn caster_of(ctx: &Ctx, state: &State, shot: &Shot) -> Actor {
    if let Some(a) = actor::get(&ctx.senses, state, shot.source) {
        return a;
    }
    match &shot.source_context {
        Some(s) => actor::restore(ctx.catalog(), s),
        None => {
            let def = state
                .entities
                .get(shot.source)
                .map_or(crate::catalog::Def(0), |e| e.definition);
            actor::placeholder(shot.source, shot.owner, def, shot.origin.milli())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn hit_unit(
    ctx: &mut Ctx,
    state: &mut State,
    caster: &Actor,
    target: Actor,
    shot: &Shot,
    power: u32,
    visual: Option<EventPoint>,
    carry: Option<&mut Cargo>,
) {
    let spell = ctx.ability(shot.ability);
    program::run(
        ctx,
        state,
        Run {
            caster: caster.clone(),
            aim: Aim::Unit(Box::new(target)),
            cast: shot.cast,
            ability: shot.ability,
            rank: shot.rank,
            power,
            aura: false,
            area_defaults: false,
            operations: Ops::Program(&spell.on_release),
            origin_viewers: None,
            visual_origin: visual,
            creation_grant: false,
            carry,
            live: None,
        },
    );
}

/// Advances every delivery (old `deliver`).
pub fn tick(ctx: &mut Ctx, state: &mut State) {
    let mut i = 0;
    while i < state.abilities.deliveries.len() {
        let shot = state.abilities.deliveries[i].clone();
        let done = step(ctx, state, shot.clone());
        // The list may have changed under nested programs (casts, cancels): find the shot.
        let at = state
            .abilities
            .deliveries
            .iter()
            .position(|d| d.cast == shot.cast);
        match (done, at) {
            (Some(updated), Some(j)) => {
                state.abilities.deliveries[j] = updated;
                i = j + 1;
            }
            (None, Some(j)) => {
                state.abilities.deliveries.remove(j);
                i = j;
            }
            (_, None) => {}
        }
    }
}

/// One delivery's tick: the updated shot, or `None` when it is finished.
fn step(ctx: &mut Ctx, state: &mut State, mut shot: Shot) -> Option<Shot> {
    let caster = caster_of(ctx, state, &shot);
    let spell = ctx.ability(shot.ability);
    let d = spell.delivery.clone()?;
    let tick = state.tick;
    match &d {
        Delivery::Swarm { .. } => {
            return super::deliveries::swarm(ctx, state, shot, &caster, spell, &d);
        }
        Delivery::Charge { speed, max_ticks } => {
            let target = shot.target.and_then(|t| actor::get(&ctx.senses, state, t));
            let mut result = Step::Blocked;
            if let Some(t) = &target
                && caster.alive
                && !caster.blocked
                && caster.owner == shot.owner
                && t.targetable
                && tick - shot.started <= *max_ticks
                && super::visible(ctx, state, caster.owner, t.id, Some(&caster))
                && eligible(ctx, spell, &caster, t)
            {
                result = charge_step(ctx, state, caster.id, t.id, *speed);
            }
            let moved = actor::get(&ctx.senses, state, caster.id);
            if let Some(m) = &moved {
                shot.position = fine_of(ctx, m);
            }
            if let Some(t) = &target {
                shot.point = fine_of(ctx, t);
            }
            let mover = moved.unwrap_or(caster);
            match result {
                Step::Arrived => {
                    let t = target.expect("target");
                    let extra = Extra {
                        origin: Some(event_point(&shot.origin)),
                        ..Extra::default()
                    };
                    super::emit(
                        ctx,
                        state,
                        &mover,
                        &Mark::of(&t),
                        shot.cast,
                        shot.ability,
                        EventKind::Impact,
                        extra,
                        None,
                    );
                    hit_unit(ctx, state, &mover, t, &shot, 1000, None, None);
                    return None;
                }
                Step::Moving => return Some(shot),
                Step::Blocked => {
                    let mark = target.as_ref().map_or_else(|| Mark::of(&mover), Mark::of);
                    super::emit(
                        ctx,
                        state,
                        &mover,
                        &mark,
                        shot.cast,
                        shot.ability,
                        EventKind::Cancelled,
                        Extra::reason("message.charge-interrupted-or-obstructed"),
                        None,
                    );
                    return None;
                }
            }
        }
        _ => {}
    }
    let gone = state
        .entities
        .get(shot.source)
        .is_some_and(|e| e.owner != shot.owner)
        || caster.owner != shot.owner
        || tick - shot.started > 4000;
    if gone {
        return None;
    }
    if tick < shot.next_tick {
        return Some(shot);
    }
    match &d {
        Delivery::Chain {
            skip_full_health,
            bounces,
            radius,
            interval_ticks,
            retention_permille,
        } => {
            let target = shot.target.and_then(|t| actor::get(&ctx.senses, state, t));
            if let Some(t) = target
                && eligible(ctx, spell, &caster, &t)
            {
                let extra = Extra {
                    origin: Some(event_point(&shot.position)),
                    ..Extra::default()
                };
                super::emit(
                    ctx,
                    state,
                    &caster,
                    &Mark::of(&t),
                    shot.cast,
                    shot.ability,
                    EventKind::Impact,
                    extra,
                    None,
                );
                let position = fine_of(ctx, &t);
                let id = t.id;
                hit_unit(ctx, state, &caster, t, &shot, shot.power, None, None);
                shot.hit.push(id);
                shot.position = position;
            }
            if let Some(t) = shot.target
                && !shot.hit.contains(&t)
            {
                shot.hit.push(t);
            }
            let center = (shot.position.xf(), shot.position.yf());
            let mut next: Vec<Actor> = candidates(ctx, state, &caster, spell, center, *radius)
                .into_iter()
                .filter(|t| {
                    !shot.hit.contains(&t.id)
                        && (!skip_full_health || t.hp < t.max_hp)
                        && super::visible(ctx, state, caster.owner, t.id, Some(&caster))
                })
                .collect();
            let dist = |t: &Actor| {
                let (dx, dy) = (t.x() - center.0, t.y() - center.1);
                dx * dx + dy * dy
            };
            next.sort_by(|a, b| dist(a).total_cmp(&dist(b)).then(a.id.cmp(&b.id)));
            let bounces = library::value(spell, shot.rank, bounces).max(0) as usize;
            match next.first() {
                Some(n) if shot.hit.len() < bounces => {
                    shot.target = Some(n.id);
                    shot.power = shot.power * retention_permille / 1000;
                    shot.next_tick = tick + interval_ticks;
                    Some(shot)
                }
                _ => None,
            }
        }
        Delivery::Projectile { speed } | Delivery::Line { speed, .. } => {
            let projectile = matches!(d, Delivery::Projectile { .. });
            let target = if projectile {
                shot.target.and_then(|t| actor::get(&ctx.senses, state, t))
            } else {
                None
            };
            if projectile
                && target
                    .as_ref()
                    .is_none_or(|t| !eligible(ctx, spell, &caster, t))
            {
                return None;
            }
            if let Some(t) = &target {
                shot.point = fine_of(ctx, t);
            }
            let before = shot.position;
            let (dx, dy) = (shot.point.xf() - before.xf(), shot.point.yf() - before.yf());
            let distance = crate::math::hypot(dx, dy);
            let step = speed / 40.0;
            let t = (step / if distance == 0.0 { 1.0 } else { distance }).min(1.0);
            shot.position = Fine::snapped(
                before.xf() + dx * t,
                before.yf() + dy * t,
                before.hf() + (shot.point.hf() - before.hf()) * t,
            );
            if projectile {
                if distance <= step {
                    let t = target.expect("target");
                    let extra = Extra {
                        origin: Some(event_point(&shot.origin)),
                        ..Extra::default()
                    };
                    super::emit(
                        ctx,
                        state,
                        &caster,
                        &Mark::of(&t),
                        shot.cast,
                        shot.ability,
                        EventKind::Impact,
                        extra,
                        None,
                    );
                    hit_unit(ctx, state, &caster, t, &shot, 1000, None, None);
                    return None;
                }
                return Some(shot);
            }
            let Delivery::Line { width, .. } = &d else {
                unreachable!()
            };
            let (vx, vy) = (
                shot.position.xf() - before.xf(),
                shot.position.yf() - before.yf(),
            );
            let l2 = vx * vx + vy * vy;
            let (bx, by) = (before.xf(), before.yf());
            for victim in candidates(ctx, state, &caster, spell, (bx, by), step + width) {
                if shot.hit.contains(&victim.id) {
                    continue;
                }
                let u = (((victim.x() - bx) * vx + (victim.y() - by) * vy)
                    / if l2 == 0.0 { 1.0 } else { l2 })
                .clamp(0.0, 1.0);
                let (ex, ey) = (victim.x() - bx - vx * u, victim.y() - by - vy * u);
                if ex * ex + ey * ey > width * width {
                    continue;
                }
                let id = victim.id;
                hit_unit(ctx, state, &caster, victim, &shot, 1000, None, None);
                shot.hit.push(id);
                if shot.hit.len() >= 128 {
                    break;
                }
            }
            (distance > step && shot.hit.len() < 128).then_some(shot)
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Moving,
    Arrived,
    Blocked,
}

/// Moves a charging caster one tick toward its target (old `chargeStep`).
fn charge_step(
    ctx: &mut Ctx,
    state: &mut State,
    source: EntityId,
    target: EntityId,
    speed: f64,
) -> Step {
    let catalog = ctx.catalog();
    let (Some(a), Some(b)) = (state.entities.get(source), state.entities.get(target)) else {
        return Step::Blocked;
    };
    let held = |e: &crate::entity::Entity| {
        e.unit
            .as_ref()
            .is_none_or(|u| u.contained.is_some() || u.garrison.is_some() || u.release.is_some())
    };
    if held(a)
        || held(b)
        || !actor::alive(a)
        || !actor::alive(b)
        || super::forms::airborne(catalog, a)
        || super::forms::airborne(catalog, b)
        || super::statuses::rooted(catalog, a)
        || super::statuses::stunned(catalog, a)
        || super::statuses::silenced(catalog, a)
    {
        return Step::Blocked;
    }
    const CONTACT: f64 = 0.65 * 0.65;
    if ctx.spatial.body_range(a, b) <= CONTACT {
        return Step::Arrived;
    }
    let from = (
        f64::from(a.position.x) / 1000.0,
        f64::from(a.position.y) / 1000.0,
    );
    let to = (
        f64::from(b.position.x) / 1000.0,
        f64::from(b.position.y) / 1000.0,
    );
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let n = crate::math::hypot(dx, dy);
    let step = (speed / 40.0).min((ctx.spatial.body_range(a, b).sqrt() - 0.3).max(0.0));
    let next = (
        ((from.0 + dx / n * step) * 1000.0 + 0.5).floor() as i32,
        ((from.1 + dy / n * step) * 1000.0 + 0.5).floor() as i32,
    );
    let deck = ctx.spatial.deck_of(a);
    let (f, t) = (
        Fixed::on(a.position.x, a.position.y, deck),
        Fixed::on(next.0, next.1, deck),
    );
    let body = ctx.spatial.body_of(a);
    if step == 0.0
        || !ctx.spatial.clear_segment(f, t, None, &body)
        || !ctx
            .spatial
            .unit_segment_clear(f, t, Some(source), None, None, None)
    {
        return Step::Blocked;
    }
    let current = f64::from(a.rotation) / 1000.0;
    let heading = crate::movement::facing::heading(from, to, current);
    let tick = state.tick;
    let a = state.entities.get_mut(source).expect("charger");
    a.rotation = crate::terrain::round_half_up(heading * 1000.0) as i32;
    a.position = Milli {
        x: next.0,
        y: next.1,
    };
    a.unit.as_mut().expect("unit").last_moved_tick = Some(tick);
    ctx.spatial.update_body(a);
    let (a, b) = (
        state.entities.get(source).expect("charger"),
        state.entities.get(target).expect("target"),
    );
    if ctx.spatial.body_range(a, b) <= CONTACT {
        Step::Arrived
    } else {
        Step::Moving
    }
}

/// Whether a swarm may bite `t` (old `swarmEligible`).
pub fn swarm_eligible(ctx: &Ctx, t: &Actor, caster: &Actor, spell: &Ability, d: &Delivery) -> bool {
    let Delivery::Swarm {
        radius,
        filter,
        include_buildings,
        ..
    } = d
    else {
        return false;
    };
    let side = actor::relation(&ctx.senses, caster, t);
    let (dx, dy) = (t.x() - caster.x(), t.y() - caster.y());
    t.id != caster.id
        && t.alive
        && t.targetable
        && (t.unit || *include_buildings)
        && side == Side::Enemy
        && dx * dx + dy * dy <= radius * radius
        && actor::matches(t, filter.as_ref())
        && cast::accepts(spell, t, side)
}

/// One swarm seeker's tick (old `stepSwarm`).
fn swarm(
    ctx: &mut Ctx,
    state: &mut State,
    mut shot: Shot,
    caster: &Actor,
    spell: &Ability,
    d: &Delivery,
) -> Option<Shot> {
    let Delivery::Swarm {
        speed,
        attack_interval_ticks,
        max_per_target,
        return_threshold,
        return_permille,
        max_hits_per_trip,
        return_timeout_ticks,
        ..
    } = d
    else {
        return None;
    };
    let now = state.tick;
    let mut s = shot.swarm.clone().expect("seeker");
    let camp = shot.source_context.as_ref().and_then(|c| c.camp.clone());
    if !caster.alive
        || !caster.targetable
        || caster.owner != shot.owner
        || caster.camp != camp
        || now >= s.expires + return_timeout_ticks
    {
        return None;
    }
    if s.phase == SeekerPhase::Waiting {
        shot.position = fine_of(ctx, caster);
        shot.point = shot.position;
        if now < s.launch_tick {
            shot.swarm = Some(s);
            return Some(shot);
        }
        s.phase = SeekerPhase::Seeking;
        let extra = Extra {
            duration_ticks: Some((s.expires + return_timeout_ticks).saturating_sub(now)),
            ..Extra::default()
        };
        super::emit(
            ctx,
            state,
            caster,
            &Mark::of(caster),
            shot.cast,
            shot.ability,
            EventKind::Projectile,
            extra,
            None,
        );
    }
    if now >= s.expires {
        s.phase = SeekerPhase::Returning;
        shot.target = None;
    }
    let can_bite = |ctx: &Ctx, state: &State, t: &Actor| {
        swarm_eligible(ctx, t, caster, spell, d)
            && super::visible(ctx, state, caster.owner, t.id, Some(caster))
    };
    if s.phase == SeekerPhase::Seeking {
        if shot.target.is_none() && now < shot.next_tick {
            shot.swarm = Some(s);
            return Some(shot);
        }
        let current = shot.target.and_then(|t| actor::get(&ctx.senses, state, t));
        if !current.as_ref().is_some_and(|t| can_bite(ctx, state, t)) {
            shot.target = None;
            let mut assigned: Vec<(EntityId, u32)> = Vec::new();
            for other in &state.abilities.deliveries {
                if other.cast == shot.cast {
                    continue;
                }
                if let (Some(o), Some(t)) = (&other.swarm, other.target)
                    && o.group == s.group
                    && o.phase == SeekerPhase::Seeking
                {
                    match assigned.iter_mut().find(|(id, _)| *id == t) {
                        Some((_, n)) => *n += 1,
                        None => assigned.push((t, 1)),
                    }
                }
            }
            let count = |id: EntityId| {
                assigned
                    .iter()
                    .find(|(t, _)| *t == id)
                    .map_or(0, |(_, n)| *n)
            };
            let (px, py) = (shot.position.xf(), shot.position.yf());
            let mut options: Vec<Actor> = state
                .entities
                .iter()
                .filter(|e| e.hp.is_some() && actor::alive(e))
                .map(|e| actor::of(&ctx.senses, state, e))
                .filter(|t| can_bite(ctx, state, t) && count(t.id) < *max_per_target)
                .collect();
            let dist = |t: &Actor| {
                let (dx, dy) = (t.x() - px, t.y() - py);
                dx * dx + dy * dy
            };
            options.sort_by(|a, b| {
                count(a.id)
                    .cmp(&count(b.id))
                    .then(dist(a).total_cmp(&dist(b)))
                    .then(a.id.cmp(&b.id))
            });
            match options.first() {
                Some(t) => shot.target = Some(t.id),
                None => s.phase = SeekerPhase::Returning,
            }
        }
    }
    let target = if s.phase == SeekerPhase::Returning {
        caster.clone()
    } else {
        actor::get(&ctx.senses, state, shot.target.expect("target")).expect("target")
    };
    shot.point = fine_of(ctx, &target);
    let before = shot.position;
    let (dx, dy, dh) = (
        shot.point.xf() - before.xf(),
        shot.point.yf() - before.yf(),
        shot.point.hf() - before.hf(),
    );
    let length = (dx * dx + dy * dy + dh * dh).sqrt();
    let step = speed / 40.0;
    let t = (step / if length == 0.0 { 1.0 } else { length }).min(1.0);
    shot.position = Fine::snapped(
        before.xf() + dx * t,
        before.yf() + dy * t,
        before.hf() + dh * t,
    );
    if length > step {
        shot.swarm = Some(s);
        return Some(shot);
    }
    if s.phase == SeekerPhase::Returning {
        if s.hits > 0 || s.health > 0 || s.mana > 0 {
            returned(ctx, state, caster, &shot, s.health, s.mana);
        }
        s.health = 0;
        s.mana = 0;
        s.hits = 0;
        shot.target = None;
        if now >= s.expires {
            return None;
        }
        s.phase = SeekerPhase::Seeking;
        shot.next_tick = shot.next_tick.max(now + attack_interval_ticks);
        shot.swarm = Some(s);
        return Some(shot);
    }
    if now < shot.next_tick {
        shot.swarm = Some(s);
        return Some(shot);
    }
    let mut cargo = Cargo::default();
    let extra = Extra {
        origin: Some(event_point(&shot.position)),
        ..Extra::default()
    };
    super::emit(
        ctx,
        state,
        caster,
        &Mark::of(&target),
        shot.cast,
        shot.ability,
        EventKind::Impact,
        extra,
        None,
    );
    let visual = Some(event_point(&shot.position));
    hit_unit(
        ctx,
        state,
        caster,
        target,
        &shot,
        shot.power,
        visual,
        Some(&mut cargo),
    );
    s.health = (s.health + cargo.health * return_permille / 1000).min(1_000_000);
    s.mana = (s.mana + cargo.mana * return_permille / 1000).min(1_000_000);
    s.hits += 1;
    shot.next_tick = now + attack_interval_ticks;
    let threshold = library::value(spell, shot.rank, return_threshold).max(0) as u32;
    if s.hits >= *max_hits_per_trip || s.health + s.mana >= threshold {
        s.phase = SeekerPhase::Returning;
        shot.target = None;
    }
    shot.swarm = Some(s);
    Some(shot)
}

/// A seeker brings its cargo home (old `returned` callback).
fn returned(ctx: &mut Ctx, state: &mut State, caster: &Actor, shot: &Shot, health: u32, mana: u32) {
    for (resource, n) in [(true, health), (false, mana)] {
        if n == 0 {
            continue;
        }
        let amount = if resource {
            super::status_ops::heal(ctx, state, caster.id, n)
        } else {
            super::status_ops::restore_mana(ctx, state, caster.id, n)
        };
        let kind = if resource {
            EventKind::Healed
        } else {
            EventKind::ManaRestored
        };
        super::emit(
            ctx,
            state,
            caster,
            &Mark::of(caster),
            shot.cast,
            shot.ability,
            kind,
            Extra::amount(amount),
            None,
        );
    }
    super::emit(
        ctx,
        state,
        caster,
        &Mark::of(caster),
        shot.cast,
        shot.ability,
        EventKind::Returned,
        Extra::default(),
        None,
    );
}
