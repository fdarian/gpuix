//! What an inline box paints around its text.
//!
//! A `<span>` has no GPUI element, so the paragraph that holds it paints what
//! CSS draws around a box, once for each line fragment of its text, as the
//! element's own painter ([`gpui::Style::paint`]) would:
//!
//! - **Behind the glyphs**: the `backgroundColor`, the border and the
//!   `borderRadius`, painted by [`paint_behind`] before the selection and search
//!   washes, so those lie over them. A box nested in another paints later, over it.
//! - **Over the glyphs**: the `outline`, which `focusVisible` rings a focused span
//!   with and `hover` / `active` can declare as well, painted by [`overlay`]. It
//!   rings the border box of each fragment and follows its corners.
//!
//! A box with an `opacity` below one, and what is inside it, paints in a group instead
//! ([`BoxDecoration::group`]): `text/paint.rs` paints the group's backgrounds, glyphs and rings
//! as one unit and fades that.
//!
//! Fragments are cut as `box-decoration-break: slice` does ([`quads`]): the box
//! is laid out as if it were one strip and then cut at the line breaks, so only
//! the first fragment has the left border and the left corners, only the last has
//! the right border and the right corners, and the top and the bottom border run
//! along every fragment. The border box of a fragment is [`border_box_rows`]: the
//! content area of the line, not the line box.
//!
//! Both are cut by nothing but the content mask the paragraph paints in.

use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    canvas, prelude::*, px, AbsoluteLength, App, BorderStyle, Bounds, CornerRadius, Corners, Fill, Hsla,
    Outline, Pixels, Refineable, Style, StyleRefinement, TextLayout, Window,
};

use crate::color::ColorContext;
use crate::css::border;
use crate::css::length::LengthContext;
use crate::events::state_styles::PointerStates;
use crate::motion::InlineStyles;
use crate::retained_tree::RetainedTree;
use crate::style::StyleDesc;

use super::inline::Paragraph;
use super::inline_bounds::border_box_rows;
use super::inline_insets::{border_widths, BoxEdges, InsetSpec};
use super::paragraph::box_opacity;

/// The decorations of one inline box.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxDecoration {
    pub range: Range<usize>,
    /// The [`OpacityGroup`](super::paragraph::OpacityGroup) the box paints in, `0` for none:
    /// the box itself when its `opacity` is below one, else the innermost box around it
    /// whose is.
    pub group: u64,
    pub edges: BoxEdges,
    pub outline: Option<Outline>,
    pub paint: BoxPaint,
}

/// What a box paints behind its text, for the box as a whole: [`quads`] cuts it
/// into the fragments.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxPaint {
    pub background: Option<Hsla>,
    pub widths: gpui::Edges<Pixels>,
    pub colors: gpui::Edges<Hsla>,
    pub styles: gpui::Edges<BorderStyle>,
    pub radii: Corners<CornerRadius>,
}

impl BoxPaint {
    /// What `style` declares, with `colors` the context of the box itself, so that
    /// `currentColor` is its own `color`.
    ///
    /// The widths are the ones that took space along the line, and the colours and
    /// the line style are read the way a `div` does, so a border looks the same on
    /// both, with a line style of its own on each side.
    fn of(style: &StyleDesc, colors: &ColorContext, lengths: &LengthContext) -> Self {
        let widths = border_widths(style, colors, lengths);
        let (border_colors, border_styles) = border_look(style, colors);
        Self {
            background: style
                .background_color
                .as_deref()
                .and_then(|value| colors.resolve(value))
                .map(Hsla::from)
                .filter(|color| !color.is_transparent()),
            widths: gpui::Edges {
                top: px(widths.top),
                right: px(widths.right),
                bottom: px(widths.bottom),
                left: px(widths.left),
            },
            colors: border_colors,
            styles: border_styles,
            radii: style.resolved_corner_radii(lengths),
        }
    }

    fn border_is_visible(&self) -> bool {
        let sides = [
            (self.widths.top, self.colors.top),
            (self.widths.right, self.colors.right),
            (self.widths.bottom, self.colors.bottom),
            (self.widths.left, self.colors.left),
        ];
        sides
            .iter()
            .any(|(width, color)| *width > Pixels::ZERO && !color.is_transparent())
    }

    /// Whether it paints anything behind the text.
    pub fn paints(&self) -> bool {
        self.background.is_some() || self.border_is_visible()
    }
}

/// A `Styled` that only keeps what is applied to it, so that the border
/// declarations can be read with the code that applies them to an element.
struct Probe(StyleRefinement);

impl gpui::Styled for Probe {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.0
    }
}

/// The side colours and the line styles `style` gives a border, in the order the
/// element builder applies them.
fn border_look(style: &StyleDesc, colors: &ColorContext) -> (gpui::Edges<Hsla>, gpui::Edges<BorderStyle>) {
    let mut probe = Probe(StyleRefinement::default());
    probe = border::apply_colors(probe, style, colors);
    probe = border::apply_styles(probe, style, colors);
    let mut resolved = Style::default();
    resolved.refine(&probe.0);
    (resolved.border_colors, resolved.border_styles)
}

/// One quad of a box: its border box in a line, with the background, border and
/// corners that fragment has.
#[derive(Clone, Debug, PartialEq)]
pub struct QuadSpec {
    pub bounds: Bounds<Pixels>,
    pub background: Option<Hsla>,
    pub widths: gpui::Edges<Pixels>,
    pub colors: gpui::Edges<Hsla>,
    pub styles: gpui::Edges<BorderStyle>,
    pub radii: Corners<CornerRadius>,
}

impl QuadSpec {
    /// The style of an element with this background and border.
    fn behind(&self) -> Style {
        Style {
            background: self.background.map(Fill::from),
            border_widths: self.widths.map(|width| AbsoluteLength::Pixels(*width)),
            border_colors: self.colors,
            border_styles: self.styles,
            corner_radii: self.radii,
            ..Style::default()
        }
    }

    /// The style of an element with this shape and nothing but an `outline`.
    fn ring(&self, outline: Outline) -> Style {
        Style {
            corner_radii: self.radii,
            outline: Some(outline),
            ..Style::default()
        }
    }
}

/// The quad of each fragment of a box, which `rows` are the border boxes of, cut
/// as `box-decoration-break: slice` does: the start of the box is its first
/// fragment's left side and the end of the box is its last fragment's right side.
/// A box in one piece keeps all four sides, and a fragment's open side has
/// neither border nor rounded corners.
pub fn quads(rows: &[Bounds<Pixels>], paint: &BoxPaint) -> Vec<QuadSpec> {
    let last = rows.len().saturating_sub(1);
    rows.iter()
        .enumerate()
        .map(|(index, bounds)| {
            let mut widths = paint.widths;
            let mut radii = paint.radii;
            if index > 0 {
                widths.left = Pixels::ZERO;
                radii.top_left = CornerRadius::default();
                radii.bottom_left = CornerRadius::default();
            }
            if index < last {
                widths.right = Pixels::ZERO;
                radii.top_right = CornerRadius::default();
                radii.bottom_right = CornerRadius::default();
            }
            QuadSpec {
                bounds: *bounds,
                background: paint.background,
                widths,
                colors: paint.colors,
                styles: paint.styles,
                radii,
            }
        })
        .collect()
}

/// The decorations of the boxes of `paragraph` that have any, outermost first,
/// so that a box nested in another paints over it.
///
/// `colors` and `lengths` are the container's, as its children inherit them, and
/// `states` says which states apply to a box, and `motion` holds the boxes whose
/// colours a transition or an animation moves, as they do for
/// [`super::paragraph::run_styles`].
/// A `display: contents` box has no box to decorate.
pub fn of(
    tree: &RetainedTree,
    paragraph: &Paragraph,
    colors: &ColorContext,
    lengths: &LengthContext,
    states: &dyn Fn(u64, &StyleDesc) -> PointerStates,
    motion: &InlineStyles,
    insets: &Rc<InsetSpec>,
) -> Vec<BoxDecoration> {
    // A box is declared the same way around every piece of text it holds, so the
    // first one decides.
    let mut looks: HashMap<u64, Option<(Option<Outline>, BoxPaint, u64)>> = HashMap::new();
    for piece in &paragraph.pieces {
        let mut colors = *colors;
        let mut lengths = *lengths;
        let mut group = 0;
        for id in &piece.boxes {
            let Some(base) = motion.declared_by(tree, *id) else {
                continue;
            };
            let declared = motion.declared(*id, base, states(*id, base));
            colors = colors.descend(&declared);
            lengths = lengths.descend(declared.font_size.as_ref(), false);
            if declared.display.as_deref() == Some("contents") {
                continue;
            }
            if box_opacity(&declared) < 1.0 {
                group = *id;
            }
            if looks.contains_key(id) {
                continue;
            }
            let outline = border::outline(&declared, &colors, &lengths).filter(is_visible);
            let paint = BoxPaint::of(&declared, &colors, &lengths);
            looks.insert(*id, (outline.is_some() || paint.paints()).then_some((outline, paint, group)));
        }
    }
    paragraph
        .boxes
        .iter()
        .filter_map(|(id, range)| {
            let (outline, paint, group) = looks.get(id)?.clone()?;
            Some(BoxDecoration {
                range: range.clone(),
                group,
                edges: insets.edges_of(*id),
                outline,
                paint,
            })
        })
        .collect()
}

fn is_visible(outline: &Outline) -> bool {
    f32::from(outline.width) > 0.0 && !outline.color.is_transparent()
}

/// Paints the background and border of `decorations`, boxes that have some
/// ([`BoxPaint::paints`]), behind the glyphs of `layout`, which has its bounds
/// once the glyphs have prepainted. Call it from paint, before anything that must
/// lie over the boxes.
pub fn paint_behind(
    layout: &TextLayout,
    decorations: &[BoxDecoration],
    window: &mut Window,
    cx: &mut App,
) {
    for decoration in decorations {
        let rows = border_box_rows(layout, &decoration.range, &decoration.edges);
        for quad in quads(&rows, &decoration.paint) {
            // The element's own painter, so the quad has the geometry and
            // colour handling of an element's background and border.
            quad.behind().paint(quad.bounds, window, cx, |_, _| {});
        }
    }
}

/// Paints the rings of `decorations` over `layout`, which has its bounds once the glyphs have
/// prepainted.
pub fn paint_rings(
    layout: &TextLayout,
    decorations: &[BoxDecoration],
    window: &mut Window,
    cx: &mut App,
) {
    for decoration in decorations {
        let Some(outline) = decoration.outline else {
            continue;
        };
        let rows = border_box_rows(layout, &decoration.range, &decoration.edges);
        for quad in quads(&rows, &decoration.paint) {
            // The element's own painter, so the ring has the geometry and
            // colour handling of an element's outline.
            quad.ring(outline).paint(quad.bounds, window, cx, |_, _| {});
        }
    }
}

/// The rings painted over `layout`, which has its bounds once the glyphs have
/// prepainted. It must come after the glyphs among its siblings, so that it
/// paints above them.
pub fn overlay(layout: TextLayout, decorations: Vec<BoxDecoration>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, cx| paint_rings(&layout, &decorations, window, cx),
    )
    .keeps_no_position()
    .absolute()
    .size_full()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Scheme;
    use crate::text::inline::{plan_children, Child};
    use std::sync::Arc;

    /// `div(1) > [text "a ", span(3) > ["b", span(5) > "c"], text " d"]`.
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

    fn decorations(
        tree: &RetainedTree,
        states: &dyn Fn(u64, &StyleDesc) -> PointerStates,
    ) -> Vec<BoxDecoration> {
        let plan = plan_children(tree, &tree.elements[&1]);
        let Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        of(
            tree,
            paragraph,
            &ColorContext::root(Scheme::Light),
            &LengthContext::initial(800.0, 600.0),
            states,
            &InlineStyles::default(),
            &InsetSpec::of(tree, paragraph, &ColorContext::root(Scheme::Light), &LengthContext::initial(800.0, 600.0), states),
        )
    }

    fn resting(tree: &RetainedTree) -> Vec<BoxDecoration> {
        decorations(tree, &|_, _| PointerStates::default())
    }

    fn ring(color: &str) -> serde_json::Value {
        serde_json::json!({"outlineWidth": 2, "outlineStyle": "solid", "outlineColor": color, "outlineOffset": 1})
    }

    #[test]
    fn a_border_in_em_is_the_font_size_of_its_span() {
        let border = |width: &str| {
            serde_json::json!({"borderWidth": width, "borderColor": "#ff0000", "borderStyle": "solid"})
        };
        let mut sized = border("1em");
        sized["fontSize"] = serde_json::json!(20);
        let found = resting(&fixture(&[(3, sized), (5, border("0.5em"))]));
        assert_eq!(found.len(), 2);
        assert_eq!(f32::from(found[0].paint.widths.left), 20.0);
        assert_eq!(
            f32::from(found[1].paint.widths.left),
            10.0,
            "the nested span inherits the 20px of its parent"
        );

        let found = resting(&fixture(&[(3, border("1em"))]));
        assert_eq!(f32::from(found[0].paint.widths.left), 16.0, "else the container's");
    }

    #[test]
    fn a_box_with_an_opacity_paints_in_its_own_group_unfaded_and_so_do_the_boxes_in_it() {
        let look = |opacity: f64| {
            serde_json::json!({
                "opacity": opacity, "backgroundColor": "#ff0000", "borderWidth": 2,
                "borderColor": "#0000ff", "borderStyle": "solid",
                "outlineWidth": 1, "outlineStyle": "solid", "outlineColor": "#00ff00"
            })
        };
        let found = resting(&fixture(&[(3, look(0.5)), (5, look(0.5))]));
        let alphas = |decoration: &BoxDecoration| {
            (
                decoration.paint.background.expect("a background").a,
                decoration.paint.colors.left.a,
                decoration.outline.expect("an outline").color.a,
            )
        };
        assert_eq!(alphas(&found[0]), (1.0, 1.0, 1.0), "the group is faded, not its colours");
        assert_eq!((found[0].group, found[1].group), (3, 5));
        let unfaded = resting(&fixture(&[(3, look(0.5)), (5, look(1.0))]));
        assert_eq!(unfaded[1].group, 3, "a box without an opacity is in the group around it");
    }

    #[test]
    fn a_display_contents_box_has_no_opacity_to_give() {
        let found = resting(&fixture(&[
            (3, serde_json::json!({"opacity": 0.5, "display": "contents"})),
            (5, serde_json::json!({"backgroundColor": "#ff0000"})),
        ]));
        assert_eq!(found[0].paint.background.expect("a background").a, 1.0);
    }

    #[test]
    fn a_paragraph_without_an_outline_has_no_decoration() {
        assert_eq!(resting(&fixture(&[])), vec![]);
        assert_eq!(resting(&fixture(&[(3, serde_json::json!({"color": "red"}))])), vec![]);
    }

    #[test]
    fn a_span_is_outlined_around_its_own_text_and_the_boxes_nested_in_it() {
        let found = resting(&fixture(&[(3, ring("#ff0000"))]));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].range, 2..4);
        assert_eq!(f32::from(found[0].outline.unwrap().width), 2.0);
        assert_eq!(f32::from(found[0].outline.unwrap().offset), 1.0);
    }

    #[test]
    fn a_nested_span_has_its_own_ring_and_does_not_inherit_the_parents() {
        let found = resting(&fixture(&[(3, ring("#ff0000")), (5, serde_json::json!({"color": "blue"}))]));
        assert_eq!(found.iter().map(|decoration| decoration.range.clone()).collect::<Vec<_>>(), vec![2..4]);

        let found = resting(&fixture(&[(5, ring("#0000ff"))]));
        assert_eq!(found.iter().map(|decoration| decoration.range.clone()).collect::<Vec<_>>(), vec![3..4]);
    }

    #[test]
    fn rings_come_outermost_first() {
        let found = resting(&fixture(&[(3, ring("#ff0000")), (5, ring("#0000ff"))]));
        assert_eq!(found.iter().map(|decoration| decoration.range.clone()).collect::<Vec<_>>(), vec![2..4, 3..4]);
        assert_ne!(found[0].outline.unwrap().color, found[1].outline.unwrap().color);
    }

    #[test]
    fn a_ring_that_paints_nothing_is_dropped() {
        for style in [
            serde_json::json!({"outlineWidth": 2, "outlineColor": "red"}),
            serde_json::json!({"outlineWidth": 2, "outlineStyle": "none", "outlineColor": "red"}),
            serde_json::json!({"outlineWidth": 0, "outlineStyle": "solid", "outlineColor": "red"}),
            serde_json::json!({"outlineWidth": 2, "outlineStyle": "solid", "outlineColor": "transparent"}),
        ] {
            assert_eq!(resting(&fixture(&[(3, style)])), vec![]);
        }
    }

    #[test]
    fn current_color_is_the_colour_of_the_box_itself() {
        let tree = fixture(&[(
            3,
            serde_json::json!({"color": "#00ff00", "outlineWidth": 2, "outlineStyle": "solid", "outlineColor": "currentColor"}),
        )]);
        let found = resting(&tree);
        assert_eq!(found[0].outline.unwrap().color, gpui::Rgba { r: 0.0, g: 1.0, b: 0.0, a: 1.0 }.into());
    }

    #[test]
    fn a_display_contents_box_has_no_box_to_ring() {
        let mut style = ring("#ff0000");
        style["display"] = "contents".into();
        assert_eq!(resting(&fixture(&[(3, style)])), vec![]);
    }

    #[test]
    fn a_state_declares_its_ring_only_while_it_applies() {
        let tree = fixture(&[(
            3,
            serde_json::json!({
                "hover": ring("#0000ff"),
                "focusVisible": ring("#ff0000"),
                "focusWithin": {"outlineWidth": 6, "outlineStyle": "solid", "outlineColor": "#00ff00"}
            }),
        )]);
        assert_eq!(resting(&tree), vec![]);

        let hovered = decorations(&tree, &|_, _| PointerStates { hovered: true, ..Default::default() });
        assert_eq!(hovered.len(), 1);

        let focused = decorations(&tree, &|id, _| PointerStates {
            focus_visible: id == 3,
            focus_within: id == 3,
            ..Default::default()
        });
        assert_eq!(focused.len(), 1);
        assert_ne!(focused[0].outline.unwrap().color, hovered[0].outline.unwrap().color);
        assert_eq!(f32::from(focused[0].outline.unwrap().width), 2.0, "focusVisible beats focusWithin");

        let within = decorations(&tree, &|_, _| PointerStates { focus_within: true, ..Default::default() });
        assert_eq!(f32::from(within[0].outline.unwrap().width), 6.0);
    }

    #[test]
    fn a_ring_of_the_focused_span_is_not_the_parents() {
        let tree = fixture(&[
            (3, serde_json::json!({"focusVisible": ring("#ff0000")})),
            (5, serde_json::json!({"focusVisible": ring("#0000ff")})),
        ]);
        let found = decorations(&tree, &|id, _| PointerStates {
            focus_visible: id == 5,
            ..Default::default()
        });
        assert_eq!(found.iter().map(|decoration| decoration.range.clone()).collect::<Vec<_>>(), vec![3..4]);
    }

    #[test]
    fn a_ring_is_around_the_border_box_so_it_leaves_the_margin_out() {
        let tree = fixture(&[(3, {
            let mut style = ring("#ff0000");
            style["marginLeft"] = 4.into();
            style["marginRight"] = 6.into();
            style
        })]);
        let plan = plan_children(&tree, &tree.elements[&1]);
        let Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        let states = |_: u64, _: &StyleDesc| PointerStates::default();
        let insets = InsetSpec::of(&tree, paragraph, &ColorContext::root(Scheme::Light), &LengthContext::initial(800.0, 600.0), &states);
        let found = of(&tree, paragraph, &ColorContext::root(Scheme::Light), &LengthContext::initial(800.0, 600.0), &states, &InlineStyles::default(), &insets);
        assert_eq!(found.len(), 1);
        let edges = found[0].edges.for_width(None);
        assert_eq!((f32::from(edges.start), f32::from(edges.end)), (4.0, 6.0));
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(gpui::point(px(x), px(y)), gpui::size(px(width), px(height)))
    }

    fn only(tree: &RetainedTree) -> BoxDecoration {
        let found = resting(tree);
        assert_eq!(found.len(), 1, "{found:?}");
        found.into_iter().next().unwrap()
    }

    fn rgb(r: f32, g: f32, b: f32) -> Hsla {
        gpui::Rgba { r, g, b, a: 1.0 }.into()
    }

    #[test]
    fn a_background_alone_is_painted_behind_the_text_and_rings_nothing() {
        let found = only(&fixture(&[(3, serde_json::json!({"backgroundColor": "#ff0000"}))]));
        assert_eq!(found.range, 2..4);
        assert_eq!(found.outline, None);
        assert_eq!(found.paint.background, Some(rgb(1.0, 0.0, 0.0)));
        assert!(found.paint.paints());
        assert!(!found.paint.border_is_visible());
    }

    #[test]
    fn a_ring_alone_paints_nothing_behind_the_text() {
        let found = only(&fixture(&[(3, ring("#ff0000"))]));
        assert!(found.outline.is_some());
        assert!(!found.paint.paints());
    }

    #[test]
    fn a_background_that_is_clear_or_not_a_colour_is_dropped() {
        for value in ["transparent", "#00000000", "not a colour"] {
            let tree = fixture(&[(3, serde_json::json!({ "backgroundColor": value }))]);
            assert_eq!(resting(&tree), vec![], "{value}");
        }
    }

    #[test]
    fn a_background_is_resolved_against_the_box_itself() {
        let tree = fixture(&[
            (3, serde_json::json!({"color": "#ff0000"})),
            (5, serde_json::json!({"color": "#0000ff", "backgroundColor": "currentColor"})),
        ]);
        let found = resting(&tree);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].range, 3..4);
        assert_eq!(found[0].paint.background, Some(rgb(0.0, 0.0, 1.0)));
    }

    #[test]
    fn a_nested_box_paints_after_the_box_around_it() {
        let found = resting(&fixture(&[
            (3, serde_json::json!({"backgroundColor": "#ff0000"})),
            (5, serde_json::json!({"backgroundColor": "#0000ff"})),
        ]));
        assert_eq!(found.iter().map(|decoration| decoration.range.clone()).collect::<Vec<_>>(), vec![2..4, 3..4]);
        assert_ne!(found[0].paint.background, found[1].paint.background);
    }

    #[test]
    fn a_display_contents_box_paints_no_background() {
        let tree = fixture(&[(3, serde_json::json!({"display": "contents", "backgroundColor": "#ff0000"}))]);
        assert_eq!(resting(&tree), vec![]);
    }

    #[test]
    fn a_state_paints_its_background_only_while_it_applies() {
        let tree = fixture(&[(
            3,
            serde_json::json!({"backgroundColor": "#ff0000", "hover": {"backgroundColor": "#0000ff"}}),
        )]);
        assert_eq!(only(&tree).paint.background, Some(rgb(1.0, 0.0, 0.0)));
        let hovered = decorations(&tree, &|_, _| PointerStates { hovered: true, ..Default::default() });
        assert_eq!(hovered[0].paint.background, Some(rgb(0.0, 0.0, 1.0)));
    }

    #[test]
    fn a_border_has_the_widths_and_colours_a_div_gives_it() {
        let tree = fixture(&[(
            3,
            serde_json::json!({"borderWidth": 2, "borderLeftWidth": 5, "borderStyle": "solid", "borderColor": "#ff0000 #0000ff", "borderBottomColor": "#00ff00"}),
        )]);
        let paint = only(&tree).paint;
        assert_eq!(
            [paint.widths.top, paint.widths.right, paint.widths.bottom, paint.widths.left],
            [px(2.0), px(2.0), px(2.0), px(5.0)]
        );
        assert_eq!(paint.colors.top, rgb(1.0, 0.0, 0.0));
        assert_eq!(paint.colors.right, rgb(0.0, 0.0, 1.0));
        assert_eq!(paint.colors.bottom, rgb(0.0, 1.0, 0.0));
        assert_eq!(paint.colors.left, rgb(0.0, 0.0, 1.0));
        assert_eq!(paint.styles, gpui::Edges::all(BorderStyle::Solid));
    }

    #[test]
    fn a_border_shorthand_has_a_colour_that_defaults_to_the_text_colour() {
        let tree = fixture(&[(3, serde_json::json!({"color": "#00ff00", "border": "3px solid", "borderRight": "1px dashed #ff0000"}))]);
        let paint = only(&tree).paint;
        assert_eq!(paint.widths.left, px(3.0));
        assert_eq!(paint.widths.right, px(1.0));
        assert_eq!(paint.colors.left, rgb(0.0, 1.0, 0.0));
        assert_eq!(paint.colors.right, rgb(1.0, 0.0, 0.0));
        assert_eq!(
            [paint.styles.top, paint.styles.right, paint.styles.bottom, paint.styles.left],
            [BorderStyle::Solid, BorderStyle::Dashed, BorderStyle::Solid, BorderStyle::Solid],
            "each side has the line style it declares"
        );
    }

    #[test]
    fn border_style_picks_the_line() {
        let tree = fixture(&[(3, serde_json::json!({"border": "2px dashed red", "borderStyle": "solid"}))]);
        assert_eq!(only(&tree).paint.styles, gpui::Edges::all(BorderStyle::Solid));
        let tree = fixture(&[(3, serde_json::json!({"borderWidth": 2, "borderColor": "red", "borderStyle": "dashed"}))]);
        assert_eq!(only(&tree).paint.styles, gpui::Edges::all(BorderStyle::Dashed));
        let tree = fixture(&[(
            3,
            serde_json::json!({"borderWidth": 2, "borderColor": "red", "borderStyle": "dotted double", "borderLeftStyle": "groove"}),
        )]);
        let styles = only(&tree).paint.styles;
        assert_eq!(
            [styles.top, styles.right, styles.bottom, styles.left],
            [BorderStyle::Dotted, BorderStyle::Double, BorderStyle::Dotted, BorderStyle::Groove]
        );
    }

    #[test]
    fn a_width_without_a_visible_colour_paints_no_border() {
        for style in [
            serde_json::json!({"borderWidth": 2, "borderStyle": "solid"}),
            serde_json::json!({"borderWidth": 2, "borderStyle": "solid", "borderColor": "transparent"}),
            serde_json::json!({"borderWidth": 0, "borderStyle": "solid", "borderColor": "red"}),
            serde_json::json!({"border": "2px solid transparent"}),
        ] {
            assert_eq!(resting(&fixture(&[(3, style.clone())])), vec![], "{style}");
        }
    }

    #[test]
    fn a_border_with_no_style_paints_no_border() {
        for style in [
            serde_json::json!({"borderWidth": 2, "borderColor": "red"}),
            serde_json::json!({"border": "2px red"}),
            serde_json::json!({"borderWidth": 2, "borderColor": "red", "borderStyle": "none"}),
            serde_json::json!({"borderWidth": 2, "borderColor": "red", "borderTopStyle": "hidden"}),
        ] {
            assert_eq!(resting(&fixture(&[(3, style.clone())])), vec![], "{style}");
        }
    }

    #[test]
    fn a_border_takes_the_sides_that_have_a_style_and_medium_without_a_width() {
        let paint = only(&fixture(&[(
            3,
            serde_json::json!({"borderColor": "red", "borderBottomWidth": 2, "borderBottomStyle": "solid", "borderLeftStyle": "dashed"}),
        )]))
        .paint;
        assert_eq!(
            [paint.widths.top, paint.widths.right, paint.widths.bottom, paint.widths.left],
            [px(0.0), px(0.0), px(2.0), px(3.0)]
        );
    }

    #[test]
    fn a_radius_is_per_corner_and_a_corner_beats_the_shorthand() {
        let tree = fixture(&[(
            3,
            serde_json::json!({"backgroundColor": "red", "borderRadius": 6, "borderTopRightRadius": 0, "borderBottomLeftRadius": 12}),
        )]);
        let radii = only(&tree).paint.radii;
        assert_eq!(
            [radii.top_left, radii.top_right, radii.bottom_right, radii.bottom_left],
            [circle(6.0), circle(0.0), circle(6.0), circle(12.0)]
        );
    }

    #[test]
    fn a_radius_may_be_elliptical_and_in_the_units_of_the_element() {
        let tree = fixture(&[(
            3,
            serde_json::json!({"backgroundColor": "red", "borderRadius": "1em 50% / 2rem", "borderBottomLeftRadius": "10px 5%"}),
        )]);
        let radii = only(&tree).paint.radii;
        let corner = |x: gpui::DefiniteLength, y: gpui::DefiniteLength| CornerRadius { x, y };
        let length = |pixels: f32| gpui::DefiniteLength::Absolute(AbsoluteLength::Pixels(px(pixels)));
        let fraction = gpui::DefiniteLength::Fraction;
        // The fixture's inherited font size is the initial 16px.
        assert_eq!(radii.top_left, corner(length(16.0), length(32.0)));
        assert_eq!(radii.top_right, corner(fraction(0.5), length(32.0)));
        assert_eq!(radii.bottom_left, corner(length(10.0), fraction(0.05)));
    }

    #[test]
    fn a_negative_radius_is_rejected_with_the_style() {
        let error = serde_json::from_value::<StyleDesc>(serde_json::json!({"borderRadius": -4})).unwrap_err();
        assert!(error.to_string().contains("-4"), "{error}");
    }

    #[test]
    fn the_edges_of_a_painted_box_are_those_of_its_space() {
        let tree = fixture(&[(
            3,
            serde_json::json!({"backgroundColor": "red", "paddingLeft": 5, "marginLeft": 3, "paddingTop": 2}),
        )]);
        let plan = plan_children(&tree, &tree.elements[&1]);
        let Child::Paragraph(paragraph) = &plan[0] else {
            panic!("a paragraph")
        };
        let states = |_: u64, _: &StyleDesc| PointerStates::default();
        let insets = InsetSpec::of(&tree, paragraph, &ColorContext::root(Scheme::Light), &LengthContext::initial(800.0, 600.0), &states);
        let found = of(&tree, paragraph, &ColorContext::root(Scheme::Light), &LengthContext::initial(800.0, 600.0), &states, &InlineStyles::default(), &insets);
        let edges = found[0].edges.for_width(None);
        assert_eq!(edges.start, px(3.0));
        assert_eq!(edges.top, px(2.0));
    }

    fn circle(radius: f32) -> CornerRadius {
        CornerRadius::from(px(radius))
    }

    fn full_look() -> BoxPaint {
        BoxPaint {
            background: Some(rgb(1.0, 1.0, 0.0)),
            widths: gpui::Edges { top: px(1.0), right: px(2.0), bottom: px(3.0), left: px(4.0) },
            colors: gpui::Edges::all(rgb(0.0, 0.0, 0.0)),
            styles: gpui::Edges::all(BorderStyle::Solid),
            radii: Corners {
                top_left: circle(5.0),
                top_right: circle(6.0),
                bottom_right: circle(7.0),
                bottom_left: circle(8.0),
            },
        }
    }

    fn widths(quad: &QuadSpec) -> [f32; 4] {
        [quad.widths.top, quad.widths.right, quad.widths.bottom, quad.widths.left].map(f32::from)
    }

    fn radii(quad: &QuadSpec) -> [f32; 4] {
        [quad.radii.top_left, quad.radii.top_right, quad.radii.bottom_right, quad.radii.bottom_left]
            .map(|corner| f32::from(corner.x.to_pixels(AbsoluteLength::Pixels(px(0.0)), px(16.0))))
    }

    #[test]
    fn a_box_on_one_line_keeps_every_side() {
        let quads = quads(&[rect(10.0, 0.0, 50.0, 20.0)], &full_look());
        assert_eq!(quads.len(), 1);
        assert_eq!(quads[0].bounds, rect(10.0, 0.0, 50.0, 20.0));
        assert_eq!(quads[0].background, Some(rgb(1.0, 1.0, 0.0)));
        assert_eq!(widths(&quads[0]), [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(radii(&quads[0]), [5.0, 6.0, 7.0, 8.0]);
    }

    #[test]
    fn a_box_over_two_lines_is_open_where_it_breaks() {
        let quads = quads(&[rect(60.0, 0.0, 40.0, 20.0), rect(0.0, 20.0, 30.0, 20.0)], &full_look());
        assert_eq!(widths(&quads[0]), [1.0, 0.0, 3.0, 4.0], "the first keeps the left border");
        assert_eq!(radii(&quads[0]), [5.0, 0.0, 0.0, 8.0], "and the left corners");
        assert_eq!(widths(&quads[1]), [1.0, 2.0, 3.0, 0.0], "the last keeps the right border");
        assert_eq!(radii(&quads[1]), [0.0, 6.0, 7.0, 0.0], "and the right corners");
    }

    #[test]
    fn a_fragment_in_the_middle_has_only_the_top_and_the_bottom() {
        let quads = quads(
            &[rect(60.0, 0.0, 40.0, 20.0), rect(0.0, 20.0, 100.0, 20.0), rect(0.0, 40.0, 30.0, 20.0)],
            &full_look(),
        );
        assert_eq!(widths(&quads[1]), [1.0, 0.0, 3.0, 0.0]);
        assert_eq!(radii(&quads[1]), [0.0; 4]);
        assert_eq!(quads[1].bounds, rect(0.0, 20.0, 100.0, 20.0));
        assert!(quads.iter().all(|quad| quad.background == Some(rgb(1.0, 1.0, 0.0))));
    }

    #[test]
    fn a_box_with_no_fragment_has_no_quad() {
        assert_eq!(quads(&[], &full_look()), vec![]);
    }
}
