use std::borrow::Cow;

use crate::color::{ColorContext, Scheme};
use crate::css::background::{self, Background, Property};
use crate::css::border::{parse_border, parse_color_sides, LineStyle};
use crate::css::mask::Mask;
use crate::css::{clip_path, filter};
use crate::style::{BackgroundValue, StyleDesc};

/// Takes `$base`'s value for every field `$over` leaves unset. Written as a
/// struct literal so a field added to `StyleDesc` and missing from this list
/// fails to compile instead of silently never taking part in state styles.
macro_rules! overlay_fields {
    ($base:expr, $over:expr; $($field:ident),+ $(,)?) => {
        StyleDesc {
            $($field: $over.$field.clone().or_else(|| $base.$field.clone()),)+
            hover: None,
            active: None,
            group_hover: None,
            group_active: None,
        }
    };
}

/// Which pointer states apply to an element: its own, and those of the nearest
/// ancestor `group` that its `groupHover` / `groupActive` follow.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PointerStates {
    pub hovered: bool,
    pub active: bool,
    pub group_hovered: bool,
    pub group_active: bool,
}

impl PointerStates {
    pub fn any(self) -> bool {
        self.hovered || self.active || self.group_hovered || self.group_active
    }
}

/// `style` with the `hover` and then the `active` refinement folded in, for an
/// element that is `hovered` and/or `active`, and both refinements cleared.
///
/// A field the state style sets replaces the base value, an unset one keeps
/// it, and `active` is applied last so it wins over `hover`. A state style's
/// own `hover` / `active` are ignored.
pub fn resolve_state_styles(style: &StyleDesc, hovered: bool, active: bool) -> StyleDesc {
    resolve_pointer_states(
        style,
        PointerStates {
            hovered,
            active,
            ..Default::default()
        },
    )
}

/// `style` with the refinements of `states` folded in and every state style
/// cleared. Later wins: `groupHover`, `hover`, `groupActive`, `active`. A
/// group refinement is one more variant of the element, so the element's own
/// `hover` beats a `groupHover` that sets the same field (as in Tailwind).
pub fn resolve_pointer_states(style: &StyleDesc, states: PointerStates) -> StyleDesc {
    let mut resolved = overlay(style, &StyleDesc::default());
    let layers = [
        (states.group_hovered, style.group_hover.as_deref(), true),
        (states.hovered, style.hover.as_deref(), false),
        (states.group_active, style.group_active.as_deref(), true),
        (states.active, style.active.as_deref(), false),
    ];
    for (applies, refinement, names_group) in layers {
        let Some(refinement) = refinement.filter(|_| applies) else {
            continue;
        };
        resolved = if names_group {
            // Its `group` names the group to follow, not one this element declares.
            let mut refinement = refinement.clone();
            refinement.group = None;
            overlay(&resolved, &refinement)
        } else {
            overlay(&resolved, refinement)
        };
    }
    resolved
}

fn overlay(base: &StyleDesc, over: &StyleDesc) -> StyleDesc {
    let over = without_invalid_values(over);
    let over = over.as_ref();
    let mut base = base.clone();
    drop_shadowed_longhands(&mut base, over);
    overlay_fields!(
        base, over;
        display, visibility,
        flex_direction, flex_wrap, flex_grow, flex_shrink, flex_basis, align_items, align_self,
        align_content, justify_content, gap, row_gap, column_gap, grid_template_columns,
        grid_template_rows, grid_column_min, grid_row_min,
        width, height, min_width, min_height, max_width, max_height,
        padding, padding_top, padding_right, padding_bottom, padding_left,
        margin, margin_top, margin_right, margin_bottom, margin_left,
        position, top, right, bottom, left,
        background, background_color, background_image, background_size, background_position,
        background_repeat, background_origin, background_clip, color, color_scheme, opacity,
        filter, backdrop_filter, mask_image, mask_size, mask_position, mask_repeat, mask_origin,
        mask_clip, mask, clip_path, font_style,
        border, border_top, border_right, border_bottom, border_left, border_top_color,
        border_right_color, border_bottom_color, border_left_color,
        border_width, border_top_width, border_right_width, border_bottom_width,
        border_left_width, border_color, border_radius, border_top_left_radius,
        border_top_right_radius, border_bottom_left_radius, border_bottom_right_radius,
        box_shadow, border_style, aspect_ratio, outline_width, outline_color, outline_offset,
        font_size, font_family, font_weight, font_variant_numeric, font_feature_settings, text_align, line_height, white_space, text_overflow,
        line_clamp, text_decoration, text_decoration_line, text_decoration_style,
        text_decoration_color,
        overflow, overflow_x, overflow_y, scrollbar_color, scrollbar_width,
        cursor, pointer_events, user_select, selection_color,
        transition, transition_property, transition_duration, transition_timing_function,
        transition_delay, interpolate_size,
        animation, animation_name, animation_duration, animation_timing_function,
        animation_delay, animation_iteration_count, animation_direction, animation_fill_mode,
        animation_play_state,
        focus_visible, focus_within, keyboard_focus_dim, group,
        order, justify_items, justify_self, grid_auto_columns, grid_auto_rows, grid_auto_flow,
        grid_template_areas, grid_area, grid_column, grid_row, grid_column_start,
        grid_column_end, grid_row_start, grid_row_end, z_index, overscroll_behavior,
        overscroll_behavior_x, overscroll_behavior_y, scroll_behavior,
    )
}

/// `over` without the values CSS drops as invalid when it parses the
/// declaration, so the base value still applies instead of being replaced by
/// one that paints nothing. A shorthand that is invalid must go before
/// `drop_shadowed_longhands` too, or it would reset the longhands it covers.
///
/// Only the properties whose value is a CSS string are checked. Whether a
/// colour resolves does not depend on the element, so the root context stands
/// in for it. Parsing here is not reused for painting: the style holds the text.
fn without_invalid_values(over: &StyleDesc) -> Cow<'_, StyleDesc> {
    let colors = ColorContext::root(Scheme::Light);
    let mut cleaned: Option<StyleDesc> = None;
    macro_rules! drop_if_invalid {
        ($($field:ident: |$value:ident| $valid:expr),+ $(,)?) => {$(
            if over.$field.as_deref().is_some_and(|$value| !$valid) {
                cleaned.get_or_insert_with(|| over.clone()).$field = None;
            }
        )+};
    }
    drop_if_invalid!(
        filter: |value| filter::parse_resolved(value, &colors).is_some(),
        backdrop_filter: |value| filter::parse_resolved(value, &colors).is_some(),
        clip_path: |value| clip_path::parse(value).is_some(),
        mask: |value| background::is_valid_shorthand::<Mask>(value, &colors),
        mask_image: |value| background::is_valid_image_list(value, &colors),
        mask_size: |value| background::is_valid_list::<Mask>(Property::Size, value),
        mask_position: |value| background::is_valid_list::<Mask>(Property::Position, value),
        mask_repeat: |value| background::is_valid_list::<Mask>(Property::Repeat, value),
        mask_origin: |value| background::is_valid_list::<Mask>(Property::Origin, value),
        mask_clip: |value| background::is_valid_list::<Mask>(Property::Clip, value),
        background_color: |value| colors.resolve(value).is_some(),
        background_image: |value| background::is_valid_image_list(value, &colors),
        background_size: |value| background::is_valid_list::<Background>(Property::Size, value),
        background_position: |value| background::is_valid_list::<Background>(Property::Position, value),
        background_repeat: |value| background::is_valid_list::<Background>(Property::Repeat, value),
        background_origin: |value| background::is_valid_list::<Background>(Property::Origin, value),
        background_clip: |value| background::is_valid_list::<Background>(Property::Clip, value),
    );
    if let Some(value) = &over.background {
        let valid = match value {
            // A plain colour, or the shorthand with layers.
            BackgroundValue::Color(text) => {
                colors.resolve(text).is_some() || background::is_valid_shorthand::<Background>(text, &colors)
            }
            BackgroundValue::Gradient(_) => value.resolve(&colors).is_some(),
        };
        if !valid {
            cleaned.get_or_insert_with(|| over.clone()).background = None;
        }
    }
    cleaned.map_or(Cow::Borrowed(over), Cow::Owned)
}

/// The border all-sides longhands (`borderWidth`, `borderColor`, `borderStyle`)
/// are applied after every side shorthand, so a base one would beat a state's
/// `borderTop` on the side it covers. In CSS the state's shorthand is the later
/// declaration and resets that side only, so the base value is pushed down to
/// per-side longhands for the sides `over` does not cover, where it can no
/// longer reach the covered one.
///
/// Must run before the drops below: an all-sides value `over` sets itself
/// drops these per-side values again, as it should.
fn split_border_all_sides(base: &mut StyleDesc, over: &StyleDesc) {
    let sides = [
        (over.border_top.as_deref(), &mut base.border_top_width, &mut base.border_top_color),
        (over.border_right.as_deref(), &mut base.border_right_width, &mut base.border_right_color),
        (over.border_bottom.as_deref(), &mut base.border_bottom_width, &mut base.border_bottom_color),
        (over.border_left.as_deref(), &mut base.border_left_width, &mut base.border_left_color),
    ];
    if sides.iter().all(|(shorthand, ..)| shorthand.is_none()) {
        return;
    }
    let all_width = base.border_width.take();
    // An invalid `borderColor` paints nothing, so it spreads nothing.
    let all_colors = base
        .border_color
        .take()
        .and_then(|value| parse_color_sides(&value).map(|sides| sides.map(str::to_owned)));
    let mut resets_style = false;
    for (index, (shorthand, width, color)) in sides.into_iter().enumerate() {
        let Some(shorthand) = shorthand else {
            // A side's own longhand already beats the all-sides value.
            if let Some(all_width) = all_width {
                width.get_or_insert(all_width);
            }
            if let Some(all_colors) = &all_colors {
                color.get_or_insert_with(|| all_colors[index].clone());
            }
            continue;
        };
        resets_style |= parse_border(shorthand).is_some_and(|parsed| parsed.style != LineStyle::None);
    }
    // gpui keeps one line style per element, so it cannot be spread per side.
    // The state's shorthand is the later declaration, and the last drawn style
    // wins, as `apply_shorthands` documents.
    if resets_style {
        base.border_style = None;
    }
}

/// A shorthand in `over` resets the longhands it covers, as a later CSS
/// declaration does. `apply_styles` reads longhands after their shorthand, so a
/// base longhand would otherwise beat a state style's shorthand.
fn drop_shadowed_longhands(base: &mut StyleDesc, over: &StyleDesc) {
    split_border_all_sides(base, over);
    if over.gap.is_some() {
        base.row_gap = None;
        base.column_gap = None;
    }
    if over.padding.is_some() {
        base.padding_top = None;
        base.padding_right = None;
        base.padding_bottom = None;
        base.padding_left = None;
    }
    if over.margin.is_some() {
        base.margin_top = None;
        base.margin_right = None;
        base.margin_bottom = None;
        base.margin_left = None;
    }
    if over.border_width.is_some() {
        base.border_top_width = None;
        base.border_right_width = None;
        base.border_bottom_width = None;
        base.border_left_width = None;
    }
    if over.border_radius.is_some() {
        base.border_top_left_radius = None;
        base.border_top_right_radius = None;
        base.border_bottom_left_radius = None;
        base.border_bottom_right_radius = None;
    }
    if over.transition.is_some() {
        base.transition_property = None;
        base.transition_duration = None;
        base.transition_timing_function = None;
        base.transition_delay = None;
    }
    if over.animation.is_some() {
        base.animation_name = None;
        base.animation_duration = None;
        base.animation_timing_function = None;
        base.animation_delay = None;
        base.animation_iteration_count = None;
        base.animation_direction = None;
        base.animation_fill_mode = None;
        base.animation_play_state = None;
    }
    if over.overflow.is_some() {
        base.overflow_x = None;
        base.overflow_y = None;
    }
    if over.overscroll_behavior.is_some() {
        base.overscroll_behavior_x = None;
        base.overscroll_behavior_y = None;
    }
    // `gridArea` covers both axes, `gridColumn` / `gridRow` one each, and the
    // start and end longhands a single line.
    if over.grid_area.is_some() {
        base.grid_column = None;
        base.grid_row = None;
    }
    if over.grid_area.is_some() || over.grid_column.is_some() {
        base.grid_column_start = None;
        base.grid_column_end = None;
    }
    if over.grid_area.is_some() || over.grid_row.is_some() {
        base.grid_row_start = None;
        base.grid_row_end = None;
    }
    // `border` covers every side's width, colour and style, and a side
    // shorthand its own width and colour. The width and colour longhands win
    // over the shorthands, so a base longhand would beat a state's shorthand.
    if over.border.is_some() {
        base.border_top = None;
        base.border_right = None;
        base.border_bottom = None;
        base.border_left = None;
        base.border_width = None;
        base.border_color = None;
        base.border_style = None;
    }
    if over.border.is_some() || over.border_width.is_some() {
        base.border_top_width = None;
        base.border_right_width = None;
        base.border_bottom_width = None;
        base.border_left_width = None;
    }
    if over.border.is_some() || over.border_color.is_some() {
        base.border_top_color = None;
        base.border_right_color = None;
        base.border_bottom_color = None;
        base.border_left_color = None;
    }
    if over.border_top.is_some() {
        base.border_top_width = None;
        base.border_top_color = None;
    }
    if over.border_right.is_some() {
        base.border_right_width = None;
        base.border_right_color = None;
    }
    if over.border_bottom.is_some() {
        base.border_bottom_width = None;
        base.border_bottom_color = None;
    }
    if over.border_left.is_some() {
        base.border_left_width = None;
        base.border_left_color = None;
    }
    // The `mask` shorthand resets every layer longhand, as `background` does.
    if over.mask.is_some() {
        base.mask_image = None;
        base.mask_size = None;
        base.mask_position = None;
        base.mask_repeat = None;
        base.mask_origin = None;
        base.mask_clip = None;
    }
    // `background_color` wins over `background` when both are set, so a base
    // colour would beat a state's shorthand. A state's colour alone leaves the
    // base shorthand: its layers still paint, as under a `background-color`.
    // The `background` shorthand also resets every layer longhand.
    if over.background.is_some() {
        base.background_color = None;
        base.background_image = None;
        base.background_size = None;
        base.background_position = None;
        base.background_repeat = None;
        base.background_origin = None;
        base.background_clip = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{BackgroundValue, DimensionValue};

    fn style(json: &str) -> StyleDesc {
        serde_json::from_str(json).unwrap()
    }

    fn button() -> StyleDesc {
        style(
            r##"{
                "backgroundColor": "#111111",
                "color": "#ffffff",
                "padding": 8,
                "hover": { "backgroundColor": "#222222", "cursor": "pointer" },
                "active": { "backgroundColor": "#333333", "opacity": 0.5 }
            }"##,
        )
    }

    #[test]
    fn at_rest_returns_the_base_without_state_styles() {
        let resolved = resolve_state_styles(&button(), false, false);
        assert_eq!(resolved.background_color.as_deref(), Some("#111111"));
        assert_eq!(resolved.cursor, None);
        assert_eq!(resolved.opacity, None);
        assert_eq!(resolved.hover, None);
        assert_eq!(resolved.active, None);
    }

    #[test]
    fn hover_overrides_set_fields_and_keeps_the_rest() {
        let resolved = resolve_state_styles(&button(), true, false);
        assert_eq!(resolved.background_color.as_deref(), Some("#222222"));
        assert_eq!(resolved.cursor.as_deref(), Some("pointer"));
        assert_eq!(resolved.color.as_deref(), Some("#ffffff"));
        assert_eq!(resolved.padding, Some(8.0));
        assert_eq!(resolved.opacity, None);
    }

    #[test]
    fn active_overrides_hover() {
        let resolved = resolve_state_styles(&button(), true, true);
        assert_eq!(resolved.background_color.as_deref(), Some("#333333"));
        assert_eq!(resolved.opacity, Some(0.5));
        assert_eq!(resolved.cursor.as_deref(), Some("pointer"), "hover fields active leaves unset survive");
    }

    #[test]
    fn active_applies_without_hover() {
        let resolved = resolve_state_styles(&button(), false, true);
        assert_eq!(resolved.background_color.as_deref(), Some("#333333"));
        assert_eq!(resolved.cursor, None);
    }

    #[test]
    fn clears_hover_and_active_in_every_state() {
        for (hovered, active) in [(false, false), (true, false), (false, true), (true, true)] {
            let resolved = resolve_state_styles(&button(), hovered, active);
            assert_eq!(resolved.hover, None);
            assert_eq!(resolved.active, None);
        }
    }

    #[test]
    fn does_not_change_its_input() {
        let base = button();
        let before = base.clone();
        resolve_state_styles(&base, true, true);
        assert_eq!(base, before);
    }

    #[test]
    fn style_without_state_styles_is_returned_unchanged() {
        let base = style(r##"{ "color": "#fff", "width": "50%" }"##);
        assert_eq!(resolve_state_styles(&base, true, true), base);
    }

    #[test]
    fn nested_state_styles_inside_a_state_style_are_ignored() {
        let base = style(
            r##"{ "hover": { "color": "red", "hover": { "color": "blue" }, "active": { "color": "green" } } }"##,
        );
        let resolved = resolve_state_styles(&base, true, true);
        assert_eq!(resolved.color.as_deref(), Some("red"));
        assert_eq!(resolved.hover, None);
        assert_eq!(resolved.active, None);
    }

    #[test]
    fn keeps_dimension_kinds() {
        let base = style(r##"{ "width": 10, "hover": { "width": "50%" } }"##);
        assert_eq!(
            resolve_state_styles(&base, false, false).width,
            Some(DimensionValue::Pixels(10.0))
        );
        assert_eq!(
            resolve_state_styles(&base, true, false).width,
            Some(DimensionValue::Percentage(0.5))
        );
    }

    #[test]
    fn state_shorthand_replaces_base_longhands() {
        let base = style(
            r##"{
                "paddingTop": 4, "marginLeft": 2, "borderTopWidth": 1, "borderTopLeftRadius": 3,
                "rowGap": 5, "overflowX": "hidden",
                "hover": { "padding": 10, "margin": 6, "borderWidth": 2, "borderRadius": 9, "gap": 7, "overflow": "visible" }
            }"##,
        );
        let at_rest = resolve_state_styles(&base, false, false);
        assert_eq!(at_rest.padding_top, Some(4.0));

        let hovered = resolve_state_styles(&base, true, false);
        assert_eq!(hovered.padding, Some(10.0));
        assert_eq!(hovered.padding_top, None);
        assert_eq!(hovered.margin_left, None);
        assert_eq!(hovered.border_top_width, None);
        assert_eq!(hovered.border_top_left_radius, None);
        assert_eq!(hovered.row_gap, None);
        assert_eq!(hovered.overflow_x, None);
    }

    #[test]
    fn state_longhand_beats_state_shorthand_and_base_shorthand() {
        let base = style(
            r##"{ "padding": 8, "hover": { "paddingTop": 1 }, "active": { "padding": 3, "paddingTop": 2 } }"##,
        );
        let hovered = resolve_state_styles(&base, true, false);
        assert_eq!((hovered.padding, hovered.padding_top), (Some(8.0), Some(1.0)));
        let pressed = resolve_state_styles(&base, true, true);
        assert_eq!((pressed.padding, pressed.padding_top), (Some(3.0), Some(2.0)));
    }

    #[test]
    fn a_state_transition_replaces_the_base_transition_and_its_longhands() {
        let base = style(
            r##"{
                "transition": "opacity 1s", "transitionDuration": "2s",
                "hover": { "transition": "color 150ms" },
                "active": { "transitionDelay": "50ms" }
            }"##,
        );
        let at_rest = resolve_state_styles(&base, false, false);
        assert_eq!(at_rest.transition_duration.as_deref(), Some("2s"));

        let hovered = resolve_state_styles(&base, true, false);
        assert_eq!(hovered.transition.as_deref(), Some("color 150ms"));
        assert_eq!(hovered.transition_duration, None);

        let pressed = resolve_state_styles(&base, true, true);
        assert_eq!(pressed.transition.as_deref(), Some("color 150ms"));
        assert_eq!(pressed.transition_delay.as_deref(), Some("50ms"));
    }

    #[test]
    fn a_state_animation_replaces_the_base_animation_and_its_longhands() {
        let base = style(
            r##"{
                "animation": "spin 1s", "animationDuration": "2s",
                "hover": { "animation": "pulse 150ms" },
                "active": { "animationPlayState": "paused" }
            }"##,
        );
        let at_rest = resolve_state_styles(&base, false, false);
        assert_eq!(at_rest.animation_duration.as_deref(), Some("2s"));

        let hovered = resolve_state_styles(&base, true, false);
        assert_eq!(hovered.animation.as_deref(), Some("pulse 150ms"));
        assert_eq!(hovered.animation_duration, None);

        let pressed = resolve_state_styles(&base, true, true);
        assert_eq!(pressed.animation.as_deref(), Some("pulse 150ms"));
        assert_eq!(pressed.animation_play_state.as_deref(), Some("paused"));
    }

    #[test]
    fn a_state_can_set_interpolate_size() {
        let base = style(
            r##"{ "interpolateSize": "numeric-only", "hover": { "interpolateSize": "allow-keywords" } }"##,
        );
        let at_rest = resolve_state_styles(&base, false, false);
        assert_eq!(at_rest.interpolate_size.as_deref(), Some("numeric-only"));
        let hovered = resolve_state_styles(&base, true, false);
        assert_eq!(hovered.interpolate_size.as_deref(), Some("allow-keywords"));
    }

    #[test]
    fn a_state_background_replaces_the_base_background_of_either_kind() {
        let gradient = r##"{"type":"linear-gradient","angle":90,"stops":[{"color":"#000000","position":0},{"color":"#ffffff","position":1}]}"##;
        let base = style(&format!(
            r##"{{ "backgroundColor": "#111111", "hover": {{ "background": {gradient} }}, "active": {{ "backgroundColor": "#333333" }} }}"##
        ));
        let hovered = resolve_state_styles(&base, true, false);
        assert_eq!(hovered.background_color, None);
        assert!(matches!(hovered.background, Some(BackgroundValue::Gradient(_))));

        let pressed = resolve_state_styles(&base, true, true);
        assert_eq!(pressed.background_color.as_deref(), Some("#333333"));
    }

    #[test]
    fn a_state_background_colour_leaves_the_base_background_layers() {
        let base = style(
            r##"{"background":"linear-gradient(red, blue)","backgroundColor":"#111111",
                "hover":{"backgroundColor":"#222222"}}"##,
        );
        let resolved = resolve_state_styles(&base, true, false);
        assert_eq!(resolved.background_color.as_deref(), Some("#222222"));
        assert!(resolved.background.is_some(), "the gradient still paints over the colour");
    }

    #[test]
    fn a_background_shorthand_state_resets_the_base_layers() {
        let base = style(
            r##"{"backgroundImage":"linear-gradient(red, red)","backgroundSize":"10px",
                "hover":{"background":"blue"}}"##,
        );
        let resolved = resolve_state_styles(&base, true, false);
        assert_eq!(resolved.background_image, None);
        assert_eq!(resolved.background_size, None);

        let longhand = style(
            r##"{"backgroundImage":"linear-gradient(red, red)",
                "hover":{"backgroundColor":"blue"}}"##,
        );
        let resolved = resolve_state_styles(&longhand, true, false);
        assert!(resolved.background_image.is_some(), "a colour leaves the layers");
    }

    #[test]
    fn a_state_overscroll_shorthand_resets_the_base_axes() {
        let base = style(
            r##"{"overscrollBehaviorX":"none","overscrollBehaviorY":"contain",
                "hover":{"overscrollBehavior":"auto"}}"##,
        );
        let resolved = resolve_state_styles(&base, true, false);
        assert_eq!(resolved.overscroll_behavior.as_deref(), Some("auto"));
        assert_eq!(resolved.overscroll_behavior_x, None);
        assert_eq!(resolved.overscroll_behavior_y, None);

        let axis = style(
            r##"{"overscrollBehavior":"contain","hover":{"overscrollBehaviorX":"none"}}"##,
        );
        let resolved = resolve_state_styles(&axis, true, false);
        assert_eq!(resolved.overscroll_behavior.as_deref(), Some("contain"));
        assert_eq!(resolved.overscroll_behavior_x.as_deref(), Some("none"));
    }

    #[test]
    fn a_state_grid_shorthand_resets_the_base_lines_it_covers() {
        let base = style(
            r##"{"gridColumnStart":2,"gridColumnEnd":4,"gridRowStart":1,"gridRowEnd":3,
                "hover":{"gridColumn":"1 / 3"},"active":{"gridRow":"2"}}"##,
        );
        let hovered = resolve_state_styles(&base, true, false);
        assert!(hovered.grid_column.is_some());
        assert_eq!(hovered.grid_column_start, None);
        assert_eq!(hovered.grid_column_end, None);
        assert!(hovered.grid_row_start.is_some(), "the row lines are another axis");

        let pressed = resolve_state_styles(&base, true, true);
        assert_eq!(pressed.grid_row_start, None);
        assert_eq!(pressed.grid_row_end, None);
    }

    #[test]
    fn a_state_grid_area_resets_both_axes() {
        let base = style(
            r##"{"gridColumn":"1 / 2","gridRow":"1 / 2","gridColumnStart":3,"gridRowEnd":4,
                "hover":{"gridArea":"2 / 2 / 3 / 3"}}"##,
        );
        let resolved = resolve_state_styles(&base, true, false);
        assert!(resolved.grid_area.is_some());
        assert_eq!(resolved.grid_column, None);
        assert_eq!(resolved.grid_row, None);
        assert_eq!(resolved.grid_column_start, None);
        assert_eq!(resolved.grid_row_end, None);
    }

    #[test]
    fn a_state_border_shorthand_resets_the_base_widths_and_colours() {
        let base = style(
            r##"{"borderWidth":2,"borderTopWidth":4,"borderColor":"red","borderLeftColor":"blue",
                "borderTop":"1px solid green","borderStyle":"dashed",
                "hover":{"border":"1px solid black"}}"##,
        );
        let resolved = resolve_state_styles(&base, true, false);
        assert_eq!(resolved.border.as_deref(), Some("1px solid black"));
        assert_eq!(resolved.border_width, None);
        assert_eq!(resolved.border_top_width, None);
        assert_eq!(resolved.border_color, None);
        assert_eq!(resolved.border_left_color, None);
        assert_eq!(resolved.border_top, None);
        assert_eq!(resolved.border_style, None);
    }

    #[test]
    fn a_state_border_colour_or_side_shorthand_resets_only_what_it_covers() {
        let base = style(
            r##"{"borderTopColor":"red","borderTopWidth":3,"borderLeftColor":"blue",
                "hover":{"borderColor":"green"},"active":{"borderTop":"1px solid black"}}"##,
        );
        let hovered = resolve_state_styles(&base, true, false);
        assert_eq!(hovered.border_color.as_deref(), Some("green"));
        assert_eq!(hovered.border_top_color, None);
        assert_eq!(hovered.border_left_color, None);
        assert_eq!(hovered.border_top_width, Some(3.0), "a colour leaves widths");

        let pressed = resolve_state_styles(&base, false, true);
        assert_eq!(pressed.border_top.as_deref(), Some("1px solid black"));
        assert_eq!(pressed.border_top_width, None);
        assert_eq!(pressed.border_top_color, None);
        assert_eq!(pressed.border_left_color.as_deref(), Some("blue"), "another side");
    }

    #[test]
    fn a_state_side_shorthand_beats_the_base_all_sides_longhands_on_its_side_only() {
        let base = style(
            r##"{"borderWidth":2,"borderColor":"red","borderRightWidth":5,
                "hover":{"borderTop":"1px solid blue"}}"##,
        );
        let at_rest = resolve_state_styles(&base, false, false);
        assert_eq!(at_rest.border_width, Some(2.0));

        let hovered = resolve_state_styles(&base, true, false);
        assert_eq!(hovered.border_top.as_deref(), Some("1px solid blue"));
        assert_eq!(hovered.border_width, None);
        assert_eq!(hovered.border_color, None);
        assert_eq!((hovered.border_top_width, hovered.border_top_color.as_deref()), (None, None));
        assert_eq!(hovered.border_right_width, Some(5.0), "a side longhand beats borderWidth");
        for (width, color) in [
            (hovered.border_bottom_width, hovered.border_bottom_color.as_deref()),
            (hovered.border_left_width, hovered.border_left_color.as_deref()),
        ] {
            assert_eq!((width, color), (Some(2.0), Some("red")));
        }
        assert_eq!(hovered.border_right_color.as_deref(), Some("red"));
    }

    #[test]
    fn a_state_side_shorthand_splits_a_multi_value_base_border_color() {
        let base = style(
            r##"{"borderColor":"red blue green","hover":{"borderLeft":"1px solid black"}}"##,
        );
        let hovered = resolve_state_styles(&base, true, false);
        assert_eq!(hovered.border_color, None);
        assert_eq!(hovered.border_top_color.as_deref(), Some("red"));
        assert_eq!(hovered.border_right_color.as_deref(), Some("blue"));
        assert_eq!(hovered.border_bottom_color.as_deref(), Some("green"));
        assert_eq!(hovered.border_left_color, None);
    }

    #[test]
    fn a_state_side_shorthand_keeps_the_base_all_sides_longhands_it_does_not_cover() {
        let base = style(r##"{"borderWidth":2,"borderColor":"red","active":{"borderWidth":4,"borderTop":"1px solid blue"}}"##);
        let pressed = resolve_state_styles(&base, false, true);
        assert_eq!(pressed.border_width, Some(4.0), "the state's own all-sides value is its own");
        assert_eq!(pressed.border_top_width, None);
        assert_eq!(pressed.border_right_width, None, "the state's borderWidth resets the base sides");
        assert_eq!(pressed.border_right_color.as_deref(), Some("red"));
    }

    #[test]
    fn a_state_side_shorthand_with_a_line_style_replaces_the_base_border_style() {
        let base = style(
            r##"{"borderStyle":"dashed","hover":{"borderTop":"1px solid blue"},"active":{"borderTop":"none"}}"##,
        );
        assert_eq!(resolve_state_styles(&base, true, false).border_style, None);
        assert_eq!(
            resolve_state_styles(&base, false, true).border_style.as_deref(),
            Some("dashed"),
            "none draws no style"
        );
    }

    #[test]
    fn a_state_side_longhand_and_base_all_sides_shorthands_need_no_split() {
        let base = style(
            r##"{"border":"2px solid red","borderWidth":3,
                "hover":{"borderTopWidth":1},"active":{"borderTop":"1px solid blue"}}"##,
        );
        let hovered = resolve_state_styles(&base, true, false);
        assert_eq!(hovered.border_width, Some(3.0));
        assert_eq!(hovered.border_top_width, Some(1.0));

        // `border` is applied before the side shorthands, so it stays whole.
        let pressed = resolve_state_styles(&base, false, true);
        assert_eq!(pressed.border.as_deref(), Some("2px solid red"));
        assert_eq!(pressed.border_left_width, Some(3.0));
        assert_eq!(pressed.border_top_width, None);
    }

    #[test]
    fn a_mask_shorthand_state_resets_the_base_mask_layers() {
        let base = style(
            r##"{"maskImage":"linear-gradient(black, transparent)","maskSize":"10px",
                "hover":{"mask":"none"}}"##,
        );
        let resolved = resolve_state_styles(&base, true, false);
        assert_eq!(resolved.mask.as_deref(), Some("none"));
        assert_eq!(resolved.mask_image, None);
        assert_eq!(resolved.mask_size, None);
    }

    #[test]
    fn group_styles_fold_in_the_order_group_hover_hover_group_active_active() {
        let base = style(
            r##"{
                "color": "#000000", "opacity": 0.1, "cursor": "default", "width": 1,
                "group": "own",
                "groupHover": { "group": "card", "color": "#111111", "opacity": 0.2, "cursor": "text", "width": 2 },
                "hover": { "opacity": 0.3, "cursor": "pointer", "width": 3 },
                "groupActive": { "group": "card", "cursor": "grab", "width": 4 },
                "active": { "width": 5 }
            }"##,
        );
        let states = |hovered, active, group_hovered, group_active| PointerStates {
            hovered,
            active,
            group_hovered,
            group_active,
        };
        let fold = |states| resolve_pointer_states(&base, states);

        let group_hover = fold(states(false, false, true, false));
        assert_eq!(group_hover.color.as_deref(), Some("#111111"));
        assert_eq!(group_hover.opacity, Some(0.2));
        assert_eq!(group_hover.group.as_ref(), base.group.as_ref(), "the followed name is dropped");

        let both = fold(states(true, false, true, false));
        assert_eq!(both.color.as_deref(), Some("#111111"), "hover leaves what it does not set");
        assert_eq!(both.opacity, Some(0.3), "hover beats groupHover");
        assert_eq!(both.cursor.as_deref(), Some("pointer"));

        let group_active = fold(states(true, false, true, true));
        assert_eq!(group_active.cursor.as_deref(), Some("grab"), "groupActive beats hover");
        assert_eq!(group_active.width.as_ref(), fold(states(false, false, false, true)).width.as_ref());

        let all = fold(states(true, true, true, true));
        assert_eq!(all.width, style(r##"{"width":5}"##).width, "active is last");
        assert_eq!(all.cursor.as_deref(), Some("grab"));

        assert_eq!(fold(states(false, false, false, false)).opacity, Some(0.1));
        for folded in [group_hover, both, group_active, all] {
            assert_eq!(
                (folded.hover, folded.active, folded.group_hover, folded.group_active),
                (None, None, None, None)
            );
        }
    }

    #[test]
    fn a_group_refinement_that_is_not_in_play_is_ignored() {
        let base = style(r##"{"opacity":1,"groupHover":{"opacity":0.5},"groupActive":{"opacity":0.2}}"##);
        let pressed_only = resolve_pointer_states(
            &base,
            PointerStates {
                group_active: true,
                ..Default::default()
            },
        );
        assert_eq!(pressed_only.opacity, Some(0.2));
        let hovered_only = resolve_pointer_states(
            &base,
            PointerStates {
                hovered: true,
                ..Default::default()
            },
        );
        assert_eq!(hovered_only.opacity, Some(1.0), "the element's hover is not the group's");
    }

    /// A state style whose value for `field` is invalid leaves the base's alone,
    /// like CSS dropping an invalid declaration, while a valid `none` replaces it.
    fn resolved_hover(base_and_hover: &str) -> StyleDesc {
        resolve_state_styles(&style(base_and_hover), true, false)
    }

    #[test]
    fn an_invalid_state_box_shadow_keeps_the_base_shadow() {
        let resolved = resolved_hover(
            r##"{"boxShadow":"0 1px 2px red","hover":{"boxShadow":"4 4 red"}}"##,
        );
        assert_eq!(resolved.box_shadow.as_ref().unwrap().as_slice().len(), 1);
        let none = resolved_hover(r##"{"boxShadow":"0 1px 2px red","hover":{"boxShadow":"none"}}"##);
        assert!(none.box_shadow.as_ref().unwrap().as_slice().is_empty());
    }

    #[test]
    fn an_invalid_state_filter_or_clip_path_keeps_the_base() {
        let resolved = resolved_hover(
            r##"{"filter":"blur(2px)","backdropFilter":"blur(4px)","clipPath":"inset(0)",
                "hover":{"filter":"blur(","backdropFilter":"drop-shadow(1px 2px notacolour)","clipPath":"circle(-5px)"}}"##,
        );
        assert_eq!(resolved.filter.as_deref(), Some("blur(2px)"));
        assert_eq!(resolved.backdrop_filter.as_deref(), Some("blur(4px)"));
        assert_eq!(resolved.clip_path.as_deref(), Some("inset(0)"));
        let none = resolved_hover(
            r##"{"filter":"blur(2px)","clipPath":"inset(0)","hover":{"filter":"none","clipPath":"none"}}"##,
        );
        assert_eq!(none.filter.as_deref(), Some("none"));
        assert_eq!(none.clip_path.as_deref(), Some("none"));
    }

    #[test]
    fn an_invalid_state_mask_keeps_the_base_mask_and_does_not_reset_its_longhands() {
        let resolved = resolved_hover(
            r##"{"mask":"linear-gradient(black, transparent)","maskSize":"10px",
                "hover":{"mask":"linear-gradient(","maskSize":"-1px","maskImage":"nope"}}"##,
        );
        assert_eq!(resolved.mask.as_deref(), Some("linear-gradient(black, transparent)"));
        assert_eq!(resolved.mask_size.as_deref(), Some("10px"));
        assert_eq!(resolved.mask_image, None);
        // Valid ones still replace, and a shorthand still resets the longhands.
        let valid = resolved_hover(
            r##"{"mask":"linear-gradient(black, transparent)","maskSize":"10px",
                "hover":{"mask":"none"}}"##,
        );
        assert_eq!(valid.mask.as_deref(), Some("none"));
        assert_eq!(valid.mask_size, None);
    }

    #[test]
    fn an_invalid_state_background_keeps_the_base_background() {
        let resolved = resolved_hover(
            r##"{"background":"linear-gradient(red, blue)","backgroundImage":"linear-gradient(blue, red)",
                "backgroundSize":"10px","backgroundColor":"#111111",
                "hover":{"background":"linear-gradient(red","backgroundImage":"linear-gradient(red)","backgroundSize":"1 2 3",
                         "backgroundColor":"nope"}}"##,
        );
        assert!(matches!(
            resolved.background,
            Some(BackgroundValue::Color(ref value)) if value == "linear-gradient(red, blue)"
        ));
        assert_eq!(resolved.background_image.as_deref(), Some("linear-gradient(blue, red)"));
        assert_eq!(resolved.background_size.as_deref(), Some("10px"));
        assert_eq!(resolved.background_color.as_deref(), Some("#111111"));
        // A plain colour and an object gradient are valid backgrounds.
        let colour = resolved_hover(r##"{"background":"blue","hover":{"background":"red"}}"##);
        assert!(matches!(
            colour.background,
            Some(BackgroundValue::Color(ref value)) if value == "red"
        ));
    }
}
