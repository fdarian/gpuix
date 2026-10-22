//! Cross-element accessibility links, resolved once per tree revision:
//! `<label>` to control, and which role-less elements an `aria-*` id reference
//! names.
//!
//! A control's accessible name comes from its labels, so `apply_accessibility`
//! needs to know which labels point at it while it builds the control, and the
//! tree has no back-pointer. The pass below starts from the tree's
//! [`RelationRegistry`](super::relations::RelationRegistry) (the labels, the elements that
//! reference ids and the elements by DOM id), so it reads those and what they relate, never the
//! rest of the tree, and keeps the answer until something it reads changes.

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

    /// What `apply_accessibility` reads of the elements around `host_id` rather than of it, as one
    /// number. None for the many elements nothing links to.
    pub(crate) fn key(&self, host_id: u64) -> Option<u64> {
        let labels = self.labels_of(host_id);
        let referenced = self.referenced_text(host_id);
        let author_id = self.label_author_id(host_id);
        let control = self.control_of(host_id);
        if labels.is_none() && referenced.is_none() && author_id.is_none() && control.is_none() {
            return None;
        }
        let mut fp = crate::retain::Fingerprint::new();
        fp.hash(&labels);
        fp.hash(&referenced);
        fp.hash(&author_id);
        fp.hash(&control);
        Some(fp.finish())
    }

    /// The key of every element that has one.
    fn keys(&self) -> HashMap<u64, u64> {
        self.labels_of
            .keys()
            .chain(self.control_of.keys())
            .chain(self.label_author_ids.keys())
            .chain(self.referenced_text.keys())
            .filter_map(|id| Some((*id, self.key(*id)?)))
            .collect()
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
    /// The elements whose key differs from the one they had before the links were computed again,
    /// until `take_changed`.
    static CHANGED: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

/// The elements whose links changed since this was last called.
pub(crate) fn take_changed() -> Vec<u64> {
    CHANGED.with(|changed| std::mem::take(&mut *changed.borrow_mut()))
}

/// [`take_changed`], leaving them for the frame that takes them.
pub(crate) fn peek_changed() -> Vec<u64> {
    CHANGED.with(|changed| changed.borrow().clone())
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
        revision: tree.relations().revision(),
    };
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.as_ref().is_some_and(|(cached, _)| *cached == key) {
            return;
        }
        let links = compute(tree);
        let before = cache.as_ref().map(|(_, links)| links.keys()).unwrap_or_default();
        let now = links.keys();
        CHANGED.with(|changed| {
            let mut changed = changed.borrow_mut();
            changed.extend(now.iter().filter(|(id, key)| before.get(id) != Some(key)).map(|(id, _)| *id));
            changed.extend(before.keys().filter(|id| !now.contains_key(id)));
        });
        *cache = Some((key, Rc::new(links)));
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

/// The elements from the root down to `id`, or none when it is not under the root. The links
/// are of the tree below the root: a detached element has none.
fn path_from_root(tree: &RetainedTree, id: u64) -> Option<Vec<u64>> {
    let mut path = vec![id];
    loop {
        let element = tree.elements.get(path.last()?)?;
        match element.parent {
            Some(parent) => path.push(parent),
            None => break,
        }
    }
    tree.relations().visit(path.len());
    (tree.root_id == path.last().copied()).then(|| {
        path.reverse();
        path
    })
}

/// Which of two attached elements comes first in document order, an ancestor before what it holds.
fn document_order(tree: &RetainedTree, first: &[u64], second: &[u64]) -> std::cmp::Ordering {
    let shared = first.iter().zip(second).take_while(|(a, b)| a == b).count();
    let (Some(a), Some(b)) = (first.get(shared), second.get(shared)) else {
        return first.len().cmp(&second.len());
    };
    let Some(parent) = tree.elements.get(&first[shared - 1]) else {
        return std::cmp::Ordering::Equal;
    };
    let position_of = |wanted: &u64| parent.children.iter().position(|child| child == wanted);
    tree.relations().visit(parent.children.len());
    position_of(a).cmp(&position_of(b))
}

/// The first element in document order with this DOM `id`, among those under the root.
fn first_with_id<'a>(tree: &'a RetainedTree, id: &str) -> Option<&'a RetainedElement> {
    let mut first: Option<(Vec<u64>, &RetainedElement)> = None;
    for candidate in tree.relations().with_dom_id(id) {
        let Some(path) = path_from_root(tree, *candidate) else {
            continue;
        };
        let element = tree.elements.get(candidate)?;
        let earlier = first
            .as_ref()
            .is_none_or(|(held, _)| document_order(tree, &path, held).is_lt());
        if earlier {
            first = Some((path, element));
        }
    }
    first.map(|(_, element)| element)
}

fn compute(tree: &RetainedTree) -> A11yLinks {
    let registry = tree.relations();
    let mut links = A11yLinks::default();
    if !registry.has_sources() {
        return links;
    }

    let mut referenced: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for source in registry.referrers() {
        registry.visit(1);
        let Some(element) = tree.elements.get(&source) else {
            continue;
        };
        for (key, _) in super::aria::RELATION_PROPS {
            if let Some(value) = element.custom_props().get(key).and_then(|value| value.as_str()) {
                referenced.extend(value.split_whitespace());
            }
        }
    }
    for id in referenced {
        let Some(element) = first_with_id(tree, id) else {
            continue;
        };
        let is_text_host = element.element_type == "text" && element.content.is_none();
        if !has_role(element) && !is_text_host && element.element_type != "label" {
            links
                .referenced_text
                .insert(element.id, text_of(tree, element));
        }
    }

    let mut by_control: HashMap<u64, Vec<(Vec<u64>, String)>> = HashMap::new();
    for label_id in registry.labels() {
        registry.visit(1);
        let Some(path) = path_from_root(tree, label_id) else {
            continue;
        };
        let Some(label) = tree.elements.get(&label_id) else {
            continue;
        };
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
                .and_then(|id| first_with_id(tree, id))
                .filter(|element| is_labelable(&element.element_type))
                .map(|element| element.id),
            None => first_labelable_descendant(tree, label),
        };
        let Some(control) = control else {
            continue;
        };
        links.control_of.insert(label.id, control);
        by_control
            .entry(control)
            .or_default()
            .push((path, label_author_id));
    }
    for (control, mut labels) in by_control {
        labels.sort_by(|(first, _), (second, _)| document_order(tree, first, second));
        links
            .labels_of
            .insert(control, labels.into_iter().map(|(_, author_id)| author_id).collect());
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
        tree.relations().visit(1);
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
        tree.relations().visit(1);
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

    fn apply(tree: &mut RetainedTree, batch: &str) {
        crate::renderer::apply_batch_to_tree(tree, batch.as_bytes()).expect("valid batch");
    }

    #[test]
    fn a_tree_without_labels_or_references_reads_nothing() {
        let mut tree = tree_with(
            r#"[["createElement",1,"div"],["setRoot",1],
                ["createElement",2,"div"],["appendChild",1,2],["setCustomProp",2,"id","a"],
                ["createElement",3,"text"],["appendChild",2,3],["setText",3,"hello"]]"#,
        );
        sync(&tree);
        apply(&mut tree, r#"[["setText",3,"bye"],["setStyle",2,{"width":10}],["destroyElement",3]]"#);
        sync(&tree);
        assert_eq!(tree.relations().visits(), 0);
    }

    #[test]
    fn a_change_that_no_link_reads_computes_nothing_again() {
        let mut tree = tree_with(
            r#"[["createElement",1,"div"],["setRoot",1],
                ["createElement",2,"label"],["appendChild",1,2],
                ["createElement",3,"input"],["appendChild",2,3],
                ["createElement",4,"div"],["appendChild",1,4],
                ["createElement",5,"text"],["appendChild",4,5],["setText",5,"elsewhere"]]"#,
        );
        sync(&tree);
        let visits = tree.relations().visits();
        assert!(visits > 0);
        apply(&mut tree, r#"[["setText",5,"changed"],["setStyle",4,{"width":10}],["setCustomProp",4,"title","t"]]"#);
        sync(&tree);
        assert_eq!(tree.relations().visits(), visits);
    }

    #[test]
    fn the_text_of_a_label_and_of_a_referenced_element_follow_their_text() {
        let mut tree = tree_with(
            r#"[["createElement",1,"div"],["setRoot",1],
                ["createElement",2,"label"],["appendChild",1,2],
                ["createElement",3,"text"],["appendChild",2,3],["setText",3,"Name"],
                ["createElement",4,"input"],["appendChild",2,4],["setCustomProp",4,"aria-describedby","hint"],
                ["createElement",5,"div"],["appendChild",1,5],["setCustomProp",5,"id","hint"],
                ["createElement",6,"div"],["appendChild",5,6],
                ["createElement",7,"text"],["appendChild",6,7],["setText",7,"Short"]]"#,
        );
        assert_eq!(links_of(&tree).label_text(2), Some("Name"));
        assert_eq!(links_of(&tree).referenced_text(5), Some("Short"));
        apply(&mut tree, r#"[["setText",3,"Full name"],["setText",7,"Long"]]"#);
        assert_eq!(links_of(&tree).label_text(2), Some("Full name"));
        assert_eq!(links_of(&tree).referenced_text(5), Some("Long"));
    }

    #[test]
    fn retargeting_an_id_or_a_reference_moves_the_link() {
        let mut tree = tree_with(
            r#"[["createElement",1,"div"],["setRoot",1],
                ["createElement",2,"label"],["appendChild",1,2],["setCustomProp",2,"htmlFor","a"],
                ["createElement",3,"input"],["appendChild",1,3],["setCustomProp",3,"id","a"],
                ["createElement",4,"input"],["appendChild",1,4],["setCustomProp",4,"id","b"],
                ["createElement",5,"div"],["appendChild",1,5],["setCustomProp",5,"id","one"],
                ["createElement",6,"div"],["appendChild",1,6],["setCustomProp",6,"id","two"],
                ["setCustomProp",3,"aria-labelledby","one"]]"#,
        );
        assert_eq!(links_of(&tree).control_of(2), Some(3));
        assert_eq!(links_of(&tree).referenced_text(5), Some(""));
        assert_eq!(links_of(&tree).referenced_text(6), None);

        apply(&mut tree, r#"[["setCustomProp",4,"id","a"],["setCustomProp",3,"id","c"]]"#);
        assert_eq!(links_of(&tree).control_of(2), Some(4), "the control that has the id now");

        apply(&mut tree, r#"[["setCustomProp",3,"aria-labelledby","two"]]"#);
        assert_eq!(links_of(&tree).referenced_text(5), None);
        assert_eq!(links_of(&tree).referenced_text(6), Some(""));

        apply(&mut tree, r#"[["setCustomProp",3,"aria-labelledby",null],["setCustomProp",2,"htmlFor",null]]"#);
        let links = links_of(&tree);
        assert_eq!(links.referenced_text(6), None);
        assert_eq!(links.control_of(2), None, "no control inside the label either");
    }

    #[test]
    fn a_role_decides_whether_a_referenced_element_is_listed() {
        let mut tree = tree_with(
            r#"[["createElement",1,"div"],["setRoot",1],
                ["createElement",2,"input"],["appendChild",1,2],["setCustomProp",2,"aria-describedby","hint"],
                ["createElement",3,"div"],["appendChild",1,3],["setCustomProp",3,"id","hint"]]"#,
        );
        assert_eq!(links_of(&tree).referenced_text(3), Some(""));
        apply(&mut tree, r#"[["setCustomProp",3,"role","note"]]"#);
        assert_eq!(links_of(&tree).referenced_text(3), None);
    }

    #[test]
    fn of_two_elements_with_an_id_the_first_in_document_order_is_the_target() {
        let tree = tree_with(
            r#"[["createElement",1,"div"],["setRoot",1],
                ["createElement",2,"label"],["appendChild",1,2],["setCustomProp",2,"htmlFor","dup"],
                ["createElement",9,"input"],["appendChild",1,9],["setCustomProp",9,"id","dup"],
                ["createElement",5,"div"],["insertBefore",1,5,9],
                ["createElement",3,"input"],["appendChild",5,3],["setCustomProp",3,"id","dup"]]"#,
        );
        assert_eq!(links_of(&tree).control_of(2), Some(3));
    }

    #[test]
    fn labels_of_a_control_are_in_document_order_whatever_the_order_they_were_made_in() {
        let tree = tree_with(
            r#"[["createElement",1,"div"],["setRoot",1],
                ["createElement",9,"input"],["appendChild",1,9],["setCustomProp",9,"id","c"],
                ["createElement",4,"label"],["appendChild",1,4],["setCustomProp",4,"htmlFor","c"],
                ["createElement",8,"label"],["insertBefore",1,8,4],["setCustomProp",8,"htmlFor","c"],
                ["createElement",2,"div"],["insertBefore",1,2,8],
                ["createElement",3,"label"],["appendChild",2,3],["setCustomProp",3,"htmlFor","c"]]"#,
        );
        assert_eq!(
            links_of(&tree).labels_of(9),
            Some(&["gpuix-label-3".to_string(), "gpuix-label-8".to_string(), "gpuix-label-4".to_string()][..])
        );
    }

    #[test]
    fn destroying_the_last_source_clears_the_links_and_a_detached_label_has_none() {
        let mut tree = tree_with(
            r#"[["createElement",1,"div"],["setRoot",1],
                ["createElement",2,"label"],["appendChild",1,2],
                ["createElement",3,"input"],["appendChild",2,3],
                ["createElement",4,"label"],
                ["createElement",5,"input"],["appendChild",4,5]]"#,
        );
        let links = links_of(&tree);
        assert_eq!(links.control_of(2), Some(3));
        assert_eq!(links.control_of(4), None);
        apply(&mut tree, r#"[["destroyElement",2]]"#);
        assert_eq!(links_of(&tree).control_of(2), None);
        apply(&mut tree, r#"[["appendChild",1,4]]"#);
        assert_eq!(links_of(&tree).control_of(4), Some(5));
    }
}
