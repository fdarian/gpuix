//! The `cursor` of an inline box.
//!
//! A cursor is a request GPUI resolves on every mouse move from the hitboxes the
//! pointer is over, so it cannot be decided while the paragraph paints. The
//! paragraph inserts one hitbox per line fragment of the border box of each box
//! that has a cursor, and requests it over each. They are hitboxes of the
//! paragraph's container as far as hit testing goes (see
//! [`crate::events::ids::host_hit_chain`]), so nothing else about the pointer
//! changes.
//!
//! The cursor is the one of the innermost box the pointer is on, inherited as in
//! CSS: a box nested in another is outermost first, so that its hitboxes are
//! later and win where the two overlap. A glyph in no box that sets one, or in
//! none at all, and the margin of a box, leave the container's cursor.

use std::collections::HashMap;
use std::ops::Range;

use gpui::{canvas, prelude::*, CursorStyle, HitboxBehavior, TextLayout};

use crate::events::state_styles::{folded, PointerStates};
use crate::retained_tree::RetainedTree;
use crate::style::StyleDesc;

use super::inline::Paragraph;
use super::inline_bounds::border_box_rows;
use super::inline_hit::{boxes_with_a_box, piece_targets};
use super::inline_insets::{Edges, Insets};

/// A box that asks for a cursor over its border box.
#[derive(Clone, Debug, PartialEq)]
pub struct CursorBox {
    pub range: Range<usize>,
    pub cursor: CursorStyle,
    pub edges: Edges,
}

/// The boxes of `paragraph` that ask for a cursor, outermost first, with the
/// cursor. A box whose cursor is the one of the box it is directly inside asks
/// for nothing more.
///
/// `states` says which pointer states apply to a box, as it does for
/// [`super::paragraph::run_styles`], so a `hover` cursor is folded in. A
/// container that keeps the pointer off its content has no cursor to give: the
/// hitboxes would be hit where it is not.
pub fn of(
    tree: &RetainedTree,
    paragraph: &Paragraph,
    pointer_events_none: bool,
    inert: bool,
    states: &dyn Fn(u64, &StyleDesc) -> PointerStates,
    insets: &Insets,
) -> Vec<CursorBox> {
    if pointer_events_none || inert {
        return Vec::new();
    }
    let with_box = boxes_with_a_box(paragraph);
    // The cursor of each box the pointer can land on. What a box inherits is the
    // cursor of the boxes around it, a `display: contents` one and one that
    // keeps the pointer off included. It is the same around every piece of text
    // the box holds, so the first one decides.
    let mut inherited_by: HashMap<u64, Option<CursorStyle>> = HashMap::new();
    for piece in &paragraph.pieces {
        let targets = piece_targets(tree, &with_box, piece, false, false);
        let mut inherited = None;
        for id in &piece.boxes {
            let Some(base) = tree
                .elements
                .get(id)
                .and_then(|element| element.style.as_deref())
            else {
                continue;
            };
            let declared = folded(base, states(*id, base));
            if let Some(cursor) = declared.cursor.as_deref().and_then(crate::style::parse_cursor)
            {
                inherited = Some(cursor);
            }
            if targets.contains(id) {
                inherited_by.entry(*id).or_insert(inherited);
            }
        }
    }
    let mut boxes = Vec::new();
    let mut around: Vec<(&Range<usize>, CursorStyle)> = Vec::new();
    for (id, range) in &paragraph.boxes {
        let Some(Some(cursor)) = inherited_by.get(id).copied() else {
            continue;
        };
        while around.last().is_some_and(|(outer, _)| outer.end <= range.start) {
            around.pop();
        }
        let repeats = around.last().is_some_and(|(_, outer)| *outer == cursor);
        around.push((range, cursor));
        if !repeats {
            boxes.push(CursorBox {
                range: range.clone(),
                cursor,
                edges: insets.edges(*id),
            });
        }
    }
    boxes
}

/// Insert the hitboxes during prepaint and request the cursors over them during
/// paint. It must come after the glyphs among its siblings: their prepaint is
/// what gives `layout` its bounds and lines.
pub fn hitboxes(layout: TextLayout, boxes: Vec<CursorBox>) -> impl IntoElement {
    canvas(
        move |_, window, _| {
            let mut hitboxes = Vec::new();
            for inline in &boxes {
                for row in border_box_rows(&layout, &inline.range, inline.edges) {
                    hitboxes.push((window.insert_hitbox(row, HitboxBehavior::Normal), inline.cursor));
                }
            }
            hitboxes
        },
        |_, hitboxes, window, _| {
            for (hitbox, cursor) in &hitboxes {
                window.set_cursor_style(*cursor, hitbox);
            }
        },
    )
    .absolute()
    .size_full()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::inline::{plan_children, Child};
    use std::sync::Arc;

    /// `div(1) > [text "a ", span(3) > ["b", span(5) > "c"], text " d"]`: the
    /// text is "a bc d", and the pieces are 0..2, 2..3, 3..4 and 4..6.
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

    fn cursors(
        tree: &RetainedTree,
        none: bool,
        inert: bool,
        states: &dyn Fn(u64, &StyleDesc) -> PointerStates,
    ) -> Vec<(Range<usize>, CursorStyle)> {
        let plan = plan_children(tree, &tree.elements[&1]);
        let Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        assert_eq!(paragraph.text, "a bc d");
        of(tree, paragraph, none, inert, states, &Insets::default())
            .into_iter()
            .map(|inline| (inline.range, inline.cursor))
            .collect()
    }

    fn resting(tree: &RetainedTree) -> Vec<(Range<usize>, CursorStyle)> {
        cursors(tree, false, false, &|_, _| PointerStates::default())
    }

    fn cursor(name: &str) -> serde_json::Value {
        serde_json::json!({ "cursor": name })
    }

    #[test]
    fn a_paragraph_without_a_cursor_asks_for_none() {
        assert_eq!(resting(&fixture(&[])), vec![]);
        assert_eq!(resting(&fixture(&[(3, serde_json::json!({"color": "red"}))])), vec![]);
    }

    #[test]
    fn a_span_asks_for_its_cursor_over_its_text_only() {
        let tree = fixture(&[(3, cursor("pointer"))]);
        assert_eq!(resting(&tree), vec![(2..4, CursorStyle::PointingHand)]);
    }

    #[test]
    fn the_innermost_box_that_sets_a_cursor_wins() {
        let tree = fixture(&[(3, cursor("pointer")), (5, cursor("text"))]);
        assert_eq!(
            resting(&tree),
            vec![(2..4, CursorStyle::PointingHand), (3..4, CursorStyle::IBeam)],
            "the nested box is later, so its hitboxes win where they overlap"
        );
    }

    #[test]
    fn a_box_inherits_the_cursor_of_the_boxes_around_it() {
        let tree = fixture(&[(3, cursor("pointer")), (5, serde_json::json!({"color": "red"}))]);
        assert_eq!(resting(&tree), vec![(2..4, CursorStyle::PointingHand)]);
    }

    #[test]
    fn an_inner_box_alone_does_not_give_its_cursor_to_the_one_around_it() {
        let tree = fixture(&[(5, cursor("grab"))]);
        assert_eq!(resting(&tree), vec![(3..4, CursorStyle::OpenHand)]);
    }

    #[test]
    fn a_display_contents_box_has_no_box_but_its_descendants_inherit_its_cursor() {
        let tree = fixture(&[(3, serde_json::json!({"display": "contents", "cursor": "pointer"}))]);
        assert_eq!(resting(&tree), vec![], "its own text is in no box");

        let tree = fixture(&[
            (3, serde_json::json!({"display": "contents", "cursor": "pointer"})),
            (5, serde_json::json!({"color": "red"})),
        ]);
        assert_eq!(resting(&tree), vec![(3..4, CursorStyle::PointingHand)]);
    }

    #[test]
    fn an_unknown_keyword_sets_nothing() {
        let tree = fixture(&[(3, cursor("zoom-in"))]);
        assert_eq!(resting(&tree), vec![]);
    }

    #[test]
    fn a_box_that_keeps_the_pointer_off_is_not_where_the_cursor_is() {
        let tree = fixture(&[
            (3, cursor("pointer")),
            (5, serde_json::json!({"cursor": "text", "pointerEvents": "none"})),
        ]);
        assert_eq!(resting(&tree), vec![(2..4, CursorStyle::PointingHand)]);

        let tree = fixture(&[(3, serde_json::json!({"cursor": "pointer", "pointerEvents": "none"}))]);
        assert_eq!(resting(&tree), vec![]);
    }

    #[test]
    fn a_container_that_keeps_the_pointer_off_has_no_cursor() {
        let tree = fixture(&[(3, cursor("pointer"))]);
        let at_rest = |_: u64, _: &StyleDesc| PointerStates::default();
        assert_eq!(cursors(&tree, true, false, &at_rest), vec![]);
        assert_eq!(cursors(&tree, false, true, &at_rest), vec![]);
    }

    #[test]
    fn a_hover_cursor_applies_while_the_box_is_hovered() {
        let tree = fixture(&[(3, serde_json::json!({"hover": {"cursor": "pointer"}}))]);
        assert_eq!(resting(&tree), vec![]);
        let hovered = |id: u64, _: &StyleDesc| PointerStates {
            hovered: id == 3,
            ..PointerStates::default()
        };
        assert_eq!(
            cursors(&tree, false, false, &hovered),
            vec![(2..4, CursorStyle::PointingHand)]
        );
    }

    #[test]
    fn a_box_with_the_cursor_of_the_box_around_it_asks_for_nothing_more() {
        let tree = fixture(&[(3, cursor("pointer")), (5, cursor("pointer"))]);
        assert_eq!(resting(&tree), vec![(2..4, CursorStyle::PointingHand)]);
    }

    #[test]
    fn a_box_gives_its_border_box_edges_for_the_hitboxes() {
        let tree = fixture(&[
            (3, serde_json::json!({"cursor": "pointer", "marginLeft": 3, "marginRight": 5})),
            (5, serde_json::json!({"cursor": "text"})),
        ]);
        let plan = plan_children(&tree, &tree.elements[&1]);
        let Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        let insets = Insets::of(&tree, paragraph, &|_, _| PointerStates::default());
        let boxes = of(&tree, paragraph, false, false, &|_, _| PointerStates::default(), &insets);
        let edges: Vec<_> = boxes
            .iter()
            .map(|inline| (inline.range.clone(), f32::from(inline.edges.start), f32::from(inline.edges.end)))
            .collect();
        // Span 5 ends where 3 does, so 3's margin is after it.
        assert_eq!(edges, vec![(2..4, 3.0, 5.0), (3..4, 0.0, 5.0)]);
    }
}
