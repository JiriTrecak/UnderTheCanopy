//! winit window events → UI input (feature `winit`).
//!
//! Typed text comes from `KeyEvent::text` and IME commits, so dead keys and input
//! methods work; hotkeys use the logical key, so they follow the keyboard layout.

use crate::input::{InputEvent, Key, Modifiers, PointerButton};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{Key as WKey, NamedKey};

/// Pixels per wheel line when a trackpad reports pixel deltas.
const PIXELS_PER_LINE: f32 = 40.0;

/// Translates a window event into zero or more UI events.
pub fn translate(event: &WindowEvent) -> Vec<InputEvent> {
    match event {
        WindowEvent::CursorMoved { position, .. } => vec![InputEvent::PointerMoved {
            x: position.x as f32,
            y: position.y as f32,
        }],
        WindowEvent::CursorLeft { .. } => vec![InputEvent::PointerLeft],
        WindowEvent::MouseInput { state, button, .. } => {
            let button = match button {
                MouseButton::Left => PointerButton::Primary,
                MouseButton::Right => PointerButton::Secondary,
                MouseButton::Middle => PointerButton::Middle,
                _ => return Vec::new(),
            };
            vec![InputEvent::PointerButton {
                button,
                pressed: *state == ElementState::Pressed,
            }]
        }
        WindowEvent::MouseWheel { delta, .. } => {
            let (x, y) = match delta {
                MouseScrollDelta::LineDelta(x, y) => (*x, *y),
                MouseScrollDelta::PixelDelta(p) => {
                    (p.x as f32 / PIXELS_PER_LINE, p.y as f32 / PIXELS_PER_LINE)
                }
            };
            vec![InputEvent::Wheel { x, y }]
        }
        WindowEvent::ModifiersChanged(m) => {
            let s = m.state();
            vec![InputEvent::Modifiers(Modifiers {
                shift: s.shift_key(),
                ctrl: s.control_key(),
                alt: s.alt_key(),
                logo: s.super_key(),
            })]
        }
        WindowEvent::KeyboardInput { event, .. } => {
            let pressed = event.state == ElementState::Pressed;
            let mut out = Vec::new();
            if let Some(key) = key(&event.logical_key) {
                out.push(InputEvent::Key {
                    key,
                    pressed,
                    repeat: event.repeat,
                });
            }
            if pressed
                && let Some(text) = &event.text
                && text.chars().any(|c| !c.is_control())
            {
                out.push(InputEvent::Text(text.to_string()));
            }
            out
        }
        WindowEvent::Ime(ime) => match ime {
            Ime::Preedit(text, _) => vec![InputEvent::ImePreedit(text.clone())],
            Ime::Commit(text) => vec![InputEvent::ImeCommit(text.clone())],
            Ime::Enabled | Ime::Disabled => Vec::new(),
        },
        WindowEvent::Focused(false) => vec![InputEvent::FocusLost],
        _ => Vec::new(),
    }
}

fn key(k: &WKey) -> Option<Key> {
    Some(match k {
        WKey::Named(n) => match n {
            NamedKey::Enter => Key::Enter,
            NamedKey::Escape => Key::Escape,
            NamedKey::Tab => Key::Tab,
            NamedKey::Backspace => Key::Backspace,
            NamedKey::Delete => Key::Delete,
            NamedKey::ArrowLeft => Key::Left,
            NamedKey::ArrowRight => Key::Right,
            NamedKey::ArrowUp => Key::Up,
            NamedKey::ArrowDown => Key::Down,
            NamedKey::Home => Key::Home,
            NamedKey::End => Key::End,
            NamedKey::PageUp => Key::PageUp,
            NamedKey::PageDown => Key::PageDown,
            NamedKey::Space => Key::Space,
            NamedKey::F1 => Key::F(1),
            NamedKey::F2 => Key::F(2),
            NamedKey::F3 => Key::F(3),
            NamedKey::F4 => Key::F(4),
            NamedKey::F5 => Key::F(5),
            NamedKey::F6 => Key::F(6),
            NamedKey::F7 => Key::F(7),
            NamedKey::F8 => Key::F(8),
            NamedKey::F9 => Key::F(9),
            NamedKey::F10 => Key::F(10),
            NamedKey::F11 => Key::F(11),
            NamedKey::F12 => Key::F(12),
            _ => return None,
        },
        WKey::Character(s) => {
            let c = s.chars().next()?;
            if c == ' ' {
                Key::Space
            } else {
                Key::Char(c.to_uppercase().next()?)
            }
        }
        _ => return None,
    })
}
