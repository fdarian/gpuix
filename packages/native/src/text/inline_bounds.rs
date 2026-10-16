//! The box of an inline element.
//!
//! A `<span>` has no host element to report its own box, so the paragraph that
//! paints it does: each line fragment of its bytes, joined into the one rect
//! `getBoundingClientRect` answers with in the DOM. It is recorded during paint
//! like every other element's bounds, and so are the observers' reports, which
//! is why a span can be observed and reads as gone when it stops painting.
//!
//! The box is the border box, as in CSS: the rows the layout gives for a range
//! are margin box rows at the first and last fragment, and
//! [`border_box_rows`] takes the span's margin off them
//! ([`super::inline_insets`] has the model).

use std::ops::Range;

use gpui::{App, Bounds, LayoutInsets, Pixels, TextLayout, Window};

use crate::automation::{record_bounds, PaintReporter};

use super::inline_insets::Edges;
use super::paint::range_rects;

/// An inline element of a paragraph that has a box of its own.
pub struct InlineBox {
    pub id: u64,
    pub range: Range<usize>,
    pub edges: Edges,
    /// Paint-driven events and observers on the element, see
    /// [`crate::automation::PaintReporter`].
    pub report: Option<PaintReporter>,
}

/// The rect of each line `range` covers, in window coordinates. A range that
/// wraps has one per line, and one that covers no glyph has none.
pub fn fragments(layout: &TextLayout, range: &Range<usize>) -> Vec<Bounds<Pixels>> {
    range_rects(layout, range, 0.0, 0.0)
}

/// `fragments` of `range` as the border box of a box with these `edges`: the
/// first fragment starts after the margin, and the last ends before it. A
/// fragment in the middle, or one that continues onto another line, is as it
/// was.
pub fn border_box_rows(layout: &TextLayout, range: &Range<usize>, edges: Edges) -> Vec<Bounds<Pixels>> {
    inside(fragments(layout, range), edges)
}

/// `rows`, the margin box fragments of a box, moved in by its `edges`.
pub fn inside(mut rows: Vec<Bounds<Pixels>>, edges: Edges) -> Vec<Bounds<Pixels>> {
    if let Some(first) = rows.first_mut() {
        first.origin.x += edges.start;
        first.size.width = (first.size.width - edges.start).max(Pixels::ZERO);
    }
    if let Some(last) = rows.last_mut() {
        last.size.width = (last.size.width - edges.end).max(Pixels::ZERO);
    }
    rows
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
        let Some(bounds) = union(&border_box_rows(layout, &inline.range, inline.edges)) else {
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

    fn edges(start: f32, end: f32) -> Edges {
        Edges {
            start: px(start),
            end: px(end),
        }
    }

    #[test]
    fn a_box_on_one_line_loses_its_margin_on_both_ends() {
        // A margin box 10..70, border box 14..64.
        let rows = inside(vec![rect(10.0, 0.0, 60.0, 20.0)], edges(4.0, 6.0));
        assert_eq!(rows, vec![rect(14.0, 0.0, 50.0, 20.0)]);
    }

    #[test]
    fn a_box_over_two_lines_loses_its_start_on_the_first_and_its_end_on_the_last() {
        let rows = inside(
            vec![rect(60.0, 0.0, 40.0, 20.0), rect(0.0, 20.0, 30.0, 20.0)],
            edges(4.0, 6.0),
        );
        assert_eq!(rows, vec![rect(64.0, 0.0, 36.0, 20.0), rect(0.0, 20.0, 24.0, 20.0)]);
    }

    #[test]
    fn the_lines_between_are_whole() {
        let rows = inside(
            vec![
                rect(20.0, 0.0, 80.0, 20.0),
                rect(0.0, 20.0, 100.0, 20.0),
                rect(0.0, 40.0, 50.0, 20.0),
            ],
            edges(4.0, 6.0),
        );
        assert_eq!(rows[1], rect(0.0, 20.0, 100.0, 20.0));
        assert_eq!(rows[0], rect(24.0, 0.0, 76.0, 20.0));
        assert_eq!(rows[2], rect(0.0, 40.0, 44.0, 20.0));
        assert_eq!(
            union(&rows),
            Some(rect(0.0, 0.0, 100.0, 60.0)),
            "the box still spans the lines"
        );
    }

    #[test]
    fn a_negative_margin_widens_the_border_box() {
        let rows = inside(vec![rect(10.0, 0.0, 40.0, 20.0)], edges(-3.0, -2.0));
        assert_eq!(rows, vec![rect(7.0, 0.0, 45.0, 20.0)]);
    }

    #[test]
    fn a_box_with_no_fragment_has_no_rows() {
        assert!(inside(Vec::new(), edges(4.0, 6.0)).is_empty());
    }
}
