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
}

#[derive(Clone, Copy)]
enum Initial {
    /// Only the element's own attribute can say, e.g. a circle's `r`.
    Unresolved,
    Number(f64),
    Black,
    Transparent,
}

macro_rules! attributes {
    ($($variant:ident $name:literal $attribute:literal $kind:ident $clamp:ident $initial:expr;)*) => {
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
        }
    };
}

use Initial::{Black, Number, Transparent, Unresolved};

attributes! {
    PathLength "pathLength" "" Number NonNegative Number(1.0);
    PathOffset "pathOffset" "" Number None Number(0.0);
    PathSpacing "pathSpacing" "" Number NonNegative Number(1.0);
    StrokeDasharray "strokeDasharray" "stroke-dasharray" Dashes None Unresolved;
    StrokeDashoffset "strokeDashoffset" "stroke-dashoffset" Number None Number(0.0);
    StrokeWidth "strokeWidth" "stroke-width" Number NonNegative Number(1.0);
    Fill "fill" "fill" Colour None Black;
    Stroke "stroke" "stroke" Colour None Transparent;
    Opacity "opacity" "opacity" Number Unit Number(1.0);
    FillOpacity "fillOpacity" "fill-opacity" Number Unit Number(1.0);
    StrokeOpacity "strokeOpacity" "stroke-opacity" Number Unit Number(1.0);
    Cx "cx" "cx" Number None Number(0.0);
    Cy "cy" "cy" Number None Number(0.0);
    R "r" "r" Number NonNegative Unresolved;
    Rx "rx" "rx" Number NonNegative Unresolved;
    Ry "ry" "ry" Number NonNegative Unresolved;
    X "x" "x" Number None Number(0.0);
    Y "y" "y" Number None Number(0.0);
    Width "width" "width" Number NonNegative Unresolved;
    Height "height" "height" Number NonNegative Unresolved;
    X1 "x1" "x1" Number None Number(0.0);
    Y1 "y1" "y1" Number None Number(0.0);
    X2 "x2" "x2" Number None Number(0.0);
    Y2 "y2" "y2" Number None Number(0.0);
    D "d" "d" Path None Unresolved;
}

impl SvgAttr {
    /// What the shape shows when its own attributes do not say.
    pub(super) fn initial(self) -> Option<AnimValue> {
        match self.initial_rule() {
            Initial::Unresolved => None,
            Initial::Number(value) => Some(AnimValue::Number(value)),
            Initial::Black => Some(AnimValue::Color(Color::from_rgba(gpui::Rgba {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }))),
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
                AnimValue::Path(PathData::parse(text).map_err(|error| format!("motion {name}: {error}"))?)
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
            _ => Ok(()),
        }
    }

    /// Springs overshoot, so what is written out has to stay in range.
    pub(super) fn clamp_value(self, value: AnimValue) -> AnimValue {
        match (self.clamp_rule(), value) {
            (Clamp::NonNegative, AnimValue::Number(value)) => AnimValue::Number(value.max(0.0)),
            (Clamp::Unit, AnimValue::Number(value)) => AnimValue::Number(value.clamp(0.0, 1.0)),
            (_, value) => value,
        }
    }
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
        AnimValue::Percent(_) | AnimValue::Shadows(_) | AnimValue::Auto | AnimValue::Keyword(_) => {
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
}
