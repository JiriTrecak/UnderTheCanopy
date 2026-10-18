//! Snapshots of units as abilities see them (old `AbilityActor`, `gameHost.get`,
//! `sourceActor`, `spellSource`), relations between them, and the trait filters and
//! conditions of the ability format (old `eligibility.ts`, `content/abilities/conditions.ts`).

use super::state::Source;
use crate::{
    catalog::{Catalog, Def},
    combat::Senses,
    entity::{Entity, EntityId, Milli},
    state::State,
    stats::Stats,
};
use canopy_content::{
    abilities::SpellImmunity,
    conditions::{
        AbilityCondition, Comparison, ConditionLeaf, Controller, Locomotion, Measure, PoolResource,
        Relation, Subject, TargetFilter, UnitNature,
    },
    game::Kind,
    placement::Owner,
};

/// How a unit stands to an ability's caster (old `Relation`): neutral units are never
/// eligible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Ally,
    Enemy,
    Neutral,
}

impl Side {
    pub fn is(self, relations: &[Relation]) -> bool {
        match self {
            Side::Ally => relations.contains(&Relation::Ally),
            Side::Enemy => relations.contains(&Relation::Enemy),
            Side::Neutral => false,
        }
    }
}

/// A unit as abilities see it at one moment.
#[derive(Debug, Clone, PartialEq)]
pub struct Actor {
    pub id: EntityId,
    pub definition: Def,
    pub owner: Owner,
    pub camp: Option<String>,
    pub position: Milli,
    pub surface: Option<String>,
    /// Eye height above the floor (flight, lookout), centimetres.
    pub elevation: i32,
    pub hp: u32,
    pub max_hp: u32,
    pub mana: u32,
    pub max_mana: u32,
    pub alive: bool,
    pub unit: bool,
    pub hero: bool,
    pub summoned: bool,
    pub level: u32,
    pub air: bool,
    pub nature: Option<UnitNature>,
    pub spell_immunity: Option<SpellImmunity>,
    pub control_immunity: u8,
    pub melee: bool,
    /// Cannot cast: stunned, silenced, taunted or held.
    pub blocked: bool,
    pub targetable: bool,
    /// The summon that made it: (source, ability).
    pub summon_origin: Option<(EntityId, super::library::AbilityId)>,
    /// A restored snapshot of something no longer in the world.
    pub restored: bool,
}

impl Actor {
    pub fn x(&self) -> f64 {
        f64::from(self.position.x) / 1000.0
    }

    pub fn y(&self) -> f64 {
        f64::from(self.position.y) / 1000.0
    }

    /// Squared distance in milli-cells.
    pub fn distance2(&self, p: Milli) -> i64 {
        distance2(self.position, p)
    }
}

pub fn distance2(a: Milli, b: Milli) -> i64 {
    let (dx, dy) = (i64::from(a.x - b.x), i64::from(a.y - b.y));
    dx * dx + dy * dy
}

/// Whether `a` is within `radius` cells of `b` (`radius` in cells, exact).
pub fn within(a: Milli, b: Milli, radius_milli: i64) -> bool {
    distance2(a, b) <= radius_milli * radius_milli
}

pub fn alive(e: &Entity) -> bool {
    e.hp.is_none_or(|hp| hp > 0) && !e.fallen
}

/// Whether the unit can be targeted: not held, garrisoned or waiting for release.
pub fn targetable(e: &Entity) -> bool {
    e.unit
        .as_ref()
        .is_none_or(|u| u.contained.is_none() && u.garrison.is_none() && u.release.is_none())
}

pub fn summoned(e: &Entity) -> bool {
    e.links.as_ref().is_some_and(|l| l.summoned.is_some())
}

pub fn nature(catalog: &Catalog, def: Def) -> Option<UnitNature> {
    let d = catalog.get(def);
    (d.kind == Kind::Unit).then(|| d.unit_nature.unwrap_or(UnitNature::Organic))
}

pub fn camp(e: &Entity) -> Option<&str> {
    e.unit.as_ref()?.camp.as_ref().map(|c| c.id.as_str())
}

/// The snapshot of a live entity (old `gameHost.get`).
pub fn of(senses: &Senses, state: &State, e: &Entity) -> Actor {
    let catalog = senses.catalog;
    let immune = super::statuses::control_immunities(catalog, e);
    let stats = Stats::of_with(catalog, e, crate::stats::research(state, e.owner), immune);
    let d = catalog.get(e.definition);
    let profile = super::forms::weapon_def(catalog, e);
    let combat = catalog.get(profile).behaviors.combat.as_ref();
    let u = e.unit.as_ref();
    let held =
        u.is_some_and(|u| u.contained.is_some() || u.garrison.is_some() || u.release.is_some());
    let blocked = u.is_some_and(|u| u.taunt.is_some())
        || super::statuses::spell_control_with(catalog, e, immune, super::library::control::STUN)
        || super::statuses::spell_control_with(
            catalog,
            e,
            immune,
            super::library::control::SILENCE,
        )
        || held;
    let sight = senses.sight(state);
    let point = sight.sight_point(e);
    Actor {
        id: e.id,
        definition: e.definition,
        owner: e.owner,
        camp: camp(e).map(str::to_string),
        position: e.position,
        surface: e.surface.clone(),
        elevation: crate::terrain::round_half_up(point.elevation * 100.0) as i32,
        hp: e.hp.unwrap_or(0),
        max_hp: stats.max_hp,
        mana: e.caster.as_ref().map_or(0, |c| c.mana),
        max_mana: stats.max_mana,
        alive: alive(e),
        unit: e.unit.is_some(),
        hero: d.is_hero(),
        summoned: summoned(e),
        level: stats.level,
        air: super::forms::airborne(catalog, e),
        nature: nature(catalog, e.definition),
        spell_immunity: super::statuses::spell_immunity(catalog, e, None),
        control_immunity: immune,
        melee: combat.is_some_and(|c| c.projectile.is_none() && c.shell.is_none()),
        blocked,
        targetable: !held,
        summon_origin: e
            .links
            .as_ref()
            .and_then(|l| l.summoned.as_ref())
            .map(|s| (s.source, s.ability)),
        restored: false,
    }
}

/// The snapshot of an entity by id, if it exists.
pub fn get(senses: &Senses, state: &State, id: EntityId) -> Option<Actor> {
    state.entities.get(id).map(|e| of(senses, state, e))
}

/// What a unit was, captured for work that outlives it (old `spellSource`).
pub fn source(senses: &Senses, state: &State, e: &Entity) -> Source {
    let a = of(senses, state, e);
    Source {
        source: e.id,
        definition: e.definition,
        owner: e.owner,
        position: e.position,
        surface: e.surface.clone(),
        elevation: a.elevation,
        air: a.air,
        camp: a.camp.clone(),
        level: a.level,
        summoned: a.summoned,
        hp: a.hp.min(a.max_hp),
        max_hp: a.max_hp,
        mana: a.mana.min(a.max_mana),
        max_mana: a.max_mana,
    }
}

/// A snapshot standing in for a unit that is gone (old `sourceActor`).
pub fn restore(catalog: &Catalog, s: &Source) -> Actor {
    let d = catalog.get(s.definition);
    Actor {
        id: s.source,
        definition: s.definition,
        owner: s.owner,
        camp: s.camp.clone(),
        position: s.position,
        surface: s.surface.clone(),
        elevation: s.elevation,
        hp: s.hp,
        max_hp: s.max_hp,
        mana: s.mana,
        max_mana: s.max_mana,
        alive: false,
        unit: d.kind == Kind::Unit,
        hero: d.is_hero(),
        summoned: s.summoned,
        level: s.level,
        air: s.air,
        nature: nature(catalog, s.definition),
        spell_immunity: None,
        control_immunity: 0,
        melee: false,
        blocked: true,
        targetable: false,
        summon_origin: None,
        restored: true,
    }
}

/// A last-resort snapshot of an unknown source at a point.
pub fn placeholder(id: EntityId, owner: Owner, definition: Def, at: Milli) -> Actor {
    Actor {
        id,
        definition,
        owner,
        camp: None,
        position: at,
        surface: None,
        elevation: 0,
        hp: 0,
        max_hp: 0,
        mana: 0,
        max_mana: 0,
        alive: false,
        unit: true,
        hero: false,
        summoned: false,
        level: 1,
        air: false,
        nature: None,
        spell_immunity: None,
        control_immunity: 0,
        melee: false,
        blocked: true,
        targetable: false,
        summon_origin: None,
        restored: true,
    }
}

/// How `b` stands to `a` (old `gameHost.relation`).
pub fn relation(senses: &Senses, a: &Actor, b: &Actor) -> Side {
    relation_of(
        senses,
        a.id,
        a.owner,
        a.camp.as_deref(),
        b.id,
        b.owner,
        b.camp.as_deref(),
    )
}

pub fn relation_of(
    senses: &Senses,
    a_id: EntityId,
    a: Owner,
    a_camp: Option<&str>,
    b_id: EntityId,
    b: Owner,
    b_camp: Option<&str>,
) -> Side {
    if a_id == b_id
        || senses.allied(a, b)
        || (a == Owner::Nobody && b == Owner::Nobody && a_camp.is_some() && a_camp == b_camp)
    {
        return Side::Ally;
    }
    if senses.opponents(a, a_camp, b, b_camp) {
        Side::Enemy
    } else {
        Side::Neutral
    }
}

/// Traits a filter or condition reads.
pub trait Traits {
    fn hp(&self) -> Option<u32>;
    fn max_hp(&self) -> Option<u32>;
    fn mana(&self) -> Option<u32>;
    fn max_mana(&self) -> Option<u32>;
    fn owner(&self) -> Option<Owner>;
    fn hero(&self) -> bool;
    fn summoned(&self) -> bool;
    fn level(&self) -> u32;
    fn nature(&self) -> Option<UnitNature>;
    fn locomotion(&self) -> Option<Locomotion>;
    /// A structure rather than a unit.
    fn building(&self) -> bool {
        false
    }
}

impl Traits for Actor {
    fn building(&self) -> bool {
        !self.unit
    }
    fn hp(&self) -> Option<u32> {
        Some(self.hp)
    }
    fn max_hp(&self) -> Option<u32> {
        Some(self.max_hp)
    }
    fn mana(&self) -> Option<u32> {
        Some(self.mana)
    }
    fn max_mana(&self) -> Option<u32> {
        Some(self.max_mana)
    }
    fn owner(&self) -> Option<Owner> {
        Some(self.owner)
    }
    fn hero(&self) -> bool {
        self.hero
    }
    fn summoned(&self) -> bool {
        self.summoned
    }
    fn level(&self) -> u32 {
        self.level
    }
    fn nature(&self) -> Option<UnitNature> {
        self.nature
    }
    fn locomotion(&self) -> Option<Locomotion> {
        Some(if self.air {
            Locomotion::Air
        } else {
            Locomotion::Ground
        })
    }
}

/// No traits at all (an absent target).
pub struct Nothing;

impl Traits for Nothing {
    fn hp(&self) -> Option<u32> {
        None
    }
    fn max_hp(&self) -> Option<u32> {
        None
    }
    fn mana(&self) -> Option<u32> {
        None
    }
    fn max_mana(&self) -> Option<u32> {
        None
    }
    fn owner(&self) -> Option<Owner> {
        None
    }
    fn hero(&self) -> bool {
        false
    }
    fn summoned(&self) -> bool {
        false
    }
    fn level(&self) -> u32 {
        1
    }
    fn nature(&self) -> Option<UnitNature> {
        None
    }
    fn locomotion(&self) -> Option<Locomotion> {
        None
    }
}

/// Traits of a corpse or an observed trait set without pools.
#[derive(Debug, Clone)]
pub struct Plain {
    pub hero: bool,
    pub summoned: bool,
    pub level: u32,
    pub nature: Option<UnitNature>,
    pub locomotion: Option<Locomotion>,
    pub building: bool,
}

impl Traits for Plain {
    fn building(&self) -> bool {
        self.building
    }
    fn hp(&self) -> Option<u32> {
        None
    }
    fn max_hp(&self) -> Option<u32> {
        None
    }
    fn mana(&self) -> Option<u32> {
        None
    }
    fn max_mana(&self) -> Option<u32> {
        None
    }
    fn owner(&self) -> Option<Owner> {
        None
    }
    fn hero(&self) -> bool {
        self.hero
    }
    fn summoned(&self) -> bool {
        self.summoned
    }
    fn level(&self) -> u32 {
        self.level
    }
    fn nature(&self) -> Option<UnitNature> {
        self.nature
    }
    fn locomotion(&self) -> Option<Locomotion> {
        self.locomotion
    }
}

/// Traits of a live entity for filters (old trait objects built in reactions, cleave,
/// weapon enhancement).
pub fn plain(catalog: &Catalog, e: &Entity) -> Plain {
    let d = catalog.get(e.definition);
    Plain {
        hero: d.is_hero(),
        summoned: summoned(e),
        level: crate::stats::level(catalog, e),
        nature: nature(catalog, e.definition),
        locomotion: Some(if super::forms::airborne(catalog, e) {
            Locomotion::Air
        } else {
            Locomotion::Ground
        }),
        building: d.kind == canopy_content::game::Kind::Building,
    }
}

/// Every present criterion holds (old `matchesTargetFilter`).
pub fn matches(t: &dyn Traits, f: Option<&TargetFilter>) -> bool {
    let Some(f) = f else {
        return true;
    };
    f.locomotion
        .as_ref()
        .is_none_or(|l| t.locomotion().is_some_and(|x| l.contains(&x)))
        && f.heroes.is_none_or(|h| t.hero() == h)
        && f.summoned.is_none_or(|s| t.summoned() == s)
        && f.natures
            .as_ref()
            .is_none_or(|n| t.nature().is_some_and(|x| n.contains(&x)))
        && f.min_level.is_none_or(|m| t.level() >= m)
        && f.max_level.is_none_or(|m| t.level() <= m)
        && f.buildings.is_none_or(|b| t.building() == b)
}

/// Evaluates a condition (old `matchesAbilityCondition`).
pub fn condition(
    c: &AbilityCondition,
    side: Side,
    caster: &dyn Traits,
    target: &dyn Traits,
) -> bool {
    let leaf = |l: &ConditionLeaf| match l {
        ConditionLeaf::Resource(r) => resource(r, caster, target),
        ConditionLeaf::Controller(r) => controller(r.of, r.is, caster, target),
        ConditionLeaf::Relation(r) => relation_is(r.is, side),
        ConditionLeaf::Matches(m) => matches_condition(m, side, caster, target),
    };
    match c {
        AbilityCondition::Resource(r) => resource(r, caster, target),
        AbilityCondition::Controller(r) => controller(r.of, r.is, caster, target),
        AbilityCondition::Relation(r) => relation_is(r.is, side),
        AbilityCondition::Matches(m) => matches_condition(m, side, caster, target),
        AbilityCondition::All(g) => g.conditions.iter().all(leaf),
        AbilityCondition::Any(g) => g.conditions.iter().any(leaf),
    }
}

fn relation_is(is: Relation, side: Side) -> bool {
    matches!(
        (is, side),
        (Relation::Ally, Side::Ally) | (Relation::Enemy, Side::Enemy)
    )
}

fn subject<'a>(of: Subject, caster: &'a dyn Traits, target: &'a dyn Traits) -> &'a dyn Traits {
    match of {
        Subject::Caster => caster,
        Subject::Target => target,
    }
}

fn matches_condition(
    m: &canopy_content::conditions::MatchesCondition,
    side: Side,
    caster: &dyn Traits,
    target: &dyn Traits,
) -> bool {
    let side = if m.of == Subject::Caster {
        Side::Ally
    } else {
        side
    };
    m.relations.as_ref().is_none_or(|r| side.is(r))
        && matches(subject(m.of, caster, target), Some(&m.filter))
}

fn controller(of: Subject, is: Controller, caster: &dyn Traits, target: &dyn Traits) -> bool {
    let owner = subject(of, caster, target).owner();
    match is {
        Controller::Neutral => owner == Some(Owner::Nobody),
        Controller::Player => owner.is_some_and(|o| o != Owner::Nobody),
    }
}

fn resource(
    r: &canopy_content::conditions::ResourceCondition,
    caster: &dyn Traits,
    target: &dyn Traits,
) -> bool {
    let actor = subject(r.of, caster, target);
    let (current, maximum) = match r.resource {
        PoolResource::Health => (actor.hp(), actor.max_hp()),
        PoolResource::Mana => (actor.mana(), actor.max_mana()),
    };
    let Some(current) = current.map(i64::from) else {
        return false;
    };
    let maximum = maximum.map(i64::from);
    if r.measure != Measure::Current && maximum.is_none_or(|m| m <= 0) {
        return false;
    }
    let value = i64::from(r.value);
    let (left, right) = match r.measure {
        Measure::Current => (current, value),
        Measure::Missing => ((maximum.unwrap_or(0) - current).max(0), value),
        Measure::FractionPermille => (current * 1000, value * maximum.unwrap_or(0)),
    };
    match r.comparison {
        Comparison::Lt => left < right,
        Comparison::Lte => left <= right,
        Comparison::Eq => left == right,
        Comparison::Gte => left >= right,
        Comparison::Gt => left > right,
    }
}
