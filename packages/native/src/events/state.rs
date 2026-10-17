//! What the pointer is doing, kept between inputs.
//!
//! The view owns one `PointerState` (`GpuixView::pointer`). The dispatcher
//! updates it; `build_element` and the key-event code read it through
//! `is_hovered` and `is_active`.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use rustc_hash::FxHashMap;

use super::element_drag::{ElementDrag, PendingDrag};
use super::ids::Hit;
use super::path::event_path;
use crate::retained_tree::RetainedTree;

/// The only pointer there is: the mouse.
pub const MOUSE_POINTER_ID: f64 = 1.0;

/// What JS asks of the UI thread, shared between the renderer handle and the
/// view's `PointerState`: explicit pointer capture, ending an element drag
/// (`cancelDrag`) and pointer lock.
///
/// Calls come from JS threads while the view lives on the UI thread, so a call
/// only records the request here and the UI thread applies it later. Capture
/// applies at the next native pointer input, which is when `gotpointercapture`
/// and `lostpointercapture` fire; `has` reads the request, as Chrome's
/// `hasPointerCapture` reads the pending capture, so it is true right after
/// `set`. A pointer lock request is applied by `pointer_lock::apply`, which the
/// renderer runs in the call on macOS and at the next frame elsewhere;
/// `locked_element` reads what it granted, so it is null until the lock is.
#[derive(Clone, Default)]
pub struct PointerRequests(Arc<Mutex<Requested>>);

#[derive(Default)]
struct Requested {
    target: Option<u64>,
    /// Held buttons as of the last input. A pointer with none down has nothing
    /// to capture, so `set` ignores it, as Chrome does for a mouse.
    buttons: u32,
    cancel_drag: bool,
    lock: Option<LockRequest>,
    locked: Option<u64>,
    /// The event types `window` or `document` has a listener for, in either phase.
    window_events: HashSet<String>,
}

/// The last pointer lock call. Only the last one counts: a request followed by
/// an exit before the next frame is no request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LockRequest {
    Enter(u64),
    Exit,
}

impl PointerRequests {
    /// `setWindowEventListeners`: the types JS listens for on `window` or
    /// `document`. A DOM listener up there hears an event whatever the path
    /// below it listens for, so the dispatcher produces these types even for an
    /// unlistened path. Replaces the previous set.
    pub fn set_window_events(&self, types: Vec<String>) {
        self.0.lock().unwrap().window_events = types.into_iter().collect();
    }

    pub(super) fn window_listens(&self, event_type: &str) -> bool {
        self.0.lock().unwrap().window_events.contains(event_type)
    }

    /// Ends the element drag in progress without another event: `dragstart`'s
    /// `preventDefault()` is a drag that never was.
    pub fn cancel_drag(&self) {
        self.0.lock().unwrap().cancel_drag = true;
    }

    pub(super) fn take_drag_cancel(&self) -> bool {
        std::mem::take(&mut self.0.lock().unwrap().cancel_drag)
    }

    /// Asks for the pointer lock on `id`. An element that is not connected is
    /// not an error here: the frame that applies the request fires
    /// `pointerlockerror`, as the DOM does.
    pub fn request_lock(&self, id: u64) {
        self.0.lock().unwrap().lock = Some(LockRequest::Enter(id));
    }

    pub fn exit_lock(&self) {
        self.0.lock().unwrap().lock = Some(LockRequest::Exit);
    }

    /// The element that holds the pointer lock, once a frame granted it.
    pub fn locked_element(&self) -> Option<u64> {
        self.0.lock().unwrap().locked
    }

    pub(super) fn take_lock_request(&self) -> Option<LockRequest> {
        self.0.lock().unwrap().lock.take()
    }

    pub(super) fn publish_locked(&self, element: Option<u64>) {
        self.0.lock().unwrap().locked = element;
    }

    /// Throws for an element that is not connected, as the DOM's `InvalidStateError`.
    pub fn set(&self, tree: &RetainedTree, id: u64, pointer_id: f64) -> Result<(), String> {
        validate_pointer_id(pointer_id)?;
        if !is_connected(tree, id) {
            return Err(format!("Element {id} is not connected to the tree"));
        }
        let mut requested = self.0.lock().unwrap();
        if requested.buttons != 0 {
            requested.target = Some(id);
        }
        Ok(())
    }

    /// A no-op for an element that is not connected, as in the DOM.
    pub fn release(&self, id: u64, pointer_id: f64) -> Result<(), String> {
        validate_pointer_id(pointer_id)?;
        let mut requested = self.0.lock().unwrap();
        if requested.target == Some(id) {
            requested.target = None;
        }
        Ok(())
    }

    /// False for an element that is not connected, as in the DOM.
    pub fn has(&self, tree: &RetainedTree, id: u64, pointer_id: f64) -> Result<bool, String> {
        validate_pointer_id(pointer_id)?;
        Ok(is_connected(tree, id) && self.0.lock().unwrap().target == Some(id))
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

fn validate_pointer_id(pointer_id: f64) -> Result<(), String> {
    if pointer_id != MOUSE_POINTER_ID {
        return Err(format!(
            "Unknown pointerId {pointer_id}: the only pointer is the mouse, pointerId {MOUSE_POINTER_ID}"
        ));
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

/// Where a drag is: the element `dragenter` last went to. Files from the OS
/// and a dragged element share it, and either replaces the pointer for its
/// duration: the dispatcher emits drag events and no pointer or mouse events.
pub(super) struct DragOver {
    pub target: u64,
}

/// The element that holds the pointer lock. While it does, it is the target of
/// every pointer and mouse event, and the pointer reports one position.
pub(super) struct PointerLock {
    pub element: u64,
    /// Where the pointer was when the lock began, and where every event says it
    /// is until the lock ends. `None` until an input shows where that is.
    pub position: Option<(f64, f64)>,
}

/// The host elements under the pointer at the last pointer input, target first.
/// `None`: no input yet. `Some` of nothing: the pointer is outside every host
/// element.
pub type ObservedChain = Rc<RefCell<Option<Vec<u64>>>>;

/// Whether `hits`, the chain painted now, differs from what the last input saw.
/// The whole chain counts, not only its target: hover follows every element of
/// it, so an ancestor that stopped covering the pointer changes hover even when
/// the innermost element did not change.
pub fn chain_changed(observed: &ObservedChain, hits: &[Hit]) -> bool {
    observed.borrow().as_ref().is_some_and(|seen| {
        seen.len() != hits.len() || seen.iter().zip(hits).any(|(seen, hit)| *seen != hit.id)
    })
}

/// Whether `id` is in `chain` (target first) but not its target.
pub(super) fn strictly_in(chain: &[u64], id: u64) -> bool {
    chain.iter().skip(1).any(|ancestor| *ancestor == id)
}

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
    /// Set when the `pointerdown` that began the contact was cancelled: until the
    /// contact ends the mouse's compatibility events (`mousedown`, `mousemove`,
    /// `mouseup`) are not fired. It belongs to the pointer, not to a button, so a
    /// button that joins the first one is covered too.
    pub(super) mouse_events_suppressed: bool,
    /// Buttons whose press went to the window (a drag region) and not to the page, as the
    /// `buttons` bitmask. The release is the window's too, and until it comes the pointer is
    /// the window's: a move is not the page's.
    pub(super) window_presses: u32,
    /// The element holding `setPointerCapture`, as applied by the last input.
    pub(super) explicit: Option<u64>,
    pub(super) requests: PointerRequests,
    pub(super) lock: Option<PointerLock>,
    /// Where the last input put the pointer, to freeze it there on a lock.
    pub(super) last_position: Option<(f64, f64)>,
    pub(super) file_drag: Option<DragOver>,
    /// The OS drag in progress carries no readable file, so it has no session.
    /// Marked once so the paths are not stat'ed and logged again on every move.
    pub(super) file_drag_unreadable: bool,
    /// A press on a `draggable` element that has not moved far enough to drag it.
    pub(super) pending_drag: Option<PendingDrag>,
    pub(super) element_drag: Option<ElementDrag>,
    observed: ObservedChain,
}

impl PointerState {
    /// Whether an element is being dragged: `dragstart` has fired and `dragend` has not.
    pub fn element_drag_active(&self) -> bool {
        self.element_drag.is_some()
    }

    /// Whether `id` is the hover target or one of its ancestors.
    pub fn is_hovered(&self, id: u64) -> bool {
        self.hovered.contains(&id)
    }

    /// Whether `id` is the primary-button press target or one of its ancestors.
    pub fn is_active(&self, id: u64) -> bool {
        self.active.contains(&id)
    }

    /// Whether a strict descendant of `id` is under the pointer: `id` is in the
    /// hover chain but is not its target. `:has(:hover)`.
    pub fn has_hovered(&self, id: u64) -> bool {
        strictly_in(&self.hovered, id)
    }

    /// Whether a strict descendant of `id` is pressed. `:has(:active)`.
    pub fn has_active(&self, id: u64) -> bool {
        strictly_in(&self.active, id)
    }

    /// The hover target, the innermost element of the hover chain.
    pub fn hover_target(&self) -> Option<u64> {
        self.hovered.first().copied()
    }

    /// Whether the pointer's cancelled `pointerdown` keeps `event_type` from firing.
    /// Hover events (`mouseover`, `mouseenter`, ...) are never held back, nor are
    /// `click`, `dblclick`, `auxclick` and `contextmenu`.
    pub(super) fn suppresses(&self, event_type: &str) -> bool {
        self.mouse_events_suppressed
            && matches!(event_type, "mousedown" | "mousemove" | "mouseup")
    }

    /// Called with the outcome of the `pointerdown` the press delivered, which
    /// only the code that delivers it knows.
    pub(crate) fn suppress_mouse_events(&mut self) {
        self.mouse_events_suppressed = true;
    }

    /// The element that holds explicit pointer capture, once an input applied it.
    pub fn explicit_capture(&self) -> Option<u64> {
        self.explicit
    }

    /// The element that holds the pointer lock, once a frame applied it.
    pub fn lock_element(&self) -> Option<u64> {
        self.lock.as_ref().map(|lock| lock.element)
    }

    /// The pointer position events report for an input at `position`: the
    /// frozen one while locked, `position` itself otherwise.
    pub(super) fn position_for(&mut self, position: (f64, f64)) -> (f64, f64) {
        match &mut self.lock {
            Some(lock) => *lock.position.get_or_insert(position),
            None => {
                self.last_position = Some(position);
                position
            }
        }
    }

    /// Locks to `element`, keeping the frozen position when the lock only moves
    /// to another element.
    pub(super) fn begin_lock(&mut self, element: u64) {
        let position = self
            .lock
            .as_ref()
            .and_then(|lock| lock.position)
            .or(self.last_position);
        self.lock = Some(PointerLock { element, position });
        self.requests.publish_locked(Some(element));
    }

    pub(super) fn end_lock(&mut self) {
        self.lock = None;
        self.requests.publish_locked(None);
    }

    pub fn with_requests(requests: PointerRequests) -> Self {
        Self {
            requests,
            ..Default::default()
        }
    }

    /// A handle for the paint pass: the chain the last input saw, so a frame
    /// whose layout moved under a still pointer can tell the dispatcher.
    pub fn observed(&self) -> ObservedChain {
        self.observed.clone()
    }

    /// The path the dispatcher gave the press of `button`, target first. `None`
    /// when that press had no target or the button is up.
    pub fn press_path(&self, button: u32) -> Option<&[u64]> {
        self.pressed.get(&button).map(|press| press.path.as_slice())
    }

    pub(super) fn observe(&self, hits: &[Hit]) {
        *self.observed.borrow_mut() = Some(hits.iter().map(|hit| hit.id).collect());
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

    fn held(capture: &PointerRequests) {
        capture.set_buttons(1);
    }

    #[test]
    fn has_pointer_capture_follows_set_and_release() {
        let tree = connected_tree();
        let capture = PointerRequests::default();
        held(&capture);
        assert!(!capture.has(&tree, 2, 1.0).unwrap());
        capture.set(&tree, 2, 1.0).unwrap();
        assert!(capture.has(&tree, 2, 1.0).unwrap());
        assert!(!capture.has(&tree, 1, 1.0).unwrap());
        capture.release(1, 1.0).unwrap();
        assert!(
            capture.has(&tree, 2, 1.0).unwrap(),
            "only the holder can release"
        );
        capture.release(2, 1.0).unwrap();
        assert!(!capture.has(&tree, 2, 1.0).unwrap());
    }

    #[test]
    fn setting_capture_replaces_the_previous_holder() {
        let tree = connected_tree();
        let capture = PointerRequests::default();
        held(&capture);
        capture.set(&tree, 2, 1.0).unwrap();
        capture.set(&tree, 1, 1.0).unwrap();
        assert!(!capture.has(&tree, 2, 1.0).unwrap());
        assert!(capture.has(&tree, 1, 1.0).unwrap());
    }

    #[test]
    fn a_pointer_with_no_button_down_captures_nothing() {
        let tree = connected_tree();
        let capture = PointerRequests::default();
        capture.set(&tree, 2, 1.0).unwrap();
        assert!(!capture.has(&tree, 2, 1.0).unwrap());
    }

    #[test]
    fn an_unknown_pointer_id_throws() {
        let tree = connected_tree();
        let capture = PointerRequests::default();
        held(&capture);
        for pointer_id in [0.0, 2.0, -1.0, f64::NAN] {
            assert!(capture
                .set(&tree, 2, pointer_id)
                .unwrap_err()
                .contains("pointerId"));
            assert!(capture.release(2, pointer_id).is_err());
            assert!(capture.has(&tree, 2, pointer_id).is_err());
        }
    }

    #[test]
    fn only_set_throws_for_an_element_that_is_not_connected() {
        let tree = connected_tree();
        let capture = PointerRequests::default();
        held(&capture);
        for id in [3, 99] {
            assert!(capture
                .set(&tree, id, 1.0)
                .unwrap_err()
                .contains("not connected"));
            capture.release(id, 1.0).unwrap();
            assert!(!capture.has(&tree, id, 1.0).unwrap());
        }
    }

    #[test]
    fn has_is_false_for_a_holder_that_left_the_tree() {
        let mut tree = connected_tree();
        let capture = PointerRequests::default();
        held(&capture);
        capture.set(&tree, 2, 1.0).unwrap();
        tree.destroy_element(2);
        assert!(!capture.has(&tree, 2, 1.0).unwrap());
    }

    fn hit(id: u64) -> Hit {
        use gpui::{point, px, size, Bounds};
        Hit {
            id,
            bounds: Bounds::new(point(px(0.0), px(0.0)), size(px(10.0), px(10.0))),
        }
    }

    #[test]
    fn nothing_is_observed_before_the_first_input() {
        let state = PointerState::default();
        assert!(state.observed().borrow().is_none());
        assert!(!chain_changed(&state.observed(), &[hit(3)]));
        state.observe(&[]);
        assert_eq!(*state.observed().borrow(), Some(Vec::new()));
    }

    #[test]
    fn a_changed_ancestor_under_the_same_target_is_a_change() {
        let state = PointerState::default();
        state.observe(&[hit(3), hit(2), hit(1)]);
        let observed = state.observed();
        assert!(!chain_changed(&observed, &[hit(3), hit(2), hit(1)]));
        assert!(chain_changed(&observed, &[hit(3), hit(1)]));
        assert!(chain_changed(&observed, &[hit(3), hit(4), hit(1)]));
        assert!(chain_changed(&observed, &[hit(3), hit(2), hit(1), hit(0)]));
        assert!(chain_changed(&observed, &[]));
    }
}
