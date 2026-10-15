//! The `@keyframes` analogue: named keyframe sets that `animation-name` refers
//! to. A renderer holds the copy JS pushes through `defineKeyframes`.
//!
//! A definition is the frames object as JSON: keys are `from`, `to`, `N%` or a
//! comma list of them (`"0%, 50%"`), values are the animatable style
//! properties plus `animationTimingFunction`, which eases from that keyframe
//! to the next, as it does in CSS. Blocks at the same offset merge, later keys
//! winning property by property. The endpoints a definition leaves out are the
//! element's underlying values, which only the runtime knows.

// Nothing reads a definition until the runtime lands; the tests do.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use super::css_animation::check_keyframes_name;
use super::css_transition::parse_list;
use super::curve::{parse_timing_function, Ease};
use super::property::{expand_shorthand, AnimProp};
use super::value::AnimValue;

const TIMING_FUNCTION: &str = "animationTimingFunction";

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Keyframe {
    /// 0 to 1.
    pub offset: f64,
    /// Longhands only, each at most once.
    pub values: Vec<(AnimProp, AnimValue)>,
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

    /// Every property some keyframe sets, in no particular order.
    pub(super) fn properties(&self) -> Vec<AnimProp> {
        let mut props: Vec<AnimProp> = self
            .frames
            .iter()
            .flat_map(|frame| frame.values.iter().map(|(prop, _)| *prop))
            .collect();
        props.sort();
        props.dedup();
        props
    }
}

impl Keyframe {
    fn merge(&mut self, later: &Keyframe) {
        for (prop, value) in &later.values {
            match self.values.iter_mut().find(|(existing, _)| existing == prop) {
                Some(slot) => slot.1 = value.clone(),
                None => self.values.push((*prop, value.clone())),
            }
        }
        if later.timing.is_some() {
            self.timing = later.timing.clone();
        }
    }
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
        let props: Vec<AnimProp> = match (AnimProp::from_name(name), expand_shorthand(name)) {
            (Some(prop), _) => vec![prop],
            (None, Some(props)) => props.to_vec(),
            (None, None) => {
                return Err(format!("`{name}` cannot be animated in keyframes"));
            }
        };
        for prop in props {
            let parsed = prop.parse(value).map_err(|error| format!("{name}: {error}"))?;
            // A shorthand sets longhands in order, so a later key wins.
            match keyframe.values.iter_mut().find(|(existing, _)| *existing == prop) {
                Some(slot) => slot.1 = parsed,
                None => keyframe.values.push((prop, parsed)),
            }
        }
    }
    Ok(keyframe)
}

/// The keyframe sets a renderer knows by name.
#[derive(Default)]
pub(crate) struct KeyframesRegistry {
    definitions: HashMap<String, Arc<Keyframes>>,
}

impl KeyframesRegistry {
    /// Defines `name`, replacing an earlier definition. An animation already
    /// running keeps the frames it started with.
    pub(crate) fn define(&mut self, name: &str, frames_json: &str) -> Result<(), String> {
        check_keyframes_name(name)?;
        let keyframes =
            Keyframes::parse(frames_json).map_err(|error| format!("keyframes {name}: {error}"))?;
        self.definitions.insert(name.to_string(), Arc::new(keyframes));
        Ok(())
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
        frame.values.iter().find_map(|(p, v)| match (p, v) {
            (p, AnimValue::Number(n)) if *p == prop => Some(*n),
            _ => None,
        })
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
    fn colours_shadows_and_percentages_parse_like_motion() {
        let keyframes = parse(
            r##"{
                "from": { "backgroundColor": "#ff0000", "width": "50%" },
                "to": { "boxShadow": { "offsetX": 0, "offsetY": 2, "blurRadius": 4, "spreadRadius": 0, "color": "#000000" } }
            }"##,
        );
        assert!(matches!(keyframes.frames[0].values[0].1, AnimValue::Color(_)));
        assert!(matches!(keyframes.frames[0].values[1].1, AnimValue::Percent(_)));
        assert!(matches!(keyframes.frames[1].values[0].1, AnimValue::Shadows(_)));
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
        // A holder of the old definition keeps what it started with.
        assert_eq!(first.frames.len(), 1);
        assert!(registry.get("Fade").is_none());
    }

    #[test]
    fn a_rejected_definition_leaves_the_registry_unchanged() {
        let mut registry = KeyframesRegistry::default();
        registry.define("fade", r#"{ "to": { "opacity": 0 } }"#).unwrap();
        let error = registry.define("fade", r#"{ "to": { "opacity": 5 } }"#).unwrap_err();
        assert!(error.starts_with("keyframes fade:"), "{error}");
        assert_eq!(registry.get("fade").unwrap().frames[0].values.len(), 1);
        assert_eq!(number(&registry.get("fade").unwrap().frames[0], AnimProp::Opacity), Some(0.0));

        assert!(registry.define("none", "{}").is_err());
        assert!(registry.define("2d", "{}").is_err());
        assert!(registry.define("", "{}").is_err());
    }
}
