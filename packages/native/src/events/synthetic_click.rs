//! The `click` of an activation that is not a pointer press: the VoiceOver
//! Press, and Enter or Space on a focused button.
//!
//! It is a DOM click like `HTMLElement.click()` makes: `detail` 0, `pointerId`
//! -1 and an empty `pointerType`, because no pointer is involved, and a
//! `target` and `path`, so it bubbles to the ancestors' `onClick`.

use crate::element_tree::EventPayload;
use crate::retained_tree::RetainedTree;

use super::path::event_path;

pub const NON_POINTER_ID: i32 = -1;

/// The click for `path`, which starts at the element that was activated.
pub fn click_payload(path: Vec<u64>) -> EventPayload {
    let target = path[0];
    EventPayload {
        element_id: target as f64,
        event_type: "click".to_string(),
        target: Some(target as f64),
        path: Some(path.iter().map(|id| *id as f64).collect()),
        button: Some(0),
        is_right_click: Some(false),
        buttons: Some(0),
        detail: Some(0),
        click_count: Some(0),
        pointer_id: Some(NON_POINTER_ID),
        pointer_type: Some(String::new()),
        ..Default::default()
    }
}

/// The click for the element `id` as the tree has it now.
pub fn click_element(tree: &RetainedTree, id: u64) -> Result<EventPayload, String> {
    let path = event_path(tree, id);
    if path.is_empty() {
        return Err(format!("Unknown element id {id}"));
    }
    Ok(click_payload(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_without_a_pointer_has_no_pointer_identity_and_bubbles_along_the_path() {
        let payload = click_payload(vec![3, 2, 1]);
        assert_eq!(payload.event_type, "click");
        assert_eq!(payload.element_id, 3.0);
        assert_eq!(payload.target, Some(3.0));
        assert_eq!(payload.path, Some(vec![3.0, 2.0, 1.0]));
        assert_eq!(payload.detail, Some(0));
        assert_eq!(payload.pointer_id, Some(-1));
        assert_eq!(payload.pointer_type.as_deref(), Some(""));
        assert_eq!(payload.button, Some(0));
        assert_eq!(payload.buttons, Some(0));
    }

    #[test]
    fn clicking_an_element_outside_the_tree_is_an_error() {
        let tree = RetainedTree::new();
        assert!(click_element(&tree, 7).unwrap_err().contains("7"));
    }
}
