//! Corpses as abilities see them (old `corpses.ts`): visible bodies with their relation to
//! the caster, the shared selector, consumption, and resurrection.

use super::{
    Ctx,
    actor::{self, Actor, Side},
    library::{self, AbilityId},
    state::Summoned,
};
use crate::{
    catalog::{Catalog, Def},
    combat::Corpse,
    entity::{EntityId, Milli},
    spatial::Spot,
    state::State,
};
use canopy_content::{
    abilities::{CorpseOrder, CorpseRelation, Ownership, Resurrect, SupplyPolicy},
    conditions::{Locomotion, TargetFilter},
    placement::Owner,
};

/// A corpse with what selectors read (old `CorpseView`).
#[derive(Debug, Clone)]
pub struct CorpseView {
    pub id: EntityId,
    pub definition: Def,
    pub owner: Owner,
    pub position: Milli,
    pub level: u32,
    pub max_hp: u32,
    pub relation: CorpseRelation,
    pub traits: actor::Plain,
}

/// Level and maximum health a corpse had (old `entityStats` on the record).
fn level_and_health(catalog: &Catalog, c: &Corpse) -> (u32, u32) {
    let d = catalog.get(c.definition);
    let experience = c.progression.as_ref().map_or(0, |p| p.experience);
    match d.behaviors.progression.as_ref().map(|p| &p.levels) {
        Some(levels) if !levels.is_empty() => {
            let reached = levels
                .iter()
                .filter(|l| l.experience <= experience)
                .count()
                .max(1);
            (reached as u32, levels[reached - 1].max_hp)
        }
        _ => (
            d.level.unwrap_or(1),
            d.body.as_ref().map_or(0, |b| b.max_hp),
        ),
    }
}

/// Live corpses `caster` sees, with their relation to it (old `host.corpses`).
pub fn visible(ctx: &Ctx, state: &State, caster: &Actor) -> Vec<CorpseView> {
    let catalog = ctx.catalog();
    let tick = state.tick;
    state
        .combat
        .corpses
        .iter()
        .filter(|c| c.expires > tick)
        .filter_map(|c| {
            let position = Milli {
                x: c.position.x * 1000,
                y: c.position.y * 1000,
            };
            if !super::visible_point(ctx, state, caster.owner, position, Some(caster)) {
                return None;
            }
            let (level, max_hp) = level_and_health(catalog, c);
            let side = actor::relation_of(
                &ctx.senses,
                caster.id,
                caster.owner,
                caster.camp.as_deref(),
                c.id,
                c.owner,
                c.camp.as_deref(),
            );
            let air = catalog
                .get(c.definition)
                .behaviors
                .movement
                .as_ref()
                .and_then(|m| m.locomotion)
                == Some(Locomotion::Air);
            Some(CorpseView {
                id: c.id,
                definition: c.definition,
                owner: c.owner,
                position,
                level,
                max_hp,
                relation: match side {
                    Side::Ally => CorpseRelation::Ally,
                    Side::Enemy => CorpseRelation::Enemy,
                    Side::Neutral => CorpseRelation::Neutral,
                },
                traits: actor::Plain {
                    hero: false,
                    summoned: false,
                    level,
                    nature: actor::nature(catalog, c.definition),
                    locomotion: Some(if air {
                        Locomotion::Air
                    } else {
                        Locomotion::Ground
                    }),
                },
            })
        })
        .collect()
}

/// Corpses a query takes, best first (old `selectCorpses`).
pub fn select<'a>(
    corpses: &'a [CorpseView],
    radius: i64,
    relations: &[CorpseRelation],
    filter: Option<&TargetFilter>,
    order: CorpseOrder,
    center: Milli,
) -> Vec<&'a CorpseView> {
    let r = radius.max(0) * 1000;
    let mut chosen: Vec<&CorpseView> = corpses
        .iter()
        .filter(|c| {
            relations.contains(&c.relation)
                && actor::matches(&c.traits, filter)
                && actor::within(c.position, center, r)
        })
        .collect();
    chosen.sort_by(|a, b| {
        let strength = if order == CorpseOrder::Strongest {
            b.level.cmp(&a.level).then(b.max_hp.cmp(&a.max_hp))
        } else {
            std::cmp::Ordering::Equal
        };
        strength
            .then(actor::distance2(a.position, center).cmp(&actor::distance2(b.position, center)))
            .then(a.id.cmp(&b.id))
    });
    chosen
}

/// Takes a corpse out of the world (old `consume`).
pub fn consume(state: &mut State, id: EntityId) -> Option<Corpse> {
    let tick = state.tick;
    let i = state
        .combat
        .corpses
        .iter()
        .position(|c| c.id == id && c.expires > tick)?;
    Some(state.combat.corpses.remove(i))
}

/// Raises corpses near `point` (old `SpellCorpses.resurrect`); returns the new units.
#[allow(clippy::too_many_arguments)]
pub fn resurrect(
    ctx: &mut Ctx,
    state: &mut State,
    caster: &Actor,
    ability: AbilityId,
    rank: u8,
    cast: u32,
    op: &Resurrect,
    point: Milli,
) -> Vec<EntityId> {
    let spell = ctx.ability(ability);
    let amount = library::value(spell, rank, &op.amount).max(0) as usize;
    let views = visible(ctx, state, caster);
    let chosen: Vec<EntityId> = select(
        &views,
        library::value(spell, rank, &op.corpses.radius),
        &op.corpses.relations,
        op.corpses.filter.as_ref(),
        op.corpses.order,
        point,
    )
    .into_iter()
    .map(|c| c.id)
    .collect();
    let mut raised = Vec::new();
    for id in chosen {
        if raised.len() >= amount {
            break;
        }
        let Some(corpse) = state.combat.corpses.iter().find(|c| c.id == id).cloned() else {
            continue;
        };
        let owner = if op.ownership == Some(Ownership::Caster) {
            caster.owner
        } else {
            corpse.owner
        };
        let d = ctx.catalog().get(corpse.definition);
        let cost = match op.supply {
            SupplyPolicy::Require => d.supply_cost.unwrap_or(0),
            SupplyPolicy::AllowOverCap => 0,
        };
        let s = super::world_ops::supply(ctx, state, owner);
        if crate::economy::supply::admission(&s, cost, None).is_some() {
            continue;
        }
        let body = ctx.spatial.profiles.get(corpse.definition).body;
        let deck = corpse
            .position
            .surface
            .as_deref()
            .and_then(|s| ctx.spatial.deck_id(s));
        let Some(at) = ctx.spatial.nearest(
            Spot::new(corpse.position.x, corpse.position.y, deck),
            op.placement_radius as i32,
            None,
            Some(&body),
            None,
            None,
        ) else {
            continue;
        };
        let unit = super::summons::spawn(ctx, state, corpse.definition, owner, at, corpse.rotation);
        let catalog = ctx.catalog();
        let e = state.entities.get_mut(unit).expect("raised");
        if let Some(p) = &corpse.progression {
            e.progression = Some(p.clone());
        }
        if let Some(c) = &corpse.caster {
            e.caster = Some(c.clone());
        }
        let e = state.entities.get(unit).expect("raised");
        let stats = crate::stats::Stats::of(catalog, e, crate::stats::research(state, e.owner));
        let e = state.entities.get_mut(unit).expect("raised");
        let health = library::value(spell, rank, &op.health_permille);
        let mana = library::value(spell, rank, &op.mana_permille);
        e.hp = Some(((i64::from(stats.max_hp) * health / 1000).max(1)) as u32);
        if let Some(c) = &mut e.caster {
            c.mana = (i64::from(stats.max_mana) * mana / 1000).max(0) as u32;
        }
        if let Some(duration) = &op.duration_ticks {
            let now = state.tick;
            let duration = library::value(spell, rank, duration).max(0) as u32;
            let e = state.entities.get_mut(unit).expect("raised");
            super::links(e).summoned = Some(Summoned {
                source: caster.id,
                ability,
                rank,
                cast,
                started: now,
                expires: now + duration,
                split: None,
                link: None,
                reanimated_from: Some(corpse.id),
            });
        }
        consume(state, corpse.id);
        raised.push(unit);
    }
    raised
}
