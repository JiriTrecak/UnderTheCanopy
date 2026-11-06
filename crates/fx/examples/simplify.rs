//! Simplifies every graph layer of the effect files in a folder ([`canopy_fx::simplify`]),
//! checks each simplified layer still compiles to valid WGSL, and reports the node counts.
//!
//!     cargo run --release -p canopy-fx --example simplify -- <folder> [--write]
//!
//! `--write` rewrites the files with the simplified graphs.

use canopy_fx::{Graph, check, compile, simplify::simplify};
use std::path::Path;

fn valid(graph: &Graph) -> Result<(), String> {
    let program = check(graph).map_err(|p| format!("{:?}", p.first()))?;
    let layer = compile::compile(&program, &[]);
    let module =
        naga::front::wgsl::parse_str(&layer.wgsl).map_err(|e| e.emit_to_string(&layer.wgsl))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map(|_| ())
    .map_err(|e| format!("{e:?}"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let folder = args.first().expect("a folder of effect files");
    let write = args.iter().any(|a| a == "--write");
    let (mut layers, mut before, mut after, mut kept, mut bad) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut files: Vec<_> = std::fs::read_dir(folder)
        .expect("folder")
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    for path in files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
    {
        let text = std::fs::read_to_string(path).expect("file");
        let mut doc: serde_json::Value = serde_json::from_str(&text).expect("json");
        let mut touched = false;
        for g in doc["graphs"].as_array_mut().into_iter().flatten() {
            let Ok(graph) = serde_json::from_value::<Graph>(g["graph"].clone()) else {
                continue;
            };
            layers += 1;
            before += graph.nodes.len();
            let simple = simplify(&graph);
            if simple.nodes.len() == graph.nodes.len() {
                kept += 1;
            }
            if let Err(e) = valid(&simple) {
                bad += 1;
                println!("BAD {} {}: {}", name(path), g["id"], &e[..e.len().min(300)]);
                after += graph.nodes.len();
                continue;
            }
            after += simple.nodes.len();
            if write {
                g["graph"] = serde_json::to_value(&simple).expect("graph");
                touched = true;
            }
        }
        if touched {
            let mut out = serde_json::to_string_pretty(&doc).expect("json");
            out.push('\n');
            std::fs::write(path, out).expect("write");
        }
    }
    println!(
        "layers {layers}: nodes {before} -> {after} ({:.0}% fewer), unchanged {kept}, invalid {bad}",
        100.0 * (1.0 - after as f64 / before.max(1) as f64)
    );
}

fn name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
