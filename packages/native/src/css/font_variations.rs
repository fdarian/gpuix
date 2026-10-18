//! `font-variation-settings`, the axes of a variable font.

use gpui::FontVariations;
use serde::de::IgnoredAny;
use serde::{Deserialize, Deserializer, Serialize};

use super::dimension::parse_number;
use super::font_features::parse_tag;
use super::syntax::{split_commas, split_whitespace};

/// `font-variation-settings`. Parsing a string that CSS does not have fails; a style
/// ignores such a declaration instead ([`deserialize_ignoring_invalid`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct VariationSettings(FontVariations);

impl VariationSettings {
    pub(crate) fn variations(&self) -> FontVariations {
        self.0.clone()
    }

    /// The axes sorted by tag, which is what motion blends.
    pub(crate) fn axes(&self) -> Vec<(u32, f64)> {
        axes_of(&self.0)
    }

    pub(crate) fn from_axes(axes: &[(u32, f64)]) -> Self {
        Self(FontVariations::new(axes.iter().map(|(tag, value)| (*tag, *value as f32))))
    }
}

/// What a style does with `fontVariationSettings`: a value CSS does not have is an invalid
/// declaration, which is dropped and leaves what the element inherits, or what the style
/// beneath it declares, as it is.
pub(crate) fn deserialize_ignoring_invalid<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<VariationSettings>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Declared {
        Settings(VariationSettings),
        Invalid(IgnoredAny),
    }
    Ok(match Option::<Declared>::deserialize(deserializer)? {
        Some(Declared::Settings(settings)) => Some(settings),
        Some(Declared::Invalid(_)) | None => None,
    })
}

pub(crate) fn axes_of(variations: &FontVariations) -> Vec<(u32, f64)> {
    variations
        .tag_value_list()
        .iter()
        .map(|(tag, value)| (*tag, f64::from(*value)))
        .collect()
}

impl TryFrom<String> for VariationSettings {
    type Error = String;

    fn try_from(value: String) -> Result<Self, String> {
        parse_variation_settings(&value)
            .map(Self)
            .ok_or_else(|| format!("invalid fontVariationSettings: {value:?}"))
    }
}

impl From<VariationSettings> for String {
    fn from(value: VariationSettings) -> String {
        if value.0.is_empty() {
            return "normal".to_owned();
        }
        value
            .0
            .tag_value_list()
            .iter()
            .map(|(tag, value)| {
                let tag: String = tag.to_be_bytes().iter().map(|byte| *byte as char).collect();
                format!("\"{tag}\" {value}")
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// `normal`, or a comma list of `"tag" <number>`. A tag that appears twice takes
/// its last value.
fn parse_variation_settings(value: &str) -> Option<FontVariations> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("normal") {
        return Some(FontVariations::default());
    }
    let mut variations = Vec::new();
    for item in split_commas(value) {
        let (tag, setting) = parse_tag(item)?;
        let setting = split_whitespace(setting);
        if setting.len() != 1 {
            return None;
        }
        let number = parse_number(setting[0])?;
        if !number.is_finite() {
            return None;
        }
        let tag: [u8; 4] = tag.as_bytes().try_into().ok()?;
        variations.push((FontVariations::tag(tag), number as f32));
    }
    Some(FontVariations::new(variations))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(variations: &FontVariations) -> Vec<(String, f32)> {
        variations
            .tag_value_list()
            .iter()
            .map(|(tag, value)| {
                (
                    String::from_utf8(tag.to_be_bytes().to_vec()).unwrap(),
                    *value,
                )
            })
            .collect()
    }

    fn pairs(list: &[(&str, f32)]) -> Vec<(String, f32)> {
        list.iter()
            .map(|(tag, value)| (tag.to_string(), *value))
            .collect()
    }

    #[test]
    fn parses_a_list_of_axes() {
        let parsed =
            parse_variation_settings(r#""wght" 650, 'wdth' 90.5, "slnt" -10"#).expect("valid");
        assert_eq!(
            tags(&parsed),
            pairs(&[("slnt", -10.0), ("wdth", 90.5), ("wght", 650.0)])
        );
        assert!(parse_variation_settings("normal")
            .expect("valid")
            .is_empty());
        assert!(parse_variation_settings(" NORMAL ")
            .expect("valid")
            .is_empty());
    }

    #[test]
    fn the_last_value_of_a_repeated_tag_wins() {
        let parsed =
            parse_variation_settings(r#""wght" 300, "wdth" 80, "wght" 700"#).expect("valid");
        assert_eq!(tags(&parsed), pairs(&[("wdth", 80.0), ("wght", 700.0)]));
    }

    #[test]
    fn equal_settings_are_equal_whatever_their_order() {
        assert_eq!(
            parse_variation_settings(r#""wght" 650, "wdth" 90"#),
            parse_variation_settings(r#""wdth" 90, "wght" 650"#)
        );
    }

    #[test]
    fn rejects_malformed_settings() {
        for input in [
            "",
            "wght 650",
            r#""wght""#,
            r#""wght" on"#,
            r#""wgh" 650"#,
            r#""wghts" 650"#,
            r#""wght" 650px"#,
            r#""wght" 1 2"#,
            r#""wght" 650,"#,
            r#""wght" 650 "wdth" 90"#,
            r#""wght" inf"#,
            r#""wght" NaN"#,
            r#"normal, "wght" 650"#,
        ] {
            assert!(parse_variation_settings(input).is_none(), "{input}");
        }
    }

    #[test]
    fn a_style_ignores_an_invalid_value() {
        for invalid in [r#""wght 650""#, "650", "true", r#""""#, "null"] {
            let parsed: crate::style::StyleDesc =
                serde_json::from_str(&format!(r#"{{"fontVariationSettings": {invalid}, "color": "red"}}"#))
                    .expect("an invalid declaration is dropped, not an error");
            assert_eq!(parsed.font_variation_settings, None, "{invalid}");
            assert_eq!(parsed.color.as_deref(), Some("red"), "the rest of the style stays");
        }
        let parsed: crate::style::StyleDesc =
            serde_json::from_str(r#"{"fontVariationSettings": "\"wght\" 650"}"#).expect("valid");
        assert_eq!(
            parsed
                .font_variation_settings
                .expect("declared")
                .variations(),
            parse_variation_settings(r#""wght" 650"#).unwrap()
        );
    }

    #[test]
    fn a_value_survives_a_round_trip() {
        let settings =
            VariationSettings::try_from(r#""wdth" 90.5, "wght" 650"#.to_owned()).expect("valid");
        let written = String::from(settings.clone());
        assert_eq!(
            VariationSettings::try_from(written).expect("valid"),
            settings
        );
        assert_eq!(
            String::from(VariationSettings::try_from("normal".to_owned()).unwrap()),
            "normal"
        );
    }
}
