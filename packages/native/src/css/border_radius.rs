//! `border-radius` and its four corner longhands, including the elliptical
//! `a / b` form, parsed into gpui's [`CornerRadius`].
//!
//! A percentage stays symbolic: gpui resolves it against the border box, `x`
//! against the width and `y` against the height.

use gpui::{px, AbsoluteLength, CornerRadius, Corners, DefiniteLength};

use super::dimension::{self, LengthPercentage};
use super::syntax::{expand_sides, split_slash_tokens};

/// A px number from a style object. Zero is a square corner.
pub(crate) fn corner_from_number(value: f64) -> Result<CornerRadius, String> {
    length_from_number(value).map(CornerRadius::from)
}

fn length_from_number(value: f64) -> Result<DefiniteLength, String> {
    if !value.is_finite() || value < 0.0 {
        return Err(format!("invalid border radius {value}: expected a non-negative number"));
    }
    Ok(DefiniteLength::Absolute(AbsoluteLength::Pixels(px(value as f32))))
}

/// The `border-radius` shorthand: one to four values, optionally followed by
/// `/` and one to four more for the vertical radii.
pub(crate) fn parse_border_radius(value: &str) -> Result<Corners<CornerRadius>, String> {
    let tokens = split_slash_tokens(value);
    let radii = parse_radii(&tokens, parse_radius_length).map_err(|reason| invalid("border-radius", value, &reason))?;
    let corner = |index: usize| corner_of(radii.horizontal[index], radii.vertical.map(|vertical| vertical[index]));
    Ok(Corners {
        top_left: corner(0),
        top_right: corner(1),
        bottom_right: corner(2),
        bottom_left: corner(3),
    })
}

/// A corner longhand: the horizontal radius, then the vertical one.
pub(crate) fn parse_corner(value: &str) -> Result<CornerRadius, String> {
    let tokens = split_slash_tokens(value);
    let lengths = tokens
        .iter()
        .map(|token| parse_radius_length(token))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|reason| invalid("border-radius", value, &reason))?;
    match lengths[..] {
        [x] => Ok(corner_of(x, None)),
        [x, y] => Ok(corner_of(x, Some(y))),
        _ => Err(invalid("border-radius", value, "expected a horizontal radius and an optional vertical one")),
    }
}

fn invalid(property: &str, value: &str, reason: &str) -> String {
    format!("invalid {property} \"{value}\": {reason}")
}

/// The one place a radius token is read, so units that need the font size or
/// the viewport can be added without touching the grammar above.
fn parse_radius_length(token: &str) -> Result<DefiniteLength, String> {
    let unsupported = || format!("\"{token}\" is not a non-negative px length or percentage");
    match dimension::parse_length_percentage(token).ok_or_else(unsupported)? {
        LengthPercentage::Length(length) if length >= 0.0 => length_from_number(length),
        LengthPercentage::Percent(fraction) if fraction >= 0.0 => Ok(DefiniteLength::Fraction(fraction as f32)),
        _ => Err(format!("\"{token}\" is negative")),
    }
}

fn is_zero(length: DefiniteLength) -> bool {
    match length {
        DefiniteLength::Absolute(AbsoluteLength::Pixels(pixels)) => f32::from(pixels) == 0.0,
        DefiniteLength::Absolute(AbsoluteLength::Rems(rems)) => rems.0 == 0.0,
        DefiniteLength::Fraction(fraction) => fraction == 0.0,
    }
}

/// gpui reads a zero vertical radius as "the same as the horizontal one", so a
/// corner with an explicit zero on either axis, which CSS squares, gets a zero
/// horizontal radius instead.
fn corner_of(x: DefiniteLength, y: Option<DefiniteLength>) -> CornerRadius {
    match y {
        None => CornerRadius::from(x),
        Some(y) if is_zero(x) || is_zero(y) => CornerRadius::default(),
        Some(y) => CornerRadius { x, y },
    }
}

/// The horizontal radii of the four corners clockwise from the top left, and
/// the vertical ones when the value had a `/`.
pub(crate) struct Radii<T> {
    pub horizontal: [T; 4],
    pub vertical: Option<[T; 4]>,
}

/// `<length>{1,4} [ / <length>{1,4} ]?` as tokens with the `/` apart.
pub(crate) fn parse_radii<T: Copy>(
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

/// The value a style object sends back out: a number for a circular px corner,
/// else the CSS text.
pub(crate) fn corner_to_json(corner: &CornerRadius) -> serde_json::Value {
    match circular_px(corner) {
        Some(pixels) => serde_json::json!(pixels),
        None => serde_json::Value::String(corner_css(corner)),
    }
}

pub(crate) fn border_radius_to_json(corners: &Corners<CornerRadius>) -> serde_json::Value {
    let all = [corners.top_left, corners.top_right, corners.bottom_right, corners.bottom_left];
    if let Some(first) = circular_px(&all[0]) {
        if all.iter().all(|corner| circular_px(corner) == Some(first)) {
            return serde_json::json!(first);
        }
    }
    let horizontal: Vec<String> = all.iter().map(|corner| length_css(corner.x)).collect();
    let vertical: Vec<String> = all.iter().map(|corner| length_css(vertical_of(corner))).collect();
    let circular = all.iter().all(|corner| vertical_of(corner) == corner.x);
    serde_json::Value::String(if circular {
        horizontal.join(" ")
    } else {
        format!("{} / {}", horizontal.join(" "), vertical.join(" "))
    })
}

fn vertical_of(corner: &CornerRadius) -> DefiniteLength {
    if is_zero(corner.y) {
        corner.x
    } else {
        corner.y
    }
}

fn circular_px(corner: &CornerRadius) -> Option<f32> {
    match (corner.x, corner.y) {
        (DefiniteLength::Absolute(AbsoluteLength::Pixels(pixels)), y) if is_zero(y) => Some(f32::from(pixels)),
        _ => None,
    }
}

fn corner_css(corner: &CornerRadius) -> String {
    let vertical = vertical_of(corner);
    if vertical == corner.x {
        length_css(corner.x)
    } else {
        format!("{} {}", length_css(corner.x), length_css(vertical))
    }
}

fn length_css(length: DefiniteLength) -> String {
    match length {
        DefiniteLength::Absolute(AbsoluteLength::Pixels(pixels)) => format!("{}px", f32::from(pixels)),
        DefiniteLength::Absolute(AbsoluteLength::Rems(rems)) => format!("{}rem", rems.0),
        DefiniteLength::Fraction(fraction) => format!("{}%", fraction * 100.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::rems;

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

    fn all(value: &str) -> [CornerRadius; 4] {
        let corners = parse_border_radius(value).unwrap();
        [corners.top_left, corners.top_right, corners.bottom_right, corners.bottom_left]
    }

    #[test]
    fn a_number_is_a_circular_px_radius() {
        assert_eq!(corner_from_number(10.0).unwrap(), circle(10.0));
        assert_eq!(corner_from_number(0.0).unwrap(), circle(0.0));
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
        assert_eq!(all("0%")[0], corner(percent(0.0), percent(0.0)));
    }

    #[test]
    fn a_slash_inside_calc_is_not_the_separator() {
        assert_eq!(all("calc(20px / 2)"), [circle(10.0); 4]);
    }

    #[test]
    fn invalid_shorthands_name_the_value() {
        for value in [
            "", "10px 20px 30px 40px 50px", "-5px", "10px /", "/ 10px", "10px / 1px 2px 3px 4px 5px",
            "10px / 5px / 3px", "red", "10", "1em", "2rem", "10vw", "10px, 20px",
        ] {
            let error = parse_border_radius(value).unwrap_err();
            assert!(error.contains("border-radius") && error.contains(&format!("\"{value}\"")), "{value}: {error}");
        }
        assert!(parse_border_radius("1em").unwrap_err().contains("\"1em\" is not a non-negative px length"));
        assert!(parse_border_radius("-5px").unwrap_err().contains("\"-5px\" is negative"));
    }

    #[test]
    fn a_longhand_takes_a_horizontal_radius_and_an_optional_vertical_one() {
        assert_eq!(parse_corner("20px").unwrap(), circle(20.0));
        assert_eq!(parse_corner("20px 10px").unwrap(), corner(len(20.0), len(10.0)));
        assert_eq!(parse_corner("50%").unwrap(), corner(percent(0.5), percent(0.5)));
        assert_eq!(parse_corner("50% 20px").unwrap(), corner(percent(0.5), len(20.0)));
        assert_eq!(parse_corner("20px 0").unwrap(), CornerRadius::default());
        assert_eq!(parse_corner("0 20px").unwrap(), CornerRadius::default());
        for value in ["", "1px 2px 3px", "1px / 2px", "-1px", "1px -2px", "1rem", "auto"] {
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
        assert_eq!(corner_to_json(&circle(4.0)), serde_json::json!(4.0));
        assert_eq!(corner_to_json(&parse_corner("50% 20px").unwrap()), serde_json::json!("50% 20px"));
        assert_eq!(corner_to_json(&CornerRadius::from(rems(2.0))), serde_json::json!("2rem"));
    }
}
