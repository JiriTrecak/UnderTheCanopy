//! Input: pointer, keyboard, text and IME events; hover, press, drag, focus, hotkeys
//! and tooltips.
//!
//! Hit testing uses the regions recorded by the last paint, so what the player sees is
//! what they click. The UI consumes a pointer event when it lands on a node with
//! `pointer-events: auto` (or while a press that began on one is held), so clicks never
//! fall through to the world.

use crate::config::Kind;
use crate::geom::Vec2;
use crate::node::{Tree, select};
use crate::paint::{Hit, HitPart, scrollbar};
use crate::{Action, ActionKind, Tooltip, Ui};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// Command on macOS, the Windows key elsewhere.
    pub logo: bool,
}

impl Modifiers {
    /// The platform's shortcut modifier (Command or Control).
    fn command(&self) -> bool {
        self.ctrl || self.logo
    }
}

/// Keys the UI understands. Characters are upper case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Space,
    F(u8),
}

impl Key {
    pub fn parse(name: &str) -> Option<Key> {
        Some(match name.to_ascii_lowercase().as_str() {
            "enter" | "return" => Key::Enter,
            "escape" | "esc" => Key::Escape,
            "tab" => Key::Tab,
            "backspace" => Key::Backspace,
            "delete" => Key::Delete,
            "left" => Key::Left,
            "right" => Key::Right,
            "up" => Key::Up,
            "down" => Key::Down,
            "home" => Key::Home,
            "end" => Key::End,
            "pageup" => Key::PageUp,
            "pagedown" => Key::PageDown,
            "space" => Key::Space,
            f if f.len() > 1 && f.starts_with('f') && f[1..].parse::<u8>().is_ok() => {
                Key::F(f[1..].parse().ok()?)
            }
            _ => {
                let mut chars = name.chars();
                let c = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                Key::Char(c.to_uppercase().next()?)
            }
        })
    }
}

/// A key with modifiers, as written in layouts (`"Q"`, `"Shift+F1"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hotkey {
    pub key: Key,
    pub mods: Modifiers,
}

impl Hotkey {
    pub fn parse(text: &str) -> Option<Hotkey> {
        let mut mods = Modifiers::default();
        let mut key = None;
        for part in text.split('+') {
            match part.trim().to_ascii_lowercase().as_str() {
                "shift" => mods.shift = true,
                "ctrl" | "control" => mods.ctrl = true,
                "alt" | "option" => mods.alt = true,
                "cmd" | "command" | "logo" | "super" => mods.logo = true,
                _ => key = Some(Key::parse(part.trim())?),
            }
        }
        Some(Hotkey { key: key?, mods })
    }

    /// Shift held with a plain hotkey still triggers it (and is reported), so
    /// shift-queued commands work.
    fn matches(&self, key: Key, m: Modifiers) -> bool {
        self.key == key
            && self.mods.ctrl == m.ctrl
            && self.mods.alt == m.alt
            && self.mods.logo == m.logo
            && (!self.mods.shift || m.shift)
    }
}

/// An input event. Positions are physical pixels within the window.
#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    PointerMoved {
        x: f32,
        y: f32,
    },
    PointerLeft,
    PointerButton {
        button: PointerButton,
        pressed: bool,
    },
    /// Wheel lines; positive `y` scrolls content towards its start.
    Wheel {
        x: f32,
        y: f32,
    },
    Key {
        key: Key,
        pressed: bool,
        repeat: bool,
    },
    Modifiers(Modifiers),
    /// Typed text (not for control keys).
    Text(String),
    /// IME composition in progress; empty text ends it.
    ImePreedit(String),
    ImeCommit(String),
    /// The window lost focus.
    FocusLost,
}

/// The mouse cursor the UI wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    Default,
    Pointer,
    Text,
    /// Over a draggable node (`drag: true`, the minimap).
    Grab,
    /// Dragging such a node.
    Grabbing,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Pressed {
    pub node: usize,
    pub button: PointerButton,
    pub part: HitPart,
    pub start: Vec2,
    pub scroll_start: f32,
    pub drag: bool,
    /// Pressed on a non-interactive or disabled part of the UI: held only so the
    /// release does not reach the world.
    pub inert: bool,
}

#[derive(Debug, Default)]
pub(crate) struct InputState {
    pub pointer: Option<Vec2>,
    pub hover: Option<usize>,
    pub hover_owner: Option<usize>,
    pub hover_part: Option<(usize, HitPart)>,
    pub pressed: Option<Pressed>,
    pub focus: Option<usize>,
    pub focus_visible: bool,
    pub modifiers: Modifiers,
    last_click: Option<(usize, f64, u8)>,
    caret_since: f64,
    open_dropdown: Option<usize>,
    tip_target: Option<usize>,
    tip_since: f64,
    tip_visible_at: f64,
    pub tip_shown: Option<(usize, Arc<Tooltip>)>,
}

impl InputState {
    /// Forgets node references (after a rebuild).
    pub fn reset(&mut self) {
        let modifiers = self.modifiers;
        let pointer = self.pointer;
        *self = InputState {
            modifiers,
            pointer,
            tip_visible_at: f64::NEG_INFINITY,
            ..InputState::default()
        };
    }

    /// Whether any state refers into a subtree about to be removed.
    pub fn involves(&self, tree: &Tree, root: usize) -> bool {
        let mut sub = Vec::new();
        tree.subtree(root, &mut sub);
        [
            self.hover,
            self.hover_owner,
            self.focus,
            self.pressed.map(|p| p.node),
            self.open_dropdown,
            self.tip_target,
        ]
        .into_iter()
        .flatten()
        .any(|n| sub.contains(&n))
    }

    pub fn caret_visible(&self, time: f64) -> bool {
        ((time - self.caret_since) / 0.53).floor() as i64 % 2 == 0
    }
}

fn is_ancestor(tree: &Tree, ancestor: usize, mut node: usize) -> bool {
    loop {
        if node == ancestor {
            return true;
        }
        match tree.node(node).parent {
            Some(p) => node = p,
            None => return false,
        }
    }
}

/// Nearest enabled interactive ancestor-or-self.
fn target(tree: &Tree, mut node: usize) -> Option<usize> {
    loop {
        let n = tree.node(node);
        if n.spec.interactive() {
            return n.enabled.then_some(node);
        }
        node = n.parent?;
    }
}

impl Ui {
    fn hit_at(&self, p: Vec2) -> Option<Hit> {
        self.hits.iter().rev().find(|h| h.contains(p)).copied()
    }

    fn action(&mut self, node: usize, kind: ActionKind) {
        let n = self.tree.node(node);
        let mut index = None;
        let mut cur = Some(node);
        while let Some(c) = cur {
            let cn = self.tree.node(c);
            if cn.repeat_index.is_some() {
                index = cn.repeat_index;
                break;
            }
            cur = cn.parent;
        }
        self.actions.push(Action {
            node: n.handle.map(crate::NodeId),
            path: n.path.clone(),
            name: n.spec.action.clone(),
            index,
            kind,
        });
    }

    /// Feeds one event; returns whether the UI consumed it.
    pub fn input(&mut self, event: &InputEvent) -> bool {
        match event {
            InputEvent::PointerMoved { x, y } => self.pointer_moved(Vec2::new(*x, *y)),
            InputEvent::PointerLeft => {
                self.input.pointer = None;
                self.input.hover = None;
                self.input.hover_owner = None;
                self.input.hover_part = None;
                false
            }
            InputEvent::PointerButton { button, pressed } => {
                if *pressed {
                    self.pointer_pressed(*button)
                } else {
                    self.pointer_released(*button)
                }
            }
            InputEvent::Wheel { y, .. } => self.wheel(*y),
            InputEvent::Key {
                key,
                pressed,
                repeat,
            } => {
                if *pressed {
                    self.key(*key, *repeat)
                } else {
                    self.focused_field().is_some()
                }
            }
            InputEvent::Modifiers(m) => {
                self.input.modifiers = *m;
                false
            }
            InputEvent::Text(text) => match self.focused_field() {
                Some(f) => {
                    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
                    if !clean.is_empty() {
                        self.edit(f, Edit::Insert(&clean));
                    }
                    true
                }
                None => false,
            },
            InputEvent::ImePreedit(text) => match self.focused_field() {
                Some(f) => {
                    self.tree.node_mut(f).field.preedit = text.clone();
                    self.show_field_text(f);
                    true
                }
                None => false,
            },
            InputEvent::ImeCommit(text) => match self.focused_field() {
                Some(f) => {
                    self.tree.node_mut(f).field.preedit.clear();
                    self.edit(f, Edit::Insert(text));
                    true
                }
                None => false,
            },
            InputEvent::FocusLost => {
                self.input.pressed = None;
                self.input.modifiers = Modifiers::default();
                false
            }
        }
    }

    /// Whether the pointer is over the UI or a press began on it: the world should
    /// ignore the pointer.
    pub fn wants_pointer(&self) -> bool {
        self.input.pressed.is_some() || self.input.pointer.is_some_and(|p| self.hit_at(p).is_some())
    }

    /// Whether a text field has focus: the game should not treat keys as hotkeys.
    pub fn wants_keyboard(&self) -> bool {
        self.focused_field().is_some()
    }

    /// While a text field has focus: the caret rectangle (physical pixels), for
    /// `Window::set_ime_cursor_area`. `None` means IME should be disabled.
    pub fn ime_area(&self) -> Option<crate::Rect> {
        let f = self.focused_field()?;
        let n = self.tree.node(f);
        let block = n.text.as_ref()?;
        let x = if block.is_empty() {
            0.0
        } else {
            block.caret_x(n.field.cursor)
        };
        let lh = n.text_style.line_height_px() * n.scale;
        Some(crate::Rect::new(
            n.content.x + x - n.field.scroll,
            n.content.y + (n.content.h - lh) * 0.5,
            1.0,
            lh,
        ))
    }

    pub fn cursor(&self) -> Cursor {
        if let Some(pr) = &self.input.pressed
            && pr.drag
        {
            return Cursor::Grabbing;
        }
        let Some(h) = self.input.hover else {
            return Cursor::Default;
        };
        if let Some(t) = target(&self.tree, h) {
            return match self.tree.node(t).kind() {
                Kind::TextField => Cursor::Text,
                _ if self.tree.node(t).spec.drag => Cursor::Grab,
                _ => Cursor::Pointer,
            };
        }
        Cursor::Default
    }

    /// Gives keyboard focus to a node (menus start keyboard navigation this way).
    pub fn focus(&mut self, id: Option<crate::NodeId>) {
        let node = id.and_then(|i| self.tree.node_of(i));
        self.set_focus(node, true);
    }

    /// Answers [`ActionKind::PasteRequest`]: inserts text into the focused field.
    pub fn paste(&mut self, text: &str) {
        if let Some(f) = self.focused_field() {
            let line: String = text.lines().next().unwrap_or("").to_owned();
            self.edit(f, Edit::Insert(&line));
        }
    }

    fn set_focus(&mut self, node: Option<usize>, visible: bool) {
        if self.input.focus != node {
            if let Some(old) = self.input.focus
                && self.tree.nodes.get(old).is_some_and(Option::is_some)
            {
                let f = &mut self.tree.node_mut(old).field;
                f.preedit.clear();
                f.anchor = f.cursor;
            }
            self.input.focus = node;
            self.input.caret_since = self.time;
        }
        self.input.focus_visible = visible;
    }

    fn focused_field(&self) -> Option<usize> {
        self.input
            .focus
            .filter(|&f| self.tree.node(f).kind() == Kind::TextField)
    }

    fn pointer_moved(&mut self, p: Vec2) -> bool {
        self.input.pointer = Some(p);
        let hit = self.hit_at(p);
        self.input.hover = hit.map(|h| h.node);
        self.input.hover_part = hit.map(|h| (h.node, h.part));
        self.input.hover_owner = hit.and_then(|h| self.tree.node(h.node).owner);
        if let Some(pr) = self.input.pressed.filter(|p| !p.inert) {
            let n = pr.node;
            match (pr.part, self.tree.node(n).kind()) {
                (HitPart::ScrollThumb, _) => {
                    let node = self.tree.node(n);
                    if let Some((track, thumb)) = scrollbar(node) {
                        let travel = (track.h - thumb.h).max(1.0);
                        let dy = (p.y - pr.start.y) / travel * node.scroll_max.y;
                        self.tree.node_mut(n).scroll.y =
                            (pr.scroll_start + dy).clamp(0.0, node.scroll_max.y);
                    }
                }
                (_, Kind::Slider) => self.slide(n, p),
                (_, Kind::TextField) => {
                    let i = self.field_index(n, p);
                    let f = &mut self.tree.node_mut(n).field;
                    f.cursor = i;
                    self.input.caret_since = self.time;
                }
                _ => {}
            }
            if pr.drag {
                let at = self.local(n, p);
                self.action(n, ActionKind::Drag { at });
            }
            return true;
        }
        hit.is_some()
    }

    fn local(&self, n: usize, p: Vec2) -> Vec2 {
        let r = self.tree.node(n).rect;
        Vec2::new(
            ((p.x - r.x) / r.w.max(1.0)).clamp(0.0, 1.0),
            ((p.y - r.y) / r.h.max(1.0)).clamp(0.0, 1.0),
        )
    }

    fn close_dropdown(&mut self) -> bool {
        match self.input.open_dropdown.take() {
            Some(d) => {
                if self.tree.nodes.get(d).is_some_and(Option::is_some) {
                    self.tree.node_mut(d).open = false;
                }
                true
            }
            None => false,
        }
    }

    fn pointer_pressed(&mut self, button: PointerButton) -> bool {
        let Some(p) = self.input.pointer else {
            return false;
        };
        let hit = self.hit_at(p);
        if let Some(open) = self.input.open_dropdown
            && hit.is_none_or(|h| h.node != open)
        {
            self.close_dropdown();
            return true;
        }
        let Some(hit) = hit else {
            if self.focused_field().is_some() {
                self.set_focus(None, false);
            }
            return false;
        };
        match hit.part {
            HitPart::Option(i) => {
                select(&mut self.tree, hit.node, i);
                self.close_dropdown();
                self.action(hit.node, ActionKind::Selected(i));
                return true;
            }
            HitPart::ScrollThumb => {
                self.input.pressed = Some(Pressed {
                    node: hit.node,
                    button,
                    part: hit.part,
                    start: p,
                    scroll_start: self.tree.node(hit.node).scroll.y,
                    drag: false,
                    inert: false,
                });
                return true;
            }
            HitPart::ScrollTrack => {
                let node = self.tree.node_mut(hit.node);
                let page = node.rect.h / node.scale * 0.9;
                if let Some((_, thumb)) = scrollbar(node) {
                    let dir = if p.y < thumb.y { -1.0 } else { 1.0 };
                    node.scroll.y = (node.scroll.y + dir * page).clamp(0.0, node.scroll_max.y);
                }
                return true;
            }
            HitPart::Node => {}
        }
        let Some(t) = target(&self.tree, hit.node) else {
            if self.focused_field().is_some() {
                self.set_focus(None, false);
            }
            self.input.pressed = Some(Pressed {
                node: hit.node,
                button,
                part: HitPart::Node,
                start: p,
                scroll_start: 0.0,
                drag: false,
                inert: true,
            });
            return true;
        };
        if button == PointerButton::Primary && self.tree.node(t).spec.focusable() {
            self.set_focus(Some(t), false);
        } else if self.focused_field().is_some() {
            self.set_focus(None, false);
        }
        let drag = self.tree.node(t).spec.drag;
        self.input.pressed = Some(Pressed {
            node: t,
            button,
            part: HitPart::Node,
            start: p,
            scroll_start: 0.0,
            drag,
            inert: false,
        });
        match self.tree.node(t).kind() {
            Kind::Slider if button == PointerButton::Primary => self.slide(t, p),
            Kind::TextField => {
                let i = self.field_index(t, p);
                let shift = self.input.modifiers.shift;
                let f = &mut self.tree.node_mut(t).field;
                f.cursor = i;
                if !shift {
                    f.anchor = i;
                }
                self.input.caret_since = self.time;
            }
            _ => {}
        }
        if drag {
            let at = self.local(t, p);
            self.action(t, ActionKind::DragStart { button, at });
        }
        true
    }

    fn pointer_released(&mut self, button: PointerButton) -> bool {
        let Some(pr) = self.input.pressed else {
            return false;
        };
        if pr.button != button {
            return true;
        }
        self.input.pressed = None;
        let n = pr.node;
        if self.tree.nodes.get(n).is_none_or(Option::is_none) {
            return true;
        }
        let p = self.input.pointer.unwrap_or(pr.start);
        if pr.drag {
            let at = self.local(n, p);
            self.action(n, ActionKind::DragEnd { at });
            return true;
        }
        if pr.part != HitPart::Node || pr.inert {
            return true;
        }
        let over = self
            .hit_at(p)
            .is_some_and(|h| is_ancestor(&self.tree, n, h.node));
        if !over {
            return true;
        }
        self.activate(n, button);
        true
    }

    /// A click (or Enter/Space) on an interactive node.
    fn activate(&mut self, n: usize, button: PointerButton) {
        let modifiers = self.input.modifiers;
        let count = match self.input.last_click {
            Some((last, t, c)) if last == n && self.time - t < 0.4 => c.saturating_add(1),
            _ => 1,
        };
        self.input.last_click = Some((n, self.time, count));
        match self.tree.node(n).kind() {
            Kind::Checkbox if button == PointerButton::Primary => {
                let node = self.tree.node_mut(n);
                node.checked = !node.checked;
                node.set.checked = Some(node.checked);
                let v = node.checked;
                self.action(n, ActionKind::Toggled(v));
            }
            Kind::Dropdown if button == PointerButton::Primary => {
                let was = self.tree.node(n).open;
                self.close_dropdown();
                if !was {
                    self.tree.node_mut(n).open = true;
                    self.input.open_dropdown = Some(n);
                }
            }
            Kind::Slider | Kind::TextField => {}
            _ => {
                if self.tree.node(n).spec.toggle && button == PointerButton::Primary {
                    let node = self.tree.node_mut(n);
                    node.checked = !node.checked;
                    node.set.checked = Some(node.checked);
                    let v = node.checked;
                    self.action(n, ActionKind::Toggled(v));
                }
                self.action(
                    n,
                    ActionKind::Click {
                        button,
                        modifiers,
                        count,
                    },
                );
            }
        }
    }

    fn slide(&mut self, n: usize, p: Vec2) {
        let node = self.tree.node(n);
        let c = node.content;
        let t = ((p.x - c.x) / c.w.max(1.0)).clamp(0.0, 1.0);
        let spec = &node.spec;
        let mut v = spec.min + t * (spec.max - spec.min);
        if spec.step > 0.0 {
            v = spec.min + ((v - spec.min) / spec.step).round() * spec.step;
        }
        self.set_slider(n, v);
    }

    fn set_slider(&mut self, n: usize, v: f32) {
        let node = self.tree.node_mut(n);
        let v = v.clamp(
            node.spec.min.min(node.spec.max),
            node.spec.max.max(node.spec.min),
        );
        if (node.value - v).abs() > f32::EPSILON {
            node.value = v;
            node.set.value = Some(v);
            self.action(n, ActionKind::Value(v));
        }
    }

    fn wheel(&mut self, lines: f32) -> bool {
        let Some(p) = self.input.pointer else {
            return false;
        };
        let Some(hit) = self.hit_at(p) else {
            return false;
        };
        let mut cur = Some(hit.node);
        while let Some(c) = cur {
            let n = self.tree.node(c);
            if n.spec.style.scroll && n.scroll_max.y > 0.0 {
                let step = self.config.scroll_line;
                let node = self.tree.node_mut(c);
                node.scroll.y = (node.scroll.y - lines * step).clamp(0.0, node.scroll_max.y);
                break;
            }
            cur = n.parent;
        }
        true
    }

    fn key(&mut self, key: Key, repeat: bool) -> bool {
        let m = self.input.modifiers;
        if let Some(f) = self.focused_field() {
            return self.field_key(f, key, m);
        }
        if key == Key::Escape {
            if self.close_dropdown() {
                return true;
            }
            if self.input.focus.is_some() {
                self.set_focus(None, false);
                return true;
            }
            if self.input.tip_shown.is_some() {
                self.input.tip_target = None;
                self.input.tip_since = f64::INFINITY;
                return true;
            }
            return false;
        }
        if let Some(open) = self.input.open_dropdown {
            let node = self.tree.node(open);
            let sel = node.selected;
            let count = node.options.len();
            match key {
                Key::Up | Key::Down if count > 0 => {
                    let next = if key == Key::Up {
                        sel.saturating_sub(1)
                    } else {
                        (sel + 1).min(count - 1)
                    };
                    select(&mut self.tree, open, next);
                    self.action(open, ActionKind::Selected(next));
                    return true;
                }
                Key::Enter | Key::Space => {
                    self.close_dropdown();
                    return true;
                }
                _ => {}
            }
        }
        if let Some(f) = self.input.focus {
            let kind = self.tree.node(f).kind();
            match key {
                Key::Tab => {
                    self.focus_next(f, m.shift);
                    return true;
                }
                Key::Enter | Key::Space if !repeat => {
                    self.activate(f, PointerButton::Primary);
                    return true;
                }
                Key::Left | Key::Right | Key::Up | Key::Down if kind == Kind::Slider => {
                    let node = self.tree.node(f);
                    let step = if node.spec.step > 0.0 {
                        node.spec.step
                    } else {
                        (node.spec.max - node.spec.min) / 20.0
                    };
                    let dir = if matches!(key, Key::Left | Key::Down) {
                        -1.0
                    } else {
                        1.0
                    };
                    let v = node.value + dir * step;
                    self.set_slider(f, v);
                    self.input.focus_visible = true;
                    return true;
                }
                Key::Up | Key::Down if kind == Kind::Dropdown => {
                    let node = self.tree.node(f);
                    let count = node.options.len();
                    if count > 0 {
                        let next = if key == Key::Up {
                            node.selected.saturating_sub(1)
                        } else {
                            (node.selected + 1).min(count - 1)
                        };
                        select(&mut self.tree, f, next);
                        self.action(f, ActionKind::Selected(next));
                    }
                    return true;
                }
                _ => {}
            }
        }
        if repeat {
            return false;
        }
        // Hotkeys: topmost painted node first.
        let found = self.hits.iter().rev().find_map(|h| {
            let n = self.tree.node(h.node);
            (h.part == HitPart::Node && n.enabled && n.hotkey.is_some_and(|k| k.matches(key, m)))
                .then_some(h.node)
        });
        match found {
            Some(n) => {
                self.action(n, ActionKind::Hotkey { modifiers: m });
                true
            }
            None => false,
        }
    }

    fn focus_next(&mut self, from: usize, back: bool) {
        let mut order: Vec<usize> = Vec::new();
        for h in &self.hits {
            let n = self.tree.node(h.node);
            if h.part == HitPart::Node
                && n.spec.focusable()
                && n.enabled
                && !order.contains(&h.node)
            {
                order.push(h.node);
            }
        }
        if order.is_empty() {
            return;
        }
        let i = order.iter().position(|&n| n == from);
        let next = match (i, back) {
            (Some(i), false) => order[(i + 1) % order.len()],
            (Some(i), true) => order[(i + order.len() - 1) % order.len()],
            (None, _) => order[0],
        };
        self.set_focus(Some(next), true);
    }

    // --- Text fields ---------------------------------------------------------------

    fn field_index(&self, n: usize, p: Vec2) -> usize {
        let node = self.tree.node(n);
        match &node.text {
            Some(t) if !t.is_empty() => t.hit_index(p.x - node.content.x + node.field.scroll),
            _ => 0,
        }
    }

    /// Shows the field's text with any IME composition inserted at the caret.
    fn show_field_text(&mut self, f: usize) {
        let base = self.field_text(f);
        let node = self.tree.node_mut(f);
        let shown = if node.field.preedit.is_empty() {
            base
        } else {
            let c = node.field.cursor.min(base.len());
            format!("{}{}{}", &base[..c], node.field.preedit, &base[c..])
        };
        if let Some(t) = &mut node.text {
            t.set_markup(&shown);
        }
        self.input.caret_since = self.time;
    }

    fn field_key(&mut self, f: usize, key: Key, m: Modifiers) -> bool {
        if !self.tree.node(f).field.preedit.is_empty() {
            // The IME owns keys while composing.
            return true;
        }
        let text = self.field_text(f);
        let (cursor, anchor) = {
            let fs = &self.tree.node(f).field;
            (fs.cursor.min(text.len()), fs.anchor.min(text.len()))
        };
        let prev = |i: usize| text[..i].char_indices().next_back().map_or(0, |(j, _)| j);
        let next = |i: usize| text[i..].chars().next().map_or(i, |c| i + c.len_utf8());
        let move_to = |ui: &mut Ui, to: usize| {
            let fs = &mut ui.tree.node_mut(f).field;
            fs.cursor = to;
            if !m.shift {
                fs.anchor = to;
            }
            ui.input.caret_since = ui.time;
        };
        match key {
            Key::Escape => {
                self.set_focus(None, false);
            }
            Key::Tab => self.focus_next(f, m.shift),
            Key::Enter => self.action(f, ActionKind::Submitted(text.clone())),
            Key::Left => {
                let to = if cursor != anchor && !m.shift {
                    cursor.min(anchor)
                } else {
                    prev(cursor)
                };
                move_to(self, to);
            }
            Key::Right => {
                let to = if cursor != anchor && !m.shift {
                    cursor.max(anchor)
                } else {
                    next(cursor)
                };
                move_to(self, to);
            }
            Key::Home | Key::Up => move_to(self, 0),
            Key::End | Key::Down => move_to(self, text.len()),
            Key::Backspace => self.edit(f, Edit::Back),
            Key::Delete => self.edit(f, Edit::Forward),
            Key::Char('A') if m.command() => {
                let fs = &mut self.tree.node_mut(f).field;
                fs.anchor = 0;
                fs.cursor = text.len();
            }
            Key::Char('C') | Key::Char('X') if m.command() => {
                let (a, b) = (cursor.min(anchor), cursor.max(anchor));
                if a != b {
                    self.action(f, ActionKind::Copy(text[a..b].to_owned()));
                    if key == Key::Char('X') {
                        self.edit(f, Edit::Insert(""));
                    }
                }
            }
            Key::Char('V') if m.command() => self.action(f, ActionKind::PasteRequest),
            _ => {}
        }
        true
    }

    /// A field's text without any IME composition.
    fn field_text(&self, f: usize) -> String {
        let n = self.tree.node(f);
        n.set
            .text
            .clone()
            .or_else(|| n.spec.text.clone())
            .unwrap_or_default()
    }

    fn edit(&mut self, f: usize, edit: Edit) {
        let mut text = self.field_text(f);
        let max = self.tree.node(f).spec.max_length;
        let fs = self.tree.node(f).field.clone();
        let (mut a, mut b) = (
            fs.cursor.min(fs.anchor).min(text.len()),
            fs.cursor.max(fs.anchor).min(text.len()),
        );
        if a == b {
            match edit {
                Edit::Back => a = text[..a].char_indices().next_back().map_or(0, |(j, _)| j),
                Edit::Forward => b = text[b..].chars().next().map_or(b, |c| b + c.len_utf8()),
                Edit::Insert(_) => {}
            }
        }
        let insert = match edit {
            Edit::Insert(s) => {
                let room = max.saturating_sub(text.chars().count() - text[a..b].chars().count());
                s.chars().take(room).collect::<String>()
            }
            _ => String::new(),
        };
        text.replace_range(a..b, &insert);
        let caret = a + insert.len();
        {
            let node = self.tree.node_mut(f);
            node.field.cursor = caret;
            node.field.anchor = caret;
        }
        self.tree.set_text(f, &text);
        self.input.caret_since = self.time;
        self.action(f, ActionKind::TextChanged(text));
    }

    // --- Tooltips ----------------------------------------------------------------------

    /// Chooses the tooltip to show and fills the tooltip template.
    pub(crate) fn update_tooltip(&mut self) {
        let Some(tip) = self.tree.tooltip_root else {
            return;
        };
        let target = self.input.hover_owner.filter(|&o| {
            self.tooltips
                && self
                    .tree
                    .node(o)
                    .tooltip
                    .as_ref()
                    .is_some_and(|t| !t.title.is_empty() || !t.body.is_empty())
        });
        if target != self.input.tip_target {
            self.input.tip_target = target;
            self.input.tip_since = self.time;
        }
        let cfg = &self.config.tooltip;
        let delay = cfg.delay_ms as f64 / 1000.0;
        let warm = cfg.warm_ms as f64 / 1000.0;
        let show = target.filter(|_| {
            self.time - self.input.tip_since >= delay
                || self.time - self.input.tip_visible_at <= warm
        });
        let Some(t) = show else {
            if self.input.tip_shown.take().is_some() {
                self.tree.set_visible_raw(tip, false);
            }
            return;
        };
        self.input.tip_visible_at = self.time;
        let content = self.tree.node(t).tooltip.clone().expect("tooltip");
        let same = self
            .input
            .tip_shown
            .as_ref()
            .is_some_and(|(n, c)| *n == t && Arc::ptr_eq(c, &content));
        if same {
            return;
        }
        self.input.tip_shown = Some((t, content.clone()));
        self.tree.set_visible_raw(tip, true);
        // Width: the anchor's, or the theme's.
        let anchor = self.tooltip_anchor(t);
        let width = match anchor {
            Some(a) => self.tree.node(a).rect.w / self.dpi,
            None => self.config.tooltip.width,
        };
        let tt = self.tree.node(tip).taffy;
        let mut style = self.tree.taffy.style(tt).expect("style").clone();
        if style.size.width != taffy::style::Dimension::length(width) {
            style.size.width = taffy::style::Dimension::length(width);
            self.tree.taffy.set_style(tt, style).expect("style");
            self.tree.layout_dirty = true;
        }
        let set = |ui: &mut Ui, name: &str, text: Option<&str>| {
            if let Some(id) = ui.find(&format!("@tooltip/{name}")) {
                ui.set_visible(id, text.is_some_and(|t| !t.is_empty()));
                ui.set_text(id, text.unwrap_or(""));
            }
        };
        let title = match &content.hotkey {
            Some(k) => format!("{} ([key]{k}[/key])", content.title),
            None => content.title.clone(),
        };
        set(self, "title", Some(&title));
        set(self, "body", Some(&content.body));
        set(self, "note", content.note.as_deref());
        if let Some(costs) = self.find("@tooltip/costs") {
            self.set_count(costs, content.costs.len());
            self.set_visible(costs, !content.costs.is_empty());
            for (i, cost) in content.costs.iter().enumerate() {
                if let Some(entry) = self.child(costs, i) {
                    if let Some(icon) = self.find_in(entry, "icon") {
                        self.set_image(icon, cost.image);
                    }
                    if let Some(amount) = self.find_in(entry, "amount") {
                        self.set_text(amount, &cost.amount);
                    }
                }
            }
        }
    }

    fn tooltip_anchor(&self, t: usize) -> Option<usize> {
        let name = self.tree.node(t).spec.tooltip_anchor.as_deref()?;
        let mut cur = self.tree.node(t).parent;
        while let Some(c) = cur {
            let n = self.tree.node(c);
            if n.spec.id.as_deref() == Some(name) {
                return Some(c);
            }
            cur = n.parent;
        }
        None
    }

    /// Places the tooltip next to its target (called during geometry, when its size is known).
    pub(crate) fn position_tooltip(&mut self, tip: usize) {
        let Some((t, _)) = &self.input.tip_shown else {
            return;
        };
        let t = *t;
        if self.tree.nodes.get(t).is_none_or(Option::is_none) {
            return;
        }
        let layout = *self
            .tree
            .taffy
            .layout(self.tree.node(tip).taffy)
            .expect("layout");
        let dpi = self.dpi;
        let cfg = &self.config.tooltip;
        let (vw, vh) = (self.viewport[0] as f32 / dpi, self.viewport[1] as f32 / dpi);
        let (w, h) = (layout.size.width, layout.size.height);
        let target = self.tree.node(t).rect;
        let r = crate::Rect::new(
            target.x / dpi,
            target.y / dpi,
            target.w / dpi,
            target.h / dpi,
        );
        let (x, y) = match self.tooltip_anchor(t) {
            Some(a) => {
                let a = self.tree.node(a).rect;
                let a = crate::Rect::new(a.x / dpi, a.y / dpi, a.w / dpi, a.h / dpi);
                (a.x, (a.y - h - cfg.gap * 2.0).max(cfg.margin))
            }
            None => {
                let y = if r.y > h + cfg.gap * 3.0 + 2.0 {
                    r.y - h - cfg.gap * 3.0
                } else {
                    (r.bottom() + cfg.gap).min(vh - h - cfg.margin)
                };
                (r.right() - w, y)
            }
        };
        let x = x.min(vw - w - cfg.margin).max(cfg.margin);
        self.tree.node_mut(tip).offset = Vec2::new(x - layout.location.x, y - layout.location.y);
    }
}

enum Edit<'a> {
    Insert(&'a str),
    Back,
    Forward,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkeys_parse_and_match() {
        let q = Hotkey::parse("Q").unwrap();
        assert!(q.matches(Key::Char('Q'), Modifiers::default()));
        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        assert!(
            q.matches(Key::Char('Q'), shift),
            "shift-queue still triggers"
        );
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::default()
        };
        assert!(!q.matches(Key::Char('Q'), ctrl));
        let f1 = Hotkey::parse("Shift+F1").unwrap();
        assert_eq!(f1.key, Key::F(1));
        assert!(!f1.matches(Key::F(1), Modifiers::default()));
        assert!(f1.matches(Key::F(1), shift));
        assert_eq!(Key::parse("q"), Some(Key::Char('Q')));
        assert_eq!(Hotkey::parse("Ctrl+"), None);
    }
}
