//! The campaign screens (docs/campaign.md "Playing a campaign"): the campaigns to choose
//! from (`campaign.json`), one campaign's screen (`campaign-steps.json`: the steps reached
//! so far under their chapters, the next one as a larger card, Continue), the cards of its
//! stories, chapters and cinematics (`card.json`) and a mission's loading screen (the
//! `mission-view` of `loading.json`).
//!
//! Only the steps reached show: the campaign screen never names a step beyond the next.

use super::{ui_host::UiHost, widgets};
use crate::campaign::Progress;
use crate::data::GameData;
use canopy_content::campaign::{CampaignPackage, Step, paragraphs};
use canopy_render::Gpu;
use std::collections::BTreeMap;

/// Seconds a chapter card stays before the campaign moves on.
pub const CHAPTER_SECONDS: f32 = 4.5;
/// Seconds a cinematic's placeholder card stays.
pub const CINEMATIC_SECONDS: f32 = 6.0;
/// Seconds before a story paragraph starts to appear, between paragraphs, and to fade in.
const REVEAL_DELAY: f32 = 0.35;
const REVEAL_STAGGER: f32 = 0.9;
const REVEAL_FADE: f32 = 1.3;

/// Escapes markup in authored text.
fn plain(text: &str) -> String {
    text.replace('[', "[[")
}

/// The image pack entry of asset `id`, bound for the UI.
fn image(
    host: &mut UiHost,
    gpu: &Gpu,
    data: &GameData,
    id: Option<&canopy_content::Id>,
) -> Option<canopy_ui::ImageKey> {
    let entry = widgets::asset_image(data, id?.as_str())?;
    host.pack_image(gpu, &data.pack, &entry)
}

fn set_image(host: &mut UiHost, path: &str, key: Option<canopy_ui::ImageKey>) {
    if let Some(node) = widgets::find(&host.ui, path) {
        host.ui.set_image(node, key);
    }
}

/// The race's name as the menus show it.
fn race_name(data: &GameData, race: &str) -> String {
    data.content
        .rules
        .races
        .get(race)
        .map_or_else(|| race.to_string(), |r| r.name.clone())
}

/// What a step is called on the campaign screen, and what its button says.
fn kind_label(step: &Step) -> (String, String) {
    match step {
        Step::Chapter(_) => (tr!("campaign.chapter"), tr!("campaign.begin")),
        Step::Story(_) => (tr!("campaign.story"), tr!("campaign.read")),
        Step::Mission(_) => (tr!("campaign.mission"), tr!("campaign.play")),
        Step::Cinematic(_) => (tr!("campaign.cinematic"), tr!("campaign.watch")),
    }
}

/// The art a step shows: its own, else the campaign's background.
pub fn step_art<'a>(
    campaign: &'a CampaignPackage,
    step: &'a Step,
    page: usize,
) -> Option<&'a canopy_content::Id> {
    let own = match step {
        Step::Chapter(s) => s.art.as_ref(),
        Step::Story(s) => s
            .pages
            .get(page)
            .and_then(|p| p.art.as_ref())
            .or_else(|| s.pages.iter().find_map(|p| p.art.as_ref())),
        Step::Mission(s) => s.art.as_ref(),
        Step::Cinematic(_) => None,
    };
    own.or(campaign.campaign.background.as_ref())
}

// --- Choosing a campaign ------------------------------------------------------------

/// Fills the campaign cards (`campaign/cards`).
pub fn show_cards(
    host: &mut UiHost,
    gpu: &Gpu,
    data: &GameData,
    progress: &BTreeMap<String, Progress>,
) {
    let Some(list) = host.ui.find("campaign/cards") else {
        return;
    };
    host.ui.set_count(list, data.campaigns.len());
    widgets::visible(&mut host.ui, "campaign/empty", data.campaigns.is_empty());
    for (i, campaign) in data.campaigns.iter().enumerate() {
        let base = format!("campaign/cards/{i}");
        let c = &campaign.campaign;
        let art = image(host, gpu, data, c.card.as_ref())
            .or_else(|| image(host, gpu, data, c.background.as_ref()));
        set_image(host, &format!("{base}/crest"), art);
        let ui = &mut host.ui;
        widgets::text(
            ui,
            &format!("{base}/chapter"),
            &tr!("campaign.the-race", race = &race_name(data, &c.race)),
        );
        widgets::text(ui, &format!("{base}/name"), &plain(campaign.name()));
        let saved = progress.get(campaign.id()).cloned().unwrap_or_default();
        let status = if saved.complete {
            tr!("campaign.complete")
        } else if saved.started() {
            c.steps.get(saved.next(c)).map_or_else(String::new, |s| {
                tr!("campaign.next", title = &plain(s.title()))
            })
        } else {
            tr!("campaign.not-begun")
        };
        widgets::text(ui, &format!("{base}/status"), &status);
        widgets::text(
            ui,
            &format!("{base}/play"),
            &if saved.started() {
                tr!("campaign.continue")
            } else {
                tr!("campaign.begin")
            },
        );
    }
}

/// The campaign id of card `index`.
pub fn card_id(data: &GameData, index: usize) -> Option<String> {
    data.campaigns.get(index).map(|c| c.id().to_string())
}

// --- The campaign screen --------------------------------------------------------------

/// One entry of the campaign screen's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// A chapter already begun: a heading over its steps.
    Heading { step: usize },
    /// A step reached: play it again.
    Row { step: usize },
    /// The next step, larger.
    Next { step: usize },
}

impl Entry {
    /// The step an entry's button plays (headings have none).
    pub fn step(&self) -> Option<usize> {
        match self {
            Self::Heading { .. } => None,
            Self::Row { step } | Self::Next { step } => Some(*step),
        }
    }
}

/// The steps reached so far, in order, chapters as headings and the next step as a card.
/// Nothing after the next step is listed.
pub fn entries(campaign: &CampaignPackage, progress: &Progress) -> Vec<Entry> {
    let c = &campaign.campaign;
    let next = progress.next(c);
    (0..progress.shown(c))
        .map(|step| {
            if step == next {
                Entry::Next { step }
            } else if matches!(c.steps[step], Step::Chapter(_)) {
                Entry::Heading { step }
            } else {
                Entry::Row { step }
            }
        })
        .collect()
}

/// The line under the next step's title.
fn next_line(step: &Step) -> String {
    match step {
        Step::Mission(m) => m
            .story
            .as_deref()
            .and_then(|s| paragraphs(s).into_iter().next())
            .or_else(|| m.chapter_line.clone())
            .unwrap_or_default(),
        Step::Story(s) => s
            .pages
            .first()
            .and_then(|p| paragraphs(&p.text).into_iter().next())
            .unwrap_or_default(),
        Step::Cinematic(s) => s.note.clone().unwrap_or_default(),
        Step::Chapter(_) => tr!("campaign.new-chapter"),
    }
}

/// Fills the campaign screen (`campaign-steps`). Returns the list's entries, which its
/// buttons index.
pub fn show_steps(
    host: &mut UiHost,
    gpu: &Gpu,
    data: &GameData,
    campaign: &CampaignPackage,
    progress: &Progress,
) -> Vec<Entry> {
    let c = &campaign.campaign;
    let background = image(host, gpu, data, c.background.as_ref())
        .or_else(|| Some(host.ui.image_key("menu-aftermath")));
    set_image(host, "campaign-steps/art", background);
    let entries = entries(campaign, progress);
    let mut arts = Vec::new();
    for entry in &entries {
        if let Entry::Next { step } = entry {
            arts.push(image(
                host,
                gpu,
                data,
                step_art(campaign, &c.steps[*step], 0),
            ));
        }
    }
    let ui = &mut host.ui;
    widgets::text(
        ui,
        "campaign-steps/kicker",
        &tr!("campaign.race-campaign", race = &race_name(data, &c.race)),
    );
    widgets::text(ui, "campaign-steps/title", &plain(campaign.name()));
    let levels: Vec<String> = canopy_sim::Difficulty::ALL
        .iter()
        .map(|d| {
            let name = crate::text::tr(&d.text_key());
            tr!("campaign.difficulty", level = &name)
        })
        .collect();
    let level = canopy_sim::Difficulty::ALL
        .iter()
        .position(|&d| d == progress.difficulty)
        .unwrap_or(1);
    widgets::options(ui, "campaign-steps/difficulty", &levels, level);
    if let Some(list) = widgets::find(ui, "campaign-steps/steps") {
        ui.set_count(list, entries.len());
    }
    let mut arts = arts.into_iter();
    for (i, entry) in entries.iter().enumerate() {
        let base = format!("campaign-steps/steps/{i}");
        let (heading, row, next) = match entry {
            Entry::Heading { .. } => (true, false, false),
            Entry::Row { .. } => (false, true, false),
            Entry::Next { .. } => (false, false, true),
        };
        widgets::visible(ui, &format!("{base}/heading"), heading);
        widgets::visible(ui, &format!("{base}/row"), row);
        widgets::visible(ui, &format!("{base}/next"), next);
        let step = match entry {
            Entry::Heading { step } | Entry::Row { step } | Entry::Next { step } => &c.steps[*step],
        };
        let (kind, act) = kind_label(step);
        let title = plain(step.title());
        // Play with AI: a mission whose map has an autoplay plan.
        let planned = step
            .mission()
            .and_then(|m| data.map(m.map.as_str()))
            .is_some_and(|m| m.autoplay);
        widgets::visible(ui, &format!("{base}/row/ai"), planned);
        widgets::visible(ui, &format!("{base}/next/ai"), planned);
        match entry {
            Entry::Heading { .. } => {
                widgets::text(ui, &format!("{base}/heading/label"), &title);
            }
            Entry::Row { .. } => {
                widgets::text(ui, &format!("{base}/row/kind"), &kind);
                widgets::text(ui, &format!("{base}/row/title"), &title);
                widgets::text(ui, &format!("{base}/row/act"), &act);
            }
            Entry::Next { .. } => {
                widgets::text(
                    ui,
                    &format!("{base}/next/eyebrow"),
                    &tr!("campaign.next-kind", kind = &kind),
                );
                widgets::text(ui, &format!("{base}/next/title"), &title);
                widgets::text(ui, &format!("{base}/next/line"), &plain(&next_line(step)));
                widgets::text(ui, &format!("{base}/next/act"), &act);
                let art = arts.next().flatten();
                if let Some(node) = widgets::find(ui, &format!("{base}/next/art")) {
                    ui.set_image(node, art);
                }
                widgets::visible(ui, &format!("{base}/next/art"), art.is_some());
            }
        }
    }
    widgets::visible(ui, "campaign-steps/complete", progress.complete);
    widgets::visible(ui, "campaign-steps/reset", progress.started());
    widgets::visible(ui, "campaign-steps/continue", !progress.complete);
    widgets::text(
        ui,
        "campaign-steps/continue",
        &if progress.started() {
            tr!("campaign.continue")
        } else {
            tr!("campaign.begin")
        },
    );
    entries
}

// --- Story, chapter and cinematic cards ---------------------------------------------------

/// The card on screen: a story's page, a chapter's title or a cinematic's placeholder.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub campaign: String,
    pub step: usize,
    pub page: usize,
    /// Seconds the page (or card) has shown.
    pub time: f32,
}

impl Card {
    pub fn new(campaign: &str, step: usize) -> Self {
        Self {
            campaign: campaign.to_string(),
            step,
            page: 0,
            time: 0.0,
        }
    }

    /// Seconds until every paragraph of the page has appeared.
    fn revealed_at(paragraphs: usize) -> f32 {
        REVEAL_DELAY + REVEAL_STAGGER * paragraphs.saturating_sub(1) as f32 + REVEAL_FADE
    }

    /// Continue: shows the rest of the page at once, else the next page. Returns whether
    /// the step is done.
    pub fn advance(&mut self, step: &Step) -> bool {
        match step {
            Step::Story(s) => {
                let page = s.pages.get(self.page);
                let count = page.map_or(0, |p| paragraphs(&p.text).len());
                if self.time < Self::revealed_at(count) {
                    self.time = Self::revealed_at(count);
                    false
                } else if self.page + 1 < s.pages.len() {
                    self.page += 1;
                    self.time = 0.0;
                    false
                } else {
                    true
                }
            }
            _ => true,
        }
    }

    /// Whether a timed card has shown long enough (stories wait for the player).
    pub fn due(&self, step: &Step) -> bool {
        match step {
            Step::Chapter(_) => self.time >= CHAPTER_SECONDS,
            Step::Cinematic(_) => self.time >= CINEMATIC_SECONDS,
            _ => false,
        }
    }
}

/// How far paragraph `index` has appeared after `time` seconds (0 to 1).
fn reveal(time: f32, index: usize) -> f32 {
    ((time - REVEAL_DELAY - REVEAL_STAGGER * index as f32) / REVEAL_FADE).clamp(0.0, 1.0)
}

/// Fills the card screen (`card`).
pub fn show_card(host: &mut UiHost, gpu: &Gpu, data: &GameData, card: &Card) {
    let Some(campaign) = data.campaign(&card.campaign) else {
        return;
    };
    let Some(step) = campaign.campaign.steps.get(card.step) else {
        return;
    };
    let art = image(host, gpu, data, step_art(campaign, step, card.page))
        .or_else(|| Some(host.ui.image_key("menu-aftermath")));
    set_image(host, "card/art", art);
    let ui = &mut host.ui;
    let story = matches!(step, Step::Story(_));
    widgets::visible(ui, "card/story", story);
    widgets::visible(ui, "card/title", !story);
    widgets::visible(ui, "card/hint", !story);
    // The art and the title come up gently.
    let fade = (card.time / 1.2).clamp(0.0, 1.0);
    if let Some(node) = widgets::find(ui, "card/art") {
        ui.set_opacity(node, if story { 1.0 } else { 0.35 + 0.65 * fade });
    }
    match step {
        Step::Story(s) => {
            let page = s.pages.get(card.page);
            let texts = page.map(|p| paragraphs(&p.text)).unwrap_or_default();
            widgets::text(ui, "card/story/panel/kicker", &plain(&s.title));
            if let Some(list) = widgets::find(ui, "card/story/panel/text") {
                ui.set_count(list, texts.len());
            }
            for (i, text) in texts.iter().enumerate() {
                let path = format!("card/story/panel/text/{i}");
                widgets::text(ui, &path, &plain(text));
                if let Some(node) = widgets::find(ui, &path) {
                    let shown = reveal(card.time, i);
                    ui.set_opacity(node, shown);
                    ui.set_shift(node, 0.0, (1.0 - shown) * 12.0);
                }
            }
            widgets::text(
                ui,
                "card/story/panel/page",
                &if s.pages.len() > 1 {
                    format!("{} / {}", card.page + 1, s.pages.len())
                } else {
                    String::new()
                },
            );
        }
        Step::Chapter(s) => {
            widgets::text(ui, "card/title/kicker", &plain(campaign.name()));
            widgets::text(ui, "card/title/name", &plain(&s.title));
            widgets::text(ui, "card/title/note", "");
        }
        Step::Cinematic(s) => {
            widgets::text(ui, "card/title/kicker", &tr!("campaign.cinematic"));
            widgets::text(ui, "card/title/name", &plain(&s.title));
            let note = match &s.note {
                Some(note) => tr!("campaign.coming-later-note", note = &plain(note)),
                None => tr!("campaign.coming-later"),
            };
            widgets::text(ui, "card/title/note", &note);
        }
        Step::Mission(_) => {}
    }
    if !story {
        if let Some(node) = widgets::find(ui, "card/title") {
            ui.set_opacity(node, fade);
            ui.set_shift(node, 0.0, (1.0 - fade) * 10.0);
        }
        if let Some(node) = widgets::find(ui, "card/hint") {
            ui.set_opacity(node, ((card.time - 1.5) / 1.0).clamp(0.0, 0.8));
        }
    }
}

// --- A mission's loading screen ----------------------------------------------------------

/// What a mission's loading screen shows besides its step.
pub struct Loading<'a> {
    pub progress: f32,
    /// Loaded: the bar gives way to the prompt.
    pub ready: bool,
    pub error: Option<&'a str>,
    /// Seconds since the screen came up (the prompt's slow pulse).
    pub time: f32,
}

/// Fills `loading/mission-view` for the mission at `step` of `campaign`.
pub fn show_loading(
    host: &mut UiHost,
    gpu: &Gpu,
    data: &GameData,
    campaign: &CampaignPackage,
    step: usize,
    state: &Loading,
) {
    let Some(Step::Mission(m)) = campaign.campaign.steps.get(step) else {
        return;
    };
    let art = image(
        host,
        gpu,
        data,
        m.art.as_ref().or(campaign.campaign.background.as_ref()),
    )
    .or_else(|| Some(host.ui.image_key("menu-aftermath")));
    set_image(host, "loading/mission-view/art", art);
    let ui = &mut host.ui;
    let base = "loading/mission-view/copy";
    widgets::text(
        ui,
        &format!("{base}/chapter"),
        &plain(m.chapter_line.as_deref().unwrap_or_default()),
    );
    widgets::text(ui, &format!("{base}/title"), &plain(&m.title));
    let story = m.story.as_deref().map(paragraphs).unwrap_or_default();
    if let Some(list) = widgets::find(ui, &format!("{base}/story")) {
        ui.set_count(list, story.len());
    }
    for (i, text) in story.iter().enumerate() {
        widgets::text(ui, &format!("{base}/story/{i}"), &plain(text));
    }
    let failed = state.error.is_some();
    widgets::visible(ui, &format!("{base}/progress"), !state.ready && !failed);
    if let Some(bar) = widgets::find(ui, &format!("{base}/progress")) {
        ui.set_value(bar, state.progress);
    }
    widgets::visible(ui, &format!("{base}/prompt"), state.ready && !failed);
    if let Some(prompt) = widgets::find(ui, &format!("{base}/prompt")) {
        let pulse = 0.72 + 0.28 * (state.time * 2.2).sin();
        ui.set_opacity(prompt, pulse);
    }
    widgets::visible(ui, &format!("{base}/error"), failed);
    widgets::visible(ui, &format!("{base}/back"), failed);
    if let Some(error) = state.error {
        widgets::text(
            ui,
            &format!("{base}/error"),
            &format!("The mission could not be loaded: {}", plain(error)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn package() -> CampaignPackage {
        let root = crate::settings::temp_folder("campaign-screen").join("c.utcc");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("package.json"),
            json!({"format": 1, "id": "c", "kind": "campaign", "name": "C", "version": "1.0.0"})
                .to_string(),
        )
        .unwrap();
        CampaignPackage {
            package: canopy_content::package::Package::open(&root).unwrap(),
            campaign: serde_json::from_value(json!({
                "race": "red",
                "steps": [
                    {"kind": "chapter", "id": "one", "title": "One"},
                    {"kind": "story", "id": "tale", "title": "Tale",
                     "pages": [{"text": "a\n\nb"}, {"text": "c"}]},
                    {"kind": "mission", "id": "first", "map": "a", "title": "First"},
                    {"kind": "chapter", "id": "two", "title": "Two"},
                    {"kind": "mission", "id": "second", "map": "b", "title": "Second"}
                ]
            }))
            .unwrap(),
            missions: Vec::new(),
        }
    }

    #[test]
    fn the_screen_lists_only_what_was_reached() {
        let campaign = package();
        let c = &campaign.campaign;
        let mut progress = Progress::default();
        assert_eq!(entries(&campaign, &progress), vec![Entry::Next { step: 0 }]);
        progress.finish(c, 0);
        progress.finish(c, 1);
        assert_eq!(
            entries(&campaign, &progress),
            vec![
                Entry::Heading { step: 0 },
                Entry::Row { step: 1 },
                Entry::Next { step: 2 }
            ]
        );
        for step in 2..5 {
            progress.finish(c, step);
        }
        let all = entries(&campaign, &progress);
        assert_eq!(all.len(), 5);
        assert!(!all.iter().any(|e| matches!(e, Entry::Next { .. })));
        assert_eq!(all[3], Entry::Heading { step: 3 });
        assert_eq!(all[4].step(), Some(4));
    }

    #[test]
    fn a_story_reveals_its_page_then_turns_it() {
        let campaign = package();
        let story = &campaign.campaign.steps[1];
        let mut card = Card::new("c", 1);
        assert_eq!(reveal(0.0, 0), 0.0);
        // The first Continue shows the whole page; the next turns it.
        assert!(!card.advance(story));
        assert_eq!(reveal(card.time, 1), 1.0);
        assert!(!card.advance(story));
        assert_eq!((card.page, card.time), (1, 0.0));
        card.time = 10.0;
        assert!(card.advance(story), "the last page ends the story");
        let chapter = &campaign.campaign.steps[0];
        let mut title = Card::new("c", 0);
        assert!(!title.due(chapter));
        title.time = CHAPTER_SECONDS;
        assert!(title.due(chapter));
        assert!(title.advance(chapter));
    }
}
