//! The repository's `config/` loads and validates, and every biome a map may name has
//! a configuration.

use canopy_render::environment::{BIOMES_FOLDER, BiomeConfig, LightingConfig};
use std::path::Path;

fn data() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

#[test]
fn every_config_file_validates() {
    let lighting = LightingConfig::load(data()).unwrap();
    let folder = data().join(BIOMES_FOLDER);
    let mut biomes = 0;
    for entry in std::fs::read_dir(&folder).unwrap() {
        let path = entry.unwrap().path();
        let id = path.file_stem().unwrap().to_str().unwrap().to_owned();
        BiomeConfig::load(data(), &id, &lighting).unwrap();
        biomes += 1;
    }
    assert!(biomes > 0);
    canopy_scene::Players::load(data()).unwrap();
    canopy_scene::Glows::load(data()).unwrap();
    for biome in canopy_content::map::BIOMES {
        assert!(
            folder.join(format!("{biome}.json")).is_file(),
            "maps may name biome {biome}, which has no configuration"
        );
    }
}
