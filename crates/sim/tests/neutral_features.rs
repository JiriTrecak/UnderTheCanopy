//! Neutral map features: breakable crates and rubble, items placed on the ground, springs
//! and shops. Test-owned content only.

mod abilities_fixture;

use abilities_fixture::*;
use canopy_content::game::Kind;
use canopy_sim::{World, entity::EntityId};
use serde_json::{Value, json};

/// A neutral building of the given footprint (the entrance just outside it).
fn neutral(id: &str, width: u32, depth: u32, extra: Value) -> Value {
    definition(
        id,
        "building",
        with(
            json!({"footprint": {"width": width, "depth": depth},
                "entrance": {"x": 0.5, "y": f64::from(depth) / 2.0 + 0.5}, "behaviors": {}}),
            extra,
        ),
    )
}

fn breakable(id: &str, width: u32, depth: u32, hp: u32, loot: Value) -> Value {
    neutral(
        id,
        width,
        depth,
        json!({"body": {"maxHp": hp, "armor": 0, "armorType": "armor.structure"}, "breakable": loot}),
    )
}

fn spec() -> Spec {
    let mut spec = Spec::default();
    spec.definitions.extend([
        item(
            "item.sword",
            json!({"type": "equipment", "damage": 5, "armor": 0, "maxHp": 0}),
        ),
        item(
            "item.ring",
            json!({"type": "equipment", "damage": 0, "armor": 1, "maxHp": 0}),
        ),
        item(
            "item.potion",
            json!({"type": "consumable", "heal": 0,
                "active": {"cooldownTicks": 40, "charges": 1, "target": "self", "radius": 0, "heal": 40}}),
        ),
        item(
            "item.tome",
            json!({"type": "consumable", "heal": 0, "onPickup": true, "permanent": {"maxHp": 20, "damage": 0, "armor": 0}}),
        ),
        definition(
            "item.gold",
            "item",
            json!({"currency": true, "stackLimit": 100000}),
        ),
        breakable(
            "building.crate",
            4,
            4,
            20,
            json!({"lootPool": "loot.swords"}),
        ),
        breakable("building.rubble", 64, 4, 30, json!({})),
        neutral(
            "building.spring",
            4,
            4,
            json!({"restoration": {"radius": 4, "intervalTicks": 40, "health": 2, "healthPermille": 50, "mana": 20}}),
        ),
        neutral(
            "building.shop",
            8,
            8,
            json!({"shop": {"range": 3, "wares": [
                {"item": "item.sword", "price": [{"item": "item.gold", "amount": 100}], "stock": 1, "restockTicks": 200},
                {"item": "item.tome", "price": [{"item": "item.gold", "amount": 50}], "stock": 2, "restockTicks": 100}]}}),
        ),
    ]);
    spec.loot_pools.insert(
        "loot.swords".into(),
        json!({"rolls": 1, "entries": [{"item": "item.sword", "weight": 1}]}),
    );
    spec.loot_pools.insert(
        "loot.tomes".into(),
        json!({"rolls": 2, "entries": [{"item": "item.tome", "weight": 1}]}),
    );
    spec
}

fn initial(placement: Value, state: Value) -> Value {
    with(placement, json!({"initialState": state}))
}

fn attack(actor: u32, target: u32, force: bool, append: bool) -> Value {
    json!({"type": "attack", "actors": [actor], "target": target, "force": force, "append": append})
}

fn buy(shop: u32, item: &str) -> Value {
    json!({"type": "produce", "actor": shop, "definition": item})
}

/// Loose items on the ground by definition id.
fn ground(world: &World) -> Vec<String> {
    let catalog = world.catalog();
    let mut items: Vec<String> = world
        .state()
        .entities
        .iter()
        .filter(|e| catalog.get(e.definition).kind == Kind::Item && e.item.is_some())
        .map(|e| catalog.id(e.definition).to_string())
        .collect();
    items.sort();
    items
}

fn slots(world: &World, placement: &str) -> Vec<String> {
    let catalog = world.catalog();
    entity(world, placement)
        .equipment
        .as_ref()
        .map(|q| {
            q.slots
                .iter()
                .flatten()
                .map(|d| catalog.id(*d).to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Entity ids of a script's placements, from a world made of them.
fn ids(script: &Script) -> impl Fn(&str) -> u32 + use<> {
    let world = World::new(script.content(), &script.map(), setup(false)).expect("world");
    let ids: std::collections::BTreeMap<String, u32> = world
        .state()
        .entities
        .iter()
        .filter_map(|e| Some((e.placement.clone()?, e.id.0)))
        .collect();
    move |p| ids[p]
}

#[test]
fn crates_break_only_when_ordered_and_drop_what_they_hold() {
    let mut script = Script::new(
        "neutral-crates",
        spec(),
        vec![
            placed("w", "unit.warrior", 100.0, 100.0, 1),
            placed("h", "unit.hero", 100.0, 104.0, 1),
            placed("c1", "building.crate", 105.5, 101.5, 0),
            initial(
                placed("c2", "building.crate", 109.5, 101.5, 0),
                json!({"inventory": {"item.potion": 2}}),
            ),
            initial(
                placed("c3", "building.crate", 113.5, 101.5, 0),
                json!({"lootPool": "loot.tomes"}),
            ),
            placed("g", "item.ring", 96.0, 104.0, 0),
        ],
        900,
        vec![],
    );
    let id = ids(&script);
    let (w, h) = (id("w"), id("h"));
    script.commands = vec![
        (1, 1, attack(w, id("c1"), false, false)),
        (
            2,
            1,
            json!({"type": "pickup", "actor": h, "target": id("g"), "append": false}),
        ),
        (120, 1, attack(w, id("c1"), true, false)),
        (121, 1, attack(w, id("c2"), true, true)),
        (122, 1, attack(w, id("c3"), true, true)),
    ];
    // Nothing attacks a crate by itself, nor when asked without force.
    let early = Script::new(
        "neutral-crates-early",
        spec(),
        script.entities.clone(),
        110,
        script.commands.clone(),
    )
    .play();
    assert_eq!(early.hp("c1"), 20, "the warrior left the crate alone");
    assert_eq!(
        slots(&early.world, "h"),
        ["item.ring"],
        "the ring was picked up"
    );

    let played = script.play();
    for c in ["c1", "c2", "c3"] {
        assert!(!played.alive(c), "{c} broke");
    }
    assert_eq!(
        ground(&played.world),
        [
            "item.potion",
            "item.potion",
            "item.sword",
            "item.tome",
            "item.tome"
        ],
        "each crate dropped its own loot"
    );
    let again = script.play_reloading(Some(300));
    assert_eq!(played.prints, again.prints);
    assert_eq!(
        serde_json::to_string(&played.world.save()).unwrap(),
        serde_json::to_string(&again.world.save()).unwrap()
    );
}

/// Ticks until the warrior reaches the far side of the rubble line, and whether it ever
/// stood inside the rubble's footprint.
fn crossing(broken: bool) -> (Option<u32>, bool) {
    let name = if broken {
        "neutral-rubble-broken"
    } else {
        "neutral-rubble"
    };
    let mut world = world(
        name,
        &spec(),
        vec![
            placed("w", "unit.warrior", 128.0, 112.0, 1),
            placed("r", "building.rubble", 127.5, 129.5, 0),
        ],
    );
    let (w, r) = (id(&world, "w"), id(&world, "r"));
    if broken {
        command(&mut world, 1, attack(w, r, true, false));
        run(&mut world, 600);
        assert!(!alive(&world, "r"), "the rubble broke");
        command(&mut world, 1, moving(w, 128, 112));
        run(&mut world, 200);
    }
    command(&mut world, 1, moving(w, 128, 146));
    let mut inside = false;
    for tick in 0..1200 {
        run(&mut world, 1);
        let p = get(&world, EntityId(w)).position;
        let (x, y) = (f64::from(p.x) / 1000.0, f64::from(p.y) / 1000.0);
        inside |= (95.5..159.5).contains(&x) && (127.5..131.5).contains(&y) && !broken;
        if (y - 146.0).abs() < 1.5 && (x - 128.0).abs() < 1.5 {
            return (Some(tick), inside);
        }
    }
    (None, inside)
}

#[test]
fn rubble_closes_the_ground_until_it_is_broken() {
    let (around, inside) = crossing(false);
    let (through, _) = crossing(true);
    let (around, through) = (
        around.expect("walks around"),
        through.expect("walks through"),
    );
    assert!(!inside, "nothing walked through the standing rubble");
    assert!(
        around > through + 200,
        "the standing rubble forced a detour ({around} ticks against {through})"
    );
}

#[test]
fn springs_restore_any_unit_near_them_and_cannot_be_attacked() {
    let mut spec = spec();
    spec.abilities.push(spell(
        "ability.test.bolt",
        "unit",
        &["enemy"],
        8,
        30,
        100,
        10,
        8,
        json!([{"op": "damage", "target": "target", "amount": 40, "damageType": "spell"}]),
    ));
    spec.caster.push(bind("bolt", "ability.test.bolt", 1));
    let mut script = Script::new(
        "neutral-springs",
        spec,
        vec![
            placed("s", "building.spring", 101.5, 101.5, 0),
            placed("near", "unit.warrior", 105.0, 101.0, 1),
            placed("caster", "unit.caster", 98.0, 102.0, 1),
            placed("critter", "unit.critter", 101.0, 105.0, 0),
            placed("far", "unit.warrior", 120.0, 101.0, 1),
            placed("far-caster", "unit.caster", 120.0, 104.0, 1),
        ],
        200,
        vec![],
    )
    .set(0, "near", json!({"hp": 20}))
    .set(0, "far", json!({"hp": 20}))
    .set(0, "critter", json!({"hp": 10}))
    .set(0, "caster", json!({"mana": 0}))
    .set(0, "far-caster", json!({"mana": 0}));
    let id = ids(&script);
    script.commands = vec![(1, 1, attack(id("near"), id("s"), true, false))];
    let played = script.play();
    // Five pulses (ticks 0, 40, …, 160) of 2 + 5% of 100.
    assert_eq!(played.hp("near"), 20 + 5 * 7);
    assert_eq!(played.hp("far"), 20, "out of reach");
    assert_eq!(played.hp("critter"), 10 + 5 * 6, "a neutral creature too");
    assert_eq!(
        played.mana("caster"),
        played.mana("far-caster") + 5 * 20,
        "mana on top of regeneration"
    );
    assert!(
        played
            .errors()
            .contains(&"Target is not visible and damageable".to_string()),
        "a spring has no body to attack: {:?}",
        played.errors()
    );
}

#[test]
fn shops_sell_to_heroes_in_range_with_limited_stock_that_grows_back() {
    let mut script = Script::new(
        "neutral-shops",
        spec(),
        vec![
            placed("s", "building.shop", 111.5, 103.5, 0),
            initial(
                placed("t", "building.shop", 131.5, 103.5, 0),
                json!({"wares": [{"item": "item.ring", "price": [{"item": "item.gold", "amount": 10}], "stock": 1, "restockTicks": 50}]}),
            ),
            placed("h", "unit.hero", 100.0, 103.0, 1),
            placed("w", "unit.warrior", 117.0, 103.0, 1),
        ],
        520,
        vec![],
    )
    .set(250, "h", json!({"wallet": {"item.gold": 500}}));
    let id = ids(&script);
    let (s, t) = (id("s"), id("t"));
    script.commands = vec![
        (1, 1, buy(s, "item.sword")),
        (2, 1, moving(id("h"), 105, 103)),
        (200, 1, buy(s, "item.sword")),
        (260, 1, buy(s, "item.sword")),
        (261, 1, buy(s, "item.sword")),
        (262, 1, buy(s, "item.tome")),
        (263, 1, buy(s, "item.ring")),
        (264, 1, buy(t, "item.ring")),
        (470, 1, buy(s, "item.sword")),
    ];
    let played = script.play();
    let errors = played.errors();
    for expected in [
        "A hero must stand near the shop",
        "Insufficient resources",
        "Sold out",
        "Not sold here",
    ] {
        assert!(
            errors.iter().any(|e| e == expected),
            "missing {expected:?} in {errors:?}"
        );
    }
    assert_eq!(
        slots(&played.world, "h"),
        ["item.sword", "item.sword"],
        "one sword, and another once it grew back"
    );
    let gold = played.world.state().wallets[&player(1)]
        .get(&canopy_content::Id::new("item.gold").unwrap())
        .copied();
    assert_eq!(gold, Some(500 - 100 - 50 - 100));
    let stalls = &played.entity("s").shop.as_ref().expect("a shop").stalls;
    assert_eq!(
        stalls.iter().map(|st| st.stock).collect::<Vec<_>>(),
        [0, 2],
        "the second sword is sold, the tome grew back"
    );
    let placed_wares = &played.entity("t").shop.as_ref().expect("a shop").stalls;
    assert_eq!(placed_wares.len(), 1);
    assert_eq!(placed_wares[0].ware.item.as_str(), "item.ring");
    assert_eq!(
        placed_wares[0].stock, 1,
        "too far to buy from the other shop"
    );

    // The view lists the wares to anyone who sees the shop.
    let view = played.world.view(player(1));
    let wares = view
        .entity(played.id("s"))
        .and_then(|e| e.shop.as_ref())
        .expect("seen wares");
    assert_eq!(
        wares.iter().map(|w| (w.stock, w.most)).collect::<Vec<_>>(),
        [(0, 1), (2, 2)]
    );
    assert!(wares[0].restock.is_some());

    let again = script.play_reloading(Some(265));
    assert_eq!(played.prints, again.prints);
    assert_eq!(
        serde_json::to_string(&played.world.save()).unwrap(),
        serde_json::to_string(&again.world.save()).unwrap()
    );
}
