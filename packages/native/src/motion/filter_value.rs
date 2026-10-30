//! Filter lists as animated values, Filter Effects 1 "Interpolation of filters".
//!
//! Lists that name the same function at every index of the common prefix of neighbouring values blend
//! function by function, a shorter one padded with the identity of the function the longer has there
//! (`none` is the empty list, so every function is padded). Lists that differ somewhere in that prefix do
//! not blend: the track steps at the middle, as Chrome does (`CSSFilterListInterpolationType`).
//!
//! A track blends channels, so a list is flattened to the channels of its functions: one for an amount, a
//! blur radius or an angle, and seven for a `drop-shadow()` (offsets, blur, and the colour as
//! premultiplied Oklab like every other colour of motion).

use serde_json::Value;

use super::text::{pixels, six_digits, Text};
use super::value::{blend_in_srgb, Aligned, AnimValue, Blend, Color, Interpolation, Layout};
use crate::color::ColorContext;
use crate::css::filter::{self, Filter};
use crate::css::length::LengthContext;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Function {
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
        color: Color,
    },
}

/// Which function it is, which decides whether two lists pair up and how many channels it takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shape {
    Blur,
    Brightness,
    Contrast,
    Grayscale,
    HueRotate,
    Invert,
    Opacity,
    Saturate,
    Sepia,
    DropShadow,
}

/// Where the colour starts among the channels of a `drop-shadow()`: after its offsets and blur.
const SHADOW_COLOUR: usize = 3;
const SHADOW_CHANNELS: usize = 7;

impl Shape {
    fn channels(self) -> usize {
        match self {
            Self::DropShadow => SHADOW_CHANNELS,
            _ => 1,
        }
    }

    /// The function that changes nothing, which a shorter list is padded with.
    fn identity(self) -> Function {
        match self {
            Self::Blur => Function::Blur(0.0),
            Self::Brightness => Function::Brightness(1.0),
            Self::Contrast => Function::Contrast(1.0),
            Self::Grayscale => Function::Grayscale(0.0),
            Self::HueRotate => Function::HueRotate(0.0),
            Self::Invert => Function::Invert(0.0),
            Self::Opacity => Function::Opacity(1.0),
            Self::Saturate => Function::Saturate(1.0),
            Self::Sepia => Function::Sepia(0.0),
            Self::DropShadow => Function::DropShadow { offset_x: 0.0, offset_y: 0.0, blur: 0.0, color: Color::TRANSPARENT },
        }
    }
}

impl Function {
    fn shape(&self) -> Shape {
        match self {
            Self::Blur(_) => Shape::Blur,
            Self::Brightness(_) => Shape::Brightness,
            Self::Contrast(_) => Shape::Contrast,
            Self::Grayscale(_) => Shape::Grayscale,
            Self::HueRotate(_) => Shape::HueRotate,
            Self::Invert(_) => Shape::Invert,
            Self::Opacity(_) => Shape::Opacity,
            Self::Saturate(_) => Shape::Saturate,
            Self::Sepia(_) => Shape::Sepia,
            Self::DropShadow { .. } => Shape::DropShadow,
        }
    }

    fn push_channels(&self, channels: &mut Vec<f64>) {
        match self {
            Self::Blur(amount)
            | Self::Brightness(amount)
            | Self::Contrast(amount)
            | Self::Grayscale(amount)
            | Self::HueRotate(amount)
            | Self::Invert(amount)
            | Self::Opacity(amount)
            | Self::Saturate(amount)
            | Self::Sepia(amount) => channels.push(*amount),
            Self::DropShadow { offset_x, offset_y, blur, color } => {
                channels.extend([*offset_x, *offset_y, *blur]);
                channels.extend(color.channels());
            }
        }
    }

    fn from_channels(shape: Shape, channels: &[f64], legacy: bool) -> Self {
        let amount = channels[0];
        match shape {
            Shape::Blur => Self::Blur(amount),
            Shape::Brightness => Self::Brightness(amount),
            Shape::Contrast => Self::Contrast(amount),
            Shape::Grayscale => Self::Grayscale(amount),
            Shape::HueRotate => Self::HueRotate(amount),
            Shape::Invert => Self::Invert(amount),
            Shape::Opacity => Self::Opacity(amount),
            Shape::Saturate => Self::Saturate(amount),
            Shape::Sepia => Self::Sepia(amount),
            Shape::DropShadow => Self::DropShadow {
                offset_x: channels[0],
                offset_y: channels[1],
                blur: channels[2],
                color: Color::from_channels(&channels[SHADOW_COLOUR..SHADOW_CHANNELS], legacy),
            },
        }
    }

    /// The function held to the range of its amount, which is how Chrome computes one: a radius and
    /// `brightness()`, `contrast()` and `saturate()` are not negative, and the other amounts are at most 1.
    fn clamped(self) -> Self {
        match self {
            Self::Blur(radius) => Self::Blur(radius.max(0.0)),
            Self::Brightness(amount) => Self::Brightness(amount.max(0.0)),
            Self::Contrast(amount) => Self::Contrast(amount.max(0.0)),
            Self::Saturate(amount) => Self::Saturate(amount.max(0.0)),
            Self::Grayscale(amount) => Self::Grayscale(amount.clamp(0.0, 1.0)),
            Self::Invert(amount) => Self::Invert(amount.clamp(0.0, 1.0)),
            Self::Opacity(amount) => Self::Opacity(amount.clamp(0.0, 1.0)),
            Self::Sepia(amount) => Self::Sepia(amount.clamp(0.0, 1.0)),
            Self::DropShadow { offset_x, offset_y, blur, color } => {
                Self::DropShadow { offset_x, offset_y, blur: blur.max(0.0), color }
            }
            hue_rotate @ Self::HueRotate(_) => hue_rotate,
        }
    }
}

/// The functions of a parsed `filter`, their colours resolved in `colors`. `None` when one does not resolve.
fn from_parsed(filters: &[Filter], colors: &ColorContext) -> Option<Vec<Function>> {
    filters
        .iter()
        .map(|filter| {
            Some(
                match filter {
                    Filter::Blur(radius) => Function::Blur(*radius),
                    Filter::Brightness(amount) => Function::Brightness(*amount),
                    Filter::Contrast(amount) => Function::Contrast(*amount),
                    Filter::Grayscale(amount) => Function::Grayscale(*amount),
                    Filter::HueRotate(degrees) => Function::HueRotate(*degrees),
                    Filter::Invert(amount) => Function::Invert(*amount),
                    Filter::Opacity(amount) => Function::Opacity(*amount),
                    Filter::Saturate(amount) => Function::Saturate(*amount),
                    Filter::Sepia(amount) => Function::Sepia(*amount),
                    Filter::DropShadow { offset_x, offset_y, blur, color } => Function::DropShadow {
                        offset_x: *offset_x,
                        offset_y: *offset_y,
                        blur: *blur,
                        color: if color.eq_ignore_ascii_case("currentcolor") {
                            // The element's colour is a legacy one here, as Chrome blends the shadow of `drop-shadow(1px 1px)`.
                            Color::from_rgba(colors.current(), true)
                        } else {
                            Color::parse(color, colors)?
                        },
                    },
                }
                .clamped(),
            )
        })
        .collect()
}

/// The `filter` a style declares as a value, `None` when it declares none or one that does not parse.
pub(super) fn read(slot: &Option<String>, colors: &ColorContext, lengths: &LengthContext) -> Option<AnimValue> {
    let filters = filter::parse(slot.as_deref()?, lengths)?;
    from_parsed(&filters, colors).map(AnimValue::Filters)
}

pub(super) fn write(slot: &mut Option<String>, value: AnimValue) {
    match value {
        AnimValue::Filters(functions) => *slot = Some(format(&functions)),
        other => unreachable!("a filter field was given {other:?}"),
    }
}

pub(super) fn parse(name: &str, value: &Value, colors: &ColorContext, lengths: &LengthContext) -> Result<AnimValue, String> {
    let text = value
        .as_str()
        .ok_or_else(|| format!("motion {name} must be none or a list of CSS filter functions"))?;
    let filters = filter::parse(text, lengths).ok_or_else(|| {
        format!("motion {name} must be none or a list of CSS filter functions (url() is not supported): {text}")
    })?;
    from_parsed(&filters, colors)
        .map(AnimValue::Filters)
        .ok_or_else(|| format!("motion {name} has a drop-shadow() with an invalid colour: {text}"))
}

pub(super) fn clamp(functions: Vec<Function>) -> Vec<Function> {
    functions.into_iter().map(Function::clamped).collect()
}

/// What Motion hands the browser for a `none` keyframe of a `filter`: the functions of the first keyframe that has any, each at
/// its identity (`motion-dom`'s `filter.getAnimatableNone`), and a `drop-shadow()` as it is. Any other value is itself.
pub(super) fn animatable_none(value: &AnimValue, literal: &[AnimValue]) -> AnimValue {
    if !matches!(value, AnimValue::Filters(functions) if functions.is_empty()) {
        return value.clone();
    }
    let template = literal.iter().find_map(|frame| match frame {
        AnimValue::Filters(functions) if !functions.is_empty() => Some(functions),
        _ => None,
    });
    AnimValue::Filters(match template {
        Some(functions) => functions
            .iter()
            .map(|function| match function {
                Function::DropShadow { .. } => function.clone(),
                function => function.shape().identity(),
            })
            .collect(),
        None => Vec::new(),
    })
}

/// A number to six decimals: a spring leaves long tails, and a text written back is parsed again every frame.
fn number(value: f64) -> String {
    format!("{}", (value * 1e6).round() / 1e6 + 0.0)
}

/// The `filter` text that [`filter::parse`] reads back as `functions`.
pub(super) fn format(functions: &[Function]) -> String {
    if functions.is_empty() {
        return "none".to_string();
    }
    functions
        .iter()
        .map(|function| match function {
            Function::Blur(radius) => format!("blur({}px)", number(*radius)),
            Function::Brightness(amount) => format!("brightness({})", number(*amount)),
            Function::Contrast(amount) => format!("contrast({})", number(*amount)),
            Function::Grayscale(amount) => format!("grayscale({})", number(*amount)),
            Function::HueRotate(degrees) => format!("hue-rotate({}deg)", number(*degrees)),
            Function::Invert(amount) => format!("invert({})", number(*amount)),
            Function::Opacity(amount) => format!("opacity({})", number(*amount)),
            Function::Saturate(amount) => format!("saturate({})", number(*amount)),
            Function::Sepia(amount) => format!("sepia({})", number(*amount)),
            Function::DropShadow { offset_x, offset_y, blur, color } => format!(
                "drop-shadow({}px {}px {}px {})",
                number(*offset_x),
                number(*offset_y),
                number(*blur),
                color.to_css()
            ),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The list as Chrome writes a computed `filter`: the colour of a `drop-shadow()` first, numbers to six digits.
pub(super) fn computed_text(functions: &[Function], of: Text) -> String {
    if functions.is_empty() {
        return "none".to_string();
    }
    let amount = |name: &str, value: f64| format!("{name}({})", six_digits(value));
    functions
        .iter()
        .map(|function| match function {
            Function::Blur(radius) => format!("blur({})", pixels(*radius)),
            Function::Brightness(value) => amount("brightness", *value),
            Function::Contrast(value) => amount("contrast", *value),
            Function::Grayscale(value) => amount("grayscale", *value),
            Function::HueRotate(degrees) => format!("hue-rotate({}deg)", six_digits(*degrees)),
            Function::Invert(value) => amount("invert", *value),
            Function::Opacity(value) => amount("opacity", *value),
            Function::Saturate(value) => amount("saturate", *value),
            Function::Sepia(value) => amount("sepia", *value),
            Function::DropShadow { offset_x, offset_y, blur, color } => format!(
                "drop-shadow({} {} {} {})",
                match of {
                    Text::Keyframe => color.computed_text(),
                    Text::Transition => color.interpolated_text(),
                },
                pixels(*offset_x),
                pixels(*offset_y),
                pixels(*blur)
            ),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Several lists as channel vectors in one layout. The lists pair up when, at each index, every list that has a
/// function has the same one; a track of lists that do not pair steps from list to list.
pub(super) fn align(lists: &[&[Function]], interpolation: Interpolation) -> Aligned {
    let longest = lists.iter().map(|list| list.len()).max().unwrap_or_default();
    let mut shapes = Vec::with_capacity(longest);
    let mut pairs = true;
    for index in 0..longest {
        let mut have = lists.iter().filter_map(|list| list.get(index)).map(Function::shape);
        let first = have.next().expect("an index below the longest list is in some list");
        pairs &= have.all(|shape| shape == first);
        shapes.push(first);
    }
    if !pairs {
        return Aligned {
            layout: Layout::DiscreteFilters(lists.iter().map(|list| list.to_vec()).collect()),
            frames: (0..lists.len()).map(|index| vec![index as f64]).collect(),
        };
    }
    let padded: Vec<Vec<Function>> = lists
        .iter()
        .map(|list| {
            shapes
                .iter()
                .enumerate()
                .map(|(index, shape)| list.get(index).cloned().unwrap_or_else(|| shape.identity()))
                .collect()
        })
        .collect();
    let blends = padded
        .windows(2)
        .map(|window| {
            window[0]
                .iter()
                .zip(&window[1])
                .map(|pair| match pair {
                    (Function::DropShadow { color: from, .. }, Function::DropShadow { color: to, .. }) => {
                        interpolation.blend(from, to)
                    }
                    _ => Blend::Oklab,
                })
                .collect()
        })
        .collect();
    Aligned {
        layout: Layout::Filters { shapes, blends },
        frames: padded
            .iter()
            .map(|functions| {
                let mut channels = Vec::new();
                for function in functions {
                    function.push_channels(&mut channels);
                }
                channels
            })
            .collect(),
    }
}

pub(super) fn decode(shapes: &[Shape], blends: &[Vec<Blend>], channels: &[f64]) -> Vec<Function> {
    let mut rest = channels;
    shapes
        .iter()
        .enumerate()
        .map(|(index, shape)| {
            let (own, after) = rest.split_at(shape.channels());
            rest = after;
            Function::from_channels(*shape, own, blends.iter().all(|segment| segment[index] == Blend::Srgb))
        })
        .collect()
}

/// A colour that two legacy colours share blends in sRGB, which is not linear in the Oklab channels; the
/// rest of `channels` is already the linear blend.
pub(super) fn blend_colours(
    shapes: &[Shape],
    blends: &[Blend],
    channels: &mut [f64],
    from: &[f64],
    to: &[f64],
    progress: f64,
) {
    let mut start = 0;
    for (shape, blend) in shapes.iter().zip(blends) {
        if *shape == Shape::DropShadow && *blend == Blend::Srgb {
            let colour = start + SHADOW_COLOUR..start + SHADOW_CHANNELS;
            blend_in_srgb(&mut channels[colour.clone()], &from[colour.clone()], &to[colour], progress);
        }
        start += shape.channels();
    }
}

/// A speed measured along the functions of one list as one along another's: what the two share from the start
/// carries, and the functions the new list adds start at rest.
pub(super) fn velocity_in(from: &[Shape], to: &[Shape], rate: &[f64]) -> Option<Vec<f64>> {
    let shared = from.iter().zip(to).take_while(|(from, to)| from == to).count();
    if shared == 0 {
        return None;
    }
    let carried: usize = to[..shared].iter().map(|shape| shape.channels()).sum();
    let total: usize = to.iter().map(|shape| shape.channels()).sum();
    let mut velocity = rate[..carried].to_vec();
    velocity.resize(total, 0.0);
    Some(velocity)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;
    use web_time::Instant;

    use super::super::curve::{ease, parse_timing_function, Transition};
    use super::super::property::AnimProp;
    use super::super::track::Track;
    use super::super::value::{pair, pair_in, Colors, Pair};
    use super::*;
    use crate::style::StyleDesc;

    fn list(text: &str) -> AnimValue {
        AnimProp::Filter.parse(&json!(text), &Colors::root()).unwrap()
    }

    fn functions(text: &str) -> Vec<Function> {
        match list(text) {
            AnimValue::Filters(functions) => functions,
            other => panic!("{other:?}"),
        }
    }

    fn css_pair(from: &str, to: &str) -> Pair {
        pair_in("filter", &list(from), &list(to), Interpolation::Css).unwrap()
    }

    fn text_of(value: AnimValue) -> String {
        match value {
            AnimValue::Filters(functions) => computed_text(&functions, Text::Transition),
            other => panic!("{other:?}"),
        }
    }

    fn at(from: &str, to: &str, progress: f64) -> String {
        text_of(css_pair(from, to).at(progress))
    }

    #[test]
    fn the_functions_of_a_list_blend_one_by_one() {
        assert_eq!(at("blur(4px) brightness(1.5)", "blur(0px) brightness(1)", 0.5), "blur(2px) brightness(1.25)");
        assert_eq!(at("hue-rotate(90deg)", "hue-rotate(-90deg)", 0.5), "hue-rotate(0deg)");
        assert_eq!(at("brightness(50%)", "brightness(150%)", 0.25), "brightness(0.75)");
    }

    #[test]
    fn a_shorter_list_is_padded_with_the_identity_of_the_longer_ones_functions() {
        assert_eq!(at("blur(4px)", "blur(0px) brightness(2)", 0.5), "blur(2px) brightness(1.5)");
        assert_eq!(at("blur(8px) grayscale(1) invert(1)", "blur(0px)", 0.5), "blur(4px) grayscale(0.5) invert(0.5)");
        assert_eq!(at("opacity(0.2)", "opacity(1) sepia(1)", 0.5), "opacity(0.6) sepia(0.5)");
        assert_eq!(at("contrast(1)", "contrast(1) saturate(0) hue-rotate(90deg)", 0.5), "contrast(1) saturate(0.5) hue-rotate(45deg)");
    }

    #[test]
    fn none_is_the_empty_list_so_every_function_is_padded() {
        assert_eq!(at("none", "blur(8px) brightness(2)", 0.5), "blur(4px) brightness(1.5)");
        assert_eq!(at("brightness(2) blur(8px)", "none", 0.25), "brightness(1.75) blur(6px)");
        assert_eq!(at("none", "drop-shadow(rgb(0, 0, 0) 4px 4px 8px)", 0.5), "drop-shadow(rgba(0, 0, 0, 0.5) 2px 2px 4px)");
        assert_eq!(at("none", "none", 0.5), "none");
        assert!(!css_pair("none", "blur(4px)").is_discrete());
    }

    #[test]
    fn lists_that_differ_in_the_common_prefix_step_at_the_middle() {
        for (from, to) in [
            ("blur(4px)", "brightness(2)"),
            ("blur(4px) brightness(2)", "brightness(2) blur(4px)"),
            ("blur(2px) brightness(2)", "blur(4px) contrast(2)"),
            ("drop-shadow(1px 1px 1px red)", "blur(4px)"),
        ] {
            let pair = css_pair(from, to);
            assert!(pair.is_discrete(), "{from} to {to}");
            assert_eq!(text_of(pair.at(0.49)), text_of(list(from)), "{from} just before the middle");
            assert_eq!(text_of(pair.at(0.5)), text_of(list(to)), "{from} at the middle");
        }
        assert!(!css_pair("blur(2px) brightness(2)", "blur(4px) brightness(1) invert(1)").is_discrete());
    }

    #[test]
    fn a_pair_of_neighbours_is_judged_on_its_own_prefix() {
        let values = [list("blur(0px)"), list("blur(4px) brightness(2)"), list("blur(8px) brightness(1) invert(1)")];
        let aligned = align("filter", &values.iter().collect::<Vec<_>>());
        assert!(matches!(aligned.layout, Layout::Filters { .. }));
        assert_eq!(aligned.frames.len(), 3);
        assert_eq!(text_of(aligned.layout.decode(&aligned.frames[0])), "blur(0px) brightness(1) invert(0)");
        // A track whose lists name different functions at one index has no one layout.
        let values = [list("blur(0px) brightness(2)"), list("blur(4px)"), list("blur(8px) contrast(2)")];
        let aligned = align("filter", &values.iter().collect::<Vec<_>>());
        assert!(matches!(aligned.layout, Layout::DiscreteFilters(_)));
    }

    fn align(name: &str, values: &[&AnimValue]) -> super::super::value::Aligned {
        super::super::value::align_in(name, values, Interpolation::Css).unwrap()
    }

    #[test]
    fn keyframes_of_many_lists_share_one_layout() {
        let values = [list("none"), list("blur(4px) brightness(2)"), list("blur(8px) brightness(1)"), list("grayscale(1)")];
        let refs: Vec<&AnimValue> = values.iter().collect();
        assert!(matches!(align("filter", &refs[..3]).layout, Layout::Filters { .. }));
        let aligned = align("filter", &refs[..3]);
        let middle = aligned.layout.blend(1, &aligned.frames[1], &aligned.frames[2], 0.5);
        assert_eq!(text_of(aligned.layout.decode(&middle)), "blur(6px) brightness(1.5)");
        let first = aligned.layout.blend(0, &aligned.frames[0], &aligned.frames[1], 0.25);
        assert_eq!(text_of(aligned.layout.decode(&first)), "blur(1px) brightness(1.25)");
    }

    #[test]
    fn the_colours_of_two_legacy_drop_shadows_blend_in_srgb_and_the_rest_in_oklab() {
        assert_eq!(
            at("drop-shadow(2px 2px 4px rgb(255, 0, 0))", "drop-shadow(10px 10px 0px rgb(0, 0, 255))", 0.5),
            "drop-shadow(rgb(128, 0, 128) 6px 6px 2px)"
        );
        let modern = at("drop-shadow(2px 2px 4px oklch(0.6 0.1 30))", "drop-shadow(2px 2px 4px oklch(0.6 0.1 250))", 0.5);
        assert!(modern.starts_with("drop-shadow(oklab(0.6"), "{modern}");
        assert_eq!(
            at("drop-shadow(0px 0px 0px rgba(0, 0, 0, 0))", "drop-shadow(4px 4px 8px rgba(0, 0, 0, 0.5))", 0.5),
            "drop-shadow(rgba(0, 0, 0, 0.25) 2px 2px 4px)"
        );
        let from_transparent = at("drop-shadow(2px 2px 4px rgba(255, 0, 0, 0))", "drop-shadow(2px 2px 4px rgb(0, 0, 255))", 0.5);
        assert_eq!(from_transparent, "drop-shadow(rgba(0, 0, 255, 0.5) 2px 2px 4px)");
    }

    #[test]
    fn the_amounts_are_held_to_the_range_chrome_computes_them_in() {
        let clamped = |text: &str| match AnimProp::Filter.clamp_value(AnimValue::Filters(functions(text))) {
            AnimValue::Filters(functions) => format(&functions),
            other => panic!("{other:?}"),
        };
        let wild = AnimValue::Filters(vec![
            Function::Blur(-3.0),
            Function::Brightness(-1.0),
            Function::Contrast(-0.5),
            Function::Saturate(-2.0),
            Function::Grayscale(1.5),
            Function::Invert(-0.2),
            Function::Opacity(3.0),
            Function::Sepia(-1.0),
            Function::HueRotate(-400.0),
            Function::DropShadow { offset_x: -5.0, offset_y: 6.0, blur: -1.0, color: Color::TRANSPARENT },
        ]);
        let AnimValue::Filters(held) = AnimProp::Filter.clamp_value(wild) else { panic!() };
        assert_eq!(
            format(&held[..9]),
            "blur(0px) brightness(0) contrast(0) saturate(0) grayscale(1) invert(0) opacity(1) sepia(0) hue-rotate(-400deg)"
        );
        assert!(format(&held[9..]).starts_with("drop-shadow(-5px 6px 0px "));
        // Large amounts a function takes above 1 stay.
        assert_eq!(clamped("brightness(3) contrast(250%) saturate(4)"), "brightness(3) contrast(2.5) saturate(4)");
        // A value written out of range computes in range, so a transition starts from there.
        assert_eq!(clamped("grayscale(2) opacity(300%) sepia(1.5) invert(200%)"), "grayscale(1) opacity(1) sepia(1) invert(1)");
    }

    #[test]
    fn a_spring_that_overshoots_is_held_while_the_physics_keeps_the_raw_value() {
        let transition = Transition::parse(json!({ "type": "spring", "stiffness": 400, "damping": 8 })).unwrap();
        let track = Track::new("filter", &list("blur(2px) grayscale(0.2)"), list("blur(10px) grayscale(0.8)"), transition, Instant::now())
            .unwrap();
        let mut undershot = false;
        let mut overshot = false;
        for step in 0..120 {
            let sample = track.sample_at(Duration::from_millis(step * 10));
            let raw = [sample.position[0], sample.position[1]];
            undershot |= raw[0] < 0.0 || raw[1] < 0.0;
            overshot |= raw[1] > 1.0;
            let AnimValue::Filters(held) = AnimProp::Filter.clamp_value(track.value(&sample.position)) else { panic!() };
            let [Function::Blur(blur), Function::Grayscale(gray)] = held.as_slice() else { panic!("{held:?}") };
            assert!(*blur >= 0.0 && (0.0..=1.0).contains(gray), "{blur} {gray}");
        }
        assert!(undershot || overshot, "the spring never left the range, so nothing was held");
    }

    #[test]
    fn a_retarget_carries_the_speed_of_the_functions_the_lists_share() {
        let spring = || Transition::parse(json!({ "type": "spring", "stiffness": 300, "damping": 30 })).unwrap();
        let started = Instant::now();
        let first = Track::new("filter", &list("blur(4px)"), list("blur(0px)"), spring(), started).unwrap();
        let later = started + Duration::from_millis(50);
        let moving = first.sample(later);
        assert!(moving.velocity[0] < -1.0, "{:?}", moving.velocity);

        let current = first.current(later);
        let mut again = Track::new("filter", &current, list("blur(8px) brightness(1.5)"), spring(), later).unwrap();
        again.carry_velocity(&first, later);
        assert_eq!(again.start_velocity(), moving.velocity[0]);
        let sample = again.sample(later);
        assert_eq!(sample.velocity, vec![moving.velocity[0], 0.0]);

        // Lists that step have no speed to carry.
        let mut stepping = Track::new("filter", &current, list("brightness(1.5)"), spring(), later).unwrap();
        stepping.carry_velocity(&first, later);
        assert_eq!(stepping.start_velocity(), 0.0);
    }

    #[test]
    fn motion_writes_a_none_keyframe_as_the_identity_of_the_functions_of_the_others() {
        let literal = [list("none"), list("blur(5px) brightness(0.5)"), list("none")];
        let written = |index: usize| text_of(animatable_none(&literal[index], &literal));
        assert_eq!(written(0), "blur(0px) brightness(1)");
        assert_eq!(written(1), "blur(5px) brightness(0.5)");
        assert_eq!(written(2), "blur(0px) brightness(1)");
        let shadowed = [list("none"), list("drop-shadow(rgb(255, 0, 0) 2px 2px 4px) grayscale(1)")];
        assert_eq!(text_of(animatable_none(&shadowed[0], &shadowed)), "drop-shadow(rgb(255, 0, 0) 2px 2px 4px) grayscale(0)");
        assert_eq!(text_of(animatable_none(&list("none"), &[list("none"), list("none")])), "none");
    }

    #[test]
    fn an_empty_list_against_itself_is_a_still_track() {
        let transition = Transition::parse(json!({ "duration": 1, "ease": "linear" })).unwrap();
        let track = Track::new("filter", &list("none"), list("none"), transition, Instant::now()).unwrap();
        assert!(!track.moves());
        assert_eq!(text_of(track.current(Instant::now())), "none");
    }

    #[test]
    fn a_list_written_back_reads_as_the_same_list() {
        for text in [
            "none",
            "blur(4px)",
            "blur(0.123457px) brightness(1.5) contrast(0.25) grayscale(0.5) hue-rotate(-33.5deg) invert(1) opacity(0.01) saturate(2.75) sepia(0.333333)",
            "drop-shadow(2px -3px 4.5px rgb(255, 0, 0)) blur(1px)",
            "drop-shadow(0px 0px 0px rgba(10, 20, 30, 0.25))",
        ] {
            let value = functions(text);
            let written = format(&value);
            let back = functions(&written);
            assert_eq!(computed_text(&back, Text::Keyframe), computed_text(&value, Text::Keyframe), "{text} written as {written}");
            assert_eq!(format(&back), written, "{text}");
        }
        let mut slot = None;
        write(&mut slot, list("blur(4px) brightness(150%)"));
        assert_eq!(slot.as_deref(), Some("blur(4px) brightness(1.5)"));
        write(&mut slot, AnimValue::Filters(Vec::new()));
        assert_eq!(slot.as_deref(), Some("none"));
    }

    #[test]
    fn a_style_reads_as_a_value_and_none_or_nothing_as_the_initial_one() {
        let colors = Colors::root();
        let read = |text: Option<&str>| {
            let style: StyleDesc = serde_json::from_value(match text {
                Some(text) => json!({ "filter": text }),
                None => json!({}),
            })
            .unwrap();
            AnimProp::Filter.read(&style, &colors)
        };
        assert_eq!(read(Some("blur(4px) brightness(150%)")), Some(list("blur(4px) brightness(1.5)")));
        assert_eq!(read(Some("none")), Some(AnimValue::Filters(Vec::new())));
        assert_eq!(read(None), None);
        assert_eq!(read(Some("url(#blur)")), None);
        assert_eq!(read(Some("blur(-1px)")), None);
        assert_eq!(AnimProp::Filter.initial(None, &colors), Some(AnimValue::Filters(Vec::new())));
    }

    #[test]
    fn current_color_in_a_drop_shadow_is_the_colour_of_the_element() {
        let style: StyleDesc = serde_json::from_value(json!({ "color": "rgb(255, 0, 0)" })).unwrap();
        let colors = Colors::of(&crate::color::ColorContext::root(crate::color::Scheme::Light), Some(&style));
        let value = AnimProp::Filter.parse(&json!("drop-shadow(2px 2px 4px)"), &colors).unwrap();
        assert_eq!(text_of(value), "drop-shadow(rgb(255, 0, 0) 2px 2px 4px)");
    }

    #[test]
    fn a_url_or_a_value_that_is_no_filter_is_an_error() {
        for bad in [json!("url(#blur)"), json!("blur(-1px)"), json!("sparkle(1)"), json!(4), json!("")] {
            assert!(AnimProp::Filter.parse(&bad, &Colors::root()).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_filter_is_a_property_that_only_paints() {
        assert!(AnimProp::Filter.paints_only());
        assert_eq!(AnimProp::from_name("filter"), Some(AnimProp::Filter));
        assert_eq!(AnimProp::from_css_name("filter"), Some(AnimProp::Filter));
        assert_eq!(AnimProp::from_css_name("backdropFilter"), None);
    }

    #[test]
    fn a_filter_is_not_part_of_what_a_blend_of_a_pair_can_mix_with() {
        assert!(pair("filter", &list("blur(1px)"), &AnimValue::Number(1.0)).is_err());
    }

    const ORACLE: &str = include_str!("../../../react/src/__tests__/fixtures/filter-interp-oracle.json");

    fn style_of(css: &str) -> Option<StyleDesc> {
        let mut style = serde_json::Map::new();
        for declaration in css.split(';').filter(|declaration| !declaration.trim().is_empty()) {
            let (name, value) = declaration.split_once(':').unwrap();
            let value = value.trim();
            match name.trim() {
                "font-size" => style.insert("fontSize".to_string(), json!(value.trim_end_matches("px").parse::<f64>().unwrap())),
                "color" => style.insert("color".to_string(), json!(value)),
                other => panic!("the oracle styles {other}"),
            };
        }
        Some(serde_json::from_value(serde_json::Value::Object(style)).unwrap())
    }

    /// The numbers of a text with the tolerance their place gives them, and the text with each as `#`.
    fn numbers(text: &str) -> (String, Vec<(f64, f64)>) {
        let chars: Vec<char> = text.chars().collect();
        let mut skeleton = String::new();
        let mut found = Vec::new();
        let mut stack: Vec<(String, usize)> = Vec::new();
        let mut word = String::new();
        let mut index = 0;
        while index < chars.len() {
            let c = chars[index];
            let next_is_digit = chars.get(index + 1).is_some_and(|next| next.is_ascii_digit() || *next == '.');
            let starts_number = (c.is_ascii_digit() || c == '.' || (c == '-' && next_is_digit))
                && !chars.get(index.wrapping_sub(1)).is_some_and(|previous| previous.is_ascii_alphanumeric());
            if starts_number {
                let start = index;
                index += 1;
                while index < chars.len() && (chars[index].is_ascii_digit() || chars[index] == '.') {
                    index += 1;
                }
                let value: f64 = chars[start..index].iter().collect::<String>().parse().unwrap();
                let (name, count) = stack.last().cloned().unwrap_or_default();
                let tolerance = match (name.as_str(), count) {
                    ("rgb" | "rgba", 0..=2) => 1.01,
                    ("rgb" | "rgba", _) => 0.006,
                    ("oklab" | "oklch", _) => 0.002,
                    _ => 0.001,
                };
                if let Some(top) = stack.last_mut() {
                    top.1 += 1;
                }
                found.push((value, tolerance));
                skeleton.push('#');
                word.clear();
                continue;
            }
            match c {
                '(' => {
                    stack.push((std::mem::take(&mut word), 0));
                    skeleton.push(c);
                }
                ')' => {
                    stack.pop();
                    word.clear();
                    skeleton.push(c);
                }
                c if c.is_ascii_alphabetic() || c == '-' => {
                    word.push(c);
                    skeleton.push(c);
                }
                c => {
                    word.clear();
                    skeleton.push(c);
                }
            }
            index += 1;
        }
        (skeleton, found)
    }

    /// `Ok` when the two texts have the same shape and numbers that agree to what a place allows.
    fn same_as_chrome(got: &str, want: &str) -> Result<(), String> {
        let (got_shape, got_numbers) = numbers(got);
        let (want_shape, want_numbers) = numbers(want);
        if got_shape != want_shape {
            return Err(format!("got {got}, Chrome {want}"));
        }
        for ((got_value, _), (want_value, tolerance)) in got_numbers.iter().zip(&want_numbers) {
            if (got_value - want_value).abs() > *tolerance {
                return Err(format!("got {got}, Chrome {want}"));
            }
        }
        Ok(())
    }

    #[test]
    fn the_checker_of_the_oracle_tells_a_wrong_number_from_a_rounding() {
        assert!(same_as_chrome("blur(2.0001px) hue-rotate(-45deg)", "blur(2px) hue-rotate(-45deg)").is_ok());
        assert!(same_as_chrome("drop-shadow(rgba(239, 0, 17, 0.8) 2px 2px 4px)", "drop-shadow(rgba(239, 0, 16, 0.8) 2px 2px 4px)").is_ok());
        assert!(same_as_chrome("blur(2.1px)", "blur(2px)").is_err());
        assert!(same_as_chrome("blur(2px)", "brightness(2)").is_err());
        assert!(same_as_chrome("drop-shadow(rgba(239, 0, 19, 0.8) 2px 2px 4px)", "drop-shadow(rgba(239, 0, 16, 0.8) 2px 2px 4px)").is_err());
    }

    /// Every sample Chrome gives of the interpolation of `filter` (`filter-interp-oracle.html`): a CSS transition of
    /// two lists, and the keyframes of an animation, sampled the way a CSS animation does, interval by interval.
    #[test]
    fn the_interpolation_follows_chrome() {
        let oracle: serde_json::Value = serde_json::from_str(ORACLE).unwrap();
        let cases = oracle["cases"].as_object().unwrap();
        assert!(cases.len() >= 90, "{} cases", cases.len());
        let (mut checked, mut wrong) = (0, Vec::new());
        for (name, case) in cases {
            assert!(case.get("error").is_none(), "{name}: {case}");
            let style = style_of(case["style"].as_str().unwrap());
            let colors = Colors::of(&crate::color::ColorContext::root(crate::color::Scheme::Light), style.as_ref());
            let parse = |text: &str| AnimProp::Filter.parse(&json!(text), &colors).unwrap();
            let texts: Vec<&str> = match case["kind"].as_str().unwrap() {
                "transition" => vec![case["from"].as_str().unwrap(), case["to"].as_str().unwrap()],
                _ => case["values"].as_array().unwrap().iter().map(|value| value.as_str().unwrap()).collect(),
            };
            let values: Vec<AnimValue> = texts.iter().map(|text| parse(text)).collect();
            let pairs: Vec<Pair> = values
                .windows(2)
                .map(|window| pair_in("filter", &window[0], &window[1], Interpolation::Css).unwrap())
                .collect();
            let started = case["animations"].as_u64().unwrap() > 0;
            if case["kind"] == "transition" {
                // Chrome starts no transition between two lists it cannot blend, or that compute to one.
                assert_eq!(started, !pairs[0].is_discrete() && values[0] != values[1], "{name}");
            }
            if !started {
                continue;
            }
            let easing = parse_timing_function(case["easing"].as_str().unwrap()).unwrap();
            for (time, want) in case["samples"].as_object().unwrap() {
                let time: f64 = time.parse().unwrap();
                let want = want.as_str().unwrap();
                // An ended transition shows the style it ended on and not the last blend.
                if time == 1.0 && case["kind"] == "transition" {
                    continue;
                }
                let value = if pairs.len() == 1 {
                    pairs[0].at(ease(time, &easing))
                } else {
                    let count = pairs.len() as f64;
                    let interval = ((time * count).floor()).min(count - 1.0);
                    pairs[interval as usize].at(time * count - interval)
                };
                let got = text_of(AnimProp::Filter.clamp_value(value));
                checked += 1;
                if let Err(error) = same_as_chrome(&got, want) {
                    wrong.push(format!("{name} at {time}: {error}"));
                }
            }
        }
        assert!(checked > 400, "{checked} samples");
        assert_eq!(wrong, Vec::<String>::new());
    }

    /// What Chrome writes for the values of a keyframe it was given (`getKeyframes()`): the lists as they were
    /// written, not padded.
    #[test]
    fn the_keyframes_read_back_as_chrome_writes_them() {
        let oracle: serde_json::Value = serde_json::from_str(ORACLE).unwrap();
        let mut checked = 0;
        for (name, case) in oracle["cases"].as_object().unwrap() {
            if case["keyframes"].is_null() || case["kind"] != "transition" {
                continue;
            }
            let style = style_of(case["style"].as_str().unwrap());
            let colors = Colors::of(&crate::color::ColorContext::root(crate::color::Scheme::Light), style.as_ref());
            for (frame, text) in case["keyframes"].as_array().unwrap().iter().zip([&case["from"], &case["to"]]) {
                // Chrome writes a `currentColor` shadow of a transition's keyframes in black, whatever the colour is.
                if !case["style"].as_str().unwrap().is_empty() && text.as_str().unwrap().matches("rgb").count() == 0 {
                    continue;
                }
                let AnimValue::Filters(value) = AnimProp::Filter.parse(&json!(text), &colors).unwrap() else { panic!() };
                let got = computed_text(&value, Text::Transition);
                same_as_chrome(&got, frame["filter"].as_str().unwrap()).unwrap_or_else(|error| panic!("{name}: {error}"));
                checked += 1;
            }
        }
        assert!(checked > 100, "{checked} keyframes");
    }
}
