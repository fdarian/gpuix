//! `text-decoration` and its longhands, down to what gpui can draw.
//!
//! gpui draws one underline and one strikethrough per run, each solid or (for
//! the underline) wavy, in a colour. So `overline` is read but not drawn, and
//! `double`, `dotted` and `dashed` draw as `solid`, as a browser that lacks the
//! style would. A wavy strikethrough is solid. `text-decoration-thickness` is
//! accepted in the shorthand and not drawn: the line is always 1px.

use super::syntax::split_whitespace;
use crate::style::StyleDesc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LineStyle {
    #[default]
    Solid,
    Double,
    Dotted,
    Dashed,
    Wavy,
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
    /// Read but never drawn, see the module docs.
    #[cfg_attr(not(test), allow(dead_code))]
    pub overline: bool,
    pub line_through: bool,
    pub style: LineStyle,
    /// The CSS colour as written, resolved in the colour context of the element
    /// that declared it. `None` is `currentColor`.
    pub color: Option<String>,
}

impl TextDecoration {
    pub fn wavy(&self) -> bool {
        self.style == LineStyle::Wavy
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
    if let Some(color) = style
        .text_decoration_color
        .as_deref()
        .filter(|value| is_color(value))
    {
        decoration.get_or_insert_with(TextDecoration::default).color = Some(color.trim().to_owned());
    }
    decoration
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
        } else if is_thickness(token) {
            if std::mem::replace(&mut has_thickness, true) {
                return None;
            }
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

fn parse_style(value: &str) -> Option<LineStyle> {
    Some(match value.trim().to_ascii_lowercase().as_str() {
        "solid" => LineStyle::Solid,
        "double" => LineStyle::Double,
        "dotted" => LineStyle::Dotted,
        "dashed" => LineStyle::Dashed,
        "wavy" => LineStyle::Wavy,
        _ => return None,
    })
}

fn is_thickness(token: &str) -> bool {
    let token = token.to_ascii_lowercase();
    if matches!(token.as_str(), "auto" | "from-font") {
        return true;
    }
    let number = token.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%');
    !number.is_empty() && number.parse::<f64>().is_ok()
}

fn is_color(value: &str) -> bool {
    let value = value.trim();
    crate::color::is_contextual(value) || crate::color::parse_color_rgba(value).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(d.underline && d.wavy());
        assert_eq!(d.color.as_deref(), Some("red"));
        let d = parse_shorthand("underline rgb(255 0 0) dotted 2px").unwrap();
        assert_eq!(d.style, LineStyle::Dotted);
        assert_eq!(d.color.as_deref(), Some("rgb(255 0 0)"));
        let d = parse_shorthand("currentColor underline").unwrap();
        assert_eq!(d.color.as_deref(), Some("currentColor"));
        assert_eq!(parse_shorthand("underline solid dashed"), None);
        assert_eq!(parse_shorthand("underline red blue"), None);
    }

    #[test]
    fn the_shorthand_leaves_the_style_solid_and_the_colour_current() {
        let d = parse_shorthand("underline").unwrap();
        assert_eq!(d.style, LineStyle::Solid);
        assert_eq!(d.color, None);
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
        assert!(!d.underline && d.line_through && d.wavy());
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
