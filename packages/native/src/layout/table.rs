//! CSS tables: which `display` values are table parts, what an element hands to GPUI to be one,
//! and which children of a table are wrapped in an anonymous cell.
//!
//! The layout is zed's (`taffy/src/compute/table`): a `display: table` node lays out the cells
//! and captions below it itself. Every part but the table is a block box with a
//! [`gpui::TableRole`], so a part outside a table, which the table never reads, lays out as the
//! block CSS would wrap in an anonymous table (without the wrapper).

use std::collections::HashMap;
use std::sync::Arc;

use gpui::Styled;

use crate::css::table::TableInherited;
use crate::layout::table_grid::BoxKey;
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

/// The table part a `display` value stands for. `inline-table` is a `table` that the inline
/// formatting context places as an atomic inline.
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

/// Whether `containerType` makes the element a query container (and gives it size containment). In CSS
/// the table box and the internal table boxes (row groups, rows, cells, columns and column groups) are not
/// containers, whatever they declare; a caption is.
pub(crate) fn can_be_container(element: &RetainedElement) -> bool {
    matches!(part(element), None | Some(Part::Caption))
}

/// The part the element is, by the `display` its `@container` blocks gave it when they select one
/// ([`RetainedElement::display`]).
pub(crate) fn part(element: &RetainedElement) -> Option<Part> {
    element.display().as_deref().and_then(part_of)
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
        // A table in a table part is not a part of it: it is content of a cell, as CSS wraps it.
        if part(child).is_some_and(|part| part != Part::Table) || is_out_of_flow(child) {
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

/// What the tables with collapsed borders built this frame handed to their parts: a cell finds its
/// border widths here when it is built, after the table.
#[derive(Default)]
pub(crate) struct Collapsed {
    cells: HashMap<BoxKey, [f32; 4]>,
    tables: HashMap<BoxKey, ([f32; 4], Arc<gpui::CollapsedBorders>)>,
    /// Whose border each edge of a table has, for [`Self::repaint`].
    owners: HashMap<BoxKey, super::collapse::Owners>,
    /// The borders of the parts of a table, as it resolved the edges with them. A part that is painted
    /// with other widths or styles afterwards (it moves in a transition) makes the table resolve again.
    used: Vec<(u64, [crate::css::border::SideBorder; 4])>,
}

impl Collapsed {
    pub(crate) fn note_used(&mut self, used: impl IntoIterator<Item = (u64, [crate::css::border::SideBorder; 4])>) {
        self.used.extend(used);
    }

    /// Takes the parts noted since `from`: the ones of the table that was just built.
    pub(crate) fn used_since(&mut self, from: usize) -> Vec<(u64, [crate::css::border::SideBorder; 4])> {
        self.used.split_off(from)
    }

    pub(crate) fn used_len(&self) -> usize {
        self.used.len()
    }

    pub(crate) fn insert(&mut self, table: BoxKey, resolved: super::collapse::Resolved) {
        self.cells.extend(resolved.cells);
        self.tables.insert(table, (resolved.table, resolved.borders));
        self.owners.insert(table, resolved.owners);
    }

    /// Gives the edges of `table` the colours their owners were painted with: the parts of a table are built
    /// after it resolved the edges with the colours of their last build, which a transition has moved.
    pub(crate) fn repaint(&mut self, table: BoxKey, painted: &dyn Fn(u64) -> Option<[crate::css::border::SideBorder; 4]>) {
        let (Some((_, borders)), Some(owners)) = (self.tables.get_mut(&table), self.owners.get(&table)) else {
            return;
        };
        *borders = Arc::new(super::collapse::recolored(borders, owners, painted));
    }

    pub(crate) fn table(&self, key: BoxKey) -> Option<&([f32; 4], Arc<gpui::CollapsedBorders>)> {
        self.tables.get(&key)
    }

    /// The border widths of a cell of a table with collapsed borders, `[top, right, bottom, left]`: a cell
    /// that is in no table has none.
    pub(crate) fn cell(&self, key: BoxKey) -> [f32; 4] {
        self.cells.get(&key).copied().unwrap_or_default()
    }
}

/// What the text below `element` knows of the table around it, for copying: which row it is in, and the cell.
/// The cells directly in a table or a row group are in one implicit row, named by the table or group.
pub(crate) fn enter(row: &mut Option<u64>, cell: &mut Option<crate::text::selection::TableCell>, element: &RetainedElement) {
    let token = BoxKey::Element(element.id).token();
    match part(element) {
        Some(Part::Table | Part::RowGroup(_) | Part::Row) => {
            *row = Some(token);
            *cell = None;
        }
        Some(Part::Cell) => {
            *cell = row.map(|row| crate::text::selection::TableCell { row, cell: token });
        }
        _ => {}
    }
}

/// The columns and rows a cell covers, as `colSpan` and `rowSpan` ask for.
pub(crate) fn cell_spans(element: &RetainedElement) -> (usize, usize) {
    (span_prop(element, "colSpan", 1, 1, 1000) as usize, span_prop(element, "rowSpan", 1, 0, 65534) as usize)
}

/// The columns a `table-column` or `table-column-group` stands for.
pub(crate) fn column_span(element: &RetainedElement) -> usize {
    span_prop(element, "span", 1, 1, 1000) as usize
}

/// The table roles and styles of `element`, which `style` (the final one, with its states
/// folded) makes a part.
pub(crate) fn apply<E: Styled + gpui::InteractiveElement>(
    mut el: E,
    tree: &RetainedTree,
    element: &RetainedElement,
    style: Option<&StyleDesc>,
    table: &TableInherited,
    collapsed: &Collapsed,
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
            if let Some(collapsed) = table.collapse.then(|| collapsed.tables.get(&BoxKey::Element(element.id))).flatten() {
                el = collapsed_table(el, collapsed);
            }
        }
        // Borders of these apply only through the borders of the cells they meet, when collapsed.
        Part::RowGroup(_) | Part::Row | Part::Column | Part::ColumnGroup => {
            el = without_borders(el);
            el = match part {
                Part::RowGroup(kind) => el.table_role(gpui::TableRole::RowGroup(kind)),
                Part::Row => el.table_role(gpui::TableRole::Row),
                Part::Column => el.table_role(gpui::TableRole::Column { span: column_span(element) as u16 }),
                _ => el.table_role(gpui::TableRole::ColumnGroup { span: column_span(element) as u16 }),
            };
        }
        Part::Cell => {
            let (col_span, row_span) = cell_spans(element);
            el = el.table_role(gpui::TableRole::Cell { col_span: col_span as u16, row_span: row_span as u16 });
            el = el.cell_vertical_align(cell_vertical_align(style));
            if table.collapse {
                el = collapsed_widths(el, collapsed.cell(BoxKey::Element(element.id)));
            } else if table.hide_empty_cells && is_empty(tree, element) {
                el = without_decoration(el);
            }
        }
        Part::Caption => {
            let side = if table.caption_bottom { gpui::CaptionSide::Bottom } else { gpui::CaptionSide::Top };
            el = el.table_role(gpui::TableRole::Caption(side));
        }
    }
    el
}

/// A table with collapsed borders: the widths its outer borders leave it, no padding and no radius (they do not
/// apply to it), and the borders to paint.
fn collapsed_table<E: Styled + gpui::InteractiveElement>(
    mut el: E,
    (halves, borders): &([f32; 4], Arc<gpui::CollapsedBorders>),
) -> E {
    el = collapsed_widths(el, *halves);
    let zero = Some(gpui::px(0.0).into());
    let square = Some(gpui::CornerRadius::default());
    let style = el.style();
    style.padding = gpui::EdgesRefinement { top: zero, right: zero, bottom: zero, left: zero };
    style.corner_radii =
        gpui::CornersRefinement { top_left: square, top_right: square, bottom_right: square, bottom_left: square };
    el.collapsed_borders(borders.clone())
}

/// The table `key` with the borders [`Collapsed::repaint`] gave it, once its parts are built.
pub(crate) fn repainted<E: gpui::InteractiveElement>(el: E, collapsed: &Collapsed, key: BoxKey) -> E {
    match collapsed.tables.get(&key) {
        Some((_, borders)) => el.collapsed_borders(borders.clone()),
        None => el,
    }
}

/// The border widths of a box of a table with collapsed borders, which the table paints.
fn collapsed_widths<E: Styled>(mut el: E, [top, right, bottom, left]: [f32; 4]) -> E {
    el = el.collapsed_border();
    let width = |width: f32| Some(gpui::px(width).into());
    el.style().border_widths = gpui::EdgesRefinement {
        top: width(top),
        right: width(right),
        bottom: width(bottom),
        left: width(left),
    };
    el
}

fn without_borders<E: Styled>(mut el: E) -> E {
    el.style().border_widths = gpui::EdgesRefinement {
        top: Some(gpui::px(0.0).into()),
        right: Some(gpui::px(0.0).into()),
        bottom: Some(gpui::px(0.0).into()),
        left: Some(gpui::px(0.0).into()),
    };
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
/// part around it. In a table with collapsed borders it has the widths its neighbours' borders
/// leave it.
pub(crate) fn anonymous_cell(collapsed: Option<[f32; 4]>) -> gpui::Div {
    use gpui::prelude::*;
    let mut cell = gpui::div().table_role(gpui::TableRole::Cell { col_span: 1, row_span: 1 });
    cell.style().position = Some(gpui::Position::Static);
    match collapsed {
        Some(halves) => collapsed_widths(cell, halves),
        None => cell,
    }
}

/// The anonymous table CSS 2.1 section 17.2.1 puts around table parts that are not in a table: a block with no style
/// of its own, which has the spacing and the collapsed borders its parent's style gives it.
pub(crate) fn anonymous_table(
    inherited: &TableInherited,
    collapsed: Option<&([f32; 4], Arc<gpui::CollapsedBorders>)>,
) -> gpui::Div {
    use gpui::prelude::*;
    let mut table = gpui::div()
        .table()
        .border_spacing(gpui::px(inherited.spacing.horizontal as f32), gpui::px(inherited.spacing.vertical as f32));
    table.style().position = Some(gpui::Position::Static);
    if inherited.collapse {
        table = table.border_collapse();
    }
    match collapsed {
        Some(collapsed) if inherited.collapse => collapsed_table(table, collapsed),
        _ => table,
    }
}

/// Wraps each run of table parts in `plan`, a flow container's, in an anonymous table (CSS 2.1 section 17.2.1): a
/// row, a row group, a cell, a caption, a column or a column group whose parent is not a table. White space between
/// the parts of a run makes nothing. Out of flow parts are blockified, and so are the parts of a flex or grid container,
/// which does not call this.
pub(crate) fn wrap_loose_parts(tree: &RetainedTree, plan: Vec<(Child, u32)>) -> Vec<(Child, u32)> {
    let is_loose = |child: &Child| match child {
        Child::Block(id) => tree
            .elements
            .get(id)
            .is_some_and(|element| matches!(part(element), Some(part) if part != Part::Table) && !is_out_of_flow(element)),
        _ => false,
    };
    if !plan.iter().any(|(child, _)| is_loose(child)) {
        return plan;
    }
    let mut out = Vec::with_capacity(plan.len());
    let mut run: Vec<(u64, u32)> = Vec::new();
    // White space after the last part of the run, which stays if nothing of the run follows it.
    let mut spaces: Vec<(Child, u32)> = Vec::new();
    fn close(out: &mut Vec<(Child, u32)>, run: &mut Vec<(u64, u32)>, spaces: &mut Vec<(Child, u32)>) {
        if let Some((_, at)) = run.first().copied() {
            out.push((Child::AnonymousTable(std::mem::take(run)), at));
        }
        out.append(spaces);
    }
    for (child, at) in plan {
        if is_loose(&child) {
            let Child::Block(id) = child else { unreachable!("only a block is loose") };
            run.push((id, at));
            spaces.clear();
        } else if !run.is_empty() && matches!(&child, Child::Paragraph(paragraph) if is_blank(&paragraph.text)) {
            spaces.push((child, at));
        } else {
            close(&mut out, &mut run, &mut spaces);
            out.push((child, at));
        }
    }
    close(&mut out, &mut run, &mut spaces);
    out
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
        let mut div = apply(gpui::div(), &RetainedTree::new(), &element, element.style.as_deref(), &TableInherited::default(), &Collapsed::default());
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
                Child::Block(_) | Child::AnonymousTable(_) | Child::EmptyLine => {}
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
                Child::EmptyLine => "line".to_string(),
                Child::AnonymousCell(cell) => {
                    let cell: Vec<Child> = cell.iter().map(|(child, _)| child.clone()).collect();
                    format!("cell[{}]", shape(&cell))
                }
                Child::AnonymousTable(parts) => format!("table{:?}", parts.iter().map(|(id, _)| *id).collect::<Vec<_>>()),
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

    #[test]
    fn consecutive_table_parts_in_a_block_are_one_anonymous_table() {
        let mut f = Fixture::new("block");
        f.part(2, "table-cell").part(3, "table-cell");
        f.part(4, "table-row");
        assert_eq!(shape(&f.plan()), "table[2, 3, 4]");
    }

    #[test]
    fn white_space_between_parts_makes_nothing_but_text_or_a_block_ends_the_table() {
        let mut f = Fixture::new("block");
        f.part(2, "table-row").text(1, 3, " \n ");
        f.part(4, "table-row");
        f.text(1, 5, "x");
        f.part(6, "table-cell");
        f.add(1, 7, "div");
        f.part(8, "table-caption");
        assert_eq!(shape(&f.plan()), r#"table[2, 4] "x" table[6] #7 table[8]"#);
    }

    #[test]
    fn a_table_inside_a_block_is_a_block_and_so_is_a_part_of_a_flex_container_or_out_of_flow() {
        let mut f = Fixture::new("block");
        f.part(2, "table").part(3, "table-cell");
        f.part(4, "table-cell");
        f.styled(4, serde_json::json!({"display": "table-cell", "position": "absolute"}));
        assert_eq!(shape(&f.plan()), "#2 table[3] #4");
        let mut f = Fixture::new("flex");
        f.part(2, "table-cell").part(3, "table-row");
        assert_eq!(shape(&f.plan()), "#2 #3");
        // A cell holds blocks, so a part in a cell is wrapped as in any block.
        let mut f = Fixture::new("table-cell");
        f.part(2, "table-cell");
        assert_eq!(shape(&f.plan()), "table[2]");
    }

    /// What the `@container` blocks of an element made of its `display`, as a build records it.
    fn container_display(f: &Fixture, id: u64, display: &str) {
        *f.tree.elements[&id].container_display.borrow_mut() = Some(Some(display.to_string()));
    }

    #[test]
    fn a_table_part_is_what_its_container_blocks_made_of_it() {
        let mut f = Fixture::new("table-row");
        f.part(2, "block").part(3, "table-cell").part(4, "block");
        assert_eq!(shape(&f.plan()), "cell[#2] #3 cell[#4]");
        container_display(&f, 2, "table-cell");
        container_display(&f, 3, "block");
        assert_eq!(shape(&f.plan()), "#2 cell[#3 #4]");
    }

    #[test]
    fn a_container_that_became_a_table_wraps_its_children_in_cells() {
        let mut f = Fixture::new("block");
        f.text(1, 2, "a");
        f.part(3, "table-row");
        assert_eq!(shape(&f.plan()), r#""a" table[3]"#);
        container_display(&f, 1, "table");
        assert_eq!(shape(&f.plan()), r#"cell["a"] #3"#);
    }

    #[test]
    fn parts_a_container_block_made_are_wrapped_in_an_anonymous_table_in_a_block() {
        let mut f = Fixture::new("block");
        f.part(2, "block").part(3, "block").part(4, "block");
        container_display(&f, 2, "table-cell");
        container_display(&f, 3, "table-cell");
        assert_eq!(shape(&f.plan()), "table[2, 3] #4");
    }

    #[test]
    fn an_element_a_container_block_made_a_part_is_no_query_container() {
        let mut f = Fixture::new("block");
        assert!(can_be_container(&f.tree.elements[&1]));
        container_display(&f, 1, "table-cell");
        assert!(!can_be_container(&f.tree.elements[&1]));
        container_display(&f, 1, "table-caption");
        assert!(can_be_container(&f.tree.elements[&1]));
        let mut f = Fixture::new("table-cell");
        assert!(!can_be_container(&f.tree.elements[&1]));
        container_display(&f, 1, "block");
        assert!(can_be_container(&f.tree.elements[&1]));
    }
}
