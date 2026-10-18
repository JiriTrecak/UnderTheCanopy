//! Rules of the ability format beyond field types: rank parameters, operation
//! combinations, uniqueness and ordering. Each ability is checked on its own; nothing
//! here needs the rest of the content.

use super::{
    Ability, AbilityLibrary, Activation, Amount, CombatModifiers, Delivery, Effect, Operation,
    PersistentAnchor, RankParameters, StatusEffect, StatusLifetime, StatusOrEffect, TargetingKind,
    TriggerEvent,
    operations::{Anchor, RankedModifier, StackScope},
};
use crate::{
    conditions::{Relation, TargetFilter, has_duplicates},
    effects::{BindingAnchor, BindingLifetime, EffectEvent},
    error::Error,
    items::ItemModifiers,
};
use std::collections::{BTreeMap, BTreeSet};

const MILLION: i64 = 1_000_000;
const MAX_TICKS: i64 = 144_000;

/// Checks every ability; errors name the ability's file (`files`: id → file).
pub(crate) fn check(
    library: &AbilityLibrary,
    files: &BTreeMap<String, String>,
    errors: &mut Vec<Error>,
) {
    let mut fail = |at: &str, message: String| {
        let file = files.get(at).map_or("abilities", String::as_str);
        errors.push(Error::new(file, at, message));
    };
    for ability in &library.abilities {
        let at = ability.id.as_str();
        let presentation = &ability.presentation;
        let ids: Vec<_> = presentation.effects.iter().map(|b| b.id.as_str()).collect();
        if has_duplicates(&ids) {
            fail(at, "Binding IDs must be unique".into());
        }
        for binding in &presentation.effects {
            if binding.lifetime == BindingLifetime::Status
                && (binding.event != EffectEvent::StatusApplied
                    || binding.anchor != BindingAnchor::Target)
            {
                fail(
                    at,
                    format!(
                        "{}: Status lifetime requires target statusApplied binding",
                        binding.id
                    ),
                );
            }
        }
        let mut check = Check {
            ability,
            problems: Vec::new(),
        };
        check.run();
        for problem in check.problems {
            fail(at, problem);
        }
    }
}

struct Check<'a> {
    ability: &'a Ability,
    problems: Vec<String>,
}

impl<'a> Check<'a> {
    fn fail(&mut self, message: impl Into<String>) {
        self.problems.push(message.into());
    }

    fn require(&mut self, ok: bool, message: &str) {
        if !ok {
            self.fail(message);
        }
    }

    /// Every rank must resolve the amount to `0..=max`.
    fn amount(&mut self, amount: &Amount, path: &str, max: i64) {
        for (index, rank) in self.ability.ranks.iter().enumerate() {
            let rank_number = index + 1;
            match (amount.at(rank), amount) {
                (None, Amount::Rank(name)) => self.fail(format!(
                    "{path}: Rank {rank_number}: missing parameter {name}"
                )),
                (Some(value), _) if !(0..=max).contains(&value) => self.fail(format!(
                    "{path}: Rank {rank_number}: value must be between 0 and {max}"
                )),
                _ => {}
            }
        }
    }

    /// True when some rank resolves the amount and the value satisfies `test`. Missing
    /// parameters are reported by [`Self::amount`] and never match here.
    fn any_rank(&self, amount: &Amount, test: impl Fn(i64) -> bool) -> bool {
        self.ability
            .ranks
            .iter()
            .any(|rank| amount.at(rank).is_some_and(&test))
    }

    fn below_one(&self, amount: &Amount) -> bool {
        self.any_rank(amount, |v| v < 1)
    }

    fn at_least(&mut self, value: f64, min: f64, what: &str) {
        if value < min {
            self.fail(format!("{what} must be at least {min}"));
        }
    }

    fn filter(&mut self, filter: Option<&TargetFilter>) {
        for problem in filter.map(TargetFilter::problems).unwrap_or_default() {
            self.fail(problem);
        }
    }

    /// A status template other features refer to by id: `statuses` only.
    fn template(&self, id: &str) -> Option<&'a StatusEffect> {
        self.ability.statuses.iter().flatten().find(|s| s.id == id)
    }

    fn independent_template(&self, id: &str) -> bool {
        self.template(id).is_some_and(|s| {
            s.target == crate::conditions::Subject::Target
                && s.query.is_none()
                && s.lifetime != StatusLifetime::Instance
        })
    }

    fn run(&mut self) {
        let a = self.ability;
        self.ranks();
        self.shapes();
        if let Some(weapon) = &a.weapon_cast {
            self.amount(&weapon.bonus, "weaponCast.bonus", MILLION);
            let t = &a.targeting;
            let plain = a.activation == Activation::Targeted
                && t.kind == TargetingKind::Unit
                && !t.allow_self
                && t.relations == [Relation::Enemy]
                && a.delivery.is_none()
                && a.persistent.is_none()
                && a.aura.is_none()
                && a.combat_modifiers.is_none()
                && a.cast.channel.is_none()
                && a.cast.prepare_ticks == 0
                && a.cast.recover_ticks == 0
                && a.on_release.is_empty();
            self.require(plain, "Weapon casts require a targeted enemy unit, zero spell preparation/recovery, and no release program, channel, delivery, persistent or passive modifiers");
            if let Some(status) = &weapon.status {
                let ok = self.independent_template(status);
                self.require(ok, "Weapon casts require a target-only status template with an independent lifetime");
            }
            if weapon.status.is_none() && self.any_rank(&weapon.bonus, |v| v == 0) {
                self.fail("Weapon casts need damage or an impact status");
            }
        }
        if let Some(modifiers) = &a.combat_modifiers {
            self.combat(modifiers);
            if a.activation != Activation::Passive {
                self.fail("Direct combat modifiers require a passive ability; targeted spells grant them through statuses");
            }
        }
        self.targeting();
        self.amount(&a.cast.cost.amount, "cast.cost.amount", MILLION);
        self.amount(&a.cast.cooldown.ticks, "cast.cooldown.ticks", MAX_TICKS);
        for (i, operation) in a
            .on_release
            .iter()
            .chain(a.persistent.iter().flat_map(|p| p.pulse.iter().flatten()))
            .enumerate()
        {
            match operation {
                Operation::Branch(branch) => {
                    for e in branch.then.iter().chain(&branch.otherwise) {
                        self.amount(&e.amount(), &format!("onRelease[{i}].amount"), MILLION);
                    }
                }
                Operation::Effect(e) => {
                    let max = if matches!(e, Effect::Summon(_)) {
                        8
                    } else {
                        MILLION
                    };
                    self.amount(&e.amount(), &format!("onRelease[{i}].amount"), max);
                }
            }
        }
        self.records();
        self.unique_ids();
        for entry in a.all_effects() {
            self.effect(entry);
        }
        self.activation();
        self.delivery();
        if a.on_release.is_empty()
            && a.triggers.as_ref().is_none_or(Vec::is_empty)
            && a.combat_modifiers.is_none()
            && a.weapon_cast.is_none()
        {
            self.fail("An ability requires release operations or triggers");
        }
        self.persistent();
        self.triggers();
    }

    fn ranks(&mut self) {
        let ranks = &self.ability.ranks;
        if ranks.is_empty() {
            self.fail("An ability needs at least one rank");
        }
        let names = |rank: &RankParameters| rank.keys().cloned().collect::<Vec<_>>();
        if ranks.iter().any(|r| names(r) != names(&ranks[0])) {
            self.fail("Every rank must declare the same parameters");
        }
        if ranks.iter().any(|r| r.len() > 24) {
            self.fail("Maximum 24 rank parameters");
        }
    }

    /// Array sizes, uniqueness and minimums that the simulation depends on.
    fn shapes(&mut self) {
        let a = self.ability;
        let t = &a.targeting;
        if t.relations.is_empty() || has_duplicates(&t.relations) {
            self.fail("Targeting needs distinct relationships");
        }
        if t.max_targets == Some(0) {
            self.fail("Targeting maxTargets must be positive");
        }
        self.filter(t.filter.as_ref());
        for filter in t.condition.iter().flat_map(|c| c.filters()) {
            self.filter(Some(filter));
        }
        if let Some(autocast) = &a.autocast {
            self.require(
                autocast.interval_ticks >= 1,
                "Autocast interval must be positive",
            );
        }
        if let Some(aura) = &a.aura {
            self.at_least(aura.radius, 1.0, "Aura radius");
        }
        if let Some(tether) = a.persistent.as_ref().and_then(|p| p.tether_range) {
            self.at_least(tether, 1.0, "Tether range");
        }
        if let Some(tether) = a.cast.channel.as_ref().and_then(|c| c.tether_range) {
            self.at_least(tether, 1.0, "Tether range");
        }
        match &a.delivery {
            Some(Delivery::Charge { speed, max_ticks }) => {
                self.at_least(*speed, 1.0, "Charge speed");
                self.require(*max_ticks >= 1, "Charge maxTicks must be positive");
            }
            Some(Delivery::Swarm {
                speed,
                radius,
                attack_interval_ticks,
                max_per_target,
                max_hits_per_trip,
                return_timeout_ticks,
                filter,
                ..
            }) => {
                self.at_least(*speed, 1.0, "Swarm speed");
                self.at_least(*radius, 1.0, "Swarm radius");
                let positive = [
                    attack_interval_ticks,
                    max_per_target,
                    max_hits_per_trip,
                    return_timeout_ticks,
                ];
                self.require(
                    positive.iter().all(|&&n| n >= 1),
                    "Swarm intervals, limits and timeouts must be positive",
                );
                self.filter(filter.as_ref());
            }
            Some(Delivery::Projectile { speed }) => self.at_least(*speed, 1.0, "Projectile speed"),
            Some(Delivery::Chain {
                radius,
                interval_ticks,
                retention_permille,
                ..
            }) => {
                self.at_least(*radius, 1.0, "Chain radius");
                self.require(
                    *interval_ticks >= 1 && *retention_permille >= 1,
                    "Chain interval and retention must be positive",
                );
            }
            Some(Delivery::Line {
                speed,
                length,
                width,
            }) => {
                self.at_least(*speed, 1.0, "Line speed");
                self.at_least(*length, 1.0, "Line length");
                self.at_least(*width, 0.1, "Line width");
            }
            None => {}
        }
        for trigger in a.triggers.iter().flatten() {
            self.filter(trigger.filter.as_ref());
            if trigger.operations.is_empty() {
                self.fail("Triggers need operations");
            }
            self.require(
                trigger.chance_permille >= 1 && trigger.every >= 1,
                "Trigger chance and frequency must be positive",
            );
        }
        let programs = a
            .on_release
            .iter()
            .chain(a.triggers.iter().flatten().flat_map(|t| &t.operations));
        for operation in programs {
            if let Operation::Branch(branch) = operation {
                if branch.then.is_empty() || branch.otherwise.is_empty() {
                    self.fail("Branches need operations on both arms");
                }
                for filter in branch.condition.filters() {
                    self.filter(Some(filter));
                }
            }
        }
    }

    fn combat(&mut self, m: &CombatModifiers) {
        if let Some(bonus) = &m.attack_bonus {
            if let Some(status) = &bonus.status
                && !self.independent_template(status)
            {
                self.fail("Weapon marks require a target-only status template with an independent lifetime");
            }
            self.amount(&bonus.amount, "combatModifiers.attackBonus.amount", MILLION);
            self.amount(
                &bonus.mana_cost,
                "combatModifiers.attackBonus.manaCost",
                MILLION,
            );
        }
        if let Some(critical) = &m.critical {
            self.amount(
                &critical.chance_permille,
                "combatModifiers.critical.chancePermille",
                1000,
            );
            self.amount(
                &critical.multiplier_permille,
                "combatModifiers.critical.multiplierPermille",
                10_000,
            );
            if self.any_rank(&critical.multiplier_permille, |v| v < 1000) {
                self.fail("Critical multiplier must be at least 1000");
            }
        }
        if let Some(evasion) = &m.evasion_permille {
            self.amount(evasion, "combatModifiers.evasionPermille", 1000);
        }
        if let Some(cleave) = &m.cleave {
            self.amount(
                &cleave.damage_permille,
                "combatModifiers.cleave.damagePermille",
                1000,
            );
            self.amount(&cleave.radius, "combatModifiers.cleave.radius", 32);
            if self.below_one(&cleave.radius) {
                self.fail("Cleave radius must be positive");
            }
            self.at_least(cleave.arc_degrees, 1.0, "Cleave arc");
            self.require(
                cleave.max_targets >= 1,
                "Cleave maxTargets must be positive",
            );
        }
        for filter in m.filters() {
            self.filter(Some(filter));
        }
        if m.attack_bonus.is_none()
            && m.critical.is_none()
            && m.evasion_permille.is_none()
            && m.cleave.is_none()
        {
            self.fail("Combat modifiers require at least one policy");
        }
    }

    fn targeting(&mut self) {
        let a = self.ability;
        let t = &a.targeting;
        self.amount(&t.range, "targeting.range", 4096);
        if !t.visible && t.kind == TargetingKind::Unit {
            self.fail("Unit aims require visibility; remote scouting uses a point aim");
        }
        let no_radius = matches!(t.radius, None | Some(Amount::Fixed(0)));
        // A line runs its own length and width; its aim needs no area (the old rule asked
        // for one it never used).
        let line = matches!(a.delivery, Some(Delivery::Line { .. }));
        if t.kind == TargetingKind::Point && no_radius && !line {
            self.fail("Ground targeting requires an area radius");
        }
        if t.kind != TargetingKind::Point && t.radius.is_some() {
            self.fail("Use an operation recipient query for unit/self area effects");
        }
        if let Some(radius) = &t.radius {
            self.amount(radius, "targeting.radius", 20);
        }
        if let Some(channel) = &a.cast.channel {
            self.amount(&channel.waves, "cast.channel.waves", 64);
            self.amount(&channel.interval_ticks, "cast.channel.intervalTicks", 400);
            for rank in &a.ranks {
                for amount in [&channel.waves, &channel.interval_ticks] {
                    if amount.at(rank).is_some_and(|v| v < 1) {
                        self.fail("Channel waves and interval must be positive");
                    }
                }
            }
        }
        if let Some(radius) = &t.radius
            && self.below_one(radius)
        {
            self.fail("Area radius must be positive");
        }
    }

    /// `record` names are unique per program and `result` scales use earlier records.
    fn records(&mut self) {
        let a = self.ability;
        let programs = std::iter::once(&a.on_release)
            .chain(a.persistent.iter().filter_map(|p| p.pulse.as_ref()))
            .chain(a.triggers.iter().flatten().map(|t| &t.operations));
        for program in programs {
            let mut records = BTreeSet::new();
            for e in program.iter().flat_map(Operation::effects) {
                if let Some(scale) = e.scale() {
                    self.amount(scale.permille(), "scale.permille", 10_000);
                    use super::operations::AmountScale;
                    if matches!(scale, AmountScale::Target { .. })
                        && a.targeting.kind == TargetingKind::Point
                    {
                        self.fail(
                            "Point aims have no resource stats; use recipient for area operations",
                        );
                    }
                    if let AmountScale::Result { id, .. } = scale
                        && !records.contains(id.as_str())
                    {
                        self.fail(
                            "Result scaling must reference an earlier record in the same program",
                        );
                    }
                }
                if let Some(record) = e.record()
                    && !records.insert(record)
                {
                    self.fail("Result record names must be unique within each program");
                }
            }
        }
    }

    fn unique_ids(&mut self) {
        let effects = self.ability.all_effects();
        let ids = |kind: &str| -> Vec<&str> {
            effects
                .iter()
                .filter_map(|entry| match (kind, *entry) {
                    ("contain", StatusOrEffect::Effect(Effect::Contain(e))) => Some(e.id.as_str()),
                    ("revive", StatusOrEffect::Effect(Effect::Revive(e))) => Some(e.id.as_str()),
                    ("vision", StatusOrEffect::Effect(Effect::Vision(e))) => Some(e.id.as_str()),
                    ("status", entry) => entry.status().map(|s| s.id.as_str()),
                    _ => None,
                })
                .collect()
        };
        for (kind, message) in [
            (
                "contain",
                "Containment IDs must be unique within an ability",
            ),
            ("revive", "Revival IDs must be unique within an ability"),
            ("vision", "Vision IDs must be unique within an ability"),
            ("status", "Status IDs must be unique within an ability"),
        ] {
            if has_duplicates(&ids(kind)) {
                self.fail(message);
            }
        }
    }

    fn effect(&mut self, entry: StatusOrEffect<'a>) {
        let a = self.ability;
        let (target, query, filter) = match entry {
            StatusOrEffect::Template(s) => {
                (Anchor::from(s.target), s.query.as_ref(), s.filter.as_ref())
            }
            StatusOrEffect::Effect(e) => (e.target(), e.query(), e.filter()),
        };
        self.filter(filter);
        if let Some(query) = query {
            self.amount(&query.radius, "onRelease.query.radius", 32);
            if self.below_one(&query.radius) {
                self.fail("Recipient query radius must be positive");
            }
            if target == Anchor::Point {
                self.fail("Point operations run once and cannot declare a unit query");
            }
            if query.relations.is_empty() || query.max_targets == 0 {
                self.fail("Recipient queries need relationships and a positive maxTargets");
            }
        }
        if let Some(status) = entry.status() {
            self.status(status);
        }
        let Some(effect) = entry.effect() else {
            return;
        };
        match effect {
            Effect::Summon(e) => {
                if let Some(max) = &e.max_active {
                    self.amount(max, "onRelease.maxActive", 128);
                    if self.below_one(max) {
                        self.fail("Summon maximum active count must be positive");
                    }
                }
                self.amount(&e.amount, "onRelease", 8);
                if let Some(duration) = &e.duration_ticks {
                    self.amount(duration, "onRelease.durationTicks", MAX_TICKS);
                    if self.below_one(duration) {
                        self.fail("Summon duration must be positive");
                    }
                }
                self.at_least(e.radius, 1.0, "Summon radius");
                if let Some(corpses) = &e.corpses {
                    self.amount(&corpses.radius, "onRelease.corpses.radius", 32);
                    self.amount(&corpses.max_targets, "onRelease.corpses.maxTargets", 8);
                    self.filter(corpses.filter.as_ref());
                    let bad_rank = a.ranks.iter().any(|r| {
                        let (amount, radius, max) = (
                            e.amount.at(r),
                            corpses.radius.at(r),
                            corpses.max_targets.at(r),
                        );
                        [amount, radius, max]
                            .iter()
                            .any(|v| v.is_some_and(|v| v < 1))
                            || amount.zip(max).is_some_and(|(n, m)| n * m > 8)
                    });
                    if e.query.is_some()
                        || e.target == Anchor::Target
                        || corpses.relations.is_empty()
                        || bad_rank
                    {
                        self.fail("Corpse summons require a caster/point anchor, no live query, positive count/radius/limit and at most eight units per operation");
                    }
                }
                if let super::RankedReference::ByRank(ids) = &e.definition
                    && ids.len() != a.ranks.len()
                {
                    self.fail("Summon definitions must cover every ability rank exactly");
                }
                self.grants(e.grants.as_deref());
            }
            Effect::Resurrect(e) => {
                self.grants(e.grants.as_deref());
                if let Some(duration) = &e.duration_ticks {
                    self.amount(duration, "onRelease.durationTicks", MAX_TICKS);
                    if self.below_one(duration) {
                        self.fail("Temporary resurrection duration must be positive");
                    }
                }
                self.amount(&e.amount, "onRelease.amount", 8);
                self.amount(&e.corpses.radius, "onRelease.corpses.radius", 32);
                self.amount(&e.health_permille, "onRelease.healthPermille", 1000);
                self.amount(&e.mana_permille, "onRelease.manaPermille", 1000);
                self.filter(e.corpses.filter.as_ref());
                if e.query.is_some()
                    || e.corpses.relations.is_empty()
                    || self.below_one(&e.amount)
                    || self.below_one(&e.corpses.radius)
                    || self.below_one(&e.health_permille)
                {
                    self.fail("Resurrection requires a corpse query with positive count, radius and health; live-unit queries are not supported");
                }
            }
            Effect::Split(e) => {
                self.amount(&e.amount, "split.amount", MAX_TICKS);
                for (key, value) in [
                    ("returnHealthPermille", &e.return_health_permille),
                    ("returnManaPermille", &e.return_mana_permille),
                ] {
                    if let Some(value) = value {
                        self.amount(value, &format!("split.{key}"), 1000);
                    }
                }
                let uncovered = e.members.iter().any(|m| match m {
                    super::RankedReference::ByRank(ids) => ids.len() != a.ranks.len(),
                    super::RankedReference::All(_) => false,
                });
                let bad_rank = a.ranks.iter().any(|r| {
                    e.amount.at(r).is_some_and(|v| v < 1)
                        || e.return_health_permille
                            .as_ref()
                            .is_some_and(|h| h.at(r).is_some_and(|v| v < 1))
                });
                if bad_rank || uncovered || e.members.len() < 2 || e.radius < 1 {
                    self.fail("Split requires positive lifetime/return health and member definitions for every rank");
                }
                let direct = matches!(
                    a.on_release.as_slice(),
                    [Operation::Effect(first)] if std::ptr::eq(first, effect)
                );
                if a.targeting.kind != TargetingKind::Caster
                    || a.activation == Activation::Passive
                    || a.delivery.is_some()
                    || a.persistent.is_some()
                    || a.cast.channel.is_some()
                    || !direct
                {
                    self.fail("Split is an atomic direct self-cast operation, not a repeated or triggered payload");
                }
            }
            Effect::Contain(e) => {
                if e.target == crate::conditions::Subject::Caster && e.query.is_none() {
                    self.fail("A carrier cannot contain itself; select other recipients");
                }
                self.amount(&e.amount, "contain.amount", MAX_TICKS);
                if let Some(digestion) = &e.digestion {
                    self.amount(&digestion.damage, "contain.digestion.damage", MILLION);
                    self.require(
                        digestion.interval_ticks >= 1,
                        "Digestion interval must be positive",
                    );
                }
                if self.below_one(&e.amount) {
                    self.fail("Containment duration must be positive");
                }
                self.require(e.capacity >= 1, "Containment capacity must be positive");
            }
            Effect::Revive(e) => {
                self.amount(&e.amount, "revive.amount", MAX_TICKS);
                self.amount(&e.health_permille, "revive.healthPermille", 1000);
                self.amount(&e.mana_permille, "revive.manaPermille", 1000);
                if e.query.is_some()
                    || self.below_one(&e.amount)
                    || self.below_one(&e.health_permille)
                {
                    self.fail("Hero revival requires positive delay/health and no recipient query");
                }
                let in_death_trigger = a.triggers.iter().flatten().any(|t| {
                    t.event == TriggerEvent::Death
                        && t.executor.is_none()
                        && t.operations
                            .iter()
                            .flat_map(Operation::effects)
                            .any(|o| std::ptr::eq(o, effect))
                });
                if !in_death_trigger {
                    self.fail("Revive must run on the fallen holder in a death trigger");
                }
            }
            Effect::Teleport(e) => {
                self.amount(&e.amount, "onRelease.amount", 12);
                if e.destination == Anchor::Point && a.targeting.kind != TargetingKind::Point {
                    self.fail("Point teleport destinations require point targeting");
                }
                if e.destination == Anchor::Target && a.targeting.kind != TargetingKind::Unit {
                    self.fail("Unit teleport destinations require a unit aim");
                }
            }
            Effect::Pull(e) => {
                self.amount(&e.amount, "onRelease.amount", 10_000);
                if e.toward == Anchor::Point && a.targeting.kind != TargetingKind::Point {
                    self.fail("Pulls toward a point require point targeting");
                }
                if e.toward == Anchor::Target && a.targeting.kind != TargetingKind::Unit {
                    self.fail("Pulls toward the target require a unit aim");
                }
                if !(0.0..=32.0).contains(&e.min_distance) {
                    self.fail("Pull minimum distance must be within 0–32");
                }
            }
            Effect::Vision(e) => {
                self.amount(&e.amount, "onRelease.amount", MAX_TICKS);
                self.amount(&e.radius, "onRelease.radius", 64);
                if e.query.is_some() {
                    self.fail("Vision sources are single anchors, not unit queries");
                }
                if self.below_one(&e.amount) || self.below_one(&e.radius) {
                    self.fail("Vision duration and radius must be positive");
                }
            }
            _ => {}
        }
    }

    /// Creation grants name declared target-only templates with independent lifetimes.
    fn grants(&mut self, grants: Option<&[String]>) {
        let Some(grants) = grants else {
            return;
        };
        if has_duplicates(grants) {
            self.fail("Duplicate creation grant");
        }
        for id in grants {
            if !self.independent_template(id) {
                self.fail("Creation grants must reference declared target-only status templates with independent lifetimes");
            }
        }
    }

    fn status(&mut self, e: &StatusEffect) {
        let a = self.ability;
        if let Some(bonus) = &e.source_attack_bonus {
            self.amount(bonus, "status.sourceAttackBonus", MILLION);
            if e.stacking.as_ref().map(|s| s.scope) != Some(StackScope::Source) {
                self.fail("Source attack bonuses require source-scoped stacking");
            }
        }
        if let Some(stacking) = &e.stacking {
            if a.aura.is_some()
                || e.lifetime != StatusLifetime::Duration
                || e.form.is_some()
                || e.periodic.is_some()
                || e.shield.is_some()
                || e.mana_shield.is_some()
                || e.combat_modifiers.is_some()
                || e.concealment.is_some()
            {
                self.fail("Stacking supports independent duration statuses with additive stat modifiers, not auras, forms, periodic damage or shields");
            }
            if !(2..=16).contains(&stacking.max) {
                self.fail("Stacking max must be between 2 and 16");
            }
            for rank in &a.ranks {
                let stacked = merged_modifiers(e, rank).into_iter().all(|(name, value)| {
                    value.is_some_and(|v| valid_modifier(name, v * f64::from(stacking.max)))
                });
                if !stacked {
                    self.fail("Maximum stacked modifiers exceed stat bounds");
                }
            }
        }
        if let Some(modifiers) = &e.combat_modifiers {
            self.combat(modifiers);
        }
        if let Some(taken) = &e.damage_taken_permille {
            if taken.is_empty() || taken.len() > 16 {
                self.fail("Use 1–16 damage types");
            }
            for (kind, value) in taken {
                self.amount(
                    value,
                    &format!("onRelease.damageTakenPermille.{kind}"),
                    5000,
                );
            }
        }
        if e.control_immunity
            .as_deref()
            .is_some_and(|c| c.is_empty() || has_duplicates(c))
        {
            self.fail("Duplicate control immunity");
        }
        if e.form.is_some() && a.aura.is_some() {
            self.fail("Forms require timed statuses, not continuously reapplied auras");
        }
        if let Some(form) = &e.form {
            if let Some(movement) = &form.movement {
                if movement.locomotion != crate::conditions::Locomotion::Air
                    && movement.flight_height.is_some()
                {
                    self.fail("Flight height requires air movement");
                }
                if let Some(height) = movement.flight_height {
                    self.at_least(height, 1.0, "Flight height");
                }
            }
            self.at_least(form.opacity, 0.1, "Form opacity");
            self.at_least(form.scale, 0.25, "Form scale");
        }
        if let Some(concealment) = &e.concealment {
            self.amount(
                &concealment.attack_bonus,
                "onRelease.concealment.attackBonus",
                MILLION,
            );
            if a.aura.is_some() {
                self.fail("Concealment requires a timed status, not a continuously reapplied aura");
            }
        }
        if let Some(barrier) = &e.barrier
            && (!(1.0..=16.0).contains(&barrier.radius)
                || !(10.0..=360.0).contains(&barrier.arc_degrees))
        {
            self.fail("A barrier needs a 1–16 radius and a 10–360° arc");
        }
        if let Some(radius) = &e.detection_radius {
            self.amount(radius, "onRelease.detectionRadius", 64);
            if self.below_one(radius) {
                self.fail("Detection radius must be positive");
            }
        }
        if e.lifetime == StatusLifetime::Instance && a.persistent.is_none() {
            self.fail("Instance-linked statuses require a persistent ability");
        }
        self.amount(&e.amount, "onRelease", MAX_TICKS);
        if let Some(duration) = &e.hero_duration {
            self.amount(duration, "onRelease", MAX_TICKS);
        }
        if let Some(periodic) = &e.periodic {
            self.amount(&periodic.damage, "onRelease", MILLION);
            self.require(
                periodic.interval_ticks >= 1,
                "Periodic interval must be positive",
            );
        }
        if let Some(shield) = &e.shield {
            self.amount(shield, "onRelease.shield", MILLION);
        }
        if let Some(mana_shield) = &e.mana_shield {
            self.at_least(
                mana_shield.damage_per_mana,
                1.0,
                "Mana shield damage per mana",
            );
            self.require(
                mana_shield.absorb_permille >= 1,
                "Mana shield absorption must be positive",
            );
        }
        if e.on_shield_depleted.is_some()
            && e.shield
                .as_ref()
                .is_none_or(|s| self.any_rank(s, |v| v <= 0))
        {
            self.fail("Shield exhaustion policy requires a positive finite shield at every rank");
        }
        if e.ranked_modifiers.is_some() {
            for rank in &a.ranks {
                let valid = merged_modifiers(e, rank)
                    .into_iter()
                    .all(|(name, value)| value.is_some_and(|v| valid_modifier(name, v)));
                if !valid {
                    self.fail("Ranked modifiers must resolve to valid shared modifiers");
                }
            }
        }
        let nonzero_amount = a
            .ranks
            .iter()
            .any(|r| e.amount.at(r).is_none_or(|v| v != 0));
        if e.lifetime == StatusLifetime::UntilDeath
            && (a.aura.is_some() || e.hero_duration.is_some() || nonzero_amount)
        {
            self.fail("Until-death statuses require amount 0, no hero duration and no aura");
        }
        if e.lifetime != StatusLifetime::UntilDeath && self.below_one(&e.amount) {
            self.fail("Status duration must be positive");
        }
    }

    fn activation(&mut self) {
        let a = self.ability;
        // An aura applies its release program only; triggers and status templates may do
        // anything (the old rule also scanned trigger operations, so an aura could not
        // carry a damage trigger).
        let aura_breaks = a.aura.is_some()
            && a.on_release
                .iter()
                .flat_map(Operation::effects)
                .any(|effect| match effect {
                    Effect::Status(e) => {
                        e.periodic.is_some()
                            || [
                                e.taunt,
                                e.stun,
                                e.disarm,
                                e.silence,
                                e.item_blocked,
                                e.break_on_damage,
                            ]
                            .contains(&Some(true))
                            || e.shield.is_some()
                            || e.immunity.is_some()
                            || e.spell_immunity.is_some()
                            || e.mana_shield.is_some()
                    }
                    _ => true,
                });
        let has_triggers = a.triggers.as_ref().is_some_and(|t| !t.is_empty());
        if a.activation == Activation::Passive
            && (a.targeting.kind != TargetingKind::Caster
                || (a.aura.is_none() && !has_triggers && a.combat_modifiers.is_none())
                || aura_breaks
                || a.delivery.is_some()
                || a.cast.channel.is_some()
                || a.autocast.is_some())
        {
            self.fail("Passives require a self aura, combat modifiers or triggers; auras only apply modifier statuses");
        }
        if a.activation != Activation::Passive && a.aura.is_some() {
            self.fail("Only passive abilities declare auras");
        }
    }

    fn delivery(&mut self) {
        let a = self.ability;
        let Some(delivery) = &a.delivery else {
            return;
        };
        let kind = a.targeting.kind;
        let mismatched = match delivery {
            Delivery::Line { .. } => kind != TargetingKind::Point,
            Delivery::Swarm { .. } => kind != TargetingKind::Caster,
            Delivery::Chain { .. } | Delivery::Projectile { .. } | Delivery::Charge { .. } => {
                kind != TargetingKind::Unit
            }
        };
        if a.cast.channel.is_some() || a.activation == Activation::Passive || mismatched {
            self.fail("Delivery does not match targeting");
        }
        match delivery {
            Delivery::Swarm {
                count,
                duration_ticks,
                return_threshold,
                launch_interval_ticks,
                ..
            } => {
                self.amount(count, "delivery.count", 32);
                self.amount(duration_ticks, "delivery.durationTicks", MAX_TICKS);
                self.amount(return_threshold, "delivery.returnThreshold", MILLION);
                let bad = a.ranks.iter().any(|r| {
                    let (count, duration, threshold) =
                        (count.at(r), duration_ticks.at(r), return_threshold.at(r));
                    count.is_some_and(|c| c < 1)
                        || duration.zip(count).is_some_and(|(d, c)| {
                            d < 1 + (c - 1) * i64::from(*launch_interval_ticks)
                        })
                        || threshold.is_some_and(|t| t < 1)
                });
                if bad {
                    self.fail("Swarm count and return threshold must be positive, and all seekers must launch before expiry");
                }
            }
            Delivery::Chain { bounces, .. } => {
                self.amount(bounces, "delivery.bounces", 16);
                if self.below_one(bounces) {
                    self.fail("A chain requires at least one hit");
                }
            }
            _ => {}
        }
    }

    fn persistent(&mut self) {
        let a = self.ability;
        let Some(p) = &a.persistent else {
            return;
        };
        self.amount(&p.duration_ticks, "persistent.durationTicks", MAX_TICKS);
        self.amount(&p.interval_ticks, "persistent.intervalTicks", 400);
        self.amount(&p.upkeep_mana, "persistent.upkeepMana", MILLION);
        let uncovered = a.ranks.iter().any(|r| {
            let (interval, duration) = (p.interval_ticks.at(r), p.duration_ticks.at(r));
            interval.is_some_and(|i| i < 1) || duration.zip(interval).is_some_and(|(d, i)| d < i)
        });
        if uncovered {
            self.fail("Persistent duration must cover at least one positive interval");
        }
        if a.activation == Activation::Passive || a.delivery.is_some() || a.cast.channel.is_some() {
            self.fail("Persistent spells use direct active release");
        }
        if p.toggle
            && (a.targeting.kind != TargetingKind::Caster
                || p.anchor != PersistentAnchor::Caster
                || !p.ends_with_caster)
        {
            self.fail("Toggles require a caster-anchored self spell that ends with the caster");
        }
        if (p.anchor == PersistentAnchor::Target && a.targeting.kind != TargetingKind::Unit)
            || (p.tether_range.is_some() && p.anchor != PersistentAnchor::Target)
        {
            self.fail("Target anchors and tethers require a unit target");
        }
        if p.toggle && p.delay_ticks > 0 {
            self.fail("Toggles start at once; a delay needs a timed instance");
        }
        if i64::from(p.delay_ticks) > MAX_TICKS {
            self.fail("Persistent delay is too long");
        }
        if let Some(pulse) = &p.pulse
            && (pulse.is_empty() || pulse.len() > 8)
        {
            self.fail("A pulse program has 1–8 operations");
        }
    }

    fn triggers(&mut self) {
        let a = self.ability;
        let status_ids: Vec<&str> = a
            .all_effects()
            .into_iter()
            .filter_map(StatusOrEffect::status)
            .map(|s| s.id.as_str())
            .collect();
        let mut ids = BTreeSet::new();
        for trigger in a.triggers.iter().flatten() {
            let event = trigger.event;
            if !ids.insert(trigger.id.as_str()) {
                self.fail("Duplicate trigger ID");
            }
            if let Some(status) = &trigger.while_status
                && !status_ids.contains(&status.as_str())
            {
                self.fail("Trigger references an unknown status");
            }
            if a.activation != Activation::Passive && trigger.while_status.is_none() {
                self.fail("Active ability triggers require an applied status");
            }
            if (event == TriggerEvent::Interval) != trigger.interval_ticks.is_some() {
                self.fail("Only interval triggers require intervalTicks");
            }
            if let Some(interval) = &trigger.interval_ticks {
                self.amount(interval, "triggers.intervalTicks", MAX_TICKS);
                if self.below_one(interval) {
                    self.fail("Trigger interval must be positive");
                }
            }
            self.amount(&trigger.mana_cost, "triggers.manaCost", MILLION);
            if let Some(cooldown) = &trigger.cooldown_ticks {
                self.amount(cooldown, "triggers.cooldownTicks", MAX_TICKS);
            }
            let costs_mana = a
                .ranks
                .iter()
                .any(|r| trigger.mana_cost.at(r).is_none_or(|v| v != 0));
            if trigger.executor.is_some()
                && (event != TriggerEvent::Death || trigger.while_status.is_none() || costs_mana)
            {
                self.fail("Status-source execution requires a death trigger, whileStatus and zero trigger mana cost");
            }
            let victim_event = matches!(event, TriggerEvent::Kill | TriggerEvent::WeaponRelease);
            if (trigger.filter.is_some() || trigger.include_buildings.is_some()) && !victim_event {
                self.fail("Trigger victim filters require kill or weaponRelease");
            }
            if trigger.weapon.is_some() && event != TriggerEvent::WeaponRelease {
                self.fail("Weapon-kind restrictions require weaponRelease");
            }
            if trigger.blocked_by_silence.is_some()
                && !matches!(event, TriggerEvent::WeaponRelease | TriggerEvent::Interval)
            {
                self.fail("Silence policy requires weaponRelease or interval");
            }
            if event == TriggerEvent::Interval && trigger.while_status.is_some() && a.aura.is_some()
            {
                self.fail("Interval grants require independent statuses, not continuously reapplied auras; use a passive interval with a recipient query");
            }
            for e in trigger.operations.iter().flat_map(Operation::effects) {
                if event == TriggerEvent::Kill
                    && e.target() == Anchor::Target
                    && e.query().is_none()
                {
                    self.fail("Kill operations require caster, point or queried recipients; the defeated victim is not alive");
                }
                if event == TriggerEvent::Death
                    && !matches!(e, Effect::Revive(_))
                    && e.target() != Anchor::Point
                    && e.query().is_none()
                    && !(trigger.executor.is_some() && e.target() == Anchor::Caster)
                {
                    self.fail("Death operations require a recipient query or point target; the dead holder is not a live recipient");
                }
                let max = if matches!(e, Effect::Summon(_)) {
                    8
                } else {
                    MILLION
                };
                self.amount(&e.amount(), "triggers.operations", max);
            }
        }
    }
}

/// A status's fixed modifiers with its ranked modifiers resolved at one rank; `None`
/// where a rank lacks the parameter.
fn merged_modifiers(
    status: &StatusEffect,
    rank: &RankParameters,
) -> BTreeMap<&'static str, Option<f64>> {
    let mut values: BTreeMap<&'static str, Option<f64>> = modifier_values(&status.modifiers)
        .into_iter()
        .map(|(name, value)| (name, Some(value)))
        .collect();
    for (modifier, amount) in status.ranked_modifiers.iter().flatten() {
        let value = amount.at(rank).map(|v| v as f64);
        values.insert(RankedModifier::name(*modifier), value);
    }
    values
}

/// The numeric modifiers that are set.
fn modifier_values(m: &ItemModifiers) -> Vec<(&'static str, f64)> {
    let unsigned = [
        ("maxHp", m.max_hp),
        ("maxMana", m.max_mana),
        ("cooldownReductionPermille", m.cooldown_reduction_permille),
        ("lifestealPermille", m.lifesteal_permille),
        ("meleeReflectionPermille", m.melee_reflection_permille),
    ]
    .into_iter()
    .filter_map(|(name, v)| v.map(|v| (name, f64::from(v))));
    let signed = [
        ("damage", m.damage),
        ("armor", m.armor),
        ("damagePermille", m.damage_permille),
        ("attackSpeedPermille", m.attack_speed_permille),
        ("moveSpeedPermille", m.move_speed_permille),
    ]
    .into_iter()
    .filter_map(|(name, v)| v.map(|v| (name, f64::from(v))));
    let rates = [
        ("healthRegenPerSecond", m.health_regen_per_second),
        ("manaRegenPerSecond", m.mana_regen_per_second),
    ]
    .into_iter()
    .filter_map(|(name, v)| v.map(|v| (name, v)));
    unsigned.chain(signed).chain(rates).collect()
}

/// Bounds of the shared item modifier vocabulary.
fn valid_modifier(name: &str, value: f64) -> bool {
    let int = |min: f64, max: f64| value.fract() == 0.0 && (min..=max).contains(&value);
    match name {
        "maxHp" | "maxMana" => int(0.0, f64::MAX),
        "damage" => int(-10_000.0, 10_000.0),
        "armor" => int(-1000.0, 1000.0),
        "healthRegenPerSecond" | "manaRegenPerSecond" => value >= 0.0 && value.is_finite(),
        "damagePermille" => int(-1000.0, 1000.0),
        "attackSpeedPermille" | "moveSpeedPermille" => int(-800.0, 1000.0),
        "cooldownReductionPermille" | "lifestealPermille" => int(0.0, 500.0),
        "meleeReflectionPermille" => int(0.0, 1000.0),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn bolt() -> Value {
        json!({"id": "ability.test.bolt", "name": "Bolt", "activation": "targeted",
            "ranks": [{"damage": 10}, {"damage": 20}],
            "targeting": {"kind": "unit", "alive": true, "visible": true, "relations": ["enemy"],
                "allowSelf": false, "range": 6, "outOfRange": "reject"},
            "cast": {"prepareTicks": 4, "recoverTicks": 4,
                "cost": {"resource": "mana", "amount": 10, "commit": "release"},
                "cooldown": {"ticks": 40, "start": "release", "scope": "ability"},
                "cancelBeforeRelease": "refund",
                "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
            "onRelease": [{"op": "damage", "target": "target", "amount": {"rankParameter": "damage"}, "damageType": "spell"}],
            "presentation": {
                "animations": {"prepare": "cast", "release": "cast", "recover": "idle", "fallback": "idle"},
                "effects": [{"id": "hit", "effect": "effect.test.spark", "event": "damaged", "anchor": "target"}]}})
    }

    fn problems(ability: Value) -> Vec<String> {
        let library = AbilityLibrary {
            abilities: vec![serde_json::from_value(ability).unwrap()],
        };
        let mut errors = Vec::new();
        check(&library, &BTreeMap::new(), &mut errors);
        errors.into_iter().map(|e| e.message).collect()
    }

    fn assert_only(ability: Value, expected: &str) {
        let found = problems(ability);
        assert!(
            !found.is_empty() && found.iter().all(|f| f.contains(expected)),
            "expected only {expected:?}, found {found:#?}"
        );
    }

    #[test]
    fn a_plain_spell_passes() {
        assert_eq!(problems(bolt()), Vec::<String>::new());
    }

    #[test]
    fn rank_parameters() {
        let mut ability = bolt();
        ability["ranks"] = json!([{"damage": 10}, {"power": 20}]);
        let found = problems(ability);
        assert!(found.contains(&"Every rank must declare the same parameters".into()));
        assert!(found.contains(&"onRelease[0].amount: Rank 2: missing parameter damage".into()));

        let mut ability = bolt();
        ability["ranks"] = json!([{"damage": -5}]);
        assert_only(ability, "Rank 1: value must be between 0 and 1000000");
    }

    #[test]
    fn status_bound_effects() {
        let mut status_bound = bolt();
        status_bound["presentation"]["effects"][0]["lifetime"] = json!("status");
        let found = problems(status_bound);
        assert_eq!(
            found,
            ["hit: Status lifetime requires target statusApplied binding"]
        );
    }

    #[test]
    fn weapon_casts_have_no_release_program() {
        let mut ability = bolt();
        ability["weaponCast"] = json!({"bonus": 10, "weapon": "melee"});
        ability["cast"]["prepareTicks"] = json!(0);
        ability["cast"]["recoverTicks"] = json!(0);
        assert_only(ability, "Weapon casts require a targeted enemy unit");
    }

    #[test]
    fn ground_targeting_needs_a_radius() {
        let mut ability = bolt();
        ability["targeting"]["kind"] = json!("point");
        ability["onRelease"][0]["target"] = json!("caster");
        assert_only(ability, "Ground targeting requires an area radius");
    }

    #[test]
    fn result_scaling_reads_earlier_records() {
        let mut ability = bolt();
        ability["onRelease"] = json!([
            {"op": "heal", "target": "caster", "amount": 0,
                "scale": {"source": "result", "id": "dealt", "stat": "applied", "permille": 500}},
            {"op": "damage", "target": "target", "amount": 5, "damageType": "spell", "record": "dealt"}
        ]);
        assert_only(
            ability.clone(),
            "Result scaling must reference an earlier record in the same program",
        );
        let ops = ability["onRelease"].as_array().unwrap().clone();
        ability["onRelease"] = json!([ops[1], ops[0]]);
        assert_eq!(problems(ability), Vec::<String>::new());
    }

    #[test]
    fn split_is_the_whole_direct_release() {
        let split = json!({"op": "split", "id": "parts", "target": "caster", "amount": 100,
            "members": ["unit.test.a", "unit.test.b"], "radius": 2});
        let mut ability = bolt();
        ability["targeting"]["kind"] = json!("self");
        ability["targeting"]["relations"] = json!(["ally"]);
        ability["onRelease"] = json!([split]);
        assert_eq!(problems(ability.clone()), Vec::<String>::new());
        ability["onRelease"] = json!([split, {"op": "mana", "target": "caster", "amount": 1}]);
        assert_only(ability, "Split is an atomic direct self-cast operation");
    }

    #[test]
    fn revive_runs_from_a_death_trigger() {
        let revive = json!({"op": "revive", "id": "again", "target": "caster", "amount": 80,
            "healthPermille": 500, "manaPermille": 0, "placementRadius": 2});
        let mut ability = bolt();
        ability["activation"] = json!("passive");
        ability["targeting"]["kind"] = json!("self");
        ability["targeting"]["relations"] = json!(["ally"]);
        ability["onRelease"] = json!([]);
        ability["triggers"] = json!([{"id": "fall", "event": "death", "operations": [revive]}]);
        assert_eq!(problems(ability.clone()), Vec::<String>::new());
        ability["triggers"][0]["event"] = json!("damaged");
        assert_only(
            ability,
            "Revive must run on the fallen holder in a death trigger",
        );
    }

    #[test]
    fn kill_triggers_cannot_target_the_victim() {
        let mut ability = bolt();
        ability["activation"] = json!("passive");
        ability["targeting"]["kind"] = json!("self");
        ability["targeting"]["relations"] = json!(["ally"]);
        ability["onRelease"] = json!([]);
        ability["triggers"] = json!([{"id": "reap", "event": "kill",
            "operations": [{"op": "heal", "target": "target", "amount": 5}]}]);
        assert_only(
            ability,
            "Kill operations require caster, point or queried recipients",
        );
    }

    #[test]
    fn status_lifetimes_and_stacks() {
        let status = json!({"op": "status", "target": "target", "id": "hex", "amount": 0,
            "polarity": "negative", "dispel": true, "lifetime": "untilDeath"});
        let mut ability = bolt();
        ability["onRelease"] = json!([status]);
        assert_eq!(problems(ability.clone()), Vec::<String>::new());
        ability["onRelease"][0]["amount"] = json!(5);
        assert_only(ability, "Until-death statuses require amount 0");

        let mut ability = bolt();
        ability["onRelease"] = json!([{"op": "status", "target": "target", "id": "sap", "amount": 40,
            "polarity": "negative", "dispel": true, "modifiers": {"damagePermille": -600},
            "stacking": {"max": 2, "scope": "ability"}}]);
        assert_only(ability, "Maximum stacked modifiers exceed stat bounds");
    }

    #[test]
    fn auras_apply_only_modifier_statuses() {
        let mut ability = bolt();
        ability["activation"] = json!("passive");
        ability["aura"] = json!({"radius": 6, "meleeOnly": false});
        ability["targeting"]["kind"] = json!("self");
        ability["targeting"]["relations"] = json!(["ally"]);
        ability["onRelease"] = json!([{"op": "status", "target": "target", "id": "might", "amount": 10,
            "polarity": "positive", "dispel": false, "modifiers": {"armor": 2},
            "query": {"center": "caster", "radius": 6, "relations": ["ally"]}}]);
        assert_eq!(problems(ability.clone()), Vec::<String>::new());
        ability["onRelease"][0]["stun"] = json!(true);
        assert_only(
            ability,
            "Passives require a self aura, combat modifiers or triggers",
        );
    }
}
