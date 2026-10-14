//! What the pointer is doing, kept between inputs.
//!
//! The view owns one `PointerState` (`GpuixView::pointer`). The dispatcher
//! updates it; `build_element` and the key-event code read it through
//! `is_hovered` and `is_active`.

use std::cell::Cell;
use std::rc::Rc;

use rustc_hash::FxHashMap;

use super::ids::Hit;

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
    /// `mousemove`. Explicit capture replaces this.
    pub(super) captured: Option<u64>,
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
        self.captured
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

    #[test]
    fn nothing_is_observed_before_the_first_input() {
        let state = PointerState::default();
        assert_eq!(state.observed().get(), None);
        state.observe(None);
        assert_eq!(state.observed().get(), Some(None));
    }
}
