//! `overflow-anchor`: `none` keeps a box, and what is in it, from being the anchor of the
//! scroller around it (and turns the anchoring of the scroller it is on off). Not inherited, not
//! animatable. The scroller's side is `gpui::elements::scroll_anchoring`.

use serde::de::IgnoredAny;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverflowAnchor {
    Auto,
    None,
}

impl OverflowAnchor {
    pub(crate) fn to_gpui(self) -> gpui::OverflowAnchor {
        match self {
            Self::Auto => gpui::OverflowAnchor::Auto,
            Self::None => gpui::OverflowAnchor::None,
        }
    }
}

/// A value CSS does not have is an invalid declaration, which is dropped and leaves the style
/// beneath it as it is.
pub(crate) fn deserialize_ignoring_invalid<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<OverflowAnchor>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Declared {
        Anchor(OverflowAnchor),
        Invalid(IgnoredAny),
    }
    Ok(match Option::<Declared>::deserialize(deserializer)? {
        Some(Declared::Anchor(anchor)) => Some(anchor),
        Some(Declared::Invalid(_)) | None => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Option<OverflowAnchor> {
        #[derive(Deserialize)]
        struct Style {
            #[serde(default, deserialize_with = "deserialize_ignoring_invalid")]
            anchor: Option<OverflowAnchor>,
        }
        serde_json::from_str::<Style>(json).unwrap().anchor
    }

    #[test]
    fn the_two_values_parse_and_anything_else_is_ignored() {
        assert_eq!(parse(r#"{"anchor":"auto"}"#), Some(OverflowAnchor::Auto));
        assert_eq!(parse(r#"{"anchor":"none"}"#), Some(OverflowAnchor::None));
        for invalid in [r#""visible""#, r#""None""#, "3", "true", "null"] {
            assert_eq!(parse(&format!(r#"{{"anchor":{invalid}}}"#)), None, "{invalid}");
        }
        assert_eq!(parse("{}"), None);
    }

    #[test]
    fn the_values_map_to_gpui() {
        assert_eq!(OverflowAnchor::Auto.to_gpui(), gpui::OverflowAnchor::Auto);
        assert_eq!(OverflowAnchor::None.to_gpui(), gpui::OverflowAnchor::None);
    }

    #[test]
    fn a_style_gives_every_kind_of_box_its_value() {
        use crate::color::{ColorContext, Scheme};
        use crate::css::length::LengthContext;
        use crate::style::StyleDesc;
        use gpui::Styled;

        let (colors, lengths) = (ColorContext::root(Scheme::Light), LengthContext::for_tests());
        let given = |json: &str| {
            let style: StyleDesc = serde_json::from_str(json).unwrap();
            let mut element = crate::layout::style::apply(gpui::div(), &style, &colors, &lengths);
            element.style().overflow_anchor
        };
        assert_eq!(given(r#"{"overflowAnchor":"none"}"#), Some(gpui::OverflowAnchor::None));
        assert_eq!(given(r#"{"overflowAnchor":"auto","overflow":"scroll"}"#), Some(gpui::OverflowAnchor::Auto));
        assert_eq!(given(r#"{"overflowAnchor":"sideways"}"#), None);
        assert_eq!(given("{}"), None);
    }
}
