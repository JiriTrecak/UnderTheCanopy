//! Draws a fixture UI through the reference wgpu pass. Skips without a GPU adapter.

mod common;

use canopy_ui::gpu::{UiRenderer, headless_device, render_to_rgba, upload_rgba};
use canopy_ui::{ImageDesc, TextureId};

#[test]
fn reference_pass_draws_boxes_images_and_text() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut ui = common::ui();
    let mut renderer = UiRenderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
    // A 2 × 2 red image on the map node.
    let view = upload_rgba(&device, &queue, "red", 2, 2, &[255, 0, 0, 255].repeat(4));
    renderer.set_texture(&device, TextureId(7), &view);
    let red = ui.register_image("red", ImageDesc::new(TextureId(7), 2.0, 2.0));
    let map = ui.find("test/map").unwrap();
    ui.set_image(map, Some(red));
    // The fixture already drew a frame before this renderer existed.
    ui.reset_glyph_atlas();
    renderer.prepare(&device, &queue, ui.frame(0.0));
    let list = ui.frame(0.1).clone();
    let pixels = render_to_rgba(&device, &queue, &mut renderer, &list, [0, 0, 64, 255]);
    let at = |x: u32, y: u32| {
        let i = ((y * 1280 + x) * 4) as usize;
        [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
    };
    assert_eq!(
        at(5, 5),
        [0, 0, 64, 255],
        "clear colour where nothing is drawn"
    );
    assert_eq!(
        at(150, 100),
        [0x40, 0x40, 0x40, 255],
        "grid cell background"
    );
    assert_eq!(at(950, 100), [255, 0, 0, 255], "image");
    // The label's glyphs: some bright pixels inside its box.
    let r = ui.rect(ui.find("test/label").unwrap()).unwrap();
    let bright = (r.y as u32..r.bottom() as u32)
        .flat_map(|y| (r.x as u32..r.right() as u32).map(move |x| (x, y)))
        .filter(|&(x, y)| at(x, y)[0] > 200)
        .count();
    assert!(bright > 10, "text drawn ({bright} bright pixels)");
}

#[test]
fn srgb_views_keep_colours() {
    use wgpu::util::DeviceExt;
    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut ui = common::ui();
    ui.reset_glyph_atlas();
    let mut renderer = UiRenderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
    // As the asset pack's colour textures: sRGB format, sampled linear.
    let texture = device.create_texture_with_data(
        &queue,
        &wgpu::TextureDescriptor {
            label: Some("grey"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &[128, 64, 200, 255],
    );
    renderer.set_texture(
        &device,
        TextureId(3),
        &texture.create_view(&Default::default()),
    );
    let mut desc = ImageDesc::new(TextureId(3), 1.0, 1.0);
    desc.srgb_view = true;
    let grey = ui.register_image("grey", desc);
    let map = ui.find("test/map").unwrap();
    ui.set_image(map, Some(grey));
    let list = ui.frame(0.1).clone();
    let pixels = render_to_rgba(&device, &queue, &mut renderer, &list, [0, 0, 0, 255]);
    let i = ((100 * 1280 + 950) * 4) as usize;
    for (got, want) in pixels[i..i + 3].iter().zip([128u8, 64, 200]) {
        assert!(got.abs_diff(want) <= 1, "{:?}", &pixels[i..i + 4]);
    }
}
