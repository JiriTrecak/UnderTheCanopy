//! The feedback form (`layouts/feedback.json`, D59): over the main menu or the game menu.
//! It previews the screenshot taken as it opened, takes a category and a description
//! (required), and shows the report going out: sending, sent, or saved for later.

use super::{ui_host::UiHost, widgets};
use crate::feedback::{Attachments, Category, Delivery, Place, grab::Shot};
use canopy_render::Gpu;
use canopy_ui::{ImageKey, Ui};

/// Where the form is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Editing,
    /// The report `report` is on its way.
    Sending { report: String },
    /// The server took it.
    Sent { id: String },
    /// It waits in the outbox to be sent later.
    Queued,
}

/// The screenshot as the form knows it.
#[derive(Debug, Clone, Default)]
enum Screenshot {
    #[default]
    Taking,
    Ready {
        shot: Box<Shot>,
        thumb: Option<ImageKey>,
    },
    /// None could be taken; the report goes without one.
    Missing,
}

/// The match's files for the report (D60), made on a worker thread as the form opens.
#[derive(Debug, Default)]
enum Files {
    #[default]
    None,
    Preparing(std::sync::mpsc::Receiver<Attachments>),
    Ready(Attachments),
}

#[derive(Debug, Default)]
pub struct FeedbackForm {
    files: Files,
    pub category: Category,
    pub description: String,
    pub place: Place,
    pub phase: Phase,
    screenshot: Screenshot,
    /// A problem to show above the buttons (a refused report).
    pub status: String,
    /// The description field takes the keyboard once the form shows.
    focus: bool,
    /// Thumbnails get names of their own (a bound image name keeps its first image).
    shots: u32,
}

impl FeedbackForm {
    /// A fresh form for a report from `place`; the screenshot is on its way. The
    /// description and category of an unsent report stay.
    pub fn open(&mut self, place: Place) {
        if !matches!(self.phase, Phase::Editing) {
            self.description.clear();
            self.category = Category::default();
        }
        self.place = place;
        self.phase = Phase::Editing;
        self.screenshot = Screenshot::Taking;
        self.files = Files::None;
        self.status.clear();
        self.focus = true;
    }

    /// The screenshot arrived (or failed); its thumbnail is bound in `host`.
    pub fn screenshot(&mut self, host: &mut UiHost, gpu: &Gpu, result: Result<Shot, String>) {
        self.screenshot = match result {
            Ok(shot) => {
                self.shots += 1;
                let t = &shot.thumb;
                let thumb = host.rgba_image(
                    gpu,
                    &format!("feedback-shot-{}", self.shots),
                    t.width,
                    t.height,
                    &t.rgba,
                );
                Screenshot::Ready {
                    shot: Box::new(shot),
                    thumb,
                }
            }
            Err(error) => {
                tracing::warn!(%error, "no feedback screenshot");
                Screenshot::Missing
            }
        };
    }

    /// Makes the match's files for the report with `make`, on a worker thread.
    pub fn prepare_files(&mut self, make: impl FnOnce() -> Attachments + Send + 'static) {
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("feedback-files".into())
            .spawn(move || {
                let _ = tx.send(make().within_limits());
            });
        self.files = match spawned {
            Ok(_) => Files::Preparing(rx),
            Err(error) => {
                tracing::warn!(%error, "the match's files are not attached");
                Files::None
            }
        };
    }

    /// Takes the match's files once made.
    pub fn poll_files(&mut self) {
        if let Files::Preparing(rx) = &self.files {
            match rx.try_recv() {
                Ok(files) => self.files = Files::Ready(files),
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.files = Files::None,
            }
        }
    }

    /// Whether the match's files are made (or none are coming).
    pub fn files_ready(&self) -> bool {
        !matches!(self.files, Files::Preparing(_))
    }

    /// The match's files to send (none until made).
    pub fn files(&self) -> Attachments {
        match &self.files {
            Files::Ready(files) => files.clone(),
            _ => Attachments::default(),
        }
    }

    /// The encoded screenshot, once taken.
    pub fn shot(&self) -> Option<&Shot> {
        match &self.screenshot {
            Screenshot::Ready { shot, .. } => Some(shot),
            _ => None,
        }
    }

    /// Whether Submit is offered: a description, and the screenshot taken (or given up).
    pub fn can_submit(&self) -> bool {
        self.phase == Phase::Editing
            && crate::feedback::description_ok(&self.description)
            && !matches!(self.screenshot, Screenshot::Taking)
            && !matches!(self.files, Files::Preparing(_))
    }

    /// A report went out (`report` is its id).
    pub fn sending(&mut self, report: String) {
        self.phase = Phase::Sending { report };
        self.status.clear();
    }

    /// What became of a report; only the form's own changes it.
    pub fn delivered(&mut self, delivery: &Delivery) {
        let Phase::Sending { report: ours } = &self.phase else {
            return;
        };
        match delivery {
            Delivery::Sent { report, id } if report == ours => {
                self.phase = Phase::Sent { id: id.clone() };
            }
            Delivery::Queued { report } if report == ours => self.phase = Phase::Queued,
            Delivery::Refused { report, message } if report == ours => {
                self.phase = Phase::Editing;
                self.status = format!("The report was not accepted: {message}");
            }
            _ => {}
        }
    }

    pub fn show(&mut self, ui: &mut Ui) {
        let editing = self.phase == Phase::Editing;
        widgets::visible(ui, "feedback/form", editing);
        widgets::visible(ui, "feedback/done", !editing);
        let (shot_label, note) = match &self.screenshot {
            Screenshot::Taking | Screenshot::Ready { .. } => (
                self.place.shot_label(),
                "This screenshot is submitted with your report.".to_string(),
            ),
            Screenshot::Missing => (
                "No screenshot",
                "A screenshot could not be taken; the report is sent without one.".to_string(),
            ),
        };
        widgets::text(ui, "feedback/form/shot-label", shot_label);
        widgets::text(ui, "feedback/form/shot-note", &note);
        let attached = match &self.files {
            Files::None => String::new(),
            Files::Preparing(_) => "Attaching the replay of this match…".into(),
            Files::Ready(files) => match (&files.replay, &files.save) {
                (Some(_), Some(_)) => {
                    "Replay of this match is attached, with a save of this moment.".into()
                }
                (Some(_), None) => "Replay of this match is attached.".into(),
                (None, Some(_)) => "A save of this moment is attached.".into(),
                (None, None) => String::new(),
            },
        };
        widgets::text(ui, "feedback/form/files-note", &attached);
        widgets::visible(ui, "feedback/form/files-note", !attached.is_empty());
        let thumb = match &self.screenshot {
            Screenshot::Ready { thumb, .. } => *thumb,
            _ => None,
        };
        if let Some(node) = widgets::find(ui, "feedback/form/thumb") {
            ui.set_image(node, thumb);
        }
        widgets::visible(ui, "feedback/form/thumb", thumb.is_some());
        widgets::visible(
            ui,
            "feedback/form/taking",
            matches!(self.screenshot, Screenshot::Taking),
        );
        widgets::enabled(ui, "feedback/form/submit", self.can_submit());
        widgets::text(ui, "feedback/form/status", &self.status);
        widgets::visible(ui, "feedback/form/status", !self.status.is_empty());
        let (title, body) = match &self.phase {
            Phase::Editing => (String::new(), String::new()),
            Phase::Sending { .. } => (
                "Sending…".into(),
                "Your report and its screenshot are on their way. You can close this; it is sent in the background.".into(),
            ),
            Phase::Sent { id } => (
                "Thank you!".into(),
                format!("Your report was sent. Its reference is {id}."),
            ),
            Phase::Queued => (
                "Saved for later".into(),
                "The report could not be sent right now. It is saved with its screenshot and sent automatically once the game reaches the server.".into(),
            ),
        };
        widgets::text(ui, "feedback/done/done-title", &title);
        widgets::text(ui, "feedback/done/done-body", &body);
        if editing && self.focus {
            self.focus = false;
            let field = ui.find("feedback/form/description");
            ui.focus(field);
        }
    }

    /// Puts the form's values into its fields (when it opens).
    pub fn fill(&self, ui: &mut Ui) {
        widgets::text(ui, "feedback/form/description", &self.description);
        if let Some(node) = widgets::find(ui, "feedback/form/category") {
            let i = Category::ALL
                .iter()
                .position(|c| *c == self.category)
                .unwrap_or(0);
            ui.set_selected(node, i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> Screenshot {
        Screenshot::Ready {
            shot: Box::default(),
            thumb: None,
        }
    }

    #[test]
    fn submit_needs_a_description_and_the_screenshot() {
        let mut form = FeedbackForm::default();
        form.open(Place::default());
        assert!(
            !form.can_submit(),
            "nothing written, screenshot still coming"
        );
        form.description = "The ants stopped gathering".into();
        assert!(!form.can_submit(), "the screenshot is still being taken");
        form.screenshot = ready();
        assert!(form.can_submit());
        form.description = "   ".into();
        assert!(!form.can_submit());
        // Without a screenshot the report still goes.
        form.description = "x".into();
        form.screenshot = Screenshot::Missing;
        assert!(form.can_submit());
    }

    #[test]
    fn the_form_follows_its_own_report() {
        let mut form = FeedbackForm::default();
        form.open(Place::default());
        form.description = "Too strong".into();
        form.sending("ours".into());
        assert!(!form.can_submit());
        // Another report (from the outbox) finishing changes nothing.
        form.delivered(&Delivery::Sent {
            report: "older".into(),
            id: "1".into(),
        });
        assert_eq!(
            form.phase,
            Phase::Sending {
                report: "ours".into()
            }
        );
        form.delivered(&Delivery::Refused {
            report: "ours".into(),
            message: "description: too long".into(),
        });
        assert_eq!(form.phase, Phase::Editing);
        assert!(form.status.contains("too long"));
        assert_eq!(form.description, "Too strong", "kept to fix and resend");
        form.sending("again".into());
        form.delivered(&Delivery::Queued {
            report: "again".into(),
        });
        assert_eq!(form.phase, Phase::Queued);
        // The next report starts from an empty form.
        form.open(Place::default());
        assert!(form.description.is_empty());
        assert_eq!(form.phase, Phase::Editing);
    }
}
