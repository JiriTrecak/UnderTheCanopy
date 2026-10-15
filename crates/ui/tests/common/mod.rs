//! Test fixtures: a small theme and layouts owned by the tests (not the shipped UI).
#![allow(dead_code)]

use canopy_ui::{FontSources, Ui, UiConfig};
use std::path::Path;

pub const THEME: &str = r##"{
  "palette": { "accent": "#ffcc00" },
  "fonts": { "families": { "ui": ["Outfit", "Helvetica", "Arial", "system-ui"] } },
  "images": { "frame": "fixture/frame.png" },
  "tooltip": { "template": "tip", "delay-ms": 300, "warm-ms": 400 },
  "hotkey-color": "#00ff00",
  "styles": {
    "root": { "font": "16px/1.25 ui", "color": "#ffffff" },
    "screen": { "pointer-events": "none" },
    "panel": { "pointer-events": "auto", "background": "#202020" },
    "button": { "background": "#404040", ":hover": { "background": "#606060" }, ":disabled": { "opacity": "0.5" } },
    "tip": { "position": "absolute", "padding": "4px", "background": "#000000", "display": "flex", "flex-direction": "column", "pointer-events": "none" },
    "list": { "overflow": "scroll", "display": "flex", "flex-direction": "column", "pointer-events": "auto" }
  }
}"##;

pub const LAYOUT: &str = r##"{
  "visible": true,
  "templates": {
    "tip": { "class": "tip", "children": [
      { "id": "title", "kind": "label" },
      { "id": "costs", "repeat": "cost" },
      { "id": "body", "kind": "label" },
      { "id": "note", "kind": "label" }
    ] },
    "cost": { "children": [ { "id": "icon", "kind": "image" }, { "id": "amount", "kind": "label" } ] },
    "cell": { "kind": "button", "class": "button", "action": "command", "children": [
      { "id": "icon", "kind": "image", "style": { "position": "absolute", "left": "10%", "top": "10%", "width": "80%", "height": "80%" } }
    ] },
    "line": { "kind": "label", "style": { "flex-shrink": 0 } }
  },
  "root": {
    "class": "screen",
    "children": [
      { "id": "grid", "class": "panel", "repeat": "cell", "count": 12,
        "style": { "position": "absolute", "left": "100px", "top": "50px", "width": "400px", "height": "300px",
                   "display": "grid", "grid-template-columns": "repeat(4, minmax(0, 1fr))", "grid-template-rows": "repeat(3, minmax(0, 1fr))" } },
      { "id": "scaled", "class": "panel", "style": { "position": "absolute", "left": "0", "bottom": "0", "width": "200px", "height": "100px",
                   "transform": "scale(0.5)", "transform-origin": "bottom left" },
        "children": [ { "id": "inner", "style": { "position": "absolute", "right": "0", "top": "0", "width": "50%", "height": "50%" } } ] },
      { "id": "label", "kind": "label", "text": "Hello", "style": { "position": "absolute", "left": "600px", "top": "50px" } },
      { "id": "wrapped", "kind": "label", "text": "one two three four five six seven eight nine ten", "style": { "position": "absolute", "left": "600px", "top": "100px", "width": "120px" } },
      { "id": "clipped", "kind": "label", "text": "a long line that will not fit", "style": { "position": "absolute", "left": "600px", "top": "300px", "width": "80px", "white-space": "nowrap", "text-overflow": "ellipsis" } },
      { "id": "field", "kind": "text_field", "class": "panel", "placeholder": "Name", "max_length": 8, "style": { "position": "absolute", "left": "600px", "top": "400px", "width": "200px", "padding": "4px" } },
      { "id": "volume", "kind": "slider", "class": "panel", "min": 0, "max": 100, "step": 10, "value": 50, "style": { "position": "absolute", "left": "600px", "top": "460px", "width": "200px", "height": "20px" } },
      { "id": "check", "kind": "checkbox", "class": "panel", "text": "Bars", "style": { "position": "absolute", "left": "600px", "top": "500px" } },
      { "id": "mode", "kind": "dropdown", "class": "panel", "options": ["Low", "Medium", "High"], "style": { "position": "absolute", "left": "600px", "top": "540px", "width": "200px" } },
      { "id": "map", "kind": "image", "class": "panel", "drag": true, "action": "minimap", "style": { "position": "absolute", "left": "900px", "top": "50px", "width": "100px", "height": "100px" } },
      { "id": "list", "class": "list", "repeat": "line", "style": { "position": "absolute", "left": "900px", "top": "200px", "width": "200px", "height": "100px" } },
      { "id": "hidden", "class": "panel", "visible": false, "style": { "position": "absolute", "left": "0", "top": "0", "width": "100px", "height": "100px" } }
    ]
  }
}"##;

/// The bundled font when Git LFS content is present, otherwise the system's fonts
/// (CI checkouts have LFS pointers only).
pub fn fonts() -> FontSources {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/ui/fonts/Outfit-Latin.ttf");
    match std::fs::read(&path) {
        Ok(bytes) if !bytes.starts_with(b"version https://git-lfs") => FontSources {
            bundled: vec![bytes],
            system: false,
        },
        _ => FontSources {
            bundled: Vec::new(),
            system: true,
        },
    }
}

pub fn config(theme: &str, layout: &str) -> UiConfig {
    UiConfig::parse(theme, &[("test".to_owned(), layout.to_owned())], |_| {
        Ok(Vec::new())
    })
    .expect("fixture config")
}

pub fn ui() -> Ui {
    let mut ui = Ui::new(config(THEME, LAYOUT), fonts()).expect("fixture ui");
    ui.set_viewport(1280, 720, 1.0);
    ui.frame(0.0);
    ui
}
