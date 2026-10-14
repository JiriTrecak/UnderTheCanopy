//! From the simulation to effect sounds: ability events, statuses, deliveries in flight
//! and returning heroes as the player's view shows them.

use crate::effects::{DeliverySighting, EffectSounds, ReturnSighting, StatusSighting, Trigger};
use canopy_content::effects::EffectEvent;
use canopy_sim::{
    abilities::{AbilityEvent, EventKind},
    entity::Milli,
    view::View,
};

/// The content event an ability event kind plays effects for.
pub fn effect_event(kind: EventKind) -> EffectEvent {
    match kind {
        EventKind::Accepted => EffectEvent::Accepted,
        EventKind::Released => EffectEvent::Released,
        EventKind::Cancelled => EffectEvent::Cancelled,
        EventKind::Finished => EffectEvent::Finished,
        EventKind::WaveStarted => EffectEvent::WaveStarted,
        EventKind::Wave => EffectEvent::Wave,
        EventKind::Projectile => EffectEvent::Projectile,
        EventKind::Impact => EffectEvent::Impact,
        EventKind::Healed => EffectEvent::Healed,
        EventKind::Damaged => EffectEvent::Damaged,
        EventKind::Drained => EffectEvent::Drained,
        EventKind::ManaRestored => EffectEvent::ManaRestored,
        EventKind::StatusApplied => EffectEvent::StatusApplied,
        EventKind::Dispelled => EffectEvent::Dispelled,
        EventKind::Summoned => EffectEvent::Summoned,
        EventKind::Resurrected => EffectEvent::Resurrected,
        EventKind::RevivalStarted => EffectEvent::RevivalStarted,
        EventKind::Revived => EffectEvent::Revived,
        EventKind::RevivalCancelled => EffectEvent::RevivalCancelled,
        EventKind::SplitStarted => EffectEvent::SplitStarted,
        EventKind::SplitEnded => EffectEvent::SplitEnded,
        EventKind::Teleported => EffectEvent::Teleported,
        EventKind::VisionCreated => EffectEvent::VisionCreated,
        EventKind::Converted => EffectEvent::Converted,
        EventKind::Contained => EffectEvent::Contained,
        EventKind::ReleasedContained => EffectEvent::ReleasedContained,
        EventKind::Digested => EffectEvent::Digested,
        EventKind::Sacrificed => EffectEvent::Sacrificed,
        EventKind::Returned => EffectEvent::Returned,
        EventKind::Death => EffectEvent::Death,
        EventKind::Kill => EffectEvent::Kill,
        EventKind::WeaponRelease => EffectEvent::WeaponRelease,
        EventKind::Interval => EffectEvent::Interval,
        EventKind::CriticalStrike => EffectEvent::CriticalStrike,
        EventKind::Evaded => EffectEvent::Evaded,
        EventKind::Cleaved => EffectEvent::Cleaved,
        EventKind::WeaponEnhanced => EffectEvent::WeaponEnhanced,
        EventKind::EnhancedHit => EffectEvent::EnhancedHit,
        EventKind::WeaponOrdered => EffectEvent::WeaponOrdered,
        EventKind::WeaponOrderCancelled => EffectEvent::WeaponOrderCancelled,
    }
}

fn cells(m: Milli) -> (f32, f32) {
    (m.x as f32 / 1000.0, m.y as f32 / 1000.0)
}

fn milli_cells(x: i32, y: i32) -> (f32, f32) {
    cells(Milli { x, y })
}

/// The trigger of an ability event.
pub fn trigger(event: &AbilityEvent) -> Trigger<'_> {
    Trigger {
        ability: usize::from(event.ability.0),
        event: effect_event(event.kind),
        cast: Some(event.cast),
        tick: event.tick,
        caster: Some(event.caster.0),
        target: event.target.map(|t| t.0),
        origin: milli_cells(event.origin.x, event.origin.y),
        point: milli_cells(event.point.x, event.point.y),
        duration_ticks: event.duration_ticks,
        status: event.status.as_deref(),
    }
}

impl EffectSounds {
    /// Plays the effects bound to one ability event (for callers that take events from
    /// `World::drain_events` and filter them by viewer themselves). Status, delivery and
    /// return cues also need [`EffectSounds::observe`] (or the `sync_*` calls) to end.
    pub fn ability_event(&mut self, event: &AbilityEvent) {
        self.trigger(&trigger(event));
    }

    /// Follows a new view of the simulation: plays the ability events the viewer has not
    /// heard yet, and starts and ends status, delivery and return cues. Call it with every
    /// view the game builds (once per simulation tick); a view older than the last one
    /// (a loaded game) clears every cue first.
    pub fn observe(&mut self, view: &View) {
        let _span = tracing::trace_span!("audio_observe").entered();
        let tick = view.tick;
        if tick < self.last_tick {
            self.clear();
        }
        self.last_tick = tick;
        self.set_positions(
            view.entities
                .iter()
                .filter(|e| !e.remembered)
                .map(|e| (e.id().0, cells(e.public.position))),
        );
        let events = &view.spells.events;
        if events
            .iter()
            .map(|e| e.id)
            .max()
            .is_some_and(|newest| newest < self.last_event)
        {
            // The event sequence started again: another world.
            self.last_event = 0;
        }
        let (last, mut newest) = (self.last_event, self.last_event);
        for event in events.iter().filter(|e| e.id > last) {
            newest = newest.max(event.id);
            self.trigger(&trigger(event));
        }
        self.last_event = newest;
        let statuses = view
            .entities
            .iter()
            .filter(|e| !e.remembered && e.public.hp.is_none_or(|hp| hp > 0))
            .filter_map(|e| Some((e, e.magic.as_ref()?)))
            .flat_map(|(e, magic)| {
                magic.statuses.iter().map(move |s| StatusSighting {
                    entity: e.id().0,
                    ability: usize::from(s.ability.0),
                    status: &s.status,
                    source: s.source.0,
                    started: s.started,
                    expires: s.expires,
                    aura: s.aura,
                    at: cells(e.public.position),
                })
            });
        self.sync_statuses(tick, statuses);
        self.sync_deliveries(
            tick,
            view.spells.deliveries.iter().map(|d| DeliverySighting {
                cast: d.cast,
                ability: usize::from(d.ability.0),
                at: milli_cells(d.position.0, d.position.1),
            }),
        );
        let returns: Vec<ReturnSighting> = view
            .spells
            .returns
            .iter()
            .map(|&(hero, ability, started, due)| ReturnSighting {
                hero: hero.0,
                ability: usize::from(ability.0),
                started,
                due,
                at: view.entity(hero).map(|e| cells(e.public.position)),
            })
            .collect();
        self.sync_returns(tick, returns);
    }
}
