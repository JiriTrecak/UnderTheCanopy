//! The shell's side of the asset editor (docs/editor.md, M4): importing art and cooking
//! the pack from the editor.

use super::*;

impl Shell {
    /// Copies art in as a new asset folder; the libraries read it at once.
    pub(super) fn import_asset(
        &mut self,
        request: &crate::assets::Import,
    ) -> Result<String, String> {
        let root = self.package.as_ref().map_err(Clone::clone)?.root.clone();
        let document = crate::assets::import(&root, request)?;
        // The folder appeared on disk: read the libraries again.
        self.structure = None;
        if let Ok(package) = self.package.clone() {
            self.refresh_libraries(&package);
        }
        self.checked = None;
        Ok(document)
    }

    /// Starts cooking the changed assets into the pack (`canopy-cook`, as `just cook`
    /// does), unless a cook is running.
    pub(super) fn start_cook(&mut self) {
        if self.cooking.is_some() {
            return;
        }
        let Some(data) = self.data.clone() else {
            self.say("The editor has no data folder to cook.".into(), true);
            return;
        };
        self.say("Cooking the changed assets…".into(), false);
        self.cooking = Some(std::thread::spawn(move || {
            let output = std::process::Command::new("cargo")
                .args(["run", "-q", "-p", "canopy-cook", "--", "--data"])
                .arg(&data)
                .arg("--out")
                .arg(data.join("target/cooked"))
                .current_dir(&data)
                .output()
                .map_err(|e| format!("cannot run the cooker: {e}"))?;
            let text = format!(
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            if output.status.success() {
                // The cooker's summary line ("cook: 3 of 364 jobs cooked, …").
                Ok(text
                    .lines()
                    .rev()
                    .find(|l| l.starts_with("cook:"))
                    .map_or("the pack is up to date", |l| {
                        l.trim_start_matches("cook:").trim()
                    })
                    .to_string())
            } else {
                let error = String::from_utf8_lossy(&output.stderr);
                let last: Vec<&str> = error.lines().rev().take(6).collect();
                Err(format!(
                    "The cook failed: {}",
                    last.into_iter().rev().collect::<Vec<_>>().join("\n")
                ))
            }
        }));
    }

    /// Takes in a finished cook: the viewports and map scenes open the new pack.
    pub(super) fn poll_cook(&mut self) {
        if !self.cooking.as_ref().is_some_and(|c| c.is_finished()) {
            return;
        }
        let result = self
            .cooking
            .take()
            .expect("a cook")
            .join()
            .unwrap_or_else(|_| Err("the cook panicked".into()));
        match &result {
            Ok(summary) => {
                self.reload_pack = true;
                self.resources = None;
                self.clips = None;
                for session in self.maps.values_mut() {
                    session.scene = None;
                }
                self.say(format!("Cooked: {summary}"), false);
            }
            Err(error) => self.say(error.clone(), true),
        }
        let answer = result.map(|summary| serde_json::json!({ "cooked": summary }));
        for reply in self.cook_waiting.drain(..) {
            let _ = reply.send(answer.clone());
        }
    }
}
