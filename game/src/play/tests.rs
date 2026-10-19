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
    /// The human seat.
    me: u8,
    /// The content the player's knowledge reads (the world's, unless a test adds
    /// presentation-only metadata).
    content: Arc<canopy_content::Content>,
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
        Self::on(world, ME, [fx::AWAY.0 as f32, fx::AWAY.1 as f32 + 2.0])
    }

    /// Plays seat `me` of `world` with the camera on `focus` (x, z).
    fn on(world: World, me: u8, focus: [f32; 2]) -> Self {
        let owner = Owner::Player(me);
        let content = world.catalog().content.clone();
        let catalog = Arc::new(Catalog::new(content.clone()));
        let camera = RtsCamera::new(focus, None).view(16.0 / 9.0, &FlatGround);
        let screen = Screen {
            camera,
            size: [1600.0, 900.0],
            scale: 1.0,
        };
        let sent = Recorder::default();
        let know = Knowledge::new(
            content,
            catalog.clone(),
            Arc::new(world.view(owner)),
            Some(me),
        );
        let play = Play::new(know, world.terrain().clone(), Box::new(sent.clone()));
        let mut h = Self {
            content: world.catalog().content.clone(),
            world,
            me,
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
        let view = Arc::new(self.world.view(Owner::Player(self.me)));
        let mut know = Knowledge::new(
            self.content.clone(),
            self.catalog.clone(),
            view.clone(),
            Some(self.me),
        );
        know.held = Arc::new(crate::sim::held_units(&view));
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
                    capsule: know.picked_by_body(e),
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
                    player: self.me,
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
        fx::all_of(&self.world, Owner::Player(self.me), definition)
    }

    fn errors(&self) -> Vec<String> {
        self.world
            .view(Owner::Player(self.me))
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
    // Aiming shows the spell's area and range at the cursor (old `gameAbilityTarget`).
    // Spells show their presentation's icon, not the caster's (old `commands`).
    assert_eq!(cast.icon.as_str(), "asset.spell-icon");
    let smite = cast.clone();
    assert!(matches!(smite.activate, super::card::Activate::Target(_)));
    {
        h.play.activate(&smite, false);
        let [hx, hz] = h
            .play
            .knowledge()
            .position(h.play.knowledge().entity(hero).unwrap());
        let near = h.pixel(hx + 1.0, hz);
        h.input(Input::PointerMoved(near));
        h.advance(50);
        let aim = h.play.world_ui().aim.expect("an aim");
        assert!(aim.range > 0.0 && aim.valid, "{aim:?}");
        assert_eq!(aim.origin, [hx, hz]);
        h.key("Escape");
        assert!(h.play.world_ui().aim.is_none());
    }
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
fn a_well_snaps_onto_its_deposit_and_workers_sent_to_it_gather_there() {
    let at = (199.5, 227.5);
    let spring = fx::place_for("spring", fx::SPRING, "none", at.0, at.1);
    let mut h = Harness::new(vec![spring]);
    let k = h.play.knowledge().clone();
    let spring = k
        .view
        .entities
        .iter()
        .find(|e| k.def(e).id.as_str() == fx::SPRING && !e.remembered)
        .map(|e| e.id().0)
        .expect("the deposit in sight");
    let workers = h.mine(fx::WORKER);
    let placement = super::placement::Placement {
        definition: fx::WELL.into(),
        actors: vec![workers[0]],
        rotation: 0,
    };
    // Near the deposit the ghost stands on it, as the simulation agrees; away it cannot.
    for (dx, dz, on) in [(2.5, -1.5, true), (0.0, 0.0, true), (-30.0, 0.0, false)] {
        let ghost = super::placement::ghost(
            &k,
            h.world.terrain(),
            &placement,
            at.0 as f32 + dx,
            at.1 as f32 + dz,
        )
        .expect("a ghost");
        let verdict = h.world.can_build(
            OWNER,
            fx::WELL,
            &ghost.point,
            Some(canopy_sim::entity::EntityId(workers[0])),
            0,
        );
        assert_eq!(ghost.problem, verdict, "{:?}", ghost.point);
        assert_eq!(ghost.center == [at.0 as f32, at.1 as f32], on, "{ghost:?}");
        assert_eq!(ghost.problem.is_none(), on, "{ghost:?}");
    }
    // Build it there.
    h.click(workers[0]);
    h.play.activate_id(&format!("build:{}", fx::WELL), false);
    h.click_px(h.pixel(at.0 as f32 + 2.0, at.1 as f32 + 1.0), Button::Left);
    let sent = h.take();
    assert!(
        matches!(sent.as_slice(), [Action::Build { .. }]),
        "{sent:?}"
    );
    h.apply(&sent, 400);
    assert!(h.errors().is_empty(), "{:?}", h.errors());
    let well = h.mine(fx::WELL)[0];
    assert!(fx::get(&h.world, well).construction.is_none(), "finished");
    // The deposit under it is neither picked nor selectable; a right-click on the well
    // sends workers to gather the deposit.
    assert!(
        h.play
            .knowledge()
            .covered
            .contains(&canopy_sim::entity::EntityId(spring))
    );
    h.click(workers[1]);
    h.right_click(well);
    let sent = h.take();
    assert!(
        matches!(sent.as_slice(), [Action::Gather { target, .. }] if *target == spring),
        "{sent:?}"
    );
    h.apply(&sent, 4);
    assert!(h.errors().is_empty(), "{:?}", h.errors());
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
fn the_idle_worker_key_skips_stunned_held_and_releasing_workers() {
    let mut h = Harness::new(vec![]);
    let idle = h.play.idle_workers();
    assert_eq!(idle.len(), 1, "{idle:?}");
    let flags: [fn(&mut canopy_sim::view::Private); 3] = [
        |p| p.stunned = true,
        |p| p.contained = true,
        |p| p.releasing = true,
    ];
    for flag in flags {
        let mut view = h.world.view(OWNER);
        let worker = view
            .entities
            .iter_mut()
            .find(|e| e.id().0 == idle[0])
            .expect("the idle worker");
        flag(worker.private.as_mut().expect("own"));
        let know = Knowledge::new(
            h.content.clone(),
            h.catalog.clone(),
            Arc::new(view),
            Some(ME),
        );
        h.play
            .update(know, h.screen, Vec::new(), &FlatGround, h.now);
        assert!(h.play.idle_workers().is_empty());
    }
    h.refresh();
    assert_eq!(h.play.idle_workers(), idle);
}

#[test]
fn the_portrait_tooltip_prices_the_definition_with_its_supply() {
    let h = Harness::new(vec![]);
    let k = h.play.knowledge();
    let hero = k.content.definition(fx::HERO).expect("hero");
    let costs = super::card::definition_costs(k, hero);
    let amounts: Vec<u32> = costs.iter().map(|c| c.amount).collect();
    assert_eq!(amounts.last(), Some(&4), "supply comes last: {amounts:?}");
    assert_eq!(costs.last().unwrap().icon, k.content.rules.supply_icon);
    assert!(costs.len() >= 2, "a price and the supply");
    let hall = k.content.definition(fx::HALL).expect("hall");
    assert!(!super::card::definition_costs(k, hall).is_empty());
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

#[test]
fn the_camera_command_cycles_unit_cameras_and_escape_returns_to_rts() {
    use super::follow::CameraMode;
    let mut h = Harness::new(vec![]);
    // The fixture has no camera metadata; add the content's presentation entry.
    let mut content = (*h.content).clone();
    let back = content.actions.navigation.back.clone();
    content.actions.navigation.camera = Some(canopy_content::game::NavigationMeta {
        name: "Cycle camera".into(),
        hotkey: Some("J".into()),
        priority: 55,
        ..back
    });
    h.content = Arc::new(content);
    h.refresh();
    let worker = h.mine(fx::WORKER)[0];
    h.click(worker);
    let camera = h.play.commands.iter().find(|c| c.id == "camera").cloned();
    assert_eq!(camera.map(|c| c.name), Some("Camera: RTS".to_owned()));
    h.key("J");
    assert_eq!(h.play.camera_mode, CameraMode::ThirdPerson);
    assert!(
        h.play
            .commands
            .iter()
            .any(|c| c.id == "camera" && c.name == "Camera: Third person")
    );
    h.key("J");
    assert_eq!(h.play.camera_mode, CameraMode::FirstPerson);
    // Escape leaves the unit camera before it opens the game menu.
    h.key("Escape");
    assert_eq!(h.play.camera_mode, CameraMode::Rts);
    assert!(h.play.take_events().is_empty());
    h.key("J");
    assert_eq!(h.play.camera_mode, CameraMode::ThirdPerson);
    // Nothing selected: no unit to follow.
    h.click_px(Vec2::new(5.0, 5.0), Button::Left);
    assert_eq!(h.play.camera_mode, CameraMode::Rts);
    assert!(h.take().is_empty(), "the camera sends no command");
}

#[test]
fn rebound_command_keys_press_their_commands() {
    use crate::settings::{Chord, keys};
    let mut h = Harness::new(vec![]);
    let worker = h.mine(fx::WORKER)[0];
    h.click(worker);
    // The fixture authors no key for Stop; the player binds one.
    h.key("Z");
    assert!(h.take().is_empty());
    let catalog = keys::full_catalog(Some(&h.content));
    // The content's commands join the engine's entries (old `shortcutCatalog`).
    for id in ["command.stop", "command.navigation:back"] {
        assert!(catalog.iter().any(|e| e.id == id), "{id}");
    }
    let trained = h.content.definitions.iter().find(|d| {
        matches!(
            d.creation,
            Some(canopy_content::game::Creation::Train { .. })
        )
    });
    if let Some(d) = trained {
        let id = format!("command.produce:{}", d.id);
        assert!(catalog.iter().any(|e| e.id == id), "{id}");
    }
    let mut bindings = crate::settings::KeyBindings::default();
    bindings
        .bind(&catalog, "command.stop", Chord::key("KeyZ"))
        .expect("Z is free");
    h.play.keymap = super::keys::Keymap::new(&bindings, &catalog);
    h.refresh();
    let label = h
        .play
        .commands
        .iter()
        .find(|c| c.id == "stop")
        .and_then(|c| c.chord.as_ref().map(Chord::label));
    assert_eq!(label.as_deref(), Some("Z"));
    h.key("Z");
    assert!(matches!(h.take().as_slice(), [Action::Stop { .. }]));
}

/// Trees: the economy fixture's felled trees on the first start, with a test-owned pick
/// profile (a trunk under a crown 1.5 wide from 1 m up, 4 m tall).
mod trees {
    use super::*;
    use crate::play::{
        orders::GATHER_RADIUS,
        picking::{Tree, TreeProfile, Trees},
    };
    use crate::sim::economy_fixture as ex;

    /// Just behind the first start's idle worker (it stands at 46, 50).
    const BEHIND: (f64, f64) = (46.0, 49.0);
    const OPEN: (f64, f64) = (52.0, 44.0);

    fn profile() -> TreeProfile {
        TreeProfile {
            height: 4.0,
            radii: [0.4, 0.4, 1.5, 1.5, 1.2, 1.0, 0.6, 0.3],
        }
    }

    fn harness() -> (Harness, u32, u32) {
        let map = ex::skirmish(vec![
            ex::resource("open", ex::TREE, OPEN.0, OPEN.1),
            ex::resource("behind", ex::TREE, BEHIND.0, BEHIND.1),
        ]);
        let mut world = ex::world(&map);
        fx::run(&mut world, 2);
        let (open, behind) = (ex::id(&world, "open"), ex::id(&world, "behind"));
        let trees: Vec<Tree> = [open, behind]
            .into_iter()
            .map(|id| {
                let p = ex::get(&world, id).position;
                Tree {
                    id,
                    root: Vec3::new(p.x as f32 / 1000.0, 0.0, p.y as f32 / 1000.0),
                    scale: 1.0,
                    profile: 0,
                }
            })
            .collect();
        let mut h = Harness::on(world, 0, [48.0, 47.0]);
        h.play
            .set_trees(Arc::new(Trees::new(vec![profile()], trees)));
        h.refresh();
        (h, open, behind)
    }

    /// The pixel of a point `up` metres above `at` and `east` of it.
    fn crown(h: &Harness, at: (f64, f64), east: f32, up: f32) -> Vec2 {
        h.screen
            .project(Vec3::new(at.0 as f32 + east, up, at.1 as f32))
            .expect("on screen")
            .0
    }

    fn hover(h: &mut Harness, p: Vec2) -> Option<u32> {
        h.input(Input::PointerMoved(p));
        h.advance(16);
        h.play.hovered
    }

    /// The starting worker without a gathering job (it stands at 46, 50).
    fn idle_worker(h: &Harness) -> u32 {
        h.mine(ex::WORKER)
            .into_iter()
            .find(|&w| {
                ex::get(&h.world, w)
                    .unit
                    .as_ref()
                    .is_some_and(|u| u.work.job.is_none())
            })
            .expect("an idle worker")
    }

    #[test]
    fn a_click_anywhere_on_the_drawn_tree_gathers_it_and_empty_ground_moves() {
        let (mut h, open, _) = harness();
        let worker = idle_worker(&h);
        h.click(worker);
        // The crown's edge, 1.2 m east of the trunk and 2.5 m up: well off the root's
        // cell (its ground point lies cells away), still the tree.
        let p = crown(&h, OPEN, 1.2, 2.5);
        let ground = h
            .screen
            .ground(p, &FlatGround)
            .expect("the ground behind the crown");
        assert!(
            (ground.x - OPEN.0 as f32).abs() > 0.8 || (ground.z - OPEN.1 as f32).abs() > 0.8,
            "off the root cell: {ground}"
        );
        assert_eq!(hover(&mut h, p), Some(open));
        h.click_px(p, Button::Right);
        let sent = h.take();
        assert!(
            matches!(sent.as_slice(), [Action::Gather { actors, target, .. }] if actors == &[worker] && *target == open),
            "{sent:?}"
        );
        h.apply(&sent, 2);
        assert!(h.errors().is_empty(), "{:?}", h.errors());
        // The trunk too.
        let trunk = crown(&h, OPEN, 0.0, 0.5);
        assert_eq!(hover(&mut h, trunk), Some(open));
        // Beside the crown is ground.
        let beside = crown(&h, OPEN, 2.5, 2.5);
        assert_eq!(hover(&mut h, beside), None);
        h.click_px(beside, Button::Right);
        assert!(matches!(h.take().as_slice(), [Action::Move { .. }]));
    }

    #[test]
    fn a_unit_in_front_of_a_tree_wins() {
        let (mut h, _, behind) = harness();
        let worker = idle_worker(&h);
        // The worker's head shows over the crown of the tree behind it.
        let p = h
            .screen
            .project(Vec3::new(46.0, 1.4, 50.0))
            .expect("on screen")
            .0;
        let (origin, dir) = h.screen.ray(p);
        assert_eq!(
            h.play
                .trees
                .hit(origin, dir, f32::MAX, |_| true)
                .map(|(id, _)| id),
            Some(behind),
            "the ray meets the tree too"
        );
        assert_eq!(hover(&mut h, p), Some(worker));
    }

    #[test]
    fn a_falling_tree_is_not_picked_but_a_struck_one_is() {
        let (mut h, open, _) = harness();
        let worker = idle_worker(&h);
        h.click(worker);
        let p = crown(&h, OPEN, 1.0, 2.0);
        h.click_px(p, Button::Right);
        let sent = h.take();
        h.apply(&sent, 1);
        let felling = |h: &Harness| {
            h.world
                .state()
                .entities
                .get(canopy_sim::entity::EntityId(open))
                .and_then(|e| e.resource.as_ref()?.felling.clone())
        };
        fx::run_until(&mut h.world, 4000, |w| {
            w.state()
                .entities
                .get(canopy_sim::entity::EntityId(open))
                .and_then(|e| e.resource.as_ref()?.felling.as_ref()?.last_hit_tick)
                .is_some()
        });
        h.refresh();
        assert!(felling(&h).is_some_and(|f| f.fall_tick.is_none()));
        assert_eq!(hover(&mut h, p), Some(open), "struck, still standing");
        fx::run_until(&mut h.world, 8000, |w| {
            w.state()
                .entities
                .get(canopy_sim::entity::EntityId(open))
                .is_none_or(|e| {
                    e.resource
                        .as_ref()
                        .and_then(|r| r.felling.as_ref())
                        .is_some_and(|f| f.fall_tick.is_some())
                })
        });
        h.refresh();
        assert_eq!(hover(&mut h, p), None, "fallen");
    }

    #[test]
    fn the_hover_ring_marks_a_tree_only_for_a_selection_that_gathers_it() {
        let (mut h, open, _) = harness();
        let p = crown(&h, OPEN, 0.8, 2.0);
        assert_eq!(hover(&mut h, p), Some(open));
        assert_eq!(h.play.world_ui().tree, None, "nothing selected");
        let hall = h.mine(ex::HALL)[0];
        h.click(hall);
        assert_eq!(hover(&mut h, p), Some(open));
        assert_eq!(h.play.world_ui().tree, None, "a hall cannot gather");
        let worker = idle_worker(&h);
        h.click(worker);
        assert_eq!(hover(&mut h, p), Some(open));
        let ring = h.play.world_ui().tree.expect("a worker can");
        assert_eq!(ring.position, [OPEN.0 as f32, OPEN.1 as f32]);
        // Just outside the crown's widest reach.
        assert!(ring.radius > 1.5 / crate::play::picking::CROWN_SHARE && ring.radius < 2.5);
        assert!(ring.radius > GATHER_RADIUS);
        // Not over the ground.
        let beside = crown(&h, OPEN, 2.5, 2.5);
        assert_eq!(hover(&mut h, beside), None);
        assert_eq!(h.play.world_ui().tree, None);
    }
}

/// A fallen hero is revived at a building with revival: the card offers it.
#[test]
fn the_sanctuary_offers_to_revive_a_fallen_hero() {
    use crate::sim::economy_fixture as ef;
    let mut world = ef::world(&ef::sandbox(vec![
        ef::place("hall", ef::HALL, 19.5, 19.5),
        ef::place("sanctuary", ef::SANCTUARY, 35.5, 19.5),
        ef::place("hero", ef::HERO, 30.0, 40.0),
    ]));
    let hero = ef::id(&world, "hero");
    let entity = world
        .state_mut()
        .entities
        .get_mut(canopy_sim::entity::EntityId(hero))
        .unwrap();
    canopy_sim::heroes::retain(entity);
    ef::fund(&mut world, ef::P1, ef::AMBER, 200);
    world.step();
    let content = world.catalog().content.clone();
    let know = Knowledge::new(
        content.clone(),
        Arc::new(Catalog::new(content)),
        Arc::new(world.view(ef::P1)),
        Some(0),
    );
    let sanctuary = ef::id(&world, "sanctuary");
    let revive = super::card::commands(&know, &[sanctuary])
        .into_iter()
        .find(|c| c.id == format!("revive:{hero}"))
        .expect("a revive command");
    assert_eq!(revive.reason, None, "affordable and ready");

    // Its portrait counts down the revival: one second (40 ticks) at level one.
    let portrait = |world: &World| {
        let content = world.catalog().content.clone();
        let know = Knowledge::new(
            content.clone(),
            Arc::new(Catalog::new(content)),
            Arc::new(world.view(ef::P1)),
            Some(0),
        );
        super::hud::revival(&know, hero)
    };
    assert_eq!(portrait(&world), None, "fallen, nobody reviving it yet");
    let refused = ef::try_command(
        &mut world,
        serde_json::json!({"type": "revive", "actor": sanctuary, "hero": hero}),
    );
    assert_eq!(refused, None);
    assert_eq!(portrait(&world), Some(Some(1)));
    for _ in 0..30 {
        world.step();
    }
    assert_eq!(
        portrait(&world),
        Some(Some(1)),
        "a part second shows as a whole"
    );
}

#[test]
fn right_clicking_the_minimap_moves_the_selection_there() {
    let mut h = Harness::new(vec![]);
    let workers = h.mine(fx::WORKER);
    h.click(workers[0]);
    h.take();
    let (x, z) = (fx::AWAY.0 as f32 + 20.0, fx::AWAY.1 as f32 + 6.0);
    h.play.minimap_command(x, z);
    let sent = h.take();
    let [
        Action::Move {
            actors,
            destination,
            ..
        },
    ] = sent.as_slice()
    else {
        panic!("{sent:?}");
    };
    assert_eq!(actors, &[workers[0]]);
    assert_eq!((destination.x, destination.y), (x as u32, z as u32));
}

/// The shipped interface, laid out headless (system fonts when the bundled ones are Git
/// LFS pointers).
fn shipped_ui() -> canopy_ui::Ui {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../config/ui");
    let mut config = canopy_ui::UiConfig::load(&dir).expect("config/ui reads");
    let pointers = config
        .font_data
        .iter()
        .any(|f| f.starts_with(b"version https://git-lfs"));
    if pointers {
        config.font_data.clear();
    }
    canopy_ui::Ui::new(
        config,
        canopy_ui::FontSources {
            bundled: Vec::new(),
            system: pointers,
        },
    )
    .expect("config/ui compiles")
}

#[test]
fn a_right_click_on_the_minimap_orders_without_moving_the_camera() {
    use canopy_ui::{ActionKind, PointerButton};
    let mut h = Harness::new(vec![]);
    let workers = h.mine(fx::WORKER);
    h.click(workers[0]);
    h.take();
    let mut ui = shipped_ui();
    let mut hud = super::hud::Hud::new();
    hud.minimap_area = [0.0, 0.0, h.world.terrain().size() as f32];
    let minimap = |kind| canopy_ui::Action {
        node: None,
        path: Some("hud/dock/minimap".into()),
        name: Some("minimap".into()),
        index: None,
        kind,
    };
    let looks = |events: &[Event]| {
        events
            .iter()
            .filter(|e| matches!(e, Event::LookAt(_)))
            .count()
    };
    // A right-click that moves the pointer a little while held (a trackpad click).
    let events = hud.act(
        &mut ui,
        &mut h.play,
        vec![
            minimap(ActionKind::DragStart {
                button: PointerButton::Secondary,
                at: canopy_ui::Vec2::new(0.5, 0.5),
            }),
            minimap(ActionKind::Drag {
                at: canopy_ui::Vec2::new(0.51, 0.5),
            }),
        ],
    );
    assert_eq!(looks(&events), 0, "{events:?}");
    let sent = h.take();
    assert!(matches!(sent.as_slice(), [Action::Move { .. }]), "{sent:?}");
    // A left drag still looks around.
    let events = hud.act(
        &mut ui,
        &mut h.play,
        vec![
            minimap(ActionKind::DragStart {
                button: PointerButton::Primary,
                at: canopy_ui::Vec2::new(0.5, 0.5),
            }),
            minimap(ActionKind::Drag {
                at: canopy_ui::Vec2::new(0.6, 0.5),
            }),
        ],
    );
    assert_eq!(looks(&events), 2);
}

#[test]
fn items_are_picked_by_their_drawn_body() {
    let (x, z) = (fx::AWAY.0.floor() as f32 + 6.0, fx::AWAY.1.floor() as f32);
    let mut h = Harness::new(vec![serde_json::json!({
        "id": "charm", "definition": fx::CHARM,
        "position": {"x": x, "y": z}, "owner": "none",
    })]);
    let charm = h
        .world
        .state()
        .entities
        .iter()
        .find(|e| e.placement.as_deref() == Some("charm"))
        .expect("the charm")
        .id
        .0;
    // A metre east and a metre up: on the drawn item, but its ground point lies well
    // outside the small square the ground rule used to need.
    let (p, _) = h
        .screen
        .project(Vec3::new(x + 1.0, 1.0, z))
        .expect("on screen");
    h.input(Input::PointerMoved(p));
    h.advance(16);
    assert_eq!(h.play.hovered, Some(charm));
}

#[test]
fn the_idle_worker_button_selects_an_idle_worker() {
    use canopy_ui::{ActionKind, Modifiers, PointerButton};
    let mut h = Harness::new(vec![]);
    let idle = h.play.idle_workers();
    assert!(!idle.is_empty(), "the fixture's workers start idle");
    let mut ui = shipped_ui();
    let mut hud = super::hud::Hud::new();
    hud.act(
        &mut ui,
        &mut h.play,
        vec![canopy_ui::Action {
            node: None,
            path: Some("hud/dock/idle".into()),
            name: Some("idle".into()),
            index: None,
            kind: ActionKind::Click {
                button: PointerButton::Primary,
                modifiers: Modifiers::default(),
                count: 1,
            },
        }],
    );
    let selected = h.play.selection.ids();
    assert_eq!(selected.len(), 1);
    assert!(idle.contains(&selected[0]));
}

#[test]
fn a_building_that_can_research_shows_its_training_queue_after_reselection() {
    use crate::sim::economy_fixture as ef;
    use canopy_ui::{ActionKind, Modifiers, PointerButton};
    let mut world = ef::world(&ef::sandbox(vec![
        ef::place("nexus", ef::NEXUS, 19.5, 19.5),
        ef::place("shop", ef::WORKSHOP, 29.5, 21.5),
        ef::place("house", ef::HOUSE, 61.5, 61.5),
    ]));
    ef::fund(&mut world, ef::P1, ef::AMBER, 500);
    let shop = ef::id(&world, "shop");
    let produce = serde_json::json!({"type": "produce", "actor": shop, "definition": ef::SOLDIER});
    assert_eq!(ef::try_command(&mut world, produce), None);
    ef::run(&mut world, 2);
    let mut h = Harness::on(world, 0, [29.5, 21.5]);
    let mut ui = shipped_ui();
    let mut hud = super::hud::Hud::new();
    let task = |ui: &canopy_ui::Ui| {
        let label = ui.find("hud/dock/selection/copy/task/meter/label")?;
        ui.text(label).map(str::to_owned)
    };
    // It trains while its research queue stands empty: the soldier is the task, every
    // time it is selected.
    let queue = ui.find("hud/dock/selection/queue").expect("the queue");
    for _ in 0..2 {
        h.click(shop);
        assert_eq!(h.play.selection.ids(), [shop]);
        hud.update(&mut ui, &h.play);
        let label = task(&ui).unwrap_or_default();
        assert!(label.contains(ef::SOLDIER), "{label}");
        assert!(ui.count(queue) > 0);
        let ground = h.pixel(36.0, 27.0);
        h.click_px(ground, Button::Left);
        assert!(h.play.selection.ids().is_empty());
        hud.update(&mut ui, &h.play);
        assert_eq!(ui.count(queue), 0);
    }
    // Researching too: the soldier stays the task and the research waits in the tiles.
    let research = serde_json::json!({"type": "research", "actor": shop, "research": ef::AMPLIFY});
    assert_eq!(ef::try_command(&mut h.world, research), None);
    h.refresh();
    h.click(shop);
    hud.update(&mut ui, &h.play);
    let label = task(&ui).unwrap_or_default();
    assert!(label.contains(ef::SOLDIER), "{label}");
    // Its tile cancels the research.
    hud.act(
        &mut ui,
        &mut h.play,
        vec![canopy_ui::Action {
            node: None,
            path: Some("hud/dock/selection/queue".into()),
            name: Some("queue".into()),
            index: Some(0),
            kind: ActionKind::Click {
                button: PointerButton::Primary,
                modifiers: Modifiers::default(),
                count: 1,
            },
        }],
    );
    let sent = h.take();
    assert!(
        matches!(sent.as_slice(), [Action::CancelResearch { actor, .. }] if *actor == shop),
        "{sent:?}"
    );
}
