//! The Effects workbench's preview (D113, docs/effect-graph.md): the open effect played by
//! the ability stage ([`crate::encounter`]) with the game's renderer, on its ground and
//! units, as it is being edited. A test ability that plays the effect on its target (or
//! its caster) is added to the content the stage casts from, and the stage's effects are
//! the pack's with this effect compiled afresh from the document (its graph layers
//! through `canopy_assets::cook_graph`, as the cooker does), so every edit shows at once.

use canopy_assets::{CookedEffect, EffectLayer, EffectTable, GraphPlacement, Pack, cook_graph};
use canopy_content::{Content, abilities::Ability, effects::VisualEffect};
use serde_json::{Value, json};
use std::sync::Arc;

/// The test ability's id (never a shipped one).
pub const ABILITY: &str = "ability.editor.effect-preview";

/// Where the test ability plays the effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Target,
    Caster,
    /// At a spot on open ground (fires, pools, marks), the caster standing aside.
    Ground,
}

/// One effect's preview: what it was built from, and what it built.
#[derive(Default)]
pub struct Preview {
    /// The content and document it was built from (by pointer and by value).
    built: Option<(Arc<Content>, Value, Anchor)>,
    content: Option<Arc<Content>>,
    table: Option<Arc<EffectTable>>,
    /// The pack's own effects, read once.
    base: Option<(Arc<Pack>, Arc<EffectTable>)>,
    pub anchor: Option<Anchor>,
    /// Why the preview shows nothing (the document does not read as an effect, or its
    /// graphs have problems).
    pub problems: Vec<String>,
}

impl Preview {
    /// Where the effect plays: as chosen, else at the caster for effects that run
    /// between the caster and the target, else at the target.
    pub fn anchor(&self) -> Anchor {
        self.anchor
            .or(self.built.as_ref().map(|b| b.2))
            .unwrap_or(Anchor::Target)
    }
}

fn seconds(ticks: u32) -> f32 {
    ticks as f32 / 40.0
}

impl Preview {
    /// The content the stage casts from (with the test ability) and the effects it plays,
    /// rebuilt when the content or the document changed. `None` until both read.
    pub fn prepare(
        &mut self,
        content: &Arc<Content>,
        pack: &Arc<Pack>,
        document: &Value,
    ) -> Option<(Arc<Content>, Arc<EffectTable>)> {
        // Effects running between the caster and the target (beams) play at the caster;
        // looping ones (fires, auras on the ground) on open ground.
        let anchor = self.anchor.unwrap_or(if reads_ends(document) {
            Anchor::Caster
        } else if document.get("loop").and_then(Value::as_bool) == Some(true) {
            Anchor::Ground
        } else {
            Anchor::Target
        });
        // Moving nodes on the canvas changes nothing that plays.
        let document = &without_layout(document);
        let fresh = self
            .built
            .as_ref()
            .is_none_or(|(c, d, a)| !Arc::ptr_eq(c, content) || d != document || *a != anchor);
        if fresh {
            self.built = Some((content.clone(), document.clone(), anchor));
            self.problems.clear();
            if self
                .base
                .as_ref()
                .is_none_or(|(p, _)| !Arc::ptr_eq(p, pack))
            {
                let table = pack.effects().unwrap_or_default();
                self.base = Some((pack.clone(), Arc::new(table)));
            }
            let base = self
                .base
                .as_ref()
                .map(|(_, t)| t.clone())
                .unwrap_or_default();
            match serde_json::from_value::<VisualEffect>(document.clone()) {
                Ok(effect) => {
                    let table = self.table_with(&base, content, &effect);
                    let content = with_test_ability(content, effect.id.as_str(), anchor);
                    match content {
                        Ok(content) => {
                            self.content = Some(Arc::new(content));
                            self.table = Some(Arc::new(table));
                        }
                        Err(error) => self.problems.push(error),
                    }
                }
                Err(error) => self.problems.push(format!("not an effect yet: {error}")),
            }
        }
        Some((self.content.clone()?, self.table.clone()?))
    }

    /// `base` with `effect`'s graph layers compiled from the document (its other layers
    /// as the pack has them).
    fn table_with(
        &mut self,
        base: &EffectTable,
        content: &Content,
        effect: &VisualEffect,
    ) -> EffectTable {
        let duration = seconds(effect.duration_ticks.max(1));
        let mut cooked = base
            .find(effect.id.as_str())
            .map(|i| base.effects[i].clone())
            .unwrap_or_else(|| CookedEffect {
                id: effect.id.to_string(),
                duration,
                looping: effect.looping,
                layers: Vec::new(),
                graphs: Vec::new(),
            });
        cooked.duration = duration;
        cooked.looping = effect.looping;
        cooked.layers.retain(|l| l.graph.is_none());
        cooked.graphs.clear();
        for g in effect.graphs.iter().filter(|g| g.enabled) {
            for (id, part) in canopy_fx::flow::parts(&g.id, &g.graph) {
                let placement = GraphPlacement {
                    id,
                    start: seconds(g.start_tick),
                    at_source: g.anchor == canopy_content::effects::LayerAnchor::Source,
                    follow: g.follow.unwrap_or(true),
                    offset: g
                        .offset
                        .as_ref()
                        .map_or([0.0; 3], |o| [o.x as f32, o.y as f32, o.z as f32]),
                };
                match cook_graph(placement, &part, duration, |asset| {
                    content.image_path(asset).map(str::to_string)
                }) {
                    Ok((graph, missing)) => {
                        for asset in missing {
                            self.problems.push(format!("{}: no image in {asset}", g.id));
                        }
                        cooked.graphs.push(graph);
                    }
                    Err(problems) => {
                        for problem in problems {
                            self.problems.push(format!("{}: {problem}", g.id));
                        }
                    }
                }
            }
        }
        self.problems
            .extend(canopy_assets::link_events(&mut cooked.graphs));
        // Event-born layers are stepped by their parent's instance: no cue of their own.
        for (index, graph) in cooked.graphs.iter().enumerate() {
            if graph.listens.is_none() {
                cooked
                    .layers
                    .push(EffectLayer::for_graph(index as u32, graph, duration));
            }
        }
        let mut table = base.clone();
        match table.find(effect.id.as_str()) {
            Some(i) => table.effects[i] = cooked,
            None => {
                let at = table
                    .effects
                    .partition_point(|e| e.id.as_str() < cooked.id.as_str());
                table.effects.insert(at, cooked);
            }
        }
        table
    }
}

/// Whether any of the effect's graph layers reads its ends (a beam or a link).
fn reads_ends(document: &Value) -> bool {
    document
        .get("graphs")
        .and_then(Value::as_array)
        .is_some_and(|layers| {
            layers.iter().any(|l| {
                l.pointer("/graph/nodes")
                    .and_then(Value::as_array)
                    .is_some_and(|nodes| {
                        nodes
                            .iter()
                            .any(|n| n.get("kind").and_then(Value::as_str) == Some("target"))
                    })
            })
        })
}

/// `document` without what only the canvas reads (node places, collapsed nodes,
/// backdrops), so moving a node does not rebuild the preview.
fn without_layout(document: &Value) -> Value {
    let mut document = document.clone();
    if let Some(Value::Array(graphs)) = document.get_mut("graphs") {
        for layer in graphs {
            let Some(graph) = layer.get_mut("graph").and_then(Value::as_object_mut) else {
                continue;
            };
            graph.remove("backdrops");
            if let Some(Value::Array(nodes)) = graph.get_mut("nodes") {
                for node in nodes.iter_mut().filter_map(Value::as_object_mut) {
                    node.remove("at");
                    node.remove("collapsed");
                }
            }
        }
    }
    document
}

/// `content` with the test ability: a free cast (a harmless hit) that plays `effect` on its
/// target (or its caster, or the ground it aims at) when it is released.
pub(crate) fn with_test_ability(
    content: &Content,
    effect: &str,
    anchor: Anchor,
) -> Result<Content, String> {
    let targeting = if anchor == Anchor::Ground {
        json!({
            "kind": "point", "radius": 1, "relations": ["enemy", "ally"], "allowSelf": true,
            "alive": true, "visible": true, "range": 40, "outOfRange": "reject"
        })
    } else {
        json!({
            "kind": "unit", "relations": ["enemy", "ally"], "allowSelf": true, "alive": true,
            "visible": true, "range": 40, "outOfRange": "reject"
        })
    };
    let ability: Ability = serde_json::from_value(json!({
        "id": ABILITY,
        "name": "Effect preview",
        "activation": "targeted",
        "ranks": [{"amount": 0}],
        "targeting": targeting,
        "cast": {
            "prepareTicks": 4, "recoverTicks": 8,
            "cost": {"resource": "mana", "amount": 0, "commit": "release"},
            "cooldown": {"ticks": 0, "start": "release", "scope": "ability"},
            "cancelBeforeRelease": "refund",
            "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]
        },
        "onRelease": [{"op": "damage", "target": "target", "amount": 0, "damageType": "spell"}],
        "presentation": {
            "animations": {"prepare": "cast", "release": "cast", "recover": "idle", "fallback": "idle"},
            "effects": [{
                "id": "preview", "effect": effect, "event": "released",
                "anchor": if anchor == Anchor::Caster { "caster" } else { "target" }
            }]
        }
    }))
    .map_err(|e| format!("the test ability: {e}"))?;
    let mut content = content.clone();
    let abilities = &mut content.abilities.abilities;
    match abilities.binary_search_by(|a| a.id.as_str().cmp(ABILITY)) {
        Ok(i) => abilities[i] = ability,
        Err(i) => abilities.insert(i, ability),
    }
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use canopy_sim::encounter::{Fixture, Take, suggest};

    #[test]
    fn the_test_ability_casts_on_the_stage() {
        let data = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let content = Content::load(data).unwrap_or_else(|e| panic!("{e}"));
        for anchor in [Anchor::Target, Anchor::Caster, Anchor::Ground] {
            let content = with_test_ability(&content, "effect.test.preview", anchor).unwrap();
            let settings = suggest(&content, ABILITY);
            let fixture = Arc::new(Fixture::new(&content, ABILITY, &settings).unwrap());
            let idle = Take::new(fixture.clone(), None).unwrap();
            let mut take = Take::new(fixture, idle.first_aim()).unwrap();
            for _ in 0..40 {
                take.step();
            }
            assert!(take.refusals().is_empty(), "{:?}", take.refusals());
            assert!(!take.events().is_empty(), "the cast released nothing");
        }
    }
}
