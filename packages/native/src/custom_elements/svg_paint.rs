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

use std::sync::Arc;

use gpui::{DevicePixels, Pixels, RenderImage, Rgba, Size, SvgRenderer, SvgSize};

#[derive(Default)]
pub struct SvgPaint {
    cached: Option<Cached>,
}

struct Cached {
    key: Key,
    image: Arc<RenderImage>,
}

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
    /// The previous image is dropped from the atlas when the inputs changed.
    pub fn image(
        &mut self,
        window: &mut gpui::Window,
        renderer: &SvgRenderer,
        bytes: &Arc<[u8]>,
        size: Size<Pixels>,
        scale_factor: f32,
        current_color: Rgba,
    ) -> Result<Arc<RenderImage>, String> {
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
        if let Some(cached) = &self.cached {
            if cached.key.same_as(&key) {
                return Ok(cached.image.clone());
            }
        }
        // Drop first: if rasterising fails the stale image must not linger.
        if let Some(stale) = self.cached.take() {
            window.drop_image(stale.image).ok();
        }
        let image = rasterize(renderer, bytes, key.width, key.height, current_color)?;
        self.cached = Some(Cached {
            key,
            image: image.clone(),
        });
        Ok(image)
    }

    /// The image currently held, so the owner can drop it from the atlas on teardown.
    pub fn held(&self) -> Option<Arc<RenderImage>> {
        self.cached.as_ref().map(|cached| cached.image.clone())
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
    let render = |width: i32| {
        renderer
            .render_parsed(
                &svg,
                SvgSize::Size(gpui::size(DevicePixels(width), DevicePixels(1))),
            )
            .map_err(|error| error.to_string())
    };
    // `SvgSize::Size` scales by width and keeps the aspect ratio. If that is too
    // tall for the box, scale by the height instead.
    let image = render(width)?;
    let rendered = image.size(0);
    if rendered.height.0 <= height {
        return Ok(image);
    }
    let fitted = (width as f64 * height as f64 / rendered.height.0 as f64).floor() as i32;
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

    #[test]
    fn rejects_input_that_is_not_an_svg() {
        assert!(rasterize(&renderer(), b"not svg", 4, 4, RED).is_err());
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
