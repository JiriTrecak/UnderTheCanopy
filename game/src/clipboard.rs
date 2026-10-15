//! The system clipboard for the UI's text fields (old: the browser's own copy and
//! paste in inputs). Failures (no clipboard, a headless run) are logged and ignored.

use std::cell::RefCell;

thread_local! {
    static CLIPBOARD: RefCell<Option<arboard::Clipboard>> = const { RefCell::new(None) };
}

fn with<T>(f: impl FnOnce(&mut arboard::Clipboard) -> Result<T, arboard::Error>) -> Option<T> {
    CLIPBOARD.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            match arboard::Clipboard::new() {
                Ok(c) => *slot = Some(c),
                Err(error) => {
                    tracing::warn!(%error, "no clipboard");
                    return None;
                }
            }
        }
        match f(slot.as_mut()?) {
            Ok(v) => Some(v),
            Err(error) => {
                tracing::debug!(%error, "clipboard");
                None
            }
        }
    })
}

/// Puts text on the clipboard.
pub fn set(text: &str) {
    with(|c| c.set_text(text.to_owned()));
}

/// The clipboard's text, if any.
pub fn get() -> Option<String> {
    with(|c| c.get_text())
}
