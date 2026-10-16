//! Native-to-JS event plumbing that is not one element's own listener:
//! synchronous dispatch and the window-level keyboard path.

pub(crate) mod activation;
pub mod app_region;
pub mod dispatcher;
pub mod element_drag;
pub mod file_drag;
pub mod ids;
pub mod inline_target;
pub(crate) mod key_codes;
pub(crate) mod key_names;
pub(crate) mod keyboard;
pub mod listeners;
pub mod path;
pub mod pointer_lock;
pub mod state;
pub mod state_styles;
pub(crate) mod sync_dispatch;
pub mod synthetic_click;

/// Every GPUIX window hit-tests like the DOM: the pointer hits the topmost
/// element that is not `pointer-events: none` and the elements enclosing it,
/// whatever each one paints. Call it when the window opens, before any frame.
pub fn enable_dom_hit_testing(window: &mut gpui::Window) {
    window.set_dom_hit_testing(true);
}

/// Pointer, mouse and drag event types, which the dispatcher delivers for every
/// element and so are never a custom element's to declare.
pub fn is_pointer_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "click"
            | "auxclick"
            | "dblclick"
            | "contextmenu"
            | "mousedown"
            | "mouseup"
            | "mousemove"
            | "mouseenter"
            | "mouseleave"
            | "mouseover"
            | "mouseout"
            | "mousedownoutside"
            | "pointerdown"
            | "pointermove"
            | "pointerup"
            | "pointerenter"
            | "pointerleave"
            | "pointerover"
            | "pointerout"
            | "pointercancel"
            | "gotpointercapture"
            | "lostpointercapture"
            | "dragstart"
            | "drag"
            | "dragend"
            | "dragenter"
            | "dragover"
            | "dragleave"
            | "drop"
    )
}
