//! Full-colour SVG painting for `<svg>`.
//!
//! gpui's own `svg()` element is a monochrome mask: it keeps only the alpha of
//! the rasterised SVG and tints it with one colour. This module rasterises with
//! resvg through gpui's `SvgRenderer` and keeps every fill and stroke, at device
//! resolution. `currentColor` resolves to the colour the caller passes, which is
//! the element's computed `color`.
//!
//! [`SvgPaint::image`] is the one entry point. It holds the single image it last
//! made and drops it from the sprite atlas when a newer one replaces it, so an
//! SVG that changes every frame (an animation feeding serialised children) keeps
//! the atlas at one image.
//!
//! A small document at a small size ([`Key::is_inline`]) rasterises in paint: it
//! costs less than the frame it would otherwise miss. Anything bigger is
//! rasterised on the background executor, one job at a time per `<svg>`, and the
//! previous image keeps being drawn (the caller stretches it to the box) until
//! the job lands and repaints the view. The first paint of a big SVG draws
//! nothing. A job cannot be cancelled once it runs, so a key that changes while
//! one runs waits for it, and only the newest key asked for is rasterised next:
//! the ones in between are dropped.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{DevicePixels, Pixels, RenderImage, Rgba, Size, SvgRenderer, SvgSize, Task};

/// Widest request for the first, cheap render that measures the SVG's aspect ratio.
const PROBE_WIDTH: i32 = 256;

/// A document up to this many bytes, drawn up to [`INLINE_PIXELS`], is rasterised in paint.
const INLINE_BYTES: usize = 4096;
const INLINE_PIXELS: i64 = 128 * 128;

type Raster = Result<Arc<RenderImage>, String>;

#[derive(Default)]
pub struct SvgPaint {
    shown: Option<Shown>,
    running: Option<Running>,
    /// Where the running job leaves its result for the next paint to pick up.
    inbox: Rc<RefCell<Option<Raster>>>,
    /// Bumped when something newer than the running job is installed, so its result is dropped.
    epoch: u64,
}

struct Shown {
    key: Key,
    /// `None` when rasterising these inputs failed. The failure is kept under
    /// the same key so bad markup is parsed and reported once, not per repaint.
    image: Option<Arc<RenderImage>>,
}

struct Running {
    key: Key,
    epoch: u64,
    /// Dropping it stops the repaint request that follows the job, not the job.
    task: Option<Task<()>>,
}

/// A job to start: rasterise `key` and hand the result to [`Job::finish`].
struct Job {
    key: Key,
    inbox: Rc<RefCell<Option<Raster>>>,
}

impl Job {
    fn finish(self, result: Raster) {
        *self.inbox.borrow_mut() = Some(result);
    }
}

/// What a paint needs to do after [`SvgPaint::resolve`].
struct Outcome {
    image: Option<Arc<RenderImage>>,
    /// Images this `<svg>` no longer holds, to drop from the sprite atlas.
    stale: Vec<Arc<RenderImage>>,
    job: Option<Job>,
}

#[derive(Clone)]
struct Key {
    bytes: Arc<[u8]>,
    width: i32,
    height: i32,
    color: [u32; 4],
}

impl Key {
    fn same_as(&self, other: &Key) -> bool {
        self.width == other.width
            && self.height == other.height
            && self.color == other.color
            && (Arc::ptr_eq(&self.bytes, &other.bytes) || self.bytes == other.bytes)
    }

    fn is_inline(&self) -> bool {
        self.bytes.len() <= INLINE_BYTES
            && i64::from(self.width) * i64::from(self.height) <= INLINE_PIXELS
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
    /// While a job for a big SVG runs, the image returned is the previous one,
    /// made for another size, colour or document, and `None` before the first.
    /// The view is repainted when the job lands.
    ///
    /// `None` also means the SVG could not be rasterised; that is logged once and
    /// retried only when the bytes, size, scale or colour change.
    #[allow(clippy::too_many_arguments)]
    pub fn image(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
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
        let outcome = self.resolve(key, |key| {
            rasterize(renderer, bytes, key.width, key.height, current_color)
        });
        for stale in outcome.stale {
            window.drop_image(stale).ok();
        }
        if let Some(job) = outcome.job {
            let renderer = renderer.clone();
            let bytes = bytes.clone();
            let (width, height) = (job.key.width, job.key.height);
            let raster = cx
                .background_executor()
                .spawn(async move { rasterize(&renderer, &bytes, width, height, current_color) });
            let view = window.current_view();
            let task = window.spawn(cx, async move |cx| {
                job.finish(raster.await);
                cx.update(|_, cx| cx.notify(view)).ok();
            });
            self.running
                .as_mut()
                .expect("resolve tracks the job it returns")
                .task = Some(task);
        }
        outcome.image
    }

    fn resolve(&mut self, key: Key, rasterize: impl FnOnce(&Key) -> Raster) -> Outcome {
        let mut stale = Vec::new();
        self.land(&mut stale);
        let current = |paint: &Self| paint.shown.as_ref().and_then(|shown| shown.image.clone());
        if self.shown.as_ref().is_some_and(|shown| shown.key.same_as(&key)) {
            return Outcome { image: current(self), stale, job: None };
        }
        if key.is_inline() {
            self.epoch += 1;
            self.install(key.clone(), rasterize(&key), &mut stale);
            return Outcome { image: current(self), stale, job: None };
        }
        if self.running.is_some() {
            return Outcome { image: current(self), stale, job: None };
        }
        self.running = Some(Running { key: key.clone(), epoch: self.epoch, task: None });
        let job = Job { key, inbox: self.inbox.clone() };
        Outcome { image: current(self), stale, job: Some(job) }
    }

    /// Installs what the running job left, unless something newer was installed since.
    fn land(&mut self, stale: &mut Vec<Arc<RenderImage>>) {
        let Some(result) = self.inbox.borrow_mut().take() else {
            return;
        };
        // No running job: `clear` dropped it while its result was on the way.
        let Some(running) = self.running.take() else {
            return;
        };
        if running.epoch == self.epoch {
            self.install(running.key, result, stale);
        }
    }

    fn install(&mut self, key: Key, result: Raster, stale: &mut Vec<Arc<RenderImage>>) {
        // Drop first: if rasterising failed the previous image must not linger.
        stale.extend(self.shown.take().and_then(|shown| shown.image));
        let image = match result {
            Ok(image) => Some(image),
            Err(error) => {
                log::warn!("<svg> could not be rasterised: {error}");
                None
            }
        };
        self.shown = Some(Shown { key, image });
    }

    /// Something was painted and nothing is on its way: painting the same box again would draw the
    /// image already shown. While a job runs, or before the first paint, it would not.
    pub fn settled(&self) -> bool {
        self.shown.is_some() && self.running.is_none() && self.inbox.borrow().is_none()
    }

    /// Drops the image held, for an `<svg>` that has nothing left to paint.
    pub fn clear(&mut self, drop_image: impl FnOnce(Arc<RenderImage>)) {
        self.running = None;
        self.inbox.borrow_mut().take();
        if let Some(stale) = self.shown.take().and_then(|shown| shown.image) {
            drop_image(stale);
        }
    }

    /// The image currently held, so the owner can drop it from the atlas on teardown.
    pub fn held(&self) -> Option<Arc<RenderImage>> {
        self.shown.as_ref().and_then(|shown| shown.image.clone())
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

    fn fake_raster(key: &Key) -> Raster {
        let (width, height) = (key.width as u32, key.height as u32);
        gpui::RenderImage::from_bgra(width, height, vec![0; width as usize * height as usize * 4])
            .map(Arc::new)
            .ok_or_else(|| "bad".to_string())
    }

    fn never_in_paint(_: &Key) -> Raster {
        panic!("rasterised in paint");
    }

    /// Wider than [`INLINE_PIXELS`] allows, so it takes the background path.
    fn big(bytes: &Arc<[u8]>, width: i32) -> Key {
        Key { height: width, ..key(bytes, width, RED) }
    }

    fn small(bytes: &Arc<[u8]>, width: i32) -> Key {
        key(bytes, width, RED)
    }

    fn width_of(outcome: &Outcome) -> Option<i32> {
        outcome.image.as_ref().map(|image| image.size(0).width.0)
    }

    fn doc() -> Arc<[u8]> {
        Arc::from(&b"x"[..])
    }

    #[test]
    fn a_small_svg_is_rasterised_in_paint() {
        let bytes = doc();
        let mut paint = SvgPaint::default();
        let outcome = paint.resolve(small(&bytes, 24), fake_raster);
        assert_eq!(width_of(&outcome), Some(24));
        assert!(outcome.job.is_none());
        let outcome = paint.resolve(small(&bytes, 40), fake_raster);
        assert_eq!(width_of(&outcome), Some(40));
        assert_eq!(outcome.stale.len(), 1, "the 24 px image left the atlas");
    }

    #[test]
    fn a_big_svg_draws_nothing_on_its_first_paint_and_starts_one_job() {
        let bytes = doc();
        let mut paint = SvgPaint::default();
        let outcome = paint.resolve(big(&bytes, 1200), never_in_paint);
        assert!(outcome.image.is_none());
        assert_eq!(outcome.job.as_ref().map(|job| job.key.width), Some(1200));
        let again = paint.resolve(big(&bytes, 1200), never_in_paint);
        assert!(again.image.is_none());
        assert!(again.job.is_none(), "the same key is already running");
    }

    #[test]
    fn the_previous_raster_is_drawn_while_a_slow_job_is_pending() {
        let bytes = doc();
        let mut paint = SvgPaint::default();
        paint.resolve(small(&bytes, 100), fake_raster);
        let outcome = paint.resolve(big(&bytes, 1200), never_in_paint);
        assert_eq!(width_of(&outcome), Some(100));
        assert!(outcome.job.is_some());
        assert!(outcome.stale.is_empty(), "the previous image stays in the atlas until the new one lands");
        let job = outcome.job.unwrap();
        // Every paint while the job runs still draws the old one.
        for width in [1100, 1000, 900] {
            let outcome = paint.resolve(big(&bytes, width), never_in_paint);
            assert_eq!(width_of(&outcome), Some(100));
            assert!(outcome.job.is_none());
        }
        let landed = fake_raster(&job.key);
        job.finish(landed);
        let outcome = paint.resolve(big(&bytes, 1200), never_in_paint);
        assert_eq!(width_of(&outcome), Some(1200));
        assert_eq!(outcome.stale.len(), 1, "the 100 px image left the atlas");
        assert!(outcome.job.is_none());
    }

    #[test]
    fn a_superseded_job_is_dropped_and_only_the_newest_request_is_rasterised_next() {
        let bytes = doc();
        let mut paint = SvgPaint::default();
        let first = paint.resolve(big(&bytes, 1000), never_in_paint).job.unwrap();
        for width in [1100, 1200, 1300] {
            assert!(paint.resolve(big(&bytes, width), never_in_paint).job.is_none());
        }
        let landed = fake_raster(&first.key);
        first.finish(landed);
        // The first job's raster is the newest thing there is: it is drawn, then the
        // latest size is started, without 1100 or 1200 ever being rasterised.
        let outcome = paint.resolve(big(&bytes, 1300), never_in_paint);
        assert_eq!(width_of(&outcome), Some(1000));
        assert_eq!(outcome.job.as_ref().map(|job| job.key.width), Some(1300));
    }

    #[test]
    fn a_job_that_lands_after_a_newer_raster_was_installed_is_dropped() {
        let bytes = doc();
        let mut paint = SvgPaint::default();
        let slow = paint.resolve(big(&bytes, 1200), never_in_paint).job.unwrap();
        // The box shrank to an icon, which rasterises in paint.
        let outcome = paint.resolve(small(&bytes, 24), fake_raster);
        assert_eq!(width_of(&outcome), Some(24));
        let landed = fake_raster(&slow.key);
        slow.finish(landed);
        let outcome = paint.resolve(small(&bytes, 24), never_in_paint);
        assert_eq!(width_of(&outcome), Some(24), "the 1200 px raster did not overwrite it");
        assert!(outcome.stale.is_empty());
        assert_eq!(paint.held().unwrap().size(0).width.0, 24);
        // The dropped job freed the slot: a big request starts again.
        assert!(paint.resolve(big(&bytes, 1200), never_in_paint).job.is_some());
    }

    #[test]
    fn a_job_that_lands_after_clear_is_dropped() {
        let bytes = doc();
        let mut paint = SvgPaint::default();
        let job = paint.resolve(big(&bytes, 1200), never_in_paint).job.unwrap();
        paint.clear(|_| {});
        let landed = fake_raster(&job.key);
        job.finish(landed);
        let outcome = paint.resolve(big(&bytes, 1200), never_in_paint);
        assert!(outcome.image.is_none());
        assert!(outcome.job.is_some());
        assert!(paint.held().is_none());
    }

    #[test]
    fn a_colour_change_on_a_big_svg_keeps_the_old_colours_until_the_job_lands() {
        let bytes = doc();
        let mut paint = SvgPaint::default();
        let job = paint.resolve(big(&bytes, 1200), never_in_paint).job.unwrap();
        let landed = fake_raster(&job.key);
        job.finish(landed);
        paint.resolve(big(&bytes, 1200), never_in_paint);
        let recoloured = Key { color: [0; 4], ..big(&bytes, 1200) };
        let outcome = paint.resolve(recoloured, never_in_paint);
        assert_eq!(width_of(&outcome), Some(1200));
        assert!(outcome.job.is_some());
        assert!(outcome.stale.is_empty());
    }

    #[test]
    fn clear_drops_the_held_image_once() {
        let bytes = doc();
        let mut paint = SvgPaint::default();
        paint.resolve(small(&bytes, 100), fake_raster);
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
        let attempt = |paint: &mut SvgPaint, key: Key| {
            let found = paint.resolve(key, |key| {
                attempts.set(attempts.get() + 1);
                rasterize(&renderer(), &bad, key.width, key.height, RED)
            });
            assert!(found.image.is_none());
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
            let outcome = paint.resolve(key(bytes, 2, RED), |key| {
                rasterize(&renderer(), bytes, key.width, key.height, RED)
            });
            dropped.set(dropped.get() + outcome.stale.len());
            outcome.image
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
            let outcome = paint.resolve(key(&bytes, 2, RED), |key| {
                rasterize(&renderer(), &bytes, key.width, key.height, RED)
            });
            dropped.set(dropped.get() + outcome.stale.len());
            assert!(outcome.image.is_some());
        }
        // Each new image replaced the one before it, which went out of the atlas.
        assert_eq!(dropped.get(), 29);
        assert!(paint.held().is_some());
    }

    struct Painted {
        paint: Rc<RefCell<SvgPaint>>,
        bytes: Arc<[u8]>,
        size: Rc<std::cell::Cell<f32>>,
        drawn: Rc<RefCell<Vec<Option<i32>>>>,
    }

    impl gpui::Render for Painted {
        fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
            use gpui::Styled;
            let (paint, bytes, size, drawn) =
                (self.paint.clone(), self.bytes.clone(), self.size.clone(), self.drawn.clone());
            gpui::canvas(
                |_, _, _| (),
                move |_, _, window, cx| {
                    let side = gpui::px(size.get());
                    let image = paint.borrow_mut().image(
                        window,
                        cx,
                        &renderer(),
                        &bytes,
                        gpui::size(side, side),
                        1.0,
                        RED,
                    );
                    drawn.borrow_mut().push(image.map(|image| image.size(0).width.0));
                },
            )
            .size_full()
        }
    }

    #[gpui::test]
    fn a_landed_raster_repaints_the_view_and_a_resize_draws_the_old_one_until_then(
        cx: &mut gpui::TestAppContext,
    ) {
        let drawn = Rc::new(RefCell::new(Vec::new()));
        let size = Rc::new(std::cell::Cell::new(300.0));
        let view = Painted {
            paint: Rc::default(),
            bytes: Arc::from(
                &br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><rect width="1" height="1"/></svg>"##[..],
            ),
            size: size.clone(),
            drawn: drawn.clone(),
        };
        let window = cx.add_window(|_, _| view);
        let last = |drawn: &Rc<RefCell<Vec<Option<i32>>>>| *drawn.borrow().last().unwrap();
        assert_eq!(last(&drawn), None, "the first paint does not wait for the raster");
        let painted_before = drawn.borrow().len();

        cx.run_until_parked();
        assert!(drawn.borrow().len() > painted_before, "the landing repainted the view");
        assert_eq!(last(&drawn), Some(300));

        size.set(400.0);
        window.update(cx, |_, window, _| window.refresh()).unwrap();
        // The job for 400 px has not run yet: the 300 px raster is still what is drawn.
        assert_eq!(last(&drawn), Some(300));
        cx.run_until_parked();
        assert_eq!(last(&drawn), Some(400));
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
