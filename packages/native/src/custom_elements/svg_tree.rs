//! Serialises the DOM-style children of an `<svg>` (`path`, `circle`, `g`, ...)
//! to SVG XML for the rasteriser.
//!
//! The children are retained-tree nodes that never become GPUI elements: the
//! `<svg>` paints them all as one image. Props are camelCase, as in React and
//! Solid, and become kebab-case attributes here. Animated attributes arrive
//! from the motion engine and replace the element's own.
//!
//! Besides shapes, the tree takes what a shape refers to by `id`: `defs`,
//! `linearGradient`, `radialGradient`, `stop`, `clipPath`, `mask`, `symbol` and
//! `use`, and `pattern`, `marker`, `filter` with its `fe*` primitives, `image`, and
//! `text` with `tspan` and `textPath`. resvg resolves those references itself, so
//! they are written as they come. A string inside a text element is a text node.
//!
//! resvg ignores the `pathLength` attribute, so every dash that depends on it
//! is turned into absolute user units against the shape's measured length.
//!
//! A shape's `style` may name CSS animations. Their keyframes drive the SVG
//! properties CSS animates (`strokeDashoffset`, `fill`, `r`...) and their values
//! are written over the shape's own attributes and over its `motion`, as an
//! animation beats a presentation attribute. A shape is never built, so the
//! `<svg>` advances these animations while it writes its markup.
//!
//! resvg knows only SVG's own colours, so a `fill` or `stroke` that needs the
//! element's context (`light-dark()`, `color-mix()`, `color()`) is resolved here,
//! with the `<svg>`'s, and written as `rgba()`.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use serde_json::Value;
use web_time::Instant;

use super::svg_names::{self, ATTRIBUTES, COLOUR_ATTRIBUTES, MARKER_ATTRIBUTES, MARKER_SHORTHAND};
use super::svg_path::{
    circle_length, ellipse_length, format_number, line_length, polyline_length, rect_length,
    PathData,
};
use crate::color::ColorContext;
use crate::motion::{
    CssAnimations, KeyframesRegistry, MotionState, ParentFont, Subject, SvgMotion, SvgSource,
};
use crate::retained_tree::RetainedTree;
use crate::style::{DimensionValue, StyleDesc};

/// Whether `tag` is an element only valid inside an `<svg>`.
pub(crate) fn is_svg_child_type(tag: &str) -> bool {
    svg_names::is_tag(tag)
}

/// Whether `element` is drawn by an `<svg>` instead of laid out: an SVG-only tag,
/// or a `<text>` inside an SVG. A `<text>` with a string is that string, not an
/// element.
pub(crate) fn is_svg_element(tree: &RetainedTree, element: &crate::retained_tree::RetainedElement) -> bool {
    if is_svg_child_type(&element.element_type) {
        return true;
    }
    element.element_type == "text"
        && element.content.is_none()
        && element
            .parent
            .and_then(|parent| tree.elements.get(&parent))
            .is_some_and(|parent| parent.element_type == "svg" || is_svg_element(tree, parent))
}

/// Props every host element takes that have nothing to say to the XML.
const HOST_PROPS: &[&str] = &[
    "src",
    "source",
    "motion",
    "tabIndex",
    "highlight",
    "role",
    "aria-label",
    "aria-description",
    "aria-id",
    "aria-expanded",
    "aria-selected",
    "aria-valuetext",
    "aria-level",
];

struct Node<'a> {
    tag: &'a str,
    props: &'a HashMap<String, Value>,
    /// A string child of a text element, written as character data. Such a node
    /// has no tag, props or children of its own.
    text: Option<&'a str>,
    motion: Option<SvgMotion>,
    /// The attributes its CSS animations write, over `motion` and its own.
    css: Vec<(&'static str, String)>,
    children: Vec<Node<'a>>,
}

/// What an `<svg>` needs from the frame to write its markup.
pub(crate) struct Frame<'a> {
    pub motion_states: &'a HashMap<u64, MotionState>,
    pub animations: &'a mut CssAnimations,
    pub keyframes: &'a KeyframesRegistry,
    /// The `<svg>`'s own colours and font, which its shapes inherit.
    pub colors: &'a ColorContext,
    pub font: &'a ParentFont,
    /// What the animations of the `<svg>` itself write on the root, where every
    /// shape below inherits it.
    pub host: Vec<(&'static str, String)>,
    pub now: Instant,
}

/// What the shapes above a node pass down.
struct Above<'a> {
    colors: ColorContext,
    font: ParentFont,
    /// Their props, the nearest last.
    props: Vec<&'a HashMap<String, Value>>,
}

/// The serialised markup of every `<svg>` with children, reused until a
/// descendant changes or animates.
#[derive(Default)]
pub(crate) struct SvgTrees {
    cache: HashMap<u64, Cached>,
    /// Counts the markups made, so no two have the same generation.
    made: u64,
}

struct Cached {
    revision: u64,
    animation: u64,
    /// What the colours in `markup` resolved in.
    colors: ColorContext,
    markup: Arc<str>,
    generation: u64,
}

impl SvgTrees {
    /// The XML for `svg_id`, or `None` when it has no shape children and its
    /// `src` / `source` should be used as before.
    pub(crate) fn markup(
        &mut self,
        tree: &RetainedTree,
        svg_id: u64,
        frame: Frame,
    ) -> Option<Arc<str>> {
        let element = tree.elements.get(&svg_id)?;
        let mut frame = frame;
        let mut above = Above {
            colors: *frame.colors,
            font: frame.font.clone(),
            props: vec![element.custom_props()],
        };
        let children = nodes(tree, "svg", &element.children, &mut frame, &mut above);
        if children.is_empty() {
            self.cache.remove(&svg_id);
            return None;
        }
        let colors = frame.colors;
        let root = Node {
            tag: "svg",
            props: element.custom_props(),
            text: None,
            motion: None,
            css: frame.host,
            children,
        };
        let animation = animation_key(std::slice::from_ref(&root));
        if let Some(cached) = self.cache.get(&svg_id) {
            if cached.revision == element.subtree_revision
                && cached.animation == animation
                && cached.colors == *colors
            {
                return Some(cached.markup.clone());
            }
        }
        let markup: Arc<str> = serialize(&root, colors).into();
        self.made += 1;
        self.cache.insert(
            svg_id,
            Cached {
                revision: element.subtree_revision,
                animation,
                colors: *colors,
                markup: markup.clone(),
                generation: self.made,
            },
        );
        Some(markup)
    }

    /// The markup `markup` last returned for `svg_id`, as the last frame wrote it.
    pub(crate) fn current(&self, svg_id: u64) -> Option<Arc<str>> {
        self.cache.get(&svg_id).map(|cached| cached.markup.clone())
    }

    /// Names the markup `markup` last returned for `svg_id`: it differs from the one before it
    /// whenever the XML was written again, which is the only way it changes. 0 without markup.
    pub(crate) fn generation(&self, svg_id: u64) -> u64 {
        self.cache.get(&svg_id).map_or(0, |cached| cached.generation)
    }

    pub(crate) fn retain(&mut self, mut is_live: impl FnMut(u64) -> bool) {
        self.cache.retain(|id, _| is_live(*id));
    }
}

fn nodes<'a>(
    tree: &'a RetainedTree,
    parent: &str,
    ids: &[u64],
    frame: &mut Frame,
    above: &mut Above<'a>,
) -> Vec<Node<'a>> {
    ids.iter()
        .filter_map(|id| tree.elements.get(id))
        .filter_map(|element| {
            if let Some(content) = text_node(element) {
                if !svg_names::holds_text(parent) {
                    log::warn!("text inside <{parent}> is not drawn: only <text>, <tspan> and <textPath> hold text");
                    return None;
                }
                return Some(Node {
                    tag: "",
                    props: element.custom_props(),
                    text: Some(content),
                    motion: None,
                    css: Vec::new(),
                    children: Vec::new(),
                });
            }
            if element.element_type != "text" && !is_svg_child_type(&element.element_type) {
                log::warn!(
                    "<{}> inside <svg> is not an SVG element and is not drawn",
                    element.element_type
                );
                return None;
            }
            let css = animate(element, frame, above);
            let own = element
                .style
                .as_deref()
                .map(|style| (above.colors.descend(style), above.font.descend(Some(style))));
            let saved = own.map(|(colors, font)| {
                (std::mem::replace(&mut above.colors, colors), std::mem::replace(&mut above.font, font))
            });
            above.props.push(element.custom_props());
            let children = nodes(tree, &element.element_type, &element.children, frame, above);
            above.props.pop();
            if let Some((colors, font)) = saved {
                above.colors = colors;
                above.font = font;
            }
            Some(Node {
                tag: &element.element_type,
                props: element.custom_props(),
                text: None,
                motion: frame
                    .motion_states
                    .get(&element.id)
                    .and_then(|state| state.visible_svg(frame.now)),
                css,
                children,
            })
        })
        .collect()
}

/// Advances the CSS animations of a shape to the frame and returns the
/// attributes they write.
fn animate(
    element: &crate::retained_tree::RetainedElement,
    frame: &mut Frame,
    above: &Above,
) -> Vec<(&'static str, String)> {
    let style = element.style.as_deref();
    if !frame.animations.wants(element.id, style) {
        return Vec::new();
    }
    let none;
    let declared = match style {
        Some(style) => style,
        None => {
            none = StyleDesc::default();
            &none
        }
    };
    let subject = Subject::svg_shape(declared, SvgSource { own: element.custom_props(), ancestors: &above.props });
    frame
        .animations
        .animate_shape(
            element.id,
            declared,
            frame.keyframes,
            &subject,
            &above.colors,
            &above.font,
            frame.now,
        )
        .map(|values| values.svg_attributes())
        .unwrap_or_default()
}

/// The string of a text node: React and Solid make a `text` element with
/// content for a string child, and one without for a `<text>` tag.
fn text_node(element: &crate::retained_tree::RetainedElement) -> Option<&str> {
    if element.element_type == "text" {
        element.content.as_deref()
    } else {
        None
    }
}

fn animation_key(nodes: &[Node]) -> u64 {
    fn visit(nodes: &[Node], hasher: &mut impl Hasher) {
        for node in nodes {
            if let Some(motion) = &node.motion {
                motion.attributes.hash(hasher);
                if let Some(draw) = motion.draw {
                    [draw.length, draw.offset, draw.spacing]
                        .map(f64::to_bits)
                        .hash(hasher);
                }
            }
            node.css.hash(hasher);
            visit(&node.children, hasher);
        }
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    visit(nodes, &mut hasher);
    hasher.finish()
}

fn serialize(root: &Node, colors: &ColorContext) -> String {
    let mut xml = String::new();
    write_node(root, colors, &mut xml);
    xml
}

fn write_node(node: &Node, colors: &ColorContext, xml: &mut String) {
    if let Some(text) = node.text {
        escape_into(text, xml);
        return;
    }
    xml.push('<');
    xml.push_str(node.tag);
    if node.tag == "svg" {
        xml.push_str(" xmlns=\"http://www.w3.org/2000/svg\"");
    }
    for (name, value) in attributes(node, colors) {
        xml.push(' ');
        xml.push_str(name);
        xml.push_str("=\"");
        escape_into(&value, xml);
        xml.push('"');
    }
    if node.children.is_empty() {
        xml.push_str("/>");
        return;
    }
    xml.push('>');
    for child in &node.children {
        write_node(child, colors, xml);
    }
    xml.push_str("</");
    xml.push_str(node.tag);
    xml.push('>');
}

fn escape_into(value: &str, xml: &mut String) {
    for character in value.chars() {
        match character {
            '&' => xml.push_str("&amp;"),
            '<' => xml.push_str("&lt;"),
            '>' => xml.push_str("&gt;"),
            '"' => xml.push_str("&quot;"),
            '\'' => xml.push_str("&apos;"),
            other => xml.push(other),
        }
    }
}

/// The attributes a node is written with, in a fixed order: its own props,
/// then the animated ones replacing them, then the dashes a draw or a DOM
/// `pathLength` turns into absolute units.
fn attributes(node: &Node, colors: &ColorContext) -> Vec<(&'static str, String)> {
    let mut written: Vec<(&'static str, String)> = Vec::new();
    for (prop, attribute) in ATTRIBUTES {
        if let Some(text) = node.props.get(*prop).and_then(attribute_text) {
            let text = if COLOUR_ATTRIBUTES.contains(attribute) {
                resolve_paint(text, colors)
            } else {
                text
            };
            written.push((attribute, text));
        }
    }
    if let Some(shorthand) = node.props.get(MARKER_SHORTHAND).and_then(attribute_text) {
        for (prop, attribute) in MARKER_ATTRIBUTES {
            if !node.props.contains_key(prop) {
                written.push((attribute, shorthand.clone()));
            }
        }
    }
    for key in node.props.keys() {
        let known = key == MARKER_SHORTHAND || ATTRIBUTES.iter().any(|(prop, _)| prop == key);
        let host = HOST_PROPS.contains(&key.as_str()) || key == "testId" || key == "autoFocus";
        if !known && !host {
            log::warn!("<{}> has no SVG attribute for the prop {key:?}", node.tag);
        }
    }
    if matches!(node.tag, "image" | "feImage") {
        resolve_image_href(node.tag, &mut written);
    }
    if let Some(motion) = &node.motion {
        for (name, text) in &motion.attributes {
            set(&mut written, name, text.clone());
        }
    }
    for (name, text) in &node.css {
        set(&mut written, name, text.clone());
    }

    // The draw wins over the element's own `pathLength`, as it does in Framer.
    let dom_path_length = if node.motion.as_ref().is_some_and(|motion| motion.draw.is_some()) {
        None
    } else {
        number(&written, "pathLength").filter(|length| *length > 0.0)
    };
    written.retain(|(name, _)| *name != "pathLength");
    let draw = node.motion.as_ref().and_then(|motion| motion.draw);
    if draw.is_none() && dom_path_length.is_none() {
        return written;
    }

    let Some(length) = shape_length(node.tag, &written).filter(|length| *length > 0.0) else {
        return written;
    };
    if let Some(draw) = draw {
        // An animation beats the motion's draw, as it beats everything it names.
        let animated = |name: &str| node.css.iter().any(|(attribute, _)| *attribute == name);
        if !animated("stroke-dasharray") {
            set(
                &mut written,
                "stroke-dasharray",
                format!(
                    "{} {}",
                    format_number(draw.length * length),
                    format_number(draw.spacing * length)
                ),
            );
        }
        if !animated("stroke-dashoffset") {
            set(
                &mut written,
                "stroke-dashoffset",
                format_number(-draw.offset * length),
            );
        }
    } else if let Some(declared) = dom_path_length {
        let scale = length / declared;
        for name in ["stroke-dasharray", "stroke-dashoffset"] {
            if let Some((_, text)) = written.iter_mut().find(|(attribute, _)| *attribute == name) {
                *text = scale_numbers(text, scale);
            }
        }
    }
    written
}

/// resvg loads an image from a `data:` URL or a file path, synchronously. A
/// `file://` URL is turned into its path, as `<img>` does. A `blob:` image is
/// fetched by JS and an `http(s)` one is a network request, and a paint cannot
/// wait for either, so they are dropped with a warning.
fn resolve_image_href(tag: &str, written: &mut Vec<(&'static str, String)>) {
    let Some(index) = written.iter().position(|(name, _)| *name == "href") else {
        return;
    };
    let href = written[index].1.trim().to_string();
    let scheme = href.split_once(':').map(|(scheme, _)| scheme.to_ascii_lowercase());
    let resolved = match scheme.as_deref() {
        Some("file") => match super::img::file_url_path(&href) {
            Some(path) => Ok(path.to_string_lossy().into_owned()),
            None => Err("is not a valid file URL"),
        },
        Some("blob" | "http" | "https") => Err("can only be a data: URL or a file"),
        _ => Ok(href.clone()),
    };
    match resolved {
        Ok(href) => written[index].1 = href,
        Err(reason) => {
            log::warn!("<{tag}> href {href:?} {reason}");
            written.remove(index);
        }
    }
}

/// A paint that is a colour needing the element's context, as `rgba()`. Anything
/// else (`none`, `url(#gradient)`, a plain colour) is the rasteriser's to read.
fn resolve_paint(text: String, colors: &ColorContext) -> String {
    if !crate::color::is_contextual(&text) {
        return text;
    }
    match colors.resolve(&text) {
        Some(color) => format!(
            "rgba({}, {}, {}, {})",
            (color.r * 255.0).round(),
            (color.g * 255.0).round(),
            (color.b * 255.0).round(),
            color.a
        ),
        None => text,
    }
}

fn set(written: &mut Vec<(&'static str, String)>, name: &'static str, text: String) {
    match written.iter_mut().find(|(attribute, _)| *attribute == name) {
        Some((_, existing)) => *existing = text,
        None => written.push((name, text)),
    }
}

fn number(written: &[(&'static str, String)], name: &str) -> Option<f64> {
    written
        .iter()
        .find(|(attribute, _)| *attribute == name)
        .and_then(|(_, text)| text.trim().trim_end_matches("px").trim().parse().ok())
}

/// Scales the plain numbers in a dash list, leaving other tokens as written.
fn scale_numbers(text: &str, scale: f64) -> String {
    text.split(|character: char| character == ',' || character.is_whitespace())
        .filter(|token| !token.is_empty())
        .map(|token| match token.parse::<f64>() {
            Ok(value) => format_number(value * scale),
            Err(_) => token.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn attribute_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => number.as_f64().map(format_number),
        Value::Array(items) => {
            let parts: Option<Vec<String>> = items.iter().map(attribute_text).collect();
            parts.map(|parts| parts.join(" "))
        }
        _ => None,
    }
}

/// The length of the stroked outline in user units, from the attributes as
/// they will be written. `None` for a shape with no measurable outline.
fn shape_length(tag: &str, written: &[(&'static str, String)]) -> Option<f64> {
    let get = |name: &str| number(written, name);
    let text = |name: &str| {
        written
            .iter()
            .find(|(attribute, _)| *attribute == name)
            .map(|(_, text)| text.as_str())
    };
    match tag {
        "path" => match PathData::parse(text("d")?).and_then(|path| path.length()) {
            Ok(length) => Some(length),
            Err(error) => {
                log::warn!("<path> cannot be drawn: {error}");
                None
            }
        },
        "circle" => Some(circle_length(get("r")?)),
        "ellipse" => {
            let (rx, ry) = (get("rx"), get("ry"));
            Some(ellipse_length(rx.or(ry)?, ry.or(rx)?))
        }
        "rect" => Some(rect_length(get("width")?, get("height")?, get("rx"), get("ry"))),
        "line" => Some(line_length(
            get("x1").unwrap_or(0.0),
            get("y1").unwrap_or(0.0),
            get("x2").unwrap_or(0.0),
            get("y2").unwrap_or(0.0),
        )),
        "polyline" => Some(polyline_length(text("points")?, false)),
        "polygon" => Some(polyline_length(text("points")?, true)),
        _ => None,
    }
}

/// `style` with the `width` and `height` props applied where it sets none, so
/// `<svg width={24} height={24}>` sizes the box as it does in a DOM.
pub(crate) fn sized_style(
    style: Option<&StyleDesc>,
    props: &HashMap<String, Value>,
) -> Option<StyleDesc> {
    let dimension = |name: &str| -> Option<DimensionValue> {
        let value = props.get(name)?;
        let value = match value {
            Value::String(text) => match text.trim().trim_end_matches("px").parse::<f64>() {
                Ok(pixels) => Value::from(pixels),
                Err(_) => value.clone(),
            },
            other => other.clone(),
        };
        serde_json::from_value(value)
            .ok()
            .filter(|dimension| *dimension != DimensionValue::Auto)
    };
    let width = style
        .map_or(true, |style| style.width.is_none())
        .then(|| dimension("width"))
        .flatten();
    let height = style
        .map_or(true, |style| style.height.is_none())
        .then(|| dimension("height"))
        .flatten();
    if width.is_none() && height.is_none() {
        return None;
    }
    let mut sized = style.cloned().unwrap_or_default();
    sized.width = width.or(sized.width);
    sized.height = height.or(sized.height);
    Some(sized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Scheme;
    use crate::motion::PathDraw;

    fn context(color: &str, scheme: &str) -> ColorContext {
        ColorContext::root(Scheme::Light).descend(&StyleDesc {
            color: Some(color.to_string()),
            color_scheme: Some(scheme.to_string()),
            ..Default::default()
        })
    }

    fn serialize(root: &Node) -> String {
        serialize_in(root, &ColorContext::root(Scheme::Light))
    }

    fn serialize_in(root: &Node, colors: &ColorContext) -> String {
        super::serialize(root, colors)
    }

    fn props(pairs: &[(&str, Value)]) -> HashMap<String, Value> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect()
    }

    fn node<'a>(
        tag: &'a str,
        props: &'a HashMap<String, Value>,
        children: Vec<Node<'a>>,
    ) -> Node<'a> {
        Node {
            tag,
            props,
            text: None,
            motion: None,
            css: Vec::new(),
            children,
        }
    }

    #[test]
    fn camel_case_props_become_kebab_case_attributes() {
        let svg = props(&[("viewBox", "0 0 24 24".into()), ("width", 24.into())]);
        let path = props(&[
            ("d", "M0 0L10 10".into()),
            ("strokeWidth", 2.into()),
            ("strokeLinecap", "round".into()),
            ("strokeDasharray", serde_json::json!([4, 2])),
            ("fillOpacity", 0.5.into()),
            ("transform", "rotate(45 12 12)".into()),
        ]);
        let xml = serialize(&node("svg", &svg, vec![node("path", &path, vec![])]));
        assert_eq!(
            xml,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" width=\"24\">\
             <path d=\"M0 0L10 10\" fill-opacity=\"0.5\" stroke-width=\"2\" stroke-linecap=\"round\" \
             stroke-dasharray=\"4 2\" transform=\"rotate(45 12 12)\"/></svg>"
        );
    }

    #[test]
    fn gradients_clips_and_use_are_written_and_the_rasteriser_resolves_them() {
        let svg = props(&[("viewBox", "0 0 20 10".into()), ("width", 20.into()), ("height", 10.into())]);
        let gradient = props(&[("id", "fade".into()), ("x1", 0.into()), ("x2", 1.into())]);
        let red = props(&[("offset", 0.into()), ("stopColor", "#ff0000".into())]);
        let blue = props(&[("offset", "100%".into()), ("stopColor", "#0000ff".into()), ("stopOpacity", 1.into())]);
        let clip = props(&[("id", "left".into())]);
        let half = props(&[("width", 10.into()), ("height", 10.into())]);
        let bar = props(&[
            ("id", "bar".into()),
            ("width", 20.into()),
            ("height", 10.into()),
            ("fill", "url(#fade)".into()),
        ]);
        let clipped = props(&[("href", "#bar".into()), ("clipPath", "url(#left)".into())]);
        let xml = serialize(&node(
            "svg",
            &svg,
            vec![
                node(
                    "defs",
                    &HashMap::new(),
                    vec![
                        node(
                            "linearGradient",
                            &gradient,
                            vec![node("stop", &red, vec![]), node("stop", &blue, vec![])],
                        ),
                        node("clipPath", &clip, vec![node("rect", &half, vec![])]),
                        node("rect", &bar, vec![]),
                    ],
                ),
                node("use", &clipped, vec![]),
            ],
        ));
        assert!(xml.contains("<linearGradient id=\"fade\" x1=\"0\" x2=\"1\">"), "{xml}");
        assert!(xml.contains("<stop offset=\"0\" stop-color=\"#ff0000\"/>"), "{xml}");
        assert!(xml.contains("<use href=\"#bar\" clip-path=\"url(#left)\"/>"), "{xml}");

        let renderer = gpui::SvgRenderer::new(Arc::new(()));
        let image = renderer.render_single_frame(xml.as_bytes(), 1.0).unwrap();
        let size = image.size(0);
        let bytes = image.as_bytes(0).unwrap();
        let pixel = |x: f32| {
            let column = (x / 20.0 * size.width.0 as f32) as usize;
            let row = size.height.0 as usize / 2;
            let at = (row * size.width.0 as usize + column) * 4;
            // BGRA, premultiplied.
            [bytes[at + 2], bytes[at], bytes[at + 3]]
        };
        let near_left = pixel(0.5);
        assert!(near_left[0] > 220 && near_left[1] < 40 && near_left[2] == 255, "gradient start {near_left:?}");
        let middle = pixel(8.0);
        assert!(middle[0] > 40 && middle[1] > 40, "gradient middle {middle:?}");
        assert_eq!(pixel(15.0)[2], 0, "the clip cuts the right half");
    }

    #[test]
    fn a_stop_colour_that_needs_the_context_is_written_as_rgba() {
        let stop = props(&[("stopColor", "light-dark(#ff0000, #0000ff)".into())]);
        let xml = serialize_in(&node("stop", &stop, vec![]), &context("black", "dark"));
        assert!(xml.contains("stop-color=\"rgba(0, 0, 255, 1)\""), "{xml}");
    }

    #[test]
    fn groups_nest_and_empty_shapes_self_close() {
        let svg = props(&[("viewBox", "0 0 10 10".into())]);
        let group = props(&[("opacity", 0.5.into())]);
        let circle = props(&[("cx", 5.into()), ("cy", 5.into()), ("r", 3.into())]);
        let inner = props(&[("x", 1.into()), ("y", 1.into()), ("width", 2.into()), ("height", 2.into())]);
        let xml = serialize(&node(
            "svg",
            &svg,
            vec![node(
                "g",
                &group,
                vec![node("circle", &circle, vec![]), node("rect", &inner, vec![])],
            )],
        ));
        assert!(xml.contains(
            "<g opacity=\"0.5\"><circle cx=\"5\" cy=\"5\" r=\"3\"/><rect x=\"1\" y=\"1\" width=\"2\" height=\"2\"/></g>"
        ), "{xml}");
    }

    #[test]
    fn values_are_escaped() {
        let svg = props(&[]);
        let path = props(&[("fill", "url(\"#a&b\")<'>".into())]);
        let xml = serialize(&node("svg", &svg, vec![node("path", &path, vec![])]));
        assert!(
            xml.contains("fill=\"url(&quot;#a&amp;b&quot;)&lt;&apos;&gt;\""),
            "{xml}"
        );
    }

    #[test]
    fn fill_and_stroke_that_need_the_context_are_written_as_rgba() {
        let svg = props(&[("fill", "light-dark(#ff0000, #0000ff)".into())]);
        let shape = props(&[
            ("fill", "light-dark(#ff0000, #0000ff)".into()),
            ("stroke", "currentColor".into()),
            ("strokeWidth", 2.into()),
        ]);
        let tree = node("svg", &svg, vec![node("path", &shape, vec![])]);
        let xml = serialize_in(&tree, &context("#00ff00", "dark"));
        assert!(xml.contains("fill=\"rgba(0, 0, 255, 1)\""), "{xml}");
        assert!(xml.contains("stroke=\"rgba(0, 255, 0, 1)\""), "{xml}");
        // The root takes the same colours as its children.
        assert!(xml.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" fill=\"rgba(0, 0, 255, 1)\""), "{xml}");

        let mixed = props(&[("fill", "color-mix(in srgb, red, blue)".into())]);
        let xml = serialize(&node("svg", &props(&[]), vec![node("rect", &mixed, vec![])]));
        assert!(xml.contains("fill=\"rgba(128, 0, 128, 1)\""), "{xml}");
    }

    #[test]
    fn a_paint_the_rasteriser_reads_itself_is_left_alone() {
        let shape = props(&[("fill", "url(#gradient)".into()), ("stroke", "none".into())]);
        let xml = serialize(&node("svg", &props(&[]), vec![node("rect", &shape, vec![])]));
        assert!(xml.contains("fill=\"url(#gradient)\""), "{xml}");
        assert!(xml.contains("stroke=\"none\""), "{xml}");
    }

    #[test]
    fn host_props_events_and_unknown_props_are_not_attributes() {
        let svg = props(&[]);
        let path = props(&[
            ("d", "M0 0".into()),
            ("testId", "p".into()),
            ("tabIndex", 0.into()),
            ("motion", serde_json::json!({})),
            ("bogus", 1.into()),
            ("fill", Value::Null),
            ("stroke", true.into()),
        ]);
        let xml = serialize(&node("svg", &svg, vec![node("path", &path, vec![])]));
        assert!(xml.contains("<path d=\"M0 0\"/>"), "{xml}");
    }

    fn animated(tag: &'static str, props: &HashMap<String, Value>, motion: SvgMotion) -> String {
        let svg = HashMap::new();
        let mut shape = node(tag, props, vec![]);
        shape.motion = Some(motion);
        serialize(&node("svg", &svg, vec![shape]))
    }

    #[test]
    fn animated_attributes_replace_the_elements_own() {
        let circle = props(&[("r", 3.into()), ("fill", "red".into())]);
        let xml = animated(
            "circle",
            &circle,
            SvgMotion {
                attributes: vec![("r", "7.5".into())],
                draw: None,
            },
        );
        assert!(xml.contains("<circle r=\"7.5\" fill=\"red\"/>"), "{xml}");
    }

    #[test]
    fn a_draw_becomes_absolute_dashes_and_drops_path_length() {
        // Framer writes pathLength="1" and dashes in fractions; resvg ignores
        // pathLength, so the dashes carry the real length (a 50 unit line).
        let line = props(&[("x2", 30.into()), ("y2", 40.into()), ("pathLength", 9.into())]);
        let xml = animated(
            "line",
            &line,
            SvgMotion {
                attributes: vec![],
                draw: Some(PathDraw {
                    length: 0.5,
                    offset: 0.1,
                    spacing: 1.0,
                }),
            },
        );
        assert!(xml.contains("stroke-dasharray=\"25 50\""), "{xml}");
        assert!(xml.contains("stroke-dashoffset=\"-5\""), "{xml}");
        assert!(!xml.contains("pathLength"), "{xml}");
    }

    #[test]
    fn a_draw_measures_every_shape() {
        let draw = || SvgMotion {
            attributes: vec![],
            draw: Some(PathDraw::default()),
        };
        let array = |tag, shape: HashMap<String, Value>| -> f64 {
            let xml = animated(tag, &shape, draw());
            let start = xml.find("stroke-dasharray=\"").unwrap() + "stroke-dasharray=\"".len();
            xml[start..].split(' ').next().unwrap().parse().unwrap()
        };
        assert_eq!(array("rect", props(&[("width", 10.into()), ("height", 5.into())])), 30.0);
        assert!((array("circle", props(&[("r", 1.into())])) - 6.2832).abs() < 1e-3);
        assert!((array("ellipse", props(&[("rx", 2.into()), ("ry", 2.into())])) - 12.5664).abs() < 1e-2);
        assert_eq!(array("polyline", props(&[("points", "0,0 3,4 3,0".into())])), 9.0);
        assert_eq!(array("polygon", props(&[("points", "0,0 3,4 3,0".into())])), 12.0);
        assert_eq!(array("path", props(&[("d", "M0 0 h10 v10".into())])), 20.0);
    }

    #[test]
    fn the_dom_path_length_scales_declared_dashes() {
        // A 100 unit line declared as pathLength 10: one unit is ten user units.
        let line = props(&[
            ("x2", 100.into()),
            ("pathLength", 10.into()),
            ("strokeDasharray", "2 1".into()),
            ("strokeDashoffset", (-3).into()),
        ]);
        let svg = props(&[]);
        let xml = serialize(&node("svg", &svg, vec![node("line", &line, vec![])]));
        assert!(xml.contains("stroke-dasharray=\"20 10\""), "{xml}");
        assert!(xml.contains("stroke-dashoffset=\"-30\""), "{xml}");
        assert!(!xml.contains("pathLength"), "{xml}");
    }

    #[test]
    fn an_unmeasurable_shape_keeps_its_dashes() {
        let path = props(&[("d", "nonsense".into()), ("strokeDasharray", "2 1".into())]);
        let xml = animated(
            "path",
            &path,
            SvgMotion {
                attributes: vec![],
                draw: Some(PathDraw::default()),
            },
        );
        assert!(xml.contains("stroke-dasharray=\"2 1\""), "{xml}");
    }

    #[test]
    fn width_and_height_props_size_the_box_unless_the_style_does() {
        let props = props(&[("width", 24.into()), ("height", "16px".into())]);
        let sized = sized_style(None, &props).unwrap();
        assert_eq!(sized.width, Some(DimensionValue::Pixels(24.0)));
        assert_eq!(sized.height, Some(DimensionValue::Pixels(16.0)));

        let style = StyleDesc {
            width: Some(DimensionValue::Pixels(50.0)),
            ..Default::default()
        };
        let sized = sized_style(Some(&style), &props).unwrap();
        assert_eq!(sized.width, Some(DimensionValue::Pixels(50.0)));
        assert_eq!(sized.height, Some(DimensionValue::Pixels(16.0)));
        assert!(sized_style(None, &HashMap::new()).is_none());
    }

    /// A rendered document, read in the user units of its `viewBox`.
    struct Raster {
        image: Arc<gpui::RenderImage>,
        view: (f32, f32),
    }

    impl Raster {
        fn of(xml: &str, view: (f32, f32)) -> Self {
            let renderer = gpui::SvgRenderer::new(Arc::new(()));
            let image = renderer
                .render_single_frame(xml.as_bytes(), 1.0)
                .unwrap_or_else(|error| panic!("{error}: {xml}"));
            Raster { image, view }
        }

        /// Straight RGBA at (`x`, `y`) in user units.
        fn at(&self, x: f32, y: f32) -> [u8; 4] {
            let size = self.image.size(0);
            let width = size.width.0 as usize;
            let column = ((x / self.view.0 * width as f32) as usize).min(width - 1);
            let row = ((y / self.view.1 * size.height.0 as f32) as usize).min(size.height.0 as usize - 1);
            let bytes = self.image.as_bytes(0).unwrap();
            let at = (row * width + column) * 4;
            let alpha = bytes[at + 3];
            let straight = |channel: u8| {
                if alpha == 0 {
                    0
                } else {
                    (channel as u32 * 255 / alpha as u32).min(255) as u8
                }
            };
            [straight(bytes[at + 2]), straight(bytes[at + 1]), straight(bytes[at]), alpha]
        }

        fn painted(&self, x: f32, y: f32) -> bool {
            self.at(x, y)[3] > 128
        }
    }

    fn document(width: u32, height: u32, children: Vec<Node>) -> String {
        let svg = props(&[
            ("viewBox", format!("0 0 {width} {height}").into()),
            ("width", width.into()),
            ("height", height.into()),
        ]);
        serialize(&node("svg", &svg, children))
    }

    static NO_PROPS: std::sync::LazyLock<HashMap<String, Value>> =
        std::sync::LazyLock::new(HashMap::new);

    fn leaf(text: &str) -> Node<'_> {
        Node {
            tag: "",
            props: &NO_PROPS,
            text: Some(text),
            motion: None,
            css: Vec::new(),
            children: vec![],
        }
    }

    #[test]
    fn a_pattern_tiles_its_content() {
        let pattern = props(&[("id", "stripes".into()), ("width", 10.into()), ("height", 10.into()), ("patternUnits", "userSpaceOnUse".into())]);
        let bar = props(&[("width", 5.into()), ("height", 10.into()), ("fill", "#ff0000".into())]);
        let filled = props(&[("width", 40.into()), ("height", 10.into()), ("fill", "url(#stripes)".into())]);
        let xml = document(
            40,
            10,
            vec![
                node("defs", &HashMap::new(), vec![node("pattern", &pattern, vec![node("rect", &bar, vec![])])]),
                node("rect", &filled, vec![]),
            ],
        );
        assert!(xml.contains("<pattern id=\"stripes\" width=\"10\" height=\"10\" patternUnits=\"userSpaceOnUse\">"), "{xml}");
        let raster = Raster::of(&xml, (40.0, 10.0));
        for tile in 0..4 {
            let origin = tile as f32 * 10.0;
            assert_eq!(raster.at(origin + 2.0, 5.0), [255, 0, 0, 255], "tile {tile} bar");
            assert!(!raster.painted(origin + 7.0, 5.0), "tile {tile} gap");
        }
    }

    #[test]
    fn a_marker_is_drawn_at_the_end_of_a_path() {
        let marker = props(&[
            ("id", "dot".into()),
            ("markerWidth", 6.into()),
            ("markerHeight", 6.into()),
            ("refX", 3.into()),
            ("refY", 3.into()),
            ("markerUnits", "userSpaceOnUse".into()),
        ]);
        let circle = props(&[("cx", 3.into()), ("cy", 3.into()), ("r", 3.into()), ("fill", "#00ff00".into())]);
        let line = props(&[("d", "M2 10 L18 10".into()), ("stroke", "#0000ff".into()), ("strokeWidth", 1.into()), ("markerEnd", "url(#dot)".into())]);
        let xml = document(
            30,
            20,
            vec![
                node("defs", &HashMap::new(), vec![node("marker", &marker, vec![node("circle", &circle, vec![])])]),
                node("path", &line, vec![]),
            ],
        );
        assert!(xml.contains("marker-end=\"url(#dot)\""), "{xml}");
        let raster = Raster::of(&xml, (30.0, 20.0));
        assert_eq!(raster.at(18.0, 10.0), [0, 255, 0, 255], "the dot sits on the path end");
        assert_eq!(raster.at(10.0, 10.0)[2], 255, "the line itself");
        assert!(!raster.painted(25.0, 10.0), "nothing past the dot");
    }

    #[test]
    fn the_marker_shorthand_sets_the_three_markers_unless_they_are_set() {
        let path = props(&[("d", "M0 0L9 9".into()), ("marker", "url(#all)".into()), ("markerMid", "url(#mid)".into())]);
        let xml = serialize(&node("svg", &props(&[]), vec![node("path", &path, vec![])]));
        assert!(xml.contains("marker-mid=\"url(#mid)\""), "{xml}");
        assert!(xml.contains("marker-start=\"url(#all)\""), "{xml}");
        assert!(xml.contains("marker-end=\"url(#all)\""), "{xml}");
        assert_eq!(xml.matches("marker-mid").count(), 1, "{xml}");
        assert!(!xml.contains(" marker="), "{xml}");
    }

    #[test]
    fn filter_primitives_are_written_with_their_attributes() {
        let filter = props(&[("id", "f".into()), ("filterUnits", "userSpaceOnUse".into()), ("x", 0.into()), ("width", 30.into())]);
        let blur = props(&[("in", "SourceGraphic".into()), ("stdDeviation", 2.5.into()), ("result", "b".into())]);
        let matrix = props(&[("type", "saturate".into()), ("values", 0.into()), ("colorInterpolationFilters", "sRGB".into())]);
        let shadow = props(&[("dx", 1.into()), ("dy", 2.into()), ("floodColor", "light-dark(#ff0000, #0000ff)".into()), ("floodOpacity", 0.5.into())]);
        let light = props(&[("azimuth", 45.into()), ("elevation", 30.into())]);
        let xml = serialize_in(
            &node(
                "svg",
                &props(&[]),
                vec![node(
                    "filter",
                    &filter,
                    vec![
                        node("feGaussianBlur", &blur, vec![]),
                        node("feColorMatrix", &matrix, vec![]),
                        node("feDropShadow", &shadow, vec![]),
                        node("feDiffuseLighting", &props(&[("surfaceScale", 2.into())]), vec![node("feDistantLight", &light, vec![])]),
                    ],
                )],
            ),
            &context("black", "dark"),
        );
        assert!(xml.contains("<filter id=\"f\" x=\"0\" width=\"30\" filterUnits=\"userSpaceOnUse\">"), "{xml}");
        assert!(xml.contains("<feGaussianBlur in=\"SourceGraphic\" result=\"b\" stdDeviation=\"2.5\"/>"), "{xml}");
        assert!(xml.contains("<feColorMatrix color-interpolation-filters=\"sRGB\" type=\"saturate\" values=\"0\"/>"), "{xml}");
        assert!(xml.contains("flood-color=\"rgba(0, 0, 255, 1)\" flood-opacity=\"0.5\""), "{xml}");
        assert!(xml.contains("<feDiffuseLighting surfaceScale=\"2\"><feDistantLight azimuth=\"45\" elevation=\"30\"/></feDiffuseLighting>"), "{xml}");
    }

    fn filtered(filter: Node, on: &HashMap<String, Value>) -> Raster {
        Raster::of(
            &document(
                40,
                20,
                vec![
                    node("defs", &HashMap::new(), vec![filter]),
                    node("rect", on, vec![]),
                ],
            ),
            (40.0, 20.0),
        )
    }

    fn square() -> HashMap<String, Value> {
        props(&[
            ("x", 10.into()),
            ("y", 5.into()),
            ("width", 10.into()),
            ("height", 10.into()),
            ("fill", "#ff0000".into()),
            ("filter", "url(#f)".into()),
        ])
    }

    fn filter_props() -> HashMap<String, Value> {
        props(&[("id", "f".into()), ("filterUnits", "userSpaceOnUse".into()), ("x", 0.into()), ("y", 0.into()), ("width", 40.into()), ("height", 20.into())])
    }

    #[test]
    fn a_blur_filter_spreads_the_shape() {
        let filter = filter_props();
        let blur = props(&[("stdDeviation", 3.into())]);
        let raster = filtered(node("filter", &filter, vec![node("feGaussianBlur", &blur, vec![])]), &square());
        let outside = raster.at(8.0, 10.0);
        assert!(outside[3] > 0 && outside[3] < 255, "the blur reaches past the edge: {outside:?}");
        assert!(raster.at(15.0, 10.0)[3] > 150, "and the middle stays solid");
        assert!(raster.at(2.0, 10.0)[3] < 8, "far from the shape the blur has faded");
    }

    #[test]
    fn an_offset_filter_moves_the_shape() {
        let filter = filter_props();
        let offset = props(&[("dx", 15.into())]);
        let raster = filtered(node("filter", &filter, vec![node("feOffset", &offset, vec![])]), &square());
        assert!(!raster.painted(15.0, 10.0), "the shape left its place");
        assert_eq!(raster.at(30.0, 10.0), [255, 0, 0, 255], "and arrived 15 units right");
    }

    #[test]
    fn a_flood_filter_fills_the_region() {
        let filter = filter_props();
        let flood = props(&[("floodColor", "#00ff00".into()), ("floodOpacity", 1.into())]);
        let raster = filtered(node("filter", &filter, vec![node("feFlood", &flood, vec![])]), &square());
        assert_eq!(raster.at(2.0, 2.0), [0, 255, 0, 255], "the whole region, not the shape");
        assert_eq!(raster.at(38.0, 18.0), [0, 255, 0, 255]);
    }

    #[test]
    fn a_flood_colour_takes_current_color() {
        let filter = filter_props();
        let flood = props(&[("floodColor", "currentColor".into())]);
        let svg = props(&[("viewBox", "0 0 40 20".into())]);
        let target = square();
        let xml = serialize_in(&node(
            "svg",
            &svg,
            vec![
                node("defs", &HashMap::new(), vec![node("filter", &filter, vec![node("feFlood", &flood, vec![])])]),
                node("rect", &target, vec![]),
            ],
        ), &context("#ff00ff", "light"));
        assert_eq!(Raster::of(&xml, (40.0, 20.0)).at(2.0, 2.0), [255, 0, 255, 255], "{xml}");
    }

    #[test]
    fn text_paints_glyphs_from_string_children() {
        let text = props(&[("x", 2.into()), ("y", 38.into()), ("fontSize", 40.into()), ("fontFamily", "sans-serif".into()), ("fill", "#000000".into())]);
        let words = leaf("MM");
        let xml = document(60, 40, vec![node("text", &text, vec![words])]);
        assert!(xml.contains("<text x=\"2\" y=\"38\" fill=\"#000000\" font-family=\"sans-serif\" font-size=\"40\">MM</text>"), "{xml}");
        let raster = Raster::of(&xml, (60.0, 40.0));
        let mut ink = 0;
        for row in 0..40 {
            for column in 0..60 {
                if raster.painted(column as f32, row as f32) {
                    ink += 1;
                }
            }
        }
        assert!(ink > 100, "two capital letters leave ink, got {ink} pixels");
        assert!(!raster.painted(58.0, 2.0), "the corner stays empty");
    }

    #[test]
    fn tspan_and_text_path_nest_in_text_and_text_is_escaped() {
        let text = props(&[]);
        let span = props(&[("dx", 4.into()), ("fontWeight", "bold".into()), ("textDecoration", "underline".into())]);
        let path = props(&[("href", "#curve".into()), ("startOffset", "10%".into())]);
        let xml = serialize(&node(
            "svg",
            &props(&[]),
            vec![node(
                "text",
                &text,
                vec![
                    leaf("a < b & "),
                    node("tspan", &span, vec![leaf("bold")]),
                    node("textPath", &path, vec![leaf("curved")]),
                ],
            )],
        ));
        assert!(xml.contains("<text>a &lt; b &amp; <tspan dx=\"4\" font-weight=\"bold\" text-decoration=\"underline\">bold</tspan><textPath href=\"#curve\" startOffset=\"10%\">curved</textPath></text>"), "{xml}");
    }

    #[test]
    fn an_image_paints_its_decoded_pixels() {
        let href = format!("data:image/png;base64,{RED_BLUE_PNG}");
        let image = props(&[("href", href.as_str().into()), ("x", 5.into()), ("y", 5.into()), ("width", 20.into()), ("height", 10.into()), ("preserveAspectRatio", "none".into())]);
        let xml = document(30, 20, vec![node("image", &image, vec![])]);
        let raster = Raster::of(&xml, (30.0, 20.0));
        assert_eq!(raster.at(8.0, 10.0), [255, 0, 0, 255], "the left half of the picture is red");
        assert_eq!(raster.at(22.0, 10.0), [0, 0, 255, 255], "the right half is blue");
        assert!(!raster.painted(2.0, 10.0), "outside the image box");
    }

    /// 4x4: red on the left two columns, blue on the right two.
    const RED_BLUE_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAQAAAAECAIAAAAmkwkpAAAAE0lEQVR4nGP4z8AARGDiPwNxHACeow/xfxmViwAAAABJRU5ErkJggg==";

    #[test]
    fn an_image_href_is_a_data_url_or_a_file() {
        let resolved = |href: &str| {
            let mut written = vec![("href", href.to_string())];
            resolve_image_href("image", &mut written);
            written.pop().map(|(_, href)| href)
        };
        assert_eq!(resolved("data:image/png;base64,AAAA").as_deref(), Some("data:image/png;base64,AAAA"));
        assert_eq!(resolved("file:///tmp/a%20b.png").as_deref(), Some("/tmp/a b.png"));
        assert_eq!(resolved("FILE://localhost/tmp/a.png#x").as_deref(), Some("/tmp/a.png"));
        assert_eq!(resolved("/tmp/a.png").as_deref(), Some("/tmp/a.png"));
        assert_eq!(resolved("#other").as_deref(), Some("#other"));
        for dropped in ["blob:abc", "https://example.test/a.png", "HTTP://example.test/a.png", "file://relative.png"] {
            assert_eq!(resolved(dropped), None, "{dropped}");
        }
    }

    fn svg_tree(build: impl FnOnce(&mut RetainedTree)) -> RetainedTree {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "svg".to_string());
        build(&mut tree);
        tree
    }

    fn markup_of(tree: &RetainedTree) -> String {
        Animated::new().markup(tree)
    }

    /// An `<svg>` over a clock that the tests move by hand, with the CSS
    /// animations and the keyframes its shapes may name.
    struct Animated {
        trees: SvgTrees,
        animations: CssAnimations,
        keyframes: KeyframesRegistry,
        start: Instant,
        at: std::time::Duration,
    }

    impl Animated {
        fn new() -> Self {
            Self {
                trees: SvgTrees::default(),
                animations: CssAnimations::default(),
                keyframes: KeyframesRegistry::default(),
                start: Instant::now(),
                at: std::time::Duration::ZERO,
            }
        }

        fn at(&mut self, millis: u64) -> &mut Self {
            self.at = std::time::Duration::from_millis(millis);
            self
        }

        fn markup(&mut self, tree: &RetainedTree) -> String {
            self.markup_with(tree, Vec::new())
        }

        /// The markup with `host` written on the root, as the `<svg>`'s own animations do.
        fn markup_with(&mut self, tree: &RetainedTree, host: Vec<(&'static str, String)>) -> String {
            let colors = ColorContext::root(Scheme::Light);
            let font = ParentFont::initial();
            let motion = HashMap::new();
            self.trees
                .markup(
                    tree,
                    1,
                    Frame {
                        motion_states: &motion,
                        animations: &mut self.animations,
                        keyframes: &self.keyframes,
                        colors: &colors,
                        font: &font,
                        host,
                        now: self.start + self.at,
                    },
                )
                .map(|markup| markup.to_string())
                .unwrap_or_default()
        }
    }

    #[test]
    fn every_svg_tag_survives_the_tree() {
        for (index, tag) in svg_names::TAGS.iter().enumerate() {
            let tree = svg_tree(|tree| {
                tree.create_element(2 + index as u64, tag.to_string());
                tree.append_child(1, 2 + index as u64);
            });
            assert!(markup_of(&tree).contains(&format!("<{tag}")), "<{tag}> is dropped");
        }
    }

    #[test]
    fn a_text_tag_in_an_svg_is_an_svg_element_and_a_string_in_it_is_character_data() {
        let tree = svg_tree(|tree| {
            tree.create_element(2, "text".to_string());
            tree.set_custom_prop(2, "x".to_string(), 3.into());
            tree.append_child(1, 2);
            tree.create_element(3, "text".to_string());
            tree.set_text(3, "Hi".to_string());
            tree.append_child(2, 3);
        });
        assert!(is_svg_element(&tree, &tree.elements[&2]));
        assert!(!is_svg_element(&tree, &tree.elements[&3]), "a string is not an element");
        assert!(markup_of(&tree).contains("<text x=\"3\">Hi</text>"));
    }

    #[test]
    fn a_text_tag_outside_an_svg_is_not_an_svg_element() {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        tree.create_element(2, "text".to_string());
        tree.append_child(1, 2);
        assert!(!is_svg_element(&tree, &tree.elements[&2]));
        assert!(is_svg_child_type("pattern") && is_svg_child_type("feGaussianBlur"));
        assert!(!is_svg_child_type("text"), "text is an SVG element only by where it sits");
    }

    #[test]
    fn a_string_that_is_not_in_a_text_element_is_not_drawn() {
        let tree = svg_tree(|tree| {
            tree.create_element(2, "text".to_string());
            tree.set_text(2, "stray".to_string());
            tree.append_child(1, 2);
            tree.create_element(3, "g".to_string());
            tree.append_child(1, 3);
            tree.create_element(4, "text".to_string());
            tree.set_text(4, "in a group".to_string());
            tree.append_child(3, 4);
        });
        let markup = markup_of(&tree);
        assert!(!markup.contains("stray") && !markup.contains("in a group"), "{markup}");
    }

    #[test]
    fn the_rasteriser_strokes_only_the_drawn_part() {
        // Half of a 10 unit line: the rasteriser must show exactly the dash we
        // computed, which is what makes a draw work without `pathLength`.
        let svg = props(&[
            ("viewBox", "0 0 10 10".into()),
            ("width", 10.into()),
            ("height", 10.into()),
        ]);
        let line = props(&[
            ("x1", 0.into()),
            ("y1", 5.into()),
            ("x2", 10.into()),
            ("y2", 5.into()),
            ("stroke", "black".into()),
            ("strokeWidth", 2.into()),
        ]);
        let mut shape = node("line", &line, vec![]);
        shape.motion = Some(SvgMotion {
            attributes: vec![],
            draw: Some(PathDraw {
                length: 0.5,
                offset: 0.0,
                spacing: 1.0,
            }),
        });
        let xml = serialize(&node("svg", &svg, vec![shape]));

        let renderer = gpui::SvgRenderer::new(Arc::new(()));
        let image = renderer.render_single_frame(xml.as_bytes(), 1.0).unwrap();
        let size = image.size(0);
        let bytes = image.as_bytes(0).unwrap();
        let alpha = |x: f32, y: f32| {
            let column = (x / 10.0 * size.width.0 as f32) as usize;
            let row = (y / 10.0 * size.height.0 as f32) as usize;
            bytes[(row * size.width.0 as usize + column) * 4 + 3]
        };
        assert!(alpha(2.0, 5.0) > 200, "drawn half");
        assert_eq!(alpha(7.5, 5.0), 0, "undrawn half");
    }

    // ── CSS animations ───────────────────────────────────────────────

    fn animated_circle(tree: &mut RetainedTree, animation: &str) {
        tree.create_element(2, "circle".to_string());
        tree.set_custom_prop(2, "r".to_string(), 5.into());
        tree.set_style(
            2,
            Arc::new(serde_json::from_value(serde_json::json!({ "animation": animation })).unwrap()),
        );
        tree.append_child(1, 2);
    }

    #[test]
    fn a_shape_is_written_with_the_values_its_css_animation_gives_it() {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "svg".to_string());
        animated_circle(&mut tree, "grow 1s linear");
        let mut svg = Animated::new();
        svg.keyframes.define("grow", r#"{ "to": { "r": 15 } }"#).unwrap();

        assert!(svg.at(0).markup(&tree).contains(r#"<circle r="5"/>"#));
        assert!(svg.at(500).markup(&tree).contains(r#"<circle r="10"/>"#));
        assert!(svg.at(1500).markup(&tree).contains(r#"<circle r="5"/>"#), "no fill, so it ends where it began");
    }

    #[test]
    fn the_markup_is_written_again_when_an_animation_moves_it_and_not_before() {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "svg".to_string());
        animated_circle(&mut tree, "grow 1s linear forwards");
        let mut svg = Animated::new();
        svg.keyframes.define("grow", r#"{ "to": { "r": 15 } }"#).unwrap();

        svg.at(0).markup(&tree);
        let first = svg.trees.generation(1);
        svg.at(0).markup(&tree);
        assert_eq!(svg.trees.generation(1), first, "nothing moved");
        svg.at(500).markup(&tree);
        assert_ne!(svg.trees.generation(1), first);
        svg.at(2000).markup(&tree);
        let ended = svg.trees.generation(1);
        svg.at(3000).markup(&tree);
        assert_eq!(svg.trees.generation(1), ended, "it holds its end value");
    }

    #[test]
    fn a_css_animation_beats_the_shapes_motion_and_its_draw() {
        let line = props(&[("x2", 100.into()), ("pathLength", 10.into())]);
        let mut shape = node("line", &line, vec![]);
        shape.motion = Some(SvgMotion {
            attributes: vec![("stroke-width", "9".into())],
            draw: Some(PathDraw { length: 0.5, offset: 0.1, spacing: 1.0 }),
        });
        shape.css = vec![("stroke-width", "3".into()), ("stroke-dasharray", "7 8".into())];
        let svg = props(&[]);
        let xml = serialize(&node("svg", &svg, vec![shape]));
        assert!(xml.contains(r#"stroke-width="3""#), "{xml}");
        assert!(xml.contains(r#"stroke-dasharray="7 8""#), "{xml}");
        // The draw still sets what the animation does not name.
        assert!(xml.contains(r#"stroke-dashoffset="-10""#), "{xml}");
    }

    #[test]
    fn the_dom_path_length_scales_animated_dashes_too() {
        let line = props(&[("x2", 100.into()), ("pathLength", 10.into())]);
        let mut shape = node("line", &line, vec![]);
        shape.css = vec![("stroke-dasharray", "2 1".into()), ("stroke-dashoffset", "-3".into())];
        let svg = props(&[]);
        let xml = serialize(&node("svg", &svg, vec![shape]));
        assert!(xml.contains(r#"stroke-dasharray="20 10""#), "{xml}");
        assert!(xml.contains(r#"stroke-dashoffset="-30""#), "{xml}");
    }

    #[test]
    fn the_animations_of_the_svg_itself_are_written_on_its_root() {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "svg".to_string());
        tree.create_element(2, "circle".to_string());
        tree.append_child(1, 2);
        let mut svg = Animated::new();
        let plain = svg.markup(&tree);
        let first = svg.trees.generation(1);
        let animated = svg.markup_with(&tree, vec![("stroke-width", "4".into())]);
        assert!(animated.starts_with(r#"<svg xmlns="http://www.w3.org/2000/svg" stroke-width="4">"#), "{animated}");
        assert!(!plain.contains("stroke-width"), "{plain}");
        assert_ne!(svg.trees.generation(1), first, "a new root value is a new markup");
    }

    #[test]
    fn a_group_that_animates_passes_its_values_to_the_shapes_in_it() {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "svg".to_string());
        tree.create_element(2, "g".to_string());
        tree.set_style(
            2,
            Arc::new(serde_json::from_value(serde_json::json!({ "animation": "thick 1s linear" })).unwrap()),
        );
        tree.create_element(3, "path".to_string());
        tree.append_child(1, 2);
        tree.append_child(2, 3);
        let mut svg = Animated::new();
        svg.keyframes.define("thick", r#"{ "from": { "strokeWidth": 2 }, "to": { "strokeWidth": 6 } }"#).unwrap();
        svg.at(0).markup(&tree);
        assert!(svg.at(500).markup(&tree).contains(r#"<g stroke-width="4"><path/></g>"#));
    }
}
