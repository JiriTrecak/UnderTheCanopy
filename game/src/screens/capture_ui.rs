//! The editor used by hand (`canopy capture-screens --only editor-ui`): flows through
//! every workbench driven as a person would, by clicks, typing, keys and drags on the
//! widgets the editor lays out (found by their labels in egui's accessibility tree), each
//! checked for what it should do. It works on a copy of the base package (the asset
//! folders linked, never written), so saving and deleting are tried for real. A failed
//! check is written down with a picture of the moment and the flows go on; the group
//! fails at the end if any check did.

use super::*;
use canopy_editor::{Editor, Widget};

/// Roles a click can act on.
const CLICKABLE: [&str; 7] = [
    "Button",
    "Link",
    "CheckBox",
    "RadioButton",
    "Tab",
    "ToggleButton",
    "ComboBox",
];

pub(super) struct Ui<'a> {
    pub editor: Editor,
    /// The last Test in game the editor asked the game for (map, encounter, campaign).
    pub test: Option<canopy_editor::Test>,
    pub rig: &'a mut Rig,
    pub failures: Vec<String>,
    passed: usize,
    flow: String,
}

impl<'a> Ui<'a> {
    pub fn new(rig: &'a mut Rig, editor: Editor) -> Self {
        let mut ui = Self {
            editor,
            test: None,
            rig,
            failures: Vec::new(),
            passed: 0,
            flow: String::new(),
        };
        ui.editor.record_widgets();
        ui.frames(4);
        ui
    }

    pub fn flow(&mut self, name: &str) {
        println!("— {name}");
        self.flow = name.to_string();
    }

    pub fn send(&mut self, events: Vec<egui::Event>) {
        let output =
            self.editor
                .run_headless_with(self.rig.size, self.rig.scale, &self.rig.gpu, events);
        if output.test.is_some() {
            self.test = output.test;
        }
        std::thread::sleep(Duration::from_millis(12));
    }

    pub fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.send(Vec::new());
        }
    }

    /// Frames until the check of the latest edits is in (and the layout has settled).
    pub fn settle(&mut self) {
        let start = Instant::now();
        let mut n = 0;
        while n < 4 || (self.editor.checking() && start.elapsed() < Duration::from_secs(20)) {
            self.frames(1);
            n += 1;
        }
    }

    pub fn widgets(&self) -> &[Widget] {
        self.editor.widgets()
    }

    /// The widget a person would mean by `label`: a clickable one named exactly that,
    /// else one whose name starts with it.
    pub fn find(&self, label: &str) -> Option<Widget> {
        let clickable = |w: &&Widget| CLICKABLE.contains(&w.role.as_str());
        let named = |w: &&Widget| w.label == label || w.value == label;
        self.widgets()
            .iter()
            .filter(clickable)
            .find(named)
            .or_else(|| self.widgets().iter().find(named))
            .or_else(|| {
                self.widgets()
                    .iter()
                    .filter(clickable)
                    .find(|w| w.label.starts_with(label))
            })
            .cloned()
    }

    /// Whether any widget shows `text` (its label or value).
    pub fn shows(&self, text: &str) -> bool {
        self.widgets().iter().any(|w| {
            // Headings are drawn in capitals: compare without case.
            let text = text.to_lowercase();
            [&w.label, &w.value, &w.placeholder]
                .iter()
                .any(|t| t.to_lowercase().contains(&text))
        })
    }

    pub fn click_at(&mut self, pos: egui::Pos2, button: egui::PointerButton) {
        let press = |pressed| egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.send(vec![egui::Event::PointerMoved(pos)]);
        self.send(vec![press(true)]);
        self.send(vec![press(false)]);
        self.frames(2);
    }

    /// Clicks the widget named `label`; whether there was one (a failed check if not).
    pub fn click(&mut self, label: &str) -> bool {
        match self.find(label) {
            Some(w) if w.enabled => {
                self.click_at(w.rect.center(), egui::PointerButton::Primary);
                true
            }
            Some(_) => {
                self.fail(&format!("“{label}” is disabled"));
                false
            }
            None => {
                self.fail(&format!("no “{label}” to click"));
                false
            }
        }
    }

    /// Clicks the text field whose placeholder is `placeholder` (or the `n`th text field)
    /// and types `text` into it.
    pub fn type_into(&mut self, placeholder: &str, text: &str) -> bool {
        let field = self
            .widgets()
            .iter()
            .find(|w| w.role.contains("TextInput") && w.placeholder.contains(placeholder))
            .cloned();
        let Some(field) = field else {
            self.fail(&format!("no field “{placeholder}”"));
            return false;
        };
        self.click_at(field.rect.center(), egui::PointerButton::Primary);
        self.type_text(text);
        true
    }

    /// Replaces the text of the field whose value is `current` with `text`.
    pub fn type_over(&mut self, current: &str, text: &str) -> bool {
        let field = self
            .widgets()
            .iter()
            .find(|w| {
                (w.role.contains("TextInput") || w.role == "SpinButton") && w.value == current
            })
            .cloned();
        let Some(field) = field else {
            self.fail(&format!("no field holding “{current}”"));
            return false;
        };
        self.click_at(field.rect.center(), egui::PointerButton::Primary);
        self.select_all();
        self.type_text(text);
        true
    }


    pub fn escape(&mut self) {
        self.key(egui::Key::Escape, egui::Modifiers::NONE);
    }

    pub fn type_text(&mut self, text: &str) {
        self.send(vec![egui::Event::Text(text.to_string())]);
        self.frames(2);
    }

    pub fn key(&mut self, key: egui::Key, modifiers: egui::Modifiers) {
        let event = |pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        self.send(vec![event(true)]);
        self.send(vec![event(false)]);
        self.frames(2);
    }

    /// Selects everything in the focused field (to type over it).
    pub fn select_all(&mut self) {
        self.key(egui::Key::A, egui::Modifiers::COMMAND);
    }

    pub fn drag(&mut self, from: egui::Pos2, to: egui::Pos2, button: egui::PointerButton) {
        let press = |pos, pressed| egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.send(vec![egui::Event::PointerMoved(from)]);
        self.send(vec![press(from, true)]);
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            self.send(vec![egui::Event::PointerMoved(from + (to - from) * t)]);
        }
        self.send(vec![press(to, false)]);
        self.frames(2);
    }


    pub fn check(&mut self, ok: bool, what: &str) -> bool {
        if ok {
            self.passed += 1;
        } else {
            self.fail(what);
        }
        ok
    }

    fn fail(&mut self, what: &str) {
        let n = self.failures.len() + 1;
        let line = format!("{}: {what}", self.flow);
        println!("   FAILED {line}");
        self.failures.push(line);
        let name = format!(
            "editor-ui-fail-{n:02}-{}",
            self.flow
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect::<String>()
        );
        let _ = self.shoot(&name);
        let _ = std::fs::write(self.rig.out.join(format!("{name}.txt")), self.dump());
    }

    pub fn shoot(&mut self, name: &str) -> Result<()> {
        let mut encoder = self
            .rig
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        self.editor.paint(&mut encoder, &self.rig.target.ui_view);
        self.rig.gpu.queue.submit([encoder.finish()]);
        self.rig.save(name)
    }

    /// Every widget now, one per line (to read what a screen offers).
    pub fn dump(&self) -> String {
        self.widgets()
            .iter()
            .filter(|w| !w.label.is_empty() || !w.value.is_empty() || !w.placeholder.is_empty())
            .map(|w| {
                format!(
                    "{:<16} {:>5.0},{:<5.0} {}{}{}{}",
                    w.role,
                    w.rect.center().x,
                    w.rect.center().y,
                    w.label,
                    if w.value.is_empty() {
                        String::new()
                    } else {
                        format!(" = {}", w.value.chars().take(60).collect::<String>())
                    },
                    if w.placeholder.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", w.placeholder)
                    },
                    if w.enabled { "" } else { " (disabled)" },
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn summary(&self) -> String {
        format!(
            "{} checks passed, {} failed",
            self.passed,
            self.failures.len()
        )
    }
}

/// A scratch copy of the base package: its documents copied, its asset folders linked
/// (read, never written by the flows).
fn scratch_package(base: &Path, to: &Path) -> Result<()> {
    let _ = std::fs::remove_dir_all(to);
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(base)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_name() == "assets" {
            #[cfg(unix)]
            std::os::unix::fs::symlink(entry.path(), &target)?;
            #[cfg(windows)]
            std::os::windows::fs::symlink_dir(entry.path(), &target)?;
            continue;
        }
        copy_tree(&entry.path(), &target)?;
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else {
        std::fs::copy(from, to)?;
    }
    Ok(())
}

pub(super) fn editor_ui(rig: &mut Rig, paths: &Paths) -> Result<()> {
    let scratch = paths.user.join("ui-base");
    scratch_package(&paths.data.join("base"), &scratch)?;
    let mut setup = crate::app::editor_setup(paths);
    setup.base = scratch.clone();
    let format = CAPTURE_FORMAT.remove_srgb_suffix();
    let editor = Editor::headless(&rig.gpu, format, setup);
    let mut ui = Ui::new(rig, editor);
    let mut report = String::new();
    for kind in canopy_editor::KINDS {
        ui.editor.select(kind);
        ui.editor.open_first(1);
        ui.settle();
        report.push_str(&format!("===== {kind:?}\n{}\n\n", ui.dump()));
    }
    std::fs::write(ui.rig.out.join("editor-ui-widgets.txt"), &report)?;
    shell_flows(&mut ui, &scratch);
    map_flows(&mut ui);
    workbench_flows(&mut ui);
    leaving_flows(&mut ui, &scratch);
    println!("{}", ui.summary());
    let failures = std::mem::take(&mut ui.failures);
    anyhow::ensure!(
        failures.is_empty(),
        "the editor used by hand:\n{}",
        failures.join("\n")
    );
    Ok(())
}

/// The shell: the rail, a library's search, an element's rename, delete and create, its
/// Form and JSON, undo, saving, the file watcher and conflicts, the leave and save
/// prompts, links and Back, the problems list and the MCP panel.
fn shell_flows(ui: &mut Ui, scratch: &Path) {
    use egui::{Key, Modifiers};
    ui.flow("rail and search");
    ui.click("Units");
    ui.settle();
    ui.check(
        ui.shows("Units and heroes"),
        "the rail opens the Units editor",
    );
    ui.type_into("Search units", "Raider");
    ui.settle();
    ui.check(ui.find("Raider").is_some(), "the search finds the Raider");
    ui.check(
        ui.find("Needler").is_none(),
        "the search hides what does not match",
    );
    ui.click("Raider");
    ui.settle();
    ui.check(
        ui.shows("unit.ants.warrior"),
        "a click opens the element (its id in the inspector)",
    );

    ui.flow("rename id");
    ui.click("Rename id…");
    ui.settle();
    ui.check(ui.shows("Rename id"), "the rename dialog opens");
    ui.type_over("unit.ants.warrior", "unit.ants.warrior-ui");
    ui.click("Rename");
    ui.settle();
    ui.check(ui.shows("unit.ants.warrior-ui"), "the id is renamed");
    ui.check(
        ui.find("Undo").is_some_and(|w| w.enabled),
        "undo is offered",
    );
    ui.click("Undo");
    ui.settle();
    ui.check(
        ui.shows("unit.ants.warrior") && !ui.shows("unit.ants.warrior-ui"),
        "undo takes the rename back",
    );

    ui.flow("delete refused while used");
    ui.click("Delete…");
    ui.settle();
    ui.check(
        ui.shows("Remove these uses first"),
        "deleting a used element is refused with its uses",
    );
    ui.click("Close");
    ui.settle();

    ui.flow("create an element");
    ui.click("Plus");
    ui.settle();
    ui.check(ui.shows("New in"), "the create dialog opens");
    let id_field = ui
        .widgets()
        .iter()
        .find(|w| w.role.contains("TextInput") && w.placeholder.contains("unit.ants.scout"))
        .cloned();
    if let Some(field) = id_field {
        ui.click_at(field.rect.center(), egui::PointerButton::Primary);
        ui.select_all();
        ui.type_text("unit.ants.ui-test");
    } else {
        ui.check(false, "the create dialog has an id field");
    }
    ui.click("Create");
    ui.settle();
    ui.check(ui.shows("unit.ants.ui-test"), "the new element opens");
    ui.click("Undo");
    ui.settle();
    ui.check(
        !ui.shows("unit.ants.ui-test"),
        "undo takes the new element back",
    );

    ui.flow("form");
    ui.escape();
    ui.click("Raider");
    ui.settle();
    ui.click("Form");
    ui.settle();
    let _ = std::fs::write(ui.rig.out.join("dump-form.txt"), ui.dump());
    ui.check(ui.shows("Max hp"), "the form shows the definition's fields");
    let hp = ui
        .widgets()
        .iter()
        .find(|w| w.role == "SpinButton" && w.value == "185")
        .cloned();
    match hp {
        Some(field) => {
            ui.click_at(field.rect.center(), egui::PointerButton::Primary);
            ui.select_all();
            ui.type_text("204");
            ui.key(Key::Enter, Modifiers::NONE);
            ui.settle();
            ui.check(
                ui.shows("1 unsaved"),
                "a form edit marks the document unsaved",
            );
        }
        None => {
            ui.check(false, "the form has the health field (185)");
        }
    }
    ui.escape();
    ui.settle();
    ui.check(ui.shows("204"), "the overview shows the new health");
    ui.click("Undo");
    ui.settle();
    ui.check(!ui.shows("1 unsaved"), "undo leaves nothing unsaved");

    ui.flow("json");
    ui.click("JSON");
    ui.settle();
    let json = ui
        .widgets()
        .iter()
        .find(|w| w.role == "MultilineTextInput" && w.value.starts_with('{'))
        .cloned();
    match json {
        Some(field) => {
            ui.click_at(field.rect.center(), egui::PointerButton::Primary);
            ui.select_all();
            ui.type_text("{ \"id\": ");
            ui.click("Apply");
            ui.settle();
            ui.check(
                ui.find("Revert").is_some(),
                "broken JSON is not applied and can be reverted",
            );
            ui.click("Revert");
            ui.settle();
        }
        None => {
            ui.check(false, "the JSON sheet shows the document");
        }
    }
    ui.escape();
    ui.settle();
    ui.check(!ui.shows("unsaved"), "nothing was applied");

    ui.flow("save, the watcher and conflicts");
    let file = scratch.join("units/ants/warrior.json");
    let before = std::fs::read_to_string(&file).unwrap_or_default();
    ui.click("Form");
    ui.settle();
    if ui.type_over("185", "190") {
        ui.key(Key::Enter, Modifiers::NONE);
    }
    ui.escape();
    ui.settle();
    ui.key(Key::S, Modifiers::COMMAND);
    ui.settle();
    let after = std::fs::read_to_string(&file).unwrap_or_default();
    ui.check(
        after.contains("190") && after != before,
        "Cmd+S writes the change to the file",
    );
    ui.check(
        after.ends_with("}\n") && after.contains("\n  \"id\""),
        "the file stays in the file format",
    );
    ui.check(!ui.shows("unsaved"), "saving clears the unsaved mark");
    // Changed on disk with no unsaved edits: the editor follows quietly.
    let _ = std::fs::write(&file, after.replace("190", "191"));
    std::thread::sleep(Duration::from_millis(400));
    ui.settle();
    ui.frames(10);
    ui.check(ui.shows("191"), "a file changed on disk reloads");
    // Changed on disk under unsaved edits: the editor asks.
    ui.click("Form");
    ui.settle();
    if ui.type_over("191", "192") {
        ui.key(Key::Enter, Modifiers::NONE);
    }
    ui.escape();
    ui.settle();
    let now = std::fs::read_to_string(&file).unwrap_or_default();
    let _ = std::fs::write(&file, now.replace("191", "193"));
    std::thread::sleep(Duration::from_millis(400));
    ui.frames(10);
    ui.check(
        ui.shows("Changed on disk"),
        "a conflict asks which version to keep",
    );
    ui.click("Take the file's version");
    ui.settle();
    ui.check(ui.shows("193"), "taking the file's version shows it");

    ui.flow("save with problems and leaving");
    ui.click("JSON");
    ui.settle();
    let json = ui
        .widgets()
        .iter()
        .find(|w| w.role == "MultilineTextInput" && w.value.starts_with('{'))
        .cloned();
    if let Some(field) = json {
        let broken = field
            .value
            .replacen("\"maxHp\": 193", "\"maxHp\": \"lots\"", 1);
        ui.click_at(field.rect.center(), egui::PointerButton::Primary);
        ui.select_all();
        ui.type_text(&broken);
        ui.click("Apply");
    }
    ui.escape();
    ui.settle();
    ui.check(ui.shows("1 problem"), "a broken edit shows as a problem");
    ui.key(Key::S, Modifiers::COMMAND);
    ui.settle();
    ui.check(
        ui.shows("The package has problems"),
        "saving with problems asks first",
    );
    ui.click("Cancel");
    ui.settle();
    ui.check(
        std::fs::read_to_string(&file)
            .unwrap_or_default()
            .contains("193"),
        "cancelling saves nothing",
    );
    ui.click("1 problem");
    ui.settle();
    ui.check(
        ui.shows("expected u32") || ui.shows("invalid type"),
        "the problems list says what is wrong",
    );
    ui.click("Exit");
    ui.settle();
    ui.check(
        ui.shows("Unsaved changes"),
        "leaving with unsaved changes asks first",
    );
    ui.click("Cancel");
    ui.settle();
    ui.click("Undo");
    ui.settle();
    ui.check(ui.shows("No problems"), "undo clears the problem");

    ui.flow("links and back");
    ui.click("Frenzy");
    ui.settle();
    ui.check(
        ui.shows("Abilities"),
        "a link opens the element in its own editor",
    );
    ui.click("ChevronLeft");
    ui.settle();
    ui.check(
        ui.shows("Units and heroes"),
        "Back returns to where the link was followed",
    );

    ui.flow("mcp panel");
    ui.click("MCP off");
    ui.settle();
    ui.check(
        ui.shows("MCP server for agents"),
        "the MCP chip opens its panel",
    );
    ui.check(
        ui.shows("claude mcp add"),
        "the panel shows the command for Claude Code",
    );
    ui.click("Close");
    ui.settle();
    ui.check(!ui.shows("MCP server for agents"), "the panel closes");
}

/// The header's unsaved count (0 when nothing is unsaved).
fn unsaved(ui: &Ui) -> usize {
    ui.widgets()
        .iter()
        .find_map(|w| {
            [&w.label, &w.value]
                .into_iter()
                .find_map(|t| t.strip_suffix(" unsaved").and_then(|n| n.parse().ok()))
        })
        .unwrap_or(0)
}

/// Undoes until nothing is unsaved (at most `n` steps).
fn undo_all(ui: &mut Ui, n: usize) {
    for _ in 0..n {
        if unsaved(ui) == 0 {
            break;
        }
        ui.key(egui::Key::Z, egui::Modifiers::COMMAND);
        ui.settle();
    }
}

/// The block editor: its tools by click and key, the views, placing, selecting and
/// deleting, the map's settings, and a new map.
fn map_flows(ui: &mut Ui) {
    use egui::{Key, Modifiers, PointerButton, pos2};
    ui.flow("open a map");
    ui.click("Maps");
    ui.settle();
    ui.click("skirmish");
    ui.settle();
    ui.type_into("Search maps", "Frozen");
    ui.settle();
    ui.click("Frozen Dream");
    ui.settle();
    ui.frames(20);
    ui.check(ui.shows("Frozen Dream"), "the map opens");
    let view = pos2(690.0, 560.0);

    ui.flow("block tools");
    ui.click("Raise");
    ui.check(
        ui.find("Raise").is_some_and(|w| w.on),
        "a click picks the tool",
    );
    ui.drag(view, view + egui::vec2(60.0, 40.0), PointerButton::Primary);
    ui.settle();
    ui.check(unsaved(ui) > 0, "a raise drag edits the blocks");
    undo_all(ui, 5);
    ui.check(unsaved(ui) == 0, "undo takes the raise back");
    for (key, tool) in [
        (Key::W, "Level"),
        (Key::E, "Paint"),
        (Key::R, "Ramp"),
        (Key::T, "Cut"),
        (Key::O, "Round"),
        (Key::C, "Cells"),
        (Key::F, "Face"),
        (Key::P, "Place"),
        (Key::Q, "Raise"),
    ] {
        ui.send(vec![egui::Event::PointerMoved(view)]);
        ui.key(key, Modifiers::NONE);
        ui.check(
            ui.find(tool).is_some_and(|w| w.on),
            &format!("{key:?} picks {tool}"),
        );
    }
    ui.click("Paint");
    // Another kind than the ground's (painting a kind over itself changes nothing).
    ui.send(vec![egui::Event::PointerMoved(view)]);
    ui.key(Key::Num4, Modifiers::NONE);
    ui.drag(view, view + egui::vec2(40.0, 40.0), PointerButton::Primary);
    ui.settle();
    ui.check(unsaved(ui) > 0, "a paint drag edits the blocks");
    undo_all(ui, 5);

    ui.flow("views");
    for (view_name, key) in [
        ("Generated", Key::G),
        ("Pathing", Key::H),
        ("Split", Key::V),
    ] {
        ui.click(view_name);
        ui.frames(10);
        ui.check(
            ui.find(view_name).is_some_and(|w| w.on),
            &format!("{view_name} shows"),
        );
        ui.send(vec![egui::Event::PointerMoved(view)]);
        ui.key(key, Modifiers::NONE);
        ui.frames(4);
    }
    ui.click("Blocks");
    ui.frames(6);
    ui.check(
        ui.find("Blocks").is_some_and(|w| w.on),
        "back to the blocks",
    );

    ui.flow("place");
    ui.click("Place");
    ui.settle();
    let picker = ui
        .widgets()
        .iter()
        .find(|w| {
            w.role == "Button"
                && w.rect.top() > 900.0
                && w.rect.left() > 700.0
                && w.rect.left() < 900.0
                && w.label.contains("Start")
        })
        .cloned();
    if let Some(picker) = picker {
        ui.click_at(picker.rect.center(), PointerButton::Primary);
        ui.settle();
        ui.check(
            ui.shows("Search everything"),
            "the picker opens its catalog",
        );
        ui.click("Buildings");
        ui.click("Ants");
        let spire = ui.find("Acid Spire");
        ui.check(spire.is_some(), "the catalog lists a race's buildings");
        ui.click("Acid Spire");
        ui.settle();
        ui.escape();
    } else {
        ui.check(false, "Place shows what it puts down");
    }
    let spot = pos2(560.0, 700.0);
    ui.send(vec![egui::Event::PointerMoved(spot)]);
    ui.frames(4);
    ui.click_at(spot, PointerButton::Primary);
    ui.settle();
    let placed = unsaved(ui) > 0;
    ui.check(
        placed || ui.shows("may not stand") || ui.shows("cannot"),
        "a click places it, or says why not",
    );
    undo_all(ui, 3);

    ui.flow("select and delete");
    ui.click("Select");
    ui.drag(
        pos2(300.0, 300.0),
        pos2(1100.0, 900.0),
        PointerButton::Primary,
    );
    ui.settle();
    let selected = ui.shows("selected") || ui.shows("Delete");
    ui.check(selected, "a box selects what lies in it");
    ui.send(vec![egui::Event::PointerMoved(view)]);
    ui.key(Key::Delete, Modifiers::NONE);
    ui.settle();
    ui.check(unsaved(ui) > 0, "Delete removes the selection");
    undo_all(ui, 3);
    ui.check(unsaved(ui) == 0, "undo brings it back");
    ui.escape();

    ui.flow("map settings");
    ui.click("Snow");
    ui.settle();
    ui.check(unsaved(ui) > 0, "the weather is an edit");
    ui.check(
        ui.find("Snow").is_some_and(|w| w.on),
        "the weather shows chosen",
    );
    ui.click("Dusk");
    ui.settle();
    ui.click("Small");
    ui.settle();
    ui.check(
        ui.shows("(now 256)"),
        "a size preset shows the new size before it applies",
    );
    ui.click("Apply");
    ui.settle();
    ui.frames(10);
    ui.check(
        ui.shows("160 cells") && !ui.shows("(now"),
        "Apply resizes the map",
    );
    undo_all(ui, 6);
    ui.check(ui.shows("256 cells"), "undo restores the size");

    ui.flow("new map");
    ui.click("Plus");
    ui.settle();
    ui.check(ui.shows("New map"), "the new map dialog opens");
    if ui.type_into("amber-falls", "ui-test-map") {
        ui.click("Create");
        ui.settle();
        ui.frames(10);
        ui.check(
            ui.shows("ui-test-map") || ui.shows("Ui Test Map"),
            "the new map opens",
        );
        undo_all(ui, 3);
        ui.check(
            !ui.shows("ui-test-map"),
            "undo takes the new map away, its tab too",
        );
    }
}

/// The tick the stage or the lab shows (`… · 18 t / 7.00 s`, `Tick 160 …`).
fn shown_tick(ui: &Ui) -> Option<u32> {
    ui.widgets().iter().find_map(|w| {
        let text = if w.value.is_empty() {
            &w.label
        } else {
            &w.value
        };
        if let Some(rest) = text
            .split(" t / ")
            .next()
            .filter(|_| text.contains(" t / "))
        {
            return rest.rsplit(' ').next()?.parse().ok();
        }
        text.strip_prefix("Tick ")?
            .split(|c: char| !c.is_ascii_digit())
            .next()?
            .parse()
            .ok()
    })
}

/// Campaigns, the ability stage, the Combat Lab, assets, the other overviews and the
/// assistant's sidebar.
fn workbench_flows(ui: &mut Ui) {
    use egui::{Key, Modifiers, PointerButton, pos2};
    ui.flow("campaign steps");
    ui.click("Campaigns");
    ui.settle();
    ui.click("The Ant Vanguard");
    ui.settle();
    ui.check(ui.shows("The Ant Vanguard"), "the campaign opens");
    ui.check(
        ui.find("Step 0: Prologue").is_some(),
        "its steps are listed by their titles",
    );
    ui.click("+ Story");
    ui.settle();
    ui.check(unsaved(ui) > 0, "+ Story adds a step");
    ui.click("Step 2: The Hollow Brood");
    ui.settle();
    ui.check(
        ui.shows("Hero level cap") || ui.shows("Map"),
        "selecting a mission shows its fields",
    );
    undo_all(ui, 4);
    ui.check(unsaved(ui) == 0, "undo takes the new step back");

    ui.flow("ability stage");
    ui.click("Abilities");
    ui.settle();
    ui.type_into("Search abilities", "Volatile");
    ui.settle();
    ui.click("Volatile Draught");
    ui.settle();
    ui.click("Stage");
    ui.settle();
    ui.frames(20);
    ui.check(
        ui.find("Cast  Q").is_some(),
        "the stage shows its transport",
    );
    ui.click("Cast  Q");
    ui.frames(10);
    // A unit spell waits for a click in the viewport; Q again aims at the first target.
    ui.send(vec![egui::Event::PointerMoved(pos2(800.0, 400.0))]);
    ui.key(Key::Q, Modifiers::NONE);
    ui.frames(30);
    let tick = shown_tick(ui).unwrap_or(0);
    ui.check(tick > 0, "a cast plays");
    ui.click("Reset  R");
    ui.frames(6);
    ui.check(
        shown_tick(ui).unwrap_or(1) == 0,
        "Reset starts the take again",
    );
    ui.click("Overview");
    ui.settle();

    ui.flow("combat lab");
    ui.click("Lab");
    ui.settle();
    let _ = ui.shoot("editor-ui-lab-library");
    ui.click("One through a hundred");
    ui.settle();
    ui.frames(10);
    ui.check(ui.find("Play").is_some(), "a scenario opens with its run");
    ui.click("Play");
    ui.frames(40);
    let tick = shown_tick(ui).unwrap_or(0);
    ui.check(tick > 0, "Play runs the scenario");
    ui.click("Pause");
    ui.click("Step");
    ui.frames(3);
    ui.check(shown_tick(ui).unwrap_or(0) > tick, "Step runs a tick");
    ui.click("Restart");
    ui.frames(4);
    ui.check(
        shown_tick(ui).unwrap_or(1) == 0,
        "Restart goes back to the start",
    );
    ui.click("Close cells C");
    ui.drag(
        pos2(500.0, 300.0),
        pos2(560.0, 340.0),
        PointerButton::Primary,
    );
    ui.settle();
    ui.check(unsaved(ui) > 0, "closing cells edits the scenario");
    undo_all(ui, 4);
    ui.check(unsaved(ui) == 0, "undo opens them again");

    ui.flow("assets");
    ui.click("Assets");
    ui.settle();
    ui.click("Only assets nothing names");
    ui.settle();
    ui.check(
        ui.find("Only assets nothing names").is_some_and(|w| w.on),
        "the filter turns on",
    );
    ui.click("Only assets nothing names");
    ui.settle();
    ui.type_into("Search assets", "Raider");
    ui.settle();
    ui.click("Raider");
    ui.settle();
    ui.frames(10);
    ui.check(ui.shows("geometry.glb"), "an asset shows its files");
    ui.click("walk");
    ui.frames(4);
    ui.check(
        ui.find("walk").is_some_and(|w| w.on),
        "a clip plays in the preview",
    );

    ui.flow("overviews");
    for (rail, search, element, shown) in [
        (
            "Buildings",
            "Search items and buildings",
            "Soldier Burrow",
            "Trains",
        ),
        (
            "Effects",
            "Search effects",
            "Amber disruption shard",
            "Used by",
        ),
        ("Races", "Search races and rules", "Ants", "Heroes"),
    ] {
        ui.click(rail);
        ui.settle();
        ui.type_into(search, element);
        ui.settle();
        ui.click(element);
        ui.settle();
        ui.check(
            ui.shows(shown),
            &format!("{element}'s overview shows {shown}"),
        );
    }

    ui.flow("assistant");
    ui.click("Robot");
    ui.settle();
    ui.check(
        ui.shows("Ask, and it works in the editor"),
        "the assistant opens",
    );
    ui.type_into("Add an API key", "Make the Raider tougher");
    ui.key(Key::Enter, Modifiers::NONE);
    ui.settle();
    ui.check(
        ui.shows("API key in the settings") && ui.shows("Provider"),
        "without a key the settings open and say why",
    );
    ui.click("OpenAI");
    ui.settle();
    ui.check(
        ui.shows("OpenAI API key") || ui.shows("OPENAI API KEY"),
        "the provider switches",
    );
    ui.click("Anthropic");
    ui.click("Threads");
    ui.settle();
    ui.click("Close");
    ui.settle();
    ui.check(!ui.shows("Ask, and it works"), "the assistant closes");
}

/// What leaves the editor (Test in game, Try it, Play from a step) and what reaches the
/// package's files (a rename's uses, a deletion), saved and read back from disk.
fn leaving_flows(ui: &mut Ui, scratch: &Path) {
    use egui::{Key, Modifiers};
    ui.flow("test in game");
    ui.click("Maps");
    ui.settle();
    ui.test = None;
    ui.click("Test in game");
    ui.frames(4);
    ui.check(
        ui.test.as_ref().is_some_and(|t| t.map.is_some()),
        "Test in game hands the game the open map",
    );

    ui.flow("try an ability");
    ui.click("Abilities");
    ui.settle();
    ui.test = None;
    ui.click("Try it");
    ui.frames(4);
    ui.check(
        ui.test.as_ref().is_some_and(|t| t.encounter.is_some()),
        "Try it plays the unit's encounter",
    );

    ui.flow("play a campaign step");
    ui.click("Campaigns");
    ui.settle();
    ui.test = None;
    ui.click("Play");
    ui.frames(4);
    ui.check(
        ui.test.as_ref().is_some_and(|t| t.campaign.is_some()),
        "Play runs the campaign's step",
    );

    ui.flow("rename, saved");
    ui.click("Units");
    ui.settle();
    ui.type_into("Search units", "Raider");
    ui.settle();
    ui.click("Raider");
    ui.settle();
    ui.click("Rename id…");
    ui.settle();
    ui.type_over("unit.ants.warrior", "unit.ants.warrior-ui");
    ui.click("Rename");
    ui.settle();
    ui.key(Key::S, Modifiers::COMMAND);
    ui.settle();
    let uses = count_in(scratch, "unit.ants.warrior-ui");
    ui.check(uses > 3, "a saved rename rewrote its uses in their files");
    ui.check(
        count_in(scratch, "\"unit.ants.warrior\"") == 0,
        "no file still names the old id",
    );
    ui.click("Rename id…");
    ui.settle();
    ui.type_over("unit.ants.warrior-ui", "unit.ants.warrior");
    ui.click("Rename");
    ui.settle();
    ui.key(Key::S, Modifiers::COMMAND);
    ui.settle();
    ui.check(
        count_in(scratch, "unit.ants.warrior-ui") == 0,
        "renaming back restores every file",
    );

    ui.flow("delete, saved");
    ui.click("Plus");
    ui.settle();
    if let Some(field) = ui
        .widgets()
        .iter()
        .find(|w| w.role.contains("TextInput") && w.placeholder.contains("unit.ants.scout"))
        .cloned()
    {
        ui.click_at(field.rect.center(), egui::PointerButton::Primary);
        ui.select_all();
        ui.type_text("unit.ants.ui-scratch");
    }
    ui.click("Create");
    ui.settle();
    let file = scratch.join("units/ants/ui-scratch.json");
    ui.key(Key::S, Modifiers::COMMAND);
    ui.settle();
    if ui.shows("The package has problems") {
        // A new element has no text yet: save it anyway, as the dialog offers.
        ui.click("Save anyway");
        ui.settle();
    }
    ui.check(file.exists(), "a new element is written on save");
    ui.click("Delete…");
    ui.settle();
    ui.check(
        ui.shows("Delete unit.ants.ui-scratch?"),
        "deleting an unused element asks first",
    );
    ui.click("Delete");
    ui.settle();
    ui.check(file.exists(), "a deletion waits for the save");
    ui.key(Key::S, Modifiers::COMMAND);
    ui.settle();
    if ui.shows("The package has problems") {
        ui.click("Save anyway");
        ui.settle();
    }
    ui.check(!file.exists(), "saving removes the deleted element's file");
    ui.check(
        count_in(scratch, "unit.ants.ui-scratch") == 0,
        "its own text library entries went with it",
    );
    ui.check(unsaved(ui) == 0, "and nothing is left unsaved");
}

/// How many times `needle` appears in the package's JSON documents.
fn count_in(root: &Path, needle: &str) -> usize {
    let mut n = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if entry.file_name() == "assets" {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "json")
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                n += text.matches(needle).count();
            }
        }
    }
    n
}
