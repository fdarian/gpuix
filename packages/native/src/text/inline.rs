//! DOM inline formatting: which children of a container are laid out as one
//! paragraph of text, and which are boxes of their own.
//!
//! The rules follow CSS, not the shape of the retained tree:
//! - A raw text node is inline. So is a `<span>`, and so is a `<text>` whose
//!   parent is a `<text>` or `<span>`. A `<text>` anywhere else is a block that
//!   starts its own inline formatting context.
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
    fn read(style: Option<&StyleDesc>) -> [Option<&str>; 5] {
        let field = |get: fn(&StyleDesc) -> Option<&str>| style.and_then(get);
        [
            field(|style| style.display.as_deref()),
            field(|style| style.position.as_deref()),
            field(|style| style.overflow.as_deref()),
            field(|style| style.overflow_x.as_deref()),
            field(|style| style.overflow_y.as_deref()),
        ]
    }
    read(old) == read(Some(new))
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
    /// The element that holds the text.
    pub leaf: u64,
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
    pub boxes: Vec<(u64, Range<usize>)>,
}

impl Paragraph {
    /// Whether the paragraph has any inline box around its text.
    pub fn has_boxes(&self) -> bool {
        self.pieces.iter().any(|piece| !piece.boxes.is_empty())
    }

    /// Innermost inline box at `byte`, if any.
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

#[derive(Default)]
struct Builder {
    owner: Option<u64>,
    text: String,
    pieces: Vec<Piece>,
    boxes: Vec<(u64, Range<usize>)>,
}

impl Builder {
    fn push_text(&mut self, leaf: u64, content: &str, boxes: &[u64]) {
        if content.is_empty() {
            return;
        }
        self.owner.get_or_insert(leaf);
        let start = self.text.len();
        self.text.push_str(content);
        self.pieces.push(Piece {
            leaf,
            range: start..self.text.len(),
            boxes: boxes.to_vec(),
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
                self.push_text(element.id, content, chain);
            }
            return;
        }
        let start = self.text.len();
        let slot = self.boxes.len();
        self.boxes.push((element.id, start..start));
        chain.push(element.id);
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
        chain.pop();
        let end = self.text.len();
        if end > start {
            self.boxes[slot].1 = start..end;
        } else {
            self.boxes.remove(slot);
        }
    }

    /// Ends the paragraph. One with no content, or only collapsible whitespace,
    /// generates no box.
    fn finish(&mut self, out: &mut Vec<Child>) {
        let builder = std::mem::take(self);
        let Some(owner) = builder.owner else { return };
        if is_blank(&builder.text) {
            return;
        }
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
    let mut paragraph = Builder::default();
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

/// The text an inline formatting context reads out: its own content and that of
/// every inline box inside it, in order.
pub fn accessible_text(tree: &RetainedTree, element: &RetainedElement) -> Option<String> {
    fn collect(tree: &RetainedTree, element: &RetainedElement, out: &mut String) {
        if let Some(content) = element.content.as_deref() {
            out.push_str(content);
        }
        for child in element
            .children
            .iter()
            .filter_map(|id| tree.elements.get(id))
            .filter(|child| !is_display_none(child))
        {
            if is_raw_text(child) || is_inline_box(tree, child, element) {
                collect(tree, child, out);
            }
        }
    }
    let mut text = String::new();
    collect(tree, element, &mut text);
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
        assert!(!paragraph.has_boxes());
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
