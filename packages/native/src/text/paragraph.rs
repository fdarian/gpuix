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
    px, FontWeight, Hsla, SharedString, StrikethroughStyle, StyledText, TextInset, TextLayout,
    TextRun, UnderlineStyle,
};

use crate::color::ColorContext;
use crate::css::font_features::FontFeatureState;
use crate::css::text_decoration;
use crate::events::state_styles::{folded, PointerStates};
use crate::retained_tree::RetainedTree;
use crate::style::StyleDesc;

use super::inline::Paragraph;

/// What one run changes about the text style it inherits.
///
/// A box's run inherits from its parent box as in CSS, so the entry for a
/// nested box already holds everything the boxes around it declared. A box's
/// background and border are not here: they are painted around its line
/// fragments by [`super::inline_decoration`], so that its padding is inside them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunStyle {
    pub len: usize,
    pub color: Option<Hsla>,
    pub weight: Option<FontWeight>,
    pub font_style: Option<gpui::FontStyle>,
    pub family: Option<SharedString>,
    pub features: Option<gpui::FontFeatures>,
    pub underline: Option<Line>,
    pub strikethrough: Option<Line>,
}

/// A decoration line of a run. Its colour is the one of the box that declared
/// it, so a descendant that changes `color` does not recolour it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub color: Hsla,
    pub wavy: bool,
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
/// `states` says which states apply to a box, so that its `hover`, `active`,
/// `focusWithin` and `focusVisible` styles are folded into the run it paints.
pub fn run_styles(
    tree: &RetainedTree,
    paragraph: &Paragraph,
    colors: &ColorContext,
    font_state: &FontFeatureState,
    states: &dyn Fn(u64, &StyleDesc) -> PointerStates,
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
            let Some(base) = tree
                .elements
                .get(id)
                .and_then(|element| element.style.as_deref())
            else {
                continue;
            };
            let declared = &*folded(base, states(*id, base));
            colors = colors.descend(declared);
            // It has no box, so what it declares reaches the text only as far as
            // it is inherited.
            let has_box = declared.display.as_deref() != Some("contents");
            if let Some(color) = colors.declared {
                style.color = Some(color.into());
            }
            if let Some(weight) = declared.font_weight.as_ref() {
                style.weight = Some(crate::renderer::parse_font_weight(weight));
            }
            if let Some(font_style) = declared
                .font_style
                .as_deref()
                .and_then(crate::renderer::parse_font_style)
            {
                style.font_style = Some(font_style);
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
            if has_box {
                if let Some(decoration) = text_decoration::of(declared) {
                    let color = decoration.resolve_color(&colors);
                    if decoration.underline {
                        style.underline = Some(Line {
                            color,
                            wavy: decoration.wavy(),
                        });
                    }
                    if decoration.line_through {
                        style.strikethrough = Some(Line { color, wavy: false });
                    }
                }
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

    /// Lays the text out with the space `insets` put around ranges of it at the
    /// width the text is laid out in, see [`super::inline_insets`].
    pub fn with_insets_for_width(
        mut self,
        insets: impl Fn(Option<gpui::Pixels>) -> Vec<TextInset> + 'static,
    ) -> Self {
        self.styled = self.styled.with_insets_for_width(insets);
        self
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
                if let Some(weight) = style.weight {
                    run.font.weight = weight;
                }
                if let Some(font_style) = style.font_style {
                    run.font.style = font_style;
                }
                if let Some(family) = &style.family {
                    run.font.family = family.clone();
                }
                if let Some(features) = &style.features {
                    run.font.features = features.clone();
                }
                if let Some(line) = style.underline {
                    run.underline = Some(UnderlineStyle {
                        thickness: px(1.0),
                        color: Some(line.color),
                        wavy: line.wavy,
                    });
                }
                if let Some(line) = style.strikethrough {
                    run.strikethrough = Some(StrikethroughStyle {
                        thickness: px(1.0),
                        color: Some(line.color),
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
        runs_with(tree, &|_, _| PointerStates::default())
    }

    fn runs_with(
        tree: &RetainedTree,
        states: &dyn Fn(u64, &StyleDesc) -> PointerStates,
    ) -> Vec<RunStyle> {
        let plan = plan_children(tree, &tree.elements[&1]);
        let Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        run_styles(
            tree,
            paragraph,
            &ColorContext::root(Scheme::Light),
            &FontFeatureState::default(),
            states,
        )
    }

    #[test]
    fn a_hovered_box_paints_its_hover_style_and_its_children_inherit_it() {
        let tree = tree_with(&[
            (
                3,
                serde_json::json!({
                    "color": "#ff0000",
                    "hover": {"color": "#0000ff", "fontWeight": 700}
                }),
            ),
            (5, serde_json::json!({"fontStyle": "italic"})),
        ]);
        let rest = runs_of(&tree);
        let hovered = runs_with(&tree, &|id, _| PointerStates {
            hovered: id == 3,
            ..Default::default()
        });
        assert_eq!(rest[1].weight, None);
        assert_ne!(hovered[1].color, rest[1].color);
        assert_eq!(hovered[1].weight, Some(FontWeight(700.0)));
        assert_eq!(
            hovered[2].color, hovered[1].color,
            "the span nested in it inherits the hover colour"
        );
        assert_eq!(hovered[0], rest[0], "the text before it does not change");
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
            (5, serde_json::json!({"fontStyle": "italic"})),
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
        assert_eq!(runs[1].font_style, None);
        assert_eq!(runs[2].weight, Some(FontWeight(700.0)));
        assert_eq!(runs[2].color, runs[1].color);
        assert_eq!(runs[2].font_style, Some(gpui::FontStyle::Italic));
        assert_eq!(
            runs[3],
            RunStyle {
                len: 4,
                ..RunStyle::default()
            }
        );
    }

    #[test]
    fn a_child_colour_wins_over_its_parents() {
        let tree = tree_with(&[
            (3, serde_json::json!({"color": "#ff0000"})),
            (5, serde_json::json!({"color": "#0000ff"})),
        ]);
        let runs = runs_of(&tree);
        assert_ne!(runs[1].color, runs[2].color);
    }

    #[test]
    fn a_background_is_not_part_of_the_run() {
        let tree = tree_with(&[(
            5,
            serde_json::json!({"backgroundColor": "#ffff00", "borderTopWidth": 2, "paddingLeft": 4}),
        )]);
        assert!(runs_of(&tree).is_empty(), "the box paints it around its fragments");
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
        let lens: Vec<_> = runs.iter().map(|run| run.len).collect();
        assert_eq!(lens, vec![2, 7, 4], "the nested none keeps the parent's underline");
        assert!(runs[1].underline.is_some());
        assert!(runs[2].underline.is_none());
    }

    #[test]
    fn a_line_keeps_the_colour_of_the_box_that_declared_it() {
        let tree = tree_with(&[
            (3, serde_json::json!({"color": "#ff0000", "textDecoration": "underline"})),
            (5, serde_json::json!({"color": "#0000ff"})),
        ]);
        let runs = runs_of(&tree);
        let red = runs[1].underline.expect("underlined").color;
        assert_eq!(runs[1].color, Some(red));
        assert_eq!(runs[2].underline.expect("inherited").color, red);
        assert_ne!(runs[2].color, Some(red));
    }

    #[test]
    fn lines_have_their_own_style_and_colour() {
        let tree = tree_with(&[
            (3, serde_json::json!({"textDecoration": "underline wavy #00ff00"})),
            (5, serde_json::json!({"textDecorationLine": "line-through", "textDecorationColor": "#0000ff"})),
        ]);
        let runs = runs_of(&tree);
        let nested = runs.iter().find(|run| run.strikethrough.is_some()).expect("a run");
        let underline = nested.underline.expect("the parent's underline");
        assert!(underline.wavy);
        assert_ne!(nested.strikethrough.unwrap().color, underline.color);
        assert!(!nested.strikethrough.unwrap().wavy, "gpui has no wavy strikethrough");
    }

    #[test]
    fn a_display_contents_box_passes_inherited_style_on_and_paints_nothing_of_its_own() {
        let tree = tree_with(&[
            (
                5,
                serde_json::json!({
                    "display": "contents",
                    "color": "#ff0000",
                    "fontWeight": 700,
                    "backgroundColor": "#ffff00",
                    "textDecoration": "underline"
                }),
            ),
        ]);
        let runs = runs_of(&tree);
        assert_eq!(runs[1].weight, Some(FontWeight(700.0)));
        assert!(runs[1].color.is_some());
        assert_eq!(runs[1].underline, None);
    }

    #[test]
    fn a_direct_display_contents_child_passes_its_style_to_the_text() {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        tree.create_element(2, "div".to_string());
        tree.append_child(1, 2);
        tree.create_element(3, "text".to_string());
        tree.append_child(2, 3);
        tree.set_text(3, "red".to_string());
        let style = serde_json::json!({"display": "contents", "color": "#ff0000", "fontWeight": 700});
        tree.set_style(2, Arc::new(serde_json::from_value(style).expect("a style")));
        let runs = runs_of(&tree);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].weight, Some(FontWeight(700.0)));
        assert!(runs[0].color.is_some());
    }

    #[test]
    fn font_style_is_inherited_and_a_child_can_reset_it() {
        let tree = tree_with(&[
            (3, serde_json::json!({"fontStyle": "italic"})),
            (5, serde_json::json!({"fontStyle": "normal"})),
        ]);
        let runs = runs_of(&tree);
        assert_eq!(runs[1].font_style, Some(gpui::FontStyle::Italic));
        assert_eq!(runs[2].font_style, Some(gpui::FontStyle::Normal));
    }

    #[test]
    fn a_focused_box_paints_its_focus_styles_and_an_ancestor_only_its_focus_within() {
        use crate::focus::FocusStyles;

        let ring = |color: &str| {
            serde_json::json!({
                "color": color,
                "focusVisible": {"color": "#ff0000"},
                "focusWithin": {"fontWeight": 700}
            })
        };
        let tree = tree_with(&[(3, ring("#000000")), (5, ring("#00ff00"))]);
        let rest = runs_of(&tree);
        let focus = FocusStyles {
            visible: Some(5),
            within: Arc::from([5, 3, 1]),
            ..Default::default()
        };
        let focused = runs_with(&tree, &|id, style| {
            focus.states_of(id, style, PointerStates::default())
        });

        assert_eq!(focused[1].weight, Some(FontWeight(700.0)), "the box around it is focus-within");
        assert_eq!(focused[1].color, rest[1].color, "but not focus-visible");
        assert_eq!(focused[2].weight, Some(FontWeight(700.0)));
        assert_ne!(focused[2].color, rest[2].color);
        assert_eq!(focused[0], rest[0]);
        assert_eq!(focused[3], rest[3], "the text after them is untouched");
    }

    #[test]
    fn focus_beats_hover_on_the_same_field() {
        let tree = tree_with(&[(
            3,
            serde_json::json!({
                "hover": {"color": "#0000ff"},
                "focusVisible": {"color": "#ff0000"}
            }),
        )]);
        let both = runs_with(&tree, &|_, _| PointerStates {
            hovered: true,
            focus_visible: true,
            ..Default::default()
        });
        let focus_only = runs_with(&tree, &|_, _| PointerStates {
            focus_visible: true,
            ..Default::default()
        });
        assert_eq!(both[1].color, focus_only[1].color);
    }

    #[test]
    fn runs_cover_the_text_exactly() {
        let tree = tree_with(&[(5, serde_json::json!({"fontWeight": 600}))]);
        assert_eq!(runs_of(&tree).iter().map(|run| run.len).sum::<usize>(), 13);
    }
}
