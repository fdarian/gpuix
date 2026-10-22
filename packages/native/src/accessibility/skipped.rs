//! The accessibility nodes of content that `content-visibility` skips.
//!
//! Chrome keeps the content of a skipped `auto` element in the accessibility tree (roles and names come
//! from the DOM, no layout is needed) and leaves a `hidden` one out. Nothing below a skipped element is
//! built here, so the element hosts a synthetic node for each element below it that has one, made from
//! the retained tree by the code that makes the node of a built element ([`super::apply_accessibility`]):
//! role, name, `aria-*`, relations and the text of a `<text>`. The nodes have no bounds and no actions.

use gpui::prelude::*;
use gpui::{A11yRelation, A11ySubtreeBuilder, Element, SharedString};

use crate::layout::content_visibility::ContentVisibility;
use crate::retained_tree::{RetainedElement, RetainedTree};

/// A node for an element below a skipped one, with the nodes of the elements below it.
pub(crate) struct Exposed {
    id: u64,
    node: gpui::accesskit::Node,
    relations: Vec<(A11yRelation, Vec<SharedString>)>,
    children: Vec<Exposed>,
}

/// The nodes of everything below `host`, which skips its content, in document order. Nothing for a
/// `hidden` element.
pub(crate) fn exposed(tree: &RetainedTree, host: &RetainedElement) -> Vec<Exposed> {
    if tree.content_visibility_of(host) == ContentVisibility::Hidden
        || super::aria::hides_subtree(host.custom_props())
    {
        return Vec::new();
    }
    let mut nodes = Vec::new();
    below(tree, host, tree.is_visibility_hidden(host.id), &mut nodes);
    nodes
}

/// Hands the nodes to `el`, whose node they hang off.
pub(crate) fn host<E>(el: E, nodes: Vec<Exposed>) -> E
where
    E: StatefulInteractiveElement,
{
    el.a11y_synthetic_children(move |builder| push(builder, nodes))
}

fn push(builder: &mut A11ySubtreeBuilder, nodes: Vec<Exposed>) {
    for exposed in nodes {
        let id = builder.synthetic_node_id(exposed.id);
        let children = exposed.children;
        if builder.push_nested(id, exposed.node, |builder| push(builder, children)) {
            for (relation, targets) in &exposed.relations {
                builder.push_relation(id, *relation, targets);
            }
        }
    }
}

/// Appends the nodes of the children of `parent` to `into`. `hidden` is whether `visibility: hidden`
/// is inherited from `parent`.
fn below(tree: &RetainedTree, parent: &RetainedElement, hidden: bool, into: &mut Vec<Exposed>) {
    for id in crate::layout::contents::flatten(tree, &parent.children).iter() {
        let Some(element) = tree.elements.get(id) else { continue };
        if crate::layout::box_generation::is_display_none(element)
            || crate::text::inline::is_part_of_paragraph(tree, element)
            || super::aria::hides_subtree(element.custom_props())
        {
            continue;
        }
        let hidden = element.declared_hidden().unwrap_or(hidden);
        let mut children = Vec::new();
        if !skips_all(tree, element) {
            below(tree, element, hidden, &mut children);
        }
        match node_of(tree, element).filter(|_| !hidden) {
            Some((node, relations)) => into.push(Exposed { id: element.id, node, relations, children }),
            None => into.extend(children),
        }
    }
}

/// Whether `element` skips its content whatever is near: `hidden`. A nested `auto` element is
/// exposed down to its leaves, as one that is not near would be.
fn skips_all(tree: &RetainedTree, element: &RetainedElement) -> bool {
    crate::text::inline::is_host_container(element)
        && !crate::layout::contents::is_display_contents(element)
        && tree.content_visibility_of(element) == ContentVisibility::Hidden
}

/// The node the build of `element` would give it, and the relations of the node. `None` for an
/// element without a role.
fn node_of(tree: &RetainedTree, element: &RetainedElement) -> Option<(gpui::accesskit::Node, Vec<(A11yRelation, Vec<SharedString>)>)> {
    let plan = if element.element_type == "text" && !element.children.is_empty() {
        crate::text::inline::plan_children_at(tree, element)
    } else {
        Vec::new()
    };
    let mut probe = super::apply_accessibility(
        gpui::div().id(crate::events::ids::host_element_id(element.id)),
        element.id,
        element.custom_props(),
        super::default_role(element),
    );
    probe = super::describe_text(probe, element, &plan);
    if element.element_type == "label" {
        probe = super::label::describe(probe, element.id);
    }
    if matches!(element.element_type.as_str(), "input" | "textarea") {
        probe = describe_field(probe, element);
    }
    let role = probe.a11y_role()?;
    let mut node = gpui::accesskit::Node::new(role);
    probe.write_a11y_info(&mut node);
    Some((node, probe.a11y_relations().to_vec()))
}

/// The value and the placeholder of a text control, as its editor gives them.
fn describe_field<E>(mut el: E, element: &RetainedElement) -> E
where
    E: StatefulInteractiveElement,
{
    let props = element.custom_props();
    let text = |key: &str| props.get(key).and_then(|value| value.as_str()).filter(|text| !text.is_empty());
    let masked = props.get("type").and_then(|value| value.as_str()) == Some("password");
    if let Some(value) = text("value").filter(|_| !masked && !props.contains_key("aria-valuetext")) {
        el = el.aria_value(value.to_string());
    }
    if let Some(placeholder) = text("placeholder") {
        el = el.aria_placeholder(placeholder.to_string());
    }
    el
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui::accesskit::Role;

    use super::*;

    fn tree() -> RetainedTree {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        tree
    }

    fn add(tree: &mut RetainedTree, parent: u64, id: u64, props: &[(&str, serde_json::Value)], style: serde_json::Value) {
        tree.create_element(id, "div".to_string());
        tree.append_child(parent, id);
        for (key, value) in props {
            tree.set_custom_prop(id, key.to_string(), value.clone());
        }
        tree.set_style(id, Arc::new(serde_json::from_value(style).expect("a style")));
    }

    fn role(role: &str) -> [(&str, serde_json::Value); 1] {
        [("role", serde_json::json!(role))]
    }

    /// The ids of the nodes, in order, as `(id, role)` and the nodes below, indented by depth.
    fn outline(nodes: &[Exposed], depth: usize, into: &mut Vec<(usize, u64, Role)>) {
        for exposed in nodes {
            into.push((depth, exposed.id, exposed.node.role()));
            outline(&exposed.children, depth + 1, into);
        }
    }

    fn outline_of(tree: &RetainedTree, host: u64) -> Vec<(usize, u64, Role)> {
        let mut out = Vec::new();
        outline(&exposed(tree, &tree.elements[&host]), 0, &mut out);
        out
    }

    #[test]
    fn the_nodes_are_in_document_order_and_nested_under_the_elements_that_have_one() {
        let mut tree = tree();
        add(&mut tree, 1, 2, &role("heading"), serde_json::json!({}));
        add(&mut tree, 1, 3, &[], serde_json::json!({}));
        add(&mut tree, 3, 4, &role("list"), serde_json::json!({}));
        add(&mut tree, 4, 5, &role("listitem"), serde_json::json!({}));
        add(&mut tree, 3, 6, &role("button"), serde_json::json!({}));
        assert_eq!(
            outline_of(&tree, 1),
            [(0, 2, Role::Heading), (0, 4, Role::List), (1, 5, Role::ListItem), (0, 6, Role::Button)]
        );
    }

    #[test]
    fn a_hidden_element_exposes_nothing_and_a_hidden_child_only_itself() {
        let mut tree = tree();
        add(&mut tree, 1, 2, &role("region"), serde_json::json!({ "contentVisibility": "hidden" }));
        add(&mut tree, 2, 3, &role("button"), serde_json::json!({}));
        assert_eq!(outline_of(&tree, 1), [(0, 2, Role::Region)]);
        tree.set_style(1, Arc::new(serde_json::from_value(serde_json::json!({ "contentVisibility": "hidden" })).unwrap()));
        assert_eq!(outline_of(&tree, 1), []);
    }

    #[test]
    fn a_nested_auto_element_is_exposed_down_to_its_leaves() {
        let mut tree = tree();
        add(&mut tree, 1, 2, &[], serde_json::json!({ "contentVisibility": "auto" }));
        add(&mut tree, 2, 3, &role("button"), serde_json::json!({}));
        assert_eq!(outline_of(&tree, 1), [(0, 3, Role::Button)]);
    }

    #[test]
    fn display_none_aria_hidden_inert_and_visibility_hidden_are_left_out() {
        let mut tree = tree();
        add(&mut tree, 1, 2, &role("button"), serde_json::json!({ "display": "none" }));
        add(&mut tree, 1, 3, &[("aria-hidden", serde_json::json!(true))], serde_json::json!({}));
        add(&mut tree, 3, 4, &role("button"), serde_json::json!({}));
        add(&mut tree, 1, 5, &[("inert", serde_json::json!(true)), ("role", serde_json::json!("button"))], serde_json::json!({}));
        add(&mut tree, 1, 6, &role("group"), serde_json::json!({ "visibility": "hidden" }));
        add(&mut tree, 6, 7, &role("button"), serde_json::json!({ "visibility": "visible" }));
        add(&mut tree, 6, 8, &role("button"), serde_json::json!({}));
        assert_eq!(outline_of(&tree, 1), [(0, 7, Role::Button)]);
    }

    #[test]
    fn flex_order_does_not_move_a_node() {
        let mut tree = tree();
        add(&mut tree, 1, 2, &role("button"), serde_json::json!({ "order": 2 }));
        add(&mut tree, 1, 3, &role("heading"), serde_json::json!({ "order": 1 }));
        tree.set_style(1, Arc::new(serde_json::from_value(serde_json::json!({ "display": "flex" })).unwrap()));
        assert_eq!(outline_of(&tree, 1), [(0, 2, Role::Button), (0, 3, Role::Heading)]);
    }

    #[test]
    fn a_text_host_reads_out_its_text_and_a_label_its_default_role() {
        let mut tree = tree();
        tree.create_element(2, "text".to_string());
        tree.append_child(1, 2);
        tree.create_element(3, "text".to_string());
        tree.append_child(2, 3);
        tree.set_text(3, "Hello ".to_string());
        tree.create_element(4, "text".to_string());
        tree.append_child(2, 4);
        tree.set_text(4, "there".to_string());
        tree.set_style(2, Arc::new(serde_json::from_value(serde_json::json!({ "height": 10 })).unwrap()));
        tree.create_element(5, "label".to_string());
        tree.append_child(1, 5);
        let nodes = exposed(&tree, &tree.elements[&1]);
        assert_eq!(nodes.len(), 2);
        assert_eq!((nodes[0].node.role(), nodes[0].node.value()), (Role::Label, Some("Hello there")));
        assert_eq!(nodes[1].node.role(), Role::Label);
    }

    #[test]
    fn a_relation_is_kept_for_the_node() {
        let mut tree = tree();
        add(
            &mut tree,
            1,
            2,
            &[("role", serde_json::json!("button")), ("aria-labelledby", serde_json::json!("title"))],
            serde_json::json!({}),
        );
        let nodes = exposed(&tree, &tree.elements[&1]);
        assert_eq!(nodes[0].relations.len(), 1);
        assert_eq!(nodes[0].relations[0].0, A11yRelation::LabelledBy);
    }
}
