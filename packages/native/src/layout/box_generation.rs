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
