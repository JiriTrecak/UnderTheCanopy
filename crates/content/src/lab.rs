//! Combat lab scenarios (docs/editor.md, "The Combat Lab"): a flat grid of open and closed
//! cells, units and buildings for players, named groups and the orders they get at given
//! ticks. Scenarios are development files (`labs/<id>.lab.json` at the repository root),
//! never shipped: the lab runs them with the real simulation to see and measure how
//! armies move and fight.

use crate::{Content, game::Kind, id::Id, placement::Owner};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::path::{Path, PathBuf};

/// Where scenarios live, relative to the repository (the data folder).
pub const LABS_FOLDER: &str = "labs";
/// A scenario file's ending.
pub const EXTENSION: &str = ".lab.json";
/// The grid sizes a scenario may have, in cells per side.
pub const SIZES: [u32; 6] = [64, 96, 128, 192, 256, 384];
/// Ticks a second.
pub const TICKS: u32 = 40;

/// A scenario.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Lab {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Cells per side, one of [`SIZES`].
    pub size: u32,
    /// How long a run lasts, in ticks.
    pub duration: u32,
    /// Closed cells, row by row: nothing walks there.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub closed: Vec<Span>,
    /// Units and buildings, in the order the world places them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub units: Vec<LabUnit>,
    /// What the groups are told to do, by tick.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub orders: Vec<LabOrder>,
}

/// `length` closed cells of row `y` from column `x`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Span {
    pub y: u32,
    pub x: u32,
    pub length: u32,
}

/// A unit or building of the scenario, at its position in cells.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LabUnit {
    pub definition: Id,
    pub owner: Owner,
    pub x: f64,
    pub y: f64,
    /// Degrees.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub facing: f64,
    /// The group orders name.
    pub group: Option<String>,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// What a group does from `tick`.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LabOrder {
    pub tick: u32,
    pub group: String,
    pub order: OrderKind,
    /// The point of a move or attack-move, in cells.
    pub x: Option<f64>,
    pub y: Option<f64>,
    /// The group an attack goes for.
    pub target: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum OrderKind {
    Move,
    AttackMove,
    /// Every member attacks the target group's first living member.
    Attack,
    Hold,
    Stop,
}

impl OrderKind {
    pub const ALL: [Self; 5] = [
        Self::Move,
        Self::AttackMove,
        Self::Attack,
        Self::Hold,
        Self::Stop,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Move => "Move",
            Self::AttackMove => "Attack-move",
            Self::Attack => "Attack",
            Self::Hold => "Hold",
            Self::Stop => "Stop",
        }
    }

    /// Whether the order goes to a point.
    pub fn has_point(self) -> bool {
        matches!(self, Self::Move | Self::AttackMove)
    }
}

impl Lab {
    /// An empty scenario of `size` cells per side that runs a minute.
    pub fn new(name: &str, size: u32) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            size,
            duration: 60 * TICKS,
            closed: Vec::new(),
            units: Vec::new(),
            orders: Vec::new(),
        }
    }

    /// Every cell, row by row: whether it is closed.
    pub fn closed_cells(&self) -> Vec<bool> {
        let n = self.size as usize;
        let mut cells = vec![false; n * n];
        for s in &self.closed {
            if s.y >= self.size {
                continue;
            }
            let end = s.x.saturating_add(s.length).min(self.size);
            for x in s.x..end {
                cells[s.y as usize * n + x as usize] = true;
            }
        }
        cells
    }

    /// Stores `cells` (row by row, [`Lab::closed_cells`]) as merged spans.
    pub fn set_closed_cells(&mut self, cells: &[bool]) {
        let n = self.size;
        let mut spans = Vec::new();
        for y in 0..n {
            let mut x = 0;
            while x < n {
                if cells.get((y * n + x) as usize).copied().unwrap_or(false) {
                    let start = x;
                    while x < n && cells.get((y * n + x) as usize).copied().unwrap_or(false) {
                        x += 1;
                    }
                    spans.push(Span {
                        y,
                        x: start,
                        length: x - start,
                    });
                } else {
                    x += 1;
                }
            }
        }
        self.closed = spans;
    }

    /// The named groups, in order of first appearance.
    pub fn groups(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for u in &self.units {
            if let Some(g) = u.group.as_deref()
                && !out.contains(&g)
            {
                out.push(g);
            }
        }
        out
    }

    /// What is wrong with the scenario against `content`, in words.
    pub fn problems(&self, content: &Content) -> Vec<String> {
        let mut out = Vec::new();
        if !SIZES.contains(&self.size) {
            out.push(format!(
                "size {} is not one of {}",
                self.size,
                SIZES.map(|s| s.to_string()).join(", ")
            ));
        }
        if self.duration == 0 {
            out.push("duration must be at least one tick".into());
        }
        let inside = |x: f64, y: f64| {
            x.is_finite() && y.is_finite() && x >= 0.0 && y >= 0.0 && {
                let s = f64::from(self.size);
                x < s && y < s
            }
        };
        for s in &self.closed {
            if s.y >= self.size || s.x.saturating_add(s.length) > self.size || s.length == 0 {
                out.push(format!(
                    "closed cells at row {}, column {} run off the grid",
                    s.y, s.x
                ));
            }
        }
        let mut owners: Vec<(&str, Owner)> = Vec::new();
        for (i, u) in self.units.iter().enumerate() {
            match content.definition(u.definition.as_str()) {
                None => out.push(format!("unit {}: no definition {}", i + 1, u.definition)),
                Some(d) if d.kind == Kind::Unit && (u.x.fract() != 0.0 || u.y.fract() != 0.0) => {
                    out.push(format!(
                        "unit {}: ({}, {}) is not a cell centre (whole cells)",
                        i + 1,
                        u.x,
                        u.y
                    ))
                }
                Some(d) if !matches!(d.kind, Kind::Unit | Kind::Building) => out.push(format!(
                    "unit {}: {} is neither a unit nor a building",
                    i + 1,
                    u.definition
                )),
                _ => {}
            }
            if !inside(u.x, u.y) {
                out.push(format!(
                    "unit {}: ({}, {}) is off the grid",
                    i + 1,
                    u.x,
                    u.y
                ));
            }
            if let Some(g) = u.group.as_deref() {
                match owners.iter().find(|(n, _)| *n == g) {
                    Some((_, o)) if *o != u.owner => {
                        out.push(format!("group {g} mixes owners"));
                    }
                    Some(_) => {}
                    None => owners.push((g, u.owner)),
                }
            }
        }
        let groups = self.groups();
        for (i, o) in self.orders.iter().enumerate() {
            let n = i + 1;
            if !groups.contains(&o.group.as_str()) {
                out.push(format!("order {n}: no group {}", o.group));
            }
            if o.tick >= self.duration {
                out.push(format!("order {n}: tick {} is after the run ends", o.tick));
            }
            if o.order.has_point() {
                match (o.x, o.y) {
                    (Some(x), Some(y)) if inside(x, y) => {}
                    _ => out.push(format!("order {n}: needs a point on the grid")),
                }
            }
            if o.order == OrderKind::Attack
                && !o.target.as_deref().is_some_and(|t| groups.contains(&t))
            {
                out.push(format!("order {n}: needs a target group"));
            }
        }
        out
    }
}

/// The scenarios in `data`'s [`LABS_FOLDER`], sorted by id (the file name without
/// [`EXTENSION`]).
pub fn library(data: &Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = std::fs::read_dir(data.join(LABS_FOLDER))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            let id = name.strip_suffix(EXTENSION)?.to_string();
            Some((id, path))
        })
        .collect();
    out.sort();
    out
}

/// Reads a scenario.
pub fn read(path: &Path) -> Result<Lab, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The scenario as its file holds it: two-space JSON with a trailing newline.
pub fn to_text(lab: &Lab) -> String {
    let mut text = serde_json::to_string_pretty(lab).expect("scenarios serialise");
    text.push('\n');
    text
}

/// Writes a scenario.
pub fn write(path: &Path, lab: &Lab) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, to_text(lab)).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_cells_round_trip_through_merged_spans() {
        let mut lab = Lab::new("t", 64);
        let mut cells = vec![false; 64 * 64];
        for x in 3..9 {
            cells[10 * 64 + x] = true;
        }
        cells[10 * 64 + 63] = true;
        cells[63 * 64] = true;
        lab.set_closed_cells(&cells);
        assert_eq!(
            lab.closed,
            [
                Span {
                    y: 10,
                    x: 3,
                    length: 6
                },
                Span {
                    y: 10,
                    x: 63,
                    length: 1
                },
                Span {
                    y: 63,
                    x: 0,
                    length: 1
                },
            ]
        );
        assert_eq!(lab.closed_cells(), cells);
    }
}
