//! DOM inline formatting: which children of a container are laid out as one
//! paragraph of text, and which are boxes of their own.
//!
//! The rules follow CSS, not the shape of the retained tree:
//! - A raw text node is inline. So is a `<span>`, and so is a `<text>` whose
//!   parent is a `<text>` or `<span>`, unless it is focusable. A `<text>`
//!   anywhere else is a block that starts its own inline formatting context.
//!   Listeners and state styles do not make a box a block: the pointer finds an
//!   inline box through the paragraph's text ([`super::inline_hit`]).
//! - In a block container, each maximal run of consecutive inline-level children
//!   is ONE anonymous paragraph. A block child ends the run.
//! - In a flex or grid container, children are blockified: each contiguous run
//!   of raw text nodes becomes one anonymous item, and every element child is an
//!   item of its own (a `<span>` there lays out its own content as a paragraph).
//! - A child with `display: inline-block`, `inline-flex` or `inline-grid` is an
//!   atomic inline: it takes the place of one U+FFFC in the paragraph and is
//!   built as an element of its own, which the paragraph sits on its line. It
//!   is a box of the paragraph, not of its own, even when nothing but it is in
//!   the container.
//! - A paragraph that holds nothing but collapsible whitespace generates no box,
//!   and neither does a child with `display: none`.
//!
//! Search, copy and paint all call [`plan_children`], because a paragraph is one
//! painted string and they must agree on where it starts and ends.

use std::collections::HashMap;
use std::ops::Range;

use crate::layout::contents::is_display_contents;
use crate::layout::display;
use crate::retained_tree::{RetainedElement, RetainedTree};
use crate::style::StyleDesc;

use super::white_space::{self, Emit, Tail, WhiteSpace};

/// CSS white space: space, tab, line feed, carriage return, form feed. A
/// no-break space is content.
pub fn is_collapsible_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{000C}')
}

/// Whether `text` is empty or only collapsible whitespace.
pub fn is_blank(text: &str) -> bool {
    text.chars().all(is_collapsible_whitespace)
}

/// Whether `element_type` is built by `build_host_container`, the retained
/// tree's own `div` rules. Paint dispatches on this and search collects by it, so
/// the two cannot disagree about which elements lay out a paragraph.
pub fn is_host_type(element_type: &str) -> bool {
    matches!(element_type, "div" | "text" | "label" | "form" | "span")
}

pub fn is_host_container(element: &RetainedElement) -> bool {
    is_host_type(&element.element_type)
}

/// The `display` the planner sees: the one an `@container` block gave the element when the
/// last build folded them in, or the style's.
fn display_of(element: &RetainedElement) -> Option<std::borrow::Cow<'_, str>> {
    element.display()
}

/// A child that generates no box at all.
fn is_display_none(element: &RetainedElement) -> bool {
    crate::layout::box_generation::is_display_none(element)
}

/// What a React/Solid string child is: a `text` node with no box of its own.
/// A `<text>` that carries a style, listener or prop is a host element even when
/// it has no children yet.
pub fn is_raw_text(element: &RetainedElement) -> bool {
    element.element_type == "text"
        && element.children.is_empty()
        && element.style.is_none()
        && element.events().is_empty()
        && element.custom_props().is_empty()
}

/// Whether children of a container with this style are blockified into flex or
/// grid items. A container that scrolls on x alone is a flex viewport too, see
/// `build_host_container`.
pub fn lays_out_items(style: Option<&StyleDesc>) -> bool {
    lays_out_items_as(style, style.and_then(|style| style.display.as_deref()))
}

/// [`lays_out_items`] for an element, with the display its `@container` blocks gave it.
fn element_lays_out_items(element: &RetainedElement) -> bool {
    lays_out_items_as(element.style.as_deref(), display_of(element).as_deref())
}

fn lays_out_items_as(style: Option<&StyleDesc>, display_value: Option<&str>) -> bool {
    if display::lays_out_items(display_value) {
        return true;
    }
    let Some(style) = style else { return false };
    let x = style.overflow_x.as_deref().or(style.overflow.as_deref());
    let y = style.overflow_y.as_deref().or(style.overflow.as_deref());
    x == Some("scroll") && y != Some("scroll")
}

/// Whether swapping `old` for `new` leaves every paragraph where it was: none
/// of the properties [`plan_children`] reads changed.
pub fn same_paragraphs(old: Option<&StyleDesc>, new: Option<&StyleDesc>) -> bool {
    fn read(style: Option<&StyleDesc>) -> [Option<&str>; 6] {
        let field = |get: fn(&StyleDesc) -> Option<&str>| style.and_then(get);
        [
            field(|style| style.display.as_deref()),
            field(|style| style.position.as_deref()),
            field(|style| style.overflow.as_deref()),
            field(|style| style.overflow_x.as_deref()),
            field(|style| style.overflow_y.as_deref()),
            field(|style| style.white_space.as_deref()),
        ]
    }
    read(old) == read(new)
}

fn is_out_of_flow(element: &RetainedElement) -> bool {
    matches!(
        element.style.as_deref().and_then(|style| style.position.as_deref()),
        Some("absolute") | Some("fixed")
    )
}

/// Whether `element` is an atomic inline of the paragraph around it: an
/// `inline-block`, `inline-flex` or `inline-grid` box that is in the flow. One
/// that is positioned absolutely or fixed is blockified, as CSS says.
pub fn is_atomic(element: &RetainedElement) -> bool {
    display::is_atomic_inline(display_of(element).as_deref())
        && !is_out_of_flow(element)
        && !matches!(element.element_type.as_str(), "anchored" | "virtual-list")
}

/// Whether `element`, a child of `parent` in a block container, is inline-level
/// together with everything inside it. An inline box holding a block is itself
/// laid out as a block, so a paragraph never has to split around it.
fn is_inline_box(tree: &RetainedTree, element: &RetainedElement, parent: &RetainedElement) -> bool {
    if is_raw_text(element) {
        return true;
    }
    // Whatever is inside it is its own, so the paragraph never splits around it.
    if is_atomic(element) {
        return true;
    }
    // It generates no box, so its children are inline when they would be
    // children of `parent`.
    if is_host_container(element) && is_display_contents(element) && !is_special_host(element) {
        return children_are_inline(tree, element, parent);
    }
    is_inline_wrapper(element, parent) && children_are_inline(tree, element, element)
}

/// A host that is a block whatever its style says, or that is not a box to begin with.
fn is_special_host(element: &RetainedElement) -> bool {
    matches!(element.element_type.as_str(), "label" | "form") || element.custom_props().contains_key("highlight")
}

/// Whether `element`, a child of `parent` in a block container, is an inline box that holds the
/// text and inline boxes inside it in the paragraph of its parent, as far as its own
/// style and kind say. Whether everything it holds is inline is up to [`is_inline_box`].
pub fn is_inline_wrapper(element: &RetainedElement, parent: &RetainedElement) -> bool {
    // A label or a form has behaviour of its own, so it is a block.
    if !is_host_container(element) || is_special_host(element) {
        return false;
    }
    // A focusable `<span>` stays inline: its paragraph anchors the focus handle
    // ([`super::inline_focus`]). A focusable `<text>` is a block, an element of
    // its own with the handle.
    if element.element_type == "text" && element.needs_focus() {
        return false;
    }
    if is_out_of_flow(element) {
        return false;
    }
    let inline_by_default = match element.element_type.as_str() {
        "span" => true,
        "text" => matches!(parent.element_type.as_str(), "text" | "span"),
        _ => false,
    };
    let inline = match display_of(element).as_deref() {
        Some("inline") => true,
        Some(_) => false,
        None => inline_by_default,
    };
    inline && !element_lays_out_items(element)
}

fn children_are_inline(tree: &RetainedTree, element: &RetainedElement, parent: &RetainedElement) -> bool {
    element
        .children
        .iter()
        .filter_map(|id| tree.elements.get(id))
        .filter(|child| !is_display_none(child))
        .all(|child| is_inline_box(tree, child, parent))
}

/// One text node of a paragraph.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub range: Range<usize>,
    /// The inline boxes around the text, outermost first. The container is not
    /// one of them: its own style reaches the paragraph through gpui. A
    /// `display: contents` element is in the list, because its descendants
    /// inherit from it, although it has no box of its own.
    pub boxes: Vec<u64>,
    /// The atomic inline this piece stands for: `range` is its U+FFFC, and it is
    /// not text.
    pub atomic: Option<u64>,
}

/// A run of inline content, painted as one string.
#[derive(Debug, Clone, PartialEq)]
pub struct Paragraph {
    /// The container the paragraph is in, and how many blocks come before it
    /// there. Together they name the paragraph in the selection and search
    /// registries, so they must outlive the text nodes: a selection stays when
    /// the first node of the paragraph is removed.
    pub container: u64,
    pub index: usize,
    pub text: String,
    pub pieces: Vec<Piece>,
    /// Every inline box that holds text, with the bytes it covers, outermost
    /// first. Hit-testing a character index back to its span reads this.
    pub boxes: Vec<(u64, Range<usize>)>,
}

impl Paragraph {
    pub fn key(&self) -> std::sync::Arc<str> {
        super::selection_key(self.container, self.index)
    }

    /// The atomic inlines of the paragraph, in order, with the piece that holds
    /// each one's U+FFFC.
    pub fn atomics(&self) -> impl Iterator<Item = (u64, &Piece)> {
        self.pieces
            .iter()
            .filter_map(|piece| piece.atomic.map(|id| (id, piece)))
    }

    /// Innermost inline box at `byte`, if any, with or without a box of its own
    /// (a `display: contents` element is one). [`super::inline_hit`] needs the
    /// whole chain and reads the pieces.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn box_at(&self, byte: usize) -> Option<u64> {
        self.pieces
            .iter()
            .find(|piece| piece.range.contains(&byte))
            .and_then(|piece| piece.boxes.last().copied())
    }
}

/// What a container builds for its children.
#[derive(Debug, Clone, PartialEq)]
pub enum Child {
    /// A child that is built as an element of its own.
    Block(u64),
    Paragraph(Paragraph),
    /// The children of a table, row group or row that are not table parts, wrapped in the one
    /// cell CSS 2.1 section 17.2.1 puts around consecutive ones. It has no element: it is a
    /// plain block with the role of a cell.
    AnonymousCell(Vec<(Child, u32)>),
}

/// Collects one paragraph and applies `white-space` as it goes: what is pushed
/// is the text that is painted, searched and copied, and every range is a byte
/// range of that text.
struct Builder {
    container: u64,
    /// Blocks seen so far in the container.
    index: usize,
    has_text: bool,
    text: String,
    pieces: Vec<Piece>,
    boxes: Vec<(u64, Range<usize>)>,
    tail: Tail,
    /// The container's `white-space`, then that of each open box.
    modes: Vec<WhiteSpace>,
    /// Every character that came in was collapsible whitespace.
    only_whitespace: bool,
    /// A flex or grid item of whitespace alone is not rendered, whatever its
    /// `white-space`.
    blockified: bool,
    /// Document position of the first child that fed the paragraph, see [`plan_children_at`].
    at: Option<u32>,
}

impl Builder {
    fn new(container: u64, base: WhiteSpace, blockified: bool) -> Self {
        Self {
            container,
            index: 0,
            has_text: false,
            text: String::new(),
            pieces: Vec::new(),
            boxes: Vec::new(),
            tail: Tail::LineStart,
            modes: vec![base],
            only_whitespace: true,
            blockified,
            at: None,
        }
    }

    fn mode(&self) -> WhiteSpace {
        *self.modes.last().expect("the container's mode is always there")
    }

    /// Removes the last character, a space or a line feed, from the text and
    /// from every range that reached it.
    fn pop(&mut self) {
        self.text.pop();
        let len = self.text.len();
        for piece in &mut self.pieces {
            piece.range.end = piece.range.end.min(len);
        }
        self.pieces.retain(|piece| !piece.range.is_empty());
        for (_, range) in &mut self.boxes {
            range.start = range.start.min(len);
            range.end = range.end.min(len);
        }
    }

    fn push_text(&mut self, content: &str, boxes: &[u64]) {
        if content.is_empty() {
            return;
        }
        self.has_text = true;
        self.only_whitespace &= is_blank(content);
        let mode = self.mode();
        let mut start = self.text.len();
        let mut chars = content.chars().peekable();
        while let Some(c) = chars.next() {
            let c = match c {
                '\r' if chars.peek() == Some(&'\n') => continue,
                '\r' => '\n',
                c => c,
            };
            match white_space::step(&mut self.tail, c, mode) {
                Emit::Nothing => {}
                Emit::Char(c) => self.text.push(c),
                Emit::Break { trim } => {
                    if trim {
                        self.pop();
                        start = start.min(self.text.len());
                    }
                    self.text.push('\n');
                }
            }
        }
        if self.text.len() > start {
            self.pieces.push(Piece {
                range: start..self.text.len(),
                boxes: boxes.to_vec(),
                atomic: None,
            });
        }
    }

    /// An atomic inline is content: it ends a collapsible space before it and
    /// starts one after it.
    fn push_atomic(&mut self, element: &RetainedElement, boxes: &[u64]) {
        self.has_text = true;
        self.only_whitespace = false;
        self.tail = Tail::Content;
        let start = self.text.len();
        self.text.push(gpui::OBJECT_REPLACEMENT_CHARACTER);
        self.pieces.push(Piece {
            range: start..self.text.len(),
            boxes: boxes.to_vec(),
            atomic: Some(element.id),
        });
    }

    fn push_element(
        &mut self,
        tree: &RetainedTree,
        element: &RetainedElement,
        chain: &mut Vec<u64>,
    ) {
        if is_raw_text(element) {
            if let Some(content) = element.content.as_deref() {
                self.push_text(content, chain);
            }
            return;
        }
        if is_atomic(element) {
            self.push_atomic(element, chain);
            return;
        }
        // A `display: contents` element passes its inherited style and its
        // white-space on, but has no box to cover bytes.
        let slot = (!is_display_contents(element)).then(|| {
            self.boxes.push((element.id, self.text.len()..self.text.len()));
            self.boxes.len() - 1
        });
        chain.push(element.id);
        self.modes.push(white_space::of_box(element, self.mode()));
        if let Some(content) = element.content.as_deref() {
            self.push_text(content, chain);
        }
        for child in element
            .children
            .iter()
            .filter_map(|id| tree.elements.get(id))
            .filter(|child| !is_display_none(child))
        {
            self.push_element(tree, child, chain);
        }
        self.modes.pop();
        chain.pop();
        // The start may have moved back when a line feed trimmed the space
        // before it, so it is read from the slot.
        if let Some(slot) = slot {
            self.boxes[slot].1.end = self.text.len();
        }
    }

    /// Runs `push` inside the `display: contents` elements `contents`, outermost
    /// first: they have no box, but their text inherits their `white-space` and
    /// they are in the chain of every piece that comes out of it.
    fn within(
        &mut self,
        contents: &[&RetainedElement],
        push: impl FnOnce(&mut Self, &mut Vec<u64>),
    ) {
        let mut chain = Vec::with_capacity(contents.len());
        for element in contents {
            chain.push(element.id);
            self.modes.push(white_space::of_box(element, self.mode()));
        }
        push(self, &mut chain);
        self.modes.truncate(self.modes.len() - contents.len());
    }

    /// Ends the paragraph. A collapsible space at its end is removed, and so is
    /// a single final line feed, which ends the last line and starts no new
    /// one. A paragraph with nothing left generates no box.
    fn finish(&mut self, out: &mut Vec<(Child, u32)>) {
        let mut next = Self::new(self.container, self.modes[0], self.blockified);
        next.index = self.index;
        let mut builder = std::mem::replace(self, next);
        if !builder.has_text {
            return;
        }
        if builder.tail == Tail::Space {
            builder.pop();
        } else if builder.text.ends_with('\n') {
            builder.pop();
        }
        if builder.text.is_empty() || (builder.blockified && builder.only_whitespace) {
            return;
        }
        builder.boxes.retain(|(_, range)| !range.is_empty());
        let paragraph = Paragraph {
            container: builder.container,
            index: builder.index,
            text: builder.text,
            pieces: builder.pieces,
            boxes: builder.boxes,
        };
        out.push((Child::Paragraph(paragraph), builder.at.unwrap_or(0)));
    }
}

/// How `container` builds its children, in order.
///
/// The container's own `content` leads as text, like a text node that comes
/// first.
pub fn plan_children(tree: &RetainedTree, container: &RetainedElement) -> Vec<Child> {
    plan_children_at(tree, container)
        .into_iter()
        .map(|(child, _)| child)
        .collect()
}

/// [`plan_children`], with where each child sits in the DOCUMENT: its index among the container's
/// children once `display: contents` is spliced out, ignoring flex and grid `order`. A paragraph
/// sits where its first child does, and `0` when only the container's own `content` feeds it.
///
/// The plan is in paint order, which `order` reorders. Selection and find number text in
/// document order, as the browser does, and read this.
pub fn plan_children_at(tree: &RetainedTree, container: &RetainedElement) -> Vec<(Child, u32)> {
    let children = children_at(tree, container);
    let content = container.content.as_deref();
    if let Some(plan) = crate::layout::table::plan_children(tree, container, &children, content) {
        return plan;
    }
    plan_flow(tree, container, &children, content, element_lays_out_items(container), &mut 0)
}

/// The children of `container` in paint order, each with its document position, see
/// [`plan_children_at`].
fn children_at(tree: &RetainedTree, container: &RetainedElement) -> Vec<(u64, u32)> {
    let document = crate::layout::contents::flatten(tree, &container.children);
    let children = crate::layout::order::children_in_order(tree, container, container.style.as_deref());
    if *document == *children {
        return children.iter().enumerate().map(|(ix, id)| (*id, ix as u32)).collect();
    }
    let document_index: HashMap<u64, u32> = document.iter().enumerate().map(|(ix, id)| (*id, ix as u32)).collect();
    children.iter().map(|id| (*id, document_index[id])).collect()
}

/// `children` of `container` as inline paragraphs and blocks. `index` is where the blocks of
/// `container` are counted from, and where the next call can start after this one.
pub(crate) fn plan_flow(
    tree: &RetainedTree,
    container: &RetainedElement,
    children: &[(u64, u32)],
    content: Option<&str>,
    items: bool,
    index: &mut usize,
) -> Vec<(Child, u32)> {
    let mut out = Vec::new();
    let mut paragraph = Builder::new(container.id, white_space::computed(tree, container), items);
    paragraph.index = *index;
    if let Some(content) = content {
        paragraph.push_text(content, &[]);
    }
    for (child, at) in children
        .iter()
        .filter_map(|(id, at)| Some((tree.elements.get(id)?, *at)))
        .filter(|(child, _)| !is_display_none(child))
    {
        // `children_in_order` spliced the `display: contents` elements out, but
        // their descendants still inherit from them.
        let contents = crate::layout::contents::contents_between(tree, container, child.id)
            .unwrap_or_default();
        if is_raw_text(child) {
            paragraph.at.get_or_insert(at);
            if let Some(content) = child.content.as_deref() {
                paragraph.within(&contents, |paragraph, chain| paragraph.push_text(content, chain));
            }
        } else if !items && is_inline_box(tree, child, container) {
            paragraph.at.get_or_insert(at);
            paragraph.within(&contents, |paragraph, chain| paragraph.push_element(tree, child, chain));
        } else {
            paragraph.finish(&mut out);
            out.push((Child::Block(child.id), at));
            paragraph.index += 1;
        }
    }
    paragraph.finish(&mut out);
    *index = paragraph.index + 1;
    out
}

/// The text a node with no children paints, after `white-space`: `content` of
/// `element`, which is a text node or a host that carries a string. `None` when
/// nothing is left of it.
pub fn own_text(tree: &RetainedTree, element: &RetainedElement) -> Option<String> {
    let content = element.content.as_deref()?;
    let mut paragraph = Builder::new(element.id, white_space::computed(tree, element), false);
    paragraph.push_text(content, &[]);
    let mut out = Vec::new();
    paragraph.finish(&mut out);
    match out.pop() {
        Some((Child::Paragraph(paragraph), _)) => Some(paragraph.text),
        _ => None,
    }
}

/// Adjacent text leaves of a parent that lays out no paragraph of its own
/// (`anchored`, `virtual-list`, a custom element), processed as ONE paragraph:
/// white space collapses across the leaves and only the run's start and end are
/// stripped, as in [`plan_children`]. Each leaf still paints its own string.
#[derive(Debug, PartialEq)]
pub struct LeafRun {
    pub text: String,
    /// Each leaf that has text left, with its bytes of `text`.
    pub parts: Vec<(u64, Range<usize>)>,
}

/// `None` when nothing is left of the run.
pub fn leaf_run(tree: &RetainedTree, parent: &RetainedElement, leaves: &[u64]) -> Option<LeafRun> {
    let mut paragraph = Builder::new(parent.id, white_space::computed(tree, parent), false);
    for leaf in leaves.iter().filter_map(|id| tree.elements.get(id)) {
        if let Some(content) = leaf.content.as_deref() {
            paragraph.within(&[leaf], |paragraph, chain| paragraph.push_text(content, chain));
        }
    }
    let mut out = Vec::new();
    paragraph.finish(&mut out);
    match out.pop() {
        Some((Child::Paragraph(paragraph), _)) => Some(LeafRun {
            text: paragraph.text,
            parts: paragraph
                .pieces
                .iter()
                .map(|piece| (piece.boxes[0], piece.range.clone()))
                .collect(),
        }),
        _ => None,
    }
}

/// The text an inline formatting context reads out, as painted: that of every
/// paragraph in `plan`, in order. The placeholders of atomic inlines are not
/// text.
pub fn accessible_text<'a>(plan: impl IntoIterator<Item = &'a Child>) -> Option<String> {
    fn collect(child: &Child, text: &mut String) {
        match child {
            Child::Paragraph(paragraph) => {
                text.extend(paragraph.text.chars().filter(|c| *c != gpui::OBJECT_REPLACEMENT_CHARACTER))
            }
            Child::AnonymousCell(cell) => cell.iter().for_each(|(child, _)| collect(child, text)),
            Child::Block(_) => {}
        }
    }
    let mut text = String::new();
    plan.into_iter().for_each(|child| collect(child, &mut text));
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn style(json: serde_json::Value) -> Arc<StyleDesc> {
        Arc::new(serde_json::from_value(json).expect("a style"))
    }

    struct Fixture {
        tree: RetainedTree,
    }

    impl Fixture {
        fn new() -> Self {
            let mut tree = RetainedTree::new();
            tree.create_element(1, "div".to_string());
            Self { tree }
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
            self.tree.set_style(id, style(json));
            self
        }

        fn plan(&self, id: u64) -> Vec<Child> {
            plan_children(&self.tree, &self.tree.elements[&id])
        }
    }

    fn paragraphs(plan: &[Child]) -> Vec<&Paragraph> {
        plan.iter()
            .filter_map(|child| match child {
                Child::Paragraph(paragraph) => Some(paragraph),
                Child::Block(_) | Child::AnonymousCell(_) => None,
            })
            .collect()
    }

    #[test]
    fn whitespace_is_css_whitespace_and_a_no_break_space_is_content() {
        for c in [' ', '\t', '\n', '\r', '\u{000C}'] {
            assert!(is_collapsible_whitespace(c), "{c:?}");
        }
        for c in ['\u{00A0}', '\u{2003}', 'a', '\u{200B}'] {
            assert!(!is_collapsible_whitespace(c), "{c:?}");
        }
        assert!(is_blank(""));
        assert!(is_blank(" \n\t"));
        assert!(!is_blank(" \u{00A0} "));
    }

    #[test]
    fn sibling_text_nodes_are_one_paragraph() {
        let mut f = Fixture::new();
        f.text(1, 2, "Hello ").text(1, 3, "Tommy").text(1, 4, "!");
        let plan = f.plan(1);
        assert_eq!(plan.len(), 1);
        let paragraph = paragraphs(&plan)[0];
        assert_eq!(paragraph.text, "Hello Tommy!");
        assert_eq!((paragraph.container, paragraph.index), (1, 0));
        assert_eq!(paragraph.pieces.len(), 3);
        assert_eq!(paragraph.pieces[1].range, 6..11);
        assert!(paragraph.pieces.iter().all(|piece| piece.boxes.is_empty()));
    }

    #[test]
    fn nested_spans_are_runs_of_one_paragraph() {
        let mut f = Fixture::new();
        f.text(1, 2, "a ");
        f.add(1, 3, "span").text(3, 4, "b");
        f.add(3, 5, "span").text(5, 6, "c");
        f.text(1, 7, " d");
        let plan = f.plan(1);
        let paragraph = paragraphs(&plan)[0];
        assert_eq!(plan.len(), 1);
        assert_eq!(paragraph.text, "a bc d");
        let boxes: Vec<_> = paragraph.pieces.iter().map(|p| p.boxes.clone()).collect();
        assert_eq!(boxes, vec![vec![], vec![3], vec![3, 5], vec![]]);
        assert_eq!(paragraph.boxes, vec![(3, 2..4), (5, 3..4)]);
        assert_eq!(paragraph.box_at(3), Some(5));
        assert_eq!(paragraph.box_at(2), Some(3));
        assert_eq!(paragraph.box_at(0), None);
    }

    #[test]
    fn a_text_is_inline_only_inside_text_or_span() {
        let mut f = Fixture::new();
        f.text(1, 2, "a");
        f.add(1, 3, "text").text(3, 4, "block");
        let plan = f.plan(1);
        assert_eq!(plan.len(), 2, "a paragraph, then the block");
        assert_eq!(plan[1], Child::Block(3));

        let mut f = Fixture::new();
        f.add(1, 2, "text").text(2, 3, "a");
        f.add(2, 4, "text").text(4, 5, "b");
        let plan = f.plan(2);
        assert_eq!(paragraphs(&plan)[0].text, "ab");
        assert_eq!(plan.len(), 1);
    }

    #[test]
    fn a_block_child_splits_paragraphs() {
        let mut f = Fixture::new();
        f.text(1, 2, "one ");
        f.add(1, 3, "span").text(3, 4, "two");
        f.add(1, 5, "div");
        f.text(1, 6, "three");
        let plan = f.plan(1);
        assert_eq!(plan.len(), 3);
        assert_eq!(paragraphs(&plan)[0].text, "one two");
        assert_eq!(plan[1], Child::Block(5));
        assert_eq!(paragraphs(&plan)[1].text, "three");
        assert_eq!(paragraphs(&plan)[0].key().as_ref(), "1:0");
        assert_eq!(paragraphs(&plan)[1].key().as_ref(), "1:1");
    }

    /// The plan is in paint order, so flex `order` moves a block in it. Its document position does
    /// not move: selection and find follow the tree.
    #[test]
    fn a_reordered_block_keeps_its_document_position() {
        let mut f = Fixture::new();
        f.styled(1, serde_json::json!({"display": "flex"}));
        f.add(1, 2, "div").styled(2, serde_json::json!({"order": 2}));
        f.add(1, 3, "div");
        f.add(1, 4, "div").styled(4, serde_json::json!({"order": 1}));
        let at = plan_children_at(&f.tree, &f.tree.elements[&1]);
        assert_eq!(
            at,
            vec![(Child::Block(3), 1), (Child::Block(4), 2), (Child::Block(2), 0)]
        );
    }

    #[test]
    fn a_paragraph_sits_where_its_first_child_does() {
        let mut f = Fixture::new();
        f.add(1, 2, "div");
        f.text(1, 3, "a");
        f.add(1, 4, "span").text(4, 5, "b");
        f.add(1, 6, "div");
        f.text(1, 7, "c");
        let positions: Vec<u32> = plan_children_at(&f.tree, &f.tree.elements[&1])
            .into_iter()
            .map(|(_, at)| at)
            .collect();
        // block 2, paragraph "ab" (children 1 and 2), block 6, paragraph "c".
        assert_eq!(positions, vec![0, 1, 3, 4]);
    }

    #[test]
    fn display_contents_children_count_in_the_flattened_list() {
        let mut f = Fixture::new();
        f.add(1, 2, "div").styled(2, serde_json::json!({"display": "contents"}));
        f.add(2, 3, "div");
        f.add(2, 4, "div");
        f.add(1, 5, "div");
        let positions: Vec<u32> = plan_children_at(&f.tree, &f.tree.elements[&1])
            .into_iter()
            .map(|(_, at)| at)
            .collect();
        assert_eq!(positions, vec![0, 1, 2]);
    }

    #[test]
    fn a_span_holding_a_block_is_a_block() {
        let mut f = Fixture::new();
        f.text(1, 2, "a");
        f.add(1, 3, "span");
        f.add(3, 4, "div");
        let plan = f.plan(1);
        assert_eq!(plan[1], Child::Block(3));
    }

    #[test]
    fn display_decides_inline_or_block() {
        let mut f = Fixture::new();
        f.add(1, 2, "span").text(2, 3, "s");
        f.styled(2, serde_json::json!({"display": "block"}));
        f.add(1, 4, "div").text(4, 5, "d");
        f.styled(4, serde_json::json!({"display": "inline"}));
        f.add(1, 6, "span").text(6, 7, "abs");
        f.styled(6, serde_json::json!({"position": "absolute"}));
        let plan = f.plan(1);
        assert_eq!(plan[0], Child::Block(2));
        assert_eq!(paragraphs(&plan)[0].text, "d");
        assert_eq!(plan[2], Child::Block(6));
    }

    #[test]
    fn a_display_none_child_generates_nothing_and_does_not_split() {
        let mut f = Fixture::new();
        f.text(1, 2, "a");
        f.add(1, 3, "div");
        f.styled(3, serde_json::json!({"display": "none"}));
        f.text(1, 4, "b");
        f.add(1, 5, "span").text(5, 6, "hidden");
        f.styled(5, serde_json::json!({"display": "none"}));
        let plan = f.plan(1);
        assert_eq!(plan.len(), 1);
        assert_eq!(paragraphs(&plan)[0].text, "ab");
    }

    #[test]
    fn a_whitespace_only_paragraph_generates_no_box() {
        let mut f = Fixture::new();
        f.text(1, 2, "  ");
        f.add(1, 3, "div");
        f.text(1, 4, "\n\t");
        f.add(1, 5, "span").text(5, 6, " ");
        assert_eq!(f.plan(1), vec![Child::Block(3)]);
    }

    #[test]
    fn whitespace_inside_a_paragraph_is_content() {
        let mut f = Fixture::new();
        f.add(1, 2, "span").text(2, 3, "a");
        f.text(1, 4, " ");
        f.add(1, 5, "span").text(5, 6, "b");
        assert_eq!(paragraphs(&f.plan(1))[0].text, "a b");
    }

    #[test]
    fn a_no_break_space_is_content() {
        let mut f = Fixture::new();
        f.text(1, 2, "\u{00A0}");
        assert_eq!(f.plan(1).len(), 1);
    }

    #[test]
    fn an_empty_text_node_is_transparent() {
        let mut f = Fixture::new();
        f.text(1, 2, "a").text(1, 3, "").text(1, 4, "b");
        let plan = f.plan(1);
        assert_eq!(plan.len(), 1);
        assert_eq!(paragraphs(&plan)[0].text, "ab");
        assert_eq!(paragraphs(&plan)[0].pieces.len(), 2);
    }

    #[test]
    fn a_container_with_only_empty_text_has_no_children() {
        let mut f = Fixture::new();
        f.text(1, 2, "");
        assert!(f.plan(1).is_empty());
    }

    #[test]
    fn a_flex_container_makes_one_item_per_raw_text_run() {
        let mut f = Fixture::new();
        f.styled(1, serde_json::json!({"display": "flex", "gap": 8}));
        f.text(1, 2, "a").text(1, 3, "b");
        f.add(1, 4, "div");
        f.text(1, 5, "c");
        let plan = f.plan(1);
        assert_eq!(plan.len(), 3);
        assert_eq!(paragraphs(&plan)[0].text, "ab");
        assert_eq!(plan[1], Child::Block(4));
        assert_eq!(paragraphs(&plan)[1].text, "c");
    }

    #[test]
    fn a_flex_container_blockifies_spans_and_texts() {
        let mut f = Fixture::new();
        f.styled(1, serde_json::json!({"display": "flex"}));
        f.add(1, 2, "span").text(2, 3, "s");
        f.add(1, 4, "text").text(4, 5, "t");
        assert_eq!(f.plan(1), vec![Child::Block(2), Child::Block(4)]);
    }

    #[test]
    fn a_grid_container_and_an_x_scroller_are_item_containers_too() {
        for json in [
            serde_json::json!({"display": "grid"}),
            serde_json::json!({"overflowX": "scroll"}),
            serde_json::json!({"overflow": "scroll", "overflowY": "hidden"}),
        ] {
            let mut f = Fixture::new();
            f.styled(1, json);
            f.add(1, 2, "span").text(2, 3, "s");
            assert_eq!(f.plan(1), vec![Child::Block(2)]);
        }
        let mut f = Fixture::new();
        f.styled(1, serde_json::json!({"overflow": "scroll"}));
        f.add(1, 2, "span").text(2, 3, "s");
        assert_eq!(f.plan(1).len(), 1, "a two-axis scroller is a block");
    }

    /// Solid makes an empty text node for every dynamic hole. In a flex
    /// container with a `gap` it must not become an item, or it adds a gap.
    #[test]
    fn a_hole_between_flex_siblings_makes_no_item() {
        let mut f = Fixture::new();
        f.styled(1, serde_json::json!({"display": "flex", "gap": 8}));
        f.add(1, 2, "div");
        f.text(1, 3, "");
        f.add(1, 4, "div");
        f.text(1, 5, " ");
        f.add(1, 6, "div");
        assert_eq!(
            f.plan(1),
            vec![Child::Block(2), Child::Block(4), Child::Block(6)]
        );
    }

    #[test]
    fn a_hole_between_text_nodes_keeps_one_flex_item() {
        let mut f = Fixture::new();
        f.styled(1, serde_json::json!({"display": "flex"}));
        f.text(1, 2, "a").text(1, 3, "").text(1, 4, "b");
        let plan = f.plan(1);
        assert_eq!(plan.len(), 1);
        assert_eq!(paragraphs(&plan)[0].text, "ab");
    }

    #[test]
    fn a_containers_own_content_leads_the_first_paragraph() {
        let mut f = Fixture::new();
        f.tree.set_text(1, "lead ".to_string());
        f.add(1, 2, "span").text(2, 3, "x");
        let plan = f.plan(1);
        let paragraph = paragraphs(&plan)[0];
        assert_eq!(paragraph.text, "lead x");
        assert_eq!(paragraph.container, 1);
    }

    #[test]
    fn a_paragraph_is_named_by_its_container_so_its_first_node_can_go() {
        let mut f = Fixture::new();
        f.text(1, 2, "Hello ");
        f.add(1, 3, "span").text(3, 4, "big");
        let before = paragraphs(&f.plan(1))[0].key();
        f.tree.destroy_element(2);
        let plan = f.plan(1);
        assert_eq!(paragraphs(&plan)[0].text, "big");
        assert_eq!(paragraphs(&plan)[0].key(), before);
    }

    #[test]
    fn a_paragraph_keeps_its_name_when_a_blank_one_comes_and_goes_before_it() {
        let mut f = Fixture::new();
        f.add(1, 2, "div");
        f.text(1, 3, "after");
        let key = paragraphs(&f.plan(1))[0].key();
        f.text(1, 4, " ");
        f.add(1, 5, "div");
        assert_eq!(paragraphs(&f.plan(1))[0].key(), key, "the same paragraph, one block before it");
    }

    #[test]
    fn label_and_form_are_host_containers_and_never_inline_boxes() {
        for kind in ["div", "text", "span", "label", "form"] {
            assert!(is_host_type(kind), "{kind}");
        }
        for kind in ["img", "svg", "input", "virtual-list", "markdown"] {
            assert!(!is_host_type(kind), "{kind}");
        }
        let mut f = Fixture::new();
        f.text(1, 2, "a");
        f.add(1, 3, "label").styled(3, serde_json::json!({"display": "inline"}));
        f.text(3, 4, "b");
        let plan = f.plan(1);
        assert_eq!(plan[1], Child::Block(3));
    }

    #[test]
    fn a_display_contents_child_of_a_span_joins_the_paragraph() {
        let mut f = Fixture::new();
        f.text(1, 2, "a ");
        f.add(1, 3, "span");
        f.add(3, 4, "span").styled(4, serde_json::json!({"display": "contents"}));
        f.text(4, 5, "b");
        f.add(4, 6, "span").text(6, 7, "c");
        f.text(1, 8, " d");
        let plan = f.plan(1);
        assert_eq!(plan.len(), 1, "no block: {plan:?}");
        let paragraph = paragraphs(&plan)[0];
        assert_eq!(paragraph.text, "a bc d");
        let boxes: Vec<_> = paragraph.pieces.iter().map(|p| p.boxes.clone()).collect();
        assert_eq!(boxes, vec![vec![], vec![3, 4], vec![3, 4, 6], vec![]]);
    }

    #[test]
    fn a_direct_display_contents_child_is_in_the_chain_and_gives_its_white_space() {
        let mut f = Fixture::new();
        f.text(1, 2, "a");
        f.add(1, 3, "div").styled(3, serde_json::json!({"display": "contents", "whiteSpace": "pre"}));
        f.text(3, 4, "b  c");
        f.add(3, 5, "div").styled(5, serde_json::json!({"display": "contents"}));
        f.text(5, 6, "d");
        let plan = f.plan(1);
        let paragraph = paragraphs(&plan)[0];
        assert_eq!(paragraph.text, "ab  cd");
        let chains: Vec<_> = paragraph.pieces.iter().map(|piece| piece.boxes.clone()).collect();
        assert_eq!(chains, vec![vec![], vec![3], vec![3, 5]]);
        assert!(paragraph.boxes.is_empty(), "a contents element covers no bytes");
    }

    #[test]
    fn a_block_inside_display_contents_inside_a_span_makes_the_span_a_block() {
        let mut f = Fixture::new();
        f.text(1, 2, "a");
        f.add(1, 3, "span");
        f.add(3, 4, "span").styled(4, serde_json::json!({"display": "contents"}));
        f.add(4, 5, "div");
        assert_eq!(f.plan(1)[1], Child::Block(3));
    }

    #[test]
    fn a_styled_empty_text_is_a_box_not_a_text_node() {
        let mut f = Fixture::new();
        f.add(1, 2, "text");
        f.styled(2, serde_json::json!({"width": 10}));
        assert_eq!(f.plan(1), vec![Child::Block(2)]);
    }

    #[test]
    fn a_highlight_declaration_is_a_block() {
        let mut f = Fixture::new();
        f.add(1, 2, "span").text(2, 3, "x");
        f.tree.set_custom_prop(
            2,
            "highlight".to_string(),
            serde_json::json!({"query": "x"}),
        );
        assert_eq!(f.plan(1), vec![Child::Block(2)]);
    }

    #[test]
    fn pieces_cover_the_paragraph_exactly() {
        let mut f = Fixture::new();
        f.text(1, 2, "héllo ");
        f.add(1, 3, "span").text(3, 4, "wörld");
        let plan = f.plan(1);
        let paragraph = paragraphs(&plan)[0];
        let mut at = 0;
        for piece in &paragraph.pieces {
            assert_eq!(piece.range.start, at);
            at = piece.range.end;
        }
        assert_eq!(at, paragraph.text.len());
    }

    #[test]
    fn only_paragraph_shaping_styles_regroup() {
        let flex = style(serde_json::json!({"display": "flex"}));
        let colored = style(serde_json::json!({"display": "flex", "color": "red"}));
        assert!(same_paragraphs(Some(&flex), Some(&colored)));
        assert!(same_paragraphs(None, Some(&StyleDesc::default())));
        assert!(!same_paragraphs(None, Some(&flex)));
        assert!(!same_paragraphs(
            Some(&flex),
            Some(&style(serde_json::json!({"display": "none"})))
        ));
        assert!(same_paragraphs(None, None));
        assert!(!same_paragraphs(Some(&flex), None));
    }

    /// `<text>one <text>two</text> three</text>` with `configure` applied to the
    /// nested text (id 3), and the plan of the outer one.
    fn nested_text_plan(configure: impl FnOnce(&mut Fixture)) -> Vec<Child> {
        let mut f = Fixture::new();
        f.add(1, 2, "text").text(2, 4, "one ");
        f.add(2, 3, "text").text(3, 5, "two");
        f.text(2, 6, " three");
        configure(&mut f);
        f.plan(2)
    }

    #[test]
    fn a_nested_text_stays_inline_whatever_it_listens_for() {
        let plain = nested_text_plan(|f| {
            f.styled(3, serde_json::json!({"color": "red"}));
        });
        assert_eq!(plain.len(), 1, "style alone keeps it inline");
        assert_eq!(paragraphs(&plain)[0].text, "one two three");

        let behaviours: [(&str, Box<dyn Fn(&mut Fixture)>); 13] = [
            ("listener", Box::new(|f| f.tree.set_event_listener(3, "click".into(), true))),
            ("hover", Box::new(|f| { f.styled(3, serde_json::json!({"hover": {"color": "red"}})); })),
            ("active", Box::new(|f| { f.styled(3, serde_json::json!({"active": {"color": "red"}})); })),
            ("focusVisible", Box::new(|f| { f.styled(3, serde_json::json!({"focusVisible": {"color": "red"}})); })),
            ("groupHover", Box::new(|f| { f.styled(3, serde_json::json!({"groupHover": {"color": "red"}})); })),
            ("groupActive", Box::new(|f| { f.styled(3, serde_json::json!({"groupActive": {"color": "red"}})); })),
            ("groupFocusWithin", Box::new(|f| { f.styled(3, serde_json::json!({"groupFocusWithin": {"color": "red"}})); })),
            ("hasHover", Box::new(|f| { f.styled(3, serde_json::json!({"hasHover": {"color": "red"}})); })),
            ("hasActive", Box::new(|f| { f.styled(3, serde_json::json!({"hasActive": {"color": "red"}})); })),
            ("hasFocus", Box::new(|f| { f.styled(3, serde_json::json!({"hasFocus": {"color": "red"}})); })),
            ("hasFocusVisible", Box::new(|f| { f.styled(3, serde_json::json!({"hasFocusVisible": {"color": "red"}})); })),
            ("group", Box::new(|f| { f.styled(3, serde_json::json!({"group": true})); })),
            ("cursor", Box::new(|f| { f.styled(3, serde_json::json!({"cursor": "pointer"})); })),
        ];
        for (name, configure) in behaviours {
            let plan = nested_text_plan(|f| configure(f));
            assert_eq!(plan.len(), 1, "{name}");
            let paragraph = paragraphs(&plan)[0];
            assert_eq!(paragraph.text, "one two three", "{name}");
            assert!(paragraph.boxes.contains(&(3, 4..7)), "{name}");
        }
    }

    #[test]
    fn a_focusable_nested_text_stays_a_block() {
        let plan = nested_text_plan(|f| {
            f.tree.set_custom_prop(3, "tabIndex".into(), serde_json::json!(0));
        });
        assert_eq!(plan.len(), 3, "paragraph, block, paragraph");
        assert_eq!(plan[1], Child::Block(3));
        // The space at the end of a line, and the one at the start of the
        // next, are removed.
        assert_eq!(paragraphs(&plan)[0].text, "one");
        assert_eq!(paragraphs(&plan)[1].text, "three");
    }

    #[test]
    fn a_span_with_behaviour_is_still_inline() {
        let mut f = Fixture::new();
        f.add(1, 2, "text").text(2, 4, "one ");
        f.add(2, 3, "span").text(3, 5, "two");
        f.tree.set_event_listener(3, "click".into(), true);
        f.styled(3, serde_json::json!({"hover": {"color": "red"}}));
        let plan = f.plan(2);
        assert_eq!(plan.len(), 1);
        assert_eq!(paragraphs(&plan)[0].text, "one two");
    }

    #[test]
    fn state_styles_do_not_regroup() {
        let plain = style(serde_json::json!({"color": "red"}));
        let hover = style(serde_json::json!({"color": "red", "hover": {"color": "blue"}}));
        assert!(same_paragraphs(Some(&plain), Some(&hover)));
        assert!(same_paragraphs(Some(&hover), Some(&plain)));
    }

    fn text_of(f: &Fixture, id: u64) -> String {
        let plan = f.plan(id);
        assert_eq!(plan.len(), 1, "{plan:?}");
        paragraphs(&plan)[0].text.clone()
    }

    #[test]
    fn spaces_collapse_inside_a_text_node() {
        let mut f = Fixture::new();
        f.text(1, 2, "a  \t b\n\n  c");
        assert_eq!(text_of(&f, 1), "a b c");
    }

    #[test]
    fn spaces_collapse_across_span_edges_and_the_first_run_keeps_the_space() {
        let mut f = Fixture::new();
        f.text(1, 2, "a ");
        f.add(1, 3, "span").text(3, 4, " b ");
        f.add(1, 5, "span").text(5, 6, " c");
        let plan = f.plan(1);
        let paragraph = paragraphs(&plan)[0];
        assert_eq!(paragraph.text, "a b c");
        assert_eq!(paragraph.pieces[0].range, 0..2, "the space stays in the first run");
        assert_eq!(paragraph.pieces[1].range, 2..4);
        assert_eq!(paragraph.pieces[2].range, 4..5);
        assert_eq!(paragraph.boxes, vec![(3, 2..4), (5, 4..5)]);
    }

    #[test]
    fn a_space_only_span_between_words_keeps_one_space() {
        let mut f = Fixture::new();
        f.text(1, 2, "a");
        f.add(1, 3, "span").text(3, 4, "  ");
        f.text(1, 5, "b");
        assert_eq!(text_of(&f, 1), "a b");
    }

    #[test]
    fn a_span_that_collapses_to_nothing_has_no_box() {
        let mut f = Fixture::new();
        f.text(1, 2, "a ");
        f.add(1, 3, "span").text(3, 4, " ");
        f.text(1, 5, "b");
        let plan = f.plan(1);
        let paragraph = paragraphs(&plan)[0];
        assert_eq!(paragraph.text, "a b");
        assert!(paragraph.boxes.is_empty());
        assert_eq!(paragraph.pieces.len(), 2);
    }

    #[test]
    fn the_start_and_end_of_a_paragraph_are_stripped() {
        let mut f = Fixture::new();
        f.text(1, 2, "  \n a ");
        f.add(1, 3, "span").text(3, 4, "b \n ");
        f.text(1, 5, "  ");
        let plan = f.plan(1);
        let paragraph = paragraphs(&plan)[0];
        assert_eq!(paragraph.text, "a b");
        assert_eq!(paragraph.boxes, vec![(3, 2..3)]);
        let covered: usize = paragraph.pieces.iter().map(|piece| piece.range.len()).sum();
        assert_eq!(covered, paragraph.text.len());
    }

    #[test]
    fn a_block_child_ends_a_line_so_the_space_before_it_goes() {
        let mut f = Fixture::new();
        f.text(1, 2, "one ");
        f.add(1, 3, "div");
        f.text(1, 4, " two");
        let plan = f.plan(1);
        assert_eq!(paragraphs(&plan)[0].text, "one");
        assert_eq!(paragraphs(&plan)[1].text, "two");
    }

    #[test]
    fn ranges_are_bytes_of_the_collapsed_text() {
        let mut f = Fixture::new();
        f.text(1, 2, "héllo   ");
        f.add(1, 3, "span").text(3, 4, "  wörld");
        let plan = f.plan(1);
        let paragraph = paragraphs(&plan)[0];
        assert_eq!(paragraph.text, "héllo wörld");
        assert_eq!(paragraph.pieces[0].range, 0..7);
        assert_eq!(paragraph.pieces[1].range, 7..paragraph.text.len());
        assert_eq!(paragraph.box_at(8), Some(3));
    }

    #[test]
    fn nowrap_collapses_like_normal() {
        let mut f = Fixture::new();
        f.styled(1, serde_json::json!({"whiteSpace": "nowrap"}));
        f.text(1, 2, " a \n  b ");
        assert_eq!(text_of(&f, 1), "a b");
    }

    #[test]
    fn pre_keeps_spaces_tabs_and_line_feeds() {
        let mut f = Fixture::new();
        f.styled(1, serde_json::json!({"whiteSpace": "pre"}));
        f.text(1, 2, "  a  b\n   c  ");
        assert_eq!(text_of(&f, 1), "  a  b\n   c  ");
    }

    #[test]
    fn a_tab_is_kept_where_the_white_space_keeps_spaces() {
        for mode in ["pre", "pre-wrap", "break-spaces"] {
            let mut f = Fixture::new();
            f.styled(1, serde_json::json!({"whiteSpace": mode}));
            f.text(1, 2, "ab\tc\n\td");
            assert_eq!(text_of(&f, 1), "ab\tc\n\td", "{mode}");
        }
    }

    #[test]
    fn a_tab_collapses_to_a_space_where_the_white_space_collapses() {
        for mode in ["normal", "nowrap", "pre-line"] {
            let mut f = Fixture::new();
            f.styled(1, serde_json::json!({"whiteSpace": mode}));
            f.text(1, 2, "ab\t\tc");
            assert_eq!(text_of(&f, 1), "ab c", "{mode}");
        }
    }

    #[test]
    fn a_tab_in_a_span_is_one_byte_of_the_text() {
        let mut f = Fixture::new();
        f.styled(1, serde_json::json!({"whiteSpace": "pre"}));
        f.text(1, 2, "a");
        f.add(1, 3, "span").text(3, 4, "\tb");
        let plan = f.plan(1);
        let paragraph = paragraphs(&plan)[0];
        assert_eq!(paragraph.text, "a\tb");
        assert_eq!(paragraph.pieces[1].range, 1..3);
        assert_eq!(paragraph.box_at(1), Some(3));
    }

    #[test]
    fn pre_ends_at_one_final_line_feed_not_two_lines() {
        let mut f = Fixture::new();
        f.styled(1, serde_json::json!({"whiteSpace": "pre"}));
        f.text(1, 2, "a\n");
        assert_eq!(text_of(&f, 1), "a");
        let mut f = Fixture::new();
        f.styled(1, serde_json::json!({"whiteSpace": "pre"}));
        f.text(1, 2, "a\n\n");
        assert_eq!(text_of(&f, 1), "a\n");
    }

    #[test]
    fn crlf_and_a_lone_cr_are_one_line_feed() {
        let mut f = Fixture::new();
        f.styled(1, serde_json::json!({"whiteSpace": "pre"}));
        f.text(1, 2, "a\r\nb\rc");
        assert_eq!(text_of(&f, 1), "a\nb\nc");
    }

    #[test]
    fn pre_wrap_keeps_trailing_spaces() {
        let mut f = Fixture::new();
        f.styled(1, serde_json::json!({"whiteSpace": "pre-wrap"}));
        f.text(1, 2, "a  ");
        assert_eq!(text_of(&f, 1), "a  ");
    }

    #[test]
    fn break_spaces_keeps_the_spaces_at_both_ends_of_a_paragraph_and_of_a_line() {
        let mut f = Fixture::new();
        f.styled(1, serde_json::json!({"whiteSpace": "break-spaces"}));
        f.text(1, 2, "  a  \n  b  ");
        assert_eq!(text_of(&f, 1), "  a  \n  b  ");
    }

    #[test]
    fn break_spaces_is_inherited_by_a_span() {
        let mut f = Fixture::new();
        f.styled(1, serde_json::json!({"whiteSpace": "break-spaces"}));
        f.add(1, 2, "span");
        f.text(2, 3, " a  ");
        f.text(1, 4, " b");
        assert_eq!(text_of(&f, 1), " a   b");
    }

    #[test]
    fn pre_line_collapses_spaces_and_keeps_line_feeds() {
        let mut f = Fixture::new();
        f.styled(1, serde_json::json!({"whiteSpace": "pre-line"}));
        f.text(1, 2, "  a   b \n   c\n\nd ");
        assert_eq!(text_of(&f, 1), "a b\nc\n\nd");
    }

    #[test]
    fn white_space_is_inherited_by_text_below_the_declaring_element() {
        let mut f = Fixture::new();
        f.styled(1, serde_json::json!({"whiteSpace": "pre"}));
        f.add(1, 2, "div");
        f.text(2, 3, "a  b");
        assert_eq!(text_of(&f, 2), "a  b");
        f.add(2, 4, "div");
        f.styled(4, serde_json::json!({"whiteSpace": "normal"}));
        f.text(4, 5, "a  b");
        assert_eq!(text_of(&f, 4), "a b");
    }

    #[test]
    fn a_span_can_declare_its_own_white_space() {
        let mut f = Fixture::new();
        f.text(1, 2, "a  ");
        f.add(1, 3, "span").text(3, 4, "b  c");
        f.styled(3, serde_json::json!({"whiteSpace": "pre"}));
        f.text(1, 5, "  d");
        let plan = f.plan(1);
        let paragraph = paragraphs(&plan)[0];
        assert_eq!(paragraph.text, "a b  c d");
        assert_eq!(paragraph.boxes, vec![(3, 2..6)]);
    }

    #[test]
    fn a_collapsible_space_before_a_forced_break_is_removed() {
        let mut f = Fixture::new();
        f.text(1, 2, "a ");
        f.add(1, 3, "span").text(3, 4, "\nb");
        f.styled(3, serde_json::json!({"whiteSpace": "pre"}));
        let plan = f.plan(1);
        let paragraph = paragraphs(&plan)[0];
        assert_eq!(paragraph.text, "a\nb");
        assert_eq!(paragraph.boxes, vec![(3, 1..3)]);
    }

    #[test]
    fn whitespace_alone_in_a_pre_block_is_content_but_not_in_a_flex_item() {
        let mut f = Fixture::new();
        f.styled(1, serde_json::json!({"whiteSpace": "pre"}));
        f.text(1, 2, "  ");
        assert_eq!(text_of(&f, 1), "  ");

        let mut f = Fixture::new();
        f.styled(1, serde_json::json!({"whiteSpace": "pre", "display": "flex"}));
        f.text(1, 2, "  ");
        f.add(1, 3, "div");
        assert_eq!(f.plan(1), vec![Child::Block(3)]);
    }

    #[test]
    fn a_text_with_content_of_its_own_collapses_too() {
        let mut f = Fixture::new();
        f.tree.set_text(1, "  a \n b ".to_string());
        assert_eq!(own_text(&f.tree, &f.tree.elements[&1]).as_deref(), Some("a b"));
        f.tree.set_text(1, " \n".to_string());
        assert_eq!(own_text(&f.tree, &f.tree.elements[&1]), None);
    }

    #[test]
    fn a_white_space_change_regroups() {
        let normal = style(serde_json::json!({"color": "red"}));
        let pre = style(serde_json::json!({"color": "red", "whiteSpace": "pre"}));
        assert!(!same_paragraphs(Some(&normal), Some(&pre)));
    }

    const OBJECT: char = gpui::OBJECT_REPLACEMENT_CHARACTER;

    fn inline_block(f: &mut Fixture, parent: u64, id: u64, display: &str) {
        f.add(parent, id, "div");
        f.styled(id, serde_json::json!({"display": display, "width": 10, "height": 10}));
    }

    #[test]
    fn an_inline_block_is_one_placeholder_in_the_paragraph() {
        for display in ["inline-block", "inline-flex", "inline-grid"] {
            let mut f = Fixture::new();
            f.text(1, 2, "a ");
            inline_block(&mut f, 1, 3, display);
            f.text(1, 4, " b");
            let plan = f.plan(1);
            assert_eq!(plan.len(), 1, "{display}: {plan:?}");
            let paragraph = paragraphs(&plan)[0];
            assert_eq!(paragraph.text, format!("a {OBJECT} b"), "{display}");
            assert_eq!(paragraph.atomics().map(|(id, _)| id).collect::<Vec<_>>(), vec![3]);
            let piece = paragraph.atomics().next().unwrap().1;
            assert_eq!(piece.range, 2..5);
            assert_eq!(piece.atomic, Some(3));
            let covered: usize = paragraph.pieces.iter().map(|piece| piece.range.len()).sum();
            assert_eq!(covered, paragraph.text.len(), "the pieces cover the text exactly");
        }
    }

    /// What a build found for `id` once its `@container` blocks were folded in.
    fn container_display(f: &Fixture, id: u64, display: Option<&str>) {
        *f.tree.elements[&id].container_display.borrow_mut() = Some(display.map(str::to_string));
    }

    #[test]
    fn the_display_an_at_container_block_gave_decides_what_kind_of_box_a_child_is() {
        let kinds = |plan: &[Child]| -> Vec<&'static str> {
            plan.iter()
                .map(|child| match child {
                    Child::Paragraph(paragraph) if paragraph.atomics().next().is_some() => "atomic paragraph",
                    Child::Paragraph(_) => "paragraph",
                    Child::Block(_) => "block",
                    Child::AnonymousCell(_) => "cell",
                })
                .collect()
        };
        // A block that a container made an inline-block, a span, and nothing at all.
        let mut f = Fixture::new();
        f.text(1, 2, "a ");
        f.add(1, 3, "div");
        f.styled(3, serde_json::json!({"display": "block", "width": 10, "height": 10}));
        f.text(1, 4, " b");
        assert_eq!(kinds(&f.plan(1)), vec!["paragraph", "block", "paragraph"]);
        container_display(&f, 3, Some("inline-block"));
        assert_eq!(kinds(&f.plan(1)), vec!["atomic paragraph"]);
        container_display(&f, 3, Some("inline"));
        assert_eq!(kinds(&f.plan(1)), vec!["paragraph"]);
        container_display(&f, 3, Some("none"));
        assert_eq!(kinds(&f.plan(1)), vec!["paragraph"]);
        container_display(&f, 3, Some("block"));
        assert_eq!(kinds(&f.plan(1)), vec!["paragraph", "block", "paragraph"]);

        // And the other way: an inline-block that became a block.
        let mut f = Fixture::new();
        f.text(1, 2, "a ");
        inline_block(&mut f, 1, 3, "inline-block");
        f.text(1, 4, " b");
        assert_eq!(kinds(&f.plan(1)), vec!["atomic paragraph"]);
        container_display(&f, 3, Some("block"));
        assert_eq!(kinds(&f.plan(1)), vec!["paragraph", "block", "paragraph"]);

        // A span that became a block, and a container that became a flex container.
        let mut f = Fixture::new();
        f.text(1, 2, "a ");
        f.add(1, 3, "span");
        f.text(3, 5, "b");
        assert_eq!(kinds(&f.plan(1)), vec!["paragraph"]);
        container_display(&f, 3, Some("block"));
        assert_eq!(kinds(&f.plan(1)), vec!["paragraph", "block"]);
        container_display(&f, 3, None);
        container_display(&f, 1, Some("flex"));
        assert_eq!(kinds(&f.plan(1)), vec!["paragraph", "block"], "the items of a flex container");
    }

    #[test]
    fn a_lone_inline_block_is_a_paragraph_so_it_has_a_line_to_sit_on() {
        let mut f = Fixture::new();
        inline_block(&mut f, 1, 2, "inline-block");
        let plan = f.plan(1);
        assert_eq!(plan.len(), 1);
        assert_eq!(paragraphs(&plan)[0].text, OBJECT.to_string());
    }

    #[test]
    fn white_space_around_a_placeholder_is_kept_between_content_and_dropped_at_the_ends() {
        let mut f = Fixture::new();
        f.text(1, 2, "  ");
        inline_block(&mut f, 1, 3, "inline-block");
        f.text(1, 4, "  ");
        assert_eq!(text_of(&f, 1), OBJECT.to_string(), "the spaces at the ends of the line go");

        let mut f = Fixture::new();
        inline_block(&mut f, 1, 2, "inline-block");
        f.text(1, 3, "   ");
        inline_block(&mut f, 1, 4, "inline-block");
        assert_eq!(text_of(&f, 1), format!("{OBJECT} {OBJECT}"), "one space between two boxes");
    }

    #[test]
    fn an_inline_block_in_a_span_is_in_its_chain() {
        let mut f = Fixture::new();
        f.text(1, 2, "a");
        f.add(1, 3, "span").text(3, 4, "b");
        inline_block(&mut f, 3, 5, "inline-block");
        let plan = f.plan(1);
        assert_eq!(plan.len(), 1);
        let paragraph = paragraphs(&plan)[0];
        assert_eq!(paragraph.text, format!("ab{OBJECT}"));
        assert_eq!(paragraph.boxes, vec![(3, 1..5)], "the span covers the placeholder");
        assert_eq!(paragraph.atomics().next().unwrap().1.boxes, vec![3]);
    }

    #[test]
    fn an_inline_block_that_is_positioned_out_of_the_flow_or_in_an_item_container_is_a_block() {
        let mut f = Fixture::new();
        f.text(1, 2, "a");
        inline_block(&mut f, 1, 3, "inline-block");
        f.styled(3, serde_json::json!({"display": "inline-block", "position": "absolute"}));
        assert_eq!(f.plan(1)[1], Child::Block(3));

        for container in ["flex", "grid", "inline-flex", "inline-grid"] {
            let mut f = Fixture::new();
            f.styled(1, serde_json::json!({"display": container}));
            f.text(1, 2, "a");
            inline_block(&mut f, 1, 3, "inline-block");
            let plan = f.plan(1);
            assert_eq!(plan[1], Child::Block(3), "{container}");
        }
    }

    #[test]
    fn a_span_holding_an_inline_block_stays_inline() {
        let mut f = Fixture::new();
        f.text(1, 2, "a");
        f.add(1, 3, "span");
        inline_block(&mut f, 3, 4, "inline-flex");
        assert_eq!(f.plan(1).len(), 1);
    }

    #[test]
    fn the_placeholder_is_not_read_out() {
        let mut f = Fixture::new();
        f.add(1, 2, "text").text(2, 3, "a ");
        inline_block(&mut f, 2, 4, "inline-block");
        assert_eq!(accessible_text(&f.plan(2)).as_deref(), Some("a "));
    }

    #[test]
    fn the_inline_level_values_change_the_paragraphs() {
        let block = style(serde_json::json!({"display": "inline-block"}));
        let flex = style(serde_json::json!({"display": "inline-flex"}));
        assert!(!same_paragraphs(Some(&block), Some(&flex)));
        assert!(lays_out_items(Some(&flex)));
        assert!(!lays_out_items(Some(&block)));
    }

    #[test]
    fn accessible_text_reads_nested_inline_text() {
        let mut f = Fixture::new();
        f.add(1, 2, "text").text(2, 3, "Hello ");
        f.add(2, 4, "span").text(4, 5, "big ");
        f.add(4, 6, "text").text(6, 7, "world");
        f.add(2, 8, "div").text(8, 9, "block");
        let text = accessible_text(&f.plan(2));
        assert_eq!(text.as_deref(), Some("Hello big world"));
    }
}
