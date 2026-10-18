//! CSS `text-wrap` (CSS Text 4 §5) and its longhands `text-wrap-mode` and
//! `text-wrap-style`: whether a line wraps, and which of the ways it can wrap it
//! takes.

use serde::{Deserialize, Serialize};

/// `text-wrap-mode`. It is also the wrap component of `white-space`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", try_from = "String")]
pub enum TextWrapMode {
    Wrap,
    Nowrap,
}

impl TryFrom<String> for TextWrapMode {
    type Error = String;

    fn try_from(value: String) -> Result<Self, String> {
        Self::parse(&value).ok_or_else(|| format!("invalid textWrapMode: {value:?}"))
    }
}

impl TextWrapMode {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "wrap" => Some(Self::Wrap),
            "nowrap" => Some(Self::Nowrap),
            _ => None,
        }
    }

    fn keyword(self) -> &'static str {
        match self {
            Self::Wrap => "wrap",
            Self::Nowrap => "nowrap",
        }
    }
}

/// `text-wrap-style`. `stable` lays a text out as `auto` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", try_from = "String")]
pub enum TextWrapStyle {
    Auto,
    Balance,
    Pretty,
    Stable,
}

impl TryFrom<String> for TextWrapStyle {
    type Error = String;

    fn try_from(value: String) -> Result<Self, String> {
        Self::parse(&value).ok_or_else(|| format!("invalid textWrapStyle: {value:?}"))
    }
}

impl TextWrapStyle {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "balance" => Some(Self::Balance),
            "pretty" => Some(Self::Pretty),
            "stable" => Some(Self::Stable),
            _ => None,
        }
    }

    fn keyword(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Balance => "balance",
            Self::Pretty => "pretty",
            Self::Stable => "stable",
        }
    }

    fn to_gpui(self) -> gpui::TextWrapStyle {
        match self {
            Self::Auto | Self::Stable => gpui::TextWrapStyle::Auto,
            Self::Balance => gpui::TextWrapStyle::Balance,
            Self::Pretty => gpui::TextWrapStyle::Pretty,
        }
    }
}

/// The `text-wrap` shorthand: `<'text-wrap-mode'> || <'text-wrap-style'>`, where
/// what the value leaves out is the longhand's initial value, `wrap` and `auto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TextWrap {
    pub mode: TextWrapMode,
    pub style: TextWrapStyle,
}

impl TryFrom<String> for TextWrap {
    type Error = String;

    fn try_from(value: String) -> Result<Self, String> {
        let invalid = || format!("invalid textWrap: {value:?}");
        let mut mode = None;
        let mut style = None;
        let mut keywords = 0;
        for keyword in value.split_whitespace() {
            keywords += 1;
            if let Some(parsed) = TextWrapMode::parse(keyword) {
                if mode.replace(parsed).is_some() {
                    return Err(invalid());
                }
            } else if let Some(parsed) = TextWrapStyle::parse(keyword) {
                if style.replace(parsed).is_some() {
                    return Err(invalid());
                }
            } else {
                return Err(invalid());
            }
        }
        if keywords == 0 {
            return Err(invalid());
        }
        Ok(Self {
            mode: mode.unwrap_or(TextWrapMode::Wrap),
            style: style.unwrap_or(TextWrapStyle::Auto),
        })
    }
}

impl From<TextWrap> for String {
    fn from(value: TextWrap) -> String {
        format!("{} {}", value.mode.keyword(), value.style.keyword())
    }
}

/// Sets what an element declares. A longhand wins over the shorthand, and
/// `text-wrap-mode` wins over the wrap component of `white-space`, which has been
/// applied to `text_style` already: the declaration is the longhand of it, and a
/// `white-space` that keeps its spaces (`pre`) wraps once this says `wrap`.
pub fn apply(
    text_style: &mut gpui::TextStyleRefinement,
    shorthand: Option<TextWrap>,
    mode: Option<TextWrapMode>,
    style: Option<TextWrapStyle>,
) {
    match mode.or(shorthand.map(|shorthand| shorthand.mode)) {
        Some(TextWrapMode::Wrap) => {
            text_style.white_space = Some(gpui::WhiteSpace::Normal);
            // Whether they hang is `break-spaces`'s to override, so this does not
            // undo an inherited one.
            text_style.hang_trailing_spaces = Some(true);
        }
        Some(TextWrapMode::Nowrap) => {
            text_style.white_space = Some(gpui::WhiteSpace::Nowrap);
            text_style.hang_trailing_spaces = Some(false);
        }
        None => {}
    }
    if let Some(style) = style.or(shorthand.map(|shorthand| shorthand.style)) {
        text_style.text_wrap_style = Some(style.to_gpui());
    }
}

/// Whether the box `style` declares that a line may break in its text, for a box inside
/// a paragraph: `text-wrap-mode` alone or in the shorthand, else the wrap component of
/// `white-space`, as [`apply`] has it. `None` leaves it to the box around.
pub fn declared_wraps(style: &crate::style::StyleDesc) -> Option<bool> {
    style
        .text_wrap_mode
        .or(style.text_wrap.map(|shorthand| shorthand.mode))
        .map(|mode| mode == TextWrapMode::Wrap)
        .or_else(|| {
            style
                .white_space
                .as_deref()
                .and_then(crate::text::white_space::WhiteSpace::parse)
                .map(crate::text::white_space::WhiteSpace::wraps)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shorthand(text: &str) -> Result<TextWrap, serde_json::Error> {
        serde_json::from_str(&format!("{text:?}"))
    }

    fn resolved(
        shorthand: Option<TextWrap>,
        mode: Option<TextWrapMode>,
        style: Option<TextWrapStyle>,
    ) -> gpui::TextStyleRefinement {
        let mut text_style = gpui::TextStyleRefinement::default();
        apply(&mut text_style, shorthand, mode, style);
        text_style
    }

    #[test]
    fn the_shorthand_takes_a_mode_a_style_or_both_in_either_order() {
        for (text, mode, style) in [
            ("wrap", TextWrapMode::Wrap, TextWrapStyle::Auto),
            ("nowrap", TextWrapMode::Nowrap, TextWrapStyle::Auto),
            ("balance", TextWrapMode::Wrap, TextWrapStyle::Balance),
            ("pretty", TextWrapMode::Wrap, TextWrapStyle::Pretty),
            ("stable", TextWrapMode::Wrap, TextWrapStyle::Stable),
            ("nowrap balance", TextWrapMode::Nowrap, TextWrapStyle::Balance),
            ("pretty wrap", TextWrapMode::Wrap, TextWrapStyle::Pretty),
        ] {
            assert_eq!(shorthand(text).unwrap(), TextWrap { mode, style }, "{text}");
        }
    }

    #[test]
    fn what_css_rejects_is_an_error() {
        for text in ["", "  ", "wrapp", "wrap nowrap", "balance pretty", "wrap balance auto", "Balance"] {
            assert!(shorthand(text).is_err(), "{text:?} is a text-wrap");
        }
        for json in ["\"balance\"", "\"\"", "1"] {
            assert!(serde_json::from_str::<TextWrapMode>(json).is_err(), "{json}");
        }
        for json in ["\"nowrap\"", "\"\"", "null"] {
            assert!(serde_json::from_str::<TextWrapStyle>(json).is_err(), "{json}");
        }
    }

    #[test]
    fn a_style_reads_them_and_an_invalid_one_is_not_interned() {
        let style: crate::style::StyleDesc = serde_json::from_str(
            r#"{"textWrap": "balance", "textWrapMode": "nowrap", "textWrapStyle": "pretty"}"#,
        )
        .unwrap();
        assert_eq!(style.text_wrap_mode, Some(TextWrapMode::Nowrap));
        assert_eq!(style.text_wrap_style, Some(TextWrapStyle::Pretty));
        assert_eq!(
            style.text_wrap,
            Some(TextWrap { mode: TextWrapMode::Wrap, style: TextWrapStyle::Balance })
        );
        let mut table = crate::retained_tree::StyleTable::default();
        assert!(table.intern(br#"{"textWrap": "nowrap balance"}"#).is_ok());
        for (json, name) in [
            (&br#"{"textWrap": "wide"}"#[..], "invalid textWrap"),
            (&br#"{"textWrapMode": "balance"}"#[..], "invalid textWrapMode"),
            (&br#"{"textWrapStyle": "nowrap"}"#[..], "invalid textWrapStyle"),
        ] {
            let error = table.intern(json).unwrap_err();
            assert!(error.contains(name), "{error}");
        }
    }

    #[test]
    fn a_box_declares_its_mode_by_the_longhand_the_shorthand_or_white_space_in_that_order() {
        let declared = |json: &str| {
            let style: crate::style::StyleDesc = serde_json::from_str(json).unwrap();
            declared_wraps(&style)
        };
        assert_eq!(declared("{}"), None);
        assert_eq!(declared(r#"{"textWrapStyle": "balance"}"#), None, "the style says nothing of the mode");
        assert_eq!(declared(r#"{"whiteSpace": "nowrap"}"#), Some(false));
        assert_eq!(declared(r#"{"whiteSpace": "pre"}"#), Some(false));
        assert_eq!(declared(r#"{"whiteSpace": "pre-wrap"}"#), Some(true));
        assert_eq!(declared(r#"{"whiteSpace": "nowrap", "textWrapMode": "wrap"}"#), Some(true));
        assert_eq!(declared(r#"{"whiteSpace": "nowrap", "textWrap": "balance"}"#), Some(true));
        assert_eq!(declared(r#"{"textWrap": "nowrap", "textWrapMode": "wrap"}"#), Some(true));
        assert_eq!(declared(r#"{"textWrap": "nowrap balance"}"#), Some(false));
    }

    #[test]
    fn the_shorthand_survives_a_round_trip() {
        let value = TextWrap { mode: TextWrapMode::Nowrap, style: TextWrapStyle::Balance };
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(serde_json::from_str::<TextWrap>(&json).unwrap(), value);
    }

    #[test]
    fn each_style_reaches_gpui_and_stable_is_auto() {
        for (style, expected) in [
            (TextWrapStyle::Auto, gpui::TextWrapStyle::Auto),
            (TextWrapStyle::Stable, gpui::TextWrapStyle::Auto),
            (TextWrapStyle::Balance, gpui::TextWrapStyle::Balance),
            (TextWrapStyle::Pretty, gpui::TextWrapStyle::Pretty),
        ] {
            assert_eq!(resolved(None, None, Some(style)).text_wrap_style, Some(expected));
        }
    }

    #[test]
    fn the_mode_reaches_the_white_space_gpui_wraps_by() {
        let wrapped = resolved(None, Some(TextWrapMode::Wrap), None);
        assert_eq!(wrapped.white_space, Some(gpui::WhiteSpace::Normal));
        assert_eq!(wrapped.hang_trailing_spaces, Some(true));
        let unwrapped = resolved(None, Some(TextWrapMode::Nowrap), None);
        assert_eq!(unwrapped.white_space, Some(gpui::WhiteSpace::Nowrap));
        assert_eq!(unwrapped.hang_trailing_spaces, Some(false));
        let neither = resolved(None, None, None);
        assert_eq!(neither.white_space, None);
        assert_eq!(neither.hang_trailing_spaces, None);
        assert_eq!(neither.text_wrap_style, None);
    }

    #[test]
    fn a_longhand_wins_over_the_shorthand_and_the_shorthand_sets_both() {
        let balance = shorthand("balance").ok();
        let both = resolved(balance, Some(TextWrapMode::Nowrap), Some(TextWrapStyle::Pretty));
        assert_eq!(both.white_space, Some(gpui::WhiteSpace::Nowrap));
        assert_eq!(both.text_wrap_style, Some(gpui::TextWrapStyle::Pretty));
        // The shorthand alone resets the mode, as a shorthand does.
        let alone = resolved(balance, None, None);
        assert_eq!(alone.white_space, Some(gpui::WhiteSpace::Normal));
        assert_eq!(alone.text_wrap_style, Some(gpui::TextWrapStyle::Balance));
        // A longhand takes only its own half.
        let half = resolved(shorthand("nowrap balance").ok(), Some(TextWrapMode::Wrap), None);
        assert_eq!(half.white_space, Some(gpui::WhiteSpace::Normal));
        assert_eq!(half.text_wrap_style, Some(gpui::TextWrapStyle::Balance));
    }
}
