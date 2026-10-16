//! Renders every widget in the old game's style to `target/shots/ui-gallery.png`
//! through the reference wgpu pass, offscreen.
//!
//! `cargo run -p canopy-ui --example gallery [-- --size 1920x1080 --scale 1 --out path]`
//!
//! Images are decoded from the base package's asset folders (run `git lfs pull` for real
//! art); the game binds the same names (`<asset id>/<file>`) from the asset pack instead.

#[path = "shared/sample.rs"]
mod sample;

use canopy_ui::gpu::{UiRenderer, headless_device, render_to_rgba, upload_rgba};
use canopy_ui::{FontSources, ImageDesc, InputEvent, PointerButton, TextureId, Ui, UiConfig};
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn main() {
    let mut size = (1920u32, 1080u32);
    let mut scale = 1.0f32;
    let mut out = repo().join("target/shots/ui-gallery.png");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--size" => {
                let v = args.next().expect("--size WxH");
                let (w, h) = v.split_once('x').expect("--size WxH");
                size = (w.parse().expect("width"), h.parse().expect("height"));
            }
            "--scale" => scale = args.next().expect("--scale").parse().expect("scale"),
            "--out" => out = PathBuf::from(args.next().expect("--out path")),
            _ => panic!("unknown argument {a}"),
        }
    }
    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter: gallery skipped");
        return;
    };
    let config = UiConfig::load(&repo().join("config/ui")).expect("config/ui");
    let mut ui = Ui::new(
        config,
        FontSources {
            bundled: Vec::new(),
            system: true,
        },
    )
    .expect("ui");
    let mut renderer = UiRenderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);

    // Theme images, icons and two stand-ins for render targets.
    let mut next_texture = 0u32;
    let mut images: Vec<(String, String)> = ui
        .theme_images()
        .map(|(n, p)| (n.to_owned(), p.to_owned()))
        .collect();
    for name in sample::ICONS {
        let path = sample::icon(name);
        images.push((path.clone(), path));
    }
    let folders = asset_folders(&repo().join("base/assets"));
    for (name, path) in images {
        let file = path
            .split_once('/')
            .and_then(|(id, file)| folders.get(id).map(|folder| folder.join(file)));
        let Some((w, h, rgba)) = file.as_deref().and_then(decode) else {
            eprintln!("skipped {path} (missing or an LFS pointer)");
            continue;
        };
        let id = TextureId(next_texture);
        next_texture += 1;
        let view = upload_rgba(&device, &queue, &name, w, h, &rgba);
        renderer.set_texture(&device, id, &view);
        ui.register_image(&name, ImageDesc::new(id, w as f32, h as f32));
    }
    for (name, (w, h, rgba)) in [("@minimap", minimap()), ("@portrait", portrait())] {
        let id = TextureId(next_texture);
        next_texture += 1;
        let view = upload_rgba(&device, &queue, name, w, h, &rgba);
        renderer.set_texture(&device, id, &view);
        ui.register_image(name, ImageDesc::new(id, w as f32, h as f32));
    }

    ui.set_viewport(size.0, size.1, scale);
    ui.show("gallery", true);
    ui.show("hud", true);
    sample::populate_hud(&mut ui);
    sample::populate_log(&mut ui, "gallery/log", 100);
    // Every frame's draw list goes to the renderer: it carries glyph-atlas uploads.
    renderer.prepare(&device, &queue, ui.frame(0.0));

    // Interact: open the dropdown, focus the text field, hover a command.
    let center = |ui: &Ui, path: &str| {
        let id = ui.find(path).unwrap_or_else(|| panic!("{path}"));
        let r = ui.rect(id).expect("laid out");
        (r.x + r.w * 0.5, r.y + r.h * 0.5)
    };
    let click = |ui: &mut Ui, (x, y): (f32, f32)| {
        ui.input(&InputEvent::PointerMoved { x, y });
        ui.input(&InputEvent::PointerButton {
            button: PointerButton::Primary,
            pressed: true,
        });
        ui.input(&InputEvent::PointerButton {
            button: PointerButton::Primary,
            pressed: false,
        });
    };
    let p = center(&ui, "gallery/shadows");
    click(&mut ui, p);
    let field = ui.find("gallery/name");
    ui.focus(field);
    let commands = ui.find("hud/dock/actions/commands").expect("commands");
    let attack = ui.child(commands, 3).expect("attack");
    let r = ui.rect(attack).expect("laid out");
    ui.input(&InputEvent::PointerMoved {
        x: r.x + r.w * 0.5,
        y: r.y + r.h * 0.5,
    });
    renderer.prepare(&device, &queue, ui.frame(0.1));
    let list = ui.frame(0.2).clone();
    if std::env::var_os("UI_DUMP").is_some() {
        println!("{}", ui.dump());
    }
    let stats = ui.stats();
    println!(
        "{} nodes, {} quads in {} batches, atlas {}²",
        stats.nodes, stats.quads, stats.batches, list.atlas.size
    );

    let rgba = render_to_rgba(
        &device,
        &queue,
        &mut renderer,
        &list,
        [0x18, 0x18, 0x1b, 255],
    );
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).expect("output folder");
    }
    image::save_buffer(&out, &rgba, size.0, size.1, image::ColorType::Rgba8).expect("png");
    println!("wrote {}", out.display());
}

/// Asset id → its folder, from every `asset.json` below `root`.
fn asset_folders(root: &Path) -> std::collections::BTreeMap<String, std::path::PathBuf> {
    let mut out = std::collections::BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == "asset.json")
                && let Ok(text) = std::fs::read_to_string(&path)
                && let Some(id) = text
                    .split("\"id\": \"")
                    .nth(1)
                    .and_then(|rest| rest.split('"').next())
            {
                out.insert(id.to_string(), dir.clone());
            }
        }
    }
    out
}

fn decode(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.starts_with(b"version https://git-lfs") {
        return None;
    }
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    Some((img.width(), img.height(), img.into_raw()))
}

/// A stand-in for the minimap render target: terrain-ish noise with player dots.
fn minimap() -> (u32, u32, Vec<u8>) {
    let n = 256u32;
    let mut out = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as f32 / n as f32, y as f32 / n as f32);
            let v = ((fx * 13.0).sin() * (fy * 9.0).cos() + (fx * fy * 31.0).sin()) * 0.5;
            let mut c = [
                (52.0 + v * 18.0) as u8,
                (60.0 + v * 22.0) as u8,
                (44.0 + v * 12.0) as u8,
                255,
            ];
            if (fx - 0.5 - (fy * 6.0).sin() * 0.08).abs() < 0.03 {
                c = [40, 72, 96, 255];
            }
            for (px, py, col) in [(0.2, 0.25, [64, 140, 255]), (0.78, 0.7, [230, 70, 60])] {
                if (fx - px).hypot(fy - py) < 0.025 {
                    c = [col[0], col[1], col[2], 255];
                }
            }
            out.extend_from_slice(&c);
        }
    }
    (n, n, out)
}

/// A stand-in for the live portrait: a neutral vignette.
fn portrait() -> (u32, u32, Vec<u8>) {
    let (w, h) = (192u32, 256u32);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f32 / w as f32 - 0.5).hypot(y as f32 / h as f32 - 0.45) * 2.0).min(1.0);
            let v = (90.0 * (1.0 - d) + 24.0) as u8;
            out.extend_from_slice(&[v, v, (v as f32 * 1.05) as u8, 255]);
        }
    }
    (w, h, out)
}
