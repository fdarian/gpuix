//! The CSS `border` shorthands and the one-to-four value `border-color`.

use super::dimension;
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
    /// Computed width in px: zero when the style is `none`.
    pub width: f64,
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
        if let Some(parsed) = parse_width(token) {
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
        LineStyle::None => 0.0,
        _ => width.unwrap_or(MEDIUM),
    };
    Some(BorderSide {
        width,
        style,
        color: color.unwrap_or("currentColor"),
    })
}

const MEDIUM: f64 = 3.0;

fn parse_width(token: &str) -> Option<f64> {
    match token.to_ascii_lowercase().as_str() {
        "thin" => return Some(1.0),
        "medium" => return Some(MEDIUM),
        "thick" => return Some(5.0),
        _ => {}
    }
    dimension::parse_length(token).filter(|width| *width >= 0.0)
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

fn set_width<E: gpui::Styled>(el: E, side: Option<Side>, width: f64) -> E {
    let width = gpui::px(width.max(0.0) as f32);
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
/// longhands, which override these.
///
/// gpui has one line style per element, so a `dashed` anywhere makes every side
/// dashed, and a later `solid` makes them solid again.
pub(crate) fn apply_shorthands<E: gpui::Styled>(
    mut el: E,
    style: &StyleDesc,
    colors: &ColorContext,
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
        el = set_width(el, side, parsed.width);
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
            width: 2.0,
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
        assert_eq!((solid_only.width, solid_only.color), (3.0, "currentColor"));
        assert_eq!(side("thin solid").width, 1.0);
        assert_eq!(side("thick solid").width, 5.0);
        // No style means no border, whatever width was written.
        assert_eq!(side("2px red").width, 0.0);
        assert_eq!(side("2px red").style, LineStyle::None);
    }

    #[test]
    fn none_and_zero_clear_the_border() {
        assert_eq!(side("none").width, 0.0);
        assert_eq!(side("hidden").style, LineStyle::None);
        assert_eq!(side("0 solid red").width, 0.0);
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
        assert_eq!(side("+2PX solid").width, 2.0);
        assert_eq!(side("calc(1px + 1px) solid").width, 2.0);
        assert_eq!(side("1in solid").width, 96.0);
        assert_eq!(parse_border("-1px solid"), None);
        assert_eq!(parse_border("calc(1px - 2px) solid"), None);
    }
}
