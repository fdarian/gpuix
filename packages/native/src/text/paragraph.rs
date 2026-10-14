//! Painting a paragraph whose inline boxes style their own runs.
//!
//! [`super::inline`] decides what a paragraph is. This turns each text node's
//! chain of inline boxes into the properties of its run, and lays the string out
//! with them.
//!
//! The runs are built at layout time, on top of `window.text_style()`: a run a
//! box does not restyle keeps the container's colour, weight and family, which
//! only gpui knows once the element is in the tree.

use gpui::{
    px, FontWeight, Hsla, SharedString, StrikethroughStyle, StyledText, TextLayout, TextRun,
    UnderlineStyle,
};

use crate::color::ColorContext;
use crate::css::font_features::FontFeatureState;
use crate::retained_tree::RetainedTree;

use super::inline::Paragraph;

/// What one run changes about the text style it inherits.
///
/// A box's run inherits from its parent box as in CSS, so the entry for a
/// nested box already holds everything the boxes around it declared. A
/// background is carried the same way: the parent's shows behind the child's
/// text unless the child paints its own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunStyle {
    pub len: usize,
    pub color: Option<Hsla>,
    pub background: Option<Hsla>,
    pub weight: Option<FontWeight>,
    pub family: Option<SharedString>,
    pub features: Option<gpui::FontFeatures>,
    pub underline: bool,
    pub strikethrough: bool,
}

impl RunStyle {
    fn same_style(&self, other: &Self) -> bool {
        Self {
            len: 0,
            ..self.clone()
        } == Self {
            len: 0,
            ..other.clone()
        }
    }

    fn inherits_everything(&self) -> bool {
        self.same_style(&Self::default())
    }
}

/// The runs of `paragraph`, covering its text exactly. Empty when no box
/// restyles anything, so the caller can paint it as plain text.
///
/// `colors` and `font_state` are the container's, as its children inherit them.
pub fn run_styles(
    tree: &RetainedTree,
    paragraph: &Paragraph,
    colors: &ColorContext,
    font_state: &FontFeatureState,
) -> Vec<RunStyle> {
    let mut runs: Vec<RunStyle> = Vec::with_capacity(paragraph.pieces.len());
    for piece in &paragraph.pieces {
        let mut style = RunStyle {
            len: piece.range.len(),
            ..RunStyle::default()
        };
        let mut colors = *colors;
        let mut font_state = font_state.clone();
        for id in &piece.boxes {
            let Some(declared) = tree
                .elements
                .get(id)
                .and_then(|element| element.style.as_deref())
            else {
                continue;
            };
            colors = colors.descend(declared);
            if let Some(color) = colors.declared {
                style.color = Some(color.into());
            }
            if let Some(background) = declared
                .background_color
                .as_deref()
                .and_then(|value| colors.resolve(value))
            {
                style.background = Some(background.into());
            }
            if let Some(weight) = declared.font_weight.as_ref() {
                style.weight = Some(crate::renderer::parse_font_weight(weight));
            }
            if let Some(family) = declared.font_family.as_ref() {
                style.family = Some(SharedString::from(family.clone()));
            }
            let (next, features) = font_state.descend(
                declared.font_variant_numeric.as_deref(),
                declared.font_feature_settings.as_deref(),
            );
            font_state = next;
            if features.is_some() {
                style.features = features;
            }
            // CSS draws a decoration over the whole box and a descendant cannot
            // take it off, so `none` on a box adds nothing and removes nothing.
            match declared.text_decoration.as_deref() {
                Some("underline") => style.underline = true,
                Some("line-through") => style.strikethrough = true,
                _ => {}
            }
        }
        match runs.last_mut() {
            Some(last) if last.same_style(&style) => last.len += style.len,
            _ => runs.push(style),
        }
    }
    if runs.iter().all(RunStyle::inherits_everything) {
        return Vec::new();
    }
    runs
}

/// `StyledText` whose runs are `styles` laid over the inherited text style.
pub struct InlineText {
    styled: StyledText,
    styles: Vec<RunStyle>,
}

impl InlineText {
    pub fn new(text: SharedString, styles: Vec<RunStyle>) -> Self {
        assert_eq!(
            styles.iter().map(|style| style.len).sum::<usize>(),
            text.len(),
            "inline runs must cover the paragraph exactly"
        );
        Self {
            styled: StyledText::new(text),
            styles,
        }
    }

    pub fn layout(&self) -> &TextLayout {
        self.styled.layout()
    }

    fn runs(&self, base: &gpui::TextStyle) -> Vec<TextRun> {
        self.styles
            .iter()
            .map(|style| {
                let mut run = base.to_run(style.len);
                if let Some(color) = style.color {
                    run.color = color;
                }
                if style.background.is_some() {
                    run.background_color = style.background;
                }
                if let Some(weight) = style.weight {
                    run.font.weight = weight;
                }
                if let Some(family) = &style.family {
                    run.font.family = family.clone();
                }
                if let Some(features) = &style.features {
                    run.font.features = features.clone();
                }
                if style.underline {
                    run.underline = Some(UnderlineStyle {
                        thickness: px(1.0),
                        ..Default::default()
                    });
                }
                if style.strikethrough {
                    run.strikethrough = Some(StrikethroughStyle {
                        thickness: px(1.0),
                        ..Default::default()
                    });
                }
                run
            })
            .collect()
    }
}

impl gpui::IntoElement for InlineText {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl gpui::Element for InlineText {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> (gpui::LayoutId, ()) {
        let runs = self.runs(&window.text_style());
        // `with_runs` takes the element by value. The placeholder is dropped
        // at once; the layout cell that callers cloned lives in `styled`.
        let styled = std::mem::replace(&mut self.styled, StyledText::new(""));
        self.styled = styled.with_runs(runs);
        gpui::Element::request_layout(&mut self.styled, id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        state: &mut (),
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        gpui::Element::prepaint(
            &mut self.styled,
            id,
            inspector_id,
            bounds,
            state,
            window,
            cx,
        )
    }

    fn paint(
        &mut self,
        id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        request_layout: &mut (),
        prepaint: &mut (),
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        gpui::Element::paint(
            &mut self.styled,
            id,
            inspector_id,
            bounds,
            request_layout,
            prepaint,
            window,
            cx,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Scheme;
    use crate::text::inline::plan_children;
    use crate::text::inline::Child;
    use std::sync::Arc;

    fn tree_with(styles: &[(u64, serde_json::Value)]) -> RetainedTree {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        tree.create_element(2, "text".to_string());
        tree.append_child(1, 2);
        tree.set_text(2, "a ".to_string());
        tree.create_element(3, "span".to_string());
        tree.append_child(1, 3);
        tree.create_element(4, "text".to_string());
        tree.append_child(3, 4);
        tree.set_text(4, "bold".to_string());
        tree.create_element(5, "span".to_string());
        tree.append_child(3, 5);
        tree.create_element(6, "text".to_string());
        tree.append_child(5, 6);
        tree.set_text(6, "red".to_string());
        tree.create_element(7, "text".to_string());
        tree.append_child(1, 7);
        tree.set_text(7, " end".to_string());
        for (id, style) in styles {
            let style = serde_json::from_value(style.clone()).expect("a style");
            tree.set_style(*id, Arc::new(style));
        }
        tree
    }

    fn runs_of(tree: &RetainedTree) -> Vec<RunStyle> {
        let plan = plan_children(tree, &tree.elements[&1]);
        let Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        run_styles(
            tree,
            paragraph,
            &ColorContext::root(Scheme::Light),
            &FontFeatureState::default(),
        )
    }

    #[test]
    fn unstyled_boxes_leave_the_paragraph_plain() {
        assert!(runs_of(&tree_with(&[])).is_empty());
    }

    #[test]
    fn a_nested_box_inherits_from_its_parent_box() {
        let tree = tree_with(&[
            (
                3,
                serde_json::json!({"color": "#ff0000", "fontWeight": 700}),
            ),
            (5, serde_json::json!({"backgroundColor": "#ffff00"})),
        ]);
        let runs = runs_of(&tree);
        let lens: Vec<_> = runs.iter().map(|run| run.len).collect();
        assert_eq!(lens, vec![2, 4, 3, 4]);
        assert_eq!(
            runs[0],
            RunStyle {
                len: 2,
                ..RunStyle::default()
            }
        );
        assert_eq!(runs[1].weight, Some(FontWeight(700.0)));
        assert_eq!(runs[1].background, None);
        assert_eq!(runs[2].weight, Some(FontWeight(700.0)));
        assert_eq!(runs[2].color, runs[1].color);
        assert!(runs[2].background.is_some());
        assert_eq!(
            runs[3],
            RunStyle {
                len: 4,
                ..RunStyle::default()
            }
        );
    }

    #[test]
    fn a_child_colour_wins_and_current_color_means_the_parents() {
        let tree = tree_with(&[
            (3, serde_json::json!({"color": "#ff0000"})),
            (
                5,
                serde_json::json!({"color": "#0000ff", "backgroundColor": "currentColor"}),
            ),
        ]);
        let runs = runs_of(&tree);
        assert_ne!(runs[1].color, runs[2].color);
        assert_eq!(runs[2].background, runs[2].color);
    }

    #[test]
    fn adjacent_pieces_with_one_style_share_a_run() {
        let tree = tree_with(&[(3, serde_json::json!({"color": "#ff0000"}))]);
        let runs = runs_of(&tree);
        let lens: Vec<_> = runs.iter().map(|run| run.len).collect();
        assert_eq!(lens, vec![2, 7, 4], "bold and red are one red run");
    }

    #[test]
    fn decorations_accumulate_and_none_removes_nothing() {
        let tree = tree_with(&[
            (3, serde_json::json!({"textDecoration": "underline"})),
            (5, serde_json::json!({"textDecoration": "none"})),
        ]);
        let runs = runs_of(&tree);
        assert!(runs[1].underline);
        assert!(runs[2].underline);
    }

    #[test]
    fn runs_cover_the_text_exactly() {
        let tree = tree_with(&[(5, serde_json::json!({"fontWeight": 600}))]);
        assert_eq!(runs_of(&tree).iter().map(|run| run.len).sum::<usize>(), 13);
    }
}
