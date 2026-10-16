//! The `background` shorthand and the `background-*` longhands, assembled into
//! gpui `BackgroundLayer`s and a final colour.
//!
//! Only gradients can be painted. `url(...)` images are valid CSS, so their layer
//! is skipped and the rest still paints; `none` skips its layer too.
//!
//! `mask` and the `mask-*` longhands have the same layer grammar, so the parsing
//! here is generic over [`Kind`] and `mask.rs` supplies the differences.

use gpui::{
    BackgroundBox, BackgroundImage, BackgroundLayer, BackgroundPosition, BackgroundRepeat,
    BackgroundSize,
};

use super::gradient::{self, GradientError};
use super::length::LengthContext;
use super::position::{
    parse_box, parse_length_percentage, parse_position, parse_repeat, parse_size,
};
use super::syntax::{split_commas, split_whitespace};
use crate::color::ColorContext;

/// What differs between the `background` and `mask` layer grammars.
pub(crate) trait Kind {
    /// The `*-origin` initial value.
    const ORIGIN: BackgroundBox;
    /// Whether the shorthand ends in a colour.
    const ALLOWS_COLOR: bool;

    /// A `<box>` keyword gpui can paint with.
    fn parse_box(token: &str) -> Option<BackgroundBox>;

    /// A keyword the shorthand takes that has no effect here, lowercased.
    fn is_ignored_keyword(token: &str) -> bool;

    /// Whether the shorthand reads this token as a box, even if gpui has no
    /// equivalent for it.
    fn is_box_token(token: &str) -> bool {
        Self::parse_box(token).is_some()
    }

    /// One item of the `*-clip` list.
    fn parse_clip(item: &str) -> Option<BackgroundBox> {
        Self::parse_box(item)
    }
}

pub(crate) struct Background;

impl Kind for Background {
    const ORIGIN: BackgroundBox = BackgroundBox::PaddingBox;
    const ALLOWS_COLOR: bool = true;

    fn parse_box(token: &str) -> Option<BackgroundBox> {
        parse_box(token)
    }

    fn is_ignored_keyword(token: &str) -> bool {
        // Attachment has no meaning without a scrolling document.
        matches!(token, "scroll" | "fixed" | "local")
    }

    fn is_box_token(token: &str) -> bool {
        parse_box(token).is_some() || token.eq_ignore_ascii_case("text")
    }

    fn parse_clip(item: &str) -> Option<BackgroundBox> {
        if item.eq_ignore_ascii_case("text") {
            return Some(BackgroundBox::Text);
        }
        parse_box(item)
    }
}

/// The `background` props of one style, as written.
#[derive(Default, Clone, Copy)]
pub(crate) struct Inputs<'a> {
    pub shorthand: Option<&'a str>,
    pub image: Option<&'a str>,
    pub size: Option<&'a str>,
    pub position: Option<&'a str>,
    pub repeat: Option<&'a str>,
    pub origin: Option<&'a str>,
    pub clip: Option<&'a str>,
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Resolved {
    /// The final colour of the `background` shorthand.
    pub color: Option<gpui::Rgba>,
    /// First is on top, like CSS.
    pub layers: Vec<BackgroundLayer>,
    /// The `background-clip` of the last layer, which is also the colour's. A
    /// layer that paints nothing (`none`, `url()`) still counts.
    pub color_clip: BackgroundBox,
}

/// One entry of the layer lists. `image` is `None` for `none`, a `url()` and a
/// gradient gpui cannot paint: the slot keeps its index so the other lists line
/// up with the right layer.
#[derive(Clone)]
pub(crate) struct Slot {
    image: Option<BackgroundImage>,
    size: BackgroundSize,
    position: BackgroundPosition,
    repeat: BackgroundRepeat,
    origin: BackgroundBox,
    clip: BackgroundBox,
}

impl Slot {
    fn new(image: Option<BackgroundImage>, origin: BackgroundBox) -> Self {
        let defaults = BackgroundLayer::new(BackgroundImage::Gradient(placeholder()));
        Self {
            image,
            size: defaults.size,
            position: defaults.position,
            repeat: defaults.repeat,
            origin,
            clip: defaults.clip,
        }
    }

    fn into_layer(self) -> Option<BackgroundLayer> {
        Some(BackgroundLayer {
            image: self.image?,
            size: self.size,
            position: self.position,
            repeat: self.repeat,
            origin: self.origin,
            clip: self.clip,
        })
    }
}

fn placeholder() -> gpui::Gradient {
    gpui::Gradient {
        shape: gpui::GradientShape::Linear(gpui::LinearDirection::ToBottom),
        items: Vec::new(),
        repeating: false,
        color_space: gpui::ColorSpace::Srgb,
    }
}

/// The longhands of one layer grammar, as written.
#[derive(Default, Clone, Copy)]
pub(crate) struct Longhands<'a> {
    pub image: Option<&'a str>,
    pub size: Option<&'a str>,
    pub position: Option<&'a str>,
    pub repeat: Option<&'a str>,
    pub origin: Option<&'a str>,
    pub clip: Option<&'a str>,
}

/// Combines the shorthand and longhands: the shorthand first, then each longhand
/// replaces its part. A longhand that is invalid is ignored on its own.
pub(crate) fn resolve(inputs: &Inputs, colors: &ColorContext, lengths: &LengthContext) -> Resolved {
    let mut color = None;
    let mut slots: Vec<Slot> = Vec::new();
    if let Some((shorthand_slots, shorthand_color)) = inputs
        .shorthand
        .and_then(|value| parse_shorthand::<Background>(value, colors, lengths))
    {
        slots = shorthand_slots;
        color = shorthand_color;
    }
    let longhands = Longhands {
        image: inputs.image,
        size: inputs.size,
        position: inputs.position,
        repeat: inputs.repeat,
        origin: inputs.origin,
        clip: inputs.clip,
    };
    if slots.is_empty() && longhands.image.is_none() {
        // CSS has one image-less layer, which `background-clip` still applies to.
        slots.push(Slot::new(None, Background::ORIGIN));
    }
    let slots = apply_longhands_to_slots::<Background>(slots, &longhands, colors, lengths);
    let color_clip = slots.last().map_or(BackgroundBox::BorderBox, |slot| slot.clip);
    Resolved { color, layers: into_layers(slots), color_clip }
}

/// A layer that paints one colour.
pub(crate) fn solid_layer(color: gpui::Hsla, clip: BackgroundBox) -> BackgroundLayer {
    let stop = gpui::GradientItem::Stop(gpui::GradientStop { color, position: None });
    let mut placeholder = placeholder();
    placeholder.items = vec![stop, stop];
    BackgroundLayer { clip, ..BackgroundLayer::new(BackgroundImage::Gradient(placeholder)) }
}

/// Replaces the parts of the shorthand's slots with the longhands, and drops the
/// slots that have nothing to paint.
pub(crate) fn apply_longhands<K: Kind>(
    slots: Vec<Slot>,
    longhands: &Longhands,
    colors: &ColorContext,
    lengths: &LengthContext,
) -> Vec<BackgroundLayer> {
    into_layers(apply_longhands_to_slots::<K>(slots, longhands, colors, lengths))
}

fn into_layers(slots: Vec<Slot>) -> Vec<BackgroundLayer> {
    slots.into_iter().filter_map(Slot::into_layer).collect()
}

fn apply_longhands_to_slots<K: Kind>(
    mut slots: Vec<Slot>,
    longhands: &Longhands,
    colors: &ColorContext,
    lengths: &LengthContext,
) -> Vec<Slot> {
    if let Some(images) = longhands.image.and_then(|value| parse_images(value, colors, lengths)) {
        // The image list sets how many layers there are; the shorthand's other
        // lists repeat to cover them, as CSS does for any list shorter than it.
        let shorthand = std::mem::take(&mut slots);
        slots = images
            .into_iter()
            .enumerate()
            .map(|(index, image)| {
                let mut slot = if shorthand.is_empty() {
                    Slot::new(None, K::ORIGIN)
                } else {
                    shorthand[index % shorthand.len()].clone()
                };
                slot.image = image;
                slot
            })
            .collect();
    }

    for (property, value) in [
        (Property::Size, longhands.size),
        (Property::Position, longhands.position),
        (Property::Repeat, longhands.repeat),
        (Property::Origin, longhands.origin),
        (Property::Clip, longhands.clip),
    ] {
        apply_list::<K>(&mut slots, property, value, lengths);
    }

    slots
}

/// The layer longhands that take a comma list of one item per layer.
#[derive(Clone, Copy)]
pub(crate) enum Property {
    Size,
    Position,
    Repeat,
    Origin,
    Clip,
}

impl Property {
    fn set<K: Kind>(self, slot: &mut Slot, text: &str, lengths: &LengthContext) -> Option<()> {
        match self {
            Self::Size => slot.size = parse_size(&split_whitespace(text), lengths)?,
            Self::Position => slot.position = parse_position(&split_whitespace(text), lengths)?,
            Self::Repeat => slot.repeat = parse_repeat(&split_whitespace(text))?,
            Self::Origin => slot.origin = K::parse_box(text)?,
            Self::Clip => slot.clip = K::parse_clip(text)?,
        }
        Some(())
    }
}

/// Whether every item of the longhand's list is valid, which is when CSS keeps
/// the declaration at all.
pub(crate) fn is_valid_list<K: Kind>(property: Property, value: &str, lengths: &LengthContext) -> bool {
    let mut probe = Slot::new(None, BackgroundBox::default());
    split_commas(value)
        .into_iter()
        .all(|item| property.set::<K>(&mut probe, item, lengths).is_some())
}

/// Applies a comma list to the slots, repeating it to cover them as CSS does. A
/// list with any invalid item is ignored whole.
fn apply_list<K: Kind>(slots: &mut [Slot], property: Property, value: Option<&str>, lengths: &LengthContext) {
    let Some(value) = value.filter(|value| is_valid_list::<K>(property, value, lengths)) else {
        return;
    };
    let items = split_commas(value);
    for (index, slot) in slots.iter_mut().enumerate() {
        // Cannot fail: every item is valid.
        let _ = property.set::<K>(slot, items[index % items.len()], lengths);
    }
}

/// Whether the shorthand parses, colours included.
pub(crate) fn is_valid_shorthand<K: Kind>(value: &str, colors: &ColorContext, lengths: &LengthContext) -> bool {
    parse_shorthand::<K>(value, colors, lengths).is_some()
}

/// Whether the `*-image` list parses.
pub(crate) fn is_valid_image_list(value: &str, colors: &ColorContext, lengths: &LengthContext) -> bool {
    parse_images(value, colors, lengths).is_some()
}

/// `background-image`: `None` for an invalid list, else one entry per layer.
pub(crate) fn parse_images(
    value: &str,
    colors: &ColorContext,
    lengths: &LengthContext,
) -> Option<Vec<Option<BackgroundImage>>> {
    split_commas(value)
        .into_iter()
        .map(|item| parse_image(item, colors, lengths))
        .collect()
}

/// `Some(None)` is a valid image gpui cannot paint.
fn parse_image(item: &str, colors: &ColorContext, lengths: &LengthContext) -> Option<Option<BackgroundImage>> {
    let lower = item.trim().to_ascii_lowercase();
    if lower == "none" || lower.starts_with("url(") || lower.starts_with("image-set(") {
        return Some(None);
    }
    match gradient::parse(item, colors, lengths) {
        Ok(parsed) => Some(Some(BackgroundImage::Gradient(parsed))),
        Err(GradientError::Unsupported) => Some(None),
        Err(GradientError::Invalid) => None,
    }
}

/// Splits on whitespace, and on `/` where it stands alone or between values.
fn shorthand_tokens(layer: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    for token in split_whitespace(layer) {
        if token.contains('/') && !token.contains('(') {
            let mut rest = token;
            while let Some(index) = rest.find('/') {
                if index > 0 {
                    tokens.push(&rest[..index]);
                }
                tokens.push("/");
                rest = &rest[index + 1..];
            }
            if !rest.is_empty() {
                tokens.push(rest);
            }
        } else {
            tokens.push(token);
        }
    }
    tokens
}

fn is_position_token(token: &str, lengths: &LengthContext) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "left" | "right" | "top" | "bottom" | "center"
    ) || parse_length_percentage(token, lengths).is_some()
}

fn is_size_token(token: &str, lengths: &LengthContext) -> bool {
    matches!(token.to_ascii_lowercase().as_str(), "auto" | "cover" | "contain")
        || parse_length_percentage(token, lengths).is_some()
}

fn is_repeat_token(token: &str) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "repeat" | "repeat-x" | "repeat-y" | "space" | "round" | "no-repeat"
    )
}

/// The layers of a shorthand and, for a grammar that has one, its final colour.
pub(crate) fn parse_shorthand<K: Kind>(
    value: &str,
    colors: &ColorContext,
    lengths: &LengthContext,
) -> Option<(Vec<Slot>, Option<gpui::Rgba>)> {
    let layers = split_commas(value);
    let last = layers.len() - 1;
    let mut slots = Vec::new();
    let mut color = None;
    for (index, layer) in layers.into_iter().enumerate() {
        let (slot, layer_color) = parse_shorthand_layer::<K>(layer, colors, lengths, index == last)?;
        slots.push(slot);
        color = layer_color;
    }
    Some((slots, color))
}

fn parse_shorthand_layer<K: Kind>(
    layer: &str,
    colors: &ColorContext,
    lengths: &LengthContext,
    is_last: bool,
) -> Option<(Slot, Option<gpui::Rgba>)> {
    let mut image: Option<Option<BackgroundImage>> = None;
    let mut position: Vec<&str> = Vec::new();
    let mut size: Vec<&str> = Vec::new();
    let mut in_size = false;
    let mut repeat: Vec<&str> = Vec::new();
    let mut boxes: Vec<&str> = Vec::new();
    let mut color = None;

    let tokens = shorthand_tokens(layer);
    // `a,,b` and a trailing comma leave a layer with nothing in it.
    if tokens.is_empty() {
        return None;
    }
    for token in tokens {
        if token == "/" {
            if position.is_empty() || in_size || !size.is_empty() {
                return None;
            }
            in_size = true;
            continue;
        }
        if in_size && is_size_token(token, lengths) {
            size.push(token);
            continue;
        }
        in_size = false;
        let lower = token.to_ascii_lowercase();
        if gradient::is_gradient(token) || lower == "none" || lower.starts_with("url(") {
            if image.replace(parse_image(token, colors, lengths)?).is_some() {
                return None;
            }
        } else if is_repeat_token(token) {
            repeat.push(token);
        } else if K::is_ignored_keyword(&lower) {
            // Valid in the grammar, but nothing to set.
        } else if K::is_box_token(token) {
            boxes.push(token);
        } else if is_position_token(token, lengths) {
            position.push(token);
        } else if K::ALLOWS_COLOR && is_last && color.is_none() {
            color = Some(colors.resolve(token)?);
        } else {
            return None;
        }
    }
    if in_size && size.is_empty() {
        return None;
    }

    let mut slot = Slot::new(image.flatten(), K::ORIGIN);
    if !position.is_empty() {
        slot.position = parse_position(&position, lengths)?;
    }
    if !size.is_empty() {
        slot.size = parse_size(&size, lengths)?;
    }
    if !repeat.is_empty() {
        slot.repeat = parse_repeat(&repeat)?;
    }
    match boxes.as_slice() {
        [] => {}
        [one] => {
            if let Some(parsed) = K::parse_box(one) {
                slot.origin = parsed;
                slot.clip = parsed;
            } else if let Some(clip) = K::parse_clip(one) {
                // `text` is only a clip: the origin keeps its initial value.
                slot.clip = clip;
            }
        }
        [origin, clip] => {
            slot.origin = K::parse_box(origin)?;
            if let Some(parsed) = K::parse_clip(clip) {
                slot.clip = parsed;
            }
        }
        _ => return None,
    }
    Some((slot, color))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Scheme;
    use gpui::{px, LengthPercentage, RepeatStyle};

    fn context() -> ColorContext {
        ColorContext::root(Scheme::Light)
    }

    fn shorthand(value: &str) -> Resolved {
        resolve(&Inputs { shorthand: Some(value), ..Default::default() }, &context(), &LengthContext::for_tests())
    }

    fn red() -> gpui::Rgba {
        gpui::Rgba { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
    }

    #[test]
    fn a_plain_colour_is_just_the_colour() {
        let resolved = shorthand("red");
        assert_eq!(resolved.color, Some(red()));
        assert!(resolved.layers.is_empty());
    }

    #[test]
    fn layers_then_a_final_colour() {
        let resolved = shorthand(
            "linear-gradient(red, blue) no-repeat center / 50% 20px padding-box content-box, radial-gradient(red, blue), #fff",
        );
        assert_eq!(resolved.layers.len(), 2);
        assert_eq!(resolved.color.unwrap().r, 1.0);
        let first = &resolved.layers[0];
        assert_eq!(first.position, BackgroundPosition::CENTER);
        assert_eq!(
            first.size,
            BackgroundSize::Explicit {
                width: Some(LengthPercentage::Percent(0.5)),
                height: Some(LengthPercentage::Length(px(20.0)))
            }
        );
        assert_eq!(first.repeat, BackgroundRepeat { x: RepeatStyle::NoRepeat, y: RepeatStyle::NoRepeat });
        assert_eq!((first.origin, first.clip), (BackgroundBox::PaddingBox, BackgroundBox::ContentBox));
        // The second layer keeps the CSS initial values.
        assert_eq!(resolved.layers[1].position, BackgroundPosition::ORIGIN);
        assert_eq!(resolved.layers[1].clip, BackgroundBox::BorderBox);
    }

    #[test]
    fn slash_may_touch_its_neighbours() {
        let resolved = shorthand("linear-gradient(red, blue) left top/cover no-repeat");
        assert_eq!(resolved.layers[0].size, BackgroundSize::Cover);
        assert_eq!(resolved.layers[0].position, BackgroundPosition::ORIGIN);
    }

    #[test]
    fn one_box_sets_origin_and_clip_and_text_only_clips() {
        let resolved = shorthand("linear-gradient(red, blue) content-box");
        assert_eq!(
            (resolved.layers[0].origin, resolved.layers[0].clip),
            (BackgroundBox::ContentBox, BackgroundBox::ContentBox)
        );
        let text = shorthand("linear-gradient(red, blue) padding-box text");
        assert_eq!(
            (text.layers[0].origin, text.layers[0].clip),
            (BackgroundBox::PaddingBox, BackgroundBox::Text)
        );
        let alone = shorthand("linear-gradient(red, blue) TEXT");
        assert_eq!(
            (alone.layers[0].origin, alone.layers[0].clip),
            (BackgroundBox::PaddingBox, BackgroundBox::Text)
        );
        let boxed = shorthand("linear-gradient(red, blue) content-box text");
        assert_eq!(
            (boxed.layers[0].origin, boxed.layers[0].clip),
            (BackgroundBox::ContentBox, BackgroundBox::Text)
        );
    }

    #[test]
    fn the_colour_follows_the_last_layers_clip() {
        let clip_only = resolve(&Inputs { clip: Some("text"), ..Default::default() }, &context(), &LengthContext::for_tests());
        assert!(clip_only.layers.is_empty());
        assert_eq!(clip_only.color_clip, BackgroundBox::Text);

        let colour_only = shorthand("red text");
        assert!(colour_only.layers.is_empty());
        assert_eq!(colour_only.color_clip, BackgroundBox::Text);

        let first_only = shorthand("linear-gradient(red, blue) text, linear-gradient(blue, red)");
        assert_eq!(first_only.layers[0].clip, BackgroundBox::Text);
        assert_eq!(first_only.color_clip, BackgroundBox::BorderBox);

        let skipped_last = shorthand("linear-gradient(red, blue), none text");
        assert_eq!(skipped_last.layers.len(), 1);
        assert_eq!(skipped_last.color_clip, BackgroundBox::Text);

        assert_eq!(shorthand("red").color_clip, BackgroundBox::BorderBox);
    }

    #[test]
    fn a_text_clipped_colour_becomes_the_bottom_layer() {
        let layer = solid_layer(gpui::red(), BackgroundBox::Text);
        let BackgroundImage::Gradient(gradient) = &layer.image else { panic!("an image layer") };
        assert_eq!(gradient.items.len(), 2);
        assert_eq!(layer.clip, BackgroundBox::Text);
    }

    #[test]
    fn url_and_none_layers_are_skipped_without_shifting_the_others() {
        let resolved = resolve(
            &Inputs {
                image: Some("url(a.png), linear-gradient(red, blue), none"),
                position: Some("left, right, center"),
                ..Default::default()
            },
            &context(),
            &LengthContext::for_tests(),
        );
        assert_eq!(resolved.layers.len(), 1);
        // The gradient is the second layer, so it gets the second position.
        assert_eq!(
            resolved.layers[0].position.x,
            gpui::BackgroundPositionAxis::from_start(LengthPercentage::Percent(1.0))
        );
    }

    #[test]
    fn longhand_lists_repeat_to_cover_the_layers() {
        let resolved = resolve(
            &Inputs {
                image: Some("linear-gradient(red, blue), linear-gradient(blue, red), linear-gradient(red, red)"),
                size: Some("10px, 20px"),
                repeat: Some("no-repeat"),
                origin: Some("content-box"),
                clip: Some("padding-box, text"),
                ..Default::default()
            },
            &context(),
            &LengthContext::for_tests(),
        );
        let widths: Vec<_> = resolved
            .layers
            .iter()
            .map(|layer| match layer.size {
                BackgroundSize::Explicit { width: Some(LengthPercentage::Length(width)), .. } => width,
                _ => panic!(),
            })
            .collect();
        assert_eq!(widths, vec![px(10.0), px(20.0), px(10.0)]);
        assert!(resolved.layers.iter().all(|layer| layer.repeat.x == RepeatStyle::NoRepeat));
        assert!(resolved.layers.iter().all(|layer| layer.origin == BackgroundBox::ContentBox));
        assert_eq!(resolved.layers[0].clip, BackgroundBox::PaddingBox);
        assert_eq!(resolved.layers[1].clip, BackgroundBox::Text);
        assert_eq!(resolved.layers[2].clip, BackgroundBox::PaddingBox);
    }

    #[test]
    fn longhands_override_the_shorthand() {
        let resolved = resolve(
            &Inputs {
                shorthand: Some("linear-gradient(red, blue) no-repeat, blue"),
                repeat: Some("repeat-x"),
                image: Some("radial-gradient(red, blue)"),
                ..Default::default()
            },
            &context(),
            &LengthContext::for_tests(),
        );
        assert_eq!(resolved.layers.len(), 1);
        let BackgroundImage::Gradient(gradient) = &resolved.layers[0].image else { panic!("an image layer") };
        assert!(matches!(gradient.shape, gpui::GradientShape::Radial { .. }));
        assert_eq!(
            resolved.layers[0].repeat,
            BackgroundRepeat { x: RepeatStyle::Repeat, y: RepeatStyle::NoRepeat }
        );
        assert_eq!(resolved.color.unwrap().b, 1.0);
    }

    #[test]
    fn an_invalid_value_is_ignored_on_its_own() {
        // The bad size list is dropped; the image list still paints.
        let resolved = resolve(
            &Inputs { image: Some("linear-gradient(red, blue)"), size: Some("10px, nope"), ..Default::default() },
            &context(),
            &LengthContext::for_tests(),
        );
        assert_eq!(resolved.layers.len(), 1);
        assert_eq!(resolved.layers[0].size, BackgroundSize::Auto);
        assert_eq!(
            resolve(&Inputs { image: Some("linear-gradient(red)"), ..Default::default() }, &context(), &LengthContext::for_tests()),
            Resolved::default()
        );
        for bad in [
            "not-a-colour",
            "linear-gradient(red, blue) red blue",
            "10px / 20px",
            "linear-gradient(red) red",
            "linear-gradient(red, blue) / cover",
        ] {
            assert_eq!(shorthand(bad), Resolved::default(), "{bad}");
        }
    }

    #[test]
    fn unsupported_interpolation_skips_only_that_layer() {
        let resolved = resolve(
            &Inputs {
                image: Some("linear-gradient(in oklch, red, blue), linear-gradient(red, blue)"),
                ..Default::default()
            },
            &context(),
            &LengthContext::for_tests(),
        );
        assert_eq!(resolved.layers.len(), 1);
    }

    #[test]
    fn oklab_gradients_keep_their_space() {
        let resolved = shorthand("linear-gradient(in oklab, red, blue)");
        let BackgroundImage::Gradient(gradient) = &resolved.layers[0].image else { panic!("an image layer") };
        assert_eq!(gradient.color_space, gpui::ColorSpace::Oklab);
    }

    #[test]
    fn an_empty_layer_makes_the_shorthand_invalid() {
        for bad in ["", "   ", "linear-gradient(red, blue),", ",linear-gradient(red, blue)", "red, , blue"] {
            assert_eq!(shorthand(bad), Resolved::default(), "{bad:?}");
        }
        // `none` and a lone position are real layers.
        assert_eq!(shorthand("none, center").layers.len(), 0);
        assert_eq!(shorthand("linear-gradient(red, blue), none").layers.len(), 1);
    }

    #[test]
    fn a_negative_background_size_makes_the_list_invalid() {
        let resolved = resolve(
            &Inputs {
                image: Some("linear-gradient(red, blue)"),
                size: Some("-10px"),
                ..Default::default()
            },
            &context(),
            &LengthContext::for_tests(),
        );
        assert_eq!(resolved.layers[0].size, BackgroundSize::Auto);
        assert_eq!(shorthand("linear-gradient(red, blue) 0 0 / -1px"), Resolved::default());
        assert_eq!(shorthand("linear-gradient(red, blue) 0 0 / 10px -5%"), Resolved::default());
        // Only the size is non-negative: a negative position is fine.
        assert_eq!(shorthand("linear-gradient(red, blue) -10px -5px").layers.len(), 1);
    }

    #[test]
    fn an_image_longhand_with_more_layers_repeats_the_shorthands_lists() {
        let resolved = resolve(
            &Inputs {
                shorthand: Some(
                    "linear-gradient(red, red) no-repeat 5px 5px / 20px, linear-gradient(blue, blue) repeat-x content-box",
                ),
                image: Some(
                    "linear-gradient(red, blue), linear-gradient(blue, red), linear-gradient(red, red), linear-gradient(blue, blue)",
                ),
                ..Default::default()
            },
            &context(),
            &LengthContext::for_tests(),
        );
        assert_eq!(resolved.layers.len(), 4);
        let no_repeat = BackgroundRepeat { x: RepeatStyle::NoRepeat, y: RepeatStyle::NoRepeat };
        let repeat_x = BackgroundRepeat { x: RepeatStyle::Repeat, y: RepeatStyle::NoRepeat };
        let repeats: Vec<_> = resolved.layers.iter().map(|layer| layer.repeat).collect();
        assert_eq!(repeats, vec![no_repeat, repeat_x, no_repeat, repeat_x]);
        let sizes: Vec<_> = resolved.layers.iter().map(|layer| layer.size).collect();
        assert_eq!(sizes[2], sizes[0]);
        assert_ne!(sizes[2], BackgroundSize::Auto);
        assert_eq!(resolved.layers[3].origin, BackgroundBox::ContentBox);
    }

    #[test]
    fn lengths_resolve_against_the_element_in_every_part_of_a_layer() {
        let resolved = shorthand(
            "linear-gradient(red calc(1em + 10%), blue 2rem) 1em calc(50% - 1rem) / calc(100% - 1em) 10vw no-repeat",
        );
        let layer = &resolved.layers[0];
        assert_eq!(layer.position.x.offset, LengthPercentage::Length(px(20.0)));
        assert_eq!(layer.position.y.offset, LengthPercentage::Calc { length: px(-10.0), percent: 0.5 });
        assert_eq!(
            layer.size,
            BackgroundSize::Explicit {
                width: Some(LengthPercentage::Calc { length: px(-20.0), percent: 1.0 }),
                height: Some(LengthPercentage::Length(px(80.0))),
            }
        );
        let BackgroundImage::Gradient(gradient) = &layer.image else { panic!("an image layer") };
        let gpui::GradientItem::Stop(stop) = gradient.items[0] else { panic!("a hint") };
        assert_eq!(stop.position, Some(gpui::StopPosition::Calc { length: px(20.0), fraction: 0.1 }));

        let lengths = LengthContext::for_tests().descend(Some(&crate::css::length::LinearLength::px(8.0)), false);
        let smaller = resolve(
            &Inputs { image: Some("linear-gradient(red 1em, blue)"), size: Some("1em"), ..Default::default() },
            &context(),
            &lengths,
        );
        assert_eq!(
            smaller.layers[0].size,
            BackgroundSize::Explicit { width: Some(LengthPercentage::Length(px(8.0))), height: None }
        );
    }

    #[test]
    fn a_unit_without_a_meaning_here_invalidates_the_value() {
        assert_eq!(shorthand("linear-gradient(red, blue) 1ch 2ch"), Resolved::default());
        let resolved = resolve(
            &Inputs { image: Some("linear-gradient(red, blue)"), size: Some("-1em"), ..Default::default() },
            &context(),
            &LengthContext::for_tests(),
        );
        assert_eq!(resolved.layers[0].size, BackgroundSize::Auto);
    }
}
