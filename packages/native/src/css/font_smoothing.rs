//! `-webkit-font-smoothing`, which Chrome honours on macOS only: `antialiased` rasterises
//! glyphs as plain coverage, without the smoothing CoreGraphics thickens strokes with.

use serde::de::IgnoredAny;
use serde::{Deserialize, Deserializer, Serialize};

/// The values Chrome has on macOS. `subpixel-antialiased` is `auto` there: current macOS
/// no longer draws subpixel text, so the two render the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FontSmoothing {
    Auto,
    Antialiased,
    SubpixelAntialiased,
}

impl FontSmoothing {
    pub(crate) fn to_gpui(self) -> gpui::FontSmoothing {
        match self {
            Self::Auto | Self::SubpixelAntialiased => gpui::FontSmoothing::Auto,
            Self::Antialiased => gpui::FontSmoothing::Antialiased,
        }
    }
}

/// A value CSS does not have is an invalid declaration, which is dropped and leaves what the
/// element inherits, or what the style beneath it declares, as it is.
pub(crate) fn deserialize_ignoring_invalid<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<FontSmoothing>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Declared {
        Smoothing(FontSmoothing),
        Invalid(IgnoredAny),
    }
    Ok(match Option::<Declared>::deserialize(deserializer)? {
        Some(Declared::Smoothing(smoothing)) => Some(smoothing),
        Some(Declared::Invalid(_)) | None => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Option<FontSmoothing> {
        #[derive(Deserialize)]
        struct Style {
            #[serde(default, deserialize_with = "deserialize_ignoring_invalid")]
            smoothing: Option<FontSmoothing>,
        }
        serde_json::from_str::<Style>(json).unwrap().smoothing
    }

    #[test]
    fn the_three_values_parse_and_anything_else_is_ignored() {
        assert_eq!(parse(r#"{"smoothing":"auto"}"#), Some(FontSmoothing::Auto));
        assert_eq!(parse(r#"{"smoothing":"antialiased"}"#), Some(FontSmoothing::Antialiased));
        assert_eq!(parse(r#"{"smoothing":"subpixel-antialiased"}"#), Some(FontSmoothing::SubpixelAntialiased));
        for invalid in [r#""none""#, r#""Antialiased""#, "3", "true", "null"] {
            assert_eq!(parse(&format!(r#"{{"smoothing":{invalid}}}"#)), None, "{invalid}");
        }
        assert_eq!(parse("{}"), None);
    }

    #[test]
    fn subpixel_antialiased_is_auto() {
        assert_eq!(FontSmoothing::SubpixelAntialiased.to_gpui(), gpui::FontSmoothing::Auto);
        assert_eq!(FontSmoothing::Antialiased.to_gpui(), gpui::FontSmoothing::Antialiased);
    }
}
