//! A CSS length as a linear expression over the units a length can mix, and
//! the context that turns one into px.
//!
//! The expression is built once, when the style is set, and resolved when the
//! tree is applied, because `em`, `rem`, `%` and the viewport units only have a
//! value once the element's font size, its containing block and the window are
//! known.

use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::dimension::{parse_linear_length, MAX_LENGTH};

/// `px + percent * basis + em * font-size + ...`. The relative terms are
/// fractions or multipliers: `50%` and `50vw` are `0.5`, `2em` is `2.0`.
/// `vmin` and `vmax` stay separate because which viewport side they follow is
/// only known against a window.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct LinearLength {
    pub(crate) px: f64,
    pub(crate) percent: f64,
    pub(crate) em: f64,
    pub(crate) rem: f64,
    pub(crate) vw: f64,
    pub(crate) vh: f64,
    pub(crate) vmin: f64,
    pub(crate) vmax: f64,
}

impl LinearLength {
    pub(crate) fn px(px: f64) -> Self {
        Self { px, ..Self::default() }
    }

    /// `fraction` is `0.5` for `50%`.
    pub(crate) fn percent(fraction: f64) -> Self {
        Self { percent: fraction, ..Self::default() }
    }

    fn terms(&self) -> [f64; 8] {
        [self.px, self.percent, self.em, self.rem, self.vw, self.vh, self.vmin, self.vmax]
    }

    fn from_terms(terms: [f64; 8]) -> Self {
        Self {
            px: terms[0],
            percent: terms[1],
            em: terms[2],
            rem: terms[3],
            vw: terms[4],
            vh: terms[5],
            vmin: terms[6],
            vmax: terms[7],
        }
    }

    pub(crate) fn plus(self, other: Self) -> Self {
        let mut terms = self.terms();
        for (term, addend) in terms.iter_mut().zip(other.terms()) {
            *term += addend;
        }
        Self::from_terms(terms)
    }

    pub(crate) fn minus(self, other: Self) -> Self {
        self.plus(other.map(|term| -term))
    }

    pub(crate) fn map(self, apply: impl FnMut(f64) -> f64) -> Self {
        Self::from_terms(self.terms().map(apply))
    }

    /// Every term clamped to [`MAX_LENGTH`], the way a lone px length is.
    pub(crate) fn clamped(self) -> Self {
        self.map(|term| term.clamp(-MAX_LENGTH, MAX_LENGTH))
    }

    pub(crate) fn is_finite(&self) -> bool {
        self.terms().iter().all(|term| term.is_finite())
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.terms().iter().all(|term| *term == 0.0)
    }

    pub(crate) fn has_percent(&self) -> bool {
        self.percent != 0.0
    }

    /// The px, when no other term contributes.
    pub(crate) fn as_px(&self) -> Option<f64> {
        self.terms()[1..].iter().all(|term| *term == 0.0).then_some(self.px)
    }

    /// The percentage as a fraction, when no other term contributes.
    pub(crate) fn as_percent(&self) -> Option<f64> {
        (self.px == 0.0 && self.terms()[2..].iter().all(|term| *term == 0.0)).then_some(self.percent)
    }
}

/// What a length needs besides itself to become px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LengthContext {
    pub(crate) font_size: f64,
    pub(crate) root_font_size: f64,
    pub(crate) viewport_width: f64,
    pub(crate) viewport_height: f64,
}

impl LengthContext {
    /// `percent_basis` is the px a `100%` stands for.
    pub(crate) fn resolve(&self, length: &LinearLength, percent_basis: f64) -> f64 {
        let (min_side, max_side) = if self.viewport_width < self.viewport_height {
            (self.viewport_width, self.viewport_height)
        } else {
            (self.viewport_height, self.viewport_width)
        };
        length.px
            + length.percent * percent_basis
            + length.em * self.font_size
            + length.rem * self.root_font_size
            + length.vw * self.viewport_width
            + length.vh * self.viewport_height
            + length.vmin * min_side
            + length.vmax * max_side
    }

    /// The px of a length that does not depend on a containing block.
    pub(crate) fn resolve_definite(&self, length: &LinearLength) -> Option<f64> {
        (!length.has_percent()).then(|| self.resolve(length, 0.0))
    }

    /// Everything that is px once the context is known, and the percent
    /// fraction that still needs a containing block.
    pub(crate) fn split(&self, length: &LinearLength) -> (f64, f64) {
        (self.resolve(&LinearLength { percent: 0.0, ..*length }, 0.0), length.percent)
    }
}

impl fmt::Display for LinearLength {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let terms: Vec<(f64, &str)> = [
            (self.percent * 100.0, "%"),
            (self.em, "em"),
            (self.rem, "rem"),
            (self.vw * 100.0, "vw"),
            (self.vh * 100.0, "vh"),
            (self.vmin * 100.0, "vmin"),
            (self.vmax * 100.0, "vmax"),
            (self.px, "px"),
        ]
        .into_iter()
        .filter(|(value, _)| *value != 0.0)
        .collect();
        match terms.as_slice() {
            [] => write!(formatter, "0px"),
            [(value, unit)] => write!(formatter, "{}{unit}", tidy(*value)),
            [(first, first_unit), rest @ ..] => {
                write!(formatter, "calc({}{first_unit}", tidy(*first))?;
                for (value, unit) in rest {
                    let sign = if *value < 0.0 { '-' } else { '+' };
                    write!(formatter, " {sign} {}{unit}", tidy(value.abs()))?;
                }
                write!(formatter, ")")
            }
        }
    }
}

/// Drops the float noise `fraction * 100` leaves (`0.07 * 100` is
/// `7.000000000000001`) so the text reads as it was written.
fn tidy(value: f64) -> f64 {
    (value * 1e9).round() / 1e9
}

impl Serialize for LinearLength {
    /// A number when the length is plain px, which is what the style accepted
    /// before lengths could mix units; otherwise its CSS text.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.as_px() {
            Some(px) => serializer.serialize_f64(px),
            None => serializer.collect_str(self),
        }
    }
}

impl<'de> Deserialize<'de> for LinearLength {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct LengthVisitor;

        impl Visitor<'_> for LengthVisitor {
            type Value = LinearLength;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a number of px or a CSS length string")
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> Result<LinearLength, E> {
                if !value.is_finite() {
                    return Err(E::custom(format!("invalid length {value}: not a finite number")));
                }
                Ok(LinearLength::px(value).clamped())
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<LinearLength, E> {
                self.visit_f64(value as f64)
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<LinearLength, E> {
                self.visit_f64(value as f64)
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<LinearLength, E> {
                parse_linear_length(value).ok_or_else(|| E::custom(format!("invalid length \"{value}\"")))
            }
        }

        deserializer.deserialize_any(LengthVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> LinearLength {
        parse_linear_length(text).unwrap_or_else(|| panic!("{text} should parse"))
    }

    #[test]
    fn as_px_and_as_percent_need_every_other_term_to_be_zero() {
        assert_eq!(LinearLength::px(12.0).as_px(), Some(12.0));
        assert_eq!(LinearLength::px(12.0).as_percent(), None);
        assert_eq!(LinearLength::percent(0.5).as_percent(), Some(0.5));
        assert_eq!(LinearLength::percent(0.5).as_px(), None);
        assert_eq!(parse("calc(100% - 20px)").as_px(), None);
        assert_eq!(parse("calc(100% - 20px)").as_percent(), None);
        assert_eq!(parse("2em").as_px(), None);
        assert_eq!(LinearLength::default().as_px(), Some(0.0));
        assert!(LinearLength::default().is_zero());
        assert!(!LinearLength::px(1.0).is_zero());
        assert!(parse("calc(100% - 20px)").has_percent());
        assert!(!parse("20px").has_percent());
    }

    #[test]
    fn displays_as_css_that_parses_back_to_the_same_length() {
        assert_eq!(LinearLength::px(12.0).to_string(), "12px");
        assert_eq!(LinearLength::percent(0.5).to_string(), "50%");
        assert_eq!(LinearLength::default().to_string(), "0px");
        assert_eq!(parse("calc(100% - 20px)").to_string(), "calc(100% - 20px)");
        assert_eq!(parse("calc(1em + 2rem - 3vw)").to_string(), "calc(1em + 2rem - 3vw)");
        assert_eq!(parse("-20px").to_string(), "-20px");
        for text in ["calc(100% - 20px)", "calc(2 * (1em + 4px))", "calc(7% + 0.5vmin - 3vmax)", "-1.5rem", "calc(1vh - 1em)"] {
            let length = parse(text);
            assert_eq!(parse(&length.to_string()), length, "{text}");
        }
    }

    #[test]
    fn serializes_plain_px_as_a_number_and_anything_else_as_css() {
        assert_eq!(serde_json::to_string(&LinearLength::px(12.5)).unwrap(), "12.5");
        assert_eq!(serde_json::to_string(&parse("calc(100% - 20px)")).unwrap(), "\"calc(100% - 20px)\"");
        assert_eq!(serde_json::to_string(&parse("50%")).unwrap(), "\"50%\"");
    }

    #[test]
    fn deserializes_a_number_as_px_and_a_string_as_css() {
        let read = |json: &str| serde_json::from_str::<LinearLength>(json);
        assert_eq!(read("12").unwrap(), LinearLength::px(12.0));
        assert_eq!(read("-3.5").unwrap(), LinearLength::px(-3.5));
        assert_eq!(read("\"2in\"").unwrap(), LinearLength::px(192.0));
        assert_eq!(read("\"0\"").unwrap(), LinearLength::px(0.0));
        assert_eq!(read("\"calc(100% - 20px)\"").unwrap(), parse("calc(100% - 20px)"));
        assert_eq!(read("1e300").unwrap(), LinearLength::px(MAX_LENGTH));
    }

    #[test]
    fn deserialize_errors_name_the_bad_value() {
        for json in ["\"12\"", "\"12 px\"", "\"auto\"", "\"calc(1px + 2)\"", "true", "null", "[1]"] {
            assert!(serde_json::from_str::<LinearLength>(json).is_err(), "{json}");
        }
        let error = serde_json::from_str::<LinearLength>("\"12\"").unwrap_err().to_string();
        assert!(error.contains("invalid length \"12\""), "{error}");
    }

    #[test]
    fn round_trips_through_serde() {
        for length in [LinearLength::px(8.0), LinearLength::percent(0.25), parse("calc(50vmin + 1em)"), parse("calc(100% / 3 + 2px)")] {
            let json = serde_json::to_string(&length).unwrap();
            let back: LinearLength = serde_json::from_str(&json).unwrap();
            assert_eq!(back.to_string(), length.to_string(), "{json}");
            assert!((back.percent - length.percent).abs() < 1e-9, "{json}");
            assert!((back.vmin - length.vmin).abs() < 1e-9, "{json}");
        }
    }

    const CONTEXT: LengthContext =
        LengthContext { font_size: 20.0, root_font_size: 16.0, viewport_width: 800.0, viewport_height: 600.0 };

    #[test]
    fn resolve_adds_every_term_in_px() {
        assert_eq!(CONTEXT.resolve(&parse("calc(50vmin + 1em)"), 0.0), 320.0);
        assert_eq!(CONTEXT.resolve(&parse("calc(50vmax + 1rem)"), 0.0), 416.0);
        assert_eq!(CONTEXT.resolve(&parse("calc(10vw + 10vh)"), 0.0), 140.0);
        assert_eq!(CONTEXT.resolve(&parse("calc(50% + 2em)"), 200.0), 140.0);
        assert_eq!(CONTEXT.resolve(&parse("10px"), 999.0), 10.0);
    }

    #[test]
    fn vmin_and_vmax_follow_the_smaller_and_larger_side() {
        let portrait = LengthContext { viewport_width: 300.0, viewport_height: 500.0, ..CONTEXT };
        assert_eq!(portrait.resolve(&parse("10vmin"), 0.0), 30.0);
        assert_eq!(portrait.resolve(&parse("10vmax"), 0.0), 50.0);
        assert_eq!(CONTEXT.resolve(&parse("10vmin"), 0.0), 60.0);
        assert_eq!(CONTEXT.resolve(&parse("10vmax"), 0.0), 80.0);
    }

    #[test]
    fn resolve_definite_is_none_while_a_percent_remains() {
        assert_eq!(CONTEXT.resolve_definite(&parse("calc(50vmin + 1em)")), Some(320.0));
        assert_eq!(CONTEXT.resolve_definite(&parse("calc(100% - 20px)")), None);
        assert_eq!(CONTEXT.resolve_definite(&parse("calc(0% + 20px)")), Some(20.0));
    }

    #[test]
    fn split_separates_the_percent_from_everything_that_is_px() {
        assert_eq!(CONTEXT.split(&parse("calc(100% - 20px)")), (-20.0, 1.0));
        assert_eq!(CONTEXT.split(&parse("calc(50% + 1em + 10vw)")), (100.0, 0.5));
        assert_eq!(CONTEXT.split(&parse("12px")), (12.0, 0.0));
    }
}
