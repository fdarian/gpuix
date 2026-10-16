//! The `mask` shorthand and the `mask-*` longhands, assembled into gpui
//! `BackgroundLayer`s. The layer grammar is the one `background` uses, so the
//! parsing is in `background.rs`; this file is what differs.
//!
//! A layer is a gradient or a `url()` image, read by `mask-mode` and combined with
//! the layers below by `mask-composite`; `match-source` reads the alpha of both.
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
    pub mode: Option<&'a str>,
    pub composite: Option<&'a str>,
}

pub(crate) struct Mask;

impl Kind for Mask {
    const ORIGIN: BackgroundBox = BackgroundBox::BorderBox;
    const ALLOWS_COLOR: bool = false;
    const HAS_MASK_PARTS: bool = true;

    fn parse_box(token: &str) -> Option<BackgroundBox> {
        // An element with a CSS layout box has no fill or stroke geometry, so
        // `fill-box` is the content box and the other SVG boxes the border box.
        match token.to_ascii_lowercase().as_str() {
            "fill-box" => Some(BackgroundBox::ContentBox),
            "stroke-box" | "view-box" => Some(BackgroundBox::BorderBox),
            _ => parse_box(token),
        }
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
        .is_some_and(|value| background::is_valid_image_list::<Mask>(value, colors, lengths));
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
        mode: inputs.mode,
        composite: inputs.composite,
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
        MaskComposite, MaskMode, RepeatStyle,
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

    fn key_of(layer: &BackgroundLayer) -> Option<gpui::ImageKey> {
        match layer.image {
            BackgroundImage::Image(key) => Some(key),
            _ => None,
        }
    }

    #[test]
    fn a_url_layer_is_an_image_layer_next_to_the_gradients() {
        let layers = resolve(
            &Inputs {
                image: Some("url(a.svg), linear-gradient(black, transparent), url(\"b c.png\")"),
                position: Some("left, right"),
                ..Default::default()
            },
            &context(),
            &LengthContext::for_tests(),
        )
        .unwrap();
        assert_eq!(layers.len(), 3);
        assert_eq!(key_of(&layers[0]), Some(crate::background_images::key("a.svg")));
        assert!(matches!(layers[1].image, BackgroundImage::Gradient(_)));
        assert_eq!(key_of(&layers[2]), Some(crate::background_images::key("b c.png")));
        assert_eq!(
            layers[1].position.x,
            gpui::BackgroundPositionAxis::from_start(LengthPercentage::Percent(1.0))
        );
        // The list repeats, so the third layer takes the first position again.
        assert_eq!(layers[2].position.x, layers[0].position.x);
        assert_eq!(layers[0].origin, BackgroundBox::BorderBox);
        assert_eq!(image("url(a.svg)").unwrap().len(), 1);
    }

    #[test]
    fn a_url_in_the_shorthand_takes_the_layer_parts_in_any_order() {
        let layers = shorthand("url(a.png) center / 40px no-repeat").unwrap();
        assert_eq!(key_of(&layers[0]), Some(crate::background_images::key("a.png")));
        assert_eq!(layers[0].position, BackgroundPosition::CENTER);
        assert_eq!(
            layers[0].size,
            BackgroundSize::Explicit { width: Some(LengthPercentage::Length(px(40.0))), height: None }
        );
        assert_eq!(layers[0].repeat, BackgroundRepeat { x: RepeatStyle::NoRepeat, y: RepeatStyle::NoRepeat });
        let reordered = shorthand("no-repeat center / 40px url(a.png)").unwrap();
        assert_eq!(reordered, layers);
        let two = shorthand("url(a.png) luminance, linear-gradient(black, transparent) subtract").unwrap();
        assert_eq!(two.len(), 2);
        assert!(matches!(two[1].image, BackgroundImage::Gradient(_)));
    }

    #[test]
    fn a_broken_url_or_image_set_is_not_a_valid_mask_image() {
        for bad in ["url(a.png", "url(a.png) x", "url(a.png) url(b.png)"] {
            assert_eq!(image(bad), None, "{bad}");
            assert_eq!(shorthand(bad), None, "{bad}");
        }
        // An image-set() is a valid layer that paints nothing, as for a background.
        assert_eq!(image("image-set(url(a.png) 1x)"), Some(Vec::new()));
        assert_eq!(image("url('')"), Some(Vec::new()));
    }

    #[test]
    fn resolving_a_mask_requests_the_addresses_it_paints() {
        crate::background_images::requested_for_tests();
        image("url(a.png), none, linear-gradient(black, transparent)");
        shorthand("url(\"b c.png\") no-repeat");
        assert_eq!(crate::background_images::requested_for_tests(), vec!["a.png", "b c.png"]);
        let (colors, lengths) = (context(), LengthContext::for_tests());
        assert!(background::is_valid_image_list::<Mask>("url(e.png)", &colors, &lengths));
        assert!(background::is_valid_shorthand::<Mask>("url(e.png) center", &colors, &lengths));
        assert!(crate::background_images::requested_for_tests().is_empty());
    }

    #[test]
    fn the_key_is_the_one_a_background_has_for_the_same_address() {
        let background = crate::css::background::resolve(
            &crate::css::background::Inputs { image: Some("url(a.png)"), ..Default::default() },
            &context(),
            &LengthContext::for_tests(),
        );
        assert_eq!(key_of(&image("url(a.png)").unwrap()[0]), key_of(&background.layers[0]));
        assert_eq!(key_of(&image("url( 'a.png' )").unwrap()[0]), key_of(&background.layers[0]));
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

    fn mode_and_composite(layer: &BackgroundLayer) -> (MaskMode, MaskComposite) {
        (layer.mask_mode, layer.mask_composite)
    }

    #[test]
    fn a_layer_starts_with_match_source_and_add() {
        let layers = shorthand("linear-gradient(black, transparent) add match-source").unwrap();
        assert_eq!(layers.len(), 1);
        assert_eq!(mode_and_composite(&layers[0]), (MaskMode::MatchSource, MaskComposite::Add));
        assert_eq!(
            mode_and_composite(&image("linear-gradient(black, transparent)").unwrap()[0]),
            (MaskMode::MatchSource, MaskComposite::Add)
        );
    }

    #[test]
    fn the_shorthand_takes_a_mode_and_a_composite_keyword_in_any_order() {
        let gradient = "linear-gradient(black, transparent)";
        for text in [
            format!("{gradient} luminance subtract"),
            format!("subtract luminance {gradient}"),
            format!("luminance {gradient} subtract no-repeat"),
            format!("{gradient} LUMINANCE Subtract"),
        ] {
            let layers = shorthand(&text).unwrap();
            assert_eq!(mode_and_composite(&layers[0]), (MaskMode::Luminance, MaskComposite::Subtract), "{text}");
        }
        let layers = shorthand(&format!("{gradient} alpha intersect, {gradient} exclude, {gradient}")).unwrap();
        let parts: Vec<_> = layers.iter().map(mode_and_composite).collect();
        assert_eq!(
            parts,
            vec![
                (MaskMode::Alpha, MaskComposite::Intersect),
                (MaskMode::MatchSource, MaskComposite::Exclude),
                (MaskMode::MatchSource, MaskComposite::Add),
            ]
        );
    }

    #[test]
    fn a_repeated_or_foreign_keyword_invalidates_the_shorthand() {
        let gradient = "linear-gradient(black, transparent)";
        for bad in [
            "luminance alpha",
            "add subtract",
            "luminance luminance",
            "source-over",
            "source-in",
            "luminosity",
        ] {
            assert_eq!(shorthand(&format!("{gradient} {bad}")), None, "{bad}");
        }
    }

    #[test]
    fn the_mode_and_composite_longhands_repeat_to_cover_the_layers() {
        let gradient = "linear-gradient(black, transparent)";
        let layers = resolve(
            &Inputs {
                image: Some(&format!("{gradient}, {gradient}, {gradient}")),
                mode: Some("luminance, alpha"),
                composite: Some("subtract"),
                ..Default::default()
            },
            &context(),
            &LengthContext::for_tests(),
        )
        .unwrap();
        let parts: Vec<_> = layers.iter().map(mode_and_composite).collect();
        assert_eq!(
            parts,
            vec![
                (MaskMode::Luminance, MaskComposite::Subtract),
                (MaskMode::Alpha, MaskComposite::Subtract),
                (MaskMode::Luminance, MaskComposite::Subtract),
            ]
        );
    }

    #[test]
    fn a_longhand_replaces_the_shorthands_mode_and_composite() {
        let gradient = "linear-gradient(black, transparent)";
        let layers = resolve(
            &Inputs {
                shorthand: Some(&format!("{gradient} luminance subtract, {gradient} intersect")),
                composite: Some("exclude"),
                ..Default::default()
            },
            &context(),
            &LengthContext::for_tests(),
        )
        .unwrap();
        let parts: Vec<_> = layers.iter().map(mode_and_composite).collect();
        assert_eq!(
            parts,
            vec![
                (MaskMode::Luminance, MaskComposite::Exclude),
                (MaskMode::MatchSource, MaskComposite::Exclude),
            ]
        );
    }

    #[test]
    fn an_invalid_mode_or_composite_list_is_ignored_on_its_own() {
        let gradient = "linear-gradient(black, transparent)";
        for (mode, composite) in [
            (Some("luminance, nope"), Some("subtract, source-over")),
            (Some("add"), Some("alpha")),
            (Some(""), Some("")),
            (Some("luminance alpha"), Some("subtract intersect")),
            (Some("luminance,"), Some("subtract,")),
        ] {
            let layers = resolve(
                &Inputs { image: Some(gradient), mode, composite, ..Default::default() },
                &context(),
                &LengthContext::for_tests(),
            )
            .unwrap();
            assert_eq!(layers.len(), 1);
            assert_eq!(mode_and_composite(&layers[0]), (MaskMode::MatchSource, MaskComposite::Add), "{mode:?} {composite:?}");
        }
        let (colors, lengths) = (context(), LengthContext::for_tests());
        for valid in ["alpha", "Luminance, match-source"] {
            assert!(background::is_valid_list::<Mask>(background::Property::Mode, valid, &lengths), "{valid}");
        }
        for invalid in ["add", "luminance, nope", "alpha alpha", ""] {
            assert!(!background::is_valid_list::<Mask>(background::Property::Mode, invalid, &lengths), "{invalid}");
        }
        for valid in ["add", "Exclude, intersect"] {
            assert!(background::is_valid_list::<Mask>(background::Property::Composite, valid, &lengths), "{valid}");
        }
        for invalid in ["alpha", "source-over", "subtract,", ""] {
            assert!(!background::is_valid_list::<Mask>(background::Property::Composite, invalid, &lengths), "{invalid}");
        }
        assert!(background::is_valid_shorthand::<Mask>("none luminance", &colors, &lengths));
    }

    #[test]
    fn a_mode_or_composite_alone_is_no_mask() {
        assert_eq!(
            resolve(&Inputs { mode: Some("luminance"), composite: Some("add"), ..Default::default() }, &context(), &LengthContext::for_tests()),
            None
        );
    }

    #[test]
    fn a_mode_longhand_applies_to_the_shorthands_layers() {
        let layers = resolve(
            &Inputs {
                shorthand: Some("linear-gradient(black, transparent)"),
                mode: Some("luminance"),
                ..Default::default()
            },
            &context(),
            &LengthContext::for_tests(),
        )
        .unwrap();
        assert_eq!(layers[0].mask_mode, MaskMode::Luminance);
    }

    #[test]
    fn a_colour_or_an_unsupported_keyword_invalidates_the_shorthand() {
        for bad in [
            "linear-gradient(black, transparent) red",
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
