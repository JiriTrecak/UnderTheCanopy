//! Pictures for the tool strip's pickers, so a choice shows what it lays down: each block
//! kind as the biome's ground texture for it, each cliff face as its texture, and what
//! Place puts down as its portrait. Made from the cooked pack when a map's biome is first
//! seen; uploaded to egui as the strip first draws them.

use super::Placing;
use canopy_assets::Pack;
use canopy_content::Content;
use canopy_content::game::Kind;
use canopy_content::map::blocks::BlockKind;
use egui::{ColorImage, TextureHandle};
use std::sync::Arc;

/// One thing a picker offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Kind(BlockKind),
    /// A face style (0: the biome's own cliff).
    Face(u8),
    Place(Placing),
}

/// Pictures smaller than this are taken from a larger mip.
const SIZE: u32 = 96;

#[derive(Clone, Default)]
pub struct Palette {
    /// The biome the pictures are of.
    pub biome: String,
    images: Vec<(Choice, Arc<ColorImage>)>,
    textures: Vec<(Choice, TextureHandle)>,
}

impl std::fmt::Debug for Palette {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Palette")
            .field("biome", &self.biome)
            .field("pictures", &self.images.len())
            .finish()
    }
}

impl Palette {
    /// The pictures of `biome`'s ground and faces and of what maps place.
    pub fn new(biome: &str, pack: &Pack, look: &canopy_scene::Look, content: &Content) -> Self {
        let mut images = Vec::new();
        let mut add = |choice: Choice, entry: Option<String>, opaque: bool| {
            if let Some(image) = entry.and_then(|e| picture(pack, &e, opaque)) {
                images.push((choice, Arc::new(image)));
            }
        };
        let terrain = &look.biome.terrain;
        let blocks = look.biome.blocks.as_ref();
        let tile = |asset: &str| Some(format!("{asset}/data.bin"));
        add(
            Choice::Kind(BlockKind::Ground),
            tile(&terrain.soil.albedo),
            true,
        );
        add(
            Choice::Kind(BlockKind::Wall),
            tile(&terrain.grass.albedo),
            true,
        );
        add(
            Choice::Kind(BlockKind::Rock),
            tile(&terrain.rock.albedo),
            true,
        );
        add(
            Choice::Kind(BlockKind::Ice),
            blocks.and_then(|b| b.ice.as_deref()).and_then(tile),
            true,
        );
        if let Some(blocks) = blocks {
            add(
                Choice::Face(0),
                blocks.cliff.as_ref().map(|c| c.albedo.clone()),
                true,
            );
            for (n, face) in blocks.faces.iter().enumerate() {
                add(
                    Choice::Face(n as u8 + 1),
                    Some(face.cliff.albedo.clone()),
                    true,
                );
            }
        }
        let icon = |id: &str| {
            content
                .definitions
                .iter()
                .find(|d| d.id.as_str() == id)
                .map(|d| format!("{}/image.png", d.icon.as_str()))
        };
        let setup = content
            .rules
            .races
            .values()
            .filter_map(|r| r.starting_setup.as_ref())
            .next()
            .unwrap_or(&content.rules.starting_setup);
        add(
            Choice::Place(Placing::Start),
            icon(setup.fort.as_str()),
            false,
        );
        let mine = content.definitions.iter().find(|d| {
            d.kind == Kind::Building && d.harvesting.is_some() && d.resource_yield.is_some()
        });
        add(
            Choice::Place(Placing::Amber),
            mine.and_then(|d| icon(d.id.as_str())),
            false,
        );
        // The strongest creep stands for a camp.
        let creep = content
            .definitions
            .iter()
            .filter(|d| d.kind == Kind::Unit && d.behaviors.camp_defense.is_some())
            .max_by_key(|d| (d.level.unwrap_or(1), std::cmp::Reverse(d.id.as_str())));
        add(
            Choice::Place(Placing::Camp),
            creep.and_then(|d| icon(d.id.as_str())),
            false,
        );
        Self {
            biome: biome.to_string(),
            images,
            textures: Vec::new(),
        }
    }

    /// `choice`'s picture, if it has one.
    pub fn texture(&mut self, ctx: &egui::Context, choice: Choice) -> Option<egui::TextureId> {
        if let Some((_, t)) = self.textures.iter().find(|(c, _)| *c == choice) {
            return Some(t.id());
        }
        let (_, image) = self.images.iter().find(|(c, _)| *c == choice)?;
        let handle = ctx.load_texture(
            format!("block-palette/{}/{choice:?}", self.biome),
            image.clone(),
            egui::TextureOptions::LINEAR,
        );
        let id = handle.id();
        self.textures.push((choice, handle));
        Some(id)
    }
}

/// A pack texture's first mip of at least [`SIZE`], as an image; `opaque` drops its alpha
/// (a ground texture's roughness).
fn picture(pack: &Pack, entry: &str, opaque: bool) -> Option<ColorImage> {
    let texture = pack.texture(entry).ok()?;
    let levels = texture.meta.mips.len();
    let level = (0..levels)
        .rev()
        .find(|&l| {
            let (w, h, _) = texture.level_extent(l);
            w.min(h) >= SIZE
        })
        .unwrap_or(0);
    let (w, h, _) = texture.level_extent(level);
    let mut texels = texture.decode_level(level);
    // A texture array's first layer.
    let len = (w * h * 4) as usize;
    if texels.len() < len {
        return None;
    }
    texels.truncate(len);
    if opaque {
        for px in texels.chunks_exact_mut(4) {
            px[3] = 255;
        }
    }
    Some(ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        &texels,
    ))
}
