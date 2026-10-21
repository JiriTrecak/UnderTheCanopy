//! What Place can put down, in categories, each in groups: buildings and units by race
//! (and the neutral ones), creep camps by strength, the special pieces (player starts,
//! amber and substance nodes), and the world's scenery by its category. Made from the
//! content whenever it changes, so new definitions and scenery appear by themselves; a
//! new kind of thing is a new category here.

use super::Placing;
use crate::map::tools::Tier;
use canopy_content::Content;
use canopy_content::assets::SceneryCategory;
use canopy_content::game::{Definition, Kind};

/// One thing to place.
#[derive(Debug, Clone)]
pub struct Entry {
    pub placing: Placing,
    pub name: String,
    /// Its picture: a pack entry (a definition's icon).
    pub icon: Option<String>,
    /// Whether it belongs to no player (creeps, nodes, neutral buildings).
    pub neutral: bool,
}

#[derive(Debug, Clone)]
pub struct Group {
    pub name: String,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone)]
pub struct Category {
    pub name: String,
    pub groups: Vec<Group>,
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub categories: Vec<Category>,
    /// The content it was made from (its address), to make it again when that changes.
    pub source: usize,
}

/// An amber mine: a building that is harvested and yields.
pub fn is_mine(d: &Definition) -> bool {
    d.kind == Kind::Building && d.harvesting.is_some() && d.resource_yield.is_some()
}

impl Catalog {
    pub fn new(content: &Content) -> Self {
        let icon = |d: &Definition| Some(format!("{}/image.png", d.icon.as_str()));
        let entry = |d: &Definition, neutral: bool| Entry {
            placing: Placing::Definition(d.id.to_string()),
            name: d.name.clone(),
            icon: icon(d),
            neutral,
        };
        fn sorted(mut list: Vec<&Definition>) -> Vec<&Definition> {
            list.sort_by(|a, b| {
                (a.display_order.unwrap_or(i64::MAX), &a.name)
                    .cmp(&(b.display_order.unwrap_or(i64::MAX), &b.name))
            });
            list
        }
        // What some building is placed on (a substance deposit), and the amber mine.
        let deposits: Vec<&str> = content
            .definitions
            .iter()
            .filter_map(|d| d.placement_on.as_ref().map(|p| p.source.as_str()))
            .collect();
        let special = |d: &Definition| is_mine(d) || deposits.contains(&d.id.as_str());
        let in_roster = |d: &Definition| {
            content
                .rules
                .races
                .values()
                .any(|r| r.roster.iter().any(|id| id == &d.id))
        };
        let by_kind = |kind: Kind| {
            let mut groups: Vec<Group> = content
                .rules
                .races
                .values()
                .map(|race| Group {
                    name: race.name.clone(),
                    entries: sorted(
                        race.roster
                            .iter()
                            .filter_map(|id| content.definition(id.as_str()))
                            .filter(|d| d.kind == kind)
                            .collect(),
                    )
                    .into_iter()
                    .map(|d| entry(d, false))
                    .collect(),
                })
                .filter(|g| !g.entries.is_empty())
                .collect();
            let neutral: Vec<Entry> = sorted(
                content
                    .definitions
                    .iter()
                    .filter(|d| d.kind == kind && !in_roster(d) && !special(d))
                    .collect(),
            )
            .into_iter()
            .map(|d| entry(d, true))
            .collect();
            if !neutral.is_empty() {
                groups.push(Group {
                    name: "Neutral".into(),
                    entries: neutral,
                });
            }
            groups
        };
        let camps = Group {
            name: "Camps".into(),
            entries: Tier::ALL
                .into_iter()
                .map(|tier| Entry {
                    placing: Placing::Camp(tier),
                    name: format!("{} camp", tier.name()),
                    icon: None,
                    neutral: true,
                })
                .collect(),
        };
        let mut nodes: Vec<Entry> = content
            .definitions
            .iter()
            .filter(|d| is_mine(d))
            .take(1)
            .map(|d| Entry {
                placing: Placing::Amber,
                name: format!("{} node", d.name),
                icon: icon(d),
                neutral: true,
            })
            .collect();
        nodes.extend(
            content
                .definitions
                .iter()
                .filter(|d| deposits.contains(&d.id.as_str()))
                .map(|d| entry(d, true)),
        );
        let starts = Group {
            name: "Player starts".into(),
            entries: (1..=8u8)
                .map(|n| Entry {
                    placing: Placing::Start(n),
                    name: format!("Start {n}"),
                    icon: None,
                    neutral: false,
                })
                .collect(),
        };
        let mut scenery: Vec<Group> = Vec::new();
        for item in canopy_content::assets::scenery(&content.manifest)
            .filter(|s| s.editor_hidden != Some(true) && s.deck.is_none())
        {
            let group = match item.category {
                SceneryCategory::Foliage => "Plants and trees",
                SceneryCategory::Landmark => "Landmarks",
                SceneryCategory::Terrain => "Ground",
                SceneryCategory::Water => "Water",
                SceneryCategory::Resource => "Resources",
                SceneryCategory::Units => "Creatures",
                SceneryCategory::Other => "Other",
            };
            let entry = Entry {
                placing: Placing::Scenery(item.id.to_string()),
                name: item.name.clone(),
                icon: None,
                neutral: true,
            };
            match scenery.iter_mut().find(|g| g.name == group) {
                Some(g) => g.entries.push(entry),
                None => scenery.push(Group {
                    name: group.into(),
                    entries: vec![entry],
                }),
            }
        }
        for group in &mut scenery {
            group.entries.sort_by(|a, b| a.name.cmp(&b.name));
        }
        let categories = vec![
            Category {
                name: "Buildings".into(),
                groups: by_kind(Kind::Building),
            },
            Category {
                name: "Units".into(),
                groups: by_kind(Kind::Unit),
            },
            Category {
                name: "Camps".into(),
                groups: vec![camps],
            },
            Category {
                name: "Special".into(),
                groups: vec![
                    starts,
                    Group {
                        name: "Resource nodes".into(),
                        entries: nodes,
                    },
                ],
            },
            Category {
                name: "World".into(),
                groups: scenery,
            },
        ];
        Self {
            categories: categories
                .into_iter()
                .filter(|c| c.groups.iter().any(|g| !g.entries.is_empty()))
                .collect(),
            source: 0,
        }
    }

    /// The entry that places `placing`.
    pub fn find(&self, placing: &Placing) -> Option<&Entry> {
        self.categories
            .iter()
            .flat_map(|c| &c.groups)
            .flat_map(|g| &g.entries)
            .find(|e| &e.placing == placing)
    }
}
