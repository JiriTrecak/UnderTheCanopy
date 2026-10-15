//! The multiplayer screens against the old TypeScript match server, run locally (the
//! `canopy-net` test fixture): the lobby list finds a room, joining enters it, the host
//! starts, and both seats play the match on simulation threads driven by their network
//! sessions, chat included. Skipped without the old repository or node (CI).

#[path = "../../../crates/net/tests/fixture/mod.rs"]
#[allow(clippy::all, dead_code)]
mod net_fixture;

use super::multiplayer::{Multiplayer, Outcome, Tab};
use crate::sim::{Control, Notice, Ready, SimThread, TICK};
use canopy_net::{ContentIdentity, Event, Session, wire::CreateRoom};
use canopy_scene::live::{Presenter, Setup};
use std::time::{Duration, Instant};

fn presenter(world: &canopy_sim::World) -> Presenter {
    Presenter::new(
        world,
        net_fixture::map(),
        None,
        Setup {
            colors: vec![[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            tree_slots: Default::default(),
            statics: 0,
        },
    )
}

fn ready() -> Ready {
    Ready {
        content: ContentIdentity::digest("canopy-test-1", [b"fixture".as_slice()]),
        build: "canopy-screens-test".into(),
    }
}

fn deadline(seconds: u64) -> impl Fn(&str) {
    let until = Instant::now() + Duration::from_secs(seconds);
    move |what: &str| assert!(Instant::now() < until, "timed out: {what}")
}

/// Starts a match on a simulation thread from a started session.
fn play(session: Session, config: &canopy_net::MatchConfig) -> SimThread {
    let world = net_fixture::world(&config.setup());
    let presenter = presenter(&world);
    SimThread::start(world, session, presenter, TICK, 0, Some(ready()))
}

#[test]
fn a_lobby_joined_from_the_screen_plays_a_networked_match() {
    let Some(server) = net_fixture::OldServer::start() else {
        return;
    };
    let net = server.net();
    let joined = net
        .create_room(&CreateRoom {
            name: "Ann's lobby".into(),
            map_id: "lockstep-fixture".into(),
            map_revision: "1".into(),
            slot_count: 2,
            guest_name: "Ann".into(),
        })
        .wait()
        .expect("create a room");
    let mut host = net.enter(&joined);

    // The list finds the room; joining enters its lobby.
    let mut mp = Multiplayer::default();
    let address = format!("http://{}", server.address.authority);
    mp.open(Some(&address));
    let wait = deadline(20);
    while !mp.rooms.iter().any(|r| r.id == joined.room.id) {
        wait("the room is listed");
        mp.poll(true);
        if mp.rooms_loaded && !mp.rooms.iter().any(|r| r.id == joined.room.id) {
            mp.refresh();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(mp.reachable, Some(true), "{}", mp.health_text);
    mp.tab = Tab::Find;
    mp.selected_room = mp.rooms.iter().position(|r| r.id == joined.room.id);
    mp.go(&[], "Bo");
    let wait = deadline(20);
    loop {
        wait("the lobby");
        if let Some(Outcome::EnterLobby) = mp.poll(true) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // Both seats show, with the server's measured round trips eventually.
    let wait = deadline(20);
    while mp
        .room()
        .is_none_or(|r| r.slots.iter().filter(|s| s.name.is_some()).count() < 2)
    {
        wait("both seats");
        assert!(mp.poll(false).is_none());
        host.poll();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!mp.is_host());

    // The host starts: the screen hands over the seat for loading.
    host.start_match();
    let mut host_config = None;
    let mut ours = None;
    let wait = deadline(20);
    while host_config.is_none() || ours.is_none() {
        wait("the start");
        for event in host.poll() {
            if let Event::Started { config, .. } = event {
                host_config = Some(config);
            }
        }
        if let Some(Outcome::Start { config, session }) = mp.poll(false) {
            assert_eq!(session.player(), Some(1));
            ours = Some((config, session));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let host_config = host_config.unwrap();
    let (config, session) = ours.unwrap();
    assert_eq!(config, host_config, "both seats run the same frozen match");
    let host_thread = play(host, &host_config);
    let our_thread = play(*session, &config);

    // Both simulate in lockstep from the server's commits.
    let wait = deadline(60);
    let newest = |t: &SimThread| t.mailbox().latest().unwrap_or(0);
    while newest(&host_thread) < 120 || newest(&our_thread) < 120 {
        wait("both seats simulate");
        for t in [&host_thread, &our_thread] {
            for notice in t.notices() {
                assert!(
                    !matches!(
                        notice,
                        Notice::Net(
                            Event::Desync { .. } | Event::Stopped { .. } | Event::Error { .. }
                        )
                    ),
                    "{notice:?}"
                );
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let status = our_thread.live().status;
    assert_eq!(status.phase, canopy_net::Phase::Playing);
    assert!(status.delay >= 1);

    // Chat reaches the other seat; pause is ignored in multiplayer.
    our_thread.control(Control::Pause(true));
    our_thread.control(Control::Chat("hello from the screens".into()));
    let wait = deadline(20);
    let mut heard = false;
    while !heard {
        wait("chat");
        for notice in host_thread.notices() {
            if let Notice::Net(Event::Chat(message)) = notice {
                assert_eq!(message.text, "hello from the screens");
                assert_eq!(message.name, "Bo");
                heard = true;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!our_thread.live().paused, "multiplayer has no pause");
    let before = newest(&our_thread);
    std::thread::sleep(Duration::from_millis(300));
    assert!(newest(&our_thread) > before, "the match ran on");
}

#[test]
fn outcomes_go_on_the_wire_as_slots() {
    use canopy_content::placement::Owner;
    let outcome = canopy_sim::state::Outcome {
        winner: Some(Owner::Player(2)),
        defeated: vec![Owner::Player(0), Owner::Nobody, Owner::Player(1)],
    };
    let wire = crate::sim::wire_outcome(&outcome);
    assert_eq!(wire.winner, Some(2));
    assert_eq!(wire.defeated, vec![0, 1]);
}
