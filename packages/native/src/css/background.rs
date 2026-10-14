//! The `background` shorthand and the `background-*` longhands, assembled into
//! gpui `BackgroundLayer`s and a final colour.
//!
//! Only gradients can be painted. `url(...)` images are valid CSS, so their layer
//! is skipped and the rest still paints; `none` skips its layer too.

use gpui::{
    BackgroundBox, BackgroundImage, BackgroundLayer, BackgroundPosition, BackgroundRepeat,
    BackgroundSize,
};

use super::gradient::{self, GradientError};
use super::position::{
    parse_box, parse_length_percentage, parse_position, parse_repeat, parse_size,
};
use super::syntax::{split_commas, split_whitespace};
use crate::color::ColorContext;

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
}

/// One entry of the layer lists. `image` is `None` for `none`, a `url()` and a
/// gradient gpui cannot paint: the slot keeps its index so the other lists line
/// up with the right layer.
#[derive(Clone)]
struct Slot {
    image: Option<BackgroundImage>,
    size: BackgroundSize,
    position: BackgroundPosition,
    repeat: BackgroundRepeat,
    origin: BackgroundBox,
    clip: BackgroundBox,
}

impl Slot {
    fn new(image: Option<BackgroundImage>) -> Self {
        let defaults = BackgroundLayer::new(BackgroundImage::Gradient(placeholder()));
        Self {
            image,
            size: defaults.size,
            position: defaults.position,
            repeat: defaults.repeat,
            origin: defaults.origin,
            clip: defaults.clip,
        }
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

/// Combines the shorthand and longhands: the shorthand first, then each longhand
/// replaces its part. A longhand that is invalid is ignored on its own.
pub(crate) fn resolve(inputs: &Inputs, colors: &ColorContext) -> Resolved {
    let mut color = None;
    let mut slots: Vec<Slot> = Vec::new();
    if let Some((shorthand_slots, shorthand_color)) = inputs
        .shorthand
        .and_then(|value| parse_shorthand(value, colors))
    {
        slots = shorthand_slots;
        color = shorthand_color;
    }

    if let Some(images) = inputs.image.and_then(|value| parse_images(value, colors)) {
        slots = images
            .into_iter()
            .enumerate()
            .map(|(index, image)| {
                let mut slot = slots.get(index).cloned().unwrap_or_else(|| Slot::new(None));
                slot.image = image;
                slot
            })
            .collect();
    }

    apply_list(&mut slots, inputs.size, |slot, text| {
        slot.size = parse_size(&split_whitespace(text))?;
        Some(())
    });
    apply_list(&mut slots, inputs.position, |slot, text| {
        slot.position = parse_position(&split_whitespace(text))?;
        Some(())
    });
    apply_list(&mut slots, inputs.repeat, |slot, text| {
        slot.repeat = parse_repeat(&split_whitespace(text))?;
        Some(())
    });
    apply_list(&mut slots, inputs.origin, |slot, text| {
        slot.origin = parse_box(text)?;
        Some(())
    });
    apply_list(&mut slots, inputs.clip, |slot, text| {
        // `text` clips the background to the glyphs. That needs an offscreen
        // pass, so for now the layer keeps its box.
        if !text.eq_ignore_ascii_case("text") {
            slot.clip = parse_box(text)?;
        }
        Some(())
    });

    let layers = slots
        .into_iter()
        .filter_map(|slot| {
            Some(BackgroundLayer {
                image: slot.image?,
                size: slot.size,
                position: slot.position,
                repeat: slot.repeat,
                origin: slot.origin,
                clip: slot.clip,
            })
        })
        .collect();
    Resolved { color, layers }
}

/// Applies a comma list to the slots, repeating it to cover them as CSS does. A
/// list with any invalid item is ignored whole.
fn apply_list(
    slots: &mut [Slot],
    value: Option<&str>,
    set: impl Fn(&mut Slot, &str) -> Option<()>,
) {
    let Some(value) = value else { return };
    let items = split_commas(value);
    // An item past the last slot is still checked, so a bad one is not skipped.
    let mut probe = Slot::new(None);
    if items.iter().any(|item| set(&mut probe, item).is_none()) {
        return;
    }
    for (index, slot) in slots.iter_mut().enumerate() {
        // Cannot fail: every item parsed above.
        let _ = set(slot, items[index % items.len()]);
    }
}

/// `background-image`: `None` for an invalid list, else one entry per layer.
fn parse_images(value: &str, colors: &ColorContext) -> Option<Vec<Option<BackgroundImage>>> {
    split_commas(value)
        .into_iter()
        .map(|item| parse_image(item, colors))
        .collect()
}

/// `Some(None)` is a valid image gpui cannot paint.
fn parse_image(item: &str, colors: &ColorContext) -> Option<Option<BackgroundImage>> {
    let lower = item.trim().to_ascii_lowercase();
    if lower == "none" || lower.starts_with("url(") || lower.starts_with("image-set(") {
        return Some(None);
    }
    match gradient::parse(item, colors) {
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

fn is_position_token(token: &str) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "left" | "right" | "top" | "bottom" | "center"
    ) || parse_length_percentage(token).is_some()
}

fn is_size_token(token: &str) -> bool {
    matches!(token.to_ascii_lowercase().as_str(), "auto" | "cover" | "contain")
        || parse_length_percentage(token).is_some()
}

fn is_repeat_token(token: &str) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "repeat" | "repeat-x" | "repeat-y" | "space" | "round" | "no-repeat"
    )
}

fn parse_shorthand(value: &str, colors: &ColorContext) -> Option<(Vec<Slot>, Option<gpui::Rgba>)> {
    let layers = split_commas(value);
    let last = layers.len() - 1;
    let mut slots = Vec::new();
    let mut color = None;
    for (index, layer) in layers.into_iter().enumerate() {
        let (slot, layer_color) = parse_shorthand_layer(layer, colors, index == last)?;
        slots.push(slot);
        color = layer_color;
    }
    Some((slots, color))
}

fn parse_shorthand_layer(
    layer: &str,
    colors: &ColorContext,
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
        if in_size && is_size_token(token) {
            size.push(token);
            continue;
        }
        in_size = false;
        let lower = token.to_ascii_lowercase();
        if gradient::is_gradient(token) || lower == "none" || lower.starts_with("url(") {
            if image.replace(parse_image(token, colors)?).is_some() {
                return None;
            }
        } else if is_repeat_token(token) {
            repeat.push(token);
        } else if matches!(lower.as_str(), "scroll" | "fixed" | "local") {
            // Attachment has no meaning without a scrolling document.
        } else if parse_box(token).is_some() || lower == "text" {
            boxes.push(token);
        } else if is_position_token(token) {
            position.push(token);
        } else if is_last && color.is_none() {
            color = Some(colors.resolve(token)?);
        } else {
            return None;
        }
    }
    if in_size && size.is_empty() {
        return None;
    }

    let mut slot = Slot::new(image.flatten());
    if !position.is_empty() {
        slot.position = parse_position(&position)?;
    }
    if !size.is_empty() {
        slot.size = parse_size(&size)?;
    }
    if !repeat.is_empty() {
        slot.repeat = parse_repeat(&repeat)?;
    }
    match boxes.as_slice() {
        [] => {}
        [one] => {
            if let Some(parsed) = parse_box(one) {
                slot.origin = parsed;
                slot.clip = parsed;
            }
        }
        [origin, clip] => {
            slot.origin = parse_box(origin)?;
            if let Some(parsed) = parse_box(clip) {
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
        resolve(&Inputs { shorthand: Some(value), ..Default::default() }, &context())
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
    fn one_box_sets_origin_and_clip_and_text_is_ignored() {
        let resolved = shorthand("linear-gradient(red, blue) content-box");
        assert_eq!(
            (resolved.layers[0].origin, resolved.layers[0].clip),
            (BackgroundBox::ContentBox, BackgroundBox::ContentBox)
        );
        let text = shorthand("linear-gradient(red, blue) padding-box text");
        assert_eq!(
            (text.layers[0].origin, text.layers[0].clip),
            (BackgroundBox::PaddingBox, BackgroundBox::BorderBox)
        );
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
        assert_eq!(resolved.layers[1].clip, BackgroundBox::BorderBox);
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
        );
        assert_eq!(resolved.layers.len(), 1);
        let BackgroundImage::Gradient(gradient) = &resolved.layers[0].image;
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
        );
        assert_eq!(resolved.layers.len(), 1);
        assert_eq!(resolved.layers[0].size, BackgroundSize::Auto);
        assert_eq!(
            resolve(&Inputs { image: Some("linear-gradient(red)"), ..Default::default() }, &context()),
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
        );
        assert_eq!(resolved.layers.len(), 1);
    }

    #[test]
    fn oklab_gradients_keep_their_space() {
        let resolved = shorthand("linear-gradient(in oklab, red, blue)");
        let BackgroundImage::Gradient(gradient) = &resolved.layers[0].image;
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
        );
        assert_eq!(resolved.layers[0].size, BackgroundSize::Auto);
        assert_eq!(shorthand("linear-gradient(red, blue) 0 0 / -1px"), Resolved::default());
        assert_eq!(shorthand("linear-gradient(red, blue) 0 0 / 10px -5%"), Resolved::default());
        // Only the size is non-negative: a negative position is fine.
        assert_eq!(shorthand("linear-gradient(red, blue) -10px -5px").layers.len(), 1);
    }
}
