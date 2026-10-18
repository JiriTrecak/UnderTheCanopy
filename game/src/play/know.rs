//! What the local player knows, with the questions input and the HUD ask of it: the
//! seat's [`View`] plus the content to read definitions from. Nothing here reads the
//! simulation's state: every answer comes from the view (or the static map).

use canopy_content::{
    Content,
    game::{Definition, Kind, SelectionClass},
    placement::Owner,
};
use canopy_sim::{catalog::Catalog, view::EntityView, view::View};
use std::sync::Arc;

/// Selection rank (old `prioritizeSelection`): heroes, army, workers, the rest.
pub fn rank(d: &Definition) -> u8 {
    if d.is_hero() {
        3
    } else if d.selection_class == Some(SelectionClass::Army) {
        2
    } else if d.selection_class == Some(SelectionClass::Worker) || d.behaviors.work.is_some() {
        1
    } else {
        0
    }
}

/// The content and the latest view, read together.
#[derive(Clone)]
pub struct Knowledge {
    pub content: Arc<Content>,
    pub catalog: Arc<Catalog>,
    pub view: Arc<View>,
    /// The local player's slot; `None` when observing.
    pub me: Option<u8>,
    /// Own units held inside another entity or waiting to be placed back (sorted).
    pub held: Arc<Vec<u32>>,
    /// Known resources a known building stands on (`View::covered`): only the building is
    /// drawn, picked and selected.
    pub covered: Arc<std::collections::BTreeSet<canopy_sim::entity::EntityId>>,
}

impl Knowledge {
    pub fn new(
        content: Arc<Content>,
        catalog: Arc<Catalog>,
        view: Arc<View>,
        me: Option<u8>,
    ) -> Self {
        let covered = Arc::new(view.covered(&catalog));
        Self {
            content,
            catalog,
            view,
            me,
            held: Arc::new(Vec::new()),
            covered,
        }
    }

    pub fn owner(&self) -> Option<Owner> {
        self.me.map(Owner::Player)
    }

    pub fn entity(&self, id: u32) -> Option<&EntityView> {
        self.view.entity(canopy_sim::entity::EntityId(id))
    }

    pub fn def(&self, e: &EntityView) -> &Definition {
        self.catalog.get(e.definition())
    }

    pub fn def_of(&self, id: u32) -> Option<&Definition> {
        self.entity(id).map(|e| self.def(e))
    }

    pub fn alive(&self, e: &EntityView) -> bool {
        e.public.hp != Some(0) && !self.view.battle.fallen.contains(&e.id())
    }

    pub fn held(&self, id: u32) -> bool {
        self.held.binary_search(&id).is_ok()
    }

    /// The viewer's own entity, seen (not a memory).
    pub fn own(&self, e: &EntityView) -> bool {
        !e.remembered && self.owner().is_some_and(|o| e.public.owner == o)
    }

    /// Own, alive, under player control and not inside anything (old "controlled").
    pub fn controlled(&self, e: &EntityView) -> bool {
        self.own(e)
            && self.alive(e)
            && self.def(e).behaviors.player_control.is_some()
            && !self.held(e.id().0)
    }

    pub fn is_unit(&self, e: &EntityView) -> bool {
        self.def(e).kind == Kind::Unit
    }

    /// Whether the pointer picks it by its drawn body (units and items) rather than by
    /// the ground point under it.
    pub fn picked_by_body(&self, e: &EntityView) -> bool {
        self.is_unit(e) || self.def(e).kind == Kind::Item
    }

    pub fn is_building(&self, e: &EntityView) -> bool {
        self.def(e).kind == Kind::Building
    }

    /// May be clicked or box-selected at all.
    pub fn selectable(&self, e: &EntityView) -> bool {
        self.alive(e)
            && self.def(e).selectable != Some(false)
            && !self.held(e.id().0)
            && !self.covered.contains(&e.id())
    }

    /// Position in world units (x east, z south).
    pub fn position(&self, e: &EntityView) -> [f32; 2] {
        [
            e.public.position.x as f32 / 1000.0,
            e.public.position.y as f32 / 1000.0,
        ]
    }

    pub fn rank_of(&self, id: u32) -> u8 {
        self.def_of(id).map_or(0, rank)
    }

    /// The name and icon image of a content icon id (`icon.action.move`).
    pub fn icon_image(&self, icon: &str) -> Option<&str> {
        self.content.asset(icon).and_then(|a| a.image.as_deref())
    }

    /// The displayed name of an item or definition id.
    pub fn name_of(&self, id: &str) -> String {
        self.content
            .definition(id)
            .map_or_else(|| id.to_owned(), |d| d.name.clone())
    }

    /// Known scenery by id (scenery is listed in id order, in chunks of ids).
    pub fn sighting(&self, id: u32) -> Option<&canopy_sim::vision::knowledge::Sighting> {
        let chunk = self
            .view
            .scenery
            .chunks
            .get((id / canopy_sim::vision::scenery::CHUNK) as usize)?;
        chunk
            .binary_search_by_key(&id, |s| s.id.0)
            .ok()
            .map(|i| &chunk[i])
    }

    /// Known scenery that still stands with something to harvest: not depleted, not
    /// falling (a struck tree stands, and is harvested, until it falls).
    pub fn standing(&self, id: u32) -> bool {
        self.sighting(id)
            .and_then(|s| s.resource.as_ref())
            .is_some_and(|r| {
                r.amount > 0 && r.felling.as_ref().is_none_or(|f| f.fall_tick.is_none())
            })
    }

    /// The tick of the view.
    pub fn tick(&self) -> u32 {
        self.view.tick
    }
}
