//! CSS gradient functions: `linear-`, `radial-` and `conic-gradient()` and their
//! `repeating-` forms, parsed into gpui's `Gradient`.

use gpui::{
    BackgroundPosition, ColorSpace, Gradient, GradientItem, GradientShape, GradientStop,
    LengthPercentage, LinearDirection, RadialShape, RadialSize, StopPosition,
};

use super::cache::{Cache, Inputs};
use super::length::LengthContext;
use super::position::{parse_angle, parse_length_percentage, parse_non_negative, parse_position};
use super::syntax::{function_args, split_commas, split_whitespace};
use crate::color::ColorContext;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GradientError {
    /// Not a valid gradient. CSS ignores the whole declaration.
    Invalid,
    /// Valid CSS that gpui cannot paint: an interpolation space other than
    /// `srgb` or `oklab`.
    Unsupported,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Linear,
    Radial,
    Conic,
}

const FUNCTIONS: [(&str, Kind, bool); 6] = [
    ("linear-gradient", Kind::Linear, false),
    ("repeating-linear-gradient", Kind::Linear, true),
    ("radial-gradient", Kind::Radial, false),
    ("repeating-radial-gradient", Kind::Radial, true),
    ("conic-gradient", Kind::Conic, false),
    ("repeating-conic-gradient", Kind::Conic, true),
];

/// Whether `value` is a gradient function call, valid or not.
pub(crate) fn is_gradient(value: &str) -> bool {
    FUNCTIONS
        .iter()
        .any(|(name, _, _)| function_args(value, name).is_some())
}

thread_local! {
    static CACHE: Cache<Result<Gradient, GradientError>> = Cache::new();
}

/// Parses one gradient. Colours resolve against `colors`, so `currentColor`,
/// `light-dark()` and `color-mix()` work in stops, and lengths against `lengths`.
pub(crate) fn parse(
    value: &str,
    colors: &ColorContext,
    lengths: &LengthContext,
) -> Result<Gradient, GradientError> {
    let inputs = Inputs { colors: Some(colors), lengths };
    CACHE.with(|cache| cache.get_or_parse(value, inputs, || parse_uncached(value, colors, lengths)))
}

pub(super) fn parse_uncached(
    value: &str,
    colors: &ColorContext,
    lengths: &LengthContext,
) -> Result<Gradient, GradientError> {
    let (args, kind, repeating) = FUNCTIONS
        .iter()
        .find_map(|(name, kind, repeating)| {
            function_args(value, name).map(|args| (args, *kind, *repeating))
        })
        .ok_or(GradientError::Invalid)?;

    let pieces = split_commas(args);
    let (header, stops) = match pieces.split_first() {
        Some((first, rest)) if is_header(first, kind, lengths) => (*first, rest),
        _ => ("", pieces.as_slice()),
    };
    let (shape, color_space) = parse_header(header, kind, lengths)?;
    let items = parse_stops(stops, kind, colors, lengths)?;

    let stop_count = items
        .iter()
        .filter(|item| matches!(item, GradientItem::Stop(_)))
        .count();
    if stop_count < 2
        || !matches!(items.first(), Some(GradientItem::Stop(_)))
        || !matches!(items.last(), Some(GradientItem::Stop(_)))
    {
        return Err(GradientError::Invalid);
    }
    if repeating {
        if let Some(average) = zero_length_average(&items, kind) {
            return Ok(Gradient {
                shape,
                items: vec![GradientItem::Stop(average); 2],
                repeating: false,
                color_space,
            });
        }
    }
    Ok(Gradient {
        shape,
        items,
        repeating,
        color_space,
    })
}

/// A position with the unit it is in, so two can be compared when they are in
/// the same one. A conic gradient's percentages are fractions of a turn.
#[derive(Clone, Copy)]
enum Placed {
    Fraction(f64),
    Length(f64),
    Angle(f64),
}

impl Placed {
    /// `None` for a sum of a length and a fraction, which is in no one unit.
    fn of(position: StopPosition, kind: Kind) -> Option<Self> {
        Some(match position {
            StopPosition::Fraction(fraction) if kind == Kind::Conic => {
                Self::Angle(f64::from(fraction) * 360.0)
            }
            StopPosition::Fraction(fraction) => Self::Fraction(f64::from(fraction)),
            StopPosition::Length(length) => Self::Length(f64::from(f32::from(length))),
            StopPosition::Calc { .. } => return None,
            StopPosition::Angle(degrees) => Self::Angle(f64::from(degrees)),
        })
    }

    fn parts(self) -> (u8, f64) {
        match self {
            Self::Fraction(value) => (0, value),
            Self::Length(value) => (1, value),
            Self::Angle(value) => (2, value),
        }
    }

    /// Zero is zero in every unit, which is the only thing two units share.
    fn is_at_most(self, other: Self) -> Option<bool> {
        let ((unit, value), (other_unit, other_value)) = (self.parts(), other.parts());
        if unit == other_unit {
            Some(value <= other_value)
        } else if value == 0.0 {
            Some(0.0 <= other_value)
        } else if other_value == 0.0 {
            Some(value <= 0.0)
        } else {
            None
        }
    }
}

/// CSS renders a repeating gradient whose first and last stop coincide as the
/// average colour of its stops, because a cycle of no length has no gradient to
/// draw. gpui would draw a hard edge instead.
///
/// The cycle length is the last stop's position minus the first's, after every
/// position is raised to the one before it. Without the box, `10px` and `5%`
/// cannot be compared, so such a gradient is left to gpui.
fn zero_length_average(items: &[GradientItem], kind: Kind) -> Option<GradientStop> {
    let stops: Vec<&GradientStop> = items
        .iter()
        .filter_map(|item| match item {
            GradientItem::Stop(stop) => Some(stop),
            GradientItem::Hint(_) => None,
        })
        .collect();
    let (first, rest) = stops.split_first()?;
    let last_default = match kind {
        Kind::Conic => Placed::Angle(360.0),
        _ => Placed::Fraction(1.0),
    };
    // The first stop defaults to the start of the line, which is zero in any unit.
    let start = match first.position {
        Some(position) => Placed::of(position, kind)?,
        None => Placed::Length(0.0),
    };
    for (index, stop) in rest.iter().enumerate() {
        let position = match (stop.position, index + 1 == rest.len()) {
            (Some(position), _) => Placed::of(position, kind)?,
            (None, true) => last_default,
            // Stops between the first and the last sit between them.
            (None, false) => continue,
        };
        if !position.is_at_most(start)? {
            return None;
        }
    }

    // The average of the stops spaced evenly: an end counts for half of a middle stop.
    let spans = (stops.len() - 1) as f32;
    let (mut red, mut green, mut blue, mut alpha) = (0.0, 0.0, 0.0, 0.0);
    for (index, stop) in stops.iter().enumerate() {
        let weight = if index == 0 || index + 1 == stops.len() { 0.5 } else { 1.0 } / spans;
        let colour = gpui::Rgba::from(stop.color);
        alpha += colour.a * weight;
        red += colour.r * colour.a * weight;
        green += colour.g * colour.a * weight;
        blue += colour.b * colour.a * weight;
    }
    let average = if alpha > 0.0 {
        gpui::Rgba { r: red / alpha, g: green / alpha, b: blue / alpha, a: alpha }
    } else {
        gpui::Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }
    };
    Some(GradientStop { color: average.into(), position: None })
}

fn is_header(first: &str, kind: Kind, lengths: &LengthContext) -> bool {
    let Some(word) = split_whitespace(first).first().map(|word| word.to_ascii_lowercase()) else {
        return false;
    };
    if matches!(word.as_str(), "in" | "at") {
        return true;
    }
    match kind {
        Kind::Linear => word == "to" || parse_angle(&word).is_some(),
        Kind::Radial => {
            matches!(
                word.as_str(),
                "circle" | "ellipse" | "closest-side" | "closest-corner" | "farthest-side" | "farthest-corner"
            ) || parse_length_percentage(&word, lengths).is_some()
        }
        Kind::Conic => word == "from",
    }
}

fn parse_header(
    header: &str,
    kind: Kind,
    lengths: &LengthContext,
) -> Result<(GradientShape, ColorSpace), GradientError> {
    let tokens = split_whitespace(header);
    let mut color_space = ColorSpace::Srgb;
    let mut position_tokens: Option<Vec<&str>> = None;
    let mut rest: Vec<&str> = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        match token.to_ascii_lowercase().as_str() {
            "in" => {
                let space = tokens.get(index + 1).ok_or(GradientError::Invalid)?;
                color_space = match space.to_ascii_lowercase().as_str() {
                    "srgb" => ColorSpace::Srgb,
                    "oklab" => ColorSpace::Oklab,
                    "srgb-linear" | "display-p3" | "a98-rgb" | "prophoto-rgb" | "rec2020" | "lab" | "oklch"
                    | "lch" | "hsl" | "hwb" | "xyz" | "xyz-d50" | "xyz-d65" => {
                        return Err(GradientError::Unsupported)
                    }
                    _ => return Err(GradientError::Invalid),
                };
                index += 2;
            }
            "at" if kind != Kind::Linear => {
                // The position runs to the next `in`.
                let end = tokens[index + 1..]
                    .iter()
                    .position(|token| token.eq_ignore_ascii_case("in"))
                    .map_or(tokens.len(), |offset| index + 1 + offset);
                position_tokens = Some(tokens[index + 1..end].to_vec());
                index = end;
            }
            _ => {
                rest.push(token);
                index += 1;
            }
        }
    }
    let position = match &position_tokens {
        Some(tokens) => parse_position(tokens, lengths).ok_or(GradientError::Invalid)?,
        None => BackgroundPosition::CENTER,
    };

    let shape = match kind {
        Kind::Linear => {
            if position_tokens.is_some() {
                return Err(GradientError::Invalid);
            }
            GradientShape::Linear(parse_direction(&rest)?)
        }
        Kind::Radial => {
            let (shape, size) = parse_radial_size(&rest, lengths)?;
            GradientShape::Radial { shape, size, position }
        }
        Kind::Conic => {
            let from = match rest.as_slice() {
                [] => 0.0,
                [from, angle] if from.eq_ignore_ascii_case("from") => {
                    parse_angle(angle).ok_or(GradientError::Invalid)?
                }
                _ => return Err(GradientError::Invalid),
            };
            GradientShape::Conic { from, position }
        }
    };
    Ok((shape, color_space))
}

fn parse_direction(tokens: &[&str]) -> Result<LinearDirection, GradientError> {
    let lower: Vec<String> = tokens.iter().map(|token| token.to_ascii_lowercase()).collect();
    let words: Vec<&str> = lower.iter().map(String::as_str).collect();
    Ok(match words.as_slice() {
        [] => LinearDirection::ToBottom,
        [angle] => LinearDirection::Angle(parse_angle(angle).ok_or(GradientError::Invalid)?),
        ["to", "top"] => LinearDirection::ToTop,
        ["to", "right"] => LinearDirection::ToRight,
        ["to", "bottom"] => LinearDirection::ToBottom,
        ["to", "left"] => LinearDirection::ToLeft,
        ["to", "top", "right"] | ["to", "right", "top"] => LinearDirection::ToTopRight,
        ["to", "bottom", "right"] | ["to", "right", "bottom"] => LinearDirection::ToBottomRight,
        ["to", "bottom", "left"] | ["to", "left", "bottom"] => LinearDirection::ToBottomLeft,
        ["to", "top", "left"] | ["to", "left", "top"] => LinearDirection::ToTopLeft,
        _ => return Err(GradientError::Invalid),
    })
}

fn parse_radial_size(
    tokens: &[&str],
    lengths: &LengthContext,
) -> Result<(RadialShape, RadialSize), GradientError> {
    let mut shape = None;
    let mut keyword = None;
    let mut radii: Vec<LengthPercentage> = Vec::new();
    for token in tokens {
        match token.to_ascii_lowercase().as_str() {
            "circle" => {
                if shape.replace(RadialShape::Circle).is_some() {
                    return Err(GradientError::Invalid);
                }
            }
            "ellipse" => {
                if shape.replace(RadialShape::Ellipse).is_some() {
                    return Err(GradientError::Invalid);
                }
            }
            "closest-side" | "closest-corner" | "farthest-side" | "farthest-corner" => {
                let size = match token.to_ascii_lowercase().as_str() {
                    "closest-side" => RadialSize::ClosestSide,
                    "closest-corner" => RadialSize::ClosestCorner,
                    "farthest-side" => RadialSize::FarthestSide,
                    _ => RadialSize::FarthestCorner,
                };
                if keyword.replace(size).is_some() {
                    return Err(GradientError::Invalid);
                }
            }
            _ => radii.push(parse_non_negative(token, lengths).ok_or(GradientError::Invalid)?),
        }
    }
    if keyword.is_some() && !radii.is_empty() {
        return Err(GradientError::Invalid);
    }
    if let Some(size) = keyword {
        return Ok((shape.unwrap_or(RadialShape::Ellipse), size));
    }
    match (radii.as_slice(), shape) {
        ([], shape) => Ok((shape.unwrap_or(RadialShape::Ellipse), RadialSize::FarthestCorner)),
        // A circle's radius is a length, never a percentage.
        ([radius @ LengthPercentage::Length(_)], None | Some(RadialShape::Circle)) => {
            Ok((RadialShape::Circle, RadialSize::Explicit { x: *radius, y: *radius }))
        }
        ([x, y], None | Some(RadialShape::Ellipse)) => {
            Ok((RadialShape::Ellipse, RadialSize::Explicit { x: *x, y: *y }))
        }
        _ => Err(GradientError::Invalid),
    }
}

fn parse_stop_position(token: &str, kind: Kind, lengths: &LengthContext) -> Option<StopPosition> {
    if kind == Kind::Conic {
        if let Some(angle) = parse_angle(token) {
            // `0` alone is an angle here, but `0%` is a fraction.
            return Some(StopPosition::Angle(angle));
        }
    }
    match parse_length_percentage(token, lengths)? {
        LengthPercentage::Percent(fraction) => Some(StopPosition::Fraction(fraction)),
        LengthPercentage::Length(length) if kind != Kind::Conic => Some(StopPosition::Length(length)),
        LengthPercentage::Calc { length, percent } if kind != Kind::Conic => {
            Some(StopPosition::Calc { length, fraction: percent })
        }
        LengthPercentage::Length(_) | LengthPercentage::Calc { .. } => None,
    }
}

fn parse_stops(
    pieces: &[&str],
    kind: Kind,
    colors: &ColorContext,
    lengths: &LengthContext,
) -> Result<Vec<GradientItem>, GradientError> {
    let mut items = Vec::new();
    for piece in pieces {
        let tokens = split_whitespace(piece);
        let (color_token, positions) = tokens.split_first().ok_or(GradientError::Invalid)?;
        // A lone position is a colour hint.
        if positions.is_empty() && colors.resolve(color_token).is_none() {
            let position = parse_stop_position(color_token, kind, lengths).ok_or(GradientError::Invalid)?;
            // A hint sits between two stops, so two in a row have none between them.
            if matches!(items.last(), Some(GradientItem::Hint(_))) {
                return Err(GradientError::Invalid);
            }
            items.push(GradientItem::Hint(position));
            continue;
        }
        let color = colors.resolve(color_token).ok_or(GradientError::Invalid)?;
        let positions: Vec<Option<StopPosition>> = match positions {
            [] => vec![None],
            [one] => vec![Some(parse_stop_position(one, kind, lengths).ok_or(GradientError::Invalid)?)],
            [first, second] => vec![
                Some(parse_stop_position(first, kind, lengths).ok_or(GradientError::Invalid)?),
                Some(parse_stop_position(second, kind, lengths).ok_or(GradientError::Invalid)?),
            ],
            _ => return Err(GradientError::Invalid),
        };
        for position in positions {
            items.push(GradientItem::Stop(GradientStop {
                color: color.into(),
                position,
            }));
        }
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::px;
    use crate::css::length::LinearLength;
    use crate::color::Scheme;

    fn colors(scheme: Scheme, current: f32) -> ColorContext {
        let mut context = ColorContext::root(scheme);
        context.scheme = scheme;
        context.current = gpui::Rgba { r: current, g: current, b: current, a: 1.0 };
        context
    }

    #[test]
    fn a_cached_parse_equals_the_uncached_one() {
        let context = colors(Scheme::Light, 0.0);
        for value in [
            "linear-gradient(135deg, #ff7e5f 0%, rgba(10, 20, 30, 0.8) 100%)",
            "radial-gradient(circle at 30% 20%, red, 20%, blue)",
            "repeating-conic-gradient(from 90deg, red, blue 30deg)",
            "linear-gradient(in oklch, red, blue)",
            "linear-gradient(red)",
            "not-a-gradient(red, blue)",
        ] {
            let uncached = parse_uncached(value, &context, &LengthContext::for_tests());
            assert_eq!(parse(value, &context, &LengthContext::for_tests()), uncached, "{value}");
            assert_eq!(parse(value, &context, &LengthContext::for_tests()), uncached, "{value} on a hit");
        }
    }

    #[test]
    fn a_cached_gradient_is_not_shared_across_colour_contexts() {
        for value in [
            "linear-gradient(currentColor, blue)",
            "linear-gradient(light-dark(red, lime), blue)",
            "linear-gradient(color-mix(in srgb, currentColor, red), blue)",
            "linear-gradient(red 0%, light-dark(red, lime) 50%, blue)",
        ] {
            let contexts = [
                colors(Scheme::Light, 0.0),
                colors(Scheme::Light, 1.0),
                colors(Scheme::Dark, 0.0),
                colors(Scheme::Dark, 1.0),
            ];
            // Twice, so every context is also read back from a hit.
            for _ in 0..2 {
                for context in &contexts {
                    assert_eq!(parse(value, context, &LengthContext::for_tests()), parse_uncached(value, context, &LengthContext::for_tests()), "{value}");
                }
            }
            let distinct = contexts
                .iter()
                .map(|context| parse(value, context, &LengthContext::for_tests()).unwrap())
                .fold(Vec::<Gradient>::new(), |mut seen, gradient| {
                    if !seen.contains(&gradient) {
                        seen.push(gradient);
                    }
                    seen
                });
            assert!(distinct.len() >= 2, "{value} resolved alike in every context");
        }
    }

    #[test]
    fn a_cached_gradient_is_not_shared_across_length_contexts() {
        let colors = ColorContext::root(Scheme::Light);
        let base = LengthContext::for_tests();
        let large_font = base.descend(Some(&LinearLength::px(40.0)), false);
        let large_root = base.descend(Some(&LinearLength::px(30.0)), true);
        let wide = base.with_viewport(1600.0, 900.0);
        let contexts = [base, large_font, large_root, wide];
        for value in [
            "linear-gradient(red 1em, blue)",
            "linear-gradient(red calc(10px + 0.5em), blue)",
            "linear-gradient(red 1rem, blue)",
            "linear-gradient(red 10vw, blue 5vh)",
            "radial-gradient(circle 2em at 1rem 10vh, red, blue)",
        ] {
            // Twice, so every context is also read back from a hit.
            for _ in 0..2 {
                for lengths in &contexts {
                    assert_eq!(parse(value, &colors, lengths), parse_uncached(value, &colors, lengths), "{value}");
                }
            }
            let distinct = contexts
                .iter()
                .map(|lengths| parse(value, &colors, lengths).unwrap())
                .fold(Vec::<Gradient>::new(), |mut seen, gradient| {
                    if !seen.contains(&gradient) {
                        seen.push(gradient);
                    }
                    seen
                });
            assert!(distinct.len() >= 2, "{value} resolved alike in every length context");
        }
    }

    #[test]
    fn a_gradient_without_a_relative_unit_is_one_result_for_every_length_context() {
        let colors = ColorContext::root(Scheme::Light);
        let base = LengthContext::for_tests();
        let value = "linear-gradient(red 20px, blue 80%)";
        let first = parse(value, &colors, &base);
        let other = parse(value, &colors, &base.with_viewport(1.0, 1.0));
        assert_eq!(first, other);
        assert_eq!(first, parse_uncached(value, &colors, &base));
    }

    fn parsed(value: &str) -> Gradient {
        parse(value, &ColorContext::root(Scheme::Light), &LengthContext::for_tests()).unwrap_or_else(|error| panic!("{value}: {error:?}"))
    }

    fn error(value: &str) -> GradientError {
        parse(value, &ColorContext::root(Scheme::Light), &LengthContext::for_tests()).unwrap_err()
    }

    fn stop(item: &GradientItem) -> (f32, Option<StopPosition>) {
        match item {
            GradientItem::Stop(stop) => (stop.color.h, stop.position),
            GradientItem::Hint(_) => panic!("a hint, not a stop"),
        }
    }

    #[test]
    fn linear_directions() {
        for (value, expected) in [
            ("linear-gradient(red, blue)", LinearDirection::ToBottom),
            ("linear-gradient(45deg, red, blue)", LinearDirection::Angle(45.0)),
            ("linear-gradient(0.25turn, red, blue)", LinearDirection::Angle(90.0)),
            ("linear-gradient(to right, red, blue)", LinearDirection::ToRight),
            ("linear-gradient(to left top, red, blue)", LinearDirection::ToTopLeft),
            ("linear-gradient(to bottom right, red, blue)", LinearDirection::ToBottomRight),
        ] {
            assert_eq!(parsed(value).shape, GradientShape::Linear(expected), "{value}");
        }
    }

    #[test]
    fn interpolation_space_in_either_position() {
        assert_eq!(parsed("linear-gradient(in oklab, red, blue)").color_space, ColorSpace::Oklab);
        assert_eq!(parsed("linear-gradient(to right in oklab, red, blue)").color_space, ColorSpace::Oklab);
        assert_eq!(parsed("linear-gradient(in oklab to right, red, blue)").color_space, ColorSpace::Oklab);
        assert_eq!(parsed("linear-gradient(in srgb, red, blue)").color_space, ColorSpace::Srgb);
        assert_eq!(error("linear-gradient(in oklch, red, blue)"), GradientError::Unsupported);
        assert_eq!(error("linear-gradient(in nonsense, red, blue)"), GradientError::Invalid);
    }

    #[test]
    fn stops_positions_double_positions_and_hints() {
        let gradient = parsed("linear-gradient(red 0%, 30%, blue 50% 80%, green 100px)");
        assert_eq!(gradient.items.len(), 5);
        assert_eq!(stop(&gradient.items[0]).1, Some(StopPosition::Fraction(0.0)));
        assert_eq!(gradient.items[1], GradientItem::Hint(StopPosition::Fraction(0.3)));
        assert_eq!(stop(&gradient.items[2]).1, Some(StopPosition::Fraction(0.5)));
        assert_eq!(stop(&gradient.items[3]).1, Some(StopPosition::Fraction(0.8)));
        assert_eq!(stop(&gradient.items[4]).1, Some(StopPosition::Length(px(100.0))));
        assert_eq!(stop(&parsed("linear-gradient(red, blue)").items[0]).1, None);
    }

    #[test]
    fn stop_and_radius_lengths_use_the_element_context() {
        let gradient = parsed("linear-gradient(red 2em, blue calc(1rem + 10%), green 10vw)");
        let positions: Vec<_> = gradient.items.iter().map(|item| stop(item).1).collect();
        assert_eq!(
            positions,
            vec![
                Some(StopPosition::Length(px(40.0))),
                Some(StopPosition::Calc { length: px(10.0), fraction: 0.1 }),
                Some(StopPosition::Length(px(80.0))),
            ]
        );
        let hint = parsed("linear-gradient(red, calc(1em + 25%), blue)");
        assert_eq!(hint.items[1], GradientItem::Hint(StopPosition::Calc { length: px(20.0), fraction: 0.25 }));
        let GradientShape::Radial { size, .. } = parsed("radial-gradient(circle 1em at 2rem 10vh, red, blue)").shape
        else {
            panic!("not radial");
        };
        assert_eq!(size, RadialSize::Explicit { x: LengthPercentage::Length(px(20.0)), y: LengthPercentage::Length(px(20.0)) });
        // A sum with a fraction is no length for a circle's radius, nor a position in a conic gradient.
        for value in [
            "radial-gradient(circle calc(1em + 10%), red, blue)",
            "conic-gradient(red calc(1em + 10%), blue)",
            "conic-gradient(red 1em, blue)",
        ] {
            assert_eq!(error(value), GradientError::Invalid, "{value}");
        }
    }

    #[test]
    fn a_repeating_gradient_with_a_sum_for_a_position_is_left_to_gpui() {
        let gradient = parsed("repeating-linear-gradient(red 0, blue calc(1em + 10%))");
        assert_eq!(gradient.items.len(), 2);
        assert_eq!(stop(&gradient.items[1]).1, Some(StopPosition::Calc { length: px(20.0), fraction: 0.1 }));
    }

    #[test]
    fn stop_colours_use_the_element_context() {
        let mut context = ColorContext::root(Scheme::Dark);
        context.scheme = Scheme::Dark;
        context.current = gpui::Rgba { r: 0.0, g: 0.0, b: 1.0, a: 1.0 };
        let gradient = parse(
            "linear-gradient(currentColor, light-dark(red, green), color-mix(in srgb, red, red))",
            &context,
            &LengthContext::for_tests(),
        )
        .unwrap();
        let hues: Vec<f32> = gradient.items.iter().map(|item| stop(item).0).collect();
        assert!((hues[0] - gpui::Hsla::from(context.current).h).abs() < 1e-6);
        assert!((hues[1] - gpui::Hsla::from(gpui::Rgba { r: 0.0, g: 0.5019608, b: 0.0, a: 1.0 }).h).abs() < 1e-3);
        assert_eq!(hues[2], 0.0);
    }

    #[test]
    fn repeating_and_radial() {
        assert!(parsed("repeating-linear-gradient(red 0 10px, blue 10px 20px)").repeating);
        assert!(!parsed("linear-gradient(red, blue)").repeating);
        let default = parsed("radial-gradient(red, blue)");
        assert_eq!(
            default.shape,
            GradientShape::Radial {
                shape: RadialShape::Ellipse,
                size: RadialSize::FarthestCorner,
                position: BackgroundPosition::CENTER
            }
        );
        let circle = parsed("radial-gradient(circle closest-side at 20% 30px, red, blue)");
        assert_eq!(
            circle.shape,
            GradientShape::Radial {
                shape: RadialShape::Circle,
                size: RadialSize::ClosestSide,
                position: BackgroundPosition::new(LengthPercentage::Percent(0.2), LengthPercentage::Length(px(30.0)))
            }
        );
        let explicit = parsed("radial-gradient(40px, red, blue)");
        let GradientShape::Radial { shape, size, .. } = explicit.shape else { panic!() };
        assert_eq!(shape, RadialShape::Circle);
        assert_eq!(size, RadialSize::Explicit { x: LengthPercentage::Length(px(40.0)), y: LengthPercentage::Length(px(40.0)) });
        let ellipse = parsed("radial-gradient(50% 30%, red, blue)");
        let GradientShape::Radial { shape, size, .. } = ellipse.shape else { panic!() };
        assert_eq!(shape, RadialShape::Ellipse);
        assert_eq!(size, RadialSize::Explicit { x: LengthPercentage::Percent(0.5), y: LengthPercentage::Percent(0.3) });
        assert_eq!(error("radial-gradient(circle 50%, red, blue)"), GradientError::Invalid);
        assert_eq!(error("radial-gradient(closest-side 10px, red, blue)"), GradientError::Invalid);
    }

    #[test]
    fn conic() {
        let gradient = parsed("conic-gradient(from 90deg at 25% 75%, red 0deg, blue 0.5turn, red 100%)");
        assert_eq!(
            gradient.shape,
            GradientShape::Conic {
                from: 90.0,
                position: BackgroundPosition::new(LengthPercentage::Percent(0.25), LengthPercentage::Percent(0.75))
            }
        );
        assert_eq!(stop(&gradient.items[1]).1, Some(StopPosition::Angle(180.0)));
        assert_eq!(stop(&gradient.items[2]).1, Some(StopPosition::Fraction(1.0)));
        assert_eq!(
            parsed("conic-gradient(red, blue)").shape,
            GradientShape::Conic { from: 0.0, position: BackgroundPosition::CENTER }
        );
        assert_eq!(error("conic-gradient(red 10px, blue)"), GradientError::Invalid);
    }

    #[test]
    fn rejects_malformed_gradients() {
        for value in [
            "linear-gradient()",
            "linear-gradient(red)",
            "linear-gradient(to sideways, red, blue)",
            "linear-gradient(red, 50%)",
            "linear-gradient(50%, red, blue)",
            "linear-gradient(red 1 2, blue)",
            "linear-gradient(notacolor, blue)",
            "linear-gradient(red, blue",
            "linear-gradient(at center, red, blue)",
            "image(red)",
        ] {
            assert_eq!(
                parse(value, &ColorContext::root(Scheme::Light), &LengthContext::for_tests()).err(),
                Some(GradientError::Invalid),
                "{value}"
            );
        }
        assert!(is_gradient("conic-gradient(red, blue)"));
        assert!(!is_gradient("url(x.png)"));
    }

    #[test]
    fn rejects_grammar_the_parser_used_to_let_through() {
        for value in [
            // Two hints in a row have no stop between them.
            "linear-gradient(red, 30%, 60%, blue)",
            "conic-gradient(red, 10deg, 20deg, blue)",
            // One size keyword at most.
            "radial-gradient(closest-side farthest-corner, red, blue)",
            "radial-gradient(circle closest-side closest-side, red, blue)",
            // Radii cannot be negative.
            "radial-gradient(circle -5px, red, blue)",
            "radial-gradient(-10px 20px, red, blue)",
            "radial-gradient(10px -20%, red, blue)",
            "radial-gradient(ellipse 5px -1px at center, red, blue)",
        ] {
            assert_eq!(error(value), GradientError::Invalid, "{value}");
        }
        // A hint between two stops, and a zero radius, stay valid.
        parsed("linear-gradient(red, 30%, blue, 60%, green)");
        parsed("radial-gradient(0px, red, blue)");
    }

    fn solid_colour(gradient: &Gradient) -> gpui::Rgba {
        assert!(!gradient.repeating);
        let colours: Vec<gpui::Rgba> = gradient
            .items
            .iter()
            .map(|item| match item {
                GradientItem::Stop(stop) => {
                    assert_eq!(stop.position, None);
                    stop.color.into()
                }
                GradientItem::Hint(_) => panic!("a hint"),
            })
            .collect();
        assert!(colours.windows(2).all(|pair| pair[0] == pair[1]), "{colours:?}");
        colours[0]
    }

    fn assert_rgb(colour: gpui::Rgba, expected: [f32; 3]) {
        for (got, want) in [colour.r, colour.g, colour.b].into_iter().zip(expected) {
            assert!((got - want).abs() < 1e-3, "{colour:?} vs {expected:?}");
        }
    }

    #[test]
    fn a_zero_length_repeating_cycle_is_the_average_colour() {
        // The CSS Images example: rgb(75%, 50%, 75%).
        assert_rgb(
            solid_colour(&parsed("repeating-linear-gradient(red 0px, white 0px, blue 0px)")),
            [0.75, 0.5, 0.75],
        );
        assert_rgb(
            solid_colour(&parsed("repeating-linear-gradient(red 10px, blue 10px)")),
            [0.5, 0.0, 0.5],
        );
        // The last stop defaults to 100%, which is where the first one is.
        assert_rgb(
            solid_colour(&parsed("repeating-radial-gradient(red 100%, blue)")),
            [0.5, 0.0, 0.5],
        );
        assert_rgb(
            solid_colour(&parsed("repeating-conic-gradient(red 0deg, blue 0turn)")),
            [0.5, 0.0, 0.5],
        );
        // Stops whose positions fall back to the first one clamp onto it.
        assert_rgb(
            solid_colour(&parsed("repeating-linear-gradient(red 20px, blue 5px, blue 20px)")),
            [0.25, 0.0, 0.75],
        );
        // Hints take no part, and a transparent stop weighs by its alpha.
        assert_rgb(
            solid_colour(&parsed("repeating-linear-gradient(red 0, 0, blue 0)")),
            [0.5, 0.0, 0.5],
        );
        let faded = solid_colour(&parsed("repeating-linear-gradient(red 0, transparent 0)"));
        assert_rgb(faded, [1.0, 0.0, 0.0]);
        assert!((faded.a - 0.5).abs() < 1e-3);
    }

    #[test]
    fn a_cycle_with_length_or_a_plain_gradient_is_left_alone() {
        for (value, repeating) in [
            ("repeating-linear-gradient(red 0, blue 10px)", true),
            ("repeating-linear-gradient(red, blue)", true),
            // The units cannot be compared without the box.
            ("repeating-linear-gradient(red 10px, blue 5%)", true),
            ("repeating-conic-gradient(red, blue)", true),
            // A plain gradient with coincident stops is a hard edge, which is right.
            ("linear-gradient(red 10px, blue 10px)", false),
        ] {
            let gradient = parsed(value);
            assert_eq!(gradient.repeating, repeating, "{value}");
            let stops: Vec<_> = gradient.items.iter().map(|item| stop(item).0).collect();
            assert_ne!(stops[0], stops[1], "{value}");
        }
    }
}
