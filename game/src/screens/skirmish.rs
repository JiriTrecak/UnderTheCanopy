//! Skirmish setup (old `src/ui/menu/skirmish.ts`): a battlefield from the map library,
//! every start filled by a human (at most one) or a computer player, each with a race,
//! a starting hero and a team.
//!
//! Teams (D20): every slot has one; "No team" lets the simulation give the slot a team
//! of its own (free for all). The old screen had no team choice.

use super::{ui_host::UiHost, widgets, widgets::Palette};
use crate::data::{GameData, MapEntry};
use canopy_render::Gpu;
use canopy_sim::{MatchSetup, Slot, SlotKind};

/// Team choices offered (`None` is "No team").
pub const TEAMS: u8 = 4;

#[derive(Debug, Clone, PartialEq)]
pub struct SlotChoice {
    /// The start's player number (1–8).
    pub player: u8,
    pub human: bool,
    pub race: String,
    pub hero: Option<String>,
    pub team: Option<u8>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Skirmish {
    /// Map id.
    pub map: Option<String>,
    pub slots: Vec<SlotChoice>,
}

impl Skirmish {
    pub fn maps(data: &GameData) -> Vec<&MapEntry> {
        data.skirmish_maps().collect()
    }

    pub fn map_index(&self, data: &GameData) -> Option<usize> {
        let id = self.map.as_deref()?;
        Self::maps(data).iter().position(|m| m.id() == id)
    }

    /// Chooses a map; keeps the human's seat where the new map has it.
    pub fn choose_map(&mut self, data: &GameData, index: usize) {
        let maps = Self::maps(data);
        let Some(map) = maps.get(index) else {
            return;
        };
        let human = self.slots.iter().find(|s| s.human).map(|s| s.player);
        let old = std::mem::take(&mut self.slots);
        let default_race = widgets::races(data)
            .first()
            .map(|r| r.0.clone())
            .unwrap_or_else(|| data.content.rules.default_race.clone());
        self.slots = map
            .starts
            .iter()
            .enumerate()
            .map(|(i, &player)| {
                let kept = old.iter().find(|s| s.player == player);
                let race = kept.map_or(default_race.clone(), |s| s.race.clone());
                let hero = kept
                    .and_then(|s| s.hero.clone())
                    .or_else(|| widgets::heroes(data, &race).1);
                SlotChoice {
                    player,
                    human: match human {
                        Some(h) => h == player,
                        None => i == 0,
                    },
                    race,
                    hero,
                    team: kept.and_then(|s| s.team),
                }
            })
            .collect();
        if !self.slots.iter().any(|s| s.human)
            && let Some(first) = self.slots.first_mut()
        {
            first.human = human.is_none();
        }
        self.map = Some(map.id().to_string());
    }

    /// Makes slot `i` the human (every other slot becomes a computer) or a computer.
    pub fn set_human(&mut self, i: usize, human: bool) {
        if i >= self.slots.len() {
            return;
        }
        for (j, slot) in self.slots.iter_mut().enumerate() {
            if j == i {
                slot.human = human;
            } else if human {
                slot.human = false;
            }
        }
    }

    pub fn set_race(&mut self, data: &GameData, i: usize, race: &str) {
        if let Some(slot) = self.slots.get_mut(i)
            && slot.race != race
        {
            slot.race = race.to_string();
            slot.hero = widgets::heroes(data, race).1;
        }
    }

    pub fn human(&self) -> Option<u8> {
        self.slots
            .iter()
            .find(|s| s.human)
            .map(|s| s.player.saturating_sub(1))
    }

    /// The frozen match: slots in start order, the human named.
    pub fn setup(&self, seed: u32, name: &str) -> MatchSetup {
        MatchSetup {
            seed: seed.max(1),
            slots: self
                .slots
                .iter()
                .map(|s| Slot {
                    player: s.player.saturating_sub(1),
                    kind: if s.human {
                        SlotKind::Human
                    } else {
                        SlotKind::Ai
                    },
                    name: s.human.then(|| name.to_string()),
                    team: s.team,
                    hero: s.hero.clone(),
                    race: Some(s.race.clone()),
                })
                .collect(),
        }
    }

    pub fn show(
        &self,
        host: &mut UiHost,
        gpu: &Gpu,
        data: &GameData,
        palette: &Palette,
        name: &str,
    ) {
        let maps = Self::maps(data);
        let index = self.map_index(data);
        widgets::map_list(host, gpu, data, "skirmish/maps", &maps, index);
        let map = index.and_then(|i| maps.get(i).copied());
        let me = self.human();
        widgets::map_preview(host, gpu, data, palette, "skirmish", map, me);
        let races = widgets::races(data);
        let race_names: Vec<String> = races.iter().map(|r| r.1.clone()).collect();
        let mut teams = vec!["No team".to_string()];
        teams.extend((1..=TEAMS).map(|t| format!("Team {t}")));
        let sandbox = map.is_some_and(|m| m.sandbox);
        if let Some(list) = host.ui.find("skirmish/players") {
            host.ui.set_count(list, self.slots.len());
        }
        for (i, slot) in self.slots.iter().enumerate() {
            let base = format!("skirmish/players/{i}");
            let ui = &mut host.ui;
            if let Some(badge) = ui.find(&format!("{base}/badge")) {
                ui.set_text(badge, &slot.player.to_string());
                ui.set_color(badge, Some(palette.badge(slot.player.saturating_sub(1))));
            }
            widgets::text(
                ui,
                &format!("{base}/name"),
                &format!("Player {}", slot.player),
            );
            let sub = if slot.human {
                if name.is_empty() { "You" } else { name }
            } else {
                "Computer"
            };
            widgets::text(ui, &format!("{base}/sub"), sub);
            widgets::options(
                ui,
                &format!("{base}/controller"),
                &["Human".to_string(), "Computer".to_string()],
                usize::from(!slot.human),
            );
            widgets::options(
                ui,
                &format!("{base}/team"),
                &teams,
                slot.team.map_or(0, |t| usize::from(t) + 1),
            );
            let race = races.iter().position(|r| r.0 == slot.race).unwrap_or(0);
            widgets::options(ui, &format!("{base}/race"), &race_names, race);
            let (heroes, _) = widgets::heroes(data, &slot.race);
            let hero_names: Vec<String> = heroes.iter().map(|h| h.1.clone()).collect();
            let hero = slot
                .hero
                .as_ref()
                .and_then(|h| heroes.iter().position(|x| &x.0 == h))
                .unwrap_or(0);
            widgets::options(ui, &format!("{base}/hero"), &hero_names, hero);
            for part in ["controller", "team"] {
                widgets::enabled(ui, &format!("{base}/{part}"), !sandbox);
            }
            for part in ["race", "hero"] {
                widgets::visible(ui, &format!("{base}/{part}"), !sandbox);
            }
            widgets::enabled(ui, &format!("{base}/hero"), hero_names.len() > 1);
        }
        let ui = &mut host.ui;
        widgets::text(
            ui,
            "skirmish/status",
            &match me {
                Some(p) => format!("Playing as Player {}", p + 1),
                None => "Observer · Watch all colonies".into(),
            },
        );
        widgets::text(
            ui,
            "skirmish/rules-caption",
            if sandbox { "Testbed" } else { "Victory" },
        );
        widgets::text(
            ui,
            "skirmish/rules",
            if sandbox {
                "A sandbox map: no victory, try anything."
            } else {
                "Destroy all rival buildings. The last surviving side wins. Computer players build their economy, lead their hero and command their own army."
            },
        );
        let label = if map.is_none() {
            "No maps available"
        } else if sandbox {
            "Open testbed →"
        } else if me.is_none() {
            "Watch match →"
        } else {
            "Start skirmish →"
        };
        widgets::text(ui, "skirmish/start", label);
        widgets::enabled(ui, "skirmish/start", map.is_some());
    }

    /// Applies a dropdown choice; `field` is the action's last word.
    pub fn choose(&mut self, data: &GameData, field: &str, slot: usize, option: usize) {
        match field {
            "controller" => self.set_human(slot, option == 0),
            "team" => {
                if let Some(s) = self.slots.get_mut(slot) {
                    s.team = (option > 0).then(|| (option - 1) as u8);
                }
            }
            "race" => {
                if let Some((id, _)) = widgets::races(data).get(option) {
                    let id = id.clone();
                    self.set_race(data, slot, &id);
                }
            }
            "hero" => {
                if let Some(s) = self.slots.get_mut(slot) {
                    let (heroes, _) = widgets::heroes(data, &s.race);
                    if let Some((id, _)) = heroes.get(option) {
                        s.hero = Some(id.clone());
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(player: u8, human: bool) -> SlotChoice {
        SlotChoice {
            player,
            human,
            race: "r".into(),
            hero: None,
            team: None,
        }
    }

    #[test]
    fn at_most_one_human_and_teams_reach_the_setup() {
        let mut s = Skirmish {
            map: Some("m".into()),
            slots: vec![slot(1, true), slot(2, false), slot(3, false)],
        };
        s.set_human(2, true);
        assert_eq!(s.human(), Some(2));
        assert_eq!(s.slots.iter().filter(|x| x.human).count(), 1);
        s.set_human(2, false);
        assert_eq!(s.human(), None, "everyone a computer: watch the match");
        s.slots[0].team = Some(0);
        s.slots[1].team = Some(0);
        s.set_human(0, true);
        let setup = s.setup(7, "Ann");
        assert_eq!(setup.seed, 7);
        let players: Vec<u8> = setup.slots.iter().map(|x| x.player).collect();
        assert_eq!(players, [0, 1, 2]);
        assert_eq!(setup.slots[0].kind, SlotKind::Human);
        assert_eq!(setup.slots[0].name.as_deref(), Some("Ann"));
        assert_eq!(setup.slots[1].kind, SlotKind::Ai);
        assert_eq!(setup.slots[1].team, Some(0));
        assert_eq!(setup.slots[2].team, None);
        let teams = canopy_sim::teams::Teams::new(&setup);
        assert_eq!(
            teams.relation(
                canopy_content::placement::Owner::Player(0),
                canopy_content::placement::Owner::Player(1)
            ),
            canopy_sim::teams::Relation::Ally
        );
    }
}
