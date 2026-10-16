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
    Panel,
    File,
    Send,
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
        Icon::Panel => vec![
            Closed(vec![[3.0, 4.0], [21.0, 4.0], [21.0, 20.0], [3.0, 20.0]]),
            Open(vec![[15.0, 4.0], [15.0, 20.0]]),
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
