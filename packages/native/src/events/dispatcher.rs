//! Turns pointer input into the DOM events an app listens for.
//!
//! One input (a move, a button going down or up, the pointer leaving the window)
//! arrives as a `PointerSample`: what GPUI's hit test found under the pointer
//! plus the raw event. `dispatch` works out the target, diffs it against where
//! the pointer was, and returns every event that follows, in DOM order. It
//! reads the retained tree and writes `PointerState`; it touches no GPUI
//! state, which is what lets the rules be tested without a window.
//!
//! Order of one input, as in Chrome:
//!   lostpointercapture, gotpointercapture for a capture JS changed since the last input
//!   boundary events for a changed target
//!     pointerout, pointerleave (innermost first), pointerover, pointerenter
//!     (outermost first), then the same four as mouseout, mouseleave, mouseover,
//!     mouseenter
//!   then the event itself, pointer before mouse
//!     pointermove/mousemove, pointerdown/mousedown, mousedownoutside, contextmenu,
//!     pointerup, lostpointercapture, mouseup, then click, dblclick or auxclick
//!
//! A type is only produced when the target or an ancestor listens for it, so a
//! window with no `mousemove` handler pays nothing for the traffic.
//!
//! A file dragged in from the OS replaces the pointer until it leaves or drops
//! (`dispatch_file_drag`). No pointer or mouse event fires, as in a browser;
//! `dragenter`, `dragover`, `dragleave` and `drop` do instead. Those are not
//! gated on listeners: JS needs every `dragover` to know whether the last one
//! was cancelled, which decides if the drop is delivered. A dragged element
//! (`element_drag`) replaces the pointer the same way and shares that code.

use std::path::PathBuf;

use gpui::{Bounds, Pixels};

use crate::element_tree::{DroppedFile, EventModifiers, EventPayload};
use crate::retained_tree::RetainedTree;
use crate::style::StyleDesc;

use super::app_region;
use super::element_drag;
use super::file_drag;
use super::ids::Hit;
use super::path::{chain_follows_path, common_ancestor, event_path, path_listens};
use super::state::{is_connected, DragOver, PointerState, Press};

const PRIMARY_BUTTON: u32 = 0;
const SECONDARY_BUTTON: u32 = 2;
/// Chrome on macOS opens the context menu for ctrl+primary as it does for the
/// secondary button, and the press stays a primary one.
const CTRL_CLICK_OPENS_CONTEXT_MENU: bool = cfg!(target_os = "macos");
const DOUBLE_CLICK: u32 = 2;
const MOUSE_DOWN_OUTSIDE: &str = "mousedownoutside";

#[derive(Clone)]
pub enum PointerAction {
    Move {
        pressed_button: Option<u32>,
    },
    Down {
        button: u32,
        click_count: u32,
    },
    Up {
        button: u32,
        click_count: u32,
    },
    /// The pointer left the window.
    Exit,
    /// Layout moved under a pointer that did not: boundary events and nothing else.
    Resync,
    /// Files from the OS are over the window (GPUI's `FileDropEvent::Entered`
    /// and `Pending`, which it turns into a move with the paths as the active drag).
    FileDragMove { paths: Vec<PathBuf> },
    /// The files were dropped on the window (`FileDropEvent::Submit`).
    FileDrop,
    /// The files left the window or the drag ended (`FileDropEvent::Exited`, `Ended`).
    FileDragLeave,
    /// The key that cancels a drag of an element went down.
    CancelDrag,
    /// One wheel or trackpad tick. It reads the pointer but does not move it: no
    /// boundary event, hover or capture changes.
    Wheel(WheelTick),
}

/// What one wheel input carries, in the DOM's terms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelTick {
    /// Pixels the scroll position would move, positive for down and right. A
    /// mouse wheel's lines are already converted.
    pub delta: (f64, f64),
    /// Pixel-precise (a trackpad) rather than line-based (a mouse wheel).
    pub precise: bool,
    /// Where the tick sits in a gesture: `started`, `moved`, `ended` or `cancelled`.
    pub phase: &'static str,
}

#[derive(Clone)]
pub struct PointerSample {
    pub action: PointerAction,
    /// Window coordinates.
    pub position: (f64, f64),
    /// How far the pointer moved with this input, in logical pixels. Only a
    /// move carries any.
    pub movement: (f64, f64),
    pub modifiers: EventModifiers,
    /// Host elements under the pointer, target first. Empty outside the window.
    pub hits: Vec<Hit>,
    /// The pointer is over a window drag region (`appRegion: "drag"`). The dispatcher decides
    /// whether that hides it from the page; see `app_region::hides_pointer`.
    pub over_drag_region: bool,
}

/// One event to deliver. The pointer position, modifiers and held buttons are
/// the same for every event of an input, so `Output` carries them once.
pub struct DomEvent {
    pub event_type: &'static str,
    /// Target first, then each retained ancestor.
    pub path: Vec<u64>,
    /// The one element the event is delivered to, when it does not go to the
    /// target and its ancestors: `mousedownoutside` runs at the element that
    /// listens for it, with the press target in `path`.
    pub listener: Option<u64>,
    pub related_target: Option<u64>,
    pub button: Option<u32>,
    pub detail: Option<u32>,
    /// Pointer position relative to the target's padding edge.
    pub offset: Option<(f64, f64)>,
    pub movement: Option<(f64, f64)>,
    pub pressed_button: Option<u32>,
    pub hovered: Option<bool>,
    /// `dataTransfer.files`, on the first event of a file drag.
    pub files: Option<Vec<DroppedFile>>,
    pub wheel: Option<WheelTick>,
}

impl DomEvent {
    pub fn target(&self) -> u64 {
        self.path[0]
    }
}

pub struct Output {
    pub events: Vec<DomEvent>,
    /// An element that entered or left the hover or active chain has a state
    /// style, so the view must rebuild to restyle it.
    pub restyle: bool,
    /// GPUI's hit chain and the retained path disagree about the target's
    /// ancestry. The retained path is what events use; this reports the drift.
    pub chain_mismatch: bool,
    position: (f64, f64),
    modifiers: EventModifiers,
    buttons: u32,
}

impl Output {
    pub(super) fn new(
        events: Vec<DomEvent>,
        restyle: bool,
        chain_mismatch: bool,
        sample: &PointerSample,
        buttons: u32,
    ) -> Self {
        Self {
            events,
            restyle,
            chain_mismatch,
            position: sample.position,
            modifiers: sample.modifiers.clone(),
            buttons,
        }
    }

    pub fn into_payloads(self) -> Vec<EventPayload> {
        let position = self.position;
        let buttons = self.buttons;
        let modifiers = self.modifiers;
        self.events
            .into_iter()
            .map(|event| EventPayload {
                element_id: event.listener.unwrap_or(event.target()) as f64,
                event_type: event.event_type.to_string(),
                target: Some(event.target() as f64),
                path: Some(event.path.iter().map(|id| *id as f64).collect()),
                related_target: event.related_target.map(|id| id as f64),
                x: Some(position.0),
                y: Some(position.1),
                client_x: Some(position.0),
                client_y: Some(position.1),
                offset_x: event.offset.map(|offset| offset.0),
                offset_y: event.offset.map(|offset| offset.1),
                movement_x: event.movement.map(|movement| movement.0),
                movement_y: event.movement.map(|movement| movement.1),
                button: event.button,
                is_right_click: event.button.map(|button| button == SECONDARY_BUTTON),
                buttons: Some(buttons),
                detail: event.detail,
                click_count: event.detail,
                pressed_button: event.pressed_button,
                hovered: event.hovered,
                files: event.files,
                delta_x: event.wheel.map(|wheel| wheel.delta.0),
                delta_y: event.wheel.map(|wheel| wheel.delta.1),
                delta_mode: event.wheel.map(|_| 0),
                precise: event.wheel.map(|wheel| wheel.precise),
                touch_phase: event.wheel.map(|wheel| wheel.phase.to_string()),
                pointer_id: Some(1),
                pointer_type: Some("mouse".to_string()),
                modifiers: Some(modifiers.clone()),
                ..Default::default()
            })
            .collect()
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct Fields {
    pub movement: Option<(f64, f64)>,
    pub related_target: Option<u64>,
    pub button: Option<u32>,
    pub detail: Option<u32>,
    pub pressed_button: Option<u32>,
    pub hovered: Option<bool>,
    pub wheel: Option<WheelTick>,
}

pub(super) struct Emitter<'a> {
    pub tree: &'a RetainedTree,
    pub(super) position: (f64, f64),
    /// Boxes of the chain under the pointer now, and of the chain it left.
    hits: &'a [Hit],
    previous_hits: &'a [Hit],
    pub events: Vec<DomEvent>,
}

impl<'a> Emitter<'a> {
    pub(super) fn new(
        tree: &'a RetainedTree,
        sample: &'a PointerSample,
        previous_hits: &'a [Hit],
    ) -> Self {
        Self {
            tree,
            position: sample.position,
            hits: &sample.hits,
            previous_hits,
            events: Vec::new(),
        }
    }

    pub(super) fn emit(&mut self, event_type: &'static str, target: u64, fields: Fields) {
        if !path_listens(self.tree, target, event_type) {
            return;
        }
        self.push(event_type, target, fields);
    }

    /// A `click`, which also has to reach a label with no listener: whether
    /// the label activates its control is decided after the event ran.
    fn emit_click(&mut self, target: u64, fields: Fields) {
        let activates = || {
            let path = event_path(self.tree, target);
            crate::accessibility::label::activated_control(self.tree, &path).is_some()
        };
        if path_listens(self.tree, target, "click") || activates() {
            self.push("click", target, fields);
        }
    }

    fn push(&mut self, event_type: &'static str, target: u64, fields: Fields) {
        self.events.push(DomEvent {
            event_type,
            path: event_path(self.tree, target),
            listener: None,
            related_target: fields.related_target,
            button: fields.button,
            detail: fields.detail,
            offset: self.offset(target),
            movement: fields.movement,
            pressed_button: fields.pressed_button,
            hovered: fields.hovered,
            files: None,
            wheel: fields.wheel,
        });
    }

    /// A drag event at `target`, whoever listens.
    pub(super) fn emit_drag(
        &mut self,
        event_type: &'static str,
        target: u64,
        related_target: Option<u64>,
        files: Option<Vec<DroppedFile>>,
    ) {
        self.events.push(DomEvent {
            event_type,
            path: event_path(self.tree, target),
            listener: None,
            related_target,
            button: None,
            detail: None,
            offset: self.offset(target),
            movement: None,
            pressed_button: None,
            hovered: None,
            files,
            wheel: None,
        });
    }

    /// `mousedownoutside` for every element that listens for it and does not
    /// contain `target` in its logical subtree: the target's retained path is
    /// the elements it is inside, anchored layers included. A press that hit no
    /// element (`None`) is a press on the document, as in the DOM: it targets
    /// the root and is outside every listener, the root's own included.
    fn emit_outside_presses(&mut self, target: Option<u64>, fields: Fields) {
        let Some(path) = target
            .or(self.tree.root_id)
            .map(|target| event_path(self.tree, target))
            .filter(|path| !path.is_empty())
        else {
            return;
        };
        let inside: &[u64] = if target.is_some() { &path } else { &[] };
        let mut listeners: Vec<u64> = self
            .tree
            .elements
            .iter()
            .filter(|(id, element)| {
                element.events().contains(MOUSE_DOWN_OUTSIDE)
                    && !inside.contains(id)
                    && is_connected(self.tree, **id)
            })
            .map(|(id, _)| *id)
            .collect();
        listeners.sort_unstable();
        for listener in listeners {
            self.events.push(DomEvent {
                event_type: MOUSE_DOWN_OUTSIDE,
                path: path.clone(),
                listener: Some(listener),
                related_target: fields.related_target,
                button: fields.button,
                detail: fields.detail,
                offset: self.offset(path[0]),
                movement: None,
                pressed_button: fields.pressed_button,
                hovered: fields.hovered,
                files: None,
                wheel: None,
            });
        }
    }

    fn offset(&self, target: u64) -> Option<(f64, f64)> {
        let origin = self.origin(target)?;
        let style = self
            .tree
            .elements
            .get(&target)
            .and_then(|element| element.style.as_deref());
        let (border_left, border_top) = border_insets(style);
        Some((
            self.position.0 - origin.0 - border_left,
            self.position.1 - origin.1 - border_top,
        ))
    }

    /// Where the element's border box starts. The painted box is the fallback for
    /// an element the pointer is not over, such as one that holds capture.
    fn origin(&self, id: u64) -> Option<(f64, f64)> {
        self.hits
            .iter()
            .chain(self.previous_hits)
            .find(|hit| hit.id == id)
            .map(|hit| hit_origin(hit.bounds))
            .or_else(|| crate::automation::get_bounds(id).map(|bounds| (bounds.x, bounds.y)))
    }
}

fn hit_origin(bounds: Bounds<Pixels>) -> (f64, f64) {
    (
        f64::from(f32::from(bounds.origin.x)),
        f64::from(f32::from(bounds.origin.y)),
    )
}

fn border_insets(style: Option<&StyleDesc>) -> (f64, f64) {
    let Some(style) = style else {
        return (0.0, 0.0);
    };
    let left = style.border_left_width.or(style.border_width);
    let top = style.border_top_width.or(style.border_width);
    (left.unwrap_or(0.0).max(0.0), top.unwrap_or(0.0).max(0.0))
}

pub(super) fn button_bit(button: u32) -> u32 {
    match button {
        0 => 1,
        1 => 4,
        2 => 2,
        3 => 8,
        _ => 16,
    }
}

/// The lowest-numbered button of a `buttons` mask: what GPUI would report as
/// the one pressed.
fn lowest_held_button(buttons: u32) -> Option<u32> {
    (0..5).find(|button| buttons & button_bit(*button) != 0)
}

pub fn dispatch(state: &mut PointerState, tree: &RetainedTree, sample: &PointerSample) -> Output {
    // Over a drag region the page sees the pointer outside the window: nothing is under it, so
    // hover leaves and no move is delivered.
    let hidden;
    let sample = if app_region::hides_pointer(state, sample) {
        hidden = PointerSample {
            hits: Vec::new(),
            ..sample.clone()
        };
        &hidden
    } else {
        sample
    };
    let position = state.position_for(sample.position);
    if position == sample.position {
        return dispatch_at(state, tree, sample);
    }
    // A locked pointer reports the position it was locked at, whatever the platform says.
    let frozen = PointerSample {
        position,
        ..sample.clone()
    };
    dispatch_at(state, tree, &frozen)
}

fn dispatch_at(state: &mut PointerState, tree: &RetainedTree, sample: &PointerSample) -> Output {
    let geometric = sample
        .hits
        .iter()
        .map(|hit| hit.id)
        .find(|id| tree.elements.contains_key(id));
    state.observe(&sample.hits);
    let chain_mismatch = geometric.is_some_and(|target| {
        let chain: Vec<u64> = sample
            .hits
            .iter()
            .map(|hit| hit.id)
            .filter(|id| tree.elements.contains_key(id))
            .collect();
        !chain_follows_path(&chain, &event_path(tree, target))
    });
    if let PointerAction::Wheel(tick) = sample.action {
        return dispatch_wheel(state, tree, sample, tick, geometric, chain_mismatch);
    }
    if state.requests.take_drag_cancel() {
        state.element_drag = None;
    }
    let file_drag_input = match sample.action {
        PointerAction::FileDragMove { .. }
        | PointerAction::FileDrop
        | PointerAction::FileDragLeave => true,
        PointerAction::Resync | PointerAction::Exit => state.file_drag.is_some(),
        _ => false,
    };
    if file_drag_input {
        return dispatch_file_drag(state, tree, sample, geometric, chain_mismatch);
    }
    let mut lost_release = false;
    match sample.action {
        PointerAction::Down { button, .. } => {
            state.buttons |= button_bit(button);
            // A press the window did not take means its earlier press of this button is over,
            // even when the OS drag loop ate the release.
            state.window_presses &= !button_bit(button);
        }
        PointerAction::Up { button, .. } => state.buttons &= !button_bit(button),
        // GPUI reports one pressed button on a move, so a move cannot say which
        // of several are held. It can say that none is: the release was lost.
        PointerAction::Move {
            pressed_button: None,
        } => {
            lost_release = std::mem::take(&mut state.buttons) != 0;
            state.window_presses = 0;
        }
        // The window cannot hold a press for a pointer that left it.
        PointerAction::Exit => state.window_presses = 0,
        PointerAction::Move { .. } | PointerAction::Resync | PointerAction::CancelDrag => {}
        PointerAction::FileDragMove { .. }
        | PointerAction::FileDrop
        | PointerAction::FileDragLeave
        | PointerAction::Wheel(_) => unreachable!("handled above"),
    }
    state.requests.set_buttons(state.buttons);

    let previous_hits = std::mem::take(&mut state.hits);
    let mut emitter = Emitter::new(tree, sample, &previous_hits);
    if let Some(restyle) = element_drag::dispatch(state, &mut emitter, sample, geometric) {
        state.hits = sample.hits.clone();
        return Output::new(emitter.events, restyle, chain_mismatch, sample, state.buttons);
    }

    // A real pointer input means the drag ended without a leave reaching us.
    end_file_drag(state, &mut emitter);
    let mut restyle = false;
    if lost_release {
        restyle |= cancel_press(state, &mut emitter);
    }
    apply_capture(state, &mut emitter);
    // A captured element keeps the pointer: it is the hover target and takes
    // every event until the release, wherever the pointer is. So does the
    // element that holds the pointer lock, and it outranks capture.
    let target = pinned_target(state, tree).or(geometric);
    restyle |= update_hover(state, &mut emitter, target);

    match sample.action {
        PointerAction::Move { pressed_button } => {
            if let Some(target) = target {
                let fields = Fields {
                    pressed_button,
                    movement: Some(sample.movement),
                    ..Default::default()
                };
                emitter.emit("pointermove", target, fields);
                emitter.emit("mousemove", target, fields);
            }
        }
        PointerAction::Down {
            button,
            click_count,
        } => {
            match target {
                Some(target) => {
                    restyle |= press(
                        state,
                        &mut emitter,
                        target,
                        button,
                        click_count,
                        &sample.modifiers,
                    );
                }
                None => emitter.emit_outside_presses(
                    None,
                    Fields {
                        button: Some(button),
                        detail: Some(click_count),
                        ..Default::default()
                    },
                ),
            }
        }
        PointerAction::Up {
            button,
            click_count,
        } => {
            restyle |= release(state, &mut emitter, target, geometric, button, click_count);
        }
        PointerAction::Exit | PointerAction::Resync | PointerAction::CancelDrag => {}
        PointerAction::FileDragMove { .. }
        | PointerAction::FileDrop
        | PointerAction::FileDragLeave
        | PointerAction::Wheel(_) => unreachable!("handled above"),
    }

    state.hits = sample.hits.clone();
    Output::new(emitter.events, restyle, chain_mismatch, sample, state.buttons)
}

/// Dispatches one wheel tick, a `wheel` event at what is under the pointer.
///
/// Pointer capture does not redirect it: a browser aims a wheel at the element
/// under the pointer even while another holds the capture. A pointer lock does,
/// as it does every mouse event. The tick changes no state: the pointer did not
/// move, so there is no hover or boundary event, and the next move diffs against
/// the chain the last one saw.
fn dispatch_wheel(
    state: &mut PointerState,
    tree: &RetainedTree,
    sample: &PointerSample,
    tick: WheelTick,
    geometric: Option<u64>,
    chain_mismatch: bool,
) -> Output {
    let locked = state.lock_element().filter(|id| is_connected(tree, *id));
    let mut emitter = Emitter::new(tree, sample, &state.hits);
    if let Some(target) = locked.or(geometric) {
        let fields = Fields {
            wheel: Some(tick),
            ..Default::default()
        };
        emitter.emit("wheel", target, fields);
    }
    Output::new(emitter.events, false, chain_mismatch, sample, state.buttons)
}

/// Dispatches one input of an OS file drag.
///
/// The target is what is under the pointer, or the root when nothing is: a
/// drop on the window outside every element is a drop on the document.
/// `dragenter` goes to the new target before `dragleave` goes to the old one,
/// then `dragover` to the target, on every move.
fn dispatch_file_drag(
    state: &mut PointerState,
    tree: &RetainedTree,
    sample: &PointerSample,
    geometric: Option<u64>,
    chain_mismatch: bool,
) -> Output {
    let previous_hits = std::mem::take(&mut state.hits);
    let mut emitter = Emitter::new(tree, sample, &previous_hits);
    let target = geometric.or(tree.root_id);
    match &sample.action {
        PointerAction::FileDragMove { paths } => {
            if let Some(target) = target {
                let files = if state.file_drag.is_none() && !state.file_drag_unreadable {
                    let files = file_drag::describe(paths);
                    state.file_drag_unreadable = files.is_empty();
                    (!files.is_empty()).then_some(files)
                } else {
                    None
                };
                // A drag with nothing readable has no session and no events.
                if files.is_some() || state.file_drag.is_some() {
                    enter_drag_target(&mut state.file_drag, &mut emitter, target, files);
                    emitter.emit_drag("dragover", target, None, None);
                }
            }
        }
        PointerAction::Resync => {
            if let Some(target) = target {
                enter_drag_target(&mut state.file_drag, &mut emitter, target, None);
            }
        }
        PointerAction::FileDrop => {
            if let (Some(target), true) = (target, state.file_drag.is_some()) {
                enter_drag_target(&mut state.file_drag, &mut emitter, target, None);
                emitter.emit_drag("drop", target, None, None);
                state.file_drag = None;
            }
            state.file_drag_unreadable = false;
        }
        PointerAction::FileDragLeave => end_file_drag(state, &mut emitter),
        _ => {}
    }

    let events = emitter.events;
    state.hits = if matches!(sample.action, PointerAction::FileDragLeave) {
        previous_hits
    } else {
        sample.hits.clone()
    };
    Output::new(events, false, chain_mismatch, sample, state.buttons)
}

/// Moves the drag to `target`: `dragenter` there, then `dragleave` at the
/// element it left. `files` start a drag of files.
pub(super) fn enter_drag_target(
    over: &mut Option<DragOver>,
    emitter: &mut Emitter,
    target: u64,
    files: Option<Vec<DroppedFile>>,
) {
    let previous = over.as_ref().map(|drag| drag.target);
    if previous == Some(target) {
        return;
    }
    emitter.emit_drag("dragenter", target, previous, files);
    if let Some(previous) = previous.filter(|id| emitter.tree.elements.contains_key(id)) {
        emitter.emit_drag("dragleave", previous, Some(target), None);
    }
    *over = Some(DragOver { target });
}

/// Ends the drag with a `dragleave` at the element it was over, which had no
/// related target: the files left the window.
fn end_file_drag(state: &mut PointerState, emitter: &mut Emitter) {
    state.file_drag_unreadable = false;
    let Some(drag) = state.file_drag.take() else {
        return;
    };
    if emitter.tree.elements.contains_key(&drag.target) {
        emitter.emit_drag("dragleave", drag.target, None, None);
    }
}

/// The element that keeps the pointer: the lock element, else the capturing one.
/// A lock element that left the tree since the frame that granted it is none.
fn pinned_target(state: &PointerState, tree: &RetainedTree) -> Option<u64> {
    state
        .lock_element()
        .filter(|id| is_connected(tree, *id))
        .or(state.explicit)
}

/// A move that shows no button down while one was: the release happened where
/// no event reached us (outside the window, over another app). The press is
/// cancelled as a browser cancels a pointer it lost: `pointercancel` at the
/// element that has or has been asked to have the pointer, else the one
/// pressed, then `lostpointercapture`, then the press and `:active` end, with
/// no click.
fn cancel_press(state: &mut PointerState, emitter: &mut Emitter) -> bool {
    let tree = emitter.tree;
    let pressed = state
        .pressed
        .iter()
        .min_by_key(|(button, _)| **button)
        .and_then(|(_, press)| press.path.first().copied());
    // The target is the pending capture, not the applied one: a capture JS
    // asked for since the last input already redirects the pointer. It never
    // took hold, so it fires neither `gotpointercapture` nor `lostpointercapture`.
    let capturer = state
        .lock_element()
        .or(state.requests.target())
        .filter(|id| is_connected(tree, *id));
    if let Some(target) = capturer.or(pressed) {
        emitter.emit("pointercancel", target, Fields::default());
    }
    state.requests.clear();
    if let Some(captured) = state.explicit.take() {
        if is_connected(tree, captured) {
            emitter.emit("lostpointercapture", captured, Fields::default());
        }
    }
    state.pressed.clear();
    state.pending_drag = None;
    set_active(state, tree, Vec::new())
}

/// Makes the capture JS asked for the one in force, firing `lostpointercapture`
/// on the element that had it and `gotpointercapture` on the one that has it now.
/// A capture whose element left the tree ends without an event, as in the DOM.
fn apply_capture(state: &mut PointerState, emitter: &mut Emitter) {
    let tree = emitter.tree;
    let mut requested = state.requests.target();
    if requested.is_some_and(|id| !is_connected(tree, id)) {
        state.requests.clear();
        requested = None;
    }
    if state.explicit == requested {
        return;
    }
    if let Some(previous) = state.explicit.filter(|id| is_connected(tree, *id)) {
        emitter.emit("lostpointercapture", previous, Fields::default());
    }
    if let Some(next) = requested {
        emitter.emit("gotpointercapture", next, Fields::default());
    }
    state.explicit = requested;
}

/// Moves the hover chain to `new_target` and emits the boundary events. Returns
/// whether an element that changed state has a `hover` style.
pub(super) fn update_hover(state: &mut PointerState, emitter: &mut Emitter, new_target: Option<u64>) -> bool {
    let tree = emitter.tree;
    let new_path = new_target
        .map(|target| event_path(tree, target))
        .unwrap_or_default();
    if state.hovered == new_path {
        return false;
    }
    let new_target = new_path.first().copied();
    let old_target = state
        .hovered
        .first()
        .copied()
        .filter(|id| tree.elements.contains_key(id));

    let leaving: Vec<u64> = state
        .hovered
        .iter()
        .copied()
        .filter(|id| tree.elements.contains_key(id) && !new_path.contains(id))
        .collect();
    let entering: Vec<u64> = new_path
        .iter()
        .rev()
        .copied()
        .filter(|id| !state.hovered.contains(id))
        .collect();

    let left = Fields {
        related_target: new_target,
        hovered: Some(false),
        ..Default::default()
    };
    let entered = Fields {
        related_target: old_target,
        hovered: Some(true),
        ..Default::default()
    };
    let left_target = old_target.filter(|old| Some(*old) != new_target);
    let entered_target = new_target.filter(|new| Some(*new) != old_target);
    // Chrome sends the four pointer boundary events, then the four mouse ones.
    for (out, leave, over, enter) in [
        ("pointerout", "pointerleave", "pointerover", "pointerenter"),
        ("mouseout", "mouseleave", "mouseover", "mouseenter"),
    ] {
        if let Some(old_target) = left_target {
            emitter.emit(out, old_target, left);
            for id in &leaving {
                emitter.emit(leave, *id, left);
            }
        }
        if let Some(new_target) = entered_target {
            emitter.emit(over, new_target, entered);
            for id in &entering {
                emitter.emit(enter, *id, entered);
            }
        }
    }

    let restyle = leaving
        .iter()
        .chain(&entering)
        .any(|id| {
            // A group's followers restyle with it.
            has_state_style(tree, *id, |style| {
                style.hover.is_some() || style.declared_group().is_some()
            })
        });
    state.hovered = new_path;
    restyle
}

/// A button changing while another is held is no new pointer contact: DOM
/// fires `pointerdown` and `pointerup` only when the first button goes down and
/// the last comes up, and a `pointermove` for the rest. The mouse events still
/// fire for every button.
fn emit_chord_move(state: &PointerState, emitter: &mut Emitter, target: u64, button: u32) {
    emitter.emit(
        "pointermove",
        target,
        Fields {
            button: Some(button),
            pressed_button: lowest_held_button(state.buttons),
            movement: Some((0.0, 0.0)),
            ..Default::default()
        },
    );
}

fn press(
    state: &mut PointerState,
    emitter: &mut Emitter,
    target: u64,
    button: u32,
    click_count: u32,
    modifiers: &EventModifiers,
) -> bool {
    let tree = emitter.tree;
    let fields = Fields {
        button: Some(button),
        detail: Some(click_count),
        ..Default::default()
    };
    if state.buttons == button_bit(button) {
        emitter.emit("pointerdown", target, fields);
    } else {
        emit_chord_move(state, emitter, target, button);
    }
    emitter.emit("mousedown", target, fields);
    emitter.emit_outside_presses(Some(target), fields);
    if button == SECONDARY_BUTTON
        || (CTRL_CLICK_OPENS_CONTEXT_MENU && button == PRIMARY_BUTTON && modifiers.ctrl)
    {
        // Where macOS and Chrome open the context menu: on the press.
        emitter.emit(
            "contextmenu",
            target,
            Fields {
                button: Some(button),
                ..Default::default()
            },
        );
    }

    let path = event_path(tree, target);
    if button != PRIMARY_BUTTON {
        state.pressed.insert(button, Press { path });
        return false;
    }
    let restyle = set_active(state, tree, path.clone());
    state.pending_drag = element_drag::pending(tree, &path, emitter.position);
    state.pressed.insert(button, Press { path });
    restyle
}

fn release(
    state: &mut PointerState,
    emitter: &mut Emitter,
    target: Option<u64>,
    geometric: Option<u64>,
    button: u32,
    click_count: u32,
) -> bool {
    let tree = emitter.tree;
    let fields = Fields {
        button: Some(button),
        detail: Some(click_count),
        ..Default::default()
    };
    if let Some(target) = target {
        if state.buttons == 0 {
            emitter.emit("pointerup", target, fields);
        } else {
            emit_chord_move(state, emitter, target, button);
        }
    }

    // Capture lasts until the last button is up. The Pointer Events spec
    // releases it right after pointerup, so mouseup still goes to the captured
    // element but lostpointercapture comes before it.
    let mut released = false;
    if state.buttons == 0 {
        state.requests.clear();
        if let Some(captured) = state.explicit.take() {
            emitter.emit("lostpointercapture", captured, Fields::default());
            released = true;
        }
    }

    if let Some(target) = target {
        emitter.emit("mouseup", target, fields);
    }

    if let Some(press) = state.pressed.remove(&button) {
        let release_path = target
            .map(|target| event_path(tree, target))
            .unwrap_or_default();
        // Only the click family: a disabled control still gets the pointer and
        // mouse events around it, as in the DOM.
        if let Some(clicked) = common_ancestor(&press.path, &release_path)
            .filter(|clicked| !tree.in_disabled_form_control(*clicked))
        {
            if button == PRIMARY_BUTTON {
                emitter.emit_click(clicked, fields);
                if click_count == DOUBLE_CLICK {
                    emitter.emit("dblclick", clicked, fields);
                }
            } else {
                emitter.emit("auxclick", clicked, fields);
            }
        }
    }

    let mut restyle = false;
    if button == PRIMARY_BUTTON {
        restyle |= set_active(state, tree, Vec::new());
    }
    if released {
        restyle |= update_hover(state, emitter, pinned_target(state, tree).or(geometric));
    }
    restyle
}

pub(super) fn set_active(state: &mut PointerState, tree: &RetainedTree, new_path: Vec<u64>) -> bool {
    if state.active == new_path {
        return false;
    }
    let restyle = state
        .active
        .iter()
        .filter(|id| !new_path.contains(id))
        .chain(new_path.iter().filter(|id| !state.active.contains(id)))
        .any(|id| {
            has_state_style(tree, *id, |style| {
                style.active.is_some() || style.declared_group().is_some()
            })
        });
    state.active = new_path;
    restyle
}

fn has_state_style(tree: &RetainedTree, id: u64, has: impl Fn(&StyleDesc) -> bool) -> bool {
    tree.elements
        .get(&id)
        .and_then(|element| element.style.as_deref())
        .is_some_and(has)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui::{point, px, size};

    use super::*;

    /// root(1) > a(2) > b(3), root > c(4), and an anchored layer d(5) declared
    /// under b with content e(6).
    fn tree(listening: &[(u64, &str)]) -> RetainedTree {
        let mut tree = RetainedTree::new();
        for (id, kind) in [
            (1, "div"),
            (2, "div"),
            (3, "div"),
            (4, "div"),
            (5, "anchored"),
            (6, "div"),
        ] {
            tree.create_element(id, kind.to_string());
        }
        tree.set_root(1);
        for (parent, child) in [(1, 2), (2, 3), (1, 4), (3, 5), (5, 6)] {
            tree.append_child(parent, child);
        }
        for (id, event_type) in listening {
            tree.set_event_listener(*id, event_type.to_string(), true);
        }
        tree
    }

    fn hit(id: u64, x: f32, y: f32) -> Hit {
        Hit {
            id,
            bounds: Bounds::new(point(px(x), px(y)), size(px(100.0), px(100.0))),
        }
    }

    fn at(action: PointerAction, hits: Vec<Hit>) -> PointerSample {
        PointerSample {
            action,
            position: (15.0, 30.0),
            movement: (0.0, 0.0),
            modifiers: EventModifiers::default(),
            hits,
            over_drag_region: false,
        }
    }

    fn chain_of(ids: &[u64]) -> Vec<Hit> {
        ids.iter().map(|id| hit(*id, 10.0, 20.0)).collect()
    }

    fn mv(hits: Vec<Hit>) -> PointerSample {
        at(
            PointerAction::Move {
                pressed_button: None,
            },
            hits,
        )
    }

    fn down(button: u32, click_count: u32, hits: Vec<Hit>) -> PointerSample {
        at(
            PointerAction::Down {
                button,
                click_count,
            },
            hits,
        )
    }

    fn up(button: u32, click_count: u32, hits: Vec<Hit>) -> PointerSample {
        at(
            PointerAction::Up {
                button,
                click_count,
            },
            hits,
        )
    }

    fn names(output: &Output) -> Vec<String> {
        output
            .events
            .iter()
            .map(|event| format!("{}@{}", event.event_type, event.target()))
            .collect()
    }

    const POINTER_BOUNDARIES: [&str; 8] = [
        "pointerover",
        "pointerout",
        "pointerenter",
        "pointerleave",
        "mouseover",
        "mouseout",
        "mouseenter",
        "mouseleave",
    ];

    fn listening_everywhere(types: &[&'static str]) -> Vec<(u64, &'static str)> {
        (1..=6)
            .flat_map(|id| types.iter().map(move |event_type| (id, *event_type)))
            .collect()
    }

    #[test]
    fn an_event_nobody_listens_for_is_not_produced() {
        let tree = tree(&[(1, "mousedown")]);
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        assert!(output.events.is_empty());
        assert!(state.is_hovered(3) && state.is_hovered(2) && state.is_hovered(1));
    }

    #[test]
    fn an_event_carries_the_target_and_its_retained_ancestors() {
        let tree = tree(&[(1, "mousedown")]);
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        assert_eq!(output.events.len(), 1);
        assert_eq!(output.events[0].path, [3, 2, 1]);
        assert_eq!(output.events[0].target(), 3);
    }

    #[test]
    fn the_hit_chain_and_the_retained_path_agree_for_every_shape_in_the_tree() {
        let tree = tree(&[(1, "mousedown")]);
        let mut state = PointerState::default();
        for chain in [
            vec![3, 2, 1],
            vec![4, 1],
            vec![6, 5, 3, 2, 1],
            vec![6, 3, 1],
        ] {
            let output = dispatch(&mut state, &tree, &down(0, 1, chain_of(&chain)));
            assert!(!output.chain_mismatch, "{chain:?}");
        }
        let wrong = dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 4, 1])));
        assert!(wrong.chain_mismatch);
    }

    #[test]
    fn an_anchored_layer_bubbles_to_the_element_it_is_declared_under() {
        let tree = tree(&[(2, "mousedown")]);
        let mut state = PointerState::default();
        // GPUI's chain for the layer: its content, then the hitbox that deferred it.
        let output = dispatch(&mut state, &tree, &down(0, 1, chain_of(&[6, 5, 3, 2, 1])));
        assert_eq!(output.events[0].path, [6, 5, 3, 2, 1]);
    }

    #[test]
    fn pointer_events_come_before_mouse_events() {
        let tree = tree(&listening_everywhere(&["pointerdown", "mousedown"]));
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["pointerdown@3", "mousedown@3"]);
    }

    #[test]
    fn entering_fires_over_then_enter_outermost_first() {
        let tree = tree(&listening_everywhere(&POINTER_BOUNDARIES));
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        assert_eq!(
            names(&output),
            [
                "pointerover@3",
                "pointerenter@1",
                "pointerenter@2",
                "pointerenter@3",
                "mouseover@3",
                "mouseenter@1",
                "mouseenter@2",
                "mouseenter@3",
            ]
        );
        assert!(output
            .events
            .iter()
            .all(|event| event.related_target.is_none()));
    }

    #[test]
    fn moving_to_a_sibling_fires_all_pointer_boundary_events_then_all_mouse_ones() {
        let tree = tree(&listening_everywhere(&POINTER_BOUNDARIES));
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        let output = dispatch(&mut state, &tree, &mv(chain_of(&[4, 1])));
        assert_eq!(
            names(&output),
            [
                "pointerout@3",
                "pointerleave@3",
                "pointerleave@2",
                "pointerover@4",
                "pointerenter@4",
                "mouseout@3",
                "mouseleave@3",
                "mouseleave@2",
                "mouseover@4",
                "mouseenter@4",
            ]
        );
        let out = &output.events[0];
        assert_eq!(out.related_target, Some(4));
        let over = &output.events[3];
        assert_eq!(over.related_target, Some(3));
        assert_eq!(output.events[8].related_target, Some(3));
        assert!(!state.is_hovered(3) && state.is_hovered(4) && state.is_hovered(1));
    }

    #[test]
    fn enter_is_dispatched_once_per_entered_element_with_its_own_path() {
        let tree = tree(&[(1, "mouseover"), (1, "mouseenter"), (3, "mouseenter")]);
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        assert_eq!(
            names(&output),
            ["mouseover@3", "mouseenter@1", "mouseenter@3"]
        );
        assert_eq!(output.events[0].path, [3, 2, 1]);
        assert_eq!(output.events[1].path, [1]);
        assert_eq!(output.events[2].path, [3, 2, 1]);
    }

    #[test]
    fn a_bubble_listener_above_does_not_make_native_produce_enter_and_leave_below() {
        let tree = tree(&[(1, "mouseenter"), (1, "mouseleave"), (1, "pointerenter")]);
        let mut state = PointerState::default();
        let entered = dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        assert_eq!(names(&entered), ["pointerenter@1", "mouseenter@1"]);
        let left = dispatch(&mut state, &tree, &mv(chain_of(&[4, 1])));
        assert!(left.events.is_empty());
        let exited = dispatch(&mut state, &tree, &at(PointerAction::Exit, Vec::new()));
        assert_eq!(names(&exited), ["mouseleave@1"]);
    }

    #[test]
    fn a_capture_listener_above_hears_enter_and_leave_below() {
        let tree = tree(&[
            (1, "mouseenterCapture"),
            (1, "pointerleaveCapture"),
            (3, "mouseleaveCapture"),
        ]);
        let mut state = PointerState::default();
        let entered = dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        assert_eq!(
            names(&entered),
            ["mouseenter@1", "mouseenter@2", "mouseenter@3"]
        );
        let left = dispatch(&mut state, &tree, &mv(chain_of(&[4, 1])));
        assert_eq!(
            names(&left),
            ["pointerleave@3", "pointerleave@2", "mouseleave@3", "mouseenter@4"]
        );
    }

    #[test]
    fn an_elements_own_capture_listener_hears_its_enter() {
        let tree = tree(&[(3, "mouseenterCapture")]);
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["mouseenter@3"]);
    }

    #[test]
    fn leaving_the_window_leaves_every_element() {
        let tree = tree(&listening_everywhere(&["mouseleave", "mouseout"]));
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        let output = dispatch(&mut state, &tree, &at(PointerAction::Exit, Vec::new()));
        assert_eq!(
            names(&output),
            ["mouseout@3", "mouseleave@3", "mouseleave@2", "mouseleave@1"]
        );
        assert!(output
            .events
            .iter()
            .all(|event| event.related_target.is_none()));
        assert!(!state.is_hovered(1));
    }

    #[test]
    fn an_element_removed_under_the_pointer_gets_no_boundary_events() {
        let mut tree = tree(&listening_everywhere(&["mouseleave", "mouseout"]));
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        tree.destroy_element(3);
        let output = dispatch(&mut state, &tree, &mv(chain_of(&[2, 1])));
        assert!(output.events.is_empty());
    }

    #[test]
    fn a_click_lands_on_the_common_ancestor_of_press_and_release() {
        let tree = tree(&listening_everywhere(&["click", "mouseup"]));
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[4, 1])));
        assert_eq!(names(&output), ["mouseup@4", "click@1"]);
    }

    #[test]
    fn a_click_inside_a_label_with_a_control_is_emitted_with_no_listener() {
        let mut tree = RetainedTree::new();
        crate::renderer::apply_batch_to_tree(
            &mut tree,
            br#"[["createElement",1,"div"],["setRoot",1],
                ["createElement",2,"label"],["appendChild",1,2],["setCustomProp",2,"htmlFor","email"],
                ["createElement",3,"input"],["appendChild",1,3],["setCustomProp",3,"id","email"],
                ["createElement",4,"div"],["appendChild",2,4]]"#,
        )
        .unwrap();
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &down(0, 1, chain_of(&[4, 2, 1])));
        let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[4, 2, 1])));
        assert_eq!(names(&output), ["click@4"]);

        dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 1])));
        let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3, 1])));
        assert!(output.events.is_empty());
    }

    #[test]
    fn pressing_and_releasing_on_the_same_element_clicks_it() {
        let tree = tree(&listening_everywhere(&["click"]));
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["click@3"]);
        assert_eq!(output.events[0].path, [3, 2, 1]);
    }

    #[test]
    fn a_release_with_no_press_in_the_window_is_not_a_click() {
        let tree = tree(&listening_everywhere(&["click", "mouseup"]));
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["mouseup@3"]);
    }

    #[test]
    fn the_second_click_also_fires_dblclick_with_detail_two() {
        let tree = tree(&listening_everywhere(&["click", "dblclick"]));
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3, 2, 1])));
        dispatch(&mut state, &tree, &down(0, 2, chain_of(&[3, 2, 1])));
        let output = dispatch(&mut state, &tree, &up(0, 2, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["click@3", "dblclick@3"]);
        assert!(output.events.iter().all(|event| event.detail == Some(2)));
    }

    #[test]
    fn other_buttons_fire_auxclick_not_click() {
        let tree = tree(&listening_everywhere(&["click", "auxclick"]));
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &down(1, 1, chain_of(&[3, 2, 1])));
        let output = dispatch(&mut state, &tree, &up(1, 1, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["auxclick@3"]);
        assert_eq!(output.events[0].button, Some(1));
    }

    fn controls_tree(control_type: &str, disabled: bool) -> RetainedTree {
        let mut tree = RetainedTree::new();
        let batch = format!(
            r#"[["createElement",1,"div"],["setRoot",1],
                ["createElement",2,"{control_type}"],["appendChild",1,2],["setCustomProp",2,"disabled",{disabled}],
                ["createElement",3,"div"],["appendChild",2,3]]"#
        );
        crate::renderer::apply_batch_to_tree(&mut tree, batch.as_bytes()).unwrap();
        for id in [1, 2, 3] {
            for event_type in [
                "pointerdown", "mousedown", "pointerup", "mouseup", "click", "dblclick", "auxclick",
            ] {
                tree.set_event_listener(id, event_type.to_string(), true);
            }
        }
        tree
    }

    #[test]
    fn a_disabled_form_control_gets_pointer_and_mouse_events_but_no_click_family() {
        for control_type in ["button", "input", "textarea", "select"] {
            let tree = controls_tree(control_type, true);
            let mut state = PointerState::default();
            let press = dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
            assert_eq!(names(&press), ["pointerdown@3", "mousedown@3"], "{control_type}");
            let release = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3, 2, 1])));
            assert_eq!(names(&release), ["pointerup@3", "mouseup@3"], "{control_type}");

            dispatch(&mut state, &tree, &down(0, 2, chain_of(&[2, 1])));
            let release = dispatch(&mut state, &tree, &up(0, 2, chain_of(&[2, 1])));
            assert_eq!(names(&release), ["pointerup@2", "mouseup@2"], "{control_type}");

            dispatch(&mut state, &tree, &down(1, 1, chain_of(&[2, 1])));
            let release = dispatch(&mut state, &tree, &up(1, 1, chain_of(&[2, 1])));
            assert_eq!(names(&release), ["pointerup@2", "mouseup@2"], "{control_type}");
        }
    }

    #[test]
    fn a_click_landing_above_a_disabled_form_control_is_dispatched() {
        let tree = controls_tree("input", true);
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[1])));
        assert_eq!(names(&output), ["pointerup@1", "mouseup@1", "click@1"]);
    }

    #[test]
    fn enabled_controls_and_disabled_non_controls_still_click() {
        for (control_type, disabled) in [("input", false), ("button", false), ("div", true)] {
            let tree = controls_tree(control_type, disabled);
            let mut state = PointerState::default();
            dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
            let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3, 2, 1])));
            assert_eq!(
                names(&output),
                ["pointerup@3", "mouseup@3", "click@3"],
                "{control_type} disabled={disabled}"
            );
        }
    }

    #[test]
    fn a_secondary_press_opens_the_context_menu_after_mousedown() {
        let tree = tree(&listening_everywhere(&["mousedown", "contextmenu"]));
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &down(2, 1, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["mousedown@3", "contextmenu@3"]);
    }

    fn with_ctrl(sample: PointerSample) -> PointerSample {
        PointerSample {
            modifiers: EventModifiers {
                ctrl: true,
                ..EventModifiers::default()
            },
            ..sample
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn a_ctrl_primary_press_opens_the_context_menu_on_macos() {
        let tree = tree(&listening_everywhere(&["mousedown", "contextmenu"]));
        let mut state = PointerState::default();
        let output = dispatch(
            &mut state,
            &tree,
            &with_ctrl(down(0, 1, chain_of(&[3, 2, 1]))),
        );
        assert_eq!(names(&output), ["mousedown@3", "contextmenu@3"]);
        assert_eq!(output.events[1].button, Some(0));
    }

    #[test]
    fn a_primary_press_without_ctrl_opens_no_context_menu() {
        let tree = tree(&listening_everywhere(&["mousedown", "contextmenu"]));
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["mousedown@3"]);
    }

    #[test]
    fn a_ctrl_press_of_another_button_is_not_a_context_menu() {
        let tree = tree(&listening_everywhere(&["mousedown", "contextmenu"]));
        let mut state = PointerState::default();
        let output = dispatch(
            &mut state,
            &tree,
            &with_ctrl(down(1, 1, chain_of(&[3, 2, 1]))),
        );
        assert_eq!(names(&output), ["mousedown@3"]);
    }

    const BUTTON_EVENTS: [&str; 5] = ["pointerdown", "pointerup", "pointermove", "mousedown", "mouseup"];

    #[test]
    fn a_second_button_down_is_a_pointermove_with_the_new_buttons_and_a_mousedown() {
        let tree = tree(&listening_everywhere(&BUTTON_EVENTS));
        let mut state = PointerState::default();
        let first = dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        assert_eq!(names(&first), ["pointerdown@3", "mousedown@3"]);
        let second = dispatch(&mut state, &tree, &down(2, 1, chain_of(&[3, 2, 1])));
        assert_eq!(names(&second), ["pointermove@3", "mousedown@3"]);
        assert_eq!(second.events[0].button, Some(2));
        assert_eq!(second.events[0].pressed_button, Some(0));
        let payloads = second.into_payloads();
        assert_eq!(payloads[0].buttons, Some(3));
    }

    #[test]
    fn releasing_one_of_two_buttons_is_a_pointermove_and_the_last_release_is_the_pointerup() {
        let tree = tree(&listening_everywhere(&BUTTON_EVENTS));
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        dispatch(&mut state, &tree, &down(2, 1, chain_of(&[3, 2, 1])));
        let first = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3, 2, 1])));
        assert_eq!(names(&first), ["pointermove@3", "mouseup@3"]);
        assert_eq!(first.events[0].button, Some(0));
        assert_eq!(first.events[0].pressed_button, Some(2));
        assert_eq!(first.into_payloads()[0].buttons, Some(2));
        let last = dispatch(&mut state, &tree, &up(2, 1, chain_of(&[3, 2, 1])));
        assert_eq!(names(&last), ["pointerup@3", "mouseup@3"]);
    }

    #[test]
    fn the_button_that_goes_down_again_after_a_chord_ended_is_a_pointerdown() {
        let tree = tree(&listening_everywhere(&BUTTON_EVENTS));
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        dispatch(&mut state, &tree, &down(2, 1, chain_of(&[3, 2, 1])));
        dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3, 2, 1])));
        dispatch(&mut state, &tree, &up(2, 1, chain_of(&[3, 2, 1])));
        let again = dispatch(&mut state, &tree, &down(2, 1, chain_of(&[3, 2, 1])));
        assert_eq!(names(&again), ["pointerdown@3", "mousedown@3"]);
    }

    fn outside_presses(output: &Output) -> Vec<String> {
        output
            .events
            .iter()
            .filter(|event| event.event_type == "mousedownoutside")
            .map(|event| {
                format!(
                    "{}<-{}",
                    event.listener.expect("an outside press has a listener"),
                    event.target()
                )
            })
            .collect()
    }

    #[test]
    fn an_element_that_left_the_tree_hears_no_outside_press() {
        let mut tree = tree(&[(3, "mousedownoutside")]);
        tree.create_element(7, "div".to_string());
        tree.set_event_listener(7, "mousedownoutside".to_string(), true);
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &down(0, 1, chain_of(&[4, 1])));
        assert_eq!(outside_presses(&output), ["3<-4"]);
    }

    #[test]
    fn a_press_outside_the_subtree_reaches_the_listener_only_with_the_press_as_target() {
        let tree = tree(&[(3, "mousedownoutside")]);
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &down(0, 1, chain_of(&[4, 1])));
        assert_eq!(outside_presses(&output), ["3<-4"]);
        assert_eq!(output.events[0].path, [4, 1]);
        let payloads = output.into_payloads();
        assert_eq!(payloads[0].element_id, 3.0);
        assert_eq!(payloads[0].target, Some(4.0));
    }

    #[test]
    fn a_press_inside_the_element_or_its_anchored_layer_is_not_outside() {
        let tree = tree(&[(3, "mousedownoutside")]);
        let mut state = PointerState::default();
        // The layer (5, content 6) is declared under 3 but paints against the window.
        for chain in [vec![3, 2, 1], vec![6, 5, 3, 2, 1]] {
            let output = dispatch(&mut state, &tree, &down(0, 1, chain_of(&chain)));
            assert!(outside_presses(&output).is_empty(), "{chain:?}");
        }
    }

    #[test]
    fn a_press_on_an_overlay_above_the_element_is_outside() {
        let tree = tree(&[(3, "mousedownoutside")]);
        let mut state = PointerState::default();
        // The overlay (4) paints over 3 but is not part of its subtree.
        let output = dispatch(&mut state, &tree, &down(0, 1, chain_of(&[4, 1])));
        assert_eq!(outside_presses(&output), ["3<-4"]);
    }

    #[test]
    fn an_ancestor_listener_is_not_outside_a_press_on_its_descendant() {
        let tree = tree(&[(1, "mousedownoutside"), (4, "mousedownoutside")]);
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &down(0, 1, chain_of(&[6, 5, 3, 2, 1])));
        assert_eq!(outside_presses(&output), ["4<-6"]);
    }

    #[test]
    fn a_press_that_hits_no_element_is_outside_every_listener_and_targets_the_root() {
        let tree = tree(&[
            (1, "mousedownoutside"),
            (3, "mousedownoutside"),
            (4, "mousedownoutside"),
            (3, "mousedown"),
        ]);
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &down(0, 1, Vec::new()));
        assert_eq!(outside_presses(&output), ["1<-1", "3<-1", "4<-1"]);
        assert_eq!(
            names(&output),
            ["mousedownoutside@1", "mousedownoutside@1", "mousedownoutside@1"],
            "no element was pressed, so nothing else fires"
        );
        let payloads = output.into_payloads();
        assert_eq!(payloads[1].element_id, 3.0);
        assert_eq!(payloads[1].target, Some(1.0));
        assert_eq!(payloads[1].path, Some(vec![1.0]));
    }

    #[test]
    fn a_press_on_nothing_with_no_outside_listener_emits_nothing() {
        let tree = tree(&[(3, "mousedown")]);
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &down(0, 1, Vec::new()));
        assert!(output.events.is_empty());
    }

    #[test]
    fn every_button_fires_an_outside_press_after_mousedown() {
        let tree = tree(&[(4, "mousedownoutside"), (3, "mousedown")]);
        let mut state = PointerState::default();
        for button in [0, 1, 2] {
            let output = dispatch(&mut state, &tree, &down(button, 1, chain_of(&[3, 2, 1])));
            assert_eq!(
                names(&output),
                ["mousedown@3", "mousedownoutside@3"],
                "button {button}"
            );
            assert_eq!(output.events[1].button, Some(button));
            dispatch(&mut state, &tree, &up(button, 1, chain_of(&[3, 2, 1])));
        }
    }

    #[test]
    fn buttons_is_the_bitmask_of_what_is_held() {
        let tree = tree(&listening_everywhere(&["mousedown", "mouseup"]));
        let mut state = PointerState::default();
        let pressed = dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3])));
        let both = dispatch(&mut state, &tree, &down(2, 1, chain_of(&[3])));
        let released = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3])));
        let payload_buttons = |output: Output| output.into_payloads()[0].buttons;
        assert_eq!(payload_buttons(pressed), Some(1));
        assert_eq!(payload_buttons(both), Some(3));
        assert_eq!(payload_buttons(released), Some(2));
    }

    #[test]
    fn offset_is_measured_from_the_padding_edge() {
        let mut tree = tree(&[(3, "mousemove")]);
        let style: StyleDesc = serde_json::from_str(r#"{"borderWidth":2}"#).unwrap();
        tree.set_style(3, Arc::new(style));
        let mut state = PointerState::default();
        // The border box starts at (10, 20); the pointer is at (15, 30).
        let output = dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        assert_eq!(output.events[0].offset, Some((3.0, 8.0)));
        let payload = &output.into_payloads()[0];
        assert_eq!((payload.offset_x, payload.offset_y), (Some(3.0), Some(8.0)));
        assert_eq!(
            (payload.client_x, payload.client_y),
            (Some(15.0), Some(30.0))
        );
        assert_eq!(payload.pointer_type.as_deref(), Some("mouse"));
        assert_eq!(payload.pointer_id, Some(1));
    }

    #[test]
    fn a_press_and_move_listener_captures_nothing_without_an_explicit_capture() {
        let tree = tree(&[
            (2, "mousedown"),
            (2, "mousemove"),
            (2, "mouseup"),
            (4, "mousemove"),
        ]);
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        assert_eq!(state.explicit_capture(), None);

        let output = dispatch(&mut state, &tree, &drag_move(chain_of(&[4, 1])));
        assert_eq!(
            names(&output),
            ["mousemove@4"],
            "the move goes to what is under the pointer, not to the pressed element"
        );
        assert!(state.is_hovered(4));

        let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[4, 1])));
        assert!(
            output.events.is_empty(),
            "the press element hears nothing outside itself"
        );
    }

    #[test]
    fn a_click_on_a_child_is_not_swallowed_by_a_press_and_move_listener() {
        let tree = tree(&[
            (2, "mousedown"),
            (2, "mousemove"),
            (2, "mouseup"),
            (3, "click"),
        ]);
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["mouseup@3", "click@3"]);
    }

    #[test]
    fn the_primary_press_is_active_until_release() {
        let mut tree = tree(&[]);
        let style: StyleDesc = serde_json::from_str(r#"{"active":{"opacity":0.5}}"#).unwrap();
        tree.set_style(2, Arc::new(style));
        let mut state = PointerState::default();
        let pressed = dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        assert!(pressed.restyle);
        assert!(state.is_active(3) && state.is_active(2) && state.is_active(1));
        let released = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3, 2, 1])));
        assert!(released.restyle);
        assert!(!state.is_active(2));
    }

    #[test]
    fn only_a_hover_style_asks_for_a_restyle() {
        let mut tree = tree(&[]);
        let style: StyleDesc = serde_json::from_str(r#"{"hover":{"opacity":0.5}}"#).unwrap();
        tree.set_style(2, Arc::new(style));
        let mut state = PointerState::default();
        assert!(dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1]))).restyle);
        assert!(!dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1]))).restyle);
        assert!(dispatch(&mut state, &tree, &mv(chain_of(&[4, 1]))).restyle);
        assert!(!dispatch(&mut state, &tree, &mv(chain_of(&[1]))).restyle);
    }

    fn drag_move(hits: Vec<Hit>) -> PointerSample {
        at(
            PointerAction::Move {
                pressed_button: Some(0),
            },
            hits,
        )
    }

    const CAPTURE_EVENTS: [&str; 7] = [
        "gotpointercapture",
        "lostpointercapture",
        "pointermove",
        "pointerup",
        "mouseup",
        "pointerover",
        "pointerout",
    ];

    /// Pressed on b(3) with capture requested for c(4), before the next input.
    fn pressed_with_capture_on_c(tree: &RetainedTree) -> PointerState {
        let mut state = PointerState::default();
        dispatch(&mut state, tree, &down(0, 1, chain_of(&[3, 2, 1])));
        state.requests.set(tree, 4, 1.0).unwrap();
        state
    }

    #[test]
    fn explicit_capture_retargets_pointer_events_to_the_capturing_element() {
        let tree = tree(&listening_everywhere(&[
            "pointermove",
            "mousemove",
            "mouseup",
        ]));
        let mut state = pressed_with_capture_on_c(&tree);
        let output = dispatch(&mut state, &tree, &drag_move(chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["pointermove@4", "mousemove@4"]);
        assert_eq!(output.events[0].path, [4, 1]);
        let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["mouseup@4"]);
    }

    #[test]
    fn capture_applies_from_the_next_input_with_got_first_and_lost_after_the_up() {
        let tree = tree(&listening_everywhere(&CAPTURE_EVENTS));
        let mut state = pressed_with_capture_on_c(&tree);
        let output = dispatch(&mut state, &tree, &drag_move(chain_of(&[3, 2, 1])));
        assert_eq!(
            names(&output),
            [
                "gotpointercapture@4",
                "pointerout@3",
                "pointerover@4",
                "pointermove@4"
            ]
        );
        assert_eq!(state.explicit_capture(), Some(4));
        let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3, 2, 1])));
        assert_eq!(
            names(&output),
            [
                "pointerup@4",
                "lostpointercapture@4",
                "mouseup@4",
                "pointerout@4",
                "pointerover@3"
            ]
        );
        assert_eq!(state.explicit_capture(), None);
        assert_eq!(state.requests.target(), None);
    }

    #[test]
    fn the_release_fires_pointerup_then_lostpointercapture_then_mouseup_then_click() {
        let tree = tree(&listening_everywhere(&[
            "pointerup",
            "lostpointercapture",
            "mouseup",
            "click",
        ]));
        let mut state = pressed_with_capture_on_c(&tree);
        dispatch(&mut state, &tree, &drag_move(chain_of(&[3, 2, 1])));
        let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3, 2, 1])));
        assert_eq!(
            names(&output),
            ["pointerup@4", "lostpointercapture@4", "mouseup@4", "click@1"]
        );
    }

    #[test]
    fn hover_and_boundary_events_follow_the_capture_target() {
        let tree = tree(&listening_everywhere(&POINTER_BOUNDARIES));
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        state.requests.set(&tree, 4, 1.0).unwrap();
        let output = dispatch(&mut state, &tree, &drag_move(chain_of(&[3, 2, 1])));
        assert_eq!(
            names(&output),
            [
                "pointerout@3",
                "pointerleave@3",
                "pointerleave@2",
                "pointerover@4",
                "pointerenter@4",
                "mouseout@3",
                "mouseleave@3",
                "mouseleave@2",
                "mouseover@4",
                "mouseenter@4",
            ]
        );
        assert!(state.is_hovered(4) && !state.is_hovered(3) && !state.is_hovered(2));
        // The pointer wandering over b does not move hover off the captured element.
        let output = dispatch(&mut state, &tree, &drag_move(chain_of(&[3, 2, 1])));
        assert!(output.events.is_empty());
    }

    #[test]
    fn releasing_from_js_loses_the_capture_at_the_next_input() {
        let tree = tree(&listening_everywhere(&CAPTURE_EVENTS));
        let mut state = pressed_with_capture_on_c(&tree);
        dispatch(&mut state, &tree, &drag_move(chain_of(&[3, 2, 1])));
        state.requests.release(4, 1.0).unwrap();
        let output = dispatch(&mut state, &tree, &drag_move(chain_of(&[3, 2, 1])));
        assert_eq!(
            names(&output),
            [
                "lostpointercapture@4",
                "pointerout@4",
                "pointerover@3",
                "pointermove@3"
            ]
        );
    }

    #[test]
    fn capture_ends_without_an_event_when_the_element_is_removed() {
        let mut tree = tree(&listening_everywhere(&CAPTURE_EVENTS));
        let mut state = pressed_with_capture_on_c(&tree);
        dispatch(&mut state, &tree, &drag_move(chain_of(&[3, 2, 1])));
        tree.destroy_element(4);
        let output = dispatch(&mut state, &tree, &drag_move(chain_of(&[3, 2, 1])));
        assert!(!names(&output).iter().any(|name| name.starts_with("lost")));
        assert_eq!(names(&output).last().unwrap(), "pointermove@3");
        assert_eq!(state.explicit_capture(), None);
        assert_eq!(state.requests.target(), None);
    }

    #[test]
    fn capture_survives_until_the_last_button_is_up() {
        let tree = tree(&listening_everywhere(&["lostpointercapture"]));
        let mut state = pressed_with_capture_on_c(&tree);
        dispatch(&mut state, &tree, &drag_move(chain_of(&[3, 2, 1])));
        dispatch(&mut state, &tree, &down(2, 1, chain_of(&[3, 2, 1])));
        let output = dispatch(&mut state, &tree, &up(2, 1, chain_of(&[3, 2, 1])));
        assert!(output.events.is_empty());
        assert_eq!(state.explicit_capture(), Some(4));
        let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["lostpointercapture@4"]);
    }

    #[test]
    fn a_move_with_no_button_after_a_press_cancels_it_then_loses_the_capture() {
        let mut listening = listening_everywhere(&[
            "pointercancel",
            "lostpointercapture",
            "pointerup",
            "mouseup",
            "click",
        ]);
        listening.extend(listening_everywhere(&["pointermove"]));
        let tree = tree(&listening);
        let mut state = pressed_with_capture_on_c(&tree);
        dispatch(&mut state, &tree, &drag_move(chain_of(&[3, 2, 1])));
        assert_eq!(state.explicit_capture(), Some(4));

        let output = dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));

        assert_eq!(
            names(&output),
            ["pointercancel@4", "lostpointercapture@4", "pointermove@3"]
        );
        assert_eq!(state.explicit_capture(), None);
        assert_eq!(state.requests.target(), None);
        assert!(state.pressed.is_empty());
        assert!(!state.is_active(3));
        assert_eq!(state.buttons, 0);

        let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["pointerup@3", "mouseup@3"], "no click");
    }

    #[test]
    fn a_lost_release_cancels_at_the_element_a_pending_capture_names() {
        let mut listening = listening_everywhere(&["pointercancel"]);
        listening.extend(listening_everywhere(&CAPTURE_EVENTS));
        let tree = tree(&listening);
        let mut state = pressed_with_capture_on_c(&tree);
        assert_eq!(state.explicit_capture(), None, "the capture is pending");

        let output = dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));

        // The capture never took hold, so there is nothing to lose or gain.
        assert_eq!(
            names(&output),
            ["pointercancel@4", "pointermove@3"],
            "pointercancel goes to the effective target, the pending capture"
        );
        assert_eq!(state.explicit_capture(), None);
        assert_eq!(state.requests.target(), None);
    }

    #[test]
    fn a_lost_release_without_capture_cancels_at_the_pressed_element() {
        let tree = tree(&listening_everywhere(&["pointercancel", "lostpointercapture"]));
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        assert!(state.is_active(3));

        let output = dispatch(&mut state, &tree, &mv(chain_of(&[4, 1])));

        assert_eq!(names(&output), ["pointercancel@3"]);
        assert!(!state.is_active(3));
    }

    #[test]
    fn a_move_with_no_button_while_none_is_down_cancels_nothing() {
        let tree = tree(&listening_everywhere(&["pointercancel"]));
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        assert!(output.events.is_empty());
    }

    #[test]
    fn a_move_does_not_decide_which_buttons_are_held() {
        let tree = tree(&listening_everywhere(&["mousedown", "mousemove", "mouseup"]));
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3])));
        dispatch(&mut state, &tree, &down(2, 1, chain_of(&[3])));

        // GPUI names one pressed button on a move: the other is still down.
        let output = dispatch(
            &mut state,
            &tree,
            &at(PointerAction::Move { pressed_button: Some(0) }, chain_of(&[3])),
        );
        assert_eq!(output.into_payloads()[0].buttons, Some(3));

        let released = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3])));
        assert_eq!(released.into_payloads()[0].buttons, Some(2));
    }

    #[test]
    fn a_resync_updates_hover_without_a_move() {
        let tree = tree(&listening_everywhere(&["mousemove", "mouseover"]));
        let mut state = PointerState::default();
        let output = dispatch(
            &mut state,
            &tree,
            &at(PointerAction::Resync, chain_of(&[4, 1])),
        );
        assert_eq!(names(&output), ["mouseover@4"]);
    }

    /// A file only this call writes, so tests running in parallel do not share it.
    fn dragged_file() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "gpuix-dispatcher-drag-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("drag.txt");
        std::fs::write(&path, b"abc").unwrap();
        path
    }

    fn file_drag_move(paths: Vec<PathBuf>, hits: Vec<Hit>) -> PointerSample {
        at(PointerAction::FileDragMove { paths }, hits)
    }

    #[test]
    fn a_file_drag_enters_then_hovers_the_target_whoever_listens() {
        let tree = tree(&[]);
        let mut state = PointerState::default();
        let output = dispatch(
            &mut state,
            &tree,
            &file_drag_move(vec![dragged_file()], chain_of(&[3, 2, 1])),
        );
        assert_eq!(names(&output), ["dragenter@3", "dragover@3"]);
        let files = output.events[0].files.as_ref().expect("the first event carries the files");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "drag.txt");
        assert_eq!(files[0].size, 3.0);
        assert_eq!(files[0].mime_type, "text/plain");
        assert!(output.events[1].files.is_none());
        assert_eq!(output.events[0].related_target, None);
    }

    #[test]
    fn a_drag_of_unreadable_files_is_not_read_again_on_every_move() {
        let tree = tree(&[]);
        let mut state = PointerState::default();
        let missing = std::env::temp_dir().join("gpuix-no-such-drag-file.txt");
        let first = dispatch(
            &mut state,
            &tree,
            &file_drag_move(vec![missing.clone()], chain_of(&[3, 2, 1])),
        );
        assert!(first.events.is_empty());
        // The file appearing mid-drag does not start a session: the drag was
        // judged once, when it entered.
        let readable = dragged_file();
        let second = dispatch(
            &mut state,
            &tree,
            &file_drag_move(vec![readable.clone()], chain_of(&[3, 2, 1])),
        );
        assert!(second.events.is_empty());
        dispatch(&mut state, &tree, &at(PointerAction::FileDragLeave, Vec::new()));
        let next_drag = dispatch(
            &mut state,
            &tree,
            &file_drag_move(vec![readable], chain_of(&[3, 2, 1])),
        );
        assert_eq!(names(&next_drag), ["dragenter@3", "dragover@3"]);
    }

    #[test]
    fn a_move_inside_the_target_is_a_dragover_alone() {
        let tree = tree(&[]);
        let mut state = PointerState::default();
        let paths = vec![dragged_file()];
        dispatch(&mut state, &tree, &file_drag_move(paths.clone(), chain_of(&[3, 2, 1])));
        let output = dispatch(&mut state, &tree, &file_drag_move(paths, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["dragover@3"]);
    }

    #[test]
    fn a_move_to_another_element_enters_it_before_leaving_the_old_one() {
        let tree = tree(&[]);
        let mut state = PointerState::default();
        let paths = vec![dragged_file()];
        dispatch(&mut state, &tree, &file_drag_move(paths.clone(), chain_of(&[3, 2, 1])));
        let output = dispatch(&mut state, &tree, &file_drag_move(paths, chain_of(&[4, 1])));
        assert_eq!(names(&output), ["dragenter@4", "dragleave@3", "dragover@4"]);
        assert_eq!(output.events[0].related_target, Some(3));
        assert_eq!(output.events[1].related_target, Some(4));
        assert!(output.events[0].files.is_none());
    }

    #[test]
    fn exiting_leaves_the_target_with_no_related_target() {
        let tree = tree(&[]);
        let mut state = PointerState::default();
        dispatch(
            &mut state,
            &tree,
            &file_drag_move(vec![dragged_file()], chain_of(&[3, 2, 1])),
        );
        let output = dispatch(&mut state, &tree, &at(PointerAction::FileDragLeave, vec![]));
        assert_eq!(names(&output), ["dragleave@3"]);
        assert_eq!(output.events[0].related_target, None);
        let output = dispatch(&mut state, &tree, &at(PointerAction::FileDragLeave, vec![]));
        assert!(output.events.is_empty(), "a second leave has nothing to leave");
    }

    #[test]
    fn a_drop_is_delivered_at_the_target_and_ends_the_drag() {
        let tree = tree(&[]);
        let mut state = PointerState::default();
        dispatch(
            &mut state,
            &tree,
            &file_drag_move(vec![dragged_file()], chain_of(&[3, 2, 1])),
        );
        let output = dispatch(&mut state, &tree, &at(PointerAction::FileDrop, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["drop@3"]);
        assert_eq!(output.events[0].path, [3, 2, 1]);
        let output = dispatch(&mut state, &tree, &at(PointerAction::FileDragLeave, vec![]));
        assert!(output.events.is_empty(), "the Exited after a drop is not a leave");
    }

    #[test]
    fn a_drop_on_another_element_enters_it_first() {
        let tree = tree(&[]);
        let mut state = PointerState::default();
        dispatch(
            &mut state,
            &tree,
            &file_drag_move(vec![dragged_file()], chain_of(&[3, 2, 1])),
        );
        let output = dispatch(&mut state, &tree, &at(PointerAction::FileDrop, chain_of(&[4, 1])));
        assert_eq!(names(&output), ["dragenter@4", "dragleave@3", "drop@4"]);
    }

    #[test]
    fn a_file_drag_over_nothing_targets_the_root() {
        let tree = tree(&[]);
        let mut state = PointerState::default();
        let output = dispatch(
            &mut state,
            &tree,
            &file_drag_move(vec![dragged_file()], vec![]),
        );
        assert_eq!(names(&output), ["dragenter@1", "dragover@1"]);
    }

    #[test]
    fn no_pointer_or_mouse_event_fires_during_a_file_drag() {
        let tree = tree(&listening_everywhere(&[
            "pointermove",
            "mousemove",
            "pointerover",
            "mouseover",
            "pointerenter",
            "mouseenter",
            "pointerout",
            "mouseout",
            "mouseup",
            "pointerup",
            "click",
        ]));
        let mut state = PointerState::default();
        let paths = vec![dragged_file()];
        let mut all = Vec::new();
        for sample in [
            file_drag_move(paths.clone(), chain_of(&[3, 2, 1])),
            file_drag_move(paths, chain_of(&[4, 1])),
            at(PointerAction::Resync, chain_of(&[3, 2, 1])),
            at(PointerAction::Exit, vec![]),
            at(PointerAction::FileDrop, chain_of(&[3, 2, 1])),
        ] {
            all.extend(names(&dispatch(&mut state, &tree, &sample)));
        }
        assert!(
            all.iter().all(|name| name.starts_with("drag") || name.starts_with("drop")),
            "{all:?}"
        );
        assert_eq!(state.hover_target(), None, "the drag did not hover anything");
    }

    #[test]
    fn a_drag_with_nothing_readable_has_no_events() {
        let tree = tree(&[]);
        let mut state = PointerState::default();
        let missing = std::env::temp_dir().join("gpuix-dispatcher-missing.txt");
        std::fs::remove_file(&missing).ok();
        for paths in [vec![], vec![missing]] {
            let output = dispatch(&mut state, &tree, &file_drag_move(paths, chain_of(&[3, 2, 1])));
            assert!(output.events.is_empty());
        }
        let output = dispatch(&mut state, &tree, &at(PointerAction::FileDrop, chain_of(&[3, 2, 1])));
        assert!(output.events.is_empty());
    }

    #[test]
    fn a_real_pointer_input_ends_a_drag_that_never_left() {
        let tree = tree(&listening_everywhere(&["mousemove"]));
        let mut state = PointerState::default();
        dispatch(
            &mut state,
            &tree,
            &file_drag_move(vec![dragged_file()], chain_of(&[3, 2, 1])),
        );
        let output = dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["dragleave@3", "mousemove@3"]);
    }

    #[test]
    fn a_resync_during_a_file_drag_moves_the_drag_target() {
        let tree = tree(&[]);
        let mut state = PointerState::default();
        dispatch(
            &mut state,
            &tree,
            &file_drag_move(vec![dragged_file()], chain_of(&[3, 2, 1])),
        );
        let output = dispatch(&mut state, &tree, &at(PointerAction::Resync, chain_of(&[4, 1])));
        assert_eq!(names(&output), ["dragenter@4", "dragleave@3"]);
    }

    /// `tree` with a(2) draggable: b(3) is inside it, c(4) is not.
    fn draggable_tree(listening: &[(u64, &str)]) -> RetainedTree {
        let mut tree = tree(listening);
        tree.set_custom_prop(2, "draggable".to_string(), serde_json::json!(true));
        tree
    }

    fn pointer_at(action: PointerAction, hits: Vec<Hit>, x: f64, y: f64) -> PointerSample {
        PointerSample {
            position: (x, y),
            ..at(action, hits)
        }
    }

    fn held_move(x: f64, y: f64, hits: Vec<Hit>) -> PointerSample {
        pointer_at(
            PointerAction::Move {
                pressed_button: Some(0),
            },
            hits,
            x,
            y,
        )
    }

    /// Pressed on b(3), inside the draggable a(2), at (15, 30).
    fn pressed_on_b(tree: &RetainedTree) -> PointerState {
        let mut state = PointerState::default();
        dispatch(&mut state, tree, &mv(chain_of(&[3, 2, 1])));
        dispatch(&mut state, tree, &down(0, 1, chain_of(&[3, 2, 1])));
        state
    }

    /// Pressed on b(3) and dragged to (30, 30): the session is running over b.
    fn dragging_from_a(tree: &RetainedTree) -> PointerState {
        let mut state = pressed_on_b(tree);
        dispatch(&mut state, tree, &held_move(30.0, 30.0, chain_of(&[3, 2, 1])));
        assert!(state.element_drag.is_some());
        state
    }

    const DRAG_EVENTS: [&str; 7] = [
        "dragstart",
        "drag",
        "dragend",
        "dragenter",
        "dragover",
        "dragleave",
        "drop",
    ];

    #[test]
    fn a_press_that_moves_no_more_than_four_pixels_is_not_a_drag() {
        let tree = draggable_tree(&listening_everywhere(&["pointermove", "dragstart"]));
        let mut state = pressed_on_b(&tree);
        let output = dispatch(&mut state, &tree, &held_move(19.0, 30.0, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["pointermove@3"]);
        let output = dispatch(&mut state, &tree, &held_move(18.0, 32.0, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["pointermove@3"]);
    }

    #[test]
    fn moving_further_than_four_pixels_starts_the_drag() {
        let tree = draggable_tree(&listening_everywhere(&["pointermove", "dragstart"]));
        let mut state = pressed_on_b(&tree);
        let output = dispatch(&mut state, &tree, &held_move(19.1, 30.0, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["dragstart@2", "dragenter@3", "dragover@3"]);
        assert!(state.element_drag.is_some());
    }

    #[test]
    fn the_threshold_is_the_distance_from_the_press_not_from_the_last_move() {
        let tree = draggable_tree(&listening_everywhere(&["pointermove", "dragstart"]));
        let mut state = pressed_on_b(&tree);
        for x in [17.0, 19.0] {
            let output = dispatch(&mut state, &tree, &held_move(x, 30.0, chain_of(&[3, 2, 1])));
            assert_eq!(names(&output), ["pointermove@3"]);
        }
        let output = dispatch(&mut state, &tree, &held_move(20.0, 30.0, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["dragstart@2", "dragenter@3", "dragover@3"]);
    }

    #[test]
    fn the_source_is_the_nearest_draggable_ancestor_of_the_press_target() {
        let mut tree = draggable_tree(&listening_everywhere(&["dragstart"]));
        let mut state = pressed_on_b(&tree);
        let output = dispatch(&mut state, &tree, &held_move(30.0, 30.0, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["dragstart@2", "dragenter@3", "dragover@3"]);
        assert_eq!(output.events[0].path, [2, 1]);

        // A draggable press target is its own source, ahead of its ancestor.
        tree.set_custom_prop(3, "draggable".to_string(), serde_json::json!(true));
        let mut state = pressed_on_b(&tree);
        let output = dispatch(&mut state, &tree, &held_move(30.0, 30.0, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["dragstart@3", "dragenter@3", "dragover@3"]);
    }

    #[test]
    fn a_press_outside_every_draggable_never_drags() {
        let tree = draggable_tree(&listening_everywhere(&["pointermove", "dragstart"]));
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &down(0, 1, chain_of(&[4, 1])));
        let output = dispatch(&mut state, &tree, &held_move(90.0, 90.0, chain_of(&[4, 1])));
        assert_eq!(names(&output), ["pointermove@4"]);
    }

    #[test]
    fn draggable_false_is_not_a_source() {
        let mut tree = draggable_tree(&listening_everywhere(&["dragstart", "pointermove"]));
        tree.set_custom_prop(2, "draggable".to_string(), serde_json::json!(false));
        let mut state = pressed_on_b(&tree);
        let output = dispatch(&mut state, &tree, &held_move(90.0, 90.0, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["pointermove@3"]);
    }

    #[test]
    fn only_the_primary_button_drags() {
        let tree = draggable_tree(&listening_everywhere(&["dragstart", "pointermove"]));
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &down(2, 1, chain_of(&[3, 2, 1])));
        let held = PointerAction::Move {
            pressed_button: Some(2),
        };
        let output = dispatch(
            &mut state,
            &tree,
            &pointer_at(held, chain_of(&[3, 2, 1]), 90.0, 90.0),
        );
        assert_eq!(names(&output), ["pointermove@3"]);
    }

    #[test]
    fn the_start_cancels_the_pointer_then_drags_in_dom_order() {
        let mut listening = listening_everywhere(&DRAG_EVENTS);
        listening.extend(listening_everywhere(&POINTER_BOUNDARIES));
        listening.extend(listening_everywhere(&["pointercancel"]));
        let tree = draggable_tree(&listening);
        let mut state = pressed_on_b(&tree);
        let output = dispatch(&mut state, &tree, &held_move(30.0, 30.0, chain_of(&[3, 2, 1])));
        assert_eq!(
            names(&output),
            [
                "pointercancel@3",
                "pointerout@3",
                "pointerleave@3",
                "pointerleave@2",
                "pointerleave@1",
                "mouseout@3",
                "mouseleave@3",
                "mouseleave@2",
                "mouseleave@1",
                "dragstart@2",
                "drag@2",
                "dragenter@3",
                "dragover@3",
            ]
        );
        assert!(!state.is_hovered(3), "the pointer left every element");
        assert!(!state.is_active(2));
    }

    #[test]
    fn each_move_sends_drag_to_the_source_then_the_boundary_and_dragover_events() {
        let tree = draggable_tree(&listening_everywhere(&DRAG_EVENTS));
        let mut state = dragging_from_a(&tree);

        let output = dispatch(&mut state, &tree, &held_move(31.0, 30.0, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["drag@2", "dragover@3"]);

        let output = dispatch(&mut state, &tree, &held_move(60.0, 30.0, chain_of(&[4, 1])));
        assert_eq!(
            names(&output),
            ["drag@2", "dragenter@4", "dragleave@3", "dragover@4"]
        );
        assert_eq!(output.events[1].related_target, Some(3));
        assert_eq!(output.events[2].related_target, Some(4));
    }

    #[test]
    fn drag_is_only_produced_when_something_listens_for_it() {
        let tree = draggable_tree(&listening_everywhere(&["dragstart"]));
        let mut state = dragging_from_a(&tree);
        let output = dispatch(&mut state, &tree, &held_move(31.0, 30.0, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["dragover@3"]);
    }

    #[test]
    fn the_release_drops_on_the_target_then_ends_the_drag_at_the_source() {
        let mut listening = listening_everywhere(&DRAG_EVENTS);
        listening.extend(listening_everywhere(&["click", "mouseup", "pointerup"]));
        let tree = draggable_tree(&listening);
        let mut state = dragging_from_a(&tree);
        let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[4, 1])));
        assert_eq!(
            names(&output),
            ["dragenter@4", "dragleave@3", "drop@4", "dragend@2"]
        );
        assert!(state.element_drag.is_none());
        assert!(state.pressed.is_empty(), "a drag is not a click");
        assert_eq!(state.buttons, 0);
    }

    #[test]
    fn a_drop_on_nothing_targets_the_root() {
        let tree = draggable_tree(&[]);
        let mut state = dragging_from_a(&tree);
        let output = dispatch(&mut state, &tree, &up(0, 1, Vec::new()));
        assert_eq!(names(&output), ["dragenter@1", "dragleave@3", "drop@1", "dragend@2"]);
    }

    #[test]
    fn escape_leaves_the_target_and_ends_the_drag_without_a_drop() {
        let tree = draggable_tree(&listening_everywhere(&DRAG_EVENTS));
        let mut state = dragging_from_a(&tree);
        let output = dispatch(
            &mut state,
            &tree,
            &at(PointerAction::CancelDrag, chain_of(&[3, 2, 1])),
        );
        assert_eq!(names(&output), ["dragleave@3", "dragend@2"]);
        assert_eq!(output.events[0].related_target, None);
        assert!(state.element_drag.is_none());

        let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3, 2, 1])));
        assert!(output.events.is_empty(), "the release after a cancel drops nothing");
    }

    #[test]
    fn the_pointer_leaving_the_window_during_a_drag_leaves_the_target_and_keeps_the_drag() {
        let tree = draggable_tree(&listening_everywhere(&DRAG_EVENTS));
        let mut state = dragging_from_a(&tree);
        let output = dispatch(&mut state, &tree, &at(PointerAction::Exit, Vec::new()));
        assert_eq!(names(&output), ["dragleave@3"]);
        assert_eq!(output.events[0].related_target, None);
        assert!(state.element_drag.is_some(), "the drag outlives the exit");

        let output = dispatch(&mut state, &tree, &held_move(30.0, 30.0, chain_of(&[4, 1])));
        assert_eq!(names(&output), ["drag@2", "dragenter@4", "dragover@4"]);
        assert_eq!(output.events[1].related_target, None);

        let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[4, 1])));
        assert_eq!(names(&output), ["drop@4", "dragend@2"]);
    }

    #[test]
    fn exiting_twice_during_a_drag_leaves_once() {
        let tree = draggable_tree(&listening_everywhere(&DRAG_EVENTS));
        let mut state = dragging_from_a(&tree);
        dispatch(&mut state, &tree, &at(PointerAction::Exit, Vec::new()));
        let output = dispatch(&mut state, &tree, &at(PointerAction::Exit, Vec::new()));
        assert!(output.events.is_empty());
    }

    #[test]
    fn escape_with_no_drag_does_nothing() {
        let tree = draggable_tree(&listening_everywhere(&["pointermove", "dragend"]));
        let mut state = pressed_on_b(&tree);
        let output = dispatch(
            &mut state,
            &tree,
            &at(PointerAction::CancelDrag, chain_of(&[3, 2, 1])),
        );
        assert!(output.events.is_empty());
    }

    #[test]
    fn a_move_with_no_button_down_means_the_release_was_lost() {
        let tree = draggable_tree(&listening_everywhere(&DRAG_EVENTS));
        let mut state = dragging_from_a(&tree);
        let output = dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["dragleave@3", "dragend@2"]);
    }

    #[test]
    fn cancel_drag_ends_the_session_and_the_pointer_comes_back() {
        let tree = draggable_tree(&listening_everywhere(&[
            "dragend",
            "dragover",
            "pointermove",
            "pointerover",
            "pointerup",
            "click",
        ]));
        let mut state = dragging_from_a(&tree);
        state.requests.cancel_drag();

        let output = dispatch(&mut state, &tree, &held_move(35.0, 30.0, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["pointerover@3", "pointermove@3"]);
        assert!(state.element_drag.is_none());
        let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["pointerup@3"], "the cancelled press is no click");
    }

    #[test]
    fn a_cancel_that_arrives_with_no_drag_is_dropped() {
        let tree = draggable_tree(&listening_everywhere(&["dragstart"]));
        let mut state = PointerState::default();
        state.requests.cancel_drag();
        dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        let output = dispatch(&mut state, &tree, &held_move(30.0, 30.0, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["dragstart@2", "dragenter@3", "dragover@3"]);
    }

    #[test]
    fn no_pointer_or_mouse_event_fires_during_an_element_drag() {
        let mut listening = listening_everywhere(&POINTER_BOUNDARIES);
        listening.extend(listening_everywhere(&[
            "pointermove",
            "mousemove",
            "pointerup",
            "mouseup",
            "pointerdown",
            "mousedown",
            "click",
            "mousedownoutside",
            "contextmenu",
            "dragover",
        ]));
        let tree = draggable_tree(&listening);
        let mut state = dragging_from_a(&tree);
        let mut log = Vec::new();
        for sample in [
            held_move(60.0, 30.0, chain_of(&[4, 1])),
            held_move(61.0, 30.0, chain_of(&[4, 1])),
            at(PointerAction::Resync, chain_of(&[3, 2, 1])),
            at(PointerAction::Exit, Vec::new()),
            down(2, 1, chain_of(&[3, 2, 1])),
            up(2, 1, chain_of(&[3, 2, 1])),
            up(0, 1, chain_of(&[4, 1])),
        ] {
            log.extend(names(&dispatch(&mut state, &tree, &sample)));
        }
        assert!(
            log.iter()
                .all(|event| event.starts_with("drag") || event.starts_with("drop")),
            "{log:?}"
        );
        assert!(log.contains(&"drop@4".to_string()));
    }

    #[test]
    fn a_resync_during_an_element_drag_moves_the_drag_target() {
        let tree = draggable_tree(&[]);
        let mut state = dragging_from_a(&tree);
        let output = dispatch(&mut state, &tree, &at(PointerAction::Resync, chain_of(&[4, 1])));
        assert_eq!(names(&output), ["dragenter@4", "dragleave@3"]);
    }

    #[test]
    fn a_capture_held_by_another_element_keeps_the_press_a_press() {
        let tree = draggable_tree(&listening_everywhere(&["dragstart", "pointermove"]));
        let mut state = pressed_on_b(&tree);
        state.requests.set(&tree, 4, 1.0).unwrap();
        let output = dispatch(&mut state, &tree, &held_move(90.0, 30.0, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["pointermove@4"]);
        assert!(state.element_drag.is_none());
    }

    #[test]
    fn a_capture_held_by_the_source_is_lost_when_the_drag_starts() {
        let tree = draggable_tree(&listening_everywhere(&[
            "dragstart",
            "pointercancel",
            "gotpointercapture",
            "lostpointercapture",
        ]));
        let mut state = pressed_on_b(&tree);
        state.requests.set(&tree, 2, 1.0).unwrap();
        let output = dispatch(&mut state, &tree, &held_move(16.0, 30.0, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["gotpointercapture@2"]);

        let output = dispatch(&mut state, &tree, &held_move(30.0, 30.0, chain_of(&[3, 2, 1])));
        assert_eq!(
            names(&output),
            ["pointercancel@3", "lostpointercapture@2", "dragstart@2", "dragenter@3", "dragover@3"]
        );
        assert_eq!(state.explicit_capture(), None);
    }

    #[test]
    fn a_release_before_the_threshold_clears_the_press() {
        let tree = draggable_tree(&listening_everywhere(&["dragstart", "pointermove"]));
        let mut state = pressed_on_b(&tree);
        dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3, 2, 1])));
        let output = dispatch(&mut state, &tree, &held_move(90.0, 30.0, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["pointermove@3"]);
    }

    #[test]
    fn a_drag_whose_source_left_the_tree_ends_without_a_dragend() {
        let mut tree = draggable_tree(&listening_everywhere(&DRAG_EVENTS));
        let mut state = dragging_from_a(&tree);
        tree.destroy_element(2);
        let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[4, 1])));
        assert_eq!(names(&output), ["dragenter@4", "drop@4"]);
    }

    fn move_to(x: f64, y: f64, hits: Vec<Hit>) -> PointerSample {
        PointerSample {
            position: (x, y),
            ..mv(hits)
        }
    }

    #[test]
    fn the_lock_element_takes_every_pointer_and_mouse_event() {
        let tree = tree(&listening_everywhere(&["pointermove", "mousemove", "mouseup"]));
        let mut state = PointerState::default();
        state.begin_lock(4);
        let output = dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["pointermove@4", "mousemove@4"]);
        assert!(state.is_hovered(4) && !state.is_hovered(3));
        let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["mouseup@4"]);
    }

    #[test]
    fn a_locked_pointer_reports_the_position_it_was_locked_at() {
        let tree = tree(&[(4, "mousemove")]);
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &move_to(15.0, 30.0, chain_of(&[3, 2, 1])));
        state.begin_lock(4);
        let output = dispatch(&mut state, &tree, &move_to(90.0, 95.0, chain_of(&[3, 2, 1])));
        let payload = &output.into_payloads()[0];
        assert_eq!((payload.client_x, payload.client_y), (Some(15.0), Some(30.0)));
        assert_eq!((payload.x, payload.y), (Some(15.0), Some(30.0)));

        state.end_lock();
        let output = dispatch(&mut state, &tree, &move_to(90.0, 95.0, chain_of(&[4, 1])));
        let payload = &output.into_payloads()[0];
        assert_eq!((payload.client_x, payload.client_y), (Some(90.0), Some(95.0)));
    }

    #[test]
    fn the_lock_outranks_a_capture() {
        let tree = tree(&listening_everywhere(&["mousemove"]));
        let mut state = pressed_with_capture_on_c(&tree);
        state.begin_lock(2);
        let output = dispatch(&mut state, &tree, &held_move(90.0, 30.0, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["mousemove@2"]);
    }

    #[test]
    fn a_lock_element_that_left_the_tree_pins_nothing() {
        let mut tree = tree(&listening_everywhere(&["mousemove"]));
        let mut state = PointerState::default();
        state.begin_lock(4);
        tree.destroy_element(4);
        let output = dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["mousemove@3"]);
    }

    #[test]
    fn hover_returns_to_what_is_under_the_pointer_when_the_lock_ends() {
        let tree = tree(&listening_everywhere(&["mouseenter", "mouseleave"]));
        let mut state = PointerState::default();
        state.begin_lock(4);
        dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        state.end_lock();
        let output = dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        assert_eq!(
            names(&output),
            ["mouseleave@4", "mouseenter@2", "mouseenter@3"]
        );
    }

    #[test]
    fn move_events_carry_the_platform_movement() {
        let tree = tree(&listening_everywhere(&["pointermove", "mousemove", "pointerdown"]));
        let mut state = PointerState::default();
        let moved = PointerSample {
            movement: (3.0, -2.5),
            ..mv(chain_of(&[3, 2, 1]))
        };
        let output = dispatch(&mut state, &tree, &moved);
        let payloads = output.into_payloads();
        assert_eq!(payloads.len(), 2);
        for payload in &payloads {
            assert_eq!((payload.movement_x, payload.movement_y), (Some(3.0), Some(-2.5)));
        }

        let pressed = dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        let payloads = pressed.into_payloads();
        assert_eq!((payloads[0].movement_x, payloads[0].movement_y), (None, None));
    }

    #[test]
    fn a_locked_pointer_still_reports_its_movement() {
        let tree = tree(&listening_everywhere(&["mousemove"]));
        let mut state = PointerState::default();
        state.begin_lock(4);
        let moved = PointerSample {
            movement: (-7.0, 1.0),
            ..move_to(90.0, 95.0, chain_of(&[3, 2, 1]))
        };
        let payloads = dispatch(&mut state, &tree, &moved).into_payloads();
        assert_eq!((payloads[0].movement_x, payloads[0].movement_y), (Some(-7.0), Some(1.0)));
    }

    fn wheel(delta: (f64, f64), hits: Vec<Hit>) -> PointerSample {
        at(
            PointerAction::Wheel(WheelTick {
                delta,
                precise: true,
                phase: "moved",
            }),
            hits,
        )
    }

    #[test]
    fn a_wheel_is_one_event_at_the_target_with_its_path_and_the_tick() {
        let tree = tree(&listening_everywhere(&["wheel"]));
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &wheel((0.0, 40.0), chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["wheel@3"]);
        assert!(!output.restyle);
        let payload = &output.into_payloads()[0];
        assert_eq!(payload.path, Some(vec![3.0, 2.0, 1.0]));
        assert_eq!(payload.target, Some(3.0));
        assert_eq!((payload.delta_x, payload.delta_y), (Some(0.0), Some(40.0)));
        assert_eq!(payload.delta_mode, Some(0));
        assert_eq!(payload.precise, Some(true));
        assert_eq!(payload.touch_phase.as_deref(), Some("moved"));
        assert_eq!((payload.client_x, payload.client_y), (Some(15.0), Some(30.0)));
        assert_eq!((payload.offset_x, payload.offset_y), (Some(5.0), Some(10.0)));
        assert_eq!(payload.buttons, Some(0));
        assert!(payload.modifiers.is_some());
    }

    #[test]
    fn a_listener_on_an_ancestor_is_enough_and_nobody_listening_produces_nothing() {
        let mut state = PointerState::default();
        let listening_above = tree(&[(1, "wheel")]);
        let output = dispatch(&mut state, &listening_above, &wheel((0.0, 40.0), chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["wheel@3"]);

        let listening_aside = tree(&[(4, "wheel")]);
        let output = dispatch(&mut state, &listening_aside, &wheel((0.0, 40.0), chain_of(&[3, 2, 1])));
        assert!(output.events.is_empty());
    }

    #[test]
    fn a_wheel_over_nothing_has_no_target() {
        let tree = tree(&listening_everywhere(&["wheel"]));
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &wheel((0.0, 40.0), Vec::new()));
        assert!(output.events.is_empty());
    }

    #[test]
    fn a_wheel_moves_no_hover_and_fires_no_boundary_event() {
        let tree = tree(&listening_everywhere(&["wheel", "pointerover", "mouseenter"]));
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &wheel((0.0, 40.0), chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["wheel@3"]);
        assert!(!state.is_hovered(3));
        // The move that follows still enters everything the wheel passed over.
        let output = dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        assert!(names(&output).contains(&"pointerover@3".to_string()));
    }

    #[test]
    fn a_capture_held_elsewhere_does_not_redirect_a_wheel() {
        let tree = tree(&listening_everywhere(&["wheel", "mouseup"]));
        let mut state = pressed_with_capture_on_c(&tree);
        let output = dispatch(&mut state, &tree, &wheel((0.0, 40.0), chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["wheel@3"]);
        let output = dispatch(&mut state, &tree, &up(0, 1, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["mouseup@4"]);
    }

    #[test]
    fn a_wheel_while_a_button_is_held_reports_the_buttons() {
        let tree = tree(&listening_everywhere(&["wheel"]));
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        let payload = &dispatch(&mut state, &tree, &wheel((8.0, 0.0), chain_of(&[3, 2, 1])))
            .into_payloads()[0];
        assert_eq!(payload.buttons, Some(1));
        assert_eq!((payload.delta_x, payload.delta_y), (Some(8.0), Some(0.0)));
    }

    #[test]
    fn the_lock_element_takes_the_wheel() {
        let tree = tree(&listening_everywhere(&["wheel"]));
        let mut state = PointerState::default();
        state.begin_lock(4);
        let output = dispatch(&mut state, &tree, &wheel((0.0, 40.0), chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["wheel@4"]);
    }

    fn in_drag_region(sample: PointerSample) -> PointerSample {
        PointerSample {
            over_drag_region: true,
            ..sample
        }
    }

    #[test]
    fn a_pointer_over_a_drag_region_leaves_the_page_and_comes_back_on_leaving_it() {
        let tree = tree(&listening_everywhere(&[
            "mouseleave",
            "mouseout",
            "mouseenter",
            "mouseover",
            "mousemove",
        ]));
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        let output = dispatch(&mut state, &tree, &in_drag_region(mv(chain_of(&[4, 1]))));
        assert_eq!(
            names(&output),
            ["mouseout@3", "mouseleave@3", "mouseleave@2", "mouseleave@1"]
        );
        assert!(!state.is_hovered(1));
        // Moving about inside the region says nothing.
        let output = dispatch(&mut state, &tree, &in_drag_region(mv(chain_of(&[3, 2, 1]))));
        assert!(output.events.is_empty());
        let output = dispatch(&mut state, &tree, &mv(chain_of(&[4, 1])));
        assert_eq!(
            names(&output),
            ["mouseover@4", "mouseenter@1", "mouseenter@4", "mousemove@4"]
        );
    }

    #[test]
    fn a_resync_under_a_drag_region_keeps_the_pointer_outside() {
        let tree = tree(&listening_everywhere(&["mouseleave", "mouseenter"]));
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        dispatch(&mut state, &tree, &in_drag_region(mv(chain_of(&[3, 2, 1]))));
        let output = dispatch(
            &mut state,
            &tree,
            &in_drag_region(at(PointerAction::Resync, chain_of(&[4, 1]))),
        );
        assert!(output.events.is_empty());
        assert!(!state.is_hovered(1));
        // The chain the next paint compares against is the empty one the page saw.
        assert_eq!(*state.observed().borrow(), Some(Vec::new()));
    }

    #[test]
    fn a_press_that_reaches_the_page_ends_the_windows_earlier_press_of_that_button() {
        let tree = tree(&listening_everywhere(&["mousedown", "mousemove"]));
        let mut state = PointerState::default();
        // The window took the left press and the drag loop ate its release.
        state.window_presses = button_bit(0);
        dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        assert_eq!(state.window_presses, 0);
        // Nothing hides the pointer from the page any more.
        let output = dispatch(&mut state, &tree, &held_move(5.0, 5.0, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["mousemove@3"]);
    }

    #[test]
    fn a_press_of_another_button_leaves_the_windows_press_alone() {
        let tree = tree(&listening_everywhere(&["mousedown"]));
        let mut state = PointerState::default();
        state.window_presses = button_bit(0);
        dispatch(&mut state, &tree, &down(2, 1, chain_of(&[3, 2, 1])));
        assert_eq!(state.window_presses, button_bit(0));
    }

    #[test]
    fn the_pointer_leaving_the_window_ends_the_windows_press() {
        let tree = tree(&listening_everywhere(&["mouseleave"]));
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        state.window_presses = button_bit(0);
        dispatch(&mut state, &tree, &at(PointerAction::Exit, Vec::new()));
        assert_eq!(state.window_presses, 0);
    }

    #[test]
    fn a_button_held_from_outside_keeps_its_moves_through_a_drag_region() {
        let tree = tree(&listening_everywhere(&["mousemove", "mouseleave"]));
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        let output = dispatch(
            &mut state,
            &tree,
            &in_drag_region(held_move(15.0, 30.0, chain_of(&[4, 1]))),
        );
        assert_eq!(names(&output), ["mouseleave@3", "mouseleave@2", "mousemove@4"]);
        assert!(state.is_hovered(4));
    }

    #[test]
    fn a_secondary_press_in_a_drag_region_reaches_its_target() {
        let tree = tree(&listening_everywhere(&["mousedown", "contextmenu"]));
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &in_drag_region(down(2, 1, chain_of(&[4, 1]))));
        assert_eq!(
            names(&output),
            ["mousedown@4", "contextmenu@4"]
        );
    }

    #[test]
    fn a_wheel_over_a_drag_region_still_reaches_what_is_under_it() {
        let tree = tree(&listening_everywhere(&["wheel"]));
        let mut state = PointerState::default();
        let output = dispatch(
            &mut state,
            &tree,
            &in_drag_region(wheel((0.0, 40.0), chain_of(&[3, 2, 1]))),
        );
        assert_eq!(names(&output), ["wheel@3"]);
    }

    #[test]
    fn a_lost_release_over_a_drag_region_still_cancels_the_press() {
        let tree = tree(&listening_everywhere(&["pointercancel"]));
        let mut state = PointerState::default();
        dispatch(&mut state, &tree, &down(0, 1, chain_of(&[3, 2, 1])));
        let output = dispatch(&mut state, &tree, &in_drag_region(mv(chain_of(&[4, 1]))));
        assert_eq!(names(&output), ["pointercancel@3"]);
    }
}
