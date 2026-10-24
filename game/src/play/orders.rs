//! Turning clicks into commands (old `session.ts` `click`, `rallyAt`, and
//! `commandFeedback.ts`): the right-click "smart" order by target, explicit targeting
//! modes, and the acknowledgement marker each accepted command shows.
//!
//! Everything here is a pure function of the player's knowledge: it produces protocol
//! `Action`s and never touches the simulation.

use super::know::Knowledge;
use canopy_content::{game::Creation, game::Kind, placement::Owner};
use canopy_sim::{
    Action,
    action::{CastTarget, CellPoint, PlainPoint},
    teams::Relation,
};

/// What was under the cursor.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub id: u32,
    pub definition: canopy_sim::catalog::Def,
    pub owner: Owner,
    pub relation: Relation,
    pub remembered: bool,
    /// World units (x, z).
    pub position: [f32; 2],
    pub construction: bool,
    pub resource: bool,
}

impl Target {
    /// A known entity or scenery resource by id.
    pub fn of(k: &Knowledge, id: u32) -> Option<Self> {
        if let Some(e) = k.entity(id) {
            return Some(Self {
                id,
                definition: e.definition(),
                owner: e.public.owner,
                relation: e.relation,
                remembered: e.remembered,
                position: k.position(e),
                construction: e.public.construction.is_some(),
                resource: e.public.resource.is_some(),
            });
        }
        let s = k.view.scenery.iter().find(|s| s.id.0 == id)?;
        Some(Self {
            id,
            definition: s.definition,
            owner: s.owner,
            relation: Relation::Neutral,
            remembered: false,
            position: [s.position.x as f32 / 1000.0, s.position.y as f32 / 1000.0],
            construction: false,
            resource: s.resource.is_some(),
        })
    }
}

/// The colour and timing of an acknowledgement ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerKind {
    /// White, one pulse.
    Move,
    /// Red, two pulses.
    Attack,
    /// Green, two pulses.
    Gather,
}

impl MarkerKind {
    pub fn pulses(self) -> u32 {
        match self {
            Self::Move => 1,
            Self::Attack | Self::Gather => 2,
        }
    }

    /// sRGB colour (old `#ffffff`, `#ff4141`, `#67ff77`).
    pub fn color(self) -> [u8; 3] {
        match self {
            Self::Move => [0xff, 0xff, 0xff],
            Self::Attack => [0xff, 0x41, 0x41],
            Self::Gather => [0x67, 0xff, 0x77],
        }
    }
}

/// Seconds a pulse shows and the gap after it.
pub const PULSE_SECONDS: f32 = 0.4;
pub const PULSE_GAP_SECONDS: f32 = 0.15;
/// Most markers at once; the oldest goes first.
pub const MAX_MARKERS: usize = 24;
/// Ring radius at a point, and for a gathered resource without a footprint.
pub const POINT_RADIUS: f32 = 0.85;
pub const GATHER_RADIUS: f32 = 1.25;

/// A ring the local player sees where an accepted command points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Marker {
    pub kind: MarkerKind,
    /// World units (x, z).
    pub position: [f32; 2],
    pub radius: f32,
}

impl Marker {
    /// Seconds the marker lives: `0.55 · pulses − 0.15`.
    pub fn lifetime(&self) -> f32 {
        (PULSE_SECONDS + PULSE_GAP_SECONDS) * self.kind.pulses() as f32 - PULSE_GAP_SECONDS
    }

    /// Opacity `age` seconds after it appeared (0 between pulses and after).
    pub fn opacity(&self, age: f32) -> f32 {
        if age < 0.0 || age >= self.lifetime() {
            return 0.0;
        }
        let phase = age % (PULSE_SECONDS + PULSE_GAP_SECONDS);
        if phase >= PULSE_SECONDS {
            0.0
        } else {
            0.95 * (1.0 - phase / PULSE_SECONDS)
        }
    }
}

fn append(shift: bool) -> Option<bool> {
    shift.then_some(true)
}

/// Units of the selection the player commands (own, controlled units).
pub fn commanded(k: &Knowledge, selection: &[u32]) -> Vec<u32> {
    selection
        .iter()
        .copied()
        .filter(|&id| {
            k.entity(id)
                .is_some_and(|e| k.controlled(e) && k.is_unit(e))
        })
        .collect()
}

/// Buildings of the selection that produce units (rally targets).
pub fn producers(k: &Knowledge, selection: &[u32]) -> Vec<u32> {
    selection
        .iter()
        .copied()
        .filter(|&id| {
            k.entity(id).is_some_and(|e| {
                let d = k.def(e);
                k.own(e)
                    && k.alive(e)
                    && d.kind == Kind::Building
                    && e.public.construction.is_none()
                    && d.behaviors.production.as_ref().is_some_and(|p| {
                        p.outputs.iter().any(|o| {
                            k.content
                                .definition(o.as_str())
                                .is_some_and(|od| od.kind == Kind::Unit)
                        })
                    })
            })
        })
        .collect()
}

/// The marker of a target-based command.
fn target_marker(k: &Knowledge, kind: MarkerKind, t: &Target) -> Marker {
    let d = k.catalog.get(t.definition);
    let radius = match &d.footprint {
        Some(f) => (f64::from(f.width).hypot(f64::from(f.depth)) / 2.0 + 0.25) as f32,
        None if kind == MarkerKind::Gather => GATHER_RADIUS,
        None => POINT_RADIUS,
    };
    Marker {
        kind,
        position: t.position,
        radius,
    }
}

fn point_marker(kind: MarkerKind, p: &CellPoint) -> Marker {
    Marker {
        kind,
        position: [p.x as f32, p.y as f32],
        radius: POINT_RADIUS,
    }
}

/// Whether `actor` can harvest `target` (some item it harvests comes from that source).
pub fn harvests(k: &Knowledge, actor: u32, target: &Target) -> bool {
    let source = k.catalog.id(target.definition);
    k.def_of(actor)
        .and_then(|d| d.behaviors.work.as_ref())
        .and_then(|w| w.harvests.as_ref())
        .is_some_and(|items| {
            items.iter().any(|item| {
                k.content.definition(item.as_str()).is_some_and(|i| {
                    matches!(&i.creation, Some(Creation::Harvest { source: s, .. }) if s == source)
                })
            })
        })
}

/// The known resource a building placed on one (`placementOn`) stands on.
pub fn resource_under(k: &Knowledge, building: &Target) -> Option<Target> {
    let on = k.catalog.get(building.definition).placement_on.as_ref()?;
    let e = k.entity(building.id)?;
    let resource = k.view.entities.iter().find(|r| {
        r.public.resource.is_some()
            && r.public.position == e.public.position
            && k.def(r).id == on.source
    })?;
    Target::of(k, resource.id().0)
}

/// A rally command for `producer` toward the click (old `rallyAt`): a resource or an
/// own unit becomes the rally target, anything else a ground point.
pub fn rally(
    k: &Knowledge,
    producer: u32,
    target: Option<&Target>,
    ground: Option<&CellPoint>,
) -> Option<(Action, Option<Marker>)> {
    if let Some(t) = target.filter(|t| {
        !t.remembered
            && t.id != producer
            && (t.resource
                || (k.owner() == Some(t.owner)
                    && k.entity(t.id)
                        .is_some_and(|e| k.is_unit(e) && !k.held(t.id))))
    }) {
        let kind = if t.resource {
            MarkerKind::Gather
        } else {
            MarkerKind::Move
        };
        return Some((
            Action::Rally {
                actor: producer,
                destination: None,
                target: Some(t.id),
            },
            Some(target_marker(k, kind, t)),
        ));
    }
    let ground = ground?;
    Some((
        Action::Rally {
            actor: producer,
            destination: Some(ground.clone()),
            target: None,
        },
        Some(point_marker(MarkerKind::Move, ground)),
    ))
}

/// The right-click order for the selection (old `click` without a mode). The first rule
/// that applies sends one command for the units it concerns; the rest of the selection
/// gets nothing, as in the old game.
pub fn smart(
    k: &Knowledge,
    selection: &[u32],
    target: Option<&Target>,
    ground: Option<&CellPoint>,
    shift: bool,
) -> Vec<(Action, Option<Marker>)> {
    let selected = commanded(k, selection);
    if selected.is_empty() {
        // Producers only: rally (right-clicking a producer clears its rally).
        return producers(k, selection)
            .into_iter()
            .filter_map(|p| {
                if target.is_some_and(|t| t.id == p) {
                    Some((
                        Action::Rally {
                            actor: p,
                            destination: None,
                            target: None,
                        },
                        None,
                    ))
                } else {
                    rally(k, p, target, ground)
                }
            })
            .collect();
    }
    let army: Vec<u32> = selected
        .iter()
        .copied()
        .filter(|&id| k.def_of(id).is_some_and(|d| d.behaviors.combat.is_some()))
        .collect();
    let mine = |t: &Target| k.owner() == Some(t.owner);
    if let Some(t) = target.filter(|t| !t.remembered) {
        let d = k.catalog.get(t.definition);
        let id = k.catalog.id(t.definition);
        // a. Resume an own construction site.
        if mine(t) && t.construction {
            let builders: Vec<u32> = selected
                .iter()
                .copied()
                .filter(|&a| {
                    k.def_of(a)
                        .and_then(|d| d.behaviors.work.as_ref())
                        .is_some_and(|w| w.builds.iter().any(|b| b == id))
                })
                .collect();
            if !builders.is_empty() {
                return vec![(
                    Action::Construct {
                        actors: builders,
                        target: t.id,
                        append: append(shift),
                    },
                    None,
                )];
            }
        }
        // b. Enter an own lookout.
        if mine(t)
            && let Some(g) = &d.garrison
        {
            let guests: Vec<u32> = selected
                .iter()
                .copied()
                .filter(|&a| {
                    k.entity(a)
                        .is_some_and(|e| g.accepts.contains(k.catalog.id(e.definition())))
                })
                .collect();
            if !guests.is_empty() {
                let marker = target_marker(k, MarkerKind::Move, t);
                return vec![(
                    Action::Garrison {
                        actors: guests,
                        target: t.id,
                        append: append(shift),
                    },
                    Some(marker),
                )];
            }
        }
        // b'. Gather at an own finished building placed on a resource (a Substance Well on
        // its deposit, D62): the workers gather the resource under it.
        if mine(t)
            && !t.construction
            && let Some(r) = resource_under(k, t)
        {
            let gatherers: Vec<u32> = selected
                .iter()
                .copied()
                .filter(|&a| harvests(k, a, &r))
                .collect();
            if !gatherers.is_empty() {
                let marker = target_marker(k, MarkerKind::Gather, t);
                return vec![(
                    Action::Gather {
                        actors: gatherers,
                        target: r.id,
                        append: append(shift),
                    },
                    Some(marker),
                )];
            }
        }
        // c. Follow an own unit (nothing when it is the only one selected).
        if mine(t) && d.kind == Kind::Unit {
            let followers: Vec<u32> = selected.iter().copied().filter(|&a| a != t.id).collect();
            if followers.is_empty() {
                return Vec::new();
            }
            let marker = target_marker(k, MarkerKind::Move, t);
            return vec![(
                Action::Follow {
                    actors: followers,
                    target: t.id,
                    append: append(shift),
                },
                Some(marker),
            )];
        }
        // d. Gather a resource.
        if t.resource {
            let gatherers: Vec<u32> = selected
                .iter()
                .copied()
                .filter(|&a| harvests(k, a, t))
                .collect();
            if !gatherers.is_empty() {
                let marker = target_marker(k, MarkerKind::Gather, t);
                return vec![(
                    Action::Gather {
                        actors: gatherers,
                        target: t.id,
                        append: append(shift),
                    },
                    Some(marker),
                )];
            }
        }
        // e. Pick up an item (the first selected unit with an inventory).
        if d.kind == Kind::Item
            && d.item_effect.is_some()
            && let Some(&carrier) = selected
                .iter()
                .find(|&&a| k.def_of(a).is_some_and(|d| d.behaviors.inventory.is_some()))
        {
            let marker = target_marker(k, MarkerKind::Move, t);
            return vec![(
                Action::Pickup {
                    actor: carrier,
                    target: t.id,
                    append: append(shift),
                },
                Some(marker),
            )];
        }
        // f, g. The army attacks enemies and neutral creatures, and breaks crates and
        // rubble (a forced attack: no unit attacks those by itself). Unlike the old game,
        // allies are not attacked by a right-click (D20 teams; force attack with A).
        let breakable = t.owner == Owner::Nobody && d.breakable.is_some();
        let hostile = match t.owner {
            Owner::Player(_) => !mine(t) && t.relation != Relation::Ally,
            Owner::Nobody => d.kind == Kind::Unit || breakable,
        };
        if !army.is_empty() && hostile && d.body.is_some() {
            let marker = target_marker(k, MarkerKind::Attack, t);
            return vec![(
                Action::Attack {
                    actors: army,
                    target: t.id,
                    force: breakable.then_some(true),
                    append: append(shift),
                },
                Some(marker),
            )];
        }
    }
    // h. Move to the ground point.
    let Some(ground) = ground else {
        return Vec::new();
    };
    vec![(
        Action::Move {
            actors: selected,
            destination: ground.clone(),
            attack_move: None,
            append: append(shift),
        },
        Some(point_marker(MarkerKind::Move, ground)),
    )]
}

/// A command mode a command-card button arms; the next left click completes it.
#[derive(Debug, Clone, PartialEq)]
pub enum Targeting {
    /// Attack a visible target (forced) or attack-move to the ground.
    Attack {
        actors: Vec<u32>,
    },
    Move {
        actors: Vec<u32>,
    },
    Patrol {
        actors: Vec<u32>,
    },
    Follow {
        actors: Vec<u32>,
    },
    Rally {
        actors: Vec<u32>,
    },
    /// Cast at a unit or a point.
    Cast {
        actor: u32,
        binding: String,
        unit: bool,
    },
}

impl Targeting {
    /// The old hint's noun.
    pub fn wants(&self) -> String {
        match self {
            Self::Attack { .. } | Self::Follow { .. } => tr!("play.wants-target-or-ground"),
            Self::Cast { unit: true, .. } => tr!("play.wants-target"),
            _ => tr!("play.wants-ground"),
        }
    }
}

/// What a left click does in a targeting mode: the commands, and whether the mode stays
/// armed (a unit spell clicked on the ground waits for a unit).
pub fn complete(
    k: &Knowledge,
    mode: &Targeting,
    target: Option<&Target>,
    ground: Option<&CellPoint>,
    shift: bool,
) -> (Vec<(Action, Option<Marker>)>, bool) {
    let visible = target.filter(|t| !t.remembered);
    match mode {
        Targeting::Attack { actors } => match (visible, ground) {
            (Some(t), _) if k.catalog.get(t.definition).body.is_some() => (
                vec![(
                    Action::Attack {
                        actors: actors.clone(),
                        target: t.id,
                        force: Some(true),
                        append: append(shift),
                    },
                    Some(target_marker(k, MarkerKind::Attack, t)),
                )],
                false,
            ),
            (_, Some(g)) => (
                vec![(
                    Action::Move {
                        actors: actors.clone(),
                        destination: g.clone(),
                        attack_move: Some(true),
                        append: append(shift),
                    },
                    Some(point_marker(MarkerKind::Attack, g)),
                )],
                false,
            ),
            _ => (Vec::new(), false),
        },
        Targeting::Move { actors } => (
            ground
                .map(|g| {
                    vec![(
                        Action::Move {
                            actors: actors.clone(),
                            destination: g.clone(),
                            attack_move: None,
                            append: append(shift),
                        },
                        Some(point_marker(MarkerKind::Move, g)),
                    )]
                })
                .unwrap_or_default(),
            false,
        ),
        Targeting::Patrol { actors } => (
            ground
                .map(|g| {
                    vec![(
                        Action::Patrol {
                            actors: actors.clone(),
                            destination: g.clone(),
                            append: append(shift),
                        },
                        Some(point_marker(MarkerKind::Move, g)),
                    )]
                })
                .unwrap_or_default(),
            false,
        ),
        Targeting::Follow { actors } => (
            visible
                .map(|t| {
                    vec![(
                        Action::Follow {
                            actors: actors.clone(),
                            target: t.id,
                            append: append(shift),
                        },
                        Some(target_marker(k, MarkerKind::Move, t)),
                    )]
                })
                .unwrap_or_default(),
            false,
        ),
        Targeting::Rally { actors } => {
            let mut out: Vec<(Action, Option<Marker>)> = actors
                .iter()
                .filter_map(|&a| rally(k, a, visible, ground))
                .collect();
            // One marker for the group.
            for (_, marker) in out.iter_mut().skip(1) {
                *marker = None;
            }
            (out, false)
        }
        Targeting::Cast {
            actor,
            binding,
            unit,
        } => {
            if *unit {
                let Some(t) =
                    visible.filter(|t| k.entity(t.id).is_some_and(|e| k.is_unit(e) && k.alive(e)))
                else {
                    return (Vec::new(), true);
                };
                (
                    vec![(
                        Action::CastAbility {
                            actor: *actor,
                            binding: binding.clone(),
                            target: CastTarget::Unit { entity: t.id },
                        },
                        None,
                    )],
                    false,
                )
            } else {
                let Some(g) = ground else {
                    return (Vec::new(), true);
                };
                (
                    vec![(
                        Action::CastAbility {
                            actor: *actor,
                            binding: binding.clone(),
                            target: CastTarget::Point {
                                position: PlainPoint { x: g.x, y: g.y },
                            },
                        },
                        None,
                    )],
                    false,
                )
            }
        }
    }
}
