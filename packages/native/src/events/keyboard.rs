//! Keyboard events.
//!
//! One `keyDown` per keystroke, aimed at the focused element, sent from a GPUI
//! keystroke interceptor. Interceptors run before key bindings and actions, so a
//! JS handler sees the editor's state before the key acts on it, and
//! `preventDefault()` stops GPUI from acting on it at all: no editor action, no
//! text insertion. `keyUp` has no default action worth cancelling and is sent
//! from a root key listener.
//!
//! Everything here runs inside GPUI's input dispatch, where `GpuixView` may be
//! leased, so the listeners read the state they need from `InputRouting`, which
//! `GpuixView` shares with them, and never touch the view.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gpui::{App, WeakFocusHandle, Window};

use crate::element_tree::{EventModifiers, EventPayload};
use crate::events::sync_dispatch;
use crate::renderer::EventCallback;
use crate::retained_tree::RetainedTree;

#[derive(Default)]
struct RoutingState {
    window_key_down: Cell<bool>,
    window_key_up: Cell<bool>,
    window_key_event_id: Cell<u64>,
    /// The gpui focus handle of every focusable element, refreshed every frame.
    /// Weak: the view owns the handles, and a strong one kept here would
    /// outlive the app at teardown.
    focusables: RefCell<HashMap<u64, WeakFocusHandle>>,
    /// A key or a mouse button has been pressed in this window.
    user_input: Cell<bool>,
}

/// What the input listeners read from `GpuixView`, shared so they never have to
/// lease it.
#[derive(Clone, Default)]
pub(crate) struct InputRouting(Rc<RoutingState>);

impl InputRouting {
    /// `setWindowKeyEvents`. `key_down` and `key_up` say whether JS wants every
    /// key even when nothing focused listens, and `event_id` is the target an
    /// event gets when nothing is focused.
    pub fn set_window_keys(&self, key_down: bool, key_up: bool, event_id: u64) {
        self.0.window_key_down.set(key_down);
        self.0.window_key_up.set(key_up);
        self.0.window_key_event_id.set(event_id);
    }

    pub fn set_focusables(&self, handles: impl Iterator<Item = (u64, WeakFocusHandle)>) {
        let mut focusables = self.0.focusables.borrow_mut();
        focusables.clear();
        focusables.extend(handles);
    }

    pub fn note_user_input(&self) {
        self.0.user_input.set(true);
    }

    pub fn user_input_seen(&self) -> bool {
        self.0.user_input.get()
    }

    fn focused_element(&self, window: &Window, cx: &App) -> Option<u64> {
        let focused = window.focused(cx)?;
        self.0
            .focusables
            .borrow()
            .iter()
            .find_map(|(id, handle)| (*handle == focused).then_some(*id))
    }

    /// The focus handle of the first of `ids` that is focusable.
    pub(crate) fn first_focusable(&self, ids: &[u64]) -> Option<gpui::FocusHandle> {
        let focusables = self.0.focusables.borrow();
        ids.iter()
            .find_map(|id| focusables.get(id))
            .and_then(WeakFocusHandle::upgrade)
    }
}

/// `element` and its ancestors, innermost first.
pub(crate) fn path_to_root(tree: &RetainedTree, element: u64) -> Vec<u64> {
    std::iter::successors(Some(element), |id| {
        tree.elements.get(id).and_then(|element| element.parent)
    })
    .collect()
}

/// A `keyDown` or `keyUp`, aimed at the focused element, or at the window when
/// nothing listening is focused.
///
/// `None` when nobody would hear it: JS did not ask for window keys and no
/// element on the path listens.
fn key_payload(
    routing: &InputRouting,
    tree: &Mutex<RetainedTree>,
    window: &Window,
    cx: &App,
    event_type: &str,
    keystroke: &gpui::Keystroke,
    is_held: Option<bool>,
) -> Option<EventPayload> {
    let window_listens = if event_type == "keyDown" {
        routing.0.window_key_down.get()
    } else {
        routing.0.window_key_up.get()
    };
    let focused = routing.focused_element(window, cx);
    let path = {
        let tree = tree.lock().unwrap();
        // A focused element that is gone from the tree has no path to report.
        focused
            .filter(|id| tree.elements.contains_key(id))
            .map(|id| path_to_root(&tree, id))
            .filter(|path| {
                window_listens
                    || path.iter().any(|id| {
                        tree.elements.get(id).is_some_and(|element| {
                            let events = element.events();
                            events.contains(event_type)
                                || events.contains(&format!("{event_type}Capture"))
                        })
                    })
            })
    };
    if path.is_none() && !window_listens {
        return None;
    }
    let mut payload = EventPayload {
        event_type: event_type.to_string(),
        key: Some(keystroke.key.clone()),
        key_char: keystroke.key_char.clone(),
        is_held,
        modifiers: Some(EventModifiers::from(keystroke.modifiers)),
        ..Default::default()
    };
    match path {
        Some(path) => {
            payload.element_id = path[0] as f64;
            payload.target = Some(path[0] as f64);
            payload.path = Some(path.into_iter().map(|id| id as f64).collect());
        }
        None => payload.element_id = routing.0.window_key_event_id.get() as f64,
    }
    Some(payload)
}

/// Sends `keyDown` for every keystroke before GPUI binds it to anything.
///
/// Tab never reaches GPUI's bindings or the platform text input even when JS
/// lets it through: it is a navigation key, never text, as in a browser field.
/// Focus moves in the JS default action, which already ran by the time the
/// outcome is back. The subscription ends the interception when dropped.
pub(crate) fn install_key_down(
    cx: &mut App,
    tree: Arc<Mutex<RetainedTree>>,
    callback: Option<EventCallback>,
    routing: InputRouting,
) -> gpui::Subscription {
    cx.intercept_keystrokes(move |event, window, cx| {
        routing.note_user_input();
        let prevented = key_payload(
            &routing,
            &tree,
            window,
            cx,
            "keyDown",
            &event.keystroke,
            Some(event.is_held),
        )
        .map(|payload| sync_dispatch::emit(window, cx, &callback, payload))
        .is_some_and(|outcome| outcome.default_prevented);
        if prevented || event.keystroke.key == "tab" {
            cx.stop_propagation();
        }
    })
}

/// The window-level listeners: `keyUp`, and the focus default of a mouse press
/// (`focus::register_mouse_down_default`).
///
/// A zero-size canvas, because listeners are registered during paint.
pub(crate) fn window_input_listeners(
    tree: Arc<Mutex<RetainedTree>>,
    callback: Option<EventCallback>,
    routing: InputRouting,
) -> impl gpui::IntoElement {
    use gpui::prelude::*;

    gpui::canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            let key_up_routing = routing.clone();
            let key_up_tree = tree.clone();
            let callback = callback.clone();
            window.on_root_key_event(move |event: &gpui::KeyUpEvent, phase, window, cx| {
                if phase != gpui::DispatchPhase::Capture {
                    return;
                }
                let prevented = key_payload(
                    &key_up_routing,
                    &key_up_tree,
                    window,
                    cx,
                    "keyUp",
                    &event.keystroke,
                    None,
                )
                .map(|payload| sync_dispatch::emit(window, cx, &callback, payload))
                .is_some_and(|outcome| outcome.default_prevented);
                if prevented {
                    cx.stop_propagation();
                }
            });

            crate::focus::register_mouse_down_default(window, tree.clone(), routing.clone());
        },
    )
    .absolute()
    .w(gpui::px(0.0))
    .h(gpui::px(0.0))
}
