//! Route smoothing (old `farthestClearWaypoint`, `src/sim/game/routeSmoothing.ts`).

/// The farthest waypoint index in `start..=end` that `clear` accepts, probing with
/// doubling strides and then a binary search. `start` is known clear. Visibility need not
/// be monotonic: the result is always an explicitly tested index.
pub fn farthest_clear_waypoint(start: usize, end: usize, mut clear: impl FnMut(usize) -> bool) -> usize {
    let mut farthest = start;
    let mut stride = 1;
    while farthest < end {
        let probe = end.min(start + stride);
        if clear(probe) {
            farthest = probe;
            stride *= 2;
            continue;
        }
        // An occluded midpoint may hide later clear candidates; missing that shortcut is
        // safe, accepting an unchecked one is not.
        let (mut low, mut high) = (farthest + 1, probe - 1);
        while low <= high {
            let mid = (low + high) / 2;
            if clear(mid) {
                farthest = mid;
                low = mid + 1;
            } else {
                if mid == 0 {
                    break;
                }
                high = mid - 1;
            }
        }
        break;
    }
    farthest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_last_visible_waypoint() {
        assert_eq!(farthest_clear_waypoint(0, 10, |i| i <= 6), 6);
        assert_eq!(farthest_clear_waypoint(0, 10, |_| true), 10);
        assert_eq!(farthest_clear_waypoint(3, 3, |_| false), 3);
        // Index 1 is checked by the stride; 2 is blocked; 3+ are never accepted unchecked.
        assert_eq!(farthest_clear_waypoint(0, 10, |i| i != 2 && i < 2), 1);
    }
}
