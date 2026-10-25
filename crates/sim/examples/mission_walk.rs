//! Plays a mission map headless as a plain player would, without casting a spell: player
//! 1's whole army walks a route of attack-moves and forced attacks, every scene is skipped
//! as it starts, and the run reports what happened at each step and how it ended. A probe
//! for mission authors (is it winnable, does the script run to its end, where does it
//! stall); not a test.
//!
//! ```text
//! cargo run --release -p canopy-sim --example mission_walk -- \
//!     --data . --map base/maps/showcase/<id>.utcm --cap 3 \
//!     --route "go:84,318; attack:cache.salve; go:134,298; wait:200; …"
//! ```
//!
//! Steps: `go:x,z` attack-moves the army and waits until the hero (`--hero`, `queen` by
//! default) is within 8 cells or the army stops; `attack:<placement>` sends every unit at
//! it until it is gone; `wait:<ticks>`. `--watch` (no scene skipping) lets scenes play.
#![allow(clippy::disallowed_methods)] // wall-clock timing

use canopy_content::{Content, placement::Owner};
use canopy_sim::{
    Action, MatchSetup, Queued, Slot, SlotKind, World,
    action::{CastTarget, CellPoint},
    campaign::CampaignStart,
};
use std::{path::PathBuf, sync::Arc};

struct Args {
    data: PathBuf,
    map: PathBuf,
    cap: Option<u32>,
    route: String,
    hero: String,
    watch: bool,
    limit: u32,
    /// Placements whose health each report shows.
    track: Vec<String>,
    /// Abilities the hero learns whenever it can, in this order.
    learn: Vec<String>,
    /// A healing ability the hero casts on the most wounded of its army.
    heal: Option<String>,
    /// A self-shield the hero casts when its health runs low.
    shield: Option<String>,
}

fn args() -> Args {
    let mut args = Args {
        data: PathBuf::from("."),
        map: PathBuf::new(),
        cap: None,
        route: String::new(),
        hero: "queen".into(),
        watch: false,
        limit: 40 * 60 * 40,
        track: Vec::new(),
        learn: Vec::new(),
        heal: None,
        shield: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().unwrap_or_else(|| panic!("{flag} needs a value"));
        match flag.as_str() {
            "--data" => args.data = PathBuf::from(value()),
            "--map" => args.map = PathBuf::from(value()),
            "--cap" => args.cap = Some(value().parse().expect("--cap")),
            "--route" => args.route = value(),
            "--hero" => args.hero = value(),
            "--limit" => args.limit = value().parse().expect("--limit"),
            "--watch" => args.watch = true,
            "--dump" => {
                value();
            }
            "--learn" => args.learn = value().split(',').map(String::from).collect(),
            "--heal" => args.heal = Some(value()),
            "--shield" => args.shield = Some(value()),
            "--track" => args.track = value().split(',').map(String::from).collect(),
            other => panic!("unknown option {other}"),
        }
    }
    args
}

fn by_placement(world: &World, id: &str) -> Option<u32> {
    world
        .state()
        .entities
        .iter()
        .find(|e| e.placement.as_deref() == Some(id) && e.hp.is_none_or(|hp| hp > 0))
        .map(|e| e.id.0)
}

fn army(world: &World) -> Vec<u32> {
    world
        .state()
        .entities
        .iter()
        .filter(|e| {
            e.owner == Owner::Player(0) && e.unit.is_some() && e.hp.is_some_and(|hp| hp > 0)
        })
        .map(|e| e.id.0)
        .collect()
}

fn send(world: &mut World, action: Action) {
    let tick = world.tick();
    let _ = world.enqueue(Queued {
        tick,
        player: 0,
        seq: 0,
        action,
    });
}

struct Run {
    world: World,
    watch: bool,
    errors: usize,
    hero: String,
    learn: Vec<String>,
    heal: Option<String>,
    shield: Option<String>,
}

impl Run {
    /// One tick; scenes are skipped as they start.
    fn step(&mut self) {
        let cinema = self
            .world
            .state()
            .mission
            .as_ref()
            .is_some_and(|m| m.cinema.scene.is_some());
        if cinema && !self.watch {
            send(&mut self.world, Action::SkipScene);
        }
        if self.world.tick() % 80 == 40 {
            self.play_hero();
        }
        self.world.step();
        let facts = &self.world.state().facts;
        let errors: Vec<_> = facts
            .iter()
            .filter(|f| f.message.contains("Mission script stopped"))
            .collect();
        if errors.len() > self.errors {
            for f in &errors[self.errors..] {
                println!("  !! {}", f.message);
            }
            self.errors = errors.len();
        }
    }

    /// What a plain player does with a hero: learns and heals.
    fn play_hero(&mut self) {
        let Some(hero) = by_placement(&self.world, &self.hero) else {
            return;
        };
        for ability in self.learn.clone() {
            send(
                &mut self.world,
                Action::LearnAbility {
                    actor: hero,
                    ability,
                },
            );
        }
        if let Some(binding) = self.shield.clone()
            && self
                .world
                .state()
                .entities
                .iter()
                .any(|e| e.id.0 == hero && e.hp.is_some_and(|hp| hp < 320))
        {
            send(
                &mut self.world,
                Action::CastAbility {
                    actor: hero,
                    binding,
                    target: CastTarget::Unit { entity: hero },
                },
            );
        }
        let Some(binding) = self.heal.clone() else {
            return;
        };
        let state = self.world.state();
        let at = state
            .entities
            .iter()
            .find(|e| e.id.0 == hero)
            .map(|e| e.position);
        let wounded = state
            .entities
            .iter()
            .filter(|e| e.owner == Owner::Player(0) && e.unit.is_some() && e.id.0 != hero)
            .filter(|e| e.hp.is_some_and(|hp| hp > 0 && hp < 130))
            .filter(|e| {
                at.is_some_and(|a| {
                    let (dx, dz) = (f64::from(e.position.x - a.x), f64::from(e.position.y - a.y));
                    dx * dx + dz * dz < 14_000.0 * 14_000.0
                })
            })
            .min_by_key(|e| e.hp)
            .map(|e| e.id.0);
        if let Some(target) = wounded {
            send(
                &mut self.world,
                Action::CastAbility {
                    actor: hero,
                    binding,
                    target: CastTarget::Unit { entity: target },
                },
            );
        }
    }

    fn over(&self) -> bool {
        self.world.state().outcome.is_some()
    }

    fn report(&self, what: &str, hero: &str, track: &[String]) {
        let state = self.world.state();
        let units = army(&self.world).len();
        let hero = state
            .entities
            .iter()
            .find(|e| e.placement.as_deref() == Some(hero));
        let (hp, xp) = hero.map_or((0, 0), |h| {
            (
                h.hp.unwrap_or(0),
                h.progression.as_ref().map_or(0, |p| p.experience),
            )
        });
        let (x, z) = hero.map_or((0, 0), |h| (h.position.x / 1000, h.position.y / 1000));
        let objectives: Vec<String> = state
            .mission
            .as_ref()
            .map(|m| {
                m.objective_states
                    .iter()
                    .map(|(k, v)| format!("{k}:{v:?}"))
                    .collect()
            })
            .unwrap_or_default();
        println!(
            "{:>6} t ({:>4.0} s) {what:<28} army {units:>2}  hero hp {hp:>4} xp {xp:>4} at {x},{z}  {}",
            state.tick,
            f64::from(state.tick) / 40.0,
            objectives.join(" ")
        );
        if let Some(m) = &state.mission {
            if let Some(error) = &m.error {
                println!("         script error: {error}");
            }
            if let Some(scene) = &m.cinema.scene {
                println!("         scene: {}", scene.name);
            }
        }
        for id in track {
            let hp = state
                .entities
                .iter()
                .find(|e| e.placement.as_deref() == Some(id.as_str()))
                .map_or("absent".to_string(), |e| format!("{:?}", e.hp));
            println!("         {id}: {hp}");
        }
    }
}

fn main() {
    let args = args();
    let content = Arc::new(Content::load(&args.data).unwrap_or_else(|e| panic!("{e}")));
    let map = canopy_content::map::package::load(&args.map).unwrap_or_else(|e| panic!("{e}"));
    let setup = MatchSetup {
        seed: 7,
        slots: vec![Slot {
            difficulty: Default::default(),
            player: 0,
            kind: SlotKind::Human,
            name: None,
            team: None,
            hero: None,
            race: None,
        }],
    };
    let start = CampaignStart {
        difficulty: Default::default(),
        player: 0,
        locked: Default::default(),
        heroes: Vec::new(),
        hero_level_cap: args.cap,
        variables: Default::default(),
    };
    let world =
        World::new_campaign(content, &map, setup, &start).unwrap_or_else(|e| panic!("{}", e.0));
    let mut run = Run {
        world,
        watch: args.watch,
        errors: 0,
        hero: args.hero.clone(),
        learn: args.learn.clone(),
        heal: args.heal.clone(),
        shield: args.shield.clone(),
    };
    // `--dump x0,z0,x1,z1`: the walkable cells of that box (every second cell), then out.
    if let Some(dump) = std::env::args().skip_while(|a| a != "--dump").nth(1) {
        let b: Vec<i64> = dump
            .split(',')
            .map(|v| v.parse().expect("--dump x0,z0,x1,z1"))
            .collect();
        let terrain = run.world.terrain();
        for z in (b[1]..b[3]).step_by(2) {
            let row: String = (b[0]..b[2])
                .step_by(2)
                .map(|x| match terrain.cell(x, z) {
                    Some(c) if terrain.walkable(c) => '.',
                    _ => '#',
                })
                .collect();
            println!("{z:4} {row}");
        }
        return;
    }
    // Let the opening play out (skipped unless watching).
    for _ in 0..80 {
        run.step();
    }
    run.report("start", &args.hero, &args.track);
    for step in args
        .route
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if run.over() || run.world.tick() > args.limit {
            break;
        }
        let (kind, value) = step.split_once(':').unwrap_or((step, ""));
        match kind {
            "go" => {
                let xy: Vec<u32> = value
                    .split(',')
                    .map(|v| v.trim().parse().expect("go:x,z"))
                    .collect();
                let actors = army(&run.world);
                send(
                    &mut run.world,
                    Action::Move {
                        actors,
                        destination: CellPoint {
                            x: xy[0],
                            y: xy[1],
                            surface: None,
                        },
                        attack_move: Some(true),
                        append: None,
                    },
                );
                let mut still = 0;
                for _ in 0..40 * 120 {
                    run.step();
                    if run.over() {
                        break;
                    }
                    let near = run
                        .world
                        .state()
                        .entities
                        .iter()
                        .find(|e| e.placement.as_deref() == Some(args.hero.as_str()))
                        .is_some_and(|h| {
                            let dx = f64::from(h.position.x) / 1000.0 - f64::from(xy[0]);
                            let dz = f64::from(h.position.y) / 1000.0 - f64::from(xy[1]);
                            dx * dx + dz * dz < 256.0
                        });
                    if near || still > 0 {
                        still += 1;
                        if still > 40 {
                            break;
                        }
                    }
                    // Re-issue the walk now and then: scenes and fights interrupt it.
                    if run.world.tick().is_multiple_of(200) {
                        let actors = army(&run.world);
                        send(
                            &mut run.world,
                            Action::Move {
                                actors,
                                destination: CellPoint {
                                    x: xy[0],
                                    y: xy[1],
                                    surface: None,
                                },
                                attack_move: Some(true),
                                append: None,
                            },
                        );
                    }
                }
            }
            "attack" => {
                for _ in 0..40 * 120 {
                    let Some(target) = by_placement(&run.world, value) else {
                        break;
                    };
                    if run.world.tick().is_multiple_of(80) {
                        let actors = army(&run.world);
                        send(
                            &mut run.world,
                            Action::Attack {
                                actors,
                                target,
                                force: Some(true),
                                append: None,
                            },
                        );
                    }
                    run.step();
                    if run.over() {
                        break;
                    }
                }
            }
            "wait" => {
                for _ in 0..value.parse::<u32>().expect("wait:ticks") {
                    run.step();
                    if run.over() {
                        break;
                    }
                }
            }
            other => panic!("unknown step {other}"),
        }
        run.report(step, &args.hero, &args.track);
    }
    // Let any last scene finish.
    for _ in 0..400 {
        if run.over() {
            break;
        }
        run.step();
    }
    let outcome = &run.world.state().outcome;
    println!(
        "outcome at tick {}: {}",
        run.world.tick(),
        match outcome {
            Some(o) if o.winner == Some(Owner::Player(0)) => "victory".to_string(),
            Some(o) => format!("defeat {o:?}"),
            None => "none".to_string(),
        }
    );
}
