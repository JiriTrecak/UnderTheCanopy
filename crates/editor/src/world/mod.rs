//! The block editor (D80): a map is shaped as columns of blocks, 4 × 4 cells each, and
//! its heights, water and closed cells are made from them. This first version is the
//! blockout alone, to get the feel: raise and lower, set a level, paint a kind, ramps and
//! diagonal cuts, drawn as blocks. The generated look comes later.
//!
//! Every edit changes the blocks, makes the map's grids from them (`Blocks::apply`) and
//! becomes one undo step: the blocks and closed cells whole, heights and water as patches
//! over what changed.

pub mod mesh;

use crate::document::{Command, Sample};
use crate::map::Session;
use crate::map::view::{Out, matrices, navigate, pick, project, step};
use crate::theme::*;
use crate::widgets::eyebrow;
use canopy_content::map::MAP_HALO;
use canopy_content::map::blocks::{BLOCK, Block, BlockKind, Blocks, Cut, MAX_LEVEL};
use egui::{Color32, Rect, RichText, Sense, Stroke as Line, Ui, pos2};
use glam::Vec3;

/// The block editor's tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlockTool {
    /// Each block in the dragged area one level up (Shift: down).
    #[default]
    Raise,
    /// The dragged area to the level of the block the drag began on.
    Level,
    /// The dragged area to the chosen kind.
    Paint,
    /// Ramps (Shift: back to ground).
    Ramp,
    /// A diagonal cut at the clicked corner (again or Shift: removed).
    Cut,
}

impl BlockTool {
    const ALL: [BlockTool; 5] = [
        BlockTool::Raise,
        BlockTool::Level,
        BlockTool::Paint,
        BlockTool::Ramp,
        BlockTool::Cut,
    ];

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|t| t.name().eq_ignore_ascii_case(name))
    }

    fn name(self) -> &'static str {
        match self {
            BlockTool::Raise => "Raise",
            BlockTool::Level => "Level",
            BlockTool::Paint => "Paint",
            BlockTool::Ramp => "Ramp",
            BlockTool::Cut => "Cut",
        }
    }

    fn key(self) -> egui::Key {
        match self {
            BlockTool::Raise => egui::Key::Q,
            BlockTool::Level => egui::Key::W,
            BlockTool::Paint => egui::Key::E,
            BlockTool::Ramp => egui::Key::R,
            BlockTool::Cut => egui::Key::T,
        }
    }

    fn hint(self) -> &'static str {
        match self {
            BlockTool::Raise => "Drag over blocks to raise them a level (Shift lowers)",
            BlockTool::Level => "Drag from a block: the area takes its level",
            BlockTool::Paint => "Drag over blocks to make them the chosen kind",
            BlockTool::Ramp => "Drag over blocks at a cliff's top to make ramps (Shift removes)",
            BlockTool::Cut => "Click near a block's corner to cut it diagonally (again removes)",
        }
    }
}

/// The block editor's state in a map's session.
#[derive(Debug, Clone, Default)]
pub struct State {
    pub tool: BlockTool,
    pub kind: BlockKind,
    /// The column a drag began on, and where it is now.
    pub drag: Option<((i32, i32), (i32, i32))>,
}

/// The map's blocks, when it has them.
pub fn blocks_of(session: &Session) -> Option<Blocks> {
    let map = session.map.as_ref()?;
    let terrain = &map.authoring.terrain;
    Blocks::decode(terrain.blocks.as_ref()?, terrain.size).ok()
}

/// Changes the map's blocks with `change`, makes its grids from them and returns the undo
/// step; `blocks` is what the map holds now (or the blocks to start from).
fn commit(
    session: &mut Session,
    label: &str,
    mut blocks: Blocks,
    change: impl FnOnce(&mut Blocks),
) -> Option<crate::tabs::Edit> {
    let root = session.root.clone();
    let map = session.map.as_mut()?;
    let terrain = &mut map.authoring.terrain;
    let before = (
        terrain.heights.0.clone(),
        terrain.water_heights.0.clone(),
        terrain.blocks.clone(),
    );
    change(&mut blocks);
    blocks.apply(terrain);
    if terrain.blocks == before.2 {
        return None;
    }
    let vertices = terrain.vertices();
    let span = terrain.span();
    let columns = span / BLOCK;
    let mut commands = vec![
        Command::Grid {
            document: format!("{root}/terrain/blocks.grid"),
            element: Sample::Rgba8,
            width: columns,
            height: columns,
            bytes: terrain.blocks.clone().unwrap_or_default().0,
        },
        Command::Grid {
            document: format!("{root}/terrain/blocked.grid"),
            element: Sample::U8,
            width: span,
            height: span,
            bytes: terrain.blocked.clone().unwrap_or_default().0,
        },
    ];
    for (file, old, new) in [
        ("terrain/heights.grid", &before.0, &terrain.heights.0),
        (
            "terrain/water-heights.grid",
            &before.1,
            &terrain.water_heights.0,
        ),
    ] {
        if let Some(patch) = patch(&root, file, old, new, vertices) {
            commands.push(patch);
        }
    }
    session.own = true;
    session.stale = true;
    Some(step(session, label, commands))
}

/// Blocks out the session's map from its heights, water and closed ground.
pub fn block_out(session: &mut Session) -> Option<crate::tabs::Edit> {
    let map = session.map.as_ref()?;
    let starts: Vec<[f32; 2]> = map
        .player_starts
        .iter()
        .map(|s| [s.x as f32, s.z as f32])
        .collect();
    let blocks = Blocks::from_terrain(&map.authoring.terrain, &starts);
    commit(session, "Block out the map", blocks, |_| {})
}

/// The rectangle of 4-byte samples where `new` differs from `old`, as a patch.
fn patch(root: &str, file: &str, old: &[u8], new: &[u8], side: u32) -> Option<Command> {
    let (mut x0, mut z0, mut x1, mut z1) = (u32::MAX, u32::MAX, 0, 0);
    for (i, (a, b)) in old.chunks_exact(4).zip(new.chunks_exact(4)).enumerate() {
        if a != b {
            let (x, z) = (i as u32 % side, i as u32 / side);
            x0 = x0.min(x);
            z0 = z0.min(z);
            x1 = x1.max(x);
            z1 = z1.max(z);
        }
    }
    if x0 == u32::MAX {
        return None;
    }
    let (width, height) = (x1 - x0 + 1, z1 - z0 + 1);
    let mut bytes = Vec::with_capacity((width * height * 4) as usize);
    for z in z0..=z1 {
        let at = (z * side + x0) as usize * 4;
        bytes.extend_from_slice(&new[at..at + width as usize * 4]);
    }
    Some(Command::Patch {
        document: format!("{root}/{file}"),
        x: x0,
        z: z0,
        width,
        height,
        bytes,
    })
}

/// The columns of a drag, inclusive.
fn area(a: (i32, i32), b: (i32, i32)) -> impl Iterator<Item = (i32, i32)> {
    let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
    let (z0, z1) = (a.1.min(b.1), a.1.max(b.1));
    (z0..=z1).flat_map(move |z| (x0..=x1).map(move |x| (x, z)))
}

fn tool_button(ui: &mut Ui, label: &str, key: &str, on: bool) -> bool {
    let text = RichText::new(format!("{label}   {key}"))
        .size(12.0)
        .color(if on { TEXT } else { MUTED });
    ui.add_sized(
        [ui.available_width(), 26.0],
        egui::Button::new(text)
            .fill(if on { RAISED } else { PANEL })
            .stroke(Line::new(1.0, if on { SOFT } else { BORDER })),
    )
    .clicked()
}

/// The left panel; returns an edit when one of its buttons made one.
fn panel(ui: &mut Ui, session: &mut Session) -> Option<crate::tabs::Edit> {
    let mut edit = None;
    ui.label(eyebrow("Blocks"));
    ui.label(
        RichText::new(
            "Shape the map in 4 × 4-cell blocks; heights, cliffs, water and closed ground follow.",
        )
        .size(11.0)
        .color(MUTED),
    );
    ui.add_space(8.0);
    let has = blocks_of(session).is_some();
    if !has {
        ui.label(
            RichText::new("This map has no blocks yet.")
                .size(12.0)
                .color(TEXT),
        );
        ui.add_space(4.0);
        if ui.button("Block out this map").on_hover_text("Blocks from the map's heights, water and closed ground; the map's heights then follow the blocks").clicked() {
            edit = block_out(session);
        }
        if ui.button("Start from flat ground").clicked()
            && let Some(map) = &session.map
        {
            let blocks = Blocks::flat(map.size);
            edit = commit(session, "Flat blocks", blocks, |_| {});
        }
        ui.add_space(10.0);
    }
    ui.label(eyebrow("Tools"));
    for tool in BlockTool::ALL {
        let key = format!("{:?}", tool.key());
        if tool_button(ui, tool.name(), &key, session.blocks.tool == tool) {
            session.blocks.tool = tool;
        }
    }
    ui.add_space(6.0);
    ui.label(
        RichText::new(session.blocks.tool.hint())
            .size(11.0)
            .color(MUTED),
    );
    if session.blocks.tool == BlockTool::Paint {
        ui.add_space(8.0);
        ui.label(eyebrow("Kind"));
        for (n, kind) in BlockKind::ALL.iter().enumerate() {
            if *kind == BlockKind::Ramp {
                continue;
            }
            let c = mesh::color(Block {
                level: 2,
                kind: *kind,
                cut: Cut::None,
            });
            let swatch = Color32::from_rgb(
                (c[0].powf(1.0 / 2.2) * 255.0) as u8,
                (c[1].powf(1.0 / 2.2) * 255.0) as u8,
                (c[2].powf(1.0 / 2.2) * 255.0) as u8,
            );
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
                ui.painter().rect_filled(rect, 2, swatch);
                let on = session.blocks.kind == *kind;
                if ui
                    .selectable_label(on, format!("{}  {}", kind.name(), n + 1))
                    .clicked()
                {
                    session.blocks.kind = *kind;
                }
            });
        }
    }
    ui.add_space(12.0);
    ui.label(eyebrow("Editor"));
    if ui
        .button("Classic map editor")
        .on_hover_text(
            "The brush editor this one replaces, while it still covers starts, amber and camps",
        )
        .clicked()
    {
        session.editor = crate::map::Editor::Classic;
        session.stale = true;
    }
    edit
}

/// The block view of `session`; `texture` is the scene's last picture.
pub fn show(ui: &mut Ui, session: &mut Session, texture: Option<egui::TextureId>) -> Out {
    let mut out = Out::default();
    let mut panel_edit = None;
    egui::Panel::left(egui::Id::new(("block-tools", &session.root)))
        .exact_size(214.0)
        .resizable(false)
        .show_separator_line(true)
        .frame(
            egui::Frame::new()
                .fill(PANEL)
                .inner_margin(egui::Margin::same(10)),
        )
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| panel_edit = panel(ui, session));
        });
    out.edits.extend(panel_edit);
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0, RAISED);
    let Some(map) = session.map.as_ref() else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Loading the map…",
            egui::FontId::proportional(12.0),
            FAINT,
        );
        return out;
    };
    if let Some(texture) = texture {
        painter.image(
            texture,
            rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    let scale = ui.ctx().pixels_per_point();
    out.size = Some([
        (rect.width() * scale) as u32,
        (rect.height() * scale) as u32,
    ]);
    let map_size = map.size;
    let ground =
        crate::map::tools::ground_at(map, session.camera.focus[0], session.camera.focus[1]);
    let (eye, _, view_projection) = matrices(
        &session.camera,
        ground,
        rect.width() / rect.height().max(1.0),
    );
    let input = ui.input(|i| i.clone());
    navigate(&input, &response, rect, &mut session.camera, map_size);
    if response.hovered() && !input.modifiers.command {
        for tool in BlockTool::ALL {
            if input.key_pressed(tool.key()) {
                session.blocks.tool = tool;
            }
        }
        for (n, key) in [
            egui::Key::Num1,
            egui::Key::Num2,
            egui::Key::Num3,
            egui::Key::Num4,
            egui::Key::Num5,
            egui::Key::Num6,
        ]
        .iter()
        .enumerate()
        {
            if input.key_pressed(*key) && BlockKind::ALL[n] != BlockKind::Ramp {
                session.blocks.kind = BlockKind::ALL[n];
                session.blocks.tool = BlockTool::Paint;
            }
        }
    }
    let Some(blocks) = blocks_of(session) else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Block out the map, or start from flat ground (left)",
            egui::FontId::proportional(13.0),
            MUTED,
        );
        return out;
    };
    let pointer = response.hover_pos().or(response.interact_pointer_pos());
    // Picking: the ground under the pointer, nudged a little back towards the camera so a
    // block's top is chosen rather than the foot of the cliff below it.
    let hit = pointer
        .and_then(|p| pick(session, view_projection, eye, rect, p))
        .map(|p| {
            let back = (eye - p).normalize_or_zero();
            p + back * 0.15
        });
    let column = hit.map(|p| (blocks.column_at(p.x, p.z), p));
    let shift = input.modifiers.shift;

    // A block's top outline, projected.
    let outline = |(x, z): (i32, i32), (x2, z2): (i32, i32), color: Color32| {
        let b = BLOCK as f32;
        let halo = MAP_HALO as f32;
        let (x0, x1) = (x.min(x2), x.max(x2) + 1);
        let (z0, z1) = (z.min(z2), z.max(z2) + 1);
        let y = area((x, z), (x2, z2))
            .map(|(cx, cz)| mesh::top(blocks.at(cx, cz)))
            .fold(f32::MIN, f32::max)
            + 0.05;
        let corners = [(x0, z0), (x1, z0), (x1, z1), (x0, z1), (x0, z0)];
        let points: Vec<_> = corners
            .iter()
            .filter_map(|&(cx, cz)| {
                project(
                    view_projection,
                    rect,
                    Vec3::new(cx as f32 * b - halo, y, cz as f32 * b - halo),
                )
            })
            .collect();
        if points.len() == 5 {
            painter.add(egui::Shape::line(points, Line::new(2.0, color)));
        }
    };

    match (session.blocks.drag, column) {
        (Some((from, _)), Some((c, _))) => {
            session.blocks.drag = Some((from, c));
            outline(from, c, SOFT);
        }
        (None, Some((c, _))) => outline(c, c, Color32::from_white_alpha(200)),
        _ => {}
    }

    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some((c, _)) = column
    {
        session.blocks.drag = Some((c, c));
    }
    let mut finished = None;
    if response.drag_stopped_by(egui::PointerButton::Primary) {
        finished = session.blocks.drag.take();
    } else if response.clicked_by(egui::PointerButton::Primary)
        && let Some((c, _)) = column
    {
        finished = Some((c, c));
    }
    if let Some((from, to)) = finished {
        let tool = session.blocks.tool;
        let kind = session.blocks.kind;
        let hit_point = column.map(|(_, p)| p);
        let start = blocks.at(from.0, from.1);
        let label = match tool {
            BlockTool::Raise if shift => "Lower blocks",
            BlockTool::Raise => "Raise blocks",
            BlockTool::Level => "Level blocks",
            BlockTool::Paint => "Paint blocks",
            BlockTool::Ramp if shift => "Remove ramps",
            BlockTool::Ramp => "Ramps",
            BlockTool::Cut => "Cut a block",
        };
        let current = blocks.clone();
        out.edits
            .extend(commit(session, label, current, |blocks| match tool {
                BlockTool::Raise => {
                    for (x, z) in area(from, to) {
                        if let Some(mut b) = blocks.get(x, z) {
                            b.level = if shift {
                                b.level.saturating_sub(1)
                            } else {
                                (b.level + 1).min(MAX_LEVEL)
                            };
                            blocks.set(x, z, b);
                        }
                    }
                }
                BlockTool::Level => {
                    for (x, z) in area(from, to) {
                        if let Some(mut b) = blocks.get(x, z) {
                            b.level = start.level;
                            blocks.set(x, z, b);
                        }
                    }
                }
                BlockTool::Paint => {
                    for (x, z) in area(from, to) {
                        if let Some(mut b) = blocks.get(x, z) {
                            b.kind = kind;
                            blocks.set(x, z, b);
                        }
                    }
                }
                BlockTool::Ramp => {
                    for (x, z) in area(from, to) {
                        if let Some(mut b) = blocks.get(x, z) {
                            b.kind = if shift {
                                BlockKind::Ground
                            } else {
                                BlockKind::Ramp
                            };
                            b.cut = Cut::None;
                            blocks.set(x, z, b);
                        }
                    }
                }
                BlockTool::Cut => {
                    let (x, z) = to;
                    if let (Some(mut b), Some(p)) = (blocks.get(x, z), hit_point) {
                        let halo = MAP_HALO as f32;
                        let u = (p.x + halo) / BLOCK as f32 - x as f32;
                        let v = (p.z + halo) / BLOCK as f32 - z as f32;
                        let corner = match (u > 0.5, v > 0.5) {
                            (false, false) => Cut::NorthWest,
                            (true, false) => Cut::NorthEast,
                            (true, true) => Cut::SouthEast,
                            (false, true) => Cut::SouthWest,
                        };
                        b.cut = if shift || b.cut == corner {
                            Cut::None
                        } else {
                            corner
                        };
                        blocks.set(x, z, b);
                    }
                }
            }));
    }
    if let Some((c, _)) = column
        && let Some(b) = blocks.get(c.0, c.1)
    {
        let text = format!("level {}  ·  {}", b.level, b.kind.name());
        painter.text(
            rect.left_bottom() + egui::vec2(12.0, -12.0),
            egui::Align2::LEFT_BOTTOM,
            text,
            egui::FontId::proportional(12.0),
            TEXT,
        );
    }
    out
}
