//! The CSS `box-shadow` string, parsed into the per-layer struct the object
//! form already uses.

use super::syntax::{split_commas, split_whitespace};
use crate::style::BoxShadowValue;

/// The colour of a layer that names none, as in CSS.
const DEFAULT_COLOR: &str = "currentColor";

/// `None` when the declaration is invalid, which CSS ignores as a whole. An
/// empty list is the valid `none`.
///
/// Colours are not resolved here: they need the element's `currentColor` and
/// `colorScheme`, so each layer keeps its colour text for `apply_styles`.
pub(crate) fn parse(value: &str) -> Option<Vec<BoxShadowValue>> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    split_commas(value).into_iter().map(parse_layer).collect()
}

fn parse_layer(layer: &str) -> Option<BoxShadowValue> {
    #[derive(PartialEq)]
    enum Run {
        NotStarted,
        Open,
        Closed,
    }

    let mut inset = false;
    let mut color: Option<&str> = None;
    let mut lengths: Vec<f64> = Vec::new();
    // The lengths must be contiguous, so `1px red 2px` is invalid.
    let mut run = Run::NotStarted;

    for token in split_whitespace(layer) {
        if let Some(length) = parse_length(token) {
            if run == Run::Closed {
                return None;
            }
            run = Run::Open;
            lengths.push(length);
            continue;
        }
        if run == Run::Open {
            run = Run::Closed;
        }
        if token.eq_ignore_ascii_case("inset") {
            if std::mem::replace(&mut inset, true) {
                return None;
            }
        } else if color.replace(token).is_some() {
            return None;
        }
    }

    let [offset_x, offset_y, rest @ ..] = lengths.as_slice() else {
        return None;
    };
    let (blur_radius, spread_radius) = match rest {
        [] => (0.0, 0.0),
        [blur] => (*blur, 0.0),
        [blur, spread] => (*blur, *spread),
        _ => return None,
    };
    if blur_radius < 0.0 {
        return None;
    }
    Some(BoxShadowValue {
        offset_x: *offset_x,
        offset_y: *offset_y,
        blur_radius,
        spread_radius,
        color: color.unwrap_or(DEFAULT_COLOR).to_owned(),
        inset,
    })
}

/// `12px`, `-2.5px`, or the unitless `0`. Other units cannot be resolved here
/// and other unitless numbers are invalid CSS.
fn parse_length(token: &str) -> Option<f64> {
    let number = token
        .strip_suffix("px")
        .or_else(|| token.strip_suffix("PX"))
        .unwrap_or(token);
    let first = number.chars().next()?;
    if !(first.is_ascii_digit() || matches!(first, '-' | '+' | '.')) {
        return None;
    }
    let value = number.parse::<f64>().ok().filter(|value| value.is_finite())?;
    // Without the unit only zero is a length: `4` is not valid CSS.
    (number.len() != token.len() || value == 0.0).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(
        offset_x: f64,
        offset_y: f64,
        blur_radius: f64,
        spread_radius: f64,
        color: &str,
        inset: bool,
    ) -> BoxShadowValue {
        BoxShadowValue {
            offset_x,
            offset_y,
            blur_radius,
            spread_radius,
            color: color.to_owned(),
            inset,
        }
    }

    #[test]
    fn parses_the_documented_example() {
        assert_eq!(
            parse("0 1px 2px rgba(0,0,0,.2), inset 0 0 0 1px red"),
            Some(vec![
                layer(0.0, 1.0, 2.0, 0.0, "rgba(0,0,0,.2)", false),
                layer(0.0, 0.0, 0.0, 1.0, "red", true),
            ])
        );
    }

    #[test]
    fn the_colour_and_inset_may_lead_or_trail() {
        let expected = Some(vec![layer(1.0, 2.0, 3.0, 4.0, "#ff0000", true)]);
        assert_eq!(parse("1px 2px 3px 4px #ff0000 inset"), expected);
        assert_eq!(parse("inset #ff0000 1px 2px 3px 4px"), expected);
        assert_eq!(parse("#ff0000 1px 2px 3px 4px inset"), expected);
        assert_eq!(parse("INSET 1px 2px 3px 4px #ff0000"), expected);
    }

    #[test]
    fn omitted_blur_spread_and_colour_default_like_css() {
        assert_eq!(
            parse("2px 3px"),
            Some(vec![layer(2.0, 3.0, 0.0, 0.0, "currentColor", false)])
        );
        assert_eq!(
            parse("2px 3px 4px"),
            Some(vec![layer(2.0, 3.0, 4.0, 0.0, "currentColor", false)])
        );
    }

    #[test]
    fn colours_with_spaces_and_functions_stay_whole() {
        let parsed = parse("0 4px 12px color-mix(in srgb, red 50%, blue), 0 0 0 1px rgb(0 0 0 / 50%)")
            .unwrap();
        assert_eq!(parsed[0].color, "color-mix(in srgb, red 50%, blue)");
        assert_eq!(parsed[1].color, "rgb(0 0 0 / 50%)");
    }

    #[test]
    fn negative_offsets_and_spread_are_allowed() {
        assert_eq!(
            parse("-2px -3.5px 0 -1px black"),
            Some(vec![layer(-2.0, -3.5, 0.0, -1.0, "black", false)])
        );
    }

    #[test]
    fn none_is_an_empty_list() {
        assert_eq!(parse("none"), Some(Vec::new()));
        assert_eq!(parse(" None "), Some(Vec::new()));
    }

    #[test]
    fn rejects_invalid_declarations() {
        for input in [
            "",
            "1px",
            "1px 2px 3px 4px 5px red",
            "1px 2px -3px red",
            "4 4 red",
            "1px 2px red blue",
            "1px red 2px",
            "inset inset 1px 2px",
            "1em 2em red",
            "1px 2px red,",
            "red",
        ] {
            assert_eq!(parse(input), None, "{input}");
        }
    }
}
