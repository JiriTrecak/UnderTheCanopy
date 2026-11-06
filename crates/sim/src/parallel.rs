//! Deterministic parallel work (D4): independent pure computations spread over the worker
//! threads, their results merged in input order. Nothing a task computes may depend on
//! another task, on which thread runs it or on when it finishes, so every thread count
//! (one included) gives bit-identical results; only time differs.

use rayon::prelude::*;

/// `f` of every item, in the items' order, computed side by side.
pub fn map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync + Send) -> Vec<R> {
    if items.len() < 2 {
        return items.iter().map(f).collect();
    }
    items.par_iter().map(f).collect()
}

/// `f` on every item, side by side.
pub fn each<T: Send>(items: &mut [T], f: impl Fn(&mut T) + Sync + Send) {
    if items.len() < 2 {
        items.iter_mut().for_each(f);
        return;
    }
    items.par_iter_mut().for_each(f);
}

#[cfg(test)]
mod tests {
    #[test]
    fn results_keep_the_input_order() {
        let items: Vec<u64> = (0..10_000).collect();
        let squares = super::map(&items, |&x| x * x);
        assert!(
            squares
                .iter()
                .enumerate()
                .all(|(i, &s)| s == (i as u64).pow(2))
        );
    }
}
