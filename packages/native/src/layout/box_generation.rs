use crate::retained_tree::{RetainedElement, RetainedTree};

/// Whether a child produces a box at all: `display: none` generates none.
///
/// Empty and white-space-only text is not decided here. Inline layout in
/// `text::inline` owns that rule, because whether white space produces a box
/// depends on the inline formatting context it sits in.
pub(crate) fn generates_box(tree: &RetainedTree, child_id: u64) -> bool {
    tree.elements
        .get(&child_id)
        .is_some_and(|child| !is_display_none(child))
}

/// `display: none` generates no box, and neither do its descendants.
pub(crate) fn is_display_none(element: &RetainedElement) -> bool {
    element
        .style
        .as_deref()
        .and_then(|style| style.display.as_deref())
        == Some("none")
}

/// Whether `element` or an ancestor is `display: none`, so no box is built for it.
pub(crate) fn is_hidden(tree: &RetainedTree, element: &RetainedElement) -> bool {
    let mut current = Some(element);
    while let Some(element) = current {
        if is_display_none(element) {
            return true;
        }
        current = element.parent.and_then(|parent| tree.elements.get(&parent));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn an_element_under_a_display_none_ancestor_is_hidden() {
        let mut tree = RetainedTree::new();
        for (id, kind) in [(1, "div"), (2, "div"), (3, "img"), (4, "img")] {
            tree.create_element(id, kind.to_string());
        }
        tree.append_child(1, 2);
        tree.append_child(2, 3);
        tree.append_child(1, 4);
        let none = serde_json::json!({"display": "none"});
        tree.set_style(2, Arc::new(serde_json::from_value(none).expect("a style")));
        assert!(is_hidden(&tree, &tree.elements[&3]));
        assert!(is_hidden(&tree, &tree.elements[&2]));
        assert!(!is_hidden(&tree, &tree.elements[&4]));
        assert!(!is_hidden(&tree, &tree.elements[&1]));
    }
}
