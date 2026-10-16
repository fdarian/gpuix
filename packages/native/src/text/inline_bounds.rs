//! The box of an inline element.
//!
//! A `<span>` has no host element to report its own box, so the paragraph that
//! paints it does: each line fragment of its bytes, joined into the one rect
//! `getBoundingClientRect` answers with in the DOM. It is recorded during paint
//! like every other element's bounds, and so are the observers' reports, which
//! is why a span can be observed and reads as gone when it stops painting.
//!
//! The box is the border box, as in CSS, and [`border_box_rows`] is the one place
//! that works it out for every consumer (bounds, hit testing, cursor hitboxes,
//! the outline and the painted background and border). Horizontally the rows
//! the layout gives for a range are margin box rows at the first and last
//! fragment, and the span's margin comes off them ([`super::inline_insets`] has
//! the model). Vertically a row is a line box, but an inline box is as tall as
//! the content area of its font, whatever the `lineHeight`: the font's ascent and
//! descent, centred in the line box as gpui paints the glyphs, plus the box's
//! vertical padding and border.

use std::ops::Range;

use gpui::{point, size, App, Bounds, Pixels, TextLayout, Window};

use crate::automation::{record_bounds, PaintReporter};

use super::inline_insets::{BoxEdges, Edges};
use super::paint::range_rows;

/// An inline element of a paragraph that has a box of its own.
pub struct InlineBox {
    pub id: u64,
    pub range: Range<usize>,
    pub edges: BoxEdges,
    /// Paint-driven events and observers on the element, see
    /// [`crate::automation::PaintReporter`].
    pub report: Option<PaintReporter>,
}

/// One line fragment of a box: the line box it is on, where the baseline of that
/// row is, and the metrics of the font of the box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pub line: Bounds<Pixels>,
    /// The y coordinate of the baseline of the row.
    pub baseline: Pixels,
    pub ascent: Pixels,
    pub descent: Pixels,
}

impl Row {
    /// The band of the row the font of the box occupies: `ascent` above the
    /// baseline and `descent` below it. A `lineHeight` taller than the font leaves
    /// the same space above and below it, as gpui centres the font in the line
    /// box. A `lineHeight` shorter than the font puts the band beyond the line box,
    /// as the content area is in CSS.
    pub fn content_area(&self) -> Bounds<Pixels> {
        Bounds::new(
            point(self.line.origin.x, self.baseline - self.ascent),
            size(self.line.size.width, self.ascent + self.descent),
        )
    }
}

/// The line box of each line `range` covers, in window coordinates, with the
/// baseline of the row and the metrics of the font of size `font_size` the box
/// has. A range that wraps has one per line, and one that covers no glyph has none.
pub fn rows(layout: &TextLayout, range: &Range<usize>, font_size: Pixels) -> Vec<Row> {
    let rows = range_rows(layout, range);
    if rows.is_empty() {
        return Vec::new();
    }
    let (ascent, descent) = layout
        .content_extent(range.clone(), font_size)
        .expect("a range with a row has a glyph");
    rows.into_iter()
        .map(|row| Row {
            line: row.rect,
            baseline: row.row.baseline,
            ascent,
            descent,
        })
        .collect()
}

/// The border box of a box with these `edges`, one rect for each line fragment of
/// `range`: the content area of the line grown by the box's vertical padding and
/// border, the first fragment starting after the margin and the last ending
/// before it. A fragment in the middle, or one that continues onto another line,
/// is as wide as its line's part of the range.
pub fn border_box_rows(layout: &TextLayout, range: &Range<usize>, edges: &BoxEdges) -> Vec<Bounds<Pixels>> {
    border_box(rows(layout, range, edges.font_size()), edges.at(layout))
}

/// [`border_box_rows`] of `rows`.
pub fn border_box(rows: Vec<Row>, edges: Edges) -> Vec<Bounds<Pixels>> {
    let grown = rows
        .iter()
        .map(|row| {
            let mut area = row.content_area();
            area.origin.y -= edges.top;
            area.size.height += edges.top + edges.bottom;
            area
        })
        .collect();
    inside(grown, edges)
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
        let Some(bounds) = union(&border_box_rows(layout, &inline.range, &inline.edges)) else {
            continue;
        };
        // An inline box has no layout node: its padding and border are its style's.
        let insets = inline.edges.layout_insets(layout);
        record_bounds(inline.id, bounds, insets);
        if let Some(report) = &inline.report {
            report(bounds, insets, window, cx);
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
            ..Edges::default()
        }
    }

    fn reaching(top: f32, bottom: f32) -> Edges {
        Edges {
            top: px(top),
            bottom: px(bottom),
            ..Edges::default()
        }
    }

    /// A 40px line box at `y` for a font of 12 above the baseline and 4 below,
    /// centred in the box as gpui paints it.
    fn row(x: f32, y: f32, width: f32) -> Row {
        Row {
            line: rect(x, y, width, 40.0),
            baseline: px(y + 24.0),
            ascent: px(12.0),
            descent: px(4.0),
        }
    }

    #[test]
    fn the_content_area_is_the_font_centred_in_the_line_box_whatever_its_height() {
        assert_eq!(row(10.0, 0.0, 50.0).content_area(), rect(10.0, 12.0, 50.0, 16.0));
        assert_eq!(row(10.0, 40.0, 50.0).content_area(), rect(10.0, 52.0, 50.0, 16.0));
        let tight = Row {
            line: rect(0.0, 0.0, 30.0, 10.0),
            baseline: px(9.0),
            ..row(0.0, 0.0, 30.0)
        };
        assert_eq!(
            tight.content_area(),
            rect(0.0, -3.0, 30.0, 16.0),
            "a line shorter than the font is overflowed on both sides"
        );
    }

    #[test]
    fn the_content_area_follows_the_baseline_of_a_row_that_is_not_centred() {
        // A 60px row of big text around a baseline at 45, and the 20px box of a
        // small span on that baseline, with its own font of 6 above and 2 below.
        let small = Row {
            line: rect(10.0, 100.0, 50.0, 60.0),
            baseline: px(145.0),
            ascent: px(6.0),
            descent: px(2.0),
        };
        assert_eq!(small.content_area(), rect(10.0, 139.0, 50.0, 8.0));
        let grown = border_box(vec![small], reaching(1.0, 3.0));
        assert_eq!(grown, vec![rect(10.0, 138.0, 50.0, 12.0)]);
    }

    #[test]
    fn the_border_box_is_the_content_area_not_the_line_box() {
        let rows = border_box(vec![row(10.0, 0.0, 50.0)], Edges::default());
        assert_eq!(rows, vec![rect(10.0, 12.0, 50.0, 16.0)]);
    }

    #[test]
    fn vertical_padding_and_border_grow_the_border_box_beyond_the_line_box() {
        let rows = border_box(vec![row(10.0, 0.0, 50.0)], reaching(15.0, 20.0));
        assert_eq!(rows, vec![rect(10.0, -3.0, 50.0, 51.0)]);
    }

    #[test]
    fn every_fragment_of_a_wrapped_box_grows_and_only_the_ends_are_cut() {
        let edges = Edges {
            start: px(4.0),
            end: px(6.0),
            top: px(2.0),
            bottom: px(3.0),
        };
        let rows = border_box(
            vec![row(60.0, 0.0, 40.0), row(0.0, 40.0, 100.0), row(0.0, 80.0, 30.0)],
            edges,
        );
        assert_eq!(
            rows,
            vec![
                rect(64.0, 10.0, 36.0, 21.0),
                rect(0.0, 50.0, 100.0, 21.0),
                rect(0.0, 90.0, 24.0, 21.0),
            ]
        );
    }

    #[test]
    fn a_box_with_no_row_has_no_border_box() {
        assert!(border_box(Vec::new(), reaching(2.0, 2.0)).is_empty());
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
