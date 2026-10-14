//! CPU cost of a UI frame (input, layout, text, paint) with the full HUD and 100 lines
//! of text, at 1440p. No GPU work is measured.
//!
//! `cargo run --release -p canopy-ui --example bench [-- --scale 2]`

#[path = "shared/sample.rs"]
mod sample;

use canopy_ui::{FontSources, ImageDesc, InputEvent, TextureId, Ui, UiConfig};
use std::path::Path;
use std::time::Instant;

fn main() {
    let mut scale = 1.0f32;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--scale" {
            scale = args.next().expect("--scale").parse().expect("scale");
        }
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config = UiConfig::load(&repo.join("config/ui")).expect("config/ui");
    let t = Instant::now();
    let mut ui = Ui::new(
        config,
        FontSources {
            bundled: Vec::new(),
            system: true,
        },
    )
    .expect("ui");
    println!(
        "startup (fonts, build): {:.1} ms",
        t.elapsed().as_secs_f64() * 1e3
    );

    // Textures are never sampled here; sizes are enough.
    let names: Vec<String> = ui
        .theme_images()
        .map(|(n, _)| n.to_owned())
        .chain(sample::ICONS.iter().map(|n| sample::icon(n)))
        .chain(["@minimap".to_owned(), "@portrait".to_owned()])
        .collect();
    for (i, n) in names.iter().enumerate() {
        ui.register_image(n, ImageDesc::new(TextureId(i as u32), 512.0, 512.0));
    }
    ui.set_viewport(2560, 1440, scale);
    ui.show("hud", true);
    ui.show("gallery", true);
    sample::populate_hud(&mut ui);
    sample::populate_log(&mut ui, "gallery/log", 100);
    ui.frame(0.0);

    let commands = ui.find("hud/dock/actions/commands").expect("commands");
    let cells: Vec<_> = (0..12)
        .map(|i| ui.child(commands, i).expect("cell"))
        .collect();
    let centers: Vec<(f32, f32)> = cells
        .iter()
        .map(|c| {
            let r = ui.rect(*c).expect("cell rect");
            (r.x + r.w * 0.5, r.y + r.h * 0.5)
        })
        .collect();
    let (x, y) = centers[3];
    ui.input(&InputEvent::PointerMoved { x, y });
    let sap = cells[6];
    let cooldown = ui.find_in(sap, "cooldown").expect("cooldown");
    let seconds = ui.find_in(sap, "cooldown/seconds").expect("seconds");
    let xp = ui.find("hud/dock/selection/experience").expect("xp");
    let resources = ui.find("hud/dock/actions/resources").expect("resources");
    let amber = ui
        .find_in(ui.child(resources, 0).expect("amber"), "value")
        .expect("value");
    let log = ui.find("gallery/log").expect("log");
    let lines: Vec<_> = (0..100).map(|i| ui.child(log, i).expect("line")).collect();

    let mut time = 0.0;
    let mut run = |ui: &mut Ui, name: &str, frames: usize, step: &mut dyn FnMut(&mut Ui, usize)| {
        for f in 0..60 {
            step(ui, f);
            time += 1.0 / 60.0;
            ui.frame(time);
        }
        let mut samples = Vec::with_capacity(frames);
        for f in 0..frames {
            let t = Instant::now();
            step(ui, f);
            time += 1.0 / 60.0;
            ui.frame(time);
            samples.push(t.elapsed().as_secs_f64() * 1e3);
        }
        samples.sort_by(f64::total_cmp);
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        let s = ui.stats();
        println!(
            "{name:<34} mean {mean:.3} ms  median {:.3}  p99 {:.3}  max {:.3}   ({} nodes, {} quads, {} batches)",
            samples[samples.len() / 2],
            samples[samples.len() * 99 / 100],
            samples[samples.len() - 1],
            s.nodes,
            s.quads,
            s.batches,
        );
    };

    println!(
        "2560×1440 at scale {scale}: HUD (12 commands, hero bar, minimap, inventory, resources), tooltip shown, 100-line log, settings and menu"
    );
    run(
        &mut ui,
        "steady (bars, sweeps animate)",
        2000,
        &mut |ui, f| {
            let t = f as f32 / 600.0;
            sample::set_vitals(ui, 300 + (f % 300) as u32, 600, 90, 200);
            ui.set_value(cooldown, 1.0 - t.fract());
            ui.set_value(xp, (t * 3.0).fract());
        },
    );
    run(
        &mut ui,
        "busy (+texts change, hover moves)",
        2000,
        &mut |ui, f| {
            let t = f as f32 / 600.0;
            sample::set_vitals(ui, 300 + (f % 300) as u32, 600, 90 + (f % 100) as u32, 200);
            ui.set_value(cooldown, 1.0 - t.fract());
            ui.set_text(seconds, &format!("{}", 10 - (f / 60) % 10));
            ui.set_text(amber, &format!("{}", 1000 + f));
            ui.set_text(
                lines[f % 100],
                &format!("[b]{f}[/b] a new line of chat arrives"),
            );
            if f % 10 == 0 {
                let (x, y) = centers[(f / 10) % 12];
                ui.input(&InputEvent::PointerMoved { x, y });
            }
        },
    );
    run(
        &mut ui,
        "worst (all 100 log lines change)",
        300,
        &mut |ui, f| {
            for (i, l) in lines.iter().enumerate() {
                ui.set_text(
                    *l,
                    &format!("line {i} of frame {f}: [color=#ff8a65]changed[/color]"),
                );
            }
        },
    );
}
