//! The campaign screens (old `src/ui/menu/campaign.ts`): faction cards from
//! `rules.campaigns`, then the missions of one campaign with their presentation, and the
//! player's progress (`campaign.json` in the user folder).
//!
//! Locks follow progress, where the old game hard-coded them: a campaign without missions
//! is locked ("Coming later"); within a campaign the first mission is open and each next
//! one opens when the one before it is won.

use super::{ui_host::UiHost, widgets};
use crate::data::{GameData, MapEntry};
use crate::settings::write_atomic;
use canopy_render::Gpu;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const FILE: &str = "campaign.json";

/// Missions won, by map id.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Progress {
    pub completed: BTreeSet<String>,
}

impl Progress {
    pub fn path(user: &Path) -> PathBuf {
        user.join(FILE)
    }

    pub fn load(user: &Path) -> Self {
        std::fs::read(Self::path(user))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, user: &Path) -> anyhow::Result<()> {
        write_atomic(&Self::path(user), &serde_json::to_vec_pretty(self)?)
    }

    /// Whether the `index`th mission of `missions` may be played.
    pub fn unlocked(&self, missions: &[&MapEntry], index: usize) -> bool {
        index == 0
            || missions
                .get(index - 1)
                .is_some_and(|m| self.completed.contains(m.id()))
    }
}

/// A campaign card.
pub struct Card {
    pub name: String,
    pub race: String,
    pub missions: usize,
    pub completed: usize,
}

pub fn cards(data: &GameData, progress: &Progress) -> Vec<Card> {
    data.content
        .rules
        .campaigns
        .iter()
        .map(|(id, c)| {
            let missions = data.missions(id);
            Card {
                name: c.name.clone(),
                race: c.race.clone(),
                completed: missions
                    .iter()
                    .filter(|m| progress.completed.contains(m.id()))
                    .count(),
                missions: missions.len(),
            }
        })
        .collect()
}

/// The crest image of a race (`asset.interface.campaign.<race>`, as the old screen used).
pub fn crest(data: &GameData, race: &str) -> Option<String> {
    widgets::asset_image(data, &format!("asset.interface.campaign.{race}"))
}

pub fn show_cards(host: &mut UiHost, gpu: &Gpu, data: &GameData, progress: &Progress) {
    let cards = cards(data, progress);
    let Some(list) = host.ui.find("campaign/cards") else {
        return;
    };
    host.ui.set_count(list, cards.len());
    widgets::visible(&mut host.ui, "campaign/empty", cards.is_empty());
    for (i, card) in cards.iter().enumerate() {
        let base = format!("campaign/cards/{i}");
        let crest = crest(data, &card.race).and_then(|e| host.pack_image(gpu, &data.pack, &e));
        let locked = card.missions == 0;
        if let Some(node) = host.ui.find(&format!("{base}/crest")) {
            host.ui.set_image(node, crest);
            host.ui.set_opacity(node, if locked { 0.55 } else { 1.0 });
        }
        let race = data
            .content
            .rules
            .races
            .get(&card.race)
            .map_or(card.race.as_str(), |r| r.name.as_str());
        widgets::text(&mut host.ui, &format!("{base}/chapter"), race);
        widgets::text(&mut host.ui, &format!("{base}/name"), &card.name);
        let status = if locked {
            "Coming later".to_string()
        } else if card.completed == card.missions {
            "Every chapter won".to_string()
        } else {
            format!("{} of {} chapters won", card.completed, card.missions)
        };
        widgets::text(&mut host.ui, &format!("{base}/status"), &status);
        widgets::text(
            &mut host.ui,
            &format!("{base}/play"),
            if locked { "Locked" } else { "Play" },
        );
        widgets::enabled(&mut host.ui, &format!("{base}/play"), !locked);
    }
}

/// The campaign id of card `index`.
pub fn card_id(data: &GameData, index: usize) -> Option<String> {
    data.content.rules.campaigns.keys().nth(index).cloned()
}

pub fn show_missions(
    host: &mut UiHost,
    gpu: &Gpu,
    data: &GameData,
    progress: &Progress,
    campaign: &str,
    selected: usize,
) {
    let missions = data.missions(campaign);
    let name = data
        .content
        .rules
        .campaigns
        .get(campaign)
        .map_or(campaign, |c| c.name.as_str());
    let race = data
        .content
        .rules
        .campaigns
        .get(campaign)
        .and_then(|c| data.content.rules.races.get(&c.race))
        .map(|r| r.name.clone());
    widgets::text(
        &mut host.ui,
        "missions/kicker",
        &match &race {
            Some(race) => format!("The {race} campaign"),
            None => "The campaign".into(),
        },
    );
    widgets::text(&mut host.ui, "missions/title", name);
    if let Some(list) = host.ui.find("missions/list") {
        host.ui.set_count(list, missions.len());
    }
    for (i, m) in missions.iter().enumerate() {
        let base = format!("missions/list/{i}");
        widgets::text(
            &mut host.ui,
            &format!("{base}/number"),
            &format!("{:02}", i + 1),
        );
        let title = m
            .mission
            .as_ref()
            .map_or(m.name.as_str(), |x| x.title.as_str());
        widgets::text(&mut host.ui, &format!("{base}/title"), title);
        widgets::checked(&mut host.ui, &base, i == selected);
        widgets::enabled(&mut host.ui, &base, progress.unlocked(&missions, i));
    }
    widgets::text(
        &mut host.ui,
        "missions/note",
        if missions.is_empty() {
            "No playable missions found."
        } else {
            "More chapters to come"
        },
    );
    let chosen = missions.get(selected);
    widgets::visible(&mut host.ui, "missions/detail", chosen.is_some());
    widgets::enabled(
        &mut host.ui,
        "missions/begin",
        chosen.is_some() && progress.unlocked(&missions, selected),
    );
    let Some(m) = chosen else {
        return;
    };
    let summary = m.mission.as_ref();
    let presentation = summary.and_then(|s| s.presentation.as_ref());
    let art = presentation
        .and_then(|p| p.selection_background.as_deref())
        .and_then(|id| widgets::asset_image(data, id))
        .or_else(|| m.preview.clone())
        .and_then(|e| host.pack_image(gpu, &data.pack, &e));
    if let Some(node) = host.ui.find("missions/detail/art") {
        host.ui.set_image(node, art);
    }
    let chapter = presentation
        .and_then(|p| p.chapter.clone())
        .unwrap_or_else(|| format!("Chapter {}", selected + 1));
    widgets::text(&mut host.ui, "missions/detail/chapter", &chapter);
    widgets::text(
        &mut host.ui,
        "missions/detail/name",
        summary.map_or(m.name.as_str(), |s| s.title.as_str()),
    );
    let briefing = presentation
        .and_then(|p| p.briefing.clone())
        .or_else(|| m.description.clone())
        .unwrap_or_default();
    widgets::text(&mut host.ui, "missions/detail/briefing", &briefing);
    let features = presentation
        .and_then(|p| p.features.as_ref())
        .map(|f| {
            f.iter()
                .take(4)
                .cloned()
                .collect::<Vec<_>>()
                .join("   ·   ")
        })
        .unwrap_or_default();
    widgets::text(&mut host.ui, "missions/detail/features", &features);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_round_trips_and_opens_missions_in_order() {
        let user = crate::settings::temp_folder("campaign");
        assert_eq!(Progress::load(&user), Progress::default());
        let mut progress = Progress::default();
        progress.completed.insert("m1".into());
        progress.save(&user).unwrap();
        assert_eq!(Progress::load(&user), progress);
    }
}
