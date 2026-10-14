//! Path finding (old `src/sim/game/navigation.ts`, `sectorNavigation.ts`,
//! `groundNavigation.ts`, `meshGridPath.ts`, `routeSmoothing.ts`, `routeCorridors.ts`,
//! `src/shared/navigation/*` and `WalkSurfaces.path`). The algorithms and costs are the old
//! game's, since they define where units walk; the data layouts are new.

pub mod cells;
pub mod corridors;
pub mod grid;
pub mod mesh;
pub mod mesh_grid;
pub mod sectors;
pub mod smoothing;
pub mod surfaces;
