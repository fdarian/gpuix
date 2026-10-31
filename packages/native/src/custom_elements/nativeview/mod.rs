//! `<nativeview>`: a `div` with a native layer in it, for a library that puts its own AppKit view
//! between the layers GPUI draws (the `<webview>` composition, `docs/fork/webview.md`, with a view
//! that is not a web page).
//!
//! The element is a host container, built as a `div` in every pass (text planning, paint
//! ranks, stacking, culling, accessibility). What this module adds is the first child of that
//! box, a canvas that paints the placeholder of the native surface: the element's own
//! background and border are below the native view and its children above it, as DOM paints a
//! first child. The canvas is absolutely positioned at the padding box, so it takes no room in the
//! layout and the native view fills the padding box.
//!
//! The registry holds a [`NativeviewElement`] per element for its state (`nativeEvents`, the
//! surface and its lifecycle), because a host container has none. The surface is made when the
//! canvas is first prepainted, and `attach` and `detach` tell JS about it (`lifecycle.rs`).
use std::{cell::RefCell, rc::Rc};

use gpui::prelude::*;

use super::{CustomElement, CustomElementFactory, CustomElementRegistry, CustomRenderContext};
use crate::element_tree::EventPayload;
use crate::renderer::EventCallback;

mod geometry;
mod lifecycle;
pub(crate) mod teardown;
#[cfg(feature = "test-support")]
pub(crate) mod test_api;
#[cfg(test)]
mod tests;

use geometry::{captures_pointer, Cut};
use lifecycle::{Availability, Lifecycle};

/// Makes `create_native_surface` fail in the test renderer, as it does on a platform that cannot
/// compose.
#[derive(Default)]
pub(crate) struct SurfacesRefused(pub(crate) bool);

impl gpui::Global for SurfacesRefused {}

/// What happened to the native surfaces of a window, which the test renderer reads: its window is a
/// real offscreen one, whose platform records nothing.
#[derive(Clone, Default)]
pub(crate) struct SurfaceLog(pub(crate) Rc<RefCell<Vec<SurfaceEvent>>>);

impl gpui::Global for SurfaceLog {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SurfaceEvent {
    Created(usize),
    Removed(usize),
}

pub struct NativeviewFactory;

impl CustomElementFactory for NativeviewFactory {
    fn element_type(&self) -> &str {
        "nativeview"
    }

    fn create(&self, id: u64) -> Box<dyn CustomElement> {
        Box::new(NativeviewElement { shared: Rc::new(RefCell::new(Shared::new(id))) })
    }
}

/// The native surface of the element and where it is, from its creation until it is taken down.
struct Attached {
    /// Dropping it removes the surface and the platform view with it.
    surface: gpui::NativeSurface,
    /// The content view of the surface, which is what JS gets.
    handle: usize,
    window: gpui::WindowId,
    log: Option<SurfaceLog>,
}

impl Drop for Attached {
    fn drop(&mut self) {
        if let Some(log) = &self.log {
            log.0.borrow_mut().push(SurfaceEvent::Removed(self.handle));
        }
    }
}

/// What the element, its paint and the registry share. Painting runs in closures that a frame
/// keeps, so this is shared and not owned by the element.
pub(crate) struct Shared {
    id: u64,
    /// The event callback of the last build: what `detach` is told through when nothing is built.
    callback: Option<EventCallback>,
    native_events: bool,
    attached: Option<Attached>,
    lifecycle: Lifecycle,
    availability: Availability,
    /// The tree and element id `teardown` finds this by.
    tracked: Option<(u64, u64)>,
}

impl Shared {
    fn new(id: u64) -> Self {
        Self {
            id,
            callback: None,
            native_events: false,
            attached: None,
            lifecycle: Lifecycle::default(),
            availability: Availability::default(),
            tracked: None,
        }
    }

    /// Makes the surface the first time, and tells JS `attach` once it exists and JS listens
    /// (`listens`). Whether there is a surface to paint.
    fn prepare(&mut self, window: &mut gpui::Window, cx: &gpui::App, listens: bool) -> bool {
        if self.lifecycle.is_closed() {
            return false;
        }
        if self.attached.is_none() {
            if !self.availability.may_try() {
                return false;
            }
            match Self::create_surface(window, cx) {
                Ok(attached) => self.attached = Some(attached),
                Err(reason) => {
                    if let Some(reason) = self.availability.refused(reason) {
                        log::warn!("<nativeview> {} cannot show a native view: {reason}", self.id);
                    }
                    return false;
                }
            }
        }
        let handle = self.attached.as_ref().expect("the surface was made above").handle;
        if self.lifecycle.offer(handle, listens) {
            crate::renderer::emit_event_full(&self.callback, self.id, "attach", |payload| {
                payload.native_handle = Some(handle.to_string());
            });
        }
        true
    }

    fn create_surface(window: &mut gpui::Window, cx: &gpui::App) -> Result<Attached, String> {
        if cx.has_global::<SurfacesRefused>() && cx.global::<SurfacesRefused>().0 {
            return Err("the test renderer refuses native surfaces".to_string());
        }
        let surface = window.create_native_surface().map_err(|error| error.to_string())?;
        let handle = surface
            .platform_handle()
            .map_err(|error| error.to_string())?
            .downcast::<usize>()
            .map_err(|_| "the surface of this window has no content view".to_string())?;
        let log = cx.has_global::<SurfaceLog>().then(|| cx.global::<SurfaceLog>().clone());
        if let Some(log) = &log {
            log.0.borrow_mut().push(SurfaceEvent::Created(*handle));
        }
        Ok(Attached { surface, handle: *handle, window: window.window_handle().window_id(), log })
    }

    /// Ends the element: tells JS `detach` if it was told `attach`, and then drops the surface. The
    /// listener of `detach` runs while the view is still there. Both the element's destruction and
    /// the drop of its state end here, so a window that closes with the element mounted pairs too.
    ///
    /// `detach` is delivered without a window or app (`emit_without_window`): the caller holds
    /// them. The surface goes even if the listener failed, since that is only reported.
    pub(crate) fn take_down(&mut self) {
        let attached = self.attached.take();
        if let Some(handle) = self.lifecycle.close() {
            let attached = attached.as_ref().expect("a surface that attach was told of is still there");
            let payload = EventPayload {
                element_id: self.id as f64,
                event_type: "detach".to_string(),
                native_handle: Some(handle.to_string()),
                ..Default::default()
            };
            crate::events::sync_dispatch::emit_without_window(attached.window, &self.callback, payload);
        }
        drop(attached);
    }

    pub(crate) fn surface_id(&self) -> Option<gpui::CompositionSurfaceId> {
        self.attached.as_ref().map(|attached| attached.surface.id())
    }

    pub(crate) fn warnings(&self) -> usize {
        self.availability.warnings()
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        self.take_down();
        if let Some(key) = self.tracked {
            teardown::forget(key);
        }
    }
}

pub struct NativeviewElement {
    shared: Rc<RefCell<Shared>>,
}

impl NativeviewElement {
    pub(crate) fn shared(&self) -> Rc<RefCell<Shared>> {
        self.shared.clone()
    }
}

impl CustomElement for NativeviewElement {
    /// The registry never builds a `<nativeview>`: it is built as a host container, with
    /// [`layer`] as its first child, and the registry only holds its state. An element that is
    /// built nowhere has no box of its own to show.
    fn render(
        &mut self,
        _ctx: CustomRenderContext,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<crate::renderer::GpuixView>,
    ) -> gpui::AnyElement {
        gpui::Empty.into_any_element()
    }

    fn set_prop(&mut self, key: &str, value: serde_json::Value) {
        if key != "nativeEvents" {
            return;
        }
        self.shared.borrow_mut().native_events = match value {
            serde_json::Value::Null => false,
            serde_json::Value::Bool(on) => on,
            other => {
                log::warn!("<nativeview> nativeEvents must be a boolean, got {other}");
                false
            }
        };
    }

    fn supported_props(&self) -> &'static [&'static str] {
        &["nativeEvents"]
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["attach", "detach"]
    }

    fn destroy(&mut self) {
        self.shared.borrow_mut().take_down();
    }

    fn as_nativeview(&mut self) -> Option<&mut NativeviewElement> {
        Some(self)
    }

    /// The surface outlives the build, and what is built is the canvas, which the tree's props,
    /// events and style decide.
    fn retain_state(&self) -> Option<u64> {
        Some(0)
    }
}

/// What the layer of an element is built from.
pub(crate) struct LayerRequest<'a> {
    pub tree: &'a crate::retained_tree::RetainedTree,
    pub registry: &'a mut CustomElementRegistry,
    pub element: &'a crate::retained_tree::RetainedElement,
    pub style: Option<&'a crate::style::StyleDesc>,
    pub colors: &'a crate::color::ColorContext,
    pub lengths: &'a crate::css::length::LengthContext,
    /// The element clips its content (`overflow` other than `visible` on either axis).
    pub clips: bool,
    /// `pointer-events: none` on the element or an ancestor.
    pub pointer_events_none: bool,
    pub event_callback: &'a Option<EventCallback>,
}

/// The first child of a `<nativeview>`: a canvas over its padding box that makes the native
/// surface when it is first prepainted and paints its placeholder.
pub(crate) fn layer(request: LayerRequest) -> gpui::AnyElement {
    let element = request.element;
    let shared = request
        .registry
        .with_element(element.id, "nativeview", element.custom_props(), |entry| {
            entry.as_nativeview().map(|nativeview| nativeview.shared())
        })
        .flatten()
        .expect("the registry of a view has the nativeview factory");
    shared.borrow_mut().callback = request.event_callback.clone();
    teardown::track((request.tree.id(), element.id), &shared);
    let captures = captures_pointer(shared.borrow().native_events, request.pointer_events_none);
    let listens = element.events().contains("attach");
    let cut = Cut::new(
        request.style.map_or_else(gpui::Corners::default, |style| style.resolved_corner_radii(request.lengths)),
        request
            .style
            .map_or([gpui::Pixels::ZERO; 4], |style| crate::css::border::used_widths(style, request.colors, request.lengths)),
        request.clips,
    );

    // A box that is not a hit target must not put itself in the chain of the pointer through the
    // hitbox of its canvas, whose parent is the box.
    let hitbox_behavior = if request.pointer_events_none {
        gpui::HitboxBehavior::PassThrough
    } else {
        gpui::HitboxBehavior::Normal
    };
    let prepainted = shared.clone();
    let painted = shared;
    gpui::canvas(
        move |bounds, window, cx| {
            let ready = prepainted.borrow_mut().prepare(window, cx, listens);
            ready.then(|| window.insert_hitbox(bounds, hitbox_behavior))
        },
        move |bounds, hitbox, window, _| {
            let Some(hitbox) = hitbox else { return };
            let shared = painted.borrow();
            let Some(attached) = shared.attached.as_ref() else { return };
            let style = gpui::NativeSurfaceStyle {
                corner_radius: cut.corner_radius(bounds.size, window.rem_size()),
                overflow: gpui::NativeSurfaceOverflow::Visible,
                captures_pointer: captures,
            };
            window.paint_native_surface(&attached.surface, &hitbox, bounds, style);
        },
    )
    .keeps_no_position()
    .absolute()
    .top_0()
    .right_0()
    .bottom_0()
    .left_0()
    // Positioned for layout only: a lifted child would paint above the in-flow children of the box.
    .unlifted()
    .into_any_element()
}
