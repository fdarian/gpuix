//! CSS `tab-size`: how far apart the tab stops of a text are.

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

use super::dimension::{self, Quantity};

/// A number is that many times the width of a space, a length is that distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TabSize {
    Spaces(f64),
    /// In px.
    Length(f64),
}

/// What `tab-size` is when nothing declares it.
pub const DEFAULT: gpui::TabSize = gpui::TabSize::Spaces(8.0);

impl TabSize {
    pub fn to_gpui(self) -> gpui::TabSize {
        match self {
            Self::Spaces(spaces) => gpui::TabSize::Spaces(spaces as f32),
            Self::Length(length) => gpui::TabSize::Length(gpui::px(length as f32)),
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
        match dimension::parse_quantity(value).ok_or_else(invalid)? {
            Quantity::Number(spaces) => Self::spaces(spaces),
            Quantity::Length(length) if length >= 0.0 => {
                Ok(Self::Length(length.min(dimension::MAX_LENGTH)))
            }
            Quantity::Length(_) | Quantity::Percent(_) => Err(invalid()),
        }
    }
}

impl Serialize for TabSize {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Spaces(spaces) => serializer.serialize_f64(*spaces),
            Self::Length(length) => serializer.serialize_str(&format!("{length}px")),
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
        assert_eq!(parse("\"40px\"").unwrap(), TabSize::Length(40.0));
        assert_eq!(parse("\"1in\"").unwrap(), TabSize::Length(96.0));
        assert_eq!(parse("\"calc(10px + 6px)\"").unwrap(), TabSize::Length(16.0));
    }

    #[test]
    fn what_css_rejects_is_an_error() {
        for json in [
            "-1",
            "\"-1\"",
            "\"-4px\"",
            "\"50%\"",
            "\"2ch\"",
            "\"2rem\"",
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
        assert_eq!(style.tab_size, Some(TabSize::Length(40.0)));
        let mut table = crate::retained_tree::StyleTable::default();
        assert!(table.intern(br#"{"tabSize": 4}"#).is_ok());
        let error = table.intern(br#"{"tabSize": "wide"}"#).unwrap_err();
        assert!(error.contains("invalid tabSize"), "{error}");
    }

    #[test]
    fn converts_to_gpui() {
        assert_eq!(TabSize::Spaces(4.0).to_gpui(), gpui::TabSize::Spaces(4.0));
        assert_eq!(
            TabSize::Length(40.0).to_gpui(),
            gpui::TabSize::Length(gpui::px(40.0))
        );
    }

    #[test]
    fn survives_a_round_trip() {
        for size in [TabSize::Spaces(4.0), TabSize::Length(40.0)] {
            let json = serde_json::to_string(&size).unwrap();
            assert_eq!(parse(&json).unwrap(), size);
        }
    }
}
