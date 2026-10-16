//! The window-level mouse listeners that feed the dispatcher.
//!
//! They are registered once per frame from one canvas at the top of the root,
//! not per element. Each reads the host elements under the pointer from GPUI
//! while the event is being dispatched, then hands the sample to the view in a
//! deferred update: AppKit dispatches with `GpuixView` leased, so updating it
//! here would panic.
//!
//! A button press and a release are the exceptions. A `mousedown` handler
//! decides whether the focus default runs, so the press is dispatched in the
//! listener, where GPUI is still dispatching the event. A `click` handler
//! decides whether a label activates its control, so the release is
//! dispatched once GPUI is done with it. Both are delivered through
//! `sync_dispatch::emit`. That reaches JS and returns what it decided on macOS
//! and in the test renderer; on Windows and Linux GPUI runs off the JS thread,
//! so `emit` queues as every other event does, `preventDefault()` cannot
//! cancel the focus change, and a label always activates its control. The
//! dispatcher state is therefore shared with the view instead of being
//! reached through it.
//!
//! A wheel tick is dispatched the same way, from the capture phase and before
//! any scroller, so a `preventDefault()` on it is known when the scrollers
//! claim the delta (GPUI's scroll routing skips a prevented event). Where
//! `emit` queues, the scroll is unconditional.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

use gpui::{
    canvas, App, DispatchPhase, ExternalPaths, FileDropEvent, IntoElement, KeyDownEvent, Modifiers,
    MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent, NavigationDirection,
    Pixels, Point, ScrollWheelEvent, Styled, TouchPhase, WeakEntity, Window,
};

use crate::element_tree::EventModifiers;
use crate::renderer::GpuixView;
use crate::retained_tree::RetainedTree;

use super::activation::Activation;
use super::app_region::{self, PressOutcome, SimulatedWindowActions, WindowActions};
use super::dispatcher::{button_bit, dispatch, Output, PointerAction, PointerSample, WheelTick};
use super::ids::host_hit_chain;
use super::inline_target::refine;
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
        hits: refine(host_hit_chain(window), position),
        over_drag_region: app_region::in_drag_region(window, position),
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

/// What a press or release needs to be dispatched where it is heard.
#[derive(Clone)]
pub(crate) struct PointerDispatch {
    pub pointer: Rc<RefCell<PointerState>>,
    pub activation: Activation,
    /// Stands in for the window when a press in a drag region asks it to move: the test
    /// renderer's offscreen window has no current mouse event to start a drag from.
    pub window_actions: Option<SimulatedWindowActions>,
}

impl PointerDispatch {
    /// Keeps a press in a window drag region from the page: no event or focus change comes of
    /// it. Where gpuix drives the window, it starts moving or does what a titlebar does on a
    /// double click, and the caller must stop GPUI's propagation; on Windows the press is left
    /// to the OS, with its default prevented (see [`PressOutcome::OsOwned`]). `None` is a press that goes to the page.
    fn take_for_window(
        &self,
        event: &MouseDownEvent,
        button: u32,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<PressOutcome> {
        if !app_region::in_drag_region(window, event.position) {
            return None;
        }
        let mut pointer = self.pointer.borrow_mut();
        let press = app_region::Press {
            button: event.button,
            ctrl: event.modifiers.control,
            click_count: event.click_count,
            other_button_down: pointer.buttons != 0,
        };
        let outcome = app_region::press_outcome(app_region::Platform::CURRENT, &press)?;
        pointer.window_presses |= button_bit(button);
        drop(pointer);
        if let PressOutcome::Perform(action) = outcome {
            match self.window_actions.clone() {
                Some(mut simulated) => simulated.perform(action, event.position, cx),
                None => window.perform(action, event.position, cx),
            }
        }
        Some(outcome)
    }

    /// Whether `button`'s release belongs to the window because it took the press.
    fn take_release_for_window(&self, button: u32) -> bool {
        let mut pointer = self.pointer.borrow_mut();
        let held = pointer.window_presses & button_bit(button) != 0;
        pointer.window_presses &= !button_bit(button);
        held
    }
}

impl PointerDispatch {
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
        let output = dispatch_sample(&self.pointer, &self.activation.tree, &sample);
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
        crate::focus::defer_mouse_down_default(window, cx, &self.activation.routing, target_path);
        let mut mouse_down_prevented = false;
        for payload in output.into_payloads() {
            let is_mouse_down = payload.event_type == "mousedown";
            let outcome = sync_dispatch::emit(window, cx, &self.activation.callback, payload);
            mouse_down_prevented |= is_mouse_down && outcome.default_prevented;
        }
        if mouse_down_prevented {
            window.prevent_default();
        }
    }
}

/// Pixels a line of a mouse wheel is worth in the `wheel` event. Scrollers pick their own
/// rate when they scroll; this is the one JS sees.
const WHEEL_LINE_PIXELS: f32 = 20.0;

fn wheel_tick(event: &ScrollWheelEvent) -> WheelTick {
    let pixels = event.delta.pixel_delta(gpui::px(WHEEL_LINE_PIXELS));
    // GPUI's delta is how far the content moves; the DOM's is how far the scroll position
    // does, so the sign flips. `0.0 -` so a zero delta is +0, not -0.
    WheelTick {
        delta: (
            0.0 - f64::from(f32::from(pixels.x)),
            0.0 - f64::from(f32::from(pixels.y)),
        ),
        precise: event.delta.precise(),
        phase: match event.touch_phase {
            TouchPhase::Started => "started",
            TouchPhase::Moved => "moved",
            TouchPhase::Ended => "ended",
            TouchPhase::Cancelled => "cancelled",
        },
    }
}

impl PointerDispatch {
    /// Dispatches a wheel tick, then cancels the scroll it would cause if a
    /// handler prevented its default.
    fn wheel(&self, sample: PointerSample, window: &mut Window, cx: &mut App) {
        let output = dispatch_sample(&self.pointer, &self.activation.tree, &sample);
        let mut prevented = false;
        for payload in output.into_payloads() {
            let outcome = sync_dispatch::emit(window, cx, &self.activation.callback, payload);
            prevented |= outcome.default_prevented;
        }
        if prevented {
            window.prevent_default();
        }
    }
}

impl PointerDispatch {
    /// Dispatches a release and delivers its events in order. A `click` that
    /// no handler cancelled activates the control of a label on its path, as
    /// the DOM's activation behaviour does: the control takes focus and gets a
    /// `click` of its own.
    fn release(
        &self,
        view: &WeakEntity<GpuixView>,
        sample: PointerSample,
        window: &mut Window,
        cx: &mut App,
    ) {
        let output = dispatch_sample(&self.pointer, &self.activation.tree, &sample);
        if output.restyle {
            if let Some(view) = view.upgrade() {
                cx.notify(view.entity_id());
            }
        }
        for payload in output.into_payloads() {
            let click = (payload.event_type == "click").then(|| payload.clone());
            let outcome = sync_dispatch::emit(window, cx, &self.activation.callback, payload);
            if let Some(click) = click.filter(|_| !outcome.default_prevented) {
                self.activation.activate_label(&click, window, cx);
            }
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
    press: PointerDispatch,
    observed: ObservedChain,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, cx| {
            let down_view = view.clone();
            let hover = press.pointer.clone();
            let release = press.clone();
            let wheel = press.clone();
            let press = press.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let button = dom_button(event.button);
                match press.take_for_window(event, button, window, cx) {
                    Some(PressOutcome::Perform(_)) => {
                        cx.stop_propagation();
                        return;
                    }
                    // Not `stop_propagation`: on Windows a stopped event makes the platform
                    // answer 0, and `DefWindowProc` then skips the drag it is about to start.
                    // The default is what the press must not do on the page's behalf: focus,
                    // the active state, a text selection, a caret.
                    Some(PressOutcome::OsOwned) => {
                        window.prevent_default();
                        return;
                    }
                    None => {}
                }
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
                let button = dom_button(event.button);
                if release.take_release_for_window(button) {
                    return;
                }
                let action = PointerAction::Up {
                    button,
                    click_count: event.click_count as u32,
                };
                let sample = sample(window, action, event.position, event.modifiers);
                let release = release.clone();
                let up_view = up_view.clone();
                // After GPUI's own release handlers, which run with the view leased.
                window.defer(cx, move |window, cx| {
                    release.release(&up_view, sample, window, cx);
                });
            });
            window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let action = PointerAction::Wheel(wheel_tick(event));
                let sample = sample(window, action, event.position, event.modifiers);
                wheel.wheel(sample, window, cx);
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

            // After the draw: GPUI swaps frames only when it ends, and the control areas of the
            // frame being painted are readable only from then on, so a drag region that appeared
            // under a still pointer is seen by this resync and not a frame late.
            window.defer(cx, move |window, cx| {
                let resync = sample(
                    window,
                    PointerAction::Resync,
                    window.mouse_position(),
                    window.modifiers(),
                );
                // What the page would see: a pointer the dispatcher hides has no chain, or this
                // would resync on every frame.
                let seen = if app_region::hides_pointer(&hover.borrow(), &resync) {
                    &[]
                } else {
                    resync.hits.as_slice()
                };
                if chain_changed(&observed, seen) {
                    submit(&view, resync, cx);
                }
            });
        },
    )
    .absolute()
    .w(gpui::px(0.0))
    .h(gpui::px(0.0))
}
