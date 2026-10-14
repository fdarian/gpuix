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
enum ImgSource {
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
    dropped: Option<std::sync::Arc<gpui::RenderImage>>,
    /// The decoded `Data` source, decoded once on first paint.
    decoded: Option<Result<std::sync::Arc<gpui::RenderImage>, String>>,
    /// A src was set and its `load` or `error` has not fired yet.
    awaiting_outcome: bool,
    /// gpui rasterises an SVG at twice its size, so the bitmap is not the
    /// intrinsic size.
    supersampled: bool,
}

impl ImgElement {
    fn load_src(&mut self, src: &str) {
        let src = src.trim();
        self.decoded = None;
        self.awaiting_outcome = !src.is_empty();
        (self.source, self.supersampled) = resolve_src(src);
    }

    /// Fires `load` or `error` once per src, after the outcome is known. Deferred
    /// so a handler that sets state never re-enters the build in progress.
    fn report(
        &mut self,
        ctx: &CustomRenderContext,
        cx: &mut gpui::App,
        outcome: Result<(u32, u32), String>,
    ) {
        if !std::mem::take(&mut self.awaiting_outcome) {
            return;
        }
        let event = if outcome.is_ok() { "load" } else { "error" };
        if !ctx.events.contains(event) {
            return;
        }
        let callback = ctx.event_callback.clone();
        let id = ctx.id;
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
}

/// The source `src` names, and whether gpui will rasterise it as an SVG.
fn resolve_src(src: &str) -> (ImgSource, bool) {
    if src.is_empty() {
        (ImgSource::Empty, false)
    } else if src.starts_with("blob:") {
        (ImgSource::Pending, false)
    } else if src.starts_with("data:") {
        // TODO: Replace JSON data URLs with binary mutations to keep base64 decoding off paint.
        match decode_image_data_url(src) {
            Some((format, bytes)) => (
                ImgSource::Data(std::sync::Arc::new(gpui::Image::from_bytes(format, bytes))),
                format == gpui::ImageFormat::Svg,
            ),
            None => (ImgSource::Failed("invalid data URL".to_string()), false),
        }
    } else if let Some(uri) = http_image_uri(src) {
        let svg = uri_path_is_svg(uri.as_ref());
        (ImgSource::Uri(uri), svg)
    } else if src
        .get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file://"))
    {
        match file_url_path(src) {
            Some(path) => {
                let svg = file_is_svg(&path);
                (ImgSource::Path(path), svg)
            }
            None => (ImgSource::Failed(format!("invalid file URL {src}")), false),
        }
    } else {
        let path = std::path::PathBuf::from(src);
        let svg = file_is_svg(&path);
        (ImgSource::Path(path), svg)
    }
}

/// gpui hands any file it does not recognise as a raster format to the SVG
/// renderer. A file that cannot be read fails to load, and `error` reports why.
fn file_is_svg(path: &std::path::Path) -> bool {
    use std::io::Read as _;
    let mut head = [0u8; 512];
    let Ok(read) = std::fs::File::open(path).and_then(|mut file| file.read(&mut head)) else {
        return false;
    };
    rasterised_as_svg(&head[..read])
}

/// gpui fetches an `http(s)` image itself, so its bytes are not here to sniff:
/// the URL's extension stands in.
fn uri_path_is_svg(uri: &str) -> bool {
    let path = uri.split(['?', '#']).next().unwrap_or(uri).as_bytes();
    path.len() >= 4 && path[path.len() - 4..].eq_ignore_ascii_case(b".svg")
}

/// What `gpui::Image::to_image_data` and the image asset loader do with `bytes`.
fn rasterised_as_svg(bytes: &[u8]) -> bool {
    matches!(sniff_image_format(bytes), None | Some(gpui::ImageFormat::Svg))
}

/// The intrinsic size in CSS px of a bitmap that is `scale` times its size.
fn css_size(device: (i32, i32), scale: f32) -> (u32, u32) {
    let css = |pixels: i32| (pixels.max(0) as f32 / scale).round() as u32;
    (css(device.0), css(device.1))
}

/// `file:///a/b%20c.png` as a path. Accepts an empty or `localhost` host.
fn file_url_path(src: &str) -> Option<std::path::PathBuf> {
    let rest = src.get(7..)?;
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
        css_size, file_url_path, http_image_uri, uri_path_is_svg, CustomElement, ImgElement,
        ImgSource,
    };
    use serde_json::json;

    #[test]
    fn svg_sources_are_flagged_as_supersampled() {
        let mut element = ImgElement::default();
        for (src, svg) in [
            ("data:image/svg+xml,%3Csvg%2F%3E", true),
            ("data:image/svg+xml;base64,PHN2Zy8+", true),
            ("data:image/png;base64,iVBORw0K", false),
            ("https://example.test/a.SVG?v=2#x", true),
            ("https://example.test/a.png", false),
            ("/does/not/exist.svg", false),
        ] {
            element.set_prop("src", json!(src));
            assert_eq!(element.supersampled, svg, "{src}");
        }
    }

    #[test]
    fn an_svg_file_is_sniffed_from_its_content() {
        let dir = std::env::temp_dir().join(format!("gpuix-img-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let svg = dir.join("icon.dat");
        std::fs::write(&svg, "<svg xmlns=\"http://www.w3.org/2000/svg\"/>").unwrap();
        let png = dir.join("pic.svg");
        std::fs::write(&png, [0x89, b'P', b'N', b'G', 0, 0, 0, 0]).unwrap();
        let mut element = ImgElement::default();
        element.set_prop("src", json!(svg.to_str().unwrap()));
        assert!(element.supersampled);
        element.set_prop("src", json!(png.to_str().unwrap()));
        assert!(!element.supersampled);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_supersampled_bitmap_reports_its_css_size() {
        assert_eq!(css_size((20, 12), 2.0), (10, 6));
        assert_eq!(css_size((20, 12), 1.0), (20, 12));
        assert_eq!(css_size((-1, 0), 1.0), (0, 0));
    }

    #[test]
    fn uri_extension_ignores_query_and_fragment() {
        assert!(uri_path_is_svg("https://e.test/a.svg"));
        assert!(uri_path_is_svg("https://e.test/a.svg?x=.png"));
        assert!(!uri_path_is_svg("https://e.test/a.svg/b"));
        assert!(!uri_path_is_svg("https://e.test/é"));
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

        if let Some(image) = self.dropped.take() {
            window.drop_image(image).ok();
        }

        // Resolve every source to a decoded image here, so the outcome and the
        // natural size are known for `load` and `error`.
        let loaded = match &self.source {
            ImgSource::Empty => return img_fallback(&ctx, &self.alt, "img: no src"),
            ImgSource::Pending => return img_blank(&ctx),
            ImgSource::Failed(message) => {
                let message = message.clone();
                self.report(&ctx, cx, Err(message));
                return img_fallback(&ctx, &self.alt, "img: load failed");
            }
            ImgSource::Render(image) => Ok(image.clone()),
            ImgSource::Path(path) => {
                let resource = gpui::Resource::Path(path.as_path().into());
                match window.use_asset::<gpui::ImageAssetLoader>(&resource, cx) {
                    Some(result) => result.map_err(|error| error.to_string()),
                    None => return img_blank(&ctx),
                }
            }
            ImgSource::Uri(uri) => {
                let resource = gpui::Resource::Uri(uri.clone());
                match window.use_asset::<gpui::ImageAssetLoader>(&resource, cx) {
                    Some(result) => result.map_err(|error| error.to_string()),
                    None => return img_blank(&ctx),
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
        let image = match loaded {
            Ok(image) => image,
            Err(message) => {
                self.report(&ctx, cx, Err(message));
                return img_fallback(&ctx, &self.alt, "img: load failed");
            }
        };
        let size = image.size(0);
        let scale = if self.supersampled { gpui::SMOOTH_SVG_SCALE_FACTOR } else { 1.0 };
        self.report(&ctx, cx, Ok(css_size((size.width.0, size.height.0), scale)));
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

        if let Some(style) = ctx.style {
            el = crate::renderer::apply_interactive_styles(el, style, &ctx.colors);
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

        let mut el = crate::accessibility::apply_accessibility(
            el,
            ctx.id,
            ctx.props,
            Some(gpui::Role::Image),
        );
        el = crate::accessibility::apply_image_label(el, ctx.props, &self.alt);
        let el = super::wire_host_events(el, &ctx);
        crate::automation::track_own_bounds(el, ctx.id, ctx.observe.clone()).into_any_element()
    }

    fn set_prop(&mut self, key: &str, value: serde_json::Value) {
        match key {
            "src" => {
                if value.is_null() && matches!(self.source, ImgSource::Render(_)) {
                    return;
                }
                if let ImgSource::Render(image) = &self.source {
                    self.dropped = Some(image.clone());
                }
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
                }
            }
            _ => {}
        }
    }

    fn supported_props(&self) -> &'static [&'static str] {
        &["src", "objectFit", "alt", "srcError"]
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["load", "error"]
    }

    fn destroy(&mut self) {}

    fn live_image(&self) -> Option<std::sync::Arc<gpui::RenderImage>> {
        match &self.source {
            ImgSource::Render(image) => Some(image.clone()),
            _ => None,
        }
    }

    fn take_dropped_image(&mut self) -> Option<std::sync::Arc<gpui::RenderImage>> {
        self.dropped.take()
    }

    fn replace_live_image(
        &mut self,
        image: std::sync::Arc<gpui::RenderImage>,
        supersampled: bool,
    ) -> Option<std::sync::Arc<gpui::RenderImage>> {
        let previous = self.live_image();
        self.source = ImgSource::Render(image);
        self.decoded = None;
        self.supersampled = supersampled;
        previous
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

/// Whether gpui rasterises `bytes` as an SVG, at twice its size.
pub fn encoded_is_svg(bytes: &[u8]) -> bool {
    rasterised_as_svg(bytes)
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
    let bytes = if is_base64 {
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .ok()?
    } else {
        percent_decode(data)
    };
    Some((format, bytes))
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
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<crate::renderer::GpuixView>,
    ) -> gpui::AnyElement {
        use gpui::prelude::*;

        let element_id = crate::events::ids::host_element_id(ctx.id);
        let mut surface = super::custom_surface(gpui::div().id(element_id).flex_none(), &ctx);
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
        if let Some(bytes) = bytes {
            let paint = self.paint.clone();
            let renderer = cx.svg_renderer();
            let color = ctx.colors.current;
            surface = surface.child(
                gpui::canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        paint_svg(&paint, &renderer, &bytes, bounds, color, window);
                    },
                )
                // In flow, not absolute: an absolute child is sized against the padding box.
                .size_full(),
            );
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

    /// The held raster, so the registry drops it from the atlas with the element.
    fn live_image(&self) -> Option<std::sync::Arc<gpui::RenderImage>> {
        self.paint.borrow().held()
    }
}

fn paint_svg(
    paint: &std::cell::RefCell<super::svg_paint::SvgPaint>,
    renderer: &gpui::SvgRenderer,
    bytes: &std::sync::Arc<[u8]>,
    bounds: gpui::Bounds<gpui::Pixels>,
    color: gpui::Rgba,
    window: &mut gpui::Window,
) {
    let scale = window.scale_factor();
    if bounds.size.width <= gpui::px(0.0) || bounds.size.height <= gpui::px(0.0) {
        return;
    }
    let Some(image) = paint
        .borrow_mut()
        .image(window, renderer, bytes, bounds.size, scale, color)
    else {
        return;
    };
    let size = image.size(0);
    let drawn = gpui::size(
        gpui::px(size.width.0 as f32 / scale),
        gpui::px(size.height.0 as f32 / scale),
    );
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
