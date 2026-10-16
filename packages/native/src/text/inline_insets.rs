//! The space an inline box takes along the line.
//!
//! A `<span>`'s margin, border and padding on the inline axis push the text
//! around it, widen the line and take part in wrapping. gpui lays that out as an
//! inset around a byte range of the text ([`gpui::TextInset`]); this module reads
//! it from the boxes' styles, and works out where the border box is inside the
//! rectangles the layout gives back.
//!
//! The model is gpui's. `position_for_index(start)` of a box with a leading inset
//! is the left edge of its margin box, and the first glyph is painted after the
//! inset. The position after its last glyph is the right edge of the margin box.
//! Boxes that start or end on the same byte nest, outermost first:
//!
//! ```text
//! start:  | outer margin border padding | inner margin border padding | glyph
//! end:    glyph | inner padding border margin | outer padding border margin |
//! ```
//!
//! so the border box of a box is its first fragment moved in by its own margin
//! and by everything the boxes around it that start on the same byte put first,
//! and its last fragment moved in the same way from the right. A fragment that
//! continues from or onto another line has no inset on that side. A first
//! fragment that begins a soft-wrapped row still has its leading inset, because
//! the inset belongs to the glyph and goes where the glyph goes.
//!
//! The inline axis is the only one that takes space. The top and bottom margin,
//! padding and border of an inline box do not move a line in CSS; the padding and
//! border still grow the border box above and below the content area of the line
//! ([`Edges::top`] and [`Edges::bottom`]), where they overlap whatever is there.
//! An inset on a box that holds no glyph has nothing to attach to and is lost.
//!
//! The lengths are CSS lengths (`em`, `rem`, the viewport units, `calc()`), resolved
//! with the [`LengthContext`] of the box that declares them, so a span's `em` is its
//! own font size. A percentage margin or padding is of the width of
//! the paragraph's containing block, as in CSS, which is the width the paragraph
//! is laid out in and is not known until it is measured. [`InsetSpec`] therefore
//! keeps the lengths as they were written, and [`InsetSpec::at`] resolves them for
//! a width. While the paragraph is measured for its content there is no width, and
//! a percentage counts as 0, as CSS does for the intrinsic sizes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use gpui::{px, Bounds, LayoutInsets, Pixels, TextInset, TextLayout};

use crate::css::border::{parse_border, resolve_width};
use crate::css::length::{LengthContext, LinearLength};
use crate::events::state_styles::{folded, PointerStates};
use crate::retained_tree::RetainedTree;
use crate::style::{DimensionValue, StyleDesc};

use super::inline::Paragraph;
use super::paragraph::box_lengths;

/// Where a box's border box is: inside the margin box rectangles of its first
/// and last line fragments, and around the content area of its lines.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Edges {
    /// How far the border box starts after the margin box of the first fragment.
    pub start: Pixels,
    /// How far it ends before the margin box of the last fragment.
    pub end: Pixels,
    /// How far it reaches above the content area of a line: padding and border.
    pub top: Pixels,
    /// How far it reaches below the content area of a line.
    pub bottom: Pixels,
}

/// The inline-axis space of one box at a width: its own, with the margin kept
/// apart because the border box starts after it.
#[derive(Clone, Debug, PartialEq)]
struct BoxInset {
    id: u64,
    range: Range<usize>,
    margin_start: f32,
    margin_end: f32,
    /// Margin, border and padding before the first glyph.
    leading: f32,
    trailing: f32,
    /// Padding and border above and below the content area. They take no space.
    top: f32,
    bottom: f32,
    /// The padding and the border on each side, as an element reports them.
    layout: LayoutInsets,
}

impl BoxInset {
    /// Whether it takes no space along the line.
    fn is_empty(&self) -> bool {
        self.leading == 0.0 && self.trailing == 0.0
    }
}

/// A length along an inline box that is not resolved yet: px plus a fraction of
/// the width of the paragraph's containing block.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Extent {
    px: f64,
    percent: f64,
}

impl Extent {
    fn of(length: &LinearLength, lengths: &LengthContext) -> Self {
        let (px, percent) = lengths.split(length);
        Self { px, percent }
    }

    /// A percentage, `auto` and `anchor-size()`: `auto` is zero on an inline box in
    /// CSS, and `anchor-size()` is only resolved for a direct child of `<anchored>`.
    fn of_margin(value: Option<&DimensionValue>, lengths: &LengthContext) -> Self {
        match value {
            Some(DimensionValue::Pixels(pixels)) => Self { px: *pixels, percent: 0.0 },
            Some(DimensionValue::Percentage(fraction)) => Self { px: 0.0, percent: *fraction },
            Some(DimensionValue::Calc(length)) => Self::of(length, lengths),
            _ => Self::default(),
        }
    }

    /// With no `width`, a percentage is 0.
    fn at(self, width: Option<f32>) -> f32 {
        (self.px + self.percent * f64::from(width.unwrap_or(0.0))) as f32
    }

    /// A padding is never negative: the sum it comes to is clamped, as CSS clamps a
    /// `calc()` to the range of its property.
    fn at_non_negative(self, width: Option<f32>) -> f32 {
        self.at(width).max(0.0)
    }

    fn is_zero(self) -> bool {
        self.px == 0.0 && self.percent == 0.0
    }
}

/// What a box declares along the line and around its content area, as written.
#[derive(Clone, Debug, PartialEq)]
struct BoxSpace {
    id: u64,
    range: Range<usize>,
    margin_start: Extent,
    margin_end: Extent,
    padding_start: Extent,
    padding_end: Extent,
    padding_top: Extent,
    padding_bottom: Extent,
    border: BorderWidths,
}

impl BoxSpace {
    fn resolve(&self, width: Option<f32>) -> BoxInset {
        let margin_start = self.margin_start.at(width);
        let margin_end = self.margin_end.at(width);
        let padding = gpui::Edges {
            top: px(self.padding_top.at_non_negative(width)),
            right: px(self.padding_end.at_non_negative(width)),
            bottom: px(self.padding_bottom.at_non_negative(width)),
            left: px(self.padding_start.at_non_negative(width)),
        };
        let border = gpui::Edges {
            top: px(self.border.top),
            right: px(self.border.right),
            bottom: px(self.border.bottom),
            left: px(self.border.left),
        };
        let start = self.border.left + f32::from(padding.left);
        let end = self.border.right + f32::from(padding.right);
        BoxInset {
            id: self.id,
            range: self.range.clone(),
            margin_start,
            margin_end,
            leading: margin_start + start,
            trailing: margin_end + end,
            top: self.border.top + f32::from(padding.top),
            bottom: self.border.bottom + f32::from(padding.bottom),
            layout: LayoutInsets { padding, border },
        }
    }

    /// Whether any of it can take space along the line.
    fn takes_space_along_the_line(&self) -> bool {
        !(self.margin_start.is_zero()
            && self.margin_end.is_zero()
            && self.padding_start.is_zero()
            && self.padding_end.is_zero()
            && self.border.left == 0.0
            && self.border.right == 0.0)
    }
}

/// The space the boxes of one paragraph declare. It is resolved for a width with
/// [`Self::at`], which the paragraph's layout asks for and so do the things that
/// find the boxes in it afterwards.
#[derive(Debug)]
pub struct InsetSpec {
    boxes: Vec<(u64, Range<usize>)>,
    spaces: Vec<BoxSpace>,
    /// The last width asked for, as the layout and the paint ask for the same one.
    memo: RefCell<Option<(Option<Pixels>, Rc<Insets>)>>,
}

impl InsetSpec {
    /// What `paragraph`'s boxes put around their text.
    ///
    /// `lengths` are the container's, from which each box gets its own, and
    /// `states` says which states apply to a box, as it does for
    /// [`super::paragraph::run_styles`], so a `hover` padding is folded in.
    pub fn of(
        tree: &RetainedTree,
        paragraph: &Paragraph,
        lengths: &LengthContext,
        states: &dyn Fn(u64, &StyleDesc) -> PointerStates,
    ) -> Rc<Self> {
        let box_lengths = box_lengths(tree, paragraph, lengths, states);
        let spaces = paragraph
            .boxes
            .iter()
            .filter_map(|(id, range)| {
                let base = tree.elements.get(id)?.style.as_deref()?;
                let declared = folded(base, states(*id, base));
                // It has no box to put space around.
                if declared.display.as_deref() == Some("contents") {
                    return None;
                }
                Some(space_of(&declared, *id, range, &box_lengths[id]))
            })
            .collect();
        Rc::new(Self {
            boxes: paragraph.boxes.clone(),
            spaces,
            memo: RefCell::new(None),
        })
    }

    /// Whether a box can take space along the line, so the paragraph has insets to
    /// lay out with.
    pub fn takes_space_along_the_line(&self) -> bool {
        self.spaces.iter().any(BoxSpace::takes_space_along_the_line)
    }

    /// The space resolved for a paragraph laid out `width` wide, or measured for
    /// its content when there is none.
    pub fn at(&self, width: Option<Pixels>) -> Rc<Insets> {
        if let Some((memoized, insets)) = &*self.memo.borrow() {
            if *memoized == width {
                return insets.clone();
            }
        }
        let insets = Rc::new(self.resolve(width.map(f32::from)));
        *self.memo.borrow_mut() = Some((width, insets.clone()));
        insets
    }

    /// Where the border box of `id` is, once the layout says which width it was
    /// laid out in.
    pub fn edges_of(self: &Rc<Self>, id: u64) -> BoxEdges {
        BoxEdges { spec: self.clone(), id }
    }

    fn resolve(&self, width: Option<f32>) -> Insets {
        let all: Vec<BoxInset> = self.spaces.iter().map(|space| space.resolve(width)).collect();
        let boxes: Vec<BoxInset> = all.iter().filter(|inset| !inset.is_empty()).cloned().collect();
        let mut edges = edges_of(&self.boxes, &boxes);
        for inset in &all {
            if inset.top != 0.0 || inset.bottom != 0.0 {
                let own = edges.entry(inset.id).or_default();
                own.top = px(inset.top);
                own.bottom = px(inset.bottom);
            }
        }
        let layouts = all.iter().map(|inset| (inset.id, inset.layout)).collect();
        Insets { boxes, edges, layouts }
    }
}

/// Where the border box of one box is, asked for with the layout it is in.
#[derive(Clone, Debug)]
pub struct BoxEdges {
    spec: Rc<InsetSpec>,
    id: u64,
}

impl BoxEdges {
    pub fn at(&self, layout: &TextLayout) -> Edges {
        self.for_width(layout.inset_width())
    }

    pub fn for_width(&self, width: Option<Pixels>) -> Edges {
        self.spec.at(width).edges(self.id)
    }

    /// The padding and border of the box, as an element reports them.
    pub fn layout_insets(&self, layout: &TextLayout) -> LayoutInsets {
        self.spec.at(layout.inset_width()).layout_insets(self.id)
    }
}

impl PartialEq for BoxEdges {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && Rc::ptr_eq(&self.spec, &other.spec)
    }
}

/// The insets of the boxes of one paragraph at a width, and where each box's
/// border box is.
#[derive(Debug, Default, PartialEq)]
pub struct Insets {
    boxes: Vec<BoxInset>,
    edges: HashMap<u64, Edges>,
    layouts: HashMap<u64, LayoutInsets>,
}

impl Insets {
    /// The insets to lay the paragraph's text out with.
    pub fn text(&self) -> Vec<TextInset> {
        self.boxes
            .iter()
            .map(|inset| TextInset {
                range: inset.range.clone(),
                leading: px(inset.leading),
                trailing: px(inset.trailing),
            })
            .collect()
    }

    /// Where the border box of `id` is. A box without an inset of its own can
    /// still sit inside the space of one that starts or ends with it.
    pub fn edges(&self, id: u64) -> Edges {
        self.edges.get(&id).copied().unwrap_or_default()
    }

    /// The padding and border of `id`, which are inside its border box.
    pub fn layout_insets(&self, id: u64) -> LayoutInsets {
        self.layouts.get(&id).copied().unwrap_or_default()
    }
}

fn space_of(style: &StyleDesc, id: u64, range: &Range<usize>, lengths: &LengthContext) -> BoxSpace {
    let padding = |side: Option<LinearLength>| {
        side.or(style.padding).map_or_else(Extent::default, |length| Extent::of(&length, lengths))
    };
    let margin = |side: &Option<DimensionValue>| {
        Extent::of_margin(side.as_ref().or(style.margin.as_ref()), lengths)
    };
    BoxSpace {
        id,
        range: range.clone(),
        margin_start: margin(&style.margin_left),
        margin_end: margin(&style.margin_right),
        padding_start: padding(style.padding_left),
        padding_end: padding(style.padding_right),
        padding_top: padding(style.padding_top),
        padding_bottom: padding(style.padding_bottom),
        border: border_widths(style, lengths),
    }
}

/// The width of each border side of a box in px.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct BorderWidths {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

pub(super) fn border_widths(style: &StyleDesc, lengths: &LengthContext) -> BorderWidths {
    BorderWidths {
        top: border_width(style, style.border_top_width, style.border_top.as_deref(), lengths),
        right: border_width(style, style.border_right_width, style.border_right.as_deref(), lengths),
        bottom: border_width(style, style.border_bottom_width, style.border_bottom.as_deref(), lengths),
        left: border_width(style, style.border_left_width, style.border_left.as_deref(), lengths),
    }
}

/// The width of one border side as the element builder applies it: the
/// `borderWidth` and side longhands win over the shorthands, and a side
/// shorthand over `border`.
fn border_width(
    style: &StyleDesc,
    side: Option<LinearLength>,
    side_shorthand: Option<&str>,
    lengths: &LengthContext,
) -> f32 {
    let shorthand = |value: Option<&str>| value.and_then(parse_border).map(|parsed| parsed.width);
    side.or(style.border_width)
        .or_else(|| shorthand(side_shorthand))
        .or_else(|| shorthand(style.border.as_deref()))
        .map_or(0.0, |width| f32::from(resolve_width(&width, lengths)))
}

/// The border box edges of every box of `all`, given the `insets` of those that
/// have one, which are in the order of `all`.
fn edges_of(all: &[(u64, Range<usize>)], insets: &[BoxInset]) -> HashMap<u64, Edges> {
    if insets.is_empty() {
        return HashMap::new();
    }
    let own: HashMap<u64, &BoxInset> = insets.iter().map(|inset| (inset.id, inset)).collect();
    let mut edges = HashMap::with_capacity(all.len());
    // The boxes around the one being visited, outermost first.
    let mut around: Vec<&(u64, Range<usize>)> = Vec::new();
    for entry in all {
        let (id, range) = entry;
        while around
            .last()
            .is_some_and(|outer| outer.1.end <= range.start)
        {
            around.pop();
        }
        let space = |same: &dyn Fn(&Range<usize>) -> bool, side: fn(&BoxInset) -> f32| -> f32 {
            around
                .iter()
                .filter(|outer| same(&outer.1))
                .filter_map(|outer| own.get(&outer.0))
                .map(|inset| side(inset))
                .sum()
        };
        let before = space(&|outer| outer.start == range.start, |inset| inset.leading);
        let after = space(&|outer| outer.end == range.end, |inset| inset.trailing);
        let this = own.get(id);
        edges.insert(
            *id,
            Edges {
                start: px(before + this.map_or(0.0, |inset| inset.margin_start)),
                end: px(after + this.map_or(0.0, |inset| inset.margin_end)),
                ..Edges::default()
            },
        );
        around.push(entry);
    }
    edges
}

/// `rects`, the wash of the text `range` of `text`, without the space of the boxes
/// at its two ends: the start of the first rect is where the first glyph is
/// painted, and the end of the last rect is the end of the last glyph.
///
/// Only the ends are trimmed. The space of boxes inside the range stays washed
/// so that a selection has no holes, which is what a browser does for the
/// padding between two boxes of a line.
pub fn trim_wash(
    layout: &TextLayout,
    text: &str,
    range: &Range<usize>,
    rects: &mut [Bounds<Pixels>],
) {
    if let Some(leading) = layout.insets_at(range.start).map(|inset| inset.leading) {
        if let Some(first) = rects.first_mut() {
            first.origin.x += leading;
            first.size.width = (first.size.width - leading).max(Pixels::ZERO);
        }
    }
    let last = text
        .get(..range.end)
        .and_then(|head| head.char_indices().next_back())
        .map(|(index, _)| index)
        .filter(|index| *index >= range.start);
    if let Some(trailing) = last
        .and_then(|index| layout.insets_at(index))
        .map(|inset| inset.trailing)
    {
        if let Some(last) = rects.last_mut() {
            last.size.width = (last.size.width - trailing).max(Pixels::ZERO);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::inline::{plan_children, Child};
    use std::sync::Arc;

    /// `div(1) > [text "a ", span(3) > ["b", span(5) > "c"], text " d"]`: the
    /// text is "a bc d", span 3 covers 2..4 and span 5 covers 3..4.
    fn fixture(styles: &[(u64, serde_json::Value)]) -> RetainedTree {
        let mut tree = RetainedTree::new();
        for (id, kind) in [(1, "div"), (2, "text"), (3, "span"), (4, "text"), (5, "span"), (6, "text"), (7, "text")] {
            tree.create_element(id, kind.to_string());
        }
        for (parent, child) in [(1, 2), (1, 3), (3, 4), (3, 5), (5, 6), (1, 7)] {
            tree.append_child(parent, child);
        }
        for (id, text) in [(2, "a "), (4, "b"), (6, "c"), (7, " d")] {
            tree.set_text(id, text.to_string());
        }
        for (id, style) in styles {
            tree.set_style(*id, Arc::new(serde_json::from_value(style.clone()).expect("a style")));
        }
        tree
    }

    fn insets_with(
        tree: &RetainedTree,
        states: &dyn Fn(u64, &StyleDesc) -> PointerStates,
    ) -> Rc<Insets> {
        let plan = plan_children(tree, &tree.elements[&1]);
        let Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        InsetSpec::of(tree, paragraph, &LengthContext::initial(800.0, 600.0), states).at(None)
    }

    fn resting(tree: &RetainedTree) -> Rc<Insets> {
        insets_with(tree, &|_, _| PointerStates::default())
    }

    fn texts(insets: &Insets) -> Vec<(Range<usize>, f32, f32)> {
        insets
            .text()
            .into_iter()
            .map(|inset| (inset.range, f32::from(inset.leading), f32::from(inset.trailing)))
            .collect()
    }

    fn edges(insets: &Insets, id: u64) -> (f32, f32) {
        let edges = insets.edges(id);
        (f32::from(edges.start), f32::from(edges.end))
    }

    #[test]
    fn a_paragraph_without_space_has_no_insets() {
        let insets = resting(&fixture(&[(3, serde_json::json!({"color": "red"}))]));
        assert!(insets.text().is_empty());
        assert_eq!(edges(&insets, 3), (0.0, 0.0));
    }

    #[test]
    fn margin_border_and_padding_add_up_on_each_side() {
        let tree = fixture(&[(
            3,
            serde_json::json!({
                "marginLeft": 3, "borderLeftWidth": 2, "paddingLeft": 5,
                "marginRight": 7, "borderRightWidth": 1, "paddingRight": 4,
            }),
        )]);
        let insets = resting(&tree);
        assert_eq!(texts(&insets), vec![(2..4, 10.0, 12.0)]);
        assert_eq!(edges(&insets, 3), (3.0, 7.0));
    }

    #[test]
    fn a_longhand_beats_its_shorthand() {
        let tree = fixture(&[(
            3,
            serde_json::json!({
                "padding": 4, "paddingLeft": 9,
                "margin": 2, "marginRight": 6,
                "borderWidth": 1, "borderRightWidth": 3,
            }),
        )]);
        assert_eq!(texts(&resting(&tree)), vec![(2..4, 2.0 + 1.0 + 9.0, 6.0 + 3.0 + 4.0)]);
    }

    #[test]
    fn a_border_shorthand_gives_a_width_that_the_longhands_override() {
        let tree = fixture(&[(3, serde_json::json!({"border": "2px solid red"}))]);
        assert_eq!(texts(&resting(&tree)), vec![(2..4, 2.0, 2.0)]);

        let tree = fixture(&[(
            3,
            serde_json::json!({"border": "2px solid red", "borderLeft": "5px solid blue"}),
        )]);
        assert_eq!(texts(&resting(&tree)), vec![(2..4, 5.0, 2.0)]);

        let tree = fixture(&[(
            3,
            serde_json::json!({"borderLeft": "5px solid blue", "borderWidth": 1}),
        )]);
        assert_eq!(texts(&resting(&tree)), vec![(2..4, 1.0, 1.0)]);

        let tree = fixture(&[(3, serde_json::json!({"border": "2px red"}))]);
        assert!(resting(&tree).text().is_empty(), "no line style, no border");
    }

    #[test]
    fn a_numeric_string_margin_is_pixels() {
        let tree = fixture(&[(3, serde_json::json!({"marginLeft": "6", "marginRight": 4}))]);
        assert_eq!(texts(&resting(&tree)), vec![(2..4, 6.0, 4.0)]);
    }

    #[test]
    fn auto_is_zero_on_an_inline_box() {
        let tree = fixture(&[(
            3,
            serde_json::json!({"marginLeft": "auto", "marginRight": "auto", "paddingLeft": 2}),
        )]);
        assert_eq!(texts(&resting(&tree)), vec![(2..4, 2.0, 0.0)]);
    }

    fn spec(tree: &RetainedTree, lengths: &LengthContext) -> Rc<InsetSpec> {
        let plan = plan_children(tree, &tree.elements[&1]);
        let Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        InsetSpec::of(tree, paragraph, lengths, &|_, _| PointerStates::default())
    }

    /// 1em is 20px, 1rem 10px and the viewport 800x600.
    fn texts_in_units(style: serde_json::Value) -> Vec<(Range<usize>, f32, f32)> {
        let spec = spec(&fixture(&[(3, style)]), &LengthContext::for_tests());
        texts(&spec.at(None))
    }

    #[test]
    fn lengths_resolve_in_the_units_of_the_box() {
        assert_eq!(
            texts_in_units(serde_json::json!({
                "marginLeft": "1em", "paddingLeft": "0.5rem", "borderLeftWidth": "2px",
                "marginRight": "10vw", "paddingRight": "calc(1em - 5px)", "borderRightWidth": "0.25em",
            })),
            vec![(2..4, 20.0 + 5.0 + 2.0, 80.0 + 15.0 + 5.0)]
        );
    }

    #[test]
    fn em_is_the_font_size_of_the_span_that_declares_it() {
        let style = |font_size: serde_json::Value| {
            serde_json::json!({"fontSize": font_size, "paddingLeft": "1em", "paddingRight": "1rem"})
        };
        let tree = fixture(&[
            (3, style(serde_json::json!(30))),
            (5, serde_json::json!({"paddingLeft": "1em"})),
        ]);
        assert_eq!(
            texts(&spec(&tree, &LengthContext::for_tests()).at(None)),
            vec![(2..4, 30.0, 10.0), (3..4, 30.0, 0.0)],
            "the nested span inherits its parent's size, and rem stays the root's"
        );

        let tree = fixture(&[
            (3, style(serde_json::json!(30))),
            (5, serde_json::json!({"fontSize": "0.5em", "paddingLeft": "2em", "borderLeftWidth": "1em"})),
        ]);
        let found = texts(&spec(&tree, &LengthContext::for_tests()).at(None));
        assert_eq!(found[1].1, 15.0 + 30.0, "a 15px border and a 30px padding: 1em and 2em of 0.5em of 30");
    }

    #[test]
    fn the_border_and_padding_shorthands_take_units_too() {
        assert_eq!(
            texts_in_units(serde_json::json!({"padding": "1em", "border": "0.5rem solid red"})),
            vec![(2..4, 25.0, 25.0)]
        );
        assert_eq!(
            texts_in_units(serde_json::json!({"borderLeft": "calc(1em + 1px) solid red"})),
            vec![(2..4, 21.0, 0.0)]
        );
    }

    #[test]
    fn vertical_reach_takes_units_too() {
        let spec = spec(
            &fixture(&[(3, serde_json::json!({"paddingTop": "1em", "borderBottomWidth": "0.5em"}))]),
            &LengthContext::for_tests(),
        );
        assert_eq!(vertical(&spec.at(None), 3), (20.0, 10.0));
    }

    /// The margin and padding of span 3 in a paragraph `width` wide.
    fn texts_at(style: serde_json::Value, width: Option<f32>) -> Vec<(Range<usize>, f32, f32)> {
        let spec = spec(&fixture(&[(3, style)]), &LengthContext::for_tests());
        texts(&spec.at(width.map(px)))
    }

    #[test]
    fn a_percentage_is_of_the_width_of_the_paragraph() {
        let style = serde_json::json!({
            "marginLeft": "10%", "paddingLeft": "5%", "paddingRight": "calc(50% - 1em)", "marginRight": "calc(10% + 4px)",
        });
        assert_eq!(texts_at(style.clone(), Some(200.0)), vec![(2..4, 20.0 + 10.0, 80.0 + 24.0)]);
        assert_eq!(texts_at(style, Some(400.0)), vec![(2..4, 40.0 + 20.0, 180.0 + 44.0)]);
    }

    #[test]
    fn a_percentage_is_zero_while_the_paragraph_is_measured_for_its_content() {
        let style = serde_json::json!({"marginLeft": "10%", "paddingLeft": "5%", "paddingRight": "calc(50% + 3px)"});
        assert_eq!(texts_at(style, None), vec![(2..4, 0.0, 3.0)]);
    }

    #[test]
    fn a_percentage_padding_that_comes_out_negative_is_zero_and_a_margin_may_be_negative() {
        let style = serde_json::json!({
            "paddingLeft": "calc(10px - 50%)", "marginLeft": "calc(10px - 50%)", "paddingRight": 2
        });
        assert_eq!(texts_at(style, Some(100.0)), vec![(2..4, -40.0, 2.0)]);
    }

    #[test]
    fn the_edges_of_a_box_follow_the_width_the_paragraph_was_laid_out_in() {
        let spec = spec(
            &fixture(&[(3, serde_json::json!({"marginLeft": "10%", "paddingTop": "5%"}))]),
            &LengthContext::for_tests(),
        );
        let edges = spec.edges_of(3);
        assert_eq!(edges.for_width(Some(px(200.0))).start, px(20.0));
        assert_eq!(edges.for_width(Some(px(200.0))).top, px(10.0));
        assert_eq!(edges.for_width(Some(px(100.0))).start, px(10.0));
        assert_eq!(edges.for_width(None).start, px(0.0));
        // The last width asked for is kept, so the layout and the paint share it.
        assert!(Rc::ptr_eq(&spec.at(Some(px(100.0))), &spec.at(Some(px(100.0)))));
    }

    #[test]
    fn a_paragraph_takes_space_along_the_line_only_if_a_box_can() {
        let tree = fixture(&[(3, serde_json::json!({"paddingTop": 4, "marginTop": 4, "color": "red"}))]);
        assert!(!spec(&tree, &LengthContext::for_tests()).takes_space_along_the_line());
        let tree = fixture(&[(3, serde_json::json!({"paddingLeft": "10%"}))]);
        assert!(spec(&tree, &LengthContext::for_tests()).takes_space_along_the_line());
    }

    #[test]
    fn a_negative_margin_pulls_and_negative_padding_and_border_are_zero() {
        let tree = fixture(&[(
            3,
            serde_json::json!({
                "marginLeft": -4, "paddingLeft": -9, "borderLeftWidth": -2, "paddingRight": 3
            }),
        )]);
        let insets = resting(&tree);
        assert_eq!(texts(&insets), vec![(2..4, -4.0, 3.0)]);
        assert_eq!(edges(&insets, 3), (-4.0, 0.0));
    }

    #[test]
    fn vertical_space_moves_no_line() {
        let tree = fixture(&[(
            3,
            serde_json::json!({"marginTop": 5, "marginBottom": 5, "paddingTop": 5, "paddingBottom": 5,
                "borderTopWidth": 2, "borderBottomWidth": 2}),
        )]);
        assert!(resting(&tree).text().is_empty());
    }

    fn vertical(insets: &Insets, id: u64) -> (f32, f32) {
        let edges = insets.edges(id);
        (f32::from(edges.top), f32::from(edges.bottom))
    }

    #[test]
    fn vertical_padding_and_border_reach_around_the_content_area() {
        let tree = fixture(&[(
            3,
            serde_json::json!({"paddingTop": 5, "paddingBottom": 6, "borderTopWidth": 2, "borderBottomWidth": 1}),
        )]);
        let insets = resting(&tree);
        assert_eq!(vertical(&insets, 3), (7.0, 7.0));
        assert!(insets.text().is_empty(), "and they take no space along the line");
        assert_eq!(vertical(&insets, 5), (0.0, 0.0), "a box inside it has none of its own");
    }

    #[test]
    fn vertical_reach_reads_the_shorthands_like_the_horizontal_space() {
        let tree = fixture(&[(
            3,
            serde_json::json!({
                "padding": 4, "paddingBottom": 9,
                "border": "3px solid red", "borderTop": "1px solid red",
            }),
        )]);
        assert_eq!(vertical(&resting(&tree), 3), (1.0 + 4.0, 3.0 + 9.0));
    }

    #[test]
    fn vertical_reach_ignores_margin_and_a_negative_padding() {
        let tree = fixture(&[(
            3,
            serde_json::json!({"marginTop": 8, "marginBottom": 8, "paddingTop": -3, "borderBottomWidth": -1}),
        )]);
        assert_eq!(vertical(&resting(&tree), 3), (0.0, 0.0));
    }

    #[test]
    fn vertical_reach_goes_with_horizontal_space_on_the_same_box() {
        let tree = fixture(&[(3, serde_json::json!({"paddingLeft": 3, "paddingTop": 5}))]);
        let insets = resting(&tree);
        assert_eq!(texts(&insets), vec![(2..4, 3.0, 0.0)]);
        assert_eq!(vertical(&insets, 3), (5.0, 0.0));
        assert_eq!(edges(&insets, 3), (0.0, 0.0));
    }

    #[test]
    fn a_display_contents_box_reaches_nowhere() {
        let tree = fixture(&[(3, serde_json::json!({"display": "contents", "paddingTop": 6}))]);
        assert_eq!(vertical(&resting(&tree), 3), (0.0, 0.0));
    }

    #[test]
    fn hover_folds_into_the_space() {
        let tree = fixture(&[(
            3,
            serde_json::json!({"paddingLeft": 2, "hover": {"paddingLeft": 8}}),
        )]);
        assert_eq!(texts(&resting(&tree)), vec![(2..4, 2.0, 0.0)]);
        let hovered = insets_with(&tree, &|_, _| PointerStates {
            hovered: true,
            ..Default::default()
        });
        assert_eq!(texts(&hovered), vec![(2..4, 8.0, 0.0)]);
    }

    #[test]
    fn a_display_contents_box_has_none() {
        let tree = fixture(&[(3, serde_json::json!({"display": "contents", "paddingLeft": 6}))]);
        assert!(resting(&tree).text().is_empty());
    }

    #[test]
    fn nested_boxes_that_start_on_the_same_byte_put_the_outer_first() {
        // div(1) > ["a ", span(3) > span(5) > "c"]: both spans cover 2..3.
        let mut tree = RetainedTree::new();
        for (id, kind) in [(1, "div"), (2, "text"), (3, "span"), (5, "span"), (6, "text")] {
            tree.create_element(id, kind.to_string());
        }
        for (parent, child) in [(1, 2), (1, 3), (3, 5), (5, 6)] {
            tree.append_child(parent, child);
        }
        tree.set_text(2, "a ".to_string());
        tree.set_text(6, "c".to_string());
        let styles = [
            (3, serde_json::json!({"marginLeft": 1, "paddingLeft": 2, "paddingRight": 20, "marginRight": 10})),
            (5, serde_json::json!({"marginLeft": 4, "borderLeftWidth": 8, "paddingRight": 5, "marginRight": 6})),
        ];
        for (id, style) in styles {
            tree.set_style(id, Arc::new(serde_json::from_value(style).expect("a style")));
        }
        let insets = resting_paragraph(&tree, "a c");
        assert_eq!(texts(&insets), vec![(2..3, 3.0, 30.0), (2..3, 12.0, 11.0)]);
        // The outer border box starts after its margin, the inner one after the
        // outer's margin, border and padding and then its own margin.
        assert_eq!(edges(&insets, 3), (1.0, 10.0));
        assert_eq!(edges(&insets, 5), (3.0 + 4.0, 30.0 + 6.0));
    }

    fn resting_paragraph(tree: &RetainedTree, text: &str) -> Rc<Insets> {
        let plan = plan_children(tree, &tree.elements[&1]);
        let Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        assert_eq!(paragraph.text, text);
        InsetSpec::of(tree, paragraph, &LengthContext::initial(800.0, 600.0), &|_, _| PointerStates::default()).at(None)
    }

    #[test]
    fn a_box_without_space_inside_the_space_of_one_that_starts_with_it() {
        let tree = fixture(&[(3, serde_json::json!({"marginLeft": 2, "paddingLeft": 3, "paddingRight": 5}))]);
        let insets = resting(&tree);
        // Span 5 covers 3..4, which ends with span 3 but does not start with it.
        assert_eq!(edges(&insets, 5), (0.0, 5.0));
        assert_eq!(edges(&insets, 3), (2.0, 0.0));
    }

    #[test]
    fn a_box_around_a_padded_one_has_that_padding_inside_its_border_box() {
        let tree = fixture(&[(5, serde_json::json!({"paddingLeft": 6, "paddingRight": 6, "marginRight": 2}))]);
        let insets = resting(&tree);
        assert_eq!(edges(&insets, 3), (0.0, 0.0), "span 3 ends with span 5 and holds all of its box");
        assert_eq!(edges(&insets, 5), (0.0, 2.0));
    }
}
