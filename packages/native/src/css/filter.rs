//! The CSS `filter` and `backdrop-filter` strings: a list of filter functions,
//! parsed into a typed list and resolved into gpui's `FilterFunction`s.

use super::cache::Cache;
use super::dimension::{parse_length, parse_number_or_percentage, split_dimension};
use super::dimension::{parse_length, parse_non_negative_length, parse_number_or_percentage, split_dimension};
use super::length::LengthContext;
use super::syntax::{function_args, split_whitespace};
use crate::color::ColorContext;

/// The colour of a `drop-shadow()` that names none, as in CSS.
const DEFAULT_SHADOW_COLOR: &str = "currentColor";

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Filter {
    Blur(f64),
    Brightness(f64),
    Contrast(f64),
    Grayscale(f64),
    /// In degrees.
    HueRotate(f64),
    Invert(f64),
    Opacity(f64),
    Saturate(f64),
    Sepia(f64),
    DropShadow {
        offset_x: f64,
        offset_y: f64,
        blur: f64,
        /// Kept as text: it needs the element's `currentColor` and `colorScheme`.
        color: String,
    },
}

thread_local! {
    static CACHE: Cache<Option<Vec<Filter>>> = Cache::new();
}

/// `None` when the declaration is invalid, which CSS ignores as a whole. An
/// empty list is the valid `none`.
pub(crate) fn parse(value: &str, lengths: &LengthContext) -> Option<Vec<Filter>> {
    CACHE.with(|cache| cache.get_or_parse(value, || parse_uncached(value, lengths)))
}

fn parse_uncached(value: &str, lengths: &LengthContext) -> Option<Vec<Filter>> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    let tokens = split_whitespace(value);
    if tokens.is_empty() {
        return None;
    }
    tokens.into_iter().map(|token| parse_function(token, lengths)).collect()
}

pub(crate) fn parse_resolved(
    value: &str,
    colors: &ColorContext,
    lengths: &LengthContext,
) -> Option<Vec<gpui::FilterFunction>> {
    resolve(&parse(value, lengths)?, colors)
}

/// The list in gpui's terms. `None` when a `drop-shadow()` colour does not
/// resolve, so the whole declaration is ignored like any other invalid one.
pub(crate) fn resolve(
    filters: &[Filter],
    colors: &ColorContext,
) -> Option<Vec<gpui::FilterFunction>> {
    filters
        .iter()
        .map(|filter| {
            Some(match filter {
                Filter::Blur(radius) => gpui::FilterFunction::Blur(gpui::px(*radius as f32)),
                Filter::Brightness(amount) => gpui::FilterFunction::Brightness(*amount as f32),
                Filter::Contrast(amount) => gpui::FilterFunction::Contrast(*amount as f32),
                Filter::Grayscale(amount) => gpui::FilterFunction::Grayscale(*amount as f32),
                Filter::HueRotate(degrees) => gpui::FilterFunction::HueRotate(*degrees as f32),
                Filter::Invert(amount) => gpui::FilterFunction::Invert(*amount as f32),
                Filter::Opacity(amount) => gpui::FilterFunction::Opacity(*amount as f32),
                Filter::Saturate(amount) => gpui::FilterFunction::Saturate(*amount as f32),
                Filter::Sepia(amount) => gpui::FilterFunction::Sepia(*amount as f32),
                Filter::DropShadow {
                    offset_x,
                    offset_y,
                    blur,
                    color,
                } => gpui::FilterFunction::DropShadow {
                    offset: gpui::point(gpui::px(*offset_x as f32), gpui::px(*offset_y as f32)),
                    blur: gpui::px(*blur as f32),
                    color: colors.resolve(color)?.into(),
                },
            })
        })
        .collect()
}

/// True when a `drop-shadow()` takes its colour from `currentColor`, so a state
/// that changes `color` must repaint the filter.
pub(crate) fn follows_current_color(value: &str, lengths: &LengthContext) -> bool {
    parse(value, lengths).is_some_and(|filters| {
        filters.iter().any(|filter| {
            matches!(filter, Filter::DropShadow { color, .. }
                if color.to_ascii_lowercase().contains("currentcolor"))
        })
    })
}

fn parse_function(token: &str, lengths: &LengthContext) -> Option<Filter> {
    let name_end = token.find('(')?;
    let name = token[..name_end].to_ascii_lowercase();
    let args = function_args(token, &name)?.trim();
    // Each amount defaults when its argument is omitted: `brightness()` is `1`.
    let amount = |default: f64| -> Option<f64> {
        if args.is_empty() {
            Some(default)
        } else {
            parse_amount(args)
        }
    };
    Some(match name.as_str() {
        "blur" => Filter::Blur(if args.is_empty() {
            0.0
        } else {
            parse_non_negative_length(args, lengths)?
        }),
        "brightness" => Filter::Brightness(amount(1.0)?),
        "contrast" => Filter::Contrast(amount(1.0)?),
        "grayscale" => Filter::Grayscale(amount(1.0)?),
        "invert" => Filter::Invert(amount(1.0)?),
        "opacity" => Filter::Opacity(amount(1.0)?),
        "saturate" => Filter::Saturate(amount(1.0)?),
        "sepia" => Filter::Sepia(amount(1.0)?),
        "hue-rotate" => Filter::HueRotate(if args.is_empty() {
            0.0
        } else {
            parse_angle(args)?
        }),
        "drop-shadow" => parse_drop_shadow(args, lengths)?,
        _ => return None,
    })
}

fn parse_drop_shadow(args: &str, lengths: &LengthContext) -> Option<Filter> {
    #[derive(PartialEq)]
    enum Run {
        NotStarted,
        Open,
        Closed,
    }

    let mut color: Option<&str> = None;
    let mut length_tokens: Vec<&str> = Vec::new();
    // The lengths must be contiguous, so `1px red 2px` is invalid.
    let mut run = Run::NotStarted;
    for token in split_whitespace(args) {
        if parse_length(token, lengths).is_some() {
            if run == Run::Closed {
                return None;
            }
            run = Run::Open;
            length_tokens.push(token);
            continue;
        }
        if run == Run::Open {
            run = Run::Closed;
        }
        if color.replace(token).is_some() {
            return None;
        }
    }
    let (offset_x, offset_y, blur) = match length_tokens.as_slice() {
        [x, y] => (parse_length(x, lengths)?, parse_length(y, lengths)?, 0.0),
        [x, y, blur] => (
            parse_length(x, lengths)?,
            parse_length(y, lengths)?,
            parse_non_negative_length(blur, lengths)?,
        ),
        _ => return None,
    };
    Some(Filter::DropShadow {
        offset_x,
        offset_y,
        blur,
        color: color.unwrap_or(DEFAULT_SHADOW_COLOR).to_owned(),
    })
}

/// A non-negative number or percentage, `0.5` or `50%`.
fn parse_amount(token: &str) -> Option<f64> {
    parse_number_or_percentage(token).filter(|value| *value >= 0.0)
}

/// An angle in degrees. Unitless is only valid for zero.
fn parse_angle(token: &str) -> Option<f64> {
    let (value, unit) = split_dimension(token)?;
    match unit.as_str() {
        "deg" => Some(value),
        "grad" => Some(value * 0.9),
        "rad" => Some(value.to_degrees()),
        "turn" => Some(value * 360.0),
        "" => (value == 0.0).then_some(0.0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cached_parse_equals_the_uncached_one() {
        for value in [
            "blur(4px) brightness(1.1) drop-shadow(0 2px 4px rgba(0, 0, 0, .3))",
            "drop-shadow(1px 2px currentColor)",
            "none",
            "blur(-1px)",
            "sparkle(1)",
        ] {
            assert_eq!(parse(value), parse_uncached(value), "{value}");
            assert_eq!(parse(value), parse_uncached(value), "{value} on a hit");
        }
    }

    #[test]
    fn a_cached_drop_shadow_colour_still_follows_the_context() {
        let value = "drop-shadow(1px 2px currentColor)";
        let mut context = ColorContext::root(crate::color::Scheme::Light);
        let black = parse_resolved(value, &context, &LengthContext::for_tests()).unwrap();
        context.current = gpui::Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        let white = parse_resolved(value, &context, &LengthContext::for_tests()).unwrap();
        assert_ne!(black, white);
        assert_eq!(white, resolve(&parse_uncached(value).unwrap(), &context).unwrap());
    }

    fn parse(value: &str) -> Option<Vec<Filter>> {
        super::parse(value, &LengthContext::for_tests())
    }

    fn parse_uncached(value: &str) -> Option<Vec<Filter>> {
        super::parse_uncached(value, &LengthContext::for_tests())
    }

    fn follows_current_color(value: &str) -> bool {
        super::follows_current_color(value, &LengthContext::for_tests())
    }

    fn shadow(offset_x: f64, offset_y: f64, blur: f64, color: &str) -> Filter {
        Filter::DropShadow {
            offset_x,
            offset_y,
            blur,
            color: color.to_owned(),
        }
    }

    #[test]
    fn parses_a_list_of_functions_in_order() {
        assert_eq!(
            parse("blur(4px) brightness(1.5) grayscale(50%)"),
            Some(vec![
                Filter::Blur(4.0),
                Filter::Brightness(1.5),
                Filter::Grayscale(0.5),
            ])
        );
    }

    #[test]
    fn every_amount_function_takes_a_number_or_a_percentage() {
        assert_eq!(
            parse("contrast(200%) invert(.25) opacity(0) saturate(3) sepia(100%)"),
            Some(vec![
                Filter::Contrast(2.0),
                Filter::Invert(0.25),
                Filter::Opacity(0.0),
                Filter::Saturate(3.0),
                Filter::Sepia(1.0),
            ])
        );
    }

    #[test]
    fn omitted_arguments_take_the_css_defaults() {
        assert_eq!(
            parse("blur() brightness() hue-rotate() grayscale()"),
            Some(vec![
                Filter::Blur(0.0),
                Filter::Brightness(1.0),
                Filter::HueRotate(0.0),
                Filter::Grayscale(1.0),
            ])
        );
    }

    #[test]
    fn names_and_units_are_case_insensitive() {
        assert_eq!(
            parse("BLUR(2PX) Hue-Rotate(90DEG)"),
            Some(vec![Filter::Blur(2.0), Filter::HueRotate(90.0)])
        );
    }

    #[test]
    fn hue_rotate_converts_every_angle_unit_to_degrees() {
        let degrees = |value: &str| match parse(value).unwrap().as_slice() {
            [Filter::HueRotate(degrees)] => *degrees,
            other => panic!("{other:?}"),
        };
        assert_eq!(degrees("hue-rotate(90deg)"), 90.0);
        assert_eq!(degrees("hue-rotate(0.25turn)"), 90.0);
        assert_eq!(degrees("hue-rotate(100grad)"), 90.0);
        assert!((degrees("hue-rotate(3.14159265358979rad)") - 180.0).abs() < 1e-9);
        assert_eq!(degrees("hue-rotate(-45deg)"), -45.0);
        assert_eq!(degrees("hue-rotate(0)"), 0.0);
    }

    #[test]
    fn drop_shadow_defaults_to_current_color_and_zero_blur() {
        assert_eq!(parse("drop-shadow(2px 3px)"), Some(vec![shadow(2.0, 3.0, 0.0, "currentColor")]));
        assert_eq!(
            parse("drop-shadow(2px 3px 4px)"),
            Some(vec![shadow(2.0, 3.0, 4.0, "currentColor")])
        );
    }

    #[test]
    fn drop_shadow_colour_may_lead_or_trail_and_stays_whole() {
        let expected = Some(vec![shadow(1.0, -2.0, 3.0, "red")]);
        assert_eq!(parse("drop-shadow(1px -2px 3px red)"), expected);
        assert_eq!(parse("drop-shadow(red 1px -2px 3px)"), expected);
        assert_eq!(
            parse("drop-shadow(0 0 4px rgb(0 0 0 / 50%)) blur(1px)"),
            Some(vec![shadow(0.0, 0.0, 4.0, "rgb(0 0 0 / 50%)"), Filter::Blur(1.0)])
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
            "blur",
            "blur(2)",
            "blur(-1px)",
            "blur(1ch)",
            "blur(50%)",
            "blur(0%)",
            "blur(calc(1em + 10%))",
            "blur(-1rem)",
            "drop-shadow(1em 50% red)",
            "drop-shadow(1px 2px -1em)",
            "blur(1px, 2px)",
            "blur(1px) none",
            "blur(1px),blur(2px)",
            "brightness(-1)",
            "brightness(red)",
            "brightness(1 2)",
            "hue-rotate(90)",
            "hue-rotate(90px)",
            "drop-shadow()",
            "drop-shadow(red)",
            "drop-shadow(1px)",
            "drop-shadow(1px 2px 3px 4px)",
            "drop-shadow(1px 2px -3px)",
            "drop-shadow(1px red 2px)",
            "drop-shadow(1px 2px red blue)",
            "url(#filter)",
            "sparkle(1)",
            "blur (1px)",
        ] {
            assert_eq!(parse(input), None, "{input}");
        }
    }

    #[test]
    fn numbers_with_exponents_and_signs() {
        assert_eq!(parse("brightness(1e1)"), Some(vec![Filter::Brightness(10.0)]));
        assert_eq!(parse("blur(+.5px)"), Some(vec![Filter::Blur(0.5)]));
    }

    #[test]
    fn drop_shadow_follows_the_current_colour_only_when_it_uses_it() {
        assert!(follows_current_color("drop-shadow(1px 1px)"));
        assert!(follows_current_color("blur(1px) drop-shadow(1px 1px currentColor)"));
        assert!(!follows_current_color("drop-shadow(1px 1px red)"));
        assert!(!follows_current_color("blur(2px)"));
    }

    #[test]
    fn resolve_uses_the_context_colour_and_rejects_an_unresolvable_one() {
        let colors = ColorContext::root(crate::color::Scheme::Light);
        let filters = parse("drop-shadow(1px 2px 3px) contrast(2)").unwrap();
        let resolved = resolve(&filters, &colors).unwrap();
        assert_eq!(
            resolved[0],
            gpui::FilterFunction::DropShadow {
                offset: gpui::point(gpui::px(1.0), gpui::px(2.0)),
                blur: gpui::px(3.0),
                color: gpui::Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }.into(),
            }
        );
        assert_eq!(resolved[1], gpui::FilterFunction::Contrast(2.0));
        let bad = parse("drop-shadow(1px 2px notacolour)").unwrap();
        assert_eq!(resolve(&bad, &colors), None);
    }

    #[test]
    fn calc_and_absolute_units_in_lengths_and_amounts() {
        assert_eq!(
            parse("blur(calc(1px + 1in)) brightness(calc(1 + 0.5)) opacity(calc(2 * 25%))"),
            Some(vec![Filter::Blur(97.0), Filter::Brightness(1.5), Filter::Opacity(0.5)])
        );
        assert_eq!(parse("drop-shadow(calc(1px * 2) 2PT red)"), Some(vec![shadow(2.0, 2.0 * 96.0 / 72.0, 0.0, "red")]));
        assert_eq!(parse("blur(calc(1px - 2px))"), None);
        assert_eq!(parse("brightness(calc(1px))"), None);
    }

    #[test]
    fn lengths_resolve_against_the_element() {
        assert_eq!(
            parse("blur(0.5em) blur(calc(1rem + 2px)) blur(10vw)"),
            Some(vec![Filter::Blur(10.0), Filter::Blur(12.0), Filter::Blur(80.0)])
        );
        assert_eq!(
            parse("drop-shadow(1em -1rem 0.25em red)"),
            Some(vec![shadow(20.0, -10.0, 5.0, "red")])
        );
        // A calc() that only comes out negative here clamps, as CSS clamps a blur radius.
        assert_eq!(parse("blur(calc(1em - 30px))"), Some(vec![Filter::Blur(0.0)]));
        let colors = ColorContext::root(crate::color::Scheme::Light);
        let lengths = LengthContext::for_tests().descend(Some(&crate::css::length::LinearLength::px(8.0)), false);
        assert_eq!(
            parse_resolved("blur(0.5em)", &colors, &lengths),
            Some(vec![gpui::FilterFunction::Blur(gpui::px(4.0))])
        );
    }
}
