//! Selection and control groups (old `presentation/selection.ts`, `controlGroups.ts`,
//! `areaSelection` and the click rules of `session.ts`).
//!
//! The selection is an ordered list of entity ids whose first entry is the **focus**: it
//! supplies the portrait, the special commands and the build menu. The order is a stable
//! sort by rank (hero, army, worker, other), with an explicitly focused id first.

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
    pub hero: bool,
    pub army: bool,
    pub worker: bool,
    /// Definition index, for "same type".
    pub kind: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    ids: Vec<u32>,
}

impl Selection {
    pub fn ids(&self) -> &[u32] {
        &self.ids
    }

    pub fn focus(&self) -> Option<u32> {
        self.ids.first().copied()
    }

    pub fn contains(&self, id: u32) -> bool {
        self.ids.contains(&id)
    }

    /// Replaces the selection, ordered by rank with `focus` (if listed) first.
    pub fn set(&mut self, ids: Vec<u32>, focus: Option<u32>, rank: impl Fn(u32) -> u8) {
        let mut ids = dedup(ids);
        // A stable sort keeps the given order within a rank.
        ids.sort_by_key(|&id| (Some(id) != focus, std::cmp::Reverse(rank(id))));
        self.ids = ids;
    }

    /// Drops ids `keep` refuses. Returns whether the focus was lost (the rest is then
    /// re-ordered by rank, as the old HUD did).
    pub fn prune(&mut self, keep: impl Fn(u32) -> bool, rank: impl Fn(u32) -> u8) -> bool {
        let focus = self.focus();
        self.ids.retain(|&id| keep(id));
        if self.focus() != focus && focus.is_some() {
            let ids = std::mem::take(&mut self.ids);
            self.set(ids, None, rank);
            return true;
        }
        false
    }

    /// Moves `id` to the front as the focus.
    pub fn focus_on(&mut self, id: u32) {
        if let Some(i) = self.ids.iter().position(|&x| x == id) {
            let id = self.ids.remove(i);
            self.ids.insert(0, id);
        }
    }

    /// Removes one id (shift-click on a selection card).
    pub fn remove(&mut self, id: u32) {
        self.ids.retain(|&x| x != id);
    }

    /// Tab / Shift+Tab: focus the first entity of the next (or previous) type in the
    /// selection, ordered by rank and then type; needs two types.
    pub fn cycle(&mut self, candidates: &[Candidate], forward: bool) {
        let mut kinds: Vec<(u8, u16)> = self
            .ids
            .iter()
            .filter_map(|id| candidates.iter().find(|c| c.id == *id))
            .map(|c| (c.rank, c.kind))
            .collect();
        kinds.sort_by_key(|&(rank, kind)| (std::cmp::Reverse(rank), kind));
        kinds.dedup();
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
        let first = self.ids.iter().copied().find(|id| {
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

/// Box selection (old `areaSelection`): own controlled units only; if any hero or army
/// unit is inside, only those; otherwise workers. Observers get every unit in the box.
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
    } else {
        own.iter().filter(|c| c.worker).map(|c| c.id).collect()
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
            unit: true,
            hero: rank == 3,
            army: rank == 2,
            worker: rank == 1,
            kind,
        }
    }

    #[test]
    fn selections_order_heroes_then_army_then_workers_and_keep_the_focus_first() {
        let ranks = [(1, 1), (2, 2), (3, 3), (4, 2), (5, 0)];
        let rank = |id: u32| ranks.iter().find(|r| r.0 == id).map_or(0, |r| r.1);
        let mut s = Selection::default();
        s.set(vec![1, 2, 3, 4, 5, 2], None, rank);
        assert_eq!(s.ids(), [3, 2, 4, 1, 5]);
        s.set(vec![1, 2, 3, 4, 5], Some(1), rank);
        assert_eq!(s.ids(), [1, 3, 2, 4, 5]);
        // Losing the focus re-orders the rest by rank.
        assert!(s.prune(|id| id != 1, rank));
        assert_eq!(s.ids(), [3, 2, 4, 5]);
        assert!(!s.prune(|id| id != 5, rank));
        assert_eq!(s.focus(), Some(3));
    }

    #[test]
    fn box_selection_prefers_the_army_then_workers() {
        let mut enemy = c(9, 2, 1);
        enemy.controlled = false;
        let mixed = [c(1, 1, 5), c(2, 2, 6), c(3, 3, 7), enemy];
        assert_eq!(area(&mixed, false), [2, 3]);
        let workers = [c(1, 1, 5), c(4, 1, 5), enemy];
        assert_eq!(area(&workers, false), [1, 4]);
        assert!(area(&[enemy], false).is_empty());
        assert_eq!(area(&mixed, true), [1, 2, 3, 9]);
    }

    #[test]
    fn tab_cycles_types_in_rank_order() {
        let cands = [c(1, 2, 10), c(2, 2, 10), c(3, 1, 4), c(4, 3, 8)];
        let mut s = Selection::default();
        s.set(vec![1, 2, 3, 4], None, |id| {
            cands.iter().find(|c| c.id == id).unwrap().rank
        });
        assert_eq!(s.focus(), Some(4));
        s.cycle(&cands, true);
        assert_eq!(s.focus(), Some(1));
        s.cycle(&cands, true);
        assert_eq!(s.focus(), Some(3));
        s.cycle(&cands, true);
        assert_eq!(s.focus(), Some(4));
        s.cycle(&cands, false);
        assert_eq!(s.focus(), Some(3));
        // Everything stays selected.
        assert_eq!(s.ids().len(), 4);
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
