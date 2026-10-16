//! What an inline box paints around its text.
//!
//! A `<span>` has no GPUI element, so the paragraph that holds it paints what
//! CSS draws around a box, once for each line fragment of its text. Today that
//! is the `outline`, which `focusVisible` rings a focused span with and
//! `hover` / `active` can declare as well.
//!
//! The decorations are painted after the glyphs, so they lie over the text of
//! the box, as an outline does in CSS, and they are cut by nothing but the
//! content mask the paragraph paints in.

use std::collections::HashMap;
use std::ops::Range;

use gpui::{canvas, prelude::*, Outline, Style, TextLayout};

use crate::color::ColorContext;
use crate::css::length::LengthContext;
use crate::events::state_styles::{folded, PointerStates};
use crate::retained_tree::RetainedTree;
use crate::style::StyleDesc;

use super::inline::Paragraph;
use super::inline_bounds::fragments;

/// The decorations of one inline box.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxDecoration {
    pub range: Range<usize>,
    pub outline: Outline,
}

/// The decorations of the boxes of `paragraph` that have any, outermost first,
/// so that a box nested in another paints over it.
///
/// `colors` and `lengths` are the container's, as its children inherit them (a
/// span's `fontSize` does not reach its text, so `em` is the container's), and
/// `states` says which states apply to a box, as it does for
/// [`super::paragraph::run_styles`].
/// A `display: contents` box has no box to decorate.
pub fn of(
    tree: &RetainedTree,
    paragraph: &Paragraph,
    colors: &ColorContext,
    lengths: &LengthContext,
    states: &dyn Fn(u64, &StyleDesc) -> PointerStates,
) -> Vec<BoxDecoration> {
    // A box is declared the same way around every piece of text it holds, so the
    // first one decides.
    let mut outlines: HashMap<u64, Outline> = HashMap::new();
    for piece in &paragraph.pieces {
        let mut colors = *colors;
        for id in &piece.boxes {
            let Some(base) = tree
                .elements
                .get(id)
                .and_then(|element| element.style.as_deref())
            else {
                continue;
            };
            let declared = folded(base, states(*id, base));
            colors = colors.descend(&declared);
            if declared.display.as_deref() == Some("contents") || outlines.contains_key(id) {
                continue;
            }
            if let Some(outline) = crate::css::border::outline(&declared, &colors, lengths).filter(is_visible) {
                outlines.insert(*id, outline);
            }
        }
    }
    paragraph
        .boxes
        .iter()
        .filter_map(|(id, range)| {
            Some(BoxDecoration {
                range: range.clone(),
                outline: *outlines.get(id)?,
            })
        })
        .collect()
}

fn is_visible(outline: &Outline) -> bool {
    f32::from(outline.width) > 0.0 && !outline.color.is_transparent()
}

/// The decorations painted over `layout`, which has its bounds once the glyphs
/// have prepainted. It must come after the glyphs among its siblings, so that
/// it paints above them.
pub fn overlay(layout: TextLayout, decorations: Vec<BoxDecoration>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, cx| {
            for decoration in &decorations {
                // The element's own painter, so the ring has the geometry and
                // colour handling of an element's outline.
                let style = Style {
                    outline: Some(decoration.outline),
                    ..Style::default()
                };
                for fragment in fragments(&layout, &decoration.range) {
                    style.paint(fragment, window, cx, |_, _| {});
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
    use crate::color::Scheme;
    use crate::text::inline::{plan_children, Child};
    use std::sync::Arc;

    /// `div(1) > [text "a ", span(3) > ["b", span(5) > "c"], text " d"]`.
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

    fn decorations(
        tree: &RetainedTree,
        states: &dyn Fn(u64, &StyleDesc) -> PointerStates,
    ) -> Vec<BoxDecoration> {
        let plan = plan_children(tree, &tree.elements[&1]);
        let Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        of(tree, paragraph, &ColorContext::root(Scheme::Light), &LengthContext::initial(800.0, 600.0), states)
    }

    fn resting(tree: &RetainedTree) -> Vec<BoxDecoration> {
        decorations(tree, &|_, _| PointerStates::default())
    }

    fn ring(color: &str) -> serde_json::Value {
        serde_json::json!({"outlineWidth": 2, "outlineColor": color, "outlineOffset": 1})
    }

    #[test]
    fn a_paragraph_without_an_outline_has_no_decoration() {
        assert_eq!(resting(&fixture(&[])), vec![]);
        assert_eq!(resting(&fixture(&[(3, serde_json::json!({"color": "red"}))])), vec![]);
    }

    #[test]
    fn a_span_is_outlined_around_its_own_text_and_the_boxes_nested_in_it() {
        let found = resting(&fixture(&[(3, ring("#ff0000"))]));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].range, 2..4);
        assert_eq!(f32::from(found[0].outline.width), 2.0);
        assert_eq!(f32::from(found[0].outline.offset), 1.0);
    }

    #[test]
    fn a_nested_span_has_its_own_ring_and_does_not_inherit_the_parents() {
        let found = resting(&fixture(&[(3, ring("#ff0000")), (5, serde_json::json!({"color": "blue"}))]));
        assert_eq!(found.iter().map(|decoration| decoration.range.clone()).collect::<Vec<_>>(), vec![2..4]);

        let found = resting(&fixture(&[(5, ring("#0000ff"))]));
        assert_eq!(found.iter().map(|decoration| decoration.range.clone()).collect::<Vec<_>>(), vec![3..4]);
    }

    #[test]
    fn rings_come_outermost_first() {
        let found = resting(&fixture(&[(3, ring("#ff0000")), (5, ring("#0000ff"))]));
        assert_eq!(found.iter().map(|decoration| decoration.range.clone()).collect::<Vec<_>>(), vec![2..4, 3..4]);
        assert_ne!(found[0].outline.color, found[1].outline.color);
    }

    #[test]
    fn a_ring_that_paints_nothing_is_dropped() {
        for style in [
            serde_json::json!({"outlineWidth": 2}),
            serde_json::json!({"outlineWidth": 0, "outlineColor": "red"}),
            serde_json::json!({"outlineWidth": 2, "outlineColor": "transparent"}),
            serde_json::json!({"outlineWidth": 2, "outlineColor": "not a colour"}),
        ] {
            assert_eq!(resting(&fixture(&[(3, style)])), vec![]);
        }
    }

    #[test]
    fn current_color_is_the_colour_of_the_box_itself() {
        let tree = fixture(&[(
            3,
            serde_json::json!({"color": "#00ff00", "outlineWidth": 2, "outlineColor": "currentColor"}),
        )]);
        let found = resting(&tree);
        assert_eq!(found[0].outline.color, gpui::Rgba { r: 0.0, g: 1.0, b: 0.0, a: 1.0 }.into());
    }

    #[test]
    fn a_display_contents_box_has_no_box_to_ring() {
        let mut style = ring("#ff0000");
        style["display"] = "contents".into();
        assert_eq!(resting(&fixture(&[(3, style)])), vec![]);
    }

    #[test]
    fn a_state_declares_its_ring_only_while_it_applies() {
        let tree = fixture(&[(
            3,
            serde_json::json!({
                "hover": ring("#0000ff"),
                "focusVisible": ring("#ff0000"),
                "focusWithin": {"outlineWidth": 6, "outlineColor": "#00ff00"}
            }),
        )]);
        assert_eq!(resting(&tree), vec![]);

        let hovered = decorations(&tree, &|_, _| PointerStates { hovered: true, ..Default::default() });
        assert_eq!(hovered.len(), 1);

        let focused = decorations(&tree, &|id, _| PointerStates {
            focus_visible: id == 3,
            focus_within: id == 3,
            ..Default::default()
        });
        assert_eq!(focused.len(), 1);
        assert_ne!(focused[0].outline.color, hovered[0].outline.color);
        assert_eq!(f32::from(focused[0].outline.width), 2.0, "focusVisible beats focusWithin");

        let within = decorations(&tree, &|_, _| PointerStates { focus_within: true, ..Default::default() });
        assert_eq!(f32::from(within[0].outline.width), 6.0);
    }

    #[test]
    fn a_ring_of_the_focused_span_is_not_the_parents() {
        let tree = fixture(&[
            (3, serde_json::json!({"focusVisible": ring("#ff0000")})),
            (5, serde_json::json!({"focusVisible": ring("#0000ff")})),
        ]);
        let found = decorations(&tree, &|id, _| PointerStates {
            focus_visible: id == 5,
            ..Default::default()
        });
        assert_eq!(found.iter().map(|decoration| decoration.range.clone()).collect::<Vec<_>>(), vec![3..4]);
    }
}
