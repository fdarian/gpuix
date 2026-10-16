//! Keyboard events.
//!
//! One `keydown` per keystroke, aimed at the focused element, sent from a GPUI
//! keystroke interceptor. Interceptors run before key bindings and actions, so a
//! JS handler sees the editor's state before the key acts on it, and
//! `preventDefault()` stops GPUI from acting on it at all: no editor action, no
//! text insertion. `keyup` has no default action worth cancelling and is sent
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
use crate::events::{key_names, sync_dispatch};
use crate::renderer::EventCallback;
use crate::retained_tree::RetainedTree;

/// DOM `KeyboardEvent.key` of a key the input method consumed.
const PROCESS_KEY: &str = "Process";

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

    /// Whether a JS root is attached, which is what runs the Tab default.
    fn js_root_attached(&self) -> bool {
        self.0.window_key_down.get()
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

    /// The focus handle of the first of `ids` that is focusable. `ids` is a
    /// path, innermost first.
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

/// A `keydown` or `keyup`, aimed at the focused element, or at the window when
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
    native_key: &gpui::NativeKey,
    is_held: Option<bool>,
    is_composing: bool,
) -> Option<EventPayload> {
    let window_listens = if event_type == "keydown" {
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
    let dom_key = key_names::dom_key_of_event(keystroke, native_key);
    let mut payload = EventPayload {
        event_type: event_type.to_string(),
        key: Some(dom_key.key),
        code: Some(dom_key.code),
        key_char: keystroke.key_char.clone(),
        is_held,
        repeat: Some(is_held.unwrap_or(false)),
        is_composing: Some(is_composing),
        shift_key: Some(keystroke.modifiers.shift),
        ctrl_key: Some(keystroke.modifiers.control),
        alt_key: Some(keystroke.modifiers.alt),
        meta_key: Some(keystroke.modifiers.platform),
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

/// Sends `keydown` for every keystroke before GPUI binds it to anything.
///
/// Tab never reaches GPUI's bindings or the platform text input when the JS
/// default will move focus (no ctrl, alt or cmd held, and a JS root attached):
/// it is a navigation key, never text, as in a browser field. That default
/// already ran by the time the outcome is back.
///
/// A key the platform's input method takes first (it is composing, or an IME
/// source takes printable keys) is sent too, before the input method gets it, as
/// a `keydown` whose `key` is `"Process"`. Nothing a handler does to it reaches
/// the input method or GPUI. The subscription ends both when dropped.
///
/// Interceptors belong to the `App`, not to a window, so with two views on one
/// app each would see the other's keys and send them again. This one answers
/// to `window_id` only.
pub(crate) fn install_key_down(
    cx: &mut App,
    window_id: gpui::WindowId,
    tree: Arc<Mutex<RetainedTree>>,
    callback: Option<EventCallback>,
    routing: InputRouting,
) -> gpui::Subscription {
    let ime_routing = routing.clone();
    let ime_tree = tree.clone();
    let ime_callback = callback.clone();
    let interception = cx.intercept_keystrokes(move |event, window, cx| {
        if window.window_handle().window_id() != window_id {
            return;
        }
        routing.note_user_input();
        let prevented = key_payload(
            &routing,
            &tree,
            window,
            cx,
            "keydown",
            &event.keystroke,
            &event.native_key,
            Some(event.is_held),
            event.is_composing,
        )
        .map(|payload| sync_dispatch::emit(window, cx, &callback, payload))
        .is_some_and(|outcome| outcome.default_prevented);
        // The JS Tab default replaces gpui's focus move, so gpui must only be
        // kept from it when that default is going to run. Otherwise Ctrl+Tab and
        // a Tab with nobody attached would do nothing at all.
        let modifiers = event.keystroke.modifiers;
        let tab_default_runs = event.keystroke.key == "tab"
            && !(modifiers.control || modifiers.alt || modifiers.platform)
            && routing.js_root_attached();
        if prevented || tab_default_runs {
            cx.stop_propagation();
        }
    });
    let ime_keys = cx.observe_ime_keystrokes(move |event, window, cx| {
        if window.window_handle().window_id() != window_id {
            return;
        }
        ime_routing.note_user_input();
        let Some(mut payload) = key_payload(
            &ime_routing,
            &ime_tree,
            window,
            cx,
            "keydown",
            &event.keystroke,
            &event.native_key,
            Some(event.is_held),
            event.is_composing,
        ) else {
            return;
        };
        // What the key types is the input method's to decide, so the page is
        // told it was processed, as Chrome does.
        payload.key = Some(PROCESS_KEY.to_string());
        payload.key_char = None;
        // The input method has the key whatever a handler says, so the outcome
        // (`preventDefault`) is not read.
        sync_dispatch::emit(window, cx, &ime_callback, payload);
    });
    gpui::Subscription::join(interception, ime_keys)
}

/// The window-level listeners: `keyup`. The focus default of a mouse press is
/// queued by the press itself (`focus::defer_mouse_down_default`).
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
                    "keyup",
                    &event.keystroke,
                    &event.native_key,
                    None,
                    event.is_composing,
                )
                .map(|payload| sync_dispatch::emit(window, cx, &callback, payload))
                .is_some_and(|outcome| outcome.default_prevented);
                if prevented {
                    cx.stop_propagation();
                }
            });
        },
    )
    .absolute()
    .w(gpui::px(0.0))
    .h(gpui::px(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::AppContext as _;

    struct Blank;

    impl gpui::Render for Blank {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
            gpui::div()
        }
    }

    #[gpui::test]
    fn a_keystroke_in_one_window_is_sent_once_when_two_views_share_the_app(
        cx: &mut gpui::TestAppContext,
    ) {
        let sent = Arc::new(Mutex::new(Vec::<f64>::new()));
        let mut subscriptions = Vec::new();
        let mut windows = Vec::new();
        for event_id in [1u64, 2] {
            let sent = sent.clone();
            let callback: Option<EventCallback> = Some(Arc::new(move |payload: EventPayload| {
                sent.lock().unwrap().push(payload.element_id);
            }));
            let routing = InputRouting::default();
            routing.set_window_keys(true, false, event_id);
            let tree = Arc::new(Mutex::new(RetainedTree::new()));
            let window = cx.add_window(|_, _| Blank);
            cx.update(|cx| subscriptions.push(install_key_down(cx, window.window_id(), tree, callback, routing)));
            windows.push(window);
        }
        cx.simulate_keystrokes(windows[0].into(), "x");
        assert_eq!(*sent.lock().unwrap(), vec![1.0]);
    }

    fn typed(
        cx: &mut gpui::TestAppContext,
        key: &str,
        modifiers: gpui::Modifiers,
        native_key: gpui::NativeKey,
    ) -> Vec<(String, String, String)> {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let callback: Option<EventCallback> = Some({
            let sent = sent.clone();
            Arc::new(move |payload: EventPayload| {
                sent.lock().unwrap().push((
                    payload.event_type,
                    payload.key.unwrap(),
                    payload.code.unwrap(),
                ));
            })
        });
        let routing = InputRouting::default();
        routing.set_window_keys(true, false, 1);
        let tree = Arc::new(Mutex::new(RetainedTree::new()));
        let window = cx.add_window(|_, _| Blank);
        let _keys = cx.update(|cx| install_key_down(cx, window.window_id(), tree, callback, routing));
        cx.update_window(window.into(), |_, window, cx| {
            let keystroke = gpui::Keystroke {
                modifiers,
                key: key.to_string(),
                key_char: None,
            };
            window.dispatch_native_keystroke(keystroke, native_key, cx);
        })
        .unwrap();
        let sent = sent.lock().unwrap().clone();
        sent
    }

    fn native(key_code: u16, layout_char: Option<&str>) -> gpui::NativeKey {
        gpui::NativeKey {
            key_code: Some(key_code),
            layout_char: layout_char.map(str::to_string),
        }
    }

    #[gpui::test]
    fn the_layout_character_and_the_key_code_decide_key_and_code(cx: &mut gpui::TestAppContext) {
        let none = gpui::Modifiers::default();
        let control = gpui::Modifiers { control: true, ..none };
        let row = |key: &str, code: &str| ("keydown".to_string(), key.to_string(), code.to_string());
        // Cyrillic layout, Ctrl on the C position
        assert_eq!(typed(cx, "c", control, native(0x08, Some("с"))), [row("с", "KeyC")]);
        // AZERTY: the key that types "a" sits where QWERTY has Q
        assert_eq!(typed(cx, "a", none, native(0x0c, Some("a"))), [row("a", "KeyQ")]);
        // Option+s
        assert_eq!(typed(cx, "s", none, native(0x01, Some("ß"))), [row("ß", "KeyS")]);
        // The numpad's Enter is not the main one
        assert_eq!(typed(cx, "enter", none, native(0x4c, None)), [row("Enter", "NumpadEnter")]);
        assert_eq!(typed(cx, "enter", none, native(0x24, None)), [row("Enter", "Enter")]);
        assert_eq!(typed(cx, "left", none, native(0x7b, None)), [row("ArrowLeft", "ArrowLeft")]);
        // No native key: derived from the keystroke, as on a platform without one
        assert_eq!(typed(cx, "a", none, gpui::NativeKey::default()), [row("a", "KeyA")]);
    }

    fn key_down(keystroke: &str, native_key: gpui::NativeKey, is_composing: bool) -> gpui::KeyDownEvent {
        gpui::KeyDownEvent {
            keystroke: gpui::Keystroke::parse(keystroke).unwrap(),
            is_held: false,
            prefer_character_input: false,
            native_key,
            is_composing,
        }
    }

    /// `(type, key, code, keyChar, isComposing)` of everything the window sent for `input`.
    fn sent_for(
        cx: &mut gpui::TestAppContext,
        input: impl FnOnce(&mut Window, &mut App) + 'static,
    ) -> Vec<(String, String, String, Option<String>, bool)> {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let callback: Option<EventCallback> = Some({
            let sent = sent.clone();
            Arc::new(move |payload: EventPayload| {
                sent.lock().unwrap().push((
                    payload.event_type,
                    payload.key.unwrap(),
                    payload.code.unwrap(),
                    payload.key_char,
                    payload.is_composing.unwrap(),
                ));
            })
        });
        let routing = InputRouting::default();
        routing.set_window_keys(true, false, 1);
        let tree = Arc::new(Mutex::new(RetainedTree::new()));
        let window = cx.add_window(|_, _| Blank);
        let _keys = cx.update(|cx| install_key_down(cx, window.window_id(), tree, callback, routing));
        cx.update_window(window.into(), |_, window, cx| input(window, cx)).unwrap();
        let sent = sent.lock().unwrap().clone();
        sent
    }

    #[gpui::test]
    fn a_key_the_input_method_takes_is_sent_as_process_with_its_physical_key(
        cx: &mut gpui::TestAppContext,
    ) {
        let sent = sent_for(cx, |window, cx| {
            let key = key_down("a", native(0x00, Some("a")), true);
            window.dispatch_event(gpui::PlatformInput::ImeKeyDown(key), cx);
        });
        assert_eq!(
            sent,
            [("keydown".into(), "Process".into(), "KeyA".into(), None, true)]
        );
    }

    #[gpui::test]
    fn a_key_the_input_method_declined_is_an_ordinary_keydown_that_says_it_is_composing(
        cx: &mut gpui::TestAppContext,
    ) {
        let sent = sent_for(cx, |window, cx| {
            let key = key_down("left", native(0x7b, None), true);
            window.dispatch_event(gpui::PlatformInput::KeyDown(key), cx);
        });
        assert_eq!(
            sent,
            [("keydown".into(), "ArrowLeft".into(), "ArrowLeft".into(), None, true)]
        );
    }
}
