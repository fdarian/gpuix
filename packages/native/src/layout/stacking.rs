//! Stacking contexts: which elements form one, which are lifted into the nearest, and how far
//! up they sort.
//!
//! CSS orders a positioned element in the nearest stacking context, whatever static boxes sit
//! between it and the context. So are a flex or grid item with a `z-index` (Flexbox 4.3, Grid
//! 9.5: it applies without a `position`) and an element whose composite is applied as a whole
//! (`opacity` below 1, a filter, a mask, a clip path), which CSS paints as if positioned with
//! `z-index: 0`. zed does the same for a `Div` flagged `stacking_context()`
//! (`Element::lift_z`): what is lifted is painted and hit tested after the in-flow ones by
//! `(z-index, tree order)`, the negative ones above the context's own background. Where no
//! context is active (a layout root such as a virtual-list row, a deferred layer, a
//! `visibility: hidden` subtree) nothing is lifted and `paint_rank` orders the
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

/// The `z-index` of an element that is lifted into the nearest stacking context, `auto` being 0.
/// `None` for one in flow, which is not lifted and has no index.
///
/// Lifted are a positioned element, a flex or grid item (`item`) with a `z-index` (the index
/// does not apply to a static box otherwise), and anything that forms a context by its effect
/// group, at 0: its own `z-index` only counts where it applies.
pub(crate) fn lift_z(
    style: Option<&StyleDesc>,
    item: bool,
    colors: &ColorContext,
    lengths: &LengthContext,
) -> Option<i32> {
    let style = style?;
    let z = style.z_index.map(round_z);
    if is_positioned(Some(style)) {
        return Some(z.unwrap_or(0));
    }
    if item && z.is_some() {
        return z;
    }
    has_effect_group(style, colors, lengths).then_some(0)
}

fn round_z(z: f64) -> i32 {
    z.round().clamp(-1_000_000.0, 1_000_000.0) as i32
}

/// Where a child paints among its siblings when no stacking context is active, lowest first.
/// Layout is not affected.
///
/// CSS paints negative `z-index` positioned boxes, then in-flow boxes, then positioned boxes by
/// `z-index` (`auto` is 0) in tree order. Only an author-set `position` other than `static`
/// counts as positioned; a lifted static box ranks as one (see `lift_z`).
pub(crate) fn paint_rank(
    style: Option<&StyleDesc>,
    item: bool,
    colors: &ColorContext,
    lengths: &LengthContext,
) -> i32 {
    match lift_z(style, item, colors, lengths) {
        None => 0,
        Some(z) if z < 0 => z,
        Some(z) => z + 1,
    }
}

/// Whether `style` forms a stacking context of its own: a positioned element with a `z-index`
/// (not `auto`), a `fixed` or `sticky` one, a flex or grid item (`item`) with a `z-index`, and
/// anything whose composite is applied as a whole, which is what zed's `effects.rs` does with
/// an effect group. zed cannot see the latter
/// (`element_opacity` is paint-only state), so every one must be flagged.
///
/// `colors` and `lengths` only decide whether a `filter` or `mask` value is valid, and a value
/// that does not parse makes no group. Not `transform`: gpuix has no such style.
pub(crate) fn creates_context(
    style: Option<&StyleDesc>,
    item: bool,
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
    } else if item && style.z_index.is_some() {
        return true;
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

/// Puts the `z-index` of a lifted element on it, and asks zed to lift it when it is not
/// positioned. zed reads it only from an element it was put on, and every styled element
/// carries it, a `virtual-list` or an `<img>` as well as a `Div`. `style.flex_or_grid_item`
/// says whether the element is an item.
pub(crate) fn apply_z_index<E: Styled>(
    mut el: E,
    style: &StyleDesc,
    colors: &ColorContext,
    lengths: &LengthContext,
) -> E {
    if let Some(z) = lift_z(Some(style), style.flex_or_grid_item, colors, lengths) {
        el.style().z_index = Some(z);
        if !is_positioned(Some(style)) {
            el = el.lifted();
        }
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
    if forced
        || creates_context(
            style,
            style.is_some_and(|style| style.flex_or_grid_item),
            colors,
            lengths,
        )
    {
        el.stacking_context()
    } else {
        el
    }
}

/// The retained-tree counterpart of what zed lifts: the `z-index` of `element` if its parent
/// hands it to a stacking context. An `<anchored>` layer is drawn by its own deferred draw, a
/// virtual-list row is a layout root, and `display: contents` makes no box.
pub(crate) fn lifted_z(
    tree: &RetainedTree,
    element: &RetainedElement,
    colors: &ColorContext,
    lengths: &LengthContext,
) -> Option<i32> {
    if element.element_type == "anchored"
        || super::contents::is_display_contents(element)
        || is_virtual_row(tree, element)
    {
        return None;
    }
    lift_z(element.style.as_deref(), is_item(tree, element), colors, lengths)
}

/// Whether `element` is an item of a flex or grid container: its box is a child of one. A
/// `display: contents` parent makes no box, so its children are items of the container around
/// it. Only a `div`, `text`, `span`, `label` or `form` parent counts, as those are the ones
/// that lay their children out by their own `display`.
pub(crate) fn is_item(tree: &RetainedTree, element: &RetainedElement) -> bool {
    if is_virtual_row(tree, element) {
        return false;
    }
    let mut parent = element.parent;
    while let Some(id) = parent {
        let Some(container) = tree.elements.get(&id) else {
            return false;
        };
        if super::contents::is_display_contents(container) {
            parent = container.parent;
            continue;
        }
        return is_flex_or_grid_container(container);
    }
    false
}

/// Whether `element` lays its children out as a flex or grid container.
pub(crate) fn is_flex_or_grid_container(element: &RetainedElement) -> bool {
    crate::text::inline::is_host_container(element)
        && super::display::lays_out_items(element.display().as_deref())
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
        item_context_of(style, false)
    }

    fn item_context_of(style: &StyleDesc, item: bool) -> bool {
        creates_context(
            Some(style),
            item,
            &ColorContext::root(Scheme::Light),
            &LengthContext::for_tests(),
        )
    }

    fn lift_of(style: &StyleDesc, item: bool) -> Option<i32> {
        lift_z(
            Some(style),
            item,
            &ColorContext::root(Scheme::Light),
            &LengthContext::for_tests(),
        )
    }

    fn rank_of(style: Option<&StyleDesc>, item: bool) -> i32 {
        paint_rank(
            style,
            item,
            &ColorContext::root(Scheme::Light),
            &LengthContext::for_tests(),
        )
    }

    #[test]
    fn in_flow_children_paint_below_positioned_ones() {
        assert_eq!(rank_of(None, false), 0);
        assert_eq!(rank_of(Some(&style(None, Some(5.0))), false), 0);
        assert_eq!(rank_of(Some(&style(Some("static"), None)), false), 0);
        assert!(rank_of(Some(&style(Some("relative"), None)), false) > 0);
        assert!(rank_of(Some(&style(Some("absolute"), None)), false) > 0);
    }

    #[test]
    fn z_index_orders_positioned_children() {
        let rank = |z| rank_of(Some(&style(Some("absolute"), z)), false);
        assert!(rank(Some(-1.0)) < 0);
        assert!(rank(Some(-2.0)) < rank(Some(-1.0)));
        assert_eq!(rank(None), rank(Some(0.0)));
        assert!(rank(Some(1.0)) > rank(Some(0.0)));
    }

    #[test]
    fn only_a_positioned_element_has_a_z_index_and_auto_is_zero() {
        assert_eq!(lift_z(None, false, &ColorContext::root(Scheme::Light), &LengthContext::for_tests()), None);
        assert_eq!(lift_of(&style(None, Some(5.0)), false), None);
        assert_eq!(lift_of(&style(Some("static"), Some(5.0)), false), None);
        for position in ["relative", "absolute", "fixed", "sticky"] {
            assert_eq!(lift_of(&style(Some(position), None), false), Some(0), "{position}");
            assert_eq!(lift_of(&style(Some(position), Some(0.0)), false), Some(0), "{position}");
            assert_eq!(lift_of(&style(Some(position), Some(-3.0)), false), Some(-3), "{position}");
        }
    }

    #[test]
    fn a_z_index_is_rounded_and_clamped() {
        let z = |z| lift_of(&style(Some("relative"), Some(z)), false);
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
            el = apply_z_index(
                el,
                &style(position, Some(7.0)),
                &ColorContext::root(Scheme::Light),
                &LengthContext::for_tests(),
            );
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

    #[test]
    fn a_flex_or_grid_item_has_a_z_index_without_a_position() {
        for item in [style(None, Some(3.0)), style(Some("static"), Some(-2.0))] {
            let z = item.z_index.unwrap() as i32;
            assert_eq!(lift_of(&item, true), Some(z));
            assert_eq!(lift_of(&item, false), None);
            assert!(item_context_of(&item, true));
            assert!(!item_context_of(&item, false));
        }
        // `auto` is a static box that paints in tree order, like an inline-block.
        assert_eq!(lift_of(&style(None, None), true), None);
        assert!(!item_context_of(&style(None, None), true));
        // Zero is not auto: it makes a context and joins the layer of the positioned boxes.
        assert_eq!(lift_of(&style(None, Some(0.0)), true), Some(0));
        assert!(item_context_of(&style(None, Some(0.0)), true));
        assert!(rank_of(Some(&style(None, Some(0.0))), true) > 0);
        assert!(rank_of(Some(&style(None, Some(-1.0))), true) < 0);
        assert_eq!(rank_of(Some(&style(None, Some(1.0))), false), 0);
    }

    #[test]
    fn a_positioned_item_keeps_its_own_z_index() {
        assert_eq!(lift_of(&style(Some("relative"), Some(4.0)), true), Some(4));
        assert_eq!(lift_of(&style(Some("relative"), None), true), Some(0));
    }

    #[test]
    fn an_element_with_an_effect_group_paints_in_the_zero_layer_whatever_its_position() {
        let faded = with(|s| s.opacity = Some(0.5));
        assert_eq!(lift_of(&faded, false), Some(0));
        assert_eq!(lift_of(&faded, true), Some(0));
        assert!(rank_of(Some(&faded), false) > 0);
        for edit in [
            (|s: &mut StyleDesc| s.filter = Some("blur(2px)".into())) as fn(&mut StyleDesc),
            |s| s.backdrop_filter = Some("blur(4px)".into()),
            |s| s.mask_image = Some("linear-gradient(black, transparent)".into()),
            |s| s.clip_path = Some("inset(2px)".into()),
        ] {
            assert_eq!(lift_of(&with(edit), false), Some(0));
        }
        assert_eq!(lift_of(&with(|s| s.opacity = Some(1.0)), false), None);
        assert_eq!(lift_of(&with(|s| s.filter = Some("none".into())), false), None);
    }

    #[test]
    fn a_z_index_applies_to_an_effect_group_only_where_it_applies_at_all() {
        let faded = |z, position: Option<&str>| StyleDesc {
            opacity: Some(0.5),
            position: position.map(str::to_string),
            z_index: Some(z),
            ..Default::default()
        };
        // Static and not an item: the index is ignored, the group still paints at 0.
        assert_eq!(lift_of(&faded(7.0, None), false), Some(0));
        assert_eq!(lift_of(&faded(7.0, None), true), Some(7));
        assert_eq!(lift_of(&faded(7.0, Some("relative")), false), Some(7));
    }

    #[test]
    fn a_lifted_static_element_asks_zed_to_lift_it() {
        let lifts = |style: &StyleDesc| {
            let mut el = gpui::div();
            el = apply_z_index(
                el,
                style,
                &ColorContext::root(Scheme::Light),
                &LengthContext::for_tests(),
            );
            (el.style().z_index, el.style().lifts)
        };
        let item = StyleDesc {
            z_index: Some(3.0),
            flex_or_grid_item: true,
            ..Default::default()
        };
        assert_eq!(lifts(&item), (Some(3), Some(true)));
        assert_eq!(lifts(&with(|s| s.opacity = Some(0.5))), (Some(0), Some(true)));
        // A positioned element is lifted by zed already.
        assert_eq!(lifts(&style(Some("relative"), Some(3.0))), (Some(3), None));
        assert_eq!(lifts(&style(None, Some(3.0))), (None, None));
        assert_eq!(lifts(&StyleDesc::default()), (None, None));
    }

    fn container_tree(display: Option<&str>, parent_type: &str) -> RetainedTree {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".into());
        tree.create_element(2, parent_type.into());
        tree.create_element(3, "div".into());
        for (id, display) in [(1, None), (2, display), (3, None)] {
            tree.set_style(
                id,
                Arc::new(StyleDesc {
                    display: display.map(str::to_string),
                    ..Default::default()
                }),
            );
        }
        tree.set_root(1);
        tree.append_child(1, 2);
        tree.append_child(2, 3);
        tree
    }

    #[test]
    fn a_child_of_a_flex_or_grid_container_is_an_item() {
        let item = |display, parent_type| {
            let tree = container_tree(display, parent_type);
            is_item(&tree, &tree.elements[&3])
        };
        assert!(item(Some("flex"), "div"));
        assert!(item(Some("grid"), "span"));
        assert!(!item(Some("block"), "div"));
        assert!(!item(None, "div"));
        // A custom element lays its children out in a box of its own.
        assert!(!item(Some("flex"), "anchored"));
    }

    #[test]
    fn a_display_contents_parent_hands_its_children_to_the_container_around_it() {
        let mut tree = container_tree(Some("flex"), "div");
        tree.create_element(4, "div".into());
        tree.set_style(
            4,
            Arc::new(StyleDesc {
                display: Some("contents".into()),
                ..Default::default()
            }),
        );
        tree.create_element(5, "div".into());
        tree.set_style(5, Arc::new(StyleDesc::default()));
        tree.append_child(2, 4);
        tree.append_child(4, 5);
        assert!(is_item(&tree, &tree.elements[&5]));
        assert!(!is_item(&tree, &tree.elements[&1]));
        // The root has no parent, and the contents element is no item of its own.
        let tree = container_tree(Some("block"), "div");
        assert!(!is_item(&tree, &tree.elements[&1]));
    }

    #[test]
    fn a_virtual_list_row_is_no_item() {
        let mut tree = container_tree(Some("flex"), "virtual-list");
        tree.set_style(
            2,
            Arc::new(StyleDesc {
                display: Some("flex".into()),
                ..Default::default()
            }),
        );
        assert!(!is_item(&tree, &tree.elements[&3]));
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
        let lifted = |id: u64| {
            lifted_z(
                &tree,
                &tree.elements[&id],
                &ColorContext::root(Scheme::Light),
                &LengthContext::for_tests(),
            )
        };
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
