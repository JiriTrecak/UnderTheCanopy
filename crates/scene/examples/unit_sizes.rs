//! Each unit's body against its model's footprint as drawn (D102): the definition's radius
//! and the model's rest bounds from above at its drawn scale, as a table.
//!
//! ```text
//! cargo run -p canopy-scene --example unit_sizes -- <data folder> <pack>
//! ```

use canopy_content::{Content, game::Kind};
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let data = PathBuf::from(args.next().unwrap_or_else(|| ".".into()));
    let pack = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "target/cooked/canopy.pack".into()),
    );
    let content = Content::load(&data).unwrap_or_else(|e| panic!("{e}"));
    let pack = canopy_assets::Pack::open(&pack).unwrap_or_else(|e| panic!("{e}"));
    println!("id\thero\tradius\twidth\tdepth\theight");
    for d in content.definitions.iter().filter(|d| d.kind == Kind::Unit) {
        let Some(asset) = content.render_of(d, d.model.as_str()) else {
            continue;
        };
        let Some(file) = &asset.file else {
            continue;
        };
        let Ok(model) = pack.model(file) else {
            continue;
        };
        let b = &model.meta.rest_bounds;
        let scale = asset.scale.unwrap_or(1.0) * d.model_scale.unwrap_or(1.0);
        let size = |i: usize| f64::from(b.max[i] - b.min[i]) * scale;
        println!(
            "{}\t{}\t{}\t{:.2}\t{:.2}\t{:.2}",
            d.id,
            d.is_hero(),
            d.dimensions.as_ref().map_or(0.0, |x| x.radius),
            size(0),
            size(2),
            size(1)
        );
    }
}
