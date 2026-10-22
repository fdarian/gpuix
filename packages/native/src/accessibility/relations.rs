//! The elements `links::compute` starts from, kept as the tree changes so that computing the
//! links never walks the elements nothing relates: a tree without a `<label>` or an `aria-*` id
//! reference costs nothing, and one with a few costs them and what they read.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use crate::retained_tree::RetainedElement;

/// The props `compute` reads of an element, apart from the relation props of `aria::RELATION_PROPS`.
const HTML_FOR: &str = "htmlFor";

#[derive(Default)]
pub struct RelationRegistry {
    labels: HashSet<u64>,
    /// Elements with an `aria-*` prop that names other elements.
    referrers: HashSet<u64>,
    /// How many props name each id, `htmlFor` included.
    named: HashMap<String, usize>,
    /// Every element with a DOM `id`, by it: resolving a reference never walks the tree.
    by_dom_id: HashMap<String, Vec<u64>>,
    /// Moves when something `compute` reads may have changed, and only while there are sources.
    revision: u64,
    visits: Cell<u64>,
}

fn dom_id(element: &RetainedElement) -> Option<&str> {
    element
        .custom_props()
        .get("id")
        .and_then(|value| value.as_str())
        .filter(|id| !id.is_empty())
}

fn ids_named(key: &str, value: Option<&serde_json::Value>) -> Vec<String> {
    let Some(text) = value.and_then(|value| value.as_str()) else {
        return Vec::new();
    };
    if key == HTML_FOR {
        return vec![text.to_string()];
    }
    text.split_whitespace().map(str::to_string).collect()
}

fn is_relation_prop(key: &str) -> bool {
    key == HTML_FOR
        || super::aria::RELATION_PROPS
            .iter()
            .any(|(relation, _)| *relation == key)
}

impl RelationRegistry {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn has_sources(&self) -> bool {
        !self.labels.is_empty() || !self.referrers.is_empty()
    }

    pub fn labels(&self) -> impl Iterator<Item = u64> + '_ {
        self.labels.iter().copied()
    }

    pub fn referrers(&self) -> impl Iterator<Item = u64> + '_ {
        self.referrers.iter().copied()
    }

    /// Every element with this DOM `id`, in no order.
    pub fn with_dom_id(&self, id: &str) -> &[u64] {
        self.by_dom_id.get(id).map(Vec::as_slice).unwrap_or_default()
    }

    /// Elements `compute` has looked at since the tree was made.
    pub fn visits(&self) -> u64 {
        self.visits.get()
    }

    pub fn visit(&self, elements: usize) {
        self.visits.set(self.visits.get() + elements as u64);
    }

    fn changed(&mut self) {
        self.revision += 1;
    }

    /// Mark a change that matters only while there is something to relate.
    fn changed_if_active(&mut self) {
        if self.has_sources() {
            self.changed();
        }
    }

    fn name(&mut self, ids: Vec<String>) {
        for id in ids {
            *self.named.entry(id).or_default() += 1;
        }
    }

    fn unname(&mut self, ids: Vec<String>) {
        for id in ids {
            if let Some(count) = self.named.get_mut(&id) {
                *count -= 1;
                if *count == 0 {
                    self.named.remove(&id);
                }
            }
        }
    }

    pub fn created(&mut self, id: u64, element_type: &str) {
        if element_type == "label" {
            self.labels.insert(id);
            self.changed();
        }
    }

    pub fn destroyed(&mut self, element: &RetainedElement) {
        let was_active = self.has_sources();
        if element.element_type == "label" {
            self.labels.remove(&element.id);
        }
        self.referrers.remove(&element.id);
        for (key, value) in element.custom_props() {
            if is_relation_prop(key) {
                self.unname(ids_named(key, Some(value)));
            }
        }
        if let Some(id) = dom_id(element) {
            self.unindex(id, element.id);
        }
        if was_active {
            self.changed();
        }
    }

    fn unindex(&mut self, dom_id: &str, element: u64) {
        if let Some(ids) = self.by_dom_id.get_mut(dom_id) {
            ids.retain(|id| *id != element);
            if ids.is_empty() {
                self.by_dom_id.remove(dom_id);
            }
        }
    }

    /// The element's structure under its parents changed: a label's control, a reference's target
    /// and what is reachable from the root can all follow.
    pub fn restructured(&mut self) {
        self.changed_if_active();
    }

    /// Whether a prop is one `compute` reads, so a change to it calls [`Self::prop_changed`].
    pub fn reads(key: &str) -> bool {
        key == "id" || key == "role" || is_relation_prop(key)
    }

    /// `previous` and `now` are the values of `key` on `element` around the change, `element` being
    /// the state after it.
    pub fn prop_changed(
        &mut self,
        element: &RetainedElement,
        key: &str,
        previous: Option<&serde_json::Value>,
        now: Option<&serde_json::Value>,
    ) {
        if is_relation_prop(key) {
            self.unname(ids_named(key, previous));
            self.name(ids_named(key, now));
            let names_any = element.custom_props().iter().any(|(key, value)| {
                is_relation_prop(key) && key != HTML_FOR && !ids_named(key, Some(value)).is_empty()
            });
            if key != HTML_FOR {
                if names_any {
                    self.referrers.insert(element.id);
                } else {
                    self.referrers.remove(&element.id);
                }
            }
            self.changed();
            return;
        }
        if key == "id" {
            let before = previous
                .and_then(|value| value.as_str())
                .filter(|id| !id.is_empty());
            let after = now.and_then(|value| value.as_str()).filter(|id| !id.is_empty());
            let touched = element.element_type == "label"
                || before.is_some_and(|id| self.named.contains_key(id))
                || after.is_some_and(|id| self.named.contains_key(id));
            if let Some(id) = before {
                self.unindex(id, element.id);
            }
            if let Some(id) = after {
                self.by_dom_id.entry(id.to_string()).or_default().push(element.id);
            }
            if touched {
                self.changed_if_active();
            }
            return;
        }
        // `role`: whether a referenced element needs a node of its own.
        if dom_id(element).is_some_and(|id| self.named.contains_key(id)) {
            self.changed_if_active();
        }
    }

    /// The text of `element` changed, or something under it did, which reaches a label's text and
    /// the text of a referenced element.
    pub fn text_changed(&mut self, tree_elements: &crate::retained_tree::ElementMap, element: u64) {
        if !self.has_sources() {
            return;
        }
        let mut current = Some(element);
        while let Some(id) = current {
            let Some(element) = tree_elements.get(&id) else {
                return;
            };
            if element.element_type == "label" || dom_id(element).is_some_and(|id| self.named.contains_key(id)) {
                self.changed();
                return;
            }
            current = element.parent;
        }
    }
}
