//! Moves units that overlap at the start out of each other's way on every map the
//! repository ships (`canopy_sim::spawn::untangle`), writing the maps' `map.json` in place:
//! for camps laid out before their creeps' bodies grew.
//!
//! `cargo run --release -p canopy-sim --example untangle_maps -- --data .`

use canopy_content::{Content, map};
use std::path::PathBuf;

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut data = PathBuf::from(".");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data" => data = PathBuf::from(args.next().ok_or("--data needs a folder")?),
            other => return Err(format!("unknown option {other}")),
        }
    }
    let content = Content::load(&data).map_err(|e| e.to_string())?;
    for file in map::library(&data).map_err(|e| e.to_string())? {
        let mut map = file.load().map_err(|e| e.to_string())?;
        let terrain = canopy_sim::terrain::Terrain::compile(&map, &content)
            .map_err(|e| format!("{}: {e:?}", file.file))?;
        let (moved, stuck) = canopy_sim::spawn::untangle(&mut map, &content, &terrain)?;
        for id in &stuck {
            println!("{}: {id} found no room", file.file);
        }
        if moved.is_empty() {
            continue;
        }
        let path = data.join(&file.file).join("map.json");
        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let mut json: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let entities = json["entities"]
            .as_array_mut()
            .ok_or("map.json without entities")?;
        for placement in &map.entities {
            if !moved.contains(&placement.id) {
                continue;
            }
            if let Some(e) = entities
                .iter_mut()
                .find(|e| e["id"].as_str() == Some(placement.id.as_str()))
            {
                e["position"]["x"] = placement.position.x.into();
                e["position"]["y"] = placement.position.y.into();
            }
        }
        let mut out = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
        out.push('\n');
        std::fs::write(&path, out).map_err(|e| e.to_string())?;
        println!("{}: moved {}", file.file, moved.len());
    }
    Ok(())
}
