//! `border-collapse: collapse`: which border each edge of a table's grid has, and what is left of it for
//! each cell and for the table.
//!
//! An edge of the grid is the side of a slot (a cell or a hole where the grid has none) that the next slot or
//! the table's own edge is on the other side of. Every box that has a border there takes part: the cells on either
//! side, their rows, row groups and columns, column groups, and the table on its outer edges. CSS 2.1 section
//! 17.6.2.1 says which wins: `hidden` removes the border, otherwise the wider one wins, then the more elaborate style
//! (`double` over `solid` over `dashed`...), then the closer to the cell: cell, row, row group, column, column group,
//! table, and between two of a kind the one that is left or above.
//!
//! Layout is handed the halves of the border of an edge: each cell gets the widest border of each of its sides,
//! halved, as its border width, and the table the same for its outer edges. The table box holds the outer halves
//! of its outer borders, so the grid starts where they are centred. As in Chrome, the halves are exact: a 3px border
//! leaves 1.5px each side. Nothing but the table paints a border: the borders of the edges, which `gpui` paints once
//! each, centred on the lines the layout recorded, see `gpui::CollapsedBorders`.

use std::collections::HashMap;
use std::sync::Arc;

use crate::css::border::{LineStyle, SideBorder};
use crate::layout::table_grid::{BoxKey, Grid};

/// What the table and its cells are laid out with, and what is painted.
pub(crate) struct Resolved {
    /// The border widths of the table, `[top, right, bottom, left]`, in px.
    pub table: [f32; 4],
    pub cells: HashMap<BoxKey, [f32; 4]>,
    pub borders: Arc<gpui::CollapsedBorders>,
}

/// A border at an edge, with what ranks it against the others there.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    style: LineStyle,
    width: f32,
    color: gpui::Hsla,
    side: gpui::BorderSide,
    class: Class,
    /// 0 for the one that is left or above.
    order: u8,
    /// The slot whose box the border is on, which ranks the borders of equal strength where they cross: the
    /// one nearer the top left covers the others.
    owner: (usize, usize),
}

/// What kind of box a border is on, the weakest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    Table,
    ColumnGroup,
    Column,
    RowGroup,
    Row,
    Cell,
}

const TOP: usize = 0;
const RIGHT: usize = 1;
const BOTTOM: usize = 2;
const LEFT: usize = 3;

fn side_of(index: usize) -> gpui::BorderSide {
    [gpui::BorderSide::Top, gpui::BorderSide::Right, gpui::BorderSide::Bottom, gpui::BorderSide::Left][index]
}

impl Candidate {
    fn of(borders: &[SideBorder; 4], side: usize, class: Class, order: u8, owner: (usize, usize), scale: f32) -> Self {
        let border = &borders[side];
        Candidate {
            style: border.style,
            width: snap(f32::from(border.width), scale),
            color: border.color,
            side: side_of(side),
            class,
            order,
            owner,
        }
    }

    /// Where the border ranks among all of them.
    fn strength(&self, scale: f32) -> (u32, u8, Class, std::cmp::Reverse<(usize, usize)>) {
        ((self.width * scale) as u32, style_rank(self.style), self.class, std::cmp::Reverse(self.owner))
    }

    /// Greater wins.
    fn rank(&self) -> (f32, u8, Class) {
        (self.width, style_rank(self.style), self.class)
    }
}

/// `width` as whole device pixels, at least one for a border that is there: the width the layout and the paint of a
/// border of its own would use.
fn snap(width: f32, scale: f32) -> f32 {
    if width <= 0.0 {
        return 0.0;
    }
    ((width * scale).abs() - 0.5).ceil().max(1.0) / scale
}

/// How elaborate a style is, for two borders of the same width.
fn style_rank(style: LineStyle) -> u8 {
    match style {
        LineStyle::None | LineStyle::Hidden => 0,
        LineStyle::Inset => 1,
        LineStyle::Groove => 2,
        LineStyle::Outset => 3,
        LineStyle::Ridge => 4,
        LineStyle::Dotted => 5,
        LineStyle::Dashed => 6,
        LineStyle::Solid => 7,
        LineStyle::Double => 8,
    }
}

/// The border that wins the edge, or none: a `hidden` one, or nothing that is there.
fn winner(candidates: &[Candidate]) -> Option<Candidate> {
    if candidates.iter().any(|candidate| candidate.style == LineStyle::Hidden) {
        return None;
    }
    candidates
        .iter()
        .filter(|candidate| candidate.style.is_visible() && candidate.width > 0.0)
        .copied()
        .reduce(|best, next| {
            let (a, b) = (best.rank(), next.rank());
            let ordering = a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2));
            // Equal in every way: the one that is left or above.
            if ordering.is_gt() || (ordering.is_eq() && best.order <= next.order) { best } else { next }
        })
}

struct Slots<'a> {
    grid: &'a Grid,
    /// The cell in every slot, row by row.
    cell_at: Vec<Option<usize>>,
}

impl Slots<'_> {
    fn cell(&self, row: usize, col: usize) -> Option<usize> {
        self.cell_at[row * self.grid.columns + col]
    }
}

/// The borders of the table whose cells `grid` places, or `None` for a table without a slot. `own` is the borders of
/// the table itself. `borders_of` has the borders of every other box of the table. `scale` is the device pixels to a
/// pixel.
pub(crate) fn resolve(
    grid: Grid,
    own: [SideBorder; 4],
    scale: f32,
    borders_of: &dyn Fn(u64) -> [SideBorder; 4],
) -> Option<Resolved> {
    let (columns, rows) = (grid.columns, grid.rows.len());
    if columns == 0 || rows == 0 {
        return None;
    }
    let mut slots = Slots { grid: &grid, cell_at: vec![None; rows * columns] };
    for (index, cell) in grid.cells.iter().enumerate() {
        for row in cell.row..cell.row + cell.row_span {
            for col in cell.col..cell.col + cell.col_span {
                slots.cell_at[row * columns + col] = Some(index);
            }
        }
    }

    let element_borders = |id: Option<u64>| id.map(borders_of);
    let cell_borders: Vec<Option<[SideBorder; 4]>> = grid
        .cells
        .iter()
        .map(|cell| match cell.key {
            BoxKey::Element(id) => Some(borders_of(id)),
            BoxKey::Anonymous { .. } => None,
        })
        .collect();
    let row_borders: Vec<Option<[SideBorder; 4]>> = grid.rows.iter().map(|row| element_borders(row.element)).collect();
    let section_borders: Vec<Option<[SideBorder; 4]>> =
        grid.sections.iter().map(|section| element_borders(section.element)).collect();
    let column_borders: Vec<[SideBorder; 4]> = grid.column_nodes.iter().map(|node| borders_of(node.element)).collect();
    let group_borders: Vec<[SideBorder; 4]> = grid.column_groups.iter().map(|node| borders_of(node.element)).collect();
    let column_at = |nodes: &[crate::layout::table_grid::Columns], col: usize| {
        nodes.iter().position(|node| col >= node.first && col < node.first + node.span)
    };
    let candidate = |borders: &[SideBorder; 4], side: usize, class: Class, order: u8, owner: (usize, usize)| {
        Candidate::of(borders, side, class, order, owner, scale)
    };

    // The edge above row `row` at column `col`, `row` up to the number of rows.
    let horizontal = |row: usize, col: usize| -> Option<Candidate> {
        let above = row.checked_sub(1).and_then(|above| slots.cell(above, col));
        let below = (row < rows).then(|| slots.cell(row, col)).flatten();
        if above.is_some() && above == below {
            return None;
        }
        let mut found = Vec::new();
        let owner = |order: u8| if order == 0 { (row.saturating_sub(1), col) } else { (row.min(rows - 1), col) };
        if let Some(Some(borders)) = above.map(|cell| &cell_borders[cell]) {
            found.push(candidate(borders, BOTTOM, Class::Cell, 0, owner(0)));
        }
        if let Some(Some(borders)) = below.map(|cell| &cell_borders[cell]) {
            found.push(candidate(borders, TOP, Class::Cell, 1, owner(1)));
        }
        if let Some(above) = row.checked_sub(1) {
            if let Some(borders) = &row_borders[above] {
                found.push(candidate(borders, BOTTOM, Class::Row, 0, owner(0)));
            }
            let section = grid.rows[above].section;
            if grid.sections[section].first_row + grid.sections[section].row_count == row {
                if let Some(borders) = &section_borders[section] {
                    found.push(candidate(borders, BOTTOM, Class::RowGroup, 0, owner(0)));
                }
            }
        }
        if row < rows {
            if let Some(borders) = &row_borders[row] {
                found.push(candidate(borders, TOP, Class::Row, 1, owner(1)));
            }
            let section = grid.rows[row].section;
            if grid.sections[section].first_row == row {
                if let Some(borders) = &section_borders[section] {
                    found.push(candidate(borders, TOP, Class::RowGroup, 1, owner(1)));
                }
            }
        }
        if row == 0 || row == rows {
            let side = if row == 0 { TOP } else { BOTTOM };
            if let Some(node) = column_at(&grid.column_nodes, col) {
                found.push(candidate(&column_borders[node], side, Class::Column, 0, owner(0)));
            }
            if let Some(node) = column_at(&grid.column_groups, col) {
                found.push(candidate(&group_borders[node], side, Class::ColumnGroup, 0, owner(0)));
            }
            found.push(candidate(&own, side, Class::Table, 0, owner(0)));
        }
        winner(&found)
    };

    // The edge left of column `col` at row `row`, `col` up to the number of columns.
    let vertical = |row: usize, col: usize| -> Option<Candidate> {
        let left = col.checked_sub(1).and_then(|left| slots.cell(row, left));
        let right = (col < columns).then(|| slots.cell(row, col)).flatten();
        if left.is_some() && left == right {
            return None;
        }
        let mut found = Vec::new();
        let owner = |order: u8| if order == 0 { (row, col.saturating_sub(1)) } else { (row, col.min(columns - 1)) };
        if let Some(Some(borders)) = left.map(|cell| &cell_borders[cell]) {
            found.push(candidate(borders, RIGHT, Class::Cell, 0, owner(0)));
        }
        if let Some(Some(borders)) = right.map(|cell| &cell_borders[cell]) {
            found.push(candidate(borders, LEFT, Class::Cell, 1, owner(1)));
        }
        if col == 0 || col == columns {
            let side = if col == 0 { LEFT } else { RIGHT };
            if let Some(borders) = &row_borders[row] {
                found.push(candidate(borders, side, Class::Row, 0, owner(0)));
            }
            if let Some(borders) = &section_borders[grid.rows[row].section] {
                found.push(candidate(borders, side, Class::RowGroup, 0, owner(0)));
            }
            found.push(candidate(&own, side, Class::Table, 0, owner(0)));
        }
        // A column or a column group has its left border on its first column and its right border on its last.
        for (nodes, borders, class) in [
            (&grid.column_nodes, &column_borders, Class::Column),
            (&grid.column_groups, &group_borders, Class::ColumnGroup),
        ] {
            for (index, node) in nodes.iter().enumerate() {
                if node.span > 0 && node.first + node.span == col {
                    found.push(candidate(&borders[index], RIGHT, class, 0, owner(0)));
                }
                if node.span > 0 && node.first == col {
                    found.push(candidate(&borders[index], LEFT, class, 1, owner(1)));
                }
            }
        }
        winner(&found)
    };

    let horizontal_edges: Vec<Option<Candidate>> =
        (0..=rows).flat_map(|row| (0..columns).map(move |col| (row, col))).map(|(row, col)| horizontal(row, col)).collect();
    let vertical_edges: Vec<Option<Candidate>> =
        (0..rows).flat_map(|row| (0..=columns).map(move |col| (row, col))).map(|(row, col)| vertical(row, col)).collect();

    // The halves: each side of a cell has the widest border of the edges along it.
    let widest = |edges: &mut dyn Iterator<Item = &Option<Candidate>>| {
        edges.flatten().map(|candidate| candidate.width).fold(0.0, f32::max) / 2.0
    };
    let h = |row: usize, col: usize| &horizontal_edges[row * columns + col];
    let v = |row: usize, col: usize| &vertical_edges[row * (columns + 1) + col];
    let mut cells = HashMap::new();
    for cell in &grid.cells {
        let (cols, row_span) = (cell.col..cell.col + cell.col_span, cell.row..cell.row + cell.row_span);
        let halves = [
            widest(&mut cols.clone().map(|col| h(cell.row, col))),
            widest(&mut row_span.clone().map(|row| v(row, cell.col + cell.col_span))),
            widest(&mut cols.map(|col| h(cell.row + cell.row_span, col))),
            widest(&mut row_span.map(|row| v(row, cell.col))),
        ];
        cells.insert(cell.key, halves);
    }
    let table_halves = [
        widest(&mut (0..columns).map(|col| h(0, col))),
        widest(&mut (0..rows).map(|row| v(row, columns))),
        widest(&mut (0..columns).map(|col| h(rows, col))),
        widest(&mut (0..rows).map(|row| v(row, 0))),
    ];

    // Where borders cross, the stronger covers the other: the strength is the place of a border among all of them.
    let mut keys: Vec<_> =
        horizontal_edges.iter().chain(&vertical_edges).flatten().map(|candidate| candidate.strength(scale)).collect();
    keys.sort();
    keys.dedup();
    let border = |edge: &Option<Candidate>| {
        edge.map(|candidate| {
            let key = candidate.strength(scale);
            gpui::CollapsedBorder {
                width: gpui::px(candidate.width),
                style: candidate.style.painted().expect("a winning border is visible"),
                color: candidate.color,
                side: candidate.side,
                strength: keys.binary_search(&key).expect("every key is listed") as u32,
            }
        })
    };
    let borders = gpui::CollapsedBorders::new(
        columns,
        rows,
        horizontal_edges.iter().map(border).collect(),
        vertical_edges.iter().map(border).collect(),
    );
    Some(Resolved { table: table_halves, cells, borders: Arc::new(borders) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::border::LineStyle;
    use crate::retained_tree::RetainedTree;

    const SCALE: f32 = 1.0;

    fn side(style: LineStyle, width: f32, color: u32) -> SideBorder {
        SideBorder { style, width: gpui::px(width), color: gpui::rgb(color).into() }
    }

    fn solid(width: f32, color: u32) -> [SideBorder; 4] {
        [side(LineStyle::Solid, width, color); 4]
    }

    const NONE: [SideBorder; 4] =
        [SideBorder { style: LineStyle::None, width: gpui::px(0.0), color: gpui::Hsla { h: 0.0, s: 0.0, l: 0.0, a: 0.0 } }; 4];

    /// A tree of table parts: `part(id, parent, display)`, with the spans of a cell.
    struct Fixture {
        tree: RetainedTree,
        borders: HashMap<u64, [SideBorder; 4]>,
    }

    impl Fixture {
        fn new() -> Self {
            let mut fixture = Fixture { tree: RetainedTree::new(), borders: HashMap::new() };
            fixture.part(1, None, "table");
            fixture
        }

        fn part(&mut self, id: u64, parent: Option<u64>, display: &str) -> &mut Self {
            self.tree.create_element(id, "div".to_string());
            self.tree.set_style(id, Arc::new(serde_json::from_value(serde_json::json!({ "display": display })).unwrap()));
            if let Some(parent) = parent {
                self.tree.append_child(parent, id);
            }
            self
        }

        fn row(&mut self, id: u64, parent: u64) -> &mut Self {
            self.part(id, Some(parent), "table-row")
        }

        fn cell(&mut self, id: u64, parent: u64, borders: [SideBorder; 4]) -> &mut Self {
            self.part(id, Some(parent), "table-cell");
            self.borders.insert(id, borders);
            self
        }

        fn spanning(&mut self, id: u64, col_span: u32, row_span: u32) -> &mut Self {
            self.tree.set_custom_prop(id, "colSpan".to_string(), serde_json::json!(col_span));
            self.tree.set_custom_prop(id, "rowSpan".to_string(), serde_json::json!(row_span));
            self
        }

        fn borders(&mut self, id: u64, borders: [SideBorder; 4]) -> &mut Self {
            self.borders.insert(id, borders);
            self
        }

        fn resolve(&self) -> Resolved {
            let borders_of = |id: u64| self.borders.get(&id).copied().unwrap_or(NONE);
            let grid = Grid::build(&self.tree, &self.tree.elements[&1], true);
            resolve(grid, borders_of(1), SCALE, &borders_of).expect("a table with cells")
        }
    }

    fn halves(resolved: &Resolved, id: u64) -> [f32; 4] {
        resolved.cells[&BoxKey::Element(id)]
    }

    /// Two rows of two cells, the case the layout tests of zed check against Chrome: a 4px table border and cells of 3,
    /// 5, 1 and 2px. Chrome lays the cells out with exactly these halves (`taffy::compute::table::tests::collapsed`).
    ///
    /// ```html
    /// <table style="border-collapse:collapse;border:4px solid black">
    ///   <tr><td style="border:3px solid red">..</td><td style="border:5px solid blue">..</td></tr>
    ///   <tr><td style="border:1px solid green">..</td><td style="border:2px solid orange">..</td></tr>
    /// </table>
    /// ```
    fn two_by_two() -> Fixture {
        let mut f = Fixture::new();
        f.borders(1, solid(4.0, 0x000000));
        f.row(2, 1).row(3, 1);
        f.cell(4, 2, solid(3.0, 0xff0000)).cell(5, 2, solid(5.0, 0x0000ff));
        f.cell(6, 3, solid(1.0, 0x008000)).cell(7, 3, solid(2.0, 0xffa500));
        f
    }

    #[test]
    fn every_edge_has_the_wider_border_and_each_side_of_it_half() {
        let resolved = two_by_two().resolve();
        assert_eq!(resolved.table, [2.5, 2.5, 2.0, 2.0]);
        assert_eq!(halves(&resolved, 4), [2.0, 2.5, 1.5, 2.0]);
        assert_eq!(halves(&resolved, 5), [2.5, 2.5, 2.5, 2.5]);
        assert_eq!(halves(&resolved, 6), [1.5, 1.0, 2.0, 2.0]);
        assert_eq!(halves(&resolved, 7), [2.5, 2.0, 2.0, 1.0]);
    }

    fn color(border: Option<&gpui::CollapsedBorder>) -> Option<u32> {
        border.map(|border| {
            let rgba = gpui::Rgba::from(border.color);
            ((rgba.r * 255.0).round() as u32) << 16 | ((rgba.g * 255.0).round() as u32) << 8 | (rgba.b * 255.0).round() as u32
        })
    }

    #[test]
    fn the_wider_border_wins_and_at_equal_width_the_cell_on_the_left_or_above() {
        let borders = two_by_two().resolve().borders;
        // a and b meet in a 3px red and a 5px blue: blue; a and c: red 3, green 1: red.
        assert_eq!(color(borders.vertical(0, 1)), Some(0x0000ff));
        assert_eq!(color(borders.horizontal(1, 0)), Some(0xff0000));
        // The outer edges: the table's 4px against the cell's.
        assert_eq!(color(borders.vertical(0, 0)), Some(0x000000));
        assert_eq!(color(borders.horizontal(0, 1)), Some(0x0000ff));
        let mut f = Fixture::new();
        f.row(2, 1).cell(3, 2, solid(3.0, 0xff0000)).cell(4, 2, solid(3.0, 0x0000ff));
        assert_eq!(color(f.resolve().borders.vertical(0, 1)), Some(0xff0000), "the left cell");
        let mut f = Fixture::new();
        f.row(2, 1).row(3, 1).cell(4, 2, solid(3.0, 0xff0000)).cell(5, 3, solid(3.0, 0x0000ff));
        assert_eq!(color(f.resolve().borders.horizontal(1, 0)), Some(0xff0000), "the cell above");
    }

    #[test]
    fn at_equal_width_a_more_elaborate_style_wins_then_the_closer_box() {
        let mut f = Fixture::new();
        f.row(2, 1);
        f.cell(3, 2, [side(LineStyle::Dashed, 4.0, 0xff0000); 4]);
        f.cell(4, 2, [side(LineStyle::Double, 4.0, 0x0000ff); 4]);
        let border = f.resolve().borders.vertical(0, 1).cloned().unwrap();
        assert_eq!(border.style, gpui::BorderStyle::Double);
        // A row against a cell of the same width and style: the cell. Against a row group: the row.
        let mut f = Fixture::new();
        f.part(10, Some(1), "table-row-group");
        f.row(2, 10).cell(3, 2, solid(4.0, 0xff0000));
        f.borders(2, solid(4.0, 0x00ff00)).borders(10, solid(4.0, 0x0000ff));
        let borders = f.resolve().borders;
        assert_eq!(color(borders.horizontal(0, 0)), Some(0xff0000), "cell over row over row group over table");
        f.borders(3, NONE);
        let borders = f.resolve().borders;
        assert_eq!(color(borders.horizontal(0, 0)), Some(0x00ff00), "row over row group");
        f.borders(2, NONE);
        assert_eq!(color(f.resolve().borders.horizontal(0, 0)), Some(0x0000ff), "row group");
    }

    #[test]
    fn hidden_removes_the_border_whatever_is_wider_and_none_loses_to_everything() {
        let mut f = Fixture::new();
        f.row(2, 1);
        f.cell(3, 2, [side(LineStyle::Hidden, 0.0, 0x000000); 4]);
        f.cell(4, 2, solid(9.0, 0x0000ff));
        let resolved = f.resolve();
        assert_eq!(resolved.borders.vertical(0, 1), None);
        // The cell on the right keeps no left half.
        assert_eq!(halves(&resolved, 4), [4.5, 4.5, 4.5, 0.0]);
        let mut f = Fixture::new();
        f.row(2, 1);
        f.cell(3, 2, [side(LineStyle::None, 0.0, 0x000000); 4]);
        f.cell(4, 2, solid(1.0, 0x0000ff));
        assert_eq!(color(f.resolve().borders.vertical(0, 1)), Some(0x0000ff));
    }

    /// Chrome has the cell with the border of the widest of the edges along a side, halved: a cell next to two rows
    /// gets the greater of the two, and the one spanning columns gets the greater of what is below it.
    ///
    /// ```html
    /// <table style="border-collapse:collapse">
    ///   <tr><td rowspan="2" style="border:2px solid red">..</td><td style="border:6px solid blue">..</td></tr>
    ///   <tr><td style="border:4px solid green">..</td></tr>
    /// </table>
    /// ```
    /// The cell is 34 wide with 30 of content: 1 on its left, 3 on its right.
    #[test]
    fn a_spanning_cell_has_the_widest_border_along_its_side() {
        let mut f = Fixture::new();
        f.row(2, 1).row(3, 1);
        f.cell(4, 2, solid(2.0, 0xff0000)).spanning(4, 1, 2);
        f.cell(5, 2, solid(6.0, 0x0000ff)).cell(6, 3, solid(4.0, 0x008000));
        let resolved = f.resolve();
        assert_eq!(halves(&resolved, 4), [1.0, 3.0, 1.0, 1.0]);
        assert_eq!(halves(&resolved, 5), [3.0, 3.0, 3.0, 3.0]);
        assert_eq!(halves(&resolved, 6), [3.0, 2.0, 2.0, 2.0]);
        // The edge inside the spanning cell has none.
        let mut f = Fixture::new();
        f.row(2, 1).row(3, 1);
        f.cell(4, 2, solid(2.0, 0xff0000)).spanning(4, 1, 2);
        let borders = f.resolve().borders;
        assert_eq!(borders.horizontal(1, 0), None);
    }

    /// Chrome, a table with a column group of 6px green, a second column of 10px blue, a row group with a 12px top
    /// border, a row with an 8px bottom border and a cell with 2px:
    ///
    /// ```html
    /// <table style="border-collapse:collapse">
    ///   <colgroup style="border:6px solid green"><col><col style="border:10px solid blue"></colgroup>
    ///   <tbody style="border-top:12px solid purple">
    ///     <tr style="border-bottom:8px solid red"><td style="border:2px solid black">..</td><td>..</td></tr>
    ///     <tr><td>..</td><td>..</td></tr>
    ///   </tbody>
    /// </table>
    /// ```
    /// Its cells are `[3, 6, 28, 20]`, `[31, 6, 30, 20]`, `[3, 26, 28, 19]` and `[31, 26, 30, 19]` around 20 by 10 of content.
    #[test]
    fn rows_row_groups_columns_and_column_groups_take_part() {
        let mut f = Fixture::new();
        f.part(10, Some(1), "table-column-group").borders(10, solid(6.0, 0x008000));
        f.part(11, Some(10), "table-column").part(12, Some(10), "table-column").borders(12, solid(10.0, 0x0000ff));
        f.part(20, Some(1), "table-row-group");
        let mut top = NONE;
        top[0] = side(LineStyle::Solid, 12.0, 0x800080);
        f.borders(20, top);
        f.row(21, 20);
        let mut bottom = NONE;
        bottom[2] = side(LineStyle::Solid, 8.0, 0xff0000);
        f.borders(21, bottom);
        f.cell(22, 21, solid(2.0, 0x000000)).cell(23, 21, NONE);
        f.row(24, 20).cell(25, 24, NONE).cell(26, 24, NONE);
        let resolved = f.resolve();
        assert_eq!(resolved.table, [6.0, 5.0, 5.0, 3.0]);
        assert_eq!(halves(&resolved, 22), [6.0, 5.0, 4.0, 3.0]);
        assert_eq!(halves(&resolved, 23), [6.0, 5.0, 4.0, 5.0]);
        assert_eq!(halves(&resolved, 25), [4.0, 5.0, 3.0, 3.0]);
        assert_eq!(halves(&resolved, 26), [4.0, 5.0, 5.0, 5.0]);
        // The row's 8px red against the cell's 2px: red, but the blue column's 10px is wider where they cross.
        let borders = resolved.borders;
        assert_eq!(color(borders.horizontal(1, 0)), Some(0xff0000));
        assert_eq!(color(borders.vertical(0, 1)), Some(0x0000ff));
        assert_eq!(color(borders.horizontal(0, 0)), Some(0x800080));
    }

    /// Chrome covers the crossing of borders that rank alike with the one nearer the top left: the red bottom border of a
    /// is over the green right border of c, the blue one of b under it.
    #[test]
    fn borders_that_rank_alike_are_covered_by_the_one_nearer_the_top_left() {
        let mut f = Fixture::new();
        f.row(2, 1).row(3, 1);
        f.cell(4, 2, solid(3.0, 0xff0000)).cell(5, 2, solid(3.0, 0x0000ff));
        f.cell(6, 3, solid(3.0, 0x008000)).cell(7, 3, solid(3.0, 0xffa500));
        let borders = f.resolve().borders;
        let strength = |border: Option<&gpui::CollapsedBorder>| border.unwrap().strength;
        assert!(strength(borders.horizontal(1, 0)) > strength(borders.vertical(1, 1)), "a over c");
        assert!(strength(borders.horizontal(1, 0)) > strength(borders.horizontal(1, 1)), "a over b");
        assert!(strength(borders.vertical(0, 1)) > strength(borders.vertical(1, 1)), "a over c");
    }

    #[test]
    fn widths_are_whole_device_pixels() {
        // 1.4px at one device pixel to a pixel is 1px, 1.5 rounds toward zero, a hair is a pixel.
        assert_eq!(snap(1.4, 1.0), 1.0);
        assert_eq!(snap(1.5, 1.0), 1.0);
        assert_eq!(snap(1.6, 1.0), 2.0);
        assert_eq!(snap(0.1, 1.0), 1.0);
        assert_eq!(snap(0.0, 2.0), 0.0);
        assert_eq!(snap(1.4, 2.0), 1.5);
    }
}
