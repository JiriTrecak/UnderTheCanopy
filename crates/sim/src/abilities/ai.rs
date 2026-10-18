//! Autocast and neutral camp casting (old `AbilityRuntime.ambient` and `ai.ts`): each
//! binding is considered on its interval; aims are scored by predicting the release program
//! on copies of the units the caster observes.
//!
//! The player AI (M2.8) uses the same scoring. Like the old game, scoring follows only the
//! release program: persistent pulses, triggers and passives are not predicted.

use super::{
    Ctx,
    actor::{self, Actor, Side},
    cast::{self, Aim},
    corpses::{self, CorpseView},
    deliveries,
    library::{self, AbilityId},
    program::{self, Recipient},
};
use crate::{
    entity::{EntityId, Milli},
    orders::Order,
    state::State,
};
use canopy_content::{
    abilities::{
        Ability, AiIntent, AiPreference, Control, Delivery, DispelPolarity, DrainResource, Effect,
        Operation, Ownership, Polarity, TargetingKind,
    },
    placement::Owner,
};

/// What the scorer reads about the world beyond the observed units.
struct World<'a> {
    ctx: &'a Ctx<'a>,
    state: &'a State,
    caster: &'a Actor,
    ability: AbilityId,
    corpses: &'a [CorpseView],
    /// How near an enemy must be for a self buff to be worth it, thousandths of a cell.
    threat: i64,
    /// Every operation chooses its recipients from the units as the earlier operations
    /// left them (the player AI); otherwise operations without a query choose from the
    /// units as they were when the cast began, as the old scorer did (autocast and camps,
    /// D25).
    sequential: bool,
}

impl World<'_> {
    fn side(&self, t: &Actor) -> Side {
        actor::relation(&self.ctx.senses, self.caster, t)
    }

    fn has_status(&self, id: EntityId) -> bool {
        super::status_ops::has_status(self.state, id, self.ability)
    }

    fn damage_taken(&self, id: EntityId, damage_type: &str) -> f64 {
        let catalog = self.ctx.catalog();
        let kind = catalog.spells.damage_type(damage_type);
        self.state.entities.get(id).map_or(1000.0, |e| {
            super::statuses::damage_multiplier(catalog, e, kind, self.ctx.senses.combat.spell.0)
        })
    }
}

/// How far from its caster (cells) a unit can matter when scoring `spell`: the range, the
/// widest area around an aim, the delivery's reach, the strategic threat checks and building
/// footprints, and never less than the 24-cell threat radius. The caster's own units count
/// wherever they are (existing summons).
fn reach(spell: &Ability, rank: u8) -> i64 {
    let value = |a: &canopy_content::abilities::Amount| library::value(spell, rank, a).max(0);
    let range = value(&spell.targeting.range);
    let area = spell.targeting.radius.as_ref().map_or(0, value);
    let query = spell
        .on_release
        .iter()
        .flat_map(Operation::effects)
        .filter_map(Effect::query)
        .map(|q| value(&q.radius))
        .max()
        .unwrap_or(0);
    let delivery = match &spell.delivery {
        Some(Delivery::Chain {
            bounces, radius, ..
        }) => value(bounces) * radius.ceil() as i64,
        Some(Delivery::Line { length, width, .. }) => (length + width).ceil() as i64,
        Some(Delivery::Swarm { radius, .. }) => radius.ceil() as i64,
        _ => 0,
    };
    (range + area + query + delivery + 16).max(24)
}

fn threats(w: &World, observed: &[Actor]) -> bool {
    observed
        .iter()
        .any(|t| w.side(t) == Side::Enemy && actor::within(t.position, w.caster.position, w.threat))
}

/// The threat radius of a binding's AI policy, thousandths of a cell.
fn threat_of(ai: Option<&canopy_content::abilities::AbilityAi>) -> i64 {
    i64::from(ai.and_then(|a| a.threat_radius).unwrap_or(24)) * 1000
}

/// The score of casting `spell` at `aim` (old `abilityAimScore`).
fn score(
    w: &World,
    spell: &Ability,
    rank: u8,
    aim: &program::Aim,
    observed: &[Actor],
    preference: AiPreference,
) -> f64 {
    let caster = w.caster;
    if let Some(Delivery::Line { width, .. }) = &spell.delivery
        && spell.targeting.radius.is_none()
    {
        // A line aimed without an area: predict the units within its width of the aim.
        let mut copy = spell.clone();
        copy.targeting.radius = Some(canopy_content::abilities::Amount::Fixed(
            width.ceil().max(1.0) as u32,
        ));
        return score(w, &copy, rank, aim, observed, preference);
    }
    if let Some(d @ Delivery::Swarm { count, .. }) = &spell.delivery {
        // Score the bite program; returning cargo is not an immediate restore.
        let mut copy = spell.clone();
        copy.delivery = None;
        copy.targeting.kind = TargetingKind::Unit;
        copy.targeting.radius = None;
        let impact_only = |e: &Effect| match e {
            Effect::Drain(d) => {
                let mut d = d.clone();
                d.restore_caster = false;
                Effect::Drain(d)
            }
            other => other.clone(),
        };
        copy.on_release = spell
            .on_release
            .iter()
            .map(|op| match op {
                Operation::Effect(e) => Operation::Effect(impact_only(e)),
                Operation::Branch(b) => {
                    let mut b = b.clone();
                    b.then = b.then.iter().map(impact_only).collect();
                    b.otherwise = b.otherwise.iter().map(impact_only).collect();
                    Operation::Branch(b)
                }
            })
            .collect();
        let count = library::value(spell, rank, count).max(0) as usize;
        let mut targets: Vec<&Actor> = observed
            .iter()
            .filter(|t| deliveries::swarm_eligible(w.ctx, t, caster, spell, d))
            .collect();
        targets.sort_by_key(|t| t.id);
        return targets
            .into_iter()
            .take(count)
            .map(|t| {
                score(
                    w,
                    &copy,
                    rank,
                    &program::Aim::Unit(Box::new(t.clone())),
                    observed,
                    preference,
                )
            })
            .sum();
    }
    if let Some(wc) = &spell.weapon_cast {
        let Some(target) = aim.id().and_then(|id| observed.iter().find(|e| e.id == id)) else {
            return 0.0;
        };
        if w.side(target) != Side::Enemy || !cast::accepts(spell, target, Side::Enemy) {
            return 0.0;
        }
        let bonus = library::value(spell, rank, &wc.bonus).max(0);
        let mark = if wc.status.is_some() && !w.has_status(target.id) {
            10.0
        } else {
            0.0
        };
        return i64::from(target.hp).min(bonus) as f64 + mark;
    }
    let mut actors: Vec<Actor> = observed.to_vec();
    if !actors.iter().any(|a| a.id == caster.id) {
        actors.push(caster.clone());
    }
    actors.sort_by_key(|a| a.id);
    let defaults = actors.clone();
    let mut relationships: Vec<(EntityId, Side)> = Vec::new();
    let mut results = Vec::new();
    let primary = aim
        .id()
        .and_then(|id| actors.iter().find(|a| a.id == id).cloned());
    let resource_caster = caster.clone();
    let near_threats = threats(w, observed);
    let mut total = 0.0;
    let favoured = match preference {
        AiPreference::WoundedAlly => Side::Ally,
        AiPreference::Enemy => Side::Enemy,
    };
    for operation in &spell.on_release {
        let (declarations, branch): (
            Vec<(&Effect, bool)>,
            Option<&canopy_content::abilities::Branch>,
        ) = match operation {
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
        let branch_caster = actors
            .iter()
            .find(|a| a.id == caster.id)
            .cloned()
            .unwrap_or_else(|| caster.clone());
        let mut decisions: Vec<(Option<EntityId>, bool)> = Vec::new();
        for (effect, arm) in declarations {
            let operation_caster = actors
                .iter()
                .find(|a| a.id == caster.id)
                .cloned()
                .unwrap_or_else(|| caster.clone());
            let selector = if effect.query().is_some()
                || matches!(effect.target(), canopy_content::abilities::Anchor::Caster)
            {
                &operation_caster
            } else {
                caster
            };
            let predicted = |relationships: &[(EntityId, Side)], t: &Actor| {
                relationships
                    .iter()
                    .find(|(id, _)| *id == t.id)
                    .map_or_else(|| w.side(t), |(_, s)| *s)
            };
            let pool = if effect.query().is_some() || w.sequential {
                &actors
            } else {
                &defaults
            };
            let targets = program::recipients(
                w.ctx,
                w.state,
                effect,
                spell,
                rank,
                selector,
                caster,
                aim,
                pool,
                true,
                Some(pool),
            );
            for target in targets {
                let i = target
                    .id_of()
                    .and_then(|id| actors.iter().position(|a| a.id == id));
                let team = match i {
                    Some(i) => predicted(&relationships, &actors[i]),
                    None => Side::Ally,
                };
                if let Some(b) = branch {
                    let chosen = match decisions.iter().find(|(id, _)| *id == target.id_of()) {
                        Some((_, d)) => *d,
                        None => {
                            let d = match i {
                                Some(i) => {
                                    actor::condition(&b.condition, team, &branch_caster, &actors[i])
                                }
                                None => actor::condition(
                                    &b.condition,
                                    team,
                                    &branch_caster,
                                    &actor::Nothing,
                                ),
                            };
                            decisions.push((target.id_of(), d));
                            d
                        }
                    };
                    if chosen != arm {
                        continue;
                    }
                }
                if team == Side::Neutral
                    || i.is_some_and(|i| {
                        !actors[i].alive || !cast::accepts(spell, &actors[i], team)
                    })
                {
                    continue;
                }
                let before = i.map(|i| actors[i].clone());
                let amount = program::magnitude(
                    spell,
                    rank,
                    effect,
                    Some(&resource_caster),
                    primary.as_ref(),
                    before.as_ref(),
                    &results,
                ) as f64;
                let friendly = team == Side::Ally;
                let sign = |good: bool| if good { 1.0 } else { -1.0 };
                let mut benefit: f64 = 0.0;
                let a = before.as_ref();
                match effect {
                    Effect::Heal(_) => {
                        if let Some(a) = a {
                            benefit =
                                amount.min(f64::from(a.max_hp) - f64::from(a.hp)) * sign(friendly);
                        }
                    }
                    Effect::Damage(d) => {
                        if let Some(a) = a {
                            benefit = (amount * w.damage_taken(a.id, &d.damage_type) / 1000.0)
                                .min(f64::from(a.hp))
                                * sign(!friendly);
                        }
                    }
                    Effect::Drain(d) => {
                        if let Some(a) = a {
                            let pool = match d.resource {
                                DrainResource::Health => f64::from(a.hp),
                                DrainResource::Mana => f64::from(a.mana),
                            };
                            benefit = amount.min(pool) * sign(!friendly);
                        }
                    }
                    Effect::Mana(_) => {
                        if let Some(a) = a {
                            benefit = amount
                                .min((f64::from(a.max_mana) - f64::from(a.mana)).max(0.0))
                                * sign(friendly);
                        }
                    }
                    Effect::Status(s) => {
                        let id = target.id_of();
                        let immune = a.is_some_and(|a| {
                            a.control_immunity != 0
                                && w.ctx
                                    .catalog()
                                    .spells
                                    .status_named(w.ability, &s.id)
                                    .is_some_and(|k| {
                                        !w.ctx
                                            .catalog()
                                            .spells
                                            .status(k)
                                            .has_payload(rank, a.control_immunity)
                                    })
                        });
                        benefit = if id.is_some_and(|id| w.has_status(id)) || immune {
                            0.0
                        } else if (s.polarity == Polarity::Positive) == friendly {
                            10.0
                        } else {
                            -10.0
                        };
                        if friendly
                            && spell.targeting.kind == TargetingKind::Caster
                            && !near_threats
                        {
                            benefit = 0.0;
                        }
                    }
                    Effect::Summon(op) => {
                        let existing = if op.replace {
                            0
                        } else {
                            observed
                                .iter()
                                .filter(|t| {
                                    t.alive
                                        && t.owner == caster.owner
                                        && t.summon_origin == Some((caster.id, w.ability))
                                })
                                .count() as i64
                        };
                        let capacity = op.max_active.as_ref().map_or(f64::INFINITY, |m| {
                            (library::value(spell, rank, m) - existing).max(0) as f64
                        });
                        let requested = amount
                            * match &op.corpses {
                                Some(q) => {
                                    let n = corpses::select(
                                        w.corpses,
                                        library::value(spell, rank, &q.radius),
                                        &q.relations,
                                        q.filter.as_ref(),
                                        q.order,
                                        target.position_of(),
                                    )
                                    .len() as i64;
                                    library::value(spell, rank, &q.max_targets).min(n) as f64
                                }
                                None => 1.0,
                            };
                        benefit = if near_threats {
                            requested.min(capacity) * 20.0
                        } else {
                            0.0
                        };
                    }
                    Effect::Split(op) => {
                        benefit = if near_threats {
                            op.members.len() as f64 * 20.0
                        } else {
                            0.0
                        };
                    }
                    Effect::Resurrect(op) => {
                        benefit = if op.duration_ticks.is_some() && !near_threats {
                            0.0
                        } else {
                            let health = library::value(spell, rank, &op.health_permille) as f64;
                            corpses::select(
                                w.corpses,
                                library::value(spell, rank, &op.corpses.radius),
                                &op.corpses.relations,
                                op.corpses.filter.as_ref(),
                                op.corpses.order,
                                target.position_of(),
                            )
                            .into_iter()
                            .take(amount as usize)
                            .map(|c| {
                                let mine = op.ownership == Some(Ownership::Caster)
                                    || c.relation
                                        == canopy_content::abilities::CorpseRelation::Ally;
                                f64::from(c.max_hp) * health / 1000.0 * sign(mine)
                            })
                            .sum()
                        };
                    }
                    Effect::Contain(_) => {
                        benefit = a
                            .filter(|a| a.unit && !a.hero && !friendly)
                            .map_or(0.0, |a| f64::from(a.hp));
                    }
                    Effect::Convert(_) => {
                        benefit = a
                            .filter(|a| a.unit && !a.hero && !friendly)
                            .map_or(0.0, |a| f64::from(a.hp) + f64::from(a.max_hp));
                    }
                    Effect::Sacrifice(_) => {
                        let Some(a) = a.filter(|a| {
                            friendly
                                && a.unit
                                && !a.hero
                                && a.id != caster.id
                                && a.owner == caster.owner
                        }) else {
                            continue;
                        };
                        benefit = -f64::from(a.hp);
                    }
                    Effect::Dispel(d) => {
                        benefit = if d.polarity == DispelPolarity::All
                            || (d.polarity == DispelPolarity::Negative) == friendly
                        {
                            10.0
                        } else {
                            0.0
                        };
                    }
                    Effect::ReleaseContained(_)
                    | Effect::Teleport(_)
                    | Effect::Pull(_)
                    | Effect::Vision(_)
                    | Effect::Revive(_) => {}
                }
                let applied = match effect {
                    Effect::Sacrifice(_) => 1.0,
                    Effect::Convert(_) => f64::from(u8::from(benefit > 0.0)),
                    _ => benefit.abs(),
                };
                program::record(&mut results, effect, before.as_ref(), applied as i64);
                if let Some(i) = i {
                    let t = &mut actors[i];
                    match effect {
                        Effect::Sacrifice(_) => {
                            t.hp = 0;
                            t.alive = false;
                        }
                        Effect::Convert(_) if applied > 0.0 => {
                            t.owner = caster.owner;
                            relationships.push((t.id, Side::Ally));
                        }
                        Effect::Heal(_) => t.hp = t.max_hp.min(t.hp + applied as u32),
                        Effect::Damage(_) => {
                            t.hp = t.hp.saturating_sub(applied as u32);
                            t.alive = t.hp > 0;
                        }
                        Effect::Drain(d) if d.resource == DrainResource::Health => {
                            t.hp = t.hp.saturating_sub(applied as u32);
                            t.alive = t.hp > 0;
                        }
                        Effect::Mana(_) => t.mana = t.max_mana.min(t.mana + applied as u32),
                        Effect::Drain(_) => t.mana = t.mana.saturating_sub(applied as u32),
                        Effect::Status(s) if benefit != 0.0 => {
                            if let Some(m) = s.form.as_ref().and_then(|f| f.movement.as_ref()) {
                                t.air = m.locomotion == canopy_content::conditions::Locomotion::Air;
                            }
                        }
                        _ => {}
                    }
                }
                if let Effect::Drain(d) = effect {
                    if d.restore_caster
                        && let Some(c) = actors.iter_mut().find(|a| a.id == caster.id)
                        && c.alive
                        && c.owner == caster.owner
                    {
                        let restored = match d.resource {
                            DrainResource::Health => {
                                applied.min(f64::from(c.max_hp.saturating_sub(c.hp)))
                            }
                            DrainResource::Mana => {
                                applied.min(f64::from(c.max_mana.saturating_sub(c.mana)))
                            }
                        };
                        match d.resource {
                            DrainResource::Health => c.hp += restored as u32,
                            DrainResource::Mana => c.mana += restored as u32,
                        }
                        total += restored
                            * if preference == AiPreference::WoundedAlly {
                                2.0
                            } else {
                                1.0
                            };
                    }
                    if d.resource == DrainResource::Mana
                        && d.damage_per_drained_permille > 0
                        && let Some(i) = i
                    {
                        let t = &mut actors[i];
                        let burn = ((applied * f64::from(d.damage_per_drained_permille) / 1000.0)
                            .floor()
                            * w.damage_taken(t.id, &d.damage_type)
                            / 1000.0)
                            .min(f64::from(t.hp));
                        t.hp = (f64::from(t.hp) - burn).max(0.0) as u32;
                        t.alive = t.hp > 0;
                        benefit += burn * sign(!friendly);
                    }
                }
                total += benefit * if team == favoured { 2.0 } else { 1.0 };
            }
        }
    }
    total
}

impl Recipient {
    fn id_of(&self) -> Option<EntityId> {
        match self {
            Recipient::Unit(a) => Some(a.id),
            Recipient::Point(_) => None,
        }
    }

    fn position_of(&self) -> Milli {
        match self {
            Recipient::Unit(a) => a.position,
            Recipient::Point(p) => *p,
        }
    }
}

/// Tactical destinations for escape, reinforce and scout intents (old
/// `strategicAbilityAims`).
#[allow(clippy::too_many_arguments)]
fn strategic(
    ctx: &Ctx,
    state: &State,
    intent: AiIntent,
    spell: &Ability,
    rank: u8,
    caster: &Actor,
    observed: &[Actor],
    usable: &dyn Fn(i32, i32) -> bool,
) -> Vec<Aim> {
    let range = library::value(spell, rank, &spell.targeting.range) as f64;
    let side = |t: &Actor| actor::relation(&ctx.senses, caster, t);
    let enemies: Vec<&Actor> = observed
        .iter()
        .filter(|t| t.alive && side(t) == Side::Enemy)
        .collect();
    let d2 = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2);
    let here = (caster.x(), caster.y());
    if intent == AiIntent::Reinforce {
        let mut list: Vec<(EntityId, usize)> = observed
            .iter()
            .filter(|t| {
                let d = d2((t.x(), t.y()), here);
                t.alive
                    && t.targetable
                    && (t.unit || spell.targeting.include_buildings == Some(true))
                    && side(t) == Side::Ally
                    && t.id != caster.id
                    && d > 144.0
                    && d <= range * range
            })
            .map(|t| {
                let threat = enemies
                    .iter()
                    .filter(|e| d2((e.x(), e.y()), (t.x(), t.y())) <= 64.0)
                    .count();
                (t.id, threat)
            })
            .filter(|(_, threat)| *threat > 0)
            .collect();
        list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        return list.into_iter().map(|(id, _)| Aim::Unit(id)).collect();
    }
    if intent == AiIntent::Escape
        && (f64::from(caster.hp) > f64::from(caster.max_hp) * 0.5
            || !enemies.iter().any(|e| d2((e.x(), e.y()), here) <= 100.0))
    {
        return Vec::new();
    }
    let size = ctx.spatial.size() as i32;
    let valid = |x: i32, y: i32| x >= 0 && y >= 0 && x < size && y < size && usable(x, y);
    let visible = |x: i32, y: i32| {
        super::visible_point(
            ctx,
            state,
            caster.owner,
            Milli {
                x: x * 1000,
                y: y * 1000,
            },
            Some(caster),
        )
    };
    let explored = |x: i32, y: i32| {
        ctx.senses
            .sight(state)
            .explored(caster.owner, &[(y * size + x) as u32])
    };
    let mut points: Vec<(i32, i32, f64, usize)> = Vec::new();
    for ring in 1..=2 {
        for i in 0..16 {
            let theta =
                (f64::from(i) + f64::from(caster.id.0 % 16)) / 16.0 * std::f64::consts::PI * 2.0;
            let radius = range.min(if intent == AiIntent::Escape {
                16.0
            } else {
                128.0
            }) * f64::from(ring)
                / 2.0;
            let x = cast::js_round(here.0 + crate::math::cos(theta) * radius) as i32;
            let y = cast::js_round(here.1 + crate::math::sin(theta) * radius) as i32;
            let p = (f64::from(x), f64::from(y));
            if d2(p, here) > range * range
                || !valid(x, y)
                || (spell.targeting.visible && !visible(x, y))
            {
                continue;
            }
            let safety = enemies
                .iter()
                .map(|e| d2((e.x(), e.y()), p))
                .fold(f64::INFINITY, f64::min);
            if intent == AiIntent::Escape
                && safety
                    <= enemies
                        .iter()
                        .map(|e| d2((e.x(), e.y()), here))
                        .fold(f64::INFINITY, f64::min)
            {
                continue;
            }
            if intent == AiIntent::Scout && visible(x, y) {
                continue;
            }
            let score = if intent == AiIntent::Escape {
                safety
            } else {
                (if explored(x, y) { 0.0 } else { 10000.0 }) + d2(p, here)
            };
            let index = points.len();
            points.push((x, y, score, index));
        }
    }
    points.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.3.cmp(&b.3)));
    points
        .into_iter()
        .map(|(x, y, ..)| {
            Aim::Point(Milli {
                x: x * 1000,
                y: y * 1000,
            })
        })
        .collect()
}

/// Corpse points for resurrection and corpse summons (old `corpseAbilityAims`).
fn corpse_aims(spell: &Ability, corpses: &[CorpseView]) -> Vec<(EntityId, Milli)> {
    let wants = spell.targeting.kind == TargetingKind::Point
        && spell
            .on_release
            .iter()
            .flat_map(Operation::effects)
            .any(|e| match e {
                Effect::Resurrect(r) => {
                    r.target == canopy_content::abilities::ResurrectTarget::Point
                }
                Effect::Summon(s) => {
                    s.corpses.is_some() && s.target == canopy_content::abilities::Anchor::Point
                }
                _ => false,
            });
    if !wants {
        return Vec::new();
    }
    let mut list: Vec<(EntityId, Milli)> = corpses.iter().map(|c| (c.id, c.position)).collect();
    list.sort_by_key(|c| c.0);
    list
}

/// What autocast learns about the units while the world stands still: sight answers per
/// player and unit snapshots. Both are pure functions of the state, which only a cast
/// changes in [`ambient`], so they are forgotten after every cast.
#[derive(Default)]
struct Observations {
    /// Answers carry the epoch they were learnt in; older ones are unknown.
    epoch: u32,
    width: usize,
    /// Per player slot, per entity id: epoch × 2 + visible.
    sight: Vec<u32>,
    /// Per entity id: epoch and index into `actors`.
    snapshot: Vec<(u32, u32)>,
    actors: Vec<Actor>,
}

impl Observations {
    /// Player slots remembered; others are asked every time.
    const PLAYERS: usize = 16;

    /// Forgets everything; `ids` bounds the entity ids to remember.
    fn forget(&mut self, ids: usize) {
        self.epoch += 1;
        self.actors.clear();
        if ids > self.width || self.epoch >= u32::MAX / 2 {
            self.width = ids.next_power_of_two();
            self.epoch = 1;
            self.sight = vec![0; self.width * Self::PLAYERS];
            self.snapshot = vec![(0, 0); self.width];
        }
    }

    /// [`super::visible`] for a player's caster: the owner's exact sight of the unit,
    /// which does not depend on which of its units asks.
    fn visible(&mut self, ctx: &Ctx, state: &State, caster: &Actor, target: EntityId) -> bool {
        let id = target.0 as usize;
        let slot = match caster.owner {
            Owner::Player(p) if usize::from(p) < Self::PLAYERS && id < self.width => usize::from(p),
            _ => return super::visible(ctx, state, caster.owner, target, Some(caster)),
        };
        let entry = &mut self.sight[slot * self.width + id];
        if *entry >> 1 == self.epoch {
            return *entry & 1 == 1;
        }
        let seen = super::visible(ctx, state, caster.owner, target, Some(caster));
        *entry = self.epoch << 1 | u32::from(seen);
        seen
    }

    fn actor(&mut self, ctx: &Ctx, state: &State, e: &crate::entity::Entity) -> Actor {
        let id = e.id.0 as usize;
        if id >= self.width {
            return actor::of(&ctx.senses, state, e);
        }
        let (epoch, index) = self.snapshot[id];
        if epoch == self.epoch {
            return self.actors[index as usize].clone();
        }
        let a = actor::of(&ctx.senses, state, e);
        self.snapshot[id] = (self.epoch, self.actors.len() as u32);
        self.actors.push(a.clone());
        a
    }
}

/// Autocast and neutral casters (old `ambient`).
pub fn ambient(ctx: &mut Ctx, state: &mut State) {
    let _span = tracing::trace_span!("autocast").entered();
    let tick = state.tick;
    let mut known = Observations::default();
    known.forget(state.counters.entity as usize + 1);
    for id in cast::casters(state) {
        let Some(e) = state.entities.get(id) else {
            continue;
        };
        let Some(c) = &e.caster else {
            continue;
        };
        if c.pending.is_some()
            || c.weapon_order.is_some()
            || matches!(
                e.unit.as_ref().and_then(|u| u.order.as_ref()),
                Some(Order::CastAbility { .. })
            )
        {
            continue;
        }
        let neutral = e.owner == Owner::Nobody
            && e.ready_tick <= tick
            && crate::spatial::present(e)
            && actor::alive(e);
        let Some(policy) = ctx.catalog().spells.caster(e.definition) else {
            continue;
        };
        let ranks = c.ranks.clone();
        let autocast = c.autocast.clone();
        for (index, b) in policy.bindings.iter().enumerate() {
            let spell = ctx.ability(b.ability);
            let rank = ranks.get(index).copied().unwrap_or(0);
            if rank == 0 || library::passive(spell) || spell.weapon_cast.is_some() {
                continue;
            }
            let auto = spell.autocast.as_ref().is_some_and(|a| {
                autocast
                    .get(&(index as u8))
                    .copied()
                    .unwrap_or(a.enabled_by_default)
            });
            let interval = if auto {
                spell.autocast.as_ref().map(|a| a.interval_ticks)
            } else {
                b.ai.as_ref().map(|a| a.interval_ticks)
            };
            let Some(interval) = interval.filter(|&i| i > 0) else {
                continue;
            };
            if (!auto && (!neutral || b.ai.is_none())) || tick % interval != id.0 % interval {
                continue;
            }
            if !b.allows(Control::Ai) {
                continue;
            }
            if spell.persistent.as_ref().is_some_and(|p| p.toggle)
                && state
                    .abilities
                    .instances
                    .iter()
                    .any(|i| i.source == id && i.ability == b.ability)
            {
                continue;
            }
            let Some(access) = cast::access(ctx, state, id) else {
                break;
            };
            let caster = access.actor;
            // Units beyond `reach` cannot change the choice; skipping them saves the exact
            // sight test, the costliest part of autocast.
            let reach = reach(spell, rank) * 1000;
            let mut observed: Vec<Actor> = Vec::new();
            for t in state.entities.iter() {
                if t.hp.is_some()
                    && actor::alive(t)
                    && (t.owner == caster.owner
                        || actor::within(t.position, caster.position, reach))
                    && known.visible(ctx, state, &caster, t.id)
                {
                    observed.push(known.actor(ctx, state, t));
                }
            }
            let binding = b.id.clone();
            if let Some(intent) =
                b.ai.as_ref()
                    .and_then(|a| a.intent)
                    .filter(|&i| i != AiIntent::Utility)
            {
                let aims = strategic(
                    ctx,
                    state,
                    intent,
                    spell,
                    rank,
                    &caster,
                    &observed,
                    &|_, _| true,
                );
                let chosen = aims.into_iter().find(|&aim| {
                    cast::reason_as(
                        ctx,
                        state,
                        id,
                        Some(&caster),
                        &binding,
                        aim,
                        Control::Ai,
                        false,
                    )
                    .is_ok()
                });
                if let Some(aim) = chosen {
                    let _ = cast::cast(ctx, state, id, &binding, aim, Control::Ai);
                    known.forget(state.counters.entity as usize + 1);
                    break;
                }
                continue;
            }
            let aim_of = |t: &Actor| match spell.targeting.kind {
                TargetingKind::Caster => Aim::Unit(id),
                TargetingKind::Point => Aim::Point(Milli {
                    x: super::cell_round(t.position.x) * 1000,
                    y: super::cell_round(t.position.y) * 1000,
                }),
                TargetingKind::Unit => Aim::Unit(t.id),
            };
            let preference =
                b.ai.as_ref()
                    .map_or(AiPreference::WoundedAlly, |a| a.preference);
            let threat = threat_of(b.ai.as_ref());
            let visible_corpses = corpses::visible(ctx, state, &caster);
            let pool: Vec<Actor> = if spell.targeting.kind == TargetingKind::Caster {
                vec![caster.clone()]
            } else {
                observed.clone()
            };
            let mut candidates: Vec<(EntityId, Aim, f64)> = Vec::new();
            for t in &pool {
                let aim = aim_of(t);
                if cast::reason_as(
                    ctx,
                    state,
                    id,
                    Some(&caster),
                    &binding,
                    aim,
                    Control::Ai,
                    false,
                )
                .is_err()
                {
                    continue;
                }
                let program_aim = match aim {
                    Aim::Point(p) => program::Aim::Point(p),
                    Aim::Unit(_) => program::Aim::Unit(Box::new(t.clone())),
                };
                let w = World {
                    ctx,
                    state,
                    caster: &caster,
                    ability: b.ability,
                    corpses: &visible_corpses,
                    threat,
                    sequential: false,
                };
                candidates.push((
                    t.id,
                    aim,
                    score(&w, spell, rank, &program_aim, &observed, preference),
                ));
            }
            for (key, point) in corpse_aims(spell, &visible_corpses) {
                let aim = Aim::Point(point);
                if cast::reason_as(
                    ctx,
                    state,
                    id,
                    Some(&caster),
                    &binding,
                    aim,
                    Control::Ai,
                    false,
                )
                .is_err()
                {
                    continue;
                }
                let w = World {
                    ctx,
                    state,
                    caster: &caster,
                    ability: b.ability,
                    corpses: &visible_corpses,
                    threat,
                    sequential: false,
                };
                candidates.push((
                    key,
                    aim,
                    score(
                        &w,
                        spell,
                        rank,
                        &program::Aim::Point(point),
                        &observed,
                        preference,
                    ),
                ));
            }
            candidates.retain(|c| c.2 > 0.0);
            candidates.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(&b.0)));
            if let Some((_, aim, _)) = candidates.into_iter().next() {
                let _ = cast::cast(ctx, state, id, &binding, aim, Control::Ai);
                known.forget(state.counters.entity as usize + 1);
                break;
            }
        }
    }
}

/// How the player AI (M2.8) values casting `ability` at `rank` from `caster` at `aim`,
/// given the units its player observes: the release program predicted in order on copies
/// of them. Unlike autocast, every operation chooses its recipients from the units as the
/// earlier operations left them.
#[allow(clippy::too_many_arguments)]
pub fn player_score(
    ctx: &Ctx,
    state: &State,
    caster: &Actor,
    ability: AbilityId,
    rank: u8,
    aim: &program::Aim,
    observed: &[Actor],
    preference: AiPreference,
    corpses: &[CorpseView],
    threat_radius: Option<u32>,
) -> f64 {
    let w = World {
        ctx,
        state,
        caster,
        ability,
        corpses,
        threat: i64::from(threat_radius.unwrap_or(24)) * 1000,
        sequential: true,
    };
    score(&w, ctx.ability(ability), rank, aim, observed, preference)
}

/// Tactical destinations for the player AI's escape, reinforce and scout intents, best
/// first; `usable` rejects cells the player knows it cannot use.
#[allow(clippy::too_many_arguments)]
pub fn player_strategic(
    ctx: &Ctx,
    state: &State,
    intent: AiIntent,
    ability: AbilityId,
    rank: u8,
    caster: &Actor,
    observed: &[Actor],
    usable: &dyn Fn(i32, i32) -> bool,
) -> Vec<Aim> {
    strategic(
        ctx,
        state,
        intent,
        ctx.ability(ability),
        rank,
        caster,
        observed,
        usable,
    )
}

/// Corpse points a resurrection or corpse summon may aim at, by corpse id.
pub fn corpse_points(ability: &Ability, corpses: &[CorpseView]) -> Vec<(EntityId, Milli)> {
    corpse_aims(ability, corpses)
}
