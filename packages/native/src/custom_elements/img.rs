/// Image custom elements for raster images and tintable SVG icons.
///
/// This provides a native `<img>` for GPUIX React apps while keeping the same
/// custom-element prop pipeline (`setCustomProp`/`custom_props`).
///
/// HTTP(S) `src` is a GPUI URI resource. GPUI fetches it through the app
/// `HttpClient` on a background task and paints once decode finishes. A
/// definite `width` and `height` keep the layout box stable during that load.
///
/// `load` and `error` fire once per `src`. Every kind of source resolves to a
/// `RenderImage` here, in `render`, so the element knows the outcome and the
/// natural size. A `blob:` src is fetched by the JS layer (`js/blob-images.ts`),
/// which hands the bytes to `setImage`, or reports failure through `srcError`.
///
/// Every image the element paints goes into the sprite atlas. The element
/// remembers it (`painted`, or the `Render` source) and hands it back to be
/// dropped when `src` replaces it or the element goes away.
use super::{CustomElement, CustomElementFactory, CustomRenderContext};
use base64::Engine as _;

pub struct ImgFactory;

pub struct SvgFactory;

impl CustomElementFactory for SvgFactory {
    fn element_type(&self) -> &str {
        "svg"
    }

    fn create(&self, _id: u64) -> Box<dyn CustomElement> {
        Box::new(SvgElement::default())
    }
}

impl CustomElementFactory for ImgFactory {
    fn element_type(&self) -> &str {
        "img"
    }

    fn create(&self, _id: u64) -> Box<dyn CustomElement> {
        Box::new(ImgElement::default())
    }
}

#[derive(Debug, Clone)]
enum ImgObjectFit {
    Fill,
    Contain,
    Cover,
    ScaleDown,
    None,
}

impl Default for ImgObjectFit {
    fn default() -> Self {
        Self::Contain
    }
}

impl ImgObjectFit {
    fn from_str(value: &str) -> Self {
        match value {
            "fill" => Self::Fill,
            "cover" => Self::Cover,
            "scaleDown" => Self::ScaleDown,
            "none" => Self::None,
            _ => Self::Contain,
        }
    }

    fn as_gpui(&self) -> gpui::ObjectFit {
        match self {
            Self::Fill => gpui::ObjectFit::Fill,
            Self::Contain => gpui::ObjectFit::Contain,
            Self::Cover => gpui::ObjectFit::Cover,
            Self::ScaleDown => gpui::ObjectFit::ScaleDown,
            Self::None => gpui::ObjectFit::None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) enum ImgSource {
    #[default]
    Empty,
    Path(std::path::PathBuf),
    Uri(gpui::SharedUri),
    Data(std::sync::Arc<gpui::Image>),
    Render(std::sync::Arc<gpui::RenderImage>),
    /// A `blob:` src whose bytes the JS layer is still fetching.
    Pending,
    Failed(String),
}

#[derive(Debug, Clone, Default)]
pub struct ImgElement {
    source: ImgSource,
    object_fit: ImgObjectFit,
    alt: String,
    /// Images `src` replaced, waiting for `drop_image` at the next render.
    dropped: Vec<std::sync::Arc<gpui::RenderImage>>,
    /// The image last handed to paint for a `Path`, `Uri` or `Data` source.
    painted: Option<std::sync::Arc<gpui::RenderImage>>,
    /// The decoded `Data` source, decoded once on first paint.
    decoded: Option<Result<std::sync::Arc<gpui::RenderImage>, String>>,
    /// A src was set and its `load` or `error` has not fired yet.
    awaiting_outcome: bool,
    /// The `src` prop has a value. Without one, the image is whatever
    /// `setImagePixels` or `setImage` last put there, and `src: null` has
    /// nothing to take away from it.
    has_src: bool,
    /// Counts the changes of source, the ones that do not come with a prop too (`setImage`).
    epoch: u64,
    /// The image last rendered has more than one frame: gpui steps it while painting.
    animated: bool,
}

impl ImgElement {
    fn load_src(&mut self, src: &str) {
        let src = src.trim();
        self.dropped.extend(self.live_image());
        self.painted = None;
        self.decoded = None;
        self.awaiting_outcome = !src.is_empty();
        self.source = resolve_src(src);
        self.epoch += 1;
    }

    /// The image this element has in the atlas, if any.
    fn live_image(&self) -> Option<std::sync::Arc<gpui::RenderImage>> {
        match &self.source {
            ImgSource::Render(image) => Some(image.clone()),
            _ => self.painted.clone(),
        }
    }

    /// Fires `load` or `error` once per src, after the outcome is known. Deferred
    /// so a handler that sets state never re-enters the build in progress.
    fn report(
        &mut self,
        id: u64,
        events: &std::collections::HashSet<String>,
        callback: &Option<crate::renderer::EventCallback>,
        cx: &mut gpui::App,
        outcome: Result<(u32, u32), String>,
    ) {
        if !std::mem::take(&mut self.awaiting_outcome) {
            return;
        }
        let event = if outcome.is_ok() { "load" } else { "error" };
        if !events.contains(event) {
            return;
        }
        let callback = callback.clone();
        cx.defer(move |_| {
            crate::renderer::emit_event_full(&callback, id, event, |payload| match outcome {
                Ok((width, height)) => {
                    payload.natural_width = Some(width as f64);
                    payload.natural_height = Some(height as f64);
                }
                Err(message) => payload.value = Some(message),
            });
        });
    }

    /// Turns the source into a decoded image, so the outcome and the natural size
    /// are known for `load` and `error`. Frees the images `src` replaced.
    fn resolve_image(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> Resolved {
        for image in self.dropped.drain(..) {
            window.drop_image(image).ok();
        }
        let loaded = match &self.source {
            ImgSource::Empty => return Resolved::NoSrc,
            ImgSource::Pending => return Resolved::Loading,
            ImgSource::Failed(message) => Err(message.clone()),
            ImgSource::Render(image) => Ok(image.clone()),
            ImgSource::Path(path) => {
                let resource = gpui::Resource::Path(path.as_path().into());
                match window.use_asset::<gpui::ImageAssetLoader>(&resource, cx) {
                    Some(result) => result.map_err(|error| error.to_string()),
                    None => return Resolved::Loading,
                }
            }
            ImgSource::Uri(uri) => {
                let resource = gpui::Resource::Uri(uri.clone());
                match window.use_asset::<gpui::ImageAssetLoader>(&resource, cx) {
                    Some(result) => result.map_err(|error| error.to_string()),
                    None => return Resolved::Loading,
                }
            }
            ImgSource::Data(image) => {
                let renderer = cx.svg_renderer();
                self.decoded
                    .get_or_insert_with(|| {
                        image.to_image_data(renderer).map_err(|error| error.to_string())
                    })
                    .clone()
            }
        };
        Resolved::Done(loaded)
    }

}

/// The intrinsic size of a decoded image, in CSS px. An SVG's bitmap is rasterised at a multiple of
/// its size, which the image carries.
fn natural_size(image: &gpui::RenderImage) -> (u32, u32) {
    let size = image.render_size(0);
    (f32::from(size.width).round() as u32, f32::from(size.height).round() as u32)
}

/// Where an `<img>`'s source stands.
enum Resolved {
    NoSrc,
    /// A fetch or a `blob:` read is still going.
    Loading,
    Done(Result<std::sync::Arc<gpui::RenderImage>, String>),
}

/// The source `src` names.
pub(crate) fn resolve_src(src: &str) -> ImgSource {
    if src.is_empty() {
        ImgSource::Empty
    } else if src.starts_with("blob:") {
        ImgSource::Pending
    } else if src.starts_with("data:") {
        // TODO: Replace JSON data URLs with binary mutations to keep base64 decoding off paint.
        match decode_image_data_url(src) {
            Some((format, bytes)) => {
                ImgSource::Data(std::sync::Arc::new(gpui::Image::from_bytes(format, bytes)))
            }
            None => ImgSource::Failed("invalid data URL".to_string()),
        }
    } else if let Some(uri) = http_image_uri(src) {
        ImgSource::Uri(uri)
    } else if src
        .get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file://"))
    {
        match file_url_path(src) {
            Some(path) => ImgSource::Path(path),
            None => ImgSource::Failed(format!("invalid file URL {src}")),
        }
    } else {
        ImgSource::Path(std::path::PathBuf::from(src))
    }
}

/// `file:///a/b%20c.png` as a path. Accepts an empty or `localhost` host. A query
/// or fragment is not part of the path.
pub(super) fn file_url_path(src: &str) -> Option<std::path::PathBuf> {
    let rest = src.get(7..)?;
    let rest = &rest[..rest.find(['?', '#']).unwrap_or(rest.len())];
    let path = match rest.find('/') {
        Some(0) => rest,
        Some(index) if rest[..index].eq_ignore_ascii_case("localhost") => &rest[index..],
        _ => return None,
    };
    let decoded = String::from_utf8(percent_decode(path)).ok()?;
    // `file:///C:/x.png` carries a slash before the drive letter.
    let bytes = decoded.as_bytes();
    let path = if cfg!(windows) && bytes.len() > 2 && bytes[0] == b'/' && bytes[2] == b':' {
        &decoded[1..]
    } else {
        decoded.as_str()
    };
    Some(path.into())
}

fn http_image_uri(src: &str) -> Option<gpui::SharedUri> {
    let scheme_end = src.find("://")?;
    let scheme = &src[..scheme_end];
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    (src.len() > scheme_end + 3).then(|| gpui::SharedUri::from(src.to_string()))
}

/// Install the GPUI HTTP client so `<img src="https://…">` can fetch.
///
/// Web already gets `fetch` from `gpui_platform::single_threaded_web`. Desktop
/// Application defaults to `NullHttpClient`, which fails every URI load.
pub fn init(cx: &mut gpui::App) {
    #[cfg(not(target_family = "wasm"))]
    match reqwest_client::ReqwestClient::user_agent("gpuix") {
        Ok(client) => cx.set_http_client(std::sync::Arc::new(client)),
        Err(error) => log::error!(
            "GPUIX HTTP client failed to start; <img src=\"http…\"> will not load: {error:#}"
        ),
    }
    #[cfg(target_family = "wasm")]
    let _ = cx;
}

#[cfg(test)]
mod tests {
    use super::{
        decode_image_data_url, file_url_path, http_image_uri, natural_size, render_image_from_pixels,
        CustomElement, ImgElement, ImgSource, PixelFormat,
    };
    use serde_json::json;
    use std::sync::Arc;

    fn image() -> Arc<gpui::RenderImage> {
        render_image_from_pixels(1, 1, vec![0; 4], PixelFormat::Rgba).unwrap()
    }

    fn has(images: &[Arc<gpui::RenderImage>], image: &Arc<gpui::RenderImage>) -> bool {
        images.iter().any(|held| Arc::ptr_eq(held, image))
    }

    #[test]
    fn a_painted_image_is_dropped_when_src_changes() {
        let mut element = ImgElement::default();
        element.set_prop("src", json!("data:image/svg+xml,%3Csvg%2F%3E"));
        let first = image();
        element.painted = Some(first.clone());
        element.set_prop("src", json!("/tmp/other.png"));
        assert!(element.painted.is_none());
        let held = element.take_held_images();
        assert_eq!(held.len(), 1);
        assert!(has(&held, &first));
        assert!(element.take_held_images().is_empty());
    }

    #[test]
    fn every_image_replaced_before_a_render_is_kept_for_dropping() {
        let mut element = ImgElement::default();
        element.set_prop("src", json!("/tmp/a.png"));
        let first = image();
        element.painted = Some(first.clone());
        element.set_prop("src", json!("/tmp/b.png"));
        let second = image();
        element.painted = Some(second.clone());
        element.set_prop("src", json!("/tmp/c.png"));
        let third = image();
        element.painted = Some(third.clone());
        let held = element.take_held_images();
        assert_eq!(held.len(), 3);
        assert!(has(&held, &first) && has(&held, &second) && has(&held, &third));
    }

    #[test]
    fn a_live_image_is_held_once_and_dropped_when_src_replaces_it() {
        let mut element = ImgElement::default();
        let live = image();
        assert!(element.replace_live_image(live.clone()).is_none());
        assert_eq!(element.take_held_images().len(), 1);
        element.set_prop("src", json!("/tmp/a.png"));
        let held = element.take_held_images();
        assert_eq!(held.len(), 1);
        assert!(has(&held, &live));
    }

    #[test]
    fn removing_src_drops_the_image_a_blob_src_put_there() {
        let mut element = ImgElement::default();
        element.set_prop("src", json!("blob:one"));
        let blob = image();
        element.replace_live_image(blob.clone());
        element.set_prop("src", serde_json::Value::Null);
        assert!(matches!(element.source, ImgSource::Empty));
        assert!(has(&element.take_held_images(), &blob));
    }

    #[test]
    fn a_live_image_survives_a_null_src_when_src_was_never_set() {
        let mut element = ImgElement::default();
        element.replace_live_image(image());
        element.set_prop("src", serde_json::Value::Null);
        assert!(matches!(element.source, ImgSource::Render(_)));
        element.set_prop("src", json!("/tmp/a.png"));
        element.set_prop("src", serde_json::Value::Null);
        assert!(matches!(element.source, ImgSource::Empty));
    }

    #[test]
    fn nothing_is_held_before_an_image_is_painted() {
        let mut element = ImgElement::default();
        element.set_prop("src", json!("https://example.test/a.png"));
        assert!(element.take_held_images().is_empty());
    }

    #[test]
    fn natural_size_follows_the_scale_the_image_was_rasterised_at() {
        let svg = gpui::SvgRenderer::new(Arc::new(()))
            .render_single_frame(
                br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="6"/>"#,
                1.0,
            )
            .unwrap();
        assert_eq!(svg.size(0).width.0, 20);
        assert_eq!(natural_size(&svg), (10, 6));
        let raster = render_image_from_pixels(3, 2, vec![0; 24], PixelFormat::Rgba).unwrap();
        assert_eq!(natural_size(&raster), (3, 2));
    }

    #[test]
    fn each_blob_failure_applies_while_the_blob_is_pending() {
        let mut element = ImgElement::default();
        element.set_prop("src", json!("blob:one"));
        element.set_prop("srcError", json!({ "message": "boom", "ticket": 1 }));
        assert!(matches!(&element.source, ImgSource::Failed(message) if message == "boom"));
        element.set_prop("src", json!("blob:two"));
        assert!(matches!(element.source, ImgSource::Pending));
        element.set_prop("srcError", json!({ "message": "boom", "ticket": 2 }));
        assert!(matches!(&element.source, ImgSource::Failed(message) if message == "boom"));
    }

    #[test]
    fn file_urls_become_paths() {
        assert_eq!(file_url_path("file:///tmp/a.png"), Some("/tmp/a.png".into()));
        assert_eq!(file_url_path("FILE:///tmp/a%20b.png"), Some("/tmp/a b.png".into()));
        assert_eq!(file_url_path("file://localhost/tmp/a.png"), Some("/tmp/a.png".into()));
        assert_eq!(file_url_path("file://example.test/tmp/a.png"), None);
        assert_eq!(file_url_path("file://relative.png"), None);
        assert_eq!(file_url_path("file:///tmp/a.png?v=2#top"), Some("/tmp/a.png".into()));
        assert_eq!(file_url_path("file:///tmp/a%23b.png#x"), Some("/tmp/a#b.png".into()));
        assert_eq!(file_url_path("file://localhost/tmp/a.png#x"), Some("/tmp/a.png".into()));
    }

    #[test]
    fn a_data_url_decodes_base64_as_browsers_do() {
        let decode = |src: &str| decode_image_data_url(src).map(|(_, bytes)| bytes);
        let plain = decode("data:image/svg+xml;base64,PHN2Zy8+");
        assert_eq!(plain.as_deref(), Some(&b"<svg/>"[..]));
        for src in [
            "data:image/svg+xml;base64,PHN2\nZy8+",
            "data:image/svg+xml;base64, PHN2 Zy8+ ",
            "data:image/svg+xml;base64,PHN2%0AZy8%2B",
        ] {
            assert_eq!(decode(src), plain, "{src}");
        }
        // Padding is optional, and the unused bits of the last character are ignored.
        assert_eq!(decode("data:image/svg+xml;base64,PHN2Zy8").as_deref(), Some(&b"<svg/"[..]));
        assert_eq!(decode("data:image/svg+xml;base64,PHN2Zy9=").as_deref(), Some(&b"<svg/"[..]));
        assert_eq!(decode("data:image/svg+xml;base64,PA").as_deref(), Some(&b"<"[..]));
        // Anything else is an error.
        for src in [
            "data:image/svg+xml;base64,P",
            "data:image/svg+xml;base64,PHN2Zy8*",
            "data:image/svg+xml;base64,PH=N2Zy8+",
            "data:image/svg+xml;base64,PA=a",
            "data:image/svg+xml;base64,PHN2Zy8+====",
        ] {
            assert_eq!(decode(src), None, "{src}");
        }
    }

    #[test]
    fn only_http_urls_become_uri_sources() {
        assert!(http_image_uri("https://example.test/a.png").is_some());
        assert!(http_image_uri("HTTP://localhost:9/a.png").is_some());
        assert!(http_image_uri("HTTPS://example.test/a.png").is_some());
        assert!(http_image_uri("/tmp/a.png").is_none());
        assert!(http_image_uri("data:image/png;base64,xx").is_none());
        assert!(http_image_uri("file:///tmp/a.png").is_none());
        assert!(http_image_uri("https://").is_none());
        assert!(http_image_uri("http://").is_none());
    }
}

fn img_fallback(ctx: &CustomRenderContext, alt: &str, message: &str) -> gpui::AnyElement {
    use gpui::prelude::*;

    let mut fallback = super::custom_surface(
        gpui::div()
            .id(crate::events::ids::host_element_id(ctx.id))
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x1f2230ff))
            .border(gpui::px(1.0))
            .border_color(gpui::rgba(0x5d6481ff))
            .text_color(gpui::rgba(0xa4accdff)),
        ctx,
    );
    fallback = crate::layout::container::contain_declared(fallback, ctx.style);
    fallback = crate::accessibility::apply_accessibility(
        fallback,
        ctx.id,
        ctx.props,
        Some(gpui::Role::Image),
    );
    fallback = crate::accessibility::apply_image_label(fallback, ctx.props, alt);
    fallback
        .child(ctx.chrome_text(message.to_string(), None))
        .into_any_element()
}

/// An `<img>` with nothing to show yet: it keeps its box and bounds while a
/// source loads.
fn img_blank(ctx: &CustomRenderContext) -> gpui::AnyElement {
    use gpui::prelude::*;

    let blank = super::custom_surface(
        gpui::div().id(crate::events::ids::host_element_id(ctx.id)),
        ctx,
    );
    let blank = crate::layout::container::contain_declared(blank, ctx.style);
    crate::accessibility::apply_accessibility(blank, ctx.id, ctx.props, Some(gpui::Role::Image))
        .into_any_element()
}

impl CustomElement for ImgElement {
    fn render(
        &mut self,
        ctx: CustomRenderContext,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<crate::renderer::GpuixView>,
    ) -> gpui::AnyElement {
        use gpui::prelude::*;

        let image = match self.resolve_image(window, cx) {
            Resolved::NoSrc => return img_fallback(&ctx, &self.alt, "img: no src"),
            Resolved::Loading => return img_blank(&ctx),
            Resolved::Done(Err(message)) => {
                self.report(ctx.id, ctx.events, ctx.event_callback, cx, Err(message));
                return img_fallback(&ctx, &self.alt, "img: load failed");
            }
            Resolved::Done(Ok(image)) => image,
        };
        let natural = natural_size(&image);
        self.report(ctx.id, ctx.events, ctx.event_callback, cx, Ok(natural));
        self.animated = image.frame_count() > 1;
        if !matches!(self.source, ImgSource::Render(_)) {
            self.painted = Some(image.clone());
        }
        let el = gpui::img(image);
        // The id is what makes gpui's `ImgState` persist. Without it `Img` has no
        // `GlobalElementId`, so the animated-GIF frame index and the delayed
        // loading state are rebuilt from scratch on every frame and an animation
        // never advances past frame zero.
        let mut el = el
            .object_fit(self.object_fit.as_gpui())
            .with_fallback(|| {
                gpui::div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(gpui::rgba(0x1f2230ff))
                    .border(gpui::px(1.0))
                    .border_color(gpui::rgba(0x5d6481ff))
                    .text_color(gpui::rgba(0xa4accdff))
                    .child("img: load failed")
                    .into_any_element()
            })
            .id(crate::events::ids::host_element_id(ctx.id));
        el = crate::events::ids::host_hitbox(el, ctx.pointer_events_none);
        el = crate::events::app_region::mark(el, ctx.app_region);

        if let Some(style) = ctx.style {
            el = crate::renderer::apply_interactive_styles(el, style, &ctx.colors, &ctx.lengths);
            // GPUI fills `aspect_ratio` from the bitmap once it loads. That
            // overrides a definite height and jumps the box. A CSS `<img>` with
            // both width and height keeps that box; `objectFit` paints inside it.
            if let (
                Some(crate::style::DimensionValue::Pixels(width)),
                Some(crate::style::DimensionValue::Pixels(height)),
            ) = (style.width.as_ref(), style.height.as_ref())
            {
                if *width > 0.0 && *height > 0.0 {
                    el = el.aspect_ratio((*width as f32) / (*height as f32));
                }
            }
        }
        el = crate::layout::container::contain_declared(el, ctx.style);

        let mut el = crate::accessibility::apply_accessibility(
            el,
            ctx.id,
            ctx.props,
            Some(gpui::Role::Image),
        );
        el = crate::accessibility::apply_image_label(el, ctx.props, &self.alt);
        let el = super::wire_host_events(el, &ctx);
        crate::automation::track_own_bounds(el, ctx.id).into_any_element()
    }

    fn set_prop(&mut self, key: &str, value: serde_json::Value) {
        match key {
            "src" => {
                if value.is_null() && !self.has_src {
                    return;
                }
                self.has_src = !value.is_null();
                self.load_src(value.as_str().unwrap_or(""));
            }
            "objectFit" => {
                self.object_fit = value
                    .as_str()
                    .map(ImgObjectFit::from_str)
                    .unwrap_or_default()
            }
            "alt" => self.alt = value.as_str().unwrap_or_default().to_string(),
            // Sent by `js/blob-images.ts` when it cannot fetch a `blob:` src.
            "srcError" => {
                let message = value.get("message").and_then(|message| message.as_str());
                if let (ImgSource::Pending, Some(message)) = (&self.source, message) {
                    self.source = ImgSource::Failed(message.to_string());
                    self.epoch += 1;
                }
            }
            _ => {}
        }
    }

    fn load_hidden(
        &mut self,
        id: u64,
        events: &std::collections::HashSet<String>,
        callback: &Option<crate::renderer::EventCallback>,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        if !self.awaiting_outcome {
            return;
        }
        match self.resolve_image(window, cx) {
            Resolved::Done(Ok(image)) => {
                let natural = natural_size(&image);
                self.report(id, events, callback, cx, Ok(natural));
            }
            Resolved::Done(Err(message)) => self.report(id, events, callback, cx, Err(message)),
            Resolved::NoSrc | Resolved::Loading => {}
        }
    }

    fn supported_props(&self) -> &'static [&'static str] {
        &["src", "objectFit", "alt", "srcError"]
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["load", "error"]
    }

    fn destroy(&mut self) {}

    fn take_held_images(&mut self) -> Vec<std::sync::Arc<gpui::RenderImage>> {
        let mut held = std::mem::take(&mut self.dropped);
        held.extend(self.live_image());
        held
    }

    fn replace_live_image(
        &mut self,
        image: std::sync::Arc<gpui::RenderImage>,
    ) -> Option<std::sync::Arc<gpui::RenderImage>> {
        let previous = self.live_image();
        self.source = ImgSource::Render(image);
        self.painted = None;
        self.decoded = None;
        self.epoch += 1;
        previous
    }

    /// Once its source has loaded and `load` or `error` has been reported, what it paints is its
    /// source, which only a change of source replaces. Until then the asset arriving is something
    /// only `render` notices, and a GIF is stepped by the paint.
    fn retain_state(&self) -> Option<u64> {
        let settled = !self.awaiting_outcome
            && self.dropped.is_empty()
            && !self.animated
            && !matches!(self.source, ImgSource::Pending);
        settled.then_some(self.epoch)
    }
}

/// Byte order of a packed `setImagePixels` buffer. Alpha is straight in both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Rgba,
    /// GPUI's own `RenderImage` order, so the upload skips the swizzle pass.
    Bgra,
}

impl PixelFormat {
    pub fn parse(format: Option<&str>) -> std::result::Result<Self, String> {
        match format {
            None | Some("rgba") => Ok(Self::Rgba),
            Some("bgra") => Ok(Self::Bgra),
            Some(other) => Err(format!(
                "Unknown pixel format {other:?}, expected \"rgba\" or \"bgra\""
            )),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Rgba => "RGBA",
            Self::Bgra => "BGRA",
        }
    }
}

pub fn render_image_from_pixels(
    width: u32,
    height: u32,
    mut bytes: Vec<u8>,
    format: PixelFormat,
) -> std::result::Result<std::sync::Arc<gpui::RenderImage>, String> {
    let name = format.name();
    let expected = (width as u64)
        .checked_mul(height as u64)
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| usize::try_from(bytes).ok())
        .ok_or_else(|| format!("{name} pixel buffer {width}x{height} is too large"))?;
    if bytes.len() != expected {
        return Err(format!(
            "{name} pixel buffer length {} does not match {width}x{height} ({expected} bytes)",
            bytes.len()
        ));
    }
    if format == PixelFormat::Rgba {
        for pixel in bytes.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
    }
    gpui::RenderImage::from_bgra(width, height, bytes)
        .map(std::sync::Arc::new)
        .ok_or_else(|| format!("{name} pixel buffer is not a valid image"))
}

pub fn render_image_from_encoded(
    bytes: Vec<u8>,
    svg_renderer: gpui::SvgRenderer,
) -> std::result::Result<std::sync::Arc<gpui::RenderImage>, String> {
    let format = sniff_image_format(&bytes)
        .ok_or_else(|| "unrecognized image format".to_string())?;
    gpui::Image::from_bytes(format, bytes)
        .to_image_data(svg_renderer)
        .map_err(|error| error.to_string())
}

fn sniff_image_format(bytes: &[u8]) -> Option<gpui::ImageFormat> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(gpui::ImageFormat::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        Some(gpui::ImageFormat::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(gpui::ImageFormat::Gif)
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some(gpui::ImageFormat::Webp)
    } else if bytes.starts_with(b"BM") {
        Some(gpui::ImageFormat::Bmp)
    } else if bytes.starts_with(&[0x49, 0x49, 0x2A, 0x00])
        || bytes.starts_with(&[0x4D, 0x4D, 0x00, 0x2A])
    {
        Some(gpui::ImageFormat::Tiff)
    } else if bytes.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        Some(gpui::ImageFormat::Ico)
    } else if bytes.starts_with(b"P1")
        || bytes.starts_with(b"P2")
        || bytes.starts_with(b"P3")
        || bytes.starts_with(b"P4")
        || bytes.starts_with(b"P5")
        || bytes.starts_with(b"P6")
    {
        Some(gpui::ImageFormat::Pnm)
    } else if looks_like_svg(bytes) {
        Some(gpui::ImageFormat::Svg)
    } else {
        None
    }
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let start = std::str::from_utf8(bytes)
        .ok()
        .map(|text| text.trim_start())
        .unwrap_or("");
    start.starts_with("<svg") || start.starts_with("<?xml")
}

/// `<svg>`: the document is rasterised in full colour at device resolution
/// (`svg_paint`), and `currentColor` is the element's computed `color`.
#[derive(Default)]
pub struct SvgElement {
    src: String,
    bytes: Option<std::sync::Arc<[u8]>>,
    /// Inline markup from the `source` prop; wins over `src` when not blank.
    source: Option<std::sync::Arc<[u8]>>,
    /// Shared with the paint closure, which runs after `render` returns.
    paint: std::rc::Rc<std::cell::RefCell<super::svg_paint::SvgPaint>>,
    /// The document the last render had to paint, and its `currentColor`.
    drawing: Option<(std::sync::Arc<[u8]>, gpui::Rgba)>,
    /// An animation runs on the shapes of the document the last render painted.
    shapes_run: bool,
}

impl SvgElement {
    fn load_src(&mut self, src: String) {
        self.bytes = svg_bytes(&src).map(std::sync::Arc::from);
        self.src = src;
    }
}

fn svg_bytes(src: &str) -> Option<Vec<u8>> {
    if src.starts_with("data:") {
        let (format, bytes) = decode_image_data_url(src)?;
        return (format == gpui::ImageFormat::Svg).then_some(bytes);
    }
    #[cfg(target_family = "wasm")]
    return None;
    #[cfg(not(target_family = "wasm"))]
    std::fs::read(src).ok()
}

fn decode_image_data_url(src: &str) -> Option<(gpui::ImageFormat, Vec<u8>)> {
    let (metadata, data) = src.strip_prefix("data:")?.split_once(',')?;
    let mut parts = metadata.split(';');
    let mime_type = parts.next()?.to_ascii_lowercase();
    let format = gpui::ImageFormat::from_mime_type(&mime_type)?;
    let is_base64 = parts.any(|part| part.eq_ignore_ascii_case("base64"));
    let body = percent_decode(data);
    let bytes = if is_base64 {
        forgiving_base64_decode(&body)?
    } else {
        body
    };
    Some((format, bytes))
}

/// The forgiving-base64 decode of the Infra standard, which a data URL goes
/// through: ASCII white space is dropped, padding may be left off, and the bits
/// after the last whole byte may be anything.
fn forgiving_base64_decode(input: &[u8]) -> Option<Vec<u8>> {
    const UNPADDED: base64::engine::GeneralPurpose = base64::engine::GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        base64::engine::GeneralPurposeConfig::new()
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::RequireNone)
            .with_decode_allow_trailing_bits(true),
    );
    let mut data: Vec<u8> = input
        .iter()
        .copied()
        .filter(|byte| !matches!(byte, b' ' | b'\t' | b'\n' | 0x0C | b'\r'))
        .collect();
    if data.len() % 4 == 0 {
        for _ in 0..2 {
            if data.last() == Some(&b'=') {
                data.pop();
            }
        }
    }
    if data.len() % 4 == 1 {
        return None;
    }
    UNPADDED.decode(data).ok()
}

fn percent_decode(input: &str) -> Vec<u8> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(
                std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or(""),
                16,
            ) {
                out.push(value);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    out
}

impl CustomElement for SvgElement {
    fn render(
        &mut self,
        ctx: CustomRenderContext,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<crate::renderer::GpuixView>,
    ) -> gpui::AnyElement {
        use gpui::prelude::*;

        let element_id = crate::events::ids::host_element_id(ctx.id);
        let mut surface = super::custom_surface(gpui::div().id(element_id).flex_none(), &ctx);
        surface = crate::layout::container::contain_declared(surface, ctx.style);
        // An inline `<svg>` is `overflow: hidden` unless the page says otherwise, which is
        // what rounds the drawing to the box's border radii.
        let overflow_declared = ctx.style.is_some_and(|style| {
            style.overflow.is_some() || style.overflow_x.is_some() || style.overflow_y.is_some()
        });
        if !overflow_declared {
            surface = surface.overflow_hidden();
        }
        let bytes = match &ctx.svg_markup {
            Some(markup) => Some(std::sync::Arc::from(markup.as_bytes())),
            None => self.source.clone().or_else(|| self.bytes.clone()),
        };
        let color = ctx.colors.current();
        self.drawing = bytes.clone().map(|bytes| (bytes, color));
        self.shapes_run = ctx.shapes_run;
        if let Some(outset) = ctx.cull_outset {
            surface = surface.cullable(outset);
        }
        if let Some(bytes) = bytes {
            let paint = self.paint.clone();
            let renderer = cx.svg_renderer();
            let in_paint = ctx.shapes_run;
            surface = surface.child(
                gpui::canvas(
                    // Only prepaint knows that the subtree is culled.
                    |_, window, _| window.is_culling(),
                    move |bounds, culled, window, cx| {
                        if culled {
                            // Nobody sees the image, so none is made: a job for it would be wasted.
                            paint.borrow_mut().note_culled();
                            return;
                        }
                        paint_svg(&paint, &renderer, &bytes, bounds, color, in_paint, window, cx);
                    },
                )
                .keeps_no_position()
                // In flow, not absolute: an absolute child is sized against the padding box.
                .size_full(),
            );
        } else {
            // Nothing to paint now, so the image of what was there must not stay in the atlas.
            self.paint.borrow_mut().clear(|stale| {
                window.drop_image(stale).ok();
            });
        }
        surface.into_any_element()
    }

    fn set_prop(&mut self, key: &str, value: serde_json::Value) {
        match key {
            "src" => self.load_src(value.as_str().unwrap_or_default().to_string()),
            "source" => {
                self.source = value
                    .as_str()
                    .filter(|markup| !markup.trim().is_empty())
                    .map(|markup| std::sync::Arc::from(markup.as_bytes()));
            }
            _ => {}
        }
    }

    fn supported_props(&self) -> &'static [&'static str] {
        &["src", "source"]
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &[]
    }

    fn destroy(&mut self) {}

    fn take_held_images(&mut self) -> Vec<std::sync::Arc<gpui::RenderImage>> {
        self.paint.borrow().held().into_iter().collect()
    }

    /// What it paints is its document, drawn at its box in its colour. Both are in the key. A
    /// small document is rasterised in the paint that needs it, and the raster of a big one is a
    /// job that lands in some later paint, so until one has been shown and none is running, and
    /// while a change of document would start one, it is not the same as the last paint.
    fn draws_changes_at_once(&self) -> bool {
        self.drawing
            .as_ref()
            .is_none_or(|(bytes, _)| self.paint.borrow().rasterises_in_paint(bytes, self.shapes_run))
    }

    fn retain_state(&self) -> Option<u64> {
        let paint = self.paint.borrow();
        // A paint nobody saw made no image and has none to wait for: the subtree is built again
        // when it is seen, and it is not the same build as one that drew.
        if paint.was_culled() {
            return Some(1);
        }
        let settled = match &self.drawing {
            None => true,
            Some((bytes, color)) => paint.paints_in_place(bytes, *color, self.shapes_run),
        };
        settled.then_some(0)
    }
}

fn paint_svg(
    paint: &std::cell::RefCell<super::svg_paint::SvgPaint>,
    renderer: &gpui::SvgRenderer,
    bytes: &std::sync::Arc<[u8]>,
    bounds: gpui::Bounds<gpui::Pixels>,
    color: gpui::Rgba,
    in_paint: bool,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) {
    let scale = window.scale_factor();
    if bounds.size.width <= gpui::px(0.0) || bounds.size.height <= gpui::px(0.0) {
        return;
    }
    let Some(image) = paint
        .borrow_mut()
        .image(window, cx, renderer, bytes, bounds.size, scale, color, in_paint)
    else {
        return;
    };
    let size = image.size(0);
    let drawn = drawn_size((size.width.0, size.height.0), bounds.size, scale);
    let image_bounds = gpui::Bounds {
        origin: bounds.origin
            + gpui::point(
                (bounds.size.width - drawn.width) / 2.0,
                (bounds.size.height - drawn.height) / 2.0,
            ),
        size: drawn,
    };
    window
        .paint_image(bounds, image_bounds, gpui::Corners::default(), image, 0, false)
        .ok();
}

/// The size an SVG bitmap of `image` device pixels is drawn at inside `bounds`.
///
/// A bitmap made for this box is drawn at its own size. One that is not (a stale
/// raster kept through a resize, or one gpui cut down to its 8192 px limit) is
/// scaled to fit the box, like `object-fit: contain`.
fn drawn_size(
    image: (i32, i32),
    bounds: gpui::Size<gpui::Pixels>,
    scale_factor: f32,
) -> gpui::Size<gpui::Pixels> {
    let (width, height) = (image.0 as f32, image.1 as f32);
    let box_width = f32::from(bounds.width) * scale_factor;
    let box_height = f32::from(bounds.height) * scale_factor;
    let (across, along) = (box_width / width, box_height / height);
    let fit = across.min(along);
    let limiting = if across <= along { width } else { height };
    // The raster was sized from the box rounded to whole pixels, and the
    // rasteriser cuts its size down to a whole pixel.
    let made_for_box = (limiting * fit - limiting).abs() <= 1.5;
    let fit = if made_for_box { 1.0 } else { fit };
    gpui::size(
        gpui::px(width * fit / scale_factor),
        gpui::px(height * fit / scale_factor),
    )
}

#[cfg(test)]
mod drawn_size_tests {
    use super::drawn_size;
    use gpui::{px, size};

    fn drawn(image: (i32, i32), bounds: (f32, f32), scale: f32) -> (f32, f32) {
        let drawn = drawn_size(image, size(px(bounds.0), px(bounds.1)), scale);
        let round = |value: gpui::Pixels| (f32::from(value) * 100.0).round() / 100.0;
        (round(drawn.width), round(drawn.height))
    }

    #[test]
    fn a_bitmap_made_for_the_box_is_drawn_at_its_own_size() {
        assert_eq!(drawn((48, 48), (24.0, 24.0), 2.0), (24.0, 24.0));
        // 10.4 logical px is 21 device px once rounded, and a 20.6 px bitmap is cut to 20.
        assert_eq!(drawn((20, 10), (10.4, 10.4), 2.0), (10.0, 5.0));
        assert_eq!(drawn((20, 10), (20.0, 20.0), 1.0), (20.0, 10.0));
    }

    #[test]
    fn a_stale_bitmap_is_scaled_to_the_box_keeping_its_ratio() {
        assert_eq!(drawn((100, 50), (150.0, 150.0), 1.0), (150.0, 75.0));
        assert_eq!(drawn((100, 50), (60.0, 60.0), 1.0), (60.0, 30.0));
        assert_eq!(drawn((50, 100), (150.0, 150.0), 2.0), (75.0, 150.0));
    }

    #[test]
    fn a_bitmap_cut_down_to_the_texture_limit_fills_the_box() {
        assert_eq!(drawn((8192, 4096), (5000.0, 5000.0), 2.0), (5000.0, 2500.0));
    }
}
