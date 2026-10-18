//! The `mask-border` shorthand and the `mask-border-*` longhands, assembled into a gpui
//! `MaskBorder`.
//!
//! The source is one gradient or `url()` image, read as `mask-image` reads its layers. Chrome only
//! knows the prefixed `-webkit-mask-box-image` forms of these, and the initial values below are
//! the ones of the CSS Masking spec: `mask-border-slice` is `0` and does not imply `fill`, and
//! `mask-border-mode` is `alpha`.

use gpui::{
    BackgroundImage, BorderImageOutset, BorderImageRepeat, BorderImageSlice, BorderImageWidth,
    MaskBorder, MaskMode, px,
};

use super::background::{parse_source_image, shorthand_tokens};
use super::dimension;
use super::length::LengthContext;
use super::position::parse_non_negative;
use super::syntax::{expand_sides, split_commas, split_whitespace};
use crate::color::ColorContext;

/// The `mask-border` props of one style, as written.
#[derive(Default, Clone, Copy)]
pub(crate) struct Inputs<'a> {
    pub shorthand: Option<&'a str>,
    pub source: Option<&'a str>,
    pub slice: Option<&'a str>,
    pub width: Option<&'a str>,
    pub outset: Option<&'a str>,
    pub repeat: Option<&'a str>,
    pub mode: Option<&'a str>,
}

/// An image the border is drawn from, and for a `url()` its address.
#[derive(Clone)]
struct Source {
    image: BackgroundImage,
    url: Option<String>,
}

#[derive(Clone)]
struct Slice {
    sides: [BorderImageSlice; 4],
    fill: bool,
}

/// What the properties give, before there is a source to make a border of.
#[derive(Clone)]
struct Parts {
    source: Option<Source>,
    slice: Slice,
    width: [BorderImageWidth; 4],
    outset: [BorderImageOutset; 4],
    repeat: (BorderImageRepeat, BorderImageRepeat),
    mode: MaskMode,
}

impl Default for Parts {
    fn default() -> Self {
        Self {
            source: None,
            slice: Slice { sides: [BorderImageSlice::Number(0.); 4], fill: false },
            width: [BorderImageWidth::Auto; 4],
            outset: [BorderImageOutset::Number(0.); 4],
            repeat: (BorderImageRepeat::Stretch, BorderImageRepeat::Stretch),
            mode: MaskMode::Alpha,
        }
    }
}

/// The mask border of the style, or `None` when it has none: no source, `none`, or only invalid
/// values. A longhand that is valid replaces its part of the shorthand, and one that is not is
/// ignored.
pub(crate) fn resolve(inputs: &Inputs, colors: &ColorContext, lengths: &LengthContext) -> Option<MaskBorder> {
    let mut parts = inputs
        .shorthand
        .and_then(|value| parse_shorthand(value, colors, lengths))
        .unwrap_or_default();
    if let Some(source) = inputs.source.and_then(|value| parse_source(value, colors, lengths)) {
        parts.source = source;
    }
    if let Some(slice) = inputs.slice.and_then(parse_slice) {
        parts.slice = slice;
    }
    if let Some(width) = inputs.width.and_then(|value| parse_width(value, lengths)) {
        parts.width = width;
    }
    if let Some(outset) = inputs.outset.and_then(|value| parse_outset(value, lengths)) {
        parts.outset = outset;
    }
    if let Some(repeat) = inputs.repeat.and_then(parse_repeat) {
        parts.repeat = repeat;
    }
    if let Some(mode) = inputs.mode.and_then(parse_mode) {
        parts.mode = mode;
    }

    let source = parts.source?;
    if let (BackgroundImage::Image(key), Some(url)) = (&source.image, &source.url) {
        crate::background_images::request(*key, url);
    }
    let mut border = MaskBorder::new(source.image);
    border.slice = parts.slice.sides;
    border.fill = parts.slice.fill;
    border.width = parts.width;
    border.outset = parts.outset;
    (border.repeat_x, border.repeat_y) = parts.repeat;
    border.mode = parts.mode;
    Some(border)
}

pub(crate) fn is_valid_shorthand(value: &str, colors: &ColorContext, lengths: &LengthContext) -> bool {
    parse_shorthand(value, colors, lengths).is_some()
}

pub(crate) fn is_valid_source(value: &str, colors: &ColorContext, lengths: &LengthContext) -> bool {
    parse_source(value, colors, lengths).is_some()
}

pub(crate) fn is_valid_slice(value: &str) -> bool {
    parse_slice(value).is_some()
}

pub(crate) fn is_valid_width(value: &str, lengths: &LengthContext) -> bool {
    parse_width(value, lengths).is_some()
}

pub(crate) fn is_valid_outset(value: &str, lengths: &LengthContext) -> bool {
    parse_outset(value, lengths).is_some()
}

pub(crate) fn is_valid_repeat(value: &str) -> bool {
    parse_repeat(value).is_some()
}

pub(crate) fn is_valid_mode(value: &str) -> bool {
    parse_mode(value).is_some()
}

/// `Some(None)` is a valid `none`, or an image gpui cannot paint.
fn parse_source(value: &str, colors: &ColorContext, lengths: &LengthContext) -> Option<Option<Source>> {
    let items = split_commas(value);
    let [item] = items.as_slice() else {
        return None;
    };
    Some(parse_source_image(item, colors, lengths)?.map(|(image, url)| Source { image, url }))
}

fn parse_number_or_percentage(token: &str) -> Option<BorderImageSlice> {
    if token.trim_end().ends_with('%') {
        let fraction = dimension::parse_number_or_percentage(token)?;
        return (fraction >= 0.).then_some(BorderImageSlice::Percent(fraction as f32));
    }
    let number = dimension::parse_number(token)?;
    (number >= 0.).then_some(BorderImageSlice::Number(number as f32))
}

/// One to four numbers or percentages and `fill`, which comes before or after them.
fn parse_slice(value: &str) -> Option<Slice> {
    let mut tokens = split_whitespace(value);
    let mut fill = false;
    if tokens.first().is_some_and(|token| token.eq_ignore_ascii_case("fill")) {
        fill = true;
        tokens.remove(0);
    } else if tokens.last().is_some_and(|token| token.eq_ignore_ascii_case("fill")) {
        fill = true;
        tokens.pop();
    }
    let sides = tokens.iter().map(|token| parse_number_or_percentage(token)).collect::<Option<Vec<_>>>()?;
    Some(Slice { sides: expand_sides(&sides)?, fill })
}

fn parse_width(value: &str, lengths: &LengthContext) -> Option<[BorderImageWidth; 4]> {
    let sides = split_whitespace(value)
        .into_iter()
        .map(|token| {
            if token.eq_ignore_ascii_case("auto") {
                return Some(BorderImageWidth::Auto);
            }
            if let Some(number) = dimension::parse_number(token) {
                return (number >= 0.).then_some(BorderImageWidth::Number(number as f32));
            }
            parse_non_negative(token, lengths).map(BorderImageWidth::Length)
        })
        .collect::<Option<Vec<_>>>()?;
    expand_sides(&sides)
}

fn parse_outset(value: &str, lengths: &LengthContext) -> Option<[BorderImageOutset; 4]> {
    let sides = split_whitespace(value)
        .into_iter()
        .map(|token| {
            if let Some(number) = dimension::parse_number(token) {
                return (number >= 0.).then_some(BorderImageOutset::Number(number as f32));
            }
            let length = dimension::parse_non_negative_length(token, lengths)?;
            Some(BorderImageOutset::Length(px(length as f32)))
        })
        .collect::<Option<Vec<_>>>()?;
    expand_sides(&sides)
}

fn parse_repeat_keyword(token: &str) -> Option<BorderImageRepeat> {
    match token.to_ascii_lowercase().as_str() {
        "stretch" => Some(BorderImageRepeat::Stretch),
        "repeat" => Some(BorderImageRepeat::Repeat),
        "round" => Some(BorderImageRepeat::Round),
        "space" => Some(BorderImageRepeat::Space),
        _ => None,
    }
}

fn parse_repeat(value: &str) -> Option<(BorderImageRepeat, BorderImageRepeat)> {
    match split_whitespace(value).as_slice() {
        [both] => parse_repeat_keyword(both).map(|style| (style, style)),
        [x, y] => Some((parse_repeat_keyword(x)?, parse_repeat_keyword(y)?)),
        _ => None,
    }
}

fn parse_mode(value: &str) -> Option<MaskMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "alpha" => Some(MaskMode::Alpha),
        "luminance" => Some(MaskMode::Luminance),
        _ => None,
    }
}

fn is_slice_token(token: &str) -> bool {
    token.eq_ignore_ascii_case("fill") || parse_number_or_percentage(token).is_some()
}

fn is_width_token(token: &str, lengths: &LengthContext) -> bool {
    token.eq_ignore_ascii_case("auto")
        || dimension::parse_number(token).is_some()
        || parse_non_negative(token, lengths).is_some()
}

/// `<source> || <slice> [ / <width>? [ / <outset> ]? ]? || <repeat> || <mode>`: the parts in any
/// order, each at most once, and every part the value leaves out at its initial value.
fn parse_shorthand(value: &str, colors: &ColorContext, lengths: &LengthContext) -> Option<Parts> {
    if split_commas(value).len() != 1 {
        return None;
    }
    let tokens = shorthand_tokens(value);
    let mut parts = Parts::default();
    let mut seen = [false; 4];
    let mut index = 0;
    let run = |index: &mut usize, accepts: &dyn Fn(&str) -> bool| {
        let start = *index;
        while *index < tokens.len() && tokens[*index] != "/" && accepts(tokens[*index]) {
            *index += 1;
        }
        tokens[start..*index].join(" ")
    };
    while index < tokens.len() {
        let token = tokens[index];
        if !seen[0] && (parse_source(token, colors, lengths).is_some()) {
            parts.source = parse_source(token, colors, lengths)?;
            seen[0] = true;
            index += 1;
        } else if !seen[1] && is_slice_token(token) {
            parts.slice = parse_slice(&run(&mut index, &is_slice_token))?;
            seen[1] = true;
            if tokens.get(index) == Some(&"/") {
                index += 1;
                let width = run(&mut index, &|token| is_width_token(token, lengths));
                if !width.is_empty() {
                    parts.width = parse_width(&width, lengths)?;
                }
                if tokens.get(index) == Some(&"/") {
                    index += 1;
                    let outset = run(&mut index, &|token| {
                        dimension::parse_number(token).is_some()
                            || dimension::parse_non_negative_length(token, lengths).is_some()
                    });
                    parts.outset = parse_outset(&outset, lengths)?;
                }
            }
        } else if !seen[2] && parse_repeat_keyword(token).is_some() {
            let keywords = run(&mut index, &|token| parse_repeat_keyword(token).is_some());
            parts.repeat = parse_repeat(&keywords)?;
            seen[2] = true;
        } else if !seen[3] && parse_mode(token).is_some() {
            parts.mode = parse_mode(token)?;
            seen[3] = true;
            index += 1;
        } else {
            return None;
        }
    }
    (index == tokens.len() && seen.iter().any(|seen| *seen)).then_some(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Scheme;
    use gpui::LengthPercentage;

    fn context() -> ColorContext {
        ColorContext::root(Scheme::Light)
    }

    fn lengths() -> LengthContext {
        LengthContext::for_tests()
    }

    fn shorthand(value: &str) -> Option<MaskBorder> {
        resolve(&Inputs { shorthand: Some(value), ..Default::default() }, &context(), &lengths())
    }

    fn gradient() -> &'static str {
        "linear-gradient(black, transparent)"
    }

    fn length(value: f32) -> BorderImageWidth {
        BorderImageWidth::Length(LengthPercentage::Length(px(value)))
    }

    #[test]
    fn a_source_alone_has_the_initial_values() {
        let border = shorthand(gradient()).unwrap();
        assert!(matches!(border.source, BackgroundImage::Gradient(_)));
        assert_eq!(border.slice, [BorderImageSlice::Number(0.); 4]);
        assert!(!border.fill);
        assert_eq!(border.width, [BorderImageWidth::Auto; 4]);
        assert_eq!(border.outset, [BorderImageOutset::Number(0.); 4]);
        assert_eq!((border.repeat_x, border.repeat_y), (BorderImageRepeat::Stretch, BorderImageRepeat::Stretch));
        assert_eq!(border.mode, MaskMode::Alpha);
    }

    #[test]
    fn the_shorthand_takes_every_part() {
        let border = shorthand("url(a.png) 10 20% fill / 4px 2 auto / 3 1px round space luminance").unwrap();
        assert_eq!(
            border.slice,
            [
                BorderImageSlice::Number(10.),
                BorderImageSlice::Percent(0.2),
                BorderImageSlice::Number(10.),
                BorderImageSlice::Percent(0.2),
            ]
        );
        assert!(border.fill);
        assert_eq!(
            border.width,
            [length(4.), BorderImageWidth::Number(2.), BorderImageWidth::Auto, BorderImageWidth::Number(2.)]
        );
        assert_eq!(
            border.outset,
            [
                BorderImageOutset::Number(3.),
                BorderImageOutset::Length(px(1.)),
                BorderImageOutset::Number(3.),
                BorderImageOutset::Length(px(1.)),
            ]
        );
        assert_eq!((border.repeat_x, border.repeat_y), (BorderImageRepeat::Round, BorderImageRepeat::Space));
        assert_eq!(border.mode, MaskMode::Luminance);
        let BackgroundImage::Image(key) = border.source else { panic!("an image") };
        assert_eq!(key, crate::background_images::key("a.png"));
    }

    #[test]
    fn the_parts_come_in_any_order_and_fill_goes_on_either_side() {
        let border = shorthand("repeat alpha fill 5 / / 2px url(a.png)").unwrap();
        assert!(border.fill);
        assert_eq!(border.slice, [BorderImageSlice::Number(5.); 4]);
        assert_eq!(border.width, [BorderImageWidth::Auto; 4]);
        assert_eq!(border.outset, [BorderImageOutset::Length(px(2.)); 4]);
        assert_eq!(border.repeat_x, BorderImageRepeat::Repeat);
        assert!(shorthand("fill 5 10 fill").is_none());
        assert!(shorthand("5 gradient fill").is_none());
        let slashes = shorthand(&format!("{} 5/10px/2px", gradient())).unwrap();
        assert_eq!(slashes.width, [length(10.); 4]);
        assert_eq!(slashes.outset, [BorderImageOutset::Length(px(2.)); 4]);
    }

    #[test]
    fn none_and_invalid_values_have_no_border() {
        assert!(shorthand("none").is_none());
        assert!(shorthand("none 10 fill").is_none());
        assert!(shorthand("").is_none());
        assert!(shorthand("bogus").is_none());
        assert!(shorthand(&format!("{} 10 / 5px 10", gradient())).is_some());
        assert!(shorthand(&format!("{} 10 {} 10", gradient(), gradient())).is_none());
        assert!(shorthand(&format!("{} 10, {} 10", gradient(), gradient())).is_none());
        assert!(shorthand(&format!("{} -1", gradient())).is_none());
        assert!(shorthand(&format!("{} 1 2 3 4 5", gradient())).is_none());
        assert!(shorthand(&format!("{} 10 / -1px", gradient())).is_none());
        assert!(shorthand(&format!("{} 10 / 1 / -1", gradient())).is_none());
        assert!(shorthand(&format!("{} stretch repeat round", gradient())).is_none());
        assert!(shorthand(&format!("{} 10 / 1 / 2 / 3", gradient())).is_none());
        assert!(shorthand(&format!("{} match-source", gradient())).is_none());
    }

    #[test]
    fn longhands_replace_the_parts_of_the_shorthand_when_valid() {
        let border = resolve(
            &Inputs {
                shorthand: Some("url(a.png) 10 / 5px round"),
                slice: Some("20 fill"),
                width: Some("bogus"),
                repeat: Some("space"),
                mode: Some("luminance"),
                ..Default::default()
            },
            &context(),
            &lengths(),
        )
        .unwrap();
        assert_eq!(border.slice, [BorderImageSlice::Number(20.); 4]);
        assert!(border.fill);
        assert_eq!(border.width, [length(5.); 4]);
        assert_eq!((border.repeat_x, border.repeat_y), (BorderImageRepeat::Space, BorderImageRepeat::Space));
        assert_eq!(border.mode, MaskMode::Luminance);
    }

    #[test]
    fn longhands_alone_make_a_border_once_they_have_a_source() {
        let slice_only = Inputs { slice: Some("10"), ..Default::default() };
        assert!(resolve(&slice_only, &context(), &lengths()).is_none());
        let with_source = Inputs { source: Some(gradient()), slice: Some("10"), ..Default::default() };
        assert!(resolve(&with_source, &context(), &lengths()).is_some());
        let none = Inputs { shorthand: Some(gradient()), source: Some("none"), ..Default::default() };
        assert!(resolve(&none, &context(), &lengths()).is_none());
    }

    #[test]
    fn longhand_grammar() {
        assert!(parse_slice("10 20 30 40 fill").is_some());
        assert!(parse_slice("fill").is_none());
        assert!(parse_slice("10px").is_none());
        assert!(parse_slice("").is_none());
        assert!(parse_width("auto 10px 5% 2", &lengths()).is_some());
        assert!(parse_width("-1", &lengths()).is_none());
        assert!(parse_width("1 2 3 4 5", &lengths()).is_none());
        assert!(parse_outset("1px 2", &lengths()).is_some());
        assert!(parse_outset("5%", &lengths()).is_none());
        assert!(parse_outset("auto", &lengths()).is_none());
        assert_eq!(parse_repeat("round"), Some((BorderImageRepeat::Round, BorderImageRepeat::Round)));
        assert!(parse_repeat("no-repeat").is_none());
        assert_eq!(parse_mode("LUMINANCE"), Some(MaskMode::Luminance));
        assert!(parse_mode("match-source").is_none());
        assert!(is_valid_source("none", &context(), &lengths()));
        assert!(!is_valid_source("a.png", &context(), &lengths()));
        assert!(!is_valid_source("url(a.png), url(b.png)", &context(), &lengths()));
    }

    #[test]
    fn a_url_source_is_requested_for_loading() {
        crate::background_images::requested_for_tests();
        shorthand("url(mask-border-source.png) 10").unwrap();
        assert_eq!(crate::background_images::requested_for_tests(), vec!["mask-border-source.png".to_owned()]);
    }
}
