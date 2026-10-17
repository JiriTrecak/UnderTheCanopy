//! Importing Warcraft III melee maps (docs/editor.md, "Warcraft maps"): the archive
//! (`mpq`), its records (`read`), and their conversion into a map package with the
//! ground, trees and camps this game has (`convert`).

pub mod convert;
mod explode;
pub mod mpq;
pub mod read;

use canopy_content::Content;
use std::path::Path;

/// Imports the Warcraft III map `source` as the map package `maps/<folder>/<id>.utcm` in
/// the package at `package_root`, with its preview; `data` is the repository (the import
/// settings). Returns what it did; the map's problems, if the check finds any, are among
/// the warnings, to fix in the map editor.
pub fn import(
    source: &Path,
    id: &str,
    folder: &str,
    package_root: &Path,
    data: &Path,
    content: &std::sync::Arc<Content>,
) -> Result<convert::Report, String> {
    canopy_content::Id::new(id)?;
    if id.contains('.') {
        return Err("a map id has no dots".into());
    }
    let folder = folder.trim_matches('/');
    let dir = package_root
        .join("maps")
        .join(folder)
        .join(format!("{id}.utcm"));
    if dir.exists() {
        return Err(format!("maps/{folder}/{id}.utcm already exists"));
    }
    let bytes = std::fs::read(source).map_err(|e| format!("{}: {e}", source.display()))?;
    let records = read::read(&bytes)?;
    let settings = convert::Settings::load(data)?;
    let fallback = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(id)
        .trim_start_matches(|c: char| c == '(' || c.is_ascii_digit() || c == ')')
        .to_string();
    let (mut map, mut report) = convert::convert(&records, content, &settings, &fallback)?;
    for gone in convert::drop_unreachable(&mut map, content)? {
        report.warnings.push(format!("Left out {gone}."));
    }
    // The checks the editor runs on every map.
    for error in canopy_content::map::validate(&map, content) {
        report
            .warnings
            .push(format!("{}: {}", error.location, error.message));
    }
    let setup = canopy_scene::spectator_setup(&map, None);
    match canopy_sim::World::new(content.clone(), &map, setup) {
        Ok(world) => {
            for check in [
                canopy_sim::spawn::playable(&map, world.catalog(), world.terrain()),
                canopy_sim::spawn::connected(&map, world.terrain()),
            ] {
                if let Err(error) = check {
                    report.warnings.push(error);
                }
            }
        }
        Err(error) => report.warnings.push(error.0),
    }
    canopy_content::map::package::write(&map, id, "1.0.0", &dir).map_err(|e| e.to_string())?;
    let trees = crate::map::tools::tree_kinds(content);
    let image = crate::map::preview::render(&map, &|a| trees.iter().any(|t| t == a))?;
    std::fs::write(dir.join("preview.webp"), crate::map::preview::webp(&image)?)
        .map_err(|e| e.to_string())?;
    Ok(report)
}
