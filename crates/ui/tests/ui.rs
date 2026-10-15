//! Layout, text, input and data behaviour of the toolkit on test-owned fixtures.

mod common;

use canopy_ui::{
    ActionKind, Color, Cost, ImageDesc, InputEvent, Key, Modifiers, PointerButton, QuadKind, Rect,
    TextureId, Tooltip, Ui,
};
use common::{LAYOUT, THEME, config, fonts, ui};

fn rect(ui: &Ui, path: &str) -> Rect {
    ui.rect(ui.find(path).unwrap_or_else(|| panic!("{path}")))
        .unwrap_or_else(|| panic!("{path} not laid out"))
}

fn center(r: Rect) -> (f32, f32) {
    (r.x + r.w * 0.5, r.y + r.h * 0.5)
}

fn move_to(ui: &mut Ui, (x, y): (f32, f32)) -> bool {
    ui.input(&InputEvent::PointerMoved { x, y })
}

fn click(ui: &mut Ui, at: (f32, f32)) -> bool {
    move_to(ui, at);
    let a = ui.input(&InputEvent::PointerButton {
        button: PointerButton::Primary,
        pressed: true,
    });
    let b = ui.input(&InputEvent::PointerButton {
        button: PointerButton::Primary,
        pressed: false,
    });
    a && b
}

fn key(ui: &mut Ui, key: Key) -> bool {
    ui.input(&InputEvent::Key {
        key,
        pressed: true,
        repeat: false,
    })
}

// --- Layout -------------------------------------------------------------------------

#[test]
fn grid_lists_and_absolute_children() {
    let ui = ui();
    // A 4 × 3 grid of 100 × 100 cells at (100, 50).
    assert_eq!(
        rect(&ui, "test/grid/0"),
        Rect::new(100.0, 50.0, 100.0, 100.0)
    );
    assert_eq!(
        rect(&ui, "test/grid/5"),
        Rect::new(200.0, 150.0, 100.0, 100.0)
    );
    assert_eq!(
        rect(&ui, "test/grid/11"),
        Rect::new(400.0, 250.0, 100.0, 100.0)
    );
    // Percentages of absolute children resolve against their parent.
    assert_eq!(
        rect(&ui, "test/grid/5/icon"),
        Rect::new(210.0, 160.0, 80.0, 80.0)
    );
}

#[test]
fn transforms_scale_subtrees_about_their_origin() {
    let ui = ui();
    // 200 × 100 at the bottom left of a 720 px tall screen, scaled by half about its
    // bottom left corner.
    assert_eq!(rect(&ui, "test/scaled"), Rect::new(0.0, 670.0, 100.0, 50.0));
    assert_eq!(
        rect(&ui, "test/scaled/inner"),
        Rect::new(50.0, 670.0, 50.0, 25.0)
    );
}

#[test]
fn display_scale_and_list_resizing() {
    let mut ui = ui();
    ui.set_viewport(2560, 1440, 2.0);
    ui.frame(0.0);
    assert_eq!(
        rect(&ui, "test/grid/0"),
        Rect::new(200.0, 100.0, 200.0, 200.0)
    );

    let grid = ui.find("test/grid").unwrap();
    let last = ui.child(grid, 11).unwrap();
    ui.set_count(grid, 4);
    ui.frame(0.0);
    assert_eq!(ui.rect(last), None, "removed entries vanish");
    ui.set_count(grid, 12);
    ui.frame(0.0);
    assert!(
        ui.rect(last).is_some(),
        "the handle follows the path when it comes back"
    );
    assert_eq!(ui.child(grid, 12), None);
}

#[test]
fn hidden_nodes_are_not_laid_out_or_hit() {
    let mut ui = ui();
    assert_eq!(ui.rect(ui.find("test/hidden").unwrap()), None);
    assert!(!move_to(&mut ui, (10.0, 10.0)), "nothing visible there");
    let hidden = ui.find("test/hidden").unwrap();
    ui.set_visible(hidden, true);
    ui.frame(0.0);
    assert!(move_to(&mut ui, (10.0, 10.0)));
}

// --- Text ---------------------------------------------------------------------------

#[test]
fn labels_size_to_their_text() {
    let mut ui = ui();
    let short = rect(&ui, "test/label");
    assert!(short.w > 20.0 && short.h >= 16.0, "{short:?}");
    let label = ui.find("test/label").unwrap();
    ui.set_text(label, "Hello, wide world");
    ui.frame(0.0);
    let long = rect(&ui, "test/label");
    assert!(long.w > short.w * 2.0);
    assert_eq!(long.h, short.h);
}

#[test]
fn text_wraps_in_a_fixed_width() {
    let ui = ui();
    let r = rect(&ui, "test/wrapped");
    assert_eq!(r.w, 120.0);
    assert!(
        r.h >= 16.0 * 1.25 * 3.0,
        "wrapped onto several lines: {r:?}"
    );
}

#[test]
fn ellipsis_keeps_one_line_inside_the_box() {
    let ui = ui();
    let r = rect(&ui, "test/clipped");
    let list = ui.draw_list();
    let glyphs: Vec<_> = list
        .quads
        .iter()
        .filter(|q| q.kind() == QuadKind::Glyph as u32)
        .filter(|q| {
            q.rect[1] >= r.y - 4.0 && q.rect[3] <= r.bottom() + 8.0 && q.rect[0] >= r.x - 2.0
        })
        .collect();
    assert!(!glyphs.is_empty());
    assert!(
        glyphs.iter().all(|q| q.rect[2] <= r.right() + 1.0),
        "glyphs stay in the box"
    );
    assert!(r.h < 16.0 * 1.25 * 1.5, "one line: {r:?}");
}

#[test]
fn markup_colours_hotkeys() {
    let mut ui = ui();
    let label = ui.find("test/label").unwrap();
    ui.set_text(label, "Attack ([key]A[/key])");
    let list = ui.frame(0.0);
    let green = [0, 255, 0, 255];
    let colours: Vec<[u8; 4]> = list
        .quads
        .iter()
        .filter(|q| q.kind() == QuadKind::Glyph as u32 && q.soft == 0.0)
        .map(|q| q.color)
        .collect();
    assert_eq!(colours.iter().filter(|c| **c == green).count(), 1);
    assert!(colours.contains(&[255, 255, 255, 255]));
}

#[test]
fn first_frame_uploads_glyphs() {
    let mut ui = Ui::new(config(THEME, LAYOUT), fonts()).unwrap();
    ui.set_viewport(1280, 720, 1.0);
    let list = ui.frame(0.0);
    assert!(list.atlas.reset);
    assert!(!list.atlas.updates.is_empty());
    let list = ui.frame(0.1);
    assert!(list.atlas.updates.is_empty(), "nothing new to rasterise");
}

// --- Input --------------------------------------------------------------------------

#[test]
fn clicks_are_captured_and_reported_with_list_index() {
    let mut ui = ui();
    let at = center(rect(&ui, "test/grid/6"));
    assert!(click(&mut ui, at));
    let actions = ui.take_actions();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].name.as_deref(), Some("command"));
    assert_eq!(actions[0].index, Some(6));
    assert!(matches!(
        actions[0].kind,
        ActionKind::Click { count: 1, .. }
    ));
    // The world gets clicks on the empty screen (pointer-events: none).
    assert!(!click(&mut ui, (50.0, 400.0)));
    assert!(!ui.wants_pointer());
}

#[test]
fn double_clicks_and_disabled_buttons() {
    let mut ui = ui();
    let at = center(rect(&ui, "test/grid/0"));
    click(&mut ui, at);
    click(&mut ui, at);
    let kinds: Vec<_> = ui.take_actions().into_iter().map(|a| a.kind).collect();
    assert!(matches!(kinds[1], ActionKind::Click { count: 2, .. }));

    let grid = ui.find("test/grid").unwrap();
    let cell = ui.child(grid, 0).unwrap();
    ui.set_enabled(cell, false);
    ui.frame(1.0);
    assert!(click(&mut ui, at), "disabled buttons still block the world");
    assert!(ui.take_actions().is_empty());
}

#[test]
fn hover_state_changes_paint_without_layout() {
    let mut ui = ui();
    let r = rect(&ui, "test/grid/0");
    let fill = |ui: &Ui| {
        ui.draw_list()
            .quads
            .iter()
            .find(|q| {
                q.kind() == QuadKind::Shape as u32 && q.rect == [r.x, r.y, r.right(), r.bottom()]
            })
            .map(|q| q.color)
    };
    assert_eq!(fill(&ui), Some([0x40, 0x40, 0x40, 255]));
    let layouts = ui.stats().layout_runs;
    move_to(&mut ui, center(r));
    ui.frame(0.1);
    assert_eq!(fill(&ui), Some([0x60, 0x60, 0x60, 255]));
    assert_eq!(ui.stats().layout_runs, layouts);
}

#[test]
fn hotkeys_trigger_visible_enabled_nodes() {
    let mut ui = ui();
    let grid = ui.find("test/grid").unwrap();
    let cell = ui.child(grid, 2).unwrap();
    ui.set_hotkey(cell, Some("E"));
    ui.frame(0.0);
    assert!(key(&mut ui, Key::Char('E')));
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    ui.input(&InputEvent::Modifiers(shift));
    assert!(key(&mut ui, Key::Char('E')));
    let actions = ui.take_actions();
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0].index, Some(2));
    assert_eq!(actions[1].kind, ActionKind::Hotkey { modifiers: shift });
    assert!(!key(&mut ui, Key::Char('Q')), "unbound keys go to the game");
}

#[test]
fn text_fields_edit_and_submit() {
    let mut ui = ui();
    let field = ui.find("test/field").unwrap();
    let at = center(rect(&ui, "test/field"));
    click(&mut ui, at);
    assert!(ui.wants_keyboard());
    assert!(ui.ime_area().is_some());
    assert!(ui.input(&InputEvent::Text("Jiri".into())));
    assert!(key(&mut ui, Key::Backspace));
    ui.input(&InputEvent::ImePreedit("ř".into()));
    assert_eq!(
        ui.text(field),
        Some("Jirř"),
        "composition shows at the caret"
    );
    ui.input(&InputEvent::ImeCommit("ří".into()));
    assert_eq!(ui.text(field), Some("Jirří"));
    ui.input(&InputEvent::Text("123456".into()));
    assert_eq!(
        ui.text(field),
        Some("Jirří123"),
        "max_length counts characters"
    );
    assert!(
        key(&mut ui, Key::Char('Q')),
        "hotkeys are typed text while editing"
    );
    key(&mut ui, Key::Enter);
    let last = ui.take_actions().pop().unwrap();
    assert_eq!(last.kind, ActionKind::Submitted("Jirří123".into()));
    key(&mut ui, Key::Escape);
    assert!(!ui.wants_keyboard());
}

#[test]
fn widgets_report_values() {
    let mut ui = ui();
    // Slider: clicking at three quarters snaps to the step.
    let s = rect(&ui, "test/volume");
    click(&mut ui, (s.x + s.w * 0.76, s.y + s.h * 0.5));
    let volume = ui.find("test/volume").unwrap();
    assert_eq!(ui.value(volume), 80.0);
    // Checkbox toggles.
    let at = center(rect(&ui, "test/check"));
    click(&mut ui, at);
    assert!(ui.checked(ui.find("test/check").unwrap()));
    // Dropdown: open, then pick the third option from the popup.
    let d = rect(&ui, "test/mode");
    click(&mut ui, center(d));
    ui.frame(0.1);
    let below = (
        d.x + 20.0,
        d.bottom() + 2.0 + 4.0 + 16.0 * 1.25 * 2.5 + 25.0,
    );
    click(&mut ui, below);
    let kinds: Vec<_> = ui.take_actions().into_iter().map(|a| a.kind).collect();
    assert_eq!(
        kinds,
        [
            ActionKind::Value(80.0),
            ActionKind::Toggled(true),
            ActionKind::Selected(2)
        ]
    );
    assert_eq!(ui.selected(ui.find("test/mode").unwrap()), 2);
}

#[test]
fn drag_nodes_report_positions() {
    let mut ui = ui();
    let m = rect(&ui, "test/map");
    move_to(&mut ui, (m.x + 25.0, m.y + 50.0));
    ui.input(&InputEvent::PointerButton {
        button: PointerButton::Primary,
        pressed: true,
    });
    move_to(&mut ui, (m.x + 300.0, m.y + 75.0));
    assert!(
        ui.wants_pointer(),
        "a press keeps the pointer while dragging"
    );
    ui.input(&InputEvent::PointerButton {
        button: PointerButton::Primary,
        pressed: false,
    });
    let kinds: Vec<_> = ui.take_actions().into_iter().map(|a| a.kind).collect();
    assert_eq!(kinds.len(), 3);
    assert!(
        matches!(kinds[0], ActionKind::DragStart { at, .. } if at == canopy_ui::Vec2::new(0.25, 0.5))
    );
    assert!(matches!(kinds[1], ActionKind::Drag { at } if at == canopy_ui::Vec2::new(1.0, 0.75)));
    assert!(matches!(kinds[2], ActionKind::DragEnd { .. }));
}

#[test]
fn wheel_scrolls_lists() {
    let mut ui = ui();
    let list = ui.find("test/list").unwrap();
    ui.set_count(list, 30);
    for i in 0..30 {
        let line = ui.child(list, i).unwrap();
        ui.set_text(line, &format!("line {i}"));
    }
    ui.frame(0.0);
    let first = rect(&ui, "test/list/0");
    let at = center(rect(&ui, "test/list"));
    assert!(move_to(&mut ui, at));
    assert!(ui.input(&InputEvent::Wheel { x: 0.0, y: -1.0 }));
    ui.frame(0.0);
    assert_eq!(rect(&ui, "test/list/0").y, first.y - 40.0);
    // Clipped: list content outside the box draws through a scissor rectangle.
    assert!(ui.draw_list().batches.iter().any(|b| b.scissor.is_some()));
}

#[test]
fn tooltips_wait_then_switch_at_once() {
    let mut ui = ui();
    let grid = ui.find("test/grid").unwrap();
    let frame = ui.image_key("frame");
    ui.register_image("frame", ImageDesc::new(TextureId(1), 16.0, 16.0));
    for i in 8..10 {
        let cell = ui.child(grid, i).unwrap();
        ui.set_tooltip(
            cell,
            Some(Tooltip {
                title: format!("Command {i}"),
                hotkey: Some("Q".into()),
                costs: vec![Cost {
                    image: Some(frame),
                    amount: "50".into(),
                }],
                body: "Does [b]things[/b].".into(),
                note: None,
            }),
        );
    }
    let at = center(rect(&ui, "test/grid/8"));
    move_to(&mut ui, at);
    ui.frame(1.0);
    assert!(
        ui.rect(ui.find("@tooltip").unwrap()).is_none(),
        "not before the delay"
    );
    ui.frame(1.35);
    let shown = ui
        .rect(ui.find("@tooltip").unwrap())
        .expect("shown after the delay");
    assert!(
        shown.bottom() <= rect(&ui, "test/grid/8").y,
        "above its target"
    );
    let title = ui.find("@tooltip/title").unwrap();
    assert_eq!(ui.text(title), Some("Command 8 ([key]Q[/key])"));
    let costs = ui.find("@tooltip/costs").unwrap();
    assert_eq!(ui.count(costs), 1);
    // Moving to the next command shows its tooltip at once.
    let at = center(rect(&ui, "test/grid/9"));
    move_to(&mut ui, at);
    ui.frame(1.4);
    assert_eq!(ui.text(title), Some("Command 9 ([key]Q[/key])"));
    // Leaving hides it.
    move_to(&mut ui, (50.0, 600.0));
    ui.frame(1.5);
    assert!(ui.rect(ui.find("@tooltip").unwrap()).is_none());
}

// --- Data ---------------------------------------------------------------------------

#[test]
fn hot_reload_keeps_game_values() {
    let mut ui = ui();
    let label = ui.find("test/label").unwrap();
    ui.set_text(label, "Set by the game");
    let grid = ui.find("test/grid").unwrap();
    ui.set_count(grid, 5);
    ui.frame(0.0);
    let theme = THEME.replace("\"#202020\"", "\"#ff0000\"");
    ui.reload(config(&theme, LAYOUT)).unwrap();
    ui.frame(0.0);
    assert_eq!(ui.text(label), Some("Set by the game"));
    assert_eq!(ui.count(grid), 5);
    let red = ui
        .draw_list()
        .quads
        .iter()
        .any(|q| q.kind() == QuadKind::Shape as u32 && q.color == [255, 0, 0, 255]);
    assert!(red, "the new theme applies");
    // A broken file keeps the old UI.
    assert!(
        ui.reload(config(THEME, "{\"root\": {\"class\": \"missing\"}}"))
            .is_err()
    );
    assert_eq!(ui.text(label), Some("Set by the game"));
}

#[test]
fn layout_errors_name_the_problem() {
    let bad = THEME.replace(
        "\"pointer-events\": \"none\"",
        "\"pointer-events\": \"sometimes\"",
    );
    let err = Ui::new(config(&bad, LAYOUT), fonts())
        .err()
        .expect("error")
        .to_string();
    assert!(err.contains("pointer-events"), "{err}");
    let err = Ui::new(
        config(
            THEME,
            &LAYOUT.replace("\"repeat\": \"line\"", "\"repeat\": \"nope\""),
        ),
        fonts(),
    )
    .err()
    .expect("error")
    .to_string();
    assert!(err.contains("nope"), "{err}");
}

#[test]
fn colour_overrides_reach_quads() {
    let mut ui = ui();
    let grid = ui.find("test/grid").unwrap();
    let cell = ui.child(grid, 3).unwrap();
    ui.set_color(cell, Some(Color::hex(0x123456)));
    let list = ui.frame(0.0);
    assert!(
        list.quads
            .iter()
            .any(|q| q.color == [0x12, 0x34, 0x56, 255])
    );
}

#[test]
fn reloads_changed_files_from_disk() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("ui-reload");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("layouts")).unwrap();
    std::fs::write(dir.join("theme.json"), THEME).unwrap();
    std::fs::write(dir.join("layouts/test.json"), LAYOUT).unwrap();
    let mut ui = Ui::new(canopy_ui::UiConfig::load(&dir).unwrap(), fonts()).unwrap();
    ui.set_viewport(1280, 720, 1.0);
    ui.frame(0.0);
    assert!(!ui.reload_if_changed().unwrap());
    let moved = LAYOUT.replace(
        "\"left\": \"100px\", \"top\": \"50px\"",
        "\"left\": \"120px\", \"top\": \"50px\"",
    );
    std::fs::write(dir.join("layouts/test.json"), moved).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(dir.join("layouts/test.json"))
        .unwrap()
        .set_modified(later)
        .unwrap();
    assert!(ui.reload_if_changed().unwrap());
    ui.frame(0.1);
    assert_eq!(rect(&ui, "test/grid/0").x, 120.0);
}

#[test]
fn a_hidden_button_loses_focus_and_enter_is_left_to_the_game() {
    let mut ui = ui();
    let at = center(rect(&ui, "test/grid/0"));
    click(&mut ui, at);
    ui.take_actions();
    // Enter presses the focused button while it is shown.
    assert!(key(&mut ui, Key::Enter));
    assert!(
        ui.take_actions()
            .iter()
            .any(|a| matches!(a.kind, ActionKind::Click { .. }))
    );
    // Its screen hides (a match starts): Enter no longer reaches it.
    let grid = ui.find("test/grid").unwrap();
    ui.set_visible(grid, false);
    ui.frame(0.0);
    assert!(!key(&mut ui, Key::Enter), "the key goes to the game");
    assert!(ui.take_actions().is_empty());
}
