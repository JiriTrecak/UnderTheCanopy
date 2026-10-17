//! The app flow as a state machine: which screen is up, which overlays sit on it, and
//! where a match leads back to. Pure logic (no UI, no GPU) so every transition is tested.
//!
//! ```text
//! Boot ─▶ MainMenu ─┬▶ Campaign ─▶ Missions ─┐
//!                   ├▶ Skirmish ─▶ Replays ──┼▶ Loading ─▶ Match ─▶ Result ─▶ (origin)
//!                   ├▶ Multiplayer ─▶ Lobby ─┘      │         │
//!                   ├▶ Load (overlay) ─────────────▶┘         └▶ GameMenu (overlay) ─▶ (origin)
//!                   ├▶ Settings (overlay)
//!                   ├▶ Releases (overlay: What's new, updates)
//!                   ├▶ Feedback (overlay; also over the game menu)
//!                   └▶ Exit
//! ```
//!
//! Overlays stack: settings and the save list open over the game menu or the main menu
//! and return to what was under them; a confirmation sits on top of everything.

/// A full screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Boot,
    MainMenu,
    /// Faction cards.
    Campaign,
    /// The missions of one campaign.
    Missions {
        campaign: String,
    },
    Skirmish,
    /// The recorded matches (D60), from the skirmish screen.
    Replays,
    /// Server status, lobbies, create and join.
    Multiplayer,
    /// Inside a room before its match.
    Lobby,
    Loading,
    Match,
}

/// Something shown over the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    Settings,
    /// The saved games, to load one.
    Saves,
    /// The in-match menu; the match is paused while it is open (single-player).
    GameMenu,
    /// The match's outcome, over the still match.
    Result,
    /// A yes/no question (its content is the caller's).
    Confirm,
    /// What's new and the update details: only over the main menu, so an update is
    /// never installed during a match (D43).
    Releases,
    /// The feedback form: over the main menu or the game menu (which keeps a
    /// single-player match paused under it).
    Feedback,
}

/// Where a match returns to when it is left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    MainMenu,
    Skirmish,
    Replays,
    Missions { campaign: String },
    Multiplayer,
}

impl Origin {
    pub fn screen(&self) -> Screen {
        match self {
            Self::MainMenu => Screen::MainMenu,
            Self::Skirmish => Screen::Skirmish,
            Self::Replays => Screen::Replays,
            Self::Missions { campaign } => Screen::Missions {
                campaign: campaign.clone(),
            },
            Self::Multiplayer => Screen::Multiplayer,
        }
    }
}

/// What the player (or the game) asks the flow to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// The game's data is loaded.
    Booted,
    /// A main-menu entry, or a step deeper (campaign → missions, multiplayer → lobby).
    Open(Screen),
    /// One step back: closes the top overlay, or leaves the screen for its parent.
    Back,
    Show(Overlay),
    /// Starts loading a match that returns to `origin`.
    StartMatch(Origin),
    /// The loading finished; the match runs.
    Loaded,
    /// The loading failed or was cancelled: back to where it came from.
    LoadFailed,
    /// The outcome is decided (or the player surrendered).
    Decided,
    /// Leaves the match (game menu, result, a lost connection).
    LeaveMatch,
    Quit,
}

/// What the app must do because of a transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Hold the match still (single-player) or let it run again.
    Pause(bool),
    /// Stop and drop the running match (and any loading job).
    EndMatch,
    /// Leave the room (multiplayer lobby).
    LeaveRoom,
    /// Close the game.
    Exit,
}

#[derive(Debug, Clone)]
pub struct Flow {
    screen: Screen,
    overlays: Vec<Overlay>,
    origin: Origin,
}

impl Default for Flow {
    fn default() -> Self {
        Self {
            screen: Screen::Boot,
            overlays: Vec::new(),
            origin: Origin::MainMenu,
        }
    }
}

impl Flow {
    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    pub fn overlays(&self) -> &[Overlay] {
        &self.overlays
    }

    pub fn top(&self) -> Option<Overlay> {
        self.overlays.last().copied()
    }

    pub fn has(&self, overlay: Overlay) -> bool {
        self.overlays.contains(&overlay)
    }

    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    pub fn in_match(&self) -> bool {
        self.screen == Screen::Match
    }

    /// Whether the match should stand still: the game menu (or anything over it) is open.
    fn match_paused(&self) -> bool {
        self.in_match() && self.has(Overlay::GameMenu)
    }

    fn set_screen(&mut self, screen: Screen, effects: &mut Vec<Effect>) {
        let leaving_match = self.screen == Screen::Match || self.screen == Screen::Loading;
        let arriving_match = screen == Screen::Match || screen == Screen::Loading;
        if leaving_match && !arriving_match {
            effects.push(Effect::EndMatch);
        }
        if self.screen == Screen::Lobby && screen != Screen::Lobby && screen != Screen::Loading {
            effects.push(Effect::LeaveRoom);
        }
        self.screen = screen;
        self.overlays.clear();
    }

    fn push(&mut self, overlay: Overlay) {
        if self.top() != Some(overlay) {
            self.overlays.retain(|o| *o != overlay);
            self.overlays.push(overlay);
        }
    }

    /// Applies an intent; returns what the app must do.
    pub fn apply(&mut self, intent: Intent) -> Vec<Effect> {
        let mut effects = Vec::new();
        let was_paused = self.match_paused();
        match intent {
            Intent::Booted => {
                if self.screen == Screen::Boot {
                    self.set_screen(Screen::MainMenu, &mut effects);
                }
            }
            Intent::Open(screen) => {
                let allowed = matches!(
                    (&self.screen, &screen),
                    (
                        Screen::MainMenu,
                        Screen::Campaign | Screen::Skirmish | Screen::Multiplayer
                    ) | (Screen::Campaign, Screen::Missions { .. })
                        | (Screen::Skirmish, Screen::Replays)
                        | (Screen::Multiplayer, Screen::Lobby)
                        | (Screen::Lobby, Screen::Multiplayer)
                );
                if allowed && self.overlays.is_empty() {
                    self.set_screen(screen, &mut effects);
                }
            }
            Intent::Back => {
                if let Some(top) = self.overlays.last().copied() {
                    // The result stays until the player leaves the match.
                    if top != Overlay::Result || !self.in_match() {
                        self.overlays.pop();
                    }
                } else {
                    let parent = match &self.screen {
                        Screen::Campaign | Screen::Skirmish | Screen::Multiplayer => {
                            Some(Screen::MainMenu)
                        }
                        Screen::Missions { .. } => Some(Screen::Campaign),
                        Screen::Replays => Some(Screen::Skirmish),
                        Screen::Lobby => Some(Screen::Multiplayer),
                        Screen::Loading => Some(self.origin.screen()),
                        // Escape in a match opens its menu.
                        Screen::Match => {
                            self.push(Overlay::GameMenu);
                            None
                        }
                        Screen::Boot | Screen::MainMenu => None,
                    };
                    if let Some(parent) = parent {
                        self.set_screen(parent, &mut effects);
                    }
                }
            }
            Intent::Show(overlay) => {
                let fits = match overlay {
                    Overlay::GameMenu | Overlay::Result => self.in_match(),
                    Overlay::Saves => {
                        self.screen == Screen::MainMenu || self.has(Overlay::GameMenu)
                    }
                    Overlay::Settings | Overlay::Confirm => self.screen != Screen::Boot,
                    Overlay::Releases => self.screen == Screen::MainMenu,
                    Overlay::Feedback => {
                        self.screen == Screen::MainMenu || self.has(Overlay::GameMenu)
                    }
                };
                if fits {
                    if overlay == Overlay::Result {
                        self.overlays.clear();
                    }
                    self.push(overlay);
                }
            }
            Intent::StartMatch(origin) => {
                self.origin = origin;
                self.set_screen(Screen::Loading, &mut effects);
            }
            Intent::Loaded => {
                if self.screen == Screen::Loading {
                    self.screen = Screen::Match;
                    self.overlays.clear();
                }
            }
            Intent::LoadFailed => {
                if self.screen == Screen::Loading {
                    let origin = self.origin.screen();
                    self.set_screen(origin, &mut effects);
                }
            }
            Intent::Decided => {
                if self.in_match() {
                    self.overlays.clear();
                    self.overlays.push(Overlay::Result);
                }
            }
            Intent::LeaveMatch => {
                if self.in_match() || self.screen == Screen::Loading {
                    let origin = self.origin.screen();
                    self.set_screen(origin, &mut effects);
                }
            }
            Intent::Quit => {
                if self.in_match() || self.screen == Screen::Loading {
                    effects.push(Effect::EndMatch);
                }
                if self.screen == Screen::Lobby {
                    effects.push(Effect::LeaveRoom);
                }
                effects.push(Effect::Exit);
            }
        }
        let paused = self.match_paused();
        if self.in_match() && paused != was_paused {
            effects.push(Effect::Pause(paused));
        }
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn booted() -> Flow {
        let mut flow = Flow::default();
        assert!(flow.apply(Intent::Booted).is_empty());
        assert_eq!(flow.screen(), &Screen::MainMenu);
        flow
    }

    #[test]
    fn replays_open_from_skirmish_and_a_watched_replay_returns_to_them() {
        let mut flow = booted();
        flow.apply(Intent::Open(Screen::Replays));
        assert_eq!(flow.screen(), &Screen::MainMenu, "only from skirmish");
        flow.apply(Intent::Open(Screen::Skirmish));
        flow.apply(Intent::Open(Screen::Replays));
        assert_eq!(flow.screen(), &Screen::Replays);
        flow.apply(Intent::StartMatch(Origin::Replays));
        flow.apply(Intent::Loaded);
        assert!(flow.in_match());
        assert_eq!(flow.apply(Intent::LeaveMatch), [Effect::EndMatch]);
        assert_eq!(flow.screen(), &Screen::Replays);
        flow.apply(Intent::Back);
        assert_eq!(flow.screen(), &Screen::Skirmish);
    }

    #[test]
    fn a_skirmish_runs_from_the_menu_to_the_result_and_back() {
        let mut flow = booted();
        flow.apply(Intent::Open(Screen::Skirmish));
        assert_eq!(flow.screen(), &Screen::Skirmish);
        flow.apply(Intent::StartMatch(Origin::Skirmish));
        assert_eq!(flow.screen(), &Screen::Loading);
        flow.apply(Intent::Loaded);
        assert!(flow.in_match());
        flow.apply(Intent::Decided);
        assert_eq!(flow.top(), Some(Overlay::Result));
        // The result stays put: Escape does not dismiss it.
        flow.apply(Intent::Back);
        assert_eq!(flow.top(), Some(Overlay::Result));
        assert_eq!(flow.apply(Intent::LeaveMatch), vec![Effect::EndMatch]);
        assert_eq!(flow.screen(), &Screen::Skirmish);
        assert!(flow.overlays().is_empty());
        flow.apply(Intent::Back);
        assert_eq!(flow.screen(), &Screen::MainMenu);
    }

    #[test]
    fn the_game_menu_pauses_and_its_pages_return_to_it() {
        let mut flow = booted();
        flow.apply(Intent::Open(Screen::Skirmish));
        flow.apply(Intent::StartMatch(Origin::Skirmish));
        flow.apply(Intent::Loaded);
        // Escape opens the menu and pauses.
        assert_eq!(flow.apply(Intent::Back), vec![Effect::Pause(true)]);
        assert_eq!(flow.top(), Some(Overlay::GameMenu));
        // Settings over the menu, then back to the menu, still paused.
        assert!(flow.apply(Intent::Show(Overlay::Settings)).is_empty());
        assert_eq!(flow.overlays(), &[Overlay::GameMenu, Overlay::Settings]);
        assert!(flow.apply(Intent::Back).is_empty());
        assert_eq!(flow.top(), Some(Overlay::GameMenu));
        flow.apply(Intent::Show(Overlay::Saves));
        flow.apply(Intent::Show(Overlay::Confirm));
        assert_eq!(flow.overlays().len(), 3);
        flow.apply(Intent::Back);
        flow.apply(Intent::Back);
        // Resume.
        assert_eq!(flow.apply(Intent::Back), vec![Effect::Pause(false)]);
        assert!(flow.overlays().is_empty());
        // Leave from the menu: the match ends, back to its origin.
        flow.apply(Intent::Show(Overlay::GameMenu));
        assert_eq!(flow.apply(Intent::LeaveMatch), vec![Effect::EndMatch]);
        assert_eq!(flow.screen(), &Screen::Skirmish);
    }

    #[test]
    fn a_failed_load_returns_to_its_origin() {
        let mut flow = booted();
        flow.apply(Intent::Open(Screen::Campaign));
        flow.apply(Intent::Open(Screen::Missions {
            campaign: "c".into(),
        }));
        flow.apply(Intent::StartMatch(Origin::Missions {
            campaign: "c".into(),
        }));
        assert_eq!(flow.apply(Intent::LoadFailed), vec![Effect::EndMatch]);
        assert_eq!(
            flow.screen(),
            &Screen::Missions {
                campaign: "c".into()
            }
        );
        flow.apply(Intent::Back);
        assert_eq!(flow.screen(), &Screen::Campaign);
        // Back from loading cancels it.
        flow.apply(Intent::Open(Screen::Missions {
            campaign: "c".into(),
        }));
        flow.apply(Intent::StartMatch(Origin::Missions {
            campaign: "c".into(),
        }));
        assert_eq!(flow.apply(Intent::Back), vec![Effect::EndMatch]);
    }

    #[test]
    fn release_notes_and_updates_open_only_over_the_main_menu() {
        let mut flow = booted();
        flow.apply(Intent::Show(Overlay::Releases));
        assert_eq!(flow.top(), Some(Overlay::Releases));
        flow.apply(Intent::Back);
        assert!(flow.overlays().is_empty());
        flow.apply(Intent::Open(Screen::Skirmish));
        flow.apply(Intent::StartMatch(Origin::Skirmish));
        flow.apply(Intent::Loaded);
        flow.apply(Intent::Back);
        flow.apply(Intent::Show(Overlay::Releases));
        assert_eq!(flow.top(), Some(Overlay::GameMenu));
    }

    #[test]
    fn feedback_opens_over_the_menus_and_keeps_the_match_paused() {
        let mut flow = booted();
        flow.apply(Intent::Show(Overlay::Feedback));
        assert_eq!(flow.top(), Some(Overlay::Feedback));
        flow.apply(Intent::Back);
        assert!(flow.overlays().is_empty());
        flow.apply(Intent::Open(Screen::Skirmish));
        flow.apply(Intent::Show(Overlay::Feedback));
        assert!(
            flow.overlays().is_empty(),
            "only the main menu and the game menu"
        );
        flow.apply(Intent::StartMatch(Origin::Skirmish));
        flow.apply(Intent::Loaded);
        flow.apply(Intent::Show(Overlay::Feedback));
        assert!(flow.overlays().is_empty(), "not over the playing match");
        assert_eq!(flow.apply(Intent::Back), vec![Effect::Pause(true)]);
        // Over the game menu the match stays paused, and closing returns to the menu.
        assert!(flow.apply(Intent::Show(Overlay::Feedback)).is_empty());
        assert_eq!(flow.overlays(), &[Overlay::GameMenu, Overlay::Feedback]);
        assert!(flow.apply(Intent::Back).is_empty());
        assert_eq!(flow.top(), Some(Overlay::GameMenu));
    }

    #[test]
    fn saves_load_from_the_main_menu() {
        let mut flow = booted();
        flow.apply(Intent::Show(Overlay::Saves));
        assert_eq!(flow.top(), Some(Overlay::Saves));
        // Screens cannot open under an overlay.
        flow.apply(Intent::Open(Screen::Skirmish));
        assert_eq!(flow.screen(), &Screen::MainMenu);
        flow.apply(Intent::StartMatch(Origin::MainMenu));
        assert!(flow.overlays().is_empty());
        flow.apply(Intent::Loaded);
        flow.apply(Intent::LeaveMatch);
        assert_eq!(flow.screen(), &Screen::MainMenu);
        // The save list belongs to the main menu and the game menu only.
        flow.apply(Intent::Open(Screen::Skirmish));
        flow.apply(Intent::Show(Overlay::Saves));
        assert!(flow.overlays().is_empty());
    }

    #[test]
    fn multiplayer_lobbies_lead_to_matches_and_leaving_leaves_the_room() {
        let mut flow = booted();
        flow.apply(Intent::Open(Screen::Multiplayer));
        flow.apply(Intent::Open(Screen::Lobby));
        assert_eq!(flow.screen(), &Screen::Lobby);
        assert_eq!(flow.apply(Intent::Back), vec![Effect::LeaveRoom]);
        assert_eq!(flow.screen(), &Screen::Multiplayer);
        flow.apply(Intent::Open(Screen::Lobby));
        // The match starts from the lobby: the room carries on into the match.
        assert!(
            flow.apply(Intent::StartMatch(Origin::Multiplayer))
                .is_empty()
        );
        flow.apply(Intent::Loaded);
        flow.apply(Intent::Decided);
        flow.apply(Intent::LeaveMatch);
        assert_eq!(flow.screen(), &Screen::Multiplayer);
    }

    #[test]
    fn quitting_ends_what_runs() {
        let mut flow = booted();
        flow.apply(Intent::Open(Screen::Skirmish));
        flow.apply(Intent::StartMatch(Origin::Skirmish));
        flow.apply(Intent::Loaded);
        assert_eq!(
            flow.apply(Intent::Quit),
            vec![Effect::EndMatch, Effect::Exit]
        );
        let mut flow = booted();
        assert_eq!(flow.apply(Intent::Quit), vec![Effect::Exit]);
        // Nothing happens before boot except quitting.
        let mut flow = Flow::default();
        flow.apply(Intent::Open(Screen::Skirmish));
        flow.apply(Intent::Show(Overlay::Settings));
        assert_eq!(flow.screen(), &Screen::Boot);
        assert!(flow.overlays().is_empty());
    }
}
