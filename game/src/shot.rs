//! `canopy shot`: renders one view offscreen at a fixed size and writes a PNG, with
//! timings for the captured view.

use crate::{
    options::{CameraSpec, Options},
    paths::Paths,
    view, world,
};
use anyhow::{Context, Result};
use canopy_render::{
    DeviceOptions, Gpu, Renderer,
    capture::{CAPTURE_FORMAT, Capture},
};
use std::path::Path;

pub fn run(options: &Options, paths: &Paths, out: &Path, size: [u32; 2]) -> Result<()> {
    let gpu = Gpu::headless(DeviceOptions::default())?.context("no GPU adapter")?;
    let map = options.map.as_deref().context("shot needs --map")?;
    let loaded = world::load(paths, map, options.hour)?;
    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, size, loaded.environment.clone());
    renderer.shadows = options.shadows;
    let started = std::time::Instant::now();
    renderer.load_world(&gpu, &loaded.pack, &loaded.scene.world)?;
    gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
    let upload_ms = started.elapsed().as_secs_f32() * 1000.0;
    let mut camera = view::initial(options.camera, &loaded.scene);
    let aspect = size[0] as f32 / size[1] as f32;
    let camera_view = view::frame(&mut camera, aspect, &loaded.scene);
    let capture = Capture::new(&gpu.device, size[0], size[1]);

    // Warm-up frames (pipelines, first-use costs), then measured ones.
    let frames = options.frames.max(1);
    let mut reports = Vec::new();
    let copy = |encoder: &mut wgpu::CommandEncoder| capture.copy(encoder);
    for i in 0..frames {
        let hook: Option<&dyn Fn(&mut wgpu::CommandEncoder)> =
            if i + 1 == frames { Some(&copy) } else { None };
        let report = renderer.render(&gpu, &camera_view, options.time, &capture.view, hook);
        gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
        reports.push(report);
    }
    // One more frame lets the last timestamps arrive.
    let pixels = capture.read(&gpu.device)?;
    let late = renderer.render(&gpu, &camera_view, options.time, &capture.view, None);
    gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    image::save_buffer(
        out,
        &pixels,
        size[0],
        size[1],
        image::ExtendedColorType::Rgba8,
    )
    .with_context(|| out.display().to_string())?;

    let spec = CameraSpec {
        x: camera.focus[0],
        z: camera.focus[1],
        zoom: camera.zoom(),
        yaw: camera.yaw.to_degrees(),
    };
    let measured = if reports.len() > 1 {
        &reports[1..]
    } else {
        &reports[..]
    };
    let cpu: f32 = measured.iter().map(|r| r.cpu_total_ms).sum::<f32>() / measured.len() as f32;
    let last = reports.last().expect("a frame");
    println!(
        "{}: {map} at {:.1},{:.1} zoom {:.3} yaw {:.0} ({}×{}), hour {:.1}",
        out.display(),
        spec.x,
        spec.z,
        spec.zoom,
        spec.yaw,
        size[0],
        size[1],
        options.hour.unwrap_or(loaded.scene.hour)
    );
    println!(
        "  gpu {} | world upload {upload_ms:.0} ms | gpu memory {:.1} MiB",
        gpu.name(),
        renderer.gpu_bytes() as f64 / (1024.0 * 1024.0)
    );
    println!(
        "  draws {} | terrain chunks {} triangles {} | statics {:?}",
        last.draws,
        last.terrain_chunks,
        last.terrain_triangles,
        renderer.statics().map(|s| s.stats)
    );
    println!(
        "  cpu record {cpu:.2} ms/frame over {} frames",
        measured.len()
    );
    if let Some(gpu_ms) = &late.gpu_ms {
        let total: f32 = gpu_ms.iter().sum();
        let passes: Vec<String> = late
            .passes
            .iter()
            .zip(gpu_ms)
            .map(|(name, ms)| format!("{name} {ms:.2}"))
            .collect();
        println!("  gpu {total:.2} ms: {}", passes.join(", "));
    }
    Ok(())
}
