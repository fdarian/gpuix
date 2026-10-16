//! The layout half of `apply_styles`: display, flex direction, flex basis,
//! position and insets, sizes, margins, padding, gaps, and the grid properties.

use gpui::Styled;

use crate::color::ColorContext;
use crate::css::length::{LengthContext, LinearLength};
use crate::style::{DimensionValue, IntrinsicKeyword, StyleDesc};

use super::grid_values::GridTemplateValue;

fn linear(value: &LinearLength, lengths: &LengthContext) -> gpui::DefiniteLength {
    let (total, fraction) = lengths.split(value);
    if fraction == 0.0 {
        gpui::px(total as f32).into()
    } else if total == 0.0 {
        gpui::relative(fraction as f32)
    } else {
        gpui::DefiniteLength::calc(gpui::px(total as f32), fraction as f32)
    }
}

/// `None` for `auto`, `anchor-size()` and the intrinsic size keywords, which leave the
/// property to layout. A keyword reaches `length` only from a field that rejects it when
/// it is deserialized, so those arms are never taken for a style that came from JS.
fn definite(value: &DimensionValue, lengths: &LengthContext) -> Option<gpui::DefiniteLength> {
    match value {
        DimensionValue::Pixels(pixels) => Some(gpui::px(*pixels as f32).into()),
        DimensionValue::Percentage(fraction) => Some(gpui::relative(*fraction as f32)),
        DimensionValue::Calc(value) => Some(linear(value, lengths)),
        DimensionValue::Auto | DimensionValue::AnchorSize(_) | DimensionValue::Intrinsic(_) => None,
    }
}

fn length(value: &DimensionValue, lengths: &LengthContext) -> gpui::Length {
    definite(value, lengths).map_or(gpui::Length::Auto, gpui::Length::Definite)
}

fn intrinsic_size(keyword: &IntrinsicKeyword, lengths: &LengthContext) -> gpui::IntrinsicSize {
    match keyword {
        IntrinsicKeyword::MinContent => gpui::IntrinsicSize::MinContent,
        IntrinsicKeyword::MaxContent => gpui::IntrinsicSize::MaxContent,
        IntrinsicKeyword::FitContent => gpui::IntrinsicSize::FitContent,
        IntrinsicKeyword::FitContentLimit(limit) => gpui::IntrinsicSize::FitContentLimit(linear(limit, lengths)),
    }
}

/// Sets one of the six size properties. `anchor-size()` leaves `slot` as it is.
///
/// A keyword is a separate gpui field that outranks `slot`, so every other value clears
/// `keyword`: a hover or focus refinement that sets a length or `auto` must replace the
/// keyword of the base style.
fn set_size(
    slot: &mut Option<gpui::Length>,
    keyword: Option<&mut Option<Option<gpui::IntrinsicSize>>>,
    value: &Option<DimensionValue>,
    lengths: &LengthContext,
) {
    let Some(value) = value else {
        return;
    };
    match (value, keyword) {
        (DimensionValue::AnchorSize(_), _) => {}
        (DimensionValue::Intrinsic(value), Some(keyword)) => {
            *slot = Some(gpui::Length::Auto);
            *keyword = Some(Some(intrinsic_size(value, lengths)));
        }
        // CSS Sizing 3: in the block axis the keywords behave as `auto`.
        (DimensionValue::Intrinsic(_), None) => *slot = Some(gpui::Length::Auto),
        (value, keyword) => {
            *slot = Some(length(value, lengths));
            if let Some(keyword) = keyword {
                *keyword = Some(None);
            }
        }
    }
}

fn align_items(value: &str) -> Option<gpui::AlignItems> {
    Some(match value {
        "start" => gpui::AlignItems::Start,
        "end" => gpui::AlignItems::End,
        "flex-start" => gpui::AlignItems::FlexStart,
        "flex-end" => gpui::AlignItems::FlexEnd,
        "center" => gpui::AlignItems::Center,
        "baseline" => gpui::AlignItems::Baseline,
        "stretch" => gpui::AlignItems::Stretch,
        _ => return None,
    })
}

fn auto_flow(value: &str) -> Option<gpui::GridAutoFlow> {
    let mut column = false;
    let mut dense = false;
    for word in value.split_whitespace() {
        match word {
            "row" => column = false,
            "column" => column = true,
            "dense" => dense = true,
            _ => return None,
        }
    }
    Some(match (column, dense) {
        (false, false) => gpui::GridAutoFlow::Row,
        (true, false) => gpui::GridAutoFlow::Column,
        (false, true) => gpui::GridAutoFlow::RowDense,
        (true, true) => gpui::GridAutoFlow::ColumnDense,
    })
}

/// `colors` and `lengths` are the context of the element `style` belongs to, which a length
/// with a unit other than px resolves in.
pub(crate) fn apply<E: Styled>(
    mut el: E,
    style: &StyleDesc,
    colors: &ColorContext,
    lengths: &LengthContext,
) -> E {
    match style.display.as_deref() {
        Some("flex") => el = el.flex(),
        Some("grid") => el = el.grid(),
        Some("block") => el = el.block(),
        Some("none") => el = el.hidden(),
        _ => {}
    }
    el = apply_flex_direction(el, style);
    el = apply_grid(el, style, lengths);
    el = apply_size(el, style, lengths);
    el = apply_margin(el, style, lengths);
    el = apply_padding(el, style, lengths);
    el = apply_gap(el, style, lengths);
    el = apply_position(el, style, lengths);
    el = super::stacking::apply_z_index(el, style, colors, lengths);
    el = apply_flex_basis(el, style, lengths);
    if let Some(value) = style.justify_items.as_deref().and_then(align_items) {
        el.style().justify_items = Some(value);
    }
    if let Some(value) = style.justify_self.as_deref().and_then(align_items) {
        el.style().justify_self = Some(value);
    }
    el
}

/// A keyword leaves `flex_basis` `auto` for layout to fill in, and every other value clears
/// the keyword, like `set_size`. Whether the keyword counts depends on the parent's direction,
/// which only layout knows.
fn apply_flex_basis<E: Styled>(mut el: E, style: &StyleDesc, lengths: &LengthContext) -> E {
    let Some(value) = &style.flex_basis else {
        return el;
    };
    let refinement = el.style();
    match value {
        DimensionValue::Intrinsic(keyword) => {
            refinement.flex_basis = Some(gpui::Length::Auto);
            refinement.intrinsic_widths.flex_basis = Some(Some(intrinsic_size(keyword, lengths)));
        }
        value => {
            refinement.flex_basis = Some(length(value, lengths));
            refinement.intrinsic_widths.flex_basis = Some(None);
        }
    }
    el
}

fn apply_flex_direction<E: Styled>(mut el: E, style: &StyleDesc) -> E {
    match style.flex_direction.as_deref() {
        Some("column") => el = el.flex_col(),
        Some("row") => el = el.flex_row(),
        Some("column-reverse") => el = el.flex_col_reverse(),
        Some("row-reverse") => el = el.flex_row_reverse(),
        _ => {}
    }
    el
}

fn apply_size<E: Styled>(mut el: E, style: &StyleDesc, lengths: &LengthContext) -> E {
    let refinement = el.style();
    let widths = &mut refinement.intrinsic_widths;
    set_size(&mut refinement.size.width, Some(&mut widths.width), &style.width, lengths);
    set_size(&mut refinement.size.height, None, &style.height, lengths);
    set_size(&mut refinement.min_size.width, Some(&mut widths.min_width), &style.min_width, lengths);
    set_size(&mut refinement.min_size.height, None, &style.min_height, lengths);
    set_size(&mut refinement.max_size.width, Some(&mut widths.max_width), &style.max_width, lengths);
    set_size(&mut refinement.max_size.height, None, &style.max_height, lengths);
    if style.measure_natural_width {
        refinement.measure_natural_width = Some(true);
    }
    el
}

fn apply_margin<E: Styled>(mut el: E, style: &StyleDesc, lengths: &LengthContext) -> E {
    if let Some(value) = &style.margin {
        let margin = &mut el.style().margin;
        let value = length(value, lengths);
        margin.top = Some(value);
        margin.right = Some(value);
        margin.bottom = Some(value);
        margin.left = Some(value);
    }
    if let Some(value) = &style.margin_top {
        el.style().margin.top = Some(length(value, lengths));
    }
    if let Some(value) = &style.margin_right {
        el.style().margin.right = Some(length(value, lengths));
    }
    if let Some(value) = &style.margin_bottom {
        el.style().margin.bottom = Some(length(value, lengths));
    }
    if let Some(value) = &style.margin_left {
        el.style().margin.left = Some(length(value, lengths));
    }
    el
}

fn apply_padding<E: Styled>(mut el: E, style: &StyleDesc, lengths: &LengthContext) -> E {
    if let Some(value) = &style.padding {
        let padding = &mut el.style().padding;
        let value = linear(value, lengths);
        padding.top = Some(value);
        padding.right = Some(value);
        padding.bottom = Some(value);
        padding.left = Some(value);
    }
    if let Some(value) = &style.padding_top {
        el.style().padding.top = Some(linear(value, lengths));
    }
    if let Some(value) = &style.padding_right {
        el.style().padding.right = Some(linear(value, lengths));
    }
    if let Some(value) = &style.padding_bottom {
        el.style().padding.bottom = Some(linear(value, lengths));
    }
    if let Some(value) = &style.padding_left {
        el.style().padding.left = Some(linear(value, lengths));
    }
    el
}

/// The axis gaps come after `gap`, so they win, as CSS orders the shorthand
/// before its longhands.
fn apply_gap<E: Styled>(mut el: E, style: &StyleDesc, lengths: &LengthContext) -> E {
    if let Some(value) = &style.gap {
        let gap = &mut el.style().gap;
        let value = linear(value, lengths);
        gap.width = Some(value);
        gap.height = Some(value);
    }
    if let Some(value) = &style.row_gap {
        el.style().gap.height = Some(linear(value, lengths));
    }
    if let Some(value) = &style.column_gap {
        el.style().gap.width = Some(linear(value, lengths));
    }
    el
}

fn apply_position<E: Styled>(mut el: E, style: &StyleDesc, lengths: &LengthContext) -> E {
    match style.position.as_deref() {
        Some("static") => el.style().position = Some(gpui::Position::Static),
        Some("relative") => el = el.relative(),
        // Laid out like relative with the insets ignored, then held inside the scrollport by
        // them when it is prepainted.
        Some("sticky") => el.style().position = Some(gpui::Position::Sticky),
        Some("absolute") => el.style().position = Some(gpui::Position::AbsoluteInAncestor),
        Some("fixed") => el.style().position = Some(gpui::Position::Fixed),
        _ => {}
    }
    if let Some(value) = &style.top {
        el.style().inset.top = Some(length(value, lengths));
    }
    if let Some(value) = &style.right {
        el.style().inset.right = Some(length(value, lengths));
    }
    if let Some(value) = &style.bottom {
        el.style().inset.bottom = Some(length(value, lengths));
    }
    if let Some(value) = &style.left {
        el.style().inset.left = Some(length(value, lengths));
    }
    el
}

fn apply_grid<E: Styled>(mut el: E, style: &StyleDesc, lengths: &LengthContext) -> E {
    match &style.grid_template_columns {
        Some(GridTemplateValue::Count(count)) => {
            el = match style.grid_column_min.as_deref() {
                Some("min-content") => el.grid_cols_min_content(*count),
                Some("max-content") => el.grid_cols_max_content(*count),
                _ => el.grid_cols(*count),
            };
        }
        Some(GridTemplateValue::Tracks(list)) => {
            el.style().grid_cols_template = Some(list.resolve(lengths));
        }
        None => {}
    }
    match &style.grid_template_rows {
        Some(GridTemplateValue::Count(count)) => {
            el = match style.grid_row_min.as_deref() {
                Some("min-content") => el.grid_rows_min_content(*count),
                Some("max-content") => el.grid_rows_max_content(*count),
                _ => el.grid_rows(*count),
            };
        }
        Some(GridTemplateValue::Tracks(list)) => {
            el.style().grid_rows_template = Some(list.resolve(lengths));
        }
        None => {}
    }
    if let Some(auto) = &style.grid_auto_columns {
        el.style().grid_auto_cols = Some(auto.resolve(lengths));
    }
    if let Some(auto) = &style.grid_auto_rows {
        el.style().grid_auto_rows = Some(auto.resolve(lengths));
    }
    if let Some(flow) = style.grid_auto_flow.as_deref().and_then(auto_flow) {
        el.style().grid_auto_flow = Some(flow);
    }
    if let Some(areas) = &style.grid_template_areas {
        el.style().grid_template_areas = Some(areas.areas.as_ref().clone());
    }

    // Shorthand first, then the longhands, as the cascade orders them.
    if let Some(area) = &style.grid_area {
        let location = el.style().grid_location_mut();
        location.row = area.row.0.clone()..area.row.1.clone();
        location.column = area.column.0.clone()..area.column.1.clone();
    }
    if let Some(column) = &style.grid_column {
        el.style().grid_location_mut().column = column.start.clone()..column.end.clone();
    }
    if let Some(row) = &style.grid_row {
        el.style().grid_location_mut().row = row.start.clone()..row.end.clone();
    }
    if let Some(line) = &style.grid_column_start {
        el.style().grid_location_mut().column.start = line.line.clone();
    }
    if let Some(line) = &style.grid_column_end {
        el.style().grid_location_mut().column.end = line.line.clone();
    }
    if let Some(line) = &style.grid_row_start {
        el.style().grid_location_mut().row.start = line.line.clone();
    }
    if let Some(line) = &style.grid_row_end {
        el.style().grid_location_mut().row.end = line.line.clone();
    }
    el
}
