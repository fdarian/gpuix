//! Pointer lock, shaped like the DOM's.
//!
//! JS asks through `PointerRequests` from any thread. A frame applies the ask
//! (`apply`), because GPUI's lock is a window call and the window belongs to
//! the UI thread. What comes out are the document-level events the DOM fires:
//! `pointerlockchange` when the lock element changes, `pointerlockerror` when
//! a request fails. A lock the platform drops by itself (the window lost key
//! status) is noticed by the next frame and ends in `pointerlockchange` too.
//!
//! While locked, the lock element is the target of every pointer and mouse
//! event, as with capture, and the events report the position the pointer had
//! when the lock began (`PointerState::position_for`).

use std::sync::{Arc, Mutex};

use crate::retained_tree::RetainedTree;

use super::state::{is_connected, LockRequest, PointerState};

/// What the lock needs from a window. GPUI's `Window` is the real one; the test
/// renderer's offscreen window is never key, so it cannot take a real lock.
pub trait LockWindow {
    /// Asks the platform for the lock. False when it refuses.
    fn request(&mut self) -> bool;
    fn exit(&mut self);
    /// Whether the platform still holds the lock.
    fn is_locked(&self) -> bool;
}

/// The lock of a real GPUI window.
pub struct GpuiLock<'a> {
    pub window: &'a mut gpui::Window,
    pub cx: &'a mut gpui::App,
}

impl LockWindow for GpuiLock<'_> {
    fn request(&mut self) -> bool {
        self.window.request_pointer_lock(self.cx)
    }

    fn exit(&mut self) {
        self.window.exit_pointer_lock(self.cx);
    }

    fn is_locked(&self) -> bool {
        self.window.is_pointer_locked()
    }
}

/// A platform the test renderer drives by hand. It grants or refuses locks and
/// drops them when told to, which a real platform does on its own.
#[derive(Clone)]
pub struct SimulatedPlatform(Arc<Mutex<Simulated>>);

struct Simulated {
    locked: bool,
    granting: bool,
}

impl Default for SimulatedPlatform {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Simulated {
            locked: false,
            granting: true,
        })))
    }
}

impl SimulatedPlatform {
    /// Whether requests succeed, as a platform that can lock the pointer does.
    pub fn set_granting(&self, granting: bool) {
        self.0.lock().unwrap().granting = granting;
    }

    /// The platform drops the lock, as it does when the window deactivates.
    pub fn drop_lock(&self) {
        self.0.lock().unwrap().locked = false;
    }
}

impl LockWindow for SimulatedPlatform {
    fn request(&mut self) -> bool {
        let mut state = self.0.lock().unwrap();
        state.locked = state.granting;
        state.locked
    }

    fn exit(&mut self) {
        self.0.lock().unwrap().locked = false;
    }

    fn is_locked(&self) -> bool {
        self.0.lock().unwrap().locked
    }
}

/// An event to fire at the document, which is the window here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockEvent {
    /// The lock element is now this one, or none.
    Change(Option<u64>),
    /// A request for this element failed.
    Error(u64),
}

impl LockEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            LockEvent::Change(_) => "pointerlockchange",
            LockEvent::Error(_) => "pointerlockerror",
        }
    }

    pub fn element(&self) -> Option<u64> {
        match self {
            LockEvent::Change(element) => *element,
            LockEvent::Error(element) => Some(*element),
        }
    }
}

/// Applies the pending request, then checks the lock still stands: the platform
/// may have dropped it, and a locked element that left the tree ends the lock.
pub fn apply(
    state: &mut PointerState,
    tree: &RetainedTree,
    window: &mut impl LockWindow,
) -> Vec<LockEvent> {
    let mut events = Vec::new();
    match state.requests.take_lock_request() {
        Some(LockRequest::Enter(id)) => enter(state, tree, window, id, &mut events),
        Some(LockRequest::Exit) => {
            if state.lock.is_some() {
                window.exit();
                state.end_lock();
                events.push(LockEvent::Change(None));
            }
        }
        None => {}
    }
    if let Some(element) = state.lock_element() {
        if !window.is_locked() {
            state.end_lock();
            events.push(LockEvent::Change(None));
        } else if !is_connected(tree, element) {
            window.exit();
            state.end_lock();
            events.push(LockEvent::Change(None));
        }
    }
    events
}

fn enter(
    state: &mut PointerState,
    tree: &RetainedTree,
    window: &mut impl LockWindow,
    id: u64,
    events: &mut Vec<LockEvent>,
) {
    if !is_connected(tree, id) {
        events.push(LockEvent::Error(id));
        return;
    }
    match state.lock_element() {
        Some(current) if current == id => {}
        // The platform lock is already held; only its element changes.
        Some(_) if window.is_locked() => {
            state.begin_lock(id);
            events.push(LockEvent::Change(Some(id)));
        }
        _ => {
            if window.request() {
                state.begin_lock(id);
                events.push(LockEvent::Change(Some(id)));
            } else {
                events.push(LockEvent::Error(id));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::state::PointerRequests;

    fn tree() -> RetainedTree {
        let mut tree = RetainedTree::new();
        for id in [1, 2, 3] {
            tree.create_element(id, "div".to_string());
        }
        tree.set_root(1);
        tree.append_child(1, 2);
        // 3 is created but never attached.
        tree
    }

    fn state() -> PointerState {
        PointerState::with_requests(PointerRequests::default())
    }

    #[test]
    fn nothing_requested_changes_nothing() {
        let mut platform = SimulatedPlatform::default();
        assert_eq!(apply(&mut state(), &tree(), &mut platform), []);
    }

    #[test]
    fn a_granted_request_locks_and_fires_pointerlockchange() {
        let mut platform = SimulatedPlatform::default();
        let mut state = state();
        state.requests.request_lock(2);
        assert_eq!(state.requests.locked_element(), None, "not before a frame");
        assert_eq!(
            apply(&mut state, &tree(), &mut platform),
            [LockEvent::Change(Some(2))]
        );
        assert_eq!(state.requests.locked_element(), Some(2));
        assert_eq!(state.lock_element(), Some(2));
        assert!(platform.is_locked());
    }

    #[test]
    fn a_refused_request_fires_pointerlockerror() {
        let mut platform = SimulatedPlatform::default();
        platform.set_granting(false);
        let mut state = state();
        state.requests.request_lock(2);
        assert_eq!(
            apply(&mut state, &tree(), &mut platform),
            [LockEvent::Error(2)]
        );
        assert_eq!(state.requests.locked_element(), None);
    }

    #[test]
    fn an_element_that_is_not_connected_cannot_be_locked() {
        for id in [3, 99] {
            let mut platform = SimulatedPlatform::default();
            let mut state = state();
            state.requests.request_lock(id);
            assert_eq!(
                apply(&mut state, &tree(), &mut platform),
                [LockEvent::Error(id)]
            );
            assert!(!platform.is_locked(), "the platform was never asked");
        }
    }

    #[test]
    fn asking_again_for_the_lock_element_is_quiet() {
        let mut platform = SimulatedPlatform::default();
        let mut state = state();
        state.requests.request_lock(2);
        apply(&mut state, &tree(), &mut platform);
        state.requests.request_lock(2);
        assert_eq!(apply(&mut state, &tree(), &mut platform), []);
    }

    #[test]
    fn locking_another_element_moves_the_lock_without_asking_the_platform_again() {
        let mut platform = SimulatedPlatform::default();
        let mut state = state();
        state.requests.request_lock(2);
        apply(&mut state, &tree(), &mut platform);
        platform.set_granting(false);
        state.requests.request_lock(1);
        assert_eq!(
            apply(&mut state, &tree(), &mut platform),
            [LockEvent::Change(Some(1))]
        );
        assert!(platform.is_locked());
    }

    #[test]
    fn exiting_releases_and_fires_pointerlockchange_with_no_element() {
        let mut platform = SimulatedPlatform::default();
        let mut state = state();
        state.requests.request_lock(2);
        apply(&mut state, &tree(), &mut platform);
        state.requests.exit_lock();
        assert_eq!(
            apply(&mut state, &tree(), &mut platform),
            [LockEvent::Change(None)]
        );
        assert_eq!(state.requests.locked_element(), None);
        assert!(!platform.is_locked());
    }

    #[test]
    fn exiting_when_not_locked_is_quiet() {
        let mut platform = SimulatedPlatform::default();
        let mut state = state();
        state.requests.exit_lock();
        assert_eq!(apply(&mut state, &tree(), &mut platform), []);
    }

    #[test]
    fn a_request_followed_by_an_exit_before_the_frame_is_no_request() {
        let mut platform = SimulatedPlatform::default();
        let mut state = state();
        state.requests.request_lock(2);
        state.requests.exit_lock();
        assert_eq!(apply(&mut state, &tree(), &mut platform), []);
        assert!(!platform.is_locked());
    }

    #[test]
    fn a_lock_the_platform_dropped_ends_with_pointerlockchange() {
        let mut platform = SimulatedPlatform::default();
        let mut state = state();
        state.requests.request_lock(2);
        apply(&mut state, &tree(), &mut platform);
        platform.drop_lock();
        assert_eq!(
            apply(&mut state, &tree(), &mut platform),
            [LockEvent::Change(None)]
        );
        assert_eq!(state.requests.locked_element(), None);
        assert_eq!(apply(&mut state, &tree(), &mut platform), []);
    }

    #[test]
    fn removing_the_locked_element_exits_the_lock() {
        let mut platform = SimulatedPlatform::default();
        let mut tree = tree();
        let mut state = state();
        state.requests.request_lock(2);
        apply(&mut state, &tree, &mut platform);
        tree.destroy_element(2);
        assert_eq!(
            apply(&mut state, &tree, &mut platform),
            [LockEvent::Change(None)]
        );
        assert!(!platform.is_locked());
        assert_eq!(state.lock_element(), None);
    }

    #[test]
    fn the_position_freezes_where_the_pointer_was_when_the_lock_began() {
        let mut platform = SimulatedPlatform::default();
        let mut state = state();
        assert_eq!(state.position_for((10.0, 20.0)), (10.0, 20.0));
        state.requests.request_lock(2);
        apply(&mut state, &tree(), &mut platform);
        assert_eq!(state.position_for((50.0, 60.0)), (10.0, 20.0));
        state.requests.exit_lock();
        apply(&mut state, &tree(), &mut platform);
        assert_eq!(state.position_for((50.0, 60.0)), (50.0, 60.0));
    }

    #[test]
    fn a_lock_before_any_input_freezes_at_the_first_one() {
        let mut platform = SimulatedPlatform::default();
        let mut state = state();
        state.requests.request_lock(2);
        apply(&mut state, &tree(), &mut platform);
        assert_eq!(state.position_for((5.0, 6.0)), (5.0, 6.0));
        assert_eq!(state.position_for((50.0, 60.0)), (5.0, 6.0));
    }
}
