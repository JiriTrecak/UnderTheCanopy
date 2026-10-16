//! Resolving behaviour sets into definitions, and the cross-reference rules every
//! loaded data set must satisfy.

use crate::{
    Content,
    error::{Error, parse},
    game::{BEHAVIOR_NAMES, Creation, Definition, Kind, ProductionMode},
    load::Element,
};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Building footprints are whole building cells of this many navigation cells.
const BUILDING_CELL_SIZE: u32 = 4;

/// Merges behaviour sets, local behaviours and `disabledBehaviors` into resolved
/// definitions, sorted by id.
pub(crate) fn resolve_definitions(
    behavior_sets: &[Element],
    raw_definitions: Vec<Element>,
    errors: &mut Vec<Error>,
) -> Vec<Definition> {
    let mut sets: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    for set in behavior_sets {
        match behavior_set(&set.value) {
            Ok((id, behaviors)) => {
                sets.insert(id, behaviors);
            }
            Err(message) => errors.push(Error::new(&set.file, "", message)),
        }
    }

    let mut definitions: Vec<Definition> = Vec::new();
    for raw in raw_definitions {
        match merged_definition(&raw.value, &sets) {
            Ok(value) => match parse::<Definition>(value, &raw.file, "") {
                Ok(definition) => {
                    if definition.kind == Kind::Unit && definition.dimensions.is_none() {
                        errors.push(Error::new(
                            &raw.file,
                            "dimensions",
                            "units require explicit dimensions",
                        ));
                    }
                    definitions.push(definition);
                }
                Err(error) => errors.push(error),
            },
            Err(message) => errors.push(Error::new(&raw.file, "", message)),
        }
    }
    // Units, buildings, items and resources share one id space.
    definitions.sort_by(|a, b| a.id.cmp(&b.id));
    for pair in definitions.windows(2) {
        if pair[0].id == pair[1].id {
            errors.push(Error::new(
                "definitions",
                pair[0].id.as_str(),
                "defined twice (as different kinds)",
            ));
        }
    }
    definitions
}

fn behavior_set(raw: &Value) -> Result<(String, Map<String, Value>), String> {
    let object = raw.as_object().ok_or("behavior set must be an object")?;
    if let Some(field) = object.keys().find(|k| *k != "id" && *k != "behaviors") {
        return Err(format!("unknown field `{field}`"));
    }
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .ok_or("behavior set needs an id")?;
    crate::Id::new(id)?;
    let behaviors = object
        .get("behaviors")
        .and_then(Value::as_object)
        .ok_or("behavior set needs behaviors")?;
    check_behavior_names(behaviors)?;
    Ok((id.to_string(), behaviors.clone()))
}

fn check_behavior_names(behaviors: &Map<String, Value>) -> Result<(), String> {
    for (name, values) in behaviors {
        if !BEHAVIOR_NAMES.contains(&name.as_str()) {
            return Err(format!("unknown behavior `{name}`"));
        }
        if !values.is_object() {
            return Err(format!("behavior `{name}` must be an object"));
        }
    }
    Ok(())
}

/// The definition JSON with `behaviorSets`/`disabledBehaviors` folded into `behaviors`.
fn merged_definition(
    raw: &Value,
    sets: &BTreeMap<String, Map<String, Value>>,
) -> Result<Value, String> {
    let mut fields = raw
        .as_object()
        .ok_or("definition must be an object")?
        .clone();
    let set_ids = take_strings(&mut fields, "behaviorSets")?;
    let disabled = take_strings(&mut fields, "disabledBehaviors")?;
    let local = match fields.remove("behaviors") {
        None => Map::new(),
        Some(Value::Object(local)) => local,
        Some(_) => return Err("behaviors must be an object".into()),
    };
    check_behavior_names(&local)?;

    let mut merged: Map<String, Value> = Map::new();
    let mut conflicts: BTreeSet<String> = BTreeSet::new();
    for set_id in &set_ids {
        let set = sets
            .get(set_id)
            .ok_or_else(|| format!("behaviorSets: unknown {set_id}"))?;
        for (name, values) in set {
            let destination = behavior(&mut merged, name);
            for (field, value) in values.as_object().into_iter().flatten() {
                if destination
                    .get(field)
                    .is_some_and(|existing| existing != value)
                {
                    conflicts.insert(format!("{name}.{field}"));
                }
                destination.insert(field.clone(), value.clone());
            }
        }
    }
    for (name, values) in &local {
        let destination = behavior(&mut merged, name);
        for (field, value) in values.as_object().into_iter().flatten() {
            destination.insert(field.clone(), value.clone());
            conflicts.remove(&format!("{name}.{field}"));
        }
    }
    for name in &disabled {
        if !BEHAVIOR_NAMES.contains(&name.as_str()) {
            return Err(format!("disabledBehaviors: unknown behavior `{name}`"));
        }
        merged.remove(name);
        conflicts.retain(|conflict| !conflict.starts_with(&format!("{name}.")));
    }
    if !conflicts.is_empty() {
        let list: Vec<_> = conflicts.into_iter().collect();
        return Err(format!(
            "conflicting behavior defaults: {}",
            list.join(", ")
        ));
    }
    fields.insert("behaviors".into(), Value::Object(merged));
    Ok(Value::Object(fields))
}

fn behavior<'a>(merged: &'a mut Map<String, Value>, name: &str) -> &'a mut Map<String, Value> {
    merged
        .entry(name)
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .expect("behaviors are objects")
}

fn take_strings(fields: &mut Map<String, Value>, key: &str) -> Result<Vec<String>, String> {
    match fields.remove(key) {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .into_iter()
            .map(|item| match item {
                Value::String(text) => Ok(text),
                _ => Err(format!("{key} must list strings")),
            })
            .collect(),
        Some(_) => Err(format!("{key} must be an array")),
    }
}

/// Collects rule violations, each attributed to the definition or table it concerns.
struct Checker<'a> {
    content: &'a Content,
    errors: &'a mut Vec<Error>,
}

impl<'a> Checker<'a> {
    fn fail(&mut self, location: impl Into<String>, message: impl Into<String>) {
        let location = location.into();
        let file = self.content.origin(&location).to_string();
        self.errors.push(Error::new(&file, location, message));
    }

    fn definition(&mut self, location: &str, id: &str, kind: Kind) -> Option<&'a Definition> {
        let content = self.content;
        match content.definition(id) {
            Some(found) if found.kind == kind => Some(found),
            Some(_) => {
                self.fail(location, format!("{id}: expected {}", kind.name()));
                None
            }
            None => {
                self.fail(location, format!("unknown definition {id}"));
                None
            }
        }
    }

    fn model(&mut self, location: &str, id: &str, what: &str) -> bool {
        let ok = self.content.asset(id).is_some_and(|a| a.file.is_some());
        if !ok {
            self.fail(location, format!("{what} must reference a model ({id})"));
        }
        ok
    }

    fn image(&mut self, location: &str, id: &str, what: &str) {
        if !self.content.asset(id).is_some_and(|a| a.image.is_some()) {
            self.fail(location, format!("{what} must reference an image ({id})"));
        }
    }

    fn unique<'b>(&mut self, location: &str, items: impl IntoIterator<Item = &'b str>, what: &str) {
        let mut seen = BTreeSet::new();
        for item in items {
            if !seen.insert(item) {
                self.fail(location, format!("duplicate {what} {item}"));
            }
        }
    }
}

/// Every cross-reference rule of the content.
pub fn validate(content: &Content, errors: &mut Vec<Error>) {
    let mut check = Checker { content, errors };
    research(&mut check);
    render_assets(&mut check);
    damage_tables(&mut check);
    loot(&mut check);
    for definition in &content.definitions {
        definition_rules(&mut check, definition);
    }
    actions(&mut check);
    races(&mut check);
    crate::abilities::validate(content, errors);
    crate::assets::validate(content, errors);
}

fn research(check: &mut Checker) {
    let content = check.content;
    for (id, research) in &content.rules.research {
        let location = format!("rules.research.{id}");
        check.image(&location, research.icon.as_str(), "research icon");
        check.unique(
            &location,
            research.items.iter().map(|c| c.item.as_str()),
            "research input",
        );
        for cost in &research.items {
            check.definition(&location, cost.item.as_str(), Kind::Item);
        }
        for prerequisite in research.requires.iter().flatten() {
            check.definition(&location, prerequisite.as_str(), Kind::Building);
        }
        for effect in &research.effects {
            check.unique(
                &location,
                effect.units.iter().map(|u| u.as_str()),
                "research unit",
            );
            for unit in &effect.units {
                let Some(target) = check.definition(&location, unit.as_str(), Kind::Unit) else {
                    continue;
                };
                let combat = target.behaviors.combat.as_ref();
                let shell = combat.is_some_and(|c| c.shell.is_some());
                let charge = combat.is_some_and(|c| c.charge.is_some());
                let worker = target.behaviors.work.is_some();
                if (effect.splash_radius.is_some() || effect.splash_slow_permille.is_some())
                    && !shell
                {
                    check.fail(&location, "splash research requires shell combat");
                }
                if effect.charge_cooldown_permille.is_some_and(|v| v != 0) && !charge {
                    check.fail(&location, "charge research requires charge combat");
                }
                if effect.tree_work_rate.is_some_and(|v| v != 0) && !worker {
                    check.fail(&location, "tree research requires worker");
                }
            }
        }
    }
}

fn render_assets(check: &mut Checker) {
    let content = check.content;
    for asset in &content.render_assets {
        let location = format!("assets/{}", asset.id);
        if asset.character.is_some() && (asset.file.is_none() || asset.carry_asset.is_some()) {
            check.fail(
                &location,
                "animated characters require a file and use carry animation instead of carryAsset",
            );
        }
        if let Some(carry) = &asset.carry_asset {
            check.model(&location, carry.as_str(), "carryAsset");
        }
        if asset.file.is_none() && asset.image.is_none() {
            check.fail(&location, "missing file/image");
        }
    }
}

fn damage_tables(check: &mut Checker) {
    let rules = &check.content.rules;
    let damage: Vec<_> = rules.damage_types.keys().collect();
    let rows: Vec<_> = rules.damage_multipliers.keys().collect();
    if damage.is_empty() || damage != rows {
        check.fail("rules", "every damage type needs exactly one matchup row");
    }
    let armor: Vec<_> = rules.armor_types.keys().collect();
    for (damage, row) in &rules.damage_multipliers {
        if row.keys().collect::<Vec<_>>() != armor {
            check.fail(
                format!("rules.damageMultipliers.{damage}"),
                "every damage row must exactly cover the armor classes",
            );
        }
    }
    for (id, armor) in &rules.armor_types {
        check.image(
            &format!("rules.armorTypes.{id}"),
            armor.icon.as_str(),
            "armor icon",
        );
    }
}

fn loot(check: &mut Checker) {
    let content = check.content;
    for (id, pool) in &content.rules.loot_pools {
        let location = format!("rules.lootPools.{id}");
        for item in pool.entries.iter().filter_map(|e| e.item.as_ref()) {
            let Some(definition) = check.definition(&location, item.as_str(), Kind::Item) else {
                continue;
            };
            if definition.item_tier.unwrap_or(1) > pool.max_tier.unwrap_or(2) {
                check.fail(&location, "item exceeds declared loot tier");
            }
            if definition.item_effect.is_none() {
                check.fail(&location, "loot must be a hero item, not a currency");
            }
        }
        let mut seen = BTreeSet::new();
        if !pool
            .entries
            .iter()
            .all(|e| seen.insert(e.item.as_ref().map(|i| i.as_str())))
        {
            check.fail(&location, "duplicate loot entry");
        }
    }
}

fn definition_rules(check: &mut Checker, d: &Definition) {
    let content = check.content;
    let at = d.id.as_str();
    let rules = &content.rules;
    let b = &d.behaviors;

    if let Some(garrison) = &d.garrison {
        if d.kind != Kind::Building || d.footprint.is_none() || d.entrance.is_none() {
            check.fail(at, "garrison requires a building footprint and entrance");
        }
        for id in &garrison.accepts {
            if let Some(unit) = check.definition(at, id.as_str(), Kind::Unit) {
                let ranged = unit
                    .behaviors
                    .combat
                    .as_ref()
                    .is_some_and(|c| c.projectile.is_some() || c.shell.is_some());
                if unit.behaviors.movement.is_none() || !ranged {
                    check.fail(at, "garrison accepts mobile projectile units");
                }
            }
        }
    }
    if let Some(requires) = &d.requires {
        check.unique(at, requires.iter().map(|r| r.as_str()), "prerequisite");
        for id in requires {
            if id == &d.id
                || content
                    .definition(id.as_str())
                    .is_none_or(|r| r.kind != Kind::Building)
            {
                check.fail(at, "prerequisites must reference another building");
            }
        }
    }
    if let Some(research) = &b.research {
        if d.kind != Kind::Building {
            check.fail(at, "research requires building");
        }
        check.unique(
            at,
            research.outputs.iter().map(|o| o.as_str()),
            "research output",
        );
        for id in &research.outputs {
            if !rules.research.contains_key(id.as_str()) {
                check.fail(at, format!("unknown research {id}"));
            }
        }
    }
    if let Some(upgrade) = &d.upgrade {
        match content.definition(upgrade.target.as_str()) {
            None => check.fail(at, format!("unknown upgrade target {}", upgrade.target)),
            Some(target) => {
                let compatible = d.kind == Kind::Building
                    && target.kind == Kind::Building
                    && target.id != d.id
                    && d.footprint == target.footprint
                    && d.entrance == target.entrance
                    && d.behaviors == target.behaviors
                    && target.body.as_ref().map(|b| b.max_hp) >= d.body.as_ref().map(|b| b.max_hp);
                if !compatible {
                    check.fail(
                        at,
                        "upgrade requires a different building with compatible footprint, behaviors and health capacity",
                    );
                }
            }
        }
        check.unique(
            at,
            upgrade.items.iter().map(|c| c.item.as_str()),
            "upgrade input",
        );
        for cost in &upgrade.items {
            check.definition(at, cost.item.as_str(), Kind::Item);
        }
    }
    if d.item_tier.is_some_and(|t| t != 0) && (d.item_effect.is_none() || d.kind != Kind::Item) {
        check.fail(at, "item tier requires a hero item");
    }
    if let Some(effect) = &d.item_effect {
        let hits = [
            effect.active().and_then(|a| a.damage_type.as_deref()),
            effect.on_hit().map(|h| h.damage_type.as_str()),
        ];
        for damage_type in hits.into_iter().flatten() {
            if !damage_type.is_empty() && !rules.damage_types.contains_key(damage_type) {
                check.fail(at, "unknown item damage type");
            }
        }
        if let Some(active) = effect.active() {
            use crate::items::ActiveTarget;
            if active.damage.is_some_and(|v| v != 0)
                && active.damage_type.as_deref().is_none_or(str::is_empty)
            {
                check.fail(at, "item damage requires damageType");
            }
            if active.target == ActiveTarget::Caster && active.radius != 0 {
                check.fail(at, "self item radius must be zero");
            }
            if active.target != ActiveTarget::Caster && active.radius == 0 {
                check.fail(at, "area item requires radius");
            }
        }
        if let crate::items::ItemEffect::Consumable {
            on_pickup: Some(true),
            permanent: None,
            ..
        } = effect
        {
            check.fail(at, "pickup consumables require permanent bonuses");
        }
    }
    if d.unit_nature.is_some() && d.kind != Kind::Unit {
        check.fail(at, "unit nature requires a unit");
    }
    if d.id.prefix() != d.kind.name() {
        check.fail(at, "ID prefix must match kind");
    }
    let model = check.model(at, d.asset.as_str(), "asset");
    if let Some(visuals) = &d.resource_visuals {
        if d.resource_yield.is_none_or(|y| y == 0) {
            check.fail(at, "resource visuals require a yield");
        }
        if !visuals.variants.contains(&d.asset) {
            check.fail(at, "default asset must be a full resource variant");
        }
        for id in visuals
            .variants
            .iter()
            .chain(visuals.stages.iter().map(|s| &s.asset))
        {
            check.model(at, id.as_str(), "resource visual");
        }
        let thresholds: Vec<f64> = visuals.stages.iter().map(|s| s.remaining).collect();
        if thresholds.first() != Some(&0.0) || thresholds.windows(2).any(|w| w[1] <= w[0]) {
            check.fail(
                at,
                "resource stages must start empty and have increasing remaining fractions",
            );
        }
    }
    check.image(at, d.icon.as_str(), "icon");
    let asset = content.asset(d.asset.as_str());
    let scenery = asset.is_some_and(|a| a.scenery_asset.is_some());
    if model && d.kind != Kind::Resource && scenery {
        check.fail(at, "batched scenery models require a resource definition");
    }
    if matches!(d.kind, Kind::Unit | Kind::Building) && d.body.is_none() {
        check.fail(at, "body required");
    }
    if let Some(body) = &d.body {
        if rules
            .damage_multipliers
            .values()
            .any(|row| !row.contains_key(body.armor_type.as_str()))
        {
            check.fail(at, "armor type must be covered by each damage row");
        }
        if !rules.armor_types.contains_key(body.armor_type.as_str()) {
            check.fail(at, "armor type needs presentation metadata");
        }
    }
    if let (Kind::Building, Some(footprint)) = (d.kind, &d.footprint)
        && (footprint.width % BUILDING_CELL_SIZE != 0 || footprint.depth % BUILDING_CELL_SIZE != 0)
    {
        check.fail(
                at,
                "building footprint dimensions must be whole building cells (multiples of 4 navigation cells)",
            );
    }
    if let Some(combat) = &b.combat {
        if combat.projectile.is_some() && combat.shell.is_some() {
            check.fail(at, "weapon cannot launch both a missile and an area shell");
        }
        if d.kind != Kind::Unit {
            check.fail(at, "combat currently requires a unit");
        }
        if !rules
            .damage_multipliers
            .contains_key(combat.damage_type.as_str())
        {
            check.fail(at, "unknown damage type");
        }
    }
    if model && d.kind == Kind::Resource && !scenery {
        check.fail(at, "resource model needs sceneryAsset");
    }
    if d.felling.is_some()
        && (d.kind != Kind::Resource
            || d.resource_yield.is_none_or(|y| y == 0)
            || asset.is_none_or(|a| a.harvest_animation.is_none()))
    {
        check.fail(
            at,
            "felling requires a resource yield and animated scenery asset",
        );
    }
    if d.hero_capacity.is_some_and(|c| c != 0) && d.kind != Kind::Building {
        check.fail(at, "hero capacity requires a building");
    }
    if d.kind != Kind::Unit && d.is_hero() {
        check.fail(at, "hero flag requires a unit");
    }
    if let Some(revival) = &b.revival {
        if d.kind != Kind::Building {
            check.fail(at, "revival requires a building");
        }
        for bill in [&revival.items, &revival.items_per_level] {
            check.unique(
                at,
                bill.iter().map(|c| c.item.as_str()),
                "revival price item",
            );
            for cost in bill {
                if let Some(item) = check.definition(at, cost.item.as_str(), Kind::Item)
                    && item.currency != Some(true)
                {
                    check.fail(at, "revival prices require currencies");
                }
            }
        }
    }
    if b.inventory.is_some() && (!d.is_hero() || b.movement.is_none()) {
        check.fail(at, "inventory requires a mobile hero");
    }
    if d.item_effect.is_some() && (d.kind != Kind::Item || d.stack_limit != Some(1)) {
        check.fail(at, "hero items require item kind and stackLimit 1");
    }
    if let Some(progression) = &b.progression {
        let levels = &progression.levels;
        let increasing = levels.windows(2).all(|w| w[1].experience > w[0].experience);
        if !d.is_hero() || levels.first().is_none_or(|l| l.experience != 0) || !increasing {
            check.fail(
                at,
                "progression requires a hero and strictly increasing XP thresholds starting at zero",
            );
        }
        if let Some(first) = levels.first() {
            let caster = b.abilities.as_ref();
            let matches = Some(first.max_hp) == d.body.as_ref().map(|b| b.max_hp)
                && Some(first.armor) == d.body.as_ref().map(|b| b.armor)
                && first.damage == b.combat.as_ref().map_or(0, |c| c.damage)
                && first.cooldown_ticks == b.combat.as_ref().map_or(1, |c| c.cooldown_ticks)
                && first.max_mana == caster.map_or(0, |c| c.max_mana)
                && first.mana_regen_per_second == caster.map_or(0.0, |c| c.mana_regen_per_second);
            if !matches {
                check.fail(
                    at,
                    "level-one stats must match body, combat and ability declarations",
                );
            }
        }
        let shrinks = levels
            .windows(2)
            .any(|w| w[1].max_hp < w[0].max_hp || w[1].max_mana < w[0].max_mana);
        let mana_without_abilities = b.abilities.is_none()
            && levels
                .iter()
                .any(|l| l.max_mana > 0 || l.mana_regen_per_second > 0.0);
        if shrinks || mana_without_abilities {
            check.fail(
                at,
                "progression pools must not shrink; mana requires abilities",
            );
        }
        if d.level.is_some_and(|l| l != 1) {
            check.fail(at, "progression starts at level 1");
        }
    }
    if d.kind == Kind::Building && (d.footprint.is_none() || d.entrance.is_none()) {
        check.fail(at, "footprint/entrance required");
    }
    if let (Some(footprint), Some(entrance)) = (&d.footprint, &d.entrance) {
        let on_cell =
            |size: u32, offset: f64| (f64::from(size) / 2.0 - 0.5 + offset).fract() == 0.0;
        if d.kind == Kind::Building
            && (!on_cell(footprint.width, entrance.x) || !on_cell(footprint.depth, entrance.y))
        {
            check.fail(at, "entrance must land on a navigation cell center");
        }
        if entrance.x.abs() <= f64::from(footprint.width) / 2.0
            && entrance.y.abs() <= f64::from(footprint.depth) / 2.0
        {
            check.fail(at, "entrance must be outside footprint");
        }
    }
    if d.kind == Kind::Item && d.stack_limit.is_none_or(|s| s == 0) {
        check.fail(at, "stackLimit required");
    }
    if d.kind == Kind::Resource && d.resource_yield.is_none_or(|y| y == 0) {
        check.fail(at, "yield required");
    }
    if d.resource_yield.is_some_and(|y| y != 0)
        && !matches!(d.kind, Kind::Resource | Kind::Building)
    {
        check.fail(at, "yield requires a resource or mine building");
    }
    if d.harvesting.is_some()
        && (d.resource_yield.is_none_or(|y| y == 0)
            || !matches!(d.kind, Kind::Building | Kind::Resource))
    {
        check.fail(at, "harvesting requires a resource reserve");
    }
    if d.currency == Some(true) && (d.kind != Kind::Item || d.item_effect.is_some()) {
        check.fail(at, "currency must be a non-equipment item");
    }
    if b.work.is_some() && (b.movement.is_none() || d.kind != Kind::Unit) {
        check.fail(at, "work requires mobile unit");
    }
    if b.camp_defense.is_some() && (b.movement.is_none() || b.combat.is_none()) {
        check.fail(at, "camp defense requires movement and combat");
    }
    for id in b.work.iter().flat_map(|w| &w.builds) {
        if let Some(building) = check.definition(at, id.as_str(), Kind::Building)
            && !matches!(building.creation, Some(Creation::Construct { .. }))
        {
            check.fail(at, format!("unconstructable {id}"));
        }
    }
    for id in b.storage.iter().flat_map(|s| &s.accepts) {
        check.definition(at, id.as_str(), Kind::Item);
    }
    if let Some(storage) = &b.storage {
        if let Some(gathering) = &storage.gathering {
            let item = check.definition(at, gathering.item.as_str(), Kind::Item);
            let staffed_source = match item.and_then(|i| i.creation.as_ref()) {
                Some(Creation::Harvest { source, .. }) => content
                    .definition(source.as_str())
                    .is_some_and(|s| s.kind == Kind::Building && s.harvesting.is_some()),
                _ => false,
            };
            if storage.dropoff != Some(true)
                || !storage.accepts.contains(&gathering.item)
                || !staffed_source
            {
                check.fail(
                    at,
                    "base gathering requires a compatible drop-off and a staffed harvest source",
                );
            }
        }
        if storage.dropoff == Some(true) && d.kind != Kind::Building {
            check.fail(at, "drop-off requires building");
        }
    }
    for id in b.work.iter().flat_map(|w| w.harvests.iter().flatten()) {
        let Some(item) = check.definition(at, id.as_str(), Kind::Item) else {
            continue;
        };
        match &item.creation {
            Some(Creation::Harvest { amount, .. }) => {
                if b.work.as_ref().is_some_and(|w| w.carry_capacity < *amount) {
                    check.fail(at, "worker must be able to carry one harvest load");
                }
            }
            _ => check.fail(at, format!("worker harvest {id} requires a harvest recipe")),
        }
    }
    if d.construction_clearance.is_some() && d.resource_yield.is_none_or(|y| y == 0) {
        check.fail(at, "construction clearance requires resource yield");
    }
    if let Some(near) = &d.placement_near {
        let finite_source = content
            .definition(near.source.as_str())
            .is_some_and(|s| s.resource_yield.is_some_and(|y| y != 0));
        if d.kind != Kind::Building
            || !matches!(d.creation, Some(Creation::Construct { .. }))
            || !finite_source
        {
            check.fail(
                at,
                "placement-near requires a constructable building and a finite resource source",
            );
        }
    }
    if (d.kind == Kind::Unit) != d.supply_cost.is_some() {
        check.fail(
            at,
            "every unit must declare supplyCost; other kinds cannot consume supply",
        );
    }
    if d.supply_provided.is_some() && d.kind != Kind::Building {
        check.fail(at, "only buildings provide supply");
    }
    if let Some(creation) = &d.creation {
        creation_rules(check, d, creation);
    }
    if let Some(production) = &b.production {
        production_rules(check, d, production);
    }
}

fn creation_rules(check: &mut Checker, d: &Definition, creation: &Creation) {
    let content = check.content;
    let at = d.id.as_str();
    check.unique(
        at,
        creation.items().iter().map(|c| c.item.as_str()),
        "creation input",
    );
    for cost in creation.items() {
        check.definition(at, cost.item.as_str(), Kind::Item);
    }
    match creation {
        Creation::Construct { .. } if d.kind != Kind::Building => {
            check.fail(at, "construct target must be building")
        }
        Creation::Train { .. } if d.kind != Kind::Unit => {
            check.fail(at, "train target must be unit")
        }
        Creation::Harvest {
            source,
            impact_tick,
            animation_ticks,
            work_ticks,
            work_animation,
            ..
        } => {
            if d.kind != Kind::Item {
                check.fail(at, "harvest targets items");
            }
            match content.definition(source.as_str()) {
                None => check.fail(at, format!("unknown harvest source {source}")),
                Some(source) => {
                    if source.resource_yield.is_none_or(|y| y == 0) {
                        check.fail(at, "harvest source requires yield");
                    }
                    let cycle = animation_ticks.unwrap_or(*work_ticks);
                    if source.felling.is_some() != impact_tick.is_some()
                        || impact_tick.is_some_and(|t| t >= cycle)
                    {
                        check.fail(
                            at,
                            "felling harvest requires an impact tick inside the work cycle",
                        );
                    }
                    if source.harvesting.is_none() {
                        check.fail(at, "harvest source requires an extraction policy");
                    }
                }
            }
            if animation_ticks.is_some() && work_animation.is_none() {
                check.fail(at, "harvest animation timing requires a work animation");
            }
        }
        Creation::Plant { .. }
            if d.kind != Kind::Resource || d.regrowth_ticks.is_none_or(|t| t == 0) =>
        {
            check.fail(at, "plant needs resource and regrowthTicks");
        }
        _ => {}
    }
}

fn production_rules(check: &mut Checker, d: &Definition, production: &crate::game::Production) {
    let content = check.content;
    let at = d.id.as_str();
    if d.kind != Kind::Building {
        check.fail(at, "production requires building");
    }
    if let Some(revival) = &d.behaviors.revival
        && (production.mode != ProductionMode::Queued
            || production.queue_capacity != Some(revival.queue_capacity))
    {
        check.fail(
            at,
            "recruitment and revival share one queued workplace and capacity",
        );
    }
    if production.mode == ProductionMode::Automatic && production.outputs.len() != 1 {
        check.fail(at, "automatic production has one output");
    }
    if production.mode == ProductionMode::Queued && production.queue_capacity.is_none_or(|c| c == 0)
    {
        check.fail(at, "queued production needs queueCapacity");
    }
    check.unique(at, production.outputs.iter().map(|o| o.as_str()), "output");
    for id in &production.outputs {
        let Some(target) = content.definition(id.as_str()) else {
            check.fail(at, format!("unknown output {id}"));
            continue;
        };
        let Some(creation) = target
            .creation
            .as_ref()
            .filter(|c| !matches!(c, Creation::Construct { .. }))
        else {
            check.fail(at, format!("unsupported output {id}"));
            continue;
        };
        let trains = matches!(creation, Creation::Train { .. });
        if trains != (production.mode == ProductionMode::Queued) {
            check.fail(
                at,
                format!("{} has incompatible production mode", creation.method()),
            );
        }
        let external = matches!(creation, Creation::Harvest { .. } | Creation::Plant { .. });
        if production.worker_slots != u32::from(external) {
            check.fail(at, format!("{} has invalid staffing", creation.method()));
        }
        if external && production.work_radius.is_none_or(|r| r == 0) {
            check.fail(at, "external work requires workRadius");
        }
        let storage = d.behaviors.storage.as_ref();
        for input in creation.items() {
            if !storage.is_some_and(|s| s.accepts.contains(&input.item)) {
                check.fail(at, format!("storage must accept {} for {id}", input.item));
            }
        }
        let required = creation
            .items()
            .iter()
            .map(|c| c.amount)
            .sum::<u32>()
            .max(u32::from(target.kind == Kind::Item));
        if required > storage.map_or(0, |s| s.capacity) {
            check.fail(at, format!("insufficient storage for {id}"));
        }
    }
}

fn actions(check: &mut Checker) {
    let content = check.content;
    let actions = &content.actions;
    let declared: BTreeSet<&str> = actions.actions.keys().map(String::as_str).collect();
    let expected: BTreeSet<&str> = crate::game::ACTION_NAMES.into_iter().collect();
    if declared != expected {
        check.fail("actions.actions", "must declare exactly the known commands");
    }
    let category_exists = |check: &mut Checker, location: &str, id: Option<&crate::Id>| {
        if let Some(id) = id
            && !actions.categories.contains_key(id)
        {
            check.fail(location, format!("unknown command category {id}"));
        }
    };
    for (id, category) in &actions.categories {
        let location = format!("actions.categories.{id}");
        category_exists(check, &location, category.parent.as_ref());
        let mut visited = BTreeSet::from([id]);
        let mut parent = category.parent.as_ref();
        while let Some(next) = parent {
            if !visited.insert(next) {
                check.fail(&location, "command category cycle");
                break;
            }
            parent = actions.categories.get(next).and_then(|c| c.parent.as_ref());
        }
    }
    check.image(
        "rules.supplyIcon",
        content.rules.supply_icon.as_str(),
        "supply icon",
    );
    for d in &content.definitions {
        category_exists(check, d.id.as_str(), d.category.as_ref());
    }
    let mut hotkeys = BTreeSet::new();
    let mut hotkey = |check: &mut Checker, location: &str, key: Option<&String>| {
        if let Some(key) = key {
            let valid =
                key == "Escape" || (key.len() == 1 && key.bytes().all(|b| b.is_ascii_uppercase()));
            if !valid {
                check.fail(location, format!("invalid hotkey {key}"));
            }
            if !hotkeys.insert(key.clone()) {
                check.fail(location, format!("duplicate hotkey {key}"));
            }
        }
    };
    for (name, action) in &actions.actions {
        let location = format!("actions.actions.{name}");
        category_exists(check, &location, action.category.as_ref());
        check.image(&location, action.icon.as_str(), "command icon");
        hotkey(check, &location, action.hotkey.as_ref());
    }
    for (name, category) in &actions.categories {
        let location = format!("actions.categories.{name}");
        check.image(&location, category.icon.as_str(), "command icon");
        hotkey(check, &location, category.hotkey.as_ref());
    }
    let navigation = [
        Some(&actions.navigation.back),
        actions.navigation.camera.as_ref(),
    ];
    for meta in navigation.into_iter().flatten() {
        check.image("actions.navigation", meta.icon.as_str(), "command icon");
        hotkey(check, "actions.navigation", meta.hotkey.as_ref());
    }
    for (binding, entry) in &actions.overrides {
        let location = format!("actions.overrides.{binding}");
        category_exists(
            check,
            &location,
            entry.category.as_ref().and_then(Option::as_ref),
        );
        let valid = binding.split_once(':').is_some_and(|(name, target)| {
            matches!(name, "build" | "produce") && content.definition(target).is_some()
        });
        if !valid {
            check.fail(&location, "invalid action binding");
        }
        hotkey(check, &location, entry.hotkey.as_ref());
    }
}

fn races(check: &mut Checker) {
    let content = check.content;
    let rules = &content.rules;
    if !rules.races.is_empty() && !rules.races.contains_key(&rules.default_race) {
        check.fail("rules.defaultRace", "unknown default race");
    }
    for (id, campaign) in &rules.campaigns {
        if !rules.races.contains_key(&campaign.race) {
            check.fail(
                format!("rules.campaigns.{id}"),
                format!("unknown campaign race {}", campaign.race),
            );
        }
    }
    let profiles: Vec<_> = rules.races.keys().filter_map(|id| rules.race(id)).collect();
    let foreign = |race: &str, id: &str| {
        profiles
            .iter()
            .any(|p| p.id != race && p.race.roster.iter().any(|r| r == id))
            && !profiles
                .iter()
                .any(|p| p.id == race && p.race.roster.iter().any(|r| r == id))
    };
    for profile in &profiles {
        let location = format!("rules.races.{}", profile.id);
        let race = profile.race;
        if let Some(construction) = &race.construction {
            check.model(
                &location,
                construction.scaffold.as_str(),
                "construction scaffold",
            );
        }
        check.unique(
            &location,
            race.roster.iter().map(|r| r.as_str()),
            "roster entry",
        );
        for id in &race.roster {
            let Some(d) = content.definition(id.as_str()) else {
                check.fail(&location, format!("unknown roster member {id}"));
                continue;
            };
            if !matches!(d.kind, Kind::Unit | Kind::Building) {
                check.fail(&location, format!("invalid roster member {id}"));
            }
            let b = &d.behaviors;
            let references = b
                .work
                .iter()
                .flat_map(|w| &w.builds)
                .chain(b.production.iter().flat_map(|p| &p.outputs))
                .chain(d.garrison.iter().flat_map(|g| &g.accepts))
                .chain(d.requires.iter().flatten())
                .chain(d.upgrade.iter().map(|u| &u.target));
            for reference in references {
                if foreign(profile.id, reference.as_str()) {
                    check.fail(
                        &location,
                        format!("{id} refers outside its roster: {reference}"),
                    );
                }
            }
            for research in b.research.iter().flat_map(|r| &r.outputs) {
                for unit in rules
                    .research
                    .get(research)
                    .iter()
                    .flat_map(|r| &r.effects)
                    .flat_map(|e| &e.units)
                {
                    if foreign(profile.id, unit.as_str()) {
                        check.fail(&location, format!("foreign research target {unit}"));
                    }
                }
            }
        }
        let setup = profile.starting_setup;
        let setup_ids = std::iter::once(&setup.fort)
            .chain(setup.units.iter().map(|u| &u.definition))
            .chain(setup.hero.iter().flat_map(|h| &h.choices));
        for id in setup_ids {
            if foreign(profile.id, id.as_str()) {
                check.fail(&location, format!("setup references foreign unit {id}"));
            }
        }
    }
    let defaults = [("default", &rules.starting_setup)];
    let setups: Vec<(&str, &crate::game::StartingSetup)> = if profiles.is_empty() {
        defaults.into_iter().collect()
    } else {
        profiles.iter().map(|p| (p.id, p.starting_setup)).collect()
    };
    for (race, setup) in setups {
        let location = format!("rules.races.{race}");
        let Some(fort) = check.definition(&location, setup.fort.as_str(), Kind::Building) else {
            continue;
        };
        let storage = fort.behaviors.storage.clone();
        for (item, amount) in &setup.inventory {
            check.definition(&location, item.as_str(), Kind::Item);
            if *amount != 0 && !storage.as_ref().is_some_and(|s| s.accepts.contains(item)) {
                check.fail(&location, format!("starting store cannot accept {item}"));
            }
        }
        if setup.inventory.values().sum::<u32>() > storage.as_ref().map_or(0, |s| s.capacity) {
            check.fail(&location, "starting inventory exceeds fort capacity");
        }
        for unit in &setup.units {
            check.definition(&location, unit.definition.as_str(), Kind::Unit);
        }
        if let Some(hero) = &setup.hero {
            let mut seen = BTreeSet::new();
            if !hero.choices.iter().all(|c| seen.insert(c)) || !hero.choices.contains(&hero.default)
            {
                check.fail(
                    &location,
                    "starting hero choices must be unique and include the default",
                );
            }
            for id in &hero.choices {
                if let Some(d) = check.definition(&location, id.as_str(), Kind::Unit)
                    && (!d.is_hero()
                        || d.behaviors.player_control.is_none()
                        || d.supply_cost != Some(4))
                {
                    check.fail(
                        &location,
                        format!(
                            "{id}: starting heroes must be controllable heroes using four supply"
                        ),
                    );
                }
            }
            if setup.units.iter().any(|u| {
                content
                    .definition(u.definition.as_str())
                    .is_some_and(Definition::is_hero)
            }) {
                check.fail(
                    &location,
                    "starting heroes belong in the hero choice, not fixed units",
                );
            }
        }
        for task in setup.gathering.iter().flatten() {
            if let Some(item) = check.definition(&location, task.item.as_str(), Kind::Item)
                && !matches!(item.creation, Some(Creation::Harvest { .. }))
            {
                check.fail(&location, "starting gather requires harvest recipe");
            }
        }
    }
}
