//! Focus on an inline box.
//!
//! A `<span>` with a `tabIndex` is focusable, but it has no GPUI element, so its
//! focus handle belongs to no dispatch tree and the window cannot focus it. The
//! paragraph gives each such span an anchor: an element of no size, under the
//! span's host id, that tracks the handle. GPUI then records the span's tab stop
//! where the anchor paints and its accessibility node, and `Window::focus` can
//! land on it. Anchors come after the glyphs among the paragraph's children, in
//! tree order, so Tab visits a paragraph's spans at the paragraph's place among
//! the other focusables, outermost span first.
//!
//! The anchor cannot be hit, so it is no target of the pointer, and it records no
//! bounds: [`super::inline_bounds`] reports the span's box from its glyphs.
//!
//! A press on the glyphs cannot move focus the way a div's does, because the
//! anchor has no area for GPUI to find the pointer in. [`focus_on_press`] does it
//! from the paragraph, so that the span wins over a focusable ancestor.

use std::cell::RefCell;
use std::collections::HashMap;

use gpui::{div, prelude::*, AnyElement, Div, FocusHandle, Stateful, Window, WindowId};

use super::inline::Paragraph;
use super::inline_hit;

thread_local! {
    /// The focusable box the press being dispatched landed on, per window. Set while the
    /// press is dispatched and taken by the default of the press, which runs after.
    static PRESS_FOCUS: RefCell<HashMap<WindowId, FocusHandle>> = RefCell::new(HashMap::new());
}

/// The focusable box the press just dispatched landed on, if any. The default of the press
/// focuses it, after the focus transfer of a focusable element around the paragraph, so the
/// focus events of the press name the span and not the element that was there for a moment.
pub(crate) fn take_press_focus(window: WindowId) -> Option<FocusHandle> {
    PRESS_FOCUS.with(|slots| slots.borrow_mut().remove(&window))
}

/// A focusable inline box of a paragraph.
pub struct FocusableBox {
    pub id: u64,
    pub handle: FocusHandle,
    /// Role, label and the other `aria-*` fields of the span, which the anchor
    /// carries.
    pub accessibility: Box<dyn FnOnce(Stateful<Div>) -> Stateful<Div>>,
}

/// The boxes of `paragraph` that `focusable` answers for, in tree order.
pub fn in_tree_order<T>(
    paragraph: &Paragraph,
    focusable: impl Fn(u64) -> Option<T>,
) -> Vec<(u64, T)> {
    paragraph
        .boxes
        .iter()
        .filter_map(|(id, _)| Some((*id, focusable(*id)?)))
        .collect()
}

/// The anchors of `boxes`, to be children of the paragraph after its glyphs.
pub fn anchors(boxes: Vec<FocusableBox>) -> Vec<AnyElement> {
    boxes
        .into_iter()
        .map(|focusable| {
            let anchor = div()
                .id(crate::events::ids::host_element_id(focusable.id))
                .absolute()
                .top_0()
                .left_0()
                .size_0()
                .track_focus(&focusable.handle);
            (focusable.accessibility)(anchor).into_any_element()
        })
        .collect()
}

/// Focuses the innermost focusable box under a press, as the DOM does for a press
/// on a focusable element, once the press has been dispatched. Call it from paint.
///
/// It is for a focusable element around the paragraph: GPUI's own handler of that
/// element takes the press for itself, because the pointer is inside it, and
/// prevents the default, which also stops the dispatcher's default from reaching
/// the span ([`crate::focus::defer_mouse_down_default`]). Focus is therefore set
/// after both: the span is handed to the default of the press, which settles the focus
/// events once, with the span. It prevents nothing, so the press still starts a text
/// selection unless something around the span took it.
pub fn focus_on_press(window: &mut Window, container: u64, handles: Vec<(u64, FocusHandle)>) {
    use gpui::{DispatchPhase, MouseDownEvent};

    if handles.is_empty() {
        return;
    }
    // A span left over from a press whose default never ran must not be focused by the next.
    window.on_mouse_event(move |_: &MouseDownEvent, phase, window, _| {
        if phase == DispatchPhase::Capture {
            take_press_focus(window.window_handle().window_id());
        }
    });
    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
        // Only a prevention that came before the paragraph's turn is the page's:
        // this runs before the listeners of the elements around it.
        if phase != DispatchPhase::Bubble || window.default_prevented() {
            return;
        }
        // What covers the paragraph is pressed, not the paragraph.
        let topmost = crate::events::ids::host_hit_chain(window)
            .first()
            .map(|hit| hit.id);
        if topmost != Some(container) {
            return;
        }
        let under = inline_hit::boxes_under(window.window_handle().window_id(), container, event.position);
        let focusable = under
            .iter()
            .find_map(|(id, _)| handles.iter().find(|(boxed, _)| boxed == id));
        if let Some((_, handle)) = focusable {
            let handle = handle.clone();
            PRESS_FOCUS.with(|slots| {
                slots.borrow_mut().insert(window.window_handle().window_id(), handle);
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::inline::{plan_children, Child};
    use crate::retained_tree::RetainedTree;

    /// `div(1) > [text "a ", span(3) > ["b", span(5) > "c"], span(7) > "d", span(9)]`: the
    /// last span holds no text.
    fn paragraph() -> Paragraph {
        let mut tree = RetainedTree::new();
        for (id, kind) in [(1, "div"), (2, "text"), (3, "span"), (4, "text"), (5, "span"), (6, "text"), (7, "span"), (8, "text"), (9, "span")] {
            tree.create_element(id, kind.to_string());
        }
        for (parent, child) in [(1, 2), (1, 3), (3, 4), (3, 5), (5, 6), (1, 7), (7, 8), (1, 9)] {
            tree.append_child(parent, child);
        }
        for (id, text) in [(2, "a "), (4, "b"), (6, "c"), (8, "d")] {
            tree.set_text(id, text.to_string());
        }
        let Child::Paragraph(paragraph) = plan_children(&tree, &tree.elements[&1]).remove(0) else {
            panic!("a paragraph")
        };
        paragraph
    }

    #[test]
    fn boxes_come_outermost_first_in_tree_order() {
        let all = in_tree_order(&paragraph(), |id| Some(id * 10));
        assert_eq!(all, vec![(3, 30), (5, 50), (7, 70)]);
    }

    #[test]
    fn only_the_focusable_boxes_are_kept() {
        let some = in_tree_order(&paragraph(), |id| (id != 3).then_some("focusable"));
        assert_eq!(some, vec![(5, "focusable"), (7, "focusable")]);
    }

    #[test]
    fn a_span_with_no_text_has_no_box_to_anchor() {
        assert!(in_tree_order(&paragraph(), |id| (id == 9).then_some(())).is_empty());
    }
}
