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

/// The capture-phase twin of a type that does not bubble, as JS registers it.
/// These types reach an ancestor in the capture phase only, so native has to
/// tell a capture listener from a bubble one to know whether anyone above hears
/// the event.
fn capture_twin(event_type: &str) -> Option<&'static str> {
    match event_type {
        "mouseenter" => Some("mouseenterCapture"),
        "mouseleave" => Some("mouseleaveCapture"),
        "pointerenter" => Some("pointerenterCapture"),
        "pointerleave" => Some("pointerleaveCapture"),
        _ => None,
    }
}

/// Whether `id` or an ancestor listens for `event_type`, in either phase. JS
/// tells native about a type once while either phase has a handler. A type that
/// does not bubble is heard by the element itself, in either phase, and by an
/// ancestor's capture listener alone.
pub fn path_listens(tree: &RetainedTree, id: u64, event_type: &str) -> bool {
    let capture_type = capture_twin(event_type);
    let mut next = Some(id);
    while let Some(current) = next {
        let Some(element) = tree.elements.get(&current) else {
            return false;
        };
        let events = element.events();
        match capture_type {
            Some(capture_type) => {
                if events.contains(capture_type) || (current == id && events.contains(event_type)) {
                    return true;
                }
            }
            None => {
                if events.contains(event_type) {
                    return true;
                }
            }
        }
        next = element.parent;
    }
    false
}

/// Whether GPUI's hit chain (target first) and the retained path of its target
/// describe the same ancestry. GPUI knows only which hitboxes were being
/// prepainted when another was inserted, so the chain skips retained elements
/// with no hitbox (a `<virtual-list>`, a text instance) but must otherwise walk
/// the same parents in the same order. A chain element missing from the path, or
/// out of order, means the build nests elements differently from the tree.
pub fn chain_follows_path(chain: &[u64], path: &[u64]) -> bool {
    let mut remaining = path.iter();
    chain
        .iter()
        .all(|id| remaining.any(|candidate| candidate == id))
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
    fn a_chain_that_skips_unhit_ancestors_still_follows_the_path() {
        assert!(chain_follows_path(&[4, 2, 1], &[4, 3, 2, 1]));
        assert!(chain_follows_path(&[], &[4, 3]));
        assert!(!chain_follows_path(&[4, 5, 1], &[4, 3, 2, 1]));
        assert!(!chain_follows_path(&[2, 4], &[4, 3, 2, 1]));
    }

    #[test]
    fn a_chain_that_ends_at_an_anchored_layer_follows_the_path_through_its_declaring_ancestors() {
        // The layer is a hitbox root in GPUI: its chain stops at the layer, while
        // the retained path continues to the elements it was declared under.
        assert!(chain_follows_path(&[6, 5], &[6, 5, 3, 2, 1]));
    }

    #[test]
    fn a_display_contents_ancestor_is_in_the_path_in_order_though_it_has_no_hitbox() {
        let mut tree = RetainedTree::new();
        for id in 1..=4 {
            tree.create_element(id, "div".to_string());
        }
        tree.append_child(1, 2);
        tree.append_child(2, 3);
        tree.append_child(3, 4);
        for id in [2, 3] {
            let contents = serde_json::json!({"display": "contents"});
            tree.set_style(id, std::sync::Arc::new(serde_json::from_value(contents).expect("a style")));
        }

        let path = event_path(&tree, 4);
        assert_eq!(path, [4, 3, 2, 1]);
        // GPUI's chain holds only the elements that built a hitbox.
        assert!(chain_follows_path(&[4, 1], &path));
    }

    #[test]
    fn the_innermost_shared_element_is_the_common_ancestor() {
        assert_eq!(common_ancestor(&[5, 3, 1], &[6, 3, 1]), Some(3));
        assert_eq!(common_ancestor(&[5, 3, 1], &[3, 1]), Some(3));
        assert_eq!(common_ancestor(&[5, 3, 1], &[9, 8]), None);
        assert_eq!(common_ancestor(&[], &[1]), None);
    }
}
