//! The scripted input harness: a real match on test-owned content (the simulation's AI
//! fixture), the player's real view, a synthetic camera and synthesized window input
//! drive selection and commands, and the tests assert the protocol actions produced —
//! and that the simulation accepts them. Headless: no window, GPU or UI.

use super::{
    Event, Mode, Play, Sink,
    input::{Button, Input, Mods},
    know::Knowledge,
    picking::{Body, Screen},
};
use crate::sim::ai_fixture as fx;
use canopy_content::placement::Owner;
use canopy_render::{RtsCamera, camera::FlatGround};
use canopy_sim::{Action, Queued, World, catalog::Catalog};
use glam::{Vec2, Vec3};
use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

/// The human seat of `fx::setup(false)`.
const ME: u8 = 1;
const OWNER: Owner = Owner::Player(ME);

#[derive(Clone, Default)]
struct Recorder(Rc<RefCell<Vec<Action>>>);

impl Sink for Recorder {
    fn send(&mut self, action: Action) -> bool {
        self.0.borrow_mut().push(action);
        true
    }
}

struct Harness {
    world: World,
    play: Play,
    sent: Recorder,
    screen: Screen,
    catalog: Arc<Catalog>,
    now: Duration,
    seq: u32,
}

impl Harness {
    fn new(extra: Vec<serde_json::Value>) -> Self {
        let map = fx::skirmish(extra, vec![]);
        let mut world = fx::world(&map, false);
        fx::run(&mut world, 2);
        let content = world.catalog().content.clone();
        let catalog = Arc::new(Catalog::new(content.clone()));
        let camera = RtsCamera::new([fx::AWAY.0 as f32, fx::AWAY.1 as f32 + 2.0], None)
            .view(16.0 / 9.0, &FlatGround);
        let screen = Screen {
            camera,
            size: [1600.0, 900.0],
            scale: 1.0,
        };
        let sent = Recorder::default();
        let know = Knowledge::new(
            content,
            catalog.clone(),
            Arc::new(world.view(OWNER)),
            Some(ME),
        );
        let play = Play::new(know, world.terrain().clone(), Box::new(sent.clone()));
        let mut h = Self {
            world,
            play,
            sent,
            screen,
            catalog,
            now: Duration::from_secs(1),
            seq: 0,
        };
        h.refresh();
        h
    }

    fn refresh(&mut self) {
        let view = Arc::new(self.world.view(OWNER));
        let mut know = Knowledge::new(
            self.world.catalog().content.clone(),
            self.catalog.clone(),
            view.clone(),
            Some(ME),
        );
        know.held = Arc::new(crate::sim::held_units(&self.world, &view));
        let bodies: Vec<Body> = view
            .entities
            .iter()
            .filter(|e| !e.remembered)
            .map(|e| {
                let d = know.def(e);
                let [x, z] = know.position(e);
                Body {
                    id: e.id().0,
                    foot: Vec3::new(x, 0.0, z),
                    height: d.dimensions.as_ref().map_or(2.5, |b| b.height as f32),
                    radius: d.dimensions.as_ref().map_or(0.9, |b| b.radius as f32),
                    unit: know.is_unit(e),
                }
            })
            .collect();
        self.play
            .update(know, self.screen, bodies, &FlatGround, self.now);
    }

    /// Like `refresh`, from the observer's view.
    fn refresh_observer(&mut self) {
        let view = Arc::new(self.world.observer_view());
        let know = Knowledge::new(
            self.world.catalog().content.clone(),
            self.catalog.clone(),
            view,
            None,
        );
        self.play
            .update(know, self.screen, Vec::new(), &FlatGround, self.now);
    }

    fn advance(&mut self, ms: u64) {
        self.now += Duration::from_millis(ms);
        self.refresh();
    }

    fn input(&mut self, input: Input) {
        self.play.input(&input, &FlatGround);
    }

    fn pixel(&self, x: f32, z: f32) -> Vec2 {
        self.screen
            .project(Vec3::new(x, 0.0, z))
            .expect("on screen")
            .0
    }

    fn entity_pixel(&self, id: u32) -> Vec2 {
        let e = self.play.knowledge().entity(id).expect("known");
        let [x, z] = self.play.knowledge().position(e);
        // A little above the feet, inside the capsule.
        self.screen
            .project(Vec3::new(x, 0.6, z))
            .expect("on screen")
            .0
    }

    fn mods(&mut self, mods: Mods) {
        self.input(Input::Modifiers(mods));
    }

    fn click_px(&mut self, p: Vec2, button: Button) {
        self.input(Input::PointerMoved(p));
        self.input(Input::Button {
            button,
            pressed: true,
        });
        self.input(Input::Button {
            button,
            pressed: false,
        });
        self.advance(400);
    }

    fn click(&mut self, id: u32) {
        let p = self.entity_pixel(id);
        self.click_px(p, Button::Left);
    }

    fn right_click(&mut self, id: u32) {
        let p = self.entity_pixel(id);
        self.click_px(p, Button::Right);
    }

    fn right_click_ground(&mut self, x: f32, z: f32) {
        let p = self.pixel(x, z);
        self.click_px(p, Button::Right);
    }

    fn drag(&mut self, from: (f32, f32), to: (f32, f32)) {
        let (a, b) = (self.pixel(from.0, from.1), self.pixel(to.0, to.1));
        self.input(Input::PointerMoved(a));
        self.input(Input::Button {
            button: Button::Left,
            pressed: true,
        });
        self.input(Input::PointerMoved(b));
        self.input(Input::Button {
            button: Button::Left,
            pressed: false,
        });
        self.advance(400);
    }

    fn key(&mut self, key: &'static str) {
        self.input(Input::Key {
            key,
            pressed: true,
            repeat: false,
        });
        self.input(Input::Key {
            key,
            pressed: false,
            repeat: false,
        });
        self.advance(100);
    }

    fn take(&mut self) -> Vec<Action> {
        std::mem::take(&mut *self.sent.0.borrow_mut())
    }

    /// Hands the sent actions to the simulation (as the session would, one tick later)
    /// and steps `ticks`.
    fn apply(&mut self, actions: &[Action], ticks: u32) {
        let tick = self.world.tick();
        for action in actions {
            self.world
                .enqueue(Queued {
                    tick,
                    player: ME,
                    seq: self.seq,
                    action: action.clone(),
                })
                .expect("well-formed");
            self.seq += 1;
        }
        fx::run(&mut self.world, ticks);
        self.refresh();
    }

    fn mine(&self, definition: &str) -> Vec<u32> {
        fx::all_of(&self.world, OWNER, definition)
    }

    fn errors(&self) -> Vec<String> {
        self.world
            .view(OWNER)
            .facts
            .iter()
            .filter(|f| f.kind == canopy_sim::state::FactKind::Error)
            .map(|f| f.message.clone())
            .collect()
    }
}

#[test]
fn clicks_select_one_shift_toggles_and_a_double_click_selects_the_type() {
    let mut h = Harness::new(vec![]);
    let workers = h.mine(fx::WORKER);
    assert_eq!(workers.len(), 4);
    h.click(workers[0]);
    assert_eq!(h.play.selection.ids(), [workers[0]]);
    h.mods(Mods {
        shift: true,
        ..Mods::default()
    });
    h.click(workers[1]);
    assert_eq!(h.play.selection.ids().len(), 2);
    h.click(workers[1]);
    assert_eq!(h.play.selection.ids(), [workers[0]]);
    h.mods(Mods::default());
    // A double click (within 350 ms, 6 px) selects every worker on screen.
    let p = h.entity_pixel(workers[2]);
    h.input(Input::PointerMoved(p));
    for _ in 0..2 {
        h.input(Input::Button {
            button: Button::Left,
            pressed: true,
        });
        h.input(Input::Button {
            button: Button::Left,
            pressed: false,
        });
        h.advance(100);
    }
    let mut selected = h.play.selection.ids().to_vec();
    selected.sort_unstable();
    assert_eq!(selected, workers);
    // Clicking the ground clears.
    h.click_px(
        h.pixel(fx::AWAY.0 as f32 + 20.0, fx::AWAY.1 as f32 - 12.0),
        Button::Left,
    );
    assert!(h.play.selection.ids().is_empty());
    assert!(h.take().is_empty(), "selection sends nothing");
}

#[test]
fn box_selection_prefers_the_hero_over_workers() {
    let mut h = Harness::new(vec![]);
    let hero = h.mine(fx::HERO);
    let workers = h.mine(fx::WORKER);
    let (x, z) = (fx::AWAY.0 as f32, fx::AWAY.1 as f32);
    // Everything around the hall: the hero (army) wins.
    h.drag((x - 14.0, z - 12.0), (x + 14.0, z + 12.0));
    assert_eq!(h.play.selection.ids(), hero.as_slice());
    // Just the workers' row above the hall (the second start faces north).
    h.drag((x - 7.0, z - 10.5), (x + 7.0, z - 7.0));
    let mut selected = h.play.selection.ids().to_vec();
    selected.sort_unstable();
    assert_eq!(selected, workers);
}

#[test]
fn right_clicks_move_gather_attack_and_shift_appends() {
    let mut h = Harness::new(vec![]);
    let workers = h.mine(fx::WORKER);
    let (x, z) = (fx::AWAY.0 as f32, fx::AWAY.1 as f32);
    h.click(workers[0]);
    h.right_click_ground(x + 10.0, z + 3.0);
    let sent = h.take();
    let [
        Action::Move {
            actors,
            destination,
            attack_move: None,
            append: None,
        },
    ] = sent.as_slice()
    else {
        panic!("{sent:?}");
    };
    assert_eq!(actors, &[workers[0]]);
    // Ground points round to the nearest cell (old `Math.round`).
    assert_eq!(
        (destination.x, destination.y),
        ((x + 10.0).round() as u32, (z + 3.0).round() as u32)
    );
    h.apply(&sent, 4);
    assert!(h.errors().is_empty(), "{:?}", h.errors());

    // Shift queues.
    h.mods(Mods {
        shift: true,
        ..Mods::default()
    });
    h.right_click_ground(x + 12.0, z + 3.0);
    assert!(matches!(
        h.take().as_slice(),
        [Action::Move {
            append: Some(true),
            ..
        }]
    ));
    h.mods(Mods::default());

    // A mine: workers gather.
    let mine = h
        .play
        .knowledge()
        .view
        .entities
        .iter()
        .find(|e| h.play.knowledge().def(e).id.as_str() == fx::MINE && !e.remembered)
        .map(|e| e.id().0)
        .expect("a mine in sight");
    h.right_click(mine);
    let sent = h.take();
    assert!(
        matches!(sent.as_slice(), [Action::Gather { target, .. }] if *target == mine),
        "{sent:?}"
    );
    h.apply(&sent, 2);
    assert!(h.errors().is_empty(), "{:?}", h.errors());

    // An enemy soldier in sight: the hero attacks it; workers alone would move.
    let enemy = fx::spawn(
        &mut h.world,
        "intruder",
        fx::SOLDIER,
        Owner::Player(0),
        f64::from(x) + 6.0,
        f64::from(z) - 6.0,
    );
    h.apply(&[], 2);
    let hero = h.mine(fx::HERO)[0];
    h.click(hero);
    h.right_click(enemy);
    let sent = h.take();
    assert!(
        matches!(sent.as_slice(), [Action::Attack { actors, target, force: None, .. }] if actors == &[hero] && *target == enemy),
        "{sent:?}"
    );
    h.click(workers[1]);
    h.right_click(enemy);
    assert!(matches!(h.take().as_slice(), [Action::Move { .. }]));
}

#[test]
fn control_groups_assign_recall_and_centre_on_a_double_press() {
    let mut h = Harness::new(vec![]);
    let workers = h.mine(fx::WORKER);
    let hero = h.mine(fx::HERO)[0];
    h.click(workers[0]);
    h.mods(Mods {
        ctrl: true,
        ..Mods::default()
    });
    h.key("1");
    h.mods(Mods::default());
    h.click(hero);
    assert_eq!(h.play.selection.ids(), [hero]);
    h.key("1");
    assert_eq!(h.play.selection.ids(), [workers[0]]);
    h.play.take_events();
    h.key("1");
    assert!(matches!(
        h.play.take_events().as_slice(),
        [Event::LookAt(_)]
    ));
    // F1 selects the hero.
    h.advance(1000);
    h.key("F1");
    assert_eq!(h.play.selection.ids(), [hero]);
}

#[test]
fn attack_mode_issues_attack_move_and_escape_cancels() {
    let mut h = Harness::new(vec![]);
    let hero = h.mine(fx::HERO)[0];
    let (x, z) = (fx::AWAY.0 as f32, fx::AWAY.1 as f32);
    h.click(hero);
    // The fixture binds no keys: press the card's command.
    h.play.activate_id("attack", false);
    assert!(matches!(h.play.mode, Mode::Target { .. }));
    assert!(h.play.hint().is_some_and(|t| t.contains("Esc cancels")));
    h.key("Escape");
    assert_eq!(h.play.mode, Mode::Idle);
    h.play.activate_id("attack", false);
    h.click_px(h.pixel(x - 15.0, z - 4.0), Button::Left);
    let sent = h.take();
    assert!(
        matches!(
            sent.as_slice(),
            [Action::Move {
                attack_move: Some(true),
                ..
            }]
        ),
        "{sent:?}"
    );
    assert_eq!(h.play.mode, Mode::Idle);
    // Right-click cancels a mode without commanding.
    h.play.activate_id("patrol", false);
    h.right_click_ground(x - 10.0, z);
    assert!(h.take().is_empty());
    assert_eq!(h.play.mode, Mode::Idle);
    // Hold sends at once.
    h.play.activate_id("hold", false);
    assert!(matches!(h.take().as_slice(), [Action::Hold { .. }]));
}

#[test]
fn placement_snaps_rotates_validates_and_builds() {
    let mut h = Harness::new(vec![]);
    let workers = h.mine(fx::WORKER);
    let (x, z) = (fx::AWAY.0 as f32, fx::AWAY.1 as f32);
    h.click(workers[0]);
    let id = format!("build:{}", fx::BARRACKS);
    assert!(
        h.play.commands.iter().any(|c| c.id == id),
        "{:?}",
        h.play.commands.iter().map(|c| &c.id).collect::<Vec<_>>()
    );
    h.play.activate_id(&id, false);
    assert!(matches!(h.play.mode, Mode::Place { .. }));
    // On the hall: invalid, nothing is sent.
    h.input(Input::PointerMoved(h.pixel(x, z)));
    h.refresh();
    let ghost = h.play.ghost().expect("ghost");
    assert!(ghost.problem.is_some());
    h.click_px(h.pixel(x, z), Button::Left);
    assert!(h.take().is_empty());
    assert!(h.play.message().is_some());
    // Open ground to the west, turned a quarter.
    h.key("R");
    let spot = h.pixel(x - 20.0, z - 2.0);
    h.input(Input::PointerMoved(spot));
    h.refresh();
    let ghost = h.play.ghost().expect("ghost");
    assert_eq!(ghost.problem, None, "{ghost:?}");
    assert_eq!(ghost.rotation, 90);
    h.click_px(spot, Button::Left);
    let sent = h.take();
    let [
        Action::Build {
            actors,
            definition,
            rotation: Some(90),
            position,
            ..
        },
    ] = sent.as_slice()
    else {
        panic!("{sent:?}");
    };
    assert_eq!(actors, &[workers[0]]);
    assert_eq!(definition.as_str(), fx::BARRACKS);
    // The footprint's edges lie on the 4-cell building lattice (origin −0.5).
    let edge = f64::from(position.x.0) / 2.0 - 4.0 + 0.5;
    assert_eq!(edge.rem_euclid(4.0), 0.0, "{position:?}");
    assert_eq!(h.play.mode, Mode::Idle, "placement ends without Shift");
    h.apply(&sent, 4);
    assert!(
        h.errors().is_empty(),
        "the simulation refused: {:?}",
        h.errors()
    );
    let sites = h
        .play
        .knowledge()
        .view
        .entities
        .iter()
        .filter(|e| h.play.knowledge().own(e) && e.public.construction.is_some())
        .count();
    assert_eq!(sites, 1);
}

#[test]
fn a_selected_hall_offers_training_and_rally_and_the_card_reports_why() {
    let mut h = Harness::new(vec![]);
    let hall = h.mine(fx::HALL)[0];
    h.click(hall);
    assert_eq!(h.play.selection.ids(), [hall]);
    let produce = h
        .play
        .commands
        .iter()
        .find(|c| c.id == format!("produce:{}", fx::WORKER))
        .expect("the hall trains workers")
        .clone();

    assert!(produce.costs.iter().any(|c| c.amount == 10));
    // Four workers and the hero fill the hall's supply: the card says so and pressing
    // the command only shows why.
    assert_eq!(produce.reason.as_deref(), Some("Not enough supply (8/8)."));
    h.play.activate(&produce, false);
    assert!(h.take().is_empty());
    assert_eq!(h.play.message().map(|m| m.0.to_owned()), produce.reason);
    // A house raises the cap.
    let (x, z) = (fx::AWAY.0, fx::AWAY.1);
    fx::spawn(&mut h.world, "house", fx::HOUSE, OWNER, x - 20.0, z + 0.5);
    h.apply(&[], 2);
    h.click(hall);
    let produce = h
        .play
        .commands
        .iter()
        .find(|c| c.id == format!("produce:{}", fx::WORKER))
        .unwrap()
        .clone();
    assert_eq!(produce.reason, None);
    h.play.activate(&produce, false);
    let sent = h.take();
    assert!(
        matches!(sent.as_slice(), [Action::Produce { actor, .. }] if *actor == hall),
        "{sent:?}"
    );
    // Right-clicking the ground with only the hall selected sets its rally point.
    h.right_click_ground(fx::AWAY.0 as f32 + 8.0, fx::AWAY.1 as f32 - 8.0);
    let rally = h.take();
    assert!(
        matches!(
            rally.as_slice(),
            [Action::Rally {
                destination: Some(_),
                ..
            }]
        ),
        "{rally:?}"
    );
    h.apply(&[sent, rally].concat(), 3);
    assert!(h.errors().is_empty(), "{:?}", h.errors());
    assert!(h.play.world_ui().rally.is_some());
}

#[test]
fn the_hero_learns_from_the_banner_menu_and_casts_from_the_bottom_row() {
    let mut h = Harness::new(vec![]);
    let hero = h.mine(fx::HERO)[0];
    h.click(hero);
    let learn: Vec<String> = h
        .play
        .commands
        .iter()
        .filter(|c| c.id.starts_with("learn:"))
        .map(|c| c.id.clone())
        .collect();
    assert_eq!(learn.len(), 3, "{learn:?}");
    h.play.activate_id("learn:smite", false);
    let sent = h.take();
    assert!(
        matches!(sent.as_slice(), [Action::LearnAbility { .. }]),
        "{sent:?}"
    );
    h.apply(&sent, 2);
    assert!(h.errors().is_empty(), "{:?}", h.errors());
    let cast = h
        .play
        .commands
        .iter()
        .find(|c| c.id == "cast:smite")
        .expect("learned spells are cast from the card");
    assert_eq!(
        cast.placement,
        Some(canopy_content::game::ActionPlacement::BottomRow)
    );
    // Learning used the only skill point.
    let rush = h
        .play
        .commands
        .iter()
        .find(|c| c.id == "learn:rush")
        .unwrap();
    assert_eq!(rush.reason.as_deref(), Some("No skill points available"));
}

#[test]
fn the_ghost_agrees_with_the_simulation_where_the_player_sees() {
    let mut h = Harness::new(vec![]);
    let worker = h.mine(fx::WORKER)[0];
    let k = h.play.knowledge().clone();
    let (cx, cz) = (fx::AWAY.0 as f32, fx::AWAY.1 as f32);
    let mut checked = 0;
    for definition in [fx::HOUSE, fx::BARRACKS] {
        for rotation in [0, 90] {
            let placement = super::placement::Placement {
                definition: definition.into(),
                actors: vec![worker],
                rotation,
            };
            for dz in (-9..=9).map(|i| i as f32 * 1.5) {
                for dx in (-9..=9).map(|i| i as f32 * 1.5) {
                    let ghost = super::placement::ghost(
                        &k,
                        h.world.terrain(),
                        &placement,
                        cx + dx,
                        cz + dz,
                    )
                    .expect("a ghost");
                    let verdict = h.world.can_build(
                        OWNER,
                        definition,
                        &ghost.point,
                        Some(canopy_sim::entity::EntityId(worker)),
                        rotation,
                    );
                    assert_eq!(
                        ghost.problem, verdict,
                        "{definition} at {:?} turned {rotation}",
                        ghost.point
                    );
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 4 * 19 * 19);
}

#[test]
fn idle_workers_menu_keys_tab_and_chat() {
    let mut h = Harness::new(vec![]);
    // Three of the four starting workers gather; F8 finds the fourth and centres on it.
    let idle = h.play.idle_workers();
    assert_eq!(idle.len(), 1, "{idle:?}");
    h.play.take_events();
    h.key("F8");
    assert_eq!(h.play.selection.ids(), idle.as_slice());
    assert!(matches!(
        h.play.take_events().as_slice(),
        [Event::LookAt(_)]
    ));
    // Escape with nothing to cancel opens the game menu.
    h.key("Escape");
    assert_eq!(h.play.take_events(), [Event::OpenMenu]);
    // Tab moves the focus between types and keeps the selection.
    let hero = h.mine(fx::HERO)[0];
    let mut ids = h.mine(fx::WORKER);
    ids.push(hero);
    h.play.select(ids.clone(), None);
    assert_eq!(h.play.selection.focus(), Some(hero));
    h.key("Tab");
    assert_ne!(h.play.selection.focus(), Some(hero));
    assert_eq!(h.play.selection.ids().len(), ids.len());
    // Enter opens the chat; a line is echoed and handed to the game.
    h.key("Enter");
    assert!(h.play.chat_open);
    h.play.say("gl hf");
    assert!(!h.play.chat_open);
    assert_eq!(h.play.chat.last().map(|l| l.text.as_str()), Some("gl hf"));
    assert_eq!(h.play.take_events(), [Event::Chat("gl hf".into())]);
    assert!(h.take().is_empty());
}

#[test]
fn an_observer_selects_but_never_commands() {
    let mut h = Harness::new(vec![]);
    let view = Arc::new(h.world.observer_view());
    let know = Knowledge::new(
        h.world.catalog().content.clone(),
        h.catalog.clone(),
        view,
        None,
    );
    h.play = Play::new(know, h.world.terrain().clone(), Box::new(h.sent.clone()));
    h.refresh_observer();
    let enemy_worker = fx::all_of(&h.world, Owner::Player(0), fx::WORKER)[0];
    let mine = h.mine(fx::WORKER)[0];
    h.play.select(vec![enemy_worker, mine], None);
    assert!(h.play.commands.is_empty());
    h.right_click_ground(fx::AWAY.0 as f32 + 5.0, fx::AWAY.1 as f32);
    assert!(h.take().is_empty());
}

#[test]
fn markers_pulse_and_expire_like_the_old_rings() {
    use super::orders::{Marker, MarkerKind};
    let attack = Marker {
        kind: MarkerKind::Attack,
        position: [0.0, 0.0],
        radius: 0.85,
    };
    assert!((attack.lifetime() - 0.95).abs() < 1e-6);
    assert!((attack.opacity(0.0) - 0.95).abs() < 1e-6);
    assert_eq!(attack.opacity(0.45), 0.0, "the gap between pulses");
    assert!(attack.opacity(0.6) > 0.5, "the second pulse");
    assert!(attack.opacity(0.951) == 0.0 && attack.opacity(0.949) < 0.01);
    let point = Marker {
        kind: MarkerKind::Move,
        ..attack
    };
    assert!((point.lifetime() - 0.4).abs() < 1e-6);
}
