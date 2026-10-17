//! CSS tables: which `display` values are table parts, what an element hands to GPUI to be one,
//! and which children of a table are wrapped in an anonymous cell.
//!
//! The layout is zed's (`taffy/src/compute/table`): a `display: table` node lays out the cells
//! and captions below it itself. Every part but the table is a block box with a
//! [`gpui::TableRole`], so a part outside a table, which the table never reads, lays out as the
//! block CSS would wrap in an anonymous table (without the wrapper).

use gpui::Styled;

use crate::css::table::TableInherited;
use crate::retained_tree::{RetainedElement, RetainedTree};
use crate::style::StyleDesc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    Table,
    RowGroup(gpui::RowGroupKind),
    Row,
    Cell,
    Caption,
    Column,
    ColumnGroup,
}

/// The table part a `display` value stands for. `inline-table` is a `table`: the inline
/// formatting context does not place atomic inline boxes yet.
pub(crate) fn part_of(display: &str) -> Option<Part> {
    Some(match display {
        "table" | "inline-table" => Part::Table,
        "table-row-group" => Part::RowGroup(gpui::RowGroupKind::Body),
        "table-header-group" => Part::RowGroup(gpui::RowGroupKind::Header),
        "table-footer-group" => Part::RowGroup(gpui::RowGroupKind::Footer),
        "table-row" => Part::Row,
        "table-cell" => Part::Cell,
        "table-caption" => Part::Caption,
        "table-column" => Part::Column,
        "table-column-group" => Part::ColumnGroup,
        _ => return None,
    })
}

pub(crate) fn part(element: &RetainedElement) -> Option<Part> {
    element
        .style
        .as_deref()
        .and_then(|style| style.display.as_deref())
        .and_then(part_of)
}

/// Whether the children of `element` that are not table parts are wrapped in anonymous cells.
pub(crate) fn wraps_children(element: &RetainedElement) -> bool {
    matches!(part(element), Some(Part::Table | Part::RowGroup(_) | Part::Row))
}

/// A child of a table that takes no part in the grid and so is not wrapped: an absolutely
/// positioned one is placed by the table itself.
pub(crate) fn is_out_of_flow(element: &RetainedElement) -> bool {
    matches!(
        element.style.as_deref().and_then(|style| style.position.as_deref()),
        Some("absolute" | "fixed")
    )
}

/// How many columns or rows a `colSpan`, `rowSpan` or `span` prop asks for. HTML reads a
/// non-negative integer, a number or a string of digits, and anything else is `default`. The
/// caps are HTML's, which the layout applies too.
fn span_prop(element: &RetainedElement, name: &str, default: u16, min: u16, max: u16) -> u16 {
    let value = element.custom_props().get(name);
    let number = value.and_then(|value| match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    });
    match number {
        Some(number) if number.is_finite() && number >= 0.0 => number.trunc().clamp(f64::from(min), f64::from(max)) as u16,
        _ => default,
    }
}

/// The table roles and styles of `element`, which `style` (the final one, with its states
/// folded) makes a part.
pub(crate) fn apply<E: Styled>(
    mut el: E,
    tree: &RetainedTree,
    element: &RetainedElement,
    style: Option<&StyleDesc>,
    table: &TableInherited,
) -> E {
    let Some(style) = style else { return el };
    let Some(part) = style.display.as_deref().and_then(part_of) else {
        return el;
    };
    match part {
        Part::Table => {
            el = el.border_spacing(
                gpui::px(table.spacing.horizontal as f32),
                gpui::px(table.spacing.vertical as f32),
            );
            if table.collapse {
                el = el.border_collapse();
            }
            if style.table_layout == Some(crate::css::table::TableLayout::Fixed) {
                el = el.table_layout(gpui::TableLayout::Fixed);
            }
        }
        Part::RowGroup(kind) => el = el.table_role(gpui::TableRole::RowGroup(kind)),
        Part::Row => el = el.table_role(gpui::TableRole::Row),
        Part::Cell => {
            el = el.table_role(gpui::TableRole::Cell {
                col_span: span_prop(element, "colSpan", 1, 1, 1000),
                row_span: span_prop(element, "rowSpan", 1, 0, 65534),
            });
            el = el.cell_vertical_align(cell_vertical_align(style));
            if table.hide_empty_cells && !table.collapse && is_empty(tree, element) {
                el = without_decoration(el);
            }
        }
        Part::Caption => {
            let side = if table.caption_bottom { gpui::CaptionSide::Bottom } else { gpui::CaptionSide::Top };
            el = el.table_role(gpui::TableRole::Caption(side));
        }
        Part::Column => {
            el = el.table_role(gpui::TableRole::Column { span: span_prop(element, "span", 1, 1, 1000) });
        }
        Part::ColumnGroup => {
            el = el.table_role(gpui::TableRole::ColumnGroup { span: span_prop(element, "span", 1, 1, 1000) });
        }
    }
    el
}

/// Whether a cell has nothing in it for `empty-cells`: no content, or only collapsible white
/// space and `display: none` children. Any element in it, even one that paints nothing, is
/// content.
fn is_empty(tree: &RetainedTree, cell: &RetainedElement) -> bool {
    use crate::text::inline::is_blank;
    cell.content.as_deref().is_none_or(is_blank)
        && cell.children.iter().filter_map(|id| tree.elements.get(id)).all(|child| {
            crate::layout::box_generation::is_display_none(child)
                || (crate::text::inline::is_raw_text(child) && child.content.as_deref().is_none_or(is_blank))
        })
}

/// `empty-cells: hide` on an empty cell: no background, border or shadow, and the cell keeps its
/// size.
fn without_decoration<E: Styled>(mut el: E) -> E {
    let style = el.style();
    style.background = Some(gpui::Fill::Color(gpui::transparent_black().into()));
    style.background_layers = Some(Vec::new());
    style.box_shadow = Some(Vec::new());
    let clear = Some(gpui::transparent_black());
    style.border_colors = gpui::EdgesRefinement { top: clear, right: clear, bottom: clear, left: clear };
    el
}

/// A cell aligns to the top, the middle or the bottom of its row, or to the baseline: the
/// other `vertical-align` values (a length, `sub`, `text-top`...) mean `baseline` on a cell.
fn cell_vertical_align(style: &StyleDesc) -> gpui::CellVerticalAlign {
    use crate::css::vertical_align::VerticalAlign;
    match style.vertical_align {
        Some(VerticalAlign::Top) => gpui::CellVerticalAlign::Top,
        Some(VerticalAlign::Middle) => gpui::CellVerticalAlign::Middle,
        Some(VerticalAlign::Bottom) => gpui::CellVerticalAlign::Bottom,
        _ => gpui::CellVerticalAlign::Baseline,
    }
}

/// An anonymous cell: a block with no style of its own, inheriting the text style of the table
/// part around it.
pub(crate) fn anonymous_cell() -> gpui::Div {
    use gpui::prelude::*;
    let mut cell = gpui::div().table_role(gpui::TableRole::Cell { col_span: 1, row_span: 1 });
    cell.style().position = Some(gpui::Position::Static);
    cell
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(json: serde_json::Value, props: &[(&str, serde_json::Value)]) -> RetainedElement {
        let mut tree = crate::retained_tree::RetainedTree::new();
        tree.create_element(1, "div".to_string());
        tree.set_style(1, std::sync::Arc::new(serde_json::from_value(json).unwrap()));
        for (key, value) in props {
            tree.set_custom_prop(1, key.to_string(), value.clone());
        }
        tree.elements.remove(&1).unwrap()
    }

    fn role(display: &str, props: &[(&str, serde_json::Value)]) -> gpui::TableRole {
        use gpui::Styled;
        let element = element(serde_json::json!({ "display": display }), props);
        let mut div = apply(gpui::div(), &RetainedTree::new(), &element, element.style.as_deref(), &TableInherited::default());
        div.style().table_role.unwrap_or_default()
    }

    #[test]
    fn every_part_display_maps() {
        let kinds = [
            ("table", Part::Table),
            ("inline-table", Part::Table),
            ("table-row-group", Part::RowGroup(gpui::RowGroupKind::Body)),
            ("table-header-group", Part::RowGroup(gpui::RowGroupKind::Header)),
            ("table-footer-group", Part::RowGroup(gpui::RowGroupKind::Footer)),
            ("table-row", Part::Row),
            ("table-cell", Part::Cell),
            ("table-caption", Part::Caption),
            ("table-column", Part::Column),
            ("table-column-group", Part::ColumnGroup),
        ];
        for (display, part) in kinds {
            assert_eq!(part_of(display), Some(part), "{display}");
        }
        for display in ["block", "flex", "grid", "none", "contents", "inline", "inline-block", "table-footer"] {
            assert_eq!(part_of(display), None, "{display}");
        }
    }

    #[test]
    fn spans_come_from_the_props_as_html_reads_them() {
        use serde_json::json;
        assert_eq!(
            role("table-cell", &[("colSpan", json!(3)), ("rowSpan", json!("2"))]),
            gpui::TableRole::Cell { col_span: 3, row_span: 2 }
        );
        assert_eq!(role("table-cell", &[]), gpui::TableRole::Cell { col_span: 1, row_span: 1 });
        // colspan 0 is 1, rowspan 0 reaches the end of the group, the rest is capped.
        assert_eq!(
            role("table-cell", &[("colSpan", json!(0)), ("rowSpan", json!(0))]),
            gpui::TableRole::Cell { col_span: 1, row_span: 0 }
        );
        assert_eq!(
            role("table-cell", &[("colSpan", json!(5000)), ("rowSpan", json!(100000))]),
            gpui::TableRole::Cell { col_span: 1000, row_span: 65534 }
        );
        assert_eq!(
            role("table-cell", &[("colSpan", json!(-2)), ("rowSpan", json!("x"))]),
            gpui::TableRole::Cell { col_span: 1, row_span: 1 }
        );
        assert_eq!(role("table-column", &[("span", json!(2))]), gpui::TableRole::Column { span: 2 });
        assert_eq!(role("table-column-group", &[("span", json!(4))]), gpui::TableRole::ColumnGroup { span: 4 });
        // Only a cell reads the span of a cell.
        assert_eq!(role("block", &[("colSpan", json!(3))]), gpui::TableRole::None);
    }

    #[test]
    fn a_cell_aligns_to_top_middle_bottom_or_the_baseline() {
        let align = |value: &str| {
            let style: StyleDesc = serde_json::from_value(serde_json::json!({ "verticalAlign": value })).unwrap();
            cell_vertical_align(&style)
        };
        assert_eq!(align("top"), gpui::CellVerticalAlign::Top);
        assert_eq!(align("middle"), gpui::CellVerticalAlign::Middle);
        assert_eq!(align("bottom"), gpui::CellVerticalAlign::Bottom);
        for other in ["baseline", "sub", "super", "text-top", "text-bottom", "4px", "20%"] {
            assert_eq!(align(other), gpui::CellVerticalAlign::Baseline, "{other}");
        }
        assert_eq!(cell_vertical_align(&StyleDesc::default()), gpui::CellVerticalAlign::Baseline);
    }

    #[test]
    fn a_cell_is_empty_for_empty_cells_without_content_or_with_only_white_space_and_hidden_boxes() {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        fn add(tree: &mut RetainedTree, id: u64, kind: &str, text: Option<&str>, style: Option<serde_json::Value>) {
            tree.create_element(id, kind.to_string());
            if let Some(text) = text {
                tree.set_text(id, text.to_string());
            }
            if let Some(style) = style {
                tree.set_style(id, std::sync::Arc::new(serde_json::from_value(style).unwrap()));
            }
            tree.append_child(1, id);
        }
        assert!(is_empty(&tree, &tree.elements[&1]));
        add(&mut tree, 2, "text", Some(" \n "), None);
        add(&mut tree, 3, "div", None, Some(serde_json::json!({"display": "none"})));
        assert!(is_empty(&tree, &tree.elements[&1]));
        add(&mut tree, 4, "text", Some("\u{a0}"), None);
        assert!(!is_empty(&tree, &tree.elements[&1]), "a no-break space is content");
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        tree.create_element(2, "div".to_string());
        tree.append_child(1, 2);
        assert!(!is_empty(&tree, &tree.elements[&1]), "a box is content even when it paints nothing");
    }
}
