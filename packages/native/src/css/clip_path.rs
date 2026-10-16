//! The CSS `clip-path` string: a basic shape and an optional reference box,
//! parsed into gpui's `ClipPath`.
//!
//! `url()`, `path()`, `rect()`, `xywh()` and a box with no shape are not
//! supported, so a value that uses them is ignored like any invalid one.
//! gpui rounds an `inset()` with circular corners, so the vertical radii after
//! `/` in `round` are dropped.

use gpui::{
    BackgroundBox, BackgroundPosition, ClipPath, ClipShape, InsetRadii, LengthPercentage,
    ShapeRadius,
};

use super::cache::Cache;
use super::position::{parse_box, parse_length_percentage, parse_non_negative, parse_position};
use super::syntax::{function_args, split_commas, split_whitespace};

thread_local! {
    static CACHE: Cache<Option<Option<ClipPath>>> = Cache::new();
}

/// `None` when the declaration is invalid. `Some(None)` is the valid `none`,
/// which a state style needs in order to clear the base clip path.
pub(crate) fn parse(value: &str) -> Option<Option<ClipPath>> {
    CACHE.with(|cache| cache.get_or_parse(value, || parse_uncached(value)))
}

fn parse_uncached(value: &str) -> Option<Option<ClipPath>> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") {
        return Some(None);
    }
    let mut shape = None;
    let mut reference = None;
    for token in split_whitespace(value) {
        if let Some(parsed) = parse_reference_box(token) {
            if reference.replace(parsed).is_some() {
                return None;
            }
        } else if shape.replace(parse_shape(token)?).is_some() {
            return None;
        }
    }
    Some(Some(ClipPath {
        shape: shape?,
        reference: reference.unwrap_or(BackgroundBox::BorderBox),
    }))
}

/// An element has one CSS layout box and no fill or stroke geometry, so
/// `fill-box` is the content box, `stroke-box` and `view-box` the border box,
/// and `margin-box`, which gpui has no region for, the border box too.
fn parse_reference_box(token: &str) -> Option<BackgroundBox> {
    match token.to_ascii_lowercase().as_str() {
        "fill-box" => Some(BackgroundBox::ContentBox),
        "stroke-box" | "view-box" | "margin-box" => Some(BackgroundBox::BorderBox),
        _ => parse_box(token),
    }
}

fn parse_shape(token: &str) -> Option<ClipShape> {
    if let Some(args) = function_args(token, "inset") {
        return parse_inset(args);
    }
    if let Some(args) = function_args(token, "circle") {
        return parse_circle(args);
    }
    if let Some(args) = function_args(token, "ellipse") {
        return parse_ellipse(args);
    }
    if let Some(args) = function_args(token, "polygon") {
        return parse_polygon(args);
    }
    None
}

/// One to four values in the order `margin` uses: all, vertical / horizontal,
/// top / horizontal / bottom, or each side clockwise from the top.
fn expand_sides<T: Copy>(values: &[T]) -> Option<[T; 4]> {
    Some(match *values {
        [all] => [all, all, all, all],
        [vertical, horizontal] => [vertical, horizontal, vertical, horizontal],
        [top, horizontal, bottom] => [top, horizontal, bottom, horizontal],
        [top, right, bottom, left] => [top, right, bottom, left],
        _ => return None,
    })
}

fn parse_inset(args: &str) -> Option<ClipShape> {
    let spaced = args.replace('/', " / ");
    let tokens = split_whitespace(&spaced);
    let round_at = tokens
        .iter()
        .position(|token| token.eq_ignore_ascii_case("round"));
    let insets = &tokens[..round_at.unwrap_or(tokens.len())];
    let sides = expand_sides(
        &insets
            .iter()
            .map(|token| parse_length_percentage(token))
            .collect::<Option<Vec<_>>>()?,
    )?;
    let radii = match round_at {
        None => InsetRadii::default(),
        Some(index) => parse_round(&tokens[index + 1..])?,
    };
    Some(ClipShape::Inset {
        top: sides[0],
        right: sides[1],
        bottom: sides[2],
        left: sides[3],
        radii,
    })
}

/// `<length-percentage>{1,4}` and, optionally, `/ <length-percentage>{1,4}`.
fn parse_round(tokens: &[&str]) -> Option<InsetRadii> {
    let horizontal_end = tokens.iter().position(|token| *token == "/");
    let horizontal = &tokens[..horizontal_end.unwrap_or(tokens.len())];
    if let Some(index) = horizontal_end {
        let vertical = tokens[index + 1..]
            .iter()
            .map(|token| parse_non_negative(token))
            .collect::<Option<Vec<_>>>()?;
        expand_sides(&vertical)?;
    }
    let corners = expand_sides(
        &horizontal
            .iter()
            .map(|token| parse_non_negative(token))
            .collect::<Option<Vec<_>>>()?,
    )?;
    Some(InsetRadii {
        top_left: corners[0],
        top_right: corners[1],
        bottom_right: corners[2],
        bottom_left: corners[3],
    })
}

fn parse_shape_radius(token: &str) -> Option<ShapeRadius> {
    match token.to_ascii_lowercase().as_str() {
        "closest-side" => Some(ShapeRadius::ClosestSide),
        "farthest-side" => Some(ShapeRadius::FarthestSide),
        _ => parse_non_negative(token).map(ShapeRadius::Length),
    }
}

/// Splits `<radii> at <position>` into the tokens before and after `at`. The
/// position is the centre of the reference box when `at` is absent.
fn split_at_position<'a>(tokens: &'a [&'a str]) -> Option<(&'a [&'a str], BackgroundPosition)> {
    match tokens.iter().position(|token| token.eq_ignore_ascii_case("at")) {
        None => Some((tokens, BackgroundPosition::CENTER)),
        Some(index) => Some((&tokens[..index], parse_position(&tokens[index + 1..])?)),
    }
}

fn parse_circle(args: &str) -> Option<ClipShape> {
    let tokens = split_whitespace(args);
    let (radii, center) = split_at_position(&tokens)?;
    let radius = match *radii {
        [] => ShapeRadius::ClosestSide,
        [one] => parse_shape_radius(one)?,
        _ => return None,
    };
    Some(ClipShape::Circle { radius, center })
}

fn parse_ellipse(args: &str) -> Option<ClipShape> {
    let tokens = split_whitespace(args);
    let (radii, center) = split_at_position(&tokens)?;
    let (rx, ry) = match *radii {
        [] => (ShapeRadius::ClosestSide, ShapeRadius::ClosestSide),
        [rx, ry] => (parse_shape_radius(rx)?, parse_shape_radius(ry)?),
        _ => return None,
    };
    Some(ClipShape::Ellipse { rx, ry, center })
}

fn parse_polygon(args: &str) -> Option<ClipShape> {
    let pieces = split_commas(args);
    let (even_odd, points) = match pieces.first()?.to_ascii_lowercase().as_str() {
        "nonzero" => (false, &pieces[1..]),
        "evenodd" => (true, &pieces[1..]),
        _ => (false, &pieces[..]),
    };
    if points.is_empty() {
        return None;
    }
    let points = points
        .iter()
        .map(|point| match *split_whitespace(point) {
            [x, y] => Some((parse_length_percentage(x)?, parse_length_percentage(y)?)),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    Some(ClipShape::Polygon { even_odd, points })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{px, BackgroundPositionAxis};

    #[test]
    fn a_cached_parse_equals_the_uncached_one() {
        for value in [
            "polygon(50% 0%, 100% 38%, 82% 100%, 18% 100%, 0% 38%)",
            "circle(40% at 50% 50%) padding-box",
            "inset(10px round 4px)",
            "none",
            "polygon(1px)",
            "url(#clip)",
        ] {
            assert_eq!(parse(value), parse_uncached(value), "{value}");
            assert_eq!(parse(value), parse_uncached(value), "{value} on a hit");
        }
    }

    fn shape(value: &str) -> ClipShape {
        parse(value).unwrap().unwrap().shape
    }

    fn pct(fraction: f32) -> LengthPercentage {
        LengthPercentage::Percent(fraction)
    }

    fn len(value: f32) -> LengthPercentage {
        LengthPercentage::Length(px(value))
    }

    #[test]
    fn none_is_a_clear_not_an_ignored_value() {
        assert_eq!(parse("none"), Some(None));
        assert_eq!(parse(" NONE "), Some(None));
    }

    #[test]
    fn inset_sides_follow_the_margin_shorthand() {
        let sides = |value: &str| match shape(value) {
            ClipShape::Inset { top, right, bottom, left, .. } => [top, right, bottom, left],
            other => panic!("{other:?}"),
        };
        assert_eq!(sides("inset(10px)"), [len(10.0); 4]);
        assert_eq!(sides("inset(1px 2px)"), [len(1.0), len(2.0), len(1.0), len(2.0)]);
        assert_eq!(sides("inset(1px 2px 3px)"), [len(1.0), len(2.0), len(3.0), len(2.0)]);
        assert_eq!(
            sides("inset(0 10% 5px 20%)"),
            [len(0.0), pct(0.1), len(5.0), pct(0.2)]
        );
        assert_eq!(parse("inset()"), None);
        assert_eq!(parse("inset(1px 2px 3px 4px 5px)"), None);
        assert_eq!(parse("inset(1em)"), None);
    }

    #[test]
    fn inset_round_expands_corners_clockwise_from_the_top_left() {
        let radii = |value: &str| match shape(value) {
            ClipShape::Inset { radii, .. } => radii,
            other => panic!("{other:?}"),
        };
        let all = radii("inset(0 round 8px)");
        assert_eq!(all.top_left, len(8.0));
        assert_eq!(all.bottom_left, len(8.0));
        let four = radii("inset(10px round 1px 2px 3px 4px)");
        assert_eq!(
            [four.top_left, four.top_right, four.bottom_right, four.bottom_left],
            [len(1.0), len(2.0), len(3.0), len(4.0)]
        );
        let two = radii("inset(0 round 4px 50%)");
        assert_eq!(two.top_left, len(4.0));
        assert_eq!(two.top_right, pct(0.5));
        assert_eq!(two.bottom_right, len(4.0));
        assert_eq!(two.bottom_left, pct(0.5));
        // The vertical radii after `/` are valid but dropped.
        assert_eq!(radii("inset(0 round 8px/2px)"), radii("inset(0 round 8px)"));
        assert_eq!(parse("inset(0 round)"), None);
        assert_eq!(parse("inset(0 round -1px)"), None);
        assert_eq!(parse("inset(round 4px)"), None);
    }

    #[test]
    fn circle_radius_and_position() {
        assert_eq!(
            shape("circle()"),
            ClipShape::Circle { radius: ShapeRadius::ClosestSide, center: BackgroundPosition::CENTER }
        );
        assert_eq!(
            shape("circle(40px)"),
            ClipShape::Circle { radius: ShapeRadius::Length(len(40.0)), center: BackgroundPosition::CENTER }
        );
        assert_eq!(
            shape("circle(farthest-side at 25% 75%)"),
            ClipShape::Circle {
                radius: ShapeRadius::FarthestSide,
                center: BackgroundPosition::new(pct(0.25), pct(0.75)),
            }
        );
        match shape("circle(at right 10px top)") {
            ClipShape::Circle { radius, center } => {
                assert_eq!(radius, ShapeRadius::ClosestSide);
                assert_eq!(center.x, BackgroundPositionAxis::from_end(len(10.0)));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(parse("circle(-5px)"), None);
        assert_eq!(parse("circle(10px 20px)"), None);
        assert_eq!(parse("circle(at)"), None);
        assert_eq!(parse("circle(at left right)"), None);
    }

    #[test]
    fn ellipse_takes_two_radii_or_none() {
        assert_eq!(
            shape("ellipse(30% closest-side at left)"),
            ClipShape::Ellipse {
                rx: ShapeRadius::Length(pct(0.3)),
                ry: ShapeRadius::ClosestSide,
                center: BackgroundPosition::new(pct(0.0), pct(0.5)),
            }
        );
        assert_eq!(
            shape("ellipse()"),
            ClipShape::Ellipse {
                rx: ShapeRadius::ClosestSide,
                ry: ShapeRadius::ClosestSide,
                center: BackgroundPosition::CENTER,
            }
        );
        assert_eq!(parse("ellipse(10px)"), None);
    }

    #[test]
    fn polygon_points_and_fill_rule() {
        assert_eq!(
            shape("polygon(50% 0, 100% 100%, 0 100%)"),
            ClipShape::Polygon {
                even_odd: false,
                points: vec![(pct(0.5), len(0.0)), (pct(1.0), pct(1.0)), (len(0.0), pct(1.0))],
            }
        );
        match shape("polygon(evenodd, 0 0, 10px 0, 5px 8px)") {
            ClipShape::Polygon { even_odd, points } => {
                assert!(even_odd);
                assert_eq!(points.len(), 3);
            }
            other => panic!("{other:?}"),
        }
        match shape("polygon(NONZERO, 0 0)") {
            ClipShape::Polygon { even_odd, .. } => assert!(!even_odd),
            other => panic!("{other:?}"),
        }
        assert_eq!(parse("polygon()"), None);
        assert_eq!(parse("polygon(evenodd)"), None);
        assert_eq!(parse("polygon(0 0,, 1px 1px)"), None);
        assert_eq!(parse("polygon(0 0 0, 1px 1px)"), None);
        assert_eq!(parse("polygon(0, 1px 1px)"), None);
    }

    #[test]
    fn the_reference_box_goes_before_or_after_the_shape() {
        let reference = |value: &str| parse(value).unwrap().unwrap().reference;
        assert_eq!(reference("circle()"), BackgroundBox::BorderBox);
        assert_eq!(reference("circle() padding-box"), BackgroundBox::PaddingBox);
        assert_eq!(reference("content-box inset(0)"), BackgroundBox::ContentBox);
        assert_eq!(reference("circle() margin-box"), BackgroundBox::BorderBox);
        assert_eq!(reference("circle() fill-box"), BackgroundBox::ContentBox);
        assert_eq!(reference("circle() stroke-box"), BackgroundBox::BorderBox);
        assert_eq!(reference("circle() view-box"), BackgroundBox::BorderBox);
        assert_eq!(parse("padding-box"), None);
        assert_eq!(parse("circle() padding-box content-box"), None);
        assert_eq!(parse("circle() inset(0)"), None);
    }

    #[test]
    fn unsupported_and_malformed_values_are_invalid() {
        assert_eq!(parse("url(#clip)"), None);
        assert_eq!(parse("path('M0 0 L10 10')"), None);
        assert_eq!(parse("rect(0 10px 10px 0)"), None);
        assert_eq!(parse("circle(10px"), None);
        assert_eq!(parse("circle (10px)"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse("none circle()"), None);
    }
}
