//! `<webview>`: a web page in a native surface of the window, after Electron's tag.
//!
//! The element is a replaced element of 300 x 150 px that paints a placeholder into the scene
//! (`Window::paint_native_surface`); the window composes the page between the GPUI layers
//! drawn before the placeholder and after it. The page is made when the element is first
//! painted, since a surface needs the window, and lives until the element is destroyed.
//!
//! Corners: the page is cut to the `border-radius` of the element when all four corners are
//! the same circle, and square otherwise.
use std::{cell::RefCell, rc::Rc};

use gpui::prelude::*;

use super::{CustomElement, CustomElementFactory, CustomRenderContext};

pub(crate) mod backend;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) mod js_api;
pub(crate) mod test_backend;
#[cfg(all(feature = "test-support", any(target_os = "macos", target_os = "windows")))]
pub(crate) mod test_api;
mod view;

pub(crate) use backend::{WebviewBackend, WebviewState};
pub(crate) use view::WebviewAction;

use backend::{EventEmitter, WebviewEvent};

/// Electron's `webview { display: inline-flex; width: 300px; height: 150px }`.
const DEFAULT_WIDTH: f32 = 300.;
const DEFAULT_HEIGHT: f32 = 150.;

/// Makes the webviews of this app pages a test scripts, in place of web engines.
pub(crate) fn install_test_pages(cx: &mut gpui::App) {
    cx.set_global(test_backend::TestWebviews::default());
}

pub struct WebviewFactory;

impl CustomElementFactory for WebviewFactory {
    fn element_type(&self) -> &str {
        "webview"
    }

    fn create(&self, id: u64) -> Box<dyn CustomElement> {
        Box::new(WebviewElement { shared: Rc::new(RefCell::new(Shared::new(id))) })
    }
}

/// Tab out of the page moves focus on in the window, a turn of the run loop after the page asked:
/// the page asks from WebKit's own callbacks, outside any update of the window.
fn focus_exit(window: &gpui::Window, cx: &gpui::App) -> backend::FocusExit {
    let handle = window.window_handle();
    let app = cx.to_async();
    Rc::new(move |backwards| {
        let executor = app.foreground_executor().clone();
        executor
            .spawn({
                let mut app = app.clone();
                async move {
                    let moved = app.update_window(handle, |_, window, cx| crate::focus::step_focus(window, cx, backwards));
                    if let Err(message) = moved.map_err(|error| error.to_string()).and_then(|result| result) {
                        log::error!("<webview> could not move focus out of its page: {message}");
                    }
                }
            })
            .detach();
    })
}

/// What the element, its paint and the calls from JS share. Painting runs in closures that a
/// frame keeps, so this is shared and not owned by the element.
pub(crate) struct Shared {
    id: u64,
    emitter: Rc<EventEmitter>,
    surface: Option<gpui::NativeSurface>,
    backend: Option<Box<dyn WebviewBackend>>,
    /// The URL to load once there is a page to load it in.
    pending_load: Option<String>,
    /// Why the element cannot show a page, reported once.
    unavailable: Option<String>,
    corner_radii: gpui::Corners<gpui::CornerRadius>,
}

impl Shared {
    fn new(id: u64) -> Self {
        Self {
            id,
            emitter: EventEmitter::new(id),
            surface: None,
            backend: None,
            pending_load: None,
            unavailable: None,
            corner_radii: gpui::Corners::default(),
        }
    }

    /// Makes the surface and the page the first time, and starts the load that waited for them.
    /// Whether there is a page to show.
    fn prepare(&mut self, window: &mut gpui::Window, cx: &gpui::App) -> bool {
        if self.unavailable.is_some() {
            return false;
        }
        if self.backend.is_none() {
            if let Err(message) = self.create_page(window, cx) {
                log::warn!("<webview> {} cannot show a page: {message}", self.id);
                self.unavailable = Some(message);
                return false;
            }
        }
        if let Some(url) = self.pending_load.take() {
            self.start_load(&url);
        }
        true
    }

    fn create_page(&mut self, window: &mut gpui::Window, cx: &gpui::App) -> Result<(), String> {
        let surface = window.create_native_surface().map_err(|error| error.to_string())?;
        let content = surface
            .platform_handle()
            .map_err(|error| error.to_string())?
            .downcast::<usize>()
            .map_err(|_| "the surface of this window has no content view".to_string())?;
        let backend = backend::create_backend(self.id, *content, self.emitter.clone(), focus_exit(window, cx), cx)?;
        self.surface = Some(surface);
        self.backend = Some(backend);
        Ok(())
    }

    /// Loads `url` in the page, which exists. A URL the engine cannot take is a failed load, as
    /// it is when the page itself finds the URL bad.
    fn start_load(&mut self, url: &str) {
        let backend = self.backend.as_mut().expect("a page to load in");
        let loaded = crate::platform::webview_url::classify(url).and_then(|_| backend.load(url));
        if let Err(message) = loaded {
            self.emitter.emit(WebviewEvent::DidFailLoad {
                error_code: crate::platform::webview_url::UNSUPPORTED_URL_ERROR,
                error_description: message,
                validated_url: Some(url.to_string()),
            });
        }
    }

    /// `src` and `loadURL`: waits for the page when it is not made yet.
    pub(crate) fn load(&mut self, url: &str) {
        match self.backend {
            Some(_) => self.start_load(url),
            None => self.pending_load = Some(url.to_string()),
        }
    }

    pub(crate) fn page(&mut self) -> Result<&mut dyn WebviewBackend, String> {
        match self.backend.as_deref_mut() {
            Some(backend) => Ok(backend),
            None => Err(match &self.unavailable {
                Some(message) => format!("The <webview> cannot show a page: {message}"),
                None => "The <webview> has no page yet: it is made when the element is first painted".to_string(),
            }),
        }
    }

    pub(crate) fn surface_id(&self) -> Option<gpui::CompositionSurfaceId> {
        self.surface.as_ref().map(gpui::NativeSurface::id)
    }

    /// The address of the content `NSView` of the page, for a test that reads the views.
    pub(crate) fn content_view(&self) -> Option<usize> {
        let handle = self.surface.as_ref()?.platform_handle().ok()?;
        handle.downcast::<usize>().ok().map(|view| *view)
    }
}

/// The radius the page is cut to, if the corners are one circle.
fn uniform_radius(radii: &gpui::Corners<gpui::CornerRadius>, size: gpui::Size<gpui::Pixels>, rem: gpui::Pixels) -> gpui::Pixels {
    let corners = [radii.top_left, radii.top_right, radii.bottom_right, radii.bottom_left]
        .map(|corner| corner.to_pixels(size, rem));
    uniform_circle(corners)
}

/// The radius of four corners that are all the same circle, zero otherwise. A corner with a
/// vertical radius of zero is circular (`CornerRadius::y`).
fn uniform_circle(corners: [gpui::Size<gpui::Pixels>; 4]) -> gpui::Pixels {
    let circle = |corner: gpui::Size<gpui::Pixels>| {
        let vertical = if corner.height == gpui::Pixels::ZERO { corner.width } else { corner.height };
        (corner.width == vertical).then_some(corner.width)
    };
    match circle(corners[0]) {
        Some(radius) if corners.iter().all(|corner| circle(*corner) == Some(radius)) => radius,
        _ => gpui::Pixels::ZERO,
    }
}

pub struct WebviewElement {
    shared: Rc<RefCell<Shared>>,
}

impl WebviewElement {
    pub(crate) fn shared(&self) -> Rc<RefCell<Shared>> {
        self.shared.clone()
    }
}

impl CustomElement for WebviewElement {
    fn render(
        &mut self,
        ctx: CustomRenderContext,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<crate::renderer::GpuixView>,
    ) -> gpui::AnyElement {
        {
            let mut shared = self.shared.borrow_mut();
            shared.emitter.set_callback(ctx.event_callback.clone());
            shared.corner_radii = match ctx.style {
                Some(style) => style.resolved_corner_radii(&ctx.lengths),
                None => gpui::Corners::default(),
            };
        }
        let prepainted = self.shared.clone();
        let painted = self.shared.clone();
        let page = gpui::canvas(
            move |bounds, window, cx| {
                let ready = prepainted.borrow_mut().prepare(window, cx);
                ready.then(|| window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal))
            },
            move |bounds, hitbox, window, _| {
                let Some(hitbox) = hitbox else { return };
                let shared = painted.borrow();
                let Some(surface) = shared.surface.as_ref() else { return };
                let style = gpui::NativeSurfaceStyle {
                    corner_radius: uniform_radius(&shared.corner_radii, bounds.size, window.rem_size()),
                };
                window.paint_native_surface(surface, &hitbox, bounds, style);
            },
        )
        .size_full();

        let el = gpui::div()
            .id(crate::events::ids::host_element_id(ctx.id))
            .w(gpui::px(DEFAULT_WIDTH))
            .h(gpui::px(DEFAULT_HEIGHT));
        let el = super::custom_surface(el, &ctx);
        let el = crate::layout::container::contain_declared(el, ctx.style);
        crate::accessibility::apply_accessibility(el, ctx.id, ctx.props, Some(gpui::Role::WebView))
            .child(page)
            .into_any_element()
    }

    fn set_prop(&mut self, key: &str, value: serde_json::Value) {
        if key != "src" {
            return;
        }
        match value {
            serde_json::Value::Null => {}
            // `src=""` has nothing to load, where a DOM frame would show `about:blank`.
            serde_json::Value::String(url) if url.is_empty() => {}
            serde_json::Value::String(url) => self.shared.borrow_mut().load(&url),
            other => log::warn!("<webview> src must be a string, got {other}"),
        }
    }

    fn supported_props(&self) -> &'static [&'static str] {
        &["src"]
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["did-start-loading", "did-stop-loading", "did-navigate", "page-title-updated", "did-fail-load"]
    }

    fn destroy(&mut self) {
        let mut shared = self.shared.borrow_mut();
        shared.backend = None;
        shared.surface = None;
        shared.pending_load = None;
    }

    fn as_webview(&mut self) -> Option<&mut WebviewElement> {
        Some(self)
    }

    /// The surface and the page outlive the build, and what is built is the element's box,
    /// the hitbox and the placeholder, which the tree's props and style decide.
    fn retain_state(&self) -> Option<u64> {
        Some(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{px, size};

    fn circle(radius: f32) -> gpui::Size<gpui::Pixels> {
        size(px(radius), px(radius))
    }

    #[test]
    fn four_equal_circles_are_a_radius() {
        assert_eq!(uniform_circle([circle(8.); 4]), px(8.));
    }

    #[test]
    fn square_corners_are_no_radius() {
        assert_eq!(uniform_circle([circle(0.); 4]), px(0.));
    }

    #[test]
    fn corners_that_differ_are_no_radius() {
        assert_eq!(uniform_circle([circle(8.), circle(8.), circle(8.), circle(4.)]), px(0.));
        assert_eq!(uniform_circle([circle(8.), circle(8.), circle(8.), circle(0.)]), px(0.));
    }

    #[test]
    fn an_ellipse_is_no_radius() {
        assert_eq!(uniform_circle([size(px(8.), px(4.)); 4]), px(0.));
    }

    #[test]
    fn a_corner_without_a_vertical_radius_is_a_circle() {
        assert_eq!(uniform_circle([size(px(6.), px(0.)); 4]), px(6.));
    }
}
