//! What the test renderer shows of the `<nativeview>` elements: where the last composed frame put
//! their surfaces, what the platform made and removed, and the switch for a platform that cannot
//! compose.
use gpui::RecordedLayer;
use napi_derive::napi;

use super::SurfacesRefused;
use crate::renderer::GpuixView;

#[napi(object)]
pub struct NativeviewRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Where the last composed frame put the native view of a `<nativeview>`, in window pixels.
#[napi(object)]
pub struct NativeviewSurfaceReading {
    /// The content view JS got with `attach`, as a decimal string: the index of the attachment
    /// on the test platform.
    pub handle: String,
    /// The whole surface, not cut by its clip.
    pub bounds: NativeviewRect,
    /// What shows of it: for an element that does not clip, the content mask around it, which
    /// can be larger than `bounds`.
    pub clip: NativeviewRect,
    pub opacity: f64,
    pub corner_radius: f64,
    pub captures_pointer: bool,
}

/// What the test platform did to a native surface.
#[napi(object)]
pub struct NativeSurfaceEventReading {
    /// `"created"` or `"removed"`.
    pub kind: String,
    /// The index of the attachment, which is the handle `attach` carries.
    pub handle: String,
}

fn rect(bounds: gpui::Bounds<gpui::Pixels>) -> NativeviewRect {
    NativeviewRect {
        x: f64::from(bounds.origin.x.as_f32()),
        y: f64::from(bounds.origin.y.as_f32()),
        width: f64::from(bounds.size.width.as_f32()),
        height: f64::from(bounds.size.height.as_f32()),
    }
}

impl GpuixView {
    fn nativeview_shared(&mut self, id: u64) -> Result<std::rc::Rc<std::cell::RefCell<super::Shared>>, String> {
        let props = {
            let tree = self.tree.lock().unwrap();
            let element = tree.elements.get(&id).ok_or_else(|| format!("Unknown element id {id}"))?;
            if element.element_type != "nativeview" {
                return Err(format!("This method is only valid on <nativeview>, got <{}>", element.element_type));
            }
            element.custom_props().clone()
        };
        self.custom_registry
            .with_element(id, "nativeview", &props, |element| {
                element.as_nativeview().map(|nativeview| nativeview.shared())
            })
            .flatten()
            .ok_or_else(|| format!("Element {id} is not a <nativeview>"))
    }

    /// Where the last composed frame put the surface of `id`; `None` while it has none or the
    /// frame did not place it (`display: none`, `visibility: hidden`, culled, clipped away).
    pub(crate) fn nativeview_surface(
        &mut self,
        id: u64,
        window: &mut gpui::Window,
    ) -> Result<Option<NativeviewSurfaceReading>, String> {
        let shared = self.nativeview_shared(id)?;
        let Some(surface) = shared.borrow().surface_id() else { return Ok(None) };
        window.present_cached_frame();
        let frame = window.composition_frame().ok_or("No frame has been composed yet")?;
        for layer in &frame.layers {
            let RecordedLayer::Native { surface: layer_surface, attachment, placement, .. } = layer else { continue };
            if *layer_surface != surface {
                continue;
            }
            return Ok(placement.map(|placement| NativeviewSurfaceReading {
                handle: attachment.to_string(),
                bounds: rect(placement.bounds),
                clip: rect(placement.clip),
                opacity: f64::from(placement.opacity),
                corner_radius: f64::from(placement.corner_radius.as_f32()),
                captures_pointer: placement.captures_pointer,
            }));
        }
        Err("The last composed frame has no layer for this nativeview".to_string())
    }

    pub(crate) fn nativeview_warnings(&mut self, id: u64) -> Result<usize, String> {
        Ok(self.nativeview_shared(id)?.borrow().warnings())
    }

    /// The element of every `<nativeview>` that has a surface, by the surface.
    pub(crate) fn nativeview_surfaces(&mut self) -> Vec<(u64, gpui::CompositionSurfaceId)> {
        self.custom_registry
            .nativeviews()
            .into_iter()
            .filter_map(|(id, shared)| shared.borrow().surface_id().map(|surface| (id, surface)))
            .collect()
    }
}

/// The native surfaces the platform made and removed, in order. The last frame is shown first:
/// a removed surface leaves the platform with the frame that no longer places it.
pub(crate) fn native_surface_events(window: &mut gpui::Window) -> Vec<NativeSurfaceEventReading> {
    window.present_cached_frame();
    window
        .native_surface_events()
        .into_iter()
        .map(|event| match event {
            gpui::NativeSurfaceEvent::Created(index) => {
                NativeSurfaceEventReading { kind: "created".to_string(), handle: index.to_string() }
            }
            gpui::NativeSurfaceEvent::Removed(index) => {
                NativeSurfaceEventReading { kind: "removed".to_string(), handle: index.to_string() }
            }
        })
        .collect()
}

pub(crate) fn set_surfaces_available(cx: &mut gpui::App, available: bool) {
    cx.set_global(SurfacesRefused(!available));
}
