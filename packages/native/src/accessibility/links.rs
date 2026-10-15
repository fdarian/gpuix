//! Cross-element accessibility links, resolved once per tree revision:
//! `<label>` to control, and which role-less elements an `aria-*` id reference
//! names.
//!
//! A control's accessible name comes from its labels, so `apply_accessibility`
//! needs to know which labels point at it while it builds the control, and the
//! tree has no back-pointer. The pass below walks the tree in document order
//! and keeps the answer until the tree changes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::retained_tree::{RetainedElement, RetainedTree};

/// Element types a `<label>` can be associated with.
pub(crate) fn is_labelable(element_type: &str) -> bool {
    matches!(element_type, "input" | "textarea")
}

#[derive(Default)]
pub(crate) struct A11yLinks {
    /// Labels of a control, in document order, as author ids.
    labels_of: HashMap<u64, Vec<String>>,
    /// The control a label activates.
    control_of: HashMap<u64, u64>,
    /// Author ids of labels that have no DOM `id`.
    label_author_ids: HashMap<u64, String>,
    label_text: HashMap<u64, String>,
    /// Role-less elements whose `id` some `aria-*` reference names, with their
    /// text content. They need a node to be a relation target, and the text is
    /// their name, like the DOM's "name from content".
    referenced_text: HashMap<u64, String>,
}

impl A11yLinks {
    pub(crate) fn labels_of(&self, control: u64) -> Option<&[String]> {
        self.labels_of.get(&control).map(Vec::as_slice)
    }

    pub(crate) fn control_of(&self, label: u64) -> Option<u64> {
        self.control_of.get(&label).copied()
    }

    /// The generated author id of a label without a DOM `id`.
    pub(crate) fn label_author_id(&self, label: u64) -> Option<&str> {
        self.label_author_ids.get(&label).map(String::as_str)
    }

    pub(crate) fn label_text(&self, label: u64) -> Option<&str> {
        self.label_text.get(&label).map(String::as_str)
    }

    pub(crate) fn referenced_text(&self, element: u64) -> Option<&str> {
        self.referenced_text.get(&element).map(String::as_str)
    }
}

/// Identifies the tree state a cached result belongs to. The tree id separates
/// renderers that share a thread (every test renderer does).
#[derive(PartialEq, Eq, Clone, Copy)]
struct CacheKey {
    tree: u64,
    root: u64,
    revision: u64,
}

thread_local! {
    static CACHE: RefCell<Option<(CacheKey, Rc<A11yLinks>)>> = const { RefCell::new(None) };
}

/// Recompute the links when the tree changed since the last call. Call before
/// building a frame, and again before any later build of part of the same
/// tree (virtual list rows), which may see a newer tree.
pub(crate) fn sync(tree: &RetainedTree) {
    let Some(root) = tree.root_id.and_then(|id| tree.elements.get(&id)) else {
        CACHE.with(|cache| *cache.borrow_mut() = None);
        return;
    };
    let key = CacheKey {
        tree: tree.id(),
        root: root.id,
        revision: root.subtree_revision,
    };
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.as_ref().is_some_and(|(cached, _)| *cached == key) {
            return;
        }
        *cache = Some((key, Rc::new(compute(tree, root))));
    });
}

pub(crate) fn current() -> Rc<A11yLinks> {
    CACHE.with(|cache| {
        cache
            .borrow()
            .as_ref()
            .map(|(_, links)| links.clone())
            .unwrap_or_default()
    })
}

fn dom_id(element: &RetainedElement) -> Option<&str> {
    element
        .custom_props()
        .get("id")
        .and_then(|value| value.as_str())
        .filter(|id| !id.is_empty())
}

fn has_role(element: &RetainedElement) -> bool {
    element
        .custom_props()
        .get("role")
        .and_then(|value| value.as_str())
        .is_some_and(|role| !matches!(role, "" | "none" | "presentation" | "generic"))
}

fn compute(tree: &RetainedTree, root: &RetainedElement) -> A11yLinks {
    let mut links = A11yLinks::default();
    let mut referenced: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut has_labels = false;
    for element in tree.elements.values() {
        has_labels |= element.element_type == "label";
        let props = element.custom_props();
        for (key, _) in super::aria::RELATION_PROPS {
            if let Some(value) = props.get(key).and_then(|value| value.as_str()) {
                referenced.extend(value.split_whitespace());
            }
        }
    }
    if !has_labels && referenced.is_empty() {
        return links;
    }

    let mut labels = Vec::new();
    let mut first_by_dom_id: HashMap<&str, &RetainedElement> = HashMap::new();
    let mut stack = vec![root];
    while let Some(element) = stack.pop() {
        if element.element_type == "label" {
            labels.push(element);
        }
        if let Some(id) = dom_id(element) {
            first_by_dom_id.entry(id).or_insert(element);
        }
        // Reversed so the first child pops first: document order.
        stack.extend(
            element
                .children
                .iter()
                .rev()
                .filter_map(|child| tree.elements.get(child)),
        );
    }

    for (id, element) in &first_by_dom_id {
        let is_text_host = element.element_type == "text" && element.content.is_none();
        if referenced.contains(id)
            && !has_role(element)
            && !is_text_host
            && element.element_type != "label"
        {
            links
                .referenced_text
                .insert(element.id, text_of(tree, element));
        }
    }

    for label in labels {
        let label_author_id = match dom_id(label) {
            Some(id) => id.to_string(),
            None => {
                let generated = format!("gpuix-label-{}", label.id);
                links.label_author_ids.insert(label.id, generated.clone());
                generated
            }
        };
        links.label_text.insert(label.id, text_of(tree, label));

        let control = match label.custom_props().get("htmlFor") {
            Some(target) => target
                .as_str()
                .and_then(|id| first_by_dom_id.get(id))
                .filter(|element| is_labelable(&element.element_type))
                .map(|element| element.id),
            None => first_labelable_descendant(tree, label),
        };
        let Some(control) = control else {
            continue;
        };
        links.control_of.insert(label.id, control);
        links
            .labels_of
            .entry(control)
            .or_default()
            .push(label_author_id);
    }
    links
}

fn first_labelable_descendant(tree: &RetainedTree, label: &RetainedElement) -> Option<u64> {
    let mut stack: Vec<&RetainedElement> = label
        .children
        .iter()
        .rev()
        .filter_map(|child| tree.elements.get(child))
        .collect();
    while let Some(element) = stack.pop() {
        if is_labelable(&element.element_type) {
            return Some(element.id);
        }
        stack.extend(
            element
                .children
                .iter()
                .rev()
                .filter_map(|child| tree.elements.get(child)),
        );
    }
    None
}

/// The label's text, without the text of any control nested in it.
fn text_of(tree: &RetainedTree, label: &RetainedElement) -> String {
    let mut text = String::new();
    let mut stack = vec![label];
    while let Some(element) = stack.pop() {
        if is_labelable(&element.element_type) {
            continue;
        }
        if let Some(content) = element.content.as_deref() {
            text.push_str(content);
        }
        stack.extend(
            element
                .children
                .iter()
                .rev()
                .filter_map(|child| tree.elements.get(child)),
        );
    }
    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree_with(batch: &str) -> RetainedTree {
        let mut tree = RetainedTree::new();
        crate::renderer::apply_batch_to_tree(&mut tree, batch.as_bytes()).expect("valid batch");
        tree
    }

    fn links_of(tree: &RetainedTree) -> Rc<A11yLinks> {
        sync(tree);
        current()
    }

    #[test]
    fn a_new_tree_at_the_address_of_a_dropped_one_does_not_inherit_its_links() {
        let mut tree = tree_with(
            r#"[["createElement",1,"div"],["setRoot",1],
                ["createElement",2,"label"],["appendChild",1,2],["setCustomProp",2,"htmlFor","email"],
                ["createElement",3,"input"],["appendChild",1,3],["setCustomProp",3,"id","email"]]"#,
        );
        assert_eq!(links_of(&tree).control_of(2), Some(3));

        // Assigning drops the old tree and builds the new one in the same slot,
        // with the same root id and the same number of mutations.
        tree = tree_with(
            r#"[["createElement",1,"div"],["setRoot",1],
                ["createElement",2,"label"],["appendChild",1,2],["setCustomProp",2,"htmlFor","email"],
                ["createElement",3,"input"],["appendChild",1,3],["setCustomProp",3,"id","other"]]"#,
        );
        assert_eq!(links_of(&tree).control_of(2), None);
    }

    #[test]
    fn html_for_targets_the_labelable_with_that_dom_id() {
        let tree = tree_with(
            r#"[["createElement",1,"div"],["setRoot",1],
                ["createElement",2,"label"],["appendChild",1,2],["setCustomProp",2,"htmlFor","email"],
                ["createElement",3,"input"],["appendChild",1,3],["setCustomProp",3,"id","email"],
                ["createElement",4,"div"],["appendChild",1,4],["setCustomProp",4,"id","other"]]"#,
        );
        let links = links_of(&tree);
        assert_eq!(links.control_of(2), Some(3));
        assert_eq!(links.labels_of(3), Some(&["gpuix-label-2".to_string()][..]));
    }

    #[test]
    fn html_for_naming_a_non_labelable_associates_nothing() {
        let tree = tree_with(
            r#"[["createElement",1,"div"],["setRoot",1],
                ["createElement",2,"label"],["appendChild",1,2],["setCustomProp",2,"htmlFor","box"],
                ["createElement",3,"input"],["appendChild",2,3],
                ["createElement",4,"div"],["appendChild",1,4],["setCustomProp",4,"id","box"]]"#,
        );
        let links = links_of(&tree);
        assert_eq!(links.control_of(2), None);
        assert!(links.labels_of(3).is_none());
    }

    #[test]
    fn without_html_for_the_first_labelable_descendant_is_the_control() {
        let tree = tree_with(
            r#"[["createElement",1,"label"],["setRoot",1],["setCustomProp",1,"id","lbl"],
                ["createElement",2,"div"],["appendChild",1,2],
                ["createElement",3,"textarea"],["appendChild",2,3],
                ["createElement",4,"input"],["appendChild",1,4]]"#,
        );
        let links = links_of(&tree);
        assert_eq!(links.control_of(1), Some(3));
        assert_eq!(links.labels_of(3), Some(&["lbl".to_string()][..]));
        assert_eq!(links.label_author_id(1), None);
    }

    #[test]
    fn referenced_role_less_elements_are_listed_with_their_text() {
        let tree = tree_with(
            r#"[["createElement",1,"div"],["setRoot",1],
                ["createElement",2,"input"],["appendChild",1,2],
                ["setCustomProp",2,"aria-describedby","hint other"],
                ["createElement",3,"div"],["appendChild",1,3],["setCustomProp",3,"id","hint"],
                ["createElement",4,"text"],["appendChild",3,4],["setText",4,"Be brief"],
                ["createElement",5,"div"],["appendChild",1,5],["setCustomProp",5,"id","other"],
                ["setCustomProp",5,"role","note"],
                ["createElement",6,"div"],["appendChild",1,6],["setCustomProp",6,"id","unused"]]"#,
        );
        let links = links_of(&tree);
        assert_eq!(links.referenced_text(3), Some("Be brief"));
        assert_eq!(links.referenced_text(5), None);
        assert_eq!(links.referenced_text(6), None);
    }

    #[test]
    fn label_text_skips_nested_controls() {
        let tree = tree_with(
            r#"[["createElement",1,"label"],["setRoot",1],
                ["createElement",2,"text"],["appendChild",1,2],["setText",2,"Full name "],
                ["createElement",3,"input"],["appendChild",1,3],["setText",3,"typed"]]"#,
        );
        assert_eq!(links_of(&tree).label_text(1), Some("Full name"));
    }
}
