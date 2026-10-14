//! Propagation paths over the retained tree.
//!
//! The path of an event is the target and then each retained ancestor, so an
//! `<anchored>` layer bubbles to the element it was declared under, not to the
//! window root it paints against. That is what a React portal does.

use crate::retained_tree::RetainedTree;

/// `id` and then each ancestor up to the root. Empty when `id` is not in the tree.
pub fn event_path(tree: &RetainedTree, id: u64) -> Vec<u64> {
    let mut path = Vec::new();
    let mut next = tree.elements.contains_key(&id).then_some(id);
    while let Some(current) = next {
        path.push(current);
        next = tree
            .elements
            .get(&current)
            .and_then(|element| element.parent);
    }
    path
}

/// Whether `id` or an ancestor listens for `event_type`, in either phase.
/// JS tells native about a type once while either phase has a handler.
pub fn path_listens(tree: &RetainedTree, id: u64, event_type: &str) -> bool {
    let mut next = Some(id);
    while let Some(current) = next {
        let Some(element) = tree.elements.get(&current) else {
            return false;
        };
        if element.events().contains(event_type) {
            return true;
        }
        next = element.parent;
    }
    false
}

/// The innermost element of `first` that is also in `second`: the DOM target of a
/// `click` whose press and release landed on different elements.
pub fn common_ancestor(first: &[u64], second: &[u64]) -> Option<u64> {
    first.iter().copied().find(|id| second.contains(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_innermost_shared_element_is_the_common_ancestor() {
        assert_eq!(common_ancestor(&[5, 3, 1], &[6, 3, 1]), Some(3));
        assert_eq!(common_ancestor(&[5, 3, 1], &[3, 1]), Some(3));
        assert_eq!(common_ancestor(&[5, 3, 1], &[9, 8]), None);
        assert_eq!(common_ancestor(&[], &[1]), None);
    }
}
