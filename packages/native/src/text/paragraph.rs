//! Painting a paragraph whose inline boxes style their own runs.
//!
//! [`super::inline`] decides what a paragraph is. This turns each text node's
//! chain of inline boxes into the properties of its run, and lays the string out
//! with them.
//!
//! The runs are built at layout time, on top of `window.text_style()`: a run a
//! box does not restyle keeps the container's colour, weight and family, which
//! only gpui knows once the element is in the tree.

use std::collections::HashMap;

use gpui::{
    px, DefiniteLength, ExtraDecoration, FontWeight, Hsla, OverlineStyle, Pixels, SharedString, StrikethroughStyle,
    StyledText, TextInset, TextLayout, TextRun, UnderlineStyle,
};

use crate::color::ColorContext;
use crate::css::font_features::FontFeatureState;
use crate::css::length::LengthContext;
use crate::css::text_decoration;
use crate::events::state_styles::{folded, PointerStates};
use crate::retained_tree::RetainedTree;
use crate::style::StyleDesc;

use crate::motion::InlineStyles;

use super::inline::Paragraph;

/// What one run changes about the text style it inherits.
///
/// A box's run inherits from its parent box as in CSS, so the entry for a
/// nested box already holds everything the boxes around it declared. A box's
/// background and border are not here: they are painted around its line
/// fragments by [`super::inline_decoration`], so that its padding is inside them.
///
/// `line_height` is what the innermost box that declared one gives, resolved
/// where it was declared as `line-height` is inherited: a multiplier stays a
/// multiplier, so a descendant with another size gets its own multiple, and a
/// length is px. `font_size` is in px.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunStyle {
    pub len: usize,
    pub font_size: Option<f32>,
    pub line_height: Option<DefiniteLength>,
    pub color: Option<Hsla>,
    pub weight: Option<FontWeight>,
    pub font_style: Option<gpui::FontStyle>,
    pub family: Option<SharedString>,
    pub features: Option<gpui::FontFeatures>,
    /// The decoration lines of a run keep the colour, style and thickness of the
    /// box that declared them, so a descendant that changes `color` does not
    /// recolour them. The innermost box's line of a kind is the field, the lines
    /// of the same kind from the boxes around it are `extra_decorations`: as in
    /// CSS, nested declarations accumulate.
    pub underline: Option<UnderlineStyle>,
    pub strikethrough: Option<StrikethroughStyle>,
    pub overline: Option<OverlineStyle>,
    pub extra_decorations: Vec<ExtraDecoration>,
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

    fn add_decoration(
        &mut self,
        decoration: &text_decoration::TextDecoration,
        color: Hsla,
        lengths: &LengthContext,
    ) {
        if decoration.underline {
            if let Some(outer) = self.underline.replace(decoration.underline_style(color, lengths)) {
                self.extra_decorations.push(ExtraDecoration::Underline(outer));
            }
        }
        if decoration.line_through {
            if let Some(outer) = self
                .strikethrough
                .replace(decoration.strikethrough_style(color, lengths))
            {
                self.extra_decorations
                    .push(ExtraDecoration::Strikethrough(outer));
            }
        }
        if decoration.overline {
            if let Some(outer) = self.overline.replace(decoration.overline_style(color, lengths)) {
                self.extra_decorations.push(ExtraDecoration::Overline(outer));
            }
        }
    }

    fn inherits_everything(&self) -> bool {
        self.same_style(&Self::default())
    }
}

/// The runs of `paragraph`, covering its text exactly. Empty when no box
/// restyles anything, so the caller can paint it as plain text.
///
/// `colors`, `lengths` and `font_state` are the container's, as its children
/// inherit them. `states` says which states apply to a box, so that its `hover`, `active`,
/// `focusWithin` and `focusVisible` styles are folded into the run it paints.
/// `motion` holds the boxes whose colours a transition or an animation moves.
pub fn run_styles(
    tree: &RetainedTree,
    paragraph: &Paragraph,
    colors: &ColorContext,
    lengths: &LengthContext,
    font_state: &FontFeatureState,
    states: &dyn Fn(u64, &StyleDesc) -> PointerStates,
    motion: &InlineStyles,
) -> Vec<RunStyle> {
    let mut runs: Vec<RunStyle> = Vec::with_capacity(paragraph.pieces.len());
    for piece in &paragraph.pieces {
        let mut style = RunStyle {
            len: piece.range.len(),
            ..RunStyle::default()
        };
        let mut colors = *colors;
        let mut lengths = *lengths;
        let mut font_state = font_state.clone();
        for id in &piece.boxes {
            let Some(base) = tree
                .elements
                .get(id)
                .and_then(|element| element.style.as_deref())
            else {
                continue;
            };
            let declared = &*motion.declared(*id, base, states(*id, base));
            colors = colors.descend(declared);
            // A `display: contents` box has a font size too, and its children
            // inherit it.
            lengths = lengths.descend(declared.font_size.as_ref(), false);
            // It has no box, so what it declares reaches the text only as far as
            // it is inherited.
            let has_box = declared.display.as_deref() != Some("contents");
            if declared.font_size.is_some() {
                style.font_size = Some(lengths.font_size as f32);
            }
            if let Some(line_height) = declared.line_height.as_ref() {
                style.line_height = Some(line_height.text_style_value(&lengths));
            }
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
                    style.add_decoration(&decoration, decoration.resolve_color(&colors), &lengths);
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

/// The `LengthContext` of every box of `paragraph`: the container's, descended
/// along the chain of boxes around the box, so that a span's `em` is its own
/// font size. A box without a style has the lengths of the box around it. A box is declared the same way around every piece of text it
/// holds, so the first piece that holds it decides.
pub fn box_lengths(
    tree: &RetainedTree,
    paragraph: &Paragraph,
    lengths: &LengthContext,
    states: &dyn Fn(u64, &StyleDesc) -> PointerStates,
) -> HashMap<u64, LengthContext> {
    let mut of_box = HashMap::with_capacity(paragraph.boxes.len());
    for piece in &paragraph.pieces {
        let mut lengths = *lengths;
        for id in &piece.boxes {
            let Some(base) = tree
                .elements
                .get(id)
                .and_then(|element| element.style.as_deref())
            else {
                of_box.entry(*id).or_insert(lengths);
                continue;
            };
            lengths = lengths.descend(folded(base, states(*id, base)).font_size.as_ref(), false);
            of_box.entry(*id).or_insert(lengths);
        }
    }
    of_box
}

/// The line height a run contributes to its line box, or `None` when it takes
/// the line's. A run with its own size has one even when no box declared a line
/// height, because the line height it inherits is a multiple of that size.
fn run_line_height(
    style: &RunStyle,
    run_font_size: Option<Pixels>,
    base: &gpui::TextStyle,
    rem_size: Pixels,
) -> Option<Pixels> {
    if style.line_height.is_none() && run_font_size.is_none() {
        return None;
    }
    let font_size = run_font_size.unwrap_or_else(|| base.font_size.to_pixels(rem_size));
    Some(
        style
            .line_height
            .unwrap_or(base.line_height)
            .to_pixels(font_size.into(), rem_size),
    )
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

    /// `snap` rounds a height to the device pixel as `TextLayout::layout` does
    /// the line's.
    fn runs(
        &self,
        base: &gpui::TextStyle,
        rem_size: Pixels,
        snap: impl Fn(Pixels) -> Pixels,
    ) -> Vec<TextRun> {
        let base_font_size = base.font_size.to_pixels(rem_size);
        self.styles
            .iter()
            .map(|style| {
                let mut run = base.to_run(style.len);
                run.font_size = style
                    .font_size
                    .map(px)
                    .filter(|size| (f32::from(*size) - f32::from(base_font_size)).abs() > 1e-3);
                run.line_height =
                    run_line_height(style, run.font_size, base, rem_size).map(&snap);
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
                // The lines the text inherits from the boxes around the paragraph
                // stay under the ones the run's own boxes declare.
                if let Some(underline) = style.underline {
                    if let Some(outer) = run.underline.replace(underline) {
                        run.extra_decorations.push(ExtraDecoration::Underline(outer));
                    }
                }
                if let Some(strikethrough) = style.strikethrough {
                    if let Some(outer) = run.strikethrough.replace(strikethrough) {
                        run.extra_decorations
                            .push(ExtraDecoration::Strikethrough(outer));
                    }
                }
                if let Some(overline) = style.overline {
                    if let Some(outer) = run.overline.replace(overline) {
                        run.extra_decorations.push(ExtraDecoration::Overline(outer));
                    }
                }
                run.extra_decorations
                    .extend(style.extra_decorations.iter().copied());
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
        let rem_size = window.rem_size();
        let runs = self.runs(&window.text_style(), rem_size, |height| {
            window.pixel_snap(height)
        });
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
    use gpui::TextDecorationStyle;
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
            &container_lengths(),
            &FontFeatureState::default(),
            states,
            &InlineStyles::default(),
        )
    }

    /// A container with `fontSize: 10`, which is the root's too.
    fn container_lengths() -> LengthContext {
        LengthContext::initial(800.0, 600.0)
            .descend(Some(&crate::css::length::LinearLength::px(10.0)), true)
    }

    fn sizes(tree: &RetainedTree) -> Vec<Option<f32>> {
        runs_of(tree).iter().map(|run| run.font_size).collect()
    }

    /// The runs gpui gets for `tree`'s paragraph in a container of 10px text
    /// whose line height is `container_line_height`.
    fn text_runs(tree: &RetainedTree, container_line_height: DefiniteLength) -> Vec<TextRun> {
        let plan = plan_children(tree, &tree.elements[&1]);
        let Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        let base = gpui::TextStyle {
            font_size: px(10.0).into(),
            line_height: container_line_height,
            ..Default::default()
        };
        let styles = run_styles(
            tree,
            paragraph,
            &ColorContext::root(Scheme::Light),
            &container_lengths(),
            &FontFeatureState::default(),
            &|_, _| PointerStates::default(),
            &InlineStyles::default(),
        );
        InlineText::new(paragraph.text.clone().into(), styles).runs(&base, px(16.0), |height| height)
    }

    fn line_heights(runs: &[TextRun]) -> Vec<Option<f32>> {
        runs.iter()
            .map(|run| run.line_height.map(f32::from))
            .collect()
    }

    #[test]
    fn a_span_font_size_is_its_runs_size_and_a_nested_span_inherits_it() {
        let tree = tree_with(&[(3, serde_json::json!({"fontSize": 20}))]);
        assert_eq!(sizes(&tree), vec![None, Some(20.0), None]);
    }

    #[test]
    fn a_nested_font_size_in_em_is_relative_to_the_span_around_it() {
        let tree = tree_with(&[
            (3, serde_json::json!({"fontSize": 20})),
            (5, serde_json::json!({"fontSize": "1.5em"})),
        ]);
        assert_eq!(sizes(&tree), vec![None, Some(20.0), Some(30.0), None]);
    }

    #[test]
    fn a_font_size_in_rem_is_the_roots() {
        let tree = tree_with(&[(3, serde_json::json!({"fontSize": "3rem"}))]);
        assert_eq!(sizes(&tree), vec![None, Some(30.0), None]);
    }

    #[test]
    fn unsized_text_has_no_font_size() {
        let tree = tree_with(&[(3, serde_json::json!({"fontWeight": 700}))]);
        assert_eq!(sizes(&tree), vec![None; 3]);
    }

    #[test]
    fn a_display_contents_box_passes_its_font_size_to_its_children() {
        let tree = tree_with(&[
            (3, serde_json::json!({"fontWeight": 700})),
            (5, serde_json::json!({"display": "contents", "fontSize": 20})),
        ]);
        assert_eq!(sizes(&tree), vec![None, None, Some(20.0), None]);
        let nested = tree_with(&[
            (3, serde_json::json!({"display": "contents", "fontSize": 20})),
            (5, serde_json::json!({"fontSize": "2em"})),
        ]);
        assert_eq!(sizes(&nested), vec![None, Some(20.0), Some(40.0), None]);
    }

    #[test]
    fn a_line_height_number_is_a_multiple_of_the_runs_own_size() {
        let tree = tree_with(&[(3, serde_json::json!({"fontSize": 20}))]);
        let runs = text_runs(&tree, gpui::relative(1.5));
        assert_eq!(line_heights(&runs), vec![None, Some(30.0), None]);
        assert_eq!(runs[1].font_size, Some(px(20.0)));
        assert_eq!(runs[0].font_size, None);
    }

    #[test]
    fn a_line_height_length_of_the_container_is_inherited_as_px() {
        let tree = tree_with(&[(3, serde_json::json!({"fontSize": 20}))]);
        let runs = text_runs(&tree, px(24.0).into());
        assert_eq!(line_heights(&runs), vec![None, Some(24.0), None]);
    }

    #[test]
    fn a_spans_own_line_height_is_resolved_where_it_is_declared() {
        let tree = tree_with(&[(3, serde_json::json!({"fontSize": 20, "lineHeight": 2}))]);
        let runs = text_runs(&tree, gpui::relative(1.5));
        assert_eq!(line_heights(&runs), vec![None, Some(40.0), None]);

        let tree = tree_with(&[(3, serde_json::json!({"fontSize": 20, "lineHeight": "1.5em"}))]);
        let runs = text_runs(&tree, gpui::relative(1.0));
        assert_eq!(line_heights(&runs), vec![None, Some(30.0), None]);

        let tree = tree_with(&[
            (3, serde_json::json!({"lineHeight": "1.5em"})),
            (5, serde_json::json!({"fontSize": 30})),
        ]);
        let runs = text_runs(&tree, gpui::relative(1.0));
        assert_eq!(
            line_heights(&runs),
            vec![None, Some(15.0), Some(15.0), None],
            "a length is px by the time the nested span sizes its text"
        );
    }

    #[test]
    fn a_nested_span_resizes_a_multiplier_it_inherits() {
        let tree = tree_with(&[
            (3, serde_json::json!({"lineHeight": 2})),
            (5, serde_json::json!({"fontSize": 30})),
        ]);
        let runs = text_runs(&tree, gpui::relative(1.0));
        assert_eq!(line_heights(&runs), vec![None, Some(20.0), Some(60.0), None]);
    }

    #[test]
    fn a_span_with_only_a_line_height_has_one_and_keeps_the_size() {
        let tree = tree_with(&[(3, serde_json::json!({"lineHeight": 3}))]);
        let runs = text_runs(&tree, gpui::relative(1.5));
        assert_eq!(line_heights(&runs), vec![None, Some(30.0), None]);
        assert_eq!(runs[1].font_size, None);
    }

    #[test]
    fn a_span_that_sets_nothing_about_size_takes_the_lines() {
        let tree = tree_with(&[(3, serde_json::json!({"fontWeight": 700}))]);
        let runs = text_runs(&tree, gpui::relative(1.5));
        assert_eq!(line_heights(&runs), vec![None; 3]);
        assert!(runs.iter().all(|run| run.font_size.is_none()));
    }

    #[test]
    fn a_font_size_equal_to_the_containers_is_no_size_of_its_own() {
        let tree = tree_with(&[(3, serde_json::json!({"fontSize": "1em", "fontWeight": 700}))]);
        let runs = text_runs(&tree, gpui::relative(1.5));
        assert!(runs.iter().all(|run| run.font_size.is_none()));
        assert_eq!(line_heights(&runs), vec![None; 3]);
    }

    #[test]
    fn boxes_get_the_lengths_of_their_own_font_size() {
        let tree = tree_with(&[
            (3, serde_json::json!({"fontSize": 20})),
            (5, serde_json::json!({"fontSize": "1.5em"})),
        ]);
        let plan = plan_children(&tree, &tree.elements[&1]);
        let Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        let lengths = box_lengths(&tree, paragraph, &container_lengths(), &|_, _| {
            PointerStates::default()
        });
        assert_eq!(lengths[&3].font_size, 20.0);
        assert_eq!(lengths[&5].font_size, 30.0);
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
            serde_json::json!({"backgroundColor": "#ffff00", "borderTopWidth": 2, "borderTopStyle": "solid", "paddingLeft": 4}),
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
        assert_eq!(runs[1].color, red);
        assert_eq!(runs[2].underline.expect("inherited").color, red);
        assert_ne!(runs[2].color, red);
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
        assert_eq!(underline.style, TextDecorationStyle::Wavy);
        assert_ne!(nested.strikethrough.unwrap().color, underline.color);
        assert_eq!(
            nested.strikethrough.unwrap().style,
            TextDecorationStyle::Solid,
            "a nested declaration of one line brings its own style"
        );
    }

    #[test]
    fn one_declaration_gives_every_line_its_style_colour_and_thickness() {
        let tree = tree_with(&[(
            3,
            serde_json::json!({"textDecoration": "underline overline line-through dotted 3px #00ff00"}),
        )]);
        let run = runs_of(&tree).remove(1);
        let underline = run.underline.expect("underline");
        let strikethrough = run.strikethrough.expect("line-through");
        let overline = run.overline.expect("overline");
        assert_eq!(underline.style, TextDecorationStyle::Dotted);
        assert_eq!(strikethrough.style, TextDecorationStyle::Dotted);
        assert_eq!(overline.style, TextDecorationStyle::Dotted);
        assert_eq!(underline.thickness, gpui::px(3.0));
        assert_eq!(strikethrough.thickness, gpui::px(3.0));
        assert_eq!(overline.thickness, gpui::px(3.0));
        assert!(underline.color.is_some());
        assert_eq!(underline.color, strikethrough.color);
        assert_eq!(underline.color, overline.color);
        assert!(!underline.thickness_from_font);
    }

    #[test]
    fn a_thickness_in_em_is_the_declaring_spans_own_font_size() {
        let tree = tree_with(&[
            (3, serde_json::json!({"fontSize": 20, "textDecoration": "underline 0.1em"})),
            (5, serde_json::json!({"fontSize": 40})),
        ]);
        let runs = runs_of(&tree);
        assert_eq!(runs[1].underline.unwrap().thickness, gpui::px(2.0));
        assert_eq!(
            runs[2].underline.unwrap().thickness,
            gpui::px(2.0),
            "a descendant with a bigger font keeps the thickness resolved at the declaring span"
        );
    }

    #[test]
    fn a_thickness_in_percent_and_rem_in_a_span() {
        let tree = tree_with(&[
            (3, serde_json::json!({"fontSize": 30, "textDecorationLine": "underline overline", "textDecorationThickness": "10%"})),
            (5, serde_json::json!({"textDecoration": "line-through 0.5rem"})),
        ]);
        let runs = runs_of(&tree);
        assert_eq!(runs[1].underline.unwrap().thickness, gpui::px(3.0));
        assert_eq!(runs[1].overline.unwrap().thickness, gpui::px(3.0));
        assert_eq!(
            runs[2].strikethrough.unwrap().thickness,
            gpui::px(5.0),
            "the container's 10px is the root's"
        );
    }

    #[test]
    fn the_longhands_set_the_style_and_thickness_of_the_lines() {
        let tree = tree_with(&[(
            3,
            serde_json::json!({
                "textDecorationLine": "overline",
                "textDecorationStyle": "double",
                "textDecorationThickness": "from-font",
            }),
        )]);
        let run = runs_of(&tree).remove(1);
        let overline = run.overline.expect("overline");
        assert_eq!(overline.style, TextDecorationStyle::Double);
        assert!(overline.thickness_from_font);
        assert_eq!(overline.thickness, gpui::px(1.0), "the fallback of a font with no metric");
        assert_eq!(run.underline, None);
    }

    #[test]
    fn a_nested_box_that_declares_a_line_again_adds_it_to_the_outer_one() {
        let tree = tree_with(&[
            (3, serde_json::json!({"textDecoration": "underline wavy 2px #ff0000"})),
            (5, serde_json::json!({"textDecoration": "underline dashed #0000ff"})),
        ]);
        let runs = runs_of(&tree);
        let outer = runs[1].underline.expect("outer");
        assert!(runs[1].extra_decorations.is_empty());
        let inner = runs[2].underline.expect("inner");
        assert_eq!((inner.style, inner.thickness), (TextDecorationStyle::Dashed, gpui::px(1.0)));
        assert_ne!(inner.color, outer.color);
        assert_eq!(
            runs[2].extra_decorations,
            vec![ExtraDecoration::Underline(outer)],
            "the outer underline is still there, with its own style, colour and thickness"
        );
        assert_eq!((outer.style, outer.thickness), (TextDecorationStyle::Wavy, gpui::px(2.0)));
    }

    #[test]
    fn lines_of_each_kind_accumulate_on_their_own() {
        let tree = tree_with(&[
            (3, serde_json::json!({"textDecoration": "underline line-through"})),
            (5, serde_json::json!({"textDecoration": "line-through double"})),
        ]);
        let run = runs_of(&tree).remove(2);
        assert_eq!(run.strikethrough.expect("inner").style, TextDecorationStyle::Double);
        assert!(run.underline.is_some());
        assert!(matches!(run.extra_decorations[..], [ExtraDecoration::Strikethrough(_)]));
    }

    #[test]
    fn a_run_keeps_the_lines_it_inherits_beneath_its_own() {
        let outer = gpui::UnderlineStyle {
            thickness: gpui::px(3.0),
            ..Default::default()
        };
        let base = gpui::TextStyle {
            underline: Some(outer),
            ..Default::default()
        };
        let tree = tree_with(&[(3, serde_json::json!({"textDecoration": "underline dotted"}))]);
        let styles = runs_of(&tree);
        let runs = InlineText::new("a boldred end".into(), styles).runs(&base, px(16.0), |height| height);
        assert_eq!(runs[0].underline, Some(outer), "text outside the box keeps its underline");
        assert!(runs[0].extra_decorations.is_empty());
        let inner = runs[1].underline.expect("the box's underline");
        assert_eq!(inner.style, TextDecorationStyle::Dotted);
        assert_eq!(runs[1].extra_decorations, vec![ExtraDecoration::Underline(outer)]);
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
