//! The animatable attributes of an SVG shape: the second target of the motion
//! engine, next to `StyleDesc` properties.
//!
//! `pathLength`, `pathOffset` and `pathSpacing` are Framer's normalised draw
//! keys, not attributes. They are sampled like any other number and combined by
//! the serialiser, which knows the shape's real length.

use std::collections::HashMap;

use serde_json::Value;

use super::value::{AnimValue, Color};
use crate::color::ColorContext;
use crate::custom_elements::svg_path::{format_number, PathData};

#[derive(Clone, Copy)]
enum Kind {
    Number,
    Colour,
    Dashes,
    Path,
}

#[derive(Clone, Copy)]
enum Clamp {
    None,
    NonNegative,
    Unit,
    AtLeastOne,
}

#[derive(Clone, Copy)]
enum Initial {
    /// Only the element's own attribute can say, e.g. a circle's `r`.
    Unresolved,
    Number(f64),
    Black,
    Transparent,
}

/// What CSS computes a property to when nothing sets it, where that differs from
/// [`Initial`], the value `motion` starts from.
#[derive(Clone, Copy)]
enum CssInitial {
    Same,
    Length(f64),
    /// A keyword that no number, list or path blends with, so the property swaps
    /// halfway against one that has a value (`none`, `auto`).
    Keyword(&'static str),
    /// An inherited property reads the font above it.
    ParentFontSize,
}

macro_rules! attributes {
    ($($variant:ident $name:literal $attribute:literal $kind:ident $clamp:ident $initial:expr, $css:expr;)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub(super) enum SvgAttr {
            $($variant),*
        }

        impl SvgAttr {
            pub(super) fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)*
                    _ => None,
                }
            }

            pub(super) fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name),*
                }
            }

            /// The XML attribute this animates, or `None` for a draw key.
            pub(super) fn attribute(self) -> Option<&'static str> {
                match self {
                    $(Self::$variant => Some($attribute).filter(|attribute| !attribute.is_empty())),*
                }
            }

            fn kind(self) -> Kind {
                match self {
                    $(Self::$variant => Kind::$kind),*
                }
            }

            fn clamp_rule(self) -> Clamp {
                match self {
                    $(Self::$variant => Clamp::$clamp),*
                }
            }

            fn initial_rule(self) -> Initial {
                match self {
                    $(Self::$variant => $initial),*
                }
            }

            fn css_initial_rule(self) -> CssInitial {
                match self {
                    $(Self::$variant => $css),*
                }
            }
        }
    };
}

use CssInitial::{Keyword, Length, ParentFontSize, Same};
use Initial::{Black, Number, Transparent, Unresolved};

attributes! {
    PathLength "pathLength" "" Number NonNegative Number(1.0), Same;
    PathOffset "pathOffset" "" Number None Number(0.0), Same;
    PathSpacing "pathSpacing" "" Number NonNegative Number(1.0), Same;
    StrokeDasharray "strokeDasharray" "stroke-dasharray" Dashes None Unresolved, Keyword("none");
    StrokeDashoffset "strokeDashoffset" "stroke-dashoffset" Number None Number(0.0), Same;
    StrokeWidth "strokeWidth" "stroke-width" Number NonNegative Number(1.0), Same;
    Fill "fill" "fill" Colour None Black, Same;
    Stroke "stroke" "stroke" Colour None Transparent, Keyword("none");
    Opacity "opacity" "opacity" Number Unit Number(1.0), Same;
    FillOpacity "fillOpacity" "fill-opacity" Number Unit Number(1.0), Same;
    StrokeOpacity "strokeOpacity" "stroke-opacity" Number Unit Number(1.0), Same;
    Cx "cx" "cx" Number None Number(0.0), Same;
    Cy "cy" "cy" Number None Number(0.0), Same;
    R "r" "r" Number NonNegative Unresolved, Length(0.0);
    Rx "rx" "rx" Number NonNegative Unresolved, Keyword("auto");
    Ry "ry" "ry" Number NonNegative Unresolved, Keyword("auto");
    X "x" "x" Number None Number(0.0), Same;
    Y "y" "y" Number None Number(0.0), Same;
    Width "width" "width" Number NonNegative Unresolved, Keyword("auto");
    Height "height" "height" Number NonNegative Unresolved, Keyword("auto");
    X1 "x1" "x1" Number None Number(0.0), Same;
    Y1 "y1" "y1" Number None Number(0.0), Same;
    X2 "x2" "x2" Number None Number(0.0), Same;
    Y2 "y2" "y2" Number None Number(0.0), Same;
    D "d" "d" Path None Unresolved, Keyword("none");
    StdDeviation "stdDeviation" "stdDeviation" Number NonNegative Number(0.0), Same;
    Dx "dx" "dx" Number None Number(0.0), Same;
    Dy "dy" "dy" Number None Number(0.0), Same;
    FloodColor "floodColor" "flood-color" Colour None Black, Same;
    FloodOpacity "floodOpacity" "flood-opacity" Number Unit Number(1.0), Same;
    StopColor "stopColor" "stop-color" Colour None Black, Same;
    StopOpacity "stopOpacity" "stop-opacity" Number Unit Number(1.0), Same;
    StrokeMiterlimit "strokeMiterlimit" "stroke-miterlimit" Number AtLeastOne Number(4.0), Same;
    RefX "refX" "refX" Number None Number(0.0), Same;
    RefY "refY" "refY" Number None Number(0.0), Same;
    FontSize "fontSize" "font-size" Number NonNegative Unresolved, ParentFontSize;
    LetterSpacing "letterSpacing" "letter-spacing" Number None Number(0.0), Same;
}

impl SvgAttr {
    /// What the shape shows when its own attributes do not say.
    pub(super) fn initial(self) -> Option<AnimValue> {
        match self.initial_rule() {
            Initial::Unresolved => None,
            Initial::Number(value) => Some(AnimValue::Number(value)),
            Initial::Black => Some(AnimValue::Color(Color::from_rgba(
                gpui::Rgba {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
                true,
            ))),
            Initial::Transparent => Some(AnimValue::Color(Color::TRANSPARENT)),
        }
    }

    /// The value the element's own attribute declares, if it declares a usable
    /// one. A draw key is never an attribute.
    pub(super) fn read(
        self,
        attributes: &HashMap<String, Value>,
        colors: &ColorContext,
    ) -> Option<AnimValue> {
        self.attribute()?;
        self.parse(attributes.get(self.name())?, colors).ok()
    }

    /// Whether CSS has a property of this name that `@keyframes` can animate on
    /// an SVG element. The draw keys and the attributes that exist only in the
    /// markup (`x1`, `stdDeviation`, `dx`, `refX`...) are not CSS properties.
    pub(super) fn is_css_property(self) -> bool {
        !matches!(
            self,
            Self::PathLength
                | Self::PathOffset
                | Self::PathSpacing
                | Self::X1
                | Self::Y1
                | Self::X2
                | Self::Y2
                | Self::StdDeviation
                | Self::Dx
                | Self::Dy
                | Self::RefX
                | Self::RefY
        )
    }

    /// Whether an element that sets no value computes to its parent's.
    pub(super) fn inherits(self) -> bool {
        matches!(
            self,
            Self::StrokeDasharray
                | Self::StrokeDashoffset
                | Self::StrokeWidth
                | Self::Fill
                | Self::Stroke
                | Self::FillOpacity
                | Self::StrokeOpacity
                | Self::StrokeMiterlimit
                | Self::FontSize
                | Self::LetterSpacing
        )
    }

    /// What CSS computes the property to when nothing sets it, which is what a
    /// keyframe set's missing endpoint stands for. `font_size` is what an element
    /// with no `font-size` of its own inherits, in px.
    pub(super) fn initial_css(self, font_size: f64) -> Option<AnimValue> {
        match self.css_initial_rule() {
            CssInitial::Same => self.initial(),
            CssInitial::Length(value) => Some(AnimValue::Number(value)),
            CssInitial::Keyword(keyword) => Some(AnimValue::Keyword(Some(keyword.to_string()))),
            CssInitial::ParentFontSize => Some(AnimValue::Number(font_size)),
        }
    }

    /// [`read`](Self::read) for a CSS animation.
    pub(super) fn read_css(
        self,
        attributes: &HashMap<String, Value>,
        colors: &ColorContext,
    ) -> Option<AnimValue> {
        self.attribute()?;
        self.parse_css(attributes.get(self.name())?, colors).ok()
    }

    /// [`parse`](Self::parse) for a CSS animation, where a paint of `none` stays
    /// the keyword: CSS does not blend it with a colour, it swaps halfway.
    pub(super) fn parse_css(self, value: &Value, colors: &ColorContext) -> Result<AnimValue, String> {
        if matches!(self.kind(), Kind::Colour) && value.as_str().is_some_and(|text| text.trim() == "none") {
            return Ok(none_paint());
        }
        self.parse(value, colors)
    }

    /// Whether `items` is one value of a list-valued attribute (a dash list)
    /// and not keyframes. Keyframes of such an attribute wrap each list in an
    /// array of its own, or write it as a string.
    pub(super) fn is_list_value(self, items: &[Value]) -> bool {
        matches!(self.kind(), Kind::Dashes) && items.iter().all(Value::is_number)
    }

    pub(super) fn parse(self, value: &Value, colors: &ColorContext) -> Result<AnimValue, String> {
        let name = self.name();
        let parsed = match self.kind() {
            Kind::Number => AnimValue::Number(parse_number(name, value)?),
            Kind::Colour => {
                let text = value
                    .as_str()
                    .ok_or_else(|| format!("motion {name} must be a colour string"))?;
                // `none` paints nothing, which is what a transparent colour does,
                // so a stroke can fade in from it.
                let color = if text.trim() == "none" {
                    Some(Color::TRANSPARENT)
                } else {
                    Color::parse(text, colors)
                };
                AnimValue::Color(
                    color.ok_or_else(|| format!("motion {name} is not a valid colour: {text}"))?,
                )
            }
            Kind::Dashes => AnimValue::Numbers(parse_dashes(name, value)?),
            Kind::Path => {
                let text = value
                    .as_str()
                    .ok_or_else(|| format!("motion {name} must be path data"))?;
                // The CSS `d` property is `path("...")`; the attribute is the bare data.
                let data = text
                    .trim()
                    .strip_prefix("path(")
                    .and_then(|rest| rest.strip_suffix(')'))
                    .map_or(text, |quoted| quoted.trim().trim_matches(|quote| quote == '"' || quote == '\''));
                AnimValue::Path(PathData::parse(data).map_err(|error| format!("motion {name}: {error}"))?)
            }
        };
        self.validate(&parsed)?;
        Ok(parsed)
    }

    fn validate(self, value: &AnimValue) -> Result<(), String> {
        let name = self.name();
        let AnimValue::Number(number) = value else {
            return Ok(());
        };
        match self.clamp_rule() {
            Clamp::NonNegative if *number < 0.0 => {
                Err(format!("motion {name} must be non-negative"))
            }
            Clamp::Unit if !(0.0..=1.0).contains(number) => {
                Err(format!("motion {name} must be between 0 and 1"))
            }
            Clamp::AtLeastOne if *number < 1.0 => Err(format!("motion {name} must be at least 1")),
            _ => Ok(()),
        }
    }

    /// Springs overshoot, so what is written out has to stay in range.
    pub(super) fn clamp_value(self, value: AnimValue) -> AnimValue {
        match (self.clamp_rule(), value) {
            (Clamp::NonNegative, AnimValue::Number(value)) => AnimValue::Number(value.max(0.0)),
            (Clamp::Unit, AnimValue::Number(value)) => AnimValue::Number(value.clamp(0.0, 1.0)),
            (Clamp::AtLeastOne, AnimValue::Number(value)) => AnimValue::Number(value.max(1.0)),
            (_, value) => value,
        }
    }
}

fn none_paint() -> AnimValue {
    AnimValue::Keyword(Some("none".to_string()))
}

/// The text of an animated value as an XML attribute.
pub(super) fn attribute_text(value: &AnimValue) -> String {
    match value {
        AnimValue::Number(number) => format_number(*number),
        AnimValue::Color(color) => color.to_css(),
        AnimValue::Numbers(numbers) => numbers
            .iter()
            .map(|number| format_number(*number))
            .collect::<Vec<_>>()
            .join(" "),
        AnimValue::Path(path) => path.to_data(),
        AnimValue::Keyword(Some(keyword)) => keyword.clone(),
        AnimValue::Percent(_) | AnimValue::Variations(_) | AnimValue::Calc { .. } | AnimValue::Relative { .. } | AnimValue::Ellipse { .. } | AnimValue::Shadows(_) | AnimValue::Auto | AnimValue::Keyword(None) | AnimValue::List(_) => {
            unreachable!("an SVG attribute never holds {value:?}")
        }
    }
}

fn parse_number(name: &str, value: &Value) -> Result<f64, String> {
    let number = match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().trim_end_matches("px").trim().parse().ok(),
        _ => None,
    }
    .ok_or_else(|| format!("motion {name} must be a number"))?;
    check_finite(name, number)
}

fn check_finite(name: &str, value: f64) -> Result<f64, String> {
    if !value.is_finite() || value.abs() > f32::MAX as f64 {
        return Err(format!("motion {name} must fit a finite 32-bit float"));
    }
    Ok(value)
}

fn parse_dashes(name: &str, value: &Value) -> Result<Vec<f64>, String> {
    let numbers = match value {
        Value::Number(_) => vec![parse_number(name, value)?],
        Value::Array(items) => items
            .iter()
            .map(|item| parse_number(name, item))
            .collect::<Result<_, _>>()?,
        Value::String(text) => text
            .split(|character: char| character == ',' || character.is_whitespace())
            .filter(|token| !token.is_empty())
            .map(|token| parse_number(name, &Value::String(token.to_string())))
            .collect::<Result<_, _>>()?,
        _ => return Err(format!("motion {name} must be a number, a list of numbers or a string")),
    };
    if numbers.is_empty() {
        return Err(format!("motion {name} needs at least one number"));
    }
    if numbers.iter().any(|number| *number < 0.0) {
        return Err(format!("motion {name} must be non-negative"));
    }
    Ok(numbers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors() -> ColorContext {
        super::super::value::Colors::root().element
    }

    #[test]
    fn names_round_trip_and_draw_keys_are_not_attributes() {
        for name in ["pathLength", "strokeDashoffset", "x2", "d", "fillOpacity"] {
            assert_eq!(SvgAttr::from_name(name).unwrap().name(), name);
        }
        assert_eq!(SvgAttr::StrokeWidth.attribute(), Some("stroke-width"));
        assert_eq!(SvgAttr::PathLength.attribute(), None);
        assert_eq!(SvgAttr::PathOffset.attribute(), None);
        assert!(SvgAttr::from_name("transform").is_none());
    }

    #[test]
    fn reads_the_elements_own_attribute() {
        let attributes = HashMap::from([
            ("r".to_string(), serde_json::json!(12)),
            ("fill".to_string(), serde_json::json!("not a colour")),
            ("pathLength".to_string(), serde_json::json!(5)),
        ]);
        assert_eq!(SvgAttr::R.read(&attributes, &colors()), Some(AnimValue::Number(12.0)));
        assert_eq!(SvgAttr::Fill.read(&attributes, &colors()), None);
        assert_eq!(SvgAttr::Cx.read(&attributes, &colors()), None);
        // The DOM `pathLength` attribute is not the motion key.
        assert_eq!(SvgAttr::PathLength.read(&attributes, &colors()), None);
        assert_eq!(SvgAttr::PathLength.initial(), Some(AnimValue::Number(1.0)));
        assert_eq!(SvgAttr::R.initial(), None);
    }

    #[test]
    fn css_initial_values_are_what_a_missing_keyframe_endpoint_stands_for() {
        let number = |value| Some(AnimValue::Number(value));
        let keyword = |word: &str| Some(AnimValue::Keyword(Some(word.to_string())));
        for (attr, expected) in [
            (SvgAttr::StrokeDashoffset, number(0.0)),
            (SvgAttr::StrokeDasharray, keyword("none")),
            (SvgAttr::StrokeWidth, number(1.0)),
            (SvgAttr::StrokeMiterlimit, number(4.0)),
            (SvgAttr::Stroke, keyword("none")),
            (SvgAttr::FillOpacity, number(1.0)),
            (SvgAttr::StrokeOpacity, number(1.0)),
            (SvgAttr::Opacity, number(1.0)),
            (SvgAttr::FloodOpacity, number(1.0)),
            (SvgAttr::StopOpacity, number(1.0)),
            (SvgAttr::Cx, number(0.0)),
            (SvgAttr::Cy, number(0.0)),
            (SvgAttr::X, number(0.0)),
            (SvgAttr::Y, number(0.0)),
            (SvgAttr::R, number(0.0)),
            (SvgAttr::Rx, keyword("auto")),
            (SvgAttr::Ry, keyword("auto")),
            (SvgAttr::Width, keyword("auto")),
            (SvgAttr::Height, keyword("auto")),
            (SvgAttr::D, keyword("none")),
            (SvgAttr::LetterSpacing, number(0.0)),
            (SvgAttr::FontSize, number(24.0)),
        ] {
            assert_eq!(attr.initial_css(24.0), expected, "{attr:?}");
        }
        for attr in [SvgAttr::Fill, SvgAttr::StopColor, SvgAttr::FloodColor] {
            let Some(AnimValue::Color(color)) = attr.initial_css(24.0) else {
                panic!("{attr:?} is a colour");
            };
            assert_eq!(color.to_css(), "rgba(0.000, 0.000, 0.000, 1.00000)", "{attr:?}");
        }
        // Motion still starts from what it cannot resolve.
        assert_eq!(SvgAttr::R.initial(), None);
        assert_eq!(SvgAttr::Stroke.initial(), Some(AnimValue::Color(Color::TRANSPARENT)));
    }

    #[test]
    fn parses_dashes_in_every_spelling() {
        let dashes = |value: Value| SvgAttr::StrokeDasharray.parse(&value, &colors());
        assert_eq!(dashes(serde_json::json!(4)).unwrap(), AnimValue::Numbers(vec![4.0]));
        assert_eq!(
            dashes(serde_json::json!([4, 2])).unwrap(),
            AnimValue::Numbers(vec![4.0, 2.0])
        );
        assert_eq!(
            dashes(serde_json::json!("4, 2 1px")).unwrap(),
            AnimValue::Numbers(vec![4.0, 2.0, 1.0])
        );
        assert!(dashes(serde_json::json!("none")).is_err());
        assert!(dashes(serde_json::json!([])).is_err());
        assert!(dashes(serde_json::json!([-1])).is_err());
    }

    #[test]
    fn rejects_bad_values() {
        for (attr, value) in [
            (SvgAttr::Opacity, serde_json::json!(1.5)),
            (SvgAttr::R, serde_json::json!(-1)),
            (SvgAttr::Fill, serde_json::json!(3)),
            (SvgAttr::Fill, serde_json::json!("not a colour")),
            (SvgAttr::D, serde_json::json!("L0 0")),
            (SvgAttr::Cx, serde_json::json!(1e300)),
        ] {
            assert!(attr.parse(&value, &colors()).is_err(), "{attr:?} {value}");
        }
        assert_eq!(
            SvgAttr::Stroke.parse(&serde_json::json!("none"), &colors()).unwrap(),
            AnimValue::Color(Color::TRANSPARENT)
        );
    }

    #[test]
    fn writes_values_as_attribute_text() {
        assert_eq!(attribute_text(&AnimValue::Number(0.33333333)), "0.3333");
        assert_eq!(attribute_text(&AnimValue::Numbers(vec![4.0, 2.5])), "4 2.5");
        assert_eq!(
            attribute_text(&SvgAttr::D.parse(&serde_json::json!("m1 1 l2 2"), &colors()).unwrap()),
            "M 1 1 L 3 3"
        );
        let colour = SvgAttr::Fill.parse(&serde_json::json!("#ff0000"), &colors()).unwrap();
        assert!(attribute_text(&colour).starts_with("rgba(255"), "{}", attribute_text(&colour));
    }

    #[test]
    fn clamps_overshoot() {
        assert_eq!(
            SvgAttr::Opacity.clamp_value(AnimValue::Number(1.2)),
            AnimValue::Number(1.0)
        );
        assert_eq!(
            SvgAttr::R.clamp_value(AnimValue::Number(-3.0)),
            AnimValue::Number(0.0)
        );
        assert_eq!(
            SvgAttr::Cx.clamp_value(AnimValue::Number(-3.0)),
            AnimValue::Number(-3.0)
        );
    }

    #[test]
    fn stops_and_miter_limits_are_attributes_with_their_ranges() {
        assert_eq!(SvgAttr::StopColor.attribute(), Some("stop-color"));
        assert_eq!(SvgAttr::StopOpacity.attribute(), Some("stop-opacity"));
        assert_eq!(SvgAttr::StrokeMiterlimit.attribute(), Some("stroke-miterlimit"));
        assert!(SvgAttr::StopOpacity.parse(&serde_json::json!(2), &colors()).is_err());
        assert!(SvgAttr::StrokeMiterlimit.parse(&serde_json::json!(0.5), &colors()).is_err());
        assert_eq!(
            SvgAttr::StrokeMiterlimit.clamp_value(AnimValue::Number(0.2)),
            AnimValue::Number(1.0)
        );
        assert_eq!(SvgAttr::StrokeMiterlimit.initial(), Some(AnimValue::Number(4.0)));
    }

    #[test]
    fn only_css_properties_are_keyframeable_and_only_some_inherit() {
        for attr in [SvgAttr::StrokeDashoffset, SvgAttr::Fill, SvgAttr::R, SvgAttr::D, SvgAttr::StopColor, SvgAttr::FloodColor] {
            assert!(attr.is_css_property(), "{attr:?}");
        }
        for attr in [SvgAttr::PathLength, SvgAttr::X1, SvgAttr::StdDeviation, SvgAttr::Dx, SvgAttr::RefY] {
            assert!(!attr.is_css_property(), "{attr:?}");
        }
        assert!(SvgAttr::StrokeWidth.inherits() && SvgAttr::Fill.inherits());
        assert!(!SvgAttr::Opacity.inherits() && !SvgAttr::R.inherits() && !SvgAttr::StopColor.inherits());
    }

    #[test]
    fn a_path_may_be_written_as_the_css_path_function() {
        let bare = SvgAttr::D.parse(&serde_json::json!("M1 1 L2 2"), &colors()).unwrap();
        for written in [r#"path("M1 1 L2 2")"#, "path('M1 1 L2 2')", r#" path( "M1 1 L2 2" ) "#] {
            assert_eq!(SvgAttr::D.parse(&serde_json::json!(written), &colors()).unwrap(), bare, "{written}");
        }
    }
}
