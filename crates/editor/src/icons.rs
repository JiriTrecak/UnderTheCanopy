//! Line icons drawn with egui's painter: strokes on a 24-unit grid in the style of the
//! mockup's (Lucide-like) icons, so the editor needs no icon font or images.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke};
use std::f32::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Logo,
    Map,
    Units,
    Abilities,
    Effects,
    Buildings,
    Races,
    Assets,
    Exit,
    Undo,
    Redo,
    Plus,
    Close,
    ChevronRight,
    ChevronLeft,
    ChevronDown,
    Search,
    Robot,
    PanelLeft,
    File,
    Send,
    // The block editor's tools.
    Raise,
    Level,
    Paint,
    Ramp,
    Cut,
    Cells,
    Face,
    Place,
    Round,
    Select,
    // The block editor's views.
    Blocks,
    World,
    Pathing,
    Split,
    // The map view's panels.
    PanelRight,
    Minimap,
    Symmetry,
}

/// One stroke of an icon: an open or closed polyline in grid units.
enum Part {
    Open(Vec<[f32; 2]>),
    Closed(Vec<[f32; 2]>),
}

fn arc(center: [f32; 2], radius: [f32; 2], from: f32, to: f32) -> Vec<[f32; 2]> {
    let steps = (((to - from).abs() / (PI / 12.0)).ceil() as usize).max(2);
    (0..=steps)
        .map(|i| {
            let a = from + (to - from) * i as f32 / steps as f32;
            [
                center[0] + radius[0] * a.cos(),
                center[1] + radius[1] * a.sin(),
            ]
        })
        .collect()
}

fn circle(center: [f32; 2], r: f32) -> Part {
    let mut points = arc(center, [r, r], 0.0, 2.0 * PI);
    points.pop();
    Part::Closed(points)
}

fn parts(icon: Icon) -> Vec<Part> {
    use Part::{Closed, Open};
    match icon {
        Icon::Logo => {
            let mut canopy = arc([12.0, 12.0], [8.5, 8.5], PI, 2.0 * PI);
            canopy.extend([
                [18.0, 11.2],
                [15.0, 11.3],
                [12.0, 12.0],
                [9.0, 11.3],
                [6.0, 11.2],
            ]);
            vec![Open(vec![[12.0, 21.0], [12.0, 12.0]]), Closed(canopy)]
        }
        Icon::Map => vec![
            Closed(vec![
                [9.0, 4.0],
                [3.0, 6.0],
                [3.0, 20.0],
                [9.0, 18.0],
                [15.0, 20.0],
                [21.0, 18.0],
                [21.0, 4.0],
                [15.0, 6.0],
            ]),
            Open(vec![[9.0, 4.0], [9.0, 18.0]]),
            Open(vec![[15.0, 6.0], [15.0, 20.0]]),
        ],
        Icon::Units => vec![
            circle([9.0, 8.0], 3.5),
            Open(arc([9.0, 20.0], [6.5, 5.5], PI, 2.0 * PI)),
            Open(arc([16.0, 8.0], [3.4, 3.4], -PI / 2.0, PI / 2.0)),
            Open(vec![[18.0, 14.8], [20.0, 16.0], [21.0, 17.8], [21.5, 20.0]]),
        ],
        Icon::Abilities => vec![
            Closed(vec![
                [12.0, 3.0],
                [13.8, 8.2],
                [19.0, 10.0],
                [13.8, 11.8],
                [12.0, 17.0],
                [10.2, 11.8],
                [5.0, 10.0],
                [10.2, 8.2],
            ]),
            Closed(vec![
                [19.0, 15.0],
                [19.7, 17.3],
                [22.0, 18.0],
                [19.7, 18.7],
                [19.0, 21.0],
                [18.3, 18.7],
                [16.0, 18.0],
                [18.3, 17.3],
            ]),
        ],
        Icon::Effects => vec![Closed(vec![
            [12.0, 3.0],
            [13.6, 6.2],
            [16.0, 9.2],
            [17.0, 13.0],
            [16.3, 15.6],
            [14.6, 17.4],
            [12.0, 18.2],
            [9.4, 17.4],
            [7.7, 15.6],
            [7.0, 13.0],
            [7.7, 10.4],
            [9.2, 8.4],
            [10.2, 10.6],
            [11.4, 11.4],
            [11.0, 7.6],
        ])],
        Icon::Buildings => vec![
            Open(vec![[3.0, 10.5], [12.0, 4.0], [21.0, 10.5]]),
            Open(vec![[5.0, 9.0], [5.0, 20.0], [19.0, 20.0], [19.0, 9.0]]),
            Open(vec![[10.0, 20.0], [10.0, 15.0], [14.0, 15.0], [14.0, 20.0]]),
        ],
        Icon::Races => vec![
            Open(vec![[5.0, 21.0], [5.0, 4.0]]),
            Open(vec![
                [5.0, 4.0],
                [17.0, 4.0],
                [14.5, 8.0],
                [17.0, 12.0],
                [5.0, 12.0],
            ]),
        ],
        Icon::Assets => vec![
            Closed(vec![
                [12.0, 3.0],
                [20.0, 7.5],
                [20.0, 16.5],
                [12.0, 21.0],
                [4.0, 16.5],
                [4.0, 7.5],
            ]),
            Open(vec![[4.0, 7.5], [12.0, 12.0], [20.0, 7.5]]),
            Open(vec![[12.0, 12.0], [12.0, 21.0]]),
        ],
        Icon::Exit => vec![
            Open(vec![[9.0, 21.0], [5.0, 21.0], [5.0, 3.0], [9.0, 3.0]]),
            Open(vec![[16.0, 17.0], [21.0, 12.0], [16.0, 7.0]]),
            Open(vec![[21.0, 12.0], [9.0, 12.0]]),
        ],
        Icon::Undo | Icon::Redo => {
            let mut tail = vec![[4.0, 9.0], [14.0, 9.0]];
            tail.extend(arc([14.0, 15.0], [6.0, 6.0], -PI / 2.0, PI / 2.0));
            tail.push([11.0, 21.0]);
            let mut parts = vec![Open(vec![[9.0, 14.0], [4.0, 9.0], [9.0, 4.0]]), Open(tail)];
            if icon == Icon::Redo {
                for part in &mut parts {
                    let (Open(points) | Closed(points)) = part;
                    for p in points {
                        p[0] = 24.0 - p[0];
                    }
                }
            }
            parts
        }
        Icon::Plus => vec![
            Open(vec![[12.0, 5.0], [12.0, 19.0]]),
            Open(vec![[5.0, 12.0], [19.0, 12.0]]),
        ],
        Icon::Close => vec![
            Open(vec![[6.0, 6.0], [18.0, 18.0]]),
            Open(vec![[18.0, 6.0], [6.0, 18.0]]),
        ],
        Icon::ChevronRight => vec![Open(vec![[9.0, 6.0], [15.0, 12.0], [9.0, 18.0]])],
        Icon::ChevronLeft => vec![Open(vec![[15.0, 6.0], [9.0, 12.0], [15.0, 18.0]])],
        Icon::ChevronDown => vec![Open(vec![[6.0, 9.0], [12.0, 15.0], [18.0, 9.0]])],
        Icon::Search => vec![
            circle([11.0, 11.0], 7.0),
            Open(vec![[20.0, 20.0], [16.5, 16.5]]),
        ],
        // A robot's head: the assistant.
        Icon::Robot => vec![
            Closed(vec![[4.0, 8.0], [20.0, 8.0], [20.0, 20.0], [4.0, 20.0]]),
            Open(vec![[12.0, 8.0], [12.0, 4.5]]),
            circle([12.0, 3.5], 1.2),
            Open(vec![[2.0, 12.5], [2.0, 15.5]]),
            Open(vec![[22.0, 12.5], [22.0, 15.5]]),
            circle([8.5, 13.0], 1.4),
            circle([15.5, 13.0], 1.4),
            Open(vec![[9.0, 17.0], [15.0, 17.0]]),
        ],
        // A window with its left column: the library.
        Icon::PanelLeft => vec![
            Closed(vec![[3.0, 4.0], [21.0, 4.0], [21.0, 20.0], [3.0, 20.0]]),
            Open(vec![[9.0, 4.0], [9.0, 20.0]]),
        ],
        Icon::File => vec![
            Closed(vec![
                [14.0, 3.0],
                [6.0, 3.0],
                [6.0, 21.0],
                [18.0, 21.0],
                [18.0, 7.0],
            ]),
            Open(vec![[14.0, 3.0], [14.0, 7.0], [18.0, 7.0]]),
        ],
        Icon::Send => vec![
            Open(vec![[12.0, 19.0], [12.0, 5.0]]),
            Open(vec![[6.0, 11.0], [12.0, 5.0], [18.0, 11.0]]),
        ],
        // A block with an arrow up out of it.
        Icon::Raise => vec![
            Closed(vec![[5.0, 14.0], [19.0, 14.0], [19.0, 20.0], [5.0, 20.0]]),
            Open(vec![[12.0, 11.0], [12.0, 3.0]]),
            Open(vec![[8.5, 6.5], [12.0, 3.0], [15.5, 6.5]]),
        ],
        // Two blocks of different heights brought to one line.
        Icon::Level => vec![
            Open(vec![[3.0, 9.0], [21.0, 9.0]]),
            Closed(vec![[4.0, 12.0], [11.0, 12.0], [11.0, 20.0], [4.0, 20.0]]),
            Closed(vec![[13.0, 15.0], [20.0, 15.0], [20.0, 20.0], [13.0, 20.0]]),
            Open(vec![[16.5, 12.5], [16.5, 10.5]]),
        ],
        // A brush.
        Icon::Paint => vec![
            Open(vec![[20.0, 4.0], [11.0, 13.0]]),
            Closed(vec![
                [11.0, 13.0],
                [8.0, 13.5],
                [6.0, 16.0],
                [4.0, 20.5],
                [8.5, 18.5],
                [10.5, 16.5],
                [11.5, 14.5],
            ]),
        ],
        // A slope down from a level.
        Icon::Ramp => vec![Closed(vec![
            [3.0, 20.0],
            [3.0, 7.0],
            [9.0, 7.0],
            [16.0, 17.0],
            [21.0, 17.0],
            [21.0, 20.0],
        ])],
        // A block with its corner cut off.
        Icon::Cut => vec![
            Closed(vec![
                [4.0, 4.0],
                [20.0, 4.0],
                [20.0, 12.0],
                [12.0, 20.0],
                [4.0, 20.0],
            ]),
            Open(vec![[16.0, 21.0], [21.0, 16.0]]),
        ],
        // A grid of cells.
        Icon::Cells => vec![
            Closed(vec![[4.0, 4.0], [20.0, 4.0], [20.0, 20.0], [4.0, 20.0]]),
            Open(vec![[9.33, 4.0], [9.33, 20.0]]),
            Open(vec![[14.67, 4.0], [14.67, 20.0]]),
            Open(vec![[4.0, 9.33], [20.0, 9.33]]),
            Open(vec![[4.0, 14.67], [20.0, 14.67]]),
        ],
        // A wall of stones: a cliff's face.
        Icon::Face => vec![
            Closed(vec![[3.0, 5.0], [21.0, 5.0], [21.0, 19.0], [3.0, 19.0]]),
            Open(vec![[3.0, 9.67], [21.0, 9.67]]),
            Open(vec![[3.0, 14.33], [21.0, 14.33]]),
            Open(vec![[9.0, 5.0], [9.0, 9.67]]),
            Open(vec![[15.0, 5.0], [15.0, 9.67]]),
            Open(vec![[12.0, 9.67], [12.0, 14.33]]),
            Open(vec![[9.0, 14.33], [9.0, 19.0]]),
            Open(vec![[15.0, 14.33], [15.0, 19.0]]),
        ],
        // A map pin.
        // Something set down on the ground: an arrow onto a spot.
        Icon::Place => {
            let mut ground = arc([12.0, 18.0], [8.5, 3.2], 0.0, 2.0 * PI);
            ground.pop();
            vec![
                Closed(ground),
                Open(vec![[12.0, 2.5], [12.0, 14.0]]),
                Open(vec![[7.5, 9.5], [12.0, 14.0], [16.5, 9.5]]),
            ]
        }
        // A block's corner rounded off.
        Icon::Round => {
            let mut edge = vec![[4.0, 20.0], [4.0, 13.0]];
            edge.extend(arc([13.0, 13.0], [9.0, 9.0], PI, 1.5 * PI));
            edge.push([20.0, 4.0]);
            vec![Open(edge)]
        }
        // An arrow cursor.
        Icon::Select => vec![Closed(vec![
            [5.0, 3.0],
            [5.0, 19.0],
            [9.5, 15.0],
            [12.5, 21.5],
            [15.0, 20.4],
            [12.0, 14.0],
            [18.0, 14.0],
        ])],
        // A block, seen from above a corner.
        Icon::Blocks => vec![
            Closed(vec![
                [12.0, 3.0],
                [20.0, 7.5],
                [20.0, 16.5],
                [12.0, 21.0],
                [4.0, 16.5],
                [4.0, 7.5],
            ]),
            Open(vec![[4.0, 7.5], [12.0, 12.0], [20.0, 7.5]]),
            Open(vec![[12.0, 12.0], [12.0, 21.0]]),
        ],
        // Hills under a sun: the world made of the blocks.
        Icon::World => vec![
            Open(vec![
                [2.5, 19.0],
                [9.0, 9.5],
                [13.0, 15.0],
                [16.0, 11.5],
                [21.5, 19.0],
            ]),
            Open(vec![[2.5, 19.0], [21.5, 19.0]]),
            circle([17.0, 5.5], 2.2),
        ],
        // A winding route between two stops: where units walk.
        Icon::Pathing => vec![
            Open(vec![[6.0, 17.5], [6.0, 12.0], [18.0, 12.0], [18.0, 6.5]]),
            circle([6.0, 19.5], 2.0),
            circle([18.0, 4.5], 2.0),
        ],
        // Two views side by side.
        Icon::Split => vec![
            Closed(vec![[2.5, 5.0], [11.0, 5.0], [11.0, 19.0], [2.5, 19.0]]),
            Closed(vec![[13.0, 5.0], [21.5, 5.0], [21.5, 19.0], [13.0, 19.0]]),
        ],
        // A window with its right column: the map's panel.
        Icon::PanelRight => vec![
            Closed(vec![[3.0, 4.0], [21.0, 4.0], [21.0, 20.0], [3.0, 20.0]]),
            Open(vec![[15.0, 4.0], [15.0, 20.0]]),
        ],
        // A map's square with the camera's view in it.
        Icon::Minimap => vec![
            Closed(vec![[3.0, 3.0], [21.0, 3.0], [21.0, 21.0], [3.0, 21.0]]),
            Closed(vec![[7.0, 11.0], [15.0, 11.0], [17.0, 17.0], [5.0, 17.0]]),
            circle([16.0, 7.0], 1.5),
        ],
        // Two halves mirrored across a dashed line.
        Icon::Symmetry => vec![
            Closed(vec![[3.0, 7.0], [9.5, 7.0], [9.5, 17.0], [3.0, 17.0]]),
            Closed(vec![[14.5, 7.0], [21.0, 7.0], [21.0, 17.0], [14.5, 17.0]]),
            Open(vec![[12.0, 3.0], [12.0, 6.0]]),
            Open(vec![[12.0, 10.5], [12.0, 13.5]]),
            Open(vec![[12.0, 18.0], [12.0, 21.0]]),
        ],
    }
}

/// Draws `icon` filling `rect` (square) in `color`.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let scale = rect.width().min(rect.height()) / 24.0;
    let origin = rect.center() - egui::vec2(12.0, 12.0) * scale;
    let stroke = Stroke::new((1.75 * scale).max(1.0), color);
    let map = |points: Vec<[f32; 2]>| -> Vec<Pos2> {
        points
            .into_iter()
            .map(|[x, y]| origin + egui::vec2(x, y) * scale)
            .collect()
    };
    for part in parts(icon) {
        painter.add(match part {
            Part::Open(points) => Shape::line(map(points), stroke),
            Part::Closed(points) => Shape::closed_line(map(points), stroke),
        });
    }
}

/// A square of `size` centred on `center`.
pub fn square(center: Pos2, size: f32) -> Rect {
    Rect::from_center_size(center, egui::vec2(size, size))
}
