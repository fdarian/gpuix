//! The CSS `clip-path` string: a basic shape and an optional reference box,
//! parsed into gpui's `ClipPath`.
//!
//! `url()` and a box with no shape are not supported, so a value that uses
//! them is ignored like any invalid one.

use gpui::{
    px, BackgroundBox, BackgroundPosition, ClipPath, ClipShape, InsetRadii, LengthPercentage,
    ShapeRadius,
};
use svgtypes::{SimplePathSegment, SimplifyingPathParser};

use super::cache::{Cache, Inputs};
use super::length::LengthContext;
use super::border_radius::parse_radii;
use super::position::{parse_box, parse_length_percentage, parse_non_negative, parse_position};
use super::syntax::{expand_sides, function_args, split_commas, split_slash_tokens, split_whitespace};

thread_local! {
    static CACHE: Cache<Option<Option<ClipPath>>> = Cache::new();
}

/// The most vertices a `path()` may flatten to. gpui draws a polygon by testing every edge for every
/// pixel, so this bounds the cost of a clipped element, not a buffer size.
const MAX_PATH_VERTICES: usize = 4096;

/// How far, in px, the flattened polygon may stray from the curves. Each is tried in turn, the
/// finest that fits in `MAX_PATH_VERTICES`.
const FLATTENING_TOLERANCES: [f64; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

/// `None` when the declaration is invalid. `Some(None)` is the valid `none`,
/// which a state style needs in order to clear the base clip path.
pub(crate) fn parse(value: &str, lengths: &LengthContext) -> Option<Option<ClipPath>> {
    CACHE.with(|cache| cache.get_or_parse(value, Inputs { colors: None, lengths }, || parse_uncached(value, lengths)))
}

fn parse_uncached(value: &str, lengths: &LengthContext) -> Option<Option<ClipPath>> {
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
        } else if shape.replace(parse_shape(token, lengths)?).is_some() {
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

fn parse_shape(token: &str, lengths: &LengthContext) -> Option<ClipShape> {
    if let Some(args) = function_args(token, "inset") {
        return parse_inset(args, lengths);
    }
    if let Some(args) = function_args(token, "circle") {
        return parse_circle(args, lengths);
    }
    if let Some(args) = function_args(token, "ellipse") {
        return parse_ellipse(args, lengths);
    }
    if let Some(args) = function_args(token, "polygon") {
        return parse_polygon(args, lengths);
    }
    if let Some(args) = function_args(token, "path") {
        return parse_path(args);
    }
    if let Some(args) = function_args(token, "rect") {
        return parse_rect(args, lengths);
    }
    if let Some(args) = function_args(token, "xywh") {
        return parse_xywh(args, lengths);
    }
    None
}

/// The arguments of a box shape split at `round`: the values before it and the parsed radii.
fn split_round(args: &str, lengths: &LengthContext) -> Option<(Vec<String>, InsetRadii)> {
    let tokens = split_slash_tokens(args);
    let round_at = tokens
        .iter()
        .position(|token| token.eq_ignore_ascii_case("round"));
    let radii = match round_at {
        None => InsetRadii::default(),
        Some(index) => parse_round(&tokens[index + 1..], lengths)?,
    };
    let values = tokens[..round_at.unwrap_or(tokens.len())]
        .iter()
        .map(|token| token.to_string())
        .collect();
    Some((values, radii))
}

fn parse_inset(args: &str, lengths: &LengthContext) -> Option<ClipShape> {
    let (insets, radii) = split_round(args, lengths)?;
    let sides = expand_sides(
        &insets
            .iter()
            .map(|token| parse_length_percentage(token, lengths))
            .collect::<Option<Vec<_>>>()?,
    )?;
    Some(ClipShape::Inset {
        top: sides[0],
        right: sides[1],
        bottom: sides[2],
        left: sides[3],
        radii,
    })
}

/// `<length-percentage>{1,4}` and, optionally, `/ <length-percentage>{1,4}`. Without the `/`
/// the vertical radii are the horizontal ones, so a percentage is of the height on that axis.
/// An explicit zero on either axis squares the corner, which a zero horizontal radius says.
fn parse_round(tokens: &[&str], lengths: &LengthContext) -> Option<InsetRadii> {
    let radii = parse_radii(tokens, |token| parse_non_negative(token, lengths).ok_or_else(String::new)).ok()?;
    let horizontal = radii.horizontal;
    let vertical = radii.vertical.unwrap_or(horizontal);
    let corner = |index: usize| {
        let (x, y) = (horizontal[index], vertical[index]);
        if is_zero(x) || is_zero(y) {
            (zero(), zero())
        } else {
            (x, y)
        }
    };
    let corners = [0, 1, 2, 3].map(corner);
    Some(InsetRadii {
        top_left: corners[0].0,
        top_right: corners[1].0,
        bottom_right: corners[2].0,
        bottom_left: corners[3].0,
        top_left_y: corners[0].1,
        top_right_y: corners[1].1,
        bottom_right_y: corners[2].1,
        bottom_left_y: corners[3].1,
    })
}

fn zero() -> LengthPercentage {
    LengthPercentage::Length(px(0.0))
}

fn is_zero(length: LengthPercentage) -> bool {
    match length {
        LengthPercentage::Length(length) => f32::from(length) == 0.0,
        LengthPercentage::Percent(fraction) => fraction == 0.0,
        LengthPercentage::Calc { length, percent } => f32::from(length) == 0.0 && percent == 0.0,
    }
}

fn parse_shape_radius(token: &str, lengths: &LengthContext) -> Option<ShapeRadius> {
    match token.to_ascii_lowercase().as_str() {
        "closest-side" => Some(ShapeRadius::ClosestSide),
        "farthest-side" => Some(ShapeRadius::FarthestSide),
        _ => parse_non_negative(token, lengths).map(ShapeRadius::Length),
    }
}

/// Splits `<radii> at <position>` into the tokens before and after `at`. The
/// position is the centre of the reference box when `at` is absent.
fn split_at_position<'a>(
    tokens: &'a [&'a str],
    lengths: &LengthContext,
) -> Option<(&'a [&'a str], BackgroundPosition)> {
    match tokens.iter().position(|token| token.eq_ignore_ascii_case("at")) {
        None => Some((tokens, BackgroundPosition::CENTER)),
        Some(index) => Some((&tokens[..index], parse_position(&tokens[index + 1..], lengths)?)),
    }
}

fn parse_circle(args: &str, lengths: &LengthContext) -> Option<ClipShape> {
    let tokens = split_whitespace(args);
    let (radii, center) = split_at_position(&tokens, lengths)?;
    let radius = match *radii {
        [] => ShapeRadius::ClosestSide,
        [one] => parse_shape_radius(one, lengths)?,
        _ => return None,
    };
    Some(ClipShape::Circle { radius, center })
}

fn parse_ellipse(args: &str, lengths: &LengthContext) -> Option<ClipShape> {
    let tokens = split_whitespace(args);
    let (radii, center) = split_at_position(&tokens, lengths)?;
    let (rx, ry) = match *radii {
        [] => (ShapeRadius::ClosestSide, ShapeRadius::ClosestSide),
        [rx, ry] => (parse_shape_radius(rx, lengths)?, parse_shape_radius(ry, lengths)?),
        _ => return None,
    };
    Some(ClipShape::Ellipse { rx, ry, center })
}

fn parse_polygon(args: &str, lengths: &LengthContext) -> Option<ClipShape> {
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
            [x, y] => Some((parse_length_percentage(x, lengths)?, parse_length_percentage(y, lengths)?)),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    Some(ClipShape::Polygon { even_odd, contours: vec![points] })
}

/// `top`/`left` default to the start of the box and `right`/`bottom` to its end.
fn parse_edge(token: &str, auto: LengthPercentage, lengths: &LengthContext) -> Option<LengthPercentage> {
    if token.eq_ignore_ascii_case("auto") {
        Some(auto)
    } else {
        parse_length_percentage(token, lengths)
    }
}

/// `rect(<top> <right> <bottom> <left> [round <radii>])`. Unlike `inset()`, each edge is a distance
/// from the top or left edge of the reference box.
fn parse_rect(args: &str, lengths: &LengthContext) -> Option<ClipShape> {
    let (edges, radii) = split_round(args, lengths)?;
    let [top, right, bottom, left] = &edges[..] else {
        return None;
    };
    let start = LengthPercentage::Length(px(0.0));
    let end = LengthPercentage::Percent(1.0);
    Some(ClipShape::Rect {
        top: parse_edge(top, start, lengths)?,
        right: parse_edge(right, end, lengths)?,
        bottom: parse_edge(bottom, end, lengths)?,
        left: parse_edge(left, start, lengths)?,
        radii,
    })
}

/// The far edge of a box from its near edge and its size: their sum, as `calc()` would add them.
fn far_edge(near: LengthPercentage, size: LengthPercentage) -> LengthPercentage {
    let parts = |value: LengthPercentage| match value {
        LengthPercentage::Length(length) => (length, 0.0),
        LengthPercentage::Percent(percent) => (px(0.0), percent),
        LengthPercentage::Calc { length, percent } => (length, percent),
    };
    let ((near_length, near_percent), (size_length, size_percent)) = (parts(near), parts(size));
    let (length, percent) = (near_length + size_length, near_percent + size_percent);
    if percent == 0.0 {
        LengthPercentage::Length(length)
    } else if f32::from(length) == 0.0 {
        LengthPercentage::Percent(percent)
    } else {
        LengthPercentage::Calc { length, percent }
    }
}

/// `xywh(<x> <y> <width> <height> [round <radii>])`. The width and height may not be negative.
fn parse_xywh(args: &str, lengths: &LengthContext) -> Option<ClipShape> {
    let (values, radii) = split_round(args, lengths)?;
    let [x, y, width, height] = &values[..] else {
        return None;
    };
    let (x, y) = (parse_length_percentage(x, lengths)?, parse_length_percentage(y, lengths)?);
    Some(ClipShape::Rect {
        top: y,
        right: far_edge(x, parse_non_negative(width, lengths)?),
        bottom: far_edge(y, parse_non_negative(height, lengths)?),
        left: x,
        radii,
    })
}

/// `path([<fill-rule>,] "<svg path data>")`, as a polygon of one contour per subpath, in px from
/// the top left of the box.
fn parse_path(args: &str) -> Option<ClipShape> {
    let args = args.trim();
    let (even_odd, literal) = if args.starts_with(['"', '\'']) {
        (false, args)
    } else {
        let (rule, literal) = args.split_once(',')?;
        match rule.trim().to_ascii_lowercase().as_str() {
            "nonzero" => (false, literal),
            "evenodd" => (true, literal),
            _ => return None,
        }
    };
    let segments = SimplifyingPathParser::from(parse_string_literal(literal)?)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if segments.is_empty() {
        return None;
    }
    let contours = FLATTENING_TOLERANCES
        .iter()
        .find_map(|tolerance| flatten(&segments, *tolerance))?;
    Some(ClipShape::Polygon {
        even_odd,
        contours: contours
            .into_iter()
            .map(|contour| {
                contour
                    .into_iter()
                    .map(|vertex| {
                        (
                            LengthPercentage::Length(px(vertex.0 as f32)),
                            LengthPercentage::Length(px(vertex.1 as f32)),
                        )
                    })
                    .collect()
            })
            .collect(),
    })
}

/// The text of a `"..."` or `'...'` string. Escapes are not supported: path data has no use for
/// them.
fn parse_string_literal(value: &str) -> Option<&str> {
    let value = value.trim();
    let quote = value.chars().next().filter(|c| matches!(c, '"' | '\''))?;
    let inner = value[1..].strip_suffix(quote)?;
    (!inner.contains([quote, '\\'])).then_some(inner)
}

type Vertex = (f64, f64);

/// The subpaths as closed contours, or `None` when they need more than `MAX_PATH_VERTICES` at this
/// tolerance (or have a coordinate that is not a finite number).
fn flatten(segments: &[SimplePathSegment], tolerance: f64) -> Option<Vec<Vec<Vertex>>> {
    let mut subpaths: Vec<Vec<Vertex>> = Vec::new();
    let mut count = 0usize;
    for segment in segments {
        match *segment {
            SimplePathSegment::MoveTo { x, y } => subpaths.push(vec![(x, y)]),
            SimplePathSegment::LineTo { x, y } => {
                subpaths.last_mut()?.push((x, y));
                count += 1;
            }
            SimplePathSegment::Quadratic { x1, y1, x, y } => {
                let subpath = subpaths.last_mut()?;
                let start = *subpath.last()?;
                let (control, end) = ((x1, y1), (x, y));
                // The error of n chords is at most |B''| / (8 n^2), with B'' = 2 (p0 - 2 p1 + p2).
                let second = (start.0 - 2.0 * x1 + x).hypot(start.1 - 2.0 * y1 + y);
                let steps = steps_for(0.25 * second, tolerance, MAX_PATH_VERTICES - count)?;
                for step in 1..=steps {
                    let t = step as f64 / steps as f64;
                    let u = 1.0 - t;
                    subpath.push((
                        u * u * start.0 + 2.0 * u * t * control.0 + t * t * end.0,
                        u * u * start.1 + 2.0 * u * t * control.1 + t * t * end.1,
                    ));
                }
                count += steps;
            }
            SimplePathSegment::CurveTo { x1, y1, x2, y2, x, y } => {
                let subpath = subpaths.last_mut()?;
                let start = *subpath.last()?;
                // B'' = 6 (max (p0 - 2 p1 + p2, p1 - 2 p2 + p3)) at its largest.
                let second = (start.0 - 2.0 * x1 + x2).hypot(start.1 - 2.0 * y1 + y2)
                    .max((x1 - 2.0 * x2 + x).hypot(y1 - 2.0 * y2 + y));
                let steps = steps_for(0.75 * second, tolerance, MAX_PATH_VERTICES - count)?;
                for step in 1..=steps {
                    let t = step as f64 / steps as f64;
                    let u = 1.0 - t;
                    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                    subpath.push((
                        a * start.0 + b * x1 + c * x2 + d * x,
                        a * start.1 + b * y1 + c * y2 + d * y,
                    ));
                }
                count += steps;
            }
            // A fill closes every subpath, and a `Z` has no other effect on one.
            SimplePathSegment::ClosePath => {}
        }
        if count > MAX_PATH_VERTICES {
            return None;
        }
    }
    let contours = without_area(subpaths);
    let vertices: Vec<&Vertex> = contours.iter().flatten().collect();
    (vertices.len() <= MAX_PATH_VERTICES
        && vertices.iter().all(|vertex| (vertex.0 as f32).is_finite() && (vertex.1 as f32).is_finite()))
    .then_some(contours)
}

fn steps_for(error_scale: f64, tolerance: f64, budget: usize) -> Option<usize> {
    let steps = (error_scale / tolerance).sqrt().ceil().max(1.0);
    (steps <= budget as f64).then_some(steps as usize)
}

/// The subpaths that have an area. A polygon needs a vertex, so a path with none is the first point
/// alone, which fills nothing.
fn without_area(subpaths: Vec<Vec<Vertex>>) -> Vec<Vec<Vertex>> {
    let first_move = subpaths[0][0];
    let contours: Vec<Vec<Vertex>> = subpaths
        .into_iter()
        .map(|mut subpath| {
            subpath.dedup();
            // A contour closes on its own, so a line back to the start repeats the first vertex.
            if subpath.len() > 1 && subpath.last() == subpath.first() {
                subpath.pop();
            }
            subpath
        })
        // Under three distinct points a subpath has no area.
        .filter(|subpath| subpath.len() >= 3)
        .collect();
    if contours.is_empty() {
        vec![vec![first_move]]
    } else {
        contours
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{px, BackgroundPositionAxis, LengthPercentage};

    fn parse(value: &str) -> Option<Option<ClipPath>> {
        super::parse(value, &LengthContext::for_tests())
    }

    fn parse_uncached(value: &str) -> Option<Option<ClipPath>> {
        super::parse_uncached(value, &LengthContext::for_tests())
    }

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

    #[test]
    fn a_cached_clip_path_is_not_shared_across_length_contexts() {
        let base = LengthContext::for_tests();
        let large_font = base.descend(Some(&crate::css::length::LinearLength::px(40.0)), false);
        let large_root = base.descend(Some(&crate::css::length::LinearLength::px(30.0)), true);
        let wide = base.with_viewport(1600.0, 900.0);
        let contexts = [base, large_font, large_root, wide];
        for value in [
            "inset(1em round 2em)",
            "circle(calc(10px + 1rem) at 50% 10vh)",
            "polygon(0 0, 10vw 0, 1em 50%)",
        ] {
            for _ in 0..2 {
                for lengths in &contexts {
                    assert_eq!(super::parse(value, lengths), super::parse_uncached(value, lengths), "{value}");
                }
            }
            let distinct = contexts
                .iter()
                .map(|lengths| format!("{:?}", super::parse(value, lengths).unwrap()))
                .collect::<std::collections::HashSet<_>>();
            assert!(distinct.len() >= 2, "{value} resolved alike in every length context");
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
        assert_eq!(parse("inset(1ch)"), None);
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
        // Without a `/` a percentage is of the width across and of the height down.
        let circular = radii("inset(0 round 10%)");
        assert_eq!([circular.top_left, circular.top_left_y], [pct(0.1), pct(0.1)]);
        let absolute = radii("inset(0 round 8px)");
        assert_eq!([absolute.top_left, absolute.top_left_y], [len(8.0), len(8.0)]);
        assert_eq!(parse("inset(0 round)"), None);
        assert_eq!(parse("inset(0 round -1px)"), None);
        assert_eq!(parse("inset(round 4px)"), None);
    }

    #[test]
    fn inset_round_takes_a_vertical_radius_per_corner_after_a_slash() {
        let radii = |value: &str| match shape(value) {
            ClipShape::Inset { radii, .. } => radii,
            other => panic!("{other:?}"),
        };
        let four = |radii: InsetRadii| {
            (
                [radii.top_left, radii.top_right, radii.bottom_right, radii.bottom_left],
                [radii.top_left_y, radii.top_right_y, radii.bottom_right_y, radii.bottom_left_y],
            )
        };
        assert_eq!(
            four(radii("inset(0 round 10px 20px / 5px)")),
            ([len(10.0), len(20.0), len(10.0), len(20.0)], [len(5.0); 4])
        );
        assert_eq!(
            four(radii("inset(0 round 50% / 10px 20px 30px 40px)")),
            ([pct(0.5); 4], [len(10.0), len(20.0), len(30.0), len(40.0)])
        );
        assert_eq!(
            four(radii("inset(0 round 1px 2px 3px 4px/5% 6% 7% 8%)")),
            ([len(1.0), len(2.0), len(3.0), len(4.0)], [pct(0.05), pct(0.06), pct(0.07), pct(0.08)])
        );
        // A zero on either axis squares the corner.
        assert_eq!(
            four(radii("inset(0 round 10px 20px / 0 5px)")),
            ([len(0.0), len(20.0), len(0.0), len(20.0)], [len(0.0), len(5.0), len(0.0), len(5.0)])
        );
        assert_eq!(four(radii("inset(0 round 0 / 10px)")), ([len(0.0); 4], [len(0.0); 4]));
        assert_eq!(four(radii("inset(0 round 10px / 0)")), ([len(0.0); 4], [len(0.0); 4]));
        for invalid in [
            "inset(0 round 8px /)",
            "inset(0 round / 8px)",
            "inset(0 round 8px / 1px 2px 3px 4px 5px)",
            "inset(0 round 8px / -2px)",
            "inset(0 round 8px / 2px / 3px)",
        ] {
            assert_eq!(parse(invalid), None, "{invalid}");
        }
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
                contours: vec![vec![(pct(0.5), len(0.0)), (pct(1.0), pct(1.0)), (len(0.0), pct(1.0))]],
            }
        );
        match shape("polygon(evenodd, 0 0, 10px 0, 5px 8px)") {
            ClipShape::Polygon { even_odd, contours } => {
                assert!(even_odd);
                assert_eq!(contours[0].len(), 3);
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
        assert_eq!(parse("circle(10px"), None);
        assert_eq!(parse("circle (10px)"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse("none circle()"), None);
    }

    #[test]
    fn lengths_resolve_against_the_context() {
        assert_eq!(
            shape("inset(1em 2rem calc(10vw + 5%) calc(100% - 1em))"),
            ClipShape::Inset {
                top: len(20.0),
                right: len(20.0),
                bottom: LengthPercentage::Calc { length: px(80.0), percent: 0.05 },
                left: LengthPercentage::Calc { length: px(-20.0), percent: 1.0 },
                radii: InsetRadii::default(),
            }
        );
        assert_eq!(
            shape("circle(2em at 10vw calc(1rem + 10%))"),
            ClipShape::Circle {
                radius: ShapeRadius::Length(len(40.0)),
                center: BackgroundPosition::new(len(80.0), LengthPercentage::Calc { length: px(10.0), percent: 0.1 }),
            }
        );
        assert_eq!(
            shape("polygon(0 0, 1em 50%, calc(1em + 10%) 10vh)"),
            ClipShape::Polygon {
                even_odd: false,
                contours: vec![vec![
                    (len(0.0), len(0.0)),
                    (len(20.0), pct(0.5)),
                    (LengthPercentage::Calc { length: px(20.0), percent: 0.1 }, len(60.0)),
                ]],
            }
        );
    }

    #[test]
    fn a_radius_that_is_negative_in_every_context_is_invalid() {
        assert_eq!(parse("circle(-1em)"), None);
        assert_eq!(parse("inset(0 round -1rem)"), None);
        assert_eq!(parse("inset(0 round calc(-1em - 1px))"), None);
    }

    fn edges(value: &str) -> [LengthPercentage; 4] {
        match shape(value) {
            ClipShape::Rect { top, right, bottom, left, .. } => [top, right, bottom, left],
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rect_edges_are_distances_from_the_top_left() {
        assert_eq!(
            edges("rect(10px 80px 50% 20%)"),
            [len(10.0), len(80.0), pct(0.5), pct(0.2)]
        );
        assert_eq!(edges("rect(-5px 10px 10px -5px)"), [len(-5.0), len(10.0), len(10.0), len(-5.0)]);
        assert_eq!(edges("RECT(0 1px 2px 3px)"), [len(0.0), len(1.0), len(2.0), len(3.0)]);
    }

    #[test]
    fn rect_auto_is_the_start_for_top_and_left_and_the_end_for_right_and_bottom() {
        assert_eq!(
            edges("rect(auto auto auto auto)"),
            [len(0.0), pct(1.0), pct(1.0), len(0.0)]
        );
        assert_eq!(edges("rect(5px AUTO 7px auto)"), [len(5.0), pct(1.0), len(7.0), len(0.0)]);
    }

    #[test]
    fn rect_and_xywh_round_their_corners() {
        let radii = |value: &str| match shape(value) {
            ClipShape::Rect { radii, .. } => radii,
            other => panic!("{other:?}"),
        };
        let four = radii("rect(0 10px 10px 0 round 1px 2px 3px 4px)");
        assert_eq!(
            [four.top_left, four.top_right, four.bottom_right, four.bottom_left],
            [len(1.0), len(2.0), len(3.0), len(4.0)]
        );
        assert_eq!(radii("xywh(0 0 5px 5px round 8px)").bottom_right, len(8.0));
        let elliptical = radii("xywh(0 0 5px 5px round 8px/2px)");
        assert_eq!([elliptical.bottom_right, elliptical.bottom_right_y], [len(8.0), len(2.0)]);
        assert_eq!(radii("rect(0 1px 1px 0)"), InsetRadii::default());
    }

    #[test]
    fn xywh_is_a_position_and_a_size() {
        assert_eq!(
            edges("xywh(10px 20px 30px 40px)"),
            [len(20.0), len(40.0), len(60.0), len(10.0)]
        );
        assert_eq!(edges("xywh(25% 25% 50% 50%)"), [pct(0.25), pct(0.75), pct(0.75), pct(0.25)]);
        // A zero size or position is in either unit.
        assert_eq!(edges("xywh(0 0 50% 50%)"), [len(0.0), pct(0.5), pct(0.5), len(0.0)]);
        assert_eq!(edges("xywh(10px 5px 0% 0)"), [len(5.0), len(10.0), len(5.0), len(10.0)]);
    }

    #[test]
    fn rect_and_xywh_reject_what_css_rejects() {
        for value in [
            "rect()",
            "rect(0 1px 2px)",
            "rect(0 1px 2px 3px 4px)",
            "rect(0, 1px, 2px, 3px)",
            "rect(0 1px 2px 3em)",
            "rect(0 1px 2px 3px round)",
            "rect(0 1px 2px 3px round -1px)",
            "rect(round 4px)",
            "xywh()",
            "xywh(0 0 5px)",
            "xywh(auto 0 5px 5px)",
            "xywh(0 0 auto 5px)",
            "xywh(0 0 -1px 5px)",
            "xywh(0 0 5px -1%)",
            "xywh(0 0 5px 5px 5px)",
            // `10px + 50%` and `5% + 20px` have no single value.
            "xywh(10px 0 50% 5px)",
            "xywh(0 5% 5px 20px)",
        ] {
            assert_eq!(parse(value), None, "{value}");
        }
    }

    fn contours(value: &str) -> Vec<Vec<(f32, f32)>> {
        match shape(value) {
            ClipShape::Polygon { contours, .. } => contours
                .into_iter()
                .map(|points| {
                    points
                        .into_iter()
                        .map(|(x, y)| match (x, y) {
                            (LengthPercentage::Length(x), LengthPercentage::Length(y)) => {
                                (f32::from(x), f32::from(y))
                            }
                            other => panic!("path() has px coordinates, not {other:?}"),
                        })
                        .collect()
                })
                .collect(),
            other => panic!("{other:?}"),
        }
    }

    /// The vertices of a value with exactly one contour.
    fn vertices(value: &str) -> Vec<(f32, f32)> {
        let mut contours = contours(value);
        assert_eq!(contours.len(), 1, "{value}");
        contours.remove(0)
    }

    #[test]
    fn path_lines_in_either_quote_and_with_relative_and_implicit_commands() {
        let square = vec![(10.0, 10.0), (90.0, 10.0), (90.0, 90.0), (10.0, 90.0)];
        assert_eq!(vertices("path('M10 10 L90 10 L90 90 L10 90 Z')"), square);
        assert_eq!(vertices("path(\"M10,10 L90,10 90,90 10,90\")"), square);
        assert_eq!(vertices("path('m10 10 l80 0 0 80 h-80 z')"), square);
        assert_eq!(vertices("path('M10 10 H90 V90 H10')"), square);
        assert_eq!(vertices("PATH( 'M10 10 H90 V90 H10' )"), square);
    }

    #[test]
    fn path_takes_a_fill_rule_before_the_data() {
        let rule = |value: &str| match shape(value) {
            ClipShape::Polygon { even_odd, .. } => even_odd,
            other => panic!("{other:?}"),
        };
        assert!(!rule("path('M0 0 L9 0 L9 9')"));
        assert!(!rule("path(nonzero, 'M0 0 L9 0 L9 9')"));
        assert!(rule("path(evenodd, 'M0 0 L9 0 L9 9')"));
        assert!(rule("path( EVENODD ,\"M0 0 L9 0 L9 9\")"));
        assert_eq!(parse("path(evenodd 'M0 0 L9 0 L9 9')"), None);
        assert_eq!(parse("path(odd, 'M0 0 L9 0 L9 9')"), None);
        assert_eq!(parse("path('M0 0 L9 0 L9 9', evenodd)"), None);
        assert_eq!(parse("path(evenodd)"), None);
        assert_eq!(parse("path(evenodd,)"), None);
    }

    fn cubic(p: [(f64, f64); 4], t: f64) -> (f64, f64) {
        let u = 1.0 - t;
        let weight = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
        let at = |axis: fn(&(f64, f64)) -> f64| (0..4).map(|i| weight[i] * axis(&p[i])).sum();
        (at(|p| p.0), at(|p| p.1))
    }

    #[test]
    fn a_cubic_is_flattened_within_a_quarter_pixel() {
        let curve = [(0.0, 0.0), (0.0, 100.0), (100.0, 100.0), (100.0, 0.0)];
        let flat = vertices("path('M0 0 C0 100 100 100 100 0')");
        assert_eq!(flat[0], (0.0, 0.0));
        assert_eq!(*flat.last().unwrap(), (100.0, 0.0));
        let chords = flat.len() - 1;
        assert!(chords > 4, "{chords}");
        for index in 0..chords {
            let on_curve = cubic(curve, (index as f64 + 0.5) / chords as f64);
            let middle = (
                (flat[index].0 + flat[index + 1].0) as f64 / 2.0,
                (flat[index].1 + flat[index + 1].1) as f64 / 2.0,
            );
            let error = (on_curve.0 - middle.0).hypot(on_curve.1 - middle.1);
            assert!(error <= 0.25, "chord {index} is {error} px off");
        }
    }

    #[test]
    fn a_quadratic_and_the_smooth_forms_end_where_they_say() {
        let quadratic = vertices("path('M0 0 Q50 100 100 0')");
        assert_eq!(*quadratic.last().unwrap(), (100.0, 0.0));
        // The apex of this parabola is at t = 1/2: y = 50.
        let apex = quadratic.iter().map(|vertex| vertex.1).fold(0.0, f32::max);
        assert!((apex - 50.0).abs() < 0.25, "{apex}");
        let smooth_quadratic = vertices("path('M0 0 Q50 100 100 0 T200 0')");
        assert_eq!(*smooth_quadratic.last().unwrap(), (200.0, 0.0));
        // `T` mirrors the control point, so the second arch dips below the line.
        assert!(smooth_quadratic.iter().any(|vertex| vertex.1 < -49.0));
        let smooth_cubic = vertices("path('M0 0 C0 50 50 50 50 0 S100 -50 100 0')");
        assert_eq!(*smooth_cubic.last().unwrap(), (100.0, 0.0));
        assert!(smooth_cubic.iter().any(|vertex| vertex.1 < -10.0));
        let relative = vertices("path('M10 10 c0 50 50 50 50 0 s50 -50 50 0')");
        assert_eq!(*relative.last().unwrap(), (110.0, 10.0));
    }

    #[test]
    fn an_arc_stays_on_its_circle() {
        let half_circle = vertices("path('M0 50 A50 50 0 0 1 100 50')");
        assert_eq!(half_circle[0], (0.0, 50.0));
        assert_eq!(*half_circle.last().unwrap(), (100.0, 50.0));
        assert!(half_circle.len() > 8);
        for (x, y) in &half_circle {
            let radius = (x - 50.0).hypot(y - 50.0);
            assert!((radius - 50.0).abs() < 0.3, "({x}, {y}) is {radius} from the centre");
            assert!(*y <= 50.01, "sweep 1 from the left to the right goes over the top, y is down");
        }
        let other_side = vertices("path('M0 50 A50 50 0 0 0 100 50')");
        assert!(other_side.iter().all(|vertex| vertex.1 >= 49.99));
        // A radius that is too small is scaled up, as in SVG.
        let scaled = vertices("path('M0 50 a1 1 0 0 1 100 0')");
        assert!(scaled.iter().all(|vertex| vertex.1 <= 50.01));
        assert!(scaled.iter().any(|vertex| vertex.1 < 1.0));
    }

    #[test]
    fn each_subpath_is_a_contour_closed_on_its_own() {
        assert_eq!(
            contours("path('M0 0 L10 0 L10 10 Z M20 0 L30 0 L30 10 Z')"),
            vec![
                vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)],
                vec![(20.0, 0.0), (30.0, 0.0), (30.0, 10.0)],
            ]
        );
        // Closing with `L` to the start gives the same contour as `Z`.
        assert_eq!(
            contours("path('M0 0 L10 0 L10 10 L0 0 M20 0 L30 0 L30 10 Z')"),
            contours("path('M0 0 L10 0 L10 10 Z M20 0 L30 0 L30 10 Z')")
        );
    }

    #[test]
    fn a_segment_after_z_starts_a_subpath_at_the_start_of_the_closed_one() {
        assert_eq!(
            contours("path('M0 0 L10 0 L10 10 Z L20 20 L0 20')"),
            vec![
                vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)],
                vec![(0.0, 0.0), (20.0, 20.0), (0.0, 20.0)],
            ]
        );
    }

    #[test]
    fn subpaths_without_an_area_are_dropped() {
        assert_eq!(
            vertices("path('M0 0 M10 10 L20 10 L20 20 Z M50 50 L60 60')"),
            vec![(10.0, 10.0), (20.0, 10.0), (20.0, 20.0)]
        );
        // A shape with nothing to fill still has a vertex, and clips everything.
        assert_eq!(vertices("path('M5 6')"), vec![(5.0, 6.0)]);
        assert_eq!(vertices("path('M5 6 L9 9')"), vec![(5.0, 6.0)]);
    }

    #[test]
    fn invalid_path_data_is_invalid() {
        for value in [
            "path()",
            "path('')",
            "path('  ')",
            "path(M0 0 L9 9)",
            "path('M0 0 L9 9\")",
            "path(\"M0 0 L9 9')",
            "path('M0 0 L9 9' 'M1 1')",
            "path('M0 0 \\4c L9 9')",
            "path('L10 10')",
            "path('Z')",
            "path('M')",
            "path('M0')",
            "path('M0 0 L10')",
            "path('M0 0 X5 5')",
            "path('M0 0 L5 5 junk')",
            "path('M0 0 A5 5 0 2 0 9 9')",
            "path(5)",
            // Not finite as an f32.
            "path('M0 0 L1e39 0 L0 1e39')",
        ] {
            assert_eq!(parse(value), None, "{value}");
        }
    }

    fn lines(count: usize) -> String {
        let zigzag: String = (0..count)
            .map(|index| format!(" L{} {}", index % 2 * 10, index + 1))
            .collect();
        format!("path('M0 0{zigzag}')")
    }

    #[test]
    fn a_path_may_have_as_many_vertices_as_the_limit() {
        assert_eq!(vertices(&lines(MAX_PATH_VERTICES - 1)).len(), MAX_PATH_VERTICES);
        assert_eq!(parse(&lines(MAX_PATH_VERTICES)), None);
        assert_eq!(parse(&lines(MAX_PATH_VERTICES * 2)), None);
    }

    #[test]
    fn a_curve_that_needs_too_many_vertices_is_coarsened_and_then_rejected() {
        let arch = |size: &str| format!("path('M0 0 C0 {size} {size} {size} {size} 0')");
        assert!(vertices(&arch("1000")).len() < MAX_PATH_VERTICES);
        // About 6500 chords at 0.25px: a coarser tolerance that fits is used.
        let coarsened = vertices(&arch("10000000"));
        assert!(coarsened.len() <= MAX_PATH_VERTICES);
        assert!(coarsened.len() > 1000, "{}", coarsened.len());
        assert_eq!(*coarsened.last().unwrap(), (1.0e7, 0.0));
        // Not even 4px is enough.
        assert_eq!(parse(&arch("1000000000")), None);
        assert_eq!(parse("path('M0 0 Q0 1e9 1e9 0')"), None);
    }

    #[test]
    fn many_curves_share_the_vertex_limit() {
        let arch = " c0 100 100 100 100 0";
        let path = |count: usize| format!("path('M0 0{}')", arch.repeat(count));
        let few = vertices(&path(20));
        // 21 chords an arch at a quarter pixel.
        assert!(few.len() > 20 * 15, "{}", few.len());
        // 500 arches would need about 10500 at that tolerance: a coarser one is used.
        let coarsened = vertices(&path(500));
        assert!(coarsened.len() <= MAX_PATH_VERTICES);
        assert!(coarsened.len() > 3000, "{}", coarsened.len());
        // 1000 arches need 6 chords each at the coarsest tolerance.
        assert_eq!(parse(&path(1000)), None);
    }

    /// Whether the contours fill a point, as the shader and `polygon_contains` in gpui decide.
    fn fills(contours: &[Vec<(f32, f32)>], point: (f32, f32), even_odd: bool) -> bool {
        let (mut winding, mut crossings) = (0i32, 0u32);
        for contour in contours {
            for (index, start) in contour.iter().enumerate() {
                let end = contour[(index + 1) % contour.len()];
                let side =
                    (end.0 - start.0) * (point.1 - start.1) - (point.0 - start.0) * (end.1 - start.1);
                if start.1 <= point.1 {
                    if end.1 > point.1 {
                        crossings += (side > 0.0) as u32;
                        winding += (side > 0.0) as i32;
                    }
                } else if end.1 <= point.1 {
                    crossings += (side < 0.0) as u32;
                    winding -= (side < 0.0) as i32;
                }
            }
        }
        if even_odd { crossings % 2 == 1 } else { winding != 0 }
    }

    /// Every sample on a grid that avoids the integer coordinates the edges are on.
    fn assert_fills_like(data: &str, even_odd: bool, expected: impl Fn((f32, f32)) -> bool) {
        let contours = contours(&format!("path({}, '{data}')", if even_odd { "evenodd" } else { "nonzero" }));
        for row in -4..64 {
            for column in -4..64 {
                let point = (column as f32 * 2.5 + 0.3, row as f32 * 2.5 + 0.7);
                assert_eq!(
                    fills(&contours, point, even_odd),
                    expected(point),
                    "{data} (evenodd: {even_odd}) at {point:?}"
                );
            }
        }
    }

    fn inside(point: (f32, f32), left: f32, top: f32, right: f32, bottom: f32) -> bool {
        point.0 > left && point.0 < right && point.1 > top && point.1 < bottom
    }

    #[test]
    fn subpaths_fill_together_under_both_rules() {
        // A 100px square, a 50px square inside it, and a separate square to the right. Every
        // square is drawn clockwise, so `nonzero` fills the inner one and `evenodd` makes it a hole.
        let same_direction = "M0 0 H100 V100 H0 Z M25 25 H75 V75 H25 Z M110 10 H150 V50 H110 Z";
        let outer = |point| inside(point, 0.0, 0.0, 100.0, 100.0);
        let inner = |point| inside(point, 25.0, 25.0, 75.0, 75.0);
        let separate = |point| inside(point, 110.0, 10.0, 150.0, 50.0);
        assert_fills_like(same_direction, false, |point| outer(point) || separate(point));
        assert_fills_like(same_direction, true, |point| {
            (outer(point) && !inner(point)) || separate(point)
        });
        // The inner square runs the other way, so both rules cut it out.
        let opposite = "M0 0 H100 V100 H0 Z M25 25 V75 H75 V25 Z M110 10 H150 V50 H110 Z";
        for even_odd in [false, true] {
            assert_fills_like(opposite, even_odd, |point| {
                (outer(point) && !inner(point)) || separate(point)
            });
        }
    }

    #[test]
    fn overlapping_subpaths_keep_their_rule_wherever_they_start() {
        // Two overlapping squares drawn the same way: a point in both has winding 2 and crosses
        // twice. `nonzero` fills the union, `evenodd` leaves the overlap out.
        let overlapping = "M0 0 H80 V80 H0 Z M40 40 H120 V120 H40 Z";
        let first = |point| inside(point, 0.0, 0.0, 80.0, 80.0);
        let second = |point| inside(point, 40.0, 40.0, 120.0, 120.0);
        assert_fills_like(overlapping, false, |point| first(point) || second(point));
        assert_fills_like(overlapping, true, |point| first(point) != second(point));
        // The first subpath is far from the others: nothing joins it to them.
        let far_origin = "M0 150 L1 150 L1 151 Z M10 10 H60 V60 H10 Z M70 70 H110 V110 H70 Z";
        assert_fills_like(far_origin, false, |point| {
            inside(point, 10.0, 10.0, 60.0, 60.0) || inside(point, 70.0, 70.0, 110.0, 110.0)
        });
        assert_fills_like(far_origin, true, |point| {
            inside(point, 10.0, 10.0, 60.0, 60.0) || inside(point, 70.0, 70.0, 110.0, 110.0)
        });
    }
}
