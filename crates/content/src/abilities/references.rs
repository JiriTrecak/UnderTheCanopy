//! Ability rules that need the rest of the content: icons, caster bindings, damage types,
//! summoned and split definitions, form assets and the AI's skill preferences.

use super::{AbilityBinding, AbilityCaster, AiIntent, Anchor, Control, Effect, TargetingKind};
use crate::{
    Content,
    conditions::has_duplicates,
    error::Error,
    game::{Definition, Kind},
};
use std::collections::BTreeSet;

pub(crate) fn check(content: &Content, errors: &mut Vec<Error>) {
    icons(content, errors);
    for definition in &content.definitions {
        if let Some(caster) = &definition.behaviors.abilities {
            for problem in caster_problems(content, definition, caster) {
                errors.push(Error::new(
                    content.file_of(definition.id.as_str()),
                    definition.id.as_str(),
                    problem,
                ));
            }
        }
    }
    effects(content, errors);
}

fn icons(content: &Content, errors: &mut Vec<Error>) {
    for ability in &content.abilities.abilities {
        if let Some(icon) = &ability.presentation.icon
            && !content
                .asset(icon.as_str())
                .is_some_and(|a| a.image.is_some())
        {
            errors.push(Error::new(
                content.file_of(ability.id.as_str()),
                ability.id.as_str(),
                format!("Spell icon must reference an image: {icon}"),
            ));
        }
    }
}

fn caster_problems(content: &Content, d: &Definition, caster: &AbilityCaster) -> Vec<String> {
    let mut problems: Vec<String> = Vec::new();
    let mut fail = |message: &str| problems.push(message.to_string());
    if caster.max_mana == 0 {
        fail("Ability casters need a positive mana pool");
    }
    let regen = caster.mana_regen_per_second * 1000.0;
    if caster.mana_regen_per_second < 0.0 || (regen - regen.round()).abs() > 1e-6 {
        fail("Mana regeneration must be a non-negative multiple of 0.001");
    }
    let ids: Vec<&str> = caster.bindings.iter().map(|b| b.id.as_str()).collect();
    if ids.is_empty() || has_duplicates(&ids) {
        fail("Binding IDs must be unique");
    }
    if d.kind != Kind::Unit {
        fail("abilities require a unit");
    }
    let mut columns = BTreeSet::new();
    for binding in &caster.bindings {
        for problem in binding_problems(content, d, binding, &mut columns) {
            problems.push(format!("{}: {problem}", binding.id));
        }
    }
    problems
}

fn binding_problems(
    content: &Content,
    d: &Definition,
    binding: &AbilityBinding,
    columns: &mut BTreeSet<u32>,
) -> Vec<String> {
    let mut problems: Vec<String> = Vec::new();
    let mut fail = |message: &str| problems.push(message.to_string());
    if binding.controls.is_empty() || has_duplicates(&binding.controls) {
        fail("Controls must be distinct");
    }
    let Some(ability) = content.abilities.ability(binding.ability.as_str()) else {
        problems.push(format!("Unknown ability {}", binding.ability));
        return problems;
    };
    let ranks = ability.ranks.len();
    if binding.initial_rank as usize > ranks {
        fail("Invalid initial ability rank");
    }
    let progression = d.behaviors.progression.as_ref();
    if let Some(learning) = &binding.learning {
        let levels = &learning.required_levels;
        if progression.is_none() || levels.len() != ranks {
            fail("Ability learning requires progression and a requirement per rank");
        }
        if let Some(progression) = progression {
            let fits = levels
                .iter()
                .all(|&l| l >= 1 && l as usize <= progression.levels.len());
            if !fits || levels.windows(2).any(|w| w[1] <= w[0]) {
                fail("Ability learning levels must increase and fit the hero progression");
            }
        }
    }
    if let Some(command) = &binding.command {
        if !binding.controls.contains(&Control::Player) {
            fail("A command requires player control");
        }
        if !(1..=4).contains(&command.column) {
            fail("Command column must be 1–4");
        }
        if !columns.insert(command.column) {
            fail("Duplicate ability column");
        }
        if let Some(icon) = &command.icon
            && content.asset(icon.as_str()).is_none()
        {
            problems.push(format!("Unknown asset: {icon}"));
        }
    }
    if let Some(ai) = &binding.ai {
        if !binding.controls.contains(&Control::Ai) {
            problems.push("AI policy requires AI control".into());
        }
        if ai.interval_ticks == 0 {
            problems.push("AI interval must be positive".into());
        }
        let effects: Vec<&Effect> = ability
            .all_effects()
            .into_iter()
            .filter_map(|e| e.effect())
            .collect();
        let kind = ability.targeting.kind;
        let intent = match ai.intent {
            Some(AiIntent::Scout) => Some((
                kind == TargetingKind::Point
                    && effects.iter().any(|e| matches!(e, Effect::Vision(_))),
                "Scout AI requires point vision",
            )),
            Some(AiIntent::Escape) => Some((
                kind == TargetingKind::Point
                    && effects
                        .iter()
                        .any(|e| matches!(e, Effect::Teleport(_)) && e.target() == Anchor::Caster),
                "Escape AI requires caster displacement to a point",
            )),
            Some(AiIntent::Reinforce) => Some((
                kind == TargetingKind::Unit
                    && effects
                        .iter()
                        .any(|e| matches!(e, Effect::Teleport(_)) && e.query().is_some()),
                "Reinforcement AI requires a unit-anchored group displacement",
            )),
            Some(AiIntent::Utility) | None => None,
        };
        if let Some((false, message)) = intent {
            problems.push(message.into());
        }
    }
    problems
}

fn effects(content: &Content, errors: &mut Vec<Error>) {
    let rules = &content.rules;
    for ability in &content.abilities.abilities {
        let mut fail = |message: String| {
            errors.push(Error::new(
                content.file_of(ability.id.as_str()),
                ability.id.as_str(),
                message,
            ));
        };
        let mut damage_types: Vec<&str> = Vec::new();
        let mut summons = Vec::new();
        let mut members = Vec::new();
        for entry in ability.all_effects() {
            if let Some(status) = entry.status() {
                damage_types.extend(status.periodic.iter().map(|p| p.damage_type.as_str()));
                damage_types.extend(
                    status
                        .damage_taken_permille
                        .iter()
                        .flatten()
                        .map(|(k, _)| k.as_str()),
                );
                if let Some(form) = &status.form {
                    if let Some(profile) = &form.combat_profile
                        && !content
                            .definition(profile.as_str())
                            .is_some_and(|d| d.kind == Kind::Unit && d.behaviors.combat.is_some())
                    {
                        fail(format!(
                            "combat profile requires an armed unit definition ({profile})"
                        ));
                    }
                    if let Some(asset) = &form.asset
                        && !content
                            .asset(asset.as_str())
                            .is_some_and(|a| a.file.is_some())
                    {
                        fail(format!("form asset must be a model ({asset})"));
                    }
                }
            }
            match entry.effect() {
                Some(Effect::Damage(e)) => damage_types.push(&e.damage_type),
                Some(Effect::Dispel(e)) => damage_types.push(&e.damage_type),
                Some(Effect::Drain(e)) => damage_types.push(&e.damage_type),
                Some(Effect::Contain(e)) => {
                    damage_types.extend(e.digestion.iter().map(|d| d.damage_type.as_str()));
                }
                Some(Effect::Summon(e)) => summons.extend(e.definition.ids()),
                Some(Effect::Split(e)) => members.extend(e.members.iter().flat_map(|m| m.ids())),
                _ => {}
            }
        }
        for damage_type in damage_types {
            if !rules.damage_types.contains_key(damage_type) {
                fail(format!("unknown damage type {damage_type}"));
            }
        }
        for id in members {
            let valid = content.definition(id.as_str()).is_some_and(|d| {
                d.kind == Kind::Unit
                    && !d.is_hero()
                    && d.supply_cost == Some(0)
                    && d.behaviors.player_control.is_some()
            });
            if !valid {
                fail(format!(
                    "split members require controllable, zero-supply non-hero units ({id})"
                ));
            }
        }
        for id in summons {
            if !content
                .definition(id.as_str())
                .is_some_and(|d| d.kind == Kind::Unit)
            {
                fail(format!("summons require a unit definition ({id})"));
            }
        }
    }
}
