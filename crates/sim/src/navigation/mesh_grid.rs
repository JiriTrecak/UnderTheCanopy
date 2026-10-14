//! Converting a mesh polyline into grid cells when its rounded corners graze an obstacle
//! (old `snappedMeshGridPath`, `src/sim/game/meshGridPath.ts`). Corners retry on nearby
//! legal cell centres; every edge still uses the caller's directed body step.

use crate::spatial::geometry::{Fixed, append_ray_cells_from_end};

fn nearby(p: Fixed, size: u32, step: &mut impl FnMut(u32, u32) -> bool) -> Vec<Fixed> {
    let x = crate::terrain::round_half_up(f64::from(p.x) / 1000.0) as i32;
    let y = crate::terrain::round_half_up(f64::from(p.y) / 1000.0) as i32;
    let mut candidates = Vec::with_capacity(9);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= size as i32 || ny >= size as i32 {
                continue;
            }
            let cell = ny as u32 * size + nx as u32;
            if step(cell, cell) {
                candidates.push(Fixed::new(nx * 1000, ny * 1000));
            }
        }
    }
    let d2 = |q: &Fixed| {
        let (dx, dy) = (i64::from(q.x - p.x), i64::from(q.y - p.y));
        dx * dx + dy * dy
    };
    candidates.sort_by(|a, b| d2(a).cmp(&d2(b)).then(a.y.cmp(&b.y)).then(a.x.cmp(&b.x)));
    candidates
}

/// Grid cells joining `points` through checked corner choices, or `None`.
pub fn snapped_mesh_grid_path(
    points: &[Fixed],
    size: u32,
    mut step: impl FnMut(u32, u32) -> bool,
) -> Option<Vec<u32>> {
    if points.len() < 2 {
        return Some(Vec::new());
    }
    let last = points.len() - 1;
    let choices: Vec<Vec<Fixed>> = points
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            if i == 0 || i == last {
                vec![p]
            } else {
                nearby(p, size, &mut step)
            }
        })
        .collect();
    if let Some(path) = connect(&choices, size, &mut step) {
        return Some(path);
    }
    // A legal endpoint may need a cardinal approach around a corner.
    let mut joined = vec![vec![points[0]], nearby(points[0], size, &mut step)];
    joined.extend(choices[1..last].iter().cloned());
    joined.push(nearby(points[last], size, &mut step));
    joined.push(vec![points[last]]);
    if let Some(path) = connect(&joined, size, &mut step) {
        return Some(path);
    }
    // A clear segment may skim an obstacle between corners: allow joins along it.
    let mut samples = vec![vec![points[0]], nearby(points[0], size, &mut step)];
    for i in 1..points.len() {
        let (a, b) = (points[i - 1], points[i]);
        let span = (b.x - a.x).abs().max((b.y - a.y).abs());
        let count = (span + 7999) / 8000;
        for j in 1..count {
            let at = |from: i32, to: i32| {
                crate::terrain::round_half_up(
                    f64::from(from) + f64::from(to - from) * f64::from(j) / f64::from(count),
                ) as i32
            };
            samples.push(nearby(
                Fixed::new(at(a.x, b.x), at(a.y, b.y)),
                size,
                &mut step,
            ));
        }
        samples.push(if i == last {
            nearby(b, size, &mut step)
        } else {
            choices[i].clone()
        });
    }
    samples.push(vec![points[last]]);
    connect(&samples, size, &mut step)
}

/// Depth-first choice of one candidate per stage, remembering dead ends so the work stays
/// linear in the number of stages.
fn connect(
    choices: &[Vec<Fixed>],
    size: u32,
    step: &mut impl FnMut(u32, u32) -> bool,
) -> Option<Vec<u32>> {
    struct Frame {
        index: usize,
        next: usize,
        segment: Vec<u32>,
    }
    let mut failed: Vec<Vec<bool>> = choices.iter().map(|c| vec![false; c.len()]).collect();
    let mut stack = vec![Frame {
        index: 0,
        next: 0,
        segment: Vec::new(),
    }];
    while let Some(depth) = stack.len().checked_sub(1) {
        if depth == choices.len() - 1 {
            return Some(stack.into_iter().flat_map(|f| f.segment).collect());
        }
        let candidates = &choices[depth + 1];
        let mut advanced = None;
        while stack[depth].next < candidates.len() {
            let index = stack[depth].next;
            stack[depth].next += 1;
            if failed[depth + 1][index] {
                continue;
            }
            let mut segment = Vec::new();
            let from = choices[depth][stack[depth].index];
            if append_ray_cells_from_end(from, candidates[index], size, &mut *step, &mut segment) {
                advanced = Some(Frame {
                    index,
                    next: 0,
                    segment,
                });
                break;
            }
        }
        match advanced {
            Some(frame) => stack.push(frame),
            None => {
                failed[depth][stack[depth].index] = true;
                stack.pop();
            }
        }
    }
    None
}
