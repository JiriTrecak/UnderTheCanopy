//! The engine's in-match shortcuts from the player's rebindable catalogue
//! (`settings::keys`): camera, menu, control groups, heroes, selection, placement,
//! inventory and health bar holds, and the content's command hotkeys (the command card,
//! `command.*` entries, defaulting to the authored keys), matched after the engine's.
//!
//! Key names: the match's input names keys the way the content writes hotkeys (`"A"`,
//! `"1"`, `"F1"`), the catalogue like `KeyboardEvent.code` (`"KeyA"`, `"Digit1"`,
//! `"F1"`); [`code_of`] converts.

use crate::settings::{
    Chord, KeyBindings,
    keys::{self, Entry},
};

/// An engine shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    ZoomIn,
    ZoomOut,
    PanLeft,
    PanRight,
    PanUp,
    PanDown,
    CenterSelection,
    Alerts,
    NextHall,
    /// Zero-based hero slot.
    Hero(usize),
    IdleWorker,
    /// Every own army unit and hero.
    Army,
    NextSubgroup,
    PreviousSubgroup,
    GameMenu,
    ToggleHud,
    Debug,
    Cancel,
    /// Autoplay (docs/campaign.md "Autoplay"): hand the match back to the player, or play
    /// it faster or slower.
    TakeOver,
    AutoplayFaster,
    AutoplaySlower,
    RotateClockwise,
    RotateCounterclockwise,
    /// Zero-based inventory slot.
    Inventory(usize),
    HealthAll,
    HealthFriendly,
    HealthEnemy,
    /// Zero-based control group slot (key 1 is slot 0, key 0 slot 9).
    Recall(usize),
    Assign(usize),
    Add(usize),
}

impl Shortcut {
    /// The shortcut of a catalogue entry id.
    pub fn of(id: &str) -> Option<Self> {
        use Shortcut::*;
        let numbered = |prefix: &str| -> Option<usize> { id.strip_prefix(prefix)?.parse().ok() };
        // Control group keys: 1–9 are slots 0–8, 0 is slot 9.
        let group = |prefix: &str| numbered(prefix).map(|n| if n == 0 { 9 } else { n - 1 });
        Some(match id {
            "camera.zoomIn" => ZoomIn,
            "camera.zoomOut" => ZoomOut,
            "camera.panLeft" => PanLeft,
            "camera.panRight" => PanRight,
            "camera.panUp" => PanUp,
            "camera.panDown" => PanDown,
            "camera.centerSelection" => CenterSelection,
            "camera.alerts" => Alerts,
            "camera.nextHall" => NextHall,
            "selection.idleWorker" | "selection.idleWorkerAlt" => IdleWorker,
            "selection.army" => Army,
            "selection.nextSubgroup" => NextSubgroup,
            "selection.previousSubgroup" => PreviousSubgroup,
            "interface.gameMenu" => GameMenu,
            "interface.toggleHud" => ToggleHud,
            "interface.debug" => Debug,
            "interface.cancel" => Cancel,
            "placement.rotateClockwise" => RotateClockwise,
            "placement.rotateCounterclockwise" => RotateCounterclockwise,
            "health.all" => HealthAll,
            "health.friendly" => HealthFriendly,
            "health.enemy" => HealthEnemy,
            "autoplay.takeOver" => TakeOver,
            "autoplay.faster" => AutoplayFaster,
            "autoplay.slower" => AutoplaySlower,
            _ => {
                if let Some(n) = numbered("heroes.select") {
                    Hero(n.checked_sub(1)?)
                } else if let Some(n) = numbered("inventory.slot") {
                    Inventory(n.checked_sub(1)?)
                } else if let Some(slot) = group("groups.recall") {
                    Recall(slot)
                } else if let Some(slot) = group("groups.assign") {
                    Assign(slot)
                } else {
                    Add(group("groups.add")?)
                }
            }
        })
    }
}

/// The catalogue's code of a key the match names (`"A"` → `"KeyA"`, `"1"` → `"Digit1"`).
pub fn code_of(name: &str) -> String {
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_uppercase() => format!("Key{c}"),
        (Some(c), None) if c.is_ascii_digit() => format!("Digit{c}"),
        _ => name.to_owned(),
    }
}

/// The player's chords of the engine shortcuts.
#[derive(Debug, Clone, PartialEq)]
pub struct Keymap {
    entries: Vec<(Chord, Shortcut)>,
    /// Each entry's chord as the interface shows it (made once: the HUD asks every frame).
    labels: Vec<String>,
    /// Command entries of the catalogue (`command.<id>`) and their chords (`None`:
    /// unbound). A command missing here keeps its authored hotkey.
    commands: std::collections::BTreeMap<String, Option<Chord>>,
}

impl Default for Keymap {
    /// The catalogue's defaults.
    fn default() -> Self {
        Self::new(&KeyBindings::default(), &keys::catalog())
    }
}

impl Keymap {
    pub fn new(bindings: &KeyBindings, catalog: &[Entry]) -> Self {
        let entries = catalog
            .iter()
            .filter_map(|e| {
                Some((
                    bindings.chord(catalog, &e.id)?.clone(),
                    Shortcut::of(&e.id)?,
                ))
            })
            .collect::<Vec<_>>();
        let labels = entries.iter().map(|(chord, _)| chord.label()).collect();
        let commands = catalog
            .iter()
            .filter(|e| e.group == keys::COMMANDS)
            .map(|e| (e.id.clone(), bindings.chord(catalog, &e.id).cloned()))
            .collect();
        Self {
            entries,
            labels,
            commands,
        }
    }

    /// The chord of a command card entry: the player's binding of `command.<id>`, else
    /// the authored hotkey.
    pub fn command(&self, id: &str, authored: Option<&str>) -> Option<Chord> {
        match self.commands.get(&keys::command_id(id)) {
            Some(chord) => chord.clone(),
            None => authored.and_then(keys::authored),
        }
    }

    /// The label of the first chord bound to `shortcut` (`""` when unbound).
    pub fn label(&self, shortcut: Shortcut) -> &str {
        self.entries
            .iter()
            .position(|(_, s)| *s == shortcut)
            .map_or("", |i| &self.labels[i])
    }

    /// The shortcuts a key press triggers, in catalogue order (defaults may share a
    /// key). Command (macOS) stands in for Control when nothing is bound with it.
    pub fn pressed(&self, chord: &Chord) -> Vec<Shortcut> {
        let find = |c: &Chord| -> Vec<Shortcut> {
            self.entries
                .iter()
                .filter(|(k, _)| k == c)
                .map(|(_, s)| *s)
                .collect()
        };
        let found = find(chord);
        if found.is_empty() && chord.meta && !chord.ctrl {
            return find(&Chord {
                meta: false,
                ctrl: true,
                ..chord.clone()
            });
        }
        found
    }

    /// Whether a held key (by catalogue code, any modifiers) holds `shortcut`: camera
    /// movement and health bar holds.
    pub fn holds(&self, shortcut: Shortcut, code: &str) -> bool {
        self.entries
            .iter()
            .any(|(k, s)| *s == shortcut && k.code == code)
    }

    /// The codes bound to `shortcut`.
    pub fn codes(&self, shortcut: Shortcut) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .filter(move |(_, s)| *s == shortcut)
            .map(|(k, _)| k.code.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalogue_entry_is_an_engine_shortcut() {
        for e in keys::catalog() {
            assert!(Shortcut::of(&e.id).is_some(), "{}", e.id);
        }
        assert_eq!(Shortcut::of("groups.recall0"), Some(Shortcut::Recall(9)));
        assert_eq!(Shortcut::of("heroes.select1"), Some(Shortcut::Hero(0)));
        assert_eq!(Shortcut::of("command.attack"), None);
    }

    #[test]
    fn presses_follow_rebinding_and_command_stands_in_for_control() {
        let catalog = keys::catalog();
        let mut bindings = KeyBindings::default();
        let map = Keymap::new(&bindings, &catalog);
        let chord = |t: &str| Chord::parse(t).unwrap();
        assert_eq!(map.pressed(&chord("Ctrl+Digit3")), [Shortcut::Assign(2)]);
        assert_eq!(map.pressed(&chord("Meta+Digit3")), [Shortcut::Assign(2)]);
        assert_eq!(
            map.pressed(&chord("Shift+Tab")),
            [Shortcut::PreviousSubgroup]
        );
        assert!(map.pressed(&chord("KeyA")).is_empty(), "content keys pass");
        assert!(map.holds(Shortcut::PanLeft, "ArrowLeft"));

        bindings
            .bind(&catalog, "interface.gameMenu", chord("KeyM"))
            .unwrap();
        bindings.clear(&catalog, "camera.panLeft");
        bindings
            .bind(&catalog, "camera.panLeft", chord("KeyJ"))
            .unwrap();
        let map = Keymap::new(&bindings, &catalog);
        assert_eq!(map.pressed(&chord("KeyM")), [Shortcut::GameMenu]);
        assert!(map.pressed(&chord("F10")).is_empty());
        assert!(map.holds(Shortcut::PanLeft, "KeyJ"));
        assert!(!map.holds(Shortcut::PanLeft, "ArrowLeft"));
        assert_eq!(code_of("J"), "KeyJ");
        assert_eq!(code_of("0"), "Digit0");
        assert_eq!(code_of("F10"), "F10");
    }
}
