//! The `<label>` element: its accessible text and DOM activation behaviour.

use std::collections::HashMap;

use gpui::prelude::*;

use crate::renderer::{emit_event_full, point_to_xy, EventCallback};
use crate::retained_tree::RetainedTree;

/// Give the label node its text, and make a click on it focus and click its
/// control.
///
/// A click on the control itself (a control nested in its label) already did
/// both, so activating again would click it twice; that case is detected by
/// the control's last painted box.
pub(crate) fn describe_and_activate<E>(
    mut el: E,
    label_id: u64,
    tree: &RetainedTree,
    focus_handles: &HashMap<u64, gpui::FocusHandle>,
    callback: &Option<EventCallback>,
) -> E
where
    E: StatefulInteractiveElement,
{
    let links = super::links::current();
    if let Some(text) = links.label_text(label_id).filter(|text| !text.is_empty()) {
        el = el.aria_value(text.to_string());
    }

    let Some(control_id) = links.control_of(label_id) else {
        return el;
    };
    let disabled = tree
        .elements
        .get(&control_id)
        .and_then(|control| control.custom_props().get("disabled"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if disabled {
        return el;
    }
    let focus_handle = focus_handles.get(&control_id).cloned();
    let callback = callback.clone();
    el.on_mouse_up(gpui::MouseButton::Left, move |event, window, cx| {
        let on_control = crate::automation::get_bounds(control_id).is_some_and(|bounds| {
            let (x, y) = point_to_xy(event.position);
            x >= bounds.x
                && x < bounds.x + bounds.width
                && y >= bounds.y
                && y < bounds.y + bounds.height
        });
        if on_control {
            return;
        }
        if let Some(handle) = &focus_handle {
            window.focus(handle, cx);
        }
        emit_event_full(&callback, control_id, "click", |payload| {
            let (x, y) = point_to_xy(event.position);
            payload.x = Some(x);
            payload.y = Some(y);
            payload.button = Some(0);
            payload.click_count = Some(event.click_count as u32);
            payload.modifiers = Some(event.modifiers.into());
            payload.is_right_click = Some(false);
        });
    })
}
