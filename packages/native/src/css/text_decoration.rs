//! `text-decoration` and its longhands, down to what gpui can draw.
//!
//! As in CSS, the style, colour and thickness of one declaration apply to every
//! line it names, and the lines of nested boxes accumulate: gpui runs hold the
//! innermost line of each kind and the outer ones as extra decorations.
//! `blink` is valid and draws nothing.

use gpui::TextDecorationStyle;

use super::dimension::{parse_linear_length, MAX_LENGTH};
use super::length::{LengthContext, LinearLength};
use super::syntax::split_whitespace;
use super::text_underline_offset::TextUnderlineOffset;
use crate::style::StyleDesc;

/// `text-decoration-thickness`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum Thickness {
    #[default]
    Auto,
    FromFont,
    /// Unresolved until the declaring element's font size is known. A percentage
    /// is parsed as that fraction of `1em`, so `percent` is always zero.
    Length(LinearLength),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Lines {
    underline: bool,
    overline: bool,
    line_through: bool,
}

/// What an element declares: `None` from [`of`] when it declares nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TextDecoration {
    pub underline: bool,
    pub overline: bool,
    pub line_through: bool,
    pub style: TextDecorationStyle,
    pub thickness: Thickness,
    /// The CSS colour as written, resolved in the colour context of the element
    /// that declared it. `None` is `currentColor`.
    pub color: Option<String>,
    /// The offset of the box's underline. Not part of `text-decoration`: it is an
    /// inherited property, so the caller sets it to the cascaded value where it has
    /// one (see [`super::text_underline_offset`]).
    pub underline_offset: TextUnderlineOffset,
}

impl TextDecoration {
    /// `lengths` is the declaring element's: a thickness in `em` is its own font
    /// size's, and the lines a descendant inherits keep the px resolved here.
    pub fn underline_style(&self, color: gpui::Hsla, lengths: &LengthContext) -> gpui::UnderlineStyle {
        gpui::UnderlineStyle {
            thickness: self.thickness.fallback(lengths),
            thickness_kind: self.thickness.kind(),
            color: Some(color),
            style: self.style,
            offset: self.underline_offset.resolve(lengths),
        }
    }

    /// How thick the lines of the declaration are, in px, for the declaring element's `lengths`.
    pub(crate) fn thickness_reach(&self, lengths: &LengthContext) -> f32 {
        f32::from(self.thickness.fallback(lengths))
    }

    pub fn strikethrough_style(&self, color: gpui::Hsla, lengths: &LengthContext) -> gpui::StrikethroughStyle {
        gpui::StrikethroughStyle {
            thickness: self.thickness.fallback(lengths),
            thickness_kind: self.thickness.kind(),
            color: Some(color),
            style: self.style,
        }
    }

    pub fn overline_style(&self, color: gpui::Hsla, lengths: &LengthContext) -> gpui::OverlineStyle {
        gpui::OverlineStyle {
            thickness: self.thickness.fallback(lengths),
            thickness_kind: self.thickness.kind(),
            color: Some(color),
            style: self.style,
        }
    }

    /// The colour of every line: the declared one when it resolves, else the
    /// declaring element's own text colour, which a descendant that changes
    /// `color` does not change.
    pub fn resolve_color(&self, colors: &crate::color::ColorContext) -> gpui::Hsla {
        self.color
            .as_deref()
            .and_then(|value| colors.resolve(value))
            .unwrap_or(colors.current)
            .into()
    }
}

impl Thickness {
    fn kind(self) -> gpui::DecorationThickness {
        match self {
            Thickness::Auto => gpui::DecorationThickness::Auto,
            Thickness::FromFont => gpui::DecorationThickness::FromFont,
            Thickness::Length(_) => gpui::DecorationThickness::Length,
        }
    }

    /// What gpui draws when it has no font thickness to use. Chrome's `auto` is a
    /// tenth of the font size of the box that declares the line, unrounded until
    /// the line is painted.
    fn fallback(self, lengths: &LengthContext) -> gpui::Pixels {
        match self {
            Thickness::Auto | Thickness::FromFont => gpui::px(lengths.font_size as f32 / 10.0),
            Thickness::Length(length) => {
                gpui::px(lengths.resolve(&length, 0.0).clamp(0.0, MAX_LENGTH) as f32)
            }
        }
    }
}

/// The declaration of `style`: the `text-decoration` shorthand, then each longhand
/// laid over it, so a longhand wins. An invalid value is ignored.
pub(crate) fn of(style: &StyleDesc) -> Option<TextDecoration> {
    let mut decoration = style.text_decoration.as_deref().and_then(parse_shorthand);
    if let Some(lines) = style.text_decoration_line.as_deref().and_then(parse_line) {
        let decoration = decoration.get_or_insert_with(TextDecoration::default);
        decoration.underline = lines.underline;
        decoration.overline = lines.overline;
        decoration.line_through = lines.line_through;
    }
    if let Some(line_style) = style.text_decoration_style.as_deref().and_then(parse_style) {
        decoration.get_or_insert_with(TextDecoration::default).style = line_style;
    }
    if let Some(thickness) = style
        .text_decoration_thickness
        .as_deref()
        .and_then(parse_thickness)
    {
        decoration
            .get_or_insert_with(TextDecoration::default)
            .thickness = thickness;
    }
    if let Some(color) = style
        .text_decoration_color
        .as_deref()
        .filter(|value| is_color(value))
    {
        decoration.get_or_insert_with(TextDecoration::default).color = Some(color.trim().to_owned());
    }
    if let Some(declared) = style.text_underline_offset {
        if let Some(decoration) = decoration.as_mut() {
            decoration.underline_offset = declared;
        }
    }
    decoration
}

/// Whether `value` is a valid `text-decoration` shorthand, so a state style's
/// invalid one does not reset the longhands it would cover.
pub(crate) fn is_valid_shorthand(value: &str) -> bool {
    parse_shorthand(value).is_some()
}

/// `<line> || <style> || <color> || <thickness>`, each at most once. Anything
/// that is none of those makes the whole value invalid.
fn parse_shorthand(value: &str) -> Option<TextDecoration> {
    let mut decoration = TextDecoration::default();
    let mut lines: Option<Lines> = None;
    let (mut has_style, mut has_color, mut has_thickness) = (false, false, false);
    let tokens = split_whitespace(value);
    if tokens.is_empty() {
        return None;
    }
    let mut index = 0;
    while index < tokens.len() {
        // The line keywords are one component, written as a run.
        let run = tokens[index..]
            .iter()
            .take_while(|token| is_line_keyword(token))
            .count();
        if run > 0 {
            if lines.is_some() {
                return None;
            }
            lines = Some(parse_line(&tokens[index..index + run].join(" "))?);
            index += run;
            continue;
        }
        let token = tokens[index];
        if let Some(style) = parse_style(token) {
            if std::mem::replace(&mut has_style, true) {
                return None;
            }
            decoration.style = style;
        } else if let Some(thickness) = parse_thickness(token) {
            if std::mem::replace(&mut has_thickness, true) {
                return None;
            }
            decoration.thickness = thickness;
        } else if is_color(token) {
            if std::mem::replace(&mut has_color, true) {
                return None;
            }
            decoration.color = Some(token.to_owned());
        } else {
            return None;
        }
        index += 1;
    }
    if let Some(lines) = lines {
        decoration.underline = lines.underline;
        decoration.overline = lines.overline;
        decoration.line_through = lines.line_through;
    }
    Some(decoration)
}

fn is_line_keyword(token: &str) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "none" | "underline" | "overline" | "line-through" | "blink"
    )
}

/// `none`, or any of `underline`, `overline`, `line-through` and `blink`, each at
/// most once. `blink` is valid and draws nothing.
fn parse_line(value: &str) -> Option<Lines> {
    let tokens = split_whitespace(value);
    if tokens.is_empty() {
        return None;
    }
    let mut lines = Lines::default();
    let (mut seen_none, mut seen_blink) = (false, false);
    for token in &tokens {
        let flag = match token.to_ascii_lowercase().as_str() {
            "none" => &mut seen_none,
            "underline" => &mut lines.underline,
            "overline" => &mut lines.overline,
            "line-through" => &mut lines.line_through,
            "blink" => &mut seen_blink,
            _ => return None,
        };
        if std::mem::replace(flag, true) {
            return None;
        }
    }
    if seen_none && tokens.len() > 1 {
        return None;
    }
    Some(lines)
}

fn parse_style(value: &str) -> Option<TextDecorationStyle> {
    Some(match value.trim().to_ascii_lowercase().as_str() {
        "solid" => TextDecorationStyle::Solid,
        "double" => TextDecorationStyle::Double,
        "dotted" => TextDecorationStyle::Dotted,
        "dashed" => TextDecorationStyle::Dashed,
        "wavy" => TextDecorationStyle::Wavy,
        _ => return None,
    })
}

/// `auto`, `from-font`, or a non-negative length or percentage. A percentage is
/// of `1em` of the declaring element (CSS Text Decoration 4), so it is folded
/// into the `em` term here.
fn parse_thickness(token: &str) -> Option<Thickness> {
    let token = token.trim();
    match token.to_ascii_lowercase().as_str() {
        "auto" => Some(Thickness::Auto),
        "from-font" => Some(Thickness::FromFont),
        _ => parse_linear_length(token)
            .filter(|length| !length.is_negative())
            .map(|length| {
                Thickness::Length(LinearLength {
                    em: length.em + length.percent,
                    percent: 0.0,
                    ..length
                })
            }),
    }
}

fn is_color(value: &str) -> bool {
    let value = value.trim();
    crate::color::is_contextual(value) || crate::color::parse_color_rgba(value).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px_thickness(px: f64) -> Thickness {
        Thickness::Length(LinearLength::px(px))
    }

    fn em_thickness(em: f64) -> Thickness {
        Thickness::Length(LinearLength { em, ..LinearLength::default() })
    }

    /// `1em` is 20px and `1rem` 10px.
    fn underline_px(declaration: &str, lengths: &LengthContext) -> gpui::Pixels {
        parse_shorthand(declaration)
            .unwrap()
            .underline_style(gpui::red(), lengths)
            .thickness
    }

    fn lines(value: &str) -> Option<(bool, bool, bool)> {
        parse_shorthand(value).map(|d| (d.underline, d.overline, d.line_through))
    }

    #[test]
    fn one_line_and_several() {
        assert_eq!(lines("underline"), Some((true, false, false)));
        assert_eq!(lines("line-through"), Some((false, false, true)));
        assert_eq!(lines("underline line-through"), Some((true, false, true)));
        assert_eq!(lines("line-through underline overline"), Some((true, true, true)));
        assert_eq!(lines("none"), Some((false, false, false)));
        assert_eq!(lines("UNDERLINE"), Some((true, false, false)));
    }

    #[test]
    fn a_line_named_twice_or_with_none_is_invalid() {
        assert_eq!(lines("underline underline"), None);
        assert_eq!(lines("none underline"), None);
        assert_eq!(lines("underline fancy"), None);
        assert_eq!(lines(""), None);
    }

    #[test]
    fn style_colour_and_thickness_in_any_order() {
        let d = parse_shorthand("wavy red underline").unwrap();
        assert!(d.underline && d.style == TextDecorationStyle::Wavy);
        assert_eq!(d.color.as_deref(), Some("red"));
        let d = parse_shorthand("underline rgb(255 0 0) dotted 2px").unwrap();
        assert_eq!(d.style, TextDecorationStyle::Dotted);
        assert_eq!(d.thickness, px_thickness(2.0));
        assert_eq!(d.color.as_deref(), Some("rgb(255 0 0)"));
        let d = parse_shorthand("currentColor underline").unwrap();
        assert_eq!(d.color.as_deref(), Some("currentColor"));
        assert_eq!(parse_shorthand("underline solid dashed"), None);
        assert_eq!(parse_shorthand("underline red blue"), None);
    }

    #[test]
    fn the_shorthand_leaves_the_style_solid_and_the_colour_current() {
        let d = parse_shorthand("underline").unwrap();
        assert_eq!(d.style, TextDecorationStyle::Solid);
        assert_eq!(d.thickness, Thickness::Auto);
        assert_eq!(d.color, None);
    }

    #[test]
    fn every_style_keyword_parses() {
        for (keyword, style) in [
            ("solid", TextDecorationStyle::Solid),
            ("double", TextDecorationStyle::Double),
            ("dotted", TextDecorationStyle::Dotted),
            ("dashed", TextDecorationStyle::Dashed),
            ("wavy", TextDecorationStyle::Wavy),
        ] {
            assert_eq!(parse_shorthand(&format!("underline {keyword}")).unwrap().style, style);
            assert_eq!(parse_style(&keyword.to_uppercase()), Some(style));
        }
    }

    #[test]
    fn thickness_is_auto_from_font_or_a_length() {
        assert_eq!(parse_thickness("auto"), Some(Thickness::Auto));
        assert_eq!(parse_thickness("From-Font"), Some(Thickness::FromFont));
        assert_eq!(parse_thickness("3px"), Some(px_thickness(3.0)));
        assert_eq!(parse_thickness("1.5PX"), Some(px_thickness(1.5)));
        assert_eq!(parse_thickness("0"), Some(px_thickness(0.0)));
        assert_eq!(parse_thickness("1in"), Some(px_thickness(96.0)));
        assert_eq!(parse_thickness("calc(1px + 2px)"), Some(px_thickness(3.0)));
    }

    #[test]
    fn thickness_takes_font_relative_units_and_a_percentage_of_one_em() {
        assert_eq!(parse_thickness("0.1em"), Some(em_thickness(0.1)));
        assert_eq!(parse_thickness("10%"), Some(em_thickness(0.1)));
        assert_eq!(parse_thickness("0%"), Some(em_thickness(0.0)));
        assert_eq!(
            parse_thickness("1rem"),
            Some(Thickness::Length(LinearLength { rem: 1.0, ..LinearLength::default() }))
        );
        assert_eq!(
            parse_thickness("calc(50% + 1px)"),
            Some(Thickness::Length(LinearLength { em: 0.5, px: 1.0, ..LinearLength::default() }))
        );
    }

    #[test]
    fn thickness_rejects_numbers_negatives_and_words() {
        for value in ["2", "-1px", "-10%", "-0.1em", "thick", "px", "%", ""] {
            assert_eq!(parse_thickness(value), None, "{value}");
        }
    }

    #[test]
    fn a_thickness_resolves_against_the_declaring_elements_lengths() {
        let lengths = LengthContext::for_tests();
        assert_eq!(underline_px("underline 0.1em", &lengths), gpui::px(2.0));
        assert_eq!(underline_px("underline 10%", &lengths), gpui::px(2.0));
        assert_eq!(underline_px("underline 150%", &lengths), gpui::px(30.0));
        assert_eq!(underline_px("underline 0.5rem", &lengths), gpui::px(5.0));
        assert_eq!(underline_px("underline calc(0.1em + 1px)", &lengths), gpui::px(3.0));
        assert_eq!(underline_px("underline 2px", &lengths), gpui::px(2.0));
        let bigger = lengths.descend(Some(&LinearLength::px(40.0)), false);
        assert_eq!(underline_px("underline 0.1em", &bigger), gpui::px(4.0));
        assert_eq!(underline_px("underline 0.5rem", &bigger), gpui::px(5.0), "rem is the root's");
    }

    #[test]
    fn a_thickness_that_calc_makes_negative_is_zero() {
        let lengths = LengthContext::for_tests();
        assert_eq!(underline_px("underline calc(1em - 30px)", &lengths), gpui::px(0.0));
    }

    #[test]
    fn a_longhand_thickness_in_em_resolves_like_the_shorthands() {
        let style: StyleDesc = serde_json::from_value(serde_json::json!({
            "textDecorationLine": "underline",
            "textDecorationThickness": "0.25em",
        }))
        .unwrap();
        let decoration = of(&style).unwrap();
        assert_eq!(decoration.thickness, em_thickness(0.25));
        let lengths = LengthContext::for_tests();
        assert_eq!(
            decoration.overline_style(gpui::red(), &lengths).thickness,
            gpui::px(5.0)
        );
        assert_eq!(
            decoration.strikethrough_style(gpui::red(), &lengths).thickness,
            gpui::px(5.0)
        );
    }

    #[test]
    fn the_shorthand_takes_a_thickness_in_any_position() {
        for value in ["underline 2px dashed red", "2px underline", "dashed 2px red underline"] {
            assert_eq!(parse_shorthand(value).unwrap().thickness, px_thickness(2.0), "{value}");
        }
        assert_eq!(
            parse_shorthand("underline from-font").unwrap().thickness,
            Thickness::FromFont
        );
        assert_eq!(parse_shorthand("underline auto").unwrap().thickness, Thickness::Auto);
    }

    #[test]
    fn an_invalid_or_repeated_thickness_invalidates_the_shorthand() {
        assert_eq!(parse_shorthand("underline -10%"), None);
        assert_eq!(parse_shorthand("underline 2"), None);
        assert_eq!(parse_shorthand("underline 10% 1em"), None);
        assert_eq!(parse_shorthand("underline 1px 2px"), None);
        assert_eq!(parse_shorthand("underline auto 2px"), None);
    }

    #[test]
    fn a_thickness_longhand_lays_over_the_shorthand_and_an_invalid_one_is_ignored() {
        let style: StyleDesc = serde_json::from_value(serde_json::json!({
            "textDecoration": "underline 2px",
            "textDecorationThickness": "4px",
        }))
        .unwrap();
        assert_eq!(of(&style).unwrap().thickness, px_thickness(4.0));
        let style: StyleDesc = serde_json::from_value(serde_json::json!({
            "textDecoration": "underline 2px",
            "textDecorationThickness": "auto",
        }))
        .unwrap();
        assert_eq!(of(&style).unwrap().thickness, Thickness::Auto);
        let style: StyleDesc = serde_json::from_value(serde_json::json!({
            "textDecoration": "underline 2px",
            "textDecorationThickness": "-5%",
        }))
        .unwrap();
        assert_eq!(of(&style).unwrap().thickness, px_thickness(2.0));
        let style: StyleDesc = serde_json::from_value(serde_json::json!({
            "textDecorationThickness": "3px",
        }))
        .unwrap();
        assert_eq!(of(&style).unwrap().thickness, px_thickness(3.0));
    }

    #[test]
    fn every_line_gets_the_style_and_thickness_of_the_declaration() {
        let d = parse_shorthand("underline overline line-through wavy 3px").unwrap();
        let color = gpui::red();
        let lengths = LengthContext::for_tests();
        let (under, over, through) = (
            d.underline_style(color, &lengths),
            d.overline_style(color, &lengths),
            d.strikethrough_style(color, &lengths),
        );
        for (style, thickness, kind) in [
            (under.style, under.thickness, under.thickness_kind),
            (over.style, over.thickness, over.thickness_kind),
            (through.style, through.thickness, through.thickness_kind),
        ] {
            assert_eq!(style, TextDecorationStyle::Wavy);
            assert_eq!(thickness, gpui::px(3.0));
            assert_eq!(kind, gpui::DecorationThickness::Length);
        }
    }

    /// `1em` is 20px in the test context.
    #[test]
    fn auto_and_from_font_carry_a_tenth_of_the_font_size_for_every_line() {
        let lengths = LengthContext::for_tests();
        for (value, kind) in [
            ("underline overline line-through", gpui::DecorationThickness::Auto),
            ("underline overline line-through auto", gpui::DecorationThickness::Auto),
            ("underline overline line-through from-font", gpui::DecorationThickness::FromFont),
        ] {
            let d = parse_shorthand(value).unwrap();
            let color = gpui::red();
            let (under, over, through) = (
                d.underline_style(color, &lengths),
                d.overline_style(color, &lengths),
                d.strikethrough_style(color, &lengths),
            );
            for (thickness, line_kind) in [
                (under.thickness, under.thickness_kind),
                (over.thickness, over.thickness_kind),
                (through.thickness, through.thickness_kind),
            ] {
                assert_eq!(thickness, gpui::px(2.0), "{value}");
                assert_eq!(line_kind, kind, "{value}");
            }
        }
    }

    #[test]
    fn longhands_lay_over_the_shorthand() {
        let style: StyleDesc = serde_json::from_value(serde_json::json!({
            "textDecoration": "underline red",
            "textDecorationLine": "line-through",
            "textDecorationStyle": "wavy",
            "textDecorationColor": "blue",
        }))
        .unwrap();
        let d = of(&style).unwrap();
        assert!(!d.underline && d.line_through && d.style == TextDecorationStyle::Wavy);
        assert_eq!(d.color.as_deref(), Some("blue"));
    }

    #[test]
    fn a_longhand_alone_declares_and_an_invalid_one_does_not() {
        let style: StyleDesc =
            serde_json::from_value(serde_json::json!({"textDecorationLine": "underline"})).unwrap();
        assert!(of(&style).unwrap().underline);
        let style: StyleDesc =
            serde_json::from_value(serde_json::json!({"textDecorationLine": "squiggle"})).unwrap();
        assert_eq!(of(&style), None);
        assert_eq!(of(&StyleDesc::default()), None);
    }
}
