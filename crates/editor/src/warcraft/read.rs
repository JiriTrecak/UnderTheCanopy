//! The records of a Warcraft III map the import uses: its information (`war3map.w3i`),
//! terrain (`war3map.w3e`), doodads (`war3map.doo`), placed units (`war3mapUnits.doo`),
//! strings (`war3map.wts`) and object changes (`war3map.w3u`, `war3map.w3b`). Layouts
//! follow the old importer (`src/editor/import/warcraft/` in the old game) and
//! mdx-m3-viewer's parsers. Scripts and triggers are never read.

use super::mpq::Archive;

/// A bounded little-endian reader.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    file: &'static str,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], file: &'static str) -> Self {
        Self { bytes, at: 0, file }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.bytes.len());
        let end = end.ok_or_else(|| format!("{}: truncated at byte {}", self.file, self.at))?;
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn skip(&mut self, n: usize) -> Result<(), String> {
        self.take(n).map(|_| ())
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn i16(&mut self) -> Result<i16, String> {
        Ok(self.u16()? as i16)
    }

    fn i32(&mut self) -> Result<i32, String> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn f32(&mut self) -> Result<f32, String> {
        let b = self.take(4)?;
        let v = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        if v.is_finite() {
            Ok(v)
        } else {
            Err(format!("{}: an invalid number", self.file))
        }
    }

    fn four(&mut self) -> Result<String, String> {
        Ok(self.take(4)?.iter().map(|&c| c as char).collect())
    }

    fn string(&mut self) -> Result<String, String> {
        let rest = &self.bytes[self.at..];
        let end = rest
            .iter()
            .position(|&c| c == 0)
            .ok_or_else(|| format!("{}: an unterminated string", self.file))?;
        let s = String::from_utf8_lossy(&rest[..end]).into_owned();
        self.at += end + 1;
        Ok(s)
    }

    fn count(&mut self, max: usize) -> Result<usize, String> {
        let n = self.i32()?;
        if n < 0 || n as usize > max {
            return Err(format!("{}: an invalid count {n}", self.file));
        }
        Ok(n as usize)
    }
}

#[derive(Debug, Clone)]
pub struct Player {
    /// 1 human, 2 computer, 3 neutral, 4 rescuable.
    pub kind: i32,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone)]
pub struct Info {
    /// Game version × 100 (132 for 1.32); 0 before it was recorded.
    pub build: i32,
    pub name: String,
    pub description: String,
    pub players: Vec<Player>,
    /// Tiles outside the playable area on the left, right, bottom and top.
    pub margins: [usize; 4],
}

fn info(bytes: &[u8]) -> Result<Info, String> {
    let mut r = Reader::new(bytes, "war3map.w3i");
    let version = r.i32()?;
    if ![18, 25, 28, 31, 32, 33].contains(&version) {
        return Err(format!("unsupported map information version {version}"));
    }
    r.skip(8)?;
    let mut build = 0;
    if version >= 27 {
        let (major, minor) = (r.i32()?, r.i32()?);
        build = major * 100 + minor;
        r.skip(8)?;
    }
    let name = r.string()?;
    r.string()?;
    let description = r.string()?;
    r.string()?;
    r.skip(32)?;
    let mut margins = [0usize; 4];
    for m in &mut margins {
        *m = r.count(512)?;
    }
    r.count(512)?;
    r.count(512)?;
    r.skip(4)?;
    r.skip(5)?;
    if version >= 25 {
        r.string()?;
    }
    for _ in 0..3 {
        r.string()?;
    }
    r.skip(4)?;
    if version >= 25 {
        r.string()?;
    }
    for _ in 0..3 {
        r.string()?;
    }
    if version >= 25 {
        r.skip(24)?;
        r.string()?;
        r.skip(5)?;
    }
    if version >= 28 {
        r.skip(4)?;
    }
    if version >= 31 {
        r.skip(8)?;
    }
    if version >= 32 {
        r.skip(8)?;
    }
    if version >= 33 {
        r.skip(4)?;
    }
    let mut players = Vec::new();
    for _ in 0..r.count(28)? {
        let _id = r.i32()?;
        let kind = r.i32()?;
        let _race = r.i32()?;
        let _fixed = r.i32()?;
        r.string()?;
        let (x, y) = (r.f32()?, r.f32()?);
        r.skip(if version >= 31 { 16 } else { 8 })?;
        players.push(Player { kind, x, y });
    }
    Ok(Info {
        build,
        name,
        description,
        players,
        margins,
    })
}

/// One terrain corner.
#[derive(Debug, Clone, Copy)]
pub struct Corner {
    /// Heights in terrain tiles (128 source units).
    pub ground: f32,
    pub water_height: f32,
    pub water: bool,
    pub ramp: bool,
    pub texture: u8,
    pub variation: u8,
    pub cliff_texture: u8,
    pub level: u8,
}

#[derive(Debug, Clone)]
pub struct Terrain {
    pub tileset: char,
    pub ground_tiles: Vec<String>,
    pub cliff_tiles: Vec<String>,
    pub width: usize,
    pub height: usize,
    pub offset_x: f32,
    pub offset_y: f32,
    pub corners: Vec<Corner>,
}

impl Terrain {
    /// A corner's cliff level as drawn: a ramp's middle corner (a ramp corner between a
    /// ramp corner one level up and one at its own level) sits half a level up, so a ramp
    /// climbs one level evenly over its two tiles.
    pub fn level(&self, x: isize, y: isize) -> f32 {
        let c = self.at(x, y);
        let level = f32::from(c.level);
        if !c.ramp {
            return level;
        }
        let up = |o: &Corner| o.ramp && o.level == c.level + 1;
        let same = |o: &Corner| o.ramp && o.level == c.level;
        let middle = [((1, 0), (-1, 0)), ((0, 1), (0, -1))]
            .iter()
            .any(|&((ax, ay), (bx, by))| {
                let (a, b) = (self.at(x + ax, y + ay), self.at(x + bx, y + by));
                (up(a) && same(b)) || (up(b) && same(a))
            });
        if middle { level + 0.5 } else { level }
    }

    pub fn at(&self, x: isize, y: isize) -> &Corner {
        let x = x.clamp(0, self.width as isize - 1) as usize;
        let y = y.clamp(0, self.height as isize - 1) as usize;
        &self.corners[y * self.width + x]
    }
}

fn terrain(bytes: &[u8]) -> Result<Terrain, String> {
    let mut r = Reader::new(bytes, "war3map.w3e");
    if r.four()? != "W3E!" {
        return Err("war3map.w3e: not a terrain file".into());
    }
    let version = r.i32()?;
    if version != 11 && version != 12 {
        return Err(format!("unsupported terrain version {version}"));
    }
    let tileset = r.u8()? as char;
    r.i32()?;
    let ground_tiles = (0..r.count(if version == 12 { 64 } else { 16 })?)
        .map(|_| r.four())
        .collect::<Result<Vec<_>, _>>()?;
    let cliff_tiles = (0..r.count(256)?)
        .map(|_| r.four())
        .collect::<Result<Vec<_>, _>>()?;
    let (width, height) = (r.count(513)?, r.count(513)?);
    if width < 2 || height < 2 {
        return Err("war3map.w3e: no cells".into());
    }
    let (offset_x, offset_y) = (r.f32()?, r.f32()?);
    let mut corners = Vec::with_capacity(width * height);
    for _ in 0..width * height {
        let ground = (f32::from(r.i16()?) - 8192.0) / 512.0;
        let packed_water = r.u16()?;
        let packed = if version == 12 {
            r.u16()?
        } else {
            u16::from(r.u8()?)
        };
        let (texture, flags) = if version == 12 {
            (packed & 63, packed >> 6)
        } else {
            (packed & 15, packed >> 4)
        };
        let variation = r.u8()?;
        let cliff = r.u8()?;
        if texture as usize >= ground_tiles.len() {
            return Err(format!("war3map.w3e: a missing texture {texture}"));
        }
        corners.push(Corner {
            ground,
            water_height: (f32::from(packed_water & 0x3FFF) - 8192.0) / 512.0,
            water: flags & 4 != 0,
            ramp: flags & 1 != 0,
            texture: texture as u8,
            variation: variation & 31,
            cliff_texture: cliff >> 4,
            level: cliff & 15,
        });
    }
    Ok(Terrain {
        tileset,
        ground_tiles,
        cliff_tiles,
        width,
        height,
        offset_x,
        offset_y,
        corners,
    })
}

/// A placed doodad or unit.
#[derive(Debug, Clone)]
pub struct Object {
    pub id: String,
    pub variation: i32,
    pub x: f32,
    pub y: f32,
    pub angle: f32,
    pub scale: f32,
    /// Doodads: life (0 is dead). Units: the owning player slot.
    pub life: u8,
    pub player: i32,
    pub gold: i32,
    pub hero_level: i32,
}

fn header(r: &mut Reader) -> Result<(i32, i32), String> {
    if r.four()? != "W3do" {
        return Err(format!("{}: not an objects file", r.file));
    }
    Ok((r.i32()?, r.i32()?))
}

fn item_sets(r: &mut Reader) -> Result<(), String> {
    for _ in 0..r.count(4096)? {
        let n = r.count(4096)?;
        r.skip(n * 8)?;
    }
    Ok(())
}

fn doodads(bytes: &[u8], build: i32) -> Result<Vec<Object>, String> {
    let mut r = Reader::new(bytes, "war3map.doo");
    let (version, _) = header(&mut r)?;
    let mut out = Vec::new();
    for _ in 0..r.count(200_000)? {
        let id = r.four()?;
        let variation = r.i32()?;
        let (x, y, _z) = (r.f32()?, r.f32()?, r.f32()?);
        let angle = r.f32()?;
        let scale = r.f32()?;
        r.skip(8)?;
        if build > 131 {
            r.skip(4)?;
        }
        let _flags = r.u8()?;
        let life = r.u8()?;
        if version > 7 {
            r.i32()?;
            item_sets(&mut r)?;
        }
        r.i32()?;
        out.push(Object {
            id,
            variation,
            x,
            y,
            angle,
            scale,
            life,
            player: -1,
            gold: 0,
            hero_level: 0,
        });
    }
    Ok(out)
}

fn units(bytes: &[u8], build: i32) -> Result<Vec<Object>, String> {
    let mut r = Reader::new(bytes, "war3mapUnits.doo");
    let (_, subversion) = header(&mut r)?;
    let mut out = Vec::new();
    for _ in 0..r.count(100_000)? {
        let id = r.four()?;
        let variation = r.i32()?;
        let (x, y, _z) = (r.f32()?, r.f32()?, r.f32()?);
        let angle = r.f32()?;
        let scale = r.f32()?;
        r.skip(8)?;
        if build > 131 {
            r.skip(4)?;
        }
        r.u8()?;
        let player = r.i32()?;
        r.skip(2 + 8)?;
        if subversion > 10 {
            r.i32()?;
        }
        item_sets(&mut r)?;
        let gold = r.i32()?;
        r.f32()?;
        let hero_level = r.i32()?;
        if subversion > 10 {
            r.skip(12)?;
        }
        let items = r.count(64)?;
        r.skip(items * 8)?;
        let abilities = r.count(256)?;
        r.skip(abilities * 12)?;
        match r.i32()? {
            0 => r.skip(4)?,
            1 => r.skip(8)?,
            2 => {
                let n = r.count(4096)?;
                r.skip(n * 8)?;
            }
            // Not random (-1 in newer maps): nothing follows.
            _ => {}
        }
        r.skip(12)?;
        out.push(Object {
            id,
            variation,
            x,
            y,
            angle,
            scale,
            life: 100,
            player,
            gold,
            hero_level,
        });
    }
    Ok(out)
}

/// A unit or destructible type a map changes: its base, id and changed fields we read
/// (a unit's level `ulev`, a destructible's targets `btar`).
#[derive(Debug, Clone)]
pub struct Change {
    pub base: String,
    pub id: String,
    pub level: Option<i32>,
    pub targets: Option<String>,
}

fn changes(bytes: &[u8], file: &'static str) -> Result<Vec<Change>, String> {
    let mut r = Reader::new(bytes, file);
    let version = r.i32()?;
    if !(1..=3).contains(&version) {
        return Err(format!("{file}: unsupported version {version}"));
    }
    let mut out = Vec::new();
    for _ in 0..2 {
        for _ in 0..r.count(20_000)? {
            let base = r.four()?;
            let custom = r.four()?;
            let id = if custom == "\0\0\0\0" {
                base.clone()
            } else {
                custom
            };
            if version >= 3 {
                let n = r.count(10_000)?;
                r.skip(n * 4)?;
            }
            let (mut level, mut targets) = (None, None);
            for _ in 0..r.count(10_000)? {
                let field = r.four()?;
                let kind = r.i32()?;
                if file.ends_with("w3d") || file.ends_with("w3a") || file.ends_with("w3q") {
                    r.skip(8)?;
                }
                match kind {
                    0 => {
                        let v = r.i32()?;
                        if field == "ulev" {
                            level = Some(v);
                        }
                    }
                    1 | 2 => {
                        r.f32()?;
                    }
                    3 => {
                        let s = r.string()?;
                        if field == "btar" {
                            targets = Some(s);
                        }
                    }
                    _ => return Err(format!("{file}: an invalid field type")),
                }
                r.skip(4)?;
            }
            out.push(Change {
                base,
                id,
                level,
                targets,
            });
        }
    }
    Ok(out)
}

/// `TRIGSTR_007` names → their text, from `war3map.wts`.
fn strings(text: &str) -> std::collections::HashMap<u32, String> {
    let mut out = std::collections::HashMap::new();
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    while let Some(line) = lines.next() {
        let Some(n) = line
            .trim()
            .strip_prefix("STRING ")
            .and_then(|n| n.trim().parse().ok())
        else {
            continue;
        };
        let mut body = Vec::new();
        let mut open = false;
        for l in lines.by_ref() {
            match (open, l.trim()) {
                (false, "{") => open = true,
                (true, "}") => break,
                (true, _) => body.push(l.to_string()),
                _ => {}
            }
        }
        out.insert(n, body.join("\n"));
    }
    out
}

/// Colour codes (`|cffRRGGBB … |r`) removed.
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '|' {
            match chars.peek() {
                Some('c') | Some('C') => {
                    for _ in 0..9 {
                        chars.next();
                    }
                    continue;
                }
                Some('r') | Some('R') => {
                    chars.next();
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
    }
    out.trim().to_string()
}

/// A Warcraft III map's records.
#[derive(Debug, Clone)]
pub struct Map {
    pub info: Info,
    pub terrain: Terrain,
    pub doodads: Vec<Object>,
    pub units: Vec<Object>,
    pub unit_changes: Vec<Change>,
    pub tree_changes: Vec<Change>,
}

pub fn read(bytes: &[u8]) -> Result<Map, String> {
    let archive = Archive::open(bytes)?;
    let mut info = info(&archive.read("war3map.w3i")?)?;
    let terrain = terrain(&archive.read("war3map.w3e")?)?;
    if archive.has("war3map.wts") {
        let text = String::from_utf8_lossy(&archive.read("war3map.wts")?).into_owned();
        let table = strings(&text);
        let resolve = |s: &str| {
            s.strip_prefix("TRIGSTR_")
                .and_then(|n| n.parse::<u32>().ok())
                .and_then(|n| table.get(&n).cloned())
                .unwrap_or_else(|| s.to_string())
        };
        info.name = resolve(&info.name);
        info.description = resolve(&info.description);
    }
    info.name = plain(&info.name);
    info.description = plain(&info.description);
    let doodads = if archive.has("war3map.doo") {
        doodads(&archive.read("war3map.doo")?, info.build)?
    } else {
        Vec::new()
    };
    let units = units(&archive.read("war3mapUnits.doo")?, info.build)?;
    let change = |name: &'static str| -> Result<Vec<Change>, String> {
        if archive.has(name) {
            changes(&archive.read(name)?, name)
        } else {
            Ok(Vec::new())
        }
    };
    Ok(Map {
        info,
        terrain,
        doodads,
        units,
        unit_changes: change("war3map.w3u")?,
        tree_changes: change("war3map.w3b")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ramps_middle_corner_sits_half_a_level_up() {
        // One row: plateau at level 5, a ramp (5r 4r 4r), ground at level 4.
        let row = [(5, false), (5, true), (4, true), (4, true), (4, false)];
        let corners: Vec<Corner> = row
            .iter()
            .cycle()
            .take(row.len() * 3)
            .map(|&(level, ramp)| Corner {
                ground: 0.0,
                water_height: 0.0,
                water: false,
                ramp,
                texture: 0,
                variation: 0,
                cliff_texture: 0,
                level,
            })
            .collect();
        let t = Terrain {
            tileset: 'L',
            ground_tiles: vec!["Ldrt".into()],
            cliff_tiles: vec![],
            width: row.len(),
            height: 3,
            offset_x: 0.0,
            offset_y: 0.0,
            corners,
        };
        let levels: Vec<f32> = (0..5).map(|x| t.level(x, 1)).collect();
        assert_eq!(levels, [5.0, 5.0, 4.5, 4.0, 4.0]);
    }

    #[test]
    fn strings_and_colour_codes_resolve() {
        let table = strings(
            "STRING 1\r\n{\r\n|cff00ff00Echo|r Isles\r\n}\r\n\r\nSTRING 2\r\n// comment\r\n{\r\nTwo\r\nlines\r\n}\r\n",
        );
        assert_eq!(plain(&table[&1]), "Echo Isles");
        assert_eq!(table[&2], "Two\nlines");
    }
}

#[cfg(test)]
mod scan {
    /// Reads every map in `$WARCRAFT_MAPS` (a folder searched recursively): a check
    /// against real maps, run by hand.
    #[test]
    #[ignore]
    fn reads_local_maps() {
        let Ok(dir) = std::env::var("WARCRAFT_MAPS") else {
            return;
        };
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out)
                } else if p.extension().is_some_and(|e| e == "w3x" || e == "w3m") {
                    out.push(p)
                }
            }
        }
        let mut files = Vec::new();
        walk(std::path::Path::new(&dir), &mut files);
        files.sort();
        for f in files {
            let name = f.file_name().unwrap().to_string_lossy().to_string();
            match super::read(&std::fs::read(&f).unwrap()) {
                Ok(m) => println!(
                    "OK {name} | {} | tileset {} {}x{} | starts {} | tiles {:?} cliffs {:?} | doodads {} units {}",
                    m.info.name,
                    m.terrain.tileset,
                    m.terrain.width - 1,
                    m.terrain.height - 1,
                    m.info.players.len(),
                    m.terrain.ground_tiles,
                    m.terrain.cliff_tiles,
                    m.doodads.len(),
                    m.units.len()
                ),
                Err(e) => println!("ERR {name}: {e}"),
            }
        }
    }
}
