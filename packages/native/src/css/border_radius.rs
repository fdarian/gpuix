//! `border-radius` and its four corner longhands, including the elliptical
//! `a / b` form.
//!
//! A radius is a [`LinearLength`] per axis, so `em`, `rem`, the viewport units and
//! `calc()` mixing a length and a percentage work as in every other length. It
//! resolves against the element's [`LengthContext`] into gpui's [`CornerRadius`]
//! when the style is applied, and a percentage stays symbolic for gpui to resolve
//! against the border box: `x` against the width and `y` against the height.

use gpui::{AbsoluteLength, CornerRadius, Corners, DefiniteLength};

use super::dimension;
use super::length::{LengthContext, LinearLength};
use super::syntax::{expand_sides, split_slash_tokens};
use crate::style::CornerRadiusValue;

/// A px number from a style object. Zero is a square corner.
pub(crate) fn corner_from_number(value: f64) -> Result<CornerRadiusValue, String> {
    length_from_number(value).map(CornerRadiusValue::circular)
}

fn length_from_number(value: f64) -> Result<LinearLength, String> {
    if !value.is_finite() || value < 0.0 {
        return Err(format!("invalid border radius {value}: expected a non-negative number"));
    }
    Ok(LinearLength::px(value).clamped())
}

/// The `border-radius` shorthand: one to four values, optionally followed by
/// `/` and one to four more for the vertical radii.
pub(crate) fn parse_border_radius(value: &str) -> Result<Corners<CornerRadiusValue>, String> {
    let tokens = split_slash_tokens(value);
    let radii = parse_radii(&tokens, parse_radius_length).map_err(|reason| invalid("border-radius", value, &reason))?;
    let corner = |index: usize| CornerRadiusValue {
        x: radii.horizontal[index],
        y: radii.vertical.map(|vertical| vertical[index]),
    };
    Ok(Corners {
        top_left: corner(0),
        top_right: corner(1),
        bottom_right: corner(2),
        bottom_left: corner(3),
    })
}

/// A corner longhand: the horizontal radius, then the vertical one.
pub(crate) fn parse_corner(value: &str) -> Result<CornerRadiusValue, String> {
    let tokens = split_slash_tokens(value);
    let lengths = tokens
        .iter()
        .map(|token| parse_radius_length(token))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|reason| invalid("border-radius", value, &reason))?;
    match lengths[..] {
        [x] => Ok(CornerRadiusValue { x, y: None }),
        [x, y] => Ok(CornerRadiusValue { x, y: Some(y) }),
        _ => Err(invalid("border-radius", value, "expected a horizontal radius and an optional vertical one")),
    }
}

fn invalid(property: &str, value: &str, reason: &str) -> String {
    format!("invalid {property} \"{value}\": {reason}")
}

/// A radius: any length or `calc()`. One that is negative whatever the font size and
/// viewport are is invalid; one that only comes out negative (`calc(1em - 20px)`) is
/// clamped to zero when it resolves, as CSS clamps a `calc()` to its property's range.
fn parse_radius_length(token: &str) -> Result<LinearLength, String> {
    let length = dimension::parse_linear_length(token)
        .ok_or_else(|| format!("\"{token}\" is not a non-negative length or percentage"))?;
    if length.is_negative() {
        return Err(format!("\"{token}\" is negative"));
    }
    Ok(length)
}

/// The horizontal radii of the four corners clockwise from the top left, and
/// the vertical ones when the value had a `/`.
pub(crate) struct Radii<T> {
    pub horizontal: [T; 4],
    pub vertical: Option<[T; 4]>,
}

/// `<length>{1,4} [ / <length>{1,4} ]?` as tokens with the `/` apart.
pub(crate) fn parse_radii<T: Clone>(
    tokens: &[&str],
    parse: impl Fn(&str) -> Result<T, String>,
) -> Result<Radii<T>, String> {
    let slash = tokens.iter().position(|token| *token == "/");
    let list = |tokens: &[&str]| -> Result<[T; 4], String> {
        let values = tokens.iter().map(|token| parse(token)).collect::<Result<Vec<_>, _>>()?;
        expand_sides(&values).ok_or_else(|| "expected one to four values".to_string())
    };
    match slash {
        None => Ok(Radii { horizontal: list(tokens)?, vertical: None }),
        Some(index) => Ok(Radii { horizontal: list(&tokens[..index])?, vertical: Some(list(&tokens[index + 1..])?) }),
    }
}

impl CornerRadiusValue {
    /// This corner for an element with `lengths`. gpui reads a zero vertical radius as "the
    /// same as the horizontal one", so a corner with an explicit zero on either axis, which
    /// CSS squares, becomes a square one.
    pub(crate) fn resolve(&self, lengths: &LengthContext) -> CornerRadius {
        let x = definite(lengths, &self.x);
        match self.y.as_ref().map(|y| definite(lengths, y)) {
            None => CornerRadius::from(x),
            Some(y) if is_zero(x) || is_zero(y) => CornerRadius::default(),
            Some(y) => CornerRadius { x, y },
        }
    }

    /// The px of a circular corner of plain px, which is what motion moves.
    pub(crate) fn circular_px(&self) -> Option<f64> {
        match self.y {
            Some(y) if y != self.x => None,
            _ => self.x.as_px(),
        }
    }

    /// A style object's value for this corner: a number for a circular px one, else CSS text.
    pub(crate) fn to_json(&self) -> serde_json::Value {
        match self.circular_px() {
            Some(pixels) => serde_json::json!(pixels),
            None => serde_json::Value::String(self.to_css()),
        }
    }

    fn to_css(&self) -> String {
        match self.y {
            Some(y) if y != self.x => format!("{} {y}", self.x),
            _ => self.x.to_string(),
        }
    }
}

/// A length for gpui to resolve against a box: the px part now, the percentage as a fraction.
fn definite(lengths: &LengthContext, length: &LinearLength) -> DefiniteLength {
    let (px, fraction) = lengths.split(length);
    let px = gpui::px(px as f32);
    if fraction == 0.0 {
        DefiniteLength::Absolute(AbsoluteLength::Pixels(px))
    } else if px == gpui::px(0.0) {
        DefiniteLength::Fraction(fraction as f32)
    } else {
        DefiniteLength::calc(px, fraction as f32)
    }
}

fn is_zero(length: DefiniteLength) -> bool {
    match length {
        DefiniteLength::Absolute(AbsoluteLength::Pixels(pixels)) => f32::from(pixels) == 0.0,
        DefiniteLength::Absolute(AbsoluteLength::Rems(rems)) => rems.0 == 0.0,
        DefiniteLength::Fraction(fraction) => fraction == 0.0,
        DefiniteLength::Calc { length, fraction } => is_zero(DefiniteLength::Absolute(length)) && fraction == 0.0,
    }
}

/// The value a style object sends back out for `borderRadius`: a number for a circular px
/// radius on every corner, else the CSS text.
pub(crate) fn border_radius_to_json(corners: &Corners<CornerRadiusValue>) -> serde_json::Value {
    let all = [corners.top_left, corners.top_right, corners.bottom_right, corners.bottom_left];
    if let Some(first) = all[0].circular_px() {
        if all.iter().all(|corner| corner.circular_px() == Some(first)) {
            return serde_json::json!(first);
        }
    }
    let horizontal: Vec<String> = all.iter().map(|corner| corner.x.to_string()).collect();
    let vertical: Vec<String> = all.iter().map(|corner| corner.y.unwrap_or(corner.x).to_string()).collect();
    let circular = all.iter().all(|corner| corner.y.is_none_or(|y| y == corner.x));
    serde_json::Value::String(if circular {
        horizontal.join(" ")
    } else {
        format!("{} / {}", horizontal.join(" "), vertical.join(" "))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::px;

    fn len(x: f32) -> DefiniteLength {
        DefiniteLength::Absolute(AbsoluteLength::Pixels(px(x)))
    }

    fn percent(fraction: f32) -> DefiniteLength {
        DefiniteLength::Fraction(fraction)
    }

    fn corner(x: DefiniteLength, y: DefiniteLength) -> CornerRadius {
        CornerRadius { x, y }
    }

    fn circle(radius: f32) -> CornerRadius {
        CornerRadius::from(px(radius))
    }

    /// `1em` is 20px, `1rem` 10px and the viewport 800 by 600.
    fn lengths() -> LengthContext {
        LengthContext::for_tests()
    }

    fn all(value: &str) -> [CornerRadius; 4] {
        let corners = parse_border_radius(value).unwrap();
        [corners.top_left, corners.top_right, corners.bottom_right, corners.bottom_left]
            .map(|corner| corner.resolve(&lengths()))
    }

    fn resolved(value: &str) -> CornerRadius {
        parse_corner(value).unwrap().resolve(&lengths())
    }

    #[test]
    fn a_number_is_a_circular_px_radius() {
        assert_eq!(corner_from_number(10.0).unwrap().resolve(&lengths()), circle(10.0));
        assert_eq!(corner_from_number(0.0).unwrap().resolve(&lengths()), circle(0.0));
        assert!(corner_from_number(-1.0).unwrap_err().contains("-1"));
        assert!(corner_from_number(f64::NAN).is_err());
    }

    #[test]
    fn the_shorthand_expands_one_to_four_values_clockwise_from_the_top_left() {
        assert_eq!(all("10px"), [circle(10.0); 4]);
        assert_eq!(all("10px 20px"), [circle(10.0), circle(20.0), circle(10.0), circle(20.0)]);
        assert_eq!(all("1px 2px 3px"), [circle(1.0), circle(2.0), circle(3.0), circle(2.0)]);
        assert_eq!(all("10px 20px 30px 40px"), [circle(10.0), circle(20.0), circle(30.0), circle(40.0)]);
        assert_eq!(all("  10px   20px "), all("10px 20px"));
        assert_eq!(all("0"), [circle(0.0); 4]);
    }

    #[test]
    fn a_percentage_fills_both_axes_and_stays_symbolic() {
        assert_eq!(all("50%"), [corner(percent(0.5), percent(0.5)); 4]);
        assert_eq!(all("10% 50%")[1], corner(percent(0.5), percent(0.5)));
        assert_eq!(all("10% 50%")[0], corner(percent(0.1), percent(0.1)));
    }

    #[test]
    fn absolute_units_resolve_to_px() {
        assert_eq!(all("1in")[0], circle(96.0));
        assert_eq!(all("calc(4px + 6px)")[0], circle(10.0));
    }

    #[test]
    fn font_relative_and_viewport_units_resolve_against_the_context() {
        assert_eq!(all("1em 2rem 10vw 5vh"), [circle(20.0), circle(20.0), circle(80.0), circle(30.0)]);
        assert_eq!(all("10vmin 10vmax")[0], circle(60.0));
        assert_eq!(all("10vmin 10vmax")[1], circle(80.0));
        assert_eq!(all("0.5em / 1rem")[0], corner(len(10.0), len(10.0)));
        let larger = parse_corner("1em").unwrap().resolve(&lengths().descend(Some(&LinearLength::px(40.0)), false));
        assert_eq!(larger, circle(40.0));
    }

    #[test]
    fn a_calc_mixing_a_length_and_a_percentage_keeps_the_percentage_for_the_box() {
        assert_eq!(resolved("calc(1em + 25%)"), corner(DefiniteLength::calc(px(20.0), 0.25), DefiniteLength::calc(px(20.0), 0.25)));
        assert_eq!(
            resolved("calc(100% - 1em) 2rem"),
            corner(DefiniteLength::calc(px(-20.0), 1.0), len(20.0))
        );
        assert_eq!(resolved("calc(50% + 0px)"), corner(percent(0.5), percent(0.5)));
        // A radius that comes out negative only in this context is zero, as CSS clamps it.
        assert_eq!(resolved("calc(1em - 30px)").x.to_pixels(AbsoluteLength::Pixels(px(100.0)), px(16.0)), px(-10.0));
        assert_eq!(
            resolved("calc(1em - 30px)").to_pixels(gpui::size(px(100.0), px(100.0)), px(16.0)),
            gpui::size(px(0.0), px(0.0))
        );
    }

    #[test]
    fn a_slash_gives_each_corner_a_vertical_radius() {
        assert_eq!(
            all("10px 20px / 5px"),
            [
                corner(len(10.0), len(5.0)),
                corner(len(20.0), len(5.0)),
                corner(len(10.0), len(5.0)),
                corner(len(20.0), len(5.0)),
            ]
        );
        assert_eq!(
            all("50% / 10px 20px"),
            [
                corner(percent(0.5), len(10.0)),
                corner(percent(0.5), len(20.0)),
                corner(percent(0.5), len(10.0)),
                corner(percent(0.5), len(20.0)),
            ]
        );
        assert_eq!(
            all("1px 2px 3px 4px/5px 6px 7px 8px"),
            [
                corner(len(1.0), len(5.0)),
                corner(len(2.0), len(6.0)),
                corner(len(3.0), len(7.0)),
                corner(len(4.0), len(8.0)),
            ]
        );
    }

    #[test]
    fn a_zero_on_either_axis_of_an_elliptical_corner_squares_it() {
        // gpui reads a zero vertical radius as "same as horizontal", which CSS does not.
        assert_eq!(all("10px / 0")[0], CornerRadius::default());
        assert_eq!(all("0 / 10px")[0], CornerRadius::default());
        assert_eq!(all("10px 20px / 0 5px"), [
            CornerRadius::default(),
            corner(len(20.0), len(5.0)),
            CornerRadius::default(),
            corner(len(20.0), len(5.0)),
        ]);
        assert_eq!(all("0%")[0], CornerRadius::default());
        assert_eq!(all("calc(1em - 20px) / 10px")[0], CornerRadius::default());
    }

    #[test]
    fn a_slash_inside_calc_is_not_the_separator() {
        assert_eq!(all("calc(20px / 2)"), [circle(10.0); 4]);
    }

    #[test]
    fn invalid_shorthands_name_the_value() {
        for value in [
            "", "10px 20px 30px 40px 50px", "-5px", "10px /", "/ 10px", "10px / 1px 2px 3px 4px 5px",
            "10px / 5px / 3px", "red", "10", "1ch", "-1em", "calc(-1em - 1px)", "10px, 20px",
        ] {
            let error = parse_border_radius(value).unwrap_err();
            assert!(error.contains("border-radius") && error.contains(&format!("\"{value}\"")), "{value}: {error}");
        }
        assert!(parse_border_radius("1ch").unwrap_err().contains("\"1ch\" is not a non-negative length"));
        assert!(parse_border_radius("-5px").unwrap_err().contains("\"-5px\" is negative"));
    }

    #[test]
    fn a_longhand_takes_a_horizontal_radius_and_an_optional_vertical_one() {
        assert_eq!(resolved("20px"), circle(20.0));
        assert_eq!(resolved("20px 10px"), corner(len(20.0), len(10.0)));
        assert_eq!(resolved("50%"), corner(percent(0.5), percent(0.5)));
        assert_eq!(resolved("50% 20px"), corner(percent(0.5), len(20.0)));
        assert_eq!(resolved("1em 2rem"), corner(len(20.0), len(20.0)));
        assert_eq!(resolved("20px 0"), CornerRadius::default());
        assert_eq!(resolved("0 20px"), CornerRadius::default());
        for value in ["", "1px 2px 3px", "1px / 2px", "-1px", "1px -2px", "1ch", "auto"] {
            assert!(parse_corner(value).is_err(), "{value}");
        }
    }

    #[test]
    fn style_values_serialize_back_to_what_a_style_object_holds() {
        assert_eq!(border_radius_to_json(&parse_border_radius("8px").unwrap()), serde_json::json!(8.0));
        assert_eq!(border_radius_to_json(&parse_border_radius("1px 2px").unwrap()), serde_json::json!("1px 2px 1px 2px"));
        assert_eq!(border_radius_to_json(&parse_border_radius("50%").unwrap()), serde_json::json!("50% 50% 50% 50%"));
        assert_eq!(
            border_radius_to_json(&parse_border_radius("10px 20px / 5px").unwrap()),
            serde_json::json!("10px 20px 10px 20px / 5px 5px 5px 5px")
        );
        assert_eq!(
            border_radius_to_json(&parse_border_radius("1em 2rem").unwrap()),
            serde_json::json!("1em 2rem 1em 2rem")
        );
        assert_eq!(CornerRadiusValue::px(4.0).to_json(), serde_json::json!(4.0));
        assert_eq!(parse_corner("50% 20px").unwrap().to_json(), serde_json::json!("50% 20px"));
        assert_eq!(parse_corner("calc(1em + 25%)").unwrap().to_json(), serde_json::json!("calc(25% + 1em)"));
        assert_eq!(parse_corner("2rem").unwrap().to_json(), serde_json::json!("2rem"));
    }
}
