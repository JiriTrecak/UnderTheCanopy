//! Rebindable keyboard shortcuts (old `src/shared/input/shortcuts.ts` and `catalog.ts`).
//!
//! A binding is a chord: a physical key named like the browser's `KeyboardEvent.code`
//! (which winit's `KeyCode` follows: `KeyH`, `Digit1`, `ArrowLeft`, `F10`…) and the
//! modifiers held with it, written `Ctrl+Shift+KeyH`. The player's file stores only the
//! entries changed from their defaults (`null` for an unbound entry), as the old game did.
//!
//! Rules (old game): a key another entry already uses is refused ("Already used by …");
//! Enter is reserved for chat; only listed keys may be bound; a bare modifier only for the
//! health bar holds. Defaults may share keys; only edited collisions are blocked.
//!
//! Content-derived command shortcuts (command card, build menus, abilities; old
//! `catalog.ts`) are entries too, with ids `command.<command id>` and their authored
//! hotkeys as defaults ([`commands`]). Defaults may share keys across cards.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::fmt;
use winit::keyboard::{KeyCode, ModifiersState};

/// A key and the modifiers held with it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Chord {
    /// The `KeyboardEvent.code` name of the key, or `Alt` for the bare Alt hold.
    pub code: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl Chord {
    pub fn key(code: &str) -> Self {
        Self {
            code: code.into(),
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let mut chord = Self::key("");
        let parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let (code, modifiers) = parts.split_last()?;
        for m in modifiers {
            match *m {
                "Ctrl" => chord.ctrl = true,
                "Alt" => chord.alt = true,
                "Shift" => chord.shift = true,
                "Meta" => chord.meta = true,
                _ => return None,
            }
        }
        if code.is_empty() {
            return None;
        }
        chord.code = (*code).into();
        Some(chord)
    }

    /// The chord of a key press, from winit.
    pub fn of(code: KeyCode, modifiers: ModifiersState) -> Option<Self> {
        let name = code_name(code)?;
        Some(Self {
            code: name.into(),
            ctrl: modifiers.control_key(),
            alt: modifiers.alt_key() && name != "Alt",
            shift: modifiers.shift_key(),
            meta: modifiers.super_key(),
        })
    }

    /// A bare modifier (only Alt may be bound alone).
    pub fn is_modifier(&self) -> bool {
        self.code == "Alt"
    }

    /// What the settings screen shows (`Ctrl+Shift+H`, `PgUp`, `Num 7`).
    pub fn label(&self) -> String {
        let mut out = String::new();
        for (on, name) in [
            (self.ctrl, "keys.ctrl"),
            (self.alt, "keys.alt"),
            (self.shift, "keys.shift"),
            (
                self.meta,
                if cfg!(target_os = "macos") {
                    "keys.cmd"
                } else {
                    "keys.win"
                },
            ),
        ] {
            if on {
                out.push_str(&crate::text::tr(name));
                out.push('+');
            }
        }
        let code = self.code.as_str();
        let key = if let Some(letter) = code.strip_prefix("Key") {
            letter.to_string()
        } else if let Some(digit) = code.strip_prefix("Digit") {
            digit.to_string()
        } else if let Some(n) = code.strip_prefix("Numpad") {
            tr!("keys.numpad", number = n)
        } else if let Some(arrow) = code.strip_prefix("Arrow") {
            arrow.to_string()
        } else {
            match code {
                "PageUp" => crate::text::tr("keys.page-up"),
                "PageDown" => crate::text::tr("keys.page-down"),
                "Escape" => crate::text::tr("keys.esc"),
                "Backquote" => "`".into(),
                "BracketLeft" => "[".into(),
                "BracketRight" => "]".into(),
                "Equal" => "=".into(),
                "Minus" => "-".into(),
                "Backspace" => crate::text::tr("keys.backspace"),
                other => other.into(),
            }
        };
        out.push_str(&key);
        out
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (on, name) in [
            (self.ctrl, "Ctrl+"),
            (self.alt, "Alt+"),
            (self.shift, "Shift+"),
            (self.meta, "Meta+"),
        ] {
            if on {
                f.write_str(name)?;
            }
        }
        f.write_str(&self.code)
    }
}

impl Serialize for Chord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Chord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Chord::parse(&text).ok_or_else(|| serde::de::Error::custom(format!("bad key {text:?}")))
    }
}

/// Key codes a binding may use (old `allowedCodes`), with their winit codes.
const KEYS: &[(&str, KeyCode)] = {
    use KeyCode::*;
    &[
        ("KeyA", KeyA),
        ("KeyB", KeyB),
        ("KeyC", KeyC),
        ("KeyD", KeyD),
        ("KeyE", KeyE),
        ("KeyF", KeyF),
        ("KeyG", KeyG),
        ("KeyH", KeyH),
        ("KeyI", KeyI),
        ("KeyJ", KeyJ),
        ("KeyK", KeyK),
        ("KeyL", KeyL),
        ("KeyM", KeyM),
        ("KeyN", KeyN),
        ("KeyO", KeyO),
        ("KeyP", KeyP),
        ("KeyQ", KeyQ),
        ("KeyR", KeyR),
        ("KeyS", KeyS),
        ("KeyT", KeyT),
        ("KeyU", KeyU),
        ("KeyV", KeyV),
        ("KeyW", KeyW),
        ("KeyX", KeyX),
        ("KeyY", KeyY),
        ("KeyZ", KeyZ),
        ("Digit0", Digit0),
        ("Digit1", Digit1),
        ("Digit2", Digit2),
        ("Digit3", Digit3),
        ("Digit4", Digit4),
        ("Digit5", Digit5),
        ("Digit6", Digit6),
        ("Digit7", Digit7),
        ("Digit8", Digit8),
        ("Digit9", Digit9),
        ("Numpad0", Numpad0),
        ("Numpad1", Numpad1),
        ("Numpad2", Numpad2),
        ("Numpad3", Numpad3),
        ("Numpad4", Numpad4),
        ("Numpad5", Numpad5),
        ("Numpad6", Numpad6),
        ("Numpad7", Numpad7),
        ("Numpad8", Numpad8),
        ("Numpad9", Numpad9),
        ("F1", F1),
        ("F2", F2),
        ("F3", F3),
        ("F4", F4),
        ("F5", F5),
        ("F6", F6),
        ("F7", F7),
        ("F8", F8),
        ("F9", F9),
        ("F10", F10),
        ("F11", F11),
        ("F12", F12),
        ("ArrowLeft", ArrowLeft),
        ("ArrowRight", ArrowRight),
        ("ArrowUp", ArrowUp),
        ("ArrowDown", ArrowDown),
        ("Home", Home),
        ("End", End),
        ("PageUp", PageUp),
        ("PageDown", PageDown),
        ("Space", Space),
        ("Tab", Tab),
        ("Escape", Escape),
        ("Enter", Enter),
        ("Backspace", Backspace),
        ("Backquote", Backquote),
        ("BracketLeft", BracketLeft),
        ("BracketRight", BracketRight),
        ("Equal", Equal),
        ("Minus", Minus),
        ("Delete", Delete),
        ("Alt", AltLeft),
        ("Alt", AltRight),
    ]
};

/// The `KeyboardEvent.code` name of a winit key, if bindable.
pub fn code_name(code: KeyCode) -> Option<&'static str> {
    KEYS.iter().find(|(_, c)| *c == code).map(|(name, _)| *name)
}

/// The winit key of a binding's code.
pub fn key_code(name: &str) -> Option<KeyCode> {
    KEYS.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

/// One bindable entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub group: &'static str,
    pub label: String,
    pub default: Option<Chord>,
}

/// The engine's own shortcuts with the old game's defaults.
pub fn catalog() -> Vec<Entry> {
    let mut out = Vec::new();
    let mut add = |id: String, group: &'static str, label: String, default: &str| {
        out.push(Entry {
            id,
            group,
            label,
            default: Chord::parse(default),
        });
    };
    for (id, key) in [
        ("camera.zoomIn", "PageUp"),
        ("camera.zoomOut", "PageDown"),
        ("camera.panLeft", "ArrowLeft"),
        ("camera.panRight", "ArrowRight"),
        ("camera.panUp", "ArrowUp"),
        ("camera.panDown", "ArrowDown"),
        ("camera.centerSelection", "Home"),
        ("camera.alerts", "Space"),
        ("camera.nextHall", "Backspace"),
    ] {
        add(
            id.into(),
            "Camera",
            crate::text::tr(&format!("keys.{id}")),
            key,
        );
    }
    for n in 1..=3 {
        add(
            format!("heroes.select{n}"),
            "Heroes",
            tr!("keys.heroes.select", number = n),
            &format!("F{}", n + 1),
        );
    }
    for (id, key) in [
        ("selection.army", "F1"),
        ("selection.idleWorker", "F8"),
        ("selection.idleWorkerAlt", "Backquote"),
        ("selection.nextSubgroup", "Tab"),
        ("selection.previousSubgroup", "Shift+Tab"),
    ] {
        add(
            id.into(),
            "Selection",
            crate::text::tr(&format!("keys.{id}")),
            key,
        );
    }
    for (id, key) in [
        ("interface.gameMenu", "F10"),
        ("interface.toggleHud", "Ctrl+Shift+KeyH"),
        ("interface.debug", "Ctrl+F3"),
        ("interface.cancel", "Escape"),
    ] {
        add(
            id.into(),
            "Interface",
            crate::text::tr(&format!("keys.{id}")),
            key,
        );
    }
    for (id, key) in [
        ("placement.rotateClockwise", "KeyR"),
        ("placement.rotateCounterclockwise", "Shift+KeyR"),
    ] {
        add(
            id.into(),
            "Placement",
            crate::text::tr(&format!("keys.{id}")),
            key,
        );
    }
    for (n, key) in ["Numpad7", "Numpad8", "Numpad4", "Numpad5"]
        .into_iter()
        .enumerate()
    {
        add(
            format!("inventory.slot{}", n + 1),
            "Inventory",
            tr!("keys.inventory.slot", number = n + 1),
            key,
        );
    }
    for (id, key) in [
        ("autoplay.takeOver", "F7"),
        ("autoplay.faster", "Equal"),
        ("autoplay.slower", "Minus"),
    ] {
        add(
            id.into(),
            "Autoplay",
            crate::text::tr(&format!("keys.{id}")),
            key,
        );
    }
    for (id, key) in [
        ("health.all", "Alt"),
        ("health.friendly", "BracketLeft"),
        ("health.enemy", "BracketRight"),
    ] {
        add(
            id.into(),
            "Health bars",
            crate::text::tr(&format!("keys.{id}")),
            key,
        );
    }
    for n in [1, 2, 3, 4, 5, 6, 7, 8, 9, 0] {
        add(
            format!("groups.recall{n}"),
            "Control groups",
            tr!("keys.groups.recall", number = n),
            &format!("Digit{n}"),
        );
        add(
            format!("groups.assign{n}"),
            "Control groups",
            tr!("keys.groups.assign", number = n),
            &format!("Ctrl+Digit{n}"),
        );
        add(
            format!("groups.add{n}"),
            "Control groups",
            tr!("keys.groups.add", number = n),
            &format!("Shift+Digit{n}"),
        );
    }
    out
}

/// A catalogue group as players read it (`keys.group.<group>` in the text library).
pub fn group_label(group: &str) -> String {
    crate::text::tr(&format!(
        "keys.group.{}",
        group.to_lowercase().replace(' ', "-")
    ))
}

/// Group of the content-derived command entries.
pub const COMMANDS: &str = "Commands";

/// An authored content hotkey as a chord (old `authoredKey`: `"A"` → `KeyA`,
/// `"ESC"` → `Escape`).
pub fn authored(key: &str) -> Option<Chord> {
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    let mut chars = key.chars();
    let code = match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => format!("Key{}", c.to_ascii_uppercase()),
        (Some(c), None) if c.is_ascii_digit() => format!("Digit{c}"),
        _ if key.eq_ignore_ascii_case("esc") => "Escape".into(),
        _ => key.into(),
    };
    Chord::parse(&code)
}

/// The catalogue id of a command card entry (`command.<id>`; Back is the old
/// `navigation:back`).
pub fn command_id(id: &str) -> String {
    match id {
        "back" => "command.navigation:back".into(),
        other => format!("command.{other}"),
    }
}

/// The content's command shortcuts (old `shortcutCatalog`): basic actions (not the
/// per-definition verbs), categories, camera and Back navigation, visible overrides,
/// every constructable and trainable definition, ability commands and research.
pub fn commands(content: &canopy_content::Content) -> Vec<Entry> {
    use canopy_content::game::Creation;
    let actions = &content.actions;
    let mut out: Vec<Entry> = Vec::new();
    let mut add = |id: String, label: &str, key: Option<&str>| {
        let id = format!("command.{id}");
        if let Some(e) = out.iter_mut().find(|e| e.id == id) {
            // Old `Map.set`: a later entry with the same id replaces the earlier one.
            e.label = label.into();
            e.default = key.and_then(authored);
        } else {
            out.push(Entry {
                id,
                group: COMMANDS,
                label: label.into(),
                default: key.and_then(authored),
            });
        }
    };
    for (id, a) in &actions.actions {
        if !["build", "produce", "research", "cancelResearch"].contains(&id.as_str()) {
            add(id.clone(), &a.name, a.hotkey.as_deref());
        }
    }
    for (id, a) in &actions.categories {
        add(format!("category:{id}"), &a.name, a.hotkey.as_deref());
    }
    if let Some(camera) = &actions.navigation.camera {
        add("camera".into(), &camera.name, camera.hotkey.as_deref());
    }
    let back = &actions.navigation.back;
    add("navigation:back".into(), &back.name, back.hotkey.as_deref());
    for (id, o) in &actions.overrides {
        if o.hidden == Some(true) {
            continue;
        }
        let (verb, target) = id.split_once(':').unwrap_or((id.as_str(), ""));
        let name = content
            .definition(target)
            .map_or(id.as_str(), |d| d.name.as_str());
        let key = o
            .hotkey
            .as_deref()
            .or_else(|| actions.actions.get(verb).and_then(|a| a.hotkey.as_deref()));
        add(id.clone(), name, key);
    }
    for d in &content.definitions {
        let verb = match d.creation {
            Some(Creation::Construct { .. }) => "build",
            Some(Creation::Train { .. }) => "produce",
            _ => continue,
        };
        let id = format!("{verb}:{}", d.id);
        // An override (visible or hidden) already decided this entry.
        if actions.overrides.contains_key(&id) {
            continue;
        }
        let key = actions.actions.get(verb).and_then(|a| a.hotkey.clone());
        add(id, &d.name, key.as_deref());
    }
    for d in &content.definitions {
        let Some(policy) = &d.behaviors.abilities else {
            continue;
        };
        for b in &policy.bindings {
            let Some(command) = &b.command else {
                continue;
            };
            let name = content
                .abilities
                .abilities
                .iter()
                .find(|a| a.id == b.ability)
                .map_or(b.ability.as_str(), |a| a.name.as_str());
            add(format!("cast:{}", b.id), name, Some(&command.hotkey));
        }
    }
    for (id, r) in &content.rules.research {
        add(format!("research:{id}"), &r.name, None);
    }
    out
}

/// The whole rebindable catalogue: the engine's shortcuts, then the content's commands.
pub fn full_catalog(content: Option<&canopy_content::Content>) -> Vec<Entry> {
    let mut out = catalog();
    if let Some(c) = content {
        out.extend(commands(c));
    }
    out
}

/// Why a key cannot be bound.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BindError {
    #[error("Already used by {0}. Clear that binding first.")]
    Conflict(String),
    #[error("Enter is reserved for chat.")]
    Reserved,
    #[error("Unsupported key. Try a letter, number, function key or navigation key.")]
    Unsupported,
    #[error("No shortcut {0}.")]
    Unknown(String),
}

impl BindError {
    /// The refusal as players read it.
    pub fn message(&self) -> String {
        match self {
            Self::Conflict(name) => tr!("settings.key-conflict", name = name.as_str()),
            Self::Reserved => tr!("settings.key-reserved"),
            Self::Unsupported => tr!("settings.key-unsupported"),
            Self::Unknown(id) => tr!("settings.key-unknown", id = id.as_str()),
        }
    }
}

/// The player's bindings: the catalog's defaults with their overrides.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct KeyBindings {
    /// Changed entries: a chord, or `None` for unbound.
    pub overrides: BTreeMap<String, Option<Chord>>,
}

impl KeyBindings {
    /// The current chord of an entry.
    pub fn chord<'a>(&'a self, catalog: &'a [Entry], id: &str) -> Option<&'a Chord> {
        match self.overrides.get(id) {
            Some(chord) => chord.as_ref(),
            None => catalog.iter().find(|e| e.id == id)?.default.as_ref(),
        }
    }

    /// Binds `id` to `chord`, refusing reserved, unsupported and taken keys.
    pub fn bind(&mut self, catalog: &[Entry], id: &str, chord: Chord) -> Result<(), BindError> {
        let entry = catalog
            .iter()
            .find(|e| e.id == id)
            .ok_or_else(|| BindError::Unknown(id.into()))?;
        if chord.code == "Enter" {
            return Err(BindError::Reserved);
        }
        if key_code(&chord.code).is_none()
            || (chord.is_modifier() && !entry.id.starts_with("health."))
        {
            return Err(BindError::Unsupported);
        }
        if let Some(other) = catalog
            .iter()
            .find(|e| e.id != id && self.chord(catalog, &e.id) == Some(&chord))
        {
            return Err(BindError::Conflict(other.label.clone()));
        }
        if entry.default.as_ref() == Some(&chord) {
            self.overrides.remove(id);
        } else {
            self.overrides.insert(id.into(), Some(chord));
        }
        Ok(())
    }

    /// Leaves an entry unbound.
    pub fn clear(&mut self, catalog: &[Entry], id: &str) {
        if catalog.iter().any(|e| e.id == id && e.default.is_some()) {
            self.overrides.insert(id.into(), None);
        } else {
            self.overrides.remove(id);
        }
    }

    pub fn restore_defaults(&mut self) {
        self.overrides.clear();
    }

    /// Pairs of entries sharing a chord where at least one was changed by the player
    /// (defaults may share keys, as the old game's command cards did); the settings
    /// screen marks them.
    pub fn conflicts<'a>(&'a self, catalog: &'a [Entry]) -> Vec<(&'a Entry, &'a Entry)> {
        let mut by_chord: BTreeMap<&Chord, Vec<&Entry>> = BTreeMap::new();
        for e in catalog {
            if let Some(chord) = self.chord(catalog, &e.id) {
                by_chord.entry(chord).or_default().push(e);
            }
        }
        by_chord
            .values()
            .flat_map(|entries| {
                entries
                    .iter()
                    .enumerate()
                    .flat_map(move |(i, a)| entries[i + 1..].iter().map(move |b| (*a, *b)))
            })
            .filter(|(a, b)| {
                self.overrides.contains_key(&a.id) || self.overrides.contains_key(&b.id)
            })
            .collect()
    }

    /// Drops overrides of entries the catalog no longer has (old files after a change).
    /// Command entries (`command.*`) depend on the content and are kept.
    pub fn prune(&mut self, catalog: &[Entry]) {
        self.overrides
            .retain(|id, _| id.starts_with("command.") || catalog.iter().any(|e| &e.id == id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_round_trip_through_text() {
        let chord = Chord::parse("Ctrl+Shift+KeyH").unwrap();
        assert!(chord.ctrl && chord.shift && !chord.alt);
        assert_eq!(chord.to_string(), "Ctrl+Shift+KeyH");
        assert_eq!(chord.label(), "Ctrl+Shift+H");
        assert_eq!(Chord::parse("PageUp").unwrap().label(), "PgUp");
        assert!(Chord::parse("Hyper+KeyA").is_none());
        assert!(Chord::parse("Ctrl+").is_none());
        let from_winit = Chord::of(KeyCode::KeyR, ModifiersState::SHIFT).unwrap();
        assert_eq!(from_winit, Chord::parse("Shift+KeyR").unwrap());
        assert_eq!(
            Chord::of(KeyCode::AltLeft, ModifiersState::ALT)
                .unwrap()
                .to_string(),
            "Alt"
        );
    }

    #[test]
    fn every_default_is_bindable_and_unique() {
        let catalog = catalog();
        let keys = KeyBindings::default();
        assert!(keys.conflicts(&catalog).is_empty());
        for e in &catalog {
            let chord = e.default.as_ref().expect("a default");
            assert!(key_code(&chord.code).is_some(), "{}", e.id);
        }
        let ids: std::collections::BTreeSet<&str> = catalog.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids.len(), catalog.len());
    }

    #[test]
    fn rebinding_refuses_taken_reserved_and_unsupported_keys() {
        let catalog = catalog();
        let mut keys = KeyBindings::default();
        let menu = "interface.gameMenu";
        // Taken by "Rotate building clockwise".
        assert_eq!(
            keys.bind(&catalog, menu, Chord::key("KeyR")),
            Err(BindError::Conflict("Rotate building clockwise".into()))
        );
        assert_eq!(
            keys.bind(&catalog, menu, Chord::key("Enter")),
            Err(BindError::Reserved)
        );
        assert_eq!(
            keys.bind(&catalog, menu, Chord::key("Alt")),
            Err(BindError::Unsupported)
        );
        assert_eq!(
            keys.bind(&catalog, menu, Chord::key("KeyLol")),
            Err(BindError::Unsupported)
        );
        // Clearing the holder frees the key.
        keys.clear(&catalog, "placement.rotateClockwise");
        assert_eq!(keys.chord(&catalog, "placement.rotateClockwise"), None);
        keys.bind(&catalog, menu, Chord::key("KeyR")).unwrap();
        assert_eq!(keys.chord(&catalog, menu), Some(&Chord::key("KeyR")));
        // Binding back to the default drops the override.
        keys.bind(&catalog, menu, Chord::key("F10")).unwrap();
        assert!(!keys.overrides.contains_key(menu));
        keys.restore_defaults();
        assert!(keys.overrides.is_empty());
    }

    #[test]
    fn authored_hotkeys_read_like_the_old_authored_key() {
        assert_eq!(authored("q"), Some(Chord::key("KeyQ")));
        assert_eq!(authored("ESC"), Some(Chord::key("Escape")));
        assert_eq!(authored("Escape"), Some(Chord::key("Escape")));
        assert_eq!(authored("F2"), Some(Chord::key("F2")));
        assert_eq!(authored(""), None);
        assert_eq!(command_id("back"), "command.navigation:back");
        assert_eq!(command_id("cast:bolt"), "command.cast:bolt");
    }

    #[test]
    fn hand_edited_files_can_collide_and_are_reported() {
        let catalog = catalog();
        let keys: KeyBindings = serde_json::from_str(
            r#"{"interface.gameMenu": "KeyR", "camera.alerts": null, "gone.entry": "KeyQ"}"#,
        )
        .unwrap();
        let conflicts = keys.conflicts(&catalog);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].0.id, "interface.gameMenu");
        assert_eq!(keys.chord(&catalog, "camera.alerts"), None);
        let mut pruned = keys.clone();
        pruned.prune(&catalog);
        assert!(!pruned.overrides.contains_key("gone.entry"));
    }
}
