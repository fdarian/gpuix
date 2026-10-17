//! The logical box shorthands: `margin-block`, `margin-inline`, `padding-block`,
//! `padding-inline`, `inset-block` and `inset-inline`, which take one or two values for the
//! start and the end of an axis.
//!
//! gpuix lays out left to right and horizontally, so the block axis is `top` and `bottom` and
//! the inline axis `left` and `right`. Each is kept as its own style field, a rung between the
//! four-sided shorthand (`margin`) and the side longhands (`marginTop`), which win: that is
//! the order `*_below_longhands` layers them in. `border-block` and `border-inline` are
//! strings like `border`, and `border.rs` layers them between `border` and `borderTop`.

use std::fmt;

use serde::{Deserializer, Serialize, Serializer};

use super::dimension::parse_linear_length;
use super::length::LinearLength;
use super::shorthand::{deserialize_optional, length_token, parse_values, px_length, Sides};
use crate::style::{DimensionValue, StyleDesc, NOT_A_SIZE};

/// The start and the end of a logical axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pair<T> {
    pub start: T,
    pub end: T,
}

impl<T> Pair<T> {
    pub fn both(value: T) -> Self
    where
        T: Clone,
    {
        Self { start: value.clone(), end: value }
    }

    pub(crate) fn values(&self) -> [&T; 2] {
        [&self.start, &self.end]
    }

    /// `None` when `apply` gives none for either end.
    pub(crate) fn try_map<U>(self, mut apply: impl FnMut(T) -> Option<U>) -> Option<Pair<U>> {
        Some(Pair { start: apply(self.start)?, end: apply(self.end)? })
    }
}

/// A single value when both ends agree, otherwise both, which parse back to the same.
impl<T: Serialize + fmt::Display + PartialEq> Serialize for Pair<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.start == self.end {
            self.start.serialize(serializer)
        } else {
            serializer.collect_str(&format_args!("{} {}", self.start, self.end))
        }
    }
}

/// `text` as one or two whitespace-separated values: the one is both.
fn parse_pair<T: Clone>(text: &str, parse_one: impl FnMut(&str) -> Result<T, String>) -> Result<Pair<T>, String> {
    match &parse_values(text, 2, parse_one)?[..] {
        [both] => Ok(Pair::both(both.clone())),
        [start, end] => Ok(Pair { start: start.clone(), end: end.clone() }),
        _ => unreachable!("one or two values were checked"),
    }
}

/// `padding-block` and `padding-inline`: a number of px or one or two CSS lengths.
pub(crate) fn deserialize_padding_pair<'de, D: Deserializer<'de>>(
    deserializer: D,
    property: &'static str,
) -> Result<Option<Pair<LinearLength>>, D::Error> {
    let token = length_token(property, "a CSS length with a unit, or calc()", parse_linear_length);
    deserialize_optional(
        deserializer,
        "a number of px or a string of one or two CSS lengths",
        |value| px_length(value, property).map(Pair::both),
        |text| parse_pair(text, &token),
    )
}

/// `margin-block`, `margin-inline`, `inset-block` and `inset-inline`: a number of px or one or
/// two dimensions, each of which may be `auto`.
pub(crate) fn deserialize_dimension_pair<'de, D: Deserializer<'de>>(
    deserializer: D,
    property: &'static str,
) -> Result<Option<Pair<DimensionValue>>, D::Error> {
    let token = |token: &str| match DimensionValue::parse(token)? {
        DimensionValue::Intrinsic(_) => Err(format!("invalid {property} \"{token}\": {NOT_A_SIZE}")),
        value => Ok(value),
    };
    deserialize_optional(
        deserializer,
        "a number of px or a string of one or two CSS lengths or auto",
        |value| Ok(Pair::both(DimensionValue::Pixels(value))),
        |text| parse_pair(text, token),
    )
}

macro_rules! pair_deserializers {
    ($($name:ident: $function:ident($property:literal) -> $value:ty;)+) => {$(
        pub(crate) fn $name<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Pair<$value>>, D::Error> {
            $function(deserializer, $property)
        }
    )+};
}

pair_deserializers! {
    deserialize_margin_block: deserialize_dimension_pair("margin-block") -> DimensionValue;
    deserialize_margin_inline: deserialize_dimension_pair("margin-inline") -> DimensionValue;
    deserialize_inset_block: deserialize_dimension_pair("inset-block") -> DimensionValue;
    deserialize_inset_inline: deserialize_dimension_pair("inset-inline") -> DimensionValue;
    deserialize_padding_block: deserialize_padding_pair("padding-block") -> LinearLength;
    deserialize_padding_inline: deserialize_padding_pair("padding-inline") -> LinearLength;
}

/// What the four-sided shorthand and then the two logical ones give each side, the later
/// ones winning. The side longhands go on top of this.
fn layered<T: Clone>(
    all: &Option<Sides<T>>,
    block: &Option<Pair<T>>,
    inline: &Option<Pair<T>>,
) -> Sides<Option<T>> {
    let mut sides = match all {
        Some(all) => Sides {
            top: Some(all.top.clone()),
            right: Some(all.right.clone()),
            bottom: Some(all.bottom.clone()),
            left: Some(all.left.clone()),
        },
        None => Sides { top: None, right: None, bottom: None, left: None },
    };
    if let Some(block) = block {
        sides.top = Some(block.start.clone());
        sides.bottom = Some(block.end.clone());
    }
    if let Some(inline) = inline {
        sides.left = Some(inline.start.clone());
        sides.right = Some(inline.end.clone());
    }
    sides
}

impl StyleDesc {
    pub(crate) fn margin_below_longhands(&self) -> Sides<Option<DimensionValue>> {
        layered(&self.margin, &self.margin_block, &self.margin_inline)
    }

    pub(crate) fn padding_below_longhands(&self) -> Sides<Option<LinearLength>> {
        layered(&self.padding, &self.padding_block, &self.padding_inline)
    }

    pub(crate) fn inset_below_longhands(&self) -> Sides<Option<DimensionValue>> {
        layered(&self.inset, &self.inset_block, &self.inset_inline)
    }

    /// Whether the style sets `top` and `bottom` to a length or a percentage, by either name.
    pub(crate) fn has_both_vertical_insets(&self) -> bool {
        let inset = self.inset_below_longhands();
        let set = |own: &Option<DimensionValue>, shorthand: Option<DimensionValue>| {
            matches!(
                own.as_ref().or(shorthand.as_ref()),
                Some(DimensionValue::Pixels(_) | DimensionValue::Percentage(_))
            )
        };
        set(&self.top, inset.top) && set(&self.bottom, inset.bottom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn length(text: &str) -> LinearLength {
        parse_linear_length(text).unwrap_or_else(|| panic!("{text} should parse"))
    }

    fn pair(text: &str) -> Result<Pair<LinearLength>, String> {
        parse_pair(text, length_token("padding-block", "a length", parse_linear_length))
    }

    #[test]
    fn one_value_is_both_ends_and_two_are_start_then_end() {
        assert_eq!(pair("1px"), Ok(Pair::both(length("1px"))));
        assert_eq!(pair("1px 2em"), Ok(Pair { start: length("1px"), end: length("2em") }));
        assert_eq!(pair("  calc(1px + 1em)  2px "), Ok(Pair { start: length("calc(1px + 1em)"), end: length("2px") }));
    }

    #[test]
    fn none_or_more_than_two_values_or_a_bad_one_is_an_error() {
        for text in ["", "1px 2px 3px", "1px 12", "1px auto"] {
            assert!(pair(text).is_err(), "{text:?}");
        }
        assert!(pair("1px 2px 3px").unwrap_err().contains("one or two values"));
    }

    #[test]
    fn serializes_as_one_value_when_both_ends_agree() {
        let json = |pair: &Pair<LinearLength>| serde_json::to_string(pair).unwrap();
        assert_eq!(json(&Pair::both(length("4px"))), "4.0");
        assert_eq!(json(&Pair { start: length("1px"), end: length("2em") }), "\"1px 2em\"");
    }

    #[test]
    fn the_logical_shorthands_sit_between_the_four_sided_one_and_the_side_longhands() {
        let style: StyleDesc = serde_json::from_str(
            r#"{"margin": "1px 2px 3px 4px", "marginInline": "10px 20px", "marginBlock": 5, "marginTop": 7,
                "padding": 8, "paddingInline": "1px 2px",
                "inset": "0 5px", "insetBlock": "auto 9px"}"#,
        )
        .unwrap();
        let margin = style.margin_below_longhands();
        let px = |value: f64| Some(DimensionValue::Pixels(value));
        assert_eq!((margin.top, margin.right, margin.bottom, margin.left), (px(5.0), px(20.0), px(5.0), px(10.0)));
        assert_eq!(style.margin_top, px(7.0), "a side longhand is read apart and wins");
        let padding = style.padding_below_longhands();
        assert_eq!(
            (padding.top, padding.right, padding.bottom, padding.left),
            (Some(length("8px")), Some(length("2px")), Some(length("8px")), Some(length("1px")))
        );
        let inset = style.inset_below_longhands();
        assert_eq!(
            (inset.top, inset.right, inset.bottom, inset.left),
            (Some(DimensionValue::Auto), px(5.0), px(9.0), px(5.0))
        );
        assert_eq!(StyleDesc::default().margin_below_longhands(), Sides { top: None, right: None, bottom: None, left: None });
    }

    #[test]
    fn an_intrinsic_size_keyword_is_not_a_margin() {
        let error = serde_json::from_str::<StyleDesc>(r#"{"marginBlock": "0 max-content"}"#).unwrap_err().to_string();
        assert!(error.contains("invalid margin-block \"max-content\""), "{error}");
        assert!(serde_json::from_str::<StyleDesc>(r#"{"insetInline": "fit-content"}"#).is_err());
        assert!(serde_json::from_str::<StyleDesc>(r#"{"paddingBlock": "auto"}"#).is_err());
    }

    #[test]
    fn both_vertical_insets_by_either_name() {
        let both = |json: &str| serde_json::from_str::<StyleDesc>(json).unwrap().has_both_vertical_insets();
        assert!(both(r#"{"top": 0, "bottom": 0}"#));
        assert!(both(r#"{"inset": 0}"#));
        assert!(both(r#"{"insetBlock": "1px 2px"}"#));
        assert!(!both(r#"{"insetBlock": 0, "top": "auto", "bottom": 5}"#));
        assert!(!both(r#"{"insetInline": 0}"#));
        assert!(!both(r#"{"top": 0}"#));
    }
}
