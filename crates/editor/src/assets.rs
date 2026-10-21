//! Importing art (docs/editor.md, M4): a glTF binary becomes a model asset, a PNG an icon
//! or texture, each in a new folder under `assets/` with its `asset.json` prefilled from
//! the file (a model's clips by their semantic names, its `socket_*` nodes). The folder is
//! written at once, as files copied in by hand would be; the loader's check and the
//! library pick it up from disk.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Clip names the game knows as semantics (and loops).
const SEMANTICS: [(&str, bool); 13] = [
    ("idle", true),
    ("walk", true),
    ("run", true),
    ("attack", false),
    ("hit", false),
    ("death", false),
    ("cast", false),
    ("build", true),
    ("chop", true),
    ("carry", true),
    ("carry_walk", true),
    ("carry_run", true),
    ("decay", false),
];

/// Model kinds (the rest take images).
const MODEL_KINDS: [&str; 7] = [
    "unit", "creature", "building", "tree", "foliage", "prop", "bridge",
];
const IMAGE_KINDS: [&str; 3] = ["icon", "interface", "texture"];

/// What to import.
#[derive(Debug, Clone)]
pub struct Import {
    /// The file: `.glb` or `.png`.
    pub source: PathBuf,
    /// The new asset's id (`asset.models.units.example`).
    pub id: String,
    pub name: String,
    /// One of the asset kinds; a model kind for a `.glb`, an image kind for a `.png`.
    pub kind: String,
    /// The folder below `assets/`, `/`-separated (`units/example`).
    pub folder: String,
}

/// The kinds a file of `source`'s type can become.
pub fn kinds_for(source: &Path) -> &'static [&'static str] {
    match source
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("glb") => &MODEL_KINDS,
        Some("png") => &IMAGE_KINDS,
        _ => &[],
    }
}

/// Writes the asset into the package at `root`; returns its `asset.json` path from the
/// package root.
pub fn import(root: &Path, request: &Import) -> Result<String, String> {
    canopy_content::Id::new(&request.id)?;
    if !request.id.starts_with("asset.") {
        return Err("an asset id starts with `asset.`".into());
    }
    let folder = request.folder.trim_matches('/');
    if folder.is_empty()
        || folder
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == ".." || p.contains('\\'))
    {
        return Err(format!("{folder:?} is not a folder below assets/"));
    }
    if !kinds_for(&request.source).contains(&request.kind.as_str()) {
        return Err(format!(
            "a {} file cannot become a {} asset",
            request
                .source
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("such"),
            request.kind
        ));
    }
    let dir = root.join("assets").join(folder);
    if dir.exists() {
        return Err(format!("assets/{folder} already exists"));
    }
    let bytes =
        std::fs::read(&request.source).map_err(|e| format!("{}: {e}", request.source.display()))?;
    let name = if request.name.trim().is_empty() {
        request
            .id
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .replace('-', " ")
    } else {
        request.name.trim().to_string()
    };
    let (file, asset) = if MODEL_KINDS.contains(&request.kind.as_str()) {
        (
            "geometry.glb",
            model_asset(&bytes, &request.id, &name, &request.kind)?,
        )
    } else {
        image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
            .map_err(|e| format!("{}: {e}", request.source.display()))?;
        let asset = json!({
            "id": request.id,
            "name": name,
            "kind": request.kind,
            "tags": [],
            "resources": [{"role": "image", "index": 1, "format": "png"}],
            "license": "Original artwork for Under the Canopy.",
        });
        ("image.png", asset)
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(file), &bytes).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("asset.json"), crate::document::format(&asset))
        .map_err(|e| e.to_string())?;
    Ok(format!("assets/{folder}/asset.json"))
}

/// A model's `asset.json` from its glTF binary: its clips under the semantics they are
/// named after (the others keep their own names), its `socket_*` nodes as sockets.
fn model_asset(glb: &[u8], id: &str, name: &str, kind: &str) -> Result<Value, String> {
    let gltf = gltf::Gltf::from_slice(glb).map_err(|e| format!("not a glTF binary: {e}"))?;
    // A semantic is an id: the clip's own name after any `Armature|` prefix, lowercased,
    // other characters as `_`; one clip per semantic.
    let mut seen = std::collections::BTreeSet::new();
    let animations: Vec<Value> = gltf
        .animations()
        .filter_map(|a| a.name().map(str::to_string))
        .filter_map(|clip| {
            let own = clip.rsplit('|').next().unwrap_or(&clip);
            let lower: String = own
                .trim()
                .to_ascii_lowercase()
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            let lower = lower.trim_matches(['_', '.', '-']).to_string();
            let (semantic, looping) = SEMANTICS
                .iter()
                .find(|(s, _)| *s == lower)
                .map_or((lower.clone(), false), |(s, l)| (s.to_string(), *l));
            (!semantic.is_empty() && seen.insert(semantic.clone()))
                .then(|| json!({"semantic": semantic, "clip": clip, "loop": looping, "events": []}))
        })
        .collect();
    let sockets: Vec<Value> = gltf
        .nodes()
        .filter_map(|n| n.name().map(str::to_string))
        .filter_map(|node| {
            let socket = node.strip_prefix("socket_")?.to_string();
            Some(json!({"name": socket, "node": node, "offset": [0, 0, 0]}))
        })
        .collect();
    let mut capabilities = serde_json::Map::new();
    if !animations.is_empty() {
        capabilities.insert("animations".into(), Value::Array(animations));
    }
    if !sockets.is_empty() {
        capabilities.insert("sockets".into(), Value::Array(sockets));
    }
    capabilities.insert("groundContact".into(), json!({"mode": "pivot"}));
    let mut asset = json!({
        "id": id,
        "name": name,
        "kind": kind,
        "tags": [],
        "resources": [{"role": "geometry", "index": 1, "format": "glb"}],
        "transform": {"scale": 1, "pivot": [0, 0, 0], "up": "Y", "forward": "+Z"},
        "capabilities": capabilities,
        "display": {"scale": 1},
        "license": "Original artwork for Under the Canopy.",
    });
    if kind == "building" {
        asset["shading"] = json!("building");
    }
    Ok(asset)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A glTF binary of just its JSON chunk.
    fn glb(json: &str) -> Vec<u8> {
        let mut chunk = json.as_bytes().to_vec();
        while !chunk.len().is_multiple_of(4) {
            chunk.push(b' ');
        }
        let total = 12 + 8 + chunk.len();
        let mut out = Vec::new();
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&chunk);
        out
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("canopy-import-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_model_becomes_an_asset_with_its_sockets() {
        let dir = temp("model");
        let source = dir.join("spearman.glb");
        std::fs::write(
            &source,
            glb(r#"{"asset":{"version":"2.0"},"nodes":[{"name":"root"},{"name":"socket_projectile"}]}"#),
        )
        .unwrap();
        let request = Import {
            source,
            id: "asset.models.units.spearman".into(),
            name: String::new(),
            kind: "unit".into(),
            folder: "units/spearman".into(),
        };
        let package = dir.join("base");
        let document = import(&package, &request).unwrap();
        assert_eq!(document, "assets/units/spearman/asset.json");
        let asset: Value =
            serde_json::from_str(&std::fs::read_to_string(package.join(&document)).unwrap())
                .unwrap();
        assert_eq!(asset["name"], "spearman");
        assert_eq!(asset["capabilities"]["sockets"][0]["name"], "projectile");
        assert!(package.join("assets/units/spearman/geometry.glb").is_file());
        // Not twice, and not as an icon.
        assert!(
            import(&package, &request)
                .unwrap_err()
                .contains("already exists")
        );
        let icon = Import {
            kind: "icon".into(),
            folder: "icons/x".into(),
            ..request
        };
        assert!(import(&package, &icon).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_png_becomes_an_icon_and_bad_input_is_refused() {
        let dir = temp("icon");
        let source = dir.join("attack.png");
        image::RgbaImage::new(4, 4).save(&source).unwrap();
        let request = Import {
            source: source.clone(),
            id: "asset.icons.attack".into(),
            name: "Attack".into(),
            kind: "icon".into(),
            folder: "icons/attack".into(),
        };
        let package = dir.join("base");
        import(&package, &request).unwrap();
        assert!(package.join("assets/icons/attack/image.png").is_file());
        for (id, folder) in [
            ("not-an-asset", "icons/a"),
            ("asset.icons.b", "../escape"),
            ("asset.icons.c", ""),
        ] {
            let bad = Import {
                id: id.into(),
                folder: folder.into(),
                ..request.clone()
            };
            assert!(import(&package, &bad).is_err(), "{id} in {folder:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
