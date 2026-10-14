//! The layout half of `apply_styles`: display, flex direction, position and
//! insets, margins, and the grid properties.

use gpui::Styled;

use crate::style::{DimensionValue, StyleDesc};

use super::grid_values::GridTemplateValue;

fn length(value: &DimensionValue) -> gpui::Length {
    match value {
        DimensionValue::Pixels(pixels) => gpui::px(*pixels as f32).into(),
        DimensionValue::Percentage(fraction) => gpui::relative(*fraction as f32).into(),
        DimensionValue::Auto => gpui::Length::Auto,
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

pub(crate) fn apply<E: Styled>(mut el: E, style: &StyleDesc) -> E {
    match style.display.as_deref() {
        Some("flex") => el = el.flex(),
        Some("grid") => el = el.grid(),
        Some("block") => el = el.block(),
        Some("none") => el = el.hidden(),
        _ => {}
    }
    el = apply_flex_direction(el, style);
    el = apply_grid(el, style);
    el = apply_margin(el, style);
    el = apply_position(el, style);
    if let Some(value) = style.justify_items.as_deref().and_then(align_items) {
        el.style().justify_items = Some(value);
    }
    if let Some(value) = style.justify_self.as_deref().and_then(align_items) {
        el.style().justify_self = Some(value);
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

fn apply_margin<E: Styled>(mut el: E, style: &StyleDesc) -> E {
    if let Some(value) = &style.margin {
        let margin = &mut el.style().margin;
        let value = length(value);
        margin.top = Some(value);
        margin.right = Some(value);
        margin.bottom = Some(value);
        margin.left = Some(value);
    }
    if let Some(value) = &style.margin_top {
        el.style().margin.top = Some(length(value));
    }
    if let Some(value) = &style.margin_right {
        el.style().margin.right = Some(length(value));
    }
    if let Some(value) = &style.margin_bottom {
        el.style().margin.bottom = Some(length(value));
    }
    if let Some(value) = &style.margin_left {
        el.style().margin.left = Some(length(value));
    }
    el
}

fn apply_position<E: Styled>(mut el: E, style: &StyleDesc) -> E {
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
        el.style().inset.top = Some(length(value));
    }
    if let Some(value) = &style.right {
        el.style().inset.right = Some(length(value));
    }
    if let Some(value) = &style.bottom {
        el.style().inset.bottom = Some(length(value));
    }
    if let Some(value) = &style.left {
        el.style().inset.left = Some(length(value));
    }
    el
}

fn apply_grid<E: Styled>(mut el: E, style: &StyleDesc) -> E {
    match &style.grid_template_columns {
        Some(GridTemplateValue::Count(count)) => {
            el = match style.grid_column_min.as_deref() {
                Some("min-content") => el.grid_cols_min_content(*count),
                Some("max-content") => el.grid_cols_max_content(*count),
                _ => el.grid_cols(*count),
            };
        }
        Some(GridTemplateValue::Tracks { tracks, .. }) => {
            el.style().grid_cols_template = Some(tracks.as_ref().clone());
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
        Some(GridTemplateValue::Tracks { tracks, .. }) => {
            el.style().grid_rows_template = Some(tracks.as_ref().clone());
        }
        None => {}
    }
    if let Some(auto) = &style.grid_auto_columns {
        el.style().grid_auto_cols = Some(auto.tracks.as_ref().clone());
    }
    if let Some(auto) = &style.grid_auto_rows {
        el.style().grid_auto_rows = Some(auto.tracks.as_ref().clone());
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
