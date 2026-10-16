//! The `mask` shorthand and the `mask-*` longhands, assembled into gpui
//! `BackgroundLayer`s. The layer grammar is the one `background` uses, so the
//! parsing is in `background.rs`; this file is what differs.
//!
//! Only the alpha of a gradient is used, which is what `mask-mode: match-source`
//! means for an image. `mask-composite` and `mask-mode` have no props, and the
//! shorthand takes only their initial values (`add`, `alpha`, `match-source`).
//! `mask-border`, `no-clip` and `margin-box` are not supported, so a value that
//! uses them is ignored.

use gpui::{BackgroundBox, BackgroundLayer};

use super::background::{self, Kind, Longhands};
use super::length::LengthContext;
use super::position::parse_box;
use crate::color::ColorContext;

/// The `mask` props of one style, as written.
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

pub(crate) struct Mask;

impl Kind for Mask {
    const ORIGIN: BackgroundBox = BackgroundBox::BorderBox;
    const ALLOWS_COLOR: bool = false;

    fn parse_box(token: &str) -> Option<BackgroundBox> {
        // An element with a CSS layout box has no fill or stroke geometry, so
        // `fill-box` is the content box and the other SVG boxes the border box.
        match token.to_ascii_lowercase().as_str() {
            "fill-box" => Some(BackgroundBox::ContentBox),
            "stroke-box" | "view-box" => Some(BackgroundBox::BorderBox),
            _ => parse_box(token),
        }
    }

    fn is_ignored_keyword(token: &str) -> bool {
        matches!(token, "add" | "alpha" | "match-source")
    }
}

/// The mask layers, first on top, or `None` when no mask prop gave a valid
/// shorthand or image list. `Some(empty)` is `none`: a state style needs it to
/// clear the base mask.
pub(crate) fn resolve(
    inputs: &Inputs,
    colors: &ColorContext,
    lengths: &LengthContext,
) -> Option<Vec<BackgroundLayer>> {
    let shorthand = inputs
        .shorthand
        .and_then(|value| background::parse_shorthand::<Mask>(value, colors, lengths))
        .map(|(slots, _)| slots);
    let image_is_valid = inputs
        .image
        .is_some_and(|value| background::parse_images(value, colors, lengths).is_some());
    if shorthand.is_none() && !image_is_valid {
        return None;
    }
    let longhands = Longhands {
        image: inputs.image,
        size: inputs.size,
        position: inputs.position,
        repeat: inputs.repeat,
        origin: inputs.origin,
        clip: inputs.clip,
    };
    Some(background::apply_longhands::<Mask>(
        shorthand.unwrap_or_default(),
        &longhands,
        colors,
        lengths,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Scheme;
    use gpui::{
        px, BackgroundImage, BackgroundPosition, BackgroundRepeat, BackgroundSize, LengthPercentage,
        RepeatStyle,
    };

    fn context() -> ColorContext {
        ColorContext::root(Scheme::Light)
    }

    fn shorthand(value: &str) -> Option<Vec<BackgroundLayer>> {
        resolve(&Inputs { shorthand: Some(value), ..Default::default() }, &context(), &LengthContext::for_tests())
    }

    fn image(value: &str) -> Option<Vec<BackgroundLayer>> {
        resolve(&Inputs { image: Some(value), ..Default::default() }, &context(), &LengthContext::for_tests())
    }

    #[test]
    fn a_scroll_fade_is_one_layer_with_mask_initial_values() {
        let layers =
            image("linear-gradient(to bottom, transparent, black 20px, black 80%, transparent)")
                .unwrap();
        assert_eq!(layers.len(), 1);
        // Unlike a background, the origin starts at the border box.
        assert_eq!(layers[0].origin, BackgroundBox::BorderBox);
        assert_eq!(layers[0].clip, BackgroundBox::BorderBox);
        assert_eq!(layers[0].size, BackgroundSize::Auto);
        assert_eq!(layers[0].position, BackgroundPosition::ORIGIN);
    }

    #[test]
    fn none_is_an_empty_list_not_an_ignored_value() {
        assert_eq!(image("none"), Some(Vec::new()));
        assert_eq!(shorthand("none"), Some(Vec::new()));
    }

    #[test]
    fn repeating_gradients_and_layer_order() {
        let layers = image(
            "repeating-linear-gradient(black 0 4px, transparent 4px 8px), radial-gradient(black, transparent)",
        )
        .unwrap();
        assert_eq!(layers.len(), 2);
        let BackgroundImage::Gradient(first) = &layers[0].image else { panic!("an image layer") };
        assert!(first.repeating);
        let BackgroundImage::Gradient(second) = &layers[1].image else { panic!("an image layer") };
        assert!(matches!(second.shape, gpui::GradientShape::Radial { .. }));
    }

    #[test]
    fn url_layers_are_skipped_without_shifting_the_others() {
        let layers = resolve(
            &Inputs {
                image: Some("url(a.svg), linear-gradient(black, transparent)"),
                position: Some("left, right"),
                ..Default::default()
            },
            &context(),
            &LengthContext::for_tests(),
        )
        .unwrap();
        assert_eq!(layers.len(), 1);
        assert_eq!(
            layers[0].position.x,
            gpui::BackgroundPositionAxis::from_start(LengthPercentage::Percent(1.0))
        );
        assert_eq!(image("url(a.svg)"), Some(Vec::new()));
    }

    #[test]
    fn the_shorthand_sets_every_part() {
        let layers = shorthand(
            "linear-gradient(black, transparent) no-repeat center / 50% 20px content-box padding-box",
        )
        .unwrap();
        let layer = &layers[0];
        assert_eq!(layer.position, BackgroundPosition::CENTER);
        assert_eq!(
            layer.size,
            BackgroundSize::Explicit {
                width: Some(LengthPercentage::Percent(0.5)),
                height: Some(LengthPercentage::Length(px(20.0)))
            }
        );
        assert_eq!(layer.repeat, BackgroundRepeat { x: RepeatStyle::NoRepeat, y: RepeatStyle::NoRepeat });
        assert_eq!((layer.origin, layer.clip), (BackgroundBox::ContentBox, BackgroundBox::PaddingBox));
    }

    #[test]
    fn one_box_sets_origin_and_clip() {
        let layers = shorthand("linear-gradient(black, transparent) content-box").unwrap();
        assert_eq!(
            (layers[0].origin, layers[0].clip),
            (BackgroundBox::ContentBox, BackgroundBox::ContentBox)
        );
    }

    #[test]
    fn svg_boxes_map_to_the_css_layout_boxes() {
        let layers = shorthand("linear-gradient(black, transparent) fill-box view-box").unwrap();
        assert_eq!(
            (layers[0].origin, layers[0].clip),
            (BackgroundBox::ContentBox, BackgroundBox::BorderBox)
        );
    }

    #[test]
    fn initial_compositing_and_mode_keywords_are_accepted() {
        let layers = shorthand("linear-gradient(black, transparent) add alpha match-source").unwrap();
        assert_eq!(layers.len(), 1);
    }

    #[test]
    fn a_colour_or_an_unsupported_keyword_invalidates_the_shorthand() {
        for bad in [
            "linear-gradient(black, transparent) red",
            "linear-gradient(black, transparent) luminance",
            "linear-gradient(black, transparent) subtract",
            "linear-gradient(black, transparent) no-clip",
            "linear-gradient(black, transparent) margin-box",
            "linear-gradient(black)",
            "scroll",
        ] {
            assert_eq!(shorthand(bad), None, "{bad}");
        }
    }

    #[test]
    fn longhands_override_the_shorthand() {
        let layers = resolve(
            &Inputs {
                shorthand: Some("linear-gradient(black, transparent) no-repeat"),
                repeat: Some("repeat-x"),
                origin: Some("padding-box"),
                image: Some("radial-gradient(black, transparent)"),
                ..Default::default()
            },
            &context(),
            &LengthContext::for_tests(),
        )
        .unwrap();
        assert_eq!(layers.len(), 1);
        let BackgroundImage::Gradient(gradient) = &layers[0].image else { panic!("an image layer") };
        assert!(matches!(gradient.shape, gpui::GradientShape::Radial { .. }));
        assert_eq!(layers[0].repeat, BackgroundRepeat { x: RepeatStyle::Repeat, y: RepeatStyle::NoRepeat });
        assert_eq!(layers[0].origin, BackgroundBox::PaddingBox);
    }

    #[test]
    fn a_bad_longhand_is_ignored_on_its_own() {
        let layers = resolve(
            &Inputs {
                image: Some("linear-gradient(black, transparent)"),
                size: Some("10px, nope"),
                clip: Some("text"),
                ..Default::default()
            },
            &context(),
            &LengthContext::for_tests(),
        )
        .unwrap();
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].size, BackgroundSize::Auto);
        // `text` is a background-clip value, not a mask-clip one.
        assert_eq!(layers[0].clip, BackgroundBox::BorderBox);
        assert_eq!(image("linear-gradient(black)"), None);
    }

    #[test]
    fn no_mask_prop_is_none() {
        assert_eq!(resolve(&Inputs::default(), &context(), &LengthContext::for_tests()), None);
        assert_eq!(
            resolve(&Inputs { size: Some("10px"), ..Default::default() }, &context(), &LengthContext::for_tests()),
            None
        );
    }

    #[test]
    fn lengths_resolve_against_the_element() {
        let layers = resolve(
            &Inputs {
                image: Some("linear-gradient(black 1em, transparent calc(100% - 1rem))"),
                size: Some("10vw 2em"),
                position: Some("1rem 10%"),
                ..Default::default()
            },
            &context(),
            &LengthContext::for_tests(),
        )
        .unwrap();
        assert_eq!(
            layers[0].size,
            BackgroundSize::Explicit {
                width: Some(LengthPercentage::Length(px(80.0))),
                height: Some(LengthPercentage::Length(px(40.0))),
            }
        );
        assert_eq!(layers[0].position.x.offset, LengthPercentage::Length(px(10.0)));
        let BackgroundImage::Gradient(gradient) = &layers[0].image else { panic!("an image layer") };
        let gpui::GradientItem::Stop(last) = gradient.items[1] else { panic!("a hint") };
        assert_eq!(last.position, Some(gpui::StopPosition::Calc { length: px(-10.0), fraction: 1.0 }));
        assert_eq!(image("linear-gradient(black 1ch, transparent)"), None);
    }
}
