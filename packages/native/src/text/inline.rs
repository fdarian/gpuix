//! DOM inline formatting: which children of a container are laid out as one
//! paragraph of text, and which are boxes of their own.
//!
//! The rules follow CSS, not the shape of the retained tree:
//! - A raw text node is inline. So is a `<span>`, and so is a `<text>` whose
//!   parent is a `<text>` or `<span>`, unless it has behaviour of its own (see
//!   [`has_behaviour`]). A `<text>` anywhere else is a block that starts its own
//!   inline formatting context.
//! - In a block container, each maximal run of consecutive inline-level children
//!   is ONE anonymous paragraph. A block child ends the run.
//! - In a flex or grid container, children are blockified: each contiguous run
//!   of raw text nodes becomes one anonymous item, and every element child is an
//!   item of its own (a `<span>` there lays out its own content as a paragraph).
//! - A paragraph that holds nothing but collapsible whitespace generates no box,
//!   and neither does a child with `display: none`.
//!
//! Search, copy and paint all call [`plan_children`], because a paragraph is one
//! painted string and they must agree on where it starts and ends.

use std::ops::Range;

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

/// Types that lay out children with the retained tree's own `div` rules.
pub fn is_host_container(element: &RetainedElement) -> bool {
    matches!(element.element_type.as_str(), "div" | "text" | "span")
}

fn display_of(element: &RetainedElement) -> Option<&str> {
    element
        .style
        .as_deref()
        .and_then(|style| style.display.as_deref())
}

/// A child that generates no box at all.
fn is_display_none(element: &RetainedElement) -> bool {
    display_of(element) == Some("none")
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
    let Some(style) = style else { return false };
    if matches!(style.display.as_deref(), Some("flex") | Some("grid")) {
        return true;
    }
    let x = style.overflow_x.as_deref().or(style.overflow.as_deref());
    let y = style.overflow_y.as_deref().or(style.overflow.as_deref());
    x == Some("scroll") && y != Some("scroll")
}

/// Whether swapping `old` for `new` leaves every paragraph where it was: none
/// of the properties [`plan_children`] reads changed.
pub fn same_paragraphs(old: Option<&StyleDesc>, new: &StyleDesc) -> bool {
    fn read(style: Option<&StyleDesc>) -> ([Option<&str>; 6], bool) {
        let field = |get: fn(&StyleDesc) -> Option<&str>| style.and_then(get);
        (
            [
                field(|style| style.display.as_deref()),
                field(|style| style.position.as_deref()),
                field(|style| style.overflow.as_deref()),
                field(|style| style.overflow_x.as_deref()),
                field(|style| style.overflow_y.as_deref()),
                field(|style| style.white_space.as_deref()),
            ],
            style.is_some_and(has_state_styles),
        )
    }
    read(old) == read(Some(new))
}

fn has_state_styles(style: &StyleDesc) -> bool {
    style.hover.is_some()
        || style.active.is_some()
        || style.focus_visible.is_some()
        || style.group_hover.is_some()
        || style.group_active.is_some()
        || style.declared_group().is_some()
}

/// Whether `element` reacts to the pointer or the keyboard: it listens for an
/// event, takes focus, or has a state style or a `group`. Inline boxes cannot
/// take events or hit-test yet, so a `<text>` like this stays a block to keep
/// working. A `<span>` is still always inline.
pub fn has_behaviour(element: &RetainedElement) -> bool {
    !element.events().is_empty()
        || element.needs_focus()
        || element.style.as_deref().is_some_and(has_state_styles)
}

/// Whether `element`, a child of `parent` in a block container, is inline-level
/// together with everything inside it. An inline box holding a block is itself
/// laid out as a block, so a paragraph never has to split around it.
fn is_inline_box(tree: &RetainedTree, element: &RetainedElement, parent: &RetainedElement) -> bool {
    if is_raw_text(element) {
        return true;
    }
    if !matches!(element.element_type.as_str(), "div" | "text" | "span") {
        return false;
    }
    if element.custom_props().contains_key("highlight") {
        return false;
    }
    if element.element_type == "text" && has_behaviour(element) {
        return false;
    }
    if let Some(style) = element.style.as_deref() {
        if matches!(style.position.as_deref(), Some("absolute") | Some("fixed")) {
            return false;
        }
    }
    let inline_by_default = match element.element_type.as_str() {
        "span" => true,
        "text" => matches!(parent.element_type.as_str(), "text" | "span"),
        _ => false,
    };
    let inline = match display_of(element) {
        Some("inline") => true,
        Some(_) => false,
        None => inline_by_default,
    };
    inline
        && !lays_out_items(element.style.as_deref())
        && element
            .children
            .iter()
            .filter_map(|id| tree.elements.get(id))
            .filter(|child| !is_display_none(child))
            .all(|child| is_inline_box(tree, child, element))
}

/// One text node of a paragraph.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub range: Range<usize>,
    /// The inline boxes around the text, outermost first. The container is not
    /// one of them: its own style reaches the paragraph through gpui.
    pub boxes: Vec<u64>,
}

/// A run of inline content, painted as one string.
#[derive(Debug, Clone, PartialEq)]
pub struct Paragraph {
    /// The element of the first piece. It names the paragraph in the selection
    /// and search registries.
    pub owner: u64,
    pub text: String,
    pub pieces: Vec<Piece>,
    /// Every inline box that holds text, with the bytes it covers, outermost
    /// first. Hit-testing a character index back to its span reads this.
    #[cfg_attr(not(test), allow(dead_code))]
    pub boxes: Vec<(u64, Range<usize>)>,
}

impl Paragraph {
    /// Innermost inline box at `byte`, if any. Nothing hit-tests a span yet;
    /// this is the lookup that will map a character index back to one.
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
}

/// Collects one paragraph and applies `white-space` as it goes: what is pushed
/// is the text that is painted, searched and copied, and every range is a byte
/// range of that text.
struct Builder {
    owner: Option<u64>,
    text: String,
    pieces: Vec<Piece>,
    boxes: Vec<(u64, Range<usize>)>,
    tail: Tail,
    /// Characters since the last forced break, which is what a tab stop counts.
    column: usize,
    /// The container's `white-space`, then that of each open box.
    modes: Vec<WhiteSpace>,
    /// Every character that came in was collapsible whitespace.
    only_whitespace: bool,
    /// A flex or grid item of whitespace alone is not rendered, whatever its
    /// `white-space`.
    blockified: bool,
}

impl Builder {
    fn new(base: WhiteSpace, blockified: bool) -> Self {
        Self {
            owner: None,
            text: String::new(),
            pieces: Vec::new(),
            boxes: Vec::new(),
            tail: Tail::LineStart,
            column: 0,
            modes: vec![base],
            only_whitespace: true,
            blockified,
        }
    }

    fn mode(&self) -> WhiteSpace {
        *self.modes.last().expect("the container's mode is always there")
    }

    /// Removes the last character, a space or a line feed, from the text and
    /// from every range that reached it.
    fn pop(&mut self) {
        self.text.pop();
        self.column = self.column.saturating_sub(1);
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

    fn push_text(&mut self, leaf: u64, content: &str, boxes: &[u64]) {
        if content.is_empty() {
            return;
        }
        self.owner.get_or_insert(leaf);
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
                Emit::Char('\t') if mode.keeps_tabs() => {
                    let width = white_space::tab_width(self.column);
                    self.text.extend(std::iter::repeat_n(' ', width));
                    self.column += width;
                }
                Emit::Char(c) => {
                    self.text.push(c);
                    self.column += 1;
                }
                Emit::Break { trim } => {
                    if trim {
                        self.pop();
                        start = start.min(self.text.len());
                    }
                    self.text.push('\n');
                    self.column = 0;
                }
            }
        }
        if self.text.len() > start {
            self.pieces.push(Piece {
                range: start..self.text.len(),
                boxes: boxes.to_vec(),
            });
        }
    }

    fn push_element(
        &mut self,
        tree: &RetainedTree,
        element: &RetainedElement,
        chain: &mut Vec<u64>,
    ) {
        if is_raw_text(element) {
            if let Some(content) = element.content.as_deref() {
                self.push_text(element.id, content, chain);
            }
            return;
        }
        let slot = self.boxes.len();
        self.boxes.push((element.id, self.text.len()..self.text.len()));
        chain.push(element.id);
        self.modes.push(white_space::of_box(element, self.mode()));
        if let Some(content) = element.content.as_deref() {
            self.push_text(element.id, content, chain);
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
        self.boxes[slot].1.end = self.text.len();
    }

    /// Ends the paragraph. A collapsible space at its end is removed, and so is
    /// a single final line feed, which ends the last line and starts no new
    /// one. A paragraph with nothing left generates no box.
    fn finish(&mut self, out: &mut Vec<Child>) {
        let mut builder = std::mem::replace(self, Self::new(self.modes[0], self.blockified));
        let Some(owner) = builder.owner else { return };
        if builder.tail == Tail::Space {
            builder.pop();
        } else if builder.text.ends_with('\n') {
            builder.pop();
        }
        if builder.text.is_empty() || (builder.blockified && builder.only_whitespace) {
            return;
        }
        builder.boxes.retain(|(_, range)| !range.is_empty());
        out.push(Child::Paragraph(Paragraph {
            owner,
            text: builder.text,
            pieces: builder.pieces,
            boxes: builder.boxes,
        }));
    }
}

/// How `container` builds its children, in order.
///
/// The container's own `content` leads as text, like a text node that comes
/// first.
pub fn plan_children(tree: &RetainedTree, container: &RetainedElement) -> Vec<Child> {
    let items = lays_out_items(container.style.as_deref());
    let mut out = Vec::new();
    let mut paragraph = Builder::new(white_space::computed(tree, container), items);
    if let Some(content) = container.content.as_deref() {
        paragraph.push_text(container.id, content, &[]);
    }
    let children = crate::layout::order::children_in_order(tree, container, container.style.as_deref());
    for child in children
        .iter()
        .filter_map(|id| tree.elements.get(id))
        .filter(|child| !is_display_none(child))
    {
        if is_raw_text(child) {
            if let Some(content) = child.content.as_deref() {
                paragraph.push_text(child.id, content, &[]);
            }
        } else if !items && is_inline_box(tree, child, container) {
            paragraph.push_element(tree, child, &mut Vec::new());
        } else {
            paragraph.finish(&mut out);
            out.push(Child::Block(child.id));
        }
    }
    paragraph.finish(&mut out);
    out
}

/// The text a node with no children paints, after `white-space`: `content` of
/// `element`, which is a text node or a host that carries a string. `None` when
/// nothing is left of it.
pub fn own_text(tree: &RetainedTree, element: &RetainedElement) -> Option<String> {
    let content = element.content.as_deref()?;
    let mut paragraph = Builder::new(white_space::computed(tree, element), false);
    paragraph.push_text(element.id, content, &[]);
    let mut out = Vec::new();
    paragraph.finish(&mut out);
    match out.pop() {
        Some(Child::Paragraph(paragraph)) => Some(paragraph.text),
        _ => None,
    }
}

/// The text an inline formatting context reads out, as painted: its own content
/// and that of every inline box inside it, in order.
pub fn accessible_text(tree: &RetainedTree, element: &RetainedElement) -> Option<String> {
    let text: String = plan_children(tree, element)
        .into_iter()
        .filter_map(|child| match child {
            Child::Paragraph(paragraph) => Some(paragraph.text),
            Child::Block(_) => None,
        })
        .collect();
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
                Child::Block(_) => None,
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
        assert_eq!(paragraph.owner, 2);
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
        assert_eq!(paragraphs(&plan)[1].owner, 6);
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
        assert_eq!(paragraph.owner, 1);
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
        assert!(same_paragraphs(Some(&flex), &colored));
        assert!(same_paragraphs(None, &StyleDesc::default()));
        assert!(!same_paragraphs(None, &flex));
        assert!(!same_paragraphs(
            Some(&flex),
            &style(serde_json::json!({"display": "none"}))
        ));
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
    fn a_nested_text_with_behaviour_stays_a_block() {
        let plain = nested_text_plan(|f| {
            f.styled(3, serde_json::json!({"color": "red"}));
        });
        assert_eq!(plain.len(), 1, "style alone keeps it inline");
        assert_eq!(paragraphs(&plain)[0].text, "one two three");

        let behaviours: [(&str, Box<dyn Fn(&mut Fixture)>); 8] = [
            ("listener", Box::new(|f| f.tree.set_event_listener(3, "click".into(), true))),
            ("tabIndex", Box::new(|f| f.tree.set_custom_prop(3, "tabIndex".into(), serde_json::json!(0)))),
            ("hover", Box::new(|f| { f.styled(3, serde_json::json!({"hover": {"color": "red"}})); })),
            ("active", Box::new(|f| { f.styled(3, serde_json::json!({"active": {"color": "red"}})); })),
            ("focusVisible", Box::new(|f| { f.styled(3, serde_json::json!({"focusVisible": {"color": "red"}})); })),
            ("groupHover", Box::new(|f| { f.styled(3, serde_json::json!({"groupHover": {"color": "red"}})); })),
            ("groupActive", Box::new(|f| { f.styled(3, serde_json::json!({"groupActive": {"color": "red"}})); })),
            ("group", Box::new(|f| { f.styled(3, serde_json::json!({"group": true})); })),
        ];
        for (name, configure) in behaviours {
            let plan = nested_text_plan(|f| configure(f));
            assert_eq!(plan.len(), 3, "{name}: paragraph, block, paragraph");
            assert_eq!(plan[1], Child::Block(3), "{name}");
            // The space at the end of a line, and the one at the start of the
            // next, are removed.
            assert_eq!(paragraphs(&plan)[0].text, "one", "{name}");
            assert_eq!(paragraphs(&plan)[1].text, "three", "{name}");
        }
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
    fn state_styles_regroup() {
        let plain = style(serde_json::json!({"color": "red"}));
        let hover = style(serde_json::json!({"color": "red", "hover": {"color": "blue"}}));
        assert!(!same_paragraphs(Some(&plain), &hover));
        assert!(!same_paragraphs(Some(&hover), &plain));
        assert!(same_paragraphs(
            Some(&hover),
            &style(serde_json::json!({"hover": {"color": "green"}}))
        ));
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
    fn a_tab_in_pre_expands_to_the_next_multiple_of_eight_columns() {
        let mut f = Fixture::new();
        f.styled(1, serde_json::json!({"whiteSpace": "pre-wrap"}));
        f.text(1, 2, "ab\tc\n\td");
        assert_eq!(text_of(&f, 1), "ab      c\n        d");
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
        assert!(!same_paragraphs(Some(&normal), &pre));
    }

    #[test]
    fn accessible_text_reads_nested_inline_text() {
        let mut f = Fixture::new();
        f.add(1, 2, "text").text(2, 3, "Hello ");
        f.add(2, 4, "span").text(4, 5, "big ");
        f.add(4, 6, "text").text(6, 7, "world");
        f.add(2, 8, "div").text(8, 9, "block");
        let text = accessible_text(&f.tree, &f.tree.elements[&2]);
        assert_eq!(text.as_deref(), Some("Hello big world"));
    }
}
