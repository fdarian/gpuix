//! Which inline box is under the pointer.
//!
//! An inline box has no hitbox: a paragraph is one painted string, so GPUI's hit
//! test stops at the container that holds it. The dispatcher asks here for the
//! boxes around the glyph under the pointer, and puts them in front of the
//! container in its hit chain. That makes a `<span>` an event target, with the
//! container and its ancestors on its path, as in the DOM.
//!
//! Only a point inside a glyph's cell hits a box. The blank area after the end of
//! a line, between two lines, or under a short paragraph belongs to the container.
//! The cell of a box's first and last glyph holds the box's margin, border and
//! padding, and the pointer is on the box only inside its border box: in the
//! margin it is on the boxes around it. A border box is as tall as the content
//! area of the font plus the box's vertical padding and border, not the line box
//! ([`super::inline_bounds::border_box_rows`]), so the part of a tall line above
//! or below its text belongs to the boxes around the span, or the container.
//! A point is looked up by the glyph cell it is in, so vertical padding that
//! reaches into the next line is on the span only where that line's glyph cell is
//! one of the span's.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use gpui::{Bounds, Pixels, Point, TextLayout, WindowId};

use crate::retained_tree::RetainedTree;

use super::inline::{Paragraph, Piece};
use super::inline_bounds::border_box_rows;
use super::inline_insets::{Edges, Insets};

/// The inline boxes of one paragraph that can be a target, by the bytes they
/// cover.
#[derive(Debug, PartialEq)]
pub struct Targets {
    /// The boxes around each text piece, outermost first.
    pieces: Vec<(Range<usize>, Vec<u64>)>,
    ranges: Vec<(u64, Range<usize>, Edges)>,
}

impl Targets {
    /// `None` when no box of the paragraph can be a target, which is every
    /// paragraph of plain text: it needs no lookup.
    ///
    /// `pointer_events_none` and `inert` are the container's, and a box takes
    /// itself out of the hit test the way an element does: `pointerEvents: "none"`
    /// until a descendant says `auto`, and `inert` for good.
    pub fn of(
        tree: &RetainedTree,
        paragraph: &Paragraph,
        pointer_events_none: bool,
        inert: bool,
        insets: &Insets,
    ) -> Option<Self> {
        let with_box = boxes_with_a_box(paragraph);
        let pieces: Vec<_> = paragraph
            .pieces
            .iter()
            .map(|piece| {
                let targets = piece_targets(tree, &with_box, piece, pointer_events_none, inert);
                (piece.range.clone(), targets)
            })
            .filter(|(_, targets)| !targets.is_empty())
            .collect();
        (!pieces.is_empty()).then(|| Self {
            pieces,
            ranges: paragraph
                .boxes
                .iter()
                .map(|(id, range)| (*id, range.clone(), insets.edges(*id)))
                .collect(),
        })
    }

    /// The boxes the pointer at `position`, in the cell of the glyph at `byte`,
    /// is on, innermost first, each with the rectangle of the first line fragment
    /// of its border box. `border_box` gives the rows of a box. A box with no
    /// rows covers no glyph, and one whose rows do not hold the point has the
    /// point in its margin, so neither is there.
    fn around(
        &self,
        byte: usize,
        position: Point<Pixels>,
        border_box: impl Fn(&Range<usize>, Edges) -> Vec<Bounds<Pixels>>,
    ) -> Vec<(u64, Bounds<Pixels>)> {
        let Some((_, boxes)) = self.pieces.iter().find(|(range, _)| range.contains(&byte)) else {
            return Vec::new();
        };
        boxes
            .iter()
            .rev()
            .filter_map(|id| {
                let (_, range, edges) = self.ranges.iter().find(|(boxed, ..)| boxed == id)?;
                let rows = border_box(range, *edges);
                rows.iter().any(|row| row.contains(&position)).then_some(())?;
                Some((*id, *rows.first()?))
            })
            .collect()
    }
}

/// The inline boxes of `paragraph` that cover text. A `display: contents`
/// element is in a piece's chain and has no box, so it is not one of them.
pub(super) fn boxes_with_a_box(paragraph: &Paragraph) -> HashSet<u64> {
    paragraph.boxes.iter().map(|(id, _)| *id).collect()
}

/// The boxes around `piece` the pointer can land on, outermost first.
pub(super) fn piece_targets(
    tree: &RetainedTree,
    with_box: &HashSet<u64>,
    piece: &Piece,
    pointer_events_none: bool,
    inert: bool,
) -> Vec<u64> {
    let (mut none, mut inert) = (pointer_events_none, inert);
    let mut targets = Vec::new();
    for id in &piece.boxes {
        let Some(element) = tree.elements.get(id) else {
            continue;
        };
        inert |= element.has_inert_prop();
        match element
            .style
            .as_deref()
            .and_then(|style| style.pointer_events.as_deref())
        {
            Some("none") => none = true,
            Some("auto") => none = false,
            _ => {}
        }
        if !none && !inert && with_box.contains(id) {
            targets.push(*id);
        }
    }
    targets
}

struct Entry {
    container: u64,
    layout: TextLayout,
    targets: Arc<Targets>,
}

thread_local! {
    /// The paragraphs painted this frame, in paint order, by window. Rebuilt every
    /// frame like the selection registry, because a list decides at paint time
    /// which rows exist. A window has its own, because every renderer numbers its
    /// containers from the same start and a window's frame must not clear another's.
    static PARAGRAPHS: RefCell<HashMap<WindowId, Vec<Entry>>> = RefCell::new(HashMap::new());
}

pub fn frame_reset(window: WindowId) {
    PARAGRAPHS.with(|paragraphs| paragraphs.borrow_mut().insert(window, Vec::new()));
}

pub fn register(window: WindowId, container: u64, layout: TextLayout, targets: Arc<Targets>) {
    PARAGRAPHS.with(|paragraphs| {
        paragraphs
            .borrow_mut()
            .entry(window)
            .or_default()
            .push(Entry {
                container,
                layout,
                targets,
            })
    });
}

/// The inline boxes of a paragraph of `container` in `window` that have a glyph
/// under `position`, innermost first. Empty when the point is in no glyph's cell.
pub fn boxes_under(
    window: WindowId,
    container: u64,
    position: Point<Pixels>,
) -> Vec<(u64, Bounds<Pixels>)> {
    PARAGRAPHS.with(|paragraphs| {
        let paragraphs = paragraphs.borrow();
        let Some(entries) = paragraphs.get(&window) else {
            return Vec::new();
        };
        // A later paragraph is painted over an earlier one.
        for entry in entries.iter().rev() {
            if entry.container != container || !entry.layout.bounds().contains(&position) {
                continue;
            }
            // `Err` is the nearest index of a point past the end of a line or
            // before its start, and no glyph is there.
            let Ok(byte) = entry.layout.index_for_position(position) else {
                continue;
            };
            return entry.targets.around(byte, position, |range, edges| {
                border_box_rows(&entry.layout, range, edges)
            });
        }
        Vec::new()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size};

    /// `div(1) > [text "a ", span(3) > ["b", span(5) > "c"], text " d"]`: the
    /// text is "a bc d".
    fn fixture(styles: &[(u64, serde_json::Value)]) -> RetainedTree {
        let mut tree = RetainedTree::new();
        for (id, kind) in [(1, "div"), (2, "text"), (3, "span"), (4, "text"), (5, "span"), (6, "text"), (7, "text")] {
            tree.create_element(id, kind.to_string());
        }
        for (parent, child) in [(1, 2), (1, 3), (3, 4), (3, 5), (5, 6), (1, 7)] {
            tree.append_child(parent, child);
        }
        for (id, text) in [(2, "a "), (4, "b"), (6, "c"), (7, " d")] {
            tree.set_text(id, text.to_string());
        }
        for (id, style) in styles {
            tree.set_style(*id, Arc::new(serde_json::from_value(style.clone()).expect("a style")));
        }
        tree
    }

    fn plan_targets(tree: &RetainedTree, none: bool, inert: bool) -> Option<Targets> {
        let plan = crate::text::inline::plan_children(tree, &tree.elements[&1]);
        let crate::text::inline::Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        assert_eq!(paragraph.text, "a bc d");
        Targets::of(tree, paragraph, none, inert, &Insets::default())
    }

    /// Ten pixels to a byte, one line.
    fn rows(range: &Range<usize>, edges: Edges) -> Vec<Bounds<Pixels>> {
        crate::text::inline_bounds::inside(
            vec![Bounds::new(
                point(px(range.start as f32 * 10.0), px(0.0)),
                size(px(range.len() as f32 * 10.0), px(20.0)),
            )],
            edges,
        )
    }

    /// The middle of the cell of `byte`.
    fn over(byte: usize) -> Point<Pixels> {
        point(px(byte as f32 * 10.0 + 5.0), px(10.0))
    }

    fn ids(around: Vec<(u64, Bounds<Pixels>)>) -> Vec<u64> {
        around.into_iter().map(|(id, _)| id).collect()
    }

    #[test]
    fn a_paragraph_of_plain_text_has_no_targets() {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        tree.create_element(2, "text".to_string());
        tree.append_child(1, 2);
        tree.set_text(2, "plain".to_string());
        let plan = crate::text::inline::plan_children(&tree, &tree.elements[&1]);
        let crate::text::inline::Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        assert_eq!(Targets::of(&tree, paragraph, false, false, &Insets::default()), None);
    }

    #[test]
    fn the_boxes_around_a_byte_come_innermost_first_with_their_first_fragment() {
        let targets = targets_of(&fixture(&[]));
        assert_eq!(ids(targets.around(3, over(3), rows)), vec![5, 3]);
        assert_eq!(ids(targets.around(2, over(2), rows)), vec![3]);
        assert!(targets.around(0, over(0), rows).is_empty(), "text outside every box");
        assert!(targets.around(5, over(5), rows).is_empty());
        let around = targets.around(3, over(3), rows);
        assert_eq!(around[0].1.origin, point(px(30.0), px(0.0)));
        assert_eq!(around[1].1.size.width, px(20.0), "span 3 covers 'bc'");
    }

    #[test]
    fn a_box_with_no_fragment_is_left_out() {
        let targets = targets_of(&fixture(&[]));
        let around = targets.around(3, over(3), |range, edges| {
            if range.len() > 1 { rows(range, edges) } else { Vec::new() }
        });
        assert_eq!(ids(around), vec![3]);
    }

    #[test]
    fn pointer_events_none_takes_a_box_out_until_a_descendant_says_auto() {
        let tree = fixture(&[(3, serde_json::json!({"pointerEvents": "none"}))]);
        assert_eq!(plan_targets(&tree, false, false), None);

        let tree = fixture(&[
            (3, serde_json::json!({"pointerEvents": "none"})),
            (5, serde_json::json!({"pointerEvents": "auto"})),
        ]);
        let targets = targets_of(&tree);
        assert!(targets.around(2, over(2), rows).is_empty());
        assert_eq!(ids(targets.around(3, over(3), rows)), vec![5]);
    }

    fn targets_of(tree: &RetainedTree) -> Targets {
        plan_targets(tree, false, false).unwrap()
    }

    #[test]
    fn inert_does_not_reopen_and_a_none_container_hides_the_boxes_in_it() {
        let tree = fixture(&[(5, serde_json::json!({"pointerEvents": "auto"}))]);
        assert_eq!(plan_targets(&tree, true, false).unwrap().pieces.len(), 1);
        assert!(plan_targets(&tree, true, false).unwrap().around(2, over(2), rows).is_empty());
        assert_eq!(ids(plan_targets(&tree, true, false).unwrap().around(3, over(3), rows)), vec![5]);
        assert_eq!(plan_targets(&tree, false, true), None);

        let mut tree = tree;
        tree.set_custom_prop(3, "inert".to_string(), serde_json::json!(true));
        assert_eq!(plan_targets(&tree, false, false), None, "auto does not undo inert");
    }

    #[test]
    fn a_display_contents_box_is_not_a_target() {
        let tree = fixture(&[(3, serde_json::json!({"display": "contents"}))]);
        let targets = targets_of(&tree);
        assert_eq!(ids(targets.around(3, over(3), rows)), vec![5]);
        assert!(targets.around(2, over(2), rows).is_empty());
    }

    #[test]
    fn the_pointer_in_the_margin_of_a_box_is_on_the_boxes_around_it_only() {
        // Span 3 covers bytes 2..4 (x 20..40) and span 5 covers 3..4: the box
        // of 3 has 4 of margin at its start and 6 at its end, which 5 ends with.
        let tree = fixture(&[(3, serde_json::json!({"marginLeft": 4, "marginRight": 6}))]);
        let plan = crate::text::inline::plan_children(&tree, &tree.elements[&1]);
        let crate::text::inline::Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        let insets = Insets::of(&tree, paragraph, &|_, _| Default::default());
        let targets = Targets::of(&tree, paragraph, false, false, &insets).unwrap();
        let at = |byte: usize, x: f32| ids(targets.around(byte, point(px(x), px(10.0)), rows));

        assert!(at(2, 22.0).is_empty(), "in the margin before the box");
        assert_eq!(at(2, 24.0), vec![3]);
        assert_eq!(at(3, 33.9), vec![5, 3]);
        assert!(at(3, 34.0).is_empty(), "in the margin after the box");
    }

    #[test]
    fn the_pointer_in_the_margin_of_an_inner_box_is_still_on_the_one_around_it() {
        // Span 5 has a margin of its own at its end, inside span 3's box.
        let tree = fixture(&[(5, serde_json::json!({"marginRight": 6}))]);
        let plan = crate::text::inline::plan_children(&tree, &tree.elements[&1]);
        let crate::text::inline::Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        let insets = Insets::of(&tree, paragraph, &|_, _| Default::default());
        let targets = Targets::of(&tree, paragraph, false, false, &insets).unwrap();
        // Span 3 ends with 5, but 5's margin is inside it.
        assert_eq!(ids(targets.around(3, point(px(36.0), px(10.0)), rows)), vec![3]);
        assert_eq!(ids(targets.around(3, point(px(32.0), px(10.0)), rows)), vec![5, 3]);
    }

    /// The same ten pixels to a byte, but a line box of 40 around a font of 12 above
    /// the baseline and 4 below: the content area is y 12 to 28.
    fn tall_rows(range: &Range<usize>, edges: Edges) -> Vec<Bounds<Pixels>> {
        crate::text::inline_bounds::border_box(
            vec![crate::text::inline_bounds::Row {
                line: Bounds::new(
                    point(px(range.start as f32 * 10.0), px(0.0)),
                    size(px(range.len() as f32 * 10.0), px(40.0)),
                ),
                ascent: px(12.0),
                descent: px(4.0),
            }],
            edges,
        )
    }

    fn insets_of(tree: &RetainedTree) -> Insets {
        let plan = crate::text::inline::plan_children(tree, &tree.elements[&1]);
        let crate::text::inline::Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        Insets::of(tree, paragraph, &|_, _| Default::default())
    }

    #[test]
    fn the_line_box_above_and_below_the_content_area_is_not_on_the_box() {
        let targets = targets_of(&fixture(&[]));
        let at = |y: f32| ids(targets.around(2, point(px(25.0), px(y)), tall_rows));
        assert!(at(5.0).is_empty(), "above the text");
        assert!(at(35.0).is_empty(), "below the text");
        assert_eq!(at(20.0), vec![3]);
        assert_eq!(at(12.5), vec![3], "the first pixel of the content area");
        assert_eq!(at(27.5), vec![3], "the last pixel of it");
        assert!(at(28.5).is_empty());
    }

    #[test]
    fn vertical_padding_and_border_are_on_the_box() {
        let tree = fixture(&[(3, serde_json::json!({"paddingTop": 6, "borderTopWidth": 2, "paddingBottom": 10}))]);
        let insets = insets_of(&tree);
        let plan = crate::text::inline::plan_children(&tree, &tree.elements[&1]);
        let crate::text::inline::Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        let targets = Targets::of(&tree, paragraph, false, false, &insets).unwrap();
        let at = |y: f32| ids(targets.around(2, point(px(25.0), px(y)), tall_rows));
        assert!(at(3.0).is_empty(), "above the border");
        assert_eq!(at(4.5), vec![3], "on the border, 8 above the content area");
        assert_eq!(at(37.0), vec![3], "10 below the content area");
        assert!(at(38.5).is_empty());
    }

    #[test]
    fn the_first_fragment_of_a_box_starts_at_its_border_box() {
        let tree = fixture(&[(3, serde_json::json!({"marginLeft": 4}))]);
        let plan = crate::text::inline::plan_children(&tree, &tree.elements[&1]);
        let crate::text::inline::Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        let insets = Insets::of(&tree, paragraph, &|_, _| Default::default());
        let targets = Targets::of(&tree, paragraph, false, false, &insets).unwrap();
        let around = targets.around(2, over(2), rows);
        assert_eq!(around[0].1.origin.x, px(24.0));
    }

    fn painted(window: WindowId) -> usize {
        PARAGRAPHS.with(|paragraphs| paragraphs.borrow().get(&window).map_or(0, Vec::len))
    }

    #[test]
    fn a_window_resetting_its_frame_leaves_the_paragraphs_of_another() {
        let targets = Arc::new(targets_of(&fixture(&[])));
        let layout = gpui::StyledText::new("a bc d").layout().clone();
        let (a, b) = (WindowId::from(1), WindowId::from(2));
        for window in [a, b] {
            frame_reset(window);
            register(window, 1, layout.clone(), targets.clone());
        }
        register(a, 1, layout.clone(), targets.clone());
        assert_eq!((painted(a), painted(b)), (2, 1));

        frame_reset(b);
        assert_eq!((painted(a), painted(b)), (2, 0));
        frame_reset(a);
        assert_eq!(painted(a), 0);
    }
}
