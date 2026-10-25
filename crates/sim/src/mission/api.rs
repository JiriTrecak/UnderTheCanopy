//! The `mission` table: 44 functions over a draft of the mission state (the computer
//! players' five in [`ai`], the campaign's four in [`campaign`]).
//!
//! Calls read the game state as it was when the run began and write only to the draft and
//! the effect queue, so a failed run leaves no trace. Item exchanges read their own writes
//! within a run.

use super::{
    Script, alive,
    effect::MissionEffect,
    entity_by_placement,
    sandbox::Scalar,
    state::{
        Camera, CameraMode, Dialogue, Facing, MAX_FACING, MAX_VARIABLE_TEXT, MAX_VARIABLES,
        MissionState, ObjectiveState, Scene, Variable,
    },
    systems::Systems,
};
use crate::{
    catalog::Catalog,
    entity::{Entity, EntityId, Milli},
    state::State,
};
use canopy_content::{
    Id,
    game::{Definition, Kind},
    map::Objective,
    placement::{Activation, Owner, Placement},
};
use std::collections::BTreeMap;

mod campaign;
mod scene;
pub use scene::NAMES as SCENE_NAMES;

/// API calls per run.
pub const CALL_LIMIT: u32 = 256;
/// Scripted damage events per run.
pub const MAX_DAMAGE: usize = 64;
/// What a variable refuses to hold.
const VARIABLE_VALUE: &str = "A variable holds a string, a number or a boolean";
/// Camera transition when a shot gives none.
const DEFAULT_TRANSITION_MS: u32 = 350;

pub const NAMES: [&str; 45] = [
    "has_item",
    "give_item",
    "take_item",
    "dialogue_busy",
    "tick",
    "count",
    "stock",
    "give_stock",
    "alive",
    "position",
    "in_region",
    "arrived",
    "facing_done",
    "get",
    "set",
    "damage",
    "recover",
    "spawn",
    "move",
    "attack_move",
    "follow",
    "attack",
    "transfer",
    "transform",
    "begin_scene",
    "end_scene",
    "camera",
    "face",
    "say",
    "begin_objective",
    "complete_objective",
    "fail_objective",
    "objective",
    "win",
    "lose",
    "ai_waves",
    "ai_attack_now",
    "ai_target",
    "ai_behaviour",
    "ai_wake",
    // The campaign (`campaign`).
    "campaign_get",
    "campaign_set",
    "allow",
    "forbid",
    "difficulty",
];

mod ai;

type Reply = Result<Vec<Scalar>, String>;

pub(super) struct Api<'a> {
    state: &'a State,
    catalog: &'a Catalog,
    script: &'a Script,
    systems: &'a dyn Systems,
    draft: MissionState,
    effects: Vec<MissionEffect>,
    /// Hero item slots as this run has changed them.
    equipment: BTreeMap<EntityId, Vec<Option<Id>>>,
    calls: u32,
    /// While the scene plays: the index of the next statement it reaches.
    cursor: Option<u32>,
}

impl<'a> Api<'a> {
    pub(super) fn new(
        state: &'a State,
        catalog: &'a Catalog,
        script: &'a Script,
        systems: &'a dyn Systems,
        draft: MissionState,
    ) -> Self {
        Self {
            state,
            catalog,
            script,
            systems,
            draft,
            effects: Vec::new(),
            equipment: BTreeMap::new(),
            calls: 0,
            cursor: None,
        }
    }

    /// The draft and effects of a run whose script finished, once the draft is valid.
    pub(super) fn finish(mut self) -> Result<(MissionState, Vec<MissionEffect>), String> {
        self.draft.started = true;
        if self.draft.variables.len() > MAX_VARIABLES {
            return Err(format!("Mission state exceeds {MAX_VARIABLES} variables"));
        }
        for (key, value) in &self.draft.variables {
            let valid = match value {
                Variable::String(text) => text.encode_utf16().count() <= MAX_VARIABLE_TEXT,
                Variable::Number(number) => number.is_finite(),
                Variable::Integer(_) | Variable::Boolean(_) => true,
            };
            if !valid {
                return Err(format!(
                    "Mission variable {key} must be a finite number, a boolean or text of at most {MAX_VARIABLE_TEXT} characters"
                ));
            }
        }
        if self.draft.facing.len() > MAX_FACING {
            return Err(format!(
                "Mission turns more than {MAX_FACING} units at once"
            ));
        }
        let damage = self
            .effects
            .iter()
            .filter(|e| matches!(e, MissionEffect::Damage { .. }))
            .count();
        if damage > MAX_DAMAGE {
            return Err(format!(
                "Mission queued more than {MAX_DAMAGE} damage events"
            ));
        }
        Ok((self.draft, self.effects))
    }

    pub(super) fn call(&mut self, name: &str, args: Vec<Scalar>) -> Reply {
        if self.cursor.is_some() && !scene::QUERIES.contains(&name) {
            return self.statement(name, args);
        }
        self.dispatch(name, args)
    }

    fn dispatch(&mut self, name: &str, args: Vec<Scalar>) -> Reply {
        self.calls += 1;
        if self.calls > CALL_LIMIT {
            return Err("Mission API call budget exceeded".into());
        }
        let a = Args(args);
        let reply = self.run(name, &a)?;
        self.ordered(name, &a);
        Ok(reply)
    }

    fn run(&mut self, name: &str, a: &Args) -> Reply {
        match name {
            "has_item" => self.has_item(a),
            "give_item" => {
                let item = self.item(a.get(1))?;
                self.exchange(a, None, Some(item))
            }
            "take_item" => {
                let item = self.item(a.get(1))?;
                self.exchange(a, Some(item), None)
            }
            "dialogue_busy" => one(Scalar::Boolean(self.dialogue_busy())),
            "tick" => one(Scalar::Integer(i64::from(self.state.tick))),
            "count" => self.count(a),
            "stock" => self.stock(a),
            "give_stock" => self.give_stock(a),
            "alive" => one(Scalar::Boolean(self.entity(a.get(0))?.is_some_and(alive))),
            "position" => self.position(a),
            "in_region" => self.in_region(a),
            "arrived" => self.arrived(a),
            "facing_done" => one(Scalar::Boolean(self.draft.facing.is_empty())),
            "get" => self.get(a),
            "set" => self.set(a),
            "damage" => self.damage(a),
            "recover" => self.recover(a),
            "spawn" => self.spawn(a),
            "move" => self.order_move(a, false),
            "attack_move" => self.order_move(a, true),
            "follow" => self.follow(a),
            "attack" => self.attack(a),
            "transfer" => self.transfer(a),
            "transform" => self.transform(a),
            "begin_scene" => self.begin_scene(a),
            "end_scene" => {
                self.draft.scene = None;
                none()
            }
            "camera" => self.camera(a),
            "face" => self.face(a),
            "say" => self.say(a),
            "begin_objective" => self.begin_objective(a),
            "complete_objective" => self.end_objective(a, ObjectiveState::Completed),
            "fail_objective" => self.end_objective(a, ObjectiveState::Failed),
            "objective" => {
                self.draft.objective = text(a.get(0), 500)?.to_string();
                none()
            }
            "ai_waves" | "ai_attack_now" | "ai_target" | "ai_behaviour" | "ai_wake" => {
                self.ai(name, a)
            }
            "win" => self.effect(MissionEffect::Win),
            "lose" => self.effect(MissionEffect::Lose),
            _ if campaign::NAMES.contains(&name) => self.campaign(name, a),
            name if scene::NAMES.contains(&name) => self.scene_call(name, a),
            _ => Err(format!("Unknown mission function {name}")),
        }
    }

    // Lookups.

    fn definition(&self, id: &str) -> Option<&'a Definition> {
        self.catalog.find(id).map(|def| self.catalog.get(def))
    }

    fn placement(&self, raw: &Scalar) -> Result<&'a Placement, String> {
        let id = text(raw, 120)?;
        self.script
            .placement(id)
            .ok_or_else(|| format!("Unknown entity ID: {id}"))
    }

    /// The placement's definition (validated when the map loaded).
    fn placed(&self, placement: &Placement) -> Result<&'a Definition, String> {
        self.definition(placement.definition.as_str())
            .ok_or_else(|| format!("{}: unknown definition", placement.id))
    }

    /// The entity created from a placement: none before a scripted spawn.
    fn entity(&self, raw: &Scalar) -> Result<Option<&'a Entity>, String> {
        let placement = self.placement(raw)?;
        Ok(entity_by_placement(self.state, &placement.id))
    }

    fn coordinate(&self, raw: &Scalar) -> Result<f64, String> {
        number(raw, 0.0, f64::from(self.script.size.saturating_sub(1)))
    }

    fn objective(&self, raw: &Scalar) -> Result<&'a Objective, String> {
        let id = text(raw, 120)?;
        self.script
            .objective(id)
            .ok_or_else(|| format!("Unknown objective: {id}"))
    }

    /// `none` or a player with a start on this map.
    fn seated(&self, raw: &Scalar) -> Result<Owner, String> {
        let owner = owner(raw)?;
        if self.script.seats(owner) {
            Ok(owner)
        } else {
            Err("Missing owner slot".into())
        }
    }

    fn effect(&mut self, effect: MissionEffect) -> Reply {
        self.effects.push(effect);
        none()
    }

    // Items.

    /// An item that fits a hero inventory.
    fn item(&self, raw: &Scalar) -> Result<Id, String> {
        match raw {
            Scalar::String(id) => self.definition(id),
            _ => None,
        }
        .filter(|d| d.kind == Kind::Item && d.item_effect.is_some())
        .map(|d| d.id.clone())
        .ok_or_else(|| "Expected a hero inventory item".into())
    }

    /// A living hero whose item slots this run has drafted (in `self.equipment`).
    fn inventory(&mut self, raw: &Scalar) -> Result<Option<EntityId>, String> {
        let Some(entity) = self.entity(raw)?.filter(|e| alive(e)) else {
            return Ok(None);
        };
        if !self.equipment.contains_key(&entity.id) {
            let Some(slots) = self.systems.equipment(entity) else {
                return Ok(None);
            };
            self.equipment.insert(entity.id, slots);
        }
        Ok(Some(entity.id))
    }

    fn has_item(&mut self, a: &Args) -> Reply {
        let item = Some(self.item(a.get(1))?);
        let has = match self.inventory(a.get(0))? {
            Some(entity) => self.equipment[&entity].contains(&item),
            None => false,
        };
        one(Scalar::Boolean(has))
    }

    /// Puts `item` into the first slot holding `from`; false when there is none.
    fn exchange(&mut self, a: &Args, from: Option<Id>, item: Option<Id>) -> Reply {
        let Some(entity) = self.inventory(a.get(0))? else {
            return one(Scalar::Boolean(false));
        };
        let slots = self.equipment.entry(entity).or_default();
        let Some(slot) = slots.iter().position(|s| *s == from) else {
            return one(Scalar::Boolean(false));
        };
        slots[slot] = item.clone();
        self.effects
            .push(MissionEffect::Equip { entity, slot, item });
        one(Scalar::Boolean(true))
    }

    // Queries.

    fn dialogue_busy(&self) -> bool {
        self.draft.dialogue.as_ref().is_some_and(|d| {
            if d.cinematic {
                d.remaining > 0
            } else {
                d.until > self.state.tick
            }
        })
    }

    fn count(&self, a: &Args) -> Reply {
        let owner = owner(a.get(0))?;
        let id = text(a.get(1), 120)?;
        let def = self
            .catalog
            .find(id)
            .filter(|&d| matches!(self.catalog.kind(d), Kind::Unit | Kind::Building))
            .ok_or("Count requires a unit or building definition")?;
        let count = self
            .state
            .entities
            .iter()
            .filter(|e| {
                e.owner == owner
                    && e.definition == def
                    && alive(e)
                    && e.ready_tick <= self.state.tick
                    && e.construction.is_none()
                    && !self.systems.fallen(e)
            })
            .count();
        one(Scalar::Integer(count as i64))
    }

    fn stock(&self, a: &Args) -> Reply {
        let owner = owner(a.get(0))?;
        let id = text(a.get(1), 120)?;
        let item = self
            .definition(id)
            .filter(|d| d.kind == Kind::Item && d.currency == Some(true))
            .ok_or("Stock requires an economy currency item")?;
        let amount = self
            .state
            .wallets
            .get(&owner)
            .and_then(|w| w.get(&item.id))
            .copied()
            .unwrap_or(0);
        one(Scalar::Integer(i64::from(amount)))
    }

    /// Adds a currency to a player's stock, or takes it (a negative amount, never below
    /// nothing).
    fn give_stock(&mut self, a: &Args) -> Reply {
        let owner = self.seated(a.get(0))?;
        if owner == Owner::Nobody {
            return Err("Stock belongs to a player".into());
        }
        let id = text(a.get(1), 120)?;
        let item = self
            .definition(id)
            .filter(|d| d.kind == Kind::Item && d.currency == Some(true))
            .ok_or("Stock requires an economy currency item")?;
        let amount = number(a.get(2), -1_000_000.0, 1_000_000.0)?;
        if amount.fract() != 0.0 {
            return Err("A stock amount must be an integer".into());
        }
        self.effect(MissionEffect::Stock {
            owner,
            item: item.id.clone(),
            amount: amount as i32,
        })
    }

    fn position(&self, a: &Args) -> Reply {
        Ok(match self.entity(a.get(0))? {
            Some(entity) => {
                let (x, y) = cell(entity);
                vec![Scalar::number(x), Scalar::number(y)]
            }
            None => vec![Scalar::Nil, Scalar::Nil],
        })
    }

    fn in_region(&self, a: &Args) -> Reply {
        let entity = self.entity(a.get(0))?;
        let id = text(a.get(1), 120)?;
        let region = self
            .script
            .region(id)
            .ok_or_else(|| format!("Unknown region: {id}"))?;
        let inside = entity.filter(|e| alive(e)).is_some_and(|e| {
            let (x, y) = cell(e);
            let (dx, dy) = (x - f64::from(region.x), y - f64::from(region.y));
            dx * dx + dy * dy <= region.radius * region.radius
        });
        one(Scalar::Boolean(inside))
    }

    fn arrived(&self, a: &Args) -> Reply {
        let entity = self.entity(a.get(0))?;
        let x = self.coordinate(a.get(1))?;
        let y = self.coordinate(a.get(2))?;
        let arrived = entity
            .filter(|e| e.unit.is_some() && alive(e) && !self.systems.has_order(e))
            .is_some_and(|e| {
                let dx = f64::from(e.position.x) - x * 1000.0;
                let dy = f64::from(e.position.y) - y * 1000.0;
                // Within half a cell.
                dx * dx + dy * dy < 250_000.0
            });
        one(Scalar::Boolean(arrived))
    }

    // Variables.

    fn get(&self, a: &Args) -> Reply {
        let key = text(a.get(0), 120)?;
        one(match self.draft.variables.get(key) {
            None => Scalar::Nil,
            Some(Variable::String(s)) => Scalar::String(s.clone()),
            Some(Variable::Integer(i)) => Scalar::Integer(*i),
            Some(Variable::Number(n)) => Scalar::Number(*n),
            Some(Variable::Boolean(b)) => Scalar::Boolean(*b),
        })
    }

    /// Values are checked when the run ends, so a run may hold a value it later replaces.
    fn set(&mut self, a: &Args) -> Reply {
        let key = text(a.get(0), 120)?.to_string();
        let value = match a.get(1) {
            Scalar::Nil => {
                self.draft.variables.remove(&key);
                return none();
            }
            Scalar::String(s) => Variable::String(s.clone()),
            Scalar::Integer(i) => Variable::Integer(*i),
            Scalar::Number(n) => Variable::Number(*n),
            Scalar::Boolean(b) => Variable::Boolean(*b),
            Scalar::Table(_) => return Err(VARIABLE_VALUE.into()),
        };
        self.draft.variables.insert(key, value);
        none()
    }

    // Combat.

    fn damage(&mut self, a: &Args) -> Reply {
        let target = self.entity(a.get(0))?;
        let source = match a.get(2) {
            Scalar::Nil => None,
            raw => Some(self.entity(raw)?),
        };
        let amount = number(a.get(1), 1.0, 100_000.0)?;
        if amount.fract() != 0.0 {
            return Err("Damage must be an integer".into());
        }
        let Some(target) = target.filter(|t| alive(t) && t.hp.is_some()) else {
            return one(Scalar::Boolean(false));
        };
        let source = match source {
            None => None,
            Some(Some(source)) if alive(source) => Some(source.id),
            Some(_) => return one(Scalar::Boolean(false)),
        };
        self.effects.push(MissionEffect::Damage {
            target: target.id,
            source,
            amount: amount as u32,
        });
        one(Scalar::Boolean(true))
    }

    fn recover(&mut self, a: &Args) -> Reply {
        let placement = self.placement(a.get(0))?;
        if self.placed(placement)?.kind != Kind::Unit {
            return Err("Recovery requires a unit".into());
        }
        self.effect(MissionEffect::Recover {
            placement: placement.id.clone(),
        })
    }

    // Units.

    fn spawn(&mut self, a: &Args) -> Reply {
        let placement = self.placement(a.get(0))?;
        let id = &placement.id;
        if placement.activation != Some(Activation::Script) {
            return Err(format!("{id} is not a scripted spawn"));
        }
        if self.draft.spawned.contains(id) {
            return Err(format!("{id} was already spawned"));
        }
        self.placed(placement)?;
        self.draft.spawned.push(id.clone());
        let camp = self.script.camp_of(id).map(|c| c.id.clone());
        self.effect(MissionEffect::Spawn {
            placement: id.clone(),
            camp,
        })
    }

    fn order_move(&mut self, a: &Args, attack_move: bool) -> Reply {
        let placement = self.placement(a.get(0))?;
        let destination = Milli {
            x: round(self.coordinate(a.get(1))?) * Milli::CELL,
            y: round(self.coordinate(a.get(2))?) * Milli::CELL,
        };
        let behaviors = &self.placed(placement)?.behaviors;
        if attack_move {
            if behaviors.combat.is_none() || behaviors.movement.is_none() {
                return Err(format!("{} cannot attack-move", placement.id));
            }
        } else if behaviors.movement.is_none() {
            return Err(format!("{} cannot move", placement.id));
        }
        self.effect(MissionEffect::Move {
            placement: placement.id.clone(),
            destination,
            attack_move,
        })
    }

    fn follow(&mut self, a: &Args) -> Reply {
        let placement = self.placement(a.get(0))?;
        let target = self.placement(a.get(1))?;
        if self.placed(placement)?.behaviors.movement.is_none() {
            return Err(format!("{} cannot follow", placement.id));
        }
        self.effect(MissionEffect::Follow {
            placement: placement.id.clone(),
            target: target.id.clone(),
        })
    }

    fn attack(&mut self, a: &Args) -> Reply {
        let placement = self.placement(a.get(0))?;
        let target = self.placement(a.get(1))?;
        self.effect(MissionEffect::Attack {
            placement: placement.id.clone(),
            target: target.id.clone(),
        })
    }

    fn transfer(&mut self, a: &Args) -> Reply {
        let placement = self.placement(a.get(0))?;
        let owner = owner(a.get(1))?;
        let entity = entity_by_placement(self.state, &placement.id);
        let definition = match entity {
            Some(e) => self.catalog.get(e.definition),
            None => self.placed(placement)?,
        };
        if !self.script.seats(owner) {
            return Err("Missing owner slot".into());
        }
        let behaviors = &definition.behaviors;
        if definition.kind != Kind::Unit
            || behaviors.camp_defense.is_some()
            || behaviors.work.is_some()
        {
            return Err("Transfer requires a non-camp military unit".into());
        }
        if entity.is_some_and(|e| self.systems.contained(e)) {
            return Err("Cannot transfer a contained unit".into());
        }
        self.effect(MissionEffect::Transfer {
            placement: placement.id.clone(),
            owner,
        })
    }

    fn transform(&mut self, a: &Args) -> Reply {
        let placement = self.placement(a.get(0))?;
        let entity = entity_by_placement(self.state, &placement.id);
        let target = self.catalog.find(text(a.get(1), 120)?);
        let owner = self.seated(a.get(2))?;
        let camp = match a.get(3) {
            Scalar::Nil => None,
            raw => {
                let camp = self.script.camp(text(raw, 120)?);
                match camp {
                    Some(camp)
                        if owner == Owner::Nobody
                            && !self.state.cleared_camps.contains(&camp.id) =>
                    {
                        Some(camp.id.clone())
                    }
                    _ => return Err("Transformation requires an uncleared neutral camp".into()),
                }
            }
        };
        let target = target
            .filter(|&def| {
                let d = self.catalog.get(def);
                let b = &d.behaviors;
                d.kind == Kind::Unit
                    && !d.is_hero()
                    && b.progression.is_none()
                    && b.inventory.is_none()
                    && b.abilities.is_none()
                    && b.camp_defense.is_some() == camp.is_some()
            })
            .ok_or(
                "Transformation target must be an ordinary unit with a matching camp assignment",
            )?;
        let Some(entity) = entity.filter(|e| e.unit.is_some() && alive(e)) else {
            return one(Scalar::Boolean(false));
        };
        let source = self.catalog.get(entity.definition);
        let b = &source.behaviors;
        if source.is_hero()
            || b.progression.is_some()
            || b.inventory.is_some()
            || b.abilities.is_some()
            || b.camp_defense.is_some()
            || self.systems.contained(entity)
        {
            return Err("Transformation requires an ordinary uncontained unit".into());
        }
        self.effects.push(MissionEffect::Transform {
            placement: placement.id.clone(),
            definition: target,
            owner,
            camp,
        });
        one(Scalar::Boolean(true))
    }

    // Presentation.

    fn begin_scene(&mut self, a: &Args) -> Reply {
        let x = self.coordinate(a.get(0))?;
        let y = self.coordinate(a.get(1))?;
        self.draft.scene = Some(Scene {
            position: milli(x, y),
            camera: None,
        });
        none()
    }

    fn camera(&mut self, a: &Args) -> Reply {
        let placement = self.placement(a.get(1))?;
        let subject = entity_by_placement(self.state, &placement.id);
        if self.placed(placement)?.kind != Kind::Unit {
            return Err("Camera subject must be a unit".into());
        }
        let look_at = match a.get(2) {
            Scalar::Nil => None,
            raw => {
                let target = self.placement(raw)?;
                if self.placed(target)?.kind != Kind::Unit {
                    return Err("Camera look-at must be a unit".into());
                }
                Some(target.id.clone())
            }
        };
        let transition_ms = match a.get(6) {
            Scalar::Nil => DEFAULT_TRANSITION_MS,
            raw => (number(raw, 0.0, 5.0)? * 1000.0).round() as u32,
        };
        let mode = match a.get(0) {
            Scalar::String(mode) => CameraMode::parse(mode),
            _ => None,
        }
        .ok_or("Camera mode must be rts, third-person or first-person")?;
        let lens = |index: usize, name: &str, min: f64, max: f64, scale: f64| match a.get(index) {
            Scalar::Nil => Ok(None),
            raw => number(raw, min, max)
                .map(|v| Some((v * scale).round() as u32))
                .map_err(|_| format!("Camera {name} must be a number in {min}..{max}")),
        };
        let camera = Camera {
            mode,
            entity: placement.id.clone(),
            look_at,
            distance: lens(3, "distance", 1.0, 30.0, 1000.0)?,
            height: lens(4, "height", 0.2, 30.0, 1000.0)?,
            fov: lens(5, "field of view", 25.0, 90.0, 1000.0)?,
            transition_ms,
        };
        let position = match subject {
            Some(entity) => {
                let (x, y) = cell(entity);
                milli(x, y)
            }
            None => milli(placement.position.x, placement.position.y),
        };
        self.draft.scene = Some(Scene {
            position,
            camera: Some(camera),
        });
        none()
    }

    fn face(&mut self, a: &Args) -> Reply {
        let placement = self.placement(a.get(0))?;
        let target = milli(self.coordinate(a.get(1))?, self.coordinate(a.get(2))?);
        if self.placed(placement)?.behaviors.movement.is_none() {
            return Err(format!("{} cannot turn", placement.id));
        }
        let id = placement.id.clone();
        self.draft.facing.retain(|f| f.placement != id);
        self.draft.facing.push(Facing {
            placement: id.clone(),
            target,
        });
        self.effect(MissionEffect::Face { placement: id })
    }

    fn say(&mut self, a: &Args) -> Reply {
        let cinematic = match a.get(4) {
            Scalar::Nil => false,
            Scalar::Boolean(b) => *b,
            _ => return Err("Cinematic must be a boolean".into()),
        };
        let portrait = self
            .definition(text(a.get(1), 120)?)
            .ok_or("Portrait must be an entity definition ID")?;
        let actor = match a.get(5) {
            Scalar::Nil => None,
            raw => match self.entity(raw)? {
                Some(e)
                    if e.unit.is_some()
                        && alive(e)
                        && self.catalog.id(e.definition) == &portrait.id =>
                {
                    Some(e.id)
                }
                _ => {
                    return Err("Dialogue actor must be a living unit matching the portrait".into());
                }
            },
        };
        let speaker = text(a.get(0), 80)?.to_string();
        let line = text(a.get(2), 2000)?.to_string();
        let ticks = match a.get(3) {
            // Nil: as long as the line reads (its source text, D96), as `speak`.
            Scalar::Nil => scene::reading_ticks(self.catalog, &line),
            raw => round(number(raw, 1.0, 60.0)? * f64::from(super::TICKS_PER_SECOND)) as u32,
        }
        .max(scene::voice_ticks(self.catalog, &line));
        let id = self.draft.next_dialogue;
        self.draft.next_dialogue += 1;
        self.draft.dialogue = Some(Dialogue {
            id,
            actor,
            speaker,
            portrait: portrait.id.clone(),
            text: line,
            duration_ticks: ticks,
            cinematic,
            remaining: if cinematic { ticks } else { 0 },
            until: self.state.tick + ticks,
        });
        none()
    }

    // Objectives.

    fn begin_objective(&mut self, a: &Args) -> Reply {
        let objective = self.objective(a.get(0))?;
        if self.draft.objective_states.contains_key(&objective.id) {
            return Err(format!("Objective already started: {}", objective.id));
        }
        if !objective.optional
            && self.script.objectives().iter().any(|other| {
                !other.optional
                    && self.draft.objective_states.get(&other.id) == Some(&ObjectiveState::Active)
            })
        {
            return Err("Complete the current primary objective before starting another".into());
        }
        self.draft
            .objective_states
            .insert(objective.id.clone(), ObjectiveState::Active);
        if !objective.optional {
            self.draft.objective = objective.description.clone();
        }
        none()
    }

    fn end_objective(&mut self, a: &Args, end: ObjectiveState) -> Reply {
        let objective = self.objective(a.get(0))?;
        match self.draft.objective_states.get_mut(&objective.id) {
            Some(state) if *state == ObjectiveState::Active => {
                *state = end;
                none()
            }
            _ => Err(format!("Objective is not active: {}", objective.id)),
        }
    }
}

/// Call arguments; a missing argument reads as nil.
struct Args(Vec<Scalar>);

impl Args {
    fn get(&self, index: usize) -> &Scalar {
        self.0.get(index).unwrap_or(&Scalar::Nil)
    }
}

fn one(value: Scalar) -> Reply {
    Ok(vec![value])
}

fn none() -> Reply {
    Ok(Vec::new())
}

/// Nonempty text of at most `max` UTF-16 code units (how the maps measure text).
fn text(raw: &Scalar, max: usize) -> Result<&str, String> {
    match raw {
        Scalar::String(s) if !s.is_empty() && s.encode_utf16().count() <= max => Ok(s),
        _ => Err("Expected a nonempty string".into()),
    }
}

fn number(raw: &Scalar, min: f64, max: f64) -> Result<f64, String> {
    let value = match raw {
        Scalar::Integer(i) => *i as f64,
        Scalar::Number(n) => *n,
        _ => f64::NAN,
    };
    if value.is_finite() && value >= min && value <= max {
        Ok(value)
    } else {
        Err(format!("Expected a number in {min}..{max}"))
    }
}

fn owner(raw: &Scalar) -> Result<Owner, String> {
    let parsed = match raw {
        Scalar::String(s) if s == "none" => Some(Owner::Nobody),
        Scalar::String(s) => s
            .strip_prefix("player.")
            .filter(|n| n.len() == 1)
            .and_then(|n| n.parse::<u8>().ok())
            .filter(|n| (1..=8).contains(n))
            .map(|n| Owner::Player(n - 1)),
        _ => None,
    };
    parsed.ok_or_else(|| "Expected an owner: none or player.1 to player.8".into())
}

/// Half up, as the old game's `Math.round`.
fn round(value: f64) -> i32 {
    (value + 0.5).floor() as i32
}

fn milli(x: f64, y: f64) -> Milli {
    Milli {
        x: round(x * f64::from(Milli::CELL)),
        y: round(y * f64::from(Milli::CELL)),
    }
}

/// Where scripts see an entity: a unit's navigation cell, or a static entity's placed
/// position.
fn cell(entity: &Entity) -> (f64, f64) {
    if entity.unit.is_some() {
        let (x, y) = entity.position.cell();
        (f64::from(x), f64::from(y))
    } else {
        let scale = f64::from(Milli::CELL);
        (
            f64::from(entity.position.x) / scale,
            f64::from(entity.position.y) / scale,
        )
    }
}
