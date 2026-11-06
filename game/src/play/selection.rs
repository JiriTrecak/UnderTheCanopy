//! Selection and control groups (old `presentation/selection.ts`, `controlGroups.ts`,
//! `areaSelection` and the click rules of `session.ts`).
//!
//! A selection has a fixed **card order**, as Warcraft III and StarCraft II keep it:
//! by rank (hero, army, worker, other), then by type, so each type's cards stand
//! together as a subgroup, then in the order given. Its **focus** supplies the portrait,
//! the special commands and the build menu; Tab moves the focus from subgroup to
//! subgroup without moving any card.

use std::time::Duration;

/// A second press within this window centres the camera (groups, hero keys) and a
/// second click selects the same type.
pub const DOUBLE_PRESS: Duration = Duration::from_millis(350);
/// Control group slots (keys 1–9 and 0).
pub const GROUPS: usize = 10;

/// What selection needs to know about a candidate entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidate {
    pub id: u32,
    /// Selection rank: hero 3, army 2, worker 1, other 0.
    pub rank: u8,
    /// Own, alive, player-controlled and not inside anything.
    pub controlled: bool,
    pub unit: bool,
    pub building: bool,
    pub hero: bool,
    pub army: bool,
    pub worker: bool,
    /// Definition index, for "same type".
    pub kind: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    /// The cards in their fixed order.
    cards: Vec<u32>,
    /// The focus, then the other cards in order: what commands read.
    ids: Vec<u32>,
}

impl Selection {
    /// The focus first, then the rest in card order.
    pub fn ids(&self) -> &[u32] {
        &self.ids
    }

    /// The selection in the order its cards show.
    pub fn cards(&self) -> &[u32] {
        &self.cards
    }

    pub fn focus(&self) -> Option<u32> {
        self.ids.first().copied()
    }

    pub fn contains(&self, id: u32) -> bool {
        self.cards.contains(&id)
    }

    /// Replaces the selection. `order` gives each id's (rank, type): cards go by rank,
    /// then type, then as given. The focus is `focus` if listed, else the first card.
    pub fn set(&mut self, ids: Vec<u32>, focus: Option<u32>, order: impl Fn(u32) -> (u8, u16)) {
        let mut cards = dedup(ids);
        // A stable sort keeps the given order within a type.
        cards.sort_by_key(|&id| {
            let (rank, kind) = order(id);
            (std::cmp::Reverse(rank), kind)
        });
        self.cards = cards;
        self.focus_or_first(focus);
    }

    /// Drops ids `keep` refuses; the cards keep their order. Returns whether the focus
    /// was lost (it then passes to the first card).
    pub fn prune(&mut self, keep: impl Fn(u32) -> bool) -> bool {
        let focus = self.focus();
        self.cards.retain(|&id| keep(id));
        let lost = focus.is_some_and(|f| !self.cards.contains(&f));
        self.focus_or_first(if lost { None } else { focus });
        lost
    }

    /// Makes `id` the focus, if it is selected.
    pub fn focus_on(&mut self, id: u32) {
        if self.cards.contains(&id) {
            self.focus_or_first(Some(id));
        }
    }

    /// Removes one id (shift-click on a selection card).
    pub fn remove(&mut self, id: u32) {
        let focus = self.focus().filter(|&f| f != id);
        self.cards.retain(|&x| x != id);
        self.focus_or_first(focus);
    }

    fn focus_or_first(&mut self, focus: Option<u32>) {
        let focus = focus
            .filter(|f| self.cards.contains(f))
            .or_else(|| self.cards.first().copied());
        self.ids.clear();
        self.ids.extend(focus);
        self.ids
            .extend(self.cards.iter().copied().filter(|&id| Some(id) != focus));
    }

    /// Tab / Shift+Tab: focus the first card of the next (or previous) subgroup, in card
    /// order; needs two types. No card moves.
    pub fn cycle(&mut self, candidates: &[Candidate], forward: bool) {
        let mut kinds: Vec<(u8, u16)> = Vec::new();
        for c in self
            .cards
            .iter()
            .filter_map(|id| candidates.iter().find(|c| c.id == *id))
        {
            if !kinds.contains(&(c.rank, c.kind)) {
                kinds.push((c.rank, c.kind));
            }
        }
        if kinds.len() < 2 {
            return;
        }
        let current = self
            .focus()
            .and_then(|f| candidates.iter().find(|c| c.id == f))
            .map(|c| (c.rank, c.kind));
        let at = current
            .and_then(|k| kinds.iter().position(|&x| x == k))
            .unwrap_or(0);
        let n = kinds.len();
        let next = kinds[if forward {
            (at + 1) % n
        } else {
            (at + n - 1) % n
        }];
        let first = self.cards.iter().copied().find(|id| {
            candidates
                .iter()
                .any(|c| c.id == *id && (c.rank, c.kind) == next)
        });
        if let Some(id) = first {
            self.focus_on(id);
        }
    }
}

fn dedup(ids: Vec<u32>) -> Vec<u32> {
    let mut seen = std::collections::BTreeSet::new();
    ids.into_iter().filter(|id| seen.insert(*id)).collect()
}

/// Box selection (old `areaSelection`): own controlled units first; if any hero or army
/// unit is inside, only those; otherwise workers; with no own unit inside, own buildings.
/// Observers get every unit in the box.
pub fn area(inside: &[Candidate], observer: bool) -> Vec<u32> {
    if observer {
        return inside.iter().filter(|c| c.unit).map(|c| c.id).collect();
    }
    let own: Vec<&Candidate> = inside.iter().filter(|c| c.unit && c.controlled).collect();
    if own.iter().any(|c| c.hero || c.army) {
        own.iter()
            .filter(|c| c.hero || c.army)
            .map(|c| c.id)
            .collect()
    } else if own.iter().any(|c| c.worker) {
        own.iter().filter(|c| c.worker).map(|c| c.id).collect()
    } else {
        inside
            .iter()
            .filter(|c| c.building && c.controlled)
            .map(|c| c.id)
            .collect()
    }
}

/// Ten control groups (old `ControlGroups`): stable ids, pruned as entities die.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ControlGroups {
    groups: [Vec<u32>; GROUPS],
    /// The last recalled slot and when, for the double press.
    last: Option<(usize, Duration)>,
}

/// What recalling a group asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recall {
    /// Select these.
    Select(Vec<u32>),
    /// Select these and centre the camera on them (second press).
    Focus(Vec<u32>),
}

impl ControlGroups {
    pub fn get(&self, slot: usize) -> &[u32] {
        self.groups.get(slot).map_or(&[], |g| g.as_slice())
    }

    /// Ctrl+N: the group becomes the (eligible part of the) selection.
    pub fn assign(&mut self, slot: usize, ids: &[u32]) {
        if let Some(g) = self.groups.get_mut(slot) {
            *g = dedup(ids.to_vec());
        }
        self.last = None;
    }

    /// Shift+N: adds to the group.
    pub fn add(&mut self, slot: usize, ids: &[u32]) {
        if let Some(g) = self.groups.get_mut(slot) {
            let mut all = std::mem::take(g);
            all.extend_from_slice(ids);
            *g = dedup(all);
        }
        self.last = None;
    }

    /// N at time `now`: selects the group; a second press within [`DOUBLE_PRESS`]
    /// focuses it. An empty group does nothing.
    pub fn recall(&mut self, slot: usize, now: Duration) -> Option<Recall> {
        let ids = self.get(slot).to_vec();
        if ids.is_empty() {
            return None;
        }
        let double = self
            .last
            .is_some_and(|(s, t)| s == slot && now.saturating_sub(t) <= DOUBLE_PRESS);
        self.last = Some((slot, now));
        Some(if double {
            Recall::Focus(ids)
        } else {
            Recall::Select(ids)
        })
    }

    /// Drops members `keep` refuses (dead, gone, no longer owned).
    pub fn prune(&mut self, keep: impl Fn(u32) -> bool) {
        for g in &mut self.groups {
            g.retain(|&id| keep(id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: u32, rank: u8, kind: u16) -> Candidate {
        Candidate {
            id,
            rank,
            controlled: true,
            unit: rank > 0,
            building: rank == 0,
            hero: rank == 3,
            army: rank == 2,
            worker: rank == 1,
            kind,
        }
    }

    #[test]
    fn selections_order_cards_by_rank_then_type_and_focus_without_moving_them() {
        // (id, rank, type)
        let units = [
            (1, 1, 5),
            (2, 2, 7),
            (3, 3, 9),
            (4, 2, 6),
            (5, 0, 1),
            (6, 2, 7),
        ];
        let order = |id: u32| {
            units
                .iter()
                .find(|u| u.0 == id)
                .map_or((0, 0), |u| (u.1, u.2))
        };
        let mut s = Selection::default();
        s.set(vec![1, 2, 3, 4, 5, 2, 6], None, order);
        assert_eq!(s.cards(), [3, 4, 2, 6, 1, 5]);
        assert_eq!(s.ids(), [3, 4, 2, 6, 1, 5]);
        s.set(vec![1, 2, 3, 4, 5, 6], Some(1), order);
        assert_eq!(s.cards(), [3, 4, 2, 6, 1, 5]);
        assert_eq!(s.ids(), [1, 3, 4, 2, 6, 5]);
        // Losing the focus passes it to the first card; the cards keep their order.
        assert!(s.prune(|id| id != 1));
        assert_eq!(s.cards(), [3, 4, 2, 6, 5]);
        assert_eq!(s.focus(), Some(3));
        assert!(!s.prune(|id| id != 5));
        assert_eq!(s.focus(), Some(3));
        s.focus_on(6);
        assert_eq!(s.cards(), [3, 4, 2, 6]);
        assert_eq!(s.ids(), [6, 3, 4, 2]);
        s.remove(6);
        assert_eq!(s.focus(), Some(3));
    }

    #[test]
    fn box_selection_prefers_the_army_then_workers_then_buildings() {
        let mut enemy = c(9, 2, 1);
        enemy.controlled = false;
        let mixed = [c(1, 1, 5), c(2, 2, 6), c(3, 3, 7), enemy];
        assert_eq!(area(&mixed, false), [2, 3]);
        let workers = [c(1, 1, 5), c(4, 1, 5), enemy];
        assert_eq!(area(&workers, false), [1, 4]);
        assert!(area(&[enemy], false).is_empty());
        // Buildings only when no own unit is inside.
        let base = [c(6, 0, 2), c(7, 0, 3), enemy];
        assert_eq!(area(&base, false), [6, 7]);
        assert_eq!(area(&[c(6, 0, 2), c(1, 1, 5)], false), [1]);
        assert_eq!(area(&mixed, true), [1, 2, 3, 9]);
    }

    #[test]
    fn tab_moves_the_focus_through_subgroups_and_keeps_the_cards_in_place() {
        let cands = [c(1, 2, 10), c(2, 2, 10), c(3, 1, 4), c(4, 3, 8), c(5, 2, 9)];
        let mut s = Selection::default();
        s.set(vec![1, 2, 3, 4, 5], None, |id| {
            let c = cands.iter().find(|c| c.id == id).unwrap();
            (c.rank, c.kind)
        });
        let cards = [4, 5, 1, 2, 3];
        assert_eq!(s.cards(), cards);
        assert_eq!(s.focus(), Some(4));
        for focus in [5, 1, 3, 4] {
            s.cycle(&cands, true);
            assert_eq!(s.focus(), Some(focus));
            assert_eq!(s.cards(), cards);
        }
        s.cycle(&cands, false);
        assert_eq!(s.focus(), Some(3));
        assert_eq!(s.cards(), cards);
        assert_eq!(s.ids().len(), 5);
    }

    #[test]
    fn control_groups_assign_add_recall_and_focus_on_a_double_press() {
        let mut g = ControlGroups::default();
        let ms = Duration::from_millis;
        assert_eq!(g.recall(0, ms(0)), None);
        g.assign(0, &[3, 1, 3]);
        assert_eq!(g.get(0), [3, 1]);
        g.add(0, &[1, 7]);
        assert_eq!(g.get(0), [3, 1, 7]);
        assert_eq!(g.recall(0, ms(1000)), Some(Recall::Select(vec![3, 1, 7])));
        assert_eq!(g.recall(0, ms(1300)), Some(Recall::Focus(vec![3, 1, 7])));
        assert_eq!(g.recall(0, ms(2000)), Some(Recall::Select(vec![3, 1, 7])));
        g.assign(1, &[5]);
        assert_eq!(g.recall(0, ms(2100)), Some(Recall::Select(vec![3, 1, 7])));
        g.prune(|id| id != 1);
        assert_eq!(g.get(0), [3, 7]);
    }
}
