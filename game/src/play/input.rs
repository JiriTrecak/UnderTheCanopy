//! World input events: what the match reacts to once the UI has declined an event.
//! Keys are physical (like the old game's `KeyboardEvent.code`) and named the way the
//! content writes hotkeys (`"A"`, `"1"`, `"F1"`, `"Escape"`), so bindings follow key
//! positions, not the keyboard layout.

use glam::Vec2;
use winit::{
    event::{ElementState, MouseButton, WindowEvent},
    keyboard::{KeyCode, PhysicalKey},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// Command on macOS.
    pub logo: bool,
}

impl Mods {
    /// Control group assignment: Control, or Command on macOS.
    pub fn command(&self) -> bool {
        self.ctrl || self.logo
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// Physical pixels.
    PointerMoved(Vec2),
    PointerLeft,
    Button {
        button: Button,
        pressed: bool,
    },
    Key {
        key: &'static str,
        pressed: bool,
        repeat: bool,
    },
    Modifiers(Mods),
    /// The window lost focus: held keys and drags end.
    FocusLost,
}

/// Every key name [`key_name`] produces.
pub const KEY_NAMES: &[&str] = &[
    "A",
    "B",
    "C",
    "D",
    "E",
    "F",
    "G",
    "H",
    "I",
    "J",
    "K",
    "L",
    "M",
    "N",
    "O",
    "P",
    "Q",
    "R",
    "S",
    "T",
    "U",
    "V",
    "W",
    "X",
    "Y",
    "Z",
    "0",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
    "Escape",
    "Tab",
    "Backspace",
    "Home",
    "Space",
    "Backquote",
    "Enter",
    "Numpad4",
    "Numpad5",
    "Numpad7",
    "Numpad8",
    "BracketLeft",
    "BracketRight",
    "Alt",
];

/// The content's name for a physical key.
pub fn key_name(code: KeyCode) -> Option<&'static str> {
    use KeyCode::*;
    Some(match code {
        KeyA => "A",
        KeyB => "B",
        KeyC => "C",
        KeyD => "D",
        KeyE => "E",
        KeyF => "F",
        KeyG => "G",
        KeyH => "H",
        KeyI => "I",
        KeyJ => "J",
        KeyK => "K",
        KeyL => "L",
        KeyM => "M",
        KeyN => "N",
        KeyO => "O",
        KeyP => "P",
        KeyQ => "Q",
        KeyR => "R",
        KeyS => "S",
        KeyT => "T",
        KeyU => "U",
        KeyV => "V",
        KeyW => "W",
        KeyX => "X",
        KeyY => "Y",
        KeyZ => "Z",
        Digit0 => "0",
        Digit1 => "1",
        Digit2 => "2",
        Digit3 => "3",
        Digit4 => "4",
        Digit5 => "5",
        Digit6 => "6",
        Digit7 => "7",
        Digit8 => "8",
        Digit9 => "9",
        F1 => "F1",
        F2 => "F2",
        F3 => "F3",
        F4 => "F4",
        F5 => "F5",
        F6 => "F6",
        F7 => "F7",
        F8 => "F8",
        F9 => "F9",
        F10 => "F10",
        F11 => "F11",
        F12 => "F12",
        Escape => "Escape",
        Tab => "Tab",
        Backspace => "Backspace",
        Home => "Home",
        Space => "Space",
        Backquote => "Backquote",
        Enter | NumpadEnter => "Enter",
        Numpad4 => "Numpad4",
        Numpad5 => "Numpad5",
        Numpad7 => "Numpad7",
        Numpad8 => "Numpad8",
        BracketLeft => "BracketLeft",
        BracketRight => "BracketRight",
        AltLeft | AltRight => "Alt",
        _ => return None,
    })
}

/// The world input of a window event.
pub fn translate(event: &WindowEvent) -> Option<Input> {
    Some(match event {
        WindowEvent::CursorMoved { position, .. } => {
            Input::PointerMoved(Vec2::new(position.x as f32, position.y as f32))
        }
        WindowEvent::CursorLeft { .. } => Input::PointerLeft,
        WindowEvent::MouseInput { state, button, .. } => Input::Button {
            button: match button {
                MouseButton::Left => Button::Left,
                MouseButton::Right => Button::Right,
                MouseButton::Middle => Button::Middle,
                _ => return None,
            },
            pressed: *state == ElementState::Pressed,
        },
        WindowEvent::ModifiersChanged(m) => {
            let s = m.state();
            Input::Modifiers(Mods {
                shift: s.shift_key(),
                ctrl: s.control_key(),
                alt: s.alt_key(),
                logo: s.super_key(),
            })
        }
        WindowEvent::KeyboardInput { event, .. } => {
            let PhysicalKey::Code(code) = event.physical_key else {
                return None;
            };
            Input::Key {
                key: key_name(code)?,
                pressed: event.state == ElementState::Pressed,
                repeat: event.repeat,
            }
        }
        WindowEvent::Focused(false) => Input::FocusLost,
        _ => return None,
    })
}
