//! The block editor's tool strip: a floating, bordered bar of tool icons at the foot of
//! the view, and above it a second strip with the chosen tool's settings (its brush, the
//! kinds and faces it lays down, what Place puts down, what Select holds).

use super::{BlockTool, Placing, mesh};
use crate::icons::{self, Icon};
use crate::map::Session;
use crate::map::tools::{Picked, Tier};
use crate::theme::*;
use canopy_content::map::blocks::{Block, BlockKind};
use egui::{Align2, Color32, Id, Order, Rect, RichText, Sense, Stroke, Ui, vec2};

/// The tools in their groups: shaping the blocks, their surface, things on the map.
const GROUPS: [&[BlockTool]; 3] = [
    &[
        BlockTool::Raise,
        BlockTool::Level,
        BlockTool::Paint,
        BlockTool::Ramp,
        BlockTool::Cut,
        BlockTool::Round,
    ],
    &[BlockTool::Cells, BlockTool::Face],
    &[BlockTool::Place, BlockTool::Select],
];

fn icon(tool: BlockTool) -> Icon {
    match tool {
        BlockTool::Raise => Icon::Raise,
        BlockTool::Level => Icon::Level,
        BlockTool::Paint => Icon::Paint,
        BlockTool::Ramp => Icon::Ramp,
        BlockTool::Cut => Icon::Cut,
        BlockTool::Cells => Icon::Cells,
        BlockTool::Face => Icon::Face,
        BlockTool::Place => Icon::Place,
        BlockTool::Round => Icon::Round,
        BlockTool::Select => Icon::Select,
    }
}

/// A floating, bordered strip.
fn frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .shadow(egui::Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(140),
        })
}

/// One tool's button: its icon, highlighted when chosen; its name, key and use on hover.
fn tool_button(ui: &mut Ui, tool: BlockTool, on: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(38.0, 38.0), Sense::click());
    let hovered = response.hovered();
    let painter = ui.painter();
    if on {
        painter.rect(
            rect,
            7,
            SELECTED,
            Stroke::new(1.0, SOFT),
            egui::StrokeKind::Inside,
        );
    } else if hovered {
        painter.rect_filled(rect, 7, RAISED);
    }
    let color = if on {
        TEXT
    } else if hovered {
        SOFT
    } else {
        MUTED
    };
    icons::paint(
        painter,
        icons::square(rect.center(), 20.0),
        icon(tool),
        color,
    );
    response
        .on_hover_text(format!(
            "{}   {:?}\n{}",
            tool.name(),
            tool.key(),
            tool.hint()
        ))
        .clicked()
}

/// A segment of a choice.
fn segment(ui: &mut Ui, on: bool, text: &str) -> bool {
    ui.selectable_label(on, RichText::new(text).size(12.0))
        .clicked()
}

fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, 26.0), Sense::hover());
    ui.painter()
        .vline(rect.center().x, rect.y_range(), Stroke::new(1.0, BORDER));
}

/// A block kind's colour in the block view, for its swatch.
fn swatch(kind: BlockKind) -> Color32 {
    let c = mesh::color(Block {
        level: 2,
        kind,
        ..Block::default()
    });
    let s = |v: f32| (v.powf(1.0 / 2.2) * 255.0) as u8;
    Color32::from_rgb(s(c[0]), s(c[1]), s(c[2]))
}

/// The kinds Paint and Cells lay down, as chips with their colour.
fn kinds(ui: &mut Ui, chosen: &mut BlockKind) {
    for (n, kind) in BlockKind::ALL.iter().enumerate() {
        if *kind == BlockKind::Ramp {
            continue;
        }
        let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
        ui.painter().rect(
            rect,
            2,
            swatch(*kind),
            Stroke::new(1.0, if *chosen == *kind { TEXT } else { BORDER }),
            egui::StrokeKind::Inside,
        );
        if ui
            .selectable_label(*chosen == *kind, RichText::new(kind.name()).size(12.0))
            .on_hover_text(format!("{}", n + 1))
            .clicked()
        {
            *chosen = *kind;
        }
        ui.add_space(4.0);
    }
}

/// How an area tool takes its blocks: a box, a brush stroke, or a lasso; and the brush's
/// size.
fn shape(ui: &mut Ui, session: &mut Session, cells: bool) {
    let state = &mut session.blocks;
    let options: &[(&str, bool, bool)] = if cells {
        &[("Stroke", true, false), ("Lasso", false, true)]
    } else {
        &[
            ("Box", false, false),
            ("Brush", true, false),
            ("Lasso", false, true),
        ]
    };
    for &(name, brush, lasso) in options {
        let on = state.lasso == lasso && (lasso || cells || state.brush == brush);
        if segment(ui, on, name) {
            state.brush = brush;
            state.lasso = lasso;
        }
    }
    if cells && !state.lasso {
        divider(ui);
        let mut r = state.cell_radius;
        ui.add(egui::Slider::new(&mut r, 1..=16).text("cells"));
        state.cell_radius = r;
    } else if state.brush && !state.lasso {
        divider(ui);
        let mut r = state.radius;
        ui.add(egui::Slider::new(&mut r, 0..=8).text("blocks"));
        state.radius = r;
    }
}

fn note(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.5).color(MUTED));
}

/// The chosen tool's settings.
fn settings(
    ui: &mut Ui,
    session: &mut Session,
    docs: Option<&crate::map::view::Docs>,
) -> Option<crate::tabs::Edit> {
    let tool = session.blocks.tool;
    ui.label(RichText::new(tool.name()).size(12.5).color(TEXT).strong());
    divider(ui);
    let mut edit = None;
    match tool {
        BlockTool::Raise => {
            shape(ui, session, false);
            divider(ui);
            note(ui, "Shift lowers");
        }
        BlockTool::Level => {
            shape(ui, session, false);
            divider(ui);
            note(ui, "takes the level of the block the drag begins on");
        }
        BlockTool::Paint => {
            shape(ui, session, false);
            divider(ui);
            let mut kind = session.blocks.kind;
            kinds(ui, &mut kind);
            session.blocks.kind = kind;
        }
        BlockTool::Round => {
            shape(ui, session, false);
            divider(ui);
            note(ui, "Shift uncuts");
        }
        BlockTool::Face => {
            shape(ui, session, false);
            divider(ui);
            let faces = session.blocks.faces.clone();
            if faces.is_empty() {
                note(ui, "this biome has one cliff face");
            }
            for (n, name) in faces.iter().enumerate() {
                let style = n as u8 + 1;
                if segment(ui, session.blocks.style == style, name) {
                    session.blocks.style = style;
                }
            }
        }
        BlockTool::Ramp => {
            let mut r = session.blocks.radius;
            ui.add(egui::Slider::new(&mut r, 0..=8).text("blocks each side"));
            session.blocks.radius = r;
            divider(ui);
            note(ui, "drag from a cliff's top down to its foot");
        }
        BlockTool::Cut => note(ui, "click near a block's corner; again removes the cut"),
        BlockTool::Cells => {
            shape(ui, session, true);
            divider(ui);
            let mut kind = session.blocks.cell_kind;
            kinds(ui, &mut kind);
            session.blocks.cell_kind = kind;
        }
        BlockTool::Place => {
            for (placing, name) in [
                (Placing::Start, "Start"),
                (Placing::Amber, "Amber site"),
                (Placing::Camp, "Creep camp"),
            ] {
                if segment(ui, session.blocks.placing == placing, name) {
                    session.blocks.placing = placing;
                }
            }
            divider(ui);
            let t = &mut session.tools;
            match session.blocks.placing {
                Placing::Start | Placing::Amber => {
                    ui.add(
                        egui::Slider::new(&mut t.amber, 500..=6000)
                            .step_by(100.0)
                            .text("amber a node"),
                    );
                    if session.blocks.placing == Placing::Start {
                        divider(ui);
                        note(ui, "with its amber, towards the middle");
                    }
                }
                Placing::Camp => {
                    egui::ComboBox::from_id_salt(("camp-tier", &session.root))
                        .selected_text(t.tier.name())
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            for tier in Tier::ALL {
                                ui.selectable_value(&mut t.tier, tier, tier.name());
                            }
                        });
                    note(ui, "strength");
                }
            }
        }
        BlockTool::Select => match session.tools.selected.clone() {
            Some((kind, id)) => {
                let what = match kind {
                    Picked::Start => format!("start of player {id}"),
                    Picked::Entity => id.clone(),
                    Picked::Object => format!("prop {id}"),
                };
                ui.label(RichText::new(what).size(12.0).color(TEXT));
                divider(ui);
                note(ui, "drag it to move it");
                if ui.button("Delete").clicked()
                    && let Some(docs) = docs
                {
                    let commands = crate::map::view::delete(session, docs, kind, &id);
                    if !commands.is_empty() {
                        edit = Some(crate::map::view::step(session, "Delete", commands));
                        session.tools.selected = None;
                    }
                }
            }
            None => note(ui, "click a start, amber site, camp or prop"),
        },
    }
    edit
}

/// The strips over `view`, the map's viewport: the tool bar at its foot, the chosen
/// tool's settings above it. Returns an edit when a setting made one (Select's Delete).
pub(super) fn show(
    ctx: &egui::Context,
    session: &mut Session,
    view: Rect,
    docs: Option<&crate::map::view::Docs>,
) -> Option<crate::tabs::Edit> {
    let bar = egui::Area::new(Id::new(("block-tools", &session.root)))
        .order(Order::Foreground)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(view.center_bottom() - vec2(0.0, 16.0))
        .constrain_to(view)
        .show(ctx, |ui| {
            frame().show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
                ui.horizontal(|ui| {
                    for (g, group) in GROUPS.iter().enumerate() {
                        if g > 0 {
                            divider(ui);
                        }
                        for &tool in *group {
                            if tool_button(ui, tool, session.blocks.tool == tool) {
                                session.blocks.tool = tool;
                            }
                        }
                    }
                });
            });
        });
    let top = bar.response.rect.top();
    egui::Area::new(Id::new(("block-tool-settings", &session.root)))
        .order(Order::Foreground)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(egui::pos2(view.center().x, top - 8.0))
        .constrain_to(view)
        .show(ctx, |ui| {
            frame()
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
                    ui.horizontal(|ui| {
                        ui.set_min_height(26.0);
                        settings(ui, session, docs)
                    })
                    .inner
                })
                .inner
        })
        .inner
}
