//! The `<label>` element: its accessible text and DOM activation behaviour.

use gpui::prelude::*;

use crate::element_tree::EventPayload;
use crate::events::path::event_path;
use crate::retained_tree::RetainedTree;

/// Give the label node its text, which is what names the control it points at.
pub(crate) fn describe<E>(mut el: E, label_id: u64) -> E
where
    E: StatefulInteractiveElement,
{
    let links = super::links::current();
    if let Some(text) = links.label_text(label_id).filter(|text| !text.is_empty()) {
        el = el.aria_value(text.to_string());
    }
    el
}

/// Interactive content that exists as a gpuix element type. A click on one
/// inside a label belongs to it, not to the label.
fn is_interactive_content(element_type: &str) -> bool {
    matches!(element_type, "input" | "textarea" | "select" | "button")
}

/// The control a `click` activates, if it does. The nearest `<label>` on the
/// click's path is the activation target, as in the DOM, and it does nothing
/// when the click landed on interactive content inside it, on its control, or
/// inside its control, or when the control is disabled or inert. `path` starts
/// at the click's target.
pub(crate) fn activated_control(tree: &RetainedTree, path: &[u64]) -> Option<u64> {
    let label_index = path.iter().position(|id| {
        tree.elements
            .get(id)
            .is_some_and(|element| element.element_type == "label")
    })?;
    let label = path[label_index];
    if tree.is_inert(label) {
        return None;
    }
    let inside_label = &path[..label_index];
    if inside_label.iter().any(|id| {
        tree.elements
            .get(id)
            .is_some_and(|element| is_interactive_content(&element.element_type))
    }) {
        return None;
    }
    super::links::sync(tree);
    let control = super::links::current().control_of(label)?;
    if path.contains(&control) {
        return None;
    }
    let disabled = tree
        .elements
        .get(&control)?
        .custom_props()
        .get("disabled")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    (!disabled && !tree.is_inert(control)).then_some(control)
}

/// The click the label's activation dispatches on `control`: the original
/// click's position, modifiers and count, aimed at the control.
pub(crate) fn activation_click(
    tree: &RetainedTree,
    click: &EventPayload,
    control: u64,
) -> EventPayload {
    EventPayload {
        element_id: control as f64,
        target: Some(control as f64),
        path: Some(
            event_path(tree, control)
                .iter()
                .map(|id| *id as f64)
                .collect(),
        ),
        offset_x: None,
        offset_y: None,
        ..click.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree_with(batch: &str) -> RetainedTree {
        let mut tree = RetainedTree::new();
        crate::renderer::apply_batch_to_tree(&mut tree, batch.as_bytes()).expect("valid batch");
        tree
    }

    /// root(1) > label(2, htmlFor=email) > text(4); root > input(3, id=email).
    const SIBLING: &str = r#"[["createElement",1,"div"],["setRoot",1],
        ["createElement",2,"label"],["appendChild",1,2],["setCustomProp",2,"htmlFor","email"],
        ["createElement",3,"input"],["appendChild",1,3],["setCustomProp",3,"id","email"],
        ["createElement",4,"div"],["appendChild",2,4]]"#;

    #[test]
    fn a_click_inside_the_label_activates_its_control() {
        let tree = tree_with(SIBLING);
        assert_eq!(activated_control(&tree, &[4, 2, 1]), Some(3));
        assert_eq!(activated_control(&tree, &[2, 1]), Some(3));
    }

    #[test]
    fn a_click_outside_every_label_activates_nothing() {
        let tree = tree_with(SIBLING);
        assert_eq!(activated_control(&tree, &[1]), None);
        assert_eq!(activated_control(&tree, &[3, 1]), None);
    }

    #[test]
    fn a_click_on_the_control_inside_its_label_does_not_activate_it_again() {
        let tree = tree_with(
            r#"[["createElement",1,"label"],["setRoot",1],
                ["createElement",2,"input"],["appendChild",1,2]]"#,
        );
        assert_eq!(activated_control(&tree, &[2, 1]), None);
        assert_eq!(activated_control(&tree, &[1]), Some(2));
    }

    #[test]
    fn a_disabled_or_inert_control_is_not_activated() {
        let mut tree = tree_with(SIBLING);
        crate::renderer::apply_batch_to_tree(
            &mut tree,
            br#"[["setCustomProp",3,"disabled",true]]"#,
        )
        .unwrap();
        assert_eq!(activated_control(&tree, &[2, 1]), None);

        let mut tree = tree_with(SIBLING);
        crate::renderer::apply_batch_to_tree(&mut tree, br#"[["setCustomProp",3,"inert",true]]"#)
            .unwrap();
        assert_eq!(activated_control(&tree, &[2, 1]), None);
    }

    #[test]
    fn the_activation_click_keeps_the_original_coordinates_and_detail() {
        let tree = tree_with(SIBLING);
        let click = EventPayload {
            element_id: 2.0,
            event_type: "click".to_string(),
            target: Some(2.0),
            x: Some(12.0),
            y: Some(7.0),
            detail: Some(1),
            offset_x: Some(3.0),
            ..Default::default()
        };
        let activation = activation_click(&tree, &click, 3);
        assert_eq!(activation.event_type, "click");
        assert_eq!(activation.target, Some(3.0));
        assert_eq!(activation.path, Some(vec![3.0, 1.0]));
        assert_eq!((activation.x, activation.y), (Some(12.0), Some(7.0)));
        assert_eq!(activation.detail, Some(1));
        assert_eq!(activation.offset_x, None);
    }
}
