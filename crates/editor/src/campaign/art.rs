//! Campaign art in the editor: thumbnails of interface images (backgrounds, card art,
//! story pages, mission illustrations) read from their asset folders, decoded on a worker
//! thread so a picker full of them never stalls a frame, and the image picker that shows
//! them.

use crate::theme::*;
use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Stroke, TextureHandle, Ui, vec2};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::mpsc;

/// The width thumbnails are decoded at.
const WIDTH: u32 = 360;

enum Thumb {
    Loading,
    Ready(TextureHandle),
    Missing,
}

/// Thumbnails by asset id.
pub struct Thumbs {
    thumbs: HashMap<String, Thumb>,
    tx: mpsc::Sender<(String, Option<egui::ColorImage>)>,
    rx: mpsc::Receiver<(String, Option<egui::ColorImage>)>,
}

impl Default for Thumbs {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            thumbs: HashMap::new(),
            tx,
            rx,
        }
    }
}

/// The picture file of an asset folder: its image, in whichever format it has.
fn picture_file(folder: &std::path::Path) -> Option<PathBuf> {
    ["image.webp", "image.png", "image.jpg", "image.jpeg"]
        .iter()
        .map(|name| folder.join(name))
        .find(|p| p.is_file())
}

fn decode(file: &std::path::Path) -> Option<egui::ColorImage> {
    let image = image::open(file).ok()?;
    let height = (image.height() as f32 * WIDTH as f32 / image.width().max(1) as f32) as u32;
    let small = image
        .resize(WIDTH, height.max(1), image::imageops::FilterType::Triangle)
        .to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [small.width() as usize, small.height() as usize],
        &small,
    ))
}

impl Thumbs {
    /// Takes the thumbnails decoded since the last frame.
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((id, image)) = self.rx.try_recv() {
            let thumb = match image {
                Some(image) => Thumb::Ready(ctx.load_texture(
                    format!("campaign-art/{id}"),
                    image,
                    egui::TextureOptions::LINEAR,
                )),
                None => Thumb::Missing,
            };
            self.thumbs.insert(id, thumb);
        }
    }

    /// The thumbnail of asset `id`, whose `asset.json` is `document` (under `root`, the
    /// package); asked for on first use.
    pub fn get(
        &mut self,
        ctx: &egui::Context,
        id: &str,
        document: Option<&str>,
        root: Option<&std::path::Path>,
    ) -> Option<egui::TextureId> {
        match self.thumbs.get(id) {
            Some(Thumb::Ready(t)) => return Some(t.id()),
            Some(_) => return None,
            None => {}
        }
        let file = root
            .zip(document)
            .and_then(|(root, d)| root.join(d).parent().map(std::path::Path::to_path_buf))
            .and_then(|folder| picture_file(&folder));
        let Some(file) = file else {
            self.thumbs.insert(id.to_string(), Thumb::Missing);
            return None;
        };
        self.thumbs.insert(id.to_string(), Thumb::Loading);
        let (tx, id, ctx) = (self.tx.clone(), id.to_string(), ctx.clone());
        let spawned = std::thread::Builder::new()
            .name("campaign art".into())
            .spawn(move || {
                let _ = tx.send((id, decode(&file)));
                ctx.request_repaint();
            });
        if spawned.is_err() {
            tracing::warn!("editor: cannot decode campaign art");
        }
        None
    }
}

/// Paints `texture` cut to fill `rect` (or a placeholder saying `empty`).
pub fn paint_cover(ui: &Ui, rect: Rect, texture: Option<egui::TextureId>, empty: &str) {
    let painter = ui.painter_at(rect);
    match texture.and_then(|t| ui.ctx().tex_manager().read().meta(t).map(|m| (t, m.size))) {
        Some((t, [w, h])) => {
            // Cover: cut the picture to the rect's shape from its middle.
            let (w, h) = (w as f32, h as f32);
            let aspect = rect.width() / rect.height().max(1.0);
            let uv = if w / h > aspect {
                let cut = (1.0 - aspect * h / w) / 2.0;
                Rect::from_min_max(egui::pos2(cut, 0.0), egui::pos2(1.0 - cut, 1.0))
            } else {
                let cut = (1.0 - w / h / aspect) / 2.0;
                Rect::from_min_max(egui::pos2(0.0, cut), egui::pos2(1.0, 1.0 - cut))
            };
            painter
                .add(egui::epaint::RectShape::filled(rect, 6, Color32::WHITE).with_texture(t, uv));
        }
        None => {
            painter.rect_filled(rect, 6, SUNK);
            painter.rect_stroke(rect, 6, Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
            if !empty.is_empty() {
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    empty,
                    FontId::proportional((rect.height() * 0.16).clamp(9.0, 12.0)),
                    FAINT,
                );
            }
        }
    }
}

/// What a picker can choose from: asset ids, each with its `asset.json`.
pub type Choices = BTreeMap<String, String>;

/// An image field: the chosen picture (or none) with its id; Choose… opens a grid of
/// every interface image with its thumbnail, a search, and None. Returns the new value
/// (`Some("")` for none).
#[allow(clippy::too_many_arguments)]
pub fn picker(
    ui: &mut Ui,
    salt: &str,
    current: &str,
    choices: &Choices,
    thumbs: &mut Thumbs,
    root: Option<&std::path::Path>,
    aspect: f32,
    search: &mut String,
) -> Option<String> {
    let mut chosen = None;
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, (width / aspect).min(180.0)), Sense::click());
    let texture = (!current.is_empty())
        .then(|| {
            thumbs.get(
                ui.ctx(),
                current,
                choices.get(current).map(String::as_str),
                root,
            )
        })
        .flatten();
    paint_cover(
        ui,
        rect,
        texture,
        if current.is_empty() {
            "No picture"
        } else {
            "Not found"
        },
    );
    if response.hovered() {
        ui.painter()
            .rect_stroke(rect, 6, Stroke::new(1.0, CONTROL), egui::StrokeKind::Inside);
    }
    ui.horizontal(|ui| {
        let label = if current.is_empty() { "—" } else { current };
        ui.add(
            egui::Label::new(RichText::new(label).size(10.5).color(FAINT).monospace()).truncate(),
        );
    });
    let choose = ui.button("Choose…");
    let popup = egui::Popup::default_response_id(&choose);
    if response.clicked() {
        egui::Popup::open_id(ui.ctx(), popup);
    }
    let _ = salt;
    egui::Popup::from_toggle_button_response(&choose)
        .id(popup)
        .align(egui::RectAlign::LEFT_START)
        .align_alternatives(&[egui::RectAlign::BOTTOM_START, egui::RectAlign::TOP_START])
        .gap(10.0)
        .frame(crate::world::strip_frame().inner_margin(10))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(560.0);
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(search)
                        .hint_text("Search interface images…")
                        .desired_width(380.0),
                );
                if ui.button("None").clicked() {
                    chosen = Some(String::new());
                    ui.close();
                }
            });
            ui.add_space(6.0);
            let filter = search.trim().to_lowercase();
            egui::ScrollArea::vertical()
                .max_height(420.0)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                        for (asset, document) in choices {
                            if !filter.is_empty() && !asset.to_lowercase().contains(&filter) {
                                continue;
                            }
                            let (tile, r) =
                                ui.allocate_exact_size(vec2(124.0, 96.0), Sense::click());
                            let image = Rect::from_min_size(tile.min, vec2(124.0, 70.0));
                            let t = thumbs.get(ui.ctx(), asset, Some(document), root);
                            paint_cover(ui, image, t, "…");
                            let on = asset == current;
                            if on || r.hovered() {
                                ui.painter().rect_stroke(
                                    image,
                                    6,
                                    Stroke::new(
                                        if on { 2.0 } else { 1.0 },
                                        if on { TEXT } else { SOFT },
                                    ),
                                    egui::StrokeKind::Outside,
                                );
                            }
                            let short = asset.rsplit('.').next().unwrap_or(asset);
                            let mut job = egui::text::LayoutJob::simple_singleline(
                                short.to_string(),
                                FontId::proportional(10.5),
                                if on { TEXT } else { MUTED },
                            );
                            job.wrap.max_width = 124.0;
                            job.wrap.max_rows = 1;
                            let galley = ui.painter().layout_job(job);
                            ui.painter().galley(
                                image.left_bottom() + vec2(0.0, 6.0),
                                galley,
                                MUTED,
                            );
                            if r.on_hover_text(asset).clicked() {
                                chosen = Some(asset.clone());
                                ui.close();
                            }
                        }
                    });
                });
        });
    chosen.filter(|c| c != current)
}
