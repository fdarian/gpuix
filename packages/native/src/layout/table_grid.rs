//! Where the cells of a table are in its grid, which the borders of a collapsed table need to know before
//! layout. It is the placement zed's table layout does (`taffy/src/compute/table/structure.rs`), on the retained
//! tree: the same rows, the same slots filled by `colSpan` and `rowSpan`, the same columns merged away. The two
//! agree because the table paints its borders by the lines the layout recorded, and gives up when the grids differ.


use crate::layout::table::{Part, cell_spans, column_span, is_out_of_flow, part};
use crate::retained_tree::{RetainedElement, RetainedTree};
use crate::text::inline::Child;

/// What a cell is called in the map of its borders: the element of a cell, or the n-th anonymous cell a
/// part holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CellKey {
    Element(u64),
    Anonymous { container: u64, ordinal: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Cell {
    pub key: CellKey,
    pub row: usize,
    pub col: usize,
    pub col_span: usize,
    pub row_span: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Row {
    /// `None` for an implicit row.
    pub element: Option<u64>,
    /// Index into [`Grid::sections`].
    pub section: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Section {
    /// `None` for the rows that are children of the table itself.
    pub element: Option<u64>,
    pub first_row: usize,
    pub row_count: usize,
}

/// A `table-column`, or a `table-column-group`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Columns {
    pub element: u64,
    pub first: usize,
    pub span: usize,
    /// For a column group: whether its columns are the `table-column`s inside it, where it otherwise stands
    /// for as many as its `span`.
    pub has_columns: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Grid {
    pub columns: usize,
    pub cells: Vec<Cell>,
    /// In layout order: the header group first and the footer group last.
    pub rows: Vec<Row>,
    pub sections: Vec<Section>,
    pub column_nodes: Vec<Columns>,
    pub column_groups: Vec<Columns>,
}

/// The kind of a row group, which decides where it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Header,
    Body,
    Footer,
}

enum RowSource {
    Row(u64),
    /// The cells of an implicit row.
    Implicit(Vec<Placed>),
}

struct SectionSource {
    element: Option<u64>,
    kind: Kind,
    rows: Vec<RowSource>,
}

/// A cell before it has a place.
#[derive(Clone, Copy)]
struct Placed {
    key: CellKey,
    col_span: usize,
    row_span: usize,
}

const MAX_COL_SPAN: usize = 1000;
const MAX_ROW_SPAN: usize = 65534;

impl Grid {
    /// The grid of `table`. `merge_empty_columns` is what the layout does unless the table has a fixed
    /// layout and a width: a column no cell starts in joins the one before it.
    pub(crate) fn build(tree: &RetainedTree, table: &RetainedElement, merge_empty_columns: bool) -> Grid {
        let mut grid = Grid::default();
        let mut sources: Vec<SectionSource> = Vec::new();
        let mut explicit_columns = 0;
        let mut run: Vec<Placed> = Vec::new();
        let mut column_nodes = Vec::new();
        let mut column_groups = Vec::new();

        for item in items(tree, table) {
            let (key, element) = match item {
                Item::Anonymous(key) => (key, None),
                Item::Element(element) => (CellKey::Element(element.id), Some(element)),
            };
            let Some(element) = element else {
                run.push(Placed { key, col_span: 1, row_span: 1 });
                continue;
            };
            // The table places its out of flow children itself.
            if is_out_of_flow(element) {
                continue;
            }
            match part(element) {
                None => unreachable!("a child of a table that is not a part is out of flow or in an anonymous cell"),
                Some(Part::Cell) => run.push(placed(element)),
                // A table inside a table is a cell, as in the layout.
                Some(Part::Table) => run.push(Placed { key, col_span: 1, row_span: 1 }),
                Some(Part::Caption) => flush(&mut sources, &mut run),
                Some(Part::Column) => {
                    flush(&mut sources, &mut run);
                    let span = column_span(element);
                    column_nodes.push(Columns { element: element.id, first: explicit_columns, span, has_columns: false });
                    explicit_columns += span;
                }
                Some(Part::ColumnGroup) => {
                    flush(&mut sources, &mut run);
                    let first = explicit_columns;
                    let mut inside = 0;
                    for column in items(tree, element) {
                        if let Item::Element(column) = column {
                            let span = column_span(column);
                            column_nodes.push(Columns {
                                element: column.id,
                                first: first + inside,
                                span,
                                has_columns: false,
                            });
                            inside += span;
                        }
                    }
                    let has_columns = inside > 0;
                    if !has_columns {
                        inside = column_span(element);
                    }
                    column_groups.push(Columns { element: element.id, first, span: inside, has_columns });
                    explicit_columns += inside;
                }
                Some(Part::Row) => {
                    flush(&mut sources, &mut run);
                    anonymous_section(&mut sources).rows.push(RowSource::Row(element.id));
                }
                Some(Part::RowGroup(kind)) => {
                    flush(&mut sources, &mut run);
                    let kind = match kind {
                        gpui::RowGroupKind::Header => Kind::Header,
                        gpui::RowGroupKind::Body => Kind::Body,
                        gpui::RowGroupKind::Footer => Kind::Footer,
                    };
                    sources.push(read_group(tree, element, kind));
                }
            }
        }
        flush(&mut sources, &mut run);

        grid.columns = explicit_columns;
        grid.column_nodes = column_nodes;
        grid.column_groups = column_groups;
        grid.place(tree, sources);
        if merge_empty_columns {
            grid.merge_empty_columns(tree);
        }
        grid
    }

    fn place(&mut self, tree: &RetainedTree, sources: Vec<SectionSource>) {
        let header = sources.iter().position(|source| source.element.is_some() && source.kind == Kind::Header);
        let footer = sources.iter().position(|source| source.element.is_some() && source.kind == Kind::Footer);
        let mut order: Vec<usize> = Vec::new();
        order.extend(header);
        order.extend((0..sources.len()).filter(|index| Some(*index) != header && Some(*index) != footer));
        order.extend(footer);
        let mut sources: Vec<Option<SectionSource>> = sources.into_iter().map(Some).collect();
        for index in order {
            let source = sources[index].take().expect("every group is placed once");
            self.place_section(tree, source);
        }
    }

    fn place_section(&mut self, tree: &RetainedTree, source: SectionSource) {
        let section = self.sections.len();
        let first_row = self.rows.len();
        let row_count = source.rows.len();
        let mut busy_until: Vec<usize> = Vec::new();
        for (row_in_section, row_source) in source.rows.into_iter().enumerate() {
            let (element, children) = match row_source {
                RowSource::Row(id) => {
                    let row = &tree.elements[&id];
                    let children = items(tree, row)
                        .into_iter()
                        .map(|item| match item {
                            Item::Anonymous(key) => Placed { key, col_span: 1, row_span: 1 },
                            Item::Element(element) => placed(element),
                        })
                        .collect();
                    (Some(id), children)
                }
                RowSource::Implicit(cells) => (None, cells),
            };
            let row = self.rows.len();
            let mut col = 0;
            for child in children {
                let col_span = child.col_span.clamp(1, MAX_COL_SPAN);
                // A `rowspan` of 0 reaches the end of the group.
                let row_span = if child.row_span == 0 { MAX_ROW_SPAN } else { child.row_span };
                let row_span = row_span.min(row_count - row_in_section);
                while col < busy_until.len() && busy_until[col] > row_in_section {
                    col += 1;
                }
                if busy_until.len() < col + col_span {
                    busy_until.resize(col + col_span, 0);
                }
                for busy in &mut busy_until[col..col + col_span] {
                    *busy = row_in_section + row_span;
                }
                self.cells.push(Cell { key: child.key, row, col, col_span, row_span });
                self.columns = self.columns.max(col + col_span);
                col += col_span;
            }
            self.rows.push(Row { element, section });
        }
        self.sections.push(Section { element: source.element, first_row, row_count });
    }

    /// A column no cell starts in, and no column node gives a width, joins the column before it.
    fn merge_empty_columns(&mut self, tree: &RetainedTree) {
        let raw = self.columns;
        let mut keep = vec![false; raw];
        for cell in &self.cells {
            keep[cell.col] = true;
        }
        let has_width = |id: u64| tree.elements.get(&id).is_some_and(declares_width);
        for node in self.column_nodes.iter().filter(|node| has_width(node.element)) {
            keep[node.first..node.first + node.span].fill(true);
        }
        for group in self.column_groups.iter().filter(|group| has_width(group.element) && !group.has_columns) {
            keep[group.first..group.first + group.span].fill(true);
        }
        let mut merged_into: Vec<Option<usize>> = Vec::with_capacity(raw);
        let mut kept = 0;
        for &is_kept in &keep {
            kept += is_kept as usize;
            merged_into.push(kept.checked_sub(1));
        }
        for cell in &mut self.cells {
            let first = merged_into[cell.col].expect("a column with a cell is kept");
            let last = merged_into[cell.col + cell.col_span - 1].expect("it ends where it starts or after");
            cell.col = first;
            cell.col_span = last - first + 1;
        }
        let remap = |first_col: usize, span: usize| -> (usize, usize) {
            let mut kept_columns = (first_col..first_col + span).filter(|column| keep[*column]);
            match kept_columns.next() {
                Some(first) => {
                    let last = kept_columns.last().unwrap_or(first);
                    let first = merged_into[first].expect("kept");
                    (first, merged_into[last].expect("kept") - first + 1)
                }
                None => (merged_into[first_col].unwrap_or(0), 0),
            }
        };
        for node in self.column_nodes.iter_mut().chain(self.column_groups.iter_mut()) {
            (node.first, node.span) = remap(node.first, node.span);
        }
        self.columns = kept;
    }
}

fn declares_width(element: &RetainedElement) -> bool {
    use crate::style::DimensionValue;
    element
        .style
        .as_deref()
        .and_then(|style| style.width.as_ref())
        .is_some_and(|width| !matches!(width, DimensionValue::Auto))
}

fn placed(element: &RetainedElement) -> Placed {
    match part(element) {
        Some(Part::Cell) => {
            let (col_span, row_span) = cell_spans(element);
            Placed { key: CellKey::Element(element.id), col_span, row_span }
        }
        _ => Placed { key: CellKey::Element(element.id), col_span: 1, row_span: 1 },
    }
}

/// A child of a table part as the plan gives it: an element, or a cell with none.
enum Item<'a> {
    Anonymous(CellKey),
    Element(&'a RetainedElement),
}

/// The children `build_element` builds below a table part, in order. An anonymous cell is named by the
/// part and its place among the part's anonymous cells.
fn items<'a>(tree: &'a RetainedTree, container: &'a RetainedElement) -> Vec<Item<'a>> {
    let mut ordinal = 0;
    crate::text::inline::plan_children(tree, container)
        .into_iter()
        .filter_map(|child| match child {
            Child::Block(id) => tree.elements.get(&id).map(Item::Element),
            Child::AnonymousCell(_) => {
                ordinal += 1;
                Some(Item::Anonymous(CellKey::Anonymous { container: container.id, ordinal: ordinal - 1 }))
            }
            Child::Paragraph(_) => None,
        })
        .collect()
}

fn read_group(tree: &RetainedTree, group: &RetainedElement, kind: Kind) -> SectionSource {
    let mut source = SectionSource { element: Some(group.id), kind, rows: Vec::new() };
    let mut run: Vec<Placed> = Vec::new();
    for item in items(tree, group) {
        match item {
            Item::Element(element) if part(element) == Some(Part::Row) => {
                if !run.is_empty() {
                    source.rows.push(RowSource::Implicit(std::mem::take(&mut run)));
                }
                source.rows.push(RowSource::Row(element.id));
            }
            Item::Element(element) => run.push(placed(element)),
            Item::Anonymous(key) => run.push(Placed { key, col_span: 1, row_span: 1 }),
        }
    }
    if !run.is_empty() {
        source.rows.push(RowSource::Implicit(run));
    }
    source
}

/// The cells that are children of the table, collected into one implicit row.
fn flush(sources: &mut Vec<SectionSource>, run: &mut Vec<Placed>) {
    if !run.is_empty() {
        anonymous_section(sources).rows.push(RowSource::Implicit(std::mem::take(run)));
    }
}

/// The group the rows that are children of the table go into: one per run of them.
fn anonymous_section(sources: &mut Vec<SectionSource>) -> &mut SectionSource {
    if sources.last().is_none_or(|last| last.element.is_some()) {
        sources.push(SectionSource { element: None, kind: Kind::Body, rows: Vec::new() });
    }
    sources.last_mut().expect("just pushed")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        tree: RetainedTree,
    }

    impl Fixture {
        fn new() -> Self {
            let mut fixture = Fixture { tree: RetainedTree::new() };
            fixture.part(1, None, "table");
            fixture
        }

        fn part(&mut self, id: u64, parent: Option<u64>, display: &str) -> &mut Self {
            self.tree.create_element(id, "div".to_string());
            self.tree.set_style(id, std::sync::Arc::new(serde_json::from_value(serde_json::json!({ "display": display })).unwrap()));
            if let Some(parent) = parent {
                self.tree.append_child(parent, id);
            }
            self
        }

        fn cell(&mut self, id: u64, parent: u64, col_span: u32, row_span: u32) -> &mut Self {
            self.part(id, Some(parent), "table-cell");
            self.tree.set_custom_prop(id, "colSpan".to_string(), serde_json::json!(col_span));
            self.tree.set_custom_prop(id, "rowSpan".to_string(), serde_json::json!(row_span));
            self
        }

        fn text(&mut self, parent: u64, id: u64, content: &str) -> &mut Self {
            self.tree.create_element(id, "text".to_string());
            self.tree.set_text(id, content.to_string());
            self.tree.append_child(parent, id);
            self
        }

        fn grid(&self, merge: bool) -> Grid {
            Grid::build(&self.tree, &self.tree.elements[&1], merge)
        }
    }

    /// `(id, row, col, col_span, row_span)` of every cell, in source order.
    fn places(grid: &Grid) -> Vec<(u64, usize, usize, usize, usize)> {
        grid.cells
            .iter()
            .map(|cell| match cell.key {
                CellKey::Element(id) => (id, cell.row, cell.col, cell.col_span, cell.row_span),
                CellKey::Anonymous { container, ordinal } => (container * 100 + ordinal as u64, cell.row, cell.col, cell.col_span, cell.row_span),
            })
            .collect()
    }

    #[test]
    fn a_cell_that_spans_rows_keeps_its_slots_from_the_cells_of_the_next_rows() {
        let mut f = Fixture::new();
        f.part(2, Some(1), "table-row").part(3, Some(1), "table-row");
        f.cell(4, 2, 1, 2).cell(5, 2, 1, 1).cell(6, 3, 1, 1);
        let grid = f.grid(true);
        assert_eq!(places(&grid), vec![(4, 0, 0, 1, 2), (5, 0, 1, 1, 1), (6, 1, 1, 1, 1)]);
        assert_eq!(grid.columns, 2);
        // A row span of 0 reaches the end of the group, and not past it.
        let mut f = Fixture::new();
        f.part(2, Some(1), "table-row").part(3, Some(1), "table-row");
        f.cell(4, 2, 1, 0).cell(5, 3, 1, 1);
        assert_eq!(places(&f.grid(true)), vec![(4, 0, 0, 1, 2), (5, 1, 1, 1, 1)]);
    }

    #[test]
    fn the_header_group_is_first_and_the_footer_group_last_wherever_they_are() {
        let mut f = Fixture::new();
        f.part(2, Some(1), "table-footer-group").part(3, Some(1), "table-row-group").part(4, Some(1), "table-header-group");
        for (group, row, cell) in [(2, 12, 22), (3, 13, 23), (4, 14, 24)] {
            f.part(row, Some(group), "table-row").cell(cell, row, 1, 1);
        }
        let grid = f.grid(true);
        let rows: Vec<_> = grid.rows.iter().map(|row| row.element.unwrap()).collect();
        assert_eq!(rows, vec![14, 13, 12]);
        let sections: Vec<_> = grid.sections.iter().map(|section| section.element.unwrap()).collect();
        assert_eq!(sections, vec![4, 3, 2]);
        assert_eq!(places(&grid), vec![(24, 0, 0, 1, 1), (23, 1, 0, 1, 1), (22, 2, 0, 1, 1)]);
    }

    #[test]
    fn cells_and_text_directly_in_a_table_form_implicit_rows_around_the_rows() {
        let mut f = Fixture::new();
        f.cell(2, 1, 1, 1).cell(3, 1, 1, 1);
        f.part(4, Some(1), "table-row").cell(5, 4, 1, 1);
        f.text(1, 6, "text");
        let grid = f.grid(true);
        // Two cells in one implicit row, a row, and the anonymous cell the text goes in, in a row of its own.
        assert_eq!(
            places(&grid),
            vec![(2, 0, 0, 1, 1), (3, 0, 1, 1, 1), (5, 1, 0, 1, 1), (100, 2, 0, 1, 1)]
        );
        assert_eq!(grid.rows.iter().map(|row| row.element).collect::<Vec<_>>(), vec![None, Some(4), None]);
    }

    #[test]
    fn consecutive_text_and_boxes_in_a_row_are_one_anonymous_cell() {
        let mut f = Fixture::new();
        f.part(2, Some(1), "table-row");
        f.text(2, 3, "a").part(4, Some(2), "block").text(2, 5, "b");
        f.cell(6, 2, 1, 1).text(2, 7, "c");
        let grid = f.grid(true);
        assert_eq!(places(&grid), vec![(200, 0, 0, 1, 1), (6, 0, 1, 1, 1), (201, 0, 2, 1, 1)]);
    }

    #[test]
    fn a_column_no_cell_starts_in_joins_the_one_before_unless_the_layout_is_fixed() {
        let mut f = Fixture::new();
        f.part(2, Some(1), "table-row").part(3, Some(1), "table-row");
        f.cell(4, 2, 3, 1).cell(5, 3, 1, 1).cell(6, 3, 1, 1);
        let merged = f.grid(true);
        assert_eq!(merged.columns, 2);
        assert_eq!(places(&merged), vec![(4, 0, 0, 2, 1), (5, 1, 0, 1, 1), (6, 1, 1, 1, 1)]);
        let fixed = f.grid(false);
        assert_eq!(fixed.columns, 3);
        assert_eq!(places(&fixed), vec![(4, 0, 0, 3, 1), (5, 1, 0, 1, 1), (6, 1, 1, 1, 1)]);
    }

    #[test]
    fn column_nodes_count_their_spans_and_a_column_group_without_columns_its_own() {
        let mut f = Fixture::new();
        f.part(2, Some(1), "table-column-group");
        f.part(3, Some(2), "table-column").part(4, Some(2), "table-column");
        f.part(5, Some(1), "table-column-group");
        f.tree.set_custom_prop(5, "span".to_string(), serde_json::json!(2));
        f.part(6, Some(1), "table-row").cell(7, 6, 1, 1);
        let grid = f.grid(false);
        assert_eq!(grid.columns, 4);
        let spans: Vec<_> = grid.column_groups.iter().map(|group| (group.first, group.span, group.has_columns)).collect();
        assert_eq!(spans, vec![(0, 2, true), (2, 2, false)]);
        assert_eq!(grid.column_nodes.iter().map(|node| node.first).collect::<Vec<_>>(), vec![0, 1]);
    }
}
