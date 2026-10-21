//! `contain-intrinsic-size` and its longhands (CSS Sizing 4): the size an element with size
//! containment has in place of the one its content would give it.
//!
//! One axis is `none`, a non-negative length, or `auto` followed by either: `auto` says to use the
//! size the element was last laid out at, when it was not skipped, and the rest when it was never
//! laid out. A bare number is px, like every length of a style object.

use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::dimension::parse_definite_length;
use super::length::{LengthContext, LinearLength};

/// The value of one axis. The default is `none`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct IntrinsicAxis {
    /// `auto <length>` or `auto none`.
    pub auto: bool,
    /// `None` is `none`, which is a size of 0.
    pub length: Option<LinearLength>,
}

const RULE: &str = "`none`, a non-negative length or `auto` followed by one of them";

impl IntrinsicAxis {
    /// The size in px the axis falls back to when there is no remembered size: 0 for `none`.
    pub(crate) fn fallback(&self, lengths: &LengthContext) -> f64 {
        self.length.map_or(0.0, |length| lengths.resolve(&length, 0.0).max(0.0))
    }

    /// The content-box size the axis gives a box that is contained in it and is skipped, given the
    /// size it was last laid out at (when it has one). `None` leaves the box at the size of an
    /// empty one.
    pub(crate) fn skipped_size(&self, remembered: Option<f64>, lengths: &LengthContext) -> Option<f64> {
        match (self.auto, remembered, self.length) {
            (true, Some(remembered), _) => Some(remembered),
            (_, _, None) if !self.auto => None,
            _ => Some(self.fallback(lengths)),
        }
    }

    fn parse_value(first: &str, second: Option<&str>) -> Option<Self> {
        let (auto, token) = match second {
            Some(second) if first.eq_ignore_ascii_case("auto") => (true, second),
            Some(_) => return None,
            None => (false, first),
        };
        if token.eq_ignore_ascii_case("none") {
            return Some(Self { auto, length: None });
        }
        let length = parse_definite_length(token).filter(|length| !length.is_negative())?;
        Some(Self { auto, length: Some(length) })
    }

    fn parse(text: &str) -> Option<Self> {
        let axes = parse_axes(text)?;
        match axes.as_slice() {
            [axis] => Some(*axis),
            _ => None,
        }
    }

    fn px(px: f64) -> Option<Self> {
        (px.is_finite() && px >= 0.0).then(|| Self { auto: false, length: Some(LinearLength::px(px).clamped()) })
    }
}

impl fmt::Display for IntrinsicAxis {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.auto {
            write!(formatter, "auto ")?;
        }
        match &self.length {
            Some(length) => write!(formatter, "{length}"),
            None => write!(formatter, "none"),
        }
    }
}

/// The axes written in `text`: one or two, each `none`, a length, or `auto` and one of those.
fn parse_axes(text: &str) -> Option<Vec<IntrinsicAxis>> {
    let mut tokens = text.split_whitespace().peekable();
    let mut axes = Vec::new();
    while let Some(first) = tokens.next() {
        let axis = if first.eq_ignore_ascii_case("auto") {
            IntrinsicAxis::parse_value(first, Some(tokens.next()?))?
        } else {
            IntrinsicAxis::parse_value(first, None)?
        };
        axes.push(axis);
    }
    (1..=2).contains(&axes.len()).then_some(axes)
}

/// `contain-intrinsic-size`: the width, then the height. One value is both.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct IntrinsicSize {
    pub width: IntrinsicAxis,
    pub height: IntrinsicAxis,
}

impl IntrinsicSize {
    fn parse(text: &str) -> Option<Self> {
        match parse_axes(text)?.as_slice() {
            [both] => Some(Self { width: *both, height: *both }),
            [width, height] => Some(Self { width: *width, height: *height }),
            _ => None,
        }
    }
}

/// What the style says of the axes, the longhands over the shorthand. The logical longhands are
/// the physical ones: the writing mode is horizontal.
pub(crate) fn axes(style: &crate::style::StyleDesc) -> IntrinsicSize {
    let shorthand = style.contain_intrinsic_size.unwrap_or_default();
    IntrinsicSize {
        width: style
            .contain_intrinsic_width
            .or(style.contain_intrinsic_inline_size)
            .unwrap_or(shorthand.width),
        height: style
            .contain_intrinsic_height
            .or(style.contain_intrinsic_block_size)
            .unwrap_or(shorthand.height),
    }
}

impl Serialize for IntrinsicAxis {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.length.as_ref().and_then(LinearLength::as_px) {
            Some(px) if !self.auto => serializer.serialize_f64(px),
            _ => serializer.collect_str(self),
        }
    }
}

impl Serialize for IntrinsicSize {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.width == self.height {
            self.width.serialize(serializer)
        } else {
            serializer.collect_str(&format_args!("{} {}", self.width, self.height))
        }
    }
}

macro_rules! deserialize_number_or_text {
    ($type:ty, $name:literal, $number:expr, $text:expr) => {
        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct Reader;

                impl Visitor<'_> for Reader {
                    type Value = $type;

                    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                        write!(formatter, "a number of px or {RULE}")
                    }

                    fn visit_f64<E: de::Error>(self, value: f64) -> Result<$type, E> {
                        $number(value).ok_or_else(|| E::custom(format!("invalid {} {value}: {RULE}", $name)))
                    }

                    fn visit_i64<E: de::Error>(self, value: i64) -> Result<$type, E> {
                        self.visit_f64(value as f64)
                    }

                    fn visit_u64<E: de::Error>(self, value: u64) -> Result<$type, E> {
                        self.visit_f64(value as f64)
                    }

                    fn visit_str<E: de::Error>(self, value: &str) -> Result<$type, E> {
                        $text(value).ok_or_else(|| E::custom(format!("invalid {} \"{value}\": {RULE}", $name)))
                    }
                }

                deserializer.deserialize_any(Reader)
            }
        }
    };
}

deserialize_number_or_text!(IntrinsicAxis, "containIntrinsicWidth", IntrinsicAxis::px, IntrinsicAxis::parse);
deserialize_number_or_text!(
    IntrinsicSize,
    "containIntrinsicSize",
    |value| IntrinsicAxis::px(value).map(|axis| IntrinsicSize { width: axis, height: axis }),
    IntrinsicSize::parse
);

#[cfg(test)]
mod tests {
    use super::*;

    fn axis(json: &str) -> Result<IntrinsicAxis, serde_json::Error> {
        serde_json::from_str(json)
    }

    fn size(json: &str) -> Result<IntrinsicSize, serde_json::Error> {
        serde_json::from_str(json)
    }

    fn px(value: f64) -> Option<LinearLength> {
        Some(LinearLength::px(value))
    }

    #[test]
    fn an_axis_is_none_a_length_or_auto_and_one_of_them() {
        assert_eq!(axis("\"none\"").unwrap(), IntrinsicAxis { auto: false, length: None });
        assert_eq!(axis("40").unwrap(), IntrinsicAxis { auto: false, length: px(40.0) });
        assert_eq!(axis("\"40px\"").unwrap(), IntrinsicAxis { auto: false, length: px(40.0) });
        assert_eq!(axis("\"auto 60px\"").unwrap(), IntrinsicAxis { auto: true, length: px(60.0) });
        assert_eq!(axis("\"AUTO none\"").unwrap(), IntrinsicAxis { auto: true, length: None });
        assert_eq!(axis("\"2em\"").unwrap().length.unwrap().em, 2.0);
        for bad in ["\"auto\"", "\"-1px\"", "\"10%\"", "\"1px 2px\"", "\"auto auto\"", "\"\"", "-4", "\"3\""] {
            assert!(axis(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_shorthand_is_the_width_and_then_the_height() {
        let both = size("\"30px\"").unwrap();
        assert_eq!((both.width, both.height), (axis("30").unwrap(), axis("30").unwrap()));
        let two = size("\"30px 50px\"").unwrap();
        assert_eq!((two.width.length, two.height.length), (px(30.0), px(50.0)));
        let mixed = size("\"auto 30px none\"").unwrap();
        assert!(mixed.width.auto && mixed.width.length == px(30.0));
        assert!(!mixed.height.auto && mixed.height.length.is_none());
        let autos = size("\"auto 10px auto 20px\"").unwrap();
        assert!(autos.width.auto && autos.height.auto);
        assert_eq!(size("12").unwrap().width.length, px(12.0));
        for bad in ["\"1px 2px 3px\"", "\"auto\"", "\"auto 1px 2px 3px\"", "true"] {
            assert!(size(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_value_survives_a_round_trip() {
        for json in ["\"auto 60px\"", "40", "\"none\"", "\"auto none\""] {
            let parsed = axis(json).unwrap();
            assert_eq!(axis(&serde_json::to_string(&parsed).unwrap()).unwrap(), parsed, "{json}");
        }
        for json in ["\"30px 50px\"", "\"auto 30px none\"", "7"] {
            let parsed = size(json).unwrap();
            assert_eq!(size(&serde_json::to_string(&parsed).unwrap()).unwrap(), parsed, "{json}");
        }
    }

    #[test]
    fn a_skipped_box_takes_the_remembered_size_only_for_auto() {
        let lengths = LengthContext::for_tests();
        let none = axis("\"none\"").unwrap();
        let fixed = axis("40").unwrap();
        let auto = axis("\"auto 60px\"").unwrap();
        let auto_none = axis("\"auto none\"").unwrap();
        assert_eq!(none.skipped_size(Some(90.0), &lengths), None);
        assert_eq!(fixed.skipped_size(Some(90.0), &lengths), Some(40.0));
        assert_eq!(auto.skipped_size(Some(90.0), &lengths), Some(90.0));
        assert_eq!(auto.skipped_size(None, &lengths), Some(60.0));
        assert_eq!(auto_none.skipped_size(None, &lengths), Some(0.0));
    }

    #[test]
    fn the_longhands_win_over_the_shorthand_and_the_physical_over_the_logical() {
        let style: crate::style::StyleDesc = serde_json::from_str(
            r#"{"containIntrinsicSize":"10px 20px","containIntrinsicBlockSize":30,"containIntrinsicInlineSize":40,"containIntrinsicWidth":50}"#,
        )
        .unwrap();
        let resolved = axes(&style);
        assert_eq!((resolved.width.length, resolved.height.length), (px(50.0), px(30.0)));
    }
}
