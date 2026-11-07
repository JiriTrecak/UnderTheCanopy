//! The map view's minimap (M, or the view bar): the map seen from straight above, drawn
//! from its blocks so it follows every edit — ground by level in the blocks view's
//! colours, water, walls and rock, darker along every edge where the ground falls (the
//! map's outline), the border outside the playable area dimmed; over it the starts in
//! their players' colours, amber nodes, camps and other placements, and what the camera
//! sees. A click or drag on it moves the camera there.

use super::{Session, mesh};
use canopy_content::map::{
    MAP_HALO, Playable,
    blocks::{BLOCK, BlockKind, Blocks},
};
use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, Shape, Stroke, pos2, vec2};
use glam::{Mat4, Vec3, Vec4};

/// The minimap's side, in points.
const SIDE: f32 = 208.0;
/// Its picture's pixels per side at most (a map is sampled at its cells up to this).
const RESOLUTION: u32 = 512;

/// What a minimap picture is drawn from: the blocks and their paint, the map's size and
/// its playable area.
type Source = (
    Option<canopy_content::map::grid::Grid>,
    Option<canopy_content::map::grid::Grid>,
    u32,
    Playable,
);

/// The minimap's picture, when it was looked at last (the map's load and scene) and what
/// it was drawn from: a new scene of the same blocks keeps it, and an edit draws again
/// only the pixels over what it changed.
#[derive(Clone)]
pub struct Cache {
    key: (Option<u64>, u64, bool),
    source: std::sync::Arc<Source>,
    blocks: std::sync::Arc<Blocks>,
    image: std::sync::Arc<egui::ColorImage>,
    texture: egui::TextureHandle,
}

impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cache").field("key", &self.key).finish()
    }
}

/// The picture of the `blocks` of a map of `size` cells: one pixel per sample, ground by
/// level, darker where it falls to a lower neighbour, dimmed outside the playable `area`.
fn picture(size: u32, area: Playable, blocks: &Blocks) -> egui::ColorImage {
    let n = RESOLUTION.min(size).max(1) as usize;
    let mut image = egui::ColorImage::new([n, n], vec![Color32::BLACK; n * n]);
    paint(size, area, blocks, &mut image, (0, 0, n - 1, n - 1));
    image
}

/// The picture's pixels in columns `i0..=i1` and rows `j0..=j1` drawn again.
fn paint(
    size: u32,
    area: Playable,
    blocks: &Blocks,
    image: &mut egui::ColorImage,
    (i0, j0, i1, j1): (usize, usize, usize, usize),
) {
    let n = image.size[0];
    let cells = size as f32 / n as f32;
    let (halo, block) = (MAP_HALO as f32, BLOCK as f32);
    // The samples under these pixels and their neighbours.
    let (si0, sj0) = (i0.saturating_sub(1), j0.saturating_sub(1));
    let (si1, sj1) = ((i1 + 1).min(n - 1), (j1 + 1).min(n - 1));
    let w = si1 - si0 + 1;
    let samples: Vec<_> = (sj0..=sj1)
        .flat_map(|j| (si0..=si1).map(move |i| (i, j)))
        .map(|(i, j)| {
            let cx = ((i as f32 + 0.5) * cells + halo) / block;
            let cz = ((j as f32 + 0.5) * cells + halo) / block;
            blocks.sample(cx.floor() as i32, cz.floor() as i32, cx.fract(), cz.fract())
        })
        .collect();
    let sample = |i: usize, j: usize| samples[(j - sj0) * w + (i - si0)];
    let inside = |i: usize, j: usize| {
        let (x, z) = ((i as f32 + 0.5) * cells, (j as f32 + 0.5) * cells);
        x >= area.x as f32
            && z >= area.z as f32
            && x < (area.x + area.width) as f32
            && z < (area.z + area.height) as f32
    };
    let srgb = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        let s = if c <= 0.003_130_8 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round() as u8
    };
    for j in j0..=j1 {
        for i in i0..=i1 {
            let here = sample(i, j);
            let mut color = mesh::color(here);
            // An edge where the ground falls to a neighbour (not along a ramp's slope).
            let falls = here.kind != BlockKind::Ramp
                && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(di, dj)| {
                    let (ni, nj) = (i as i32 + di, j as i32 + dj);
                    if ni < 0 || nj < 0 || ni >= n as i32 || nj >= n as i32 {
                        return false;
                    }
                    let there = sample(ni as usize, nj as usize);
                    there.kind != BlockKind::Ramp && there.level < here.level
                });
            let shade = match (falls, inside(i, j)) {
                (true, true) => 0.35,
                (true, false) => 0.2,
                (false, true) => 1.0,
                (false, false) => 0.4,
            };
            color = color.map(|c| c * shade);
            image.pixels[j * n + i] =
                Color32::from_rgb(srgb(color[0]), srgb(color[1]), srgb(color[2]));
        }
    }
}

/// The pixels whose picture `blocks` changes from `before`'s (`None`: none), for a map of
/// `size` cells drawn `n` pixels across: those over the columns that differ and their
/// neighbours (a cut takes its fill from them), and one more around (an edge looks at the
/// next pixel).
fn changed_pixels(
    size: u32,
    n: usize,
    blocks: &Blocks,
    before: &Blocks,
) -> Option<(usize, usize, usize, usize)> {
    let side = blocks.side as i32;
    let b = BLOCK as usize;
    let cells = (blocks.side as usize) * b;
    let mut changed: Option<(i32, i32, i32, i32)> = None;
    for z in 0..side {
        for x in 0..side {
            let i = (z * side + x) as usize;
            let painted = match (&blocks.paint, &before.paint) {
                (Some(new), Some(old)) => (0..b).any(|r| {
                    let at = (z as usize * b + r) * cells + x as usize * b;
                    new[at..at + b] != old[at..at + b]
                }),
                _ => false,
            };
            if blocks.columns[i] != before.columns[i] || painted {
                changed = Some(match changed {
                    Some((x0, z0, x1, z1)) => (x0.min(x), z0.min(z), x1.max(x), z1.max(z)),
                    None => (x, z, x, z),
                });
            }
        }
    }
    let (x0, z0, x1, z1) = changed?;
    let (x0, z0, x1, z1) = (x0 - 1, z0 - 1, x1 + 1, z1 + 1);
    let cells = size as f32 / n as f32;
    let (halo, block) = (MAP_HALO as f32, BLOCK as f32);
    // The column each pixel samples (clamped as `Blocks::sample` clamps).
    let column = |i: usize| {
        let c = (((i as f32 + 0.5) * cells + halo) / block).floor() as i32;
        c.clamp(0, side - 1)
    };
    let span = |lo: i32, hi: i32| {
        let mut hit = (0..n).filter(|&i| (lo..=hi).contains(&column(i)));
        let first = hit.next()?;
        let last = hit.next_back().unwrap_or(first);
        Some((first.saturating_sub(1), (last + 1).min(n - 1)))
    };
    let ((i0, i1), (j0, j1)) = (span(x0, x1)?, span(z0, z1)?);
    Some((i0, j0, i1, j1))
}

/// Where the corners of the view's `rect` meet the ground at `ground` height (corners that
/// look above the horizon stop a few camera distances out).
fn view_on_ground(
    view_projection: Mat4,
    eye: Vec3,
    rect: Rect,
    ground: f32,
    far: f32,
) -> [[f32; 2]; 4] {
    let inverse = view_projection.inverse();
    [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ]
    .map(|p| {
        let ndc = (
            (p.x - rect.left()) / rect.width().max(1.0) * 2.0 - 1.0,
            1.0 - (p.y - rect.top()) / rect.height().max(1.0) * 2.0,
        );
        let point = inverse * Vec4::new(ndc.0, ndc.1, 1.0, 1.0);
        let direction = (point.truncate() / point.w - eye).normalize_or_zero();
        let t = if direction.y < -1e-3 {
            ((ground - eye.y) / direction.y).min(far)
        } else {
            far
        };
        let hit = eye + direction * t;
        [hit.x, hit.z]
    })
}

/// Draws the minimap in the bottom-left corner of `view` (when it is on), for the camera
/// that draws `pane`.
pub(super) fn show(
    ctx: &egui::Context,
    session: &mut Session,
    view: Rect,
    pane: Rect,
    view_projection: Mat4,
    eye: Vec3,
) {
    if !super::Panels::get(ctx).minimap {
        return;
    }
    let Some(map) = session.map.as_ref() else {
        return;
    };
    let key = (session.loaded_at, session.version, session.stale);
    if let Some(cache) = &mut session.blocks.minimap
        && cache.key != key
    {
        let terrain = &map.authoring.terrain;
        let (blocks, paint, size, area) = &*cache.source;
        if *blocks == terrain.blocks
            && *paint == terrain.block_paint
            && *size == map.size
            && *area == map.playable_area()
        {
            cache.key = key;
        }
    }
    if session.blocks.minimap.as_ref().is_none_or(|c| c.key != key)
        && let Some(Ok(blocks)) = Blocks::of(&map.authoring.terrain)
    {
        let _span = tracing::info_span!("minimap picture").entered();
        let (size, area) = (map.size, map.playable_area());
        let n = RESOLUTION.min(size).max(1) as usize;
        // The same map shape: the picture as it was, drawn again where the blocks differ.
        let kept = session.blocks.minimap.as_ref().filter(|c| {
            let (_, _, was_size, was_area) = &*c.source;
            *was_size == size
                && *was_area == area
                && c.blocks.side == blocks.side
                && c.blocks.paint.is_some() == blocks.paint.is_some()
                && c.image.size == [n, n]
        });
        let (image, texture) = match kept {
            Some(cache) => {
                let mut image = (*cache.image).clone();
                if let Some(rect) = changed_pixels(size, n, &blocks, &cache.blocks) {
                    paint(size, area, &blocks, &mut image, rect);
                }
                let mut texture = cache.texture.clone();
                texture.set(image.clone(), egui::TextureOptions::LINEAR);
                (image, texture)
            }
            None => {
                let image = picture(size, area, &blocks);
                let texture = ctx.load_texture(
                    format!("minimap {}", session.root),
                    image.clone(),
                    egui::TextureOptions::LINEAR,
                );
                (image, texture)
            }
        };
        let terrain = &map.authoring.terrain;
        let source = std::sync::Arc::new((
            terrain.blocks.clone(),
            terrain.block_paint.clone(),
            map.size,
            map.playable_area(),
        ));
        session.blocks.minimap = Some(Cache {
            key,
            source,
            blocks: std::sync::Arc::new(blocks),
            image: std::sync::Arc::new(image),
            texture,
        });
    }
    let Some(cache) = session.blocks.minimap.clone() else {
        return;
    };
    let size = map.size as f32;
    let ground =
        crate::map::tools::ground_at(map, session.camera.focus[0], session.camera.focus[1]);
    let seen = view_on_ground(
        view_projection,
        eye,
        pane,
        ground,
        session.camera.distance * 4.0,
    );
    let colors = session
        .scene
        .as_ref()
        .map(|s| s.colors.clone())
        .unwrap_or_default();
    let camp_members: Vec<String> = map
        .camps
        .iter()
        .flat_map(|c| c.members.iter().map(|m| m.to_string()))
        .collect();
    let mut focus = None;
    egui::Area::new(Id::new(("minimap", &session.root)))
        .order(Order::Foreground)
        .pivot(Align2::LEFT_BOTTOM)
        .fixed_pos(view.left_bottom() + vec2(12.0, -12.0))
        .constrain_to(view)
        .show(ctx, |ui| {
            super::strip::frame()
                .inner_margin(egui::Margin::same(4))
                .show(ui, |ui| {
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(SIDE, SIDE), Sense::click_and_drag());
                    let painter = ui.painter_at(rect);
                    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                    painter.image(cache.texture.id(), rect, uv, Color32::WHITE);
                    let at = |x: f32, z: f32| -> Pos2 {
                        pos2(
                            rect.left() + x / size * rect.width(),
                            rect.top() + z / size * rect.height(),
                        )
                    };
                    let area = map.playable_area();
                    painter.rect_stroke(
                        Rect::from_min_max(
                            at(area.x as f32, area.z as f32),
                            at((area.x + area.width) as f32, (area.z + area.height) as f32),
                        ),
                        0,
                        Stroke::new(1.0, Color32::from_white_alpha(90)),
                        egui::StrokeKind::Middle,
                    );
                    let player = |n: u8| -> Color32 {
                        let c = colors
                            .get(usize::from(n.saturating_sub(1)) % colors.len().max(1))
                            .copied()
                            .unwrap_or([0.9, 0.9, 0.9]);
                        egui::Rgba::from_rgb(c[0], c[1], c[2]).into()
                    };
                    let dark = Color32::from_black_alpha(200);
                    // Placements: amber yellow, creeps red, players' their colour, the rest sand.
                    for e in &map.entities {
                        let id = e.id.as_str();
                        let color = match e.owner {
                            canopy_content::placement::Owner::Player(slot) => player(slot + 1),
                            _ if id.starts_with("amber.") || id.starts_with("mine.") => {
                                Color32::from_rgb(0xff, 0xd4, 0x3b)
                            }
                            _ if camp_members.iter().any(|m| m == id) => {
                                Color32::from_rgb(0xe0, 0x5a, 0x4f)
                            }
                            _ => Color32::from_rgb(0xa9, 0x9c, 0x78),
                        };
                        let p = at(e.position.x as f32, e.position.y as f32);
                        painter.rect_filled(Rect::from_center_size(p, vec2(3.0, 3.0)), 0, color);
                    }
                    // The starts: a disc in the player's colour with its number.
                    for s in &map.player_starts {
                        let p = at(s.x as f32, s.z as f32);
                        painter.circle(p, 6.0, player(s.player), Stroke::new(1.5, dark));
                        painter.text(
                            p,
                            Align2::CENTER_CENTER,
                            s.player.to_string(),
                            egui::FontId::proportional(9.0),
                            Color32::WHITE,
                        );
                    }
                    // What the camera sees.
                    let quad: Vec<Pos2> = seen.iter().map(|[x, z]| at(*x, *z)).collect();
                    painter.add(Shape::closed_line(quad.clone(), Stroke::new(3.0, dark)));
                    painter.add(Shape::closed_line(
                        quad,
                        Stroke::new(1.4, Color32::from_rgb(0xf2, 0xee, 0xe0)),
                    ));
                    if (response.clicked() || response.dragged())
                        && let Some(p) = response.interact_pointer_pos()
                    {
                        focus = Some([
                            ((p.x - rect.left()) / rect.width() * size).clamp(0.0, size),
                            ((p.y - rect.top()) / rect.height() * size).clamp(0.0, size),
                        ]);
                    }
                    response.on_hover_cursor(egui::CursorIcon::Crosshair);
                });
        });
    if let Some(focus) = focus {
        session.camera.focus = focus;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_raised_plateau_is_outlined_where_it_falls() {
        let size = 128;
        let mut blocks = Blocks::flat(size);
        let halo = (MAP_HALO / BLOCK) as i32;
        for z in 10..20 {
            for x in 10..20 {
                blocks.set(
                    x + halo,
                    z + halo,
                    canopy_content::map::blocks::Block::ground(3),
                );
            }
        }
        let area = Playable {
            x: 0,
            z: 0,
            width: size,
            height: size,
        };
        let image = picture(size, area, &blocks);
        let pixel = |x: usize, z: usize| image.pixels[z * image.size[0] + x];
        // Inside the plateau, at its edge, and on the plain beside it.
        let (inside, edge, plain) = (pixel(60, 60), pixel(40, 60), pixel(20, 60));
        assert_ne!(inside, plain);
        assert!(edge.r() < inside.r() && edge.g() < inside.g());
    }

    /// Strokes of every kind, cut, ramp and painted cell: drawing again only the pixels
    /// over what changed makes the same picture as drawing it whole.
    #[test]
    fn drawing_what_changed_matches_the_whole_picture() {
        use canopy_content::map::blocks::{Block, Cut};
        let mut seed = 0x6c07_8965_u32;
        let mut next = |n: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed % n
        };
        let kinds = [
            BlockKind::Ground,
            BlockKind::Ramp,
            BlockKind::Wall,
            BlockKind::Rock,
            BlockKind::Shallow,
            BlockKind::Deep,
            BlockKind::Ice,
        ];
        // A map drawn one pixel a cell, and one drawn at half that.
        for size in [256, 1024] {
            let area = Playable {
                x: 8,
                z: 8,
                width: size - 16,
                height: size - 16,
            };
            let mut blocks = Blocks::flat(size);
            blocks.paint_cell(0, 0, None);
            let mut image = picture(size, area, &blocks);
            let n = image.size[0];
            for _ in 0..40 {
                let before = blocks.clone();
                let (x, z) = (next(blocks.side) as i32, next(blocks.side) as i32);
                for dz in 0..1 + next(3) as i32 {
                    for dx in 0..1 + next(3) as i32 {
                        let block = Block {
                            level: next(8) as u8,
                            kind: kinds[next(7) as usize],
                            cut: Cut::from_u8(next(5) as u8),
                            ramp: next(9) as u8,
                            style: 0,
                        };
                        blocks.set(x + dx, z + dz, block);
                    }
                }
                let cells = blocks.cells();
                let (cx, cz) = (next(cells) as i32, next(cells) as i32);
                blocks.paint_cell(cx, cz, Some(kinds[next(7) as usize]));
                if let Some(rect) = changed_pixels(size, n, &blocks, &before) {
                    paint(size, area, &blocks, &mut image, rect);
                }
                assert!(image == picture(size, area, &blocks));
            }
        }
    }
}
