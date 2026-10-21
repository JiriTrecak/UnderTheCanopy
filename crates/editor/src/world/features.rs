//! The map panel's fields for what a placement holds or offers beyond where it stands:
//! what a breakable drops, what a shop sells, what a spring restores; and turning a
//! placement (a building by quarter turns, checked as the game checks it).

use super::{Landing, Picked, Placing};
use crate::document::Command;
use crate::map::Session;
use crate::map::view::step;
use crate::tabs::Edit;
use crate::theme::*;
use canopy_content::{
    Content, Id,
    game::{Definition, Kind, Ware},
    placement::{InitialState, Placement},
};
use egui::{RichText, Ui};
use serde_json::{Value, json};

/// Authored ticks per second.
const TICKS: f64 = 40.0;

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(14.0);
    ui.label(crate::widgets::eyebrow(title));
    ui.add_space(4.0);
}

fn note(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).color(MUTED));
}

fn row(ui: &mut Ui, name: &str, value: String) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).size(11.5).color(MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(value).size(12.0).color(TEXT));
        });
    });
}

fn name_of(content: &Content, id: &str) -> String {
    content
        .definition(id)
        .map_or_else(|| id.to_string(), |d| d.name.clone())
}

/// Hero items, by name: what a crate may hold and a shop may sell.
fn hero_items(content: &Content) -> Vec<&Definition> {
    let mut items: Vec<&Definition> = content
        .definitions
        .iter()
        .filter(|d| d.kind == Kind::Item && d.item_effect.is_some())
        .collect();
    items.sort_by(|a, b| (a.item_tier, &a.name).cmp(&(b.item_tier, &b.name)));
    items
}

/// The placement's `initialState` replaced by `state` (removed when empty), one step.
fn set_state(
    session: &Session,
    index: usize,
    entity: &Placement,
    label: &str,
    state: &InitialState,
    merge: Option<&str>,
) -> Edit {
    let document = session.path("map.json");
    let pointer = format!("/entities/{index}/initialState");
    let value = serde_json::to_value(state).unwrap_or(Value::Null);
    let empty = value.as_object().is_none_or(|o| o.is_empty());
    let command = if empty {
        Command::Remove { document, pointer }
    } else {
        Command::Set {
            document,
            pointer,
            value,
            at: None,
        }
    };
    let mut edit = step(session, label, vec![command]);
    if empty && entity.initial_state.is_none() {
        edit.commands.clear();
    }
    edit.merge = merge.map(|m| format!("{}/entities/{index}/{m}", session.root));
    edit
}

/// The fields for a placement of `definition`: drops, wares, a spring's reach.
pub fn show(
    ui: &mut Ui,
    session: &Session,
    content: &Content,
    index: usize,
    entity: &Placement,
    definition: &Definition,
    edits: &mut Vec<Edit>,
) {
    if definition.breakable.is_some() {
        drops(ui, session, content, index, entity, definition, edits);
    }
    if definition.shop.is_some() {
        wares(ui, session, content, index, entity, definition, edits);
    }
    if let Some(r) = &definition.restoration {
        section(ui, "Spring");
        row(ui, "Reach", format!("{} cells", r.radius));
        row(
            ui,
            "Every",
            format!("{} s", f64::from(r.interval_ticks) / TICKS),
        );
        let amount = |flat: Option<u32>, permille: Option<u32>| {
            let mut parts = Vec::new();
            if let Some(f) = flat.filter(|&f| f > 0) {
                parts.push(format!("+{f}"));
            }
            if let Some(p) = permille.filter(|&p| p > 0) {
                parts.push(format!("+{}%", f64::from(p) / 10.0));
            }
            parts.join(" ")
        };
        let health = amount(r.health, r.health_permille);
        if !health.is_empty() {
            row(ui, "Health", health);
        }
        let mana = amount(r.mana, r.mana_permille);
        if !mana.is_empty() {
            row(ui, "Mana", mana);
        }
        note(ui, "Restores every unit near it, of any owner.");
    }
    if definition.kind == Kind::Item {
        section(ui, "On the ground");
        note(ui, "Lies here until a hero picks it up.");
    }
}

/// What a breakable drops: the items it holds, else a roll of a loot pool.
fn drops(
    ui: &mut Ui,
    session: &Session,
    content: &Content,
    index: usize,
    entity: &Placement,
    definition: &Definition,
    edits: &mut Vec<Edit>,
) {
    let own = definition.breakable.as_ref().expect("breakable");
    section(ui, "Drops");
    let state = entity.initial_state.clone().unwrap_or_default();
    let held: Vec<(Id, u32)> = state
        .inventory
        .iter()
        .flatten()
        .map(|(i, &n)| (i.clone(), n))
        .collect();
    let usual = match (&own.loot_pool, &own.drops) {
        (Some(pool), _) => format!("Its own: {pool}"),
        (None, Some(items)) if !items.is_empty() => format!(
            "Its own: {}",
            items
                .iter()
                .map(|i| name_of(content, i.as_str()))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => "Nothing".to_string(),
    };
    ui.add_enabled_ui(held.is_empty(), |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Loot pool").size(11.5).color(MUTED));
            let mut chosen = state.loot_pool.clone();
            egui::ComboBox::from_id_salt(("loot", index))
                .selected_text(chosen.as_ref().map_or(usual.clone(), |p| p.to_string()))
                .width(170.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut chosen, None, usual.as_str());
                    for pool in content.rules.loot_pools.keys() {
                        ui.selectable_value(&mut chosen, Some(pool.clone()), pool.as_str());
                    }
                });
            if chosen != state.loot_pool {
                let mut next = state.clone();
                next.loot_pool = chosen;
                edits.push(set_state(session, index, entity, "Loot pool", &next, None));
            }
        });
    });
    for (item, count) in &held {
        ui.horizontal(|ui| {
            ui.label(RichText::new(name_of(content, item.as_str())).color(TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let remove = ui.small_button("×").on_hover_text("Drops none").clicked();
                let mut n = *count;
                ui.add(egui::DragValue::new(&mut n).range(1..=9).prefix("× "));
                if remove || n != *count {
                    let mut next = state.clone();
                    let inventory = next.inventory.get_or_insert_with(Default::default);
                    if remove {
                        inventory.remove(item);
                    } else {
                        inventory.insert(item.clone(), n);
                    }
                    if inventory.is_empty() {
                        next.inventory = None;
                    }
                    edits.push(set_state(
                        session,
                        index,
                        entity,
                        "Drops",
                        &next,
                        (!remove).then_some("drops"),
                    ));
                }
            });
        });
    }
    let mut added: Option<Id> = None;
    egui::ComboBox::from_id_salt(("drop", index))
        .selected_text("Add an item…")
        .width(200.0)
        .show_ui(ui, |ui| {
            for d in hero_items(content) {
                if ui
                    .selectable_label(
                        false,
                        format!("{} (tier {})", d.name, d.item_tier.unwrap_or(1)),
                    )
                    .clicked()
                {
                    added = Some(d.id.clone());
                }
            }
        });
    if let Some(item) = added {
        let mut next = state.clone();
        *next
            .inventory
            .get_or_insert_with(Default::default)
            .entry(item)
            .or_insert(0) += 1;
        next.loot_pool = None;
        edits.push(set_state(session, index, entity, "Drops", &next, None));
    }
    note(
        ui,
        if held.is_empty() {
            "Dropped when it breaks: a roll of the pool when the match starts."
        } else {
            "Dropped when it breaks: these items, in place of any pool."
        },
    );
}

/// What a shop sells: its definition's wares, or the placement's own.
fn wares(
    ui: &mut Ui,
    session: &Session,
    content: &Content,
    index: usize,
    entity: &Placement,
    definition: &Definition,
    edits: &mut Vec<Edit>,
) {
    let usual = &definition.shop.as_ref().expect("shop").wares;
    section(ui, "Wares");
    let state = entity.initial_state.clone().unwrap_or_default();
    let Some(own) = state.wares.clone() else {
        for ware in usual {
            ware_summary(ui, content, ware);
        }
        if ui
            .button("Sell other wares here")
            .on_hover_text("This merchant's own list, starting from the usual one")
            .clicked()
        {
            let mut next = state.clone();
            next.wares = Some(usual.clone());
            edits.push(set_state(session, index, entity, "Wares", &next, None));
        }
        note(ui, "The merchant's usual wares.");
        return;
    };
    let mut next = own.clone();
    let mut removed = None;
    for (i, ware) in next.iter_mut().enumerate() {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(name_of(content, ware.item.as_str()))
                    .color(TEXT)
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("×")
                    .on_hover_text("Not sold here")
                    .clicked()
                {
                    removed = Some(i);
                }
            });
        });
        ui.horizontal(|ui| {
            for cost in &mut ware.price {
                ui.add(
                    egui::DragValue::new(&mut cost.amount)
                        .range(0..=5000)
                        .speed(5.0)
                        .suffix(format!(" {}", name_of(content, cost.item.as_str()))),
                );
            }
            ui.add(
                egui::DragValue::new(&mut ware.stock)
                    .range(1..=9)
                    .prefix("stock "),
            );
            let mut seconds = f64::from(ware.restock_ticks) / TICKS;
            if ui
                .add(
                    egui::DragValue::new(&mut seconds)
                        .range(1.0..=600.0)
                        .speed(1.0)
                        .suffix(" s"),
                )
                .on_hover_text("Time to grow one back after a sale")
                .changed()
            {
                ware.restock_ticks = (seconds * TICKS).round().max(1.0) as u32;
            }
        });
    }
    if let Some(i) = removed {
        next.remove(i);
    }
    let currency = usual
        .iter()
        .flat_map(|w| &w.price)
        .map(|c| c.item.clone())
        .next();
    let mut added: Option<Id> = None;
    egui::ComboBox::from_id_salt(("ware", index))
        .selected_text("Add a ware…")
        .width(200.0)
        .show_ui(ui, |ui| {
            for d in hero_items(content) {
                if next.iter().any(|w| w.item == d.id) {
                    continue;
                }
                if ui
                    .selectable_label(
                        false,
                        format!("{} (tier {})", d.name, d.item_tier.unwrap_or(1)),
                    )
                    .clicked()
                {
                    added = Some(d.id.clone());
                }
            }
        });
    if let Some(item) = added {
        next.push(Ware {
            item,
            price: currency
                .clone()
                .map(|c| {
                    vec![canopy_content::game::Cost {
                        item: c,
                        amount: 100,
                    }]
                })
                .unwrap_or_default(),
            stock: 1,
            restock_ticks: 2400,
        });
    }
    if next != own {
        let mut state = state.clone();
        let structural = next.len() != own.len();
        state.wares = Some(next);
        edits.push(set_state(
            session,
            index,
            entity,
            "Wares",
            &state,
            (!structural).then_some("wares"),
        ));
    }
    ui.add_space(6.0);
    if ui.button("Sell the usual wares").clicked() {
        let mut state = state.clone();
        state.wares = None;
        edits.push(set_state(session, index, entity, "Wares", &state, None));
    }
}

fn ware_summary(ui: &mut Ui, content: &Content, ware: &Ware) {
    let price = ware
        .price
        .iter()
        .map(|c| format!("{} {}", c.amount, name_of(content, c.item.as_str())))
        .collect::<Vec<_>>()
        .join(", ");
    row(
        ui,
        &name_of(content, ware.item.as_str()),
        format!(
            "{price} · {} · {} s",
            ware.stock,
            f64::from(ware.restock_ticks) / TICKS
        ),
    );
}

/// Buttons that turn the placement: a building a quarter turn (its centre kept on the
/// building lattice, refused where it would not stand), anything else an eighth.
pub fn turn(
    ui: &mut Ui,
    session: &mut Session,
    content: &Content,
    index: usize,
    entity: &Placement,
    edits: &mut Vec<Edit>,
) {
    let Some(definition) = content.definition(entity.definition.as_str()) else {
        return;
    };
    let building = definition.kind == Kind::Building;
    let step_by = if building { 90.0 } else { 45.0 };
    let terrain = super::sim_terrain(session, content);
    let Some(map) = session.map.clone() else {
        return;
    };
    let option = |by: f64| {
        let rotation = (entity.rotation + by).rem_euclid(360.0);
        let (mut x, mut z) = (entity.position.x, entity.position.y);
        if let Some(f) = definition.footprint.as_ref().filter(|_| building) {
            let quarter = (rotation / 90.0).round() as i64;
            let (w, d) = if quarter % 2 == 1 {
                (f.depth, f.width)
            } else {
                (f.width, f.depth)
            };
            (x, z) = super::on_lattice(x, z, w, d);
        }
        let problem = terrain.as_ref().and_then(|t| {
            super::problem(
                &map,
                content,
                t,
                &Placing::Definition(entity.definition.to_string()),
                &Landing {
                    x,
                    z,
                    footprint: None,
                    rotation,
                },
                &[(Picked::Entity, entity.id.clone())],
            )
        });
        (rotation, x, z, problem)
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new("Turn").size(11.5).color(MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for (label, by) in [("⟳", step_by), ("⟲", -step_by)] {
                let (rotation, x, z, problem) = option(by);
                let button = ui
                    .add_enabled(problem.is_none(), egui::Button::new(label).small())
                    .on_hover_text(format!("Turn {by:+}°"))
                    .on_disabled_hover_text(problem.unwrap_or_default());
                if button.clicked() {
                    let document = session.path("map.json");
                    let mut commands = vec![Command::Set {
                        document: document.clone(),
                        pointer: format!("/entities/{index}/rotation"),
                        value: json!(rotation),
                        at: None,
                    }];
                    if (x, z) != (entity.position.x, entity.position.y) {
                        commands.push(Command::Set {
                            document,
                            pointer: format!("/entities/{index}/position"),
                            value: json!({"x": x, "y": z}),
                            at: None,
                        });
                    }
                    edits.push(step(session, "Turn", commands));
                }
            }
        });
    });
}
