//! Effect graph particles on the GPU (D113, docs/effect-graph.md): every compiled layer
//! (`canopy_fx::compile`) is a compute program with a `birth` and an `update` kernel.
//! Instances of layers own ranges of a particle pool and a part buffer; each frame the
//! caller lists the steps to simulate ([`GraphStep`], fixed steps, in order) and the
//! renderers to draw ([`GraphDraw`]).
//!
//! [`GraphCompute`] owns the simulation: the pool, the parts, the step uniforms and the
//! compiled programs. [`GraphFx`] draws its part records in the effects pass, shaded like
//! the other effects (shaders/graph_parts.wgsl); the editor's node previews draw another
//! [`GraphCompute`]'s parts into thumbnails (`crate::graph_thumbs`).

use crate::targets;
use bytemuck::{Pod, Zeroable};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, mpsc};

/// Vec4s of the game's particle pool (16 MiB).
pub const POOL_VEC4S: u32 = 1 << 20;
/// Part records (four vec4s each) of the game's part buffer (4 MiB).
pub const PART_RECORDS: u32 = 1 << 16;
/// Steps simulated per frame (each a birth and an update dispatch).
pub const MAX_STEPS: usize = 1024;
/// Renderer draws per frame.
pub const MAX_DRAWS: usize = 1024;
/// Dynamic uniform offsets are multiples of this.
pub(crate) const UNIFORM_STRIDE: u64 = 256;
const WORKGROUP: u32 = 64;

/// One step of one layer instance. Mirrors `Layer` in crates/fx/src/wgsl/prelude.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Pod, Zeroable)]
pub struct GraphLayerUniform {
    pub effect_time: f32,
    pub dt: f32,
    pub seed: u32,
    pub base: u32,
    pub capacity: u32,
    pub stride: u32,
    pub spawn_first: u32,
    pub spawn_count: u32,
    pub spawn_start: f32,
    pub spawn_spacing: f32,
    pub render_base: u32,
    pub renderers: u32,
    /// This layer's event records, in pool vec4s (its Emit event nodes write them).
    pub events_out: u32,
    /// An event-born layer's parent's records, the parent channel and the child slots
    /// each parent particle owns; its `spawn_count` is the parent's capacity.
    pub events_in: u32,
    pub channel: u32,
    pub per_parent: u32,
    /// 1 while the effect plays.
    pub running: u32,
    /// An event-born layer's parent instance: its records and their stride.
    pub parent_base: u32,
    pub parent_stride: u32,
    pub pad: u32,
    pub emitter: [[f32; 4]; 4],
    pub params: [[f32; 4]; 2],
    /// The cue's caster and target ends in the emitter's space (w 1 when known).
    pub source: [f32; 4],
    pub target: [f32; 4],
    /// The owner's team colour, linear rgb and alpha 1.
    pub team: [f32; 4],
    /// Where the emitter has travelled to in its own space (a missile's head; w 0).
    pub carrier: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<GraphLayerUniform>() == 240);

/// A step to simulate: `program` (an index into [`GraphCompute::set_programs`]'s list) with
/// its uniform; births run first when `spawn_count` is above 0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GraphStep {
    pub program: u32,
    pub layer: GraphLayerUniform,
}

/// How a renderer's billboards face. Mirrors `MODE_*` in graph_parts.wgsl.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphMode {
    Camera = 0,
    Axis = 1,
    Spheroid = 2,
    Plane = 3,
    /// Each particle joined to the next of its chain (a strip segment).
    Ribbon = 4,
}

/// One renderer of one instance: its part records drawn as billboards.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GraphDraw {
    /// The instance's cue (the caller's key for it).
    pub cue: u32,
    /// Index into the draw's textures ([`crate::effects::EffectsGpu::load_textures`]).
    pub texture: Option<u32>,
    pub mode: GraphMode,
    pub additive: bool,
    /// Bends the scene behind (drawn first, from the copy of the scene).
    pub distort: bool,
    pub soft: f32,
    pub flipbook: [u32; 2],
    /// The instance's first part record in vec4s (four per record), this renderer's
    /// index among its renderers, renderers per particle, and particles (the instance's
    /// capacity).
    pub render_base: u32,
    pub renderer: u32,
    pub renderers: u32,
    pub particles: u32,
}

/// Ranges to zero before simulating (a new or restarted instance): vec4 offsets and
/// lengths in the pool, and part records.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GraphFrame {
    pub clear_pool: Vec<(u32, u32)>,
    pub clear_parts: Vec<(u32, u32)>,
    pub steps: Vec<GraphStep>,
    pub draws: Vec<GraphDraw>,
}

impl GraphFrame {
    pub fn clear(&mut self) {
        self.clear_pool.clear();
        self.clear_parts.clear();
        self.steps.clear();
        self.draws.clear();
    }
}

/// A draw's uniform. Mirrors `GraphDraw` in graph_parts.wgsl and graph_thumbs.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, Pod, Zeroable)]
pub(crate) struct DrawUniform {
    ints: [u32; 4],
    flip: [u32; 4],
    look: [f32; 4],
}

impl DrawUniform {
    /// Overrides the mode (the thumbnails' ground disc).
    pub(crate) fn set_mode(&mut self, mode: u32) {
        self.ints[3] = mode;
    }

    pub(crate) fn of(d: &GraphDraw) -> Self {
        Self {
            ints: [d.render_base, d.renderer, d.renderers.max(1), d.mode as u32],
            flip: [
                d.flipbook[0],
                d.flipbook[1],
                u32::from(d.texture.is_some()),
                0,
            ],
            look: [
                if d.additive { 1.0 } else { 0.0 },
                d.soft,
                if d.distort { 1.0 } else { 0.0 },
                0.0,
            ],
        }
    }
}

/// Uniforms at [`UNIFORM_STRIDE`] offsets, one per item.
pub(crate) fn strided<T: Pod>(items: impl ExactSizeIterator<Item = T>) -> Vec<u8> {
    let mut bytes = vec![0u8; items.len() * UNIFORM_STRIDE as usize];
    for (i, item) in items.enumerate() {
        let at = i * UNIFORM_STRIDE as usize;
        let raw = bytemuck::bytes_of(&item);
        bytes[at..at + raw.len()].copy_from_slice(raw);
    }
    bytes
}

/// Premultiplied with the part's additivity, as every effect (effects.rs).
pub(crate) const BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

pub(crate) fn storage(read_only: bool) -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only },
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

pub(crate) fn dynamic_uniform(size: u64) -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: true,
        min_binding_size: wgpu::BufferSize::new(size),
    }
}

pub(crate) fn binding(buffer: &wgpu::Buffer, size: Option<u64>) -> wgpu::BindingResource<'_> {
    wgpu::BindingResource::Buffer(wgpu::BufferBinding {
        buffer,
        offset: 0,
        size: size.and_then(wgpu::BufferSize::new),
    })
}

pub(crate) fn buffer(
    device: &wgpu::Device,
    label: &str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

struct Program {
    birth: wgpu::ComputePipeline,
    update: wgpu::ComputePipeline,
}

/// A program compiled in the background, by the hash of its WGSL.
type Arrival = (u64, Program);

/// Builds `todo` across `threads` threads.
fn build_all(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    todo: Vec<(u64, Arc<str>)>,
    threads: usize,
) -> Vec<(u64, Program)> {
    if todo.is_empty() {
        return Vec::new();
    }
    std::thread::scope(|scope| {
        let workers: Vec<_> = todo
            .chunks(todo.len().div_ceil(threads.max(1)))
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|(key, source)| (*key, build(device, layout, source)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().expect("a program builds"))
            .collect()
    })
}

/// Compiles effect graph programs on any thread; they reach the renderer's cache with
/// its next use of programs (see [`GraphCompute::compiler`]).
#[derive(Clone)]
pub struct ProgramCompiler {
    device: wgpu::Device,
    layout: wgpu::PipelineLayout,
    known: Arc<Mutex<BTreeSet<u64>>>,
    arrivals: mpsc::Sender<Arrival>,
}

impl ProgramCompiler {
    /// Compiles the `wgsl` programs not compiled (or compiling) yet across the CPU's
    /// cores, on this thread. Returns how many it compiled.
    pub fn compile(&self, wgsl: &[&str]) -> usize {
        let todo: Vec<(u64, Arc<str>)> = {
            let mut known = self.known.lock().unwrap_or_else(|e| e.into_inner());
            wgsl.iter()
                .filter_map(|w| {
                    let key = hash(w);
                    known.insert(key).then(|| (key, Arc::from(*w)))
                })
                .collect()
        };
        if todo.is_empty() {
            return 0;
        }
        let _span = tracing::info_span!("graph_fx_compile_ahead", programs = todo.len()).entered();
        let started = std::time::Instant::now();
        let threads = std::thread::available_parallelism()
            .map_or(4, |n| n.get())
            .min(todo.len());
        let count = todo.len();
        for arrival in build_all(&self.device, &self.layout, todo, threads) {
            let _ = self.arrivals.send(arrival);
        }
        tracing::info!(
            programs = count,
            threads,
            ms = started.elapsed().as_millis() as u64,
            "effect programs compiled while loading"
        );
        count
    }
}

/// A layer's birth and update kernels from its WGSL.
fn build(device: &wgpu::Device, layout: &wgpu::PipelineLayout, source: &str) -> Program {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("graph fx layer"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline = |entry: &str| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: Some(layout),
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    Program {
        birth: pipeline("birth"),
        update: pipeline("update"),
    }
}

fn hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// The simulation: a particle pool, a part buffer, the steps' uniforms and the compiled
/// layer programs.
pub struct GraphCompute {
    pipeline_layout: wgpu::PipelineLayout,
    pool: wgpu::Buffer,
    pool_vec4s: u32,
    parts: wgpu::Buffer,
    part_records: u32,
    layers: wgpu::Buffer,
    group: wgpu::BindGroup,
    /// Programs by the hash of their WGSL, so the same layer compiles once.
    cache: BTreeMap<u64, Arc<Program>>,
    /// Every program's WGSL and its hash; each compiles the first time a step runs it (a
    /// table of thousands of layers must not compile them all up front).
    sources: Vec<(u64, Arc<str>)>,
    programs: Vec<Option<Arc<Program>>>,
    /// Programs compiled here or by a [`ProgramCompiler`] (those not in `cache` yet are
    /// still compiling, or on their way), by hash, and the channel compiled programs
    /// arrive on.
    known: Arc<Mutex<BTreeSet<u64>>>,
    arrivals: (mpsc::Sender<Arrival>, mpsc::Receiver<Arrival>),
    frame: GraphFrame,
}

impl GraphCompute {
    pub fn new(device: &wgpu::Device, pool_vec4s: u32, part_records: u32) -> Self {
        let compute = wgpu::ShaderStages::COMPUTE;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("graph fx compute"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: compute,
                    ty: dynamic_uniform(std::mem::size_of::<GraphLayerUniform>() as u64),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: compute,
                    ty: storage(false),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: compute,
                    ty: storage(false),
                    count: None,
                },
            ],
        });
        let pool = buffer(
            device,
            "graph fx pool",
            u64::from(pool_vec4s) * 16,
            wgpu::BufferUsages::STORAGE,
        );
        let parts = buffer(
            device,
            "graph fx parts",
            u64::from(part_records) * 64,
            wgpu::BufferUsages::STORAGE,
        );
        let layers = buffer(
            device,
            "graph fx layers",
            MAX_STEPS as u64 * UNIFORM_STRIDE,
            wgpu::BufferUsages::UNIFORM,
        );
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("graph fx compute"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: binding(
                        &layers,
                        Some(std::mem::size_of::<GraphLayerUniform>() as u64),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: binding(&pool, None),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: binding(&parts, None),
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("graph fx compute"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        Self {
            pipeline_layout,
            pool,
            pool_vec4s,
            parts,
            part_records,
            layers,
            group,
            cache: BTreeMap::new(),
            sources: Vec::new(),
            programs: Vec::new(),
            known: Arc::default(),
            arrivals: mpsc::channel(),
            frame: GraphFrame::default(),
        }
    }

    /// The part records the draws read.
    pub fn parts(&self) -> &wgpu::Buffer {
        &self.parts
    }

    /// The compiled layers steps refer to by position. Programs seen before are reused.
    /// The programs steps refer to by index; each compiles when a step first runs it.
    pub fn set_programs(&mut self, wgsl: &[&str]) {
        self.receive(None);
        self.sources = wgsl.iter().map(|w| (hash(w), Arc::from(*w))).collect();
        self.programs = self
            .sources
            .iter()
            .map(|(key, _)| self.cache.get(key).cloned())
            .collect();
    }

    /// What compiles programs on other threads: a match's loader compiles the programs
    /// its effects run while the loading screen shows.
    pub fn compiler(&self, device: &wgpu::Device) -> ProgramCompiler {
        ProgramCompiler {
            device: device.clone(),
            layout: self.pipeline_layout.clone(),
            known: self.known.clone(),
            arrivals: self.arrivals.0.clone(),
        }
    }

    fn known(&self) -> std::sync::MutexGuard<'_, BTreeSet<u64>> {
        self.known.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Takes the programs compilers sent; with `wait`, first blocks until that program
    /// (when one is compiling it) has arrived.
    fn receive(&mut self, wait: Option<u64>) {
        while let Ok((k, program)) = self.arrivals.1.try_recv() {
            self.arrive(k, program);
        }
        let Some(key) = wait.filter(|k| !self.cache.contains_key(k) && self.known().contains(k))
        else {
            return;
        };
        let _span = tracing::info_span!("graph_fx_wait").entered();
        while !self.cache.contains_key(&key) {
            // The sender half kept here never closes the channel, so a compiling thread
            // that died would block this forever: bounded waits instead.
            match self
                .arrivals
                .1
                .recv_timeout(std::time::Duration::from_secs(10))
            {
                Ok((k, program)) => self.arrive(k, program),
                Err(_) => {
                    self.known().remove(&key);
                    break;
                }
            }
        }
    }

    fn arrive(&mut self, key: u64, program: Program) {
        self.cache.entry(key).or_insert_with(|| Arc::new(program));
    }

    /// Compiles program `index` if no step has run it yet.
    fn compile(&mut self, device: &wgpu::Device, index: usize) {
        if self.programs[index].is_some() {
            return;
        }
        let (key, source) = self.sources[index].clone();
        self.receive(Some(key));
        let layout = &self.pipeline_layout;
        let program = match self.cache.get(&key) {
            Some(program) => program.clone(),
            None => {
                let _span = tracing::info_span!("graph_fx_compile").entered();
                self.known
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(key);
                let program = Arc::new(build(device, layout, &source));
                self.cache.insert(key, program.clone());
                program
            }
        };
        self.programs[index] = Some(program);
    }

    /// Compiles programs `indices` now, across the CPU's cores, so the effects that run
    /// them start without a stall (a match's spells, at load); the rest compile when a
    /// step first runs them. Programs a compiler is compiling are waited for.
    pub fn prewarm(&mut self, device: &wgpu::Device, indices: &[u32]) {
        let _span = tracing::info_span!("graph_fx_prewarm", programs = indices.len()).entered();
        let started = std::time::Instant::now();
        self.receive(None);
        let mut todo: Vec<(u64, Arc<str>)> = Vec::new();
        let mut waits = Vec::new();
        {
            let mut known = self.known.lock().unwrap_or_else(|e| e.into_inner());
            for &i in indices {
                let Some((key, source)) = self.sources.get(i as usize) else {
                    continue;
                };
                if self.cache.contains_key(key) {
                    continue;
                }
                if known.insert(*key) {
                    todo.push((*key, source.clone()));
                } else if !todo.iter().any(|(k, _)| k == key) {
                    waits.push(*key);
                }
            }
        }
        let threads = std::thread::available_parallelism()
            .map_or(4, |n| n.get())
            .min(todo.len().max(1));
        let compiled = todo.len();
        for (key, program) in build_all(device, &self.pipeline_layout, todo, threads) {
            self.cache.insert(key, Arc::new(program));
        }
        for key in &waits {
            self.receive(Some(*key));
        }
        tracing::info!(
            programs = compiled,
            waited = waits.len(),
            threads,
            ms = started.elapsed().as_millis() as u64,
            "effect programs compiled ahead"
        );
        for &i in indices {
            let i = i as usize;
            if let Some((key, _)) = self.sources.get(i)
                && self.programs[i].is_none()
            {
                self.programs[i] = self.cache.get(key).cloned();
            }
        }
    }

    /// Takes the frame's clears and steps (clipped to the budget) and uploads the steps'
    /// uniforms.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &impl crate::upload::Writes,
        frame: &GraphFrame,
    ) {
        self.frame.clear_pool.clone_from(&frame.clear_pool);
        self.frame.clear_parts.clone_from(&frame.clear_parts);
        self.frame.steps.clone_from(&frame.steps);
        self.frame.steps.truncate(MAX_STEPS);
        let programs = self.programs.len();
        self.frame.steps.retain(|s| (s.program as usize) < programs);
        for i in 0..self.frame.steps.len() {
            let program = self.frame.steps[i].program as usize;
            self.compile(device, program);
        }
        let bytes = strided(self.frame.steps.iter().map(|s| s.layer));
        if !bytes.is_empty() {
            queue.write_buffer(&self.layers, 0, &bytes);
        }
    }

    /// Zeroes new instances' ranges and runs the prepared steps, in order.
    pub fn simulate(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.frame.steps.is_empty() && self.frame.clear_pool.is_empty() {
            return;
        }
        for &(first, len) in &self.frame.clear_pool {
            let end = (first + len).min(self.pool_vec4s);
            if first < end {
                encoder.clear_buffer(
                    &self.pool,
                    u64::from(first) * 16,
                    Some(u64::from(end - first) * 16),
                );
            }
        }
        for &(first, len) in &self.frame.clear_parts {
            let end = (first + len).min(self.part_records);
            if first < end {
                encoder.clear_buffer(
                    &self.parts,
                    u64::from(first) * 64,
                    Some(u64::from(end - first) * 64),
                );
            }
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("graph fx"),
            timestamp_writes: None,
        });
        for (i, step) in self.frame.steps.iter().enumerate() {
            let Some(program) = &self.programs[step.program as usize] else {
                continue;
            };
            pass.set_bind_group(0, &self.group, &[(i as u64 * UNIFORM_STRIDE) as u32]);
            if step.layer.spawn_count > 0 {
                pass.set_pipeline(&program.birth);
                pass.dispatch_workgroups(step.layer.spawn_count.div_ceil(WORKGROUP), 1, 1);
            }
            pass.set_pipeline(&program.update);
            pass.dispatch_workgroups(step.layer.capacity.div_ceil(WORKGROUP), 1, 1);
        }
    }
}

/// The game's effect graph particles: simulated before the effects pass, drawn in it.
pub struct GraphFx {
    compute: GraphCompute,
    draw_pipeline: wgpu::RenderPipeline,
    draw_uniforms: wgpu::Buffer,
    draw_group: wgpu::BindGroup,
    draws: Vec<GraphDraw>,
}

impl GraphFx {
    /// `texture_layout` and `depth_layout` are the effects pass's (groups 1 and 3); the
    /// draws reuse its textures and the scene's linear depth. `view` is its camera
    /// right/up uniform.
    pub fn new(
        device: &wgpu::Device,
        frame_layout: &wgpu::BindGroupLayout,
        texture_layout: &wgpu::BindGroupLayout,
        depth_layout: &wgpu::BindGroupLayout,
        view: &wgpu::Buffer,
    ) -> Self {
        let compute = GraphCompute::new(device, POOL_VEC4S, PART_RECORDS);
        let vertex_fragment = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("graph fx draw"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: storage(true),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: vertex_fragment,
                    ty: dynamic_uniform(std::mem::size_of::<DrawUniform>() as u64),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let draw_uniforms = buffer(
            device,
            "graph fx draws",
            MAX_DRAWS as u64 * UNIFORM_STRIDE,
            wgpu::BufferUsages::UNIFORM,
        );
        let draw_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("graph fx draw"),
            layout: &draw_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: binding(compute.parts(), None),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: binding(
                        &draw_uniforms,
                        Some(std::mem::size_of::<DrawUniform>() as u64),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: binding(view, None),
                },
            ],
        });
        let module = crate::shader::module(
            device,
            "graph_parts",
            &[crate::shader::Module::Frame],
            include_str!("../shaders/graph_parts.wgsl"),
        );
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("graph fx draw"),
            bind_group_layouts: &[
                Some(frame_layout),
                Some(texture_layout),
                Some(&draw_layout),
                Some(depth_layout),
            ],
            immediate_size: 0,
        });
        let draw_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("graph fx"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_graph"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_graph"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: targets::HDR_FORMAT,
                    blend: Some(BLEND),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            compute,
            draw_pipeline,
            draw_uniforms,
            draw_group,
            draws: Vec::new(),
        }
    }

    /// The compiled layers steps refer to by position.
    pub fn set_programs(&mut self, wgsl: &[&str]) {
        self.compute.set_programs(wgsl);
    }

    /// Compiles programs `indices` now (see [`GraphCompute::prewarm`]).
    pub fn prewarm(&mut self, device: &wgpu::Device, indices: &[u32]) {
        self.compute.prewarm(device, indices);
    }

    /// What compiles programs on other threads (see [`GraphCompute::compiler`]).
    pub fn compiler(&self, device: &wgpu::Device) -> ProgramCompiler {
        self.compute.compiler(device)
    }

    /// Uploads the frame's step and draw uniforms (clipped to the budgets), compiling the
    /// programs its steps run for the first time.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &impl crate::upload::Writes,
        frame: &GraphFrame,
    ) {
        self.compute.prepare(device, queue, frame);
        self.draws.clone_from(&frame.draws);
        self.draws.truncate(MAX_DRAWS);
        let bytes = strided(self.draws.iter().map(DrawUniform::of));
        if !bytes.is_empty() {
            queue.write_buffer(&self.draw_uniforms, 0, &bytes);
        }
    }

    pub fn has_draws(&self) -> bool {
        !self.draws.is_empty()
    }

    /// Zeroes new instances' ranges and runs the frame's steps, in order.
    pub fn simulate(&self, encoder: &mut wgpu::CommandEncoder) {
        self.compute.simulate(encoder);
    }

    /// Draws the renderers in the effects pass; `textures` gives an effect texture's bind
    /// group (white when `None`). Returns the draw calls.
    /// Whether a renderer bends the scene this frame (the scene is copied for it first).
    pub fn has_distortion(&self) -> bool {
        self.draws.iter().any(|d| d.distort)
    }

    /// Draws the renderers that bend the scene (`distort`), or all the others.
    pub fn draw<'t>(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        distort: bool,
        textures: impl Fn(Option<u32>) -> &'t wgpu::BindGroup,
    ) -> u32 {
        if !self.draws.iter().any(|d| d.distort == distort) {
            return 0;
        }
        pass.set_pipeline(&self.draw_pipeline);
        let mut drawn = 0;
        for (i, d) in self.draws.iter().enumerate() {
            if d.distort != distort {
                continue;
            }
            drawn += 1;
            pass.set_bind_group(1, textures(d.texture), &[]);
            pass.set_bind_group(2, &self.draw_group, &[(i as u64 * UNIFORM_STRIDE) as u32]);
            pass.draw(0..6, 0..d.particles);
        }
        drawn
    }
}
