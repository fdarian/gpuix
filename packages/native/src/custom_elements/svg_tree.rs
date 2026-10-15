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
//! `use`. resvg resolves those references itself, so they are written as they come.
//!
//! resvg ignores the `pathLength` attribute, so every dash that depends on it
//! is turned into absolute user units against the shape's measured length.
//!
//! resvg knows only SVG's own colours, so a `fill` or `stroke` that needs the
//! element's context (`light-dark()`, `color-mix()`, `color()`) is resolved here,
//! with the `<svg>`'s, and written as `rgba()`.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use serde_json::Value;
use web_time::Instant;

use super::svg_path::{
    circle_length, ellipse_length, format_number, line_length, polyline_length, rect_length,
    PathData,
};
use crate::color::ColorContext;
use crate::motion::{MotionState, SvgMotion};
use crate::retained_tree::RetainedTree;
use crate::style::{DimensionValue, StyleDesc};

const SHAPE_TAGS: [&str; 16] = [
    "path",
    "circle",
    "ellipse",
    "rect",
    "line",
    "polyline",
    "polygon",
    "g",
    "defs",
    "linearGradient",
    "radialGradient",
    "stop",
    "clipPath",
    "mask",
    "use",
    "symbol",
];

/// Whether `tag` is an element only valid inside an `<svg>`.
pub(crate) fn is_svg_child_type(tag: &str) -> bool {
    SHAPE_TAGS.contains(&tag)
}

/// Prop name and the attribute it becomes. SVG's own camelCase attributes
/// (`viewBox`, `pathLength`) keep their case.
const ATTRIBUTES: &[(&str, &str)] = &[
    ("id", "id"),
    ("href", "href"),
    ("viewBox", "viewBox"),
    ("preserveAspectRatio", "preserveAspectRatio"),
    ("d", "d"),
    ("points", "points"),
    ("cx", "cx"),
    ("cy", "cy"),
    ("r", "r"),
    ("rx", "rx"),
    ("ry", "ry"),
    ("x", "x"),
    ("y", "y"),
    ("width", "width"),
    ("height", "height"),
    ("x1", "x1"),
    ("y1", "y1"),
    ("x2", "x2"),
    ("y2", "y2"),
    ("pathLength", "pathLength"),
    ("fx", "fx"),
    ("fy", "fy"),
    ("offset", "offset"),
    ("stopColor", "stop-color"),
    ("stopOpacity", "stop-opacity"),
    ("gradientUnits", "gradientUnits"),
    ("gradientTransform", "gradientTransform"),
    ("spreadMethod", "spreadMethod"),
    ("clipPathUnits", "clipPathUnits"),
    ("maskUnits", "maskUnits"),
    ("maskContentUnits", "maskContentUnits"),
    ("clipPath", "clip-path"),
    ("mask", "mask"),
    ("fill", "fill"),
    ("fillOpacity", "fill-opacity"),
    ("fillRule", "fill-rule"),
    ("clipRule", "clip-rule"),
    ("stroke", "stroke"),
    ("strokeWidth", "stroke-width"),
    ("strokeOpacity", "stroke-opacity"),
    ("strokeLinecap", "stroke-linecap"),
    ("strokeLinejoin", "stroke-linejoin"),
    ("strokeMiterlimit", "stroke-miterlimit"),
    ("strokeDasharray", "stroke-dasharray"),
    ("strokeDashoffset", "stroke-dashoffset"),
    ("opacity", "opacity"),
    ("transform", "transform"),
    ("display", "display"),
    ("visibility", "visibility"),
    ("shapeRendering", "shape-rendering"),
];

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
    motion: Option<SvgMotion>,
    children: Vec<Node<'a>>,
}

/// The serialised markup of every `<svg>` with children, reused until a
/// descendant changes or animates.
#[derive(Default)]
pub(crate) struct SvgTrees {
    cache: HashMap<u64, Cached>,
}

struct Cached {
    revision: u64,
    animation: u64,
    /// What the colours in `markup` resolved in.
    colors: ColorContext,
    markup: Arc<str>,
}

impl SvgTrees {
    /// The XML for `svg_id`, or `None` when it has no shape children and its
    /// `src` / `source` should be used as before.
    pub(crate) fn markup(
        &mut self,
        tree: &RetainedTree,
        svg_id: u64,
        motion_states: &HashMap<u64, MotionState>,
        colors: &ColorContext,
        now: Instant,
    ) -> Option<Arc<str>> {
        let element = tree.elements.get(&svg_id)?;
        let children = nodes(tree, &element.children, motion_states, now);
        if children.is_empty() {
            self.cache.remove(&svg_id);
            return None;
        }
        let animation = animation_key(&children);
        if let Some(cached) = self.cache.get(&svg_id) {
            if cached.revision == element.subtree_revision
                && cached.animation == animation
                && cached.colors == *colors
            {
                return Some(cached.markup.clone());
            }
        }
        let root = Node {
            tag: "svg",
            props: element.custom_props(),
            motion: None,
            children,
        };
        let markup: Arc<str> = serialize(&root, colors).into();
        self.cache.insert(
            svg_id,
            Cached {
                revision: element.subtree_revision,
                animation,
                colors: *colors,
                markup: markup.clone(),
            },
        );
        Some(markup)
    }

    pub(crate) fn retain(&mut self, mut is_live: impl FnMut(u64) -> bool) {
        self.cache.retain(|id, _| is_live(*id));
    }
}

fn nodes<'a>(
    tree: &'a RetainedTree,
    ids: &[u64],
    motion_states: &HashMap<u64, MotionState>,
    now: Instant,
) -> Vec<Node<'a>> {
    ids.iter()
        .filter_map(|id| tree.elements.get(id))
        .filter_map(|element| {
            if !is_svg_child_type(&element.element_type) {
                log::warn!(
                    "<{}> inside <svg> is not an SVG element and is not drawn",
                    element.element_type
                );
                return None;
            }
            Some(Node {
                tag: &element.element_type,
                props: element.custom_props(),
                motion: motion_states
                    .get(&element.id)
                    .and_then(|state| state.visible_svg(now)),
                children: nodes(tree, &element.children, motion_states, now),
            })
        })
        .collect()
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
            let text = match *attribute {
                "fill" | "stroke" | "stop-color" => resolve_paint(text, colors),
                _ => text,
            };
            written.push((attribute, text));
        }
    }
    for key in node.props.keys() {
        let known = ATTRIBUTES.iter().any(|(prop, _)| prop == key);
        let host = HOST_PROPS.contains(&key.as_str()) || key == "testId" || key == "autoFocus";
        if !known && !host {
            log::warn!("<{}> has no SVG attribute for the prop {key:?}", node.tag);
        }
    }
    if let Some(motion) = &node.motion {
        for (name, text) in &motion.attributes {
            set(&mut written, name, text.clone());
        }
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
        set(
            &mut written,
            "stroke-dasharray",
            format!(
                "{} {}",
                format_number(draw.length * length),
                format_number(draw.spacing * length)
            ),
        );
        set(
            &mut written,
            "stroke-dashoffset",
            format_number(-draw.offset * length),
        );
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
            motion: None,
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
}
