//! CSS `text-underline-offset`: how far below the alphabetic baseline an
//! underline sits.
//!
//! The property is inherited, but an underline takes the offset of the box that
//! declared the line (Chrome reads it when it builds the decoration, so a
//! descendant's own offset moves nothing of its ancestor's line). `em` resolves at
//! the element that sets the value and the descendants inherit the px; a
//! percentage is of `1em` of the box that declares the line, so it is inherited as
//! a percentage.

use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

use super::dimension::{parse_linear_length, MAX_LENGTH};
use super::length::{LengthContext, LinearLength};
use crate::style::StyleDesc;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TextUnderlineOffset {
    /// A gap below the baseline that grows with the underline's thickness.
    #[default]
    Auto,
    /// Negative is above the baseline.
    Length(LinearLength),
}

impl TextUnderlineOffset {
    fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.eq_ignore_ascii_case("auto") {
            return Some(Self::Auto);
        }
        parse_linear_length(value).map(Self::Length)
    }

    /// The value an element with `lengths` passes to its descendants, when it
    /// declares `declared` (`None` inherits `self`). What depends on the element is
    /// px from here on; the percentage waits for the box that draws the line.
    pub(crate) fn inherit(self, declared: Option<&Self>, lengths: &LengthContext) -> Self {
        match declared {
            None => self,
            Some(Self::Auto) => Self::Auto,
            Some(Self::Length(length)) => {
                let (px, percent) = lengths.split(length);
                Self::Length(LinearLength { px, percent, ..LinearLength::default() })
            }
        }
    }

    /// The offset in px for a line drawn by the box that has `lengths`; `None` is
    /// `auto`.
    pub(crate) fn resolve(self, lengths: &LengthContext) -> Option<gpui::Pixels> {
        match self {
            Self::Auto => None,
            Self::Length(length) => {
                let px = lengths.resolve(&length, lengths.font_size).clamp(-MAX_LENGTH, MAX_LENGTH);
                Some(gpui::px(px as f32))
            }
        }
    }
}

fn declares_decoration(style: &StyleDesc) -> bool {
    style.text_decoration.is_some() || style.text_decoration_line.is_some()
}

/// `state` (a `focusWithin` or `focusVisible` refinement) as it paints over `below`, the
/// style it refines, whose offset is `below_offset`. A state is applied as a refinement
/// of the element's text style, so it has to carry the whole line: one that declares a
/// decoration takes the offset the element has when it declares none, and one that only
/// changes the offset of a line `below` declares takes that line along. `None` when
/// `state` needs nothing.
fn filled_state(state: &StyleDesc, below: &StyleDesc, below_offset: TextUnderlineOffset) -> Option<StyleDesc> {
    if declares_decoration(state) {
        return (state.text_underline_offset.is_none() && below_offset != TextUnderlineOffset::Auto)
            .then(|| StyleDesc { text_underline_offset: Some(below_offset), ..state.clone() });
    }
    (state.text_underline_offset.is_some() && declares_decoration(below)).then(|| {
        let mut filled = state.clone();
        filled.text_decoration = below.text_decoration.clone();
        filled.text_decoration_line = below.text_decoration_line.clone();
        filled.text_decoration_style = filled.text_decoration_style.or_else(|| below.text_decoration_style.clone());
        filled.text_decoration_color = filled.text_decoration_color.or_else(|| below.text_decoration_color.clone());
        filled.text_decoration_thickness =
            filled.text_decoration_thickness.or_else(|| below.text_decoration_thickness.clone());
        filled
    })
}

/// `style` with the offset it inherits filled in, when it declares a text
/// decoration without an offset of its own: the line takes the offset of the box
/// that declares it, and that box inherits one. The `focusWithin` and `focusVisible`
/// refinements get the same, each over the style under it. `None` when `style` needs
/// nothing.
pub(crate) fn with_inherited(style: &StyleDesc, inherited: TextUnderlineOffset) -> Option<StyleDesc> {
    let base = (declares_decoration(style) && style.text_underline_offset.is_none() && inherited != TextUnderlineOffset::Auto)
        .then(|| StyleDesc { text_underline_offset: Some(inherited), ..style.clone() });
    if style.focus_within.is_none() && style.focus_visible.is_none() {
        return base;
    }
    let mut filled = base.unwrap_or_else(|| style.clone());
    let offset_of = |style: &StyleDesc| style.text_underline_offset.unwrap_or(inherited);
    // `focusVisible` sits on `focusWithin`, which sits on the element's own style.
    let mut below = filled.clone();
    if let Some(within) = filled.focus_within.as_deref() {
        let within = filled_state(within, &below, offset_of(&below)).unwrap_or_else(|| within.clone());
        below = crate::events::state_styles::overlay(&below, &within);
        filled.focus_within = Some(Box::new(within));
    }
    if let Some(visible) = filled.focus_visible.as_deref() {
        if let Some(visible) = filled_state(visible, &below, offset_of(&below)) {
            filled.focus_visible = Some(Box::new(visible));
        }
    }
    (filled != *style).then_some(filled)
}

impl fmt::Display for TextUnderlineOffset {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => formatter.write_str("auto"),
            Self::Length(length) => length.fmt(formatter),
        }
    }
}

impl Serialize for TextUnderlineOffset {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for TextUnderlineOffset {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OffsetVisitor;

        impl Visitor<'_> for OffsetVisitor {
            type Value = TextUnderlineOffset;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("auto, a number of px or a CSS length or percentage")
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
                if !value.is_finite() {
                    return Err(E::custom(format!("invalid textUnderlineOffset {value}: not a finite number")));
                }
                Ok(TextUnderlineOffset::Length(LinearLength::px(value).clamped()))
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                self.visit_f64(value as f64)
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                self.visit_f64(value as f64)
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                TextUnderlineOffset::parse(value).ok_or_else(|| {
                    E::custom(format!(
                        "invalid textUnderlineOffset \"{value}\": auto, a CSS length or a percentage"
                    ))
                })
            }
        }

        deserializer.deserialize_any(OffsetVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<TextUnderlineOffset, serde_json::Error> {
        serde_json::from_str(json)
    }

    fn length(json: &str) -> LinearLength {
        match parse(json).unwrap() {
            TextUnderlineOffset::Length(length) => length,
            TextUnderlineOffset::Auto => panic!("{json} is auto"),
        }
    }

    /// `1em` is 20px and `1rem` 10px.
    fn lengths() -> LengthContext {
        LengthContext::for_tests()
    }

    #[test]
    fn auto_a_number_and_a_length_parse() {
        assert_eq!(parse("\"auto\"").unwrap(), TextUnderlineOffset::Auto);
        assert_eq!(parse("\"AUTO\"").unwrap(), TextUnderlineOffset::Auto);
        assert_eq!(length("4"), LinearLength::px(4.0));
        assert_eq!(length("\"4px\""), LinearLength::px(4.0));
        assert_eq!(length("\"1in\""), LinearLength::px(96.0));
        assert_eq!(length("\"0\""), LinearLength::default());
        assert_eq!(length("\"0.25em\"").em, 0.25);
        assert_eq!(length("\"25%\"").percent, 0.25);
        let calc = length("\"calc(25% + 1px)\"");
        assert_eq!((calc.px, calc.percent), (1.0, 0.25));
    }

    #[test]
    fn a_negative_offset_is_valid() {
        assert_eq!(length("-3"), LinearLength::px(-3.0));
        assert_eq!(length("\"-0.1em\"").em, -0.1);
        assert_eq!(length("\"-10%\"").percent, -0.1);
    }

    #[test]
    fn what_css_rejects_is_an_error() {
        for json in ["\"4\"", "\"thick\"", "\"\"", "\"from-font\"", "\"px\"", "true", "null"] {
            assert!(parse(json).is_err(), "{json} is accepted");
        }
    }

    #[test]
    fn a_length_resolves_against_the_box_that_draws_the_line() {
        let lengths = lengths();
        let resolve = |json: &str| parse(json).unwrap().resolve(&lengths);
        assert_eq!(resolve("\"auto\""), None);
        assert_eq!(resolve("\"4px\""), Some(gpui::px(4.0)));
        assert_eq!(resolve("\"0.25em\""), Some(gpui::px(5.0)));
        assert_eq!(resolve("\"25%\""), Some(gpui::px(5.0)));
        assert_eq!(resolve("\"0.5rem\""), Some(gpui::px(5.0)));
        assert_eq!(resolve("\"calc(10% + 1px)\""), Some(gpui::px(3.0)));
        assert_eq!(resolve("-4"), Some(gpui::px(-4.0)));
    }

    #[test]
    fn em_is_inherited_as_px_and_a_percentage_as_a_percentage() {
        let parent = lengths();
        let child = parent.descend(Some(&LinearLength::px(40.0)), false);
        let inherited = |json: &str| {
            TextUnderlineOffset::Auto.inherit(Some(&parse(json).unwrap()), &parent)
        };
        assert_eq!(inherited("\"0.5em\"").resolve(&child), Some(gpui::px(10.0)));
        assert_eq!(inherited("\"50%\"").resolve(&child), Some(gpui::px(20.0)));
        assert_eq!(inherited("\"calc(50% + 0.5em)\"").resolve(&child), Some(gpui::px(30.0)));
    }

    #[test]
    fn an_element_that_declares_nothing_passes_on_what_it_inherited() {
        let lengths = lengths();
        let declared = TextUnderlineOffset::Length(LinearLength::px(4.0));
        assert_eq!(declared.inherit(None, &lengths), declared);
        assert_eq!(declared.inherit(Some(&TextUnderlineOffset::Auto), &lengths), TextUnderlineOffset::Auto);
    }

    #[test]
    fn a_style_that_declares_a_decoration_takes_the_inherited_offset() {
        let inherited = TextUnderlineOffset::Length(LinearLength::px(4.0));
        let style = |json: &str| serde_json::from_str::<StyleDesc>(json).unwrap();
        let filled = with_inherited(&style(r#"{"textDecoration": "underline"}"#), inherited).unwrap();
        assert_eq!(filled.text_underline_offset, Some(inherited));
        assert!(with_inherited(&style(r#"{"textDecorationLine": "underline"}"#), inherited).is_some());
        assert!(with_inherited(&style(r#"{"color": "red"}"#), inherited).is_none());
        assert!(with_inherited(&style(r#"{"textDecoration": "underline"}"#), TextUnderlineOffset::Auto).is_none());
        assert!(
            with_inherited(&style(r#"{"textDecoration": "underline", "textUnderlineOffset": 2}"#), inherited)
                .is_none(),
            "its own offset wins"
        );
    }

    fn state_of(json: &str) -> StyleDesc {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_focus_state_that_declares_a_decoration_takes_the_inherited_offset() {
        let inherited = TextUnderlineOffset::Length(LinearLength::px(4.0));
        for state in ["focusWithin", "focusVisible"] {
            let style = state_of(&format!(r#"{{"{state}": {{"textDecoration": "underline"}}}}"#));
            let filled = with_inherited(&style, inherited).unwrap_or_else(|| panic!("{state}"));
            let state = filled.focus_within.as_deref().or(filled.focus_visible.as_deref()).unwrap();
            assert_eq!(state.text_underline_offset, Some(inherited));
            assert_eq!(filled.text_underline_offset, None, "the element declares no line");
        }
    }

    #[test]
    fn a_focus_state_takes_the_elements_own_offset_before_the_inherited_one() {
        let inherited = TextUnderlineOffset::Length(LinearLength::px(4.0));
        let style = state_of(
            r#"{"textUnderlineOffset": 7, "focusVisible": {"textDecoration": "underline"}}"#,
        );
        let filled = with_inherited(&style, inherited).unwrap();
        assert_eq!(
            filled.focus_visible.unwrap().text_underline_offset,
            Some(TextUnderlineOffset::Length(LinearLength::px(7.0)))
        );
    }

    #[test]
    fn focus_visible_takes_the_offset_of_focus_within_under_it() {
        let inherited = TextUnderlineOffset::Length(LinearLength::px(4.0));
        let style = state_of(
            r#"{"focusWithin": {"textUnderlineOffset": 9}, "focusVisible": {"textDecoration": "underline"}}"#,
        );
        let filled = with_inherited(&style, inherited).unwrap();
        assert_eq!(
            filled.focus_visible.unwrap().text_underline_offset,
            Some(TextUnderlineOffset::Length(LinearLength::px(9.0)))
        );
    }

    #[test]
    fn a_focus_state_keeps_the_offset_it_declares_and_auto_needs_nothing() {
        let inherited = TextUnderlineOffset::Length(LinearLength::px(4.0));
        let own = state_of(
            r#"{"focusWithin": {"textDecoration": "underline", "textUnderlineOffset": 2}}"#,
        );
        assert!(with_inherited(&own, inherited).is_none());
        let plain = state_of(r#"{"focusWithin": {"textDecoration": "underline"}}"#);
        assert!(with_inherited(&plain, TextUnderlineOffset::Auto).is_none());
    }

    #[test]
    fn a_focus_state_that_only_sets_the_offset_carries_the_line_of_the_element() {
        let style = state_of(
            r##"{"textDecoration": "underline #ff0000", "focusWithin": {"textUnderlineOffset": 6}}"##,
        );
        let filled = with_inherited(&style, TextUnderlineOffset::Auto).unwrap();
        let within = filled.focus_within.unwrap();
        assert_eq!(within.text_decoration.as_deref(), Some("underline #ff0000"));
        assert_eq!(within.text_underline_offset, Some(TextUnderlineOffset::Length(LinearLength::px(6.0))));
        let no_line = state_of(r#"{"focusWithin": {"textUnderlineOffset": 6}}"#);
        assert!(with_inherited(&no_line, TextUnderlineOffset::Auto).is_none());
    }
}
