use crate::style::StyleDesc;

/// Takes `$base`'s value for every field `$over` leaves unset. Written as a
/// struct literal so a field added to `StyleDesc` and missing from this list
/// fails to compile instead of silently never taking part in state styles.
macro_rules! overlay_fields {
    ($base:expr, $over:expr; $($field:ident),+ $(,)?) => {
        StyleDesc {
            $($field: $over.$field.clone().or_else(|| $base.$field.clone()),)+
            hover: None,
            active: None,
        }
    };
}

/// `style` with the `hover` and then the `active` refinement folded in, for an
/// element that is `hovered` and/or `active`, and both refinements cleared.
///
/// Matches what GPUI's `.hover()` / `.active()` do: a field the state style sets
/// replaces the base value, an unset one keeps it, and `active` is applied last
/// so it wins over `hover`. A state style's own `hover` / `active` are ignored.
pub fn resolve_state_styles(style: &StyleDesc, hovered: bool, active: bool) -> StyleDesc {
    let mut resolved = overlay(style, &StyleDesc::default());
    if hovered {
        if let Some(hover) = style.hover.as_deref() {
            resolved = overlay(&resolved, hover);
        }
    }
    if active {
        if let Some(active) = style.active.as_deref() {
            resolved = overlay(&resolved, active);
        }
    }
    resolved
}

fn overlay(base: &StyleDesc, over: &StyleDesc) -> StyleDesc {
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
        border, border_top, border_right, border_bottom, border_left, border_top_color,
        border_right_color, border_bottom_color, border_left_color,
        border_width, border_top_width, border_right_width, border_bottom_width,
        border_left_width, border_color, border_radius, border_top_left_radius,
        border_top_right_radius, border_bottom_left_radius, border_bottom_right_radius,
        box_shadow, border_style, aspect_ratio, outline_width, outline_color, outline_offset,
        font_size, font_family, font_weight, font_variant_numeric, font_feature_settings, text_align, line_height, white_space, text_overflow,
        line_clamp, text_decoration,
        overflow, overflow_x, overflow_y, scrollbar_color, scrollbar_width,
        cursor, pointer_events, user_select, selection_color,
        focus_visible, focus_within, keyboard_focus_dim, group, group_hover, group_active,
        order, justify_items, justify_self, grid_auto_columns, grid_auto_rows, grid_auto_flow,
        grid_template_areas, grid_area, grid_column, grid_row, grid_column_start,
        grid_column_end, grid_row_start, grid_row_end, z_index, overscroll_behavior,
        overscroll_behavior_x, overscroll_behavior_y, scroll_behavior,
    )
}

/// A shorthand in `over` resets the longhands it covers, as a later CSS
/// declaration does. `apply_styles` reads longhands after their shorthand, so a
/// base longhand would otherwise beat a state style's shorthand.
fn drop_shadowed_longhands(base: &mut StyleDesc, over: &StyleDesc) {
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
    if over.overflow.is_some() {
        base.overflow_x = None;
        base.overflow_y = None;
    }
    // `background_color` wins over `background` when both are set.
    if over.background.is_some() || over.background_color.is_some() {
        base.background = None;
        base.background_color = None;
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
    fn a_state_background_replaces_the_base_background_of_either_kind() {
        let gradient = r##"{"type":"linear-gradient","angle":90,"stops":[{"color":"#000000","position":0},{"color":"#ffffff","position":1}]}"##;
        let base = style(&format!(
            r##"{{ "backgroundColor": "#111111", "hover": {{ "background": {gradient} }}, "active": {{ "backgroundColor": "#333333" }} }}"##
        ));
        let hovered = resolve_state_styles(&base, true, false);
        assert_eq!(hovered.background_color, None);
        assert!(matches!(hovered.background, Some(BackgroundValue::Gradient(_))));

        let pressed = resolve_state_styles(&base, true, true);
        assert_eq!(pressed.background, None);
        assert_eq!(pressed.background_color.as_deref(), Some("#333333"));
    }
}
