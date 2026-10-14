//! What the pointer is doing, kept between inputs.
//!
//! The view owns one `PointerState` (`GpuixView::pointer`). The dispatcher
//! updates it; `build_element` and the key-event code read it through
//! `is_hovered` and `is_active`.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use rustc_hash::FxHashMap;

use super::ids::Hit;
use super::path::event_path;
use crate::retained_tree::RetainedTree;

/// The only pointer there is: the mouse.
pub const MOUSE_POINTER_ID: f64 = 1.0;

/// Explicit pointer capture as JS asks for it, shared between the renderer
/// handle and the view's `PointerState`.
///
/// Calls come from JS threads while the view lives on the UI thread, so a call
/// only records the request here and the dispatcher applies it at the next
/// native pointer input, which is when `gotpointercapture` and
/// `lostpointercapture` fire. `has` reads the request, as Chrome's
/// `hasPointerCapture` reads the pending capture, so it is true right after
/// `set`.
#[derive(Clone, Default)]
pub struct PointerCapture(Arc<Mutex<Requested>>);

#[derive(Default)]
struct Requested {
    target: Option<u64>,
    /// Held buttons as of the last input. A pointer with none down has nothing
    /// to capture, so `set` ignores it, as Chrome does for a mouse.
    buttons: u32,
}

impl PointerCapture {
    pub fn set(&self, tree: &RetainedTree, id: u64, pointer_id: f64) -> Result<(), String> {
        validate(tree, id, pointer_id)?;
        let mut requested = self.0.lock().unwrap();
        if requested.buttons != 0 {
            requested.target = Some(id);
        }
        Ok(())
    }

    pub fn release(&self, tree: &RetainedTree, id: u64, pointer_id: f64) -> Result<(), String> {
        validate(tree, id, pointer_id)?;
        let mut requested = self.0.lock().unwrap();
        if requested.target == Some(id) {
            requested.target = None;
        }
        Ok(())
    }

    pub fn has(&self, tree: &RetainedTree, id: u64, pointer_id: f64) -> Result<bool, String> {
        validate(tree, id, pointer_id)?;
        Ok(self.0.lock().unwrap().target == Some(id))
    }

    pub(super) fn target(&self) -> Option<u64> {
        self.0.lock().unwrap().target
    }

    pub(super) fn clear(&self) {
        self.0.lock().unwrap().target = None;
    }

    pub(super) fn set_buttons(&self, buttons: u32) {
        self.0.lock().unwrap().buttons = buttons;
    }
}

fn validate(tree: &RetainedTree, id: u64, pointer_id: f64) -> Result<(), String> {
    if pointer_id != MOUSE_POINTER_ID {
        return Err(format!(
            "Unknown pointerId {pointer_id}: the only pointer is the mouse, pointerId {MOUSE_POINTER_ID}"
        ));
    }
    if !is_connected(tree, id) {
        return Err(format!("Element {id} is not connected to the tree"));
    }
    Ok(())
}

/// Whether `id` is in the tree and reaches its root.
pub(super) fn is_connected(tree: &RetainedTree, id: u64) -> bool {
    event_path(tree, id).last().copied() == tree.root_id
}

/// A button that is down: where it went down, so the release knows whether a
/// `click` happened and on which ancestor.
pub(super) struct Press {
    /// Event path at the press, target first.
    pub path: Vec<u64>,
}

/// The target the last pointer input produced. `None` outside: no input yet.
/// `Some(None)`: the pointer is outside every host element.
pub type ObservedTarget = Rc<Cell<Option<Option<u64>>>>;

#[derive(Default)]
pub struct PointerState {
    /// Hover target, then its retained ancestors. This is the DOM `:hover` chain.
    pub(super) hovered: Vec<u64>,
    /// The host elements under the pointer at the last input, with their boxes.
    /// Boundary events that leave an element read its box from here.
    pub(super) hits: Vec<Hit>,
    /// Primary-button press target and its ancestors: the `:active` chain.
    pub(super) active: Vec<u64>,
    /// Buttons that are down, by DOM button number.
    pub(super) pressed: FxHashMap<u32, Press>,
    /// Held buttons as the DOM `buttons` bitmask.
    pub(super) buttons: u32,
    /// An element that keeps receiving pointer events after the pointer leaves
    /// it, armed by a press on an element that listens for both `mousedown` and
    /// `mousemove`. Explicit capture outranks it.
    pub(super) captured: Option<u64>,
    /// The element holding `setPointerCapture`, as applied by the last input.
    pub(super) explicit: Option<u64>,
    pub(super) capture: PointerCapture,
    observed: ObservedTarget,
}

impl PointerState {
    /// Whether `id` is the hover target or one of its ancestors.
    pub fn is_hovered(&self, id: u64) -> bool {
        self.hovered.contains(&id)
    }

    /// Whether `id` is the primary-button press target or one of its ancestors.
    pub fn is_active(&self, id: u64) -> bool {
        self.active.contains(&id)
    }

    /// The hover target, the innermost element of the hover chain.
    pub fn hover_target(&self) -> Option<u64> {
        self.hovered.first().copied()
    }

    /// The element receiving pointer events while the pointer is held on it.
    pub fn captured(&self) -> Option<u64> {
        self.explicit.or(self.captured)
    }

    /// The element that holds explicit pointer capture, once an input applied it.
    pub fn explicit_capture(&self) -> Option<u64> {
        self.explicit
    }

    pub fn with_capture(capture: PointerCapture) -> Self {
        Self {
            capture,
            ..Default::default()
        }
    }

    /// A handle for the paint pass: the target the last input saw, so a frame
    /// whose layout moved under a still pointer can tell the dispatcher.
    pub fn observed(&self) -> ObservedTarget {
        self.observed.clone()
    }

    pub(super) fn observe(&self, target: Option<u64>) {
        self.observed.set(Some(target));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_and_active_cover_the_whole_chain() {
        let mut state = PointerState::default();
        state.hovered = vec![3, 2, 1];
        state.active = vec![2, 1];
        assert!(state.is_hovered(3) && state.is_hovered(1));
        assert!(!state.is_hovered(9));
        assert!(!state.is_active(3) && state.is_active(2));
        assert_eq!(state.hover_target(), Some(3));
    }

    fn connected_tree() -> RetainedTree {
        let mut tree = RetainedTree::new();
        for id in [1, 2, 3] {
            tree.create_element(id, "div".to_string());
        }
        tree.set_root(1);
        tree.append_child(1, 2);
        // 3 is created but never attached.
        tree
    }

    fn held(capture: &PointerCapture) {
        capture.set_buttons(1);
    }

    #[test]
    fn has_pointer_capture_follows_set_and_release() {
        let tree = connected_tree();
        let capture = PointerCapture::default();
        held(&capture);
        assert!(!capture.has(&tree, 2, 1.0).unwrap());
        capture.set(&tree, 2, 1.0).unwrap();
        assert!(capture.has(&tree, 2, 1.0).unwrap());
        assert!(!capture.has(&tree, 1, 1.0).unwrap());
        capture.release(&tree, 1, 1.0).unwrap();
        assert!(
            capture.has(&tree, 2, 1.0).unwrap(),
            "only the holder can release"
        );
        capture.release(&tree, 2, 1.0).unwrap();
        assert!(!capture.has(&tree, 2, 1.0).unwrap());
    }

    #[test]
    fn setting_capture_replaces_the_previous_holder() {
        let tree = connected_tree();
        let capture = PointerCapture::default();
        held(&capture);
        capture.set(&tree, 2, 1.0).unwrap();
        capture.set(&tree, 1, 1.0).unwrap();
        assert!(!capture.has(&tree, 2, 1.0).unwrap());
        assert!(capture.has(&tree, 1, 1.0).unwrap());
    }

    #[test]
    fn a_pointer_with_no_button_down_captures_nothing() {
        let tree = connected_tree();
        let capture = PointerCapture::default();
        capture.set(&tree, 2, 1.0).unwrap();
        assert!(!capture.has(&tree, 2, 1.0).unwrap());
    }

    #[test]
    fn an_unknown_pointer_id_throws() {
        let tree = connected_tree();
        let capture = PointerCapture::default();
        held(&capture);
        for pointer_id in [0.0, 2.0, -1.0, f64::NAN] {
            assert!(capture
                .set(&tree, 2, pointer_id)
                .unwrap_err()
                .contains("pointerId"));
            assert!(capture.release(&tree, 2, pointer_id).is_err());
            assert!(capture.has(&tree, 2, pointer_id).is_err());
        }
    }

    #[test]
    fn an_element_that_is_not_connected_throws() {
        let tree = connected_tree();
        let capture = PointerCapture::default();
        held(&capture);
        for id in [3, 99] {
            assert!(capture
                .set(&tree, id, 1.0)
                .unwrap_err()
                .contains("not connected"));
            assert!(capture.release(&tree, id, 1.0).is_err());
            assert!(capture.has(&tree, id, 1.0).is_err());
        }
    }

    #[test]
    fn nothing_is_observed_before_the_first_input() {
        let state = PointerState::default();
        assert_eq!(state.observed().get(), None);
        state.observe(None);
        assert_eq!(state.observed().get(), Some(None));
    }
}
