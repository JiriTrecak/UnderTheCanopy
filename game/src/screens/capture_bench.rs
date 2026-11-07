//! The editor's benchmark (`canopy capture-screens --only editor-bench`): every workbench
//! opened and used as a person would (idle, switching, opening, typing, panning the graph,
//! the stage and the lab running, brushes on a large map, the check after an edit,
//! saving), on a copy of the base package, each frame timed from the interface built to
//! the GPU idle. It prints a table per scenario (frames, CPU mean, p95 and worst, GPU mean
//! and worst) with the costliest spans, and writes it to `<out>/editor-bench.md`. Content
//! is picked from the package itself (the largest effect graph and lab scenario, the
//! first ability with a stage), so no id is named here.

use super::ui::{Sample, Ui, scratch_package};
use super::*;
use crate::telemetry;
use canopy_editor::{Editor, KINDS, Kind};
use serde_json::{Value, json};
use std::fmt::Write as _;

/// One scenario's measured frames and spans.
struct Row {
    name: String,
    samples: Vec<Sample>,
    spans: std::collections::BTreeMap<&'static str, telemetry::SpanTime>,
    /// Wall time of the whole scenario (frames and the driver's pauses).
    wall: Duration,
}

struct Bench<'a> {
    ui: Ui<'a>,
    rows: Vec<Row>,
    started: Instant,
    name: String,
}

impl<'a> Bench<'a> {
    fn begin(&mut self, name: &str) {
        println!("— {name}");
        self.name = name.to_string();
        self.ui.timing = Some(Vec::new());
        let _ = telemetry::span_times();
        self.started = Instant::now();
    }

    fn end(&mut self) {
        let samples = self.ui.timing.take().unwrap_or_default();
        self.rows.push(Row {
            name: std::mem::take(&mut self.name),
            samples,
            spans: telemetry::span_times(),
            wall: self.started.elapsed(),
        });
    }

    /// `n` idle frames, measured as `name`.
    fn idle(&mut self, name: &str, n: usize) {
        self.begin(name);
        self.ui.frames(n);
        self.end();
    }

    /// `n` frames that only draw the viewports again (the window between the interface's
    /// frames), measured as `name`.
    fn again(&mut self, name: &str, n: usize) {
        self.begin(name);
        for _ in 0..n {
            let start = Instant::now();
            let gpu = &self.ui.rig.gpu;
            self.ui.editor.redraw_viewports(gpu);
            let mut encoder = gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            self.ui
                .editor
                .paint(&mut encoder, &self.ui.rig.target.ui_view);
            gpu.queue.submit([encoder.finish()]);
            let submitted = Instant::now();
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            if let Some(samples) = &mut self.ui.timing {
                samples.push(Sample {
                    cpu: submitted - start,
                    gpu: submitted.elapsed(),
                    at_once: false,
                });
            }
            std::thread::sleep(Duration::from_millis(12));
        }
        self.end();
    }
}

fn ms(d: Duration) -> f32 {
    d.as_secs_f32() * 1000.0
}

fn table(rows: &[Row]) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "| scenario | frames | next at once | cpu mean | cpu p95 | cpu max | gpu mean | gpu max | wall s | costliest spans (total ms / max ms) |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|---|---|---|---|---|");
    for row in rows.iter().filter(|r| !r.samples.is_empty()) {
        let n = row.samples.len().max(1);
        let mut cpu: Vec<f32> = row.samples.iter().map(|s| ms(s.cpu)).collect();
        cpu.sort_by(f32::total_cmp);
        let mean = |v: &[f32]| v.iter().sum::<f32>() / n as f32;
        let gpu: Vec<f32> = row.samples.iter().map(|s| ms(s.gpu)).collect();
        let p95 = cpu
            .get((cpu.len() * 95 / 100).min(cpu.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0.0);
        let mut spans: Vec<_> = row
            .spans
            .iter()
            .filter(|(name, _)| !matches!(**name, "editor_frame" | "editor_paint"))
            .collect();
        spans.sort_by_key(|s| std::cmp::Reverse(s.1.total));
        let spans: Vec<String> = spans
            .iter()
            .take(10)
            .map(|(name, t)| format!("{name} {:.1}/{:.1}", ms(t.total), ms(t.max)))
            .collect();
        let _ = writeln!(
            out,
            "| {} | {} | {}% | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.1} | {} |",
            row.name,
            row.samples.len(),
            100 * row.samples.iter().filter(|s| s.at_once).count() / n,
            mean(&cpu),
            p95,
            cpu.last().copied().unwrap_or(0.0),
            mean(&gpu),
            gpu.iter().copied().fold(0.0, f32::max),
            row.wall.as_secs_f32(),
            spans.join(", ")
        );
    }
    out
}

/// The elements the editor `editor` lists (MCP's list_elements).
fn elements(ui: &mut Ui, editor: &str) -> Vec<Value> {
    match ui
        .editor
        .run_tool("list_elements", json!({ "editor": editor }))
    {
        Ok(Value::Array(list)) => list,
        Ok(value) => value
            .as_object()
            .and_then(|o| o.values().find_map(Value::as_array))
            .cloned()
            .unwrap_or_default(),
        Err(error) => {
            println!("  list_elements {editor}: {error}");
            Vec::new()
        }
    }
}

fn field<'v>(element: &'v Value, name: &str) -> &'v str {
    element.get(name).and_then(Value::as_str).unwrap_or("")
}

/// The element whose document is the largest file (the busiest one to show).
fn largest(base: &Path, list: &[Value], pick: impl Fn(&Value) -> bool) -> Option<Value> {
    list.iter()
        .filter(|e| pick(e))
        .max_by_key(|e| {
            let document = field(e, "document");
            let path = match document.strip_prefix("@labs/") {
                Some(lab) => base.join("..").join("labs").join(lab),
                None => base.join(document),
            };
            std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
        })
        .cloned()
}

pub(super) fn editor_bench(rig: &mut Rig, paths: &Paths) -> Result<()> {
    telemetry::time_spans(true);
    let scratch = paths.user.join("bench-base");
    scratch_package(&paths.data.join("base"), &scratch)?;
    let mut setup = crate::app::editor_setup(paths);
    setup.base = scratch.clone();
    let format = CAPTURE_FORMAT.remove_srgb_suffix();
    let editor = Editor::headless(&rig.gpu, format, setup);
    let out = rig.out.clone();
    let mut bench = Bench {
        ui: Ui::new(rig, editor),
        rows: Vec::new(),
        started: Instant::now(),
        name: String::new(),
    };
    bench.ui.settle();
    let base = paths.data.join("base");
    let b = &mut bench;
    // `CANOPY_BENCH=map,graph` runs only the sections named.
    let wanted = std::env::var("CANOPY_BENCH").ok();
    let on = |section: &str| {
        wanted
            .as_deref()
            .is_none_or(|w| w.split(',').any(|s| s.trim() == section))
    };

    // Idle: nothing changes, frame after frame.
    for kind in KINDS.into_iter().filter(|_| on("idle")) {
        b.ui.editor.select(kind);
        b.ui.editor.open_first(1);
        b.ui.settle();
        b.ui.frames(20);
        b.idle(&format!("idle {kind:?}"), 60);
        if matches!(kind, Kind::Maps | Kind::Units) {
            b.again(&format!("viewports again {kind:?}"), 60);
        }
    }

    // Switching workbenches by the rail.
    b.begin("switch workbenches");
    for _ in 0..if on("switch") { 2 } else { 0 } {
        for kind in KINDS {
            b.ui.click(kind.label());
        }
    }
    b.end();

    // Opening documents: the units one after another.
    let units = elements(&mut b.ui, "units");
    b.ui.editor.select(Kind::Units);
    b.ui.settle();
    b.begin("open 12 units");
    for unit in units.iter().take(if on("open") { 12 } else { 0 }) {
        b.ui.editor.open_document(field(unit, "document"));
        b.ui.frames(3);
    }
    b.end();

    // Typing in a form: a unit's Form, characters into its first text field.
    if let Some(unit) = units.first().filter(|_| on("form")) {
        b.ui.editor.open_document(field(unit, "document"));
        b.ui.settle();
        b.ui.editor.show_sheet("form");
        b.ui.settle();
        let text =
            b.ui.widgets()
                .iter()
                .find(|w| w.role.contains("TextInput") && !w.value.is_empty())
                .cloned();
        if let Some(text) = text {
            b.ui.click_at(text.rect.center(), egui::PointerButton::Primary);
            b.ui.key(egui::Key::End, egui::Modifiers::NONE);
            b.begin("type in a form");
            for c in "abcdefghijkl".chars() {
                b.ui.type_text(&c.to_string());
            }
            b.end();
            b.idle("form after typing (check catching up)", 30);
        } else {
            println!("  no text field in the form");
        }
        b.ui.editor.show_sheet("none");
        b.ui.editor.undo_all();
        b.ui.settle();
    }

    // An edit and the check after it, then saving it.
    if let Some(unit) = units.first().filter(|_| on("check")) {
        let document = field(unit, "document").to_string();
        b.begin("edit + check");
        let started = Instant::now();
        let _ = b.ui.editor.run_step(
            "bench",
            vec![canopy_editor::Command::Set {
                document: document.clone(),
                pointer: "/bench".into(),
                value: json!(1),
                at: None,
            }],
        );
        b.ui.settle();
        println!("  check after an edit: {:.0} ms", ms(started.elapsed()));
        b.end();
        b.begin("save");
        let _ = b.ui.editor.run_tool("save", json!({ "force": true }));
        b.ui.frames(4);
        b.end();
    }

    // The effect graph: its node previews looping, and the canvas panned.
    let effects = elements(&mut b.ui, "effects");
    let graph = largest(&base, &effects, |e| {
        std::fs::read_to_string(base.join(field(e, "document")))
            .is_ok_and(|t| t.contains("\"graph\""))
    });
    if let Some(effect) = graph.as_ref().filter(|_| on("graph")) {
        println!("  graph: {}", field(effect, "document"));
        b.ui.editor.select(Kind::Effects);
        b.ui.editor.open_document(field(effect, "document"));
        b.ui.editor.show_graph();
        b.ui.settle();
        b.ui.frames(40);
        b.idle("effect graph (previews looping)", 90);
        let (w, h) = (
            b.ui.rig.size[0] as f32 / b.ui.rig.scale,
            b.ui.rig.size[1] as f32 / b.ui.rig.scale,
        );
        b.begin("effect graph pan");
        for _ in 0..3 {
            b.ui.drag(
                egui::pos2(w * 0.45, h * 0.8),
                egui::pos2(w * 0.55, h * 0.75),
                egui::PointerButton::Middle,
            );
        }
        b.end();
        b.ui.editor.open_stage(field(effect, "id"));
        b.ui.settle();
        b.ui.editor.stage_cast();
        b.idle("effect stage playing", 90);
    }

    // The ability stage casting.
    let abilities = elements(&mut b.ui, "abilities");
    if let Some(ability) = abilities.first().filter(|_| on("stage")) {
        b.ui.editor.select(Kind::Abilities);
        b.ui.editor.open_stage(field(ability, "id"));
        b.ui.settle();
        b.ui.frames(20);
        b.ui.editor.stage_cast();
        b.idle("ability stage casting", 90);
        // The cast over, the stage stands still.
        b.ui.frames(200);
        b.idle("ability stage paused", 60);
        b.again("ability stage paused viewports again", 60);
    }

    // The Combat Lab running its largest scenario.
    let labs = elements(&mut b.ui, "lab");
    if let Some(lab) = largest(&base, &labs, |_| true).filter(|_| on("lab")) {
        let document = field(&lab, "document").to_string();
        println!("  lab: {document}");
        b.ui.editor.select(Kind::Lab);
        b.ui.editor.open_document(&document);
        b.ui.settle();
        b.ui.frames(10);
        b.idle("lab paused", 60);
        b.again("lab paused viewports again", 60);
        if b.ui.find("Play").is_some() {
            b.ui.click("Play");
            b.idle("lab running", 120);
            b.ui.click("Pause");
        }
    }

    // Campaigns: the steps, and the read-through.
    let campaigns = elements(&mut b.ui, "campaigns");
    if let Some(campaign) = campaigns.first().filter(|_| on("campaign")) {
        let document = field(campaign, "document").to_string();
        b.ui.editor.select(Kind::Campaigns);
        b.ui.editor.open_document(&document);
        b.ui.settle();
        b.idle("campaign steps", 60);
        b.ui.editor.set_campaign_mode(&document, "read-through");
        b.ui.settle();
        b.idle("campaign read-through", 60);
        b.ui.editor.set_campaign_mode(&document, "steps");
    }

    // The assistant beside an editor.
    if on("assistant") {
        b.ui.editor.set_assistant(true);
        b.ui.settle();
        b.idle("assistant open", 60);
        b.ui.editor.set_assistant(false);
    }

    // Maps: a large map, idle, a brush stroke and the world regenerating.
    let created = on("map").then(|| {
        b.ui.editor.run_tool(
            "create_map",
            json!({"id": "bench-large", "name": "Bench", "size": 1024, "folder": "skirmish"}),
        )
    });
    match created.unwrap_or(Err("skipped".into())) {
        Ok(_) => {
            b.ui.editor.select(Kind::Maps);
            b.ui.editor
                .open_document("maps/skirmish/bench-large.utcm/map.json");
            b.ui.settle();
            b.ui.frames(30);
            b.idle("map 1024 idle", 60);
            b.again("map 1024 viewports again", 60);
            let (w, h) = (
                b.ui.rig.size[0] as f32 / b.ui.rig.scale,
                b.ui.rig.size[1] as f32 / b.ui.rig.scale,
            );
            b.ui.editor.set_block_tool("raise");
            b.begin("map 1024 raise strokes");
            for i in 0..3 {
                let y = h * (0.4 + 0.1 * i as f32);
                b.ui.drag(
                    egui::pos2(w * 0.4, y),
                    egui::pos2(w * 0.6, y + 20.0),
                    egui::PointerButton::Primary,
                );
            }
            b.end();
            b.idle("map 1024 after strokes", 60);
            b.ui.editor.set_block_view("split");
            b.ui.settle();
            b.ui.frames(60);
            b.idle("map 1024 split idle", 60);
            b.ui.editor.set_block_view("blocks");
        }
        Err(error) => println!("  create_map: {error}"),
    }

    let table = table(&bench.rows);
    println!("\n{table}");
    // Every span of every scenario, for digging.
    let mut details = String::new();
    for row in bench.rows.iter().filter(|r| !r.samples.is_empty()) {
        let _ = writeln!(details, "\n## {}\n", row.name);
        let mut spans: Vec<_> = row.spans.iter().collect();
        spans.sort_by_key(|s| std::cmp::Reverse(s.1.total));
        for (name, t) in spans {
            let _ = writeln!(
                details,
                "- {name}: {} × total {:.1} ms, max {:.1} ms",
                t.count,
                ms(t.total),
                ms(t.max)
            );
        }
    }
    std::fs::write(out.join("editor-bench.md"), format!("{table}{details}"))?;
    telemetry::time_spans(false);
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(())
}
