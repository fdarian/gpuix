//! Atomic inlines (`display: inline-block`, `inline-flex`, `inline-grid`): what
//! gpuix adds to the box that [`gpui::AtomicInline`] puts on a line of text.
//!
//! The box is built as any element is (`build_element`), as a layout root that
//! the paragraph sizes: the paragraph gives it the margin that [`strip`] took off
//! its style, the percentage width, and the baseline that [`baseline`] reads off
//! the texts built inside it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{
    AnyElement, App, AtomicBaseline, Bounds, DefiniteLength, Edges, Element, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, TextLayout, Window,
};

use crate::css::length::LengthContext;
use crate::css::line_height::LineHeight;
use crate::retained_tree::RetainedTree;
use crate::style::{DimensionValue, StyleDesc};

/// What the paragraph lays out around the box instead of the box itself.
pub struct Extracted {
    pub margin: Edges<DefiniteLength>,
    /// The `width` of the box when it is a percentage of the width of the
    /// paragraph, which the box cannot resolve as the root of a layout of its own.
    pub width: Option<DefiniteLength>,
    /// How far `position: relative` moves the box from where the line puts it.
    pub offset: gpui::Point<Pixels>,
}

impl Extracted {
    pub fn none() -> Self {
        Self {
            margin: Edges::default(),
            width: None,
            offset: gpui::Point::default(),
        }
    }
}

/// The offset of a `position: relative` box: `left` before `right` and `top` before `bottom`. A
/// percentage is of a containing block the paragraph does not have, and counts as `auto`.
fn relative_offset(style: &StyleDesc, lengths: &LengthContext) -> gpui::Point<Pixels> {
    if style.position.as_deref() != Some("relative") {
        return gpui::Point::default();
    }
    let px = |value: &Option<DimensionValue>| {
        let linear = match value.as_ref()? {
            DimensionValue::Pixels(pixels) => return Some(*pixels as f32),
            DimensionValue::Calc(length) => length,
            DimensionValue::Percentage(_)
            | DimensionValue::Auto
            | DimensionValue::Intrinsic(_)
            | DimensionValue::AnchorSize(_) => return None,
        };
        lengths.resolve_definite(linear).map(|pixels| pixels as f32)
    };
    let x = px(&style.left).or_else(|| px(&style.right).map(|right| -right)).unwrap_or(0.);
    let y = px(&style.top).or_else(|| px(&style.bottom).map(|bottom| -bottom)).unwrap_or(0.);
    gpui::point(gpui::px(x), gpui::px(y))
}

/// `style` without what the paragraph handles for the box: its margin, which
/// the line puts around it, and a width that is a percentage. An `auto` margin
/// is zero, as it is for an inline-level box.
pub fn strip(style: &StyleDesc, lengths: &LengthContext) -> (StyleDesc, Extracted) {
    let side = |own: &Option<DimensionValue>| {
        own.as_ref()
            .or(style.margin.as_ref())
            .and_then(|value| crate::layout::style::definite(value, lengths))
            .unwrap_or_else(|| gpui::px(0.).into())
    };
    let margin = Edges {
        top: side(&style.margin_top),
        right: side(&style.margin_right),
        bottom: side(&style.margin_bottom),
        left: side(&style.margin_left),
    };
    let mut stripped = style.clone();
    stripped.atomic_inline = true;
    stripped.margin = None;
    stripped.margin_top = None;
    stripped.margin_right = None;
    stripped.margin_bottom = None;
    stripped.margin_left = None;
    let width = match &style.width {
        Some(DimensionValue::Percentage(fraction)) => Some(gpui::relative(*fraction as f32)),
        Some(DimensionValue::Calc(length)) if length.has_percent() => {
            crate::layout::style::definite(&DimensionValue::Calc(*length), lengths)
        }
        _ => None,
    };
    if width.is_some() {
        stripped.width = Some(DimensionValue::Auto);
    }
    (
        stripped,
        Extracted {
            margin,
            width,
            offset: relative_offset(style, lengths),
        },
    )
}

/// The `line-height` of the box in px, which a percentage `vertical-align` is of:
/// the one it declares or inherits from the boxes around it in the paragraph.
/// `None` is the line height of the paragraph. `lengths` is the box's own.
pub fn line_height(line_height: Option<&LineHeight>, lengths: &LengthContext) -> Option<Pixels> {
    line_height?.resolve_px(lengths).map(|px| gpui::px(px as f32))
}

struct Frame {
    texts: Vec<(u64, TextLayout)>,
    /// Open `out_of_flow` calls.
    out_of_flow: usize,
}

thread_local! {
    static FRAMES: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) };
}

/// Runs `build`, and returns the texts that were built inside it as in-flow
/// content of the box it builds, in document order, with the element that holds
/// each. A box nested in it has a frame of its own.
pub fn collecting<R>(build: impl FnOnce() -> R) -> (R, Vec<(u64, TextLayout)>) {
    FRAMES.with(|frames| {
        frames.borrow_mut().push(Frame {
            texts: Vec::new(),
            out_of_flow: 0,
        })
    });
    let built = build();
    let frame = FRAMES
        .with(|frames| frames.borrow_mut().pop())
        .expect("the frame pushed above");
    (built, frame.texts)
}

/// A text was built for `container`.
pub fn note(container: u64, layout: &TextLayout) {
    FRAMES.with(|frames| {
        if let Some(frame) = frames.borrow_mut().last_mut() {
            if frame.out_of_flow == 0 {
                frame.texts.push((container, layout.clone()));
            }
        }
    });
}

/// Runs `build` for content that is out of the flow of the box being collected:
/// an absolutely positioned descendant has no line box in it.
pub fn out_of_flow<R>(build: impl FnOnce() -> R) -> R {
    let counted = FRAMES.with(|frames| {
        frames.borrow_mut().last_mut().map(|frame| frame.out_of_flow += 1).is_some()
    });
    let built = build();
    if counted {
        FRAMES.with(|frames| {
            if let Some(frame) = frames.borrow_mut().last_mut() {
                frame.out_of_flow -= 1;
            }
        });
    }
    built
}

/// Where the first item of a flex or grid container is, for a baseline to be
/// read off it.
pub struct Probe {
    /// The container the probe is for.
    pub container: u64,
    state: Rc<ProbeState>,
}

#[derive(Default)]
struct ProbeState {
    /// The first in-flow item, `None` for the anonymous item of a run of text.
    item: Cell<Option<Item>>,
    layout_id: Cell<Option<LayoutId>>,
}

#[derive(Clone, Copy)]
enum Item {
    Element(u64),
    Text,
}

impl Probe {
    pub fn new(container: u64) -> Self {
        Self {
            container,
            state: Rc::default(),
        }
    }

    pub fn clone_handle(&self) -> Self {
        Self {
            container: self.container,
            state: self.state.clone(),
        }
    }

    /// The first item of the container is a run of text.
    pub fn first_is_text(&self) {
        if self.state.item.get().is_none() {
            self.state.item.set(Some(Item::Text));
        }
    }

    /// Whether the first item of the container is not known yet.
    pub fn is_open(&self) -> bool {
        self.state.item.get().is_none()
    }

    /// `child`, the first in-flow item of the container `item`, built into the
    /// element that notes its layout node.
    pub fn watch(&self, item: u64, child: AnyElement) -> AnyElement {
        self.state.item.set(Some(Item::Element(item)));
        Watched {
            state: self.state.clone(),
            child,
        }
        .into_any_element()
    }
}

struct Watched {
    state: Rc<ProbeState>,
    child: AnyElement,
}

impl IntoElement for Watched {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Watched {
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
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        // The child's own node, so that what is read off it is the child's.
        let layout_id = self.child.request_layout(window, cx);
        self.state.layout_id.set(Some(layout_id));
        (layout_id, ())
    }

    fn escapes_to(&self) -> Option<gpui::Escape> {
        self.child.escapes_to()
    }

    fn lift_z(&self) -> Option<i32> {
        self.child.lift_z()
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

/// Which line of the box the baseline is read from.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    /// An `inline-block`: the last line box of its in-flow content.
    LastLine,
    /// An `inline-flex` or `inline-grid`: the first baseline of the container.
    FirstItem,
    /// A box with no baseline of its own: an `inline-block` whose `overflow`
    /// is `hidden`, `scroll` or `auto`.
    BottomMarginEdge,
}

/// The kind of baseline a box with this style has.
pub fn kind(style: Option<&StyleDesc>) -> Kind {
    let Some(style) = style else { return Kind::LastLine };
    match style.display.as_deref() {
        Some("inline-flex" | "inline-grid") => Kind::FirstItem,
        _ => {
            // `clip` is not a scroll container, and Chrome keeps the baseline of the text in it.
            let visible = |own: &Option<String>| {
                matches!(own.as_deref().or(style.overflow.as_deref()), None | Some("visible" | "clip"))
            };
            if visible(&style.overflow_x) && visible(&style.overflow_y) {
                Kind::LastLine
            } else {
                Kind::BottomMarginEdge
            }
        }
    }
}

fn is_within(tree: &RetainedTree, element: u64, ancestor: u64) -> bool {
    let mut current = Some(element);
    while let Some(id) = current {
        if id == ancestor {
            return true;
        }
        current = tree.elements.get(&id).and_then(|element| element.parent);
    }
    false
}

/// Reads the baseline of a box off the `texts` built inside it. `probe` is
/// armed for a flex or grid box, see [`Probe`].
pub fn baseline(
    tree: &RetainedTree,
    kind: Kind,
    texts: &[(u64, TextLayout)],
    probe: Option<&Probe>,
) -> AtomicBaseline {
    let from_text = |layout: &TextLayout, last: bool| {
        let layout = layout.clone();
        AtomicBaseline::Measured(Box::new(move |window, root, _| {
            let top = window.layout_bounds(layout.layout_id()?).origin.y
                - window.layout_bounds(root).origin.y;
            Some(top + if last { layout.last_baseline()? } else { layout.first_baseline()? })
        }))
    };
    match kind {
        Kind::BottomMarginEdge => AtomicBaseline::BottomMarginEdge,
        Kind::LastLine => match texts.last() {
            Some((_, layout)) => from_text(layout, true),
            None => AtomicBaseline::BottomMarginEdge,
        },
        Kind::FirstItem => {
            let Some(probe) = probe else {
                return AtomicBaseline::BottomMarginEdge;
            };
            let first = texts.first();
            match probe.state.item.get() {
                None => AtomicBaseline::BottomMarginEdge,
                Some(Item::Text) => match first {
                    Some((_, layout)) => from_text(layout, false),
                    None => AtomicBaseline::BottomMarginEdge,
                },
                Some(Item::Element(item)) => match first {
                    Some((container, layout)) if is_within(tree, *container, item) => {
                        from_text(layout, false)
                    }
                    // An item with no line of text has its border box's bottom edge.
                    _ => {
                        let state = probe.state.clone();
                        AtomicBaseline::Measured(Box::new(move |window, root, _| {
                            let item = window.layout_bounds(state.layout_id.get()?);
                            Some(item.bottom() - window.layout_bounds(root).origin.y)
                        }))
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{px, relative};

    fn style(json: serde_json::Value) -> StyleDesc {
        serde_json::from_value(json).expect("a style")
    }

    fn lengths() -> LengthContext {
        LengthContext::initial(800.0, 600.0).descend(Some(&crate::css::length::LinearLength::px(10.0)), true)
    }

    #[test]
    fn the_margin_is_taken_off_the_style_and_auto_is_zero() {
        let (stripped, extracted) = strip(
            &style(serde_json::json!({"margin": 4, "marginLeft": "2em", "marginTop": "auto", "marginBottom": "10%", "width": 50})),
            &lengths(),
        );
        assert_eq!(extracted.margin.left, px(20.).into());
        assert_eq!(extracted.margin.right, px(4.).into());
        assert_eq!(extracted.margin.top, px(0.).into());
        assert_eq!(extracted.margin.bottom, relative(0.1));
        assert!(stripped.margin.is_none() && stripped.margin_left.is_none() && stripped.margin_bottom.is_none());
        assert!(stripped.atomic_inline, "the decorations of the boxes around it stop at it");
        assert_eq!(extracted.width, None, "a width in px stays on the box");
        assert_eq!(stripped.width, style(serde_json::json!({"width": 50})).width);
    }

    #[test]
    fn a_width_in_percent_goes_to_the_paragraph_and_leaves_the_box_auto() {
        let (stripped, extracted) = strip(&style(serde_json::json!({"width": "50%"})), &lengths());
        assert_eq!(extracted.width, Some(relative(0.5)));
        assert_eq!(stripped.width, Some(DimensionValue::Auto));

        let (stripped, extracted) = strip(&style(serde_json::json!({"width": "calc(100% - 20px)"})), &lengths());
        assert_eq!(extracted.width, Some(DefiniteLength::calc(px(-20.), 1.0)));
        assert_eq!(stripped.width, Some(DimensionValue::Auto));

        let (stripped, extracted) = strip(&style(serde_json::json!({"width": "calc(2em + 4px)"})), &lengths());
        assert_eq!(extracted.width, None, "no percent in it");
        assert_ne!(stripped.width, Some(DimensionValue::Auto));
    }

    #[test]
    fn a_relative_box_is_offset_by_left_before_right_and_top_before_bottom() {
        let offset = |json: serde_json::Value| strip(&style(json), &lengths()).1.offset;
        assert_eq!(offset(serde_json::json!({"position": "relative", "left": 7, "top": 5})), gpui::point(px(7.), px(5.)));
        assert_eq!(
            offset(serde_json::json!({"position": "relative", "left": 7, "right": 3, "bottom": 4})),
            gpui::point(px(7.), px(-4.))
        );
        assert_eq!(offset(serde_json::json!({"position": "relative", "right": "2em"})), gpui::point(px(-20.), px(0.)));
        assert_eq!(offset(serde_json::json!({"position": "relative", "left": "50%"})), gpui::Point::default());
        assert_eq!(offset(serde_json::json!({"left": 7, "top": 5})), gpui::Point::default(), "static");
    }

    #[test]
    fn the_baseline_comes_from_the_display_and_the_overflow() {
        assert_eq!(kind(Some(&style(serde_json::json!({"display": "inline-block"})))), Kind::LastLine);
        assert_eq!(kind(None), Kind::LastLine);
        assert_eq!(kind(Some(&style(serde_json::json!({"display": "inline-flex"})))), Kind::FirstItem);
        assert_eq!(kind(Some(&style(serde_json::json!({"display": "inline-grid"})))), Kind::FirstItem);
        for overflow in [
            serde_json::json!({"overflow": "hidden"}),
            serde_json::json!({"overflowY": "auto"}),
            serde_json::json!({"overflow": "visible", "overflowX": "scroll"}),
        ] {
            assert_eq!(kind(Some(&style(overflow))), Kind::BottomMarginEdge);
        }
        assert_eq!(kind(Some(&style(serde_json::json!({"overflow": "clip"})))), Kind::LastLine);
        assert_eq!(kind(Some(&style(serde_json::json!({"overflow": "visible"})))), Kind::LastLine);
    }

    #[test]
    fn a_line_height_is_the_boxs_own_and_normal_is_the_paragraphs() {
        let lengths = lengths();
        assert_eq!(line_height(Some(&LineHeight::Multiplier(2.0)), &lengths), Some(px(20.)));
        assert_eq!(line_height(Some(&LineHeight::Normal), &lengths), None);
        assert_eq!(line_height(None, &lengths), None);
    }

    #[test]
    fn texts_are_collected_for_the_box_being_built_and_not_the_ones_nested_or_out_of_flow() {
        // `note` needs a layout, so this checks the frames with none.
        let (_, texts) = collecting(|| {
            let (_, inner) = collecting(|| out_of_flow(|| ()));
            assert!(inner.is_empty());
        });
        assert!(texts.is_empty());
        // Outside any frame a note is dropped.
        note(1, &TextLayout::default());
        let (_, texts) = collecting(|| {
            out_of_flow(|| note(1, &TextLayout::default()));
            note(2, &TextLayout::default());
        });
        assert_eq!(texts.iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![2]);
    }
}
