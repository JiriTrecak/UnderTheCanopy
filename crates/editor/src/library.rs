//! What each editor lists: the collections it edits, read from a package with the
//! loader's own walk ([`Collection::read`]), as a tree of the package's folders.

use canopy_content::{
    assets::package::{ASSETS, SCENERY, WATER},
    collections::*,
    package::{CAMPAIGN_EXTENSION, Collection, Form, Found, MANIFEST, MAP_EXTENSION, Package},
};
use serde_json::Value;

/// The editors on the rail, in rail order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Maps,
    Campaigns,
    Units,
    Abilities,
    Effects,
    Buildings,
    Races,
    Assets,
    /// The combat lab's scenarios (`crate::lab`), beside the package.
    Lab,
}

pub const KINDS: [Kind; 9] = [
    Kind::Maps,
    Kind::Campaigns,
    Kind::Units,
    Kind::Abilities,
    Kind::Effects,
    Kind::Buildings,
    Kind::Races,
    Kind::Assets,
    Kind::Lab,
];

const MAPS: Collection = Collection {
    dir: "maps",
    form: Form::Packages(MAP_EXTENSION),
};
const BIOMES: Collection = Collection::files("landscape/biomes");
/// The lab's scenarios: mounted beside the package (`crate::lab`), never read from it.
const LABS: Collection = Collection::files("@labs");
/// Campaigns' mission maps (`campaigns/<id>.utcc/missions/<map>.utcm`): the Maps editor
/// opens them; its library lists them under each campaign (`crate::campaign`).
const MISSION_MAPS: Collection = Collection {
    dir: crate::campaign::DIR,
    form: Form::Packages(MAP_EXTENSION),
};

impl Kind {
    /// The editor's name in its header.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Maps => "Maps",
            Kind::Campaigns => "Campaigns",
            Kind::Units => "Units and heroes",
            Kind::Abilities => "Abilities",
            Kind::Effects => "Effects",
            Kind::Buildings => "Items and buildings",
            Kind::Races => "Races and rules",
            Kind::Assets => "Assets",
            Kind::Lab => "Combat lab",
        }
    }

    /// The short label under its rail icon.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Maps => "Maps",
            Kind::Campaigns => "Campaigns",
            Kind::Units => "Units",
            Kind::Abilities => "Abilities",
            Kind::Effects => "Effects",
            Kind::Buildings => "Buildings",
            Kind::Races => "Races",
            Kind::Assets => "Assets",
            Kind::Lab => "Lab",
        }
    }

    /// What one element is called (the library's search hint).
    pub fn plural(self) -> &'static str {
        match self {
            Kind::Maps => "maps",
            Kind::Campaigns => "campaigns",
            Kind::Units => "units",
            Kind::Abilities => "abilities",
            Kind::Effects => "effects",
            Kind::Buildings => "items and buildings",
            Kind::Races => "races and rules",
            Kind::Assets => "assets",
            Kind::Lab => "scenarios",
        }
    }

    /// The editor whose collections hold `document` (a path from the package root).
    pub fn of_document(document: &str) -> Option<Kind> {
        Self::collection_of(document).map(|(kind, _)| kind)
    }

    /// The editor and collection that hold `document`.
    pub fn collection_of(document: &str) -> Option<(Kind, &'static Collection)> {
        if crate::campaign::is_mission_document(document) {
            return Some((Kind::Maps, &MISSION_MAPS));
        }
        if crate::lab::is_lab(document) {
            return Some((Kind::Lab, &LABS));
        }
        KINDS.into_iter().find_map(|kind| {
            kind.sections()
                .iter()
                .find(|(_, collection)| {
                    document
                        .strip_prefix(collection.dir)
                        .is_some_and(|rest| rest.starts_with('/'))
                })
                .map(|(_, collection)| (kind, collection))
        })
    }

    /// The collections it edits, each with its section title.
    pub fn sections(self) -> &'static [(&'static str, Collection)] {
        match self {
            Kind::Maps => &[("Maps", MAPS)],
            Kind::Campaigns => &[("Campaigns", CAMPAIGNS)],
            Kind::Units => &[("Units", UNITS)],
            Kind::Abilities => &[("Abilities", ABILITIES)],
            Kind::Effects => &[("Effects", EFFECTS)],
            Kind::Buildings => &[
                ("Buildings", BUILDINGS),
                ("Items", ITEMS),
                ("Loot pools", LOOT_POOLS),
            ],
            Kind::Races => &[
                ("Races", RACES),
                ("Research", RESEARCH),
                ("Resources", RESOURCES),
                ("Behaviours", BEHAVIORS),
                ("Rules", RULES),
            ],
            Kind::Assets => &[
                ("Assets", ASSETS),
                ("Sounds", SOUNDS),
                ("Scenery", SCENERY),
                ("Biomes", BIOMES),
                ("Water", WATER),
            ],
            Kind::Lab => &[],
        }
    }
}

/// One element as the library lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub id: String,
    /// Its `name`, or the id when it has none.
    pub name: String,
    /// The element's path in the package, `/`-separated (a file or a folder).
    pub path: String,
    /// The document a tab shows, `/`-separated from the package root.
    pub document: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Folder {
        name: String,
        /// Unique key: the folder's path in the package.
        path: String,
        children: Vec<Node>,
        /// Elements beneath it.
        count: usize,
    },
    Element(Element),
}

/// An editor's library: its tree and what could not be read.
#[derive(Debug, Default)]
pub struct Library {
    pub nodes: Vec<Node>,
    pub count: usize,
    pub problems: Vec<String>,
}

impl Library {
    /// Reads `kind`'s collections from `package`. One collection lists its tree at the
    /// root; several list one folder per section. The lab's scenarios are in `data`'s
    /// `labs/` (the folder the store mounts), beside the package when it is not given.
    pub fn read(package: &Package, kind: Kind, data: Option<&std::path::Path>) -> Self {
        if kind == Kind::Lab {
            return crate::lab::library(data.or(package.root.parent()));
        }
        let sections = kind.sections();
        let mut library = Library::default();
        let mut errors = Vec::new();
        // Named elements take their names from the package's text library (D96).
        let text = canopy_content::text::Text::read(
            std::slice::from_ref(package),
            crate::text::LANGUAGE,
            &mut Vec::new(),
        );
        for (title, collection) in sections {
            let mut nodes = Vec::new();
            for found in collection.read(package, &mut errors) {
                let element = element(package, collection, found, &text);
                let relative = element
                    .path
                    .strip_prefix(collection.dir)
                    .map_or(element.path.as_str(), |p| p.trim_start_matches('/'))
                    .to_string();
                let folders: Vec<&str> = relative.split('/').collect();
                insert(
                    &mut nodes,
                    collection.dir,
                    &folders[..folders.len() - 1],
                    element,
                );
            }
            sort(&mut nodes);
            // The Maps library lists each campaign's mission maps too, in step order.
            if kind == Kind::Maps {
                nodes.extend(crate::campaign::maps_group(package));
            }
            let count = nodes.iter().map(Node::count).sum::<usize>();
            library.count += count;
            if sections.len() == 1 {
                library.nodes = nodes;
            } else {
                library.nodes.push(Node::Folder {
                    name: (*title).to_string(),
                    path: collection.dir.to_string(),
                    children: nodes,
                    count,
                });
            }
        }
        library.problems = errors.iter().map(ToString::to_string).collect();
        library
    }
}

impl Node {
    pub fn count(&self) -> usize {
        match self {
            Node::Folder { count, .. } => *count,
            Node::Element(_) => 1,
        }
    }

    /// Every element beneath this node, depth first.
    pub fn elements<'a>(&'a self, out: &mut Vec<&'a Element>) {
        match self {
            Node::Folder { children, .. } => children.iter().for_each(|c| c.elements(out)),
            Node::Element(e) => out.push(e),
        }
    }
}

fn element(
    package: &Package,
    collection: &Collection,
    found: Found,
    text: &canopy_content::text::Text,
) -> Element {
    let (document, value) = match collection.form {
        Form::Folders(manifest) => (format!("{}/{manifest}", found.path), found.value),
        Form::Packages(extension) => {
            let manifest = format!("{}/{MANIFEST}", found.path);
            let value = package
                .read_text(&manifest)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or(Value::Null);
            let document = if extension == CAMPAIGN_EXTENSION {
                crate::campaign::FILE
            } else {
                "map.json"
            };
            (format!("{}/{document}", found.path), value)
        }
        _ => (found.path.clone(), found.value),
    };
    let named = crate::text::key(&document, &value)
        .and_then(|key| text.get(&format!("{key}.name")).map(str::to_string));
    let name = named.unwrap_or_else(|| {
        value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&found.id)
            .to_string()
    });
    Element {
        id: found.id,
        name,
        path: found.path,
        document,
    }
}

fn insert(nodes: &mut Vec<Node>, base: &str, folders: &[&str], element: Element) {
    let Some((first, rest)) = folders.split_first() else {
        nodes.push(Node::Element(element));
        return;
    };
    let path = format!("{base}/{first}");
    let index = nodes
        .iter()
        .position(|n| matches!(n, Node::Folder { path: p, .. } if *p == path))
        .unwrap_or_else(|| {
            nodes.push(Node::Folder {
                name: (*first).to_string(),
                path: path.clone(),
                children: Vec::new(),
                count: 0,
            });
            nodes.len() - 1
        });
    if let Node::Folder {
        children, count, ..
    } = &mut nodes[index]
    {
        *count += 1;
        insert(children, &path, rest, element);
    }
}

/// Folders first, then elements, each by name.
fn sort(nodes: &mut [Node]) {
    nodes.sort_by(|a, b| match (a, b) {
        (Node::Folder { name: x, .. }, Node::Folder { name: y, .. }) => x.cmp(y),
        (Node::Folder { .. }, Node::Element(_)) => std::cmp::Ordering::Less,
        (Node::Element(_), Node::Folder { .. }) => std::cmp::Ordering::Greater,
        (Node::Element(x), Node::Element(y)) => x.name.to_lowercase().cmp(&y.name.to_lowercase()),
    });
    for node in nodes {
        if let Node::Folder { children, .. } = node {
            sort(children);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(root: &std::path::Path, path: &str, text: &str) {
        let file = root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }

    fn package() -> (tempdir::Dir, Package) {
        let dir = tempdir::Dir::new();
        let root = dir.path();
        write(
            root,
            "package.json",
            r#"{"format":1,"id":"fixture","kind":"base","name":"Fixture","version":"1.0.0"}"#,
        );
        write(
            root,
            "abilities/red/blast.json",
            r#"{"id":"ability.blast","name":"Blast"}"#,
        );
        write(root, "abilities/red/aura.json", r#"{"id":"ability.aura"}"#);
        write(
            root,
            "abilities/calm.json",
            r#"{"id":"ability.calm","name":"Calm"}"#,
        );
        write(
            root,
            "maps/skirmish/isle.utcm/package.json",
            r#"{"format":1,"id":"isle","kind":"map","name":"The Isle","version":"1.0.0"}"#,
        );
        let package = Package::open(root).unwrap();
        (dir, package)
    }

    #[test]
    fn folders_come_first_and_elements_take_their_names() {
        let (_dir, package) = package();
        let library = Library::read(&package, Kind::Abilities, None);
        assert_eq!(library.count, 3);
        assert!(library.problems.is_empty(), "{:?}", library.problems);
        let Node::Folder {
            name,
            children,
            count,
            ..
        } = &library.nodes[0]
        else {
            panic!("a folder first: {:?}", library.nodes);
        };
        assert_eq!((name.as_str(), *count), ("red", 2));
        let names: Vec<_> = children
            .iter()
            .map(|c| match c {
                Node::Element(e) => e.name.as_str(),
                Node::Folder { .. } => "folder",
            })
            .collect();
        assert_eq!(names, ["ability.aura", "Blast"]);
        assert!(matches!(&library.nodes[1], Node::Element(e) if e.name == "Calm"));
    }

    #[test]
    fn maps_show_their_package_name_and_open_their_terrain_document() {
        let (_dir, package) = package();
        let library = Library::read(&package, Kind::Maps, None);
        let mut elements = Vec::new();
        library.nodes.iter().for_each(|n| n.elements(&mut elements));
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0].name, "The Isle");
        assert_eq!(elements[0].document, "maps/skirmish/isle.utcm/map.json");
    }

    #[test]
    fn several_collections_list_one_folder_each() {
        let (_dir, package) = package();
        let library = Library::read(&package, Kind::Buildings, None);
        let names: Vec<_> = library
            .nodes
            .iter()
            .map(|n| match n {
                Node::Folder { name, .. } => name.as_str(),
                Node::Element(_) => "element",
            })
            .collect();
        assert_eq!(names, ["Buildings", "Items", "Loot pools"]);
        assert_eq!(library.count, 0);
    }

    /// A scratch folder removed on drop (the workspace has no tempfile crate).
    mod tempdir {
        pub struct Dir(std::path::PathBuf);

        impl Dir {
            pub fn new() -> Self {
                static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let path =
                    std::env::temp_dir().join(format!("canopy-editor-{}-{n}", std::process::id()));
                let _ = std::fs::remove_dir_all(&path);
                std::fs::create_dir_all(&path).unwrap();
                Self(path)
            }

            pub fn path(&self) -> &std::path::Path {
                &self.0
            }
        }

        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }
}
