//! The GPU device, created inside the WebGPU baseline (D2).
//!
//! Limits are WebGPU's defaults. Features are WebGPU's own optional features, each used
//! only when the adapter offers it and with a fallback:
//!
//! | Feature | Used for | Without it |
//! |---|---|---|
//! | `texture-compression-bc` (D26) | BC7 textures from the pack | decoded to RGBA8 at load |
//! | `timestamp-query` | GPU time per frame-graph pass | CPU times only |
//!
//! Nothing native-only is requested.

use anyhow::{Context, Result};

/// The optional features the device was created with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GpuFeatures {
    /// BC7 textures upload as stored.
    pub bc: bool,
    /// Pass timestamps are available.
    pub timestamps: bool,
}

/// What the caller wants from the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceOptions {
    /// Ask for timestamp queries when the adapter offers them.
    pub timestamps: bool,
    /// Ignore BC support and decode on the CPU (tests the fallback).
    pub force_uncompressed: bool,
}

impl Default for DeviceOptions {
    fn default() -> Self {
        Self {
            timestamps: true,
            force_uncompressed: false,
        }
    }
}

/// Device, queue and what they can do.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub features: GpuFeatures,
}

impl Gpu {
    /// A device that can present to `surface` (created from `instance`).
    pub fn for_surface(
        instance: wgpu::Instance,
        surface: &wgpu::Surface<'_>,
        options: DeviceOptions,
    ) -> Result<Self> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .context("no compatible GPU")?;
        Self::from_adapter(instance, adapter, options)
    }

    /// A device without a surface, for offscreen captures and tests. `Ok(None)` when the
    /// machine has no adapter (some CI runners).
    pub fn headless(options: DeviceOptions) -> Result<Option<Self>> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::PRIMARY;
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }));
        match adapter {
            Ok(adapter) => Self::from_adapter(instance, adapter, options).map(Some),
            Err(error) => {
                tracing::warn!(%error, "no GPU adapter");
                Ok(None)
            }
        }
    }

    fn from_adapter(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        options: DeviceOptions,
    ) -> Result<Self> {
        let info = adapter.get_info();
        let offered = adapter.features();
        let mut required = wgpu::Features::empty();
        let features = GpuFeatures {
            bc: !options.force_uncompressed
                && offered.contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
            timestamps: options.timestamps && offered.contains(wgpu::Features::TIMESTAMP_QUERY),
        };
        if features.bc {
            required |= wgpu::Features::TEXTURE_COMPRESSION_BC;
        }
        if features.timestamps {
            required |= wgpu::Features::TIMESTAMP_QUERY;
        }
        tracing::info!(gpu = %info.name, backend = ?info.backend, ?features, "adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("canopy"),
            required_features: required,
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            ..Default::default()
        }))
        .context("cannot create the GPU device")?;
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            features,
        })
    }

    /// The adapter's name, for reports.
    /// The graphics preset for this adapter on first run ([`crate::Preset::for_adapter`]).
    pub fn preset(&self) -> crate::Preset {
        let info = self.adapter.get_info();
        crate::Preset::for_adapter(
            &info.name,
            info.device_type == wgpu::DeviceType::DiscreteGpu,
        )
    }

    pub fn name(&self) -> String {
        let info = self.adapter.get_info();
        format!("{} ({:?})", info.name, info.backend)
    }
}
