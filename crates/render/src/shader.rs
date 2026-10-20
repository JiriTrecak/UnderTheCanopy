//! The shared WGSL library and shader assembly (renderer §4.3).
//!
//! WGSL has no includes, so a shader is its library modules followed by its own source.
//! Every material uses the same modules: one lighting model, one team-colour formula,
//! one wind, one dither. Nothing patches a shader at runtime.

/// A library module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Module {
    /// Group 0: the frame uniform, shadow map, canopy pattern, samplers, point lights.
    Frame,
    Color,
    Packing,
    Dither,
    Team,
    Wind,
    /// `surface_light` and its parts; needs `Frame` and `Dither`.
    Lighting,
    /// Cooked model materials (group 1) and `shade_material`; needs everything above.
    Material,
    /// Group 1 of post passes: the view uniform, the resolved linear depth, surface
    /// reconstruction and the full-screen vertex shader.
    View,
}

impl Module {
    pub fn source(self) -> &'static str {
        match self {
            Self::Frame => include_str!("../shaders/lib/frame.wgsl"),
            Self::Color => include_str!("../shaders/lib/color.wgsl"),
            Self::Packing => include_str!("../shaders/lib/packing.wgsl"),
            Self::Dither => include_str!("../shaders/lib/dither.wgsl"),
            Self::Team => include_str!("../shaders/lib/team.wgsl"),
            Self::Wind => include_str!("../shaders/lib/wind.wgsl"),
            Self::Lighting => include_str!("../shaders/lib/lighting.wgsl"),
            Self::Material => include_str!("../shaders/lib/material.wgsl"),
            Self::View => include_str!("../shaders/lib/view.wgsl"),
        }
    }
}

/// The modules every lit material uses.
pub const LIT: &[Module] = &[
    Module::Frame,
    Module::Color,
    Module::Packing,
    Module::Dither,
    Module::Team,
    Module::Wind,
    Module::Lighting,
];

/// The modules every model pass (statics, actors) uses.
pub const MODEL: &[Module] = &[
    Module::Frame,
    Module::Color,
    Module::Packing,
    Module::Dither,
    Module::Team,
    Module::Wind,
    Module::Lighting,
    Module::Material,
];

/// The modules of the final pass (lit helpers for the canopy shade, and the view).
pub const POST: &[Module] = &[
    Module::Frame,
    Module::Color,
    Module::Packing,
    Module::Dither,
    Module::Team,
    Module::Wind,
    Module::Lighting,
    Module::View,
];

/// The modules of the haze passes.
pub const HAZE: &[Module] = &[Module::Frame, Module::Dither, Module::View];

/// Library modules followed by a shader's own source.
pub fn assemble(modules: &[Module], body: &str) -> String {
    let mut source = String::new();
    for module in modules {
        source.push_str(module.source());
        source.push('\n');
    }
    source.push_str(body);
    source
}

pub fn module(
    device: &wgpu::Device,
    label: &str,
    modules: &[Module],
    body: &str,
) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(assemble(modules, body).into()),
    })
}

/// Every shader of the renderer with the modules it is assembled from.
pub fn all() -> Vec<(&'static str, &'static [Module], &'static str)> {
    vec![
        ("terrain", LIT, include_str!("../shaders/terrain.wgsl")),
        ("statics", MODEL, include_str!("../shaders/statics.wgsl")),
        ("water", LIT, include_str!("../shaders/water.wgsl")),
        ("floor", LIT, include_str!("../shaders/floor.wgsl")),
        ("actors", MODEL, include_str!("../shaders/actors.wgsl")),
        (
            "effects",
            &[Module::Frame],
            include_str!("../shaders/effects.wgsl"),
        ),
        ("minimap", &[], include_str!("../shaders/minimap.wgsl")),
        ("portrait", &[], include_str!("../shaders/portrait.wgsl")),
        (
            "world_ui",
            &[Module::Frame, Module::Color],
            include_str!("../shaders/world_ui.wgsl"),
        ),
        ("skin", &[], include_str!("../shaders/skin.wgsl")),
        ("cull", &[], include_str!("../shaders/cull.wgsl")),
        ("ripples", &[], include_str!("../shaders/ripples.wgsl")),
        (
            "caustics",
            &[Module::Color],
            include_str!("../shaders/caustics.wgsl"),
        ),
        ("post", POST, include_str!("../shaders/post.wgsl")),
        (
            "contact",
            &[Module::View],
            include_str!("../shaders/contact.wgsl"),
        ),
        (
            "bloom",
            &[Module::View],
            include_str!("../shaders/bloom.wgsl"),
        ),
        ("haze", HAZE, include_str!("../shaders/haze.wgsl")),
        (
            "surroundings",
            LIT,
            include_str!("../shaders/surroundings.wgsl"),
        ),
        ("motes", LIT, include_str!("../shaders/motes.wgsl")),
        ("blockout", LIT, include_str!("../shaders/blockout.wgsl")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every shader parses and validates with naga, without a GPU.
    #[test]
    fn every_shader_validates() {
        for (name, modules, body) in all() {
            let source = assemble(modules, body);
            let module = naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(&source)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        }
    }
}
