//! The CSS `transform`, `translate`, `rotate`, `scale` and `transform-origin` strings, parsed
//! into the functions gpui draws an element through.
//!
//! Only the 2D plane is drawn. Functions that move or scale along z are accepted and do
//! nothing, which is what they do without a `perspective`; a rotation about x or y, `perspective()`
//! and the 4x4 `matrix3d()` are invalid, so the declaration is ignored.

use gpui::{ElementTransform, TransformFunction, TransformLength};

use super::length::LinearLength;

use super::cache::{Cache, Inputs};
use super::dimension::{
    parse_length, parse_length_percentage, parse_number, parse_number_or_percentage, split_dimension, LengthPercentage,
};
use super::length::LengthContext;
use super::syntax::{function_args, split_commas, split_whitespace};
use crate::style::StyleDesc;

/// A transform property's value: the keyword, or the functions it stands for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    None,
    /// May be empty: `translateZ(0)` is a transform that moves nothing, and still gives its
    /// element a stacking context and a containing block.
    List(Vec<TransformFunction>),
}

impl Value {
    fn functions(&self) -> &[TransformFunction] {
        match self {
            Self::None => &[],
            Self::List(functions) => functions,
        }
    }
}

pub(crate) type Origin = (TransformLength, TransformLength);

thread_local! {
    static TRANSFORM: Cache<Option<Value>> = Cache::new();
    static TRANSLATE: Cache<Option<Value>> = Cache::new();
    static ROTATE: Cache<Option<Value>> = Cache::new();
    static SCALE: Cache<Option<Value>> = Cache::new();
    static ORIGIN: Cache<Option<Origin>> = Cache::new();
}

/// `None` is an invalid value, which CSS ignores as a whole.
pub(crate) fn parse_transform(value: &str, lengths: &LengthContext) -> Option<Value> {
    TRANSFORM.with(|cache| {
        cache.get_or_parse(value, Inputs { colors: None, lengths }, || parse_transform_uncached(value, lengths))
    })
}

pub(crate) fn parse_translate(value: &str, lengths: &LengthContext) -> Option<Value> {
    TRANSLATE.with(|cache| {
        cache.get_or_parse(value, Inputs { colors: None, lengths }, || parse_translate_uncached(value, lengths))
    })
}

pub(crate) fn parse_rotate(value: &str) -> Option<Value> {
    ROTATE.with(|cache| {
        cache.get_or_parse(value, Inputs { colors: None, lengths: &LengthContext::initial(0.0, 0.0) }, || {
            parse_rotate_uncached(value)
        })
    })
}

pub(crate) fn parse_scale(value: &str) -> Option<Value> {
    SCALE.with(|cache| {
        cache.get_or_parse(value, Inputs { colors: None, lengths: &LengthContext::initial(0.0, 0.0) }, || {
            parse_scale_uncached(value)
        })
    })
}

pub(crate) fn parse_origin(value: &str, lengths: &LengthContext) -> Option<Origin> {
    ORIGIN.with(|cache| {
        cache.get_or_parse(value, Inputs { colors: None, lengths }, || parse_origin_uncached(value, lengths))
    })
}

/// The five properties of a style, each `None` where it is unset or invalid.
pub(crate) struct Parts {
    pub translate: Option<Value>,
    pub rotate: Option<Value>,
    pub scale: Option<Value>,
    pub list: Option<Value>,
    pub origin: Option<Origin>,
}

impl Parts {
    pub(crate) fn of(style: &StyleDesc, lengths: &LengthContext) -> Self {
        Self {
            translate: style.translate.as_deref().and_then(|value| parse_translate(value, lengths)),
            rotate: style.rotate.as_deref().and_then(parse_rotate),
            scale: style.scale.as_deref().and_then(parse_scale),
            // Motion writes its keys into `transform`, so they replace it.
            list: motion_list(style, lengths)
                .map(Value::List)
                .or_else(|| style.transform.as_deref().and_then(|value| parse_transform(value, lengths))),
            origin: motion_origin(style, lengths, style.transform_origin.as_deref().and_then(|value| parse_origin(value, lengths))),
        }
    }

    /// Whether the element has a transform in CSS's sense, which is what gives it a stacking
    /// context and makes it the containing block of its absolute and fixed descendants: some
    /// property has a value other than `none`, an identity one included.
    pub(crate) fn is_active(&self) -> bool {
        [&self.translate, &self.rotate, &self.scale, &self.list]
            .into_iter()
            .any(|value| matches!(value, Some(Value::List(_))))
    }

    /// The transform gpui draws with. `None` when no property has a valid value, so the element
    /// is left as it is. A `none` that is valid is a transform with no functions, which a
    /// refinement needs to clear the transform of the style under it.
    ///
    /// CSS Transforms 2 applies, around the origin, `translate`, then `rotate`, then `scale`, then
    /// `transform`, each to what the ones after it have placed: the matrices multiply in that order.
    pub(crate) fn element_transform(&self) -> Option<ElementTransform> {
        let values = [&self.translate, &self.rotate, &self.scale, &self.list];
        if values.iter().all(|value| value.is_none()) {
            return None;
        }
        Some(ElementTransform {
            functions: values.into_iter().flatten().flat_map(|value| value.functions().iter().cloned()).collect(),
            origin: self.origin.unwrap_or(DEFAULT_ORIGIN),
            origin_z: 0.0,
            animating: false,
        })
    }
}

fn transform_length(length: &LinearLength, lengths: &LengthContext) -> TransformLength {
    let (px, fraction) = lengths.split(length);
    TransformLength { px: px as f32, fraction: fraction as f32 }
}

/// The functions of Motion's keys, in the order of its `buildTransform`; `None` when the style has
/// none. A key at its default stays in the list: the element has a transform whatever the value.
/// The 3D keys are not read: nothing draws them.
fn motion_list(style: &StyleDesc, lengths: &LengthContext) -> Option<Vec<TransformFunction>> {
    let zero = TransformLength::default();
    let mut functions = Vec::new();
    let horizontal = |length: &LinearLength| TransformFunction::Translate(transform_length(length, lengths), zero);
    let vertical = |length: &LinearLength| TransformFunction::Translate(zero, transform_length(length, lengths));
    functions.extend(style.x.as_ref().map(horizontal));
    functions.extend(style.y.as_ref().map(vertical));
    functions.extend(style.translate_x.as_ref().map(horizontal));
    functions.extend(style.translate_y.as_ref().map(vertical));
    functions.extend(style.motion_scale.map(|factor| TransformFunction::Scale(factor as f32, factor as f32)));
    functions.extend(style.scale_x.map(|factor| TransformFunction::Scale(factor as f32, 1.0)));
    functions.extend(style.scale_y.map(|factor| TransformFunction::Scale(1.0, factor as f32)));
    functions.extend(style.motion_rotate.map(|degrees| TransformFunction::Rotate(degrees.to_radians() as f32)));
    functions.extend(style.rotate_z.map(|degrees| TransformFunction::Rotate(degrees.to_radians() as f32)));
    functions.extend(style.skew.map(|degrees| TransformFunction::Skew(degrees.to_radians() as f32, 0.0)));
    functions.extend(style.skew_x.map(|degrees| TransformFunction::SkewX(degrees.to_radians() as f32)));
    functions.extend(style.skew_y.map(|degrees| TransformFunction::SkewY(degrees.to_radians() as f32)));
    (!functions.is_empty()).then_some(functions)
}

/// `transform-origin` with the axes `originX` and `originY` replace.
fn motion_origin(style: &StyleDesc, lengths: &LengthContext, declared: Option<Origin>) -> Option<Origin> {
    if style.origin_x.is_none() && style.origin_y.is_none() {
        return declared;
    }
    let base = declared.unwrap_or(DEFAULT_ORIGIN);
    Some((
        style.origin_x.as_ref().map_or(base.0, |length| transform_length(length, lengths)),
        style.origin_y.as_ref().map_or(base.1, |length| transform_length(length, lengths)),
    ))
}

/// `50% 50%`.
const DEFAULT_ORIGIN: Origin = (
    TransformLength { px: 0.0, fraction: 0.5 },
    TransformLength { px: 0.0, fraction: 0.5 },
);

/// The transform `style` declares, for the element whose lengths are `lengths`.
pub(crate) fn of(style: &StyleDesc, lengths: &LengthContext) -> Option<ElementTransform> {
    if !declares(style) {
        return None;
    }
    Parts::of(style, lengths).element_transform().map(|transform| ElementTransform {
        animating: style.transform_in_motion,
        ..transform
    })
}

/// Whether the element has a transform, see [`Parts::is_active`], for a style that declares a
/// valid one: `Some(false)` for `none`, which a state style needs in order to clear the flag of
/// the style under it.
pub(crate) fn is_active_if_declared(style: &StyleDesc, lengths: &LengthContext) -> Option<bool> {
    if style.animating_transform {
        return Some(true);
    }
    if !declares(style) {
        return None;
    }
    let parts = Parts::of(style, lengths);
    parts.element_transform().map(|_| parts.is_active())
}

/// Whether the element has a transform, see [`Parts::is_active`].
pub(crate) fn is_active(style: &StyleDesc, lengths: &LengthContext) -> bool {
    style.animating_transform || (declares(style) && Parts::of(style, lengths).is_active())
}

fn declares(style: &StyleDesc) -> bool {
    style.transform.is_some()
        || style.translate.is_some()
        || style.rotate.is_some()
        || style.scale.is_some()
        || has_motion_keys(style)
}

/// Whether the style has a key of Motion's transform. `originX` and `originY` are not one: they
/// place a transform and are none.
pub(crate) fn has_motion_keys(style: &StyleDesc) -> bool {
    [&style.x, &style.y, &style.translate_x, &style.translate_y].into_iter().any(Option::is_some)
        || [
            style.motion_scale,
            style.scale_x,
            style.scale_y,
            style.motion_rotate,
            style.rotate_z,
            style.skew,
            style.skew_x,
            style.skew_y,
        ]
        .into_iter()
        .any(|key| key.is_some())
}

fn parse_transform_uncached(value: &str, lengths: &LengthContext) -> Option<Value> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") {
        return Some(Value::None);
    }
    let tokens = split_whitespace(value);
    if tokens.is_empty() {
        return None;
    }
    let mut functions = Vec::new();
    for token in tokens {
        parse_function(token, lengths, &mut functions)?;
    }
    Some(Value::List(functions))
}

/// Appends what one function of a `transform` list stands for.
fn parse_function(token: &str, lengths: &LengthContext, out: &mut Vec<TransformFunction>) -> Option<()> {
    let name = token[..token.find('(')?].to_ascii_lowercase();
    let args = split_commas(function_args(token, &name)?);
    let length = |arg: &str| translate_length(arg, lengths);
    let zero = TransformLength::default();
    match (name.as_str(), args.as_slice()) {
        ("translate", [x]) => out.push(TransformFunction::Translate(length(x)?, zero)),
        ("translate", [x, y]) => out.push(TransformFunction::Translate(length(x)?, length(y)?)),
        ("translatex", [x]) => out.push(TransformFunction::Translate(length(x)?, zero)),
        ("translatey", [y]) => out.push(TransformFunction::Translate(zero, length(y)?)),
        ("translate3d", [x, y, z]) => {
            parse_length(z, lengths)?;
            out.push(TransformFunction::Translate(length(x)?, length(y)?));
        }
        ("translatez", [z]) => {
            parse_length(z, lengths)?;
        }
        ("scale", [x]) => {
            let x = scale_factor(x)?;
            out.push(TransformFunction::Scale(x, x));
        }
        ("scale", [x, y]) => out.push(TransformFunction::Scale(scale_factor(x)?, scale_factor(y)?)),
        ("scalex", [x]) => out.push(TransformFunction::Scale(scale_factor(x)?, 1.0)),
        ("scaley", [y]) => out.push(TransformFunction::Scale(1.0, scale_factor(y)?)),
        ("scale3d", [x, y, z]) => {
            scale_factor(z)?;
            out.push(TransformFunction::Scale(scale_factor(x)?, scale_factor(y)?));
        }
        ("scalez", [z]) => {
            scale_factor(z)?;
        }
        ("rotate" | "rotatez", [angle]) => out.push(TransformFunction::Rotate(parse_angle(angle)?)),
        ("skew", [x]) => out.push(TransformFunction::Skew(parse_angle(x)?, 0.0)),
        ("skew", [x, y]) => out.push(TransformFunction::Skew(parse_angle(x)?, parse_angle(y)?)),
        ("skewx", [x]) => out.push(TransformFunction::SkewX(parse_angle(x)?)),
        ("skewy", [y]) => out.push(TransformFunction::SkewY(parse_angle(y)?)),
        (INTERPOLATED, [progress, from, to]) => out.push(TransformFunction::Interpolated {
            from: parse_transform_uncached(from, lengths)?.functions().to_vec(),
            to: parse_transform_uncached(to, lengths)?.functions().to_vec(),
            progress: finite_number(progress)? as f32,
        }),
        ("matrix", [a, b, c, d, e, f]) => {
            let mut values = [0.0f32; 6];
            for (slot, arg) in values.iter_mut().zip([a, b, c, d, e, f]) {
                *slot = finite_number(arg)? as f32;
            }
            out.push(TransformFunction::Matrix(values));
        }
        _ => return None,
    }
    Some(())
}

/// How [`format_list`] writes `TransformFunction::Interpolated`, which no CSS function stands for: an
/// animation hands its style to the renderer as text, and the list in the middle of a blend between lists
/// that do not pair up has to survive that. `-gpuix-interpolated(<progress>, <from list>, <to list>)`.
const INTERPOLATED: &str = "-gpuix-interpolated";

fn translate_length(token: &str, lengths: &LengthContext) -> Option<TransformLength> {
    Some(match parse_length_percentage(token, lengths)? {
        LengthPercentage::Length(px) => TransformLength::pixels(px as f32),
        LengthPercentage::Percent(fraction) => TransformLength::fraction(fraction as f32),
        LengthPercentage::Calc { px, percent } => TransformLength { px: px as f32, fraction: percent as f32 },
    })
}

/// A number, or a percentage as its fraction: `scale(50%)` is `scale(0.5)`.
fn scale_factor(token: &str) -> Option<f32> {
    parse_number_or_percentage(token).filter(|value| value.is_finite()).map(|value| value as f32)
}

/// A number, or a percentage as its fraction, as one factor of `scale`.
pub(crate) fn parse_factor(token: &str) -> Option<f64> {
    scale_factor(token).map(f64::from)
}

fn finite_number(token: &str) -> Option<f64> {
    parse_number(token).filter(|value| value.is_finite())
}

/// An angle in radians. A unitless number is only valid for zero.
fn parse_angle(token: &str) -> Option<f32> {
    parse_degrees(token).map(|degrees| degrees.to_radians() as f32)
}

/// An angle in degrees. A unitless number is only valid for zero.
pub(crate) fn parse_degrees(token: &str) -> Option<f64> {
    let (value, unit) = split_dimension(token.trim())?;
    let degrees = match unit.as_str() {
        "deg" => value,
        "grad" => value * 0.9,
        "rad" => value.to_degrees(),
        "turn" => value * 360.0,
        "" if value == 0.0 => 0.0,
        _ => return None,
    };
    degrees.is_finite().then_some(degrees)
}

/// `none | <length-percentage> [<length-percentage> <length>?]?`. The length along z does nothing,
/// as in `translate3d()`.
fn parse_translate_uncached(value: &str, lengths: &LengthContext) -> Option<Value> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") {
        return Some(Value::None);
    }
    let zero = TransformLength::default();
    let function = match split_whitespace(value).as_slice() {
        [x] => TransformFunction::Translate(translate_length(x, lengths)?, zero),
        [x, y] => TransformFunction::Translate(translate_length(x, lengths)?, translate_length(y, lengths)?),
        [x, y, z] => {
            parse_length(z, lengths)?;
            TransformFunction::Translate(translate_length(x, lengths)?, translate_length(y, lengths)?)
        }
        _ => return None,
    };
    Some(Value::List(vec![function]))
}

/// `none | <angle> | [x | y | z | <number>{3}] && <angle>`. Only the rotation about z is drawn,
/// so the axes `x` and `y` make the value invalid.
fn parse_rotate_uncached(value: &str) -> Option<Value> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") {
        return Some(Value::None);
    }
    let angle = match split_whitespace(value).as_slice() {
        [angle] => parse_angle(angle)?,
        [axis, angle] | [angle, axis] if axis.eq_ignore_ascii_case("z") => parse_angle(angle)?,
        [x, y, z, angle] => {
            let (x, y, z) = (finite_number(x)?, finite_number(y)?, finite_number(z)?);
            if x != 0.0 || y != 0.0 || z == 0.0 {
                return None;
            }
            parse_angle(angle)? * z.signum() as f32
        }
        _ => return None,
    };
    Some(Value::List(vec![TransformFunction::Rotate(angle)]))
}

/// `none | [<number> | <percentage>]{1,3}`. The third, along z, does nothing.
fn parse_scale_uncached(value: &str) -> Option<Value> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") {
        return Some(Value::None);
    }
    let function = match split_whitespace(value).as_slice() {
        [factor] => {
            let factor = scale_factor(factor)?;
            TransformFunction::Scale(factor, factor)
        }
        [x, y] => TransformFunction::Scale(scale_factor(x)?, scale_factor(y)?),
        [x, y, z] => {
            scale_factor(z)?;
            TransformFunction::Scale(scale_factor(x)?, scale_factor(y)?)
        }
        _ => return None,
    };
    Some(Value::List(vec![function]))
}

/// The `transform` text that [`parse_transform`] reads back as `functions`.
pub(crate) fn format_list(functions: &[TransformFunction]) -> String {
    if functions.is_empty() {
        return "none".to_string();
    }
    functions.iter().map(format_function).collect::<Vec<_>>().join(" ")
}

fn format_function(function: &TransformFunction) -> String {
    match function {
        TransformFunction::Translate(x, y) => format!("translate({}, {})", format_length(x), format_length(y)),
        TransformFunction::Scale(x, y) => format!("scale({x}, {y})"),
        TransformFunction::Rotate(angle) => format!("rotate({angle}rad)"),
        TransformFunction::Skew(x, y) => format!("skew({x}rad, {y}rad)"),
        TransformFunction::SkewX(x) => format!("skewX({x}rad)"),
        TransformFunction::SkewY(y) => format!("skewY({y}rad)"),
        TransformFunction::Matrix([a, b, c, d, e, f]) => format!("matrix({a}, {b}, {c}, {d}, {e}, {f})"),
        TransformFunction::Translate3d(x, y, z) => {
            format!("translate3d({}, {}, {z}px)", format_length(x), format_length(y))
        }
        TransformFunction::Scale3d(x, y, z) => format!("scale3d({x}, {y}, {z})"),
        TransformFunction::Rotate3d(x, y, z, angle) => format!("rotate3d({x}, {y}, {z}, {angle}rad)"),
        TransformFunction::Perspective(Some(distance)) => format!("perspective({distance}px)"),
        TransformFunction::Perspective(None) => "perspective(none)".to_string(),
        TransformFunction::Matrix3d(values) => {
            format!("matrix3d({})", values.iter().map(f32::to_string).collect::<Vec<_>>().join(", "))
        }
        TransformFunction::Interpolated { from, to, progress } => {
            format!("{INTERPOLATED}({progress}, {}, {})", format_list(from), format_list(to))
        }
    }
}

fn format_length(length: &TransformLength) -> String {
    let percent = f64::from(length.fraction) * 100.0;
    if length.fraction == 0.0 {
        format!("{}px", length.px)
    } else if length.px == 0.0 {
        format!("{percent}%")
    } else if percent < 0.0 {
        format!("calc({}px - {}%)", length.px, -percent)
    } else {
        format!("calc({}px + {percent}%)", length.px)
    }
}

/// The `translate` text that [`parse_translate`] reads back as one translation, or `none`.
pub(crate) fn format_translate(functions: &[TransformFunction]) -> String {
    match functions {
        [] => "none".to_string(),
        [TransformFunction::Translate(x, y)] => format!("{} {}", format_length(x), format_length(y)),
        other => unreachable!("translate was given {other:?}"),
    }
}

/// The `rotate` text that [`parse_rotate`] reads back as one rotation, or `none`.
pub(crate) fn format_rotate(functions: &[TransformFunction]) -> String {
    match functions {
        [] => "none".to_string(),
        [TransformFunction::Rotate(angle)] => format!("{angle}rad"),
        other => unreachable!("rotate was given {other:?}"),
    }
}

/// The `scale` text that [`parse_scale`] reads back as one scaling, or `none`.
pub(crate) fn format_scale(functions: &[TransformFunction]) -> String {
    match functions {
        [] => "none".to_string(),
        [TransformFunction::Scale(x, y)] => format!("{x} {y}"),
        other => unreachable!("scale was given {other:?}"),
    }
}

/// The `transform-origin` text that [`parse_origin`] reads back as `origin`.
pub(crate) fn format_origin(origin: &Origin) -> String {
    format!("{} {}", format_length(&origin.0), format_length(&origin.1))
}

#[derive(Clone, Copy, PartialEq)]
enum Position {
    Length(TransformLength),
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

impl Position {
    fn parse(token: &str, lengths: &LengthContext) -> Option<Self> {
        Some(match token.to_ascii_lowercase().as_str() {
            "left" => Self::Left,
            "right" => Self::Right,
            "top" => Self::Top,
            "bottom" => Self::Bottom,
            "center" => Self::Center,
            _ => Self::Length(translate_length(token, lengths)?),
        })
    }

    fn horizontal(self) -> Option<TransformLength> {
        match self {
            Self::Length(length) => Some(length),
            Self::Left => Some(TransformLength::fraction(0.0)),
            Self::Center => Some(TransformLength::fraction(0.5)),
            Self::Right => Some(TransformLength::fraction(1.0)),
            Self::Top | Self::Bottom => None,
        }
    }

    fn vertical(self) -> Option<TransformLength> {
        match self {
            Self::Length(length) => Some(length),
            Self::Top => Some(TransformLength::fraction(0.0)),
            Self::Center => Some(TransformLength::fraction(0.5)),
            Self::Bottom => Some(TransformLength::fraction(1.0)),
            Self::Left | Self::Right => None,
        }
    }
}

/// One or two positions, and a length along z that does nothing. One position names the
/// horizontal side, or the vertical one when it is `top` or `bottom`; two may come in either
/// order when a keyword says which is which.
fn parse_origin_uncached(value: &str, lengths: &LengthContext) -> Option<Origin> {
    let tokens = split_whitespace(value.trim());
    if tokens.len() == 3 {
        parse_length(tokens[2], lengths)?;
    }
    let center = TransformLength::fraction(0.5);
    match tokens.as_slice() {
        [only] => {
            let only = Position::parse(only, lengths)?;
            match (only.horizontal(), only.vertical()) {
                (Some(x), _) => Some((x, center)),
                (None, Some(y)) => Some((center, y)),
                (None, None) => None,
            }
        }
        [first, second] | [first, second, _] => {
            let (first, second) = (Position::parse(first, lengths)?, Position::parse(second, lengths)?);
            if let (Some(x), Some(y)) = (first.horizontal(), second.vertical()) {
                return Some((x, y));
            }
            Some((second.horizontal()?, first.vertical()?))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lengths() -> LengthContext {
        LengthContext::for_tests()
    }

    fn list(value: &str) -> Vec<TransformFunction> {
        match parse_transform(value, &lengths()) {
            Some(Value::List(functions)) => functions,
            other => panic!("{value}: {other:?}"),
        }
    }

    fn px(value: f32) -> TransformLength {
        TransformLength::pixels(value)
    }

    fn fraction(value: f32) -> TransformLength {
        TransformLength::fraction(value)
    }

    #[test]
    fn translate_takes_lengths_and_percentages() {
        assert_eq!(
            list("translate(10px, 50%) translateX(-5px) translateY(25%)"),
            [
                TransformFunction::Translate(px(10.0), fraction(0.5)),
                TransformFunction::Translate(px(-5.0), TransformLength::default()),
                TransformFunction::Translate(TransformLength::default(), fraction(0.25)),
            ]
        );
        assert_eq!(list("translate(7px)"), [TransformFunction::Translate(px(7.0), TransformLength::default())]);
        assert_eq!(
            list("translate(calc(10px + 50%), 0)"),
            [TransformFunction::Translate(TransformLength { px: 10.0, fraction: 0.5 }, TransformLength::default())]
        );
    }

    #[test]
    fn lengths_in_em_rem_and_viewport_units_are_resolved_for_the_element() {
        let base = lengths();
        let large = base.descend(Some(&crate::css::length::LinearLength::px(40.0)), false);
        let resolved = |context: &LengthContext| match parse_transform("translate(1em, 10vw)", context) {
            Some(Value::List(functions)) => functions,
            other => panic!("{other:?}"),
        };
        assert_ne!(resolved(&base), resolved(&large));
        assert_eq!(resolved(&large)[0], TransformFunction::Translate(px(40.0), px(10.0 * base_viewport_width() / 100.0)));
        let rem = crate::css::length::LinearLength { rem: 2.0, ..Default::default() };
        assert_eq!(list("translate(2rem, 0)")[0], TransformFunction::Translate(px(base.resolve(&rem, 0.0) as f32), px(0.0)));
    }

    fn base_viewport_width() -> f32 {
        match parse_transform("translate(100vw, 0)", &lengths()) {
            Some(Value::List(functions)) => match functions[0] {
                TransformFunction::Translate(width, _) => width.px,
                _ => unreachable!(),
            },
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn scale_takes_numbers_and_percentages() {
        assert_eq!(
            list("scale(2) scale(1.5, 0.5) scaleX(-1) scaleY(50%)"),
            [
                TransformFunction::Scale(2.0, 2.0),
                TransformFunction::Scale(1.5, 0.5),
                TransformFunction::Scale(-1.0, 1.0),
                TransformFunction::Scale(1.0, 0.5),
            ]
        );
    }

    #[test]
    fn angles_come_in_four_units_and_a_bare_zero() {
        let rotation = |value: &str| match list(value)[0] {
            TransformFunction::Rotate(angle) => angle,
            ref other => panic!("{other:?}"),
        };
        let quarter = std::f32::consts::FRAC_PI_2;
        for value in ["rotate(90deg)", "rotate(100grad)", "rotate(0.25turn)", "rotate(1.5707964rad)", "ROTATE(90DEG)"] {
            assert!((rotation(value) - quarter).abs() < 1e-5, "{value}");
        }
        assert_eq!(rotation("rotate(0)"), 0.0);
        assert_eq!(rotation("rotateZ(-90deg)"), -quarter);
        assert!(parse_transform("rotate(90)", &lengths()).is_none());
    }

    #[test]
    fn skew_and_matrix() {
        assert_eq!(
            list("skew(45deg) skew(0, 45deg) skewX(10deg) skewY(10deg)"),
            [
                TransformFunction::Skew(std::f32::consts::FRAC_PI_4, 0.0),
                TransformFunction::Skew(0.0, std::f32::consts::FRAC_PI_4),
                TransformFunction::SkewX(10f32.to_radians()),
                TransformFunction::SkewY(10f32.to_radians()),
            ]
        );
        assert_eq!(
            list("matrix(1, 0, 0, 1, 20, 30)"),
            [TransformFunction::Matrix([1.0, 0.0, 0.0, 1.0, 20.0, 30.0])]
        );
    }

    #[test]
    fn the_functions_along_z_do_nothing() {
        assert_eq!(
            list("translate3d(1px, 2px, 3px) scale3d(2, 3, 4) translateZ(5px) scaleZ(2)"),
            [
                TransformFunction::Translate(px(1.0), px(2.0)),
                TransformFunction::Scale(2.0, 3.0),
            ]
        );
        assert_eq!(parse_transform("translateZ(0)", &lengths()), Some(Value::List(Vec::new())));
        for value in ["rotateX(10deg)", "rotateY(10deg)", "perspective(100px)", "matrix3d(1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1)"] {
            assert!(parse_transform(value, &lengths()).is_none(), "{value}");
        }
    }

    #[test]
    fn none_is_a_value_of_its_own() {
        assert_eq!(parse_transform("none", &lengths()), Some(Value::None));
        assert_eq!(parse_transform(" NONE ", &lengths()), Some(Value::None));
        assert_eq!(parse_translate("none", &lengths()), Some(Value::None));
        assert_eq!(parse_rotate("none"), Some(Value::None));
        assert_eq!(parse_scale("none"), Some(Value::None));
    }

    #[test]
    fn an_invalid_list_is_none_as_a_whole() {
        for value in [
            "",
            "translate(10px, 20px) bogus(1)",
            "translate()",
            "translate(1px, 2px, 3px)",
            "translate(10)",
            "translateX(1px, 2px)",
            "scale(1px)",
            "scale(1, 2, 3)",
            "rotate(1px)",
            "rotate()",
            "matrix(1, 0, 0, 1, 0)",
            "translate(1px 2px)",
            "translate (1px, 2px)",
            "none translate(1px, 2px)",
            "scale(1e999)",
        ] {
            assert!(parse_transform(value, &lengths()).is_none(), "{value:?}");
        }
    }

    #[test]
    fn the_individual_properties() {
        let lengths = lengths();
        assert_eq!(
            parse_translate("10px 20%", &lengths),
            Some(Value::List(vec![TransformFunction::Translate(px(10.0), fraction(0.2))]))
        );
        assert_eq!(
            parse_translate("10px 20px 30px", &lengths),
            Some(Value::List(vec![TransformFunction::Translate(px(10.0), px(20.0))]))
        );
        assert_eq!(
            parse_translate("10px", &lengths),
            Some(Value::List(vec![TransformFunction::Translate(px(10.0), TransformLength::default())]))
        );
        assert_eq!(parse_rotate("90deg"), Some(Value::List(vec![TransformFunction::Rotate(std::f32::consts::FRAC_PI_2)])));
        assert_eq!(parse_rotate("z 90deg"), parse_rotate("90deg"));
        assert_eq!(parse_rotate("90deg z"), parse_rotate("90deg"));
        assert_eq!(parse_rotate("0 0 1 90deg"), parse_rotate("90deg"));
        assert_eq!(parse_rotate("0 0 -1 90deg"), parse_rotate("-90deg"));
        assert!(parse_rotate("x 90deg").is_none());
        assert!(parse_rotate("1 0 0 90deg").is_none());
        assert!(parse_rotate("90").is_none());
        assert_eq!(parse_scale("2"), Some(Value::List(vec![TransformFunction::Scale(2.0, 2.0)])));
        assert_eq!(parse_scale("2 50%"), Some(Value::List(vec![TransformFunction::Scale(2.0, 0.5)])));
        assert_eq!(parse_scale("2 3 4"), Some(Value::List(vec![TransformFunction::Scale(2.0, 3.0)])));
        assert!(parse_scale("2px").is_none());
        assert!(parse_scale("1 2 3 4").is_none());
        assert!(parse_translate("10", &lengths).is_none());
    }

    #[test]
    fn the_origin_takes_keywords_and_lengths_in_either_order() {
        let origin = |value: &str| parse_origin(value, &lengths());
        let (left, center, right) = (fraction(0.0), fraction(0.5), fraction(1.0));
        assert_eq!(origin("left"), Some((left, center)));
        assert_eq!(origin("bottom"), Some((center, right)));
        assert_eq!(origin("center"), Some((center, center)));
        assert_eq!(origin("10px"), Some((px(10.0), center)));
        assert_eq!(origin("0 0"), Some((px(0.0), px(0.0))));
        assert_eq!(origin("20px 30%"), Some((px(20.0), fraction(0.3))));
        assert_eq!(origin("right top"), Some((right, left)));
        assert_eq!(origin("top right"), Some((right, left)));
        assert_eq!(origin("center top"), Some((center, left)));
        assert_eq!(origin("top center"), Some((center, left)));
        assert_eq!(origin("left 10px"), Some((left, px(10.0))));
        assert_eq!(origin("10px bottom"), Some((px(10.0), right)));
        assert_eq!(origin("10px left"), Some((left, px(10.0))));
        assert_eq!(origin("50% 50% 10px"), Some((center, center)));
        assert_eq!(origin("LEFT TOP"), Some((left, left)));
        for value in ["", "left right", "top bottom", "left left", "10", "1px 2px 3px 4px", "50% 50% 10%", "nowhere"] {
            assert!(origin(value).is_none(), "{value:?}");
        }
    }

    fn style(json: &str) -> StyleDesc {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn the_properties_compose_in_the_order_of_css_transforms_2() {
        let transform = of(
            &style(r#"{"transform":"skewX(1deg)","scale":2,"rotate":"10deg","translate":"5px","transformOrigin":"0 0"}"#),
            &lengths(),
        )
        .unwrap();
        assert_eq!(
            transform.functions,
            [
                TransformFunction::Translate(px(5.0), TransformLength::default()),
                TransformFunction::Rotate(10f32.to_radians()),
                TransformFunction::Scale(2.0, 2.0),
                TransformFunction::SkewX(1f32.to_radians()),
            ]
        );
        assert_eq!(transform.origin, (px(0.0), px(0.0)));
    }

    #[test]
    fn the_origin_defaults_to_the_middle_and_alone_makes_no_transform() {
        assert_eq!(of(&style(r#"{"rotate":"1deg"}"#), &lengths()).unwrap().origin, DEFAULT_ORIGIN);
        assert_eq!(of(&style(r#"{"transformOrigin":"0 0"}"#), &lengths()), None);
        assert_eq!(of(&style("{}"), &lengths()), None);
    }

    #[test]
    fn an_invalid_property_is_ignored_and_the_others_still_apply() {
        let transform = of(&style(r#"{"transform":"bogus(1)","scale":2,"rotate":"banana"}"#), &lengths()).unwrap();
        assert_eq!(transform.functions, [TransformFunction::Scale(2.0, 2.0)]);
        assert_eq!(of(&style(r#"{"transform":"bogus(1)","rotate":"banana"}"#), &lengths()), None);
    }

    #[test]
    fn motions_keys_compose_in_the_order_of_build_transform() {
        let transform = of(
            &style(
                r#"{"skewY":4,"skewX":3,"skew":"5deg","rotateZ":10,"motionRotate":20,"scaleY":0.5,"scaleX":2,"motionScale":1.5,
                    "translateY":"7px","translateX":"5%","y":20,"x":10}"#,
            ),
            &lengths(),
        )
        .unwrap();
        let zero = TransformLength::default();
        assert_eq!(
            transform.functions,
            [
                TransformFunction::Translate(px(10.0), zero),
                TransformFunction::Translate(zero, px(20.0)),
                TransformFunction::Translate(fraction(0.05), zero),
                TransformFunction::Translate(zero, px(7.0)),
                TransformFunction::Scale(1.5, 1.5),
                TransformFunction::Scale(2.0, 1.0),
                TransformFunction::Scale(1.0, 0.5),
                TransformFunction::Rotate(20f32.to_radians()),
                TransformFunction::Rotate(10f32.to_radians()),
                TransformFunction::Skew(5f32.to_radians(), 0.0),
                TransformFunction::SkewX(3f32.to_radians()),
                TransformFunction::SkewY(4f32.to_radians()),
            ]
        );
    }

    #[test]
    fn motions_keys_replace_transform_but_not_the_individual_properties() {
        let transform = of(&style(r#"{"transform":"scale(9)","x":5,"translate":"1px"}"#), &lengths()).unwrap();
        assert_eq!(
            transform.functions,
            [
                TransformFunction::Translate(px(1.0), TransformLength::default()),
                TransformFunction::Translate(px(5.0), TransformLength::default()),
            ]
        );
    }

    #[test]
    fn a_motion_key_at_its_default_is_still_a_transform() {
        assert!(is_active(&style(r#"{"x":0}"#), &lengths()));
        assert!(is_active(&style(r#"{"motionScale":1}"#), &lengths()));
        assert!(!is_active(&style(r#"{"originX":0.5}"#), &lengths()));
        assert!(!is_active(&style(r#"{"z":4,"rotateX":10}"#), &lengths()));
    }

    #[test]
    fn origin_x_and_y_replace_one_axis_of_the_origin() {
        let origin = |json: &str| of(&style(json), &lengths()).unwrap().origin;
        assert_eq!(origin(r#"{"x":1,"originX":0}"#), (fraction(0.0), fraction(0.5)));
        assert_eq!(origin(r#"{"x":1,"originY":"10px"}"#), (fraction(0.5), px(10.0)));
        assert_eq!(origin(r#"{"x":1,"originX":1,"transformOrigin":"0 0"}"#), (fraction(1.0), px(0.0)));
        assert_eq!(origin(r#"{"x":1,"originX":"25%","originY":0.75}"#), (fraction(0.25), fraction(0.75)));
    }

    #[test]
    fn an_animating_transform_is_active_whatever_the_style_says() {
        let running = StyleDesc { animating_transform: true, ..Default::default() };
        assert!(is_active(&running, &lengths()));
        assert_eq!(is_active_if_declared(&running, &lengths()), Some(true));
        assert!(!is_active(&style(r#"{"transform":"none"}"#), &lengths()));
    }

    #[test]
    fn scale_factors_are_numbers_or_percentages() {
        assert_eq!(style(r#"{"motionScale":"1.5","scaleX":"50%","scaleY":2}"#).scale_x, Some(0.5));
        assert_eq!(style(r#"{"motionScale":"1.5"}"#).motion_scale, Some(1.5));
        assert!(serde_json::from_str::<StyleDesc>(r#"{"motionScale":"2px"}"#).is_err());
    }

    #[test]
    fn angles_are_degrees_or_css_angles() {
        let degrees = |json: &str| style(json).skew_x;
        assert_eq!(degrees(r#"{"skewX":30}"#), Some(30.0));
        assert_eq!(degrees(r#"{"skewX":"0.25turn"}"#), Some(90.0));
        assert_eq!(degrees(r#"{"skewX":"100grad"}"#), Some(90.0));
        assert!(serde_json::from_str::<StyleDesc>(r#"{"skewX":"30"}"#).is_err());
        assert!(serde_json::from_str::<StyleDesc>(r#"{"skewX":"fast"}"#).is_err());
    }

    #[test]
    fn none_is_a_transform_with_no_functions_and_is_not_active() {
        let none = style(r#"{"transform":"none"}"#);
        assert_eq!(of(&none, &lengths()).unwrap().functions, []);
        assert!(!is_active(&none, &lengths()));
        assert!(!is_active(&style(r#"{"transform":"none","translate":"none","rotate":"none","scale":"none"}"#), &lengths()));
        assert!(!is_active(&style(r#"{"transform":"bogus(1)"}"#), &lengths()));
        assert!(!is_active(&style("{}"), &lengths()));
    }

    #[test]
    fn any_property_with_a_value_is_active_even_if_it_moves_nothing() {
        for json in [
            r#"{"transform":"translateX(0)"}"#,
            r#"{"transform":"translateZ(0)"}"#,
            r#"{"translate":"0"}"#,
            r#"{"rotate":"0deg"}"#,
            r#"{"scale":1}"#,
            r#"{"transform":"none","scale":"1"}"#,
        ] {
            assert!(is_active(&style(json), &lengths()), "{json}");
        }
    }

    #[test]
    fn a_number_is_degrees_for_rotate_and_a_factor_for_scale() {
        let transform = of(&style(r#"{"rotate":90,"scale":1.5}"#), &lengths()).unwrap();
        assert_eq!(
            transform.functions,
            [TransformFunction::Rotate(std::f32::consts::FRAC_PI_2), TransformFunction::Scale(1.5, 1.5)]
        );
    }

    #[test]
    fn a_cached_parse_equals_the_uncached_one() {
        let lengths = lengths();
        for value in ["translate(1em, 10%) rotate(1turn)", "none", "bogus", "scale(2)"] {
            assert_eq!(parse_transform(value, &lengths), parse_transform_uncached(value, &lengths), "{value}");
            assert_eq!(parse_transform(value, &lengths), parse_transform_uncached(value, &lengths), "{value} on a hit");
        }
    }
}
