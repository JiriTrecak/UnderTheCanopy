//! The viewer's fog of war on the render thread: follows the view's `FogView` by revision
//! and hands the renderer only the cells that changed (bridge decks fold into the ground
//! cell under them: in sight when any floor is, as the old flat overview).

use canopy_render::{Gpu, Renderer, fog::FogCells};
use canopy_sim::view::FogView;

#[derive(Debug, Default)]
pub struct FogFeed {
    revision: Option<u32>,
    size: u32,
    flat: Vec<u8>,
    changed: Vec<u32>,
}

impl FogFeed {
    /// Brings the renderer's fog to `fog` (`None`: observers see everything).
    pub fn feed(&mut self, renderer: &mut Renderer, gpu: &Gpu, fog: Option<&FogView>) {
        let Some(fog) = fog else {
            if self.revision.take().is_some() {
                renderer.set_fog(gpu, None);
            }
            return;
        };
        let cells = (fog.size * fog.size) as usize;
        let full = self.revision.is_none() || self.size != fog.size;
        if !full && self.revision == Some(fog.revision) {
            return;
        }
        let changes = if full {
            None
        } else {
            fog.changes_since(self.revision.unwrap_or(0))
        };
        match changes {
            Some(nodes) => {
                self.changed.clear();
                for node in nodes {
                    let cell = if (node as usize) < cells {
                        node
                    } else {
                        match fog.deck_cells.get(node as usize - cells) {
                            Some(&cell) => cell,
                            None => continue,
                        }
                    };
                    self.flat[cell as usize] = fog.flat(cell);
                    self.changed.push(cell);
                }
                renderer.set_fog(
                    gpu,
                    Some(FogCells {
                        size: fog.size,
                        cells: &self.flat,
                        changed: Some(&self.changed),
                    }),
                );
            }
            None => {
                self.size = fog.size;
                self.flat.clear();
                self.flat.extend(fog.cells.iter().take(cells).copied());
                self.flat.resize(cells, 0);
                for (i, &cell) in fog.deck_cells.iter().enumerate() {
                    let node = fog.cells.get(cells + i).copied().unwrap_or(0);
                    if let Some(flat) = self.flat.get_mut(cell as usize) {
                        *flat = (*flat).max(node);
                    }
                }
                renderer.set_fog(
                    gpu,
                    Some(FogCells {
                        size: fog.size,
                        cells: &self.flat,
                        changed: None,
                    }),
                );
            }
        }
        self.revision = Some(fog.revision);
    }
}
