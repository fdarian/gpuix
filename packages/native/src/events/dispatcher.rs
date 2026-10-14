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
//!     pointerout, pointerleave (innermost first), mouseout, mouseleave,
//!     pointerover, pointerenter (outermost first), mouseover, mouseenter
//!   then the event itself, pointer before mouse
//!     pointermove/mousemove, pointerdown/mousedown, mousedownoutside, contextmenu,
//!     pointerup, lostpointercapture, mouseup, then click, dblclick or auxclick
//!
//! A type is only produced when the target or an ancestor listens for it, so a
//! window with no `mousemove` handler pays nothing for the traffic.

use gpui::{Bounds, Pixels};

use crate::element_tree::{EventModifiers, EventPayload};
use crate::retained_tree::RetainedTree;
use crate::style::StyleDesc;

use super::ids::Hit;
use super::path::{chain_follows_path, common_ancestor, event_path, path_listens};
use super::state::{is_connected, PointerState, Press};

const PRIMARY_BUTTON: u32 = 0;
const SECONDARY_BUTTON: u32 = 2;
const DOUBLE_CLICK: u32 = 2;
const MOUSE_DOWN_OUTSIDE: &str = "mousedownoutside";

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
}

pub struct PointerSample {
    pub action: PointerAction,
    /// Window coordinates.
    pub position: (f64, f64),
    pub modifiers: EventModifiers,
    /// Host elements under the pointer, target first. Empty outside the window.
    pub hits: Vec<Hit>,
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
    pub pressed_button: Option<u32>,
    pub hovered: Option<bool>,
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
                button: event.button,
                is_right_click: event.button.map(|button| button == SECONDARY_BUTTON),
                buttons: Some(buttons),
                detail: event.detail,
                click_count: event.detail,
                pressed_button: event.pressed_button,
                hovered: event.hovered,
                pointer_id: Some(1),
                pointer_type: Some("mouse".to_string()),
                modifiers: Some(modifiers.clone()),
                ..Default::default()
            })
            .collect()
    }
}

#[derive(Clone, Copy, Default)]
struct Fields {
    related_target: Option<u64>,
    button: Option<u32>,
    detail: Option<u32>,
    pressed_button: Option<u32>,
    hovered: Option<bool>,
}

struct Emitter<'a> {
    tree: &'a RetainedTree,
    position: (f64, f64),
    /// Boxes of the chain under the pointer now, and of the chain it left.
    hits: &'a [Hit],
    previous_hits: &'a [Hit],
    events: Vec<DomEvent>,
}

impl Emitter<'_> {
    fn emit(&mut self, event_type: &'static str, target: u64, fields: Fields) {
        if !path_listens(self.tree, target, event_type) {
            return;
        }
        self.events.push(DomEvent {
            event_type,
            path: event_path(self.tree, target),
            listener: None,
            related_target: fields.related_target,
            button: fields.button,
            detail: fields.detail,
            offset: self.offset(target),
            pressed_button: fields.pressed_button,
            hovered: fields.hovered,
        });
    }

    /// `mousedownoutside` for every element that listens for it and does not
    /// contain `target` in its logical subtree: the target's retained path is
    /// the elements it is inside, anchored layers included.
    fn emit_outside_presses(&mut self, target: u64, fields: Fields) {
        let path = event_path(self.tree, target);
        let mut listeners: Vec<u64> = self
            .tree
            .elements
            .iter()
            .filter(|(id, element)| {
                element.events().contains(MOUSE_DOWN_OUTSIDE) && !path.contains(id)
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
                offset: self.offset(target),
                pressed_button: fields.pressed_button,
                hovered: fields.hovered,
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

fn button_bit(button: u32) -> u32 {
    match button {
        0 => 1,
        1 => 4,
        2 => 2,
        3 => 8,
        _ => 16,
    }
}

pub fn dispatch(state: &mut PointerState, tree: &RetainedTree, sample: &PointerSample) -> Output {
    let geometric = sample
        .hits
        .iter()
        .map(|hit| hit.id)
        .find(|id| tree.elements.contains_key(id));
    state.observe(geometric);
    let chain_mismatch = geometric.is_some_and(|target| {
        let chain: Vec<u64> = sample
            .hits
            .iter()
            .map(|hit| hit.id)
            .filter(|id| tree.elements.contains_key(id))
            .collect();
        !chain_follows_path(&chain, &event_path(tree, target))
    });
    match sample.action {
        PointerAction::Down { button, .. } => state.buttons |= button_bit(button),
        PointerAction::Up { button, .. } => state.buttons &= !button_bit(button),
        PointerAction::Move { pressed_button } => {
            state.buttons = pressed_button.map_or(0, button_bit);
        }
        PointerAction::Exit | PointerAction::Resync => {}
    }
    state.capture.set_buttons(state.buttons);

    let previous_hits = std::mem::take(&mut state.hits);
    let mut emitter = Emitter {
        tree,
        position: sample.position,
        hits: &sample.hits,
        previous_hits: &previous_hits,
        events: Vec::new(),
    };
    let mut restyle = false;

    apply_capture(state, &mut emitter);
    // A captured element keeps the pointer: it is the hover target and takes
    // every event until the release, wherever the pointer is.
    restyle |= update_hover(state, &mut emitter, state.explicit.or(geometric));
    let target = state.explicit.or(geometric);

    match sample.action {
        PointerAction::Move { pressed_button } => {
            if let Some(target) = target {
                let fields = Fields {
                    pressed_button,
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
            if let Some(target) = target {
                restyle |= press(state, &mut emitter, target, button, click_count);
            }
        }
        PointerAction::Up {
            button,
            click_count,
        } => {
            restyle |= release(state, &mut emitter, target, geometric, button, click_count);
        }
        PointerAction::Exit | PointerAction::Resync => {}
    }

    state.hits = sample.hits.clone();
    Output {
        events: emitter.events,
        restyle,
        chain_mismatch,
        position: sample.position,
        modifiers: sample.modifiers.clone(),
        buttons: state.buttons,
    }
}

/// Makes the capture JS asked for the one in force, firing `lostpointercapture`
/// on the element that had it and `gotpointercapture` on the one that has it now.
/// A capture whose element left the tree ends without an event, as in the DOM.
fn apply_capture(state: &mut PointerState, emitter: &mut Emitter) {
    let tree = emitter.tree;
    let mut requested = state.capture.target();
    if requested.is_some_and(|id| !is_connected(tree, id)) {
        state.capture.clear();
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
fn update_hover(state: &mut PointerState, emitter: &mut Emitter, new_target: Option<u64>) -> bool {
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
    if let Some(old_target) = old_target.filter(|old| Some(*old) != new_target) {
        for (out, leave) in [("pointerout", "pointerleave"), ("mouseout", "mouseleave")] {
            emitter.emit(out, old_target, left);
            for id in &leaving {
                emitter.emit(leave, *id, left);
            }
        }
    }
    if let Some(new_target) = new_target.filter(|new| Some(*new) != old_target) {
        for (over, enter) in [("pointerover", "pointerenter"), ("mouseover", "mouseenter")] {
            emitter.emit(over, new_target, entered);
            for id in &entering {
                emitter.emit(enter, *id, entered);
            }
        }
    }

    let restyle = leaving
        .iter()
        .chain(&entering)
        .any(|id| has_state_style(tree, *id, |style| style.hover.is_some()));
    state.hovered = new_path;
    restyle
}

fn press(
    state: &mut PointerState,
    emitter: &mut Emitter,
    target: u64,
    button: u32,
    click_count: u32,
) -> bool {
    let tree = emitter.tree;
    let fields = Fields {
        button: Some(button),
        detail: Some(click_count),
        ..Default::default()
    };
    emitter.emit("pointerdown", target, fields);
    emitter.emit("mousedown", target, fields);
    emitter.emit_outside_presses(target, fields);
    if button == SECONDARY_BUTTON {
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
        emitter.emit("pointerup", target, fields);
    }

    // Capture lasts until the last button is up. The Pointer Events spec
    // releases it right after pointerup, so mouseup still goes to the captured
    // element but lostpointercapture comes before it.
    let mut released = false;
    if state.buttons == 0 {
        state.capture.clear();
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
        if let Some(clicked) = common_ancestor(&press.path, &release_path) {
            if button == PRIMARY_BUTTON {
                emitter.emit("click", clicked, fields);
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
        restyle |= update_hover(state, emitter, geometric);
    }
    restyle
}

fn set_active(state: &mut PointerState, tree: &RetainedTree, new_path: Vec<u64>) -> bool {
    if state.active == new_path {
        return false;
    }
    let restyle = state
        .active
        .iter()
        .filter(|id| !new_path.contains(id))
        .chain(new_path.iter().filter(|id| !state.active.contains(id)))
        .any(|id| has_state_style(tree, *id, |style| style.active.is_some()));
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
            modifiers: EventModifiers::default(),
            hits,
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
    fn moving_to_a_sibling_leaves_innermost_first_and_keeps_the_shared_ancestor() {
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
                "mouseout@3",
                "mouseleave@3",
                "mouseleave@2",
                "pointerover@4",
                "pointerenter@4",
                "mouseover@4",
                "mouseenter@4",
            ]
        );
        let out = &output.events[0];
        assert_eq!(out.related_target, Some(4));
        let over = &output.events[6];
        assert_eq!(over.related_target, Some(3));
        assert!(!state.is_hovered(3) && state.is_hovered(4) && state.is_hovered(1));
    }

    #[test]
    fn enter_is_dispatched_once_per_entered_element_with_its_own_path() {
        // An ancestor's listener makes native produce the event, because it cannot
        // tell a capture handler from a bubble one. JS then runs it at the target
        // only, so a non-capturing root hears `mouseenter` only for itself.
        let tree = tree(&[(1, "mouseover"), (1, "mouseenter")]);
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &mv(chain_of(&[3, 2, 1])));
        assert_eq!(
            names(&output),
            [
                "mouseover@3",
                "mouseenter@1",
                "mouseenter@2",
                "mouseenter@3"
            ]
        );
        assert_eq!(output.events[0].path, [3, 2, 1]);
        assert_eq!(output.events[1].path, [1]);
        assert_eq!(output.events[3].path, [3, 2, 1]);
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

    #[test]
    fn a_secondary_press_opens_the_context_menu_after_mousedown() {
        let tree = tree(&listening_everywhere(&["mousedown", "contextmenu"]));
        let mut state = PointerState::default();
        let output = dispatch(&mut state, &tree, &down(2, 1, chain_of(&[3, 2, 1])));
        assert_eq!(names(&output), ["mousedown@3", "contextmenu@3"]);
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
        state.capture.set(tree, 4, 1.0).unwrap();
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
        assert_eq!(state.capture.target(), None);
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
        state.capture.set(&tree, 4, 1.0).unwrap();
        let output = dispatch(&mut state, &tree, &drag_move(chain_of(&[3, 2, 1])));
        assert_eq!(
            names(&output),
            [
                "pointerout@3",
                "pointerleave@3",
                "pointerleave@2",
                "mouseout@3",
                "mouseleave@3",
                "mouseleave@2",
                "pointerover@4",
                "pointerenter@4",
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
        state.capture.release(4, 1.0).unwrap();
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
        assert_eq!(state.capture.target(), None);
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
}
