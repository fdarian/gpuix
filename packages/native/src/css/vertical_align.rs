//! CSS `vertical-align` (CSS 2.1 §10.8.1): where an inline box sits on its line,
//! relative to the line's baseline or to the line box itself.

use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::dimension::parse_linear_length;
use super::length::{LengthContext, LinearLength};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VerticalAlign {
    Baseline,
    /// Lowers the baseline to the parent's subscript position.
    Sub,
    /// Raises the baseline to the parent's superscript position.
    Super,
    /// Aligns the top of the box with the top of the parent's content area.
    TextTop,
    /// Aligns the bottom of the box with the bottom of the parent's content area.
    TextBottom,
    /// Aligns the midpoint of the box with the parent's baseline plus half its x-height.
    Middle,
    /// Aligns the top of the box with the top of the line box.
    Top,
    /// Aligns the bottom of the box with the bottom of the line box.
    Bottom,
    /// Raises the box's baseline by this much (lowers it when negative). A `%` is
    /// of the element's own `line-height`, which only the text layout knows.
    Length(LinearLength),
}

/// A [`VerticalAlign`] with its lengths in px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResolvedVerticalAlign {
    Baseline,
    Sub,
    Super,
    TextTop,
    TextBottom,
    Middle,
    Top,
    Bottom,
    /// Raised by `px + line_height_fraction * line-height`.
    Offset { px: f64, line_height_fraction: f64 },
}

impl VerticalAlign {
    pub(crate) fn resolve(&self, lengths: &LengthContext) -> ResolvedVerticalAlign {
        match self {
            Self::Baseline => ResolvedVerticalAlign::Baseline,
            Self::Sub => ResolvedVerticalAlign::Sub,
            Self::Super => ResolvedVerticalAlign::Super,
            Self::TextTop => ResolvedVerticalAlign::TextTop,
            Self::TextBottom => ResolvedVerticalAlign::TextBottom,
            Self::Middle => ResolvedVerticalAlign::Middle,
            Self::Top => ResolvedVerticalAlign::Top,
            Self::Bottom => ResolvedVerticalAlign::Bottom,
            Self::Length(length) => {
                let (px, line_height_fraction) = lengths.split(length);
                ResolvedVerticalAlign::Offset { px, line_height_fraction }
            }
        }
    }

    fn keyword(&self) -> Option<&'static str> {
        Some(match self {
            Self::Baseline => "baseline",
            Self::Sub => "sub",
            Self::Super => "super",
            Self::TextTop => "text-top",
            Self::TextBottom => "text-bottom",
            Self::Middle => "middle",
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Length(_) => return None,
        })
    }

    fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let keywords = [
            Self::Baseline,
            Self::Sub,
            Self::Super,
            Self::TextTop,
            Self::TextBottom,
            Self::Middle,
            Self::Top,
            Self::Bottom,
        ];
        keywords
            .into_iter()
            .find(|keyword| keyword.keyword().is_some_and(|name| name.eq_ignore_ascii_case(text)))
            .or_else(|| parse_linear_length(text).map(Self::Length))
    }
}

impl Serialize for VerticalAlign {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Length(length) => length.serialize(serializer),
            keyword => serializer.serialize_str(keyword.keyword().expect("not a length")),
        }
    }
}

const RULE: &str = "baseline, sub, super, text-top, text-bottom, middle, top, bottom, a CSS length or a percentage of the line height";

impl<'de> Deserialize<'de> for VerticalAlign {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct VerticalAlignVisitor;

        impl Visitor<'_> for VerticalAlignVisitor {
            type Value = VerticalAlign;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(RULE)
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> Result<VerticalAlign, E> {
                if !value.is_finite() {
                    return Err(E::custom(format!("invalid verticalAlign {value}: not a finite number")));
                }
                Ok(VerticalAlign::Length(LinearLength::px(value).clamped()))
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<VerticalAlign, E> {
                self.visit_f64(value as f64)
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<VerticalAlign, E> {
                self.visit_f64(value as f64)
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<VerticalAlign, E> {
                VerticalAlign::parse(value)
                    .ok_or_else(|| E::custom(format!("invalid verticalAlign \"{value}\": {RULE}")))
            }
        }

        deserializer.deserialize_any(VerticalAlignVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<VerticalAlign, serde_json::Error> {
        serde_json::from_str(json)
    }

    fn resolved(json: &str) -> ResolvedVerticalAlign {
        // 1em is 20px, 1rem 10px, the viewport 800x600.
        parse(json).unwrap().resolve(&LengthContext::for_tests())
    }

    #[test]
    fn every_keyword_parses_in_any_case() {
        for (text, keyword) in [
            ("baseline", VerticalAlign::Baseline),
            ("sub", VerticalAlign::Sub),
            ("super", VerticalAlign::Super),
            ("text-top", VerticalAlign::TextTop),
            ("text-bottom", VerticalAlign::TextBottom),
            ("middle", VerticalAlign::Middle),
            ("top", VerticalAlign::Top),
            ("bottom", VerticalAlign::Bottom),
        ] {
            assert_eq!(parse(&format!("\"{text}\"")).unwrap(), keyword);
            assert_eq!(parse(&format!("\" {} \"", text.to_ascii_uppercase())).unwrap(), keyword);
        }
    }

    #[test]
    fn a_number_is_px_and_a_length_is_resolved_through_the_context() {
        let offset = |px: f64| ResolvedVerticalAlign::Offset { px, line_height_fraction: 0.0 };
        assert_eq!(resolved("4"), offset(4.0));
        assert_eq!(resolved("-4.5"), offset(-4.5));
        assert_eq!(resolved("\"-6px\""), offset(-6.0));
        assert_eq!(resolved("\"2em\""), offset(40.0));
        assert_eq!(resolved("\"2rem\""), offset(20.0));
        assert_eq!(resolved("\"10vw\""), offset(80.0));
        assert_eq!(resolved("\"1in\""), offset(96.0));
        assert_eq!(resolved("\"calc(1em + 4px)\""), offset(24.0));
        assert_eq!(resolved("\"0\""), offset(0.0));
    }

    #[test]
    fn a_percentage_is_left_as_a_fraction_of_the_line_height() {
        assert_eq!(
            resolved("\"50%\""),
            ResolvedVerticalAlign::Offset { px: 0.0, line_height_fraction: 0.5 }
        );
        assert_eq!(
            resolved("\"-25%\""),
            ResolvedVerticalAlign::Offset { px: 0.0, line_height_fraction: -0.25 }
        );
        assert_eq!(
            resolved("\"calc(100% - 1em)\""),
            ResolvedVerticalAlign::Offset { px: -20.0, line_height_fraction: 1.0 }
        );
    }

    #[test]
    fn keywords_resolve_to_themselves() {
        assert_eq!(resolved("\"text-top\""), ResolvedVerticalAlign::TextTop);
        assert_eq!(resolved("\"bottom\""), ResolvedVerticalAlign::Bottom);
    }

    #[test]
    fn what_css_rejects_is_an_error_naming_the_value() {
        for json in ["\"center\"", "\"12\"", "\"1 2px\"", "\"auto\"", "\"\"", "\"text\"", "true", "null"] {
            assert!(parse(json).is_err(), "{json} is accepted");
        }
        let error = parse("\"center\"").unwrap_err().to_string();
        assert!(error.contains("invalid verticalAlign \"center\""), "{error}");
    }

    #[test]
    fn it_survives_a_round_trip() {
        for json in ["\"baseline\"", "\"text-bottom\"", "4", "\"50%\"", "\"calc(10% + 2px)\"", "\"2em\""] {
            let parsed = parse(json).unwrap();
            let again = parse(&serde_json::to_string(&parsed).unwrap()).unwrap();
            assert_eq!(again, parsed, "{json}");
        }
    }

    #[test]
    fn a_style_reads_it_and_an_invalid_one_is_not_interned() {
        let style: crate::style::StyleDesc = serde_json::from_str(r#"{"verticalAlign": "middle"}"#).unwrap();
        assert_eq!(style.vertical_align, Some(VerticalAlign::Middle));
        let mut table = crate::retained_tree::StyleTable::default();
        assert!(table.intern(br#"{"verticalAlign": "-3px"}"#).is_ok());
        let error = table.intern(br#"{"verticalAlign": "center"}"#).unwrap_err();
        assert!(error.contains("invalid verticalAlign"), "{error}");
    }
}
