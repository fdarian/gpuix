//! Maps GPUI's hit chain onto retained element ids.
//!
//! Every host element builds its GPUI root under `host_element_id`, a variant no
//! other element in this crate uses, so a hitbox's identity says unambiguously
//! which retained element it belongs to. Hitboxes under any other id (list
//! rows' internals, a code block's scroll body, a text editor's inner view) are
//! GPUI details and never targets.

use gpui::{Bounds, ElementId, InteractiveElement, Pixels, SharedString, Window};

const NAMESPACE: &str = "gpuix";

pub fn host_element_id(id: u64) -> ElementId {
    ElementId::NamedInteger(SharedString::new_static(NAMESPACE), id)
}

/// Makes a host element's GPUI root a target of pointer hit testing: it inserts a
/// hitbox even with no listener, and `pointer_events_none` makes it only a link
/// in the chain of a descendant that is hit.
pub fn host_hitbox<E: InteractiveElement>(el: E, pointer_events_none: bool) -> E {
    let el = el.always_hitbox();
    if pointer_events_none {
        el.pointer_events_none()
    } else {
        el
    }
}

pub fn host_id_of(element_id: &ElementId) -> Option<u64> {
    match element_id {
        ElementId::NamedInteger(name, id) if name.as_ref() == NAMESPACE => Some(*id),
        _ => None,
    }
}

/// A host element under the pointer and its border box in its own space: the window's outside a
/// transform, and the space the element's transform and the ones around it map to the window
/// otherwise (`Hitbox::bounds`). The pointer's offset in the element is measured from it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub id: u64,
    pub bounds: Bounds<Pixels>,
}

/// The host elements under the pointer, target first.
///
/// GPUI reports the topmost hitbox and the hitboxes that enclosed it when it was
/// inserted. An element without an id of its own reports its nearest ancestor's,
/// so the same host id can appear twice in a row. Those collapse into one, and the
/// outermost hitbox of the run, the host element's own, supplies the bounds.
pub fn host_hit_chain(window: &Window) -> Vec<Hit> {
    let mut chain: Vec<Hit> = Vec::new();
    for entry in window.hit_chain_entries() {
        let Some(id) = entry.id.last().and_then(host_id_of) else {
            continue;
        };
        let bounds = entry.bounds;
        match chain.last_mut() {
            Some(last) if last.id == id => last.bounds = bounds,
            _ => chain.push(Hit { id, bounds }),
        }
    }
    chain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_ids_round_trip() {
        assert_eq!(host_id_of(&host_element_id(42)), Some(42));
    }

    #[test]
    fn other_ids_are_not_host_ids() {
        assert_eq!(host_id_of(&ElementId::Integer(42)), None);
        assert_eq!(host_id_of(&ElementId::Name("__gpuix_img_42".into())), None);
        assert_eq!(
            host_id_of(&ElementId::NamedInteger("other".into(), 42)),
            None
        );
    }
}
