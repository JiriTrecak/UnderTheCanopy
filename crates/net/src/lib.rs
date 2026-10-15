//! `canopy-net`: the client for the existing match server and its frozen protocol
//! (docs/decisions.md D10, D32).
//!
//! - [`Net`] talks to one server: the lobby over HTTP (`/api`) and room sockets over
//!   WebSocket (`/match/:id`), all on its own network thread.
//! - [`Session`] is one seat in one match, networked ([`Net::enter`]) or local
//!   ([`Session::local`], the old in-memory channel). The game builds its `World` from the
//!   session's [`MatchSetup`](canopy_sim::MatchSetup), feeds it the committed commands of
//!   each beat ([`Session::next_turn`] or [`Session::advance`]) and reports each simulated
//!   beat back ([`Session::stepped`]) so desync hashes go out every 8 beats.
//! - [`wire`] is the message format, [`lockstep`] the outbox, [`mailbox`] the server's
//!   commit rule (used in-process for single-player), [`latency`] the server's input-delay
//!   choice.
//!
//! Beats and ticks: the server numbers beats from 1; beat `T` holds the commands applied
//! by the simulation step that starts at `World::tick() == T - 1` (this simulation counts
//! ticks one lower than the old game, D21), and the desync hash for beat `T` is the
//! fingerprint after that step.

pub mod latency;
mod link;
pub mod lockstep;
pub mod mailbox;
mod net;
pub mod server;
pub mod session;
/// The TLS client configuration (the platform's verifier); the game's updater shares it.
pub mod tls;
pub mod wire;

pub use net::{Net, NetError, Pending};
pub use server::ServerAddress;
pub use session::{Commands, Event, Phase, Session, Status, Turn};
pub use wire::{ClientIdentity, ContentIdentity, MatchConfig, RoomView};

#[cfg(test)]
mod tests {
    #[test]
    fn handles_move_between_threads() {
        fn send<T: Send>() {}
        fn shared<T: Send + Sync>() {}
        send::<crate::Session>();
        shared::<crate::Commands>();
        shared::<crate::Net>();
    }
}
