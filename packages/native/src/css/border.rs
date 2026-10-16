//! The CSS `border` shorthands, the one-to-four value `border-color`, and the
//! lengths of a border or an outline.

use super::dimension;
use super::length::{LengthContext, LinearLength};
use super::syntax::split_whitespace;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineStyle {
    /// `none` and `hidden`: the border is not drawn and takes no width.
    None,
    Solid,
    Dashed,
}

/// The parts of `border` / `border-top` / ... A part the value does not name has
/// its CSS initial value: `medium` width, `none` style, `currentColor`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BorderSide<'a> {
    /// Zero when the style is `none`.
    pub width: LinearLength,
    pub style: LineStyle,
    /// Colour text, still to be resolved against the element.
    pub color: &'a str,
}

/// `[<width> || <style> || <color>]`, in any order. `None` for an invalid value.
///
/// Only `solid` and `dashed` can be drawn, so the other line styles (`dotted`,
/// `double`, `groove`, ...) make the declaration invalid rather than painting
/// something else.
pub(crate) fn parse_border(value: &str) -> Option<BorderSide<'_>> {
    let mut width = None;
    let mut style = None;
    let mut color = None;
    for token in split_whitespace(value) {
        if let Some(parsed) = parse_shorthand_width(token) {
            if width.replace(parsed).is_some() {
                return None;
            }
        } else if let Some(parsed) = parse_style(token) {
            if style.replace(parsed).is_some() {
                return None;
            }
        } else if dimension::is_numeric(token) || color.replace(token).is_some() {
            // A number that is not a valid width, such as `-1px`, is not a colour either.
            return None;
        }
    }
    if width.is_none() && style.is_none() && color.is_none() {
        return None;
    }
    let style = style.unwrap_or(LineStyle::None);
    let width = match style {
        LineStyle::None => LinearLength::default(),
        _ => width.unwrap_or(MEDIUM),
    };
    Some(BorderSide {
        width,
        style,
        color: color.unwrap_or("currentColor"),
    })
}

const MEDIUM: LinearLength = LinearLength::px(3.0);

/// A border or outline width: `thin`, `medium`, `thick` (Chrome's 1, 3 and 5px)
/// or a length of any unit. There is no percentage. A negative width is invalid
/// in CSS; a longhand is still accepted and clamps to zero when applied.
pub(crate) fn parse_width(token: &str) -> Option<LinearLength> {
    match token.to_ascii_lowercase().as_str() {
        "thin" => return Some(LinearLength::px(1.0)),
        "medium" => return Some(MEDIUM),
        "thick" => return Some(LinearLength::px(5.0)),
        _ => {}
    }
    parse_offset(token)
}

/// A length of any unit with no percentage, which `outline-offset` takes.
pub(crate) fn parse_offset(token: &str) -> Option<LinearLength> {
    dimension::parse_linear_length(token).filter(|length| !length.has_percent())
}

/// A width in the `border` shorthand, where a negative one makes the declaration
/// invalid. A `calc()` that is only negative in some contexts (`1px - 1em`)
/// clamps to zero when it resolves.
fn parse_shorthand_width(token: &str) -> Option<LinearLength> {
    parse_width(token).filter(|width| !width.is_negative())
}

/// The px of a border or outline width, which a `%` never reaches.
pub(crate) fn resolve_width(width: &LinearLength, lengths: &LengthContext) -> gpui::Pixels {
    gpui::px(lengths.resolve(width, 0.0).max(0.0) as f32)
}

fn parse_style(token: &str) -> Option<LineStyle> {
    match token.to_ascii_lowercase().as_str() {
        "none" | "hidden" => Some(LineStyle::None),
        "solid" => Some(LineStyle::Solid),
        "dashed" => Some(LineStyle::Dashed),
        _ => None,
    }
}

/// `border-color`'s one to four values as `[top, right, bottom, left]`, with the
/// CSS rule for the missing ones. Colour text is not validated here.
pub(crate) fn parse_color_sides(value: &str) -> Option<[&str; 4]> {
    match split_whitespace(value).as_slice() {
        [all] => Some([all, all, all, all]),
        [vertical, horizontal] => Some([vertical, horizontal, vertical, horizontal]),
        [top, horizontal, bottom] => Some([top, horizontal, bottom, horizontal]),
        [top, right, bottom, left] => Some([top, right, bottom, left]),
        _ => None,
    }
}

// ── Applying ────────────────────────────────────────────────────────────

use crate::color::ColorContext;
use crate::style::StyleDesc;

#[derive(Clone, Copy)]
enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

fn set_width<E: gpui::Styled>(el: E, side: Option<Side>, width: gpui::Pixels) -> E {
    match side {
        None => el.border(width),
        Some(Side::Top) => el.border_t(width),
        Some(Side::Right) => el.border_r(width),
        Some(Side::Bottom) => el.border_b(width),
        Some(Side::Left) => el.border_l(width),
    }
}

fn set_color<E: gpui::Styled>(el: E, side: Option<Side>, color: gpui::Rgba) -> E {
    match side {
        None => el.border_color(color),
        Some(Side::Top) => el.border_top_color(color),
        Some(Side::Right) => el.border_right_color(color),
        Some(Side::Bottom) => el.border_bottom_color(color),
        Some(Side::Left) => el.border_left_color(color),
    }
}

/// `border` and `borderTop/Right/Bottom/Left`, in that order, so a side
/// overrides the whole-element value. Call before the width and colour
/// longhands, which override these. `lengths` is the element's, which a width
/// in `em` resolves against.
///
/// gpui has one line style per element, so a `dashed` anywhere makes every side
/// dashed, and a later `solid` makes them solid again.
pub(crate) fn apply_shorthands<E: gpui::Styled>(
    mut el: E,
    style: &StyleDesc,
    colors: &ColorContext,
    lengths: &LengthContext,
) -> E {
    let shorthands = [
        (&style.border, None),
        (&style.border_top, Some(Side::Top)),
        (&style.border_right, Some(Side::Right)),
        (&style.border_bottom, Some(Side::Bottom)),
        (&style.border_left, Some(Side::Left)),
    ];
    for (value, side) in shorthands {
        let Some(parsed) = value.as_deref().and_then(parse_border) else {
            continue;
        };
        let color = match parsed.style {
            LineStyle::None => None,
            _ => match colors.resolve(parsed.color) {
                Some(color) => Some(color),
                // An unresolvable colour invalidates the whole declaration.
                None => continue,
            },
        };
        el = set_width(el, side, resolve_width(&parsed.width, lengths));
        if let Some(color) = color {
            el = set_color(el, side, color);
        }
        match parsed.style {
            LineStyle::Dashed => el = el.border_dashed(),
            LineStyle::Solid => el.style().border_style = Some(gpui::BorderStyle::Solid),
            LineStyle::None => {}
        }
    }
    el
}

/// The outline `style` declares, or `None` when it declares no width or colour.
/// The width is 1px unless given, and the colour is transparent unless given.
pub(crate) fn outline(style: &StyleDesc, colors: &ColorContext, lengths: &LengthContext) -> Option<gpui::Outline> {
    if style.outline_width.is_none() && style.outline_color.is_none() {
        return None;
    }
    let color = style
        .outline_color
        .as_deref()
        .and_then(|value| colors.resolve(value))
        .unwrap_or_else(|| gpui::transparent_black().into());
    Some(gpui::Outline {
        width: style
            .outline_width
            .as_ref()
            .map_or(gpui::px(1.0), |width| resolve_width(width, lengths)),
        color: color.into(),
        offset: style
            .outline_offset
            .as_ref()
            .map_or(gpui::px(0.0), |offset| gpui::px(lengths.resolve(offset, 0.0) as f32)),
    })
}

/// `borderColor` (one to four values) and then the per-side colours.
pub(crate) fn apply_colors<E: gpui::Styled>(
    mut el: E,
    style: &StyleDesc,
    colors: &ColorContext,
) -> E {
    if let Some(sides) = style.border_color.as_deref().and_then(parse_color_sides) {
        let resolved = sides.map(|color| colors.resolve(color));
        // One invalid colour invalidates the declaration, as in CSS.
        if let [Some(top), Some(right), Some(bottom), Some(left)] = resolved {
            el = set_color(el, Some(Side::Top), top);
            el = set_color(el, Some(Side::Right), right);
            el = set_color(el, Some(Side::Bottom), bottom);
            el = set_color(el, Some(Side::Left), left);
        }
    }
    for (value, side) in [
        (&style.border_top_color, Side::Top),
        (&style.border_right_color, Side::Right),
        (&style.border_bottom_color, Side::Bottom),
        (&style.border_left_color, Side::Left),
    ] {
        if let Some(color) = value.as_deref().and_then(|value| colors.resolve(value)) {
            el = set_color(el, Some(side), color);
        }
    }
    el
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side(value: &str) -> BorderSide<'_> {
        parse_border(value).unwrap_or_else(|| panic!("did not parse {value}"))
    }

    #[test]
    fn parses_width_style_and_colour_in_any_order() {
        let expected = BorderSide {
            width: LinearLength::px(2.0),
            style: LineStyle::Dashed,
            color: "red",
        };
        for value in ["2px dashed red", "red dashed 2px", "dashed 2px red", "2px red dashed"] {
            assert_eq!(side(value), expected, "{value}");
        }
    }

    #[test]
    fn colour_functions_stay_whole() {
        assert_eq!(side("1px solid rgb(0 0 0 / 50%)").color, "rgb(0 0 0 / 50%)");
        assert_eq!(side("1px solid color-mix(in srgb, red, blue)").color, "color-mix(in srgb, red, blue)");
    }

    #[test]
    fn omitted_parts_take_their_css_initial_values() {
        let solid_only = side("solid");
        assert_eq!((solid_only.width, solid_only.color), (LinearLength::px(3.0), "currentColor"));
        assert_eq!(side("thin solid").width, LinearLength::px(1.0));
        assert_eq!(side("thick solid").width, LinearLength::px(5.0));
        // No style means no border, whatever width was written.
        assert_eq!(side("2px red").width, LinearLength::default());
        assert_eq!(side("2px red").style, LineStyle::None);
    }

    #[test]
    fn none_and_zero_clear_the_border() {
        assert_eq!(side("none").width, LinearLength::default());
        assert_eq!(side("hidden").style, LineStyle::None);
        assert_eq!(side("0 solid red").width, LinearLength::default());
    }

    #[test]
    fn rejects_invalid_borders() {
        for value in [
            "",
            "2 solid red",
            "1px 2px solid red",
            "solid dashed red",
            "1px solid red blue",
            "1px dotted red",
            "1px double red",
        ] {
            assert_eq!(parse_border(value), None, "{value}");
        }
    }

    #[test]
    fn border_color_expands_like_css() {
        assert_eq!(parse_color_sides("red"), Some(["red"; 4]));
        assert_eq!(parse_color_sides("red blue"), Some(["red", "blue", "red", "blue"]));
        assert_eq!(parse_color_sides("red blue green"), Some(["red", "blue", "green", "blue"]));
        assert_eq!(
            parse_color_sides("red rgb(0 0 1) green #fff"),
            Some(["red", "rgb(0 0 1)", "green", "#fff"])
        );
        assert_eq!(parse_color_sides(""), None);
        assert_eq!(parse_color_sides("a b c d e"), None);
    }

    #[test]
    fn widths_take_any_case_sign_and_calc() {
        assert_eq!(side("+2PX solid").width, LinearLength::px(2.0));
        assert_eq!(side("calc(1px + 1px) solid").width, LinearLength::px(2.0));
        assert_eq!(side("1in solid").width, LinearLength::px(96.0));
        assert_eq!(parse_border("-1px solid"), None);
        assert_eq!(parse_border("calc(1px - 2px) solid"), None);
        assert_eq!(parse_border("-0.5em solid"), None);
    }

    fn length(text: &str) -> LinearLength {
        dimension::parse_linear_length(text).unwrap()
    }

    #[test]
    fn the_shorthand_keeps_a_width_with_a_relative_unit_to_resolve_later() {
        assert_eq!(side("0.25em solid red").width, length("0.25em"));
        assert_eq!(side("calc(1px + 1rem) solid").width, length("calc(1px + 1rem)"));
        assert_eq!(side("2vw dashed").width, length("2vw"));
        // The negative part is only known once `em` is.
        assert_eq!(side("calc(1px - 1em) solid").width, length("calc(1px - 1em)"));
    }

    #[test]
    fn a_percentage_is_not_a_width() {
        for value in ["10% solid red", "calc(1px + 10%) solid", "solid calc(100% - 1px)"] {
            assert_eq!(parse_border(value), None, "{value}");
        }
        assert_eq!(parse_width("5%"), None);
        assert_eq!(parse_width("calc(1px + 1%)"), None);
        assert_eq!(parse_offset("5%"), None);
    }

    #[test]
    fn keywords_are_chromes_widths_in_any_case() {
        assert_eq!(parse_width("thin"), Some(LinearLength::px(1.0)));
        assert_eq!(parse_width("Medium"), Some(LinearLength::px(3.0)));
        assert_eq!(parse_width("THICK"), Some(LinearLength::px(5.0)));
        assert_eq!(parse_width("wide"), None);
        assert_eq!(parse_offset("thin"), None);
        assert_eq!(side("medium solid").width, LinearLength::px(3.0));
    }

    #[test]
    fn a_longhand_width_may_be_negative_and_clamps_when_resolved() {
        let lengths = LengthContext::initial(800.0, 600.0);
        let negative = parse_width("-4px").unwrap();
        assert_eq!(resolve_width(&negative, &lengths), gpui::px(0.0));
        let calc = parse_width("calc(1px - 1em)").unwrap();
        assert_eq!(resolve_width(&calc, &lengths), gpui::px(0.0));
    }

    fn outline_of(json: &str, font_size: f64) -> Option<gpui::Outline> {
        let style: StyleDesc = serde_json::from_str(json).unwrap();
        let lengths = LengthContext::initial(800.0, 600.0).descend(Some(&LinearLength::px(font_size)), true);
        outline(&style, &ColorContext::root(crate::color::Scheme::Light), &lengths)
    }

    #[test]
    fn an_outline_resolves_its_width_and_offset_in_the_elements_font_size() {
        let outline = outline_of(r#"{"outlineWidth": "0.25em", "outlineColor": "red", "outlineOffset": "calc(1em + 2px)"}"#, 20.0).unwrap();
        assert_eq!((outline.width, outline.offset), (gpui::px(5.0), gpui::px(22.0)));
        let keyword = outline_of(r#"{"outlineWidth": "thick"}"#, 20.0).unwrap();
        assert_eq!((keyword.width, keyword.offset), (gpui::px(5.0), gpui::px(0.0)));
        let negative = outline_of(r#"{"outlineWidth": -2, "outlineOffset": "-0.5em", "outlineColor": "red"}"#, 20.0).unwrap();
        assert_eq!((negative.width, negative.offset), (gpui::px(0.0), gpui::px(-10.0)));
    }

    #[test]
    fn an_outline_defaults_to_one_px_and_needs_a_width_or_a_colour() {
        assert_eq!(outline_of(r#"{"outlineColor": "red"}"#, 16.0).unwrap().width, gpui::px(1.0));
        assert_eq!(outline_of(r#"{"outlineOffset": 4}"#, 16.0), None);
        assert_eq!(outline_of("{}", 16.0), None);
    }
}
