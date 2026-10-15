//! The command card (old `presentation/commands.ts`): which commands the selection
//! offers, their costs, availability and cooldowns, the category menus and the 4 × 3
//! layout with Back in slot 9, bottom-row spells and full-width banners.
//!
//! Commands come from the focus (the first selected entity): a controlled building
//! offers only its own commands; a controlled unit offers the basic orders for every
//! eligible selected unit, its type's spells and its type's build menu. Availability
//! (reasons) is the player's best knowledge; the simulation decides, and refusals come
//! back as error facts.

use super::{
    know::Knowledge,
    orders::{Targeting, commanded},
};
use canopy_content::{
    Id,
    abilities::{Activation, TargetingKind},
    game::{ActionPlacement, Creation, Definition, Kind},
};
use canopy_sim::{Action, abilities::library};

/// What pressing a command does.
#[derive(Debug, Clone, PartialEq)]
pub enum Activate {
    /// Sends at once; `queue` commands take Shift as append.
    Immediate { action: Action, queue: bool },
    /// Arms a targeting mode.
    Target(Targeting),
    /// Starts building placement.
    Place {
        definition: String,
        actors: Vec<u32>,
    },
    /// Opens a category (`None`: the root card).
    Menu(Option<String>),
}

/// One cost row: the icon id and the amount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostView {
    pub icon: Id,
    pub amount: u32,
    /// Lower first (currency `displayOrder`; supply and mana last).
    pub order: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: Id,
    pub hotkey: Option<String>,
    pub costs: Vec<CostView>,
    pub reason: Option<String>,
    pub priority: i64,
    pub category: Option<String>,
    pub placement: Option<ActionPlacement>,
    /// Fixed bottom-row column (1–4) of a spell.
    pub column: Option<u32>,
    /// Remaining and total cooldown ticks.
    pub cooldown: Option<(u32, u32)>,
    /// Autocast state; right-click sends `alternate`.
    pub autocast: Option<bool>,
    pub alternate: Option<Action>,
    pub actors: Vec<u32>,
    pub activate: Activate,
}

impl Command {
    pub fn enabled(&self) -> bool {
        self.reason.is_none()
    }

    fn new(
        id: impl Into<String>,
        name: &str,
        description: &str,
        icon: &Id,
        priority: i64,
        activate: Activate,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.to_owned(),
            description: description.to_owned(),
            icon: icon.clone(),
            hotkey: None,
            costs: Vec::new(),
            reason: None,
            priority,
            category: None,
            placement: None,
            column: None,
            cooldown: None,
            autocast: None,
            alternate: None,
            actors: Vec::new(),
            activate,
        }
    }
}

const SECONDS_PER_TICK: f64 = 0.025;

/// The cost row icon id that stands for mana (no content icon exists).
pub const MANA: &str = "mana";

fn mana_icon() -> Id {
    Id::new(MANA).expect("a valid id")
}

fn seconds(ticks: u32) -> String {
    let s = f64::from(ticks) * SECONDS_PER_TICK;
    if s.fract() == 0.0 {
        format!("{s:.0}")
    } else {
        format!("{s}")
    }
}

/// A verb's command from the content's action metadata (`actions.actions[verb]`) and
/// its override for `target` (`verb:<id>`); `None` when hidden or unknown.
fn add(
    k: &Knowledge,
    verb: &str,
    target: Option<&Definition>,
    activate: Activate,
) -> Option<Command> {
    let actions = &k.content.actions;
    let meta = actions.actions.get(verb)?;
    let id = match target {
        Some(t) => format!("{verb}:{}", t.id),
        None => verb.to_owned(),
    };
    let over = actions.overrides.get(&id);
    if over.and_then(|o| o.hidden) == Some(true) {
        return None;
    }
    let (name, description, icon) = match target {
        Some(t) => (t.name.as_str(), t.description.as_str(), &t.icon),
        None => (meta.name.as_str(), meta.description.as_str(), &meta.icon),
    };
    let mut c = Command::new(
        id,
        name,
        description,
        icon,
        over.and_then(|o| o.priority).unwrap_or(meta.priority),
        activate,
    );
    c.hotkey = over
        .and_then(|o| o.hotkey.clone())
        .or_else(|| meta.hotkey.clone());
    c.category = match over.and_then(|o| o.category.clone()) {
        Some(None) => None,
        Some(Some(cat)) => Some(cat.to_string()),
        None => target
            .and_then(|t| t.category.as_ref())
            .or(meta.category.as_ref())
            .map(|c| c.to_string()),
    };
    c.placement = meta.placement;
    if let Some(t) = target {
        c.costs = creation_costs(k, t);
    }
    Some(c)
}

fn currency_order(k: &Knowledge, item: &str) -> i64 {
    k.content
        .definition(item)
        .and_then(|d| d.display_order)
        .unwrap_or(1000)
}

/// Items of `items` as cost rows, sorted by display order.
pub fn item_costs(k: &Knowledge, items: &[canopy_content::game::Cost]) -> Vec<CostView> {
    let mut costs: Vec<CostView> = items
        .iter()
        .filter_map(|c| {
            let d = k.content.definition(c.item.as_str())?;
            Some(CostView {
                icon: d.icon.clone(),
                amount: c.amount,
                order: currency_order(k, c.item.as_str()),
            })
        })
        .collect();
    costs.sort_by_key(|c| c.order);
    costs
}

/// The price of creating `d` plus its supply.
fn creation_costs(k: &Knowledge, d: &Definition) -> Vec<CostView> {
    let mut costs = d
        .creation
        .as_ref()
        .map(|c| item_costs(k, c.items()))
        .unwrap_or_default();
    if let Some(supply) = d.supply_cost.filter(|&s| s > 0) {
        costs.push(CostView {
            icon: k.content.rules.supply_icon.clone(),
            amount: supply,
            order: i64::MAX,
        });
    }
    costs
}

/// Completed own buildings' definitions and what they fulfil (old
/// `fulfilledPrerequisites`: a building and every building that upgrades into it).
pub fn fulfilled_buildings(k: &Knowledge) -> Vec<String> {
    let mut done: Vec<String> = Vec::new();
    for e in k.view.entities.iter() {
        if k.own(e) && k.alive(e) && k.is_building(e) && e.public.construction.is_none() {
            let mut d = k.def(e);
            done.push(d.id.to_string());
            // Upgrades fulfil what they came from: walk the chain backwards.
            let mut guard = 0;
            while let Some(from) = k
                .content
                .definitions
                .iter()
                .find(|x| x.upgrade.as_ref().is_some_and(|u| u.target == d.id))
            {
                done.push(from.id.to_string());
                d = from;
                guard += 1;
                if guard > 16 {
                    break;
                }
            }
        }
    }
    done.sort();
    done.dedup();
    done
}

/// "Requires X, Y" for missing prerequisite buildings.
pub fn prerequisite(k: &Knowledge, requires: Option<&[Id]>, done: &[String]) -> Option<String> {
    let missing: Vec<String> = requires?
        .iter()
        .filter(|id| done.binary_search(&id.to_string()).is_err())
        .map(|id| k.name_of(id.as_str()))
        .collect();
    (!missing.is_empty()).then(|| format!("Requires {}", missing.join(", ")))
}

/// Spendable amount of each good.
fn available(k: &Knowledge, item: &str) -> u32 {
    k.view
        .colony
        .as_ref()
        .and_then(|c| c.goods.iter().find(|g| g.item.as_str() == item))
        .map_or(0, |g| g.available)
}

fn affordable(k: &Knowledge, items: &[canopy_content::game::Cost]) -> bool {
    items
        .iter()
        .all(|c| available(k, c.item.as_str()) >= c.amount)
}

/// The name of the player's cheapest supply building, for supply messages.
fn supply_hint(k: &Knowledge) -> Option<String> {
    let own: Vec<&str> = k
        .view
        .entities
        .iter()
        .filter(|e| k.own(e))
        .map(|e| k.def(e).id.as_str())
        .collect();
    let race = k
        .content
        .rules
        .races
        .values()
        .find(|r| r.roster.iter().any(|id| own.contains(&id.as_str())))?;
    race.roster
        .iter()
        .filter_map(|id| k.content.definition(id.as_str()))
        .filter(|d| {
            d.supply_provided.is_some_and(|s| s > 0)
                && matches!(d.creation, Some(Creation::Construct { .. }))
        })
        .min_by_key(|d| {
            d.creation
                .as_ref()
                .map_or(0, |c| c.items().iter().map(|i| i.amount).sum::<u32>())
        })
        .map(|d| d.name.clone())
}

/// Why a unit costing `cost` supply cannot be queued (old `supplyAdmission`).
fn supply_admission(k: &Knowledge, cost: u32) -> Option<String> {
    let colony = k.view.colony.as_ref()?;
    canopy_sim::economy::supply::admission(&colony.supply, cost, supply_hint(k).as_deref())
}

/// Why a hero cannot be trained (old `heroAdmission`).
fn hero_admission(k: &Knowledge, d: &Definition) -> Option<String> {
    if !d.is_hero() {
        return None;
    }
    let mine = k
        .view
        .entities
        .iter()
        .filter(|e| k.own(e) || k.view.battle.fallen.contains(&e.id()));
    let heroes: Vec<&Definition> = mine
        .clone()
        .map(|e| k.def(e))
        .filter(|x| x.is_hero())
        .collect();
    if heroes.iter().any(|h| h.id == d.id) {
        return Some("This hero already belongs to your roster. Revive fallen heroes.".into());
    }
    let capacity = mine
        .filter(|e| k.is_building(e) && e.public.construction.is_none())
        .filter_map(|e| k.def(e).hero_capacity)
        .max()
        .unwrap_or(0);
    (heroes.len() >= capacity as usize).then(|| {
        format!(
            "Hero roster full ({}/{capacity}). Upgrade a Hall.",
            heroes.len()
        )
    })
}

/// Every command the selection offers (old `commandCard`), unsorted.
pub fn commands(k: &Knowledge, selection: &[u32]) -> Vec<Command> {
    let Some(focus) = selection.first().and_then(|&id| k.entity(id)) else {
        return Vec::new();
    };
    if !k.controlled(focus) {
        return Vec::new();
    }
    let ended = k.view.outcome.is_some();
    let done = fulfilled_buildings(k);
    let mut out = Vec::new();
    let fid = focus.id().0;
    let d = k.def(focus);
    let private = focus.private.as_ref();
    let work = private.map(|p| &*p.economy);
    if d.kind == Kind::Building {
        if focus.public.construction.is_some() {
            if let Some(mut c) = add(
                k,
                "cancel",
                None,
                Activate::Immediate {
                    action: Action::Cancel {
                        actor: fid,
                        queue: None,
                    },
                    queue: false,
                },
            ) {
                c.description = format!(
                    "Cancel construction and recover {}% of the paid resources, rounded down per resource.",
                    k.content.rules.construction_refund_permille / 10
                );
                c.actors = vec![fid];
                out.push(c);
            }
            return finish(out, ended);
        }
        let occupied = k.view.entities.iter().any(|e| {
            e.combat
                .as_ref()
                .is_some_and(|c| c.garrison == Some(focus.id()))
        });
        if occupied
            && let Some(c) = add(
                k,
                "unload",
                None,
                Activate::Immediate {
                    action: Action::Unload { actor: fid },
                    queue: false,
                },
            )
        {
            out.push(c);
        }
        if let Some(up) = &d.upgrade {
            let target = k.content.definition(up.target.as_str());
            if let Some(task) = work.and_then(|w| w.upgrade.as_ref()) {
                if let Some(mut c) = add(
                    k,
                    "cancelUpgrade",
                    None,
                    Activate::Immediate {
                        action: Action::CancelUpgrade { actor: fid },
                        queue: false,
                    },
                ) {
                    let pct = (f64::from(task.progress) / f64::from(up.work_ticks.max(1)) * 100.0)
                        .floor();
                    c.description = format!(
                        "{}: {pct}% complete. Cancel to refund the full price.",
                        target.map_or(up.target.as_str(), |t| t.name.as_str())
                    );
                    out.push(c);
                }
            } else if let Some(mut c) = add(
                k,
                "upgrade",
                None,
                Activate::Immediate {
                    action: Action::Upgrade { actor: fid },
                    queue: false,
                },
            ) {
                if let Some(t) = target {
                    c.name = format!("Upgrade to {}", t.name);
                    c.icon = t.icon.clone();
                    c.description = format!(
                        "{}\n{}s. Unit training pauses during the upgrade.",
                        t.description,
                        seconds(up.work_ticks)
                    );
                    c.reason = prerequisite(k, t.requires.as_deref(), &done);
                }
                c.costs = item_costs(k, &up.items);
                if c.reason.is_none() && !affordable(k, &up.items) {
                    c.reason = Some("Insufficient resources".into());
                }
                out.push(c);
            }
        }
        if let Some(policy) = &d.behaviors.research {
            let queued: Vec<&str> = k
                .view
                .entities
                .iter()
                .filter(|e| k.own(e))
                .filter_map(|e| e.private.as_ref())
                .filter_map(|p| p.economy.research.as_ref())
                .flat_map(|q| q.queue.iter().map(|r| r.id.as_str()))
                .collect();
            let here = work
                .and_then(|w| w.research.as_ref())
                .map_or(0, |q| q.queue.len());
            let researched: Vec<&str> = k
                .view
                .colony
                .as_ref()
                .map(|c| c.research.iter().map(|r| r.as_str()).collect())
                .unwrap_or_default();
            for id in &policy.outputs {
                let Some(rule) = k.content.rules.research.get(id.as_str()) else {
                    continue;
                };
                let mut c = Command::new(
                    format!("research:{id}"),
                    &rule.name,
                    &format!(
                        "{}\n{}s. Applies to existing and future units.",
                        rule.description,
                        seconds(rule.work_ticks)
                    ),
                    &rule.icon,
                    rule.priority,
                    Activate::Immediate {
                        action: Action::Research {
                            actor: fid,
                            research: id.clone(),
                        },
                        queue: false,
                    },
                );
                c.costs = item_costs(k, &rule.items);
                c.reason = if researched.contains(&id.as_str()) {
                    Some("Already researched".into())
                } else if queued.contains(&id.as_str()) {
                    Some("Research already queued".into())
                } else if here >= policy.queue_capacity as usize {
                    Some("Research queue is full".into())
                } else if let Some(r) = prerequisite(k, rule.requires.as_deref(), &done) {
                    Some(r)
                } else if !affordable(k, &rule.items) {
                    Some("Insufficient resources".into())
                } else {
                    None
                };
                c.actors = vec![fid];
                out.push(c);
            }
        }
        if let Some(policy) = &d.behaviors.revival {
            let reviving: Vec<u32> = work
                .and_then(|w| w.revival.as_ref())
                .map(|q| q.queue.iter().map(|r| r.hero.0).collect())
                .unwrap_or_default();
            let returning: Vec<u32> = k.view.spells.returns.iter().map(|r| r.0.0).collect();
            for &hero in &k.view.battle.fallen {
                let Some(h) = k
                    .entity(hero.0)
                    .filter(|h| k.owner() == Some(h.public.owner))
                else {
                    continue;
                };
                let hd = k.def(h);
                let level = h.combat.as_ref().map_or(1, |c| c.stats.level);
                let items: Vec<canopy_content::game::Cost> = policy
                    .items
                    .iter()
                    .map(|c| {
                        let per = policy
                            .items_per_level
                            .iter()
                            .find(|p| p.item == c.item)
                            .map_or(0, |p| p.amount);
                        canopy_content::game::Cost {
                            item: c.item.clone(),
                            amount: c.amount + per * level.saturating_sub(1),
                        }
                    })
                    .collect();
                let ticks =
                    policy.work_ticks + policy.work_ticks_per_level * level.saturating_sub(1);
                let mut c = Command::new(
                    format!("revive:{}", hero.0),
                    &format!("Revive {}", hd.name),
                    &format!(
                        "Return this level {level} hero with their items and learned abilities. {}s. Cancellation refunds the full price.",
                        seconds(ticks)
                    ),
                    &hd.icon,
                    100,
                    Activate::Immediate {
                        action: Action::Revive {
                            actor: fid,
                            hero: hero.0,
                        },
                        queue: false,
                    },
                );
                c.costs = item_costs(k, &items);
                let supply = hd.supply_cost.unwrap_or(0);
                if supply > 0 {
                    c.costs.push(CostView {
                        icon: k.content.rules.supply_icon.clone(),
                        amount: supply,
                        order: i64::MAX,
                    });
                }
                c.reason = if returning.contains(&hero.0) {
                    Some("Hero is returning through an ability".into())
                } else if reviving.contains(&hero.0) {
                    Some("Hero is already being revived".into())
                } else if reviving.len() >= policy.queue_capacity as usize {
                    Some("Revival queue is full".into())
                } else if let Some(r) = supply_admission(k, supply) {
                    Some(r)
                } else if !affordable(k, &items) {
                    Some("Insufficient resources".into())
                } else {
                    None
                };
                out.push(c);
            }
        }
        if let Some(policy) = &d.behaviors.production {
            let production = work.and_then(|w| w.production.as_ref());
            let queued = production.map_or(0, |p| p.queue.len());
            let mut units = false;
            for output in &policy.outputs {
                let Some(od) = k.content.definition(output.as_str()) else {
                    continue;
                };
                units |= od.kind == Kind::Unit;
                if policy.mode != canopy_content::game::ProductionMode::Queued {
                    continue;
                }
                let Some(mut c) = add(
                    k,
                    "produce",
                    Some(od),
                    Activate::Immediate {
                        action: Action::Produce {
                            actor: fid,
                            definition: output.clone(),
                        },
                        queue: false,
                    },
                ) else {
                    continue;
                };
                c.reason = prerequisite(k, od.requires.as_deref(), &done)
                    .or_else(|| hero_admission(k, od))
                    .or_else(|| {
                        policy
                            .queue_capacity
                            .is_some_and(|cap| queued >= cap as usize)
                            .then(|| "Queue full".to_owned())
                    })
                    .or_else(|| supply_admission(k, od.supply_cost.unwrap_or(0)))
                    .or_else(|| {
                        let items = od.creation.as_ref().map_or(&[][..], |c| c.items());
                        (!affordable(k, items)).then(|| "Insufficient resources".to_owned())
                    });
                c.actors = vec![fid];
                out.push(c);
            }
            if units
                && let Some(mut c) = add(
                    k,
                    "rally",
                    None,
                    Activate::Target(Targeting::Rally { actor: fid }),
                )
            {
                c.actors = vec![fid];
                out.push(c);
            }
            if let Some(p) = production
                && let Some(mut c) = add(
                    k,
                    "pause",
                    None,
                    Activate::Immediate {
                        action: Action::Pause {
                            actor: fid,
                            paused: !p.paused,
                        },
                        queue: false,
                    },
                )
            {
                if p.paused {
                    c.name = "Resume production".into();
                }
                out.push(c);
            }
        }
        return finish(out, ended);
    }
    // A unit: basic orders for every eligible selected unit.
    let units = commanded(k, selection);
    let movers: Vec<u32> = units
        .iter()
        .copied()
        .filter(|&id| k.def_of(id).is_some_and(|d| d.behaviors.movement.is_some()))
        .collect();
    let fighters: Vec<u32> = units
        .iter()
        .copied()
        .filter(|&id| k.def_of(id).is_some_and(|d| d.behaviors.combat.is_some()))
        .collect();
    if !movers.is_empty() {
        let basic: [(&str, Activate); 5] = [
            (
                "move",
                Activate::Target(Targeting::Move {
                    actors: movers.clone(),
                }),
            ),
            (
                "stop",
                Activate::Immediate {
                    action: Action::Stop {
                        actors: movers.clone(),
                    },
                    queue: false,
                },
            ),
            (
                "hold",
                Activate::Immediate {
                    action: Action::Hold {
                        actors: movers.clone(),
                        append: None,
                    },
                    queue: true,
                },
            ),
            (
                "patrol",
                Activate::Target(Targeting::Patrol {
                    actors: movers.clone(),
                }),
            ),
            (
                "follow",
                Activate::Target(Targeting::Follow {
                    actors: movers.clone(),
                }),
            ),
        ];
        for (verb, activate) in basic {
            if let Some(mut c) = add(k, verb, None, activate) {
                c.actors = movers.clone();
                out.push(c);
            }
        }
    }
    if !fighters.is_empty()
        && let Some(mut c) = add(
            k,
            "attack",
            None,
            Activate::Target(Targeting::Attack {
                actors: fighters.clone(),
            }),
        )
    {
        c.actors = fighters;
        out.push(c);
    }
    spells(k, selection, focus, &mut out);
    // Build: the focus type's menu; actors are every selected unit that can build it.
    if let Some(w) = &d.behaviors.work {
        for building in &w.builds {
            let Some(bd) = k.content.definition(building.as_str()) else {
                continue;
            };
            let actors: Vec<u32> = units
                .iter()
                .copied()
                .filter(|&id| {
                    k.def_of(id)
                        .and_then(|d| d.behaviors.work.as_ref())
                        .is_some_and(|w| w.builds.contains(building))
                })
                .collect();
            let Some(mut c) = add(
                k,
                "build",
                Some(bd),
                Activate::Place {
                    definition: building.to_string(),
                    actors: actors.clone(),
                },
            ) else {
                continue;
            };
            c.reason = prerequisite(k, bd.requires.as_deref(), &done);
            c.actors = actors;
            out.push(c);
        }
    }
    finish(out, ended)
}

/// The focus type's spells: learn entries (banner menu) and cast entries (bottom row).
fn spells(
    k: &Knowledge,
    selection: &[u32],
    focus: &canopy_sim::view::EntityView,
    out: &mut Vec<Command>,
) {
    // The first controlled unit of the focus's type with abilities casts.
    let Some(caster) = selection.iter().filter_map(|&id| k.entity(id)).find(|e| {
        e.definition() == focus.definition()
            && k.controlled(e)
            && e.magic.as_ref().is_some_and(|m| m.caster.is_some())
    }) else {
        return;
    };
    let d = k.def(caster);
    let Some(authored) = d.behaviors.abilities.as_ref() else {
        return;
    };
    let Some(magic) = caster.magic.as_ref() else {
        return;
    };
    let Some(cv) = magic.caster.as_ref() else {
        return;
    };
    let Some(policy) = k.catalog.spells.caster(caster.definition()) else {
        return;
    };
    let level = caster.combat.as_ref().map_or(1, |c| c.stats.level);
    let cdr = caster
        .combat
        .as_ref()
        .map_or(0, |c| c.stats.cooldown_reduction_permille);
    let spent: u32 = policy
        .bindings
        .iter()
        .enumerate()
        .map(|(i, b)| {
            u32::from(
                cv.ranks
                    .get(i)
                    .copied()
                    .unwrap_or(0)
                    .saturating_sub(b.initial_rank),
            )
        })
        .sum();
    let casting = magic.casting.is_some();
    let actor = caster.id().0;
    for (i, binding) in policy.bindings.iter().enumerate() {
        let Some(content_binding) = authored.bindings.get(i) else {
            continue;
        };
        let Some(command) = &content_binding.command else {
            continue;
        };
        if !binding.allows(canopy_content::abilities::Control::Player) {
            continue;
        }
        let ability = &k.content.abilities.abilities[usize::from(binding.ability.0)];
        let rank = cv.ranks.get(i).copied().unwrap_or(0);
        let max = ability.ranks.len() as u8;
        let icon = command.icon.clone().unwrap_or_else(|| d.icon.clone());
        if let Some(levels) = &binding.learning {
            let mut c = Command::new(
                format!("learn:{}", binding.id),
                &ability.name,
                "",
                &icon,
                60,
                Activate::Immediate {
                    action: Action::LearnAbility {
                        actor,
                        ability: binding.id.clone(),
                    },
                    queue: false,
                },
            );
            let required = levels.get(usize::from(rank)).copied();
            c.description = match required {
                Some(l) if rank < max => format!(
                    "{}\nRank {}/{max} · Requires level {l}",
                    ability.description,
                    rank + 1
                ),
                _ => format!("{}\nRank {rank}/{max} · Fully learned", ability.description),
            };
            c.reason = if rank >= max {
                Some("Fully learned".into())
            } else if casting {
                Some("Casting".into())
            } else if required.is_some_and(|l| level < l) {
                Some(format!("Requires level {}", required.unwrap_or(0)))
            } else if spent >= level {
                Some("No skill points available".into())
            } else {
                None
            };
            c.category = Some("category.hero.skills".into());
            c.column = Some(command.column);
            c.hotkey = Some(command.hotkey.clone());
            c.actors = vec![actor];
            out.push(c);
        }
        if rank == 0 {
            continue;
        }
        let passive = ability.activation == Activation::Passive;
        let mana = library::value(ability, rank, &ability.cast.cost.amount).max(0) as u32;
        let base = library::value(ability, rank, &ability.cast.cooldown.ticks).max(0) as f64;
        let total = (base * f64::from(1000 - cdr.min(1000) as i32) / 1000.0).round() as u32;
        let ready = cv
            .cooldowns
            .iter()
            .find(|(a, _)| *a == binding.ability)
            .map_or(0, |&(_, t)| t);
        let remaining = ready.saturating_sub(k.tick());
        let kind = ability.targeting.kind;
        let activate = match kind {
            TargetingKind::Caster => Activate::Immediate {
                action: Action::CastAbility {
                    actor,
                    binding: binding.id.clone(),
                    target: canopy_sim::action::CastTarget::Unit { entity: actor },
                },
                queue: false,
            },
            TargetingKind::Unit | TargetingKind::Point => Activate::Target(Targeting::Cast {
                actor,
                binding: binding.id.clone(),
                unit: kind == TargetingKind::Unit,
            }),
        };
        let mut c = Command::new(
            format!("cast:{}", binding.id),
            &ability.name,
            "",
            &icon,
            60,
            activate,
        );
        let mut description = ability.description.clone();
        if kind == TargetingKind::Point
            && let Some(radius) = &ability.targeting.radius
        {
            description.push_str(&format!(
                " · radius {}",
                library::value(ability, rank, radius)
            ));
        }
        description.push_str(&format!(
            "\nRank {rank}/{max} · {}s cooldown",
            seconds(total)
        ));
        c.description = description;
        if mana > 0 {
            c.costs.push(CostView {
                icon: mana_icon(),
                amount: mana,
                order: i64::MAX,
            });
        }
        c.cooldown = (remaining > 0).then_some((remaining, total.max(1)));
        c.reason = if passive {
            Some("Passive ability".into())
        } else if casting {
            Some("Casting".into())
        } else if remaining > 0 {
            Some(format!(
                "Ready in {}s",
                (f64::from(remaining) * SECONDS_PER_TICK).ceil()
            ))
        } else if cv.mana < mana {
            Some("Not enough mana".into())
        } else {
            None
        };
        c.placement = Some(ActionPlacement::BottomRow);
        c.column = Some(command.column);
        c.hotkey = Some(command.hotkey.clone());
        if ability.autocast.is_some() {
            let on = cv.autocast.get(i).copied().unwrap_or(false);
            c.autocast = Some(on);
            c.alternate = Some(Action::AbilityAutocast {
                actor,
                binding: binding.id.clone(),
                enabled: !on,
            });
        }
        c.actors = vec![actor];
        out.push(c);
    }
}

/// Every command is unavailable once the match is decided.
fn finish(mut out: Vec<Command>, ended: bool) -> Vec<Command> {
    if ended {
        for c in &mut out {
            c.reason.get_or_insert_with(|| "Match has ended".into());
        }
    }
    out
}

/// Old `commandOrder`: priority descending, learn entries by column, then id.
fn order(a: &Command, b: &Command) -> std::cmp::Ordering {
    b.priority
        .cmp(&a.priority)
        .then_with(|| {
            if a.id.starts_with("learn:") && b.id.starts_with("learn:") {
                a.column.cmp(&b.column)
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .then_with(|| a.id.cmp(&b.id))
}

/// The entries of one menu (old `commandMenu`): commands in `category` and its
/// populated child categories, sorted, with Back first inside a submenu.
pub fn menu(k: &Knowledge, all: &[Command], category: Option<&str>) -> Vec<Command> {
    let cats = &k.content.actions.categories;
    // A category is populated if a command lies in it or in a descendant.
    let populated = |cat: &str| -> bool {
        all.iter().any(|c| {
            let mut at = c.category.clone();
            let mut guard = 0;
            while let Some(x) = at {
                if x == cat {
                    return true;
                }
                at = cats
                    .get(x.as_str())
                    .and_then(|m| m.parent.as_ref())
                    .map(|p| p.to_string());
                guard += 1;
                if guard > 8 {
                    break;
                }
            }
            false
        })
    };
    // A command in an empty or unknown category falls back to the root.
    let home = |c: &Command| -> Option<String> {
        c.category
            .clone()
            .filter(|cat| cats.contains_key(cat.as_str()) && populated(cat))
    };
    let category = category.filter(|c| populated(c));
    let mut entries: Vec<Command> = all
        .iter()
        .filter(|c| home(c).as_deref() == category)
        .cloned()
        .collect();
    for (id, meta) in cats {
        let parent = meta.parent.as_ref().map(|p| p.to_string());
        if parent.as_deref() == category && populated(id.as_str()) {
            let mut c = Command::new(
                format!("category:{id}"),
                &meta.name,
                &meta.description,
                &meta.icon,
                meta.priority,
                Activate::Menu(Some(id.to_string())),
            );
            c.hotkey = meta.hotkey.clone();
            c.placement = meta.placement;
            entries.push(c);
        }
    }
    entries.sort_by(order);
    if let Some(cat) = category {
        let back = &k.content.actions.navigation.back;
        let parent = cats
            .get(cat)
            .and_then(|m| m.parent.as_ref())
            .map(|p| p.to_string());
        let mut c = Command::new(
            "back",
            &back.name,
            &back.description,
            &back.icon,
            back.priority,
            Activate::Menu(parent),
        );
        c.hotkey = back.hotkey.clone();
        entries.insert(0, c);
    }
    entries
}

/// Grid columns and rows of the card.
pub const COLUMNS: usize = 4;
pub const ROWS: usize = 3;
pub const SLOTS: usize = COLUMNS * ROWS;
/// Back's slot (row 3, column 1).
pub const BACK_SLOT: usize = 8;

/// One page of a menu laid out (old `commandLayout`/`commandPage`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout {
    /// Index into the menu's entries for each of the 12 cells.
    pub slots: [Option<usize>; SLOTS],
    /// Full-width banner entries below the grid.
    pub banners: Vec<usize>,
    pub page: usize,
    pub pages: usize,
}

pub fn layout(entries: &[Command], page: usize) -> Layout {
    let back = entries.iter().position(|c| c.id == "back");
    let banners: Vec<usize> = (0..entries.len())
        .filter(|&i| entries[i].placement == Some(ActionPlacement::Banner))
        .collect();
    let bottom: Vec<usize> = (0..entries.len())
        .filter(|&i| entries[i].placement == Some(ActionPlacement::BottomRow))
        .collect();
    let ordinary: Vec<usize> = (0..entries.len())
        .filter(|&i| Some(i) != back && !banners.contains(&i) && !bottom.contains(&i))
        .collect();
    let size = if !bottom.is_empty() {
        8
    } else if back.is_some() {
        11
    } else {
        12
    };
    let bottom_size = if back.is_some() { 3 } else { 4 };
    let pages = 1
        .max(ordinary.len().div_ceil(size))
        .max(bottom.len().div_ceil(bottom_size));
    let page = page.min(pages - 1);
    let mut out = Layout {
        banners,
        page,
        pages,
        ..Layout::default()
    };
    if let Some(b) = back {
        out.slots[BACK_SLOT] = Some(b);
    }
    for (n, &i) in ordinary.iter().skip(page * size).take(size).enumerate() {
        let mut slot = n;
        if back.is_some() && slot >= BACK_SLOT {
            slot += 1;
        }
        if slot < SLOTS {
            out.slots[slot] = Some(i);
        }
    }
    for (n, &i) in bottom
        .iter()
        .skip(page * bottom_size)
        .take(bottom_size)
        .enumerate()
    {
        let column = entries[i]
            .column
            .map_or(1 + usize::from(back.is_some()) + n, |c| c as usize);
        let slot = 2 * COLUMNS + column.clamp(1, COLUMNS) - 1;
        if out.slots[slot].is_none() {
            out.slots[slot] = Some(i);
        }
    }
    out
}

/// The command a key chord selects (old `commandShortcut`): an exact match among the
/// page's entries and the always-available basic orders first; otherwise, with Shift,
/// the unshifted chord (Shift queues). Returns the entry and whether Shift queues.
pub fn shortcut<'a>(
    visible: &'a [Command],
    all: &'a [Command],
    key: &str,
    shift: bool,
) -> Option<(&'a Command, bool)> {
    const ALWAYS: [&str; 5] = ["move", "attack", "stop", "hold", "patrol"];
    let candidates = || {
        visible
            .iter()
            .chain(all.iter().filter(|c| ALWAYS.contains(&c.id.as_str())))
    };
    let chord = if shift {
        format!("Shift+{key}")
    } else {
        key.to_owned()
    };
    let matches = |c: &Command, chord: &str| {
        c.hotkey
            .as_deref()
            .is_some_and(|h| h.eq_ignore_ascii_case(chord))
    };
    if let Some(c) = candidates().find(|c| matches(c, &chord)) {
        return Some((c, false));
    }
    if shift {
        return candidates().find(|c| matches(c, key)).map(|c| (c, true));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(
        id: &str,
        priority: i64,
        placement: Option<ActionPlacement>,
        column: Option<u32>,
    ) -> Command {
        let mut c = Command::new(
            id,
            id,
            "",
            &Id::new("icon").unwrap(),
            priority,
            Activate::Menu(None),
        );
        c.placement = placement;
        c.column = column;
        c
    }

    #[test]
    fn the_layout_fills_rows_reserves_back_and_keeps_spell_columns() {
        let entries: Vec<Command> = (0..5)
            .map(|i| cmd(&format!("c{i}"), 0, None, None))
            .collect();
        let l = layout(&entries, 0);
        assert_eq!(
            &l.slots[..6],
            &[Some(0), Some(1), Some(2), Some(3), Some(4), None]
        );
        assert_eq!(l.pages, 1);

        // Back in slot 9 pushes the ninth ordinary entry past it.
        let mut with_back = vec![cmd("back", 0, None, None)];
        with_back.extend((0..10).map(|i| cmd(&format!("c{i}"), 0, None, None)));
        let l = layout(&with_back, 0);
        assert_eq!(l.slots[BACK_SLOT], Some(0));
        assert_eq!(l.slots[7], Some(8));
        assert_eq!(l.slots[9], Some(9));
        assert_eq!(l.slots[10], Some(10));

        // Bottom-row spells keep their authored columns; ordinary pages hold 8.
        let mut spells: Vec<Command> = (0..9)
            .map(|i| cmd(&format!("o{i}"), 0, None, None))
            .collect();
        spells.push(cmd("cast:a", 0, Some(ActionPlacement::BottomRow), Some(1)));
        spells.push(cmd("cast:b", 0, Some(ActionPlacement::BottomRow), Some(4)));
        spells.push(cmd("banner", 0, Some(ActionPlacement::Banner), None));
        let l = layout(&spells, 0);
        assert_eq!(l.slots[8], Some(9));
        assert_eq!(l.slots[9], None);
        assert_eq!(l.slots[11], Some(10));
        assert_eq!(l.banners, [11]);
        assert_eq!(l.pages, 2);
        let second = layout(&spells, 1);
        assert_eq!(second.slots[0], Some(8));
        assert_eq!(layout(&spells, 9).page, 1);
    }

    #[test]
    fn shortcuts_match_exactly_then_queue_with_shift() {
        let mut attack = cmd("attack", 190, None, None);
        attack.hotkey = Some("A".into());
        let mut hold = cmd("hold", 170, None, None);
        hold.hotkey = Some("Shift+A".into());
        let mut build = cmd("category:build", 60, None, None);
        build.hotkey = Some("B".into());
        let all = vec![attack.clone(), hold.clone(), build.clone()];
        // Inside a submenu only Back and the basic orders are reachable.
        let visible = vec![cmd("back", 0, None, None)];
        assert_eq!(
            shortcut(&visible, &all, "A", false).map(|(c, q)| (c.id.as_str(), q)),
            Some(("attack", false))
        );
        assert_eq!(
            shortcut(&visible, &all, "A", true).map(|(c, q)| (c.id.as_str(), q)),
            Some(("hold", false))
        );
        assert!(shortcut(&visible, &all, "B", false).is_none());
        let root = vec![attack, build];
        assert_eq!(
            shortcut(&root, &all, "B", true).map(|(c, q)| (c.id.as_str(), q)),
            Some(("category:build", true))
        );
    }
}
