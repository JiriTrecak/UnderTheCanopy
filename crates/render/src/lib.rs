//! `canopy-render`: the wgpu renderer (docs/renderer.md). It knows nothing about the
//! simulation: callers describe the world with plain data and drive a camera.

pub mod actors;
pub mod camera;
pub mod capture;
pub mod device;
pub mod effects;
pub mod environment;
pub mod fog;
pub mod frame;
pub mod graph;
pub mod material;
pub mod post;
pub mod quality;
pub mod renderer;
pub mod shader;
pub mod shadow;
pub mod statics;
pub mod surroundings;
pub mod targets;
pub mod terrain;
pub mod texture;
pub mod timing;
pub mod water;
pub mod world_ui;

pub use camera::{CameraView, RtsCamera};
pub use device::{DeviceOptions, Gpu, GpuFeatures};
pub use quality::{HazeQuality, Quality};
pub use renderer::{Environment, FrameReport, Renderer, World};
