//! The `appRegion` of an inline box.
//!
//! A host element registers its border box as a window drag region or a hole in one (see
//! [`crate::events::app_region`]). An inline box has no host box, so the paragraph registers
//! one rectangle per line fragment of its border box instead, which is also what the
//! box's cursor and events use. Like a host box, a region is layout geometry: the hitboxes are
//! pass-through, so they change nothing about the pointer, and `pointerEvents` and `inert` do not
//! take part.
//!
//! The paragraph's regions come after its container's own, and a box nested in another is later
//! than it, so the innermost value wins where they overlap, as for host boxes in tree order.

use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use gpui::{canvas, prelude::*, HitboxBehavior, TextLayout, WindowControlArea};

use crate::events::app_region::AppRegion;
use crate::events::state_styles::{folded, PointerStates};
use crate::retained_tree::RetainedTree;
use crate::style::StyleDesc;

use super::inline::Paragraph;
use super::inline_bounds::border_box_rows;
use super::inline_hit::boxes_with_a_box;
use super::inline_insets::{BoxEdges, InsetSpec};

/// A box whose region differs from the one around it.
#[derive(Clone, Debug, PartialEq)]
pub struct RegionBox {
    pub range: Range<usize>,
    pub region: AppRegion,
    pub edges: BoxEdges,
}

/// The boxes of `paragraph` that change the region, outermost first. `container` is the region
/// of the element holding the paragraph, which is what text in no box has; a box that ends up
/// with the value of the box directly around it (or `container`) registers nothing more.
///
/// `states` says which pointer states apply to a box, as for [`super::inline_cursor::of`].
pub fn of(
    tree: &RetainedTree,
    paragraph: &Paragraph,
    container: Option<AppRegion>,
    states: &dyn Fn(u64, &StyleDesc) -> PointerStates,
    insets: &Rc<InsetSpec>,
) -> Vec<RegionBox> {
    let with_box = boxes_with_a_box(paragraph);
    // The value of every box that has one, inherited through the boxes around it (a
    // `display: contents` one included). It is the same around every piece of text the box
    // holds, so the first one decides.
    let mut effective: HashMap<u64, Option<AppRegion>> = HashMap::new();
    for piece in &paragraph.pieces {
        let mut value = container;
        for id in &piece.boxes {
            let Some(base) = tree
                .elements
                .get(id)
                .and_then(|element| element.style.as_deref())
            else {
                if with_box.contains(id) {
                    effective.entry(*id).or_insert(value);
                }
                continue;
            };
            let declared = folded(base, states(*id, base));
            value = declared
                .app_region
                .as_deref()
                .and_then(AppRegion::parse)
                .or(value);
            if with_box.contains(id) {
                effective.entry(*id).or_insert(value);
            }
        }
    }
    let mut boxes = Vec::new();
    let mut around: Vec<(&Range<usize>, Option<AppRegion>)> = Vec::new();
    for (id, range) in &paragraph.boxes {
        let Some(value) = effective.get(id).copied() else {
            continue;
        };
        while around.last().is_some_and(|(outer, _)| outer.end <= range.start) {
            around.pop();
        }
        let enclosing = around.last().map_or(container, |(_, outer)| *outer);
        around.push((range, value));
        // A value is never dropped on the way down, so a change is always to a `Some`.
        if let (true, Some(region)) = (value != enclosing, value) {
            boxes.push(RegionBox {
                range: range.clone(),
                region,
                edges: insets.edges_of(*id),
            });
        }
    }
    boxes
}

/// Insert the hitboxes during prepaint and register them as window regions during paint. It must
/// come after the glyphs among its siblings: their prepaint is what gives `layout` its bounds
/// and lines.
pub fn regions(layout: TextLayout, boxes: Vec<RegionBox>) -> impl IntoElement {
    canvas(
        move |_, window, _| {
            let mut hitboxes = Vec::new();
            for inline in &boxes {
                for row in border_box_rows(&layout, &inline.range, &inline.edges) {
                    hitboxes.push((window.insert_hitbox(row, HitboxBehavior::PassThrough), inline.region));
                }
            }
            hitboxes
        },
        |_, hitboxes, window, _| {
            for (hitbox, region) in hitboxes {
                match region {
                    AppRegion::Drag => {
                        window.insert_window_control_hitbox(WindowControlArea::Drag, hitbox)
                    }
                    AppRegion::NoDrag => window.insert_window_client_hitbox(hitbox),
                }
            }
        },
    )
    .absolute()
    .size_full()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::length::LengthContext;
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

    fn regions_in(
        tree: &RetainedTree,
        container: Option<AppRegion>,
        states: &dyn Fn(u64, &StyleDesc) -> PointerStates,
    ) -> Vec<(Range<usize>, AppRegion)> {
        let plan = plan_children(tree, &tree.elements[&1]);
        let Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        let insets = InsetSpec::of(tree, paragraph, &crate::color::ColorContext::root(crate::color::Scheme::Light), &LengthContext::initial(800.0, 600.0), states);
        of(tree, paragraph, container, states, &insets)
            .into_iter()
            .map(|inline| (inline.range, inline.region))
            .collect()
    }

    fn resting(tree: &RetainedTree, container: Option<AppRegion>) -> Vec<(Range<usize>, AppRegion)> {
        regions_in(tree, container, &|_, _| PointerStates::default())
    }

    fn region(name: &str) -> serde_json::Value {
        serde_json::json!({ "appRegion": name })
    }

    #[test]
    fn a_paragraph_without_a_region_registers_none() {
        assert_eq!(resting(&fixture(&[]), None), vec![]);
        assert_eq!(resting(&fixture(&[(3, serde_json::json!({"color": "red"}))]), None), vec![]);
    }

    #[test]
    fn a_span_registers_its_value_over_its_text() {
        let tree = fixture(&[(3, region("drag"))]);
        assert_eq!(resting(&tree, None), vec![(2..4, AppRegion::Drag)]);
        let tree = fixture(&[(3, region("no-drag"))]);
        assert_eq!(resting(&tree, Some(AppRegion::Drag)), vec![(2..4, AppRegion::NoDrag)]);
    }

    #[test]
    fn a_span_with_the_value_around_it_registers_nothing() {
        let tree = fixture(&[(3, region("drag"))]);
        assert_eq!(resting(&tree, Some(AppRegion::Drag)), vec![]);
        let tree = fixture(&[(3, region("drag")), (5, region("drag"))]);
        assert_eq!(resting(&tree, None), vec![(2..4, AppRegion::Drag)]);
    }

    #[test]
    fn a_nested_box_is_later_so_it_wins_where_they_overlap() {
        let tree = fixture(&[(3, region("drag")), (5, region("no-drag"))]);
        assert_eq!(
            resting(&tree, None),
            vec![(2..4, AppRegion::Drag), (3..4, AppRegion::NoDrag)]
        );
    }

    #[test]
    fn a_box_inherits_and_an_invalid_value_is_no_declaration() {
        let tree = fixture(&[(3, region("drag")), (5, region("none"))]);
        assert_eq!(resting(&tree, None), vec![(2..4, AppRegion::Drag)]);
    }

    #[test]
    fn a_display_contents_box_passes_its_value_to_its_descendants() {
        // Span 3 has no box, so "b" is in none; span 5 inherits the value, styled or not.
        let tree = fixture(&[(3, serde_json::json!({"display": "contents", "appRegion": "drag"}))]);
        assert_eq!(resting(&tree, None), vec![(3..4, AppRegion::Drag)]);
        let tree = fixture(&[
            (3, serde_json::json!({"display": "contents", "appRegion": "drag"})),
            (5, serde_json::json!({"color": "red"})),
        ]);
        assert_eq!(resting(&tree, None), vec![(3..4, AppRegion::Drag)]);
    }

    #[test]
    fn pointer_events_do_not_take_part() {
        let tree = fixture(&[(3, serde_json::json!({"appRegion": "drag", "pointerEvents": "none"}))]);
        assert_eq!(resting(&tree, None), vec![(2..4, AppRegion::Drag)]);
    }

    #[test]
    fn a_hover_value_applies_while_the_box_is_hovered() {
        let tree = fixture(&[(3, serde_json::json!({"hover": {"appRegion": "drag"}}))]);
        assert_eq!(resting(&tree, None), vec![]);
        let hovered = |id: u64, _: &StyleDesc| PointerStates {
            hovered: id == 3,
            ..PointerStates::default()
        };
        assert_eq!(regions_in(&tree, None, &hovered), vec![(2..4, AppRegion::Drag)]);
    }
}
