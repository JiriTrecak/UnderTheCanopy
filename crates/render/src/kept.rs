//! GPU objects built once per device and kept for its life ([`crate::Gpu::kept`]): the
//! pipelines every world load builds again (a match, each editor viewport, every reload)
//! with the same descriptors. Explicit bind group layouts with the same entries are one
//! layout to the device, so pipelines built for one renderer's layouts draw with
//! another's bind groups.

use std::any::Any;
use std::sync::{Arc, Mutex};

/// One kept object by its name.
type Entry = (&'static str, Box<dyn Any + Send>);

/// The kept objects of one device, by name.
#[derive(Clone, Default)]
pub struct Kept {
    entries: Arc<Mutex<Vec<Entry>>>,
}

impl std::fmt::Debug for Kept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Kept")
    }
}

impl Kept {
    /// The objects `name`, built by `build` the first time (whose descriptors must depend
    /// on nothing but the device: what it returns is shared by every later caller).
    pub(crate) fn get<T: Clone + Send + 'static>(
        &self,
        name: &'static str,
        build: impl FnOnce() -> T,
    ) -> T {
        let found = |entries: &[Entry]| {
            entries
                .iter()
                .find(|(n, _)| *n == name)
                .and_then(|(_, value)| value.downcast_ref::<T>().cloned())
        };
        if let Some(value) = found(&self.entries.lock().unwrap_or_else(|e| e.into_inner())) {
            return value;
        }
        let _span = tracing::info_span!("kept_build", name).entered();
        // Built outside the lock: loads on several threads build different objects at once.
        let value = build();
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(first) = found(&entries) {
            return first;
        }
        entries.push((name, Box::new(value.clone())));
        value
    }
}
