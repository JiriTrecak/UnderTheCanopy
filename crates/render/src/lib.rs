//! `canopy-render`: the wgpu renderer (docs/renderer.md). It knows nothing about the
//! simulation: callers describe the world with plain data and drive a camera.

pub mod camera;
pub mod capture;
pub mod device;
pub mod environment;
pub mod frame;
pub mod graph;
pub mod renderer;
pub mod shader;
pub mod shadow;
pub mod statics;
pub mod targets;
pub mod terrain;
pub mod texture;
pub mod timing;

pub use camera::{CameraView, RtsCamera};
pub use device::{DeviceOptions, Gpu, GpuFeatures};
pub use renderer::{Environment, FrameReport, Renderer, World};
