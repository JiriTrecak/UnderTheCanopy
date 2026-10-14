//! Connection measurement and the shared input delay, as the match server does it (old
//! `src/net/latency.ts`).
//!
//! The server owns the measurement: it sends an opaque `latencyProbe` nonce, the client
//! echoes it at once as `latencyReply` (the I/O thread does, before any game code sees the
//! message), and the server times the round trip on its own clock. The client never
//! reports a duration. At start the server freezes one delay for every peer from the worst
//! recent player round trip. This module is the same algorithm, used by the in-process
//! mailbox's tests and to explain the lobby's numbers; the deployed server stays the
//! authority.

use crate::wire::{COMMAND_DELAY, TICK_MS};

/// Unanswered probes expire after this long, and longer round trips are not samples.
pub const PROBE_TIMEOUT_MS: f64 = 5000.0;
/// Once warmed up, a connection is probed at most this often.
pub const PROBE_INTERVAL_MS: f64 = 2000.0;
/// Samples older than this no longer count.
pub const FRESH_MS: f64 = 15000.0;
/// Samples kept per connection; also the warm-up count of back-to-back probes.
pub const SAMPLES: usize = 5;
/// Fresh samples needed before a round trip is reported.
pub const MIN_SAMPLES: usize = 3;
/// Bounds of the shared input delay in beats.
pub const MIN_DELAY: u32 = 2;
pub const MAX_DELAY: u32 = 40;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Sample {
    rtt: f64,
    at: f64,
}

/// One connection's probes and samples (`ConnectionLatency`). Times are monotonic
/// milliseconds.
#[derive(Debug, Clone, Default)]
pub struct ConnectionLatency {
    pending: Option<(String, f64)>,
    samples: Vec<Sample>,
    last_probe: Option<f64>,
    issued: u64,
}

impl ConnectionLatency {
    /// Forgets everything (a rebind).
    pub fn reset(&mut self) {
        self.pending = None;
        self.samples.clear();
        self.last_probe = None;
    }

    /// A new probe nonce if one is due: none while one is pending (until it expires), and
    /// after warm-up at most one per [`PROBE_INTERVAL_MS`].
    pub fn probe(&mut self, now: f64) -> Option<String> {
        if let Some((_, sent)) = &self.pending
            && now - sent < PROBE_TIMEOUT_MS
        {
            return None;
        }
        if self.samples.len() >= SAMPLES
            && self
                .last_probe
                .is_some_and(|last| now - last < PROBE_INTERVAL_MS)
        {
            return None;
        }
        self.issued += 1;
        let id = format!("probe-{}", self.issued);
        self.pending = Some((id.clone(), now));
        self.last_probe = Some(now);
        Some(id)
    }

    /// Records the reply to the pending probe. Unknown, repeated and expired replies are
    /// refused.
    pub fn reply(&mut self, id: &str, now: f64) -> bool {
        let Some((pending, sent)) = &self.pending else {
            return false;
        };
        if pending != id {
            return false;
        }
        let elapsed = now - sent;
        self.pending = None;
        if !elapsed.is_finite() || !(0.0..=PROBE_TIMEOUT_MS).contains(&elapsed) {
            return false;
        }
        self.samples.push(Sample {
            rtt: elapsed,
            at: now,
        });
        if self.samples.len() > SAMPLES {
            self.samples.remove(0);
        }
        true
    }

    /// The worst fresh round trip in whole milliseconds, once enough samples are fresh.
    pub fn round_trip(&self, now: f64) -> Option<u32> {
        let fresh: Vec<f64> = self
            .samples
            .iter()
            .filter(|s| now - s.at <= FRESH_MS)
            .map(|s| s.rtt)
            .collect();
        if fresh.len() < MIN_SAMPLES {
            return None;
        }
        Some(fresh.iter().copied().fold(0.0, f64::max).ceil() as u32)
    }
}

/// The match's input delay in beats from every player's round trip (`connectionDelay`):
/// `ceil((worst + 25) / 25)`, at least 2, at most 40, and at least [`COMMAND_DELAY`] while
/// any player is unmeasured.
pub fn connection_delay(round_trips: &[Option<u32>]) -> u32 {
    let known: Vec<u32> = round_trips.iter().flatten().copied().collect();
    let worst = known.iter().copied().max().unwrap_or(0);
    let measured = (worst + TICK_MS).div_ceil(TICK_MS).max(MIN_DELAY);
    let fallback = if known.len() != round_trips.len() || round_trips.is_empty() {
        COMMAND_DELAY
    } else {
        MIN_DELAY
    };
    measured.max(fallback).min(MAX_DELAY)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured(rtts: &[f64]) -> ConnectionLatency {
        let mut latency = ConnectionLatency::default();
        let mut now = 0.0;
        for rtt in rtts {
            let id = latency.probe(now).expect("probe due");
            now += rtt;
            assert!(latency.reply(&id, now));
        }
        latency
    }

    #[test]
    fn round_trip_is_the_worst_of_enough_fresh_samples() {
        let latency = measured(&[40.0, 60.2]);
        assert_eq!(
            latency.round_trip(100.2),
            None,
            "two samples are not enough"
        );
        let latency = measured(&[40.0, 60.2, 50.0]);
        assert_eq!(latency.round_trip(150.2), Some(61));
        assert_eq!(
            latency.round_trip(150.2 + FRESH_MS + 61.0),
            None,
            "stale samples do not count"
        );
    }

    #[test]
    fn keeps_the_latest_five_samples() {
        let mut latency = measured(&[500.0, 10.0, 10.0, 10.0, 10.0]);
        assert_eq!(latency.round_trip(550.0), Some(500));
        let id = latency.probe(550.0 + PROBE_INTERVAL_MS).unwrap();
        assert!(latency.reply(&id, 560.0 + PROBE_INTERVAL_MS));
        assert_eq!(latency.round_trip(560.0 + PROBE_INTERVAL_MS), Some(10));
    }

    #[test]
    fn refuses_spoofed_repeated_and_expired_replies() {
        let mut latency = ConnectionLatency::default();
        let id = latency.probe(0.0).unwrap();
        assert!(latency.probe(10.0).is_none(), "one probe pending at a time");
        assert!(!latency.reply("other", 20.0));
        assert!(latency.reply(&id, 20.0));
        assert!(!latency.reply(&id, 30.0), "a nonce answers once");
        let late = latency.probe(30.0).unwrap();
        assert!(!latency.reply(&late, 30.0 + PROBE_TIMEOUT_MS + 1.0));
        let retry = latency
            .probe(30.0 + PROBE_TIMEOUT_MS)
            .expect("expired probe replaced");
        assert_ne!(retry, late);
    }

    #[test]
    fn probes_back_to_back_until_warm_then_every_two_seconds() {
        let mut latency = measured(&[10.0; 5]);
        assert!(latency.probe(51.0).is_none(), "warm: interval applies");
        assert!(latency.probe(40.0 + PROBE_INTERVAL_MS).is_some());
        latency.reset();
        assert!(latency.probe(0.0).is_some(), "a rebind warms up again");
    }

    #[test]
    fn delay_follows_the_worst_player_with_fallback() {
        assert_eq!(connection_delay(&[]), COMMAND_DELAY);
        assert_eq!(connection_delay(&[Some(0), Some(10)]), MIN_DELAY);
        assert_eq!(connection_delay(&[Some(30), Some(80)]), 5);
        assert_eq!(connection_delay(&[Some(30), None]), COMMAND_DELAY);
        assert_eq!(
            connection_delay(&[Some(400), None]),
            17,
            "measured slow players still raise it"
        );
        assert_eq!(connection_delay(&[Some(5000)]), MAX_DELAY);
    }
}
