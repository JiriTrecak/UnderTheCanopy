//! Economy commands from players, with the old validation and messages (the build,
//! construct, gather, produce, cancel, rally, pause, research, upgrade and revive branches
//! of `Game.command` and their cancels).

use super::{Ctx, alive, construction, harvest, production, research, revival, upgrade, work};
use crate::{
    action::{Action, CellPoint, FoundationPoint},
    entity::{EntityId, Place},
    orders::{Order, busy, can_issue, issue},
    spatial::Spot,
    vision::Sight,
};
use canopy_content::{game::Kind, placement::Owner};

/// Whether `action` is an economy command.
pub fn handles(action: &Action) -> bool {
    matches!(
        action,
        Action::Build { .. }
            | Action::Construct { .. }
            | Action::Gather { .. }
            | Action::Produce { .. }
            | Action::Cancel { .. }
            | Action::Rally { .. }
            | Action::Pause { .. }
            | Action::Research { .. }
            | Action::CancelResearch { .. }
            | Action::Upgrade { .. }
            | Action::CancelUpgrade { .. }
            | Action::Revive { .. }
            | Action::CancelRevival { .. }
    )
}

/// The sight a command consults, borrowed beside the economy context.
#[derive(Clone, Copy)]
pub struct Senses<'a> {
    pub vision: &'a crate::vision::Vision,
    pub terrain: &'a crate::terrain::Terrain,
    pub rules: &'a dyn crate::vision::SightRules,
}

impl<'a> Senses<'a> {
    pub fn sight<'s>(&'s self, ctx: &'s Ctx) -> Sight<'s> {
        Sight {
            vision: self.vision,
            state: ctx.state,
            catalog: ctx.catalog,
            terrain: self.terrain,
            rules: self.rules,
        }
    }
}

fn valid_cell(ctx: &Ctx, p: &CellPoint) -> bool {
    let deck = match &p.surface {
        None => None,
        Some(name) => match ctx.spatial.deck_id(name) {
            Some(deck) => Some(deck),
            None => return false,
        },
    };
    ctx.spatial
        .valid_spot(Spot::new(p.x as i32, p.y as i32, deck))
}

fn valid_foundation(ctx: &Ctx, p: &FoundationPoint) -> bool {
    let size = ctx.spatial.size();
    if p.x.0 >= 2 * size || p.y.0 >= 2 * size {
        return false;
    }
    match &p.surface {
        None => true,
        Some(name) => ctx.spatial.deck_id(name).is_some_and(|deck| {
            // A half cell rounds up to the next cell, as the old `Math.round` did.
            let spot = Spot::new(p.x.0.div_ceil(2) as i32, p.y.0.div_ceil(2) as i32, Some(deck));
            ctx.spatial.valid_spot(spot)
        }),
    }
}

/// Applies an economy command for `owner`; returns the actors that took it. Errors are the
/// old rejection messages.
pub fn apply(
    ctx: &mut Ctx,
    senses: Senses,
    owner: Owner,
    action: &Action,
) -> Result<Vec<EntityId>, String> {
    let point_valid = match action {
        Action::Rally {
            destination: Some(p),
            ..
        } => valid_cell(ctx, p),
        Action::Build { position, .. } => valid_foundation(ctx, position),
        _ => true,
    };
    if !point_valid {
        return Err("Destination is not on a declared walk surface".into());
    }
    let ids: Vec<EntityId> = match action {
        Action::Build { actors, .. } => actors.iter().map(|&a| EntityId(a)).collect(),
        Action::Gather { actors, .. } | Action::Construct { actors, .. } => {
            let mut ids: Vec<EntityId> = actors.iter().map(|&a| EntityId(a)).collect();
            ids.sort_unstable();
            ids
        }
        Action::Produce { actor, .. }
        | Action::Cancel { actor, .. }
        | Action::Rally { actor, .. }
        | Action::Pause { actor, .. }
        | Action::Research { actor, .. }
        | Action::CancelResearch { actor, .. }
        | Action::Upgrade { actor }
        | Action::CancelUpgrade { actor }
        | Action::Revive { actor, .. }
        | Action::CancelRevival { actor, .. } => vec![EntityId(*actor)],
        _ => return Err("Invalid request".into()),
    };
    if ids
        .iter()
        .any(|&id| ctx.state.entities.get(id).is_some_and(|e| e.owner != owner))
    {
        return Err("Not your actors".into());
    }
    let eligible: Vec<EntityId> = ids
        .into_iter()
        .filter(|&id| {
            ctx.state.entities.get(id).is_some_and(|e| {
                alive(e)
                    && ctx
                        .catalog
                        .get(e.definition)
                        .behaviors
                        .player_control
                        .is_some()
                    && e.unit
                        .as_ref()
                        .is_none_or(|u| u.contained.is_none() && u.release.is_none())
            })
        })
        .collect();
    if eligible.is_empty() {
        return Err("No eligible controlled actors".into());
    }
    match action {
        Action::Construct { target, append, .. } => construct(
            ctx,
            owner,
            &eligible,
            EntityId(*target),
            append.unwrap_or(false),
        ),
        Action::Gather { target, append, .. } => gather(
            ctx,
            senses,
            owner,
            &eligible,
            EntityId(*target),
            append.unwrap_or(false),
        ),
        Action::Build {
            definition,
            position,
            rotation,
            append,
            ..
        } => {
            let request = construction::Request {
                definition: definition.as_str(),
                point: position,
                rotation: rotation.unwrap_or(0),
                append: append.unwrap_or(false),
            };
            let builder = {
                let sight = senses.sight(ctx);
                construction::check_build(ctx, &sight, owner, &eligible, &request)?
            };
            construction::place_site(ctx, owner, builder, &request);
            Ok(vec![builder])
        }
        _ => workplace(ctx, senses, owner, eligible[0], action).map(|()| vec![eligible[0]]),
    }
}

fn construct(
    ctx: &mut Ctx,
    owner: Owner,
    eligible: &[EntityId],
    target: EntityId,
    append: bool,
) -> Result<Vec<EntityId>, String> {
    let site = ctx
        .state
        .entities
        .get(target)
        .filter(|t| t.construction.is_some() && alive(t) && t.owner == owner)
        .ok_or("Select an owned unfinished building")?;
    let site_id = ctx.catalog.id(site.definition);
    let builder = eligible
        .iter()
        .copied()
        .find(|&id| {
            ctx.state.entities.get(id).is_some_and(|e| {
                e.unit.is_some()
                    && ctx
                        .catalog
                        .get(e.definition)
                        .behaviors
                        .work
                        .as_ref()
                        .is_some_and(|w| w.builds.contains(site_id))
                    && can_issue(e, append)
            })
        })
        .ok_or("Select an eligible worker")?;
    issue(ctx.state, builder, Order::Construct { target }, append);
    Ok(vec![builder])
}

fn gather(
    ctx: &mut Ctx,
    senses: Senses,
    owner: Owner,
    eligible: &[EntityId],
    target: EntityId,
    append: bool,
) -> Result<Vec<EntityId>, String> {
    let visible = ctx.state.entities.get(target).is_some_and(|t| {
        t.resource.as_ref().is_some_and(|r| r.amount > 0) && senses.sight(ctx).seen(owner, t)
    });
    if !visible {
        return Err("Resource is not visible".into());
    }
    let mut actors = Vec::new();
    for &id in eligible {
        let (Some(e), Some(t)) = (ctx.state.entities.get(id), ctx.state.entities.get(target))
        else {
            continue;
        };
        let takes = e.unit.is_some()
            && harvest::harvest_item(ctx.catalog, e, t).is_some()
            && can_issue(e, append)
            && ((append && busy(e)) || harvest::can_assign_gather(ctx.catalog, e, t));
        if !takes {
            continue;
        }
        issue(ctx.state, id, Order::Gather { target }, append);
        actors.push(id);
    }
    if actors.is_empty() {
        return Err("No eligible workers can gather this resource".into());
    }
    Ok(actors)
}

/// The single-actor commands: research, upgrades, revival, cancelling a site, and
/// production.
fn workplace(
    ctx: &mut Ctx,
    senses: Senses,
    owner: Owner,
    actor: EntityId,
    action: &Action,
) -> Result<(), String> {
    match action {
        Action::Research { research: id, .. } => return research::enqueue(ctx, actor, id),
        Action::CancelResearch { research: id, .. } => return research::cancel(ctx, actor, id),
        Action::Upgrade { .. } => return upgrade::enqueue(ctx, actor),
        Action::CancelUpgrade { .. } => return upgrade::cancel(ctx, actor),
        Action::Revive { hero, .. } => return revival::enqueue(ctx, actor, EntityId(*hero)),
        Action::CancelRevival { hero, .. } => {
            return revival::cancel(ctx, actor, EntityId(*hero));
        }
        _ => {}
    }
    let e = ctx.state.entities.get(actor).expect("actor");
    if let Action::Cancel { queue, .. } = action
        && e.construction.is_some()
    {
        if queue.is_some() {
            return Err("Project has no training queue".into());
        }
        work::remove(ctx, actor, true);
        return Ok(());
    }
    let policy = ctx.catalog.get(e.definition).behaviors.production.as_ref();
    let Some(policy) = policy.filter(|_| e.construction.is_none() && e.production.is_some())
    else {
        return Err("No completed production capability".into());
    };
    match action {
        Action::Produce { definition, .. } => production::produce(ctx, actor, definition.as_str()),
        Action::Cancel { queue, .. } => match queue {
            Some(entry) if production::cancel_entry(ctx, actor, *entry) => Ok(()),
            _ => Err("Queue entry no longer exists".into()),
        },
        Action::Pause { paused, .. } => {
            production::pause(ctx, actor, *paused);
            Ok(())
        }
        Action::Rally {
            destination,
            target,
            ..
        } => {
            let units = policy.outputs.iter().any(|id| {
                ctx.catalog
                    .content
                    .definition(id.as_str())
                    .is_some_and(|d| d.kind == Kind::Unit)
            });
            if !units {
                return Err("No unit production".into());
            }
            let rally = match target {
                Some(target) => {
                    let sight = senses.sight(ctx);
                    let t = ctx.state.entities.get(EntityId(*target)).filter(|t| {
                        alive(t)
                            && sight.seen(owner, t)
                            && (t.resource.is_some()
                                || (t.owner == owner
                                    && t.unit.as_ref().is_some_and(|u| u.contained.is_none())))
                    });
                    Some(production::rally_on(
                        t.ok_or("Rally onto a visible resource or friendly unit")?,
                    ))
                }
                None => destination.as_ref().map(|d| super::Rally {
                    point: Place {
                        x: d.x as i32,
                        y: d.y as i32,
                        surface: d.surface.clone(),
                    },
                    target: None,
                }),
            };
            production::set_rally(ctx, actor, rally);
            Ok(())
        }
        _ => Err("Invalid request".into()),
    }
}
