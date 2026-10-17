//! `place-items`, `place-content` and `place-self`: `<align> <justify>?`, where one value is
//! both.
//!
//! Each is kept as its own style field holding the two longhands it sets (see
//! [`super::flex`]), and a consumer reads them through the `effective_*` methods of
//! [`StyleDesc`].

use serde::{Deserializer, Serialize, Serializer};

use super::shorthand::deserialize_optional;
use super::syntax::split_whitespace;
use crate::style::StyleDesc;

/// The two longhands of a `place-*` shorthand, each as the keyword the longhand takes:
/// `"center"`, `"safe center"`, `"last baseline"`. `first baseline` is `baseline`.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub align: String,
    pub justify: String,
}

impl Serialize for Place {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.align == self.justify {
            serializer.serialize_str(&self.align)
        } else {
            serializer.collect_str(&format_args!("{} {}", self.align, self.justify))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Longhand {
    AlignItems,
    JustifyItems,
    AlignSelf,
    JustifySelf,
    AlignContent,
    JustifyContent,
}

impl Longhand {
    fn is_justify(self) -> bool {
        matches!(self, Self::JustifyItems | Self::JustifySelf | Self::JustifyContent)
    }

    fn is_content(self) -> bool {
        matches!(self, Self::AlignContent | Self::JustifyContent)
    }

    fn is_self(self) -> bool {
        matches!(self, Self::AlignSelf | Self::JustifySelf)
    }

    /// Whether `value` (`center`, `safe center`, `space-between`, ...) is one this longhand
    /// takes.
    fn accepts(self, value: &str) -> bool {
        let (overflow, keyword) = match value.split_once(' ') {
            Some(("safe" | "unsafe", keyword)) => (true, keyword),
            Some(("last", "baseline")) => return true,
            Some(_) => return false,
            None => (false, value),
        };
        let position = match keyword {
            "center" | "start" | "end" | "flex-start" | "flex-end" => true,
            "self-start" | "self-end" => !self.is_content(),
            "left" | "right" => self.is_justify(),
            _ => false,
        };
        if overflow {
            return position;
        }
        position
            || match keyword {
                "normal" | "baseline" | "stretch" => true,
                "auto" => self.is_self(),
                "space-between" | "space-around" | "space-evenly" => self.is_content(),
                _ => false,
            }
    }
}

/// The values of `text`, each one or two words: `safe center` and `last baseline` are one.
fn values(text: &str) -> Result<Vec<String>, String> {
    let lowered = text.to_ascii_lowercase();
    let mut words = split_whitespace(&lowered).into_iter();
    let mut values = Vec::new();
    while let Some(word) = words.next() {
        let value = match word {
            "safe" | "unsafe" | "first" | "last" => {
                let next = words.next().ok_or_else(|| format!("\"{word}\" needs a keyword after it"))?;
                match (word, next) {
                    ("first", "baseline") => "baseline".to_string(),
                    _ => format!("{word} {next}"),
                }
            }
            _ => word.to_string(),
        };
        values.push(value);
    }
    Ok(values)
}

impl Place {
    fn parse(text: &str, align: Longhand, justify: Longhand) -> Result<Self, String> {
        let invalid = |reason: String| format!("invalid {} \"{text}\": {reason}", name(align));
        let values = values(text).map_err(&invalid)?;
        let (first, second) = match values.as_slice() {
            [both] => (both, both),
            [first, second] => (first, second),
            _ => return Err(invalid("expected one or two values".to_string())),
        };
        for (value, longhand) in [(first, align), (second, justify)] {
            if !longhand.accepts(value) {
                return Err(invalid(format!("\"{value}\" is not a value of {}", longhand_name(longhand))));
            }
        }
        Ok(Self { align: first.clone(), justify: second.clone() })
    }
}

fn name(align: Longhand) -> &'static str {
    match align {
        Longhand::AlignItems => "place-items",
        Longhand::AlignSelf => "place-self",
        _ => "place-content",
    }
}

fn longhand_name(longhand: Longhand) -> &'static str {
    match longhand {
        Longhand::AlignItems => "align-items",
        Longhand::JustifyItems => "justify-items",
        Longhand::AlignSelf => "align-self",
        Longhand::JustifySelf => "justify-self",
        Longhand::AlignContent => "align-content",
        Longhand::JustifyContent => "justify-content",
    }
}

fn deserialize_place<'de, D: Deserializer<'de>>(
    deserializer: D,
    align: Longhand,
    justify: Longhand,
) -> Result<Option<Place>, D::Error> {
    deserialize_optional(
        deserializer,
        "a CSS value such as 'center' or 'center end'",
        |number| Err(format!("invalid {} {number}: expected a string", name(align))),
        |text| Place::parse(text, align, justify),
    )
}

pub(crate) fn deserialize_place_items<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Place>, D::Error> {
    deserialize_place(deserializer, Longhand::AlignItems, Longhand::JustifyItems)
}

pub(crate) fn deserialize_place_content<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Place>, D::Error> {
    deserialize_place(deserializer, Longhand::AlignContent, Longhand::JustifyContent)
}

pub(crate) fn deserialize_place_self<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Place>, D::Error> {
    deserialize_place(deserializer, Longhand::AlignSelf, Longhand::JustifySelf)
}

/// What the shorthands fill in for a longhand the style leaves unset.
impl StyleDesc {
    pub(crate) fn effective_align_items(&self) -> Option<&str> {
        self.align_items.as_deref().or_else(|| self.place_items.as_ref().map(|place| place.align.as_str()))
    }

    pub(crate) fn effective_justify_items(&self) -> Option<&str> {
        self.justify_items.as_deref().or_else(|| self.place_items.as_ref().map(|place| place.justify.as_str()))
    }

    pub(crate) fn effective_align_self(&self) -> Option<&str> {
        self.align_self.as_deref().or_else(|| self.place_self.as_ref().map(|place| place.align.as_str()))
    }

    pub(crate) fn effective_justify_self(&self) -> Option<&str> {
        self.justify_self.as_deref().or_else(|| self.place_self.as_ref().map(|place| place.justify.as_str()))
    }

    pub(crate) fn effective_align_content(&self) -> Option<&str> {
        self.align_content.as_deref().or_else(|| self.place_content.as_ref().map(|place| place.align.as_str()))
    }

    pub(crate) fn effective_justify_content(&self) -> Option<&str> {
        self.justify_content.as_deref().or_else(|| self.place_content.as_ref().map(|place| place.justify.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(text: &str) -> Result<(String, String), String> {
        Place::parse(text, Longhand::AlignItems, Longhand::JustifyItems).map(|place| (place.align, place.justify))
    }

    fn content(text: &str) -> Result<(String, String), String> {
        Place::parse(text, Longhand::AlignContent, Longhand::JustifyContent).map(|place| (place.align, place.justify))
    }

    fn own(text: &str) -> Result<(String, String), String> {
        Place::parse(text, Longhand::AlignSelf, Longhand::JustifySelf).map(|place| (place.align, place.justify))
    }

    fn pair(align: &str, justify: &str) -> Result<(String, String), String> {
        Ok((align.to_string(), justify.to_string()))
    }

    #[test]
    fn one_value_is_both_and_two_are_align_then_justify() {
        assert_eq!(items("center"), pair("center", "center"));
        assert_eq!(items("center end"), pair("center", "end"));
        assert_eq!(items("  FLEX-START   stretch "), pair("flex-start", "stretch"));
        assert_eq!(content("space-between"), pair("space-between", "space-between"));
        assert_eq!(content("center space-evenly"), pair("center", "space-evenly"));
        assert_eq!(own("auto"), pair("auto", "auto"));
        assert_eq!(own("auto center"), pair("auto", "center"));
    }

    #[test]
    fn an_overflow_position_and_a_baseline_position_are_one_value() {
        assert_eq!(items("safe center"), pair("safe center", "safe center"));
        assert_eq!(items("unsafe end safe start"), pair("unsafe end", "safe start"));
        assert_eq!(items("first baseline"), pair("baseline", "baseline"));
        assert_eq!(items("last baseline center"), pair("last baseline", "center"));
    }

    #[test]
    fn left_and_right_are_for_the_inline_axis_only() {
        assert_eq!(items("stretch left"), pair("stretch", "left"));
        assert!(items("left").is_err());
        assert!(content("left").is_err());
        assert_eq!(content("center right"), pair("center", "right"));
    }

    #[test]
    fn each_property_takes_its_own_keywords_and_what_chrome_rejects_is_an_error() {
        for text in ["", "space-between", "auto", "bogus", "center end start", "safe", "safe baseline", "center,end"] {
            assert!(items(text).is_err(), "place-items {text:?} should be invalid");
        }
        for text in ["", "auto", "self-start", "bogus", "center end start"] {
            assert!(content(text).is_err(), "place-content {text:?} should be invalid");
        }
        for text in ["", "space-around", "bogus"] {
            assert!(own(text).is_err(), "place-self {text:?} should be invalid");
        }
        assert!(items("self-end").is_ok());
        assert!(items("normal").is_ok());
        let error = items("space-between").unwrap_err();
        assert!(error.contains("invalid place-items \"space-between\""), "{error}");
    }

    #[test]
    fn the_shorthand_fills_in_what_a_longhand_leaves_unset() {
        let style: StyleDesc = serde_json::from_str(
            r#"{"placeItems": "center end", "alignItems": "start", "placeContent": "space-between", "placeSelf": "end"}"#,
        )
        .unwrap();
        assert_eq!(style.effective_align_items(), Some("start"));
        assert_eq!(style.effective_justify_items(), Some("end"));
        assert_eq!(style.effective_align_content(), Some("space-between"));
        assert_eq!(style.effective_justify_content(), Some("space-between"));
        assert_eq!(style.effective_align_self(), Some("end"));
        assert_eq!(style.effective_justify_self(), Some("end"));
        assert_eq!(StyleDesc::default().effective_align_items(), None);
    }

    #[test]
    fn serializes_as_the_text_it_parses_from() {
        let json = |text: &str| serde_json::to_string(&Place::parse(text, Longhand::AlignItems, Longhand::JustifyItems).unwrap()).unwrap();
        assert_eq!(json("center"), "\"center\"");
        assert_eq!(json("center end"), "\"center end\"");
        assert_eq!(json("safe center last baseline"), "\"safe center last baseline\"");
    }
}
