//! An inline box with nothing in it that still takes space along the line.
//!
//! CSS gives an empty `<span>` no line of its own, unless it has margin, border or padding on the
//! inline axis (a block-axis one alone is not enough, as in Blink's `line_breaker.cc`): then the line
//! it is on is as tall as the strut, and the box sits on it between the content before and after.
//! A box like that is planned as a placeholder, one U+FFFC with no advance and no ink ([`gpui::InlineContent::Empty`]),
//! so that the box has a glyph for its insets and its fragment to sit at, in the paragraph it is in.

use crate::color::{ColorContext, Scheme};
use crate::css::length::LengthContext;
use crate::retained_tree::RetainedElement;
use crate::style::StyleDesc;

/// Whether `element`, an inline box with no content, takes space along the line. It reads the
/// element's own style: a state style or a motion value does not plan a placeholder, see
/// "An empty `<span>` with inline padding, border or margin" in `docs/fork/deviations.md`.
pub fn takes_space(element: &RetainedElement) -> bool {
    element.style.as_deref().is_some_and(style_takes_space)
}

/// [`takes_space`] for a style.
pub fn style_takes_space(style: &StyleDesc) -> bool {
    if style.display.as_deref() == Some("contents") {
        return false;
    }
    if !mentions_inline_space(style) {
        return false;
    }
    let colors = ColorContext::root(Scheme::Light);
    let lengths = LengthContext::initial(1000.0, 1000.0);
    super::inline_insets::declares_inline_space(style, &colors, &lengths)
}

/// Whether `style` sets any property that can give a box space along the line. A style change
/// asks this for every element, and most styles set none.
fn mentions_inline_space(style: &StyleDesc) -> bool {
    style.padding.is_some()
        || style.padding_inline.is_some()
        || style.padding_left.is_some()
        || style.padding_right.is_some()
        || style.margin.is_some()
        || style.margin_inline.is_some()
        || style.margin_left.is_some()
        || style.margin_right.is_some()
        || style.border_width.is_some()
        || style.border_left_width.is_some()
        || style.border_right_width.is_some()
        || style.border.is_some()
        || style.border_inline.is_some()
        || style.border_left.is_some()
        || style.border_right.is_some()
        || style.border_style.is_some()
        || style.border_left_style.is_some()
        || style.border_right_style.is_some()
}
