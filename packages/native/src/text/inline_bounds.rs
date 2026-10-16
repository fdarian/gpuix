//! The box of an inline element.
//!
//! A `<span>` has no host element to report its own box, so the paragraph that
//! paints it does: each line fragment of its bytes, joined into the one rect
//! `getBoundingClientRect` answers with in the DOM. It is recorded during paint
//! like every other element's bounds, and so are the observers' reports, which
//! is why a span can be observed and reads as gone when it stops painting.

use std::ops::Range;

use gpui::{App, Bounds, LayoutInsets, Pixels, TextLayout, Window};

use crate::automation::{record_bounds, PaintReporter};

use super::paint::range_rects;

/// An inline element of a paragraph that has a box of its own.
pub struct InlineBox {
    pub id: u64,
    pub range: Range<usize>,
    /// Paint-driven events and observers on the element, see
    /// [`crate::automation::PaintReporter`].
    pub report: Option<PaintReporter>,
}

/// The rect of each line `range` covers, in window coordinates. A range that
/// wraps has one per line, and one that covers no glyph has none.
pub fn fragments(layout: &TextLayout, range: &Range<usize>) -> Vec<Bounds<Pixels>> {
    range_rects(layout, range, 0.0, 0.0)
}

/// The smallest rect that holds every fragment, or `None` for an element with
/// no fragment, which has no box.
pub fn union(fragments: &[Bounds<Pixels>]) -> Option<Bounds<Pixels>> {
    fragments
        .iter()
        .copied()
        .reduce(|joined, fragment| joined.union(&fragment))
}

/// Records the painted box of every inline element of the paragraph `layout`
/// belongs to and reports it to the element's listeners. Call it from paint.
pub fn record(layout: &TextLayout, boxes: &[InlineBox], window: &mut Window, cx: &mut App) {
    for inline in boxes {
        let Some(bounds) = union(&fragments(layout, &inline.range)) else {
            continue;
        };
        // An inline box has no layout node, so no padding or border to report.
        record_bounds(inline.id, bounds, LayoutInsets::default());
        if let Some(report) = &inline.report {
            report(bounds, LayoutInsets::default(), window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size};

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    #[test]
    fn no_fragment_is_no_box() {
        assert_eq!(union(&[]), None);
    }

    #[test]
    fn one_line_is_its_fragment() {
        assert_eq!(union(&[rect(30.0, 0.0, 40.0, 20.0)]), Some(rect(30.0, 0.0, 40.0, 20.0)));
    }

    #[test]
    fn a_span_that_wraps_spans_the_lines_from_the_first_x_to_the_widest_edge() {
        // "ab cd" wrapped: the tail of line one, then the head of line two.
        let joined = union(&[rect(60.0, 0.0, 40.0, 20.0), rect(0.0, 20.0, 30.0, 20.0)]);
        assert_eq!(joined, Some(rect(0.0, 0.0, 100.0, 40.0)));
    }

    #[test]
    fn a_span_over_three_lines_covers_the_full_line_between() {
        let joined = union(&[
            rect(20.0, 0.0, 80.0, 20.0),
            rect(0.0, 20.0, 100.0, 20.0),
            rect(0.0, 40.0, 50.0, 20.0),
        ]);
        assert_eq!(joined, Some(rect(0.0, 0.0, 100.0, 60.0)));
    }
}
