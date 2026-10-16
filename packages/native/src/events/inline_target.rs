//! The inline boxes under the pointer, in front of the hit chain.
//!
//! GPUI's hit test ends at the host element that holds a paragraph. When that
//! element is the target, the pointer may still be on a glyph of a `<span>` (or
//! a nested `<text>`) inside it, and that box is the DOM target. Its retained
//! ancestors are the path from there on (`events/path.rs`), so the chain only has
//! to name the boxes and where the first line fragment of each starts, which is
//! what `offsetX` is measured from.

use gpui::{Pixels, Point, WindowId};

use super::ids::Hit;
use crate::text::inline_hit;

/// `chain`, the host elements under `position` with the target first, with the
/// inline boxes around the glyph under it put in front, innermost first. The
/// chain is returned as it came when its target is not a paragraph's container,
/// something is painted over the text, or the point is in no glyph's cell.
pub fn refine(window: WindowId, mut chain: Vec<Hit>, position: Point<Pixels>) -> Vec<Hit> {
    let Some(container) = chain.first().map(|hit| hit.id) else {
        return chain;
    };
    let inside = inline_hit::boxes_under(window, container, position);
    chain.splice(
        0..0,
        inside.into_iter().map(|(id, bounds)| Hit { id, bounds }),
    );
    chain
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui::{point, px, size, Bounds};

    use super::*;
    use crate::element_tree::EventModifiers;
    use crate::events::dispatcher::{dispatch, Output, PointerAction, PointerSample};
    use crate::events::state::PointerState;
    use crate::retained_tree::RetainedTree;

    /// root(1) > text(2) > ["Hello ", span(4) > "world", "!", span(7) > "again"]
    fn paragraph(listening: &[(u64, &str)]) -> RetainedTree {
        let mut tree = RetainedTree::new();
        for (id, kind) in [(1, "div"), (2, "text"), (4, "span"), (7, "span")] {
            tree.create_element(id, kind.to_string());
        }
        tree.set_root(1);
        tree.append_child(1, 2);
        tree.append_child(2, 4);
        tree.append_child(2, 7);
        for (id, text, parent) in [(3, "Hello ", 2), (5, "world", 4), (6, "!", 2), (8, "again", 7)] {
            tree.create_element(id, "text".to_string());
            tree.append_child(parent, id);
            tree.set_text(id, text.to_string());
        }
        for (id, event_type) in listening {
            tree.set_event_listener(*id, event_type.to_string(), true);
        }
        tree
    }

    fn hit(id: u64, x: f32) -> Hit {
        Hit {
            id,
            bounds: Bounds::new(point(px(x), px(20.0)), size(px(100.0), px(20.0))),
        }
    }

    /// What the dispatcher is given over a glyph of `span`.
    fn over(span: u64) -> Vec<Hit> {
        vec![hit(span, 60.0), hit(2, 10.0), hit(1, 0.0)]
    }

    fn over_text() -> Vec<Hit> {
        vec![hit(2, 10.0), hit(1, 0.0)]
    }

    fn input(action: PointerAction, hits: Vec<Hit>) -> PointerSample {
        PointerSample {
            action,
            position: (65.0, 30.0),
            movement: (0.0, 0.0),
            modifiers: EventModifiers::default(),
            hits,
            over_drag_region: false,
            window: gpui::WindowId::from(1),
        }
    }

    fn mv(hits: Vec<Hit>) -> PointerSample {
        input(PointerAction::Move { pressed_button: None }, hits)
    }

    fn down(hits: Vec<Hit>) -> PointerSample {
        input(PointerAction::Down { button: 0, click_count: 1 }, hits)
    }

    fn up(hits: Vec<Hit>) -> PointerSample {
        input(PointerAction::Up { button: 0, click_count: 1 }, hits)
    }

    fn names(output: &Output) -> Vec<String> {
        output
            .events
            .iter()
            .map(|event| format!("{}@{}", event.event_type, event.target().expect("an element")))
            .collect()
    }

    #[test]
    fn a_click_on_a_span_targets_it_and_bubbles_through_the_paragraph() {
        let tree = paragraph(&[(4, "click"), (2, "click"), (1, "click")]);
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &down(over(4)));
        let output = dispatch(&mut state, &tree, &up(over(4)));
        assert_eq!(names(&output), ["click@4"]);
        assert_eq!(output.events[0].path, [4, 2, 1]);
        assert!(!output.chain_mismatch);
    }

    #[test]
    fn a_click_on_the_text_around_a_span_never_reaches_the_span() {
        let tree = paragraph(&[(4, "click"), (2, "click")]);
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &down(over_text()));
        let output = dispatch(&mut state, &tree, &up(over_text()));
        assert_eq!(names(&output), ["click@2"]);
        assert_eq!(output.events[0].path, [2, 1]);
    }

    #[test]
    fn a_span_nobody_listens_on_is_still_the_target_its_parent_hears() {
        let tree = paragraph(&[(2, "mousedown")]);
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &down(over(7)));
        assert_eq!(names(&output), ["mousedown@7"]);
        assert_eq!(output.events[0].path, [7, 2, 1]);
    }

    #[test]
    fn a_nested_span_is_the_target_with_its_span_on_the_path() {
        let mut tree = paragraph(&[(2, "mousedown")]);
        tree.create_element(9, "span".to_string());
        tree.append_child(4, 9);
        let mut state = PointerState::default();
        let chain = vec![hit(9, 80.0), hit(4, 60.0), hit(2, 10.0), hit(1, 0.0)];
        let output = dispatch(&mut state, &tree, &down(chain));
        assert_eq!(output.events[0].path, [9, 4, 2, 1]);
        assert!(!output.chain_mismatch);
    }

    #[test]
    fn moving_between_spans_leaves_one_and_enters_the_other_but_not_the_paragraph() {
        let listening: Vec<(u64, &str)> = [1, 2, 4, 7]
            .into_iter()
            .flat_map(|id| {
                ["mouseenter", "mouseleave", "mouseover", "mouseout"]
                    .into_iter()
                    .map(move |event_type| (id, event_type))
            })
            .collect();
        let tree = paragraph(&listening);
        let mut state = PointerState::default();
        let entered = dispatch(&mut state, &tree, &mv(over(4)));
        assert_eq!(
            names(&entered),
            ["mouseover@4", "mouseenter@1", "mouseenter@2", "mouseenter@4"]
        );

        let between = dispatch(&mut state, &tree, &mv(over(7)));
        assert_eq!(
            names(&between),
            ["mouseout@4", "mouseleave@4", "mouseover@7", "mouseenter@7"]
        );
        assert_eq!(between.events[0].related_target, Some(7));
        assert_eq!(between.events[2].related_target, Some(4));

        let onto_text = dispatch(&mut state, &tree, &mv(over_text()));
        assert_eq!(
            names(&onto_text),
            ["mouseout@7", "mouseleave@7", "mouseover@2"]
        );
        assert_eq!(onto_text.events[2].related_target, Some(7));
    }

    #[test]
    fn a_press_on_a_span_released_on_the_text_clicks_the_paragraph() {
        let tree = paragraph(&[(4, "click"), (2, "click")]);
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &down(over(4)));
        let output = dispatch(&mut state, &tree, &up(over_text()));
        assert_eq!(names(&output), ["click@2"]);
    }

    #[test]
    fn the_offset_is_measured_from_the_first_fragment_of_the_span() {
        let tree = paragraph(&[(4, "mousedown")]);
        // A span is recorded by its paragraph, with the padding and border it has.
        crate::automation::record_bounds(gpui::WindowId::from(1), 4, Bounds::default(), gpui::LayoutInsets::default());
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &down(over(4)));
        assert_eq!(output.events[0].offset, Some((5.0, 10.0)));
    }

    #[test]
    fn a_span_with_a_hover_style_restyles_when_it_is_entered_and_left() {
        let mut tree = paragraph(&[]);
        tree.set_style(
            4,
            Arc::new(serde_json::from_value(serde_json::json!({"hover": {"color": "red"}})).unwrap()),
        );
        let mut state = PointerState::default();
        assert!(dispatch(&mut state, &tree, &mv(over(4))).restyle);
        assert!(state.is_hovered(4) && state.is_hovered(2));
        assert!(!dispatch(&mut state, &tree, &mv(over(4))).restyle);
        assert!(dispatch(&mut state, &tree, &mv(over(7))).restyle);
        assert!(!state.is_hovered(4));
    }

    #[test]
    fn a_span_is_active_while_the_primary_button_is_held_on_it() {
        let mut tree = paragraph(&[]);
        tree.set_style(
            4,
            Arc::new(serde_json::from_value(serde_json::json!({"active": {"color": "red"}})).unwrap()),
        );
        let mut state = PointerState::default();
        assert!(dispatch(&mut state, &tree, &down(over(4))).restyle);
        assert!(state.is_active(4));
        assert!(dispatch(&mut state, &tree, &up(over(4))).restyle);
        assert!(!state.is_active(4));
    }

    #[test]
    fn nothing_is_put_in_front_of_a_chain_with_no_target() {
        assert!(refine(WindowId::from(1), Vec::new(), point(px(1.0), px(1.0))).is_empty());
    }
}
