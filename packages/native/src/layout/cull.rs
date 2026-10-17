//! Which subtrees gpui may skip when they lie outside their clip (`InteractiveElement::cullable`).
//!
//! Browsers keep style and layout for clipped content and skip paint. Here the build and layout
//! stay, and a subtree whose boxes (and the boxes of what overflows them) are outside the content
//! mask is not prepainted or painted: it only reports the bounds, sizes, scroll metrics and tab
//! stops that JS can read. gpui checks the geometry against the real layout. What it cannot know,
//! and this module says, is how far the subtree draws past its boxes, and whether anything in it
//! needs a prepaint and a paint of its own.

use crate::css::length::LengthContext;
use crate::style::StyleDesc;

/// What is built under an element so far, as the parent will see it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Subtree {
    /// How far past its boxes the subtree draws, or `None` once something in it cannot be culled.
    ink: Option<f32>,
}

impl Subtree {
    pub(crate) const fn empty() -> Self {
        Self { ink: Some(0.0) }
    }

    pub(crate) const fn blocked() -> Self {
        Self { ink: None }
    }

    /// A text of this font size: its glyphs may reach a line height past the box of a line that
    /// is shorter than they are.
    pub(crate) fn of_text(font_size: f32) -> Self {
        Self { ink: Some(font_size + 2.0) }
    }

    /// Something here may draw or be hit outside its box in a way this cannot bound, or needs its
    /// own prepaint and paint (a custom element, a virtual list), so it and what holds it stay.
    pub(crate) fn block(&mut self) {
        self.ink = None;
    }

    pub(crate) fn reach(&mut self, px: f32) {
        self.ink = self.ink.map(|ink| ink.max(px));
    }

    pub(crate) fn merge(&mut self, child: Subtree) {
        match child.ink {
            Some(ink) => self.reach(ink),
            None => self.block(),
        }
    }

    /// What gpui is told, if the subtree can be culled.
    pub(crate) fn outset(self) -> Option<gpui::Edges<gpui::Pixels>> {
        self.ink.map(|ink| gpui::Edges::all(gpui::px(ink)))
    }

    /// The subtree of an element with its own `style` over what was built below it: its own ink,
    /// and the ink of what is below it unless it clips both axes.
    pub(crate) fn of_element(
        below: Subtree,
        style: Option<&StyleDesc>,
        colors: &crate::color::ColorContext,
        lengths: &LengthContext,
    ) -> Subtree {
        let Some(style) = style else {
            return below;
        };
        let Some(own) = ink_outset(style, colors, lengths) else {
            return Subtree { ink: None };
        };
        let axes = crate::scroll::Axes::of(style);
        let mut subtree = if axes.x.clips() && axes.y.clips() {
            // What a clip holds is cut by it, but not what is built in it that needs more than that.
            Subtree { ink: below.ink.map(|_| 0.0) }
        } else {
            below
        };
        subtree.reach(own);
        subtree
    }
}

/// The state styles an element can have on top of its own, which paint differently while they
/// apply.
fn variants(style: &StyleDesc) -> impl Iterator<Item = &StyleDesc> {
    [
        &style.hover,
        &style.active,
        &style.focus_visible,
        &style.focus_within,
        &style.group_hover,
        &style.group_active,
        &style.group_focus_within,
        &style.has_hover,
        &style.has_active,
        &style.has_focus,
        &style.has_focus_visible,
    ]
    .into_iter()
    .filter_map(|variant| variant.as_deref())
}

/// How far the element draws past its border box in the style or any state of it, in px, or
/// `None` when it cannot be culled: it is placed by something else than its parent's box
/// (`absolute`, `fixed`, `sticky`). A style that has `@container` blocks is the one they
/// fold into, so the blocks that apply are in it already.
pub(crate) fn ink_outset(
    style: &StyleDesc,
    colors: &crate::color::ColorContext,
    lengths: &LengthContext,
) -> Option<f32> {
    let mut reach = 0.0f32;
    for style in std::iter::once(style).chain(variants(style)) {
        if matches!(style.position.as_deref(), Some("absolute" | "fixed" | "sticky")) {
            return None;
        }
        reach = reach.max(own_ink(style, colors, lengths));
    }
    Some(reach)
}

/// A gaussian of standard deviation `sigma` is gone by about three of them.
const SIGMAS: f32 = 3.0;

fn own_ink(style: &StyleDesc, colors: &crate::color::ColorContext, lengths: &LengthContext) -> f32 {
    let mut reach = 0.0f32;
    if let Some(shadows) = &style.box_shadow {
        for shadow in shadows.as_slice().iter().filter(|shadow| !shadow.inset) {
            let length = |length: &crate::css::length::LinearLength| lengths.resolve(length, 0.0) as f32;
            let offset = length(&shadow.offset_x).abs().max(length(&shadow.offset_y).abs());
            // The CSS blur radius is two standard deviations.
            let blur = length(&shadow.blur_radius).max(0.0) / 2.0 * SIGMAS;
            reach = reach.max(offset + length(&shadow.spread_radius).max(0.0) + blur);
        }
    }
    if let Some(outline) = crate::css::border::outline(style, colors, lengths) {
        reach = reach.max(f32::from(outline.width) + f32::from(outline.offset).max(0.0));
    }
    if let Some(filters) = style
        .filter
        .as_deref()
        .and_then(|value| crate::css::filter::parse(value, lengths))
    {
        for filter in filters {
            let filter_reach = match filter {
                crate::css::filter::Filter::Blur(sigma) => sigma as f32 * SIGMAS,
                crate::css::filter::Filter::DropShadow { offset_x, offset_y, blur, .. } => {
                    offset_x.abs().max(offset_y.abs()) as f32 + blur as f32 * SIGMAS
                }
                _ => 0.0,
            };
            reach = reach.max(filter_reach);
        }
    }
    reach
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(json: &str) -> StyleDesc {
        serde_json::from_str(json).expect("a style")
    }

    fn ink(json: &str) -> Option<f32> {
        ink_outset(
            &style(json),
            &crate::color::ColorContext::root(crate::color::Scheme::Light),
            &LengthContext::for_tests(),
        )
    }

    #[test]
    fn a_plain_box_draws_inside_itself() {
        assert_eq!(ink(r#"{"width": 20, "backgroundColor": "red"}"#), Some(0.0));
    }

    #[test]
    fn a_shadow_reaches_by_its_offset_spread_and_blur() {
        let reach = ink(r#"{"boxShadow": "0 4px 12px 2px rgba(0,0,0,.2)"}"#).unwrap();
        // Offset 4, spread 2, and a CSS blur of 12px is a deviation of 6 that is gone by 18.
        assert_eq!(reach, 4.0 + 2.0 + 18.0);
    }

    #[test]
    fn an_inset_shadow_stays_inside() {
        assert_eq!(ink(r#"{"boxShadow": "inset 0 0 10px red"}"#), Some(0.0));
    }

    #[test]
    fn an_outline_reaches_by_its_width_and_offset() {
        assert_eq!(ink(r#"{"outlineWidth": 2, "outlineStyle": "solid", "outlineOffset": 3, "outlineColor": "red"}"#), Some(5.0));
    }

    #[test]
    fn a_blur_and_a_drop_shadow_reach_by_their_deviation() {
        assert_eq!(ink(r#"{"filter": "blur(4px)"}"#), Some(12.0));
        assert_eq!(ink(r#"{"filter": "drop-shadow(2px 5px 1px red)"}"#), Some(5.0 + 3.0));
    }

    #[test]
    fn a_state_style_counts_for_the_reach() {
        let reach = ink(r#"{"focusVisible": {"outlineWidth": 2, "outlineStyle": "solid", "outlineOffset": 2, "outlineColor": "red"}, "hover": {"boxShadow": "0 0 8px red"}}"#);
        assert_eq!(reach, Some(12.0));
    }

    #[test]
    fn a_box_placed_by_something_else_than_its_parent_cannot_be_culled() {
        assert_eq!(ink(r#"{"position": "absolute"}"#), None);
        assert_eq!(ink(r#"{"position": "fixed"}"#), None);
        assert_eq!(ink(r#"{"position": "sticky"}"#), None);
        assert_eq!(ink(r#"{"hover": {"position": "absolute"}}"#), None);
        assert_eq!(ink(r#"{"position": "relative"}"#), Some(0.0));
    }

    #[test]
    fn a_clip_holds_what_is_below_it_but_not_its_own_shadow() {
        let colors = crate::color::ColorContext::root(crate::color::Scheme::Light);
        let lengths = LengthContext::for_tests();
        let mut below = Subtree::empty();
        below.reach(30.0);
        let clipping = style(r#"{"overflow": "hidden", "boxShadow": "0 0 4px red"}"#);
        assert_eq!(Subtree::of_element(below, Some(&clipping), &colors, &lengths).ink, Some(6.0));
        let open = style(r#"{"boxShadow": "0 0 4px red"}"#);
        assert_eq!(Subtree::of_element(below, Some(&open), &colors, &lengths).ink, Some(30.0));
    }

    #[test]
    fn something_that_cannot_be_culled_blocks_every_parent() {
        let mut parent = Subtree::empty();
        let mut child = Subtree::empty();
        child.block();
        parent.merge(child);
        assert_eq!(parent.outset(), None);
        let clipping = style(r#"{"overflow": "hidden"}"#);
        let colors = crate::color::ColorContext::root(crate::color::Scheme::Light);
        assert_eq!(
            Subtree::of_element(parent, Some(&clipping), &colors, &LengthContext::for_tests()).ink,
            None
        );
    }
}
