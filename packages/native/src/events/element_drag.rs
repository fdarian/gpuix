//! Dragging an element: the `draggable` prop, `dragstart` to `dragend`.
//!
//! A primary press on a `draggable` element, or inside one, waits for the pointer
//! to move more than `DRAG_THRESHOLD`. Then the press is over for the pointer
//! events: the press target gets `pointercancel` and the pointer leaves it,
//! and the session begins with `dragstart` at the draggable ancestor, the
//! source. For as long as it lasts no pointer or mouse event fires, as in a
//! browser. Each move sends `drag` to the source and the boundary and
//! `dragover` events of `dispatch_file_drag` to what is under the pointer; the
//! release is a `drop` there, then `dragend` at the source. The pointer leaving
//! the window is a `dragleave` and the drag goes on.
//!
//! Which `drop` JS delivers, and the `dropEffect` of `dragend`, are JS's to
//! decide (it keeps the `dataTransfer` and sees every `dragover`), so native
//! sends every drag event whoever listens, except `drag`. A drag of the
//! element ends on `dragend`, and on a cancel with no event at all: `cancelDrag`,
//! which JS calls when `dragstart` was cancelled.
//!
//! There is no drag image: GPUI has no OS drag, so nothing follows the pointer.

use crate::retained_tree::RetainedTree;

use super::dispatcher::{
    enter_drag_target, set_active, update_hover, Emitter, Fields, PointerAction, PointerSample,
};
use super::state::{DragOver, PointerState};

const PRIMARY_BUTTON: u32 = 0;

/// How far the pointer moves with the button held before a press becomes a
/// drag, in logical pixels.
const DRAG_THRESHOLD: f64 = 4.0;

/// A press on a draggable element that has not moved far enough.
pub(super) struct PendingDrag {
    source: u64,
    origin: (f64, f64),
}

pub(super) struct ElementDrag {
    source: u64,
    over: Option<DragOver>,
}

/// The nearest `draggable` element on a press's `path` (target first), the
/// target included.
pub(super) fn pending(
    tree: &RetainedTree,
    path: &[u64],
    origin: (f64, f64),
) -> Option<PendingDrag> {
    path.iter()
        .copied()
        .find(|id| {
            tree.elements
                .get(id)
                .is_some_and(|element| element.draggable())
        })
        .map(|source| PendingDrag { source, origin })
}

/// Handles `sample` when it belongs to an element drag, one that is in
/// progress or one it starts. `None` hands it to the pointer. `Some` is
/// whether the view must restyle.
pub(super) fn dispatch(
    state: &mut PointerState,
    emitter: &mut Emitter,
    sample: &PointerSample,
    geometric: Option<u64>,
) -> Option<bool> {
    let target = geometric.or(emitter.tree.root_id);
    if let Some(drag) = state.element_drag.take() {
        return Some(continue_drag(state, drag, emitter, sample, target));
    }
    match sample.action {
        PointerAction::Move { pressed_button } => {
            let pending = state.pending_drag.as_ref()?;
            if pressed_button != Some(PRIMARY_BUTTON)
                || !emitter.tree.elements.contains_key(&pending.source)
            {
                state.pending_drag = None;
                return None;
            }
            let (dx, dy) = (
                sample.position.0 - pending.origin.0,
                sample.position.1 - pending.origin.1,
            );
            if dx.hypot(dy) <= DRAG_THRESHOLD {
                return None;
            }
            let source = pending.source;
            // Another element holds the pointer: its press is not a drag.
            let holder = state.capture.target().or(state.explicit);
            if holder.is_some_and(|holder| holder != source) {
                return None;
            }
            Some(begin(state, emitter, source, target))
        }
        PointerAction::Up {
            button: PRIMARY_BUTTON,
            ..
        } => {
            state.pending_drag = None;
            None
        }
        PointerAction::CancelDrag => Some(false),
        _ => None,
    }
}

fn begin(
    state: &mut PointerState,
    emitter: &mut Emitter,
    source: u64,
    target: Option<u64>,
) -> bool {
    state.pending_drag = None;
    if let Some(press) = state.pressed.remove(&PRIMARY_BUTTON) {
        if let Some(pressed) = press.path.first() {
            emitter.emit("pointercancel", *pressed, Fields::default());
        }
    }
    // The drag takes the pointer, and with it any capture the source held.
    state.capture.clear();
    if let Some(captured) = state.explicit.take() {
        emitter.emit("lostpointercapture", captured, Fields::default());
    }
    let mut restyle = update_hover(state, emitter, None);
    restyle |= set_active(state, emitter.tree, Vec::new());

    emitter.emit_drag("dragstart", source, None, None);
    let mut drag = ElementDrag { source, over: None };
    drag.step(emitter, target);
    state.element_drag = Some(drag);
    restyle
}

fn continue_drag(
    state: &mut PointerState,
    mut drag: ElementDrag,
    emitter: &mut Emitter,
    sample: &PointerSample,
    target: Option<u64>,
) -> bool {
    match sample.action {
        PointerAction::Move {
            pressed_button: Some(_),
        } => {
            drag.step(emitter, target);
            state.element_drag = Some(drag);
        }
        // The button went up where no release reached us: nothing was dropped.
        PointerAction::Move {
            pressed_button: None,
        }
        | PointerAction::CancelDrag => drag.cancel(emitter),
        PointerAction::Up {
            button: PRIMARY_BUTTON,
            ..
        } => drag.release(emitter, target),
        PointerAction::Resync => {
            if let Some(target) = target {
                enter_drag_target(&mut drag.over, emitter, target, None);
            }
            state.element_drag = Some(drag);
        }
        // Unlike a drag of files, the drag outlives the pointer leaving the
        // window: the element being dragged is still the source.
        PointerAction::Exit => {
            drag.leave(emitter);
            state.element_drag = Some(drag);
        }
        _ => state.element_drag = Some(drag),
    }
    false
}

impl ElementDrag {
    /// The pointer moved: `drag` at the source, then the boundary events and
    /// `dragover` of whatever is under it.
    fn step(&mut self, emitter: &mut Emitter, target: Option<u64>) {
        emitter.emit("drag", self.source, Fields::default());
        if let Some(target) = target {
            enter_drag_target(&mut self.over, emitter, target, None);
            emitter.emit_drag("dragover", target, None, None);
        }
    }

    /// The pointer left the window: `dragleave` with no related target, and the
    /// next move enters its target again.
    fn leave(&mut self, emitter: &mut Emitter) {
        let Some(over) = self.over.take() else {
            return;
        };
        if emitter.tree.elements.contains_key(&over.target) {
            emitter.emit_drag("dragleave", over.target, None, None);
        }
    }

    fn release(self, emitter: &mut Emitter, target: Option<u64>) {
        let mut over = self.over;
        if let Some(target) = target {
            enter_drag_target(&mut over, emitter, target, None);
            emitter.emit_drag("drop", target, None, None);
        }
        end(emitter, self.source);
    }

    fn cancel(self, emitter: &mut Emitter) {
        if let Some(over) = self
            .over
            .filter(|over| emitter.tree.elements.contains_key(&over.target))
        {
            emitter.emit_drag("dragleave", over.target, None, None);
        }
        end(emitter, self.source);
    }
}

/// `dragend` goes to the source if it is still in the tree: an event needs an
/// element to be aimed at.
fn end(emitter: &mut Emitter, source: u64) {
    if emitter.tree.elements.contains_key(&source) {
        emitter.emit_drag("dragend", source, None, None);
    }
}
