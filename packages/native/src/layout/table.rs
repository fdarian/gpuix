//! CSS tables: which `display` values are table parts, what an element hands to GPUI to be one,
//! and which children of a table are wrapped in an anonymous cell.
//!
//! The layout is zed's (`taffy/src/compute/table`): a `display: table` node lays out the cells
//! and captions below it itself. Every part but the table is a block box with a
//! [`gpui::TableRole`], so a part outside a table, which the table never reads, lays out as the
//! block CSS would wrap in an anonymous table (without the wrapper).

use gpui::Styled;

use crate::css::table::TableInherited;
use crate::layout::box_generation::is_display_none;
use crate::retained_tree::{RetainedElement, RetainedTree};
use crate::style::StyleDesc;
use crate::text::inline::{Child, is_blank, is_raw_text, plan_flow};

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

/// A child of a table that takes no part in the grid and so is not wrapped: an absolutely
/// positioned one is placed by the table itself.
pub(crate) fn is_out_of_flow(element: &RetainedElement) -> bool {
    matches!(
        element.style.as_deref().and_then(|style| style.position.as_deref()),
        Some("absolute" | "fixed")
    )
}

/// How a table part plans its children: the table parts as they are, and each run of consecutive
/// other children of a table, row group or row in one anonymous cell (CSS 2.1 section 17.2.1).
/// A run of nothing but white space is dropped, and an absolutely positioned child is out of the
/// run, as the table places it. A column has no children and a column group holds nothing but
/// columns. `None` for an element that is not one of these.
pub(crate) fn plan_children(
    tree: &RetainedTree,
    container: &RetainedElement,
    children: &[(u64, u32)],
    content: Option<&str>,
) -> Option<Vec<(Child, u32)>> {
    match part(container)? {
        Part::Table | Part::RowGroup(_) | Part::Row => Some(plan_cells(tree, container, children, content)),
        Part::Column => Some(Vec::new()),
        Part::ColumnGroup => Some(
            children
                .iter()
                .filter_map(|(id, at)| Some((tree.elements.get(id)?, *at)))
                .filter(|(child, _)| !is_display_none(child) && part(child) == Some(Part::Column))
                .map(|(child, at)| (Child::Block(child.id), at))
                .collect(),
        ),
        Part::Cell | Part::Caption => None,
    }
}

fn plan_cells(
    tree: &RetainedTree,
    container: &RetainedElement,
    children: &[(u64, u32)],
    content: Option<&str>,
) -> Vec<(Child, u32)> {
    struct Cells<'a> {
        tree: &'a RetainedTree,
        container: &'a RetainedElement,
        run: Vec<(u64, u32)>,
        leading: Option<&'a str>,
        index: usize,
        out: Vec<(Child, u32)>,
    }

    impl Cells<'_> {
        fn close(&mut self) {
            // The container's own text is in front of everything else, so in its first cell.
            let leading = self.leading.take();
            let run = std::mem::take(&mut self.run);
            let blank = run.iter().all(|(id, _)| {
                self.tree
                    .elements
                    .get(id)
                    .is_some_and(|child| is_raw_text(child) && child.content.as_deref().is_none_or(is_blank))
            });
            if leading.is_none() && blank {
                return;
            }
            let at = run.first().map_or(0, |(_, at)| *at);
            let cell = plan_flow(self.tree, self.container, &run, leading, false, &mut self.index);
            if !cell.is_empty() {
                self.out.push((Child::AnonymousCell(cell), at));
            }
        }
    }

    let mut cells = Cells {
        tree,
        container,
        run: Vec::new(),
        leading: content.filter(|content| !is_blank(content)),
        index: 0,
        out: Vec::new(),
    };
    for (child, at) in children
        .iter()
        .filter_map(|(id, at)| Some((tree.elements.get(id)?, *at)))
        .filter(|(child, _)| !is_display_none(child))
    {
        if part(child).is_some() || is_out_of_flow(child) {
            cells.close();
            cells.out.push((Child::Block(child.id), at));
        } else {
            cells.run.push((child.id, at));
        }
    }
    cells.close();
    cells.out
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
    cell.content.as_deref().is_none_or(is_blank)
        && cell.children.iter().filter_map(|id| tree.elements.get(id)).all(|child| {
            is_display_none(child) || (is_raw_text(child) && child.content.as_deref().is_none_or(is_blank))
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

    /// A tree whose root (id 1) is the container planned.
    struct Fixture {
        tree: RetainedTree,
    }

    impl Fixture {
        fn new(display: &str) -> Self {
            let mut f = Self { tree: RetainedTree::new() };
            f.tree.create_element(1, "div".to_string());
            f.styled(1, serde_json::json!({"display": display}));
            f
        }

        fn add(&mut self, parent: u64, id: u64, kind: &str) -> &mut Self {
            self.tree.create_element(id, kind.to_string());
            self.tree.append_child(parent, id);
            self
        }

        fn text(&mut self, parent: u64, id: u64, content: &str) -> &mut Self {
            self.add(parent, id, "text");
            self.tree.set_text(id, content.to_string());
            self
        }

        fn styled(&mut self, id: u64, json: serde_json::Value) -> &mut Self {
            self.tree.set_style(id, std::sync::Arc::new(serde_json::from_value(json).unwrap()));
            self
        }

        fn part(&mut self, id: u64, display: &str) -> &mut Self {
            self.add(1, id, "div");
            self.styled(id, serde_json::json!({"display": display}))
        }

        fn plan(&self) -> Vec<Child> {
            crate::text::inline::plan_children(&self.tree, &self.tree.elements[&1])
        }
    }

    fn paragraphs(plan: &[Child]) -> Vec<&crate::text::inline::Paragraph> {
        fn collect<'a>(child: &'a Child, out: &mut Vec<&'a crate::text::inline::Paragraph>) {
            match child {
                Child::Paragraph(paragraph) => out.push(paragraph),
                Child::AnonymousCell(cell) => cell.iter().for_each(|(child, _)| collect(child, out)),
                Child::Block(_) => {}
            }
        }
        let mut out = Vec::new();
        plan.iter().for_each(|child| collect(child, &mut out));
        out
    }

    /// What a table's plan holds, as `cell[...]` for an anonymous cell, `#id` for a block and the
    /// text for a paragraph.
    fn shape(plan: &[Child]) -> String {
        plan.iter()
            .map(|child| match child {
                Child::Block(id) => format!("#{id}"),
                Child::Paragraph(paragraph) => format!("{:?}", paragraph.text),
                Child::AnonymousCell(cell) => {
                    let cell: Vec<Child> = cell.iter().map(|(child, _)| child.clone()).collect();
                    format!("cell[{}]", shape(&cell))
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn text_in_a_table_is_wrapped_in_one_anonymous_cell() {
        let mut f = Fixture::new("table");
        f.text(1, 2, "hello ").text(1, 3, "world");
        assert_eq!(shape(&f.plan()), r#"cell["hello world"]"#);
    }

    #[test]
    fn consecutive_children_that_are_not_parts_share_a_cell_and_a_part_splits_them() {
        let mut f = Fixture::new("table-row");
        f.add(1, 2, "div");
        f.add(1, 3, "span").text(3, 4, "inline");
        f.part(5, "table-cell");
        f.add(1, 6, "div");
        f.add(1, 7, "div");
        assert_eq!(shape(&f.plan()), r#"cell[#2 "inline"] #5 cell[#6 #7]"#);
    }

    #[test]
    fn white_space_between_parts_is_dropped_but_not_white_space_inside_a_run() {
        let mut f = Fixture::new("table");
        f.text(1, 2, "\n  ");
        f.part(3, "table-row");
        f.text(1, 4, " ");
        f.part(5, "table-row");
        f.text(1, 6, "\n");
        assert_eq!(shape(&f.plan()), "#3 #5");
        let mut f = Fixture::new("table");
        f.text(1, 2, " a ");
        f.add(1, 3, "div");
        f.text(1, 4, " ");
        assert_eq!(shape(&f.plan()), r#"cell["a" #3]"#);
    }

    #[test]
    fn an_absolutely_positioned_child_and_display_none_do_not_make_a_cell() {
        let mut f = Fixture::new("table");
        f.add(1, 2, "div");
        f.styled(2, serde_json::json!({"position": "absolute"}));
        f.add(1, 3, "div");
        f.styled(3, serde_json::json!({"display": "none"}));
        f.text(1, 4, " ");
        f.add(1, 5, "div");
        f.styled(5, serde_json::json!({"position": "fixed"}));
        assert_eq!(shape(&f.plan()), "#2 #5");
    }

    #[test]
    fn a_cell_is_a_plain_block_container() {
        let mut f = Fixture::new("table-cell");
        f.text(1, 2, "a").add(1, 3, "div");
        f.text(1, 4, "b");
        assert_eq!(shape(&f.plan()), r#""a" #3 "b""#);
    }

    #[test]
    fn a_column_has_no_children_and_a_column_group_only_columns() {
        let mut f = Fixture::new("table-column");
        f.add(1, 2, "div");
        assert_eq!(shape(&f.plan()), "");
        let mut f = Fixture::new("table-column-group");
        f.part(2, "table-column");
        f.add(1, 3, "div");
        f.part(4, "table-column");
        f.text(1, 5, "x");
        assert_eq!(shape(&f.plan()), "#2 #4");
    }

    #[test]
    fn paragraphs_of_different_cells_have_different_selection_keys() {
        let mut f = Fixture::new("table");
        f.text(1, 2, "a");
        f.part(3, "table-row");
        f.text(1, 4, "b");
        let plan = f.plan();
        let keys: Vec<_> = paragraphs(&plan).iter().map(|paragraph| paragraph.key()).collect();
        assert_eq!(keys.len(), 2);
        assert_ne!(keys[0], keys[1]);
    }
}
