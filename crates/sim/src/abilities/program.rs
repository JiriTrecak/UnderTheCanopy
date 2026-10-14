//! The operation interpreter (old `AbilityRuntime.hit`, `recipients.ts`, `magnitudes.ts`):
//! one program run against one aim, operation by operation, each choosing its recipients,
//! deciding branches per recipient, scaling amounts and recording results.

use super::{
    Ctx, Extra, Mark,
    actor::{self, Actor, Side},
    cast,
    events::{EventKind, EventPoint},
    library::{self, AbilityId},
    status_ops, summons, world_ops,
};
use crate::{
    entity::{EntityId, Milli},
    state::State,
};
use canopy_content::{
    abilities::{
        Ability, AmountScale, Anchor, Effect, Operation, RecipientOrder, RecipientQuery,
        ResultStat, StatusLifetime, UnitStat,
    },
    placement::Owner,
};
use std::borrow::Cow;

/// What a program is aimed at (old `EventTarget`): a unit snapshot or a point.
#[derive(Debug, Clone)]
pub enum Aim {
    Unit(Box<Actor>),
    Point(Milli),
}

impl Aim {
    pub fn id(&self) -> Option<EntityId> {
        match self {
            Aim::Unit(a) => Some(a.id),
            Aim::Point(_) => None,
        }
    }

    pub fn position(&self) -> Milli {
        match self {
            Aim::Unit(a) => a.position,
            Aim::Point(p) => *p,
        }
    }

    pub fn mark(&self) -> Mark {
        match self {
            Aim::Unit(a) => Mark::of(a),
            Aim::Point(p) => Mark::point(*p),
        }
    }
}

/// Resources a swarm seeker carries home.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cargo {
    pub health: u32,
    pub mana: u32,
}

/// A program run's parameters (old `hit` arguments).
pub struct Run<'a> {
    pub caster: Actor,
    pub aim: Aim,
    pub cast: u32,
    pub ability: AbilityId,
    pub rank: u8,
    /// Chain retention, permille.
    pub power: u32,
    pub aura: bool,
    /// A release, wave or pulse at a point aim selects the default area.
    pub area_defaults: bool,
    pub operations: Ops<'a>,
    pub origin_viewers: Option<Vec<Owner>>,
    pub visual_origin: Option<EventPoint>,
    /// Statuses granted to a unit the spell just created bypass spell immunity.
    pub creation_grant: bool,
    pub carry: Option<&'a mut Cargo>,
}

/// The operations of a run.
pub enum Ops<'a> {
    Program(&'a [Operation]),
    List(Vec<Cow<'a, Operation>>),
}

impl<'a> Ops<'a> {
    fn list(self) -> Vec<Cow<'a, Operation>> {
        match self {
            Ops::Program(ops) => ops.iter().map(Cow::Borrowed).collect(),
            Ops::List(list) => list,
        }
    }
}

/// A recorded result (old `EffectResult`).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Record {
    applied: i64,
    count: i64,
    health: i64,
    max_health: i64,
    mana: i64,
    max_mana: i64,
}

const MILLION: i64 = 1_000_000;

fn bounded(n: i64) -> i64 {
    n.clamp(0, MILLION)
}

struct Pools {
    health: i64,
    max_health: i64,
    mana: i64,
    max_mana: i64,
}

fn pools(a: Option<&Actor>) -> Pools {
    match a {
        Some(a) => Pools {
            health: bounded(i64::from(a.hp)),
            max_health: bounded(i64::from(a.max_hp)),
            mana: bounded(i64::from(a.mana)),
            max_mana: bounded(i64::from(a.max_mana)),
        },
        None => Pools {
            health: 0,
            max_health: 0,
            mana: 0,
            max_mana: 0,
        },
    }
}

fn unit_stat(p: &Pools, stat: UnitStat) -> i64 {
    match stat {
        UnitStat::Health => p.health,
        UnitStat::MaxHealth => p.max_health,
        UnitStat::MissingHealth => (p.max_health - p.health).max(0),
        UnitStat::Mana => p.mana,
        UnitStat::MaxMana => p.max_mana,
        UnitStat::MissingMana => (p.max_mana - p.mana).max(0),
    }
}

/// An operation's amount with its scale (old `effectMagnitude`).
pub(crate) fn magnitude(
    spell: &Ability,
    rank: u8,
    effect: &Effect,
    caster: Option<&Actor>,
    target: Option<&Actor>,
    recipient: Option<&Actor>,
    results: &[(String, Record)],
) -> i64 {
    let base = library::value(spell, rank, &effect.amount());
    let Some(scale) = effect.scale() else {
        return bounded(base);
    };
    let stat = match scale {
        AmountScale::Result { id, stat, .. } => {
            let r = results
                .iter()
                .find(|(n, _)| n == id)
                .map(|(_, r)| *r)
                .unwrap_or_default();
            match stat {
                ResultStat::Applied => r.applied,
                ResultStat::Count => r.count,
                ResultStat::Health => r.health,
                ResultStat::MaxHealth => r.max_health,
                ResultStat::Mana => r.mana,
                ResultStat::MaxMana => r.max_mana,
            }
        }
        AmountScale::Caster { stat, .. } => unit_stat(&pools(caster), *stat),
        AmountScale::Target { stat, .. } => unit_stat(&pools(target), *stat),
        AmountScale::Recipient { stat, .. } => unit_stat(&pools(recipient), *stat),
    };
    let permille = library::value(spell, rank, scale.permille());
    bounded(base + (stat as f64 * permille as f64 / 1000.0).floor() as i64)
}

/// Adds a successful recipient to the effect's record (old `recordEffect`).
pub(crate) fn record(
    results: &mut Vec<(String, Record)>,
    effect: &Effect,
    before: Option<&Actor>,
    applied: i64,
) {
    let Some(name) = effect.record() else {
        return;
    };
    if applied <= 0 {
        return;
    }
    let p = pools(before);
    let i = match results.iter().position(|(n, _)| n == name) {
        Some(i) => i,
        None => {
            results.push((name.to_string(), Record::default()));
            results.len() - 1
        }
    };
    let r = &mut results[i].1;
    r.applied = bounded(r.applied + applied);
    r.count += 1;
    r.health = bounded(r.health + p.health);
    r.max_health = bounded(r.max_health + p.max_health);
    r.mana = bounded(r.mana + p.mana);
    r.max_mana = bounded(r.max_mana + p.max_mana);
}

/// A recipient: a unit snapshot, or a point.
#[derive(Debug, Clone)]
pub enum Recipient {
    Unit(Actor),
    Point(Milli),
}

impl Recipient {
    fn id(&self) -> Option<EntityId> {
        match self {
            Recipient::Unit(a) => Some(a.id),
            Recipient::Point(_) => None,
        }
    }

    fn position(&self) -> Milli {
        match self {
            Recipient::Unit(a) => a.position,
            Recipient::Point(p) => *p,
        }
    }

    fn mark(&self) -> Mark {
        match self {
            Recipient::Unit(a) => Mark::of(a),
            Recipient::Point(p) => Mark::point(*p),
        }
    }
}

/// Live bodies within `radius_milli` of `center`, as snapshots in id order (old
/// `targets()` mapped through `get`, narrowed by the distance every user checks).
pub fn bodies_near(ctx: &Ctx, state: &State, center: Milli, radius_milli: i64) -> Vec<Actor> {
    state
        .entities
        .iter()
        .filter(|e| e.hp.is_some() && actor::alive(e))
        .filter(|e| actor::within(e.position, center, radius_milli))
        .map(|e| actor::of(&ctx.senses, state, e))
        .collect()
}

/// The recipients of one operation (old `operationTargets`).
#[allow(clippy::too_many_arguments)]
pub fn recipients(
    ctx: &Ctx,
    state: &State,
    effect: &Effect,
    spell: &Ability,
    rank: u8,
    caster: &Actor,
    relation_caster: &Actor,
    aim: &Aim,
    defaults: &[Actor],
    area_defaults: bool,
    pool: Option<&[Actor]>,
) -> Vec<Recipient> {
    let side = |t: &Actor| actor::relation(&ctx.senses, relation_caster, t);
    if effect.target() == Anchor::Point {
        return vec![Recipient::Point(aim.position())];
    }
    let filter = effect.filter();
    if let Some(q) = effect.query() {
        let center = match q.center {
            Anchor::Caster => caster.position,
            Anchor::Target => match aim {
                Aim::Unit(a) => match pool {
                    Some(list) => list
                        .iter()
                        .find(|t| t.id == a.id)
                        .map_or(a.position, |t| t.position),
                    None => state.entities.get(a.id).map_or(a.position, |e| e.position),
                },
                Aim::Point(p) => *p,
            },
            Anchor::Point => aim.position(),
        };
        let radius = library::value(spell, rank, &q.radius).max(0) * 1000;
        let near;
        let candidates: &[Actor] = match pool {
            Some(list) => list,
            None => {
                near = bodies_near(ctx, state, center, radius);
                &near
            }
        };
        return query(
            ctx,
            candidates,
            q,
            spell,
            caster,
            aim.id(),
            center,
            radius,
            filter,
            &side,
        );
    }
    if effect.target() == Anchor::Caster {
        return if cast::accepts(spell, caster, Side::Ally) && actor::matches(caster, filter) {
            vec![Recipient::Unit(caster.clone())]
        } else {
            Vec::new()
        };
    }
    if area_defaults && let Some(radius) = &spell.targeting.radius {
        let radius = library::value(spell, rank, radius).max(0) * 1000;
        let center = aim.position();
        let max = spell.targeting.max_targets.unwrap_or(128) as usize;
        return defaults
            .iter()
            .filter(|t| {
                let s = side(t);
                t.alive
                    && t.targetable
                    && cast::accepts(spell, t, s)
                    && (t.unit || spell.targeting.include_buildings == Some(true))
                    && (spell.targeting.allow_self || t.id != caster.id)
                    && s.is(&spell.targeting.relations)
                    && cast::matches_target(spell, t, caster, s)
                    && actor::matches(*t, filter)
                    && actor::within(t.position, center, radius)
            })
            .take(max)
            .cloned()
            .map(Recipient::Unit)
            .collect();
    }
    match aim {
        Aim::Unit(a) => {
            if actor::matches(a.as_ref(), filter) {
                vec![Recipient::Unit((**a).clone())]
            } else {
                Vec::new()
            }
        }
        Aim::Point(_) => Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn query(
    _ctx: &Ctx,
    candidates: &[Actor],
    q: &RecipientQuery,
    spell: &Ability,
    caster: &Actor,
    primary: Option<EntityId>,
    center: Milli,
    radius: i64,
    filter: Option<&canopy_content::conditions::TargetFilter>,
    side: &dyn Fn(&Actor) -> Side,
) -> Vec<Recipient> {
    let mut chosen: Vec<&Actor> = candidates
        .iter()
        .filter(|t| {
            let s = side(t);
            t.alive
                && t.targetable
                && cast::accepts(spell, t, s)
                && (t.unit || q.include_buildings)
                && (q.allow_self || t.id != caster.id)
                && (!q.exclude_primary || Some(t.id) != primary)
                && s.is(&q.relations)
                && actor::matches(*t, filter)
                && actor::within(t.position, center, radius)
        })
        .collect();
    chosen.sort_by(|a, b| {
        let health = if q.order == RecipientOrder::LowestHealth {
            (i64::from(a.hp) * i64::from(b.max_hp)).cmp(&(i64::from(b.hp) * i64::from(a.max_hp)))
        } else {
            std::cmp::Ordering::Equal
        };
        health
            .then(a.distance2(center).cmp(&b.distance2(center)))
            .then(a.id.cmp(&b.id))
    });
    chosen
        .into_iter()
        .take(q.max_targets as usize)
        .cloned()
        .map(Recipient::Unit)
        .collect()
}

/// The event kind of an operation's recipients.
fn event_kind(effect: &Effect) -> EventKind {
    match effect {
        Effect::Heal(_) => EventKind::Healed,
        Effect::Damage(_) => EventKind::Damaged,
        Effect::Status(_) => EventKind::StatusApplied,
        Effect::Dispel(_) => EventKind::Dispelled,
        Effect::Drain(_) => EventKind::Drained,
        Effect::Mana(_) => EventKind::ManaRestored,
        Effect::Contain(_) => EventKind::Contained,
        Effect::Sacrifice(_) => EventKind::Sacrificed,
        Effect::Convert(_) => EventKind::Converted,
        Effect::Teleport(_) => EventKind::Teleported,
        Effect::Vision(_) => EventKind::VisionCreated,
        _ => EventKind::Summoned,
    }
}

/// The live caster when it still has its original owner, else the snapshot.
fn current(ctx: &Ctx, state: &State, caster: &Actor) -> Actor {
    match actor::get(&ctx.senses, state, caster.id) {
        Some(live) if live.owner == caster.owner => live,
        _ => caster.clone(),
    }
}

/// Runs a program (old `hit`).
pub fn run(ctx: &mut Ctx, state: &mut State, mut run: Run) {
    let spell = ctx.ability(run.ability);
    let rank = run.rank;
    if library::parameters(spell, rank).is_none() {
        return;
    }
    let caster = run.caster.clone();
    let mut results: Vec<(String, Record)> = Vec::new();
    let resource_caster = current(ctx, state, &caster);
    let resource_target = run
        .aim
        .id()
        .and_then(|id| actor::get(&ctx.senses, state, id));
    // The default area is captured once, so a heal does not drop a recipient from the
    // following ward.
    let defaults: Vec<Actor> = match (&spell.targeting.radius, run.area_defaults) {
        (Some(radius), true) => {
            let radius = library::value(spell, rank, radius).max(0) * 1000;
            bodies_near(ctx, state, run.aim.position(), radius)
        }
        _ => Vec::new(),
    };
    let operations = std::mem::replace(&mut run.operations, Ops::List(Vec::new())).list();
    for operation in &operations {
        let (declarations, branch): (
            Vec<(&Effect, bool)>,
            Option<&canopy_content::abilities::Branch>,
        ) = match operation.as_ref() {
            Operation::Effect(e) => (vec![(e, true)], None),
            Operation::Branch(b) => (
                b.then
                    .iter()
                    .map(|e| (e, true))
                    .chain(b.otherwise.iter().map(|e| (e, false)))
                    .collect(),
                Some(b),
            ),
        };
        let branch_caster = branch.map(|_| current(ctx, state, &caster));
        let mut decisions: Vec<(Option<EntityId>, bool)> = Vec::new();
        for (effect, arm) in declarations {
            let operation_caster = current(ctx, state, &caster);
            let operation_aim = match (effect, &run.aim) {
                (Effect::Teleport(t), Aim::Unit(a)) if t.destination == Anchor::Target => {
                    match actor::get(&ctx.senses, state, a.id) {
                        Some(live) => Aim::Unit(Box::new(live)),
                        None => run.aim.clone(),
                    }
                }
                _ => run.aim.clone(),
            };
            let selector = if effect.query().is_some() || effect.target() == Anchor::Caster {
                &operation_caster
            } else {
                &caster
            };
            let targets = recipients(
                ctx,
                state,
                effect,
                spell,
                rank,
                selector,
                &caster,
                &run.aim,
                &defaults,
                run.area_defaults,
                None,
            );
            for target in targets {
                let live = target
                    .id()
                    .and_then(|id| actor::get(&ctx.senses, state, id));
                if let Some(b) = branch {
                    let chosen = match decisions.iter().find(|(id, _)| *id == target.id()) {
                        Some((_, d)) => *d,
                        None => {
                            let side = actor::relation(
                                &ctx.senses,
                                &caster,
                                live.as_ref().unwrap_or(&caster),
                            );
                            let d = match &live {
                                Some(t) => actor::condition(
                                    &b.condition,
                                    side,
                                    branch_caster.as_ref().expect("branch"),
                                    t,
                                ),
                                None => actor::condition(
                                    &b.condition,
                                    side,
                                    branch_caster.as_ref().expect("branch"),
                                    &actor::Nothing,
                                ),
                            };
                            decisions.push((target.id(), d));
                            d
                        }
                    };
                    if chosen != arm {
                        continue;
                    }
                }
                if let Effect::Revive(op) = effect {
                    let duration = world_ops::schedule_revive(
                        ctx,
                        state,
                        &caster,
                        run.ability,
                        rank,
                        run.cast,
                        op,
                    );
                    if duration > 0 {
                        let extra = Extra {
                            duration_ticks: Some(duration),
                            ..Extra::default()
                        };
                        super::emit(
                            ctx,
                            state,
                            &operation_caster,
                            &target.mark(),
                            run.cast,
                            run.ability,
                            EventKind::RevivalStarted,
                            extra,
                            run.origin_viewers.clone(),
                        );
                    }
                    continue;
                }
                if target.id().is_some() {
                    let Some(a) = &live else {
                        continue;
                    };
                    let side = actor::relation(&ctx.senses, &caster, a);
                    if !a.alive
                        || !a.targetable
                        || !actor::matches(a, effect.filter())
                        || (!run.creation_grant && !cast::accepts(spell, a, side))
                    {
                        continue;
                    }
                }
                if effect.target() == Anchor::Caster
                    && effect.query().is_none()
                    && live.as_ref().map(|a| a.owner) != Some(caster.owner)
                {
                    continue;
                }
                let amount = magnitude(
                    spell,
                    rank,
                    effect,
                    Some(&resource_caster),
                    resource_target.as_ref(),
                    live.as_ref(),
                    &results,
                );
                let creates = matches!(effect, Effect::Summon(_) | Effect::Resurrect(_));
                let audience: Vec<Owner> = if run.aura && !creates {
                    Vec::new()
                } else {
                    audience(
                        ctx,
                        state,
                        effect,
                        &caster,
                        live.as_ref(),
                        &target,
                        run.origin_viewers.as_deref(),
                    )
                };
                if let Effect::Summon(op) = effect {
                    let groups = summons::groups(
                        ctx,
                        state,
                        &caster,
                        run.ability,
                        rank,
                        run.cast,
                        op,
                        target.position(),
                    );
                    for group in groups {
                        let grants = op.grants.clone().unwrap_or_default();
                        let duration = op
                            .duration_ticks
                            .as_ref()
                            .map(|d| library::value(spell, rank, d).max(0) as u32);
                        let audience: Vec<Owner> = audience
                            .iter()
                            .copied()
                            .filter(|&o| {
                                let live_caster = actor::get(&ctx.senses, state, caster.id);
                                super::visible_point(
                                    ctx,
                                    state,
                                    o,
                                    group.point,
                                    live_caster.as_ref(),
                                )
                            })
                            .collect();
                        let mark = Mark {
                            id: if op.corpses.is_some() {
                                None
                            } else {
                                target.id()
                            },
                            position: group.point,
                            surface: None,
                            height: None,
                        };
                        let extra = Extra {
                            amount: Some(group.units.len() as i64),
                            spawned: group.units.clone(),
                            until_death: duration.is_none(),
                            duration_ticks: duration,
                            origin: run.visual_origin,
                            ..Extra::default()
                        };
                        super::emit(
                            ctx,
                            state,
                            &operation_caster,
                            &mark,
                            run.cast,
                            run.ability,
                            EventKind::Summoned,
                            extra,
                            Some(audience.clone()),
                        );
                        for unit in group.units {
                            grant(
                                ctx,
                                state,
                                &caster,
                                unit,
                                run.cast,
                                run.ability,
                                rank,
                                &grants,
                                &audience,
                                run.visual_origin,
                            );
                        }
                    }
                    continue;
                }
                if let Effect::Resurrect(op) = effect {
                    let raised = super::corpses::resurrect(
                        ctx,
                        state,
                        &caster,
                        run.ability,
                        rank,
                        run.cast,
                        op,
                        target.position(),
                    );
                    let duration = op
                        .duration_ticks
                        .as_ref()
                        .map(|d| library::value(spell, rank, d).max(0) as u32);
                    let grants = op.grants.clone().unwrap_or_default();
                    for unit in raised {
                        let Some(raised_actor) = actor::get(&ctx.senses, state, unit) else {
                            continue;
                        };
                        let live_caster = actor::get(&ctx.senses, state, caster.id);
                        let audience: Vec<Owner> = audience
                            .iter()
                            .copied()
                            .filter(|&o| {
                                super::visible_point(
                                    ctx,
                                    state,
                                    o,
                                    raised_actor.position,
                                    live_caster.as_ref(),
                                )
                            })
                            .collect();
                        let extra = Extra {
                            amount: Some(1),
                            duration_ticks: duration,
                            origin: run.visual_origin,
                            ..Extra::default()
                        };
                        super::emit(
                            ctx,
                            state,
                            &operation_caster,
                            &Mark::of(&raised_actor),
                            run.cast,
                            run.ability,
                            EventKind::Resurrected,
                            extra,
                            Some(audience.clone()),
                        );
                        grant(
                            ctx,
                            state,
                            &caster,
                            unit,
                            run.cast,
                            run.ability,
                            rank,
                            &grants,
                            &audience,
                            run.visual_origin,
                        );
                    }
                    continue;
                }
                let n = (amount * i64::from(run.power) / 1000).max(0);
                let target_id = target.id();
                let applied: i64 = match effect {
                    Effect::Heal(_) => {
                        target_id.map_or(0, |t| status_ops::heal(ctx, state, t, n as u32))
                    }
                    Effect::Damage(d) => target_id.map_or(0, |t| {
                        let kind = ctx.catalog().spells.damage_type(&d.damage_type);
                        i64::from(status_ops::spell_damage(
                            ctx,
                            state,
                            caster.id,
                            caster.owner,
                            t,
                            n as u32,
                            kind,
                        ))
                    }),
                    _ => {
                        let restore = match (effect, &run.carry) {
                            (Effect::Drain(d), Some(_)) => d.restore_caster && false,
                            (Effect::Drain(d), None) => d.restore_caster,
                            _ => false,
                        };
                        status_ops::apply(
                            ctx,
                            state,
                            status_ops::Apply {
                                source: caster.id,
                                owner: caster.owner,
                                source_camp: caster.camp.clone(),
                                target: &target,
                                ability: run.ability,
                                rank,
                                cast: run.cast,
                                effect,
                                amount: if matches!(effect, Effect::Drain(_) | Effect::Mana(_)) {
                                    n
                                } else {
                                    amount
                                },
                                restore_caster: restore,
                                aura: run.aura,
                                aim: &operation_aim,
                                origin: &operation_caster,
                                creation_grant: run.creation_grant,
                            },
                        )
                    }
                };
                if let (Effect::Drain(d), Some(carry)) = (effect, run.carry.as_deref_mut())
                    && d.restore_caster
                {
                    match d.resource {
                        canopy_content::abilities::DrainResource::Health => {
                            carry.health += applied.max(0) as u32
                        }
                        canopy_content::abilities::DrainResource::Mana => {
                            carry.mana += applied.max(0) as u32
                        }
                    }
                }
                record(&mut results, effect, live.as_ref(), applied);
                if matches!(effect, Effect::ReleaseContained(_) | Effect::Split(_)) {
                    continue;
                }
                let zero_skips = matches!(
                    effect,
                    Effect::Status(_)
                        | Effect::Convert(_)
                        | Effect::Contain(_)
                        | Effect::Sacrifice(_)
                );
                if run.aura || (zero_skips && applied == 0) {
                    continue;
                }
                let moved = match effect {
                    Effect::Teleport(_) if applied != 0 => {
                        target_id.and_then(|t| actor::get(&ctx.senses, state, t))
                    }
                    _ => None,
                };
                let mut extra = Extra {
                    amount: Some(if matches!(effect, Effect::Contain(_)) {
                        1
                    } else {
                        applied
                    }),
                    origin: run.visual_origin,
                    ..Extra::default()
                };
                if let (Some(_), Some(before)) = (&moved, &live) {
                    extra.origin = Some(EventPoint {
                        x: before.position.x,
                        y: before.position.y,
                        height: super::actor_height(ctx, before),
                    });
                }
                match effect {
                    Effect::Status(s) => {
                        extra.duration_ticks = Some(applied.clamp(0, i64::from(u32::MAX)) as u32);
                        extra.status = Some(s.id.clone());
                        extra.until_death = s.lifetime == StatusLifetime::UntilDeath;
                    }
                    Effect::Contain(c) => {
                        extra.duration_ticks = Some(applied.clamp(0, i64::from(u32::MAX)) as u32);
                        extra.until_death =
                            c.lifetime == canopy_content::abilities::ContainLifetime::UntilDeath;
                    }
                    Effect::Vision(v) => {
                        extra.duration_ticks = Some(applied.max(0) as u32);
                        extra.radius = Some(library::value(spell, rank, &v.radius) as f64);
                    }
                    _ => {}
                }
                let mark = match &moved {
                    Some(m) => Mark::of(m),
                    None => target.mark(),
                };
                super::emit(
                    ctx,
                    state,
                    &operation_caster,
                    &mark,
                    run.cast,
                    run.ability,
                    event_kind(effect),
                    extra,
                    Some(audience),
                );
            }
        }
    }
}

/// Who sees an operation's event (old audience of `hit`).
fn audience(
    ctx: &Ctx,
    state: &State,
    effect: &Effect,
    caster: &Actor,
    live: Option<&Actor>,
    target: &Recipient,
    origin_viewers: Option<&[Owner]>,
) -> Vec<Owner> {
    let live_caster = actor::get(&ctx.senses, state, caster.id);
    super::owners(ctx)
        .iter()
        .copied()
        .filter(|&owner| {
            (matches!(effect, Effect::Vision(_)) && owner == caster.owner)
                || (match origin_viewers {
                    Some(v) => v.contains(&owner),
                    None => super::visible(ctx, state, owner, caster.id, Some(caster)),
                } && match live {
                    Some(a) => super::visible(ctx, state, owner, a.id, Some(caster)),
                    None => super::visible_point(
                        ctx,
                        state,
                        owner,
                        target.position(),
                        live_caster.as_ref(),
                    ),
                })
        })
        .collect()
}

/// Applies a spell's status templates to a unit it created (old `creationGrants`).
#[allow(clippy::too_many_arguments)]
pub fn grant(
    ctx: &mut Ctx,
    state: &mut State,
    caster: &Actor,
    unit: EntityId,
    cast: u32,
    ability: AbilityId,
    rank: u8,
    grants: &[String],
    viewers: &[Owner],
    visual_origin: Option<EventPoint>,
) {
    let spell = ctx.ability(ability);
    for id in grants {
        let Some(template) = spell.statuses.iter().flatten().find(|s| &s.id == id) else {
            continue;
        };
        let Some(target) = actor::get(&ctx.senses, state, unit) else {
            return;
        };
        let op = Operation::Effect(Effect::Status(Box::new(template.clone())));
        run(
            ctx,
            state,
            Run {
                caster: caster.clone(),
                aim: Aim::Unit(Box::new(target)),
                cast,
                ability,
                rank,
                power: 1000,
                aura: false,
                area_defaults: false,
                operations: Ops::List(vec![Cow::Owned(op)]),
                origin_viewers: Some(viewers.to_vec()),
                visual_origin,
                creation_grant: true,
                carry: None,
            },
        );
    }
}

/// Runs one template status from a weapon enhancement against its victim (old
/// `weaponStatus`).
pub fn template(
    ctx: &mut Ctx,
    state: &mut State,
    caster: Actor,
    victim: EntityId,
    cast: u32,
    ability: AbilityId,
    rank: u8,
    status: &str,
) {
    let spell = ctx.ability(ability);
    let Some(mark) = spell.statuses.iter().flatten().find(|s| s.id == status) else {
        return;
    };
    if library::parameters(spell, rank).is_none() {
        return;
    }
    let Some(target) = actor::get(&ctx.senses, state, victim) else {
        return;
    };
    if !target.alive || actor::relation(&ctx.senses, &caster, &target) != Side::Enemy {
        return;
    }
    let audience: Vec<Owner> = super::owners(ctx)
        .iter()
        .copied()
        .filter(|&o| super::visible(ctx, state, o, victim, None))
        .collect();
    let origin = EventPoint {
        x: target.position.x,
        y: target.position.y,
        height: super::actor_height(ctx, &target),
    };
    let op = Operation::Effect(Effect::Status(Box::new(mark.clone())));
    run(
        ctx,
        state,
        Run {
            caster,
            aim: Aim::Unit(Box::new(target)),
            cast,
            ability,
            rank,
            power: 1000,
            aura: false,
            area_defaults: false,
            operations: Ops::List(vec![Cow::Owned(op)]),
            origin_viewers: Some(audience),
            visual_origin: Some(origin),
            creation_grant: false,
            carry: None,
        },
    );
}

/// The subject of a [`Subject`] recipient field.
pub fn subject_anchor(s: canopy_content::conditions::Subject) -> Anchor {
    s.into()
}
