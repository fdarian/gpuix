//! The `@keyframes` analogue: named keyframe sets that `animation-name` refers
//! to. A renderer holds the copy JS pushes through `defineKeyframes`.
//!
//! A definition is the frames object as JSON: keys are `from`, `to`, `N%` or a
//! comma list of them (`"0%, 50%"`), values are the animatable style
//! properties and the SVG properties CSS animates (`strokeDashoffset`, `fill`,
//! `r`...), plus `animationTimingFunction`, which eases from that keyframe
//! to the next, as it does in CSS. Blocks at the same offset merge, later keys
//! winning property by property. The endpoints a definition leaves out are the
//! element's underlying values, which only the runtime knows.
//!
//! A definition does not know what it will animate, so a name that is both a
//! style property and an SVG one (`opacity`, `width`, `height`, `fontSize`) is
//! kept for both, with each value that parses for it. The element picks.
//!
//! A definition is shared by every element that names it, so its values stay as
//! written: a colour means something only at an element (`currentColor`,
//! `light-dark()`), which resolves it when its animation plans.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::css::custom_properties::{self, CustomProperties};
use crate::css::transform::DEFAULT_ORIGIN_TEXT;

use super::css_animation::check_keyframes_name;
use super::css_transition::parse_list;
use super::curve::{parse_timing_function, Ease};
use super::property::{split_shorthand, AnimProp};
use super::svg_attr::SvgAttr;
use super::value::{AnimValue, Colors};
use super::PropKey;

const TIMING_FUNCTION: &str = "animationTimingFunction";

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Keyframe {
    /// 0 to 1.
    pub offset: f64,
    /// Longhands only, each at most once, as written. They were parsed once when
    /// the keyframes were defined, to reject a bad one where it is written.
    pub values: Vec<(PropKey, Value)>,
    /// Eases from this keyframe to the next. `None` uses the animation's.
    pub timing: Option<Ease>,
}

/// The keyframes of one name, ascending by offset with one entry per offset.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Keyframes {
    pub frames: Vec<Keyframe>,
}

/// The block keys in the order the definition wrote them, which decides who
/// wins when two blocks share an offset. A `serde_json::Map` would sort them.
struct Blocks(Vec<(String, Value)>);

impl<'de> Deserialize<'de> for Blocks {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BlocksVisitor;

        impl<'de> Visitor<'de> for BlocksVisitor {
            type Value = Blocks;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("an object of keyframe selector to style")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Blocks, A::Error> {
                let mut blocks = Vec::new();
                while let Some(entry) = map.next_entry::<String, Value>()? {
                    blocks.push(entry);
                }
                Ok(Blocks(blocks))
            }
        }

        deserializer.deserialize_map(BlocksVisitor)
    }
}

impl Keyframes {
    pub(super) fn parse(frames_json: &str) -> Result<Self, String> {
        let blocks: Blocks = serde_json::from_str(frames_json)
            .map_err(|error| format!("the frames must be a JSON object of keyframe selector to style: {error}"))?;
        let mut frames: Vec<Keyframe> = Vec::new();
        for (selector, block) in &blocks.0 {
            let block = parse_block(block).map_err(|error| format!("{selector}: {error}"))?;
            for offset in parse_selector(selector)? {
                match frames.iter_mut().find(|frame| frame.offset == offset) {
                    Some(frame) => frame.merge(&block),
                    None => frames.push(Keyframe {
                        offset,
                        values: block.values.clone(),
                        timing: block.timing.clone(),
                    }),
                }
            }
        }
        frames.sort_by(|a, b| a.offset.total_cmp(&b.offset));
        Ok(Self { frames })
    }

    /// The rule a definition has to pass before any renderer takes it: the
    /// name and the frames, with errors that say which keyframes they are for.
    fn parse_named(name: &str, frames_json: &str) -> Result<Self, String> {
        check_keyframes_name(name)?;
        Self::parse(frames_json).map_err(|error| format!("keyframes {name}: {error}"))
    }

    /// Whether the value of some property depends on the custom properties of the element.
    pub(super) fn reads_vars(&self) -> bool {
        self.frames.iter().any(Keyframe::reads_vars)
    }

    /// Every property some keyframe sets, in no particular order.
    pub(super) fn properties(&self) -> Vec<PropKey> {
        let mut props: Vec<PropKey> = self
            .frames
            .iter()
            .flat_map(|frame| frame.values.iter().map(|(key, _)| *key))
            .collect();
        props.sort();
        props.dedup();
        props
    }
}

impl Keyframe {
    /// What `key` is at this keyframe for an element with `colors` and the custom properties
    /// `custom`, which are none when no keyframe reads one. A value that does not parse for the
    /// element is no value of that keyframe, as CSS drops a declaration it cannot read. A `var()`
    /// that does not resolve is invalid at computed-value time instead, which a keyframe makes the
    /// initial value of the property (not the element's own).
    pub(super) fn value(&self, key: PropKey, colors: &Colors, custom: Option<&CustomProperties>) -> Option<AnimValue> {
        let (_, written) = self.values.iter().find(|(k, _)| *k == key)?;
        if !reads_vars(key, written) {
            return key.parse_css(written, colors).ok();
        }
        let text = match custom.and_then(|custom| custom_properties::substitute(written.as_str()?, custom)) {
            Some(substituted) => substituted.trim().to_owned(),
            None => match key {
                PropKey::Style(AnimProp::TransformOrigin | AnimProp::PerspectiveOrigin) => DEFAULT_ORIGIN_TEXT.to_owned(),
                _ => "none".to_owned(),
            },
        };
        key.parse_css(&Value::String(text), colors).ok()
    }

    /// Whether some value of this keyframe holds a `var()`.
    fn reads_vars(&self) -> bool {
        self.values.iter().any(|(key, written)| reads_vars(*key, written))
    }

    fn merge(&mut self, later: &Keyframe) {
        for (key, value) in &later.values {
            self.set(*key, value.clone());
        }
        if later.timing.is_some() {
            self.timing = later.timing.clone();
        }
    }

    fn set(&mut self, key: PropKey, value: Value) {
        match self.values.iter_mut().find(|(existing, _)| *existing == key) {
            Some(slot) => slot.1 = value,
            None => self.values.push((key, value)),
        }
    }
}

/// Whether `written` is a `var()` of a transform property, the only ones that substitute it: it
/// is checked against the element it is at, and the keyframes only hold the text.
fn reads_vars(key: PropKey, written: &Value) -> bool {
    matches!(key, PropKey::Style(prop) if prop.reads_vars())
        && written.as_str().is_some_and(custom_properties::has_var)
}

/// `from`, `to`, `50%` or a comma list of them, as offsets in 0 to 1.
fn parse_selector(selector: &str) -> Result<Vec<f64>, String> {
    selector
        .split(',')
        .map(|part| {
            let part = part.trim();
            match part.to_ascii_lowercase().as_str() {
                "from" => return Ok(0.0),
                "to" => return Ok(1.0),
                _ => {}
            }
            let percent = part
                .strip_suffix('%')
                .and_then(|number| number.trim_end().parse::<f64>().ok())
                .filter(|percent| percent.is_finite())
                .ok_or_else(|| {
                    format!("`{part}` is not a keyframe selector (from, to or a percentage)")
                })?;
            if !(0.0..=100.0).contains(&percent) {
                return Err(format!("`{part}` is outside 0% to 100%"));
            }
            Ok(percent / 100.0)
        })
        .collect()
}

fn parse_block(block: &Value) -> Result<Keyframe, String> {
    let block = block
        .as_object()
        .ok_or_else(|| "a keyframe must be an object of style properties".to_string())?;
    let mut keyframe = Keyframe { offset: 0.0, values: Vec::new(), timing: None };
    for (name, value) in block {
        if name == TIMING_FUNCTION {
            let text = value
                .as_str()
                .ok_or_else(|| format!("{TIMING_FUNCTION} must be a string"))?;
            // A list in CSS, of which a keyframe uses the first.
            let timings = parse_list(text, parse_timing_function)
                .map_err(|error| format!("{TIMING_FUNCTION}: {error}"))?;
            keyframe.timing = timings.into_iter().next();
            continue;
        }
        let split = split_shorthand(name, value).transpose()?;
        let candidates: Vec<(PropKey, Value)> = match &split {
            Some(split) => split
                .channels
                .iter()
                .map(|(prop, value)| (PropKey::Style(*prop), value.clone()))
                .collect(),
            None => {
                let style = AnimProp::from_css_animated_name(name).map(PropKey::Style);
                let svg = SvgAttr::from_name(name)
                    .filter(|attr| attr.is_css_property())
                    .map(PropKey::Svg);
                let found: Vec<_> = [style, svg]
                    .into_iter()
                    .flatten()
                    .map(|key| (key, value.clone()))
                    .collect();
                if found.is_empty() {
                    return Err(format!("`{name}` cannot be animated in keyframes"));
                }
                found
            }
        };
        let mut errors = Vec::new();
        let mut accepted = Vec::new();
        for (key, value) in candidates {
            if reads_vars(key, &value) {
                accepted.push((key, value));
                continue;
            }
            match key.parse_css(&value, &Colors::root()) {
                Ok(_) => accepted.push((key, value)),
                Err(error) => errors.push(match (&split, key) {
                    (Some(split), PropKey::Style(prop)) => split.error(prop, error),
                    _ => error,
                }),
            }
        }
        // A shorthand needs every longhand to parse. A name both tables know
        // needs one of them to, and the first to say why is the one it reports.
        if let Some(error) = errors.into_iter().find(|_| split.is_some() || accepted.is_empty()) {
            return Err(format!("{name}: {error}"));
        }
        // A shorthand sets longhands in order, so a later key wins.
        for (key, value) in accepted {
            keyframe.set(key, value);
        }
    }
    Ok(keyframe)
}

/// Checks a definition without a renderer, so a bad one fails where it is
/// written and not when the first renderer receives it.
pub(crate) fn validate_keyframes(name: &str, frames_json: &str) -> Result<(), String> {
    Keyframes::parse_named(name, frames_json).map(drop)
}

/// The keyframe sets a renderer knows by name.
#[derive(Default)]
pub(crate) struct KeyframesRegistry {
    definitions: HashMap<String, Arc<Keyframes>>,
    /// Counts the definitions, for whoever kept an answer that depends on them.
    epoch: u64,
}

impl KeyframesRegistry {
    /// Defines `name`, replacing an earlier definition. An animation already
    /// running takes the new frames from its next frame on.
    pub(crate) fn define(&mut self, name: &str, frames_json: &str) -> Result<(), String> {
        let keyframes = Keyframes::parse_named(name, frames_json)?;
        self.definitions.insert(name.to_string(), Arc::new(keyframes));
        self.epoch += 1;
        Ok(())
    }

    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }

    pub(super) fn get(&self, name: &str) -> Option<&Arc<Keyframes>> {
        self.definitions.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Keyframes {
        Keyframes::parse(json).unwrap()
    }

    fn error(json: &str) -> String {
        Keyframes::parse(json).unwrap_err()
    }

    fn number(frame: &Keyframe, prop: AnimProp) -> Option<f64> {
        match frame.value(PropKey::Style(prop), &Colors::root(), None)? {
            AnimValue::Number(number) => Some(number),
            _ => None,
        }
    }

    #[test]
    fn validation_applies_the_name_rule_and_prefixes_the_frames_error() {
        assert_eq!(validate_keyframes("fade", r#"{ "to": { "opacity": 1 } }"#), Ok(()));
        assert!(validate_keyframes("none", r#"{ "to": { "opacity": 1 } }"#)
            .unwrap_err()
            .contains("animation name"));
        let message = validate_keyframes("fade", r#"{ "to": { "opacity": 5 } }"#).unwrap_err();
        assert!(message.starts_with("keyframes fade: to: opacity"), "{message}");
    }

    #[test]
    fn from_to_and_percentages_become_sorted_offsets() {
        let keyframes = parse(r#"{ "to": { "opacity": 1 }, "50%": { "opacity": 0.5 }, "from": { "opacity": 0 } }"#);
        let offsets: Vec<f64> = keyframes.frames.iter().map(|frame| frame.offset).collect();
        assert_eq!(offsets, [0.0, 0.5, 1.0]);
        assert_eq!(number(&keyframes.frames[1], AnimProp::Opacity), Some(0.5));
    }

    #[test]
    fn a_comma_list_applies_the_block_at_each_offset() {
        let keyframes = parse(r#"{ "0%, 50%, 100%": { "opacity": 1 }, "25%": { "opacity": 0 } }"#);
        let offsets: Vec<f64> = keyframes.frames.iter().map(|frame| frame.offset).collect();
        assert_eq!(offsets, [0.0, 0.25, 0.5, 1.0]);
    }

    #[test]
    fn blocks_at_one_offset_merge_with_the_later_key_winning() {
        let keyframes = parse(
            r#"{
                "from": { "opacity": 0, "width": 10, "animationTimingFunction": "linear" },
                "0%": { "opacity": 0.5, "height": 4 }
            }"#,
        );
        assert_eq!(keyframes.frames.len(), 1);
        let frame = &keyframes.frames[0];
        assert_eq!(number(frame, AnimProp::Opacity), Some(0.5));
        assert_eq!(number(frame, AnimProp::Width), Some(10.0));
        assert_eq!(number(frame, AnimProp::Height), Some(4.0));
        assert_eq!(frame.timing, Some(Ease::Name("linear".to_string())));
    }

    #[test]
    fn the_writing_order_decides_a_tie_not_the_key_text() {
        // Alphabetically "0%" sorts before "from"; written order must win.
        let keyframes = parse(r#"{ "from": { "opacity": 0.25 }, "0%": { "opacity": 0.75 } }"#);
        assert_eq!(number(&keyframes.frames[0], AnimProp::Opacity), Some(0.75));
        let keyframes = parse(r#"{ "0%": { "opacity": 0.75 }, "from": { "opacity": 0.25 } }"#);
        assert_eq!(number(&keyframes.frames[0], AnimProp::Opacity), Some(0.25));
    }

    #[test]
    fn shorthands_expand_and_a_later_longhand_wins() {
        let keyframes = parse(r#"{ "to": { "padding": 8, "paddingTop": 2 } }"#);
        let frame = &keyframes.frames[0];
        assert_eq!(number(frame, AnimProp::PaddingTop), Some(2.0));
        assert_eq!(number(frame, AnimProp::PaddingLeft), Some(8.0));
        assert_eq!(keyframes.properties().len(), 4);
    }

    #[test]
    fn a_multi_value_shorthand_gives_each_longhand_its_own_value() {
        let keyframes = parse(
            r#"{ "to": { "padding": "1px 2px", "gap": "3px 4px", "borderRadius": "5px / 6px", "inset": "7px 8px" } }"#,
        );
        let frame = &keyframes.frames[0];
        assert_eq!(number(frame, AnimProp::PaddingTop), Some(1.0));
        assert_eq!(number(frame, AnimProp::PaddingRight), Some(2.0));
        assert_eq!(number(frame, AnimProp::RowGap), Some(3.0));
        assert_eq!(number(frame, AnimProp::ColumnGap), Some(4.0));
        assert_eq!(number(frame, AnimProp::Top), Some(7.0));
        assert_eq!(number(frame, AnimProp::Left), Some(8.0));
        assert_eq!(
            frame.value(PropKey::Style(AnimProp::BorderTopLeftRadius), &Colors::root(), None),
            Some(AnimValue::Ellipse { x: (5.0, 0.0), y: (6.0, 0.0) })
        );
    }

    fn svg(frame: &Keyframe, attr: SvgAttr) -> Option<AnimValue> {
        frame.value(PropKey::Svg(attr), &Colors::root(), None)
    }

    #[test]
    fn svg_properties_are_keyframeable_and_keep_their_own_key() {
        let keyframes = parse(
            r##"{
                "from": { "strokeDashoffset": 100, "strokeDasharray": "4 2", "fill": "#ff0000", "stroke": "none", "r": 1 },
                "to": { "strokeDashoffset": 0, "strokeWidth": 3, "fillOpacity": 0.5, "stopColor": "red", "stopOpacity": 0.2,
                        "strokeMiterlimit": 2, "d": "path('M0 0 L1 1')", "cx": 4, "x": 1, "rx": 2 }
            }"##,
        );
        assert_eq!(svg(&keyframes.frames[0], SvgAttr::StrokeDashoffset), Some(AnimValue::Number(100.0)));
        assert_eq!(svg(&keyframes.frames[0], SvgAttr::StrokeDasharray), Some(AnimValue::Numbers(vec![4.0, 2.0])));
        assert!(matches!(svg(&keyframes.frames[0], SvgAttr::Fill), Some(AnimValue::Color(_))));
        assert_eq!(svg(&keyframes.frames[1], SvgAttr::StrokeWidth), Some(AnimValue::Number(3.0)));
        assert_eq!(svg(&keyframes.frames[1], SvgAttr::StrokeMiterlimit), Some(AnimValue::Number(2.0)));
        assert!(matches!(svg(&keyframes.frames[1], SvgAttr::D), Some(AnimValue::Path(_))));
        assert!(keyframes.properties().iter().all(|key| matches!(key, PropKey::Svg(_))));
    }

    #[test]
    fn a_name_both_tables_know_is_kept_for_each_that_reads_the_value() {
        let keyframes = parse(r#"{ "from": { "opacity": 0.5, "width": 10, "height": "50%", "fontSize": 12 } }"#);
        let frame = &keyframes.frames[0];
        assert_eq!(number(frame, AnimProp::Opacity), Some(0.5));
        assert_eq!(svg(frame, SvgAttr::Opacity), Some(AnimValue::Number(0.5)));
        assert_eq!(number(frame, AnimProp::Width), Some(10.0));
        assert_eq!(svg(frame, SvgAttr::Width), Some(AnimValue::Number(10.0)));
        // A percentage is a style length and no SVG number.
        assert!(matches!(frame.value(PropKey::Style(AnimProp::Height), &Colors::root(), None), Some(AnimValue::Percent(_))));
        assert_eq!(svg(frame, SvgAttr::Height), None);
        assert_eq!(number(frame, AnimProp::FontSize), Some(12.0));
        assert_eq!(svg(frame, SvgAttr::FontSize), Some(AnimValue::Number(12.0)));
    }

    #[test]
    fn a_later_svg_key_wins_like_any_other() {
        let keyframes = parse(r#"{ "from": { "r": 1 }, "0%": { "r": 2 } }"#);
        assert_eq!(svg(&keyframes.frames[0], SvgAttr::R), Some(AnimValue::Number(2.0)));
    }

    #[test]
    fn attributes_that_are_not_css_properties_are_rejected() {
        for name in ["pathLength", "pathOffset", "pathSpacing", "x1", "y1", "x2", "y2", "stdDeviation", "dx", "dy", "refX", "refY"] {
            let message = error(&format!(r#"{{ "to": {{ "{name}": 1 }} }}"#));
            assert!(message.contains(&format!("`{name}` cannot be animated")), "{message}");
        }
    }

    #[test]
    fn a_bad_svg_value_says_which_property() {
        assert!(error(r#"{ "to": { "strokeWidth": -1 } }"#).starts_with("to: strokeWidth:"));
        assert!(error(r#"{ "to": { "fill": "nope" } }"#).starts_with("to: fill:"));
        assert!(error(r#"{ "to": { "strokeDashoffset": "far" } }"#).starts_with("to: strokeDashoffset:"));
        assert!(error(r#"{ "to": { "strokeDasharray": [-1] } }"#).starts_with("to: strokeDasharray:"));
        assert!(error(r#"{ "to": { "strokeMiterlimit": 0.5 } }"#).starts_with("to: strokeMiterlimit:"));
        assert!(error(r#"{ "to": { "d": "L1 1" } }"#).starts_with("to: d:"));
        // Both tables refuse it: the message is the style one.
        assert!(error(r#"{ "to": { "opacity": 2 } }"#).starts_with("to: opacity:"));
    }

    #[test]
    fn a_bad_multi_value_shorthand_fails_where_it_is_written() {
        assert!(error(r#"{ "to": { "padding": "1px 2px 3px 4px 5px" } }"#).contains("padding"));
        assert!(error(r#"{ "to": { "padding": "1px -2px" } }"#).contains("paddingRight"));
        assert!(error(r#"{ "to": { "padding": "-2px" } }"#).contains("padding must"));
    }

    #[test]
    fn colours_shadows_and_percentages_parse_like_motion() {
        let keyframes = parse(
            r##"{
                "from": { "backgroundColor": "#ff0000", "width": "50%" },
                "to": { "boxShadow": { "offsetX": 0, "offsetY": 2, "blurRadius": 4, "spreadRadius": 0, "color": "#000000" } }
            }"##,
        );
        let colors = Colors::root();
        let value = |frame: usize, prop| keyframes.frames[frame].value(PropKey::Style(prop), &colors, None);
        assert!(matches!(value(0, AnimProp::BackgroundColor), Some(AnimValue::Color(_))));
        assert!(matches!(value(0, AnimProp::Width), Some(AnimValue::Percent(_))));
        assert!(matches!(value(1, AnimProp::BoxShadow), Some(AnimValue::Shadows(_))));
    }

    #[test]
    fn a_keyframe_timing_function_takes_the_first_of_a_list() {
        let keyframes = parse(
            r#"{ "from": { "animationTimingFunction": "cubic-bezier(0.4, 0, 0.2, 1), linear", "opacity": 0 } }"#,
        );
        assert_eq!(keyframes.frames[0].timing, Some(Ease::CubicBezier([0.4, 0.0, 0.2, 1.0])));
    }

    #[test]
    fn an_empty_definition_has_no_frames() {
        assert!(parse("{}").frames.is_empty());
        assert!(parse(r#"{ "from": {} }"#).frames[0].values.is_empty());
    }

    #[test]
    fn invalid_definitions_say_where() {
        assert!(error("[]").contains("JSON object"));
        assert!(error("nope").contains("JSON object"));
        assert!(error(r#"{ "middle": {} }"#).contains("not a keyframe selector"));
        assert!(error(r#"{ "101%": {} }"#).contains("outside 0% to 100%"));
        assert!(error(r#"{ "-1%": {} }"#).contains("outside 0% to 100%"));
        assert!(error(r#"{ "50": {} }"#).contains("not a keyframe selector"));
        assert!(error(r#"{ "from": 1 }"#).starts_with("from: a keyframe must be an object"));
        assert!(error(r#"{ "to": { "display": "none" } }"#).contains("`display` cannot be animated"));
        assert!(error(r#"{ "to": { "animationDuration": "1s" } }"#).contains("cannot be animated"));
        assert!(error(r#"{ "to": { "opacity": 2 } }"#).starts_with("to: opacity:"));
        assert!(error(r#"{ "to": { "opacity": [0, 1] } }"#).starts_with("to: opacity:"));
        assert!(error(r#"{ "to": { "color": "nope" } }"#).starts_with("to: color:"));
        assert!(error(r#"{ "to": { "animationTimingFunction": "wobble" } }"#)
            .contains("animationTimingFunction"));
        assert!(error(r#"{ "to": { "animationTimingFunction": 3 } }"#).contains("must be a string"));
    }

    #[test]
    fn the_registry_defines_replaces_and_looks_up_by_name() {
        let mut registry = KeyframesRegistry::default();
        assert!(registry.get("fade").is_none());
        registry.define("fade", r#"{ "to": { "opacity": 0 } }"#).unwrap();
        let first = registry.get("fade").unwrap().clone();
        assert_eq!(first.frames.len(), 1);

        registry.define("fade", r#"{ "from": { "opacity": 1 }, "to": { "opacity": 0 } }"#).unwrap();
        assert_eq!(registry.get("fade").unwrap().frames.len(), 2);
        // A holder of the old Arc keeps it: replacing does not edit in place.
        assert_eq!(first.frames.len(), 1);
        assert!(registry.get("Fade").is_none());
    }

    #[test]
    fn a_rejected_definition_leaves_the_registry_unchanged() {
        let mut registry = KeyframesRegistry::default();
        registry.define("fade", r#"{ "to": { "opacity": 0 } }"#).unwrap();
        let error = registry.define("fade", r#"{ "to": { "opacity": 5 } }"#).unwrap_err();
        assert!(error.starts_with("keyframes fade:"), "{error}");
        assert_eq!(number(&registry.get("fade").unwrap().frames[0], AnimProp::Opacity), Some(0.0));

        assert!(registry.define("none", "{}").is_err());
        assert!(registry.define("2d", "{}").is_err());
        assert!(registry.define("", "{}").is_err());
    }
    #[test]
    fn a_transform_value_with_a_var_is_kept_and_read_against_the_custom_properties_of_the_element() {
        let keyframes = parse(r#"{ "from": { "transform": "translateX(var(--p))" }, "to": { "transform": "translateX(calc(2 * var(--p)))" } }"#);
        assert!(keyframes.reads_vars());
        let custom = CustomProperties::declared([("--p".to_owned(), "10px".to_owned())]).unwrap();
        let custom = CustomProperties::default().descend(&custom);
        let list = |frame: usize, custom| match keyframes.frames[frame].value(PropKey::Style(AnimProp::Transform), &Colors::root(), custom) {
            Some(AnimValue::Transform(functions)) => functions.len(),
            other => panic!("{other:?}"),
        };
        assert_eq!((list(0, Some(&custom)), list(1, Some(&custom))), (1, 1));
        assert_eq!(list(0, None), 0, "unresolved, it is none");
        assert!(!parse(r#"{ "to": { "transform": "none" } }"#).reads_vars());
        assert!(error(r#"{ "to": { "width": "var(--w)" } }"#).contains("width"), "other properties still do not read it");
    }
}
