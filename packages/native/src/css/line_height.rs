//! CSS `line-height`: `normal`, a unitless multiplier of the element's font
//! size, or a length where `%` is a fraction of that same font size.

use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::dimension::{parse_linear_length, MAX_LENGTH};
use super::length::{LengthContext, LinearLength};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineHeight {
    /// The font's default leading, which is also what an element that sets no
    /// line height gets.
    Normal,
    /// A multiple of the font size of the element the text is in: a child with
    /// another font size inherits the multiplier, not the px.
    Multiplier(f64),
    /// Resolved at the element that declares it, and inherited as px.
    Length(LinearLength),
}

impl LineHeight {
    /// `None` for `normal`. A `calc()` that comes out negative clamps to zero.
    pub(crate) fn resolve_px(&self, lengths: &LengthContext) -> Option<f64> {
        match self {
            Self::Normal => None,
            Self::Multiplier(multiplier) => Some(multiplier * lengths.font_size),
            Self::Length(length) => Some(lengths.resolve(length, lengths.font_size).max(0.0)),
        }
    }

    /// The text-style line height an element declaring this sets.
    pub(crate) fn text_style_value(&self, lengths: &LengthContext) -> gpui::DefiniteLength {
        match self {
            Self::Normal => gpui::phi(),
            Self::Multiplier(multiplier) => gpui::relative(*multiplier as f32),
            Self::Length(_) => {
                let px = self.resolve_px(lengths).expect("a length resolves to px");
                gpui::px(px.min(MAX_LENGTH) as f32).into()
            }
        }
    }

    /// Clamped like a length, so a huge multiple cannot overflow `f32`.
    fn multiplier(value: f64) -> Self {
        Self::Multiplier(value.min(MAX_LENGTH))
    }

    fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.eq_ignore_ascii_case("normal") {
            return Some(Self::Normal);
        }
        if let Ok(multiplier) = text.parse::<f64>() {
            return multiplier.is_finite().then(|| Self::multiplier(multiplier));
        }
        parse_linear_length(text).map(Self::Length)
    }

    fn is_negative(&self) -> bool {
        match self {
            Self::Normal => false,
            Self::Multiplier(multiplier) => *multiplier < 0.0,
            Self::Length(length) => length.is_negative(),
        }
    }
}

impl Serialize for LineHeight {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Normal => serializer.serialize_str("normal"),
            Self::Multiplier(multiplier) => serializer.serialize_f64(*multiplier),
            Self::Length(length) => serializer.collect_str(length),
        }
    }
}

const RULE: &str = "a number, normal, or a non-negative CSS length or percentage of the font size";

impl<'de> Deserialize<'de> for LineHeight {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct LineHeightVisitor;

        impl Visitor<'_> for LineHeightVisitor {
            type Value = LineHeight;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(RULE)
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> Result<LineHeight, E> {
                if !value.is_finite() {
                    return Err(E::custom(format!("invalid line height {value}: not a finite number")));
                }
                check(LineHeight::multiplier(value), &value.to_string())
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<LineHeight, E> {
                self.visit_f64(value as f64)
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<LineHeight, E> {
                self.visit_f64(value as f64)
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<LineHeight, E> {
                let parsed = LineHeight::parse(value)
                    .ok_or_else(|| E::custom(format!("invalid line height \"{value}\": {RULE}")))?;
                check(parsed, value)
            }
        }

        fn check<E: de::Error>(line_height: LineHeight, written: &str) -> Result<LineHeight, E> {
            if line_height.is_negative() {
                return Err(E::custom(format!("invalid line height {written}: it cannot be negative")));
            }
            Ok(line_height)
        }

        deserializer.deserialize_any(LineHeightVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<LineHeight, serde_json::Error> {
        serde_json::from_str(json)
    }

    fn lengths() -> LengthContext {
        // 1em is 20px, 1rem 10px, the viewport 800x600.
        LengthContext::for_tests()
    }

    #[test]
    fn a_number_and_a_numeric_string_are_multipliers() {
        assert_eq!(parse("1.5").unwrap(), LineHeight::Multiplier(1.5));
        assert_eq!(parse("2").unwrap(), LineHeight::Multiplier(2.0));
        assert_eq!(parse("0").unwrap(), LineHeight::Multiplier(0.0));
        assert_eq!(parse(r#""1.5""#).unwrap(), LineHeight::Multiplier(1.5));
        assert_eq!(parse(r#"" 2 ""#).unwrap(), LineHeight::Multiplier(2.0));
        assert_eq!(parse(r#"".5""#).unwrap(), LineHeight::Multiplier(0.5));
    }

    #[test]
    fn normal_is_a_keyword_in_any_case() {
        assert_eq!(parse(r#""normal""#).unwrap(), LineHeight::Normal);
        assert_eq!(parse(r#""Normal""#).unwrap(), LineHeight::Normal);
    }

    #[test]
    fn a_string_with_a_unit_is_a_length_and_percent_is_of_the_font_size() {
        let lengths = lengths();
        let px = |json: &str| parse(json).unwrap().resolve_px(&lengths).unwrap();
        assert_eq!(px(r#""24px""#), 24.0);
        assert_eq!(px(r#""1.5em""#), 30.0);
        assert_eq!(px(r#""150%""#), 30.0);
        assert_eq!(px(r#""2rem""#), 20.0);
        assert_eq!(px(r#""10vw""#), 80.0);
        assert_eq!(px(r#""1in""#), 96.0);
        assert_eq!(px(r#""calc(1em + 2px)""#), 22.0);
        assert_eq!(px(r#""calc(50% - 1rem)""#), 0.0);
        assert_eq!(px(r#""0px""#), 0.0);
    }

    #[test]
    fn a_calc_that_is_negative_only_in_some_contexts_clamps_to_zero() {
        assert_eq!(parse(r#""calc(1px - 1em)""#).unwrap().resolve_px(&lengths()), Some(0.0));
    }

    #[test]
    fn a_multiplier_resolves_against_the_font_size() {
        assert_eq!(LineHeight::Multiplier(1.5).resolve_px(&lengths()), Some(30.0));
        assert_eq!(LineHeight::Normal.resolve_px(&lengths()), None);
    }

    #[test]
    fn a_negative_or_unparsable_line_height_names_the_value() {
        for json in [r#"-1"#, r#""-1.5""#, r#""-4px""#, r#""-10%""#, r#""calc(-1em - 2px)""#] {
            let error = parse(json).unwrap_err().to_string();
            assert!(error.contains("cannot be negative"), "{json}: {error}");
        }
        for json in [r#""big""#, r#""12 px""#, r#""inf""#, r#""NaN""#, r#""""#, "true"] {
            assert!(parse(json).is_err(), "{json}");
        }
        let error = parse(r#""big""#).unwrap_err().to_string();
        assert!(error.contains("invalid line height \"big\""), "{error}");
    }

    #[test]
    fn it_round_trips_through_serialize() {
        for json in ["1.5", r#""normal""#, r#""24px""#, r#""calc(100% + 2px)""#] {
            let parsed = parse(json).unwrap();
            let again: LineHeight = serde_json::from_str(&serde_json::to_string(&parsed).unwrap()).unwrap();
            assert_eq!(again, parsed, "{json}");
        }
    }

    #[test]
    fn the_text_style_value_is_a_fraction_for_a_multiplier_and_px_for_a_length() {
        let lengths = lengths();
        assert_eq!(LineHeight::Multiplier(2.0).text_style_value(&lengths), gpui::relative(2.0));
        assert_eq!(LineHeight::Normal.text_style_value(&lengths), gpui::phi());
        assert_eq!(
            parse(r#""1.5em""#).unwrap().text_style_value(&lengths),
            gpui::DefiniteLength::from(gpui::px(30.0))
        );
    }
}
