//! Ear-clipping triangulation with holes: mapbox/earcut 3.0.2, as bundled with three.js
//! 0.185 and used by the old game's ground mesh (`ShapeUtils.triangulateShape`). Ported
//! faithfully: the triangulation decides the navigation polygons, so it must not change.
//! Only `+ − × ÷` and comparisons are used, which are exact and identical everywhere.

const NIL: usize = usize::MAX;

#[derive(Debug, Clone, Copy)]
struct Node {
    /// Vertex index in the input.
    i: usize,
    x: f64,
    y: f64,
    prev: usize,
    next: usize,
    z: i32,
    prev_z: usize,
    next_z: usize,
    steiner: bool,
}

struct Earcut<'a> {
    data: &'a [f64],
    nodes: Vec<Node>,
    triangles: Vec<usize>,
    min_x: f64,
    min_y: f64,
    inv_size: f64,
}

/// Triangulates `outer` with `holes` (each a ring of points). Returns vertex indices into
/// the concatenation `outer ++ holes…`, three per triangle (old `triangulateShape`).
pub fn triangulate_shape(outer: &[(f64, f64)], holes: &[Vec<(f64, f64)>]) -> Vec<usize> {
    let trim = |ring: &[(f64, f64)]| -> usize {
        let l = ring.len();
        if l > 2 && ring[l - 1] == ring[0] {
            l - 1
        } else {
            l
        }
    };
    let mut data = Vec::new();
    let outer_len = trim(outer);
    data.extend(outer[..outer_len].iter().flat_map(|&(x, y)| [x, y]));
    let mut hole_indices = Vec::with_capacity(holes.len());
    let mut index = outer_len;
    for hole in holes {
        let len = trim(hole);
        hole_indices.push(index);
        index += len;
        data.extend(hole[..len].iter().flat_map(|&(x, y)| [x, y]));
    }
    earcut(&data, &hole_indices)
}

/// mapbox earcut over flat `[x0, y0, x1, y1, …]` with hole start indices (in points).
pub fn earcut(data: &[f64], hole_indices: &[usize]) -> Vec<usize> {
    let mut e = Earcut {
        data,
        nodes: Vec::with_capacity(data.len() / 2 * 3 / 2 + 8),
        triangles: Vec::new(),
        min_x: 0.0,
        min_y: 0.0,
        inv_size: 0.0,
    };
    let has_holes = !hole_indices.is_empty();
    let outer_len = if has_holes {
        hole_indices[0] * 2
    } else {
        data.len()
    };
    let mut outer = e.linked_list(0, outer_len, true);
    if outer == NIL || e.nodes[outer].next == e.nodes[outer].prev {
        return e.triangles;
    }
    if has_holes {
        outer = e.eliminate_holes(hole_indices, outer);
    }
    if data.len() > 80 * 2 {
        let mut min_x = data[0];
        let mut min_y = data[1];
        let (mut max_x, mut max_y) = (min_x, min_y);
        let mut i = 2;
        while i < outer_len {
            let (x, y) = (data[i], data[i + 1]);
            if x < min_x {
                min_x = x;
            }
            if y < min_y {
                min_y = y;
            }
            if x > max_x {
                max_x = x;
            }
            if y > max_y {
                max_y = y;
            }
            i += 2;
        }
        let size = (max_x - min_x).max(max_y - min_y);
        e.min_x = min_x;
        e.min_y = min_y;
        e.inv_size = if size != 0.0 { 32767.0 / size } else { 0.0 };
    }
    e.earcut_linked(outer, 0);
    e.triangles
}

impl Earcut<'_> {
    fn create(&mut self, i: usize, x: f64, y: f64) -> usize {
        self.nodes.push(Node {
            i,
            x,
            y,
            prev: NIL,
            next: NIL,
            z: 0,
            prev_z: NIL,
            next_z: NIL,
            steiner: false,
        });
        self.nodes.len() - 1
    }

    fn insert(&mut self, i: usize, x: f64, y: f64, last: usize) -> usize {
        let p = self.create(i, x, y);
        if last == NIL {
            self.nodes[p].prev = p;
            self.nodes[p].next = p;
        } else {
            let next = self.nodes[last].next;
            self.nodes[p].next = next;
            self.nodes[p].prev = last;
            self.nodes[next].prev = p;
            self.nodes[last].next = p;
        }
        p
    }

    fn remove(&mut self, p: usize) {
        let Node {
            prev,
            next,
            prev_z,
            next_z,
            ..
        } = self.nodes[p];
        self.nodes[next].prev = prev;
        self.nodes[prev].next = next;
        if prev_z != NIL {
            self.nodes[prev_z].next_z = next_z;
        }
        if next_z != NIL {
            self.nodes[next_z].prev_z = prev_z;
        }
    }

    fn equals(&self, a: usize, b: usize) -> bool {
        self.nodes[a].x == self.nodes[b].x && self.nodes[a].y == self.nodes[b].y
    }

    fn area(&self, p: usize, q: usize, r: usize) -> f64 {
        let (p, q, r) = (&self.nodes[p], &self.nodes[q], &self.nodes[r]);
        (q.y - p.y) * (r.x - q.x) - (q.x - p.x) * (r.y - q.y)
    }

    fn signed_area(&self, start: usize, end: usize) -> f64 {
        let d = self.data;
        let mut sum = 0.0;
        let mut j = end - 2;
        let mut i = start;
        while i < end {
            sum += (d[j] - d[i]) * (d[i + 1] + d[j + 1]);
            j = i;
            i += 2;
        }
        sum
    }

    fn linked_list(&mut self, start: usize, end: usize, clockwise: bool) -> usize {
        let mut last = NIL;
        if start >= end {
            return last;
        }
        if clockwise == (self.signed_area(start, end) > 0.0) {
            let mut i = start;
            while i < end {
                last = self.insert(i / 2, self.data[i], self.data[i + 1], last);
                i += 2;
            }
        } else {
            let mut i = end - 2;
            loop {
                last = self.insert(i / 2, self.data[i], self.data[i + 1], last);
                if i < start + 2 {
                    break;
                }
                i -= 2;
            }
        }
        if last != NIL && self.equals(last, self.nodes[last].next) {
            self.remove(last);
            last = self.nodes[last].next;
        }
        last
    }

    fn filter_points(&mut self, start: usize, end: usize) -> usize {
        if start == NIL {
            return start;
        }
        let mut end = if end == NIL { start } else { end };
        let mut p = start;
        loop {
            let mut again = false;
            let (prev, next) = (self.nodes[p].prev, self.nodes[p].next);
            if !self.nodes[p].steiner && (self.equals(p, next) || self.area(prev, p, next) == 0.0)
            {
                self.remove(p);
                p = self.nodes[p].prev;
                end = p;
                if p == self.nodes[p].next {
                    break;
                }
                again = true;
            } else {
                p = next;
            }
            if !(again || p != end) {
                break;
            }
        }
        end
    }

    fn earcut_linked(&mut self, ear: usize, pass: u8) {
        if ear == NIL {
            return;
        }
        let mut ear = ear;
        if pass == 0 && self.inv_size != 0.0 {
            self.index_curve(ear);
        }
        let mut stop = ear;
        while self.nodes[ear].prev != self.nodes[ear].next {
            let prev = self.nodes[ear].prev;
            let next = self.nodes[ear].next;
            let is_ear = if self.inv_size != 0.0 {
                self.is_ear_hashed(ear)
            } else {
                self.is_ear(ear)
            };
            if is_ear {
                self.triangles
                    .extend([self.nodes[prev].i, self.nodes[ear].i, self.nodes[next].i]);
                self.remove(ear);
                ear = self.nodes[next].next;
                stop = ear;
                continue;
            }
            ear = next;
            if ear == stop {
                match pass {
                    0 => {
                        let filtered = self.filter_points(ear, NIL);
                        self.earcut_linked(filtered, 1);
                    }
                    1 => {
                        let filtered = self.filter_points(ear, NIL);
                        let cured = self.cure_local_intersections(filtered);
                        self.earcut_linked(cured, 2);
                    }
                    _ => self.split_earcut(ear),
                }
                break;
            }
        }
    }

    fn is_ear(&self, ear: usize) -> bool {
        let (a, b, c) = (self.nodes[ear].prev, ear, self.nodes[ear].next);
        if self.area(a, b, c) >= 0.0 {
            return false;
        }
        let (na, nb, nc) = (self.nodes[a], self.nodes[b], self.nodes[c]);
        let x0 = na.x.min(nb.x).min(nc.x);
        let y0 = na.y.min(nb.y).min(nc.y);
        let x1 = na.x.max(nb.x).max(nc.x);
        let y1 = na.y.max(nb.y).max(nc.y);
        let mut p = nc.next;
        while p != a {
            let n = self.nodes[p];
            if n.x >= x0
                && n.x <= x1
                && n.y >= y0
                && n.y <= y1
                && point_in_triangle_except_first(na.x, na.y, nb.x, nb.y, nc.x, nc.y, n.x, n.y)
                && self.area(n.prev, p, n.next) >= 0.0
            {
                return false;
            }
            p = n.next;
        }
        true
    }

    fn is_ear_hashed(&self, ear: usize) -> bool {
        let (a, b, c) = (self.nodes[ear].prev, ear, self.nodes[ear].next);
        if self.area(a, b, c) >= 0.0 {
            return false;
        }
        let (na, nb, nc) = (self.nodes[a], self.nodes[b], self.nodes[c]);
        let x0 = na.x.min(nb.x).min(nc.x);
        let y0 = na.y.min(nb.y).min(nc.y);
        let x1 = na.x.max(nb.x).max(nc.x);
        let y1 = na.y.max(nb.y).max(nc.y);
        let min_z = self.z_order(x0, y0);
        let max_z = self.z_order(x1, y1);
        let inside = |p: usize| {
            let n = &self.nodes[p];
            n.x >= x0
                && n.x <= x1
                && n.y >= y0
                && n.y <= y1
                && p != a
                && p != c
                && point_in_triangle_except_first(na.x, na.y, nb.x, nb.y, nc.x, nc.y, n.x, n.y)
                && self.area(n.prev, p, n.next) >= 0.0
        };
        let mut p = self.nodes[ear].prev_z;
        let mut n = self.nodes[ear].next_z;
        while p != NIL && self.nodes[p].z >= min_z && n != NIL && self.nodes[n].z <= max_z {
            if inside(p) {
                return false;
            }
            p = self.nodes[p].prev_z;
            if inside(n) {
                return false;
            }
            n = self.nodes[n].next_z;
        }
        while p != NIL && self.nodes[p].z >= min_z {
            if inside(p) {
                return false;
            }
            p = self.nodes[p].prev_z;
        }
        while n != NIL && self.nodes[n].z <= max_z {
            if inside(n) {
                return false;
            }
            n = self.nodes[n].next_z;
        }
        true
    }

    fn cure_local_intersections(&mut self, start: usize) -> usize {
        let mut start = start;
        let mut p = start;
        loop {
            let a = self.nodes[p].prev;
            let b = self.nodes[self.nodes[p].next].next;
            let pn = self.nodes[p].next;
            if !self.equals(a, b)
                && self.intersects(a, p, pn, b)
                && self.locally_inside(a, b)
                && self.locally_inside(b, a)
            {
                self.triangles
                    .extend([self.nodes[a].i, self.nodes[p].i, self.nodes[b].i]);
                self.remove(p);
                let next = self.nodes[p].next;
                self.remove(next);
                p = b;
                start = b;
            }
            p = self.nodes[p].next;
            if p == start {
                break;
            }
        }
        self.filter_points(p, NIL)
    }

    fn split_earcut(&mut self, start: usize) {
        let mut a = start;
        loop {
            let mut b = self.nodes[self.nodes[a].next].next;
            while b != self.nodes[a].prev {
                if self.nodes[a].i != self.nodes[b].i && self.is_valid_diagonal(a, b) {
                    let c = self.split_polygon(a, b);
                    let a_next = self.nodes[a].next;
                    let a = self.filter_points(a, a_next);
                    let c_next = self.nodes[c].next;
                    let c = self.filter_points(c, c_next);
                    self.earcut_linked(a, 0);
                    self.earcut_linked(c, 0);
                    return;
                }
                b = self.nodes[b].next;
            }
            a = self.nodes[a].next;
            if a == start {
                break;
            }
        }
    }

    fn eliminate_holes(&mut self, hole_indices: &[usize], outer: usize) -> usize {
        let mut queue = Vec::with_capacity(hole_indices.len());
        for (i, &index) in hole_indices.iter().enumerate() {
            let start = index * 2;
            let end = hole_indices
                .get(i + 1)
                .map_or(self.data.len(), |&next| next * 2);
            let list = self.linked_list(start, end, false);
            if list == NIL {
                continue;
            }
            if list == self.nodes[list].next {
                self.nodes[list].steiner = true;
            }
            queue.push(self.leftmost(list));
        }
        queue.sort_by(|&a, &b| {
            let (na, nb) = (&self.nodes[a], &self.nodes[b]);
            let mut result = na.x - nb.x;
            if result == 0.0 {
                result = na.y - nb.y;
                if result == 0.0 {
                    let (an, bn) = (&self.nodes[na.next], &self.nodes[nb.next]);
                    let a_slope = (an.y - na.y) / (an.x - na.x);
                    let b_slope = (bn.y - nb.y) / (bn.x - nb.x);
                    result = a_slope - b_slope;
                }
            }
            // JavaScript treats a NaN comparison as equal.
            result.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut outer = outer;
        for hole in queue {
            outer = self.eliminate_hole(hole, outer);
        }
        outer
    }

    fn eliminate_hole(&mut self, hole: usize, outer: usize) -> usize {
        let bridge = self.find_hole_bridge(hole, outer);
        if bridge == NIL {
            return outer;
        }
        let reverse = self.split_polygon(bridge, hole);
        let reverse_next = self.nodes[reverse].next;
        self.filter_points(reverse, reverse_next);
        let bridge_next = self.nodes[bridge].next;
        self.filter_points(bridge, bridge_next)
    }

    fn find_hole_bridge(&self, hole: usize, outer: usize) -> usize {
        let mut p = outer;
        let (hx, hy) = (self.nodes[hole].x, self.nodes[hole].y);
        let mut qx = f64::NEG_INFINITY;
        let mut m = NIL;
        if self.equals(hole, p) {
            return p;
        }
        loop {
            let next = self.nodes[p].next;
            if self.equals(hole, next) {
                return next;
            }
            let (np, nn) = (&self.nodes[p], &self.nodes[next]);
            if hy <= np.y && hy >= nn.y && nn.y != np.y {
                let x = np.x + (hy - np.y) * (nn.x - np.x) / (nn.y - np.y);
                if x <= hx && x > qx {
                    qx = x;
                    m = if np.x < nn.x { p } else { next };
                    if x == hx {
                        return m;
                    }
                }
            }
            p = next;
            if p == outer {
                break;
            }
        }
        if m == NIL {
            return NIL;
        }
        let stop = m;
        let (mx, my) = (self.nodes[m].x, self.nodes[m].y);
        let mut tan_min = f64::INFINITY;
        p = m;
        loop {
            let (px, py) = (self.nodes[p].x, self.nodes[p].y);
            if hx >= px
                && px >= mx
                && hx != px
                && point_in_triangle(
                    if hy < my { hx } else { qx },
                    hy,
                    mx,
                    my,
                    if hy < my { qx } else { hx },
                    hy,
                    px,
                    py,
                )
            {
                let tan = (hy - py).abs() / (hx - px);
                if self.locally_inside(p, hole)
                    && (tan < tan_min
                        || (tan == tan_min
                            && (px > self.nodes[m].x
                                || (px == self.nodes[m].x && self.sector_contains_sector(m, p)))))
                {
                    m = p;
                    tan_min = tan;
                }
            }
            p = self.nodes[p].next;
            if p == stop {
                break;
            }
        }
        m
    }

    fn sector_contains_sector(&self, m: usize, p: usize) -> bool {
        self.area(self.nodes[m].prev, m, self.nodes[p].prev) < 0.0
            && self.area(self.nodes[p].next, m, self.nodes[m].next) < 0.0
    }

    fn index_curve(&mut self, start: usize) {
        let mut p = start;
        loop {
            if self.nodes[p].z == 0 {
                self.nodes[p].z = self.z_order(self.nodes[p].x, self.nodes[p].y);
            }
            self.nodes[p].prev_z = self.nodes[p].prev;
            self.nodes[p].next_z = self.nodes[p].next;
            p = self.nodes[p].next;
            if p == start {
                break;
            }
        }
        let tail = self.nodes[p].prev_z;
        self.nodes[tail].next_z = NIL;
        self.nodes[p].prev_z = NIL;
        self.sort_linked(p);
    }

    /// Simon Tatham's linked-list merge sort over the z-order links.
    fn sort_linked(&mut self, list: usize) {
        let mut list = list;
        let mut in_size = 1;
        loop {
            let mut p = list;
            list = NIL;
            let mut tail = NIL;
            let mut merges = 0;
            while p != NIL {
                merges += 1;
                let mut q = p;
                let mut p_size = 0;
                for _ in 0..in_size {
                    p_size += 1;
                    q = self.nodes[q].next_z;
                    if q == NIL {
                        break;
                    }
                }
                let mut q_size = in_size;
                while p_size > 0 || (q_size > 0 && q != NIL) {
                    let e;
                    if p_size != 0 && (q_size == 0 || q == NIL || self.nodes[p].z <= self.nodes[q].z)
                    {
                        e = p;
                        p = self.nodes[p].next_z;
                        p_size -= 1;
                    } else {
                        e = q;
                        q = self.nodes[q].next_z;
                        q_size -= 1;
                    }
                    if tail != NIL {
                        self.nodes[tail].next_z = e;
                    } else {
                        list = e;
                    }
                    self.nodes[e].prev_z = tail;
                    tail = e;
                }
                p = q;
            }
            self.nodes[tail].next_z = NIL;
            in_size *= 2;
            if merges <= 1 {
                break;
            }
        }
    }

    fn z_order(&self, x: f64, y: f64) -> i32 {
        let mut x = ((x - self.min_x) * self.inv_size) as i32;
        let mut y = ((y - self.min_y) * self.inv_size) as i32;
        x = (x | (x << 8)) & 0x00FF_00FF;
        x = (x | (x << 4)) & 0x0F0F_0F0F;
        x = (x | (x << 2)) & 0x3333_3333;
        x = (x | (x << 1)) & 0x5555_5555;
        y = (y | (y << 8)) & 0x00FF_00FF;
        y = (y | (y << 4)) & 0x0F0F_0F0F;
        y = (y | (y << 2)) & 0x3333_3333;
        y = (y | (y << 1)) & 0x5555_5555;
        x | (y << 1)
    }

    fn leftmost(&self, start: usize) -> usize {
        let mut p = start;
        let mut leftmost = start;
        loop {
            let (n, l) = (&self.nodes[p], &self.nodes[leftmost]);
            if n.x < l.x || (n.x == l.x && n.y < l.y) {
                leftmost = p;
            }
            p = n.next;
            if p == start {
                break;
            }
        }
        leftmost
    }

    fn is_valid_diagonal(&self, a: usize, b: usize) -> bool {
        let (na, nb) = (&self.nodes[a], &self.nodes[b]);
        self.nodes[na.next].i != nb.i
            && self.nodes[na.prev].i != nb.i
            && !self.intersects_polygon(a, b)
            && ((self.locally_inside(a, b)
                && self.locally_inside(b, a)
                && self.middle_inside(a, b)
                && (self.area(na.prev, a, nb.prev) != 0.0 || self.area(a, nb.prev, b) != 0.0))
                || (self.equals(a, b)
                    && self.area(na.prev, a, na.next) > 0.0
                    && self.area(nb.prev, b, nb.next) > 0.0))
    }

    fn intersects(&self, p1: usize, q1: usize, p2: usize, q2: usize) -> bool {
        let o1 = sign(self.area(p1, q1, p2));
        let o2 = sign(self.area(p1, q1, q2));
        let o3 = sign(self.area(p2, q2, p1));
        let o4 = sign(self.area(p2, q2, q1));
        if o1 != o2 && o3 != o4 {
            return true;
        }
        (o1 == 0 && self.on_segment(p1, p2, q1))
            || (o2 == 0 && self.on_segment(p1, q2, q1))
            || (o3 == 0 && self.on_segment(p2, p1, q2))
            || (o4 == 0 && self.on_segment(p2, q1, q2))
    }

    fn on_segment(&self, p: usize, q: usize, r: usize) -> bool {
        let (p, q, r) = (&self.nodes[p], &self.nodes[q], &self.nodes[r]);
        q.x <= p.x.max(r.x) && q.x >= p.x.min(r.x) && q.y <= p.y.max(r.y) && q.y >= p.y.min(r.y)
    }

    fn intersects_polygon(&self, a: usize, b: usize) -> bool {
        let (ai, bi) = (self.nodes[a].i, self.nodes[b].i);
        let mut p = a;
        loop {
            let next = self.nodes[p].next;
            let (pi, ni) = (self.nodes[p].i, self.nodes[next].i);
            if pi != ai && ni != ai && pi != bi && ni != bi && self.intersects(p, next, a, b) {
                return true;
            }
            p = next;
            if p == a {
                break;
            }
        }
        false
    }

    fn locally_inside(&self, a: usize, b: usize) -> bool {
        let n = &self.nodes[a];
        if self.area(n.prev, a, n.next) < 0.0 {
            self.area(a, b, n.next) >= 0.0 && self.area(a, n.prev, b) >= 0.0
        } else {
            self.area(a, b, n.prev) < 0.0 || self.area(a, n.next, b) < 0.0
        }
    }

    fn middle_inside(&self, a: usize, b: usize) -> bool {
        let mut p = a;
        let mut inside = false;
        let px = (self.nodes[a].x + self.nodes[b].x) / 2.0;
        let py = (self.nodes[a].y + self.nodes[b].y) / 2.0;
        loop {
            let n = &self.nodes[p];
            let nn = &self.nodes[n.next];
            if ((n.y > py) != (nn.y > py))
                && nn.y != n.y
                && (px < (nn.x - n.x) * (py - n.y) / (nn.y - n.y) + n.x)
            {
                inside = !inside;
            }
            p = n.next;
            if p == a {
                break;
            }
        }
        inside
    }

    fn split_polygon(&mut self, a: usize, b: usize) -> usize {
        let (ai, ax, ay) = (self.nodes[a].i, self.nodes[a].x, self.nodes[a].y);
        let (bi, bx, by) = (self.nodes[b].i, self.nodes[b].x, self.nodes[b].y);
        let a2 = self.create(ai, ax, ay);
        let b2 = self.create(bi, bx, by);
        let an = self.nodes[a].next;
        let bp = self.nodes[b].prev;
        self.nodes[a].next = b;
        self.nodes[b].prev = a;
        self.nodes[a2].next = an;
        self.nodes[an].prev = a2;
        self.nodes[b2].next = a2;
        self.nodes[a2].prev = b2;
        self.nodes[bp].next = b2;
        self.nodes[b2].prev = bp;
        b2
    }
}

fn sign(v: f64) -> i8 {
    if v > 0.0 {
        1
    } else if v < 0.0 {
        -1
    } else {
        0
    }
}

#[allow(clippy::too_many_arguments)]
fn point_in_triangle(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64, px: f64, py: f64) -> bool {
    (cx - px) * (ay - py) >= (ax - px) * (cy - py)
        && (ax - px) * (by - py) >= (bx - px) * (ay - py)
        && (bx - px) * (cy - py) >= (cx - px) * (by - py)
}

#[allow(clippy::too_many_arguments)]
fn point_in_triangle_except_first(
    ax: f64,
    ay: f64,
    bx: f64,
    by: f64,
    cx: f64,
    cy: f64,
    px: f64,
    py: f64,
) -> bool {
    !(ax == px && ay == py) && point_in_triangle(ax, ay, bx, by, cx, cy, px, py)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(points: &[(f64, f64)], triangles: &[usize]) -> f64 {
        triangles
            .chunks(3)
            .map(|t| {
                let (a, b, c) = (points[t[0]], points[t[1]], points[t[2]]);
                ((b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)).abs() / 2.0
            })
            .sum()
    }

    #[test]
    fn squares_and_holes_cover_their_area() {
        let square = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let triangles = triangulate_shape(&square, &[]);
        assert_eq!(triangles.len(), 6);
        assert_eq!(area(&square, &triangles), 100.0);
        let hole = vec![(4.0, 4.0), (4.0, 6.0), (6.0, 6.0), (6.0, 4.0)];
        let triangles = triangulate_shape(&square, std::slice::from_ref(&hole));
        let all: Vec<_> = square.iter().copied().chain(hole).collect();
        assert_eq!(area(&all, &triangles), 96.0);
    }

    /// The exact triangles three.js 0.185 `ShapeUtils.triangulateShape` returns for these
    /// shapes (recorded from the old game's dependency): z-order hashing and holes.
    #[test]
    fn matches_the_reference_triangulation() {
        // A comb with 47 vertices (over the 80-point hashing threshold).
        let mut ring = vec![(0.0, 0.0)];
        for i in 0..22 {
            let x = f64::from(i) * 2.0;
            ring.push((x + 1.0, 5.0));
            ring.push((x + 2.0, 0.0));
        }
        ring.push((44.0, 10.0));
        ring.push((0.0, 10.0));
        let triangles = triangulate_shape(&ring, &[]);
        let reference = [
            46, 0, 1, 1, 2, 3, 3, 4, 5, 5, 6, 7, 7, 8, 9, 9, 10, 11, 11, 12, 13, 13, 14, 15, 15,
            16, 17, 17, 18, 19, 19, 20, 21, 21, 22, 23, 23, 24, 25, 25, 26, 27, 27, 28, 29, 29, 30,
            31, 31, 32, 33, 33, 34, 35, 35, 36, 37, 37, 38, 39, 39, 40, 41, 41, 42, 43, 43, 44, 45,
            45, 46, 1, 41, 43, 45, 45, 1, 3, 39, 41, 45, 45, 3, 5, 37, 39, 45, 45, 5, 7, 35, 37, 45,
            45, 7, 9, 33, 35, 45, 45, 9, 11, 31, 33, 45, 45, 11, 13, 29, 31, 45, 45, 13, 15, 27,
            29, 45, 45, 15, 17, 25, 27, 45, 45, 17, 19, 23, 25, 45, 45, 19, 21, 21, 23, 45,
        ];
        assert_eq!(triangles, reference);
        let square = |x: f64, y: f64, w: f64, h: f64| vec![(x, y), (x, y + h), (x + w, y + h), (x + w, y)];
        let outer = [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)];
        let holes = [
            square(4.0, 4.0, 4.0, 4.0),
            square(10.0, 10.0, 4.0, 4.0),
            square(4.0, 12.0, 2.0, 4.0),
        ];
        let reference = [
            5, 8, 9, 7, 4, 0, 3, 0, 12, 0, 4, 5, 7, 0, 1, 3, 12, 13, 12, 0, 5, 11, 8, 5, 6, 7, 1,
            2, 3, 13, 15, 12, 5, 11, 5, 6, 2, 13, 14, 15, 5, 9, 11, 6, 1, 2, 14, 15, 10, 11, 1, 2,
            15, 9, 10, 1, 2, 2, 9, 10,
        ];
        assert_eq!(triangulate_shape(&outer, &holes), reference);
    }
}
