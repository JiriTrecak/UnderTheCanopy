//! World UI on the render thread (old `VitalBar`, `SelectionCircles`, `RallyMarkers`,
//! `MineLabels`, `StatusBadges`): from the presentation's bodies, the interpolated model
//! positions, the camera and what the player has selected, the ground marks and the
//! pixel overlay the renderer draws in one pass each (`canopy_render::world_ui`).
//!
//! Selection belongs to the game: it fills a [`WorldUiInput`] every frame.

use crate::live::Body;
use canopy_assets::Pack;
use canopy_content::Content;
use canopy_render::{
    CameraView,
    water::ripples::WaterField,
    world_ui::{AtlasImage, GroundVertex, OverlayVertex, WorldUiFrame, overlay},
};
use canopy_sim::teams::Relation;
use glam::{Vec3, Vec4Swizzles};
use std::{collections::BTreeMap, path::Path};

/// Which health bars show (old: selected or damaged units, plus held keys for all,
/// enemies or friends).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HealthBars {
    /// Selected entities and damaged units (the old default).
    #[default]
    Damaged,
    /// Every unit and building with health (old Alt held).
    Always,
    /// Selected entities only (a setting).
    Selected,
    /// The default, and every friendly owned body (old `]` held).
    Friendly,
    /// The default, and every hostile body (old `[` held).
    Hostile,
}

/// An order marker the player's command left on the ground (old command feedback ring:
/// `radius` to `radius + 0.12`, drawn over everything).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroundMarker {
    pub position: [f32; 2],
    pub radius: f32,
    /// Linear RGB.
    pub color: [f32; 3],
    pub opacity: f32,
}

/// The placement grid under a building being placed (old `placementGrid`): its cells
/// (inclusive bounds, cells centred on integers), whether the site looks buildable and
/// its entrance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacementMark {
    pub cells: [i64; 4],
    pub valid: bool,
    pub entrance: [f32; 2],
}

/// A spell aimed at the cursor (old `AbilityTarget`): its area (a disc at the point, or
/// a line from the caster towards it), the cast range around the caster and whether the
/// aim is valid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AimMark {
    pub origin: [f32; 2],
    pub point: [f32; 2],
    pub radius: f32,
    /// A line delivery's length and half width.
    pub line: Option<(f32, f32)>,
    pub range: f32,
    pub valid: bool,
}

/// What the player has selected and how the world UI is set; the game fills it.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldUiInput {
    /// Selected entity ids.
    pub selected: Vec<u32>,
    pub hovered: Option<u32>,
    pub health: HealthBars,
    /// The viewer's player slot: rally markers show for its own producers (all when
    /// `None`, spectating).
    pub viewer: Option<u8>,
    /// Physical pixels per CSS pixel of the old overlay sizes.
    pub scale: f32,
    /// Thin construction, training and cast bars under the health bar of selected
    /// entities. (review: the old game showed these only in the HUD)
    pub progress_bars: bool,
    pub markers: Vec<GroundMarker>,
    pub placement: Option<PlacementMark>,
    pub aim: Option<AimMark>,
    /// A hovered tree's ring (x, z, radius): the neutral hover ring of a body.
    pub tree: Option<[f32; 3]>,
}

impl Default for WorldUiInput {
    fn default() -> Self {
        Self {
            selected: Vec::new(),
            hovered: None,
            health: HealthBars::default(),
            viewer: None,
            scale: 1.0,
            progress_bars: true,
            markers: Vec::new(),
            placement: None,
            aim: None,
            tree: None,
        }
    }
}

/// `#rrggbb` (sRGB) as linear RGB.
pub fn linear(hex: u32) -> [f32; 3] {
    let channel = |shift: u32| {
        let c = ((hex >> shift) & 255) as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    [channel(16), channel(8), channel(0)]
}

/// The old continuous red → amber → green health colour (sRGB `#rrggbb`).
pub fn health_color(hp: u32, max_hp: u32) -> u32 {
    let ratio = health_ratio(hp, max_hp);
    let (a, b, t) = if ratio < 0.5 {
        (0xdb3935, 0xe8bf32, ratio * 2.0)
    } else {
        (0xe8bf32, 0x64cc39, (ratio - 0.5) * 2.0)
    };
    let channel = |shift: u32| {
        let ca = ((a >> shift) & 255) as f32;
        let cb = ((b >> shift) & 255) as f32;
        ((ca * (1.0 - t) + cb * t).round() as u32) & 255
    };
    (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

pub fn health_ratio(value: u32, max: u32) -> f32 {
    if max == 0 {
        0.0
    } else {
        (value as f32 / max as f32).clamp(0.0, 1.0)
    }
}

/// Whether a body's health bar shows (old `healthBarVisible`).
pub fn health_bar_visible(body: &Body, selected: bool, mode: HealthBars) -> bool {
    let Some(hp) = body.hp else { return false };
    if hp == 0 || body.remembered || body.max_hp == 0 {
        return false;
    }
    // Damaged units and buildings alike (the old game showed buildings' bars only when
    // selected); a building still being raised is not damaged.
    let default = selected || (hp < body.max_hp && !body.constructing);
    match mode {
        HealthBars::Always => true,
        HealthBars::Selected => selected,
        HealthBars::Damaged => default,
        HealthBars::Friendly => default || (body.hostile == Some(false) && body.owner.is_some()),
        HealthBars::Hostile => default || body.hostile == Some(true),
    }
}

/// Circle tints (old `SelectionCircles`): own green, allies cooler green, neutral yellow,
/// enemies and attack targets red.
fn circle_color(relation: Relation, target: bool) -> u32 {
    if target {
        return 0xff3a30;
    }
    match relation {
        Relation::Own => 0x3cff4e,
        Relation::Ally => 0x3cffc0,
        Relation::Neutral => 0xffe53c,
        Relation::Enemy => 0xff3a30,
    }
}

/// Ring rows: radius factor, line-width offset, alpha (soft inner glow, solid core,
/// feathered rim); the shader's `ring_alpha` interpolates them.
const PROFILE: [(f32, f32, f32); 5] = [
    (1.0, -3.5, 0.0),
    (1.0, -1.2, 0.25),
    (1.0, -0.9, 0.95),
    (1.0, 0.0, 1.0),
    (1.0, 0.45, 0.0),
];
/// Sectors of a ring (the old ring had 56 segments).
const SECTORS: usize = 48;

/// A rasterised glyph in the label atlas.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Glyph {
    uv: [f32; 4],
    size: [f32; 2],
    left: f32,
    top: f32,
    advance: f32,
}

/// The label font: printable ASCII of the HUD font at the label size, in one R8 atlas.
pub struct LabelFont {
    pub atlas: AtlasImage,
    glyphs: BTreeMap<char, Glyph>,
    /// Pixels the font was rasterised at per CSS pixel.
    pub scale: f32,
    line: f32,
    ascent: f32,
}

/// The old label: `600 13px/18px system-ui`.
const LABEL_SIZE: f32 = 13.0;
const LABEL_LINE: f32 = 18.0;

impl LabelFont {
    /// Rasterises `font` (TrueType bytes) at `scale` physical pixels per CSS pixel.
    pub fn new(font: &[u8], scale: f32) -> Self {
        use cosmic_text::{
            Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent, Weight,
            fontdb,
        };
        let mut db = fontdb::Database::new();
        db.load_font_data(font.to_vec());
        let family = db
            .faces()
            .next()
            .and_then(|f| f.families.first().map(|(n, _)| n.clone()))
            .unwrap_or_default();
        let mut fonts = FontSystem::new_with_locale_and_db("en-US".into(), db);
        let mut swash = SwashCache::new();
        let metrics = Metrics::new(LABEL_SIZE * scale, LABEL_LINE * scale);
        let mut buffer = Buffer::new(&mut fonts, metrics);
        let attrs = Attrs::new()
            .family(Family::Name(&family))
            .weight(Weight::SEMIBOLD);
        let side = 512u32;
        let mut pixels = vec![0u8; (side * side) as usize];
        let (mut x, mut y, mut row) = (1u32, 1u32, 0u32);
        let mut glyphs = BTreeMap::new();
        let mut ascent = LABEL_SIZE * scale * 0.8;
        for c in (32u8..127).map(char::from) {
            buffer.set_text(&c.to_string(), &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(&mut fonts, false);
            let Some(run) = buffer.layout_runs().next() else {
                continue;
            };
            ascent = run.line_y - run.line_top;
            let Some(g) = run.glyphs.first() else {
                continue;
            };
            let advance = g.w;
            let key = g.physical((0.0, 0.0), 1.0).cache_key;
            let image = swash.get_image_uncached(&mut fonts, key);
            let (mut w, mut h, mut left, mut top) = (0, 0, 0.0, 0.0);
            let mut uv = [0.0; 4];
            if let Some(image) = image {
                w = image.placement.width;
                h = image.placement.height;
                left = image.placement.left as f32;
                top = image.placement.top as f32;
                let coverage: Vec<u8> = match image.content {
                    SwashContent::Mask => image.data,
                    SwashContent::SubpixelMask => image
                        .data
                        .chunks_exact(4)
                        .map(|p| ((u16::from(p[0]) + u16::from(p[1]) + u16::from(p[2])) / 3) as u8)
                        .collect(),
                    SwashContent::Color => image.data.chunks_exact(4).map(|p| p[3]).collect(),
                };
                if x + w + 1 >= side {
                    x = 1;
                    y += row + 1;
                    row = 0;
                }
                if y + h + 1 < side && w > 0 && h > 0 {
                    for gy in 0..h {
                        let from = (gy * w) as usize;
                        let to = ((y + gy) * side + x) as usize;
                        pixels[to..to + w as usize]
                            .copy_from_slice(&coverage[from..from + w as usize]);
                    }
                    uv = [
                        x as f32 / side as f32,
                        y as f32 / side as f32,
                        (x + w) as f32 / side as f32,
                        (y + h) as f32 / side as f32,
                    ];
                    x += w + 1;
                    row = row.max(h);
                }
            }
            glyphs.insert(
                c,
                Glyph {
                    uv,
                    size: [w as f32, h as f32],
                    left,
                    top,
                    advance,
                },
            );
        }
        Self {
            atlas: AtlasImage {
                width: side,
                height: side,
                pixels,
            },
            glyphs,
            scale,
            line: LABEL_LINE * scale,
            ascent,
        }
    }

    /// Physical width of `text`.
    pub fn width(&self, text: &str) -> f32 {
        text.chars()
            .map(|c| self.glyphs.get(&c).map_or(0.0, |g| g.advance))
            .sum()
    }

    /// Glyph quads of `text` with the line's top left at `at` (physical pixels).
    fn draw(&self, out: &mut WorldUiFrame, text: &str, at: [f32; 2], color: [f32; 4]) {
        let mut pen = at[0];
        let baseline = at[1] + self.ascent;
        for c in text.chars() {
            let Some(g) = self.glyphs.get(&c) else {
                continue;
            };
            if g.size[0] > 0.0 {
                let x0 = (pen + g.left).round();
                let y0 = (baseline - g.top).round();
                let (x1, y1) = (x0 + g.size[0], y0 + g.size[1]);
                let v = |x: f32, y: f32, u: f32, w: f32| OverlayVertex {
                    position: [x, y],
                    uv: [u, w],
                    color,
                    kind: [overlay::GLYPH, 0, 0, 0],
                    ..Default::default()
                };
                out.quad([
                    v(x0, y0, g.uv[0], g.uv[1]),
                    v(x1, y0, g.uv[2], g.uv[1]),
                    v(x1, y1, g.uv[2], g.uv[3]),
                    v(x0, y1, g.uv[0], g.uv[3]),
                ]);
            }
            pen += g.advance;
        }
    }
}

/// Item status icons packed into one RGBA atlas (64-pixel cells).
pub struct IconAtlas {
    pub atlas: AtlasImage,
    cells: BTreeMap<String, [f32; 4]>,
}

const ICON_CELL: u32 = 64;

impl IconAtlas {
    /// The icons of every item definition, decoded from the pack.
    pub fn new(pack: Option<&Pack>, content: &Content) -> Self {
        let icons: Vec<(String, String)> = content
            .definitions
            .iter()
            .filter(|d| d.kind == canopy_content::game::Kind::Item)
            .filter_map(|d| {
                let image = content.asset(d.icon.as_str())?.image.clone()?;
                Some((d.icon.to_string(), image))
            })
            .collect();
        let columns = 16u32;
        let rows = (icons.len() as u32).div_ceil(columns).max(1);
        let (width, height) = (columns * ICON_CELL, rows * ICON_CELL);
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let mut cells = BTreeMap::new();
        for (i, (id, image)) in icons.iter().enumerate() {
            let Some(pack) = pack else { break };
            let texture = pack
                .texture(image)
                .or_else(|_| pack.texture(&format!("assets/{image}")));
            let Ok(texture) = texture else {
                tracing::warn!(icon = %id, "badge icon not in the pack");
                continue;
            };
            // The mip closest to the cell.
            let level = (0..texture.meta.mips.len())
                .find(|&l| texture.level_extent(l).0 <= ICON_CELL)
                .unwrap_or(texture.meta.mips.len() - 1);
            let (w, h, _) = texture.level_extent(level);
            let rgba = texture.decode_level(level);
            if rgba.len() < (w * h * 4) as usize {
                continue;
            }
            let (cx, cy) = (
                (i as u32 % columns) * ICON_CELL,
                (i as u32 / columns) * ICON_CELL,
            );
            for y in 0..h.min(ICON_CELL) {
                for x in 0..w.min(ICON_CELL) {
                    let from = ((y * w + x) * 4) as usize;
                    let to = (((cy + y) * width + cx + x) * 4) as usize;
                    pixels[to..to + 4].copy_from_slice(&rgba[from..from + 4]);
                }
            }
            cells.insert(
                id.clone(),
                [
                    cx as f32 / width as f32,
                    cy as f32 / height as f32,
                    (cx + w.min(ICON_CELL)) as f32 / width as f32,
                    (cy + h.min(ICON_CELL)) as f32 / height as f32,
                ],
            );
        }
        Self {
            atlas: AtlasImage {
                width,
                height,
                pixels,
            },
            cells,
        }
    }
}

/// Where a world point lands on the output, in physical pixels, and how many pixels a
/// metre is there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projected {
    pub x: f32,
    pub y: f32,
    pub pixels_per_metre: f32,
}

/// Projects `p` with `camera` onto an output of `size` pixels (`None` behind the eye).
pub fn project(camera: &CameraView, size: [u32; 2], p: Vec3) -> Option<Projected> {
    let clip = camera.view_projection * p.extend(1.0);
    if clip.w <= 1e-4 {
        return None;
    }
    let ndc = clip.xy() / clip.w;
    let (w, h) = (size[0] as f32, size[1] as f32);
    Some(Projected {
        x: (ndc.x + 1.0) * 0.5 * w,
        y: (1.0 - ndc.y) * 0.5 * h,
        pixels_per_metre: h * camera.projection.y_axis.y / (2.0 * clip.w),
    })
}

/// The world UI builder: the label font, the badge icons and the player colours.
pub struct WorldUi {
    font: Option<LabelFont>,
    font_bytes: Vec<u8>,
    icons: IconAtlas,
    colors: Vec<[f32; 3]>,
    /// The atlases changed since the renderer last took them.
    pub atlases_dirty: bool,
}

/// The bundled HUD font (`config/ui/fonts`).
pub const LABEL_FONT: &str = "config/ui/fonts/Outfit-Latin.ttf";

impl WorldUi {
    /// `data` holds the HUD font; `colors` are the player colours (linear) by slot.
    pub fn new(data: &Path, pack: Option<&Pack>, content: &Content, colors: Vec<[f32; 3]>) -> Self {
        let font_bytes = std::fs::read(data.join(LABEL_FONT)).unwrap_or_else(|e| {
            tracing::warn!("world labels without a font ({LABEL_FONT}): {e}");
            Vec::new()
        });
        Self {
            font: None,
            font_bytes,
            icons: IconAtlas::new(pack, content),
            colors,
            atlases_dirty: true,
        }
    }

    /// The label glyphs and the badge icons for `Renderer::set_world_ui_atlases`.
    pub fn atlases(&mut self, scale: f32) -> (&AtlasImage, &AtlasImage) {
        self.ensure_font(scale);
        self.atlases_dirty = false;
        static EMPTY: AtlasImage = AtlasImage {
            width: 0,
            height: 0,
            pixels: Vec::new(),
        };
        (
            self.font.as_ref().map_or(&EMPTY, |f| &f.atlas),
            &self.icons.atlas,
        )
    }

    fn ensure_font(&mut self, scale: f32) {
        if self.font_bytes.is_empty() {
            return;
        }
        if self
            .font
            .as_ref()
            .is_none_or(|f| (f.scale - scale).abs() > 1e-3)
        {
            self.font = Some(LabelFont::new(&self.font_bytes, scale));
            self.atlases_dirty = true;
        }
    }

    fn player_color(&self, slot: Option<u8>) -> [f32; 3] {
        slot.and_then(|s| self.colors.get(usize::from(s) % self.colors.len().max(1)))
            .copied()
            .unwrap_or([1.0; 3])
    }

    /// Builds the world UI of a frame. `position(key)` is where an interpolated model
    /// stands; `field` gives walking heights for draping.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        &mut self,
        bodies: &[Body],
        position: &dyn Fn(u64) -> Option<Vec3>,
        input: &WorldUiInput,
        camera: &CameraView,
        size: [u32; 2],
        field: &dyn WaterField,
        seconds: f32,
        out: &mut WorldUiFrame,
    ) {
        let _span = tracing::trace_span!("world_ui").entered();
        out.clear();
        self.ensure_font(input.scale);
        let chosen: std::collections::BTreeSet<u32> = input.selected.iter().copied().collect();
        let selected = |id: u32| chosen.contains(&id);
        let find = |id: u32| {
            bodies
                .binary_search_by_key(&id, |b| b.id)
                .ok()
                .map(|i| &bodies[i])
        };
        let at = |b: &Body| position(b.key).unwrap_or(b.position);
        // Attack targets of the selection.
        let targets: Vec<u32> = bodies
            .iter()
            .filter(|b| selected(b.id))
            .filter_map(|b| b.commanded)
            .collect();
        for b in bodies {
            let chosen = selected(b.id);
            let target = targets.contains(&b.id) && !b.remembered;
            let hovered = input.hovered == Some(b.id);
            if chosen || target || hovered {
                // Observers see every owned entity as their own (their view calls it an
                // enemy only because it is not theirs).
                let relation = match (input.viewer, b.relation, b.owner) {
                    (None, Relation::Enemy, Some(_)) => Relation::Own,
                    (_, r, _) => r,
                };
                circle(
                    out,
                    b.radius,
                    b.surface,
                    at(b),
                    field,
                    circle_color(relation, target),
                    !chosen && !target,
                );
            }
        }
        if let Some([x, z, radius]) = input.tree {
            circle(
                out,
                radius,
                0,
                Vec3::new(x, 0.0, z),
                field,
                circle_color(Relation::Neutral, false),
                true,
            );
        }
        // Rally routes and flags of the viewer's selected producers.
        for b in bodies.iter().filter(|b| selected(b.id)) {
            let Some(rally) = b.rally else { continue };
            if b.remembered || b.constructing || input.viewer.is_some_and(|v| b.owner != Some(v)) {
                continue;
            }
            let from = at(b);
            let followed = rally
                .target
                .and_then(find)
                .filter(|t| !t.building && !t.remembered);
            let mut to = match followed {
                Some(t) => {
                    let p = at(t);
                    (p.x, p.z, t.surface)
                }
                None => (rally.x, rally.z, rally.surface),
            };
            // A flag planted on a resource stands in front of it, towards the building.
            if rally.target.is_some() && followed.is_none() && rally.target.and_then(find).is_none()
            {
                let (dx, dz) = (from.x - to.0, from.z - to.1);
                let len = (dx * dx + dz * dz).sqrt().max(1e-3);
                to.0 += dx / len * 1.3;
                to.1 += dz / len * 1.3;
            }
            let color = self.player_color(b.owner);
            rally_marker(
                out,
                from,
                b.surface,
                to,
                field,
                color,
                camera,
                size,
                input.scale,
                seconds,
                b.id,
            );
        }
        if let Some(mark) = &input.placement {
            placement(out, mark, field, camera, size, input.scale);
        }
        for m in &input.markers {
            marker(out, m, field, camera, size);
        }
        if let Some(a) = &input.aim {
            aim(out, a, field, camera, size, input.scale);
        }
        // Bars, badges and labels, farthest first so nearer ones draw on top.
        let mut order: Vec<(f32, usize)> = bodies
            .iter()
            .enumerate()
            .map(|(i, b)| (camera.eye.distance_squared(at(b)), i))
            .collect();
        order.sort_by(|a, b| b.0.total_cmp(&a.0));
        let s = input.scale;
        for &(_, i) in &order {
            let b = &bodies[i];
            let p = at(b);
            let chosen = selected(b.id);
            if health_bar_visible(b, chosen, input.health)
                && let Some(anchor) = project(camera, size, p + Vec3::Y * (b.height + 0.25))
            {
                let has_mana = b.max_mana > 0;
                let (w, h) = (
                    if b.building { 76.0 } else { 52.0 },
                    if has_mana { 15.0 } else { 8.0 },
                );
                let (x0, y1) = ((anchor.x - w * s * 0.5).round(), anchor.y.round());
                let (x1, y0) = (x0 + w * s, y1 - h * s);
                let hp = b.hp.unwrap_or(0);
                let tint = linear(health_color(hp, b.max_hp));
                let params = [w, h, health_ratio(hp, b.max_hp), b.max_hp as f32];
                let extra = [
                    health_ratio(b.mana.unwrap_or(0), b.max_mana),
                    if has_mana { 1.0 } else { 0.0 },
                    0.0,
                    0.0,
                ];
                let v = |x: f32, y: f32, u: f32, t: f32| OverlayVertex {
                    position: [x, y],
                    uv: [u, t],
                    color: [tint[0], tint[1], tint[2], 1.0],
                    params,
                    extra,
                    kind: [overlay::VITAL, 0, 0, 0],
                };
                out.quad([
                    v(x0, y0, 0.0, 0.0),
                    v(x1, y0, w, 0.0),
                    v(x1, y1, w, h),
                    v(x0, y1, 0.0, h),
                ]);
                // Progress under the bar (review).
                let progress = if input.progress_bars && chosen {
                    b.cast
                        .map(|c| (c, linear(0x4f8fe8)))
                        .or(b.progress.map(|p| (p, linear(0xe8bf32))))
                } else {
                    None
                };
                if let Some((fill, color)) = progress {
                    let (py0, py1) = (y1 + 2.0 * s, y1 + 6.0 * s);
                    solid(out, [x0, py0, x1, py1], [0.008, 0.012, 0.016, 1.0]);
                    let inner = [
                        x0 + s,
                        py0 + s,
                        x0 + s + (x1 - x0 - 2.0 * s) * fill,
                        py1 - s,
                    ];
                    solid(out, inner, [color[0], color[1], color[2], 1.0]);
                }
            }
            // Item status badges (world-sized, above the bar).
            if !b.badges.is_empty() && !b.remembered && b.hp.is_some_and(|hp| hp > 0) {
                let columns = b.badges.len().min(6);
                for (k, icon) in b.badges.iter().enumerate() {
                    let Some(&uv) = self.icons.cells.get(icon) else {
                        continue;
                    };
                    let offset = Vec3::new(
                        ((k % 6) as f32 - (columns as f32 - 1.0) / 2.0) * 0.37,
                        b.height + 0.42 + (k / 6) as f32 * 0.37,
                        0.0,
                    );
                    let Some(c) = project(camera, size, p + offset) else {
                        continue;
                    };
                    let half = 0.34 * c.pixels_per_metre * 0.5;
                    let v = |x: f32, y: f32, u: f32, t: f32| OverlayVertex {
                        position: [x, y],
                        uv: [u, t],
                        color: [1.0; 4],
                        kind: [overlay::ICON, 0, 0, 0],
                        ..Default::default()
                    };
                    out.quad([
                        v(c.x - half, c.y - half, uv[0], uv[1]),
                        v(c.x + half, c.y - half, uv[2], uv[1]),
                        v(c.x + half, c.y + half, uv[2], uv[3]),
                        v(c.x - half, c.y + half, uv[0], uv[3]),
                    ]);
                }
            }
            // The mine's workforce label.
            if let (Some((workers, wanted)), Some(font)) = (b.workforce, &self.font)
                && !b.remembered
                && b.hp.is_some_and(|hp| hp > 0)
                && let Some(anchor) = project(camera, size, p + Vec3::Y * (b.height + 1.4))
            {
                let text = format!("Workers {workers}/{wanted}");
                let (pad_x, pad_y, border) = (10.0 * s, 4.0 * s, 1.0 * s);
                let w = (font.width(&text) + 2.0 * (pad_x + border)).round();
                let h = (font.line + 2.0 * (pad_y + border)).round();
                let (x0, y1) = ((anchor.x - w * 0.5).round(), anchor.y.round());
                let (x1, y0) = (x0 + w, y1 - h);
                let bg = linear(0x091016);
                let edge = linear(if workers >= wanted {
                    0xdaa856
                } else {
                    0x8d9991
                });
                let v = |x: f32, y: f32, u: f32, t: f32| OverlayVertex {
                    position: [x, y],
                    uv: [u, t],
                    color: [bg[0], bg[1], bg[2], f32::from(0xdfu8) / 255.0],
                    params: [w, h, 4.0 * s, border],
                    extra: [edge[0], edge[1], edge[2], 1.0],
                    kind: [overlay::BOX, 0, 0, 0],
                };
                out.quad([
                    v(x0, y0, 0.0, 0.0),
                    v(x1, y0, w, 0.0),
                    v(x1, y1, w, h),
                    v(x0, y1, 0.0, h),
                ]);
                let ink = linear(0xf2dfb1);
                font.draw(
                    out,
                    &text,
                    [x0 + border + pad_x, y0 + border + pad_y],
                    [ink[0], ink[1], ink[2], 1.0],
                );
            }
        }
    }
}

fn solid(out: &mut WorldUiFrame, [x0, y0, x1, y1]: [f32; 4], color: [f32; 4]) {
    let v = |x: f32, y: f32| OverlayVertex {
        position: [x, y],
        color,
        kind: [overlay::SOLID, 0, 0, 0],
        ..Default::default()
    };
    out.quad([v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)]);
}

/// A selection ring draped on the walking surface (old `SelectionCircles.drape`): one
/// quad per sector from the inner glow to the rim, its profile drawn per pixel.
#[allow(clippy::too_many_arguments)]
fn circle(
    out: &mut WorldUiFrame,
    radius: f32,
    surface: u32,
    at: Vec3,
    field: &dyn WaterField,
    hex: u32,
    hover: bool,
) {
    let color = linear(hex);
    let width = (0.07 + radius * 0.025).min(0.2) * if hover { 0.7 } else { 1.0 };
    let opacity = if hover { 0.5 } else { 1.0 };
    let (inner, outer) = (PROFILE[0].1, PROFILE[PROFILE.len() - 1].1);
    let corner = |a: f32, across: f32, height: f32| {
        let radius = radius + width * across;
        GroundVertex {
            position: [at.x + a.cos() * radius, height, at.z + a.sin() * radius],
            color: [color[0], color[1], color[2], opacity],
            across,
            ring_profile: 1.0,
        }
    };
    let angle = |s: usize| s as f32 / SECTORS as f32 * std::f32::consts::TAU;
    let height =
        |a: f32| field.walk(at.x + a.cos() * radius, at.z + a.sin() * radius, surface) + 0.07;
    let mut h0 = height(0.0);
    let first = h0;
    for s in 0..SECTORS {
        let (a0, a1) = (angle(s), angle(s + 1));
        let h1 = if s + 1 == SECTORS { first } else { height(a1) };
        let (p, q, r, t) = (
            corner(a0, inner, h0),
            corner(a1, inner, h1),
            corner(a1, outer, h1),
            corner(a0, outer, h0),
        );
        out.ground.extend_from_slice(&[p, q, r, p, r, t]);
        h0 = h1;
    }
}

/// The placement grid: faint cell fills and their outlines, draped on the ground, and a
/// marker at the entrance (white, or `#f26960` where the site is refused).
fn placement(
    out: &mut WorldUiFrame,
    mark: &PlacementMark,
    field: &dyn WaterField,
    camera: &CameraView,
    size: [u32; 2],
    scale: f32,
) {
    let color = linear(if mark.valid { 0xffffff } else { 0xf26960 });
    let point = |x: f32, z: f32| Vec3::new(x, field.walk(x, z, 0) + 0.08, z);
    let fill = |p: Vec3| GroundVertex {
        position: p.to_array(),
        color: [color[0], color[1], color[2], 0.13],
        ..Default::default()
    };
    let [x0, z0, x1, z1] = mark.cells;
    for cz in z0..=z1 {
        for cx in x0..=x1 {
            let (l, t) = (cx as f32 - 0.5, cz as f32 - 0.5);
            let (a, b, c, d) = (
                point(l, t),
                point(l + 1.0, t),
                point(l + 1.0, t + 1.0),
                point(l, t + 1.0),
            );
            out.ground
                .extend_from_slice(&[fill(a), fill(d), fill(b), fill(b), fill(d), fill(c)]);
        }
    }
    // Grid lines, about a pixel wide.
    let line = |out: &mut WorldUiFrame, a: Vec3, b: Vec3| {
        let mid = (a + b) * 0.5;
        let Some(p) = project(camera, size, mid) else {
            return;
        };
        let half = 0.6 * scale / p.pixels_per_metre.max(1e-3);
        let side = (b - a)
            .normalize_or_zero()
            .cross(camera.eye - mid)
            .normalize_or_zero()
            * half;
        let v = |p: Vec3| GroundVertex {
            position: p.to_array(),
            color: [color[0], color[1], color[2], 0.65],
            ..Default::default()
        };
        out.ground.extend_from_slice(&[
            v(a - side),
            v(a + side),
            v(b + side),
            v(a - side),
            v(b + side),
            v(b - side),
        ]);
    };
    for cx in x0..=x1 + 1 {
        let x = cx as f32 - 0.5;
        for cz in z0..=z1 {
            let z = cz as f32 - 0.5;
            line(out, point(x, z), point(x, z + 1.0));
        }
    }
    for cz in z0..=z1 + 1 {
        let z = cz as f32 - 0.5;
        for cx in x0..=x1 {
            let x = cx as f32 - 0.5;
            line(out, point(x, z), point(x + 1.0, z));
        }
    }
    // The entrance: a small diamond lying on the ground.
    let e = point(mark.entrance[0], mark.entrance[1]) + Vec3::Y * 0.12;
    let r = 0.35;
    let c = [1.0, 0.85, 0.34, 0.9];
    let v = |p: Vec3| GroundVertex {
        position: p.to_array(),
        color: c,
        ..Default::default()
    };
    let (n, so, w, ea) = (
        e + Vec3::new(0.0, 0.0, -r),
        e + Vec3::new(0.0, 0.0, r),
        e + Vec3::new(-r, 0.0, 0.0),
        e + Vec3::new(r, 0.0, 0.0),
    );
    out.ground
        .extend_from_slice(&[v(n), v(ea), v(so), v(n), v(so), v(w)]);
}

/// The outline of a spell's area at the cursor (old `abilityOutline`): a line delivery's
/// rectangle from the caster, else 48 points around the point.
pub fn aim_outline(a: &AimMark) -> Vec<[f32; 2]> {
    if let Some((length, width)) = a.line {
        let (dx, dz) = (a.point[0] - a.origin[0], a.point[1] - a.origin[1]);
        let n = dx.hypot(dz);
        let (ux, uz) = if n > 0.0 {
            (dx / n, dz / n)
        } else {
            (1.0, 0.0)
        };
        return [
            (0.0, -width),
            (length, -width),
            (length, width),
            (0.0, width),
        ]
        .map(|(along, across)| {
            [
                a.origin[0] + ux * along - uz * across,
                a.origin[1] + uz * along + ux * across,
            ]
        })
        .to_vec();
    }
    (0..48)
        .map(|i| {
            let t = i as f32 * std::f32::consts::PI / 24.0;
            [
                a.point[0] + t.cos() * a.radius,
                a.point[1] + t.sin() * a.radius,
            ]
        })
        .collect()
}

/// A spell aimed at the cursor (old `AbilityTarget`), drawn over everything: the area
/// filled faintly (`#68dac9`, refused `#ef514b`) with a bright edge (`#befff0` /
/// `#ff7770`), draped on the ground a metre at a time, and the cast range as 48 dashes
/// around the caster (`#e8dfb0`).
fn aim(
    out: &mut WorldUiFrame,
    a: &AimMark,
    field: &dyn WaterField,
    camera: &CameraView,
    size: [u32; 2],
    scale: f32,
) {
    let ground = |p: [f32; 2]| {
        project(
            camera,
            size,
            Vec3::new(p[0], field.walk(p[0], p[1], 0) + 0.12, p[1]),
        )
        .map(|q| [q.x, q.y])
    };
    let solid = |p: [f32; 2], color: [f32; 4]| OverlayVertex {
        position: p,
        color,
        kind: [overlay::SOLID, 0, 0, 0],
        ..Default::default()
    };
    let rgba = |hex: u32, alpha: f32| {
        let [r, g, b] = linear(hex);
        [r, g, b, alpha]
    };
    let line = |out: &mut WorldUiFrame, p: [f32; 2], q: [f32; 2], color: [f32; 4]| {
        let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
        let n = dx.hypot(dy);
        if n <= 1e-3 {
            return;
        }
        let half = 0.75 * scale;
        let (sx, sy) = (-dy / n * half, dx / n * half);
        out.quad([
            solid([p[0] + sx, p[1] + sy], color),
            solid([q[0] + sx, q[1] + sy], color),
            solid([q[0] - sx, q[1] - sy], color),
            solid([p[0] - sx, p[1] - sy], color),
        ]);
    };
    let (fill, edge) = if a.valid {
        (rgba(0x68dac9, 0.16), rgba(0xbefff0, 0.9))
    } else {
        (rgba(0xef514b, 0.16), rgba(0xff7770, 0.9))
    };
    let outline = aim_outline(a);
    let n = outline.len() as f32;
    let centre = outline
        .iter()
        .fold([0.0, 0.0], |c, p| [c[0] + p[0] / n, c[1] + p[1] / n]);
    let centre_px = ground(centre);
    for (i, &p) in outline.iter().enumerate() {
        let q = outline[(i + 1) % outline.len()];
        let steps = (q[0] - p[0]).hypot(q[1] - p[1]).ceil().max(1.0) as usize;
        let at = |j: usize| {
            let t = j as f32 / steps as f32;
            [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]
        };
        for j in 0..steps {
            let (Some(u), Some(v)) = (ground(at(j)), ground(at(j + 1))) else {
                continue;
            };
            if let Some(c) = centre_px {
                out.overlay
                    .extend_from_slice(&[solid(c, fill), solid(u, fill), solid(v, fill)]);
            }
            line(out, u, v, edge);
        }
    }
    let range = rgba(0xe8dfb0, 0.35);
    for i in (0..96).step_by(2) {
        let [t0, t1] = [i, i + 1].map(|k| k as f32 * std::f32::consts::PI / 48.0);
        let around = |t: f32| {
            [
                a.origin[0] + t.cos() * a.range,
                a.origin[1] + t.sin() * a.range,
            ]
        };
        if let (Some(u), Some(v)) = (ground(around(t0)), ground(around(t1))) {
            line(out, u, v, range);
        }
    }
}

/// An order marker: a thin ring on the ground drawn over everything (old command
/// feedback), projected to pixels.
fn marker(
    out: &mut WorldUiFrame,
    m: &GroundMarker,
    field: &dyn WaterField,
    camera: &CameraView,
    size: [u32; 2],
) {
    if m.opacity <= 0.0 {
        return;
    }
    let (x, z) = (m.position[0], m.position[1]);
    let y = field.walk(x, z, 0) + 0.12;
    let color = [m.color[0], m.color[1], m.color[2], m.opacity];
    const SIDES: usize = 48;
    for s in 0..SIDES {
        let a0 = s as f32 / SIDES as f32 * std::f32::consts::TAU;
        let a1 = (s + 1) as f32 / SIDES as f32 * std::f32::consts::TAU;
        let at = |a: f32, r: f32| {
            project(camera, size, Vec3::new(x + a.cos() * r, y, z + a.sin() * r)).map(|p| {
                OverlayVertex {
                    position: [p.x, p.y],
                    color,
                    kind: [overlay::SOLID, 0, 0, 0],
                    ..Default::default()
                }
            })
        };
        let (Some(a), Some(b), Some(c), Some(d)) = (
            at(a0, m.radius),
            at(a1, m.radius),
            at(a1, m.radius + 0.12),
            at(a0, m.radius + 0.12),
        ) else {
            continue;
        };
        out.quad([a, b, c, d]);
    }
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// A rally route and flag (old `RallyMarkers`): a dashed line from the building marching
/// towards the flag, a pulsing ring on the ground and the team banner on its pole.
#[allow(clippy::too_many_arguments)]
fn rally_marker(
    out: &mut WorldUiFrame,
    from: Vec3,
    from_surface: u32,
    (tx, tz, to_surface): (f32, f32, u32),
    field: &dyn WaterField,
    color: [f32; 3],
    camera: &CameraView,
    size: [u32; 2],
    scale: f32,
    seconds: f32,
    id: u32,
) {
    const FLAG_SCALE: f32 = 1.6;
    const POLE: f32 = 1.9;
    const BANNER: (f32, f32) = (0.78, 0.46);
    let ground = field.walk(tx, tz, to_surface);
    let foot = Vec3::new(tx, ground, tz);
    // The dashed route, sampled every half metre on the ground.
    let line = mix(color, [1.0; 3], 0.45);
    let (dx, dz) = (tx - from.x, tz - from.z);
    let length = (dx * dx + dz * dz).sqrt();
    let steps = (length / 0.5).ceil().max(1.0) as usize;
    let point = |t: f32| {
        let (x, z) = (from.x + dx * t, from.z + dz * t);
        let surface = if t < 0.5 { from_surface } else { to_surface };
        Vec3::new(x, field.walk(x, z, surface) + 0.14, z)
    };
    let (dash, gap) = (0.55, 0.35);
    let offset = -seconds * 1.4;
    for i in 0..steps {
        let (t0, t1) = (i as f32 / steps as f32, (i + 1) as f32 / steps as f32);
        // Only the dashed parts of this sample.
        let d0 = t0 * length - offset;
        let phase = d0.rem_euclid(dash + gap);
        if phase > dash {
            continue;
        }
        let (a, b) = (point(t0), point(t1));
        let mid = (a + b) * 0.5;
        let Some(p) = project(camera, size, mid) else {
            continue;
        };
        let half = 1.25 * scale / p.pixels_per_metre.max(1e-3);
        let along = (b - a).normalize_or_zero();
        let side = along.cross(camera.eye - mid).normalize_or_zero() * half;
        let c = [line[0], line[1], line[2], 0.85];
        let v = |p: Vec3| GroundVertex {
            position: p.to_array(),
            color: c,
            ..Default::default()
        };
        out.ground.extend_from_slice(&[
            v(a - side),
            v(a + side),
            v(b + side),
            v(a - side),
            v(b + side),
            v(b - side),
        ]);
    }
    // The pulsing ring around the flag's foot.
    let ring = mix(color, [1.0; 3], 0.25);
    let alpha = 0.55 + 0.35 * (seconds * 3.2).sin();
    let (inner, outer) = (0.42 * FLAG_SCALE, 0.55 * FLAG_SCALE);
    for s in 0..40 {
        let a0 = s as f32 / 40.0 * std::f32::consts::TAU;
        let a1 = (s + 1) as f32 / 40.0 * std::f32::consts::TAU;
        let at = |a: f32, r: f32| GroundVertex {
            position: [
                tx + a.cos() * r,
                ground + 0.07 * FLAG_SCALE,
                tz + a.sin() * r,
            ],
            color: [ring[0], ring[1], ring[2], alpha],
            ..Default::default()
        };
        out.ground.extend_from_slice(&[
            at(a0, inner),
            at(a0, outer),
            at(a1, outer),
            at(a0, inner),
            at(a1, outer),
            at(a1, inner),
        ]);
    }
    // The flag over everything (the old one drew without depth so canopies cannot hide
    // it): pole, finial and the waving banner, projected to pixels.
    let px = |p: Vec3| project(camera, size, p).map(|q| [q.x, q.y]);
    let quad = |out: &mut WorldUiFrame, corners: [Vec3; 4], color: [f32; 3]| {
        let mut v = [OverlayVertex::default(); 4];
        for (k, c) in corners.iter().enumerate() {
            let Some(p) = px(*c) else { return };
            v[k] = OverlayVertex {
                position: p,
                color: [color[0], color[1], color[2], 1.0],
                kind: [overlay::SOLID, 0, 0, 0],
                ..Default::default()
            };
        }
        out.quad(v);
    };
    let right = Vec3::new(
        camera.view.x_axis.x,
        camera.view.y_axis.x,
        camera.view.z_axis.x,
    );
    let pole_top = foot + Vec3::Y * POLE * FLAG_SCALE;
    let r = 0.045 * FLAG_SCALE;
    quad(
        out,
        [
            pole_top - right * r,
            pole_top + right * r,
            foot + right * r,
            foot - right * r,
        ],
        linear(0x5a4030),
    );
    let finial = pole_top + Vec3::Y * 0.04 * FLAG_SCALE;
    let f = 0.07 * FLAG_SCALE;
    quad(
        out,
        [
            finial + Vec3::new(-f, f, 0.0),
            finial + Vec3::new(f, f, 0.0),
            finial + Vec3::new(f, -f, 0.0),
            finial + Vec3::new(-f, -f, 0.0),
        ],
        linear(0xd8b25a),
    );
    let t = seconds + id as f32 * 0.37;
    let top = (POLE - 0.06) * FLAG_SCALE;
    let bottom = top - BANNER.1 * FLAG_SCALE;
    let banner = mix(color, [0.0; 3], 0.1);
    for k in 0..8 {
        let u0 = k as f32 / 8.0;
        let u1 = (k + 1) as f32 / 8.0;
        let at = |u: f32, y: f32| {
            let wave = (t * 4.2 - u * 5.5).sin() * 0.09 * u * FLAG_SCALE;
            foot + Vec3::new(u * BANNER.0 * FLAG_SCALE, y, wave)
        };
        let shade = 0.85 + 0.15 * (t * 4.2 - u0 * 5.5).cos();
        quad(
            out,
            [at(u0, top), at(u1, top), at(u1, bottom), at(u0, bottom)],
            [banner[0] * shade, banner[1] * shade, banner[2] * shade],
        );
    }
}

/// Where the camera's view meets the ground plane at its focus height (x, z), corners
/// from the top left clockwise: the minimap's view quad (old `camera.viewGround`).
pub fn ground_footprint(camera: &CameraView) -> [[f32; 2]; 4] {
    let inverse = camera.view_projection.inverse();
    let height = camera.target.y;
    [[-1.0, 1.0], [1.0, 1.0], [1.0, -1.0], [-1.0, -1.0]].map(|[x, y]| {
        let near = inverse.project_point3(Vec3::new(x, y, 1.0));
        let far = inverse.project_point3(Vec3::new(x, y, 0.0));
        let d = far - near;
        // Rays that never reach the ground stop far away along their direction.
        let t = if d.y.abs() > 1e-6 {
            ((height - near.y) / d.y).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let p = near + d * t;
        [p.x, p.z]
    })
}

/// The minimap's markers (old `entityMarker`): units as 2-pixel squares and buildings at
/// their footprint in player colours lifted by 1.4, deposits yellow, other unowned
/// buildings sand, remembered ones at half strength; then the camera's view quad with a
/// dark outline. Map cells span the minimap's `pixels`.
pub fn minimap_frame(
    bodies: &[Body],
    position: &dyn Fn(u64) -> Option<Vec3>,
    colors: &[[f32; 3]],
    area: [f32; 3],
    pixels: u32,
    view: [[f32; 2]; 4],
    out: &mut canopy_render::minimap::MinimapFrame,
) {
    out.markers.clear();
    let scale = pixels as f32 / area[2].max(1.0);
    let (ox, oz) = (area[0], area[1]);
    let srgb = |c: f32| {
        if c <= 0.0031308 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        }
    };
    let lin = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    for b in bodies {
        let unowned = b.owner.is_none();
        if b.item
            || b.hp == Some(0)
            || (b.building && unowned && !b.deposit && b.footprint.is_none())
        {
            continue;
        }
        let p = position(b.key).unwrap_or(b.position);
        let [w, h] = match (b.building, b.footprint) {
            (true, Some([w, h])) => [(w * scale).max(1.0), (h * scale).max(1.0)],
            _ => [2.0, 2.0],
        };
        let color = match b.owner {
            Some(slot) => {
                let c = colors
                    .get(usize::from(slot) % colors.len().max(1))
                    .copied()
                    .unwrap_or([1.0; 3]);
                c.map(|v| lin((srgb(v) * 1.4).min(1.0)))
            }
            None if b.deposit => linear(0xffd43b),
            None => linear(0xa99c78),
        };
        let alpha = if b.remembered { 0.5 } else { 1.0 };
        let (x, y) = ((p.x - ox) * scale, (p.z - oz) * scale);
        out.rect(
            [x - w / 2.0, y - h / 2.0, x + w / 2.0, y + h / 2.0],
            [color[0], color[1], color[2], alpha],
        );
    }
    let corners = view.map(|[x, z]| [(x - ox) * scale, (z - oz) * scale]);
    let dark = linear(0x10150e);
    let light = linear(0xf2eee0);
    for (width, color) in [
        (3.0, [dark[0], dark[1], dark[2], 0.8]),
        (1.4, [light[0], light[1], light[2], 1.0]),
    ] {
        for i in 0..4 {
            out.line(corners[i], corners[(i + 1) % 4], width, color);
        }
    }
}

/// A team ping on the minimap: cell, seconds since it was made, the pinging player's
/// colour (linear).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinimapPing {
    pub x: f32,
    pub z: f32,
    pub age: f32,
    pub color: [f32; 3],
}

/// Ping marks on top of the minimap: a dot, and a ring that grows out of it each second
/// and fades, with a dark edge so it reads on any ground.
pub fn minimap_pings(
    pings: &[MinimapPing],
    area: [f32; 3],
    pixels: u32,
    out: &mut canopy_render::minimap::MinimapFrame,
) {
    let scale = pixels as f32 / area[2].max(1.0);
    let (ox, oz) = (area[0], area[1]);
    let reach = pixels as f32 * 0.07;
    let dark = linear(0x10150e);
    for p in pings.iter().filter(|p| (0.0..4.0).contains(&p.age)) {
        let (x, y) = ((p.x - ox) * scale, (p.z - oz) * scale);
        let fade = (1.0 - p.age / 4.0).clamp(0.0, 1.0);
        let pulse = p.age.fract();
        let radius = 2.0 + reach * pulse;
        let alpha = fade * (1.0 - pulse * 0.7);
        const SIDES: usize = 24;
        let at = |i: usize| {
            let a = i as f32 / SIDES as f32 * std::f32::consts::TAU;
            [x + radius * a.cos(), y + radius * a.sin()]
        };
        for (width, color) in [
            (3.2, [dark[0], dark[1], dark[2], alpha * 0.7]),
            (1.6, [p.color[0], p.color[1], p.color[2], alpha]),
        ] {
            for i in 0..SIDES {
                out.line(at(i), at(i + 1), width, color);
            }
        }
        let d = 2.5;
        out.rect(
            [x - d, y - d, x + d, y + d],
            [p.color[0], p.color[1], p.color[2], fade],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::Body;

    #[test]
    fn aim_outlines_follow_the_old_shapes() {
        let mut aim = AimMark {
            origin: [10.0, 10.0],
            point: [14.0, 10.0],
            radius: 2.0,
            line: None,
            range: 8.0,
            valid: true,
        };
        // A disc: 48 points at the radius around the point.
        let disc = aim_outline(&aim);
        assert_eq!(disc.len(), 48);
        for p in &disc {
            assert!(((p[0] - 14.0).hypot(p[1] - 10.0) - 2.0).abs() < 1e-4);
        }
        // A line: the rectangle from the caster towards the point, half width across.
        aim.line = Some((6.0, 1.0));
        assert_eq!(
            aim_outline(&aim),
            vec![[10.0, 9.0], [16.0, 9.0], [16.0, 11.0], [10.0, 11.0]]
        );
    }

    fn body(hp: u32, max: u32, building: bool) -> Body {
        Body {
            id: 1,
            key: 0,
            position: Vec3::ZERO,
            building,
            item: false,
            hero: false,
            relation: Relation::Own,
            owner: Some(0),
            hp: Some(hp),
            max_hp: max,
            mana: None,
            max_mana: 0,
            radius: 0.9,
            height: 2.0,
            remembered: false,
            hostile: Some(false),
            surface: 0,
            workforce: None,
            rally: None,
            badges: Vec::new(),
            progress: None,
            constructing: false,
            cast: None,
            commanded: None,
            model: 0,
            scale: 1.0,
            idle: None,
            footprint: None,
            deposit: false,
        }
    }

    #[test]
    fn health_bars_follow_the_old_rules_and_settings() {
        let hurt = body(50, 100, false);
        let whole = body(100, 100, false);
        let wall = body(50, 100, true);
        assert!(health_bar_visible(&hurt, false, HealthBars::Damaged));
        assert!(!health_bar_visible(&whole, false, HealthBars::Damaged));
        // Damaged buildings show like units; one still being raised does not.
        assert!(health_bar_visible(&wall, false, HealthBars::Damaged));
        let raising = Body {
            constructing: true,
            ..wall.clone()
        };
        assert!(!health_bar_visible(&raising, false, HealthBars::Damaged));
        assert!(health_bar_visible(&raising, true, HealthBars::Damaged));
        assert!(health_bar_visible(&whole, false, HealthBars::Always));
        assert!(!health_bar_visible(&hurt, false, HealthBars::Selected));
        let mut dead = body(0, 100, false);
        assert!(!health_bar_visible(&dead, true, HealthBars::Always));
        dead.hp = Some(10);
        dead.remembered = true;
        assert!(!health_bar_visible(&dead, true, HealthBars::Always));
    }

    #[test]
    fn health_colours_ramp_red_amber_green() {
        assert_eq!(health_color(0, 100), 0xdb3935);
        assert_eq!(health_color(50, 100), 0xe8bf32);
        assert_eq!(health_color(100, 100), 0x64cc39);
        assert_eq!(health_color(1, 0), 0xdb3935);
    }

    #[test]
    fn the_view_footprint_surrounds_the_focus() {
        let camera = CameraView::look_at(
            Vec3::new(50.0, 40.0, 80.0),
            Vec3::new(50.0, 0.0, 50.0),
            0.6,
            16.0 / 9.0,
            1.0,
            300.0,
        );
        let quad = ground_footprint(&camera);
        // The far edge (top) is wider than the near edge (bottom), and north of it.
        let top = quad[1][0] - quad[0][0];
        let bottom = quad[2][0] - quad[3][0];
        assert!(top > bottom && quad[0][1] < quad[3][1], "{quad:?}");
        let cx = quad.iter().map(|p| p[0]).sum::<f32>() / 4.0;
        assert!((cx - 50.0).abs() < 1e-3);
    }

    #[test]
    fn projection_lands_the_focus_in_the_middle() {
        let camera = CameraView::look_at(
            Vec3::new(0.0, 10.0, 10.0),
            Vec3::ZERO,
            0.6,
            16.0 / 9.0,
            1.0,
            100.0,
        );
        let p = project(&camera, [1920, 1080], Vec3::ZERO).unwrap();
        assert!((p.x - 960.0).abs() < 0.5 && (p.y - 540.0).abs() < 0.5);
        // Farther away, a metre is fewer pixels.
        let far = project(&camera, [1920, 1080], Vec3::new(0.0, 0.0, -20.0)).unwrap();
        assert!(far.pixels_per_metre < p.pixels_per_metre);
        assert!(project(&camera, [1920, 1080], Vec3::new(0.0, 10.0, 20.0)).is_none());
    }
}
