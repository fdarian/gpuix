//! The CSS `border` shorthands, the one-to-four value `border-color` and `border-style`,
//! and the lengths of a border or an outline.

use super::dimension;
use super::length::{LengthContext, LinearLength};
use super::syntax::split_whitespace;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineStyle {
    /// `none` and `hidden`: the border is not drawn and takes no width.
    None,
    Dotted,
    Dashed,
    Solid,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

impl LineStyle {
    /// A `<line-style>` keyword, in any case.
    pub(crate) fn parse(token: &str) -> Option<Self> {
        match token.to_ascii_lowercase().as_str() {
            "none" | "hidden" => Some(Self::None),
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

    /// What gpui paints, `None` for a style that takes no width.
    fn painted(self) -> Option<gpui::BorderStyle> {
        match self {
            Self::None => None,
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
    /// Zero when the style is `none`.
    pub width: LinearLength,
    pub style: LineStyle,
    /// Colour text, still to be resolved against the element.
    pub color: &'a str,
}

/// `[<width> || <style> || <color>]`, in any order. `None` for an invalid value.
pub(crate) fn parse_border(value: &str) -> Option<BorderSide<'_>> {
    let mut width = None;
    let mut style = None;
    let mut color = None;
    for token in split_whitespace(value) {
        if let Some(parsed) = parse_shorthand_width(token) {
            if width.replace(parsed).is_some() {
                return None;
            }
        } else if let Some(parsed) = LineStyle::parse(token) {
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

/// The CSS rule for the sides a one to four value property leaves out, as
/// `[top, right, bottom, left]`.
fn expand_sides<T: Copy>(values: &[T]) -> Option<[T; 4]> {
    match *values {
        [all] => Some([all; 4]),
        [vertical, horizontal] => Some([vertical, horizontal, vertical, horizontal]),
        [top, horizontal, bottom] => Some([top, horizontal, bottom, horizontal]),
        [top, right, bottom, left] => Some([top, right, bottom, left]),
        _ => None,
    }
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
enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

/// The order of `[top, right, bottom, left]` arrays.
const SIDES: [Side; 4] = [Side::Top, Side::Right, Side::Bottom, Side::Left];

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

/// The shorthands of `style` as `(value, the side it is for)`, `None` for `border`, in the
/// order they apply so a side overrides the whole-element value.
fn shorthands(style: &StyleDesc) -> [(&Option<String>, Option<Side>); 5] {
    [
        (&style.border, None),
        (&style.border_top, Some(Side::Top)),
        (&style.border_right, Some(Side::Right)),
        (&style.border_bottom, Some(Side::Bottom)),
        (&style.border_left, Some(Side::Left)),
    ]
}

/// A shorthand that parses and whose colour resolves. An unresolvable colour
/// invalidates the whole declaration, width and style included.
fn valid_shorthand<'a>(
    value: &'a Option<String>,
    colors: &ColorContext,
) -> Option<(BorderSide<'a>, Option<gpui::Rgba>)> {
    let parsed = parse_border(value.as_deref()?)?;
    match parsed.style {
        LineStyle::None => Some((parsed, None)),
        _ => {
            let color = colors.resolve(parsed.color)?;
            Some((parsed, Some(color)))
        }
    }
}

/// `border` and `borderTop/Right/Bottom/Left`, in that order, so a side
/// overrides the whole-element value. Call before the width and colour
/// longhands, which override these. `lengths` is the element's, which a width in
/// `em` resolves against. The line style is [`apply_styles`]'s.
pub(crate) fn apply_shorthands<E: gpui::Styled>(
    mut el: E,
    style: &StyleDesc,
    colors: &ColorContext,
    lengths: &LengthContext,
) -> E {
    for (value, side) in shorthands(style) {
        let Some((parsed, color)) = valid_shorthand(value, colors) else {
            continue;
        };
        el = set_width(el, side, resolve_width(&parsed.width, lengths));
        if let Some(color) = color {
            el = set_color(el, side, color);
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

/// The line style each side declares as `[top, right, bottom, left]`, `None` for a
/// side nothing names. From weakest to strongest: the shorthands, `borderStyle`
/// (one to four values) and the per-side longhands.
fn declared_styles(style: &StyleDesc, colors: &ColorContext) -> [Option<LineStyle>; 4] {
    let mut declared = [None; 4];
    for (value, only) in shorthands(style) {
        let Some((parsed, _)) = valid_shorthand(value, colors) else {
            continue;
        };
        match only {
            None => declared = [Some(parsed.style); 4],
            Some(side) => declared[side as usize] = Some(parsed.style),
        }
    }
    if let Some(sides) = style.border_style.as_deref().and_then(parse_style_sides) {
        for (index, keyword) in sides.into_iter().enumerate() {
            declared[index] = LineStyle::parse(keyword);
        }
    }
    for (value, side) in [
        (&style.border_top_style, Side::Top),
        (&style.border_right_style, Side::Right),
        (&style.border_bottom_style, Side::Bottom),
        (&style.border_left_style, Side::Left),
    ] {
        if let Some(line_style) = value.as_deref().and_then(LineStyle::parse) {
            declared[side as usize] = Some(line_style);
        }
    }
    declared
}

/// The line style of every side. A side that is `none` or `hidden` has no width,
/// whatever the width longhands say, so call after them. A side no declaration
/// names stays solid: unlike CSS there is no `border-style: none` initial value
/// to start from.
pub(crate) fn apply_styles<E: gpui::Styled>(
    mut el: E,
    style: &StyleDesc,
    colors: &ColorContext,
) -> E {
    for (side, declared) in SIDES.into_iter().zip(declared_styles(style, colors)) {
        let Some(declared) = declared else {
            continue;
        };
        el = match declared.painted() {
            Some(painted) => set_style(el, side, painted),
            None => set_width(el, Some(side), gpui::px(0.0)),
        };
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

    #[test]
    fn every_line_style_parses_in_the_shorthand_in_any_case() {
        for (keyword, expected) in [
            ("none", LineStyle::None),
            ("hidden", LineStyle::None),
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
        assert_eq!(side("2px hidden red").width, LinearLength::px(0.0));
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
    fn an_invalid_style_leaves_the_border_as_it_was(cx: &mut gpui::TestAppContext) {
        use gpui::BorderStyle::*;
        let cx = cx.add_empty_window();
        for json in [
            r#"{"borderWidth":4,"borderColor":"red","borderStyle":"wavy"}"#,
            r#"{"borderWidth":4,"borderColor":"red","borderStyle":"dotted wavy"}"#,
            r#"{"borderWidth":4,"borderColor":"red","borderTopStyle":"solid solid"}"#,
            // The colour does not resolve, so the whole shorthand is dropped, its style too.
            r#"{"border":"4px dotted not-a-colour","borderWidth":4,"borderColor":"red"}"#,
        ] {
            let quad = border_quad(cx, json).expect(json);
            assert_eq!(styles(&quad), [Solid; 4], "{json}");
        }
    }

    #[gpui::test]
    fn a_style_alone_still_paints_the_longhand_width_and_colour(cx: &mut gpui::TestAppContext) {
        use gpui::BorderStyle::*;
        let cx = cx.add_empty_window();
        let quad = border_quad(cx, r#"{"borderWidth":4,"borderColor":"red","borderStyle":"dashed"}"#)
            .expect("a border");
        assert_eq!(styles(&quad), [Dashed; 4]);
        assert_eq!(widths(&quad), [true; 4]);

        let quad = border_quad(cx, r#"{"borderWidth":4,"borderColor":"red"}"#).expect("a border");
        assert_eq!(styles(&quad), [Solid; 4]);
    }
}
