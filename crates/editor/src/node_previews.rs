//! The live previews on an effect's nodes (D113, D115; docs/effect-graph.md): every
//! authoring node of every layer shows its layer playing as the chain makes it up to that
//! node (`canopy_fx::flow::expand` with `until`): the Spawner's plain dots, then their
//! life, their shape, their launch, their colour… and the renderer the finished layer. A
//! value node shows the node it drives. Each variant is compiled and cooked as the game
//! cooks a layer, played on a loop by the scene's graph runtime (an event-born layer under
//! the layers it is born of), and drawn into a cell of the thumbnail atlas
//! (`canopy_render::graph_thumbs`); finished layers take cells first.

use canopy_assets::{CookedEffect, EffectLayer, EffectTable, GraphPlacement, cook_program};
use canopy_content::Content;
use canopy_render::graph_fx::GraphFrame;
use canopy_render::graph_thumbs::{CELLS, ThumbCamera, ThumbFrame};
use canopy_scene::effects::graphs::{CueEnds, GraphRuntime};

/// Where previews put an effect's target (beams), and the team colour they show.
const PREVIEW_TARGET: Vec3 = Vec3::new(9.0, 2.0, 0.0);
const PREVIEW_TEAM: [f32; 3] = [0.1, 0.3, 1.0];
use glam::{Quat, Vec3};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

/// What the shell renders after the frame's UI (`crate::Editor`): the frame's steps and
/// cells, the programs when they changed, and the textures the draws name.
pub struct Request {
    pub pack: Arc<canopy_assets::Pack>,
    pub frame: ThumbFrame,
    pub programs: Option<Vec<String>>,
    pub textures: Vec<String>,
}

/// The previews' camera, shared by the layer's cells.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub focus: f32,
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            focus: 4.0,
            distance: 24.0,
            yaw: 0.6,
            pitch: 0.42,
        }
    }
}

#[derive(Default)]
pub struct NodePreviews {
    /// What the variants were built from: the layers' graphs without their layout and
    /// the effect's duration.
    built: Option<(Value, u32)>,
    table: Arc<EffectTable>,
    runtime: Option<GraphRuntime>,
    /// (Layer, node id) → variant (= cell = cue).
    cells: BTreeMap<(usize, String), u32>,
    /// The whole effect's variant.
    effect: Option<u32>,
    textures: Vec<String>,
    /// Each variant's graphs' renderers' texture indices (an event-born layer plays
    /// under its parent chain).
    texture_of: Vec<Vec<Vec<Option<u32>>>>,
    programs_changed: bool,
    started: Option<Instant>,
    /// Seconds of one loop.
    period: f64,
    pub camera: Camera,
}

fn without_layout(graph: &Value) -> Value {
    let mut graph = graph.clone();
    if let Some(object) = graph.as_object_mut() {
        object.remove("backdrops");
        if let Some(Value::Array(nodes)) = object.get_mut("nodes") {
            for node in nodes.iter_mut().filter_map(Value::as_object_mut) {
                node.remove("at");
                node.remove("collapsed");
            }
        }
    }
    graph
}

/// The layer an event-born layer's spawner names.
fn listened_layer(graph: &Value) -> Option<String> {
    graph
        .get("nodes")?
        .as_array()?
        .iter()
        .find(|n| {
            let kind = n.get("kind").and_then(Value::as_str);
            let mode = n
                .get("inputs")
                .and_then(|i| i.get("mode"))
                .and_then(Value::as_str);
            kind == Some("onEvent") || (kind == Some("spawner") && mode == Some("event"))
        })?
        .get("inputs")?
        .get("layer")?
        .as_str()
        .map(str::to_string)
}

impl NodePreviews {
    /// The cell of `node`'s preview in layer `layer`, if it has one.
    pub fn cell(&self, layer: usize, node: &str) -> Option<u32> {
        self.cells.get(&(layer, node.to_string())).copied()
    }

    /// The cell of the whole effect playing.
    pub fn effect_cell(&self) -> Option<u32> {
        self.effect
    }

    /// Builds the variants of every layer of the effect `document` again when it changed
    /// (moving nodes changes nothing).
    pub fn update(&mut self, content: &Content, document: &Value) {
        let layers: Vec<&Value> = document
            .get("graphs")
            .and_then(Value::as_array)
            .map(|g| g.iter().collect())
            .unwrap_or_default();
        let duration_ticks = document
            .get("durationTicks")
            .and_then(Value::as_u64)
            .unwrap_or(40) as u32;
        let all = Value::Array(
            layers
                .iter()
                .map(|l| without_layout(l.get("graph").unwrap_or(&Value::Null)))
                .collect(),
        );
        let key = (all, duration_ticks);
        if self.built.as_ref() == Some(&key) {
            return;
        }
        self.built = Some(key);
        let duration = duration_ticks.max(1) as f32 / 40.0;
        let placement = GraphPlacement {
            id: "preview".into(),
            start: 0.0,
            at_source: false,
            follow: true,
            offset: [0.0; 3],
        };
        let image = |asset: &str| content.image_path(asset).map(str::to_string);
        let id_of = |l: &Value| {
            l.get("id")
                .and_then(Value::as_str)
                .unwrap_or("preview")
                .to_string()
        };
        // Each layer as it compiles (`canopy_fx::flow::parts`): a spray listens to its
        // hidden sprayer, which plays under it.
        let mut sprayers: Vec<Option<(String, canopy_fx::Graph)>> = Vec::new();
        let graphs: Vec<Option<canopy_fx::Graph>> = layers
            .iter()
            .map(|l| {
                let graph = l
                    .get("graph")
                    .and_then(|g| serde_json::from_value::<canopy_fx::Graph>(g.clone()).ok())
                    .filter(canopy_fx::flow::is_flow);
                let Some(graph) = graph else {
                    sprayers.push(None);
                    return None;
                };
                let mut parts = canopy_fx::flow::parts(&id_of(l), &graph);
                let own = parts.pop().map(|(_, g)| g);
                sprayers.push(parts.pop());
                own
            })
            .collect();
        // What each node previews: (layer, the node the chain stops after, or None for the
        // finished layer), finished layers first.
        let mut wanted: Vec<(usize, Option<String>)> = Vec::new();
        for (l, graph) in graphs.iter().enumerate() {
            if graph.is_some() {
                wanted.push((l, None));
            }
        }
        for (l, graph) in graphs.iter().enumerate() {
            let Some(graph) = graph else { continue };
            for node in canopy_fx::flow::chain(graph) {
                let sink = canopy_fx::flow::find(&node.kind)
                    .is_some_and(|d| d.flow == canopy_fx::flow::Flow::Sink);
                // Spawners and Emits change no look: the canvas shows no preview on them.
                if !sink && !matches!(node.kind.as_str(), "spawner" | "emit") {
                    wanted.push((l, Some(node.id.clone())));
                }
            }
        }
        // The layers an event-born layer plays under, root first (finished).
        let ancestors_of = |l: usize| -> Vec<canopy_assets::CookedGraph> {
            let mut out: Vec<canopy_assets::CookedGraph> = Vec::new();
            let sprayer = |k: usize| {
                sprayers[k].as_ref().and_then(|(id, g)| {
                    canopy_assets::cook_graph(
                        GraphPlacement {
                            id: id.clone(),
                            ..placement.clone()
                        },
                        g,
                        duration,
                        image,
                    )
                    .ok()
                    .map(|(cooked, _)| cooked)
                })
            };
            if let Some(cooked) = sprayer(l) {
                out.push(cooked);
            }
            let mut listens = listened_layer(layers[l].get("graph").unwrap_or(&Value::Null));
            while let Some(layer_id) = listens.take() {
                if out.len() >= 8 {
                    break;
                }
                let Some(k) = layers.iter().position(|x| id_of(x) == layer_id) else {
                    break;
                };
                let Some(graph) = &graphs[k] else { break };
                let Ok((cooked, _)) = canopy_assets::cook_graph(
                    GraphPlacement {
                        id: layer_id.clone(),
                        ..placement.clone()
                    },
                    graph,
                    duration,
                    image,
                ) else {
                    break;
                };
                listens = listened_layer(layers[k].get("graph").unwrap_or(&Value::Null));
                out.insert(0, cooked);
                if let Some(cooked) = sprayer(k) {
                    out.insert(0, cooked);
                }
            }
            out
        };
        let mut cells = BTreeMap::new();
        let mut textures: Vec<String> = Vec::new();
        let mut texture_of = Vec::new();
        let mut longest = 0.0f32;
        let mut last_birth = 0.0f32;
        let mut effects = Vec::new();
        // The whole effect first: every layer as the game cooks it.
        let mut whole: Vec<canopy_assets::CookedGraph> = Vec::new();
        for (l, graph) in graphs.iter().enumerate() {
            let Some(graph) = graph else { continue };
            for (id, part) in sprayers[l]
                .iter()
                .cloned()
                .chain(std::iter::once((id_of(layers[l]), graph.clone())))
            {
                if let Ok((cooked, _)) = canopy_assets::cook_graph(
                    GraphPlacement {
                        id,
                        ..placement.clone()
                    },
                    &part,
                    duration,
                    image,
                ) {
                    whole.push(cooked);
                }
            }
        }
        canopy_assets::link_events(&mut whole);
        let mut effect_cell = None;
        if !whole.is_empty() {
            texture_of.push(
                whole
                    .iter()
                    .map(|g| {
                        g.renderers
                            .iter()
                            .map(|r| {
                                r.texture.as_ref().map(|name| {
                                    match textures.iter().position(|t| t == name) {
                                        Some(i) => i as u32,
                                        None => {
                                            textures.push(name.clone());
                                            textures.len() as u32 - 1
                                        }
                                    }
                                })
                            })
                            .collect()
                    })
                    .collect(),
            );
            let layers_of = whole
                .iter()
                .enumerate()
                .filter(|(_, g)| g.listens.is_none())
                .map(|(i, g)| EffectLayer::for_graph(i as u32, g, duration))
                .collect();
            effect_cell = Some(effects.len() as u32);
            effects.push(CookedEffect {
                id: "whole".into(),
                duration,
                looping: false,
                layers: layers_of,
                graphs: whole,
            });
        }
        let mut ancestors: BTreeMap<usize, Vec<canopy_assets::CookedGraph>> = BTreeMap::new();
        for (l, until) in wanted {
            if effects.len() as u32 >= CELLS {
                break;
            }
            let Some(graph) = &graphs[l] else { continue };
            let Ok(expanded) = canopy_fx::flow::expand(graph, until.as_deref()) else {
                continue;
            };
            let Ok(program) = canopy_fx::check(&expanded) else {
                continue;
            };
            let (cooked, _) = cook_program(
                GraphPlacement {
                    id: id_of(layers[l]),
                    ..placement.clone()
                },
                &program,
                &[],
                duration,
                image,
            );
            let mut chain = ancestors
                .entry(l)
                .or_insert_with(|| ancestors_of(l))
                .clone();
            chain.push(cooked);
            canopy_assets::link_events(&mut chain);
            longest = longest.max(chain.iter().map(|g| g.max_life).sum());
            for spawner in &chain[0].spawners {
                let end = match *spawner {
                    canopy_assets::CookedSpawner::Burst { time, .. } => time,
                    canopy_assets::CookedSpawner::Rate {
                        start, duration: d, ..
                    }
                    | canopy_assets::CookedSpawner::Curved {
                        start, duration: d, ..
                    } => start + if d > 0.0 { d } else { duration },
                };
                last_birth = last_birth.max(end);
            }
            texture_of.push(
                chain
                    .iter()
                    .map(|g| {
                        g.renderers
                            .iter()
                            .map(|r| {
                                r.texture.as_ref().map(|name| {
                                    match textures.iter().position(|t| t == name) {
                                        Some(i) => i as u32,
                                        None => {
                                            textures.push(name.clone());
                                            textures.len() as u32 - 1
                                        }
                                    }
                                })
                            })
                            .collect()
                    })
                    .collect(),
            );
            let v = effects.len() as u32;
            effects.push(CookedEffect {
                id: format!("v{v:03}"),
                duration,
                looping: false,
                layers: vec![EffectLayer::for_graph(0, &chain[0], duration)],
                graphs: chain,
            });
            match until {
                Some(node) => {
                    cells.insert((l, node), v);
                }
                None => {
                    // The finished layer: its renderers show it.
                    if let Some(graph) = &graphs[l] {
                        for node in &graph.nodes {
                            if canopy_fx::flow::find(&node.kind)
                                .is_some_and(|d| d.flow == canopy_fx::flow::Flow::Sink)
                            {
                                cells.insert((l, node.id.clone()), v);
                            }
                        }
                    }
                }
            }
        }
        // A value node shows the node it drives.
        for (l, graph) in graphs.iter().enumerate() {
            let Some(graph) = graph else { continue };
            for node in &graph.nodes {
                if let Some((target, _)) = canopy_fx::flow::driven(graph, &node.id)
                    && let Some(&v) = cells.get(&(l, target.to_string()))
                {
                    cells.insert((l, node.id.clone()), v);
                }
            }
        }
        if effects.is_empty() {
            self.clear();
            return;
        }
        let table = Arc::new(EffectTable { effects });
        self.runtime = Some(GraphRuntime::new(&table));
        self.table = table;
        self.cells = cells;
        self.effect = effect_cell;
        self.textures = textures;
        self.texture_of = texture_of;
        self.programs_changed = true;
        // From the first birth to the last particle's death, and a breath.
        self.period = f64::from(last_birth + longest) + 0.25;
        self.started.get_or_insert_with(Instant::now);
    }

    fn clear(&mut self) {
        self.cells.clear();
        self.effect = None;
        self.runtime = None;
        self.table = Arc::default();
    }

    /// This frame's request: every variant stepped to the same moment of its loop.
    pub fn request(&mut self, pack: &Arc<canopy_assets::Pack>) -> Option<Request> {
        let (frame, programs) = self.frame()?;
        Some(Request {
            pack: pack.clone(),
            frame,
            programs,
            textures: self.textures.clone(),
        })
    }

    /// Every variant stepped to the same moment of its loop, and the programs when they
    /// changed.
    fn frame(&mut self) -> Option<(ThumbFrame, Option<Vec<String>>)> {
        let runtime = self.runtime.as_mut()?;
        let elapsed = self
            .started
            .get_or_insert_with(Instant::now)
            .elapsed()
            .as_secs_f64();
        let age = elapsed % self.period.max(0.1);
        let mut graph = GraphFrame::default();
        runtime.begin();
        let variants = self.table.effects.len() as u32;
        let mut cells = BTreeMap::new();
        for v in 0..variants {
            // Every root layer of the variant (the whole effect has many; a node's chain
            // one, its event-born layers playing under it), each its own cue.
            for layer in &self.table.effects[v as usize].layers {
                let Some(g) = layer.graph else { continue };
                let cue = (v << 16) | g;
                cells.insert(cue, v);
                runtime.cue(
                    &self.table,
                    cue,
                    v as u16,
                    g as u16,
                    1 + g,
                    age,
                    Vec3::ZERO,
                    Quat::IDENTITY,
                    // A stand-in target beside the emitter, so beams show on the nodes.
                    CueEnds {
                        source: Vec3::ZERO,
                        target: PREVIEW_TARGET,
                        team: PREVIEW_TEAM,
                        carrier: None,
                    },
                    &mut graph,
                );
            }
        }
        let texture_of = &self.texture_of;
        runtime.end(
            &self.table,
            elapsed * 40.0,
            |e, g, r| {
                texture_of
                    .get(usize::from(e))
                    .and_then(|t| t.get(usize::from(g)))
                    .and_then(|t| t.get(r).copied().flatten())
            },
            &mut graph,
        );
        let c = self.camera;
        let camera = ThumbCamera::orbit(Vec3::new(0.0, c.focus, 0.0), c.distance, c.yaw, c.pitch);
        let frame = ThumbFrame {
            graph,
            cells,
            cameras: (0..variants).map(|v| (v, camera)).collect(),
        };
        let programs = std::mem::take(&mut self.programs_changed).then(|| {
            runtime
                .programs(&self.table)
                .into_iter()
                .map(str::to_string)
                .collect()
        });
        Some((frame, programs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A layer of `count` dots born at once, drawn as camera-facing sprites.
    fn dots(id: &str, count: u32) -> Value {
        serde_json::json!({"id": id, "name": id, "graph": {"nodes": [
            {"id": "spawner", "kind": "spawner", "inputs": {"mode": "burst", "count": count, "life": [1.0, 1.0]}},
            {"id": "billboard", "kind": "billboard"}
        ], "wires": [{"from": "spawner.out", "to": "billboard.in"}]}})
    }

    #[test]
    fn the_effect_node_plays_every_layer() {
        let data = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let content = Content::load(data).unwrap_or_else(|e| panic!("{e}"));
        let document = serde_json::json!({
            "durationTicks": 40,
            "graphs": [dots("a", 3), dots("b", 5), dots("c", 7)]
        });
        let mut previews = NodePreviews::default();
        previews.update(&content, &document);
        let whole = previews.effect_cell().expect("the whole effect has a cell");
        let (frame, _) = previews.frame().expect("a frame");
        let drawn: Vec<u32> = frame
            .graph
            .draws
            .iter()
            .filter(|d| frame.cells.get(&d.cue) == Some(&whole))
            .map(|d| d.cue & 0xffff)
            .collect();
        assert_eq!(
            drawn.len(),
            3,
            "every layer draws in the Effect node: {drawn:?}"
        );
    }
}
