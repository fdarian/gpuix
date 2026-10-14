//! Where hover and press state comes from until the pointer event model lands.
//!
//! A [`StateSource`] wraps an element in a probe that records, in GPUI's own
//! terms (its hitbox hover, a mouse press that began over it), whether the
//! element is hovered or pressed. The renderer reads that at build time to fold
//! `hover` / `active` into the style a CSS transition compares. A change asks
//! for another frame, so the fold shows up one frame after the pointer moves.
//!
//! Temporary: the event model replaces this with its pointer state, and this
//! file goes with it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, DispatchPhase, Element, ElementId, GlobalElementId, Hitbox,
    HitboxBehavior, InspectorElementId, IntoElement, LayoutId, MouseDownEvent, MouseExitEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Window,
};

#[derive(Clone, Copy, Default, PartialEq)]
struct Flags {
    hovered: bool,
    pressed: bool,
}

#[derive(Clone, Default)]
pub(crate) struct StateSource(Rc<RefCell<HashMap<u64, Flags>>>);

impl StateSource {
    /// `(hovered, pressed)` as of the last paint or pointer event.
    pub(crate) fn flags(&self, id: u64) -> (bool, bool) {
        let flags = self.0.borrow().get(&id).copied().unwrap_or_default();
        (flags.hovered, flags.pressed)
    }

    pub(crate) fn retain(&self, mut keep: impl FnMut(u64) -> bool) {
        self.0.borrow_mut().retain(|id, _| keep(*id));
    }

    /// `child` wrapped in a probe for element `id`.
    pub(crate) fn probe(&self, id: u64, child: AnyElement) -> AnyElement {
        Probe {
            id,
            source: self.clone(),
            child,
        }
        .into_any_element()
    }

    fn update(&self, id: u64, change: impl FnOnce(&mut Flags), window: &mut Window) {
        let mut map = self.0.borrow_mut();
        let flags = map.entry(id).or_default();
        let before = *flags;
        change(flags);
        if *flags != before {
            drop(map);
            window.refresh();
        }
    }
}

struct Probe {
    id: u64,
    source: StateSource,
    child: AnyElement,
}

impl IntoElement for Probe {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Probe {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Hitbox {
        self.child.prepaint(window, cx);
        // After the child, so it sits above the child's own hitboxes without
        // blocking anything: it reads "is the pointer over this box".
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        hitbox: &mut Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);

        let id = self.id;
        let hovered = !cx.has_active_drag() && hitbox.is_hovered(window);
        // A layout change under a stationary pointer produces no mouse event.
        let source = self.source.clone();
        window.defer(cx, move |window, _| {
            source.update(id, |flags| flags.hovered = hovered, window);
        });

        window.on_mouse_event({
            let source = self.source.clone();
            let hitbox = hitbox.clone();
            move |_: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble {
                    let hovered = !cx.has_active_drag() && hitbox.is_hovered(window);
                    source.update(id, |flags| flags.hovered = hovered, window);
                }
            }
        });
        // The pointer can leave the window without a last move.
        window.on_mouse_event({
            let source = self.source.clone();
            move |_: &MouseExitEvent, phase, window, _| {
                if phase == DispatchPhase::Bubble {
                    source.update(id, |flags| flags.hovered = false, window);
                }
            }
        });
        window.on_mouse_event({
            let source = self.source.clone();
            let hitbox = hitbox.clone();
            move |_: &MouseDownEvent, phase, window, _| {
                if phase == DispatchPhase::Bubble
                    && !window.default_prevented()
                    && hitbox.is_hovered(window)
                {
                    source.update(id, |flags| flags.pressed = true, window);
                }
            }
        });
        window.on_mouse_event({
            let source = self.source.clone();
            move |_: &MouseUpEvent, phase, window, _| {
                if phase == DispatchPhase::Capture {
                    source.update(id, |flags| flags.pressed = false, window);
                }
            }
        });
    }
}
