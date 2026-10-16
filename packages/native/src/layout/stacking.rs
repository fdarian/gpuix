//! Stacking contexts: which elements form one, which are lifted into the nearest, and how far
//! up they sort.
//!
//! CSS orders a positioned element in the nearest stacking context, whatever static boxes sit
//! between it and the context. zed does the same for a `Div` flagged `stacking_context()`
//! (`Element::lift_z`): the positioned descendants are painted and hit tested after the
//! in-flow ones by `(z-index, tree order)`, the negative ones above the context's own
//! background. Where no context is active (a layout root such as a virtual-list row, a deferred
//! layer, a `visibility: hidden` subtree) nothing is lifted and `paint_rank` orders the
//! children of one parent as before.
//!
//! These predicates are the only place that decides it. `build_element` flags the GPUI
//! elements with them and `RetainedTree::elements_at` orders the retained tree with them, so
//! the two cannot drift.

use gpui::{Div, Stateful, Styled};

use crate::color::ColorContext;
use crate::css::length::LengthContext;
use crate::retained_tree::{RetainedElement, RetainedTree};
use crate::style::StyleDesc;

/// An author-set `position` other than `static`. An element with none is in flow, whatever
/// zed's own default for `Style::position` is.
pub(crate) fn is_positioned(style: Option<&StyleDesc>) -> bool {
    matches!(
        style.and_then(|style| style.position.as_deref()),
        Some("relative" | "absolute" | "fixed" | "sticky")
    )
}

/// The `z-index` of a positioned element, `auto` being 0. `None` for an element in flow, which
/// is not lifted and has no index.
pub(crate) fn lift_z(style: Option<&StyleDesc>) -> Option<i32> {
    if !is_positioned(style) {
        return None;
    }
    Some(style.and_then(|style| style.z_index).map_or(0, round_z))
}

fn round_z(z: f64) -> i32 {
    z.round().clamp(-1_000_000.0, 1_000_000.0) as i32
}

/// Where a child paints among its siblings when no stacking context is active, lowest first.
/// Layout is not affected.
///
/// CSS paints negative `z-index` positioned boxes, then in-flow boxes, then positioned boxes by
/// `z-index` (`auto` is 0) in tree order. Only an author-set `position` other than `static`
/// counts as positioned.
pub(crate) fn paint_rank(style: Option<&StyleDesc>) -> i32 {
    match lift_z(style) {
        None => 0,
        Some(z) if z < 0 => z,
        Some(z) => z + 1,
    }
}

/// Whether `style` forms a stacking context of its own: a positioned element with a `z-index`
/// (not `auto`), a `fixed` or `sticky` one, and anything whose composite is applied as a whole,
/// which is what zed's `effects.rs` does with an effect group. zed cannot see the latter
/// (`element_opacity` is paint-only state), so every one must be flagged.
///
/// `colors` and `lengths` only decide whether a `filter` or `mask` value is valid, and a value
/// that does not parse makes no group. Not `transform`: gpuix has no such style.
pub(crate) fn creates_context(
    style: Option<&StyleDesc>,
    colors: &ColorContext,
    lengths: &LengthContext,
) -> bool {
    let Some(style) = style else {
        return false;
    };
    if is_positioned(Some(style)) {
        let fixed_or_sticky = matches!(style.position.as_deref(), Some("fixed" | "sticky"));
        if fixed_or_sticky || style.z_index.is_some() {
            return true;
        }
    }
    has_effect_group(style, colors, lengths)
}

/// The condition of zed's `Style::group_effect`: opacity below 1, a non-empty `filter` or
/// `backdrop-filter`, a mask layer, or a clip path other than `none`.
fn has_effect_group(style: &StyleDesc, colors: &ColorContext, lengths: &LengthContext) -> bool {
    let filters = |value: &Option<String>| {
        value
            .as_deref()
            .and_then(|value| crate::css::filter::parse_resolved(value, colors, lengths))
            .is_some_and(|filters| !filters.is_empty())
    };
    style.opacity.is_some_and(|opacity| opacity < 1.0)
        || filters(&style.filter)
        || filters(&style.backdrop_filter)
        || style
            .resolved_mask(colors, lengths)
            .is_some_and(|layers| !layers.is_empty())
        || style
            .clip_path
            .as_deref()
            .and_then(|value| crate::css::clip_path::parse(value, lengths))
            .is_some_and(|clip_path| clip_path.is_some())
}

/// Puts the style's `z-index` on a positioned element. zed reads it only from one that is
/// positioned, and every styled element carries it, a `virtual-list` or an `<img>` as well as a
/// `Div`.
pub(crate) fn apply_z_index<E: Styled>(mut el: E, style: &StyleDesc) -> E {
    if let Some(z) = lift_z(Some(style)) {
        el.style().z_index = Some(z);
    }
    el
}

/// Flags `el` a stacking context when the style says so, or when `forced`: the root of what is
/// built (the window root, a virtual-list row) and an `<anchored>` layer begin with no context
/// around them, so z-index inside them needs one of their own.
pub(crate) fn apply_context(
    el: Stateful<Div>,
    style: Option<&StyleDesc>,
    forced: bool,
    colors: &ColorContext,
    lengths: &LengthContext,
) -> Stateful<Div> {
    if forced || creates_context(style, colors, lengths) {
        el.stacking_context()
    } else {
        el
    }
}

/// The retained-tree counterpart of what zed lifts: the `z-index` of `element` if its parent
/// hands it to a stacking context. An `<anchored>` layer is drawn by its own deferred draw, a
/// virtual-list row is a layout root, and `display: contents` makes no box.
pub(crate) fn lifted_z(tree: &RetainedTree, element: &RetainedElement) -> Option<i32> {
    if element.element_type == "anchored"
        || super::contents::is_display_contents(element)
        || is_virtual_row(tree, element)
    {
        return None;
    }
    lift_z(element.style.as_deref())
}

/// Whether `element` is a context whatever its style says. The counterpart of the `forced`
/// flag of `apply_context`.
pub(crate) fn is_forced_context(tree: &RetainedTree, element: &RetainedElement) -> bool {
    tree.root_id == Some(element.id)
        || element.element_type == "anchored"
        || is_virtual_row(tree, element)
}

fn is_virtual_row(tree: &RetainedTree, element: &RetainedElement) -> bool {
    element
        .parent
        .and_then(|parent| tree.elements.get(&parent))
        .is_some_and(|parent| parent.element_type == "virtual-list")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::color::Scheme;

    fn style(position: Option<&str>, z_index: Option<f64>) -> StyleDesc {
        StyleDesc {
            position: position.map(str::to_string),
            z_index,
            ..Default::default()
        }
    }

    fn context_of(style: &StyleDesc) -> bool {
        creates_context(
            Some(style),
            &ColorContext::root(Scheme::Light),
            &LengthContext::for_tests(),
        )
    }

    #[test]
    fn in_flow_children_paint_below_positioned_ones() {
        assert_eq!(paint_rank(None), 0);
        assert_eq!(paint_rank(Some(&style(None, Some(5.0)))), 0);
        assert_eq!(paint_rank(Some(&style(Some("static"), None))), 0);
        assert!(paint_rank(Some(&style(Some("relative"), None))) > 0);
        assert!(paint_rank(Some(&style(Some("absolute"), None))) > 0);
    }

    #[test]
    fn z_index_orders_positioned_children() {
        let rank = |z| paint_rank(Some(&style(Some("absolute"), z)));
        assert!(rank(Some(-1.0)) < 0);
        assert!(rank(Some(-2.0)) < rank(Some(-1.0)));
        assert_eq!(rank(None), rank(Some(0.0)));
        assert!(rank(Some(1.0)) > rank(Some(0.0)));
    }

    #[test]
    fn only_a_positioned_element_has_a_z_index_and_auto_is_zero() {
        assert_eq!(lift_z(None), None);
        assert_eq!(lift_z(Some(&style(None, Some(5.0)))), None);
        assert_eq!(lift_z(Some(&style(Some("static"), Some(5.0)))), None);
        for position in ["relative", "absolute", "fixed", "sticky"] {
            assert_eq!(lift_z(Some(&style(Some(position), None))), Some(0), "{position}");
            assert_eq!(lift_z(Some(&style(Some(position), Some(0.0)))), Some(0), "{position}");
            assert_eq!(lift_z(Some(&style(Some(position), Some(-3.0)))), Some(-3), "{position}");
        }
    }

    #[test]
    fn a_z_index_is_rounded_and_clamped() {
        let z = |z| lift_z(Some(&style(Some("relative"), Some(z))));
        assert_eq!(z(2.4), Some(2));
        assert_eq!(z(2.6), Some(3));
        assert_eq!(z(f64::INFINITY), Some(1_000_000));
        assert_eq!(z(f64::NEG_INFINITY), Some(-1_000_000));
        assert_eq!(z(f64::NAN), Some(0));
    }

    #[test]
    fn the_z_index_reaches_the_gpui_style_of_a_positioned_element_only() {
        let z = |position: Option<&str>| {
            let mut el = gpui::div();
            el = apply_z_index(el, &style(position, Some(7.0)));
            el.style().z_index
        };
        assert_eq!(z(Some("relative")), Some(7));
        assert_eq!(z(Some("static")), None);
        assert_eq!(z(None), None);
    }

    #[test]
    fn a_positioned_element_is_a_context_with_a_z_index_not_with_auto() {
        assert!(!context_of(&style(Some("relative"), None)));
        assert!(!context_of(&style(Some("absolute"), None)));
        assert!(context_of(&style(Some("relative"), Some(0.0))));
        assert!(context_of(&style(Some("absolute"), Some(-1.0))));
        // zed's z-index only means something on a positioned element.
        assert!(!context_of(&style(Some("static"), Some(1.0))));
        assert!(!context_of(&style(None, Some(1.0))));
    }

    #[test]
    fn fixed_and_sticky_are_always_a_context() {
        assert!(context_of(&style(Some("fixed"), None)));
        assert!(context_of(&style(Some("sticky"), None)));
    }

    fn with(edit: impl FnOnce(&mut StyleDesc)) -> StyleDesc {
        let mut style = StyleDesc::default();
        edit(&mut style);
        style
    }

    #[test]
    fn an_element_composited_as_a_whole_is_a_context() {
        assert!(!context_of(&StyleDesc::default()));
        assert!(!context_of(&with(|s| s.opacity = Some(1.0))));
        assert!(context_of(&with(|s| s.opacity = Some(0.99))));
        assert!(context_of(&with(|s| s.opacity = Some(0.0))));
        assert!(context_of(&with(|s| s.filter = Some("blur(2px)".into()))));
        assert!(context_of(&with(|s| s.backdrop_filter = Some("blur(4px)".into()))));
        assert!(context_of(&with(|s| {
            s.mask_image = Some("linear-gradient(black, transparent)".into())
        })));
        assert!(context_of(&with(|s| s.clip_path = Some("inset(2px)".into()))));
    }

    #[test]
    fn a_value_that_makes_no_effect_group_makes_no_context() {
        assert!(!context_of(&with(|s| s.filter = Some("none".into()))));
        assert!(!context_of(&with(|s| s.filter = Some("bogus(1)".into()))));
        assert!(!context_of(&with(|s| s.backdrop_filter = Some("none".into()))));
        assert!(!context_of(&with(|s| s.mask_image = Some("none".into()))));
        assert!(!context_of(&with(|s| s.clip_path = Some("none".into()))));
        assert!(!context_of(&with(|s| s.clip_path = Some("bogus".into()))));
    }

    fn tree(nodes: &[(u64, &str, Option<&str>)]) -> RetainedTree {
        let mut tree = RetainedTree::new();
        for (id, kind, display) in nodes {
            tree.create_element(*id, kind.to_string());
            let style = StyleDesc {
                position: Some("relative".into()),
                display: display.map(str::to_string),
                ..Default::default()
            };
            tree.set_style(*id, Arc::new(style));
        }
        tree.set_root(nodes[0].0);
        for window in nodes.windows(2) {
            tree.append_child(window[0].0, window[1].0);
        }
        tree
    }

    #[test]
    fn what_zed_does_not_lift_is_not_lifted_on_the_retained_tree() {
        let tree = tree(&[
            (1, "div", None),
            (2, "anchored", None),
            (3, "div", Some("contents")),
            (4, "virtual-list", None),
            (5, "div", None),
        ]);
        let lifted = |id: u64| lifted_z(&tree, &tree.elements[&id]);
        assert_eq!(lifted(1), Some(0));
        assert_eq!(lifted(2), None);
        assert_eq!(lifted(3), None);
        assert_eq!(lifted(4), Some(0));
        // A row is the root of its own layout.
        assert_eq!(lifted(5), None);
    }

    #[test]
    fn the_root_a_layer_and_a_row_are_contexts_whatever_their_style() {
        let tree = tree(&[
            (1, "div", None),
            (2, "div", None),
            (3, "anchored", None),
            (4, "virtual-list", None),
            (5, "div", None),
        ]);
        let forced = |id: u64| is_forced_context(&tree, &tree.elements[&id]);
        assert!(forced(1));
        assert!(!forced(2));
        assert!(forced(3));
        assert!(!forced(4));
        assert!(forced(5));
    }
}
