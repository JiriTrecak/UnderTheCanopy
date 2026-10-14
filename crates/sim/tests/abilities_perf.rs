//! Ability cost per tick. Measurements, not behaviour tests; run them in release:
//!
//! `cargo test --release -p canopy-sim --test abilities_perf -- --ignored --nocapture`
#![allow(clippy::disallowed_methods)] // wall-clock timing of the benchmark itself

mod abilities_fixture;

use abilities_fixture::*;
use canopy_sim::{MatchSetup, Slot, SlotKind, World};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tracing::{
    Event, Metadata, Subscriber,
    span::{Attributes, Id, Record},
};

/// Sums the time spent in each named span.
#[derive(Default)]
struct Spans {
    next: AtomicU64,
    names: Mutex<BTreeMap<u64, &'static str>>,
    open: Mutex<BTreeMap<u64, Instant>>,
    totals: Mutex<BTreeMap<&'static str, Duration>>,
}

impl Subscriber for &'static Spans {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        self.names
            .lock()
            .unwrap()
            .insert(id, span.metadata().name());
        Id::from_u64(id)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, _: &Event<'_>) {}
    fn enter(&self, span: &Id) {
        self.open
            .lock()
            .unwrap()
            .insert(span.into_u64(), Instant::now());
    }
    fn exit(&self, span: &Id) {
        let id = span.into_u64();
        let Some(start) = self.open.lock().unwrap().remove(&id) else {
            return;
        };
        let name = self.names.lock().unwrap().remove(&id).unwrap_or("?");
        *self.totals.lock().unwrap().entry(name).or_default() += start.elapsed();
    }
}

/// Span totals over a whole run.
static ALL: Mutex<BTreeMap<&'static str, Duration>> = Mutex::new(BTreeMap::new());

/// Ability spans nested in the ones above, reported separately.
const PARTS: [&str; 2] = ["autocast", "auras"];
/// Top-level ability and item spans of a tick.
const ABILITIES: [&str; 6] = [
    "ability lifecycle",
    "cast approach",
    "ability delivery",
    "regeneration",
    "item effects",
    "item pickups",
];

fn setup_for(players: u8) -> MatchSetup {
    MatchSetup {
        seed: 7,
        slots: (0..players)
            .map(|player| Slot {
                player,
                kind: SlotKind::Human,
                name: None,
                team: None,
                hero: None,
                race: None,
            })
            .collect(),
    }
}

/// Heroes with an armour aura, critical strikes, cleave, a bashing trigger and an autocast
/// nova; casters with autocast heals and bolts and a slowing aura; warriors whose blows
/// stack a slowing mark.
fn armed_spec() -> Spec {
    let mut spec = Spec::default();
    let auto = json!({"intervalTicks": 12, "enabledByDefault": true});
    spec.abilities.push(with(passive("ability.test.command", json!([
        {"op": "status", "target": "target", "id": "command", "amount": 1, "polarity": "positive", "dispel": false, "modifiers": {"armor": 2, "damagePermille": 100}}])),
        json!({"aura": {"radius": 6, "meleeOnly": false},
            "combatModifiers": {"critical": {"chancePermille": 200, "multiplierPermille": 1750},
                "cleave": {"damagePermille": 300, "radius": 3, "arcDegrees": 120, "maxTargets": 4}},
            "triggers": [{"id": "bash", "event": "weaponHit", "chancePermille": 200, "operations": [
                {"op": "status", "target": "target", "id": "bashed", "amount": 30, "polarity": "negative", "dispel": false, "stun": true}]}]})));
    spec.abilities.push(with(spell("ability.test.nova", "self", &["ally"], 0, 40, 160, 6, 6, json!([
        {"op": "damage", "target": "target", "query": {"center": "caster", "radius": 5, "relations": ["enemy"]}, "amount": 25, "damageType": "spell"},
        {"op": "status", "target": "target", "query": {"center": "caster", "radius": 5, "relations": ["enemy"]}, "id": "dazed", "amount": 40, "polarity": "negative", "dispel": true, "modifiers": {"moveSpeedPermille": -300}}])),
        json!({"autocast": auto})));
    spec.hero.push(bind("command", "ability.test.command", 1));
    spec.hero.push(bind("nova", "ability.test.nova", 1));
    let mut mend = with(
        spell(
            "ability.test.mend",
            "unit",
            &["ally"],
            10,
            20,
            40,
            6,
            6,
            json!([{"op": "heal", "target": "target", "amount": 30}]),
        ),
        json!({"autocast": auto}),
    );
    mend["targeting"]["allowSelf"] = json!(true);
    mend["targeting"]["condition"] = json!({"kind": "resource", "of": "target", "resource": "health", "measure": "missing", "comparison": "gt", "value": 0});
    spec.abilities.push(mend);
    spec.abilities.push(with(
        spell(
            "ability.test.bolt",
            "unit",
            &["enemy"],
            10,
            20,
            60,
            6,
            6,
            json!([
        {"op": "damage", "target": "target", "amount": 30, "damageType": "spell"}]),
        ),
        json!({"autocast": auto, "delivery": {"kind": "projectile", "speed": 14}}),
    ));
    let mut howl = with(
        passive(
            "ability.test.howl",
            json!([
        {"op": "status", "target": "target", "id": "howl", "amount": 1, "polarity": "negative", "dispel": false, "modifiers": {"moveSpeedPermille": -200}}]),
        ),
        json!({"aura": {"radius": 5, "meleeOnly": false}}),
    );
    howl["targeting"]["relations"] = json!(["enemy"]);
    howl["targeting"]["allowSelf"] = json!(false);
    spec.abilities.push(howl);
    spec.caster.push(bind("mend", "ability.test.mend", 1));
    spec.caster.push(bind("bolt", "ability.test.bolt", 1));
    spec.caster.push(bind("howl", "ability.test.howl", 1));
    spec.abilities.push(with(passive("ability.test.pressure", json!([])), json!({
        "combatModifiers": {"attackBonus": {"status": "pressure", "amount": 0, "manaCost": 0, "weapon": "melee", "blockedBySilence": false}},
        "statuses": [{"op": "status", "target": "target", "id": "pressure", "amount": 240, "polarity": "negative", "dispel": true,
            "stacking": {"max": 3, "scope": "source"}, "sourceAttackBonus": 2, "modifiers": {"moveSpeedPermille": -50}}]})));
    spec.warrior
        .push(bind("pressure", "ability.test.pressure", 1));
    spec
}

fn armies() -> (Vec<Value>, Vec<Vec<String>>) {
    let mut entities = Vec::new();
    let mut names = vec![Vec::new(); 8];
    for player in 0..8u8 {
        let (cx, cy) = [
            (158, 128),
            (149, 149),
            (128, 158),
            (107, 149),
            (98, 128),
            (107, 107),
            (128, 98),
            (149, 107),
        ][usize::from(player)];
        for i in 0..50u32 {
            let (dx, dy) = ((i % 7) as i32 * 2 - 6, (i / 7) as i32 * 2 - 7);
            let kind = match i {
                0 => "unit.hero",
                1..=6 => "unit.caster",
                _ if i % 3 == 0 => "unit.archer",
                _ => "unit.warrior",
            };
            let name = format!("p{player}u{i:02}");
            entities.push(placed(
                &name,
                kind,
                f64::from(cx + dx),
                f64::from(cy + dy),
                player + 1,
            ));
            names[usize::from(player)].push(name);
        }
    }
    (entities, names)
}

fn measure(label: &str, spec: &Spec) {
    let (entities, names) = armies();
    let mut world = World::new(content(label, spec), &map(entities), setup_for(8)).unwrap();
    for (player, names) in names.iter().enumerate() {
        let actors: Vec<u32> = names.iter().map(|n| id(&world, n)).collect();
        command(
            &mut world,
            player as u8 + 1,
            json!({"type": "move", "actors": actors, "destination": {"x": 128, "y": 128}, "attackMove": true}),
        );
    }
    let spans: &'static Spans = Box::leak(Box::default());
    let mut samples = Vec::new();
    let mut ability = Vec::new();
    let mut casts = 0;
    tracing::subscriber::with_default(spans, || {
        for _ in 0..1200 {
            spans.totals.lock().unwrap().clear();
            let start = Instant::now();
            world.step();
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
            let totals = spans.totals.lock().unwrap();
            let mut all = ALL.lock().unwrap();
            for (k, v) in totals.iter() {
                *all.entry(k).or_default() += *v;
            }
            ability.push(
                ABILITIES
                    .iter()
                    .filter_map(|n| totals.get(n))
                    .map(|d| d.as_secs_f64() * 1000.0)
                    .sum::<f64>(),
            );
            casts += world
                .drain_events()
                .iter()
                .filter(|e| e.kind == canopy_sim::abilities::EventKind::Released)
                .count();
        }
    });
    let parts: Vec<String> = {
        let mut all = ALL.lock().unwrap();
        let parts = ABILITIES
            .iter()
            .chain(&PARTS)
            .map(|n| {
                format!(
                    "{n} {:.3}",
                    all.get(n)
                        .map_or(0.0, |d| d.as_secs_f64() * 1000.0 / 1200.0)
                )
            })
            .collect();
        all.clear();
        parts
    };
    println!("  per tick (ms): {}", parts.join(", "));
    let ability_mean = ability.iter().sum::<f64>() / ability.len() as f64;
    let ability_busy = ability[..400].iter().sum::<f64>() / 400.0;
    let alive = world
        .state()
        .entities
        .iter()
        .filter(|e| e.unit.is_some() && e.hp.is_some_and(|h| h > 0))
        .count();
    let mut sorted = samples.clone();
    sorted.sort_by(f64::total_cmp);
    let at = |p: f64| sorted[((sorted.len() - 1) as f64 * p) as usize];
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    let busy = &samples[..400];
    let busy_mean = busy.iter().sum::<f64>() / busy.len() as f64;
    println!(
        "{label}: whole tick mean {mean:.3} ms, p50 {:.3}, p99 {:.3}, max {:.3}; first 400 ticks mean {busy_mean:.3} ms; abilities and items {ability_mean:.3} ms (first 400 ticks {ability_busy:.3}); {casts} casts released; {alive} of 400 units left",
        at(0.5),
        at(0.99),
        at(1.0)
    );
}

#[test]
#[ignore = "benchmark"]
fn eight_armies_of_fifty_with_heroes_and_casters() {
    measure("8 × 50 with abilities", &armed_spec());
    measure("8 × 50 without abilities", &Spec::default());
}
