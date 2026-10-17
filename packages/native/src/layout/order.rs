use std::borrow::Cow;

use crate::retained_tree::{RetainedElement, RetainedTree};

/// The children of a container with `display: contents` elements spliced in, in `order` order
/// when it is a flex or grid container, by the display its `@container` blocks gave it.
///
/// Taffy has no `order`, and GPUI paints children in the order they are added,
/// so sorting the list that gets built moves layout and paint together. The sort
/// is stable, so equal `order` values keep document order. Focus and tab order
/// never read this list: they follow the retained tree.
pub(crate) fn children_in_order<'a>(
    tree: &RetainedTree,
    element: &'a RetainedElement,
) -> Cow<'a, [u64]> {
    let children = super::contents::flatten(tree, &element.children);
    let is_flex_or_grid = super::display::lays_out_items(element.display().as_deref());
    if !is_flex_or_grid || !children.iter().any(|id| order_of(tree, *id) != 0) {
        return children;
    }
    let mut sorted = children.into_owned();
    sorted.sort_by_key(|id| order_of(tree, *id));
    Cow::Owned(sorted)
}

/// Out-of-flow children are not flex or grid items, so `order` does not apply.
fn order_of(tree: &RetainedTree, id: u64) -> i32 {
    let Some(style) = tree
        .elements
        .get(&id)
        .and_then(|child| child.style.as_deref())
    else {
        return 0;
    };
    if matches!(style.position.as_deref(), Some("absolute") | Some("fixed")) {
        return 0;
    }
    style
        .order
        .map_or(0, |order| order.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::StyleDesc;

    fn tree_with_orders(orders: &[Option<f64>], display: &str) -> (RetainedTree, u64) {
        let mut tree = RetainedTree::new();
        let mut parent = RetainedElement::new(1, "div".to_string(), 0);
        for (index, order) in orders.iter().enumerate() {
            let id = 10 + index as u64;
            let mut child = RetainedElement::new(id, "div".to_string(), 0);
            child.style = Some(std::sync::Arc::new(StyleDesc {
                order: *order,
                ..Default::default()
            }));
            parent.children.push(id);
            tree.elements.insert(id, child);
        }
        parent.style = Some(std::sync::Arc::new(StyleDesc {
            display: Some(display.to_string()),
            ..Default::default()
        }));
        tree.elements.insert(1, parent);
        (tree, 1)
    }

    fn sorted(orders: &[Option<f64>], display: &str) -> Vec<u64> {
        let (tree, id) = tree_with_orders(orders, display);
        let element = tree.elements.get(&id).unwrap();
        children_in_order(&tree, element).into_owned()
    }

    #[test]
    fn sorts_by_order_and_keeps_document_order_for_ties() {
        let result = sorted(&[Some(2.0), None, Some(-1.0), Some(2.0), Some(0.0)], "flex");
        assert_eq!(result, vec![12, 11, 14, 10, 13]);
    }

    #[test]
    fn a_display_a_container_block_gave_decides_whether_order_applies() {
        let (tree, id) = tree_with_orders(&[Some(2.0), Some(1.0)], "block");
        let element = tree.elements.get(&id).unwrap();
        *element.container_display.borrow_mut() = Some(Some("inline-flex".to_string()));
        assert_eq!(children_in_order(&tree, element).into_owned(), vec![11, 10]);

        let (tree, id) = tree_with_orders(&[Some(2.0), Some(1.0)], "flex");
        let element = tree.elements.get(&id).unwrap();
        *element.container_display.borrow_mut() = Some(Some("block".to_string()));
        assert_eq!(children_in_order(&tree, element).into_owned(), vec![10, 11]);
    }

    #[test]
    fn leaves_block_containers_alone() {
        let result = sorted(&[Some(2.0), Some(1.0)], "block");
        assert_eq!(result, vec![10, 11]);
    }
}
