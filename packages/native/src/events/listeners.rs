//! The window-level mouse listeners that feed the dispatcher.
//!
//! They are registered once per frame from one canvas at the top of the root,
//! not per element. Each reads the host elements under the pointer from GPUI
//! while the event is being dispatched, then hands the sample to the view in a
//! deferred update: AppKit dispatches with `GpuixView` leased, so updating it
//! here would panic.
//!
//! A button press is the exception. Its `mousedown` handlers decide whether
//! the focus default runs, so the press is dispatched in the listener, where
//! GPUI is still dispatching the event, and delivered through
//! `sync_dispatch::emit`. That reaches JS and returns what it decided on macOS
//! and in the test renderer; on Windows and Linux GPUI runs off the JS thread,
//! so `emit` queues as every other event does and `preventDefault()` cannot
//! cancel the focus change there. The dispatcher state is therefore shared
//! with the view instead of being reached through it.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gpui::{
    canvas, App, DispatchPhase, ExternalPaths, FileDropEvent, IntoElement, KeyDownEvent, Modifiers,
    MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent, NavigationDirection,
    Pixels, Point, Styled, WeakEntity, Window,
};

use crate::element_tree::EventModifiers;
use crate::renderer::{EventCallback, GpuixView};
use crate::retained_tree::RetainedTree;

use super::dispatcher::{dispatch, Output, PointerAction, PointerSample};
use super::ids::host_hit_chain;
use super::keyboard::InputRouting;
use super::state::{chain_changed, ObservedChain, PointerState};
use super::sync_dispatch;

/// The DOM `MouseEvent.button` for a GPUI button.
pub fn dom_button(button: MouseButton) -> u32 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::Navigate(NavigationDirection::Back) => 3,
        MouseButton::Navigate(NavigationDirection::Forward) => 4,
    }
}

fn sample(
    window: &Window,
    action: PointerAction,
    position: Point<Pixels>,
    modifiers: Modifiers,
) -> PointerSample {
    // GPUI keeps the delta of the latest move, so only a move reads it.
    let movement = match action {
        PointerAction::Move { .. } => window.last_mouse_movement(),
        _ => Point::default(),
    };
    PointerSample {
        action,
        position: (
            f64::from(f32::from(position.x)),
            f64::from(f32::from(position.y)),
        ),
        movement: (
            f64::from(f32::from(movement.x)),
            f64::from(f32::from(movement.y)),
        ),
        modifiers: EventModifiers::from(modifiers),
        hits: host_hit_chain(window),
    }
}

/// Runs one input through the dispatcher.
pub(crate) fn dispatch_sample(
    pointer: &RefCell<PointerState>,
    tree: &Mutex<RetainedTree>,
    sample: &PointerSample,
) -> Output {
    let output = {
        let tree = tree.lock().unwrap();
        dispatch(&mut pointer.borrow_mut(), &tree, sample)
    };
    if output.chain_mismatch {
        log::warn!(
            "GPUI's hit chain and the retained path of the target disagree: \
             a host element is built under a different parent than the tree gives it"
        );
    }
    output
}

/// What a press needs to be dispatched where it is heard.
#[derive(Clone)]
pub(crate) struct PressDispatch {
    pub pointer: Rc<RefCell<PointerState>>,
    pub tree: Arc<Mutex<RetainedTree>>,
    pub callback: Option<EventCallback>,
    pub routing: InputRouting,
}

impl PressDispatch {
    /// Dispatches a press and delivers its events in order. A `mousedown`
    /// handler that cancels it keeps the focus default from running, the same
    /// way GPUI's own handlers do. `pointerdown` does not: in the DOM only
    /// `mousedown` decides that.
    fn press(
        &self,
        view: &WeakEntity<GpuixView>,
        button: u32,
        sample: PointerSample,
        window: &mut Window,
        cx: &mut App,
    ) {
        let output = dispatch_sample(&self.pointer, &self.tree, &sample);
        if output.restyle {
            // The view may be leased by the input being dispatched.
            if let Some(view) = view.upgrade() {
                cx.notify(view.entity_id());
            }
        }
        // A press that hit nothing has no path, and the default blurs.
        let target_path = self
            .pointer
            .borrow()
            .press_path(button)
            .map(<[u64]>::to_vec)
            .unwrap_or_default();
        crate::focus::defer_mouse_down_default(window, cx, &self.routing, target_path);
        let mut mouse_down_prevented = false;
        for payload in output.into_payloads() {
            let is_mouse_down = payload.event_type == "mousedown";
            let outcome = sync_dispatch::emit(window, cx, &self.callback, payload);
            mouse_down_prevented |= is_mouse_down && outcome.default_prevented;
        }
        if mouse_down_prevented {
            window.prevent_default();
        }
    }
}

fn submit(view: &WeakEntity<GpuixView>, sample: PointerSample, cx: &mut App) {
    let view = view.clone();
    cx.defer(move |cx| {
        view.update(cx, |view, cx| view.handle_pointer(sample, cx))
            .ok();
    });
}

/// Registers the frame's pointer listeners and notices a layout that moved under
/// a still pointer.
///
/// Capture phase, so it sees every event whatever the elements below do with
/// it, and before them, as a DOM handler runs before the default action.
///
/// `observed` is the chain of elements the dispatcher last saw under the pointer.
/// When a frame paints a different one without any input, hover must follow: a wheel scroll moves
/// content under the pointer, and a popover can open beneath it.
pub(crate) fn pointer_frame(
    view: WeakEntity<GpuixView>,
    press: PressDispatch,
    observed: ObservedChain,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, cx| {
            let down_view = view.clone();
            let press = press.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let button = dom_button(event.button);
                let action = PointerAction::Down {
                    button,
                    click_count: event.click_count as u32,
                };
                let sample = sample(window, action, event.position, event.modifiers);
                press.press(&down_view, button, sample, window, cx);
            });
            let up_view = view.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                // GPUI turns `FileDropEvent::Submit` into this release, and clears
                // the drag only after the listeners have run.
                if cx.active_drag_value::<ExternalPaths>().is_some() {
                    submit(
                        &up_view,
                        sample(
                            window,
                            PointerAction::FileDrop,
                            event.position,
                            event.modifiers,
                        ),
                        cx,
                    );
                    return;
                }
                let action = PointerAction::Up {
                    button: dom_button(event.button),
                    click_count: event.click_count as u32,
                };
                submit(
                    &up_view,
                    sample(window, action, event.position, event.modifiers),
                    cx,
                );
            });
            let move_view = view.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                // GPUI turns `FileDropEvent::Entered` and `Pending` into a move with
                // the dragged paths as the active drag.
                let action = match cx.active_drag_value::<ExternalPaths>() {
                    Some(dragged) => PointerAction::FileDragMove {
                        paths: dragged.paths().to_vec(),
                    },
                    None => PointerAction::Move {
                        pressed_button: event.pressed_button.map(dom_button),
                    },
                };
                submit(
                    &move_view,
                    sample(window, action, event.position, event.modifiers),
                    cx,
                );
            });
            let leave_view = view.clone();
            window.on_mouse_event(move |event: &FileDropEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                // `Entered`, `Pending` and `Submit` never get here: GPUI rewrote them.
                if matches!(event, FileDropEvent::Exited | FileDropEvent::Ended) {
                    submit(
                        &leave_view,
                        sample(
                            window,
                            PointerAction::FileDragLeave,
                            window.mouse_position(),
                            window.modifiers(),
                        ),
                        cx,
                    );
                }
            });
            let cancel_view = view.clone();
            window.on_root_key_event(move |event: &KeyDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture || event.keystroke.key != "escape" {
                    return;
                }
                submit(
                    &cancel_view,
                    sample(
                        window,
                        PointerAction::CancelDrag,
                        window.mouse_position(),
                        window.modifiers(),
                    ),
                    cx,
                );
            });
            let exit_view = view.clone();
            window.on_mouse_event(move |event: &MouseExitEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                submit(
                    &exit_view,
                    sample(window, PointerAction::Exit, event.position, event.modifiers),
                    cx,
                );
            });

            if chain_changed(&observed, &host_hit_chain(window)) {
                submit(
                    &view,
                    sample(
                        window,
                        PointerAction::Resync,
                        window.mouse_position(),
                        window.modifiers(),
                    ),
                    cx,
                );
            }
        },
    )
    .absolute()
    .w(gpui::px(0.0))
    .h(gpui::px(0.0))
}
