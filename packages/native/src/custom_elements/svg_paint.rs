//! Full-colour SVG painting for `<svg>`.
//!
//! gpui's own `svg()` element is a monochrome mask: it keeps only the alpha of
//! the rasterised SVG and tints it with one colour. This module rasterises with
//! resvg through gpui's `SvgRenderer` and keeps every fill and stroke, at device
//! resolution. `currentColor` resolves to the colour the caller passes, which is
//! the element's computed `color`.
//!
//! [`SvgPaint::image`] is the one entry point. It holds the single image it last
//! made and, when any input changes, drops that image from the sprite atlas
//! before making the next one, so an SVG that changes every frame (an animation
//! feeding serialised children) keeps the atlas at one image.
//!
//! A box that changes size every frame (a transition, a window drag) would
//! rasterise every frame. The first change after a pause rasterises at once, so
//! a one-off resize is sharp. When the next change comes within [`SETTLE`], the
//! last raster is drawn stretched until the size has been still for [`SETTLE`],
//! and then it is rasterised again. A raster more than twice off the target
//! size is replaced at once, so a stretch never gets that blurry.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{DevicePixels, Pixels, RenderImage, Rgba, Size, SvgRenderer, SvgSize};

/// How long the box must hold one size before a stretched raster is replaced.
const SETTLE: Duration = Duration::from_millis(100);

/// Widest request for the first, cheap render that measures the SVG's aspect ratio.
const PROBE_WIDTH: i32 = 256;

#[derive(Default)]
pub struct SvgPaint {
    cached: Option<Cached>,
    resize: Option<Resize>,
}

/// The box size last asked for, and when it moved there.
struct Resize {
    target: (i32, i32),
    changed_at: Instant,
    /// The move came within [`SETTLE`] of the one before it.
    in_burst: bool,
}

struct Cached {
    key: Key,
    /// `None` when rasterising these inputs failed. The failure is cached under
    /// the same key so bad markup is parsed and reported once, not per repaint.
    image: Option<Arc<RenderImage>>,
}

struct Key {
    bytes: Arc<[u8]>,
    width: i32,
    height: i32,
    color: [u32; 4],
}

impl Key {
    fn same_as(&self, other: &Key) -> bool {
        self.width == other.width && self.height == other.height && self.same_content(other)
    }

    /// Same document and colour, whatever the size.
    fn same_content(&self, other: &Key) -> bool {
        self.color == other.color
            && (Arc::ptr_eq(&self.bytes, &other.bytes) || self.bytes == other.bytes)
    }

    /// Whether a raster of this key's size is close enough to stretch to `other`'s.
    fn within_twice(&self, other: &Key) -> bool {
        let ratio = |a: i32, b: i32| a as f32 / b as f32;
        [ratio(self.width, other.width), ratio(self.height, other.height)]
            .iter()
            .all(|ratio| (0.5..=2.0).contains(ratio))
    }
}

impl SvgPaint {
    /// The SVG in `bytes` rasterised to fit `size` (logical pixels) at
    /// `scale_factor`, with `currentColor` resolved to `current_color`.
    ///
    /// The SVG keeps its aspect ratio like `preserveAspectRatio="xMidYMid meet"`,
    /// so the image can be smaller than `size` on one axis: centre it, using the
    /// image's size divided by `scale_factor`. It is in the BGRA layout
    /// `Window::paint_image` takes as it is.
    ///
    /// The previous image is dropped from the atlas when the inputs changed. While
    /// the size is changing every frame it is kept instead, and the caller draws
    /// it stretched (see the module docs); a frame is requested to replace it.
    ///
    /// `None` means the SVG could not be rasterised; that is logged once and
    /// retried only when the bytes, size, scale or colour change.
    pub fn image(
        &mut self,
        window: &mut gpui::Window,
        renderer: &SvgRenderer,
        bytes: &Arc<[u8]>,
        size: Size<Pixels>,
        scale_factor: f32,
        current_color: Rgba,
    ) -> Option<Arc<RenderImage>> {
        let device = |value: Pixels| (f32::from(value) * scale_factor).round().max(1.0) as i32;
        let key = Key {
            bytes: bytes.clone(),
            width: device(size.width),
            height: device(size.height),
            color: [
                current_color.r.to_bits(),
                current_color.g.to_bits(),
                current_color.b.to_bits(),
                current_color.a.to_bits(),
            ],
        };
        let mut pending = false;
        let image = self.resolve(
            key,
            Instant::now(),
            &mut pending,
            |key| rasterize(renderer, bytes, key.width, key.height, current_color),
            |stale| {
                window.drop_image(stale).ok();
            },
        );
        if pending {
            window.request_animation_frame();
        }
        image
    }

    /// `pending` is set when the image returned is a stale raster that a later
    /// frame must replace.
    fn resolve(
        &mut self,
        key: Key,
        now: Instant,
        pending: &mut bool,
        rasterize: impl FnOnce(&Key) -> Result<Arc<RenderImage>, String>,
        drop_image: impl FnOnce(Arc<RenderImage>),
    ) -> Option<Arc<RenderImage>> {
        if let Some(cached) = &self.cached {
            if cached.key.same_as(&key) {
                return cached.image.clone();
            }
            if cached.key.same_content(&key) && cached.image.is_some() {
                let target = (key.width, key.height);
                match &mut self.resize {
                    Some(resize) if resize.target == target => {}
                    resize => {
                        let in_burst = resize
                            .as_ref()
                            .is_some_and(|last| now.duration_since(last.changed_at) < SETTLE);
                        *resize = Some(Resize {
                            target,
                            changed_at: now,
                            in_burst,
                        });
                    }
                }
                let resize = self.resize.as_ref().expect("set above");
                if resize.in_burst
                    && now.duration_since(resize.changed_at) < SETTLE
                    && cached.key.within_twice(&key)
                {
                    *pending = true;
                    return cached.image.clone();
                }
            }
        }
        // Drop first: if rasterising fails the stale image must not linger.
        if let Some(stale) = self.cached.take().and_then(|cached| cached.image) {
            drop_image(stale);
        }
        let image = match rasterize(&key) {
            Ok(image) => Some(image),
            Err(error) => {
                log::warn!("<svg> could not be rasterised: {error}");
                None
            }
        };
        self.cached = Some(Cached {
            key,
            image: image.clone(),
        });
        image
    }

    /// Drops the image held, for an `<svg>` that has nothing left to paint.
    pub fn clear(&mut self, drop_image: impl FnOnce(Arc<RenderImage>)) {
        self.resize = None;
        if let Some(stale) = self.cached.take().and_then(|cached| cached.image) {
            drop_image(stale);
        }
    }

    /// The image currently held, so the owner can drop it from the atlas on teardown.
    pub fn held(&self) -> Option<Arc<RenderImage>> {
        self.cached.as_ref().and_then(|cached| cached.image.clone())
    }
}

fn rasterize(
    renderer: &SvgRenderer,
    bytes: &[u8],
    width: i32,
    height: i32,
    current_color: Rgba,
) -> Result<Arc<RenderImage>, String> {
    let bytes = with_current_color(bytes, current_color);
    let svg = renderer.parse_svg(&bytes).map_err(|error| error.to_string())?;
    fit(width, height, |width| {
        renderer
            .render_parsed(
                &svg,
                SvgSize::Size(gpui::size(DevicePixels(width), DevicePixels(1))),
            )
            .map_err(|error| error.to_string())
    })
}

/// The image `render` makes, at the widest width that fits `width` by `height`.
/// `render` scales by width and keeps the aspect ratio.
fn fit(
    width: i32,
    height: i32,
    render: impl Fn(i32) -> Result<Arc<RenderImage>, String>,
) -> Result<Arc<RenderImage>, String> {
    let refit = |image: Arc<RenderImage>| {
        let rendered = image.size(0);
        if rendered.height.0 <= height {
            return Ok(image);
        }
        let fitted = (width as f64 * height as f64 / rendered.height.0 as f64).floor() as i32;
        render(fitted.max(1))
    };
    if width <= PROBE_WIDTH {
        return refit(render(width)?);
    }
    // A full-width render of an SVG that is too tall would be thrown away, and
    // gpui has no way to ask a parsed SVG for its size. A cheap render says how
    // tall it is. Its height is cut down to whole pixels, so the ratio it shows
    // is a lower bound, and one more pixel of height is an upper bound.
    let probe = render(PROBE_WIDTH)?.size(0);
    let (probe_width, probe_height) = (probe.width.0.max(1) as f64, probe.height.0 as f64);
    if width as f64 * probe_height / probe_width <= height as f64 {
        return refit(render(width)?);
    }
    // Too tall for sure. The upper bound keeps the image inside the box, at the
    // price of being a pixel or so narrower than the exact fit.
    let fitted = (height as f64 * probe_width / (probe_height + 1.0)).floor() as i32;
    render(fitted.max(1))
}

/// Gives the root `<svg>` a `color` so `currentColor` resolves to it, unless the
/// document sets its own. usvg resolves `currentColor` against the `color`
/// property, and gpui's renderer offers no way to pass one in.
fn with_current_color(bytes: &[u8], color: Rgba) -> std::borrow::Cow<'_, [u8]> {
    let Some((tag_start, tag_end)) = root_tag(bytes) else {
        return bytes.into();
    };
    let tag = &bytes[tag_start..tag_end];
    if has_attribute(tag, b"color") {
        return bytes.into();
    }
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let attribute = format!(
        " color=\"rgba({},{},{},{})\"",
        channel(color.r),
        channel(color.g),
        channel(color.b),
        color.a.clamp(0.0, 1.0)
    );
    let insert_at = tag_start + b"<svg".len();
    let mut out = Vec::with_capacity(bytes.len() + attribute.len());
    out.extend_from_slice(&bytes[..insert_at]);
    out.extend_from_slice(attribute.as_bytes());
    out.extend_from_slice(&bytes[insert_at..]);
    out.into()
}

/// The byte range of the root element's start tag, `<svg ...>`.
fn root_tag(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut start = 0;
    // Skip the prolog, comments and doctype: the first `<svg` that is a tag.
    let tag_start = loop {
        let found = bytes[start..].windows(4).position(|window| window == b"<svg")? + start;
        let follows = bytes.get(found + 4).copied();
        if matches!(follows, Some(b' ' | b'\t' | b'\r' | b'\n' | b'>' | b'/')) {
            break found;
        }
        start = found + 4;
    };
    let mut quote = None;
    for (offset, &byte) in bytes[tag_start..].iter().enumerate() {
        match (quote, byte) {
            (Some(open), byte) if byte == open => quote = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'') => quote = Some(byte),
            (None, b'>') => return Some((tag_start, tag_start + offset + 1)),
            _ => {}
        }
    }
    None
}

/// Whether the start tag has an attribute with this exact name.
fn has_attribute(tag: &[u8], name: &[u8]) -> bool {
    let mut index = 0;
    let mut quote = None;
    while index < tag.len() {
        let byte = tag[index];
        match (quote, byte) {
            (Some(open), byte) if byte == open => quote = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'') => quote = Some(byte),
            (None, _) => {
                let preceded_by_space = index > 0 && tag[index - 1].is_ascii_whitespace();
                if preceded_by_space && tag[index..].starts_with(name) {
                    let after = tag[index + name.len()..]
                        .iter()
                        .find(|byte| !byte.is_ascii_whitespace());
                    if after == Some(&b'=') {
                        return true;
                    }
                }
            }
        }
        index += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer() -> SvgRenderer {
        SvgRenderer::new(Arc::new(()))
    }

    fn pixel(image: &RenderImage, x: usize, y: usize) -> [u8; 4] {
        let width = image.size(0).width.0 as usize;
        let bytes = image.as_bytes(0).unwrap();
        let at = (y * width + x) * 4;
        // BGRA.
        [bytes[at + 2], bytes[at + 1], bytes[at], bytes[at + 3]]
    }

    const RED: Rgba = Rgba { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };

    #[test]
    fn current_color_is_the_passed_colour_and_other_fills_are_kept() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2" viewBox="0 0 4 2">
            <rect width="2" height="2" fill="currentColor"/><rect x="2" width="2" height="2" fill="#00ff00"/></svg>"##;
        let image = rasterize(&renderer(), svg, 8, 4, RED).unwrap();
        assert_eq!(pixel(&image, 1, 1), [255, 0, 0, 255]);
        assert_eq!(pixel(&image, 6, 1), [0, 255, 0, 255]);
    }

    #[test]
    fn an_element_without_fill_is_black() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><rect width="2" height="2"/></svg>"##;
        let image = rasterize(&renderer(), svg, 2, 2, RED).unwrap();
        assert_eq!(pixel(&image, 0, 0), [0, 0, 0, 255]);
    }

    #[test]
    fn the_documents_own_color_wins_over_the_inherited_one() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2" color="#0000ff"><rect width="2" height="2" fill="currentColor"/></svg>"##;
        let image = rasterize(&renderer(), svg, 2, 2, RED).unwrap();
        assert_eq!(pixel(&image, 0, 0), [0, 0, 255, 255]);
    }

    #[test]
    fn alpha_in_the_colour_reaches_the_pixels() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><rect width="2" height="2" fill="currentColor"/></svg>"##;
        let half = Rgba { r: 1.0, g: 0.0, b: 0.0, a: 0.5 };
        let image = rasterize(&renderer(), svg, 2, 2, half).unwrap();
        let [_, _, _, a] = pixel(&image, 0, 0);
        assert!((a as i32 - 128).abs() <= 2, "{a}");
    }

    #[test]
    fn keeps_the_aspect_ratio_inside_the_box() {
        let wide = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 10"><rect width="20" height="10"/></svg>"##;
        // 20x10 into 10x10: scaled by width, 10x5.
        let image = rasterize(&renderer(), wide, 10, 10, RED).unwrap();
        assert_eq!((image.size(0).width.0, image.size(0).height.0), (10, 5));
        // Into 40x10 it is height-limited: 20x10.
        let image = rasterize(&renderer(), wide, 40, 10, RED).unwrap();
        assert_eq!((image.size(0).width.0, image.size(0).height.0), (20, 10));
    }

    fn widths_rendered(width: i32, height: i32, ratio: f64) -> (Vec<i32>, (i32, i32)) {
        let widths = std::cell::RefCell::new(Vec::new());
        let image = fit(width, height, |width| {
            widths.borrow_mut().push(width);
            let height = (width as f64 * ratio) as i32;
            let pixels = vec![0u8; width as usize * height as usize * 4];
            gpui::RenderImage::from_bgra(width as u32, height as u32, pixels)
                .map(Arc::new)
                .ok_or_else(|| "bad image".to_string())
        })
        .unwrap();
        let size = image.size(0);
        (widths.into_inner(), (size.width.0, size.height.0))
    }

    #[test]
    fn a_tall_svg_is_not_rendered_at_full_width_first() {
        // 1:10 into 1200x600: the width that fits is 60.
        let (widths, size) = widths_rendered(1200, 600, 10.0);
        assert_eq!(widths.len(), 2, "{widths:?}");
        assert_eq!(widths[0], PROBE_WIDTH);
        assert!(widths.iter().all(|width| *width <= PROBE_WIDTH));
        assert!(size.1 <= 600 && size.1 >= 590, "{size:?}");
    }

    #[test]
    fn an_svg_that_fits_is_rendered_once_at_full_width_after_the_probe() {
        let (widths, size) = widths_rendered(1200, 600, 0.5);
        assert_eq!(widths, vec![PROBE_WIDTH, 1200]);
        assert_eq!(size, (1200, 600));
    }

    #[test]
    fn a_small_box_renders_at_its_width_first_as_before() {
        let (widths, size) = widths_rendered(100, 100, 0.5);
        assert_eq!(widths, vec![100]);
        assert_eq!(size, (100, 50));
        let (widths, size) = widths_rendered(100, 100, 2.0);
        assert_eq!(widths, vec![100, 50]);
        assert_eq!(size, (50, 100));
    }

    #[test]
    fn an_svg_a_pixel_taller_than_the_probe_shows_is_refitted() {
        // The probe's height is cut down, so it cannot tell this one is too tall.
        let (widths, size) = widths_rendered(600, 601, 1.0035);
        assert_eq!(widths, vec![PROBE_WIDTH, 600, 599]);
        assert!(size.1 <= 601, "{size:?}");
    }

    fn resized(paint: &mut SvgPaint, bytes: &Arc<[u8]>, width: i32, at: Instant) -> (bool, i32) {
        let mut pending = false;
        let image = paint
            .resolve(
                key(bytes, width, RED),
                at,
                &mut pending,
                |key| {
                    let height = key.height as u32;
                    gpui::RenderImage::from_bgra(key.width as u32, height, vec![0; key.width as usize * height as usize * 4])
                        .map(Arc::new)
                        .ok_or_else(|| "bad".to_string())
                },
                |_| {},
            )
            .unwrap();
        (pending, image.size(0).width.0)
    }

    #[test]
    fn a_resize_after_a_pause_is_sharp_at_once() {
        let bytes: Arc<[u8]> = Arc::from(&b"x"[..]);
        let mut paint = SvgPaint::default();
        let start = Instant::now();
        assert_eq!(resized(&mut paint, &bytes, 100, start), (false, 100));
        let later = start + SETTLE * 5;
        assert_eq!(resized(&mut paint, &bytes, 120, later), (false, 120));
    }

    #[test]
    fn a_size_that_changes_every_frame_reuses_the_raster_until_it_settles() {
        let bytes: Arc<[u8]> = Arc::from(&b"x"[..]);
        let mut paint = SvgPaint::default();
        let start = Instant::now();
        let frame = Duration::from_millis(16);
        assert_eq!(resized(&mut paint, &bytes, 100, start), (false, 100));
        // The first change rasterises, the ones right after it do not.
        assert_eq!(resized(&mut paint, &bytes, 104, start + frame), (false, 104));
        for step in 2..6 {
            let at = start + frame * step;
            let (pending, width) = resized(&mut paint, &bytes, 100 + 4 * step as i32, at);
            assert!(pending);
            assert_eq!(width, 104);
        }
        // Still at 120 a frame after the last change: waiting.
        let last = start + frame * 5;
        assert_eq!(resized(&mut paint, &bytes, 120, last + frame), (true, 104));
        // Settled: the next frame rasterises at the final size.
        assert_eq!(resized(&mut paint, &bytes, 120, last + SETTLE), (false, 120));
        assert_eq!(resized(&mut paint, &bytes, 120, last + SETTLE * 2), (false, 120));
    }

    #[test]
    fn a_raster_more_than_twice_off_is_replaced_during_a_resize() {
        let bytes: Arc<[u8]> = Arc::from(&b"x"[..]);
        let mut paint = SvgPaint::default();
        let start = Instant::now();
        let frame = Duration::from_millis(16);
        resized(&mut paint, &bytes, 100, start);
        resized(&mut paint, &bytes, 110, start + frame);
        assert_eq!(resized(&mut paint, &bytes, 150, start + frame * 2), (true, 110));
        assert_eq!(resized(&mut paint, &bytes, 300, start + frame * 3), (false, 300));
    }

    #[test]
    fn clear_drops_the_held_image_once() {
        let bytes: Arc<[u8]> = Arc::from(&b"x"[..]);
        let mut paint = SvgPaint::default();
        resized(&mut paint, &bytes, 100, Instant::now());
        let dropped = std::cell::Cell::new(0);
        paint.clear(|_| dropped.set(dropped.get() + 1));
        paint.clear(|_| dropped.set(dropped.get() + 1));
        assert_eq!(dropped.get(), 1);
        assert!(paint.held().is_none());
    }

    #[test]
    fn rejects_input_that_is_not_an_svg() {
        assert!(rasterize(&renderer(), b"not svg", 4, 4, RED).is_err());
    }

    fn key(bytes: &Arc<[u8]>, width: i32, color: Rgba) -> Key {
        Key {
            bytes: bytes.clone(),
            width,
            height: 4,
            color: [color.r.to_bits(), color.g.to_bits(), color.b.to_bits(), color.a.to_bits()],
        }
    }

    #[test]
    fn a_failed_rasterisation_is_cached_until_an_input_changes() {
        let bad: Arc<[u8]> = Arc::from(&b"not svg"[..]);
        let mut paint = SvgPaint::default();
        let attempts = std::cell::Cell::new(0);
        let mut attempt = |paint: &mut SvgPaint, key: Key| {
            let found = paint.resolve(
                key,
                Instant::now(),
                &mut false,
                |key| {
                    attempts.set(attempts.get() + 1);
                    rasterize(&renderer(), &bad, key.width, key.height, RED)
                },
                |_| {},
            );
            assert!(found.is_none());
        };
        attempt(&mut paint, key(&bad, 4, RED));
        attempt(&mut paint, key(&bad, 4, RED));
        // The same bytes in a new allocation are the same input.
        let copy: Arc<[u8]> = Arc::from(&b"not svg"[..]);
        attempt(&mut paint, key(&copy, 4, RED));
        assert_eq!(attempts.get(), 1);
        assert!(paint.held().is_none());

        attempt(&mut paint, key(&bad, 8, RED));
        assert_eq!(attempts.get(), 2, "a new size retries");
        attempt(&mut paint, key(&bad, 8, Rgba { r: 0.0, ..RED }));
        assert_eq!(attempts.get(), 3, "a new colour retries");
    }

    #[test]
    fn a_failure_after_success_drops_the_stale_image_once() {
        let good: Arc<[u8]> =
            Arc::from(&br##"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><rect width="2" height="2"/></svg>"##[..]);
        let bad: Arc<[u8]> = Arc::from(&b"not svg"[..]);
        let mut paint = SvgPaint::default();
        let dropped = std::cell::Cell::new(0);
        let run = |paint: &mut SvgPaint, bytes: &Arc<[u8]>| {
            paint.resolve(
                key(bytes, 2, RED),
                Instant::now(),
                &mut false,
                |key| rasterize(&renderer(), bytes, key.width, key.height, RED),
                |_| dropped.set(dropped.get() + 1),
            )
        };
        assert!(run(&mut paint, &good).is_some());
        assert!(run(&mut paint, &bad).is_none());
        assert!(run(&mut paint, &bad).is_none());
        assert_eq!(dropped.get(), 1);
        assert!(paint.held().is_none());
    }

    #[test]
    fn an_svg_that_changes_every_frame_holds_one_image_in_the_atlas() {
        let mut paint = SvgPaint::default();
        let dropped = std::cell::Cell::new(0);
        for frame in 0..30 {
            let bytes: Arc<[u8]> = Arc::from(
                format!(
                    r##"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><rect width="2" height="2" fill="rgb({}, 0, 0)"/></svg>"##,
                    frame * 8
                )
                .into_bytes(),
            );
            let image = paint.resolve(
                key(&bytes, 2, RED),
                Instant::now(),
                &mut false,
                |key| rasterize(&renderer(), &bytes, key.width, key.height, RED),
                |_| dropped.set(dropped.get() + 1),
            );
            assert!(image.is_some());
        }
        // Each new image replaced the one before it, which went out of the atlas.
        assert_eq!(dropped.get(), 29);
        assert!(paint.held().is_some());
    }

    #[test]
    fn finds_the_root_tag_past_a_prolog_and_comments() {
        let svg = b"<?xml version=\"1.0\"?><!-- <svgfoo> --><svg a=\">\" b='>'>";
        let (start, end) = root_tag(svg).unwrap();
        assert!(svg[start..end].starts_with(b"<svg a="));
        assert!(svg[start..end].ends_with(b"b='>'>"));
        assert!(has_attribute(&svg[start..end], b"b"));
        assert!(!has_attribute(b"<svg stroke-color=\"x\">", b"color"));
        assert!(has_attribute(b"<svg  color = \"x\">", b"color"));
    }
}
