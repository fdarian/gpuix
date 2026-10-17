//! The CSS `outline` shorthand: `<width> || <style> || <color>`.
//!
//! It is kept as its own style field holding the three longhands it sets, a part the
//! value leaves out at its initial value (`medium`, `none`, `currentcolor`), so a state
//! style that sets it still replaces the longhands it covers and `outlineWidth`,
//! `outlineStyle` and `outlineColor` still beat it. `outlineOffset` is not part of it.

use std::fmt;

use serde::{Deserializer, Serialize, Serializer};

use super::border::{parse_line, LineStyle, MEDIUM};
use super::length::LinearLength;
use super::shorthand::deserialize_optional;
use crate::color::{ColorContext, Scheme};
use crate::style::StyleDesc;

#[derive(Debug, Clone, PartialEq)]
pub struct Outline {
    /// As written: a `none` style paints nothing whatever the width says.
    pub width: LinearLength,
    pub style: LineStyle,
    /// Colour text, still to be resolved against the element.
    pub color: String,
}

impl Outline {
    fn parse(text: &str) -> Result<Self, String> {
        let parsed = parse_line(text, LineStyle::parse_outline).ok_or_else(|| {
            format!(
                "invalid outline \"{text}\": expected a width, a style (not hidden) and a colour, each at most once, in any order"
            )
        })?;
        // Whether a colour resolves never depends on the element, so the root context stands in.
        if ColorContext::root(Scheme::Light).resolve(parsed.color).is_none() {
            return Err(format!("invalid outline \"{text}\": \"{}\" is not a colour", parsed.color));
        }
        Ok(Self { width: parsed.width, style: parsed.style, color: parsed.color.to_string() })
    }
}

impl fmt::Display for Outline {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {} {}", self.width, self.style, self.color)
    }
}

impl Serialize for Outline {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl Default for Outline {
    /// `outline: initial`.
    fn default() -> Self {
        Self { width: MEDIUM, style: LineStyle::None, color: "currentColor".to_string() }
    }
}

impl StyleDesc {
    /// Whether the style sets anything about the outline's line (not its offset alone).
    pub(crate) fn declares_outline(&self) -> bool {
        self.outline.is_some()
            || self.outline_width.is_some()
            || self.outline_style.is_some()
            || self.outline_color.is_some()
            || self.outline_offset.is_some()
    }
}

pub(crate) fn deserialize_outline<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Outline>, D::Error> {
    deserialize_optional(
        deserializer,
        "a CSS outline such as '2px solid red'",
        |number| Err(format!("invalid outline {number}: expected a string such as '2px solid red'")),
        Outline::parse,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::dimension::parse_linear_length;

    fn outline(text: &str) -> Outline {
        Outline::parse(text).unwrap_or_else(|error| panic!("{text}: {error}"))
    }

    #[test]
    fn width_style_and_colour_come_in_any_order() {
        let expected = Outline {
            width: parse_linear_length("2px").unwrap(),
            style: LineStyle::Dashed,
            color: "red".to_string(),
        };
        for text in ["2px dashed red", "red dashed 2px", "dashed 2px red", "red 2px dashed", "  2px   dashed   red "] {
            assert_eq!(outline(text), expected, "{text}");
        }
    }

    #[test]
    fn a_missing_part_has_its_initial_value() {
        assert_eq!(outline("solid"), Outline { style: LineStyle::Solid, ..Outline::default() });
        assert_eq!(outline("2px"), Outline { width: parse_linear_length("2px").unwrap(), ..Outline::default() });
        assert_eq!(outline("red"), Outline { color: "red".to_string(), ..Outline::default() });
        assert_eq!(outline("none").style, LineStyle::None);
        assert_eq!(outline("thick solid").width, parse_linear_length("5px").unwrap());
    }

    #[test]
    fn a_colour_may_hold_spaces() {
        assert_eq!(outline("2px solid rgb(0 128 255 / 50%)").color, "rgb(0 128 255 / 50%)");
        assert_eq!(outline("calc(1px + 1em) solid").style, LineStyle::Solid);
    }

    #[test]
    fn auto_is_solid_and_hidden_is_invalid() {
        assert_eq!(outline("2px auto red").style, LineStyle::Solid);
        assert!(Outline::parse("hidden").is_err());
        assert!(Outline::parse("2px hidden red").is_err());
    }

    #[test]
    fn what_chrome_rejects_is_an_error() {
        for text in ["", "  ", "2px 3px", "solid dashed", "red blue", "50%", "-1px solid", "2px solid notacolour", "2"] {
            assert!(Outline::parse(text).is_err(), "{text:?} should be invalid");
        }
    }

    #[test]
    fn serializes_as_the_css_it_parses_from() {
        let outline = outline("2px dotted rgb(0 128 255 / 50%)");
        let json = serde_json::to_string(&outline).unwrap();
        assert_eq!(json, "\"2px dotted rgb(0 128 255 / 50%)\"");
        assert_eq!(Outline::parse(json.trim_matches('"')).unwrap(), outline);
    }
}
