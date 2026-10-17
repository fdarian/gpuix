//! `flex` and `flex-flow`.
//!
//! Each is kept as its own style field holding the longhands it sets, so a state style
//! that sets it still replaces the longhands it covers and an explicit longhand still
//! beats it. A consumer reads a longhand through the `effective_*` methods of
//! [`StyleDesc`], which fall back to the shorthand.

use std::fmt;

use serde::{Deserializer, Serialize, Serializer};

use super::dimension::parse_number;
use super::shorthand::deserialize_optional;
use super::syntax::split_whitespace;
use crate::style::{DimensionValue, StyleDesc};

/// `flex`: the grow factor, the shrink factor and the basis, which a shorthand that
/// leaves one out has already filled in.
#[derive(Debug, Clone, PartialEq)]
pub struct Flex {
    pub grow: f64,
    pub shrink: f64,
    pub basis: DimensionValue,
}

impl Flex {
    /// `flex: none`.
    const NONE: Self = Self { grow: 0.0, shrink: 0.0, basis: DimensionValue::Auto };
    /// `flex: auto`.
    const AUTO: Self = Self { grow: 1.0, shrink: 1.0, basis: DimensionValue::Auto };
    /// `flex: initial`, which is also what an item has when `flex` is not set.
    const INITIAL: Self = Self { grow: 0.0, shrink: 1.0, basis: DimensionValue::Auto };

    /// A lone number `n`: `n 1 0%`.
    fn grow_with_zero_basis(grow: f64) -> Self {
        Self { grow, shrink: 1.0, basis: DimensionValue::Percentage(0.0) }
    }

    /// The CSS text of a flex factor: a number of the style's own, never `NaN`.
    fn factor(token: &str) -> Option<f64> {
        parse_number(token).filter(|factor| factor.is_finite() && *factor >= 0.0)
    }

    /// A `<'flex-basis'>`. A bare number is not one, except `0`, which is the one number a
    /// length can be written as.
    fn basis(token: &str) -> Option<DimensionValue> {
        if token == "0" {
            return Some(DimensionValue::Pixels(0.0));
        }
        if parse_number(token).is_some() {
            return None;
        }
        let basis = DimensionValue::parse(token).ok()?;
        let negative = match &basis {
            DimensionValue::Pixels(pixels) => *pixels < 0.0,
            DimensionValue::Percentage(fraction) => *fraction < 0.0,
            DimensionValue::Calc(_)
            | DimensionValue::Auto
            | DimensionValue::Intrinsic(_)
            | DimensionValue::AnchorSize(_) => false,
        };
        (!negative).then_some(basis)
    }

    /// `none | auto | initial | [ <grow> <shrink>? || <basis> ]`, and a lone number,
    /// which is a grow factor with a zero basis and not a basis.
    fn parse(text: &str) -> Result<Self, String> {
        let invalid = |reason: &str| format!("invalid flex \"{text}\": {reason}");
        let lowered = text.to_ascii_lowercase();
        let tokens = split_whitespace(&lowered);
        let rule = "expected none, auto, initial, or a grow factor, a shrink factor and a basis";
        let flex = match tokens.as_slice() {
            [] => return Err(invalid(rule)),
            ["none"] => Self::NONE,
            ["auto"] => Self::AUTO,
            ["initial"] => Self::INITIAL,
            [token] => match (Self::factor(token), Self::basis(token)) {
                (Some(grow), _) => Self::grow_with_zero_basis(grow),
                (None, Some(basis)) => Self { grow: 1.0, shrink: 1.0, basis },
                (None, None) => return Err(invalid(rule)),
            },
            [first, second] => match (Self::factor(first), Self::factor(second)) {
                (Some(grow), Some(shrink)) => Self { grow, shrink, basis: DimensionValue::Percentage(0.0) },
                _ => match (Self::factor(first), Self::basis(second), Self::basis(first), Self::factor(second)) {
                    (Some(grow), Some(basis), _, _) | (None, _, Some(basis), Some(grow)) => {
                        Self { grow, shrink: 1.0, basis }
                    }
                    _ => return Err(invalid(rule)),
                },
            },
            [first, second, third] => match (Self::factor(first), Self::factor(second), Self::basis(third)) {
                (Some(grow), Some(shrink), Some(basis)) => Self { grow, shrink, basis },
                _ => match (Self::basis(first), Self::factor(second), Self::factor(third)) {
                    (Some(basis), Some(grow), Some(shrink)) if Self::factor(first).is_none() => {
                        Self { grow, shrink, basis }
                    }
                    _ => return Err(invalid(rule)),
                },
            },
            _ => return Err(invalid(rule)),
        };
        Ok(flex)
    }
}

impl fmt::Display for Flex {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {} {}", self.grow, self.shrink, self.basis)
    }
}

impl Serialize for Flex {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// `flex`: a number, which is `<n> 1 0%` (React DOM sends a numeric `flex` unitless), or
/// the CSS string.
pub(crate) fn deserialize_flex<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Flex>, D::Error> {
    deserialize_optional(
        deserializer,
        "a number or a CSS flex value such as '1 1 0%', 'auto' or 'none'",
        |number| {
            if number.is_finite() && number >= 0.0 {
                Ok(Flex::grow_with_zero_basis(number))
            } else {
                Err(format!("invalid flex {number}: expected a non-negative number"))
            }
        },
        Flex::parse,
    )
}

/// `flex-flow`: the direction and the wrap, each at its initial value when the shorthand
/// leaves it out.
#[derive(Debug, Clone, PartialEq)]
pub struct FlexFlow {
    pub direction: String,
    pub wrap: String,
}

const DIRECTIONS: [&str; 4] = ["row", "row-reverse", "column", "column-reverse"];
const WRAPS: [&str; 3] = ["nowrap", "wrap", "wrap-reverse"];

impl FlexFlow {
    /// `<'flex-direction'> || <'flex-wrap'>`.
    fn parse(text: &str) -> Result<Self, String> {
        let invalid = || format!("invalid flex-flow \"{text}\": expected a direction, a wrap, or both in either order");
        let lowered = text.to_ascii_lowercase();
        let mut direction = None;
        let mut wrap = None;
        let tokens = split_whitespace(&lowered);
        if tokens.is_empty() {
            return Err(invalid());
        }
        for token in tokens {
            let slot = if DIRECTIONS.contains(&token) {
                &mut direction
            } else if WRAPS.contains(&token) {
                &mut wrap
            } else {
                return Err(invalid());
            };
            if slot.replace(token.to_string()).is_some() {
                return Err(invalid());
            }
        }
        Ok(Self {
            direction: direction.unwrap_or_else(|| "row".to_string()),
            wrap: wrap.unwrap_or_else(|| "nowrap".to_string()),
        })
    }
}

impl Serialize for FlexFlow {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!("{} {}", self.direction, self.wrap))
    }
}

pub(crate) fn deserialize_flex_flow<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<FlexFlow>, D::Error> {
    deserialize_optional(
        deserializer,
        "a CSS flex-flow value such as 'column wrap'",
        |number| Err(format!("invalid flex-flow {number}: expected a string")),
        FlexFlow::parse,
    )
}

/// What the shorthands fill in for a longhand the style leaves unset.
impl StyleDesc {
    pub(crate) fn effective_flex_grow(&self) -> Option<f64> {
        self.flex_grow.or_else(|| self.flex.as_ref().map(|flex| flex.grow))
    }

    pub(crate) fn effective_flex_shrink(&self) -> Option<f64> {
        self.flex_shrink.or_else(|| self.flex.as_ref().map(|flex| flex.shrink))
    }

    pub(crate) fn effective_flex_basis(&self) -> Option<&DimensionValue> {
        self.flex_basis.as_ref().or_else(|| self.flex.as_ref().map(|flex| &flex.basis))
    }

    pub(crate) fn effective_flex_direction(&self) -> Option<&str> {
        self.flex_direction
            .as_deref()
            .or_else(|| self.flex_flow.as_ref().map(|flow| flow.direction.as_str()))
    }

    pub(crate) fn effective_flex_wrap(&self) -> Option<&str> {
        self.flex_wrap.as_deref().or_else(|| self.flex_flow.as_ref().map(|flow| flow.wrap.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flex(text: &str) -> Flex {
        Flex::parse(text).unwrap_or_else(|error| panic!("{text}: {error}"))
    }

    fn parts(text: &str) -> (f64, f64, String) {
        let flex = flex(text);
        (flex.grow, flex.shrink, flex.basis.to_string())
    }

    #[test]
    fn the_keywords_are_their_expansions() {
        assert_eq!(parts("none"), (0.0, 0.0, "auto".to_string()));
        assert_eq!(parts("auto"), (1.0, 1.0, "auto".to_string()));
        assert_eq!(parts("initial"), (0.0, 1.0, "auto".to_string()));
        assert_eq!(parts("  NONE "), parts("none"));
    }

    #[test]
    fn a_lone_number_is_a_grow_factor_with_a_zero_basis() {
        assert_eq!(parts("1"), (1.0, 1.0, "0%".to_string()));
        assert_eq!(parts("2.5"), (2.5, 1.0, "0%".to_string()));
        assert_eq!(parts("0"), (0.0, 1.0, "0%".to_string()));
    }

    #[test]
    fn a_lone_basis_is_a_basis_with_grow_and_shrink_of_one() {
        assert_eq!(parts("100px"), (1.0, 1.0, "100px".to_string()));
        assert_eq!(parts("50%"), (1.0, 1.0, "50%".to_string()));
        assert_eq!(parts("2em").2, "2em");
        assert_eq!(parts("calc(100% - 20px)").0, 1.0);
        assert_eq!(flex("fit-content").basis, DimensionValue::parse("fit-content").unwrap());
        assert_eq!(flex("min-content").basis, DimensionValue::parse("min-content").unwrap());
    }

    #[test]
    fn two_numbers_are_grow_and_shrink_over_a_zero_basis() {
        assert_eq!(parts("2 3"), (2.0, 3.0, "0%".to_string()));
        assert_eq!(parts("1 0"), (1.0, 0.0, "0%".to_string()));
    }

    #[test]
    fn a_number_and_a_basis_in_either_order() {
        assert_eq!(parts("2 100px"), (2.0, 1.0, "100px".to_string()));
        assert_eq!(parts("100px 2"), (2.0, 1.0, "100px".to_string()));
        assert_eq!(parts("1 auto"), (1.0, 1.0, "auto".to_string()));
        assert_eq!(parts("auto 3"), (3.0, 1.0, "auto".to_string()));
        assert_eq!(parts("0 calc(10px + 1em)").0, 0.0);
    }

    #[test]
    fn three_values_are_grow_shrink_basis_or_basis_grow_shrink() {
        assert_eq!(parts("1 2 30px"), (1.0, 2.0, "30px".to_string()));
        assert_eq!(parts("30px 1 2"), (1.0, 2.0, "30px".to_string()));
        assert_eq!(parts("0 0 auto"), (0.0, 0.0, "auto".to_string()));
        assert_eq!(parts("1 1 calc(50% - 4px)").0, 1.0);
    }

    #[test]
    fn a_unitless_zero_is_a_factor_unless_two_factors_come_before_it() {
        assert_eq!(parts("1 0"), (1.0, 0.0, "0%".to_string()));
        assert_eq!(parts("1 1 0"), (1.0, 1.0, "0px".to_string()));
        assert_eq!(parts("0 0 0"), (0.0, 0.0, "0px".to_string()));
    }

    #[test]
    fn what_chrome_rejects_is_an_error() {
        for text in [
            "", "   ", "-1", "1 -1", "1 2 3", "0 1 1", "1 30px 2", "1 auto 2", "none 1", "auto auto", "initial 1",
            "1 2 3 4", "bogus", "1 1 content", "-10px", "1 -10px", "1,2", "inherit",
        ] {
            assert!(Flex::parse(text).is_err(), "{text:?} should be invalid");
        }
        assert!(Flex::parse("1 2 30px").is_ok());
    }

    #[test]
    fn a_number_from_json_is_unitless_like_react_dom() {
        #[derive(serde::Deserialize)]
        struct Holder {
            #[serde(default, deserialize_with = "deserialize_flex")]
            flex: Option<Flex>,
        }
        let holder = |json: &str| serde_json::from_str::<Holder>(json);
        let one = holder(r#"{"flex": 1}"#).unwrap().flex.unwrap();
        assert_eq!((one.grow, one.shrink, one.basis), (1.0, 1.0, DimensionValue::Percentage(0.0)));
        let zero = holder(r#"{"flex": 0}"#).unwrap().flex.unwrap();
        assert_eq!((zero.grow, zero.shrink), (0.0, 1.0));
        assert_eq!(holder(r#"{"flex": null}"#).unwrap().flex, None);
        assert_eq!(holder("{}").unwrap().flex, None);
        assert!(holder(r#"{"flex": -1}"#).is_err());
        let error = holder(r#"{"flex": "1 2 3"}"#).err().unwrap().to_string();
        assert!(error.contains("invalid flex \"1 2 3\""), "{error}");
    }

    #[test]
    fn serializes_as_the_three_longhands_and_parses_back() {
        for text in ["none", "auto", "1", "2 100px", "30px 1 2", "1 1 calc(50% - 4px)", "1 1 fit-content"] {
            let flex = flex(text);
            let json = serde_json::to_string(&flex).unwrap();
            let again = Flex::parse(json.trim_matches('"')).unwrap();
            assert_eq!(again, flex, "{text}: {json}");
        }
    }

    fn flow(text: &str) -> Result<(String, String), String> {
        FlexFlow::parse(text).map(|flow| (flow.direction, flow.wrap))
    }

    #[test]
    fn flex_flow_is_a_direction_and_a_wrap_in_either_order() {
        let pair = |direction: &str, wrap: &str| Ok((direction.to_string(), wrap.to_string()));
        assert_eq!(flow("column wrap"), pair("column", "wrap"));
        assert_eq!(flow("wrap column"), pair("column", "wrap"));
        assert_eq!(flow("row-reverse wrap-reverse"), pair("row-reverse", "wrap-reverse"));
        assert_eq!(flow("COLUMN"), pair("column", "nowrap"));
        assert_eq!(flow("wrap"), pair("row", "wrap"));
        assert_eq!(flow("nowrap"), pair("row", "nowrap"));
    }

    #[test]
    fn flex_flow_rejects_what_chrome_does() {
        for text in ["", "row row", "column column-reverse", "wrap nowrap", "wrap wrap", "sideways", "column wrap wrap"] {
            assert!(flow(text).is_err(), "{text:?} should be invalid");
        }
    }

    #[test]
    fn the_shorthand_fills_in_what_a_longhand_leaves_unset() {
        let style: StyleDesc = serde_json::from_str(
            r#"{"flex": "2 3 40px", "flexShrink": 0, "flexFlow": "column wrap", "flexWrap": "nowrap"}"#,
        )
        .unwrap();
        assert_eq!(style.effective_flex_grow(), Some(2.0));
        assert_eq!(style.effective_flex_shrink(), Some(0.0));
        assert_eq!(style.effective_flex_basis(), Some(&DimensionValue::Pixels(40.0)));
        assert_eq!(style.effective_flex_direction(), Some("column"));
        assert_eq!(style.effective_flex_wrap(), Some("nowrap"));
        let none = StyleDesc::default();
        assert_eq!(none.effective_flex_grow(), None);
        assert_eq!(none.effective_flex_direction(), None);
    }
}
