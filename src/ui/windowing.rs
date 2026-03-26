//! Pure helpers for viewport/window selection.

use std::ops::Range;

/// Compute the visible range for a linear collection.
pub fn visible_range(total: usize, scroll: usize, viewport: usize) -> Range<usize> {
    if total == 0 || viewport == 0 {
        return 0..0;
    }

    let start = scroll.min(total.saturating_sub(1));
    let end = start.saturating_add(viewport).min(total);
    start..end
}

/// Compute the visible range plus overscan.
pub fn overscanned_range(
    total: usize,
    scroll: usize,
    viewport: usize,
    overscan: usize,
) -> Range<usize> {
    let visible = visible_range(total, scroll, viewport);
    let start = visible.start.saturating_sub(overscan);
    let end = visible.end.saturating_add(overscan).min(total);
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_range_clamps_scroll() {
        assert_eq!(visible_range(10, 50, 4), 9..10);
        assert_eq!(visible_range(10, 3, 4), 3..7);
    }

    #[test]
    fn overscan_expands_visible_range() {
        assert_eq!(overscanned_range(20, 5, 4, 2), 3..11);
    }
}
