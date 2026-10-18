//! `getRetainDebug`: what the last recorded frame built, and why, for finding the next
//! exclusion worth lifting. It is read by `testing.ts` as JSON and by nothing else.
//!
//! It runs only with `GPUIX_RETAIN_DEBUG` set; without it every hook is a test of one flag.
//! Pass 1 of a draw is the frame: a later pass overwrites nothing but the reasons an element
//! was kept, which show what that pass had to build again.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use super::*;

/// How many elements of a kind a report names, largest first.
const TOP: usize = 100;

#[derive(Default)]
pub(super) struct Debug {
    on: bool,
    recorded: bool,
    /// What named an element as changed before it was built, by kind of source.
    sources: Vec<(&'static str, u64)>,
    /// Elements that could have been skipped and were built, and the first reason.
    kept: Vec<(&'static str, u64, u32)>,
    /// The subtrees gpui asked for after the frame was built, and how many elements each held.
    deferred: Vec<(u64, u32)>,
    nodes: Vec<DebugNode>,
}

struct DebugNode {
    id: u64,
    parent: u32,
    size: u32,
    skipped: bool,
    retained: bool,
    slot: usize,
    cause: Option<Cause>,
    spine: bool,
    dynamic: u8,
}

impl Debug {
    pub(super) fn from_env() -> Self {
        Self { on: std::env::var_os("GPUIX_RETAIN_DEBUG").is_some(), ..Self::default() }
    }

    pub(super) fn begin_frame(&mut self, pass: usize) {
        if self.on && pass == 1 {
            self.sources.clear();
            self.kept.clear();
            self.deferred.clear();
        }
    }

    pub(super) fn sources(&mut self, kind: &'static str, ids: impl IntoIterator<Item = u64>) {
        if self.on {
            self.sources.extend(ids.into_iter().map(|id| (kind, id)));
        }
    }

    pub(super) fn deferred(&mut self, root: Option<&Node>, nodes: usize) {
        if let (true, Some(root)) = (self.on, root) {
            self.deferred.push((root.id, nodes as u32));
        }
    }
}

impl Retain {
    /// Notes that `id` had a record that said it could be skipped, and was built.
    pub(super) fn debug_kept(&mut self, kept: Kept, id: u64) {
        if !self.debug.on || self.deferred {
            return;
        }
        if let Some(record) = self.records.get(&id).filter(|record| record.eligible && record.size >= self.min_root) {
            self.debug.kept.push((kept.name(), id, record.size));
        }
    }

    /// Keeps what the frame that was just judged decided about each element.
    pub(super) fn debug_capture(&mut self, slots: &[usize]) {
        if !self.debug.on || self.pass != 1 {
            return;
        }
        self.debug.recorded = true;
        self.debug.nodes.clear();
        for (at, node) in self.nodes.iter().enumerate() {
            let below = self.below[at];
            self.debug.nodes.push(DebugNode {
                id: node.id,
                parent: node.parent,
                size: below.size,
                skipped: node.skipped.is_some(),
                retained: self.retained[at],
                slot: slots[at],
                cause: below.changed,
                spine: below.changed_below,
                dynamic: node.vol.dynamic,
            });
        }
    }

    #[cfg(test)]
    pub(super) fn set_debug(&mut self, on: bool) {
        self.debug.on = on;
    }

    pub(super) fn debug_wants_slots(&self) -> bool {
        self.debug.on && self.pass == 1
    }

    /// The last recorded frame as JSON; elements are named by their type, `testId` and nearest
    /// ancestors, the way a person looks for them on the page.
    pub(crate) fn debug_report(&self, tree: &crate::retained_tree::RetainedTree) -> String {
        let label = |id: u64| element_label(tree, id);
        let nodes = &self.debug.nodes;
        let depth_of = |index: usize| -> u32 {
            let mut depth = 0;
            let mut at = nodes[index].parent;
            while at != NO_PARENT {
                depth += 1;
                at = nodes[at as usize].parent;
            }
            depth
        };
        let top = |mut list: Vec<(u32, u64, u32)>, count: usize| -> Vec<Value> {
            list.sort_by(|left, right| right.0.cmp(&left.0));
            list.into_iter()
                .take(count)
                .map(|(size, id, depth)| json!({"label": label(id), "size": size, "depth": depth, "id": id}))
                .collect()
        };

        let mut by_slot: BTreeMap<String, (u32, Vec<(u32, u64, u32)>)> = BTreeMap::new();
        let mut skipped_roots = Vec::new();
        let mut retained_built_roots = Vec::new();
        for (index, node) in nodes.iter().enumerate() {
            if node.skipped {
                skipped_roots.push((node.size, node.id, 0));
            } else if !node.retained {
                let entry = by_slot.entry(slot_name(node.slot)).or_default();
                entry.0 += 1;
                entry.1.push((node.size, node.id, depth_of(index)));
            } else if node.parent == NO_PARENT || !nodes[node.parent as usize].retained {
                retained_built_roots.push((node.size, node.id, 0));
            }
        }
        let slots: Map<String, Value> = by_slot
            .into_iter()
            .map(|(slot, (count, list))| (slot, json!({"nodes": count, "top": top(list, TOP)})))
            .collect();

        let mut kept: BTreeMap<&str, (u32, u32, Vec<(u32, u64, u32)>)> = BTreeMap::new();
        for (reason, id, size) in &self.debug.kept {
            let entry = kept.entry(reason).or_default();
            entry.0 += 1;
            entry.1 += size;
            entry.2.push((*size, *id, 0));
        }
        let kept: Map<String, Value> = kept
            .into_iter()
            .map(|(reason, (roots, elements, list))| {
                (reason.to_owned(), json!({"roots": roots, "elements": elements, "top": top(list, 6)}))
            })
            .collect();

        let present: std::collections::HashSet<u64> = nodes.iter().map(|node| node.id).collect();
        let mut sources: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
        for (kind, id) in &self.debug.sources {
            sources.entry(kind).or_default().push(*id);
        }
        let sources: Map<String, Value> = sources
            .into_iter()
            .map(|(kind, ids)| {
                let absent = ids.iter().filter(|id| !present.contains(id)).count();
                let labels = std::iter::once(json!(format!("not built this frame: {absent}"))).chain(ids.iter().take(24).map(|id| {
                    json!(format!("{}{}", label(*id), if present.contains(id) { "" } else { " (NOT BUILT)" }))
                }));
                (kind.to_owned(), json!({"count": ids.len(), "labels": labels.collect::<Vec<_>>()}))
            })
            .collect();

        let mut causes: BTreeMap<String, (u32, Vec<(u32, u64, u32)>)> = BTreeMap::new();
        let mut dynamics: Vec<(u32, u64, u32)> = Vec::new();
        for (index, node) in nodes.iter().enumerate().filter(|(_, node)| !node.skipped) {
            if let Some(cause) = node.cause {
                let entry = causes.entry(format!("{} [{}]", cause.name(), slot_name(node.slot))).or_default();
                entry.0 += 1;
                entry.1.push((node.size, node.id, depth_of(index)));
            }
            if node.dynamic != 0 {
                dynamics.push((node.size, node.id, u32::from(node.dynamic)));
            }
        }
        let causes: Map<String, Value> = causes
            .into_iter()
            .map(|(cause, (count, list))| (cause, json!({"nodes": count, "top": top(list, 8)})))
            .collect();
        dynamics.sort_by(|left, right| right.0.cmp(&left.0));
        let dynamics: Vec<Value> = dynamics
            .into_iter()
            .take(40)
            .map(|(size, id, bits)| json!({"label": label(id), "size": size, "bits": bits}))
            .collect();

        let named = |list: &[(u32, u64, u32)]| -> Vec<Value> {
            let mut list = list.to_vec();
            list.sort_by(|left, right| right.0.cmp(&left.0));
            list.into_iter().take(8).map(|(size, id, _)| json!({"label": label(id), "size": size})).collect()
        };
        json!({
            "recorded": self.debug.recorded,
            "nodes": nodes.len(),
            "builtNodes": nodes.iter().filter(|node| !node.skipped).count(),
            "spineNodes": nodes.iter().filter(|node| node.spine && !node.skipped).count(),
            "skippedRoots": skipped_roots.len(),
            "skippedElements": skipped_roots.iter().map(|(size, ..)| u64::from(*size)).sum::<u64>(),
            "slots": slots,
            "causes": causes,
            "dynamicNodes": dynamics,
            "sources": sources,
            "notSkippedWhy": kept,
            "skippedTop": named(&skipped_roots),
            "retainableButBuiltRoots": {
                "count": retained_built_roots.len(),
                "elements": retained_built_roots.iter().map(|(size, ..)| u64::from(*size)).sum::<u64>(),
                "top": named(&retained_built_roots),
            },
            "deferred": {
                "count": self.debug.deferred.len(),
                "top": self.debug.deferred.iter().take(10).map(|(id, size)| json!({"label": label(*id), "nodes": size})).collect::<Vec<_>>(),
            },
        })
        .to_string()
    }
}

/// An element as `type#testId[position][ov:overflow]{props}"text"`, then its parents the same
/// way without the props, and the `testId` of the nearest ancestor that has one.
fn element_label(tree: &crate::retained_tree::RetainedTree, id: u64) -> String {
    let mut parts = Vec::new();
    let mut at = Some(id);
    while let Some(current) = at.filter(|_| parts.len() < 5) {
        let Some(element) = tree.elements.get(&current) else { break };
        let mut part = element.element_type.clone();
        if let Some(test_id) = &element.test_id {
            part.push('#');
            part.push_str(test_id);
        }
        if let Some(style) = element.style.as_deref() {
            if let Some(position) = &style.position {
                part.push_str(&format!("[{position}]"));
            }
            if let Some(overflow) = style.overflow.as_ref().or(style.overflow_x.as_ref()).or(style.overflow_y.as_ref()) {
                part.push_str(&format!("[ov:{overflow}]"));
            }
        }
        if parts.is_empty() {
            let mut keys: Vec<&str> = element
                .custom_props()
                .keys()
                .map(String::as_str)
                .filter(|key| !matches!(*key, "testId" | "tabIndex" | "role" | "aria-label" | "id"))
                .collect();
            keys.sort_unstable();
            keys.truncate(6);
            if !keys.is_empty() {
                part.push_str(&format!("{{{}}}", keys.join(",")));
            }
            if let Some(content) = &element.content {
                part.push_str(&format!("\"{}\"", content.chars().take(20).collect::<String>()));
            }
        }
        parts.push(part);
        at = element.parent;
    }
    let mut owner = None;
    let mut up = tree.elements.get(&id).and_then(|element| element.parent);
    while let Some(current) = up {
        let Some(element) = tree.elements.get(&current) else { break };
        if let Some(test_id) = &element.test_id {
            owner = Some(test_id.clone());
            break;
        }
        up = element.parent;
    }
    match owner {
        Some(owner) => format!("{} @{owner}", parts.join(" < ")),
        None => parts.join(" < "),
    }
}
