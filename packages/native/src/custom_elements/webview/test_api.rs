//! What the test renderer shows of the `<webview>` elements: the commands they issued, the
//! pages a test scripts, and where the window put their surfaces.
use gpui::RecordedLayer;
use napi_derive::napi;

use super::backend::WebviewEvent;
use super::test_backend::{TestWebviews, WebviewCommand};
use crate::renderer::GpuixView;

#[napi(object)]
pub struct WebviewCommandReading {
    /// `loadURL`, `reload`, `stop`, `goBack`, `goForward`, `executeJavaScript` or
    /// `setFirstResponder`.
    pub name: String,
    /// The URL, the code, or `"true"` / `"false"` for the first responder.
    pub argument: Option<String>,
}

impl From<WebviewCommand> for WebviewCommandReading {
    fn from(command: WebviewCommand) -> Self {
        let (name, argument) = match command {
            WebviewCommand::Load(url) => ("loadURL", Some(url)),
            WebviewCommand::Reload => ("reload", None),
            WebviewCommand::Stop => ("stop", None),
            WebviewCommand::GoBack => ("goBack", None),
            WebviewCommand::GoForward => ("goForward", None),
            WebviewCommand::ExecuteJavaScript(code) => ("executeJavaScript", Some(code)),
            WebviewCommand::SetFirstResponder(focused) => ("setFirstResponder", Some(focused.to_string())),
        };
        Self { name: name.to_string(), argument }
    }
}

/// Where the last composed frame put the surface of a webview, in window pixels.
#[napi(object)]
pub struct WebviewSurfaceReading {
    /// The surface is shown: it was painted and something of it is not clipped away.
    pub visible: bool,
    /// The whole surface, not cut by its clip.
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// What shows of it. Zero while it is not visible.
    pub clip_x: f64,
    pub clip_y: f64,
    pub clip_width: f64,
    pub clip_height: f64,
    pub opacity: f64,
    pub corner_radius: f64,
    /// The position of the layer among the frame's layers, bottom to top.
    pub layer: u32,
}

/// One layer of the last composed frame.
#[napi(object)]
pub struct CompositionLayerReading {
    /// `"gpui"` or `"native"`.
    pub kind: String,
    /// The window's own GPUI layer, the one below every surface.
    pub is_base: bool,
    /// For a GPUI layer: the primitives and groups of the scene it draws.
    pub primitives: u32,
    /// For a native layer: the element of its webview.
    pub webview: Option<f64>,
    /// For a native layer: whether it shows.
    pub visible: bool,
}

/// The container view AppKit applied the placement to, in window points from the top left.
#[napi(object)]
pub struct WebviewNativeViewReading {
    pub hidden: bool,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub content_x: f64,
    pub content_y: f64,
    pub content_width: f64,
    pub content_height: f64,
    pub opacity: f64,
    /// A corner radius is cut into it.
    pub masked: bool,
}

fn pixels(value: gpui::Pixels) -> f64 {
    f64::from(value.as_f32())
}

/// Shows the last frame the window drew: the test window has no display link to present it, and
/// the platform composes the surfaces and records what it composed when a frame is presented.
fn present(window: &mut gpui::Window) {
    window.present_cached_frame();
}

fn test_pages(cx: &gpui::App) -> Result<&TestWebviews, String> {
    if cx.has_global::<TestWebviews>() {
        Ok(cx.global::<TestWebviews>())
    } else {
        Err("This renderer has no scripted webview pages".to_string())
    }
}

impl GpuixView {
    pub(crate) fn webview_commands(&mut self, id: u64, cx: &gpui::App) -> Result<Vec<WebviewCommandReading>, String> {
        let commands = test_pages(cx)?.with_page(id, |page| page.commands.clone())?;
        Ok(commands.into_iter().map(WebviewCommandReading::from).collect())
    }

    /// Makes the page of `id` report `event`, after `change` moved its state.
    pub(crate) fn webview_report(
        &mut self,
        id: u64,
        cx: &gpui::App,
        change: impl FnOnce(&mut super::backend::WebviewState),
        event: WebviewEvent,
    ) -> Result<(), String> {
        test_pages(cx)?.report(id, change, event)
    }

    pub(crate) fn webview_settle_script(
        &mut self,
        id: u64,
        cx: &gpui::App,
        outcome: super::backend::ScriptOutcome,
    ) -> Result<(), String> {
        test_pages(cx)?.settle_script(id, outcome)
    }

    pub(crate) fn webview_surface(
        &mut self,
        id: u64,
        window: &mut gpui::Window,
    ) -> Result<Option<WebviewSurfaceReading>, String> {
        let shared = self.webview_shared(id)?;
        let Some(surface) = shared.borrow().surface_id() else { return Ok(None) };
        present(window);
        let frame = window.composition_frame().ok_or("No frame has been composed yet")?;
        for layer in &frame.layers {
            if let RecordedLayer::Native { surface: layer_surface, placement, z, .. } = layer {
                if *layer_surface != surface {
                    continue;
                }
                let reading = match placement {
                    Some(placement) => WebviewSurfaceReading {
                        visible: true,
                        x: pixels(placement.bounds.origin.x),
                        y: pixels(placement.bounds.origin.y),
                        width: pixels(placement.bounds.size.width),
                        height: pixels(placement.bounds.size.height),
                        clip_x: pixels(placement.clip.origin.x),
                        clip_y: pixels(placement.clip.origin.y),
                        clip_width: pixels(placement.clip.size.width),
                        clip_height: pixels(placement.clip.size.height),
                        opacity: f64::from(placement.opacity),
                        corner_radius: pixels(placement.corner_radius),
                        layer: *z as u32,
                    },
                    None => WebviewSurfaceReading {
                        visible: false,
                        x: 0.,
                        y: 0.,
                        width: 0.,
                        height: 0.,
                        clip_x: 0.,
                        clip_y: 0.,
                        clip_width: 0.,
                        clip_height: 0.,
                        opacity: 0.,
                        corner_radius: 0.,
                        layer: *z as u32,
                    },
                };
                return Ok(Some(reading));
            }
        }
        Err("The last composed frame has no layer for this webview".to_string())
    }

    pub(crate) fn composition_layers(
        &mut self,
        window: &mut gpui::Window,
    ) -> Result<Vec<CompositionLayerReading>, String> {
        present(window);
        let frame = window.composition_frame().ok_or("No frame has been composed yet")?;
        let webviews: Vec<(u64, Option<gpui::CompositionSurfaceId>)> = self
            .custom_registry
            .webviews()
            .into_iter()
            .map(|(id, shared)| (id, shared.borrow().surface_id()))
            .collect();
        Ok(frame
            .layers
            .iter()
            .map(|layer| match layer {
                RecordedLayer::Gpui { is_base, primitives, .. } => CompositionLayerReading {
                    kind: "gpui".to_string(),
                    is_base: *is_base,
                    primitives: *primitives as u32,
                    webview: None,
                    visible: true,
                },
                RecordedLayer::Native { surface, placement, .. } => CompositionLayerReading {
                    kind: "native".to_string(),
                    is_base: false,
                    primitives: 0,
                    webview: webviews
                        .iter()
                        .find(|(_, candidate)| *candidate == Some(*surface))
                        .map(|(id, _)| *id as f64),
                    visible: placement.is_some(),
                },
            })
            .collect())
    }

    /// The views AppKit holds for the webview, to check that the placements the frame recorded
    /// were applied.
    #[cfg(target_os = "macos")]
    pub(crate) fn webview_native_view(
        &mut self,
        id: u64,
        window: &mut gpui::Window,
    ) -> Result<WebviewNativeViewReading, String> {
        present(window);
        let shared = self.webview_shared(id)?;
        let content = shared.borrow().content_view().ok_or("The webview has no surface yet")?;
        let view = crate::platform::webview_macos::inspect::applied(content);
        Ok(WebviewNativeViewReading {
            hidden: view.hidden,
            x: view.x,
            y: view.y,
            width: view.width,
            height: view.height,
            content_x: view.content_x,
            content_y: view.content_y,
            content_width: view.content_width,
            content_height: view.content_height,
            opacity: view.opacity,
            masked: view.masked,
        })
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn webview_native_view(
        &mut self,
        _id: u64,
        _window: &mut gpui::Window,
    ) -> Result<WebviewNativeViewReading, String> {
        Err(super::js_api::UNSUPPORTED.to_string())
    }
}
