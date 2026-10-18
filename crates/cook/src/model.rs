//! glTF models to pack models: the authored transform, pivot and soil offset folded in,
//! node hierarchy flattened, primitives merged per material, streams quantised, clips
//! baked to model-space skinning matrices, and material conventions turned into numbers.

use crate::{
    anim::{self, Animation, Node},
    diag::Diagnostics,
    math::{Mat4, Vec3, normalize},
    texture::{AlphaPolicy, ColorSpace, Encoding, TextureParams},
};
use canopy_assets::{
    Aabb, ActiveGlow, AlphaMode, AnimationEvent, AnimationState, BlobBuilder, Blocker,
    BlockerShape, CLIP_FRAME_RATE, Clip, DepthStream, EntryKind, GroundContact, IndexFormat, Joint,
    Lod, Material, MaterialModel, MaterialTextures, ModelMeta, Prototype, Section, Skeleton,
    SkinVertex, Socket, TeamColor, Vertex, VertexColor, Wind, WindMode,
};
use canopy_content::assets::models::{self, PublishedModel};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

/// What the cooker knows about a glb besides its bytes.
#[derive(Debug, Clone)]
pub struct ModelSource<'a> {
    /// Pack entry name: the glb path relative to the data folder.
    pub name: &'a str,
    /// Published asset record id.
    pub asset: &'a str,
    /// The record's publication profile (`building`, `prop`, …).
    pub profile: &'a str,
    /// Whether the record belongs to a faction (player-owned art).
    pub faction: bool,
    /// The model's entry in `assets/authoring/models.json`.
    pub published: Option<&'a PublishedModel>,
}

/// Which pixels of an embedded image a texture takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Select {
    Image,
    /// The alpha channel, as the red channel.
    Alpha,
}

/// An embedded image a material samples; the caller cooks it through the texture cache.
#[derive(Debug, Clone)]
pub struct TextureRequest {
    pub name: String,
    pub bytes: Vec<u8>,
    pub select: Select,
    pub params: TextureParams,
}

pub struct CookedModel {
    pub blob: Vec<u8>,
    pub textures: Vec<TextureRequest>,
}

/// Material extras the cooker reads. Others are reported.
const READ_EXTRAS: &[&str] = &[
    "teamColorMask",
    "teamColorSourceMax",
    "teamColorDefault",
    "noShadow",
    "foliage",
    "backsideLighting",
    "painted",
    "vertexColorUsage",
    "crownNormal",
    "snowSurface",
    "activeGlow",
];
/// Extras of shading paths the native renderer drops (renderer §7, Q5 and Q6).
const RETIRED_EXTRAS: &[&str] = &[
    "referenceEnvironment",
    "sourceShader",
    "sourceShaderAttributes",
    "resinShimmer",
    "underlay",
    "shadingTexture",
];

/// The material name the old game recolours when no capability lists slots.
const LEGACY_TEAM_SLOT: &str = "TC_TeamColor";

pub fn cook(source: &ModelSource, glb: &[u8], diags: &mut Diagnostics) -> Option<CookedModel> {
    let gltf = match gltf::Gltf::from_slice(glb) {
        Ok(gltf) => gltf,
        Err(e) => {
            diags.error(format!("not a readable glTF binary: {e}"));
            return None;
        }
    };
    let blob: &[u8] = gltf.blob.as_deref().unwrap_or_default();
    let document = &gltf.document;
    for extension in document.extensions_required() {
        if !matches!(
            extension,
            "KHR_materials_unlit" | "KHR_materials_specular" | "KHR_texture_transform"
        ) {
            diags.error(format!("requires unsupported extension {extension}"));
        }
    }
    let Some(scene) = document
        .default_scene()
        .or_else(|| document.scenes().next())
    else {
        diags.error("has no scene");
        return None;
    };
    let nodes = anim::nodes(document);
    let rest_locals: Vec<Mat4> = nodes.iter().map(|n| n.rest_matrix).collect();
    let rest = anim::globals(&nodes, &rest_locals);
    let mut order = Vec::new();
    for root in scene.nodes() {
        walk(root, &mut order);
    }
    let caps = source.published.and_then(|p| p.capabilities.as_ref());

    let mut notes = Vec::new();
    let animations: Vec<Animation> = anim::animations(document, blob, &mut notes)
        .into_iter()
        .filter(|a| !a.channels.is_empty())
        .collect();
    for note in notes {
        diags.warn(note);
    }

    // Read every primitive in rest pose first: the soil offset needs the source bounds.
    let mut prims = Vec::new();
    for &node_index in &order {
        let node = document.nodes().nth(node_index).expect("walked node");
        let Some(mesh) = node.mesh() else { continue };
        for primitive in mesh.primitives() {
            if let Some(prim) = read_primitive(&node, &primitive, blob, &rest, diags) {
                prims.push(prim);
            }
        }
    }
    if prims.is_empty() {
        diags.error("has no triangles");
        return None;
    }
    let source_bounds = bounds(prims.iter().flat_map(|p| p.rest_positions.iter().copied()));

    // The authored transform (models.json) and the soil line, as one matrix.
    let transform = source.published.map(|p| &p.transform);
    let scale = transform.map_or(1.0, |t| t.scale);
    let pivot = transform.map_or([0.0; 3], |t| t.pivot);
    let ground = match caps.and_then(|c| c.ground_contact.as_ref()).map(|g| g.mode) {
        Some(models::GroundContactMode::Pivot) => GroundContact::Pivot,
        Some(models::GroundContactMode::Terrain) => GroundContact::Terrain,
        Some(models::GroundContactMode::Water) => GroundContact::Water,
        Some(models::GroundContactMode::Free) | None => GroundContact::Free,
    };
    let soil = if ground == GroundContact::Terrain {
        -source_bounds.0[1]
    } else {
        0.0
    };
    let turn = match transform.map(|t| t.forward) {
        Some(models::Forward::MinusZ) => std::f64::consts::PI,
        _ => 0.0,
    };
    let fold = Mat4::rotation_y(turn)
        * Mat4::scale([scale; 3])
        * Mat4::translation([-pivot[0], -pivot[1] + soil, -pivot[2]]);
    let unfold = fold.inverse();

    // Joint palette: skin joints, plus rigidly animated mesh nodes.
    let animated = !animations.is_empty();
    let mut palette: Vec<(usize, Mat4)> = Vec::new();
    let palette_index = |node: usize, inverse_bind: Mat4, palette: &mut Vec<(usize, Mat4)>| {
        if let Some(i) = palette
            .iter()
            .position(|(n, m)| *n == node && *m == inverse_bind)
        {
            return i;
        }
        palette.push((node, inverse_bind));
        palette.len() - 1
    };
    if animated {
        for prim in &mut prims {
            match &prim.skin {
                Some(skin) => {
                    let map: Vec<usize> = skin
                        .joints
                        .iter()
                        .map(|&(node, ibm)| palette_index(node, ibm, &mut palette))
                        .collect();
                    prim.palette_map = map;
                }
                None => {
                    let i = palette_index(prim.node, rest[prim.node].inverse(), &mut palette);
                    prim.palette_map = vec![i];
                }
            }
        }
        if palette.len() > 256 {
            diags.error(format!(
                "{} joints; at most 256 fit the skin stream",
                palette.len()
            ));
            return None;
        }
    }

    // Wind: the old game sways only models whose nodes carry `foliageWind`.
    let node_extras: Vec<Value> = document.nodes().map(|n| extras(n.extras())).collect();
    let foliage_wind = order
        .iter()
        .find_map(|&i| node_extras[i].get("foliageWind").cloned());
    let plant_height = order
        .iter()
        .find_map(|&i| node_extras[i].get("plantHeight").and_then(Value::as_f64))
        .map(|h| (h * scale) as f32);
    let wind_capability = caps.and_then(|c| c.wind.as_ref());
    let wind = match (&foliage_wind, wind_capability) {
        (None, None) => None,
        (sway, capability) => {
            if sway.is_none() {
                diags.warn(
                    "capabilities.wind is declared but no node carries foliageWind: cooked without a sway amplitude",
                );
            }
            let number = |key: &str| {
                sway.as_ref()
                    .and_then(|w| w.get(key))
                    .and_then(Value::as_f64)
            };
            Some(Wind {
                amplitude: (number("amplitude").unwrap_or(0.0) * scale) as f32,
                speed: number("speed").unwrap_or(0.0) as f32,
                mode: capability.map(|c| match c.mode {
                    models::WindMode::Tree => WindMode::Tree,
                    models::WindMode::Foliage => WindMode::Foliage,
                    models::WindMode::Grass => WindMode::Grass,
                }),
                strength: capability.map_or(0.0, |c| c.strength as f32),
                stiffness: capability.map_or(0.0, |c| c.stiffness as f32),
            })
        }
    };
    let sway_height = (source_bounds.1[1] - source_bounds.0[1]).max(0.1);

    // Materials, in glTF order, only those used.
    let skinned = animated && prims.iter().any(|p| p.skin.is_some());
    let team = team_slots(source, document, skinned, diags);
    let mut used: Vec<Option<usize>> = prims.iter().map(|p| p.material).collect();
    used.sort();
    used.dedup();
    let mut textures = Vec::new();
    let mut materials = Vec::new();
    let mut material_slot = BTreeMap::new();
    for &index in &used {
        let has_colors = prims
            .iter()
            .any(|p| p.material == index && p.colors.is_some());
        let material = match index {
            Some(i) => {
                let m = document.materials().nth(i).expect("material index");
                cook_material(
                    source,
                    &m,
                    i,
                    blob,
                    has_colors,
                    skinned,
                    &team,
                    &mut textures,
                    diags,
                )
            }
            None => default_material(skinned, has_colors),
        };
        material_slot.insert(index, materials.len() as u32);
        materials.push(material);
    }

    // Final streams, grouped by material.
    let has_colors = prims.iter().any(|p| p.colors.is_some());
    let has_leaves = prims.iter().any(|p| p.leaves.is_some());
    let mut stream = Stream::default();
    let mut sections = Vec::new();
    for &index in &used {
        let base_vertex = stream.positions.len() as u32;
        let first_index = stream.indices.len() as u32;
        for prim in prims.iter().filter(|p| p.material == index) {
            stream.append(
                prim,
                animated,
                &fold,
                &rest,
                sway_height,
                wind.is_some(),
                base_vertex,
                diags,
            );
        }
        sections.push(Section {
            material: material_slot[&index],
            first_index,
            index_count: stream.indices.len() as u32 - first_index,
            base_vertex,
            vertex_count: stream.positions.len() as u32 - base_vertex,
        });
    }

    // Skeleton, clips and sockets.
    let joints: Vec<Joint> = palette
        .iter()
        .map(|&(node, _)| Joint {
            name: nodes[node].name.clone(),
            parent: ancestor_in(&nodes, node, &palette),
        })
        .collect();
    let sockets = sockets(source, &nodes, &order, diags);
    let socket_rest: Vec<Mat4> = sockets
        .iter()
        .map(|s| fold * rest[s.node] * Mat4::translation(s.offset))
        .collect();
    let mut baked = Vec::new();
    for animation in &animations {
        let times = anim::frame_times(animation.duration, f64::from(CLIP_FRAME_RATE));
        let mut matrices = Vec::with_capacity(times.len() * palette.len());
        let mut socket_frames = Vec::with_capacity(times.len() * sockets.len());
        for &t in &times {
            let globals = anim::globals(&nodes, &animation.locals(&nodes, t));
            for &(node, ibm) in &palette {
                matrices.push(fold * globals[node] * ibm * unfold);
            }
            for s in &sockets {
                socket_frames.push(fold * globals[s.node] * Mat4::translation(s.offset));
            }
        }
        baked.push((animation, times.len(), matrices, socket_frames));
    }
    for animation in &animations {
        let moved = animation.animated_nodes().any(|n| {
            palette.iter().any(|&(p, _)| affects(&nodes, n, p))
                || sockets.iter().any(|s| affects(&nodes, n, s.node))
        });
        if !moved {
            diags.warn(format!("clip {} moves no joint or socket", animation.name));
        }
    }

    // Bounds: rest, every baked frame (per-joint boxes), and wind sway.
    let rest_bounds = bounds(stream.positions.iter().copied());
    let mut culling = rest_bounds;
    if animated {
        let joint_boxes = joint_bounds(&stream, palette.len());
        for (_, _, matrices, _) in &baked {
            for frame in matrices.chunks(palette.len().max(1)) {
                for (joint, matrix) in frame.iter().enumerate() {
                    if let Some(b) = joint_boxes[joint] {
                        for corner in corners(b) {
                            include(&mut culling, matrix.transform_point(corner));
                        }
                    }
                }
            }
        }
    }
    if let Some(w) = &wind {
        let pad = 5.0 * f64::from(w.amplitude);
        culling.0[0] -= pad;
        culling.0[2] -= pad;
        culling.1[0] += pad;
        culling.1[2] += pad;
    }

    // Animation states.
    let states = states(source, &animations, diags);
    if !animations.is_empty() && states.is_empty() {
        diags.warn("has clips but declares no animation states (capabilities.animations)");
    }

    // Quantise and write.
    let quant = Quantiser::new(rest_bounds, &stream.uvs);
    let mut builder = BlobBuilder::new();
    let vertices: Vec<Vertex> = (0..stream.positions.len())
        .map(|i| Vertex {
            position: quant.position(stream.positions[i], stream.bend[i]),
            normal: octahedral(stream.normals[i]),
            uv: quant.uv(stream.uvs[i]),
        })
        .collect();
    let skin: Option<Vec<SkinVertex>> = animated.then(|| stream.skin.clone());
    let vertices_span = builder.push(&vertices);
    let colors_span = has_colors.then(|| builder.push(&stream.colors));
    let leaves_span = has_leaves.then(|| builder.push(&stream.leaves));
    let skin_span = skin.as_ref().map(|s| builder.push(s));
    let max_section_vertices = sections.iter().map(|s| s.vertex_count).max().unwrap_or(0);
    let index_format = if max_section_vertices <= 65536 {
        IndexFormat::U16
    } else {
        IndexFormat::U32
    };
    // Levels of detail: coarser index lists over the same vertices, after the full ones.
    let mut lods = Vec::new();
    for (error, lists) in simplify_levels(&stream, &sections, rest_bounds) {
        let ranges = lists
            .iter()
            .map(|list| {
                let first = stream.indices.len() as u32;
                stream.indices.extend_from_slice(list);
                [first, list.len() as u32]
            })
            .collect();
        lods.push(Lod {
            error,
            sections: ranges,
            depth: [0, 0],
        });
    }
    let indices_span = push_indices(&mut builder, index_format, &stream.indices);

    let depth = depth_stream(
        &vertices,
        skin.as_deref(),
        &stream.indices,
        &sections,
        &materials,
        &mut lods,
    );
    let depth_positions = builder.push(&depth.positions);
    let depth_skin = depth.skin.as_ref().map(|s| builder.push(s));
    let depth_format = if depth.positions.len() <= 65536 {
        IndexFormat::U16
    } else {
        IndexFormat::U32
    };
    let depth_indices = push_indices(&mut builder, depth_format, &depth.indices);

    let clips = baked
        .iter()
        .map(|(animation, frames, matrices, socket_frames)| {
            let m: Vec<[f32; 12]> = matrices.iter().map(|m| m.to_3x4()).collect();
            let s: Vec<[f32; 12]> = socket_frames.iter().map(|m| m.to_3x4()).collect();
            Clip {
                name: animation.name.clone(),
                duration: animation.duration as f32,
                frame_count: *frames as u32,
                matrices: builder.push(&m),
                sockets: builder.push(&s),
            }
        })
        .collect();

    let meta = ModelMeta {
        asset: source.asset.to_owned(),
        rest_bounds: aabb(rest_bounds),
        bounds: aabb(culling),
        position_min: quant.position_min.map(|v| v as f32),
        position_extent: quant.position_extent.map(|v| v as f32),
        uv_min: quant.uv_min.map(|v| v as f32),
        uv_extent: quant.uv_extent.map(|v| v as f32),
        vertex_count: vertices.len() as u32,
        vertices: vertices_span,
        colors: colors_span,
        leaves: leaves_span,
        skin: skin_span,
        index_format,
        indices: indices_span,
        sections,
        depth: DepthStream {
            vertex_count: depth.positions.len() as u32,
            positions: depth_positions,
            skin: depth_skin,
            index_format: depth_format,
            index_count: depth.full,
            indices: depth_indices,
            masked: depth.masked,
        },
        lods,
        materials,
        skeleton: animated.then_some(Skeleton { joints }),
        clips,
        states,
        sockets: sockets
            .iter()
            .zip(&socket_rest)
            .map(|(s, m)| Socket {
                name: s.name.clone(),
                node: nodes[s.node].name.clone(),
                rest: m.to_3x4(),
            })
            .collect(),
        wind,
        plant_height,
        ground,
        ground_offset: (soil * scale) as f32,
        canopy: source.published.is_some_and(|p| p.canopy.is_some()),
        prototype: prototype(source),
    };
    Some(CookedModel {
        blob: builder.finish(EntryKind::Model, &meta),
        textures,
    })
}

fn walk(node: gltf::Node, order: &mut Vec<usize>) {
    order.push(node.index());
    for child in node.children() {
        walk(child, order);
    }
}

fn extras(raw: &gltf::json::Extras) -> Value {
    raw.as_ref()
        .and_then(|r| serde_json::from_str(r.get()).ok())
        .unwrap_or(Value::Null)
}

/// Whether moving node `moved` moves node `node` (it is `node` or an ancestor of it).
fn affects(nodes: &[Node], moved: usize, node: usize) -> bool {
    let mut current = Some(node);
    while let Some(n) = current {
        if n == moved {
            return true;
        }
        current = nodes[n].parent;
    }
    false
}

fn ancestor_in(nodes: &[Node], node: usize, palette: &[(usize, Mat4)]) -> Option<u32> {
    let mut current = nodes[node].parent;
    while let Some(n) = current {
        if let Some(i) = palette.iter().position(|(p, _)| *p == n) {
            return Some(i as u32);
        }
        current = nodes[n].parent;
    }
    None
}

struct PrimSkin {
    /// Palette candidates: (joint node, inverse bind matrix).
    joints: Vec<(usize, Mat4)>,
    /// Per vertex: four skin-joint indices and weights.
    influences: Vec<([u16; 4], [f32; 4])>,
}

struct Prim {
    node: usize,
    material: Option<usize>,
    /// Mesh-local positions as stored in the file.
    local_positions: Vec<Vec3>,
    local_normals: Vec<Vec3>,
    /// Source-space rest pose (node transforms and skins applied).
    rest_positions: Vec<Vec3>,
    uvs: Vec<[f64; 2]>,
    colors: Option<Vec<[f32; 4]>>,
    leaves: Option<Vec<[f32; 4]>>,
    skin: Option<PrimSkin>,
    indices: Vec<u32>,
    /// Palette index per skin joint (or the node's own entry for rigid primitives).
    palette_map: Vec<usize>,
}

fn read_primitive(
    node: &gltf::Node,
    primitive: &gltf::Primitive,
    blob: &[u8],
    rest: &[Mat4],
    diags: &mut Diagnostics,
) -> Option<Prim> {
    use gltf::mesh::Mode;
    let what = format!("node {:?}", node.name().unwrap_or_default());
    if primitive.mode() != Mode::Triangles {
        diags.error(format!(
            "{what}: {:?} primitives are not supported",
            primitive.mode()
        ));
        return None;
    }
    if primitive.morph_targets().len() > 0 {
        diags.warn(format!("{what}: morph targets are ignored"));
    }
    let reader = primitive.reader(|_| Some(blob));
    let Some(positions) = reader.read_positions() else {
        diags.error(format!("{what}: primitive without positions"));
        return None;
    };
    let local_positions: Vec<Vec3> = positions.map(|p| p.map(f64::from)).collect();
    let count = local_positions.len();
    let indices: Vec<u32> = match reader.read_indices() {
        Some(i) => i.into_u32().collect(),
        None => (0..count as u32).collect(),
    };
    if !indices.len().is_multiple_of(3) || indices.iter().any(|&i| i as usize >= count) {
        diags.error(format!("{what}: broken index buffer"));
        return None;
    }
    let local_normals: Vec<Vec3> = match reader.read_normals() {
        Some(n) => n.map(|n| normalize(n.map(f64::from))).collect(),
        None => {
            diags.warn(format!("{what}: no normals; computed smooth ones"));
            smooth_normals(&local_positions, &indices)
        }
    };
    let uvs: Vec<[f64; 2]> = match reader.read_tex_coords(0) {
        Some(uv) => uv.into_f32().map(|t| t.map(f64::from)).collect(),
        None => vec![[0.0; 2]; count],
    };
    let colors = reader
        .read_colors(0)
        .map(|c| c.into_rgba_f32().collect::<Vec<_>>());
    let leaves = primitive
        .get(&gltf::Semantic::Extras("LEAF".into()))
        .and_then(|accessor| gltf::accessor::Iter::<[f32; 4]>::new(accessor, |_| Some(blob)))
        .map(|it| it.collect::<Vec<_>>());
    if [colors.as_ref().map(Vec::len), leaves.as_ref().map(Vec::len)]
        .into_iter()
        .flatten()
        .any(|n| n != count)
        || local_normals.len() != count
        || uvs.len() != count
    {
        diags.error(format!("{what}: attribute counts differ"));
        return None;
    }
    let skin = match (node.skin(), reader.read_joints(0), reader.read_weights(0)) {
        (Some(skin), Some(joints), Some(weights)) => {
            let ibms: Vec<Mat4> = match skin.reader(|_| Some(blob)).read_inverse_bind_matrices() {
                Some(m) => m.map(Mat4::from_cols_f32).collect(),
                None => vec![Mat4::IDENTITY; skin.joints().len()],
            };
            let joints_list: Vec<(usize, Mat4)> = skin
                .joints()
                .zip(ibms)
                .map(|(j, ibm)| (j.index(), ibm))
                .collect();
            let influences: Vec<([u16; 4], [f32; 4])> =
                joints.into_u16().zip(weights.into_f32()).collect();
            if influences.len() != count
                || influences
                    .iter()
                    .any(|(j, _)| j.iter().any(|&j| j as usize >= joints_list.len()))
            {
                diags.error(format!("{what}: skin joints out of range"));
                return None;
            }
            Some(PrimSkin {
                joints: joints_list,
                influences,
            })
        }
        (Some(_), _, _) => {
            diags.error(format!("{what}: skinned node without JOINTS_0/WEIGHTS_0"));
            return None;
        }
        (None, Some(_), _) => {
            diags.warn(format!("{what}: has joints but no skin; drawn rigid"));
            None
        }
        (None, None, _) => None,
    };
    let rest_positions = match &skin {
        Some(skin) => (0..count)
            .map(|v| skinned_matrix(skin, v, |n| rest[n]).transform_point(local_positions[v]))
            .collect(),
        None => local_positions
            .iter()
            .map(|&p| rest[node.index()].transform_point(p))
            .collect(),
    };
    Some(Prim {
        node: node.index(),
        material: primitive.material().index(),
        local_positions,
        local_normals,
        rest_positions,
        uvs,
        colors,
        leaves,
        skin,
        indices,
        palette_map: Vec::new(),
    })
}

/// The weighted skinning matrix of vertex `v` with joint globals from `global`.
fn skinned_matrix(skin: &PrimSkin, v: usize, global: impl Fn(usize) -> Mat4) -> Mat4 {
    let (joints, weights) = skin.influences[v];
    let total: f32 = weights.iter().sum();
    let mut out = [[0.0; 4]; 4];
    for k in 0..4 {
        if weights[k] <= 0.0 {
            continue;
        }
        let w = f64::from(weights[k] / if total > 0.0 { total } else { 1.0 });
        let (node, ibm) = skin.joints[joints[k] as usize];
        let m = global(node) * ibm;
        for (c, column) in out.iter_mut().enumerate() {
            for (r, value) in column.iter_mut().enumerate() {
                *value += m.0[c][r] * w;
            }
        }
    }
    if total <= 0.0 {
        return Mat4::IDENTITY;
    }
    Mat4(out)
}

fn smooth_normals(positions: &[Vec3], indices: &[u32]) -> Vec<Vec3> {
    let mut normals = vec![[0.0; 3]; positions.len()];
    for tri in indices.chunks_exact(3) {
        let [a, b, c] = [0, 1, 2].map(|k| positions[tri[k] as usize]);
        let (e1, e2) = (
            [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
            [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
        );
        let n = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        for &i in tri {
            for k in 0..3 {
                normals[i as usize][k] += n[k];
            }
        }
    }
    normals.into_iter().map(normalize).collect()
}

/// The merged streams in model space, before quantisation.
#[derive(Default)]
struct Stream {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<[f64; 2]>,
    bend: Vec<f64>,
    colors: Vec<[u8; 4]>,
    leaves: Vec<[u16; 4]>,
    skin: Vec<SkinVertex>,
    /// Section-local indices.
    indices: Vec<u32>,
}

impl Stream {
    #[allow(clippy::too_many_arguments)]
    fn append(
        &mut self,
        prim: &Prim,
        animated: bool,
        fold: &Mat4,
        rest: &[Mat4],
        sway_height: f64,
        sway: bool,
        base_vertex: u32,
        diags: &mut Diagnostics,
    ) {
        let first = self.positions.len() as u32 - base_vertex;
        let count = prim.local_positions.len();
        // Static models are posed at rest; animated ones keep skinned vertices in bind
        // space and rigid ones at their node's rest transform.
        let shared = match (&prim.skin, animated) {
            (Some(_), false) => None,
            (Some(_), true) => Some(*fold),
            (None, _) => Some(*fold * rest[prim.node]),
        };
        let shared_normals = shared.map(|m| m.normal_matrix());
        let flip = shared.unwrap_or(*fold).determinant3() < 0.0;
        for v in 0..count {
            let (matrix, normals) = match (shared, shared_normals, &prim.skin) {
                (Some(m), Some(n), _) => (m, n),
                (_, _, Some(skin)) => {
                    let m = *fold * skinned_matrix(skin, v, |n| rest[n]);
                    (m, m.normal_matrix())
                }
                _ => unreachable!("rigid primitives share one matrix"),
            };
            self.positions
                .push(matrix.transform_point(prim.local_positions[v]));
            self.normals
                .push(normals.transform_normal(prim.local_normals[v]));
            self.uvs.push(prim.uvs[v]);
            let bend = if sway {
                (prim.local_positions[v][1] / sway_height)
                    .clamp(0.0, 1.0)
                    .powi(2)
            } else {
                0.0
            };
            self.bend.push(bend);
            let color = prim.colors.as_ref().map_or([1.0; 4], |c| c[v]);
            self.colors
                .push(color.map(|c| (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8));
            let leaf = prim.leaves.as_ref().map_or([0.0; 4], |l| l[v]);
            self.leaves
                .push(leaf.map(|c| (c.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16));
            if animated {
                self.skin.push(match &prim.skin {
                    Some(skin) => {
                        let (joints, weights) = skin.influences[v];
                        quantise_skin(
                            joints.map(|j| prim.palette_map[j as usize] as u8),
                            weights,
                            diags,
                        )
                    }
                    None => SkinVertex {
                        joints: [prim.palette_map[0] as u8, 0, 0, 0],
                        weights: [255, 0, 0, 0],
                    },
                });
            }
        }
        for tri in prim.indices.chunks_exact(3) {
            let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| first + i);
            if flip {
                self.indices.extend([a, c, b]);
            } else {
                self.indices.extend([a, b, c]);
            }
        }
    }
}

/// Unorm8 weights that sum to exactly 255 (largest remainders get the rest).
fn quantise_skin(joints: [u8; 4], weights: [f32; 4], diags: &mut Diagnostics) -> SkinVertex {
    let weights = weights.map(|w| w.max(0.0));
    let total: f32 = weights.iter().sum();
    if (total - 1.0).abs() > 0.02 {
        diags.warn("skin weights do not sum to 1 (renormalised)");
    }
    if total <= 0.0 {
        diags.warn("vertex without skin weight (bound to its first joint)");
        return SkinVertex {
            joints,
            weights: [255, 0, 0, 0],
        };
    }
    let scaled = weights.map(|w| f64::from(w / total) * 255.0);
    let mut q = scaled.map(|s| s.floor() as u8);
    let mut rest = 255 - q.iter().map(|&v| u32::from(v)).sum::<u32>();
    let mut order = [0usize, 1, 2, 3];
    order.sort_by(|&a, &b| {
        let (fa, fb) = (scaled[a] - scaled[a].floor(), scaled[b] - scaled[b].floor());
        fb.total_cmp(&fa).then(a.cmp(&b))
    });
    for &k in order.iter().cycle() {
        if rest == 0 {
            break;
        }
        q[k] += 1;
        rest -= 1;
    }
    // Unused slots point at joint 0 with no weight.
    let joints = std::array::from_fn(|k| if q[k] == 0 { 0 } else { joints[k] });
    SkinVertex { joints, weights: q }
}

struct Quantiser {
    position_min: [f64; 3],
    position_extent: [f64; 3],
    uv_min: [f64; 2],
    uv_extent: [f64; 2],
}

impl Quantiser {
    fn new(bounds: (Vec3, Vec3), uvs: &[[f64; 2]]) -> Self {
        let (mut uv_min, mut uv_max) = ([f64::MAX; 2], [f64::MIN; 2]);
        for uv in uvs {
            for k in 0..2 {
                uv_min[k] = uv_min[k].min(uv[k]);
                uv_max[k] = uv_max[k].max(uv[k]);
            }
        }
        if uvs.is_empty() {
            (uv_min, uv_max) = ([0.0; 2], [1.0; 2]);
        }
        Self {
            position_min: bounds.0,
            position_extent: std::array::from_fn(|k| (bounds.1[k] - bounds.0[k]).max(1e-6)),
            uv_min,
            uv_extent: std::array::from_fn(|k| (uv_max[k] - uv_min[k]).max(1e-6)),
        }
    }

    fn position(&self, p: Vec3, bend: f64) -> [u16; 4] {
        let q = |k: usize| unorm16((p[k] - self.position_min[k]) / self.position_extent[k]);
        [q(0), q(1), q(2), unorm16(bend)]
    }

    fn uv(&self, uv: [f64; 2]) -> [u16; 2] {
        std::array::from_fn(|k| unorm16((uv[k] - self.uv_min[k]) / self.uv_extent[k]))
    }
}

fn unorm16(v: f64) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16
}

/// Octahedral encoding as snorm16 ×2.
pub fn octahedral(n: Vec3) -> [i16; 2] {
    let n = normalize(n);
    let l1 = n[0].abs() + n[1].abs() + n[2].abs();
    let (mut x, mut y) = (n[0] / l1, n[1] / l1);
    if n[2] < 0.0 {
        let sign = |v: f64| if v >= 0.0 { 1.0 } else { -1.0 };
        (x, y) = ((1.0 - y.abs()) * sign(x), (1.0 - x.abs()) * sign(y));
    }
    [x, y].map(|v| (v.clamp(-1.0, 1.0) * 32767.0).round() as i16)
}

/// Inverse of [`octahedral`], for tests and inspection.
pub fn octahedral_decode(e: [i16; 2]) -> Vec3 {
    let (x, y) = (f64::from(e[0]) / 32767.0, f64::from(e[1]) / 32767.0);
    let z = 1.0 - x.abs() - y.abs();
    let t = (-z).max(0.0);
    let sign = |v: f64| if v >= 0.0 { 1.0 } else { -1.0 };
    normalize([x - t * sign(x), y - t * sign(y), z])
}

fn push_indices(
    builder: &mut BlobBuilder,
    format: IndexFormat,
    indices: &[u32],
) -> canopy_assets::Span {
    match format {
        IndexFormat::U16 => {
            let short: Vec<u16> = indices.iter().map(|&i| i as u16).collect();
            builder.push(&short)
        }
        IndexFormat::U32 => builder.push(indices),
    }
}

struct Depth {
    positions: Vec<[u16; 4]>,
    skin: Option<Vec<SkinVertex>>,
    /// The full mesh's indices, then every level of detail's.
    indices: Vec<u32>,
    /// Indices of the full mesh.
    full: u32,
    masked: Vec<u32>,
}

/// Welds the opaque shadow casters into one position (and skin) stream, and gives every
/// level of detail its depth index range over the same welded vertices.
fn depth_stream(
    vertices: &[Vertex],
    skin: Option<&[SkinVertex]>,
    indices: &[u32],
    sections: &[Section],
    materials: &[Material],
    lods: &mut [Lod],
) -> Depth {
    let mut depth = Depth {
        positions: Vec::new(),
        skin: skin.map(|_| Vec::new()),
        indices: Vec::new(),
        full: 0,
        masked: Vec::new(),
    };
    let mut welded: HashMap<([u16; 4], SkinVertex), u32> = HashMap::new();
    let casters: Vec<usize> = (0..sections.len())
        .filter(|&s| {
            let material = &materials[sections[s].material as usize];
            material.casts_shadow && material.alpha == AlphaMode::Opaque
        })
        .collect();
    depth.masked = (0..sections.len())
        .filter(|&s| {
            let material = &materials[sections[s].material as usize];
            material.casts_shadow && material.alpha == AlphaMode::Mask
        })
        .map(|s| s as u32)
        .collect();
    let mut weld = |depth: &mut Depth, section: &Section, first: u32, count: u32| {
        for &local in &indices[first as usize..(first + count) as usize] {
            let v = (section.base_vertex + local) as usize;
            let skin_vertex = skin.map_or(SkinVertex::default(), |s| s[v]);
            let key = (vertices[v].position, skin_vertex);
            let index = *welded.entry(key).or_insert_with(|| {
                depth.positions.push(vertices[v].position);
                if let Some(s) = depth.skin.as_mut() {
                    s.push(skin_vertex);
                }
                depth.positions.len() as u32 - 1
            });
            depth.indices.push(index);
        }
    };
    for &s in &casters {
        let section = &sections[s];
        weld(
            &mut depth,
            section,
            section.first_index,
            section.index_count,
        );
    }
    depth.full = depth.indices.len() as u32;
    for lod in lods {
        let first = depth.indices.len() as u32;
        for &s in &casters {
            let [first_index, count] = lod.sections[s];
            weld(&mut depth, &sections[s], first_index, count);
        }
        lod.depth = [first, depth.indices.len() as u32 - first];
    }
    depth
}

/// Fewest triangles a model needs before it gets levels of detail.
const LOD_MIN_TRIANGLES: usize = 1024;
/// The deviation each level of detail may have, as a share of the model's size: each
/// level is the smallest mesh within it.
const LOD_ERRORS: [f64; 4] = [0.004, 0.012, 0.03, 0.07];

/// Simplified index lists (section-local, per section) for each level of detail with the
/// largest deviation of any section in model units, finest first. Levels that shrink the
/// previous one by less than a fifth are left out. Topology is kept: no piece vanishes
/// and seams stay closed, which bounds how far a level can shrink. Positions only: seams
/// (split UVs and normals) stay closed, as meshoptimizer keeps vertices sharing a
/// position together.
fn simplify_levels(
    stream: &Stream,
    sections: &[Section],
    rest: (Vec3, Vec3),
) -> Vec<(f32, Vec<Vec<u32>>)> {
    let total: usize = sections.iter().map(|s| s.index_count as usize / 3).sum();
    if total < LOD_MIN_TRIANGLES {
        return Vec::new();
    }
    let size = (0..3)
        .map(|i| (rest.1[i] - rest.0[i]).powi(2))
        .sum::<f64>()
        .sqrt();
    let mut levels: Vec<(f32, Vec<Vec<u32>>)> = Vec::new();
    let mut previous = total;
    for share in LOD_ERRORS {
        let limit = (share * size) as f32;
        let options = meshopt::SimplifyOptions::ErrorAbsolute;
        let mut error = 0.0f32;
        let mut lists = Vec::with_capacity(sections.len());
        for section in sections {
            let base = section.base_vertex as usize;
            let positions: Vec<[f32; 3]> = stream.positions
                [base..base + section.vertex_count as usize]
                .iter()
                .map(|p| p.map(|v| v as f32))
                .collect();
            let first = section.first_index as usize;
            let indices = &stream.indices[first..first + section.index_count as usize];
            if indices.is_empty() {
                lists.push(Vec::new());
                continue;
            }
            let target = 3;
            let mut section_error = 0.0f32;
            let list = meshopt::simplify_decoder(
                indices,
                &positions,
                target,
                limit,
                options,
                Some(&mut section_error),
            );
            error = error.max(section_error);
            lists.push(list);
        }
        let triangles: usize = lists.iter().map(|l| l.len() / 3).sum();
        if triangles * 5 > previous * 4 {
            continue;
        }
        previous = triangles;
        levels.push((error, lists));
    }
    levels
}

fn bounds(points: impl Iterator<Item = Vec3>) -> (Vec3, Vec3) {
    let mut b = ([f64::MAX; 3], [f64::MIN; 3]);
    for p in points {
        include(&mut b, p);
    }
    if b.0[0] > b.1[0] {
        return ([0.0; 3], [0.0; 3]);
    }
    b
}

fn include(b: &mut (Vec3, Vec3), p: Vec3) {
    b.0 = std::array::from_fn(|k| b.0[k].min(p[k]));
    b.1 = std::array::from_fn(|k| b.1[k].max(p[k]));
}

fn corners(b: (Vec3, Vec3)) -> [Vec3; 8] {
    std::array::from_fn(|i| {
        [
            if i & 1 == 0 { b.0[0] } else { b.1[0] },
            if i & 2 == 0 { b.0[1] } else { b.1[1] },
            if i & 4 == 0 { b.0[2] } else { b.1[2] },
        ]
    })
}

/// Bind-space box of the vertices each joint moves.
fn joint_bounds(stream: &Stream, joints: usize) -> Vec<Option<(Vec3, Vec3)>> {
    let mut boxes: Vec<Option<(Vec3, Vec3)>> = vec![None; joints];
    for (p, skin) in stream.positions.iter().zip(&stream.skin) {
        for k in 0..4 {
            if skin.weights[k] > 0 {
                let b = boxes[skin.joints[k] as usize].get_or_insert((*p, *p));
                include(b, *p);
            }
        }
    }
    boxes
}

fn aabb(b: (Vec3, Vec3)) -> Aabb {
    Aabb {
        min: b.0.map(|v| v as f32),
        max: b.1.map(|v| v as f32),
    }
}

/// Material names that take the player colour.
fn team_slots(
    source: &ModelSource,
    document: &gltf::Document,
    skinned: bool,
    diags: &mut Diagnostics,
) -> Vec<String> {
    let names: Vec<&str> = document.materials().filter_map(|m| m.name()).collect();
    let capability = source
        .published
        .and_then(|p| p.capabilities.as_ref())
        .and_then(|c| c.team_color.as_ref());
    match capability {
        Some(team) => {
            let found: Vec<String> = team
                .slots
                .iter()
                .filter(|slot| {
                    let present = names.contains(&slot.as_str());
                    if !present {
                        diags.warn(format!(
                            "team colour slot {slot} is not a material of the model"
                        ));
                    }
                    present
                })
                .cloned()
                .collect();
            if found.is_empty() {
                diags.error("declares team colour but no material can take it");
            }
            found
        }
        None => {
            if names.contains(&LEGACY_TEAM_SLOT) {
                diags.warn(format!(
                    "material {LEGACY_TEAM_SLOT} without a teamColor capability: recoloured by convention"
                ));
                vec![LEGACY_TEAM_SLOT.to_owned()]
            } else {
                let owned = skinned || source.profile == "building";
                if source.faction
                    && owned
                    && source.published.is_some_and(|p| p.capabilities.is_some())
                {
                    diags.warn(
                        "faction unit or building without team colour: owners cannot be told apart",
                    );
                }
                Vec::new()
            }
        }
    }
}

fn default_material(skinned: bool, has_colors: bool) -> Material {
    Material {
        name: "default".into(),
        model: if skinned {
            MaterialModel::Unit
        } else {
            MaterialModel::Prop
        },
        base_color: [1.0; 4],
        emissive: [0.0; 3],
        roughness: 1.0,
        metalness: 1.0,
        specular: 1.0,
        specular_color: [1.0; 3],
        alpha: AlphaMode::Opaque,
        alpha_cutoff: 0.5,
        double_sided: false,
        casts_shadow: true,
        vertex_color: if has_colors {
            VertexColor::Tint
        } else {
            VertexColor::None
        },
        painted: false,
        foliage: false,
        backside_lighting: 0.0,
        crown_normal: 1.0,
        snow_surface: false,
        team_color: None,
        normal_scale: 1.0,
        occlusion_strength: 1.0,
        active: None,
        textures: MaterialTextures::default(),
    }
}

#[allow(clippy::too_many_arguments)]
fn cook_material(
    source: &ModelSource,
    m: &gltf::Material,
    index: usize,
    blob: &[u8],
    has_colors: bool,
    skinned: bool,
    team: &[String],
    textures: &mut Vec<TextureRequest>,
    diags: &mut Diagnostics,
) -> Material {
    let name = m.name().unwrap_or_default().to_owned();
    let what = format!("material {name:?}");
    let x = extras(m.extras());
    if let Value::Object(map) = &x {
        for key in map.keys() {
            if !READ_EXTRAS.contains(&key.as_str()) && !RETIRED_EXTRAS.contains(&key.as_str()) {
                diags.warn(format!("{what}: unknown extras key {key:?} ignored"));
            }
        }
    }
    let flag = |key: &str| x.get(key).and_then(Value::as_bool).unwrap_or(false);
    let number = |key: &str| x.get(key).and_then(Value::as_f64);
    let pbr = m.pbr_metallic_roughness();
    let alpha = match m.alpha_mode() {
        gltf::material::AlphaMode::Opaque => AlphaMode::Opaque,
        gltf::material::AlphaMode::Mask => AlphaMode::Mask,
        gltf::material::AlphaMode::Blend => AlphaMode::Blend,
    };
    let alpha_cutoff = m.alpha_cutoff().unwrap_or(0.5);
    let is_team = team.contains(&name);
    let mask_in_alpha = x.get("teamColorMask").and_then(Value::as_str) == Some("baseColorAlpha");
    if mask_in_alpha && !is_team {
        diags.warn(format!(
            "{what}: has a team colour mask but is not a team colour slot"
        ));
    }
    let wants_mask = source
        .published
        .and_then(|p| p.capabilities.as_ref())
        .and_then(|c| c.team_color.as_ref())
        .is_some_and(|t| t.mode == models::TeamColorMode::Mask);
    if is_team && wants_mask && !mask_in_alpha {
        diags.warn(format!(
            "{what}: teamColor mode is mask but the material has no teamColorMask; the whole material takes the colour"
        ));
    }
    let base_factor = pbr.base_color_factor();
    let team_color = is_team.then(|| TeamColor {
        source_max: number("teamColorSourceMax").unwrap_or(1.0).max(0.001) as f32,
        default: x
            .get("teamColorDefault")
            .and_then(|v| serde_json::from_value::<[f32; 3]>(v.clone()).ok())
            .unwrap_or([base_factor[0], base_factor[1], base_factor[2]]),
    });
    let foliage = flag("foliage");
    let model = if m.unlit() {
        MaterialModel::Effect
    } else if foliage {
        MaterialModel::Foliage
    } else if skinned {
        MaterialModel::Unit
    } else if source.profile == "building" {
        MaterialModel::Building
    } else {
        MaterialModel::Prop
    };

    let mut out = MaterialTextures::default();
    let entry = |slot: &str| format!("{}#material{index}.{slot}", source.name);
    let request = |info_texture: gltf::Texture,
                   tex_coord: u32,
                   transform: bool,
                   slot: &str,
                   select: Select,
                   params: TextureParams,
                   textures: &mut Vec<TextureRequest>,
                   diags: &mut Diagnostics|
     -> Option<String> {
        if tex_coord != 0 {
            diags.warn(format!(
                "{what}: {slot} uses TEXCOORD_{tex_coord}; only TEXCOORD_0 is cooked"
            ));
        }
        if transform {
            diags.warn(format!("{what}: {slot} texture transform ignored"));
        }
        let bytes = match info_texture.source().source() {
            gltf::image::Source::View { view, .. } => {
                let start = view.offset();
                blob.get(start..start + view.length())?.to_vec()
            }
            gltf::image::Source::Uri { uri, .. } => {
                diags.error(format!(
                    "{what}: external image {uri:?}; publication embeds images"
                ));
                return None;
            }
        };
        let name = entry(slot);
        textures.push(TextureRequest {
            name: name.clone(),
            bytes,
            select,
            params,
        });
        Some(name)
    };
    if let Some(info) = pbr.base_color_texture() {
        let transform = info.texture_transform().is_some();
        let alpha_policy = match alpha {
            _ if is_team && mask_in_alpha => AlphaPolicy::Opaque,
            AlphaMode::Opaque => AlphaPolicy::Opaque,
            AlphaMode::Mask => AlphaPolicy::Cutout(alpha_cutoff),
            AlphaMode::Blend => AlphaPolicy::Auto,
        };
        let params = TextureParams {
            color: ColorSpace::Srgb,
            alpha: alpha_policy,
            normal_map: false,
            encoding: Encoding::Bc7,
        };
        out.base_color = request(
            info.texture(),
            info.tex_coord(),
            transform,
            "base",
            Select::Image,
            params,
            textures,
            diags,
        );
        if is_team && mask_in_alpha {
            if alpha != AlphaMode::Opaque {
                diags.warn(format!(
                    "{what}: alpha holds the team mask, so the material is drawn opaque"
                ));
            }
            let params = TextureParams {
                color: ColorSpace::Linear,
                alpha: AlphaPolicy::Opaque,
                normal_map: false,
                encoding: Encoding::R8,
            };
            out.team_mask = request(
                info.texture(),
                info.tex_coord(),
                transform,
                "team",
                Select::Alpha,
                params,
                textures,
                diags,
            );
        }
    } else if is_team && mask_in_alpha {
        diags.warn(format!(
            "{what}: teamColorMask without a base colour texture"
        ));
    }
    let linear = TextureParams {
        color: ColorSpace::Linear,
        alpha: AlphaPolicy::Opaque,
        normal_map: false,
        encoding: Encoding::Bc7,
    };
    let mut normal_scale = 1.0;
    if let Some(info) = m.normal_texture() {
        normal_scale = info.scale();
        let params = TextureParams {
            normal_map: true,
            ..linear
        };
        out.normal = request(
            info.texture(),
            info.tex_coord(),
            false,
            "normal",
            Select::Image,
            params,
            textures,
            diags,
        );
    }
    if let Some(info) = pbr.metallic_roughness_texture() {
        let transform = info.texture_transform().is_some();
        out.metallic_roughness = request(
            info.texture(),
            info.tex_coord(),
            transform,
            "metallic_roughness",
            Select::Image,
            linear,
            textures,
            diags,
        );
    }
    let mut occlusion_strength = 1.0;
    if let Some(info) = m.occlusion_texture() {
        occlusion_strength = info.strength();
        out.occlusion = request(
            info.texture(),
            info.tex_coord(),
            false,
            "occlusion",
            Select::Image,
            linear,
            textures,
            diags,
        );
    }
    if let Some(info) = m.emissive_texture() {
        let transform = info.texture_transform().is_some();
        let params = TextureParams {
            color: ColorSpace::Srgb,
            ..linear
        };
        out.emissive = request(
            info.texture(),
            info.tex_coord(),
            transform,
            "emissive",
            Select::Image,
            params,
            textures,
            diags,
        );
    }
    let vertex_color = match (
        has_colors,
        x.get("vertexColorUsage").and_then(Value::as_str),
    ) {
        (false, _) => VertexColor::None,
        (true, None | Some("tint")) => VertexColor::Tint,
        (true, Some(_)) => VertexColor::Occlusion,
    };
    let emissive_strength = m.emissive_strength().unwrap_or(1.0);
    let specular = m.specular();
    Material {
        name,
        model,
        base_color: team_color.as_ref().map_or(base_factor, |t| {
            [t.default[0], t.default[1], t.default[2], base_factor[3]]
        }),
        emissive: m.emissive_factor().map(|c| c * emissive_strength),
        roughness: pbr.roughness_factor(),
        metalness: pbr.metallic_factor(),
        specular: specular.as_ref().map_or(1.0, |s| s.specular_factor()),
        specular_color: specular
            .as_ref()
            .map_or([1.0; 3], |s| s.specular_color_factor()),
        alpha: if is_team && mask_in_alpha {
            AlphaMode::Opaque
        } else {
            alpha
        },
        alpha_cutoff,
        double_sided: m.double_sided() || foliage,
        casts_shadow: !flag("noShadow"),
        vertex_color,
        painted: flag("painted"),
        foliage,
        backside_lighting: number("backsideLighting").unwrap_or(0.0) as f32,
        crown_normal: number("crownNormal").unwrap_or(1.0).clamp(0.0, 1.0) as f32,
        snow_surface: flag("snowSurface"),
        team_color,
        normal_scale,
        occlusion_strength,
        active: x.get("activeGlow").map(|g| ActiveGlow {
            pulse: g.get("pulse").and_then(Value::as_f64).unwrap_or(0.0) as f32,
            flow: g.get("flow").and_then(Value::as_f64).unwrap_or(0.0) as f32,
        }),
        textures: out,
    }
}

struct SocketSource {
    name: String,
    node: usize,
    offset: Vec3,
}

/// Declared sockets plus nodes named `socket*` (the old game's convention).
fn sockets(
    source: &ModelSource,
    nodes: &[Node],
    order: &[usize],
    diags: &mut Diagnostics,
) -> Vec<SocketSource> {
    let mut out: Vec<SocketSource> = Vec::new();
    let declared = source
        .published
        .and_then(|p| p.capabilities.as_ref())
        .and_then(|c| c.sockets.as_ref());
    for socket in declared.into_iter().flatten() {
        match order.iter().find(|&&i| nodes[i].name == socket.node) {
            Some(&node) => out.push(SocketSource {
                name: socket.name.to_string(),
                node,
                offset: socket.offset,
            }),
            None => diags.warn(format!(
                "socket {} names missing node {:?}",
                socket.name, socket.node
            )),
        }
    }
    for &i in order {
        let name = &nodes[i].name;
        let conventional = name.len() >= 6 && name[..6].eq_ignore_ascii_case("socket");
        if conventional && !out.iter().any(|s| s.node == i) && !out.iter().any(|s| &s.name == name)
        {
            out.push(SocketSource {
                name: name.clone(),
                node: i,
                offset: [0.0; 3],
            });
        }
    }
    out
}

fn states(
    source: &ModelSource,
    animations: &[Animation],
    diags: &mut Diagnostics,
) -> Vec<AnimationState> {
    let declared = source
        .published
        .and_then(|p| p.capabilities.as_ref())
        .and_then(|c| c.animations.as_ref());
    let mut out = Vec::new();
    for state in declared.into_iter().flatten() {
        let Some(clip) = animations.iter().position(|a| a.name == state.clip) else {
            diags.warn(format!(
                "animation {} names missing clip {:?}",
                state.semantic, state.clip
            ));
            continue;
        };
        let duration = animations[clip].duration;
        let events = state
            .events
            .iter()
            .map(|e| {
                if e.time < 0.0 || e.time > duration + 1e-3 {
                    diags.warn(format!(
                        "animation {}: event {} at {:.3}s is outside the {:.3}s clip",
                        state.semantic, e.name, e.time, duration
                    ));
                }
                AnimationEvent {
                    name: e.name.to_string(),
                    time: if duration > 0.0 {
                        (e.time / duration).clamp(0.0, 1.0) as f32
                    } else {
                        0.0
                    },
                }
            })
            .collect();
        out.push(AnimationState {
            semantic: state.semantic.to_string(),
            clip: clip as u32,
            looping: state.looping,
            nominal_speed: state.nominal_speed.map(|s| s as f32),
            events,
        });
    }
    out
}

fn prototype(source: &ModelSource) -> Prototype {
    let caps = source.published.and_then(|p| p.capabilities.as_ref());
    Prototype {
        dimensions: caps
            .and_then(|c| c.dimensions.as_ref())
            .map(|d| [d.radius as f32, d.height as f32, d.formation_spacing as f32]),
        blocker: caps.and_then(|c| c.blocker.as_ref()).map(|b| Blocker {
            shape: match b.shape {
                models::ModelBlockerShape::Box => BlockerShape::Box,
                models::ModelBlockerShape::Ellipse => BlockerShape::Ellipse,
            },
            width: b.size[0] as f32,
            depth: b.size[1] as f32,
            x: b.offset[0] as f32,
            z: b.offset[1] as f32,
            yaw: 0.0,
        }),
        vegetation_clearance: caps.and_then(|c| c.vegetation_clearance).unwrap_or(0.0) as f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octahedral_normals_round_trip() {
        for n in [
            [0.0, 1.0, 0.0],
            [0.0, 0.0, -1.0],
            [0.3, -0.5, 0.8],
            [-0.7, 0.1, -0.7],
        ] {
            let n = normalize(n);
            let back = octahedral_decode(octahedral(n));
            let dot: f64 = (0..3).map(|k| n[k] * back[k]).sum();
            assert!(dot > 0.99999, "{n:?} → {back:?}");
        }
    }

    #[test]
    fn skin_weights_sum_to_255() {
        let mut diags = Diagnostics::new("test");
        for weights in [
            [1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0, 0.0],
            [0.5, 0.25, 0.125, 0.125],
            [0.7, 0.2, 0.1, 0.0],
        ] {
            let q = quantise_skin([1, 2, 3, 4], weights, &mut diags);
            assert_eq!(q.weights.iter().map(|&w| u32::from(w)).sum::<u32>(), 255);
        }
        assert!(diags.list.is_empty());
    }
}
