//! CSS `tab-size`: how far apart the tab stops of a text are.

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

use super::dimension;
use super::length::{LengthContext, LinearLength};

/// A number is that many times the width of a space, a length is that distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TabSize {
    Spaces(f64),
    /// A length without a percentage, unresolved until the element's font size is known.
    Length(LinearLength),
}

/// What `tab-size` is when nothing declares it.
pub const DEFAULT: gpui::TabSize = gpui::TabSize::Spaces(8.0);

impl TabSize {
    pub fn to_gpui(self, lengths: &LengthContext) -> gpui::TabSize {
        match self {
            Self::Spaces(spaces) => gpui::TabSize::Spaces(spaces as f32),
            Self::Length(length) => {
                let px = lengths.resolve(&length, 0.0).clamp(0.0, dimension::MAX_LENGTH);
                gpui::TabSize::Length(gpui::px(px as f32))
            }
        }
    }

    fn spaces<E: de::Error>(spaces: f64) -> Result<Self, E> {
        if spaces.is_finite() && spaces >= 0.0 {
            Ok(Self::Spaces(spaces))
        } else {
            Err(E::custom(format!(
                "tabSize must not be negative, got {spaces}"
            )))
        }
    }

    fn parse<E: de::Error>(value: &str) -> Result<Self, E> {
        let invalid = || E::custom(format!("invalid tabSize: {value:?}"));
        if let Some(spaces) = dimension::parse_number(value) {
            return Self::spaces(spaces);
        }
        dimension::parse_definite_length(value)
            .filter(|length| !length.is_negative())
            .map(Self::Length)
            .ok_or_else(invalid)
    }
}

impl Serialize for TabSize {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Spaces(spaces) => serializer.serialize_f64(*spaces),
            Self::Length(length) => serializer.collect_str(length),
        }
    }
}

impl<'de> Deserialize<'de> for TabSize {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TabSizeVisitor;

        impl Visitor<'_> for TabSizeVisitor {
            type Value = TabSize;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a number of spaces or a CSS length")
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                TabSize::spaces(v)
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                TabSize::spaces(v as f64)
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                TabSize::spaces(v as f64)
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                TabSize::parse(v)
            }
        }

        deserializer.deserialize_any(TabSizeVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<TabSize, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn a_number_is_spaces() {
        assert_eq!(parse("4").unwrap(), TabSize::Spaces(4.0));
        assert_eq!(parse("2.5").unwrap(), TabSize::Spaces(2.5));
        assert_eq!(parse("0").unwrap(), TabSize::Spaces(0.0));
        assert_eq!(parse("\"4\"").unwrap(), TabSize::Spaces(4.0));
    }

    #[test]
    fn a_length_is_a_distance() {
        assert_eq!(parse("\"40px\"").unwrap(), TabSize::Length(LinearLength::px(40.0)));
        assert_eq!(parse("\"1in\"").unwrap(), TabSize::Length(LinearLength::px(96.0)));
        assert_eq!(parse("\"calc(10px + 6px)\"").unwrap(), TabSize::Length(LinearLength::px(16.0)));
    }

    #[test]
    fn what_css_rejects_is_an_error() {
        for json in [
            "-1",
            "\"-1\"",
            "\"-4px\"",
            "\"50%\"",
            "\"2ch\"",
            "\"wide\"",
            "\"\"",
            "true",
        ] {
            assert!(parse(json).is_err(), "{json} is accepted");
        }
    }

    #[test]
    fn a_style_reads_it_and_an_invalid_one_is_not_interned() {
        let style: crate::style::StyleDesc = serde_json::from_str(r#"{"tabSize": "40px"}"#).unwrap();
        assert_eq!(style.tab_size, Some(TabSize::Length(LinearLength::px(40.0))));
        let mut table = crate::retained_tree::StyleTable::default();
        assert!(table.intern(br#"{"tabSize": 4}"#).is_ok());
        let error = table.intern(br#"{"tabSize": "wide"}"#).unwrap_err();
        assert!(error.contains("invalid tabSize"), "{error}");
    }

    #[test]
    fn converts_to_gpui() {
        let lengths = LengthContext::for_tests();
        assert_eq!(TabSize::Spaces(4.0).to_gpui(&lengths), gpui::TabSize::Spaces(4.0));
        assert_eq!(
            TabSize::Length(LinearLength::px(40.0)).to_gpui(&lengths),
            gpui::TabSize::Length(gpui::px(40.0))
        );
    }

    #[test]
    fn a_length_in_the_units_of_the_context_resolves_against_it() {
        // 1em is 20px, 1rem 10px and the viewport 800x600 in the test context.
        let lengths = LengthContext::for_tests();
        for (json, px) in [("\"2em\"", 40.0), ("\"2rem\"", 20.0), ("\"calc(1em + 4px)\"", 24.0), ("\"10vw\"", 80.0)] {
            assert_eq!(parse(json).unwrap().to_gpui(&lengths), gpui::TabSize::Length(gpui::px(px)), "{json}");
        }
    }

    #[test]
    fn survives_a_round_trip() {
        for size in [TabSize::Spaces(4.0), TabSize::Length(LinearLength::px(40.0)), TabSize::Length(LinearLength { em: 2.0, ..LinearLength::default() })] {
            let json = serde_json::to_string(&size).unwrap();
            assert_eq!(parse(&json).unwrap(), size);
        }
    }
}
