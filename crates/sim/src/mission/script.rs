//! A map's mission compiled for the simulation: the script and everything it names.

use super::sandbox;
use canopy_content::{
    map::{Mission, Objective, Region, UtcMap},
    placement::{Camp, Owner, Placement},
};

/// Immutable for the match; rebuilt from the map on restore.
#[derive(Debug)]
pub struct Script {
    pub mission: Mission,
    /// The map's placements, script-activated ones included, sorted by id. Scripts address
    /// entities by placement id; numeric entity ids stay private.
    placements: Vec<Placement>,
    camps: Vec<Camp>,
    /// Owners with a player start.
    owners: Vec<Owner>,
    /// Cells per side.
    pub size: u32,
}

impl Script {
    /// The map's mission, if it has one. Its script must compile.
    pub fn new(map: &UtcMap) -> Result<Option<Self>, String> {
        let Some(mission) = &map.mission else {
            return Ok(None);
        };
        sandbox::validate(&mission.script)?;
        let mut placements = map.entities.clone();
        placements.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Some(Self {
            mission: mission.clone(),
            placements,
            camps: map.camps.clone(),
            owners: map.player_starts.iter().map(|s| s.owner()).collect(),
            size: map.size,
        }))
    }

    pub fn placement(&self, id: &str) -> Option<&Placement> {
        self.placements
            .binary_search_by(|p| p.id.as_str().cmp(id))
            .ok()
            .map(|index| &self.placements[index])
    }

    pub fn camp(&self, id: &str) -> Option<&Camp> {
        self.camps.iter().find(|c| c.id == id)
    }

    /// The camp a placement belongs to.
    pub fn camp_of(&self, placement: &str) -> Option<&Camp> {
        self.camps
            .iter()
            .find(|c| c.members.iter().any(|m| m == placement))
    }

    pub fn region(&self, id: &str) -> Option<&Region> {
        self.mission.regions.iter().find(|r| r.id == id)
    }

    pub fn objectives(&self) -> &[Objective] {
        self.mission.objectives.as_deref().unwrap_or_default()
    }

    pub fn objective(&self, id: &str) -> Option<&Objective> {
        self.objectives().iter().find(|o| o.id == id)
    }

    /// Nobody, or a player with a start on this map.
    pub fn seats(&self, owner: Owner) -> bool {
        owner == Owner::Nobody || self.owners.contains(&owner)
    }
}
