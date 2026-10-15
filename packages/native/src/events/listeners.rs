//! The window-level mouse listeners that feed the dispatcher.
//!
//! They are registered once per frame from one canvas at the top of the root,
//! not per element. Each reads the host elements under the pointer from GPUI
//! while the event is being dispatched, then hands the sample to the view in a
//! deferred update: AppKit dispatches with `GpuixView` leased, so updating it
//! here would panic.

use gpui::{
    canvas, App, DispatchPhase, ExternalPaths, FileDropEvent, IntoElement, KeyDownEvent, Modifiers,
    MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent, NavigationDirection,
    Pixels, Point, Styled, WeakEntity, Window,
};

use crate::element_tree::EventModifiers;
use crate::renderer::GpuixView;

use super::dispatcher::{PointerAction, PointerSample};
use super::ids::{host_hit_chain, host_hit_target};
use super::state::ObservedTarget;

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
/// `observed` is the target the dispatcher last saw. When a frame paints a
/// different one without any input, hover must follow: a wheel scroll moves
/// content under the pointer, and a popover can open beneath it.
pub(crate) fn pointer_frame(
    view: WeakEntity<GpuixView>,
    observed: ObservedTarget,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, cx| {
            let down_view = view.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let action = PointerAction::Down {
                    button: dom_button(event.button),
                    click_count: event.click_count as u32,
                };
                submit(
                    &down_view,
                    sample(window, action, event.position, event.modifiers),
                    cx,
                );
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

            let target = host_hit_target(window);
            if observed.get().is_some_and(|seen| seen != target) {
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
