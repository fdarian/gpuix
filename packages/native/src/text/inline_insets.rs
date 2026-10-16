//! The space an inline box takes along the line.
//!
//! A `<span>`'s margin, border and padding on the inline axis push the text
//! around it, widen the line and take part in wrapping. gpui lays that out as an
//! inset around a byte range of the text ([`gpui::TextInset`]); this module reads
//! it from the boxes' styles.
//!
//! gpui puts the leading inset before the first glyph of the range and the
//! trailing inset after its last. A box that wraps has the leading inset on its
//! first line and the trailing on its last; a first fragment that begins a
//! soft-wrapped row still has its leading inset, because the inset belongs to the
//! glyph and goes where the glyph goes.
//!
//! Only the inline axis is read. The top and bottom margin, padding and border
//! of an inline box do not move a line in CSS. An inset on a box that holds no
//! glyph has nothing to attach to and is lost.

use std::ops::Range;

use gpui::{px, Bounds, Pixels, TextInset, TextLayout};

use crate::css::border::parse_border;
use crate::events::state_styles::{folded, PointerStates};
use crate::retained_tree::RetainedTree;
use crate::style::{DimensionValue, StyleDesc};

use super::inline::Paragraph;

/// The inline-axis space of one box: margin, border and padding before its
/// first glyph and after its last.
#[derive(Clone, Debug, PartialEq)]
struct BoxInset {
    range: Range<usize>,
    leading: f32,
    trailing: f32,
}

impl BoxInset {
    fn is_empty(&self) -> bool {
        self.leading == 0.0 && self.trailing == 0.0
    }
}

/// The insets of the boxes of one paragraph.
#[derive(Debug, Default, PartialEq)]
pub struct Insets {
    boxes: Vec<BoxInset>,
}

impl Insets {
    /// What `paragraph`'s boxes put around their text.
    ///
    /// `states` says which states apply to a box, as it does for
    /// [`super::paragraph::run_styles`], so a `hover` padding is folded in.
    pub fn of(
        tree: &RetainedTree,
        paragraph: &Paragraph,
        states: &dyn Fn(u64, &StyleDesc) -> PointerStates,
    ) -> Self {
        let boxes: Vec<BoxInset> = paragraph
            .boxes
            .iter()
            .filter_map(|(id, range)| {
                let base = tree.elements.get(id)?.style.as_deref()?;
                let declared = folded(base, states(*id, base));
                // It has no box to put space around.
                if declared.display.as_deref() == Some("contents") {
                    return None;
                }
                let inset = of_style(&declared, range);
                (!inset.is_empty()).then_some(inset)
            })
            .collect();
        Self { boxes }
    }

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
}

fn of_style(style: &StyleDesc, range: &Range<usize>) -> BoxInset {
    let leading = margin(style.margin_left.as_ref().or(style.margin.as_ref()))
        + border_width(style, style.border_left_width, style.border_left.as_deref())
        + padding(style.padding_left.or(style.padding));
    let trailing = margin(style.margin_right.as_ref().or(style.margin.as_ref()))
        + border_width(style, style.border_right_width, style.border_right.as_deref())
        + padding(style.padding_right.or(style.padding));
    BoxInset {
        range: range.clone(),
        leading,
        trailing,
    }
}

/// A percentage, `auto` and `anchor-size()` are not resolved: there is no
/// containing block width to take a percentage of, and an inline box's `auto`
/// margin is zero in CSS anyway. They are 0 here (README documents it).
fn margin(value: Option<&DimensionValue>) -> f32 {
    match value {
        Some(DimensionValue::Pixels(pixels)) => *pixels as f32,
        _ => 0.0,
    }
}

fn padding(value: Option<f64>) -> f32 {
    value.unwrap_or(0.0).max(0.0) as f32
}

/// The width of one border side as the element builder applies it: the
/// `borderWidth` and side longhands win over the shorthands, and a side
/// shorthand over `border`.
fn border_width(style: &StyleDesc, side: Option<f64>, side_shorthand: Option<&str>) -> f32 {
    let shorthand = |value: Option<&str>| value.and_then(parse_border).map(|parsed| parsed.width);
    side.or(style.border_width)
        .or_else(|| shorthand(side_shorthand))
        .or_else(|| shorthand(style.border.as_deref()))
        .unwrap_or(0.0)
        .max(0.0) as f32
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
    ) -> Insets {
        let plan = plan_children(tree, &tree.elements[&1]);
        let Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        Insets::of(tree, paragraph, states)
    }

    fn resting(tree: &RetainedTree) -> Insets {
        insets_with(tree, &|_, _| PointerStates::default())
    }

    fn texts(insets: &Insets) -> Vec<(Range<usize>, f32, f32)> {
        insets
            .text()
            .into_iter()
            .map(|inset| (inset.range, f32::from(inset.leading), f32::from(inset.trailing)))
            .collect()
    }

    #[test]
    fn a_paragraph_without_space_has_no_insets() {
        let insets = resting(&fixture(&[(3, serde_json::json!({"color": "red"}))]));
        assert!(insets.text().is_empty());
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
        assert_eq!(texts(&resting(&tree)), vec![(2..4, 10.0, 12.0)]);
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
    fn a_margin_that_is_not_pixels_is_zero() {
        let tree = fixture(&[(
            3,
            serde_json::json!({"marginLeft": "10%", "marginRight": "auto", "paddingLeft": 2}),
        )]);
        assert_eq!(texts(&resting(&tree)), vec![(2..4, 2.0, 0.0)]);
    }

    #[test]
    fn a_negative_margin_pulls_and_negative_padding_and_border_are_zero() {
        let tree = fixture(&[(
            3,
            serde_json::json!({
                "marginLeft": -4, "paddingLeft": -9, "borderLeftWidth": -2, "paddingRight": 3
            }),
        )]);
        assert_eq!(texts(&resting(&tree)), vec![(2..4, -4.0, 3.0)]);
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
    fn nested_boxes_that_cover_the_same_bytes_each_have_their_own_inset() {
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
        // The outer box first, as gpui sums them on the glyph they share.
        assert_eq!(texts(&resting(&tree)), vec![(2..3, 3.0, 30.0), (2..3, 12.0, 11.0)]);
    }
}
