//! The CSS `border` shorthands, the one-to-four value `border-color` and `border-style`,
//! and the lengths of a border or an outline.

use std::fmt;

use super::dimension;
use super::length::{LengthContext, LinearLength};
use super::syntax::{expand_sides, split_whitespace};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStyle {
    /// The border is not drawn and takes no width.
    None,
    /// Like `none`, but in a table with collapsed borders it also removes the borders it meets.
    Hidden,
    Dotted,
    Dashed,
    Solid,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

impl fmt::Display for LineStyle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::None => "none",
            Self::Hidden => "hidden",
            Self::Dotted => "dotted",
            Self::Dashed => "dashed",
            Self::Solid => "solid",
            Self::Double => "double",
            Self::Groove => "groove",
            Self::Ridge => "ridge",
            Self::Inset => "inset",
            Self::Outset => "outset",
        })
    }
}

impl LineStyle {
    /// An `<outline-style>` keyword: a `<line-style>` but `hidden`, or `auto`, which paints
    /// as `solid` here (Chrome draws its focus ring for it).
    pub(crate) fn parse_outline(token: &str) -> Option<Self> {
        match token.to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Solid),
            "hidden" => None,
            _ => Self::parse(token),
        }
    }

    /// A `<line-style>` keyword, in any case.
    pub(crate) fn parse(token: &str) -> Option<Self> {
        match token.to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "hidden" => Some(Self::Hidden),
            "dotted" => Some(Self::Dotted),
            "dashed" => Some(Self::Dashed),
            "solid" => Some(Self::Solid),
            "double" => Some(Self::Double),
            "groove" => Some(Self::Groove),
            "ridge" => Some(Self::Ridge),
            "inset" => Some(Self::Inset),
            "outset" => Some(Self::Outset),
            _ => None,
        }
    }

    /// Whether the border is there: it has a width and is drawn.
    pub(crate) fn is_visible(self) -> bool {
        !matches!(self, Self::None | Self::Hidden)
    }

    /// What gpui paints, `None` for a style that takes no width.
    pub(crate) fn painted(self) -> Option<gpui::BorderStyle> {
        match self {
            Self::None | Self::Hidden => None,
            Self::Dotted => Some(gpui::BorderStyle::Dotted),
            Self::Dashed => Some(gpui::BorderStyle::Dashed),
            Self::Solid => Some(gpui::BorderStyle::Solid),
            Self::Double => Some(gpui::BorderStyle::Double),
            Self::Groove => Some(gpui::BorderStyle::Groove),
            Self::Ridge => Some(gpui::BorderStyle::Ridge),
            Self::Inset => Some(gpui::BorderStyle::Inset),
            Self::Outset => Some(gpui::BorderStyle::Outset),
        }
    }
}

/// The parts of `border` / `border-top` / ... A part the value does not name has
/// its CSS initial value: `medium` width, `none` style, `currentColor`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BorderSide<'a> {
    /// As written: a `none` style takes the used width to zero, not this one.
    pub width: LinearLength,
    pub style: LineStyle,
    /// Colour text, still to be resolved against the element.
    pub color: &'a str,
}

/// `[<width> || <style> || <color>]`, in any order. `None` for an invalid value.
pub(crate) fn parse_border(value: &str) -> Option<BorderSide<'_>> {
    parse_line(value, LineStyle::parse)
}

/// [`parse_border`] with `parse_style` reading the style keyword, which `outline` takes
/// differently.
pub(crate) fn parse_line(value: &str, parse_style: fn(&str) -> Option<LineStyle>) -> Option<BorderSide<'_>> {
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
    Some(BorderSide {
        width: width.unwrap_or(MEDIUM),
        style: style.unwrap_or(LineStyle::None),
        color: color.unwrap_or("currentColor"),
    })
}

/// Whether CSS keeps a `border` (or side) shorthand: the grammar holds and the colour resolves.
pub(crate) fn is_valid_shorthand(value: &str, colors: &crate::color::ColorContext) -> bool {
    parse_border(value).is_some_and(|side| colors.resolve(side.color).is_some())
}

/// Whether CSS keeps a `border-color`: one to four colours that resolve.
pub(crate) fn is_valid_colors(value: &str, colors: &crate::color::ColorContext) -> bool {
    parse_color_sides(value).is_some_and(|sides| sides.iter().all(|color| colors.resolve(color).is_some()))
}

pub(crate) const MEDIUM: LinearLength = LinearLength::px(3.0);

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
pub(crate) fn parse_shorthand_width(token: &str) -> Option<LinearLength> {
    parse_width(token).filter(|width| !width.is_negative())
}

/// The px of a border or outline width, which a `%` never reaches.
pub(crate) fn resolve_width(width: &LinearLength, lengths: &LengthContext) -> gpui::Pixels {
    gpui::px(lengths.resolve(width, 0.0).max(0.0) as f32)
}

/// `border-color`'s one to four values as `[top, right, bottom, left]`, with the
/// CSS rule for the missing ones. Colour text is not validated here.
pub(crate) fn parse_color_sides(value: &str) -> Option<[&str; 4]> {
    expand_sides(&split_whitespace(value))
}

/// `border-style`'s one to four keywords as `[top, right, bottom, left]`. `None`
/// when any of them is not a line style, which invalidates the declaration.
pub(crate) fn parse_style_sides(value: &str) -> Option<[&str; 4]> {
    let tokens = split_whitespace(value);
    if tokens.iter().any(|token| LineStyle::parse(token).is_none()) {
        return None;
    }
    expand_sides(&tokens)
}

// ── Applying ────────────────────────────────────────────────────────────

use crate::color::ColorContext;
use crate::style::StyleDesc;

#[derive(Clone, Copy)]
pub(crate) enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

/// The order of `[top, right, bottom, left]` arrays.
const SIDES: [Side; 4] = [Side::Top, Side::Right, Side::Bottom, Side::Left];

fn set_width<E: gpui::Styled>(el: E, side: Side, width: gpui::Pixels) -> E {
    match side {
        Side::Top => el.border_t(width),
        Side::Right => el.border_r(width),
        Side::Bottom => el.border_b(width),
        Side::Left => el.border_l(width),
    }
}

fn set_color<E: gpui::Styled>(el: E, side: Side, color: gpui::Rgba) -> E {
    match side {
        Side::Top => el.border_top_color(color),
        Side::Right => el.border_right_color(color),
        Side::Bottom => el.border_bottom_color(color),
        Side::Left => el.border_left_color(color),
    }
}

fn set_style<E: gpui::Styled>(mut el: E, side: Side, style: gpui::BorderStyle) -> E {
    let styles = &mut el.style().border_styles;
    match side {
        Side::Top => styles.top = Some(style),
        Side::Right => styles.right = Some(style),
        Side::Bottom => styles.bottom = Some(style),
        Side::Left => styles.left = Some(style),
    }
    el
}

/// The shorthands of `style` as `(value, the sides it is for)`, in the order they apply so a
/// narrower one overrides a broader one: `border`, then `borderBlock` and `borderInline`, then
/// a side's own.
fn shorthands(style: &StyleDesc) -> [(&Option<String>, &'static [Side]); 7] {
    [
        (&style.border, &SIDES),
        (&style.border_block, &[Side::Top, Side::Bottom]),
        (&style.border_inline, &[Side::Left, Side::Right]),
        (&style.border_top, &[Side::Top]),
        (&style.border_right, &[Side::Right]),
        (&style.border_bottom, &[Side::Bottom]),
        (&style.border_left, &[Side::Left]),
    ]
}

/// A shorthand that parses and whose colour resolves. An unresolvable colour
/// invalidates the whole declaration, width and style included.
fn valid_shorthand<'a>(
    value: &'a Option<String>,
    colors: &ColorContext,
) -> Option<(BorderSide<'a>, gpui::Rgba)> {
    let parsed = parse_border(value.as_deref()?)?;
    let color = colors.resolve(parsed.color)?;
    Some((parsed, color))
}

/// The outline `style` declares, or `None` when it paints nothing. As in CSS the style is
/// `none` unless `outlineStyle` or the `outline` shorthand gives one, the width is `medium`
/// and the colour `currentColor`. A longhand that is invalid is as if it was not written.
pub(crate) fn outline(style: &StyleDesc, colors: &ColorContext, lengths: &LengthContext) -> Option<gpui::Outline> {
    let shorthand = style.outline.as_ref();
    let line = style
        .outline_style
        .as_deref()
        .and_then(LineStyle::parse_outline)
        .or(shorthand.map(|shorthand| shorthand.style))?
        .painted()?;
    let width = style
        .outline_width
        .or(shorthand.map(|shorthand| shorthand.width))
        .map_or_else(|| resolve_width(&MEDIUM, lengths), |width| resolve_width(&width, lengths));
    let color = style
        .outline_color
        .as_deref()
        .and_then(|value| colors.resolve(value))
        .or_else(|| shorthand.and_then(|shorthand| colors.resolve(&shorthand.color)))
        .unwrap_or(colors.current);
    Some(gpui::Outline {
        width,
        color: color.into(),
        offset: style
            .outline_offset
            .as_ref()
            .map_or(gpui::px(0.0), |offset| gpui::px(lengths.resolve(offset, 0.0) as f32)),
        style: line,
    })
}

/// The colour of `border` and `borderTop/Right/Bottom/Left`, in that order so a side
/// overrides the whole-element value, then `borderColor` (one to four values) and
/// the per-side colours, which override the shorthands.
pub(crate) fn apply_colors<E: gpui::Styled>(
    mut el: E,
    style: &StyleDesc,
    colors: &ColorContext,
) -> E {
    for (value, sides) in shorthands(style) {
        let Some((_, color)) = valid_shorthand(value, colors) else {
            continue;
        };
        for side in sides {
            el = set_color(el, *side, color);
        }
    }
    if let Some(sides) = style.border_color.as_deref().and_then(parse_color_sides) {
        let resolved = sides.map(|color| colors.resolve(color));
        // One invalid colour invalidates the declaration, as in CSS.
        if let [Some(top), Some(right), Some(bottom), Some(left)] = resolved {
            el = set_color(el, Side::Top, top);
            el = set_color(el, Side::Right, right);
            el = set_color(el, Side::Bottom, bottom);
            el = set_color(el, Side::Left, left);
        }
    }
    for (value, side) in [
        (&style.border_top_color, Side::Top),
        (&style.border_right_color, Side::Right),
        (&style.border_bottom_color, Side::Bottom),
        (&style.border_left_color, Side::Left),
    ] {
        if let Some(color) = value.as_deref().and_then(|value| colors.resolve(value)) {
            el = set_color(el, side, color);
        }
    }
    el
}

/// What the declarations of a style say about each side, as `[top, right, bottom, left]`.
/// `None` is a side nothing names. From weakest to strongest: the shorthands, then
/// `borderWidth` and `borderStyle` (one to four values), then the per-side longhands.
struct Declared {
    widths: [Option<LinearLength>; 4],
    styles: [Option<LineStyle>; 4],
}

fn declare(style: &StyleDesc, colors: &ColorContext) -> Declared {
    let mut declared = Declared {
        widths: [None; 4],
        styles: [None; 4],
    };
    for (value, sides) in shorthands(style) {
        let Some((parsed, _)) = valid_shorthand(value, colors) else {
            continue;
        };
        for side in sides {
            declared.widths[*side as usize] = Some(parsed.width);
            declared.styles[*side as usize] = Some(parsed.style);
        }
    }
    if let Some(widths) = &style.border_width {
        declared.widths = widths.values().map(|width| Some(*width));
    }
    if let Some(sides) = style.border_style.as_deref().and_then(parse_style_sides) {
        for (index, keyword) in sides.into_iter().enumerate() {
            declared.styles[index] = LineStyle::parse(keyword);
        }
    }
    for (value, side) in [
        (style.border_top_width, Side::Top),
        (style.border_right_width, Side::Right),
        (style.border_bottom_width, Side::Bottom),
        (style.border_left_width, Side::Left),
    ] {
        if value.is_some() {
            declared.widths[side as usize] = value;
        }
    }
    for (value, side) in [
        (&style.border_top_style, Side::Top),
        (&style.border_right_style, Side::Right),
        (&style.border_bottom_style, Side::Bottom),
        (&style.border_left_style, Side::Left),
    ] {
        if let Some(line_style) = value.as_deref().and_then(LineStyle::parse) {
            declared.styles[side as usize] = Some(line_style);
        }
    }
    declared
}

impl Declared {
    /// The used width of a side. As in CSS, a side with no style declared is `none`
    /// and takes no width whatever the width says, and a visible style with no
    /// width is `medium`.
    fn used_width(&self, side: Side, lengths: &LengthContext) -> gpui::Pixels {
        match self.styles[side as usize] {
            None | Some(LineStyle::None | LineStyle::Hidden) => gpui::px(0.0),
            Some(_) => resolve_width(&self.widths[side as usize].unwrap_or(MEDIUM), lengths),
        }
    }

    fn names(&self, side: Side) -> bool {
        self.widths[side as usize].is_some() || self.styles[side as usize].is_some()
    }
}

/// The used width of every side as `[top, right, bottom, left]`: what a box lays out
/// with and what an inline box takes along the line. `lengths` is the element's.
pub(crate) fn used_widths(style: &StyleDesc, colors: &ColorContext, lengths: &LengthContext) -> [gpui::Pixels; 4] {
    let declared = declare(style, colors);
    SIDES.map(|side| declared.used_width(side, lengths))
}

/// Whether `style` declares a border width, style or shorthand on any side.
pub(crate) fn declares_widths(style: &StyleDesc, colors: &ColorContext) -> bool {
    let declared = declare(style, colors);
    SIDES.into_iter().any(|side| declared.names(side))
}

/// One side of a box as a collapsed table reads it: what it declares, with the width it would have.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SideBorder {
    pub style: LineStyle,
    /// The used width: 0 for a side that is not visible.
    pub width: gpui::Pixels,
    /// Transparent when no colour is declared, as for a box of its own.
    pub color: gpui::Hsla,
}

/// What `style` declares for every side, as `[top, right, bottom, left]`. `lengths` is the element's.
pub(crate) fn sides(style: &StyleDesc, colors: &ColorContext, lengths: &LengthContext) -> [SideBorder; 4] {
    use gpui::Styled;
    let declared = declare(style, colors);
    let mut probe = apply_colors(gpui::div(), style, colors);
    let painted = &probe.style().border_colors;
    let color = [painted.top, painted.right, painted.bottom, painted.left];
    SIDES.map(|side| SideBorder {
        style: declared.styles[side as usize].unwrap_or(LineStyle::None),
        width: declared.used_width(side, lengths),
        color: color[side as usize].unwrap_or_else(gpui::transparent_black),
    })
}

/// The computed `border-*-width` of `side` when no longhand sets it, which is what an animation
/// starts from: 0 under a `none` or `hidden` style, else the width a shorthand gives or `medium`.
pub(crate) fn computed_width(style: &StyleDesc, colors: &ColorContext, lengths: &LengthContext, side: Side) -> f64 {
    f64::from(declare(style, colors).used_width(side, lengths))
}

/// The computed `outline-width` when no longhand sets it: 0 under a style that paints nothing,
/// else the width the shorthand gives or `medium`.
pub(crate) fn computed_outline_width(style: &StyleDesc, colors: &ColorContext, lengths: &LengthContext) -> f64 {
    outline(style, colors, lengths).map_or(0.0, |outline| f64::from(outline.width))
}

/// The border widths of the sides `style` names. A side it leaves out is not
/// touched, not set to zero: the element may bring a border of its own, such as
/// the fallback of an `<img>`.
pub(crate) fn apply_widths<E: gpui::Styled>(
    mut el: E,
    style: &StyleDesc,
    colors: &ColorContext,
    lengths: &LengthContext,
) -> E {
    let declared = declare(style, colors);
    for side in SIDES {
        if declared.names(side) {
            el = set_width(el, side, declared.used_width(side, lengths));
        }
    }
    el
}

/// The line style of every side that declares a visible one. A side with none has
/// no width, which [`apply_widths`] sees to.
pub(crate) fn apply_styles<E: gpui::Styled>(
    mut el: E,
    style: &StyleDesc,
    colors: &ColorContext,
) -> E {
    let declared = declare(style, colors);
    for side in SIDES {
        if let Some(painted) = declared.styles[side as usize].and_then(LineStyle::painted) {
            el = set_style(el, side, painted);
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
        // No style is `none`, which takes the used width to zero; the written one stays.
        assert_eq!(side("2px red").width, LinearLength::px(2.0));
        assert_eq!(side("2px red").style, LineStyle::None);
        assert_eq!(side("red").width, LinearLength::px(3.0));
    }

    #[test]
    fn none_and_zero_clear_the_border() {
        assert_eq!(side("none").style, LineStyle::None);
        assert_eq!(side("hidden").style, LineStyle::Hidden);
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
            "1px wavy red",
            "1px dotted double red",
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
        let outline = outline_of(
            r#"{"outlineWidth": "0.25em", "outlineStyle": "solid", "outlineColor": "red", "outlineOffset": "calc(1em + 2px)"}"#,
            20.0,
        )
        .unwrap();
        assert_eq!((outline.width, outline.offset), (gpui::px(5.0), gpui::px(22.0)));
        let keyword = outline_of(r#"{"outlineWidth": "thick", "outlineStyle": "solid"}"#, 20.0).unwrap();
        assert_eq!((keyword.width, keyword.offset), (gpui::px(5.0), gpui::px(0.0)));
        let negative = outline_of(
            r#"{"outlineWidth": -2, "outlineStyle": "solid", "outlineOffset": "-0.5em", "outlineColor": "red"}"#,
            20.0,
        )
        .unwrap();
        assert_eq!((negative.width, negative.offset), (gpui::px(0.0), gpui::px(-10.0)));
    }

    #[test]
    fn an_outline_is_painted_only_with_a_style_and_the_rest_is_medium_and_current_colour() {
        let lone = outline_of(r#"{"outlineStyle": "solid"}"#, 16.0).unwrap();
        let current = ColorContext::root(crate::color::Scheme::Light).current;
        assert_eq!((lone.width, lone.color), (gpui::px(3.0), current.into()));
        for none in [
            r#"{"outlineWidth": 4, "outlineColor": "red"}"#,
            r#"{"outlineWidth": 4, "outlineStyle": "none"}"#,
            r#"{"outlineWidth": 4, "outlineStyle": "hidden"}"#,
            r#"{"outlineWidth": 4, "outlineStyle": "bogus"}"#,
            r#"{"outline": "4px red"}"#,
            r#"{"outlineOffset": 4}"#,
            "{}",
        ] {
            assert_eq!(outline_of(none, 16.0), None, "{none}");
        }
    }

    #[test]
    fn the_outline_shorthand_gives_the_longhands_below_it_and_they_win() {
        let width = |json: &str| outline_of(json, 16.0).unwrap().width;
        assert_eq!(width(r#"{"outline": "5px solid red"}"#), gpui::px(5.0));
        assert_eq!(width(r#"{"outline": "5px solid red", "outlineWidth": 2}"#), gpui::px(2.0));
        assert_eq!(width(r#"{"outline": "solid"}"#), gpui::px(3.0));
        assert_eq!(outline_of(r#"{"outline": "5px solid red", "outlineStyle": "none"}"#, 16.0), None);
        assert_eq!(
            outline_of(r#"{"outline": "5px solid red", "outlineColor": "blue"}"#, 16.0).unwrap().color,
            colors_of("blue")
        );
        assert_eq!(
            outline_of(r#"{"outline": "5px solid red", "outlineOffset": 3}"#, 16.0).unwrap().color,
            colors_of("red")
        );
    }

    fn colors_of(name: &str) -> gpui::Hsla {
        ColorContext::root(crate::color::Scheme::Light).resolve(name).unwrap().into()
    }

    #[test]
    fn an_invalid_outline_colour_is_as_if_it_was_not_written() {
        let current = ColorContext::root(crate::color::Scheme::Light).current;
        let found = outline_of(r#"{"outlineStyle": "solid", "outlineColor": "not a colour"}"#, 16.0).unwrap();
        assert_eq!(found.color, current.into());
        let under_shorthand = outline_of(r#"{"outline": "solid red", "outlineColor": "not a colour"}"#, 16.0).unwrap();
        assert_eq!(under_shorthand.color, colors_of("red"));
    }

    #[test]
    fn the_outline_style_is_painted_as_the_line_style_it_names() {
        for (keyword, expected) in [
            ("solid", gpui::BorderStyle::Solid),
            ("auto", gpui::BorderStyle::Solid),
            ("dashed", gpui::BorderStyle::Dashed),
            ("dotted", gpui::BorderStyle::Dotted),
            ("double", gpui::BorderStyle::Double),
            ("groove", gpui::BorderStyle::Groove),
            ("ridge", gpui::BorderStyle::Ridge),
            ("inset", gpui::BorderStyle::Inset),
            ("outset", gpui::BorderStyle::Outset),
        ] {
            let json = format!(r#"{{"outlineStyle": "{}"}}"#, keyword.to_uppercase());
            assert_eq!(outline_of(&json, 16.0).unwrap().style, expected, "{keyword}");
        }
    }

    #[test]
    fn every_line_style_parses_in_the_shorthand_in_any_case() {
        for (keyword, expected) in [
            ("none", LineStyle::None),
            ("hidden", LineStyle::Hidden),
            ("dotted", LineStyle::Dotted),
            ("dashed", LineStyle::Dashed),
            ("solid", LineStyle::Solid),
            ("double", LineStyle::Double),
            ("groove", LineStyle::Groove),
            ("ridge", LineStyle::Ridge),
            ("inset", LineStyle::Inset),
            ("outset", LineStyle::Outset),
        ] {
            assert_eq!(side(&format!("2px {keyword} red")).style, expected, "{keyword}");
            assert_eq!(side(&format!("2px {} red", keyword.to_uppercase())).style, expected);
        }
    }

    #[test]
    fn border_style_expands_like_css() {
        assert_eq!(parse_style_sides("dotted"), Some(["dotted"; 4]));
        assert_eq!(parse_style_sides("dotted solid"), Some(["dotted", "solid", "dotted", "solid"]));
        assert_eq!(
            parse_style_sides("none Solid double"),
            Some(["none", "Solid", "double", "Solid"])
        );
        assert_eq!(
            parse_style_sides("groove ridge inset outset"),
            Some(["groove", "ridge", "inset", "outset"])
        );
    }

    #[test]
    fn border_style_with_a_bad_keyword_or_count_is_invalid() {
        for value in ["", "wavy", "solid wavy", "solid 1px", "solid solid solid solid solid"] {
            assert_eq!(parse_style_sides(value), None, "{value}");
        }
    }

    /// The border quad a div painted from the style `json` carries, `None` when nothing
    /// is painted for the border.
    fn border_quad(cx: &mut gpui::VisualTestContext, json: &str) -> Option<gpui::Quad> {
        use crate::color::{ColorContext, Scheme};
        use gpui::{point, px, size, Styled as _};

        let desc: StyleDesc = serde_json::from_str(json).expect("a style");
        let colors = ColorContext::root(Scheme::Light);
        cx.draw(point(px(0.), px(0.)), size(px(200.), px(200.)), |_, _| {
            crate::renderer::apply_styles(
                gpui::div().w(px(60.)).h(px(40.)),
                &desc,
                &colors,
                &LengthContext::initial(800.0, 600.0),
            )
        });
        cx.update(|window, _| {
            window
                .painted_quads()
                .into_iter()
                .find(|quad| quad.border_widths.any(|width| width.0 > 0.))
        })
    }

    fn styles(quad: &gpui::Quad) -> [gpui::BorderStyle; 4] {
        let styles = &quad.border_styles;
        [styles.top, styles.right, styles.bottom, styles.left]
    }

    fn widths(quad: &gpui::Quad) -> [bool; 4] {
        let widths = &quad.border_widths;
        [widths.top, widths.right, widths.bottom, widths.left].map(|width| width.0 > 0.)
    }

    #[gpui::test]
    fn a_border_shorthand_styles_every_side(cx: &mut gpui::TestAppContext) {
        use gpui::BorderStyle::*;
        let cx = cx.add_empty_window();
        for (value, expected) in [
            ("4px dotted red", Dotted),
            ("4px dashed red", Dashed),
            ("4px solid red", Solid),
            ("4px double red", Double),
            ("4px groove red", Groove),
            ("4px ridge red", Ridge),
            // Inset and outset only differ in their colours, which the scene resolves.
            ("4px inset red", Solid),
            ("4px outset red", Solid),
        ] {
            let quad = border_quad(cx, &format!(r#"{{"border":"{value}"}}"#)).expect(value);
            assert_eq!(styles(&quad), [expected; 4], "{value}");
        }
    }

    #[gpui::test]
    fn a_border_with_no_style_declares_none_and_paints_nothing(cx: &mut gpui::TestAppContext) {
        let cx = cx.add_empty_window();
        for json in [
            r#"{"border":"4px red"}"#,
            r#"{"border":"4px none red"}"#,
            r#"{"border":"4px hidden red"}"#,
            r#"{"borderWidth":4,"borderColor":"red","borderStyle":"none"}"#,
            r#"{"borderWidth":4,"borderColor":"red","borderStyle":"hidden"}"#,
            r#"{"border":"4px solid red","borderStyle":"none"}"#,
        ] {
            assert!(border_quad(cx, json).is_none(), "{json}");
        }
    }

    #[gpui::test]
    fn none_and_hidden_take_the_width_of_their_side_only(cx: &mut gpui::TestAppContext) {
        use gpui::BorderStyle::*;
        let cx = cx.add_empty_window();
        let quad = border_quad(
            cx,
            r#"{"borderWidth":4,"borderColor":"red","borderStyle":"none dashed hidden"}"#,
        )
        .expect("the left and right sides remain");
        assert_eq!(widths(&quad), [false, true, false, true]);
        assert_eq!(styles(&quad)[1], Dashed);
        assert_eq!(styles(&quad)[3], Dashed);

        let quad = border_quad(
            cx,
            r#"{"border":"4px dotted red","borderLeftStyle":"none"}"#,
        )
        .expect("the other sides remain");
        assert_eq!(widths(&quad), [true, true, true, false]);
    }

    #[gpui::test]
    fn border_style_takes_one_to_four_values_for_the_sides(cx: &mut gpui::TestAppContext) {
        use gpui::BorderStyle::*;
        let cx = cx.add_empty_window();
        for (value, expected) in [
            ("dotted", [Dotted; 4]),
            ("dotted dashed", [Dotted, Dashed, Dotted, Dashed]),
            ("dotted dashed double", [Dotted, Dashed, Double, Dashed]),
            ("dotted dashed double groove", [Dotted, Dashed, Double, Groove]),
        ] {
            let json = format!(r#"{{"borderWidth":4,"borderColor":"red","borderStyle":"{value}"}}"#);
            let quad = border_quad(cx, &json).expect(value);
            assert_eq!(styles(&quad), expected, "{value}");
        }
    }

    #[gpui::test]
    fn the_side_longhands_beat_border_style_which_beats_the_shorthands(cx: &mut gpui::TestAppContext) {
        use gpui::BorderStyle::*;
        let cx = cx.add_empty_window();
        let quad = border_quad(
            cx,
            r#"{"border":"4px solid red","borderLeft":"4px ridge red",
                "borderStyle":"dotted dotted double","borderTopStyle":"dashed"}"#,
        )
        .expect("a border");
        assert_eq!(styles(&quad), [Dashed, Dotted, Double, Dotted]);

        let quad = border_quad(
            cx,
            r#"{"border":"4px solid red","borderLeft":"4px ridge red","borderBottom":"4px double red"}"#,
        )
        .expect("a border");
        assert_eq!(styles(&quad), [Solid, Solid, Double, Ridge]);
    }

    #[gpui::test]
    fn an_invalid_style_is_ignored_and_leaves_the_one_before_it(cx: &mut gpui::TestAppContext) {
        use gpui::BorderStyle::*;
        let cx = cx.add_empty_window();
        for json in [
            r#"{"border":"4px dashed red","borderStyle":"wavy"}"#,
            r#"{"border":"4px dashed red","borderStyle":"dotted wavy"}"#,
            r#"{"border":"4px dashed red","borderTopStyle":"solid solid"}"#,
            // The colour does not resolve, so the whole shorthand is dropped, its style too.
            r#"{"border":"4px dotted not-a-colour","borderTop":"4px dashed red","borderRight":"4px dashed red",
                "borderBottom":"4px dashed red","borderLeft":"4px dashed red"}"#,
        ] {
            let quad = border_quad(cx, json).expect(json);
            assert_eq!(styles(&quad), [Dashed; 4], "{json}");
        }
    }

    #[gpui::test]
    fn an_invalid_style_leaves_no_style_declared(cx: &mut gpui::TestAppContext) {
        let cx = cx.add_empty_window();
        for json in [
            r#"{"borderWidth":4,"borderColor":"red","borderStyle":"wavy"}"#,
            r#"{"borderWidth":4,"borderColor":"red","borderStyle":"dotted wavy"}"#,
            r#"{"borderWidth":4,"borderColor":"red","borderTopStyle":"solid solid"}"#,
            r#"{"border":"4px dotted not-a-colour","borderWidth":4,"borderColor":"red"}"#,
        ] {
            assert!(border_quad(cx, json).is_none(), "{json}");
        }
    }

    #[gpui::test]
    fn a_style_alone_paints_with_the_longhand_width_and_colour(cx: &mut gpui::TestAppContext) {
        use gpui::BorderStyle::*;
        let cx = cx.add_empty_window();
        let quad = border_quad(cx, r#"{"borderWidth":4,"borderColor":"red","borderStyle":"dashed"}"#)
            .expect("a border");
        assert_eq!(styles(&quad), [Dashed; 4]);
        assert_eq!(widths(&quad), [true; 4]);
    }

    fn used(json: &str, font_size: f64) -> [f32; 4] {
        let style: StyleDesc = serde_json::from_str(json).unwrap();
        let lengths = LengthContext::initial(800.0, 600.0).descend(Some(&LinearLength::px(font_size)), true);
        used_widths(&style, &ColorContext::root(crate::color::Scheme::Light), &lengths).map(f32::from)
    }

    #[test]
    fn a_side_with_no_style_declared_has_no_width() {
        for json in [
            r#"{"borderWidth":4}"#,
            r#"{"borderWidth":4,"borderColor":"red"}"#,
            r#"{"borderTopWidth":4,"borderLeftWidth":"2em"}"#,
            r#"{"border":"4px red"}"#,
            r#"{"borderTop":"4px","borderBottom":"thick"}"#,
            r#"{"borderWidth":4,"borderStyle":"none"}"#,
            r#"{"borderWidth":4,"borderStyle":"hidden"}"#,
        ] {
            assert_eq!(used(json, 16.0), [0.0; 4], "{json}");
        }
    }

    #[test]
    fn a_visible_style_gives_the_width_the_declarations_name() {
        assert_eq!(used(r#"{"borderWidth":4,"borderStyle":"solid"}"#, 16.0), [4.0; 4]);
        assert_eq!(used(r#"{"border":"2px solid red"}"#, 16.0), [2.0; 4]);
        assert_eq!(used(r#"{"borderWidth":"0.5em","borderStyle":"dashed"}"#, 20.0), [10.0; 4]);
        // The style can come after the shorthand that left it out, as in CSS.
        assert_eq!(used(r#"{"border":"2px red","borderStyle":"solid"}"#, 16.0), [2.0; 4]);
    }

    #[test]
    fn border_block_is_the_top_and_bottom_and_border_inline_the_left_and_right() {
        assert_eq!(used(r#"{"borderBlock":"3px solid red"}"#, 16.0), [3.0, 0.0, 3.0, 0.0]);
        assert_eq!(used(r#"{"borderInline":"solid 5px"}"#, 16.0), [0.0, 5.0, 0.0, 5.0]);
        assert_eq!(used(r#"{"borderBlock":"3px solid","borderInline":"1px dashed"}"#, 16.0), [3.0, 1.0, 3.0, 1.0]);
        // No style is `none`, as for `border`.
        assert_eq!(used(r#"{"borderBlock":"3px red"}"#, 16.0), [0.0; 4]);
    }

    #[test]
    fn a_narrower_shorthand_beats_a_broader_one_and_a_longhand_beats_both() {
        let find = |json: &str| used(json, 16.0);
        assert_eq!(find(r#"{"border":"1px solid red","borderBlock":"4px solid red"}"#), [4.0, 1.0, 4.0, 1.0]);
        assert_eq!(find(r#"{"borderBlock":"4px solid red","borderTop":"2px solid red"}"#), [2.0, 0.0, 4.0, 0.0]);
        assert_eq!(find(r#"{"borderBlock":"4px solid red","borderTopWidth":7}"#), [7.0, 0.0, 4.0, 0.0]);
        // A block shorthand that leaves the style out resets the style `border` gave the sides.
        assert_eq!(find(r#"{"border":"1px solid red","borderInline":"2px red"}"#), [1.0, 0.0, 1.0, 0.0]);
    }

    #[test]
    fn each_side_needs_its_own_style() {
        assert_eq!(
            used(r#"{"borderBottomWidth":2,"borderBottomStyle":"solid"}"#, 16.0),
            [0.0, 0.0, 2.0, 0.0]
        );
        assert_eq!(
            used(r#"{"borderWidth":4,"borderStyle":"solid none"}"#, 16.0),
            [4.0, 0.0, 4.0, 0.0]
        );
        assert_eq!(
            used(r#"{"borderWidth":4,"borderLeftStyle":"dotted"}"#, 16.0),
            [0.0, 0.0, 0.0, 4.0]
        );
        // A side shorthand without a style resets the style `border` gave the side.
        assert_eq!(
            used(r#"{"border":"4px solid red","borderLeft":"4px red"}"#, 16.0),
            [4.0, 4.0, 4.0, 0.0]
        );
    }

    #[test]
    fn a_visible_style_with_no_width_is_medium() {
        assert_eq!(used(r#"{"borderStyle":"solid"}"#, 16.0), [3.0; 4]);
        assert_eq!(used(r#"{"borderColor":"red","borderStyle":"dashed double"}"#, 16.0), [3.0; 4]);
        assert_eq!(used(r#"{"borderTopStyle":"solid"}"#, 16.0), [3.0, 0.0, 0.0, 0.0]);
        assert_eq!(used(r#"{"border":"solid"}"#, 16.0), [3.0; 4]);
        // The shorthand named no width, so it is the initial one for the sides it covers, unless a
        // width is written after it.
        assert_eq!(used(r#"{"border":"solid","borderWidth":6}"#, 16.0), [6.0; 4]);
        assert_eq!(used(r#"{"borderWidth":6,"border":"solid"}"#, 16.0), [3.0; 4]);
        assert_eq!(used(r#"{"borderTop":"solid","borderWidth":6}"#, 16.0), [6.0, 0.0, 0.0, 0.0]);
        assert_eq!(used(r#"{"borderWidth":6,"borderTop":"solid"}"#, 16.0), [3.0, 0.0, 0.0, 0.0]);
        assert_eq!(used(r#"{"borderTopWidth":0,"borderStyle":"solid"}"#, 16.0), [0.0, 3.0, 3.0, 3.0]);
    }

    #[gpui::test]
    fn a_width_alone_paints_nothing_and_a_style_gives_it_a_quad(cx: &mut gpui::TestAppContext) {
        use gpui::BorderStyle::*;
        let cx = cx.add_empty_window();
        for json in [
            r#"{"borderWidth":4}"#,
            r#"{"borderWidth":4,"borderColor":"red"}"#,
            r#"{"borderTopWidth":4,"borderColor":"red"}"#,
            r#"{"border":"2px red"}"#,
            r#"{"borderTop":"2px red"}"#,
        ] {
            assert!(border_quad(cx, json).is_none(), "{json}");
        }
        let quad = border_quad(cx, r#"{"borderWidth":4,"borderColor":"red","borderStyle":"solid"}"#).expect("a border");
        assert_eq!((widths(&quad), styles(&quad)), ([true; 4], [Solid; 4]));
    }

    #[gpui::test]
    fn only_the_side_that_names_a_style_paints(cx: &mut gpui::TestAppContext) {
        let cx = cx.add_empty_window();
        let quad = border_quad(
            cx,
            r#"{"borderBottomWidth":2,"borderBottomStyle":"solid","borderColor":"red"}"#,
        )
        .expect("the bottom border");
        assert_eq!(widths(&quad), [false, false, true, false]);

        let quad = border_quad(cx, r#"{"borderColor":"red","borderTopStyle":"solid"}"#).expect("a medium top border");
        assert_eq!(widths(&quad), [true, false, false, false]);
    }

    #[gpui::test]
    fn a_side_that_declares_nothing_keeps_the_border_the_element_came_with(cx: &mut gpui::TestAppContext) {
        use gpui::{point, px, size, Styled as _};
        let cx = cx.add_empty_window();
        let desc: StyleDesc = serde_json::from_str(r#"{"borderTopWidth":0,"opacity":1}"#).unwrap();
        let colors = ColorContext::root(crate::color::Scheme::Light);
        cx.draw(point(px(0.), px(0.)), size(px(200.), px(200.)), |_, _| {
            crate::renderer::apply_styles(
                gpui::div().w(px(60.)).h(px(40.)).border(px(1.)).border_color(gpui::red()),
                &desc,
                &colors,
                &LengthContext::initial(800.0, 600.0),
            )
        });
        let quad = cx
            .update(|window, _| window.painted_quads().into_iter().find(|quad| quad.border_widths.any(|width| width.0 > 0.)))
            .expect("the element's own border");
        assert_eq!(widths(&quad), [false, true, true, true]);
    }
}
