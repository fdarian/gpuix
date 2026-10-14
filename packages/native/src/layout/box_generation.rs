use crate::retained_tree::{RetainedElement, RetainedTree};

/// Whether a child of `parent` produces a box at all.
///
/// In the DOM an empty text node has no layout box, and a text node of only
/// white space produces none either unless it sits in an inline formatting
/// context. Solid creates an empty text node for every dynamic hole (`<Show>`,
/// `For`, `{cond && x}`), and React emits `{" "}` between siblings, so letting
/// them build a wrapper `<div>` made each one a flex/grid item that took a
/// `gap` slot.
///
/// A `<text>` parent is the inline context here, so it keeps its white space.
/// `display: none` children produce no box either.
pub(crate) fn generates_box(tree: &RetainedTree, parent: &RetainedElement, child_id: u64) -> bool {
    let Some(child) = tree.elements.get(&child_id) else {
        return false;
    };
    is_visible_text_node(parent, child)
}

/// `display: none` generates no box, and neither do its descendants.
pub(crate) fn is_display_none(element: &RetainedElement) -> bool {
    element
        .style
        .as_deref()
        .and_then(|style| style.display.as_deref())
        == Some("none")
}

fn is_visible_text_node(parent: &RetainedElement, child: &RetainedElement) -> bool {
    if is_display_none(child) {
        return false;
    }
    if child.element_type != "text" {
        return true;
    }
    let Some(content) = child.content.as_deref() else {
        return true;
    };
    if content.is_empty() {
        return false;
    }
    parent.element_type == "text" || !is_collapsible_whitespace(content)
}

/// CSS white space is space, tab, line feed, carriage return and form feed.
/// A no-break space is content, not white space.
fn is_collapsible_whitespace(content: &str) -> bool {
    content
        .chars()
        .all(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(id: u64, element_type: &str, content: Option<&str>) -> RetainedElement {
        let mut element = RetainedElement::new(id, element_type.to_string(), 0);
        element.content = content.map(str::to_string);
        element
    }

    #[test]
    fn an_empty_text_node_has_no_box_anywhere() {
        let empty = element(2, "text", Some(""));
        assert!(!is_visible_text_node(&element(1, "div", None), &empty));
        assert!(!is_visible_text_node(&element(1, "text", None), &empty));
    }

    #[test]
    fn white_space_only_counts_inside_text() {
        let space = element(2, "text", Some(" \n"));
        assert!(!is_visible_text_node(&element(1, "div", None), &space));
        assert!(is_visible_text_node(&element(1, "text", None), &space));
    }

    #[test]
    fn a_no_break_space_is_content() {
        let nbsp = element(2, "text", Some("\u{a0}"));
        assert!(is_visible_text_node(&element(1, "div", None), &nbsp));
    }

    #[test]
    fn elements_and_text_hosts_always_build() {
        let parent = element(1, "div", None);
        assert!(is_visible_text_node(&parent, &element(2, "div", None)));
        assert!(is_visible_text_node(&parent, &element(2, "text", None)));
    }
}
