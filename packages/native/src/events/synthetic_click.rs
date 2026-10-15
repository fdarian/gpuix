//! The `click` of an activation that is not a pointer press: the VoiceOver
//! Press, and Enter or Space on a focused button.
//!
//! It is a DOM click like `HTMLElement.click()` makes: `detail` 0, `pointerId`
//! -1 and an empty `pointerType`, because no pointer is involved, and a
//! `target` and `path`, so it bubbles to the ancestors' `onClick`. A disabled
//! form control gets none, the same rule a pointer click follows. A click on a
//! label goes on to activate its control, as `events::activation` does for a
//! pointer click.

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

/// The click for the element `id` as the tree has it now. `None` for a disabled
/// form control, where `HTMLElement.click()` does nothing.
pub fn click_element(tree: &RetainedTree, id: u64) -> Result<Option<EventPayload>, String> {
    let path = event_path(tree, id);
    if path.is_empty() {
        return Err(format!("Unknown element id {id}"));
    }
    Ok((!tree.in_disabled_form_control(id)).then(|| click_payload(path)))
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

    fn control_tree(control_type: &str, disabled: bool) -> RetainedTree {
        let mut tree = RetainedTree::new();
        let batch = format!(
            r#"[["createElement",1,"div"],["setRoot",1],
                ["createElement",2,"{control_type}"],["appendChild",1,2],["setCustomProp",2,"disabled",{disabled}],
                ["createElement",3,"div"],["appendChild",2,3]]"#
        );
        crate::renderer::apply_batch_to_tree(&mut tree, batch.as_bytes()).unwrap();
        for id in [1, 2, 3] {
            tree.set_event_listener(id, "click".to_string(), true);
        }
        tree
    }

    #[test]
    fn a_disabled_form_control_and_what_is_inside_it_get_no_click() {
        for control_type in ["button", "input", "textarea", "select"] {
            let tree = control_tree(control_type, true);
            for id in [2, 3] {
                assert!(click_element(&tree, id).unwrap().is_none(), "{control_type} {id}");
                let element = &tree.elements[&id];
                assert!(crate::renderer::click_path(&tree, element).is_empty());
            }
            assert!(click_element(&tree, 1).unwrap().is_some());
        }
    }

    #[test]
    fn enabled_controls_and_disabled_non_controls_still_click() {
        for (control_type, disabled) in [("button", false), ("input", false), ("div", true)] {
            let tree = control_tree(control_type, disabled);
            for id in [2, 3] {
                assert!(click_element(&tree, id).unwrap().is_some(), "{control_type} {id}");
                let element = &tree.elements[&id];
                assert!(!crate::renderer::click_path(&tree, element).is_empty());
            }
        }
    }

    #[test]
    fn clicking_an_element_outside_the_tree_is_an_error() {
        let tree = RetainedTree::new();
        assert!(click_element(&tree, 7).unwrap_err().contains("7"));
    }
}
