//! The CSS `box-shadow` string, parsed into the per-layer struct the object
//! form already uses.

use super::dimension;
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
        if let Some(length) = dimension::parse_length(token) {
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

    #[test]
    fn units_are_case_insensitive_and_absolute_units_convert_to_px() {
        assert_eq!(
            parse("1PX +2Px 3pX red"),
            Some(vec![layer(1.0, 2.0, 3.0, 0.0, "red", false)])
        );
        assert_eq!(
            parse("1in 0 0 0 red"),
            Some(vec![layer(96.0, 0.0, 0.0, 0.0, "red", false)])
        );
        let parsed = parse("1pt 25.4MM 2.54cm 4q red").unwrap();
        assert!((parsed[0].offset_x - 96.0 / 72.0).abs() < 1e-9);
        assert!((parsed[0].offset_y - 96.0).abs() < 1e-9);
        assert!((parsed[0].blur_radius - 96.0).abs() < 1e-9);
        assert!((parsed[0].spread_radius - 4.0 * 96.0 / 101.6).abs() < 1e-9);
    }

    #[test]
    fn calc_takes_lengths() {
        assert_eq!(
            parse("calc(1px + 2px) CALC(2 * 3px) 0 0 red"),
            Some(vec![layer(3.0, 6.0, 0.0, 0.0, "red", false)])
        );
        assert_eq!(
            parse("calc((10px - 4px) / 2) 0 red"),
            Some(vec![layer(3.0, 0.0, 0.0, 0.0, "red", false)])
        );
        // A length and a percentage cannot be added in a box-shadow, which has no percentage.
        assert_eq!(parse("calc(1px + 50%) 0 red"), None);
        assert_eq!(parse("calc(1px * 2px) 0 red"), None);
        assert_eq!(parse("calc(1px+2px) 0 red"), None);
        assert_eq!(parse("calc(1px / 0) 0 red"), None);
    }

    #[test]
    fn units_without_a_context_are_invalid() {
        for input in ["1em 1em red", "1rem 0 red", "1vw 0 red", "1vh 0 red"] {
            assert_eq!(parse(input), None, "{input}");
        }
    }

    #[test]
    fn a_huge_length_clamps_like_a_browser_instead_of_overflowing_f32() {
        let parsed = parse("1e300px -1e999px 1e300px 0 red").unwrap();
        assert_eq!(parsed[0].offset_x, dimension::MAX_LENGTH);
        assert_eq!(parsed[0].offset_y, -dimension::MAX_LENGTH);
        assert!((parsed[0].blur_radius as f32).is_finite());
    }
}
