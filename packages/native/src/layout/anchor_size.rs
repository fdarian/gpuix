//! CSS `anchor-size(width)` / `anchor-size(height)` on the sizes, margins and insets of
//! an `<anchored>` layer's direct children.
//!
//! The trigger is only measured once layout has run, and the child's style is needed
//! to run it, so a child is built with the size the previous placement recorded and
//! the renderer is notified when that size changes. A layer mounted under a trigger
//! that was already painted starts from that painted size. With no size at all the
//! child is built transparent, so it never shows at a guessed size.

use std::cell::RefCell;
use std::collections::HashMap;

use gpui::WindowId;

use crate::retained_tree::{RetainedElement, RetainedTree};
use crate::style::{AnchorAxis, DimensionValue, StyleDesc};

#[derive(Clone, Copy, Default)]
struct AnchorInfo {
    size: Option<(i32, i32)>,
    /// A child used `anchor-size()` the last time this layer was built.
    used: bool,
}

thread_local! {
    static ANCHORS: RefCell<HashMap<u64, AnchorInfo>> = RefCell::new(HashMap::new());
}

/// Every longhand of a style that is a `DimensionValue` and takes `anchor-size()`.
/// `$borrow` is `&` or `&mut`. The `margin` and `inset` shorthands hold one per side and are
/// handled apart.
macro_rules! anchor_slots {
    ($style:expr, $($borrow:tt)+) => {
        [
            $($borrow)+ $style.width,
            $($borrow)+ $style.min_width,
            $($borrow)+ $style.max_width,
            $($borrow)+ $style.height,
            $($borrow)+ $style.min_height,
            $($borrow)+ $style.max_height,
            $($borrow)+ $style.margin_top,
            $($borrow)+ $style.margin_right,
            $($borrow)+ $style.margin_bottom,
            $($borrow)+ $style.margin_left,
            $($borrow)+ $style.top,
            $($borrow)+ $style.right,
            $($borrow)+ $style.bottom,
            $($borrow)+ $style.left,
        ]
    };
}

/// Records the trigger's border-box size. Returns whether it changed.
pub(crate) fn record(anchored_id: u64, size: (i32, i32)) -> bool {
    ANCHORS.with(|anchors| {
        let mut anchors = anchors.borrow_mut();
        let info = anchors.entry(anchored_id).or_default();
        let changed = info.size != Some(size);
        info.size = Some(size);
        changed
    })
}

/// Starts a build of the layer. When no placement has measured the trigger yet, the
/// trigger's size from the last paint stands in, so a layer mounted under a trigger on
/// screen builds at the right size in its first frame.
pub(crate) fn begin_build(window: WindowId, tree: &RetainedTree, anchored: &RetainedElement) {
    let painted = ANCHORS
        .with(|anchors| anchors.borrow().get(&anchored.id).is_none_or(|info| info.size.is_none()))
        .then(|| painted_trigger_size(window, tree, anchored))
        .flatten();
    ANCHORS.with(|anchors| {
        let mut anchors = anchors.borrow_mut();
        let info = anchors.entry(anchored.id).or_default();
        info.used = false;
        if info.size.is_none() {
            info.size = painted;
        }
    });
}

/// The layout parent of an `<anchored>` is the trigger: its nearest ancestor that is
/// not `display: contents`.
fn painted_trigger_size(
    window: WindowId,
    tree: &RetainedTree,
    anchored: &RetainedElement,
) -> Option<(i32, i32)> {
    let mut parent = anchored.parent?;
    while let Some(element) = tree.elements.get(&parent) {
        if !crate::layout::contents::is_display_contents(element) {
            break;
        }
        parent = element.parent?;
    }
    let bounds = crate::automation::get_bounds(window, parent)?;
    Some((bounds.width.round() as i32, bounds.height.round() as i32))
}

pub(crate) fn uses_anchor_size(anchored_id: u64) -> bool {
    ANCHORS.with(|anchors| anchors.borrow().get(&anchored_id).is_some_and(|info| info.used))
}

pub(crate) fn forget(anchored_id: u64) {
    ANCHORS.with(|anchors| {
        anchors.borrow_mut().remove(&anchored_id);
    });
}

/// `style` with every `anchor-size()` replaced by the trigger's size, or `None` when it
/// has none.
pub(crate) fn resolve(anchored_id: u64, style: &StyleDesc) -> Option<StyleDesc> {
    let uses_anchor_size = anchor_slots!(style, &)
        .into_iter()
        .any(|value| matches!(value, Some(DimensionValue::AnchorSize(_))))
        || [&style.margin, &style.inset]
            .into_iter()
            .flatten()
            .any(|sides| sides.values().into_iter().any(|value| matches!(value, DimensionValue::AnchorSize(_))))
        || [&style.margin_block, &style.margin_inline, &style.inset_block, &style.inset_inline]
            .into_iter()
            .flatten()
            .any(|pair| pair.values().into_iter().any(|value| matches!(value, DimensionValue::AnchorSize(_))));
    if !uses_anchor_size {
        return None;
    }

    let size = ANCHORS.with(|anchors| {
        let mut anchors = anchors.borrow_mut();
        let info = anchors.entry(anchored_id).or_default();
        info.used = true;
        info.size
    });

    let anchor_px = |axis: AnchorAxis| {
        size.map(|(width, height)| {
            DimensionValue::Pixels(match axis {
                AnchorAxis::Width => width as f64,
                AnchorAxis::Height => height as f64,
            })
        })
    };
    let mut resolved = style.clone();
    for slot in anchor_slots!(resolved, &mut) {
        let axis = match slot {
            Some(DimensionValue::AnchorSize(axis)) => *axis,
            _ => continue,
        };
        *slot = anchor_px(axis);
    }
    // A side that cannot resolve yet takes its whole shorthand with it: the child is
    // transparent until the trigger is measured.
    for slot in [&mut resolved.margin, &mut resolved.inset] {
        *slot = slot.take().and_then(|sides| {
            sides.try_map(|value| match value {
                DimensionValue::AnchorSize(axis) => anchor_px(axis),
                other => Some(other),
            })
        });
    }
    for slot in [
        &mut resolved.margin_block,
        &mut resolved.margin_inline,
        &mut resolved.inset_block,
        &mut resolved.inset_inline,
    ] {
        *slot = slot.take().and_then(|pair| {
            pair.try_map(|value| match value {
                DimensionValue::AnchorSize(axis) => anchor_px(axis),
                other => Some(other),
            })
        });
    }
    if size.is_none() {
        resolved.opacity = Some(0.0);
    }
    Some(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::shorthand::Sides;
    use std::sync::Arc;

    fn style_with(width: Option<DimensionValue>, min_height: Option<DimensionValue>) -> StyleDesc {
        StyleDesc {
            width,
            min_height,
            ..Default::default()
        }
    }

    fn anchor(axis: AnchorAxis) -> Option<DimensionValue> {
        Some(DimensionValue::AnchorSize(axis))
    }

    fn paint(id: u64, width: f32, height: f32) {
        let origin = gpui::point(gpui::px(10.0), gpui::px(10.0));
        crate::automation::record_bounds(
            WindowId::from(1),
            id,
            crate::automation::LaidOut::of_box(gpui::Bounds::new(origin, gpui::size(gpui::px(width), gpui::px(height))), 1.0),
            origin,
            gpui::LayoutInsets::default(),
        );
    }

    #[test]
    fn leaves_styles_without_anchor_size_alone() {
        assert_eq!(resolve(1, &style_with(Some(DimensionValue::Pixels(5.0)), None)), None);
    }

    #[test]
    fn is_transparent_until_the_trigger_has_been_measured() {
        let style = style_with(anchor(AnchorAxis::Width), None);
        let resolved = resolve(900, &style).unwrap();
        assert_eq!(resolved.width, None);
        assert_eq!(resolved.opacity, Some(0.0));
        forget(900);
    }

    #[test]
    fn resolves_either_axis_against_the_recorded_size() {
        assert!(record(901, (120, 30)));
        assert!(!record(901, (120, 30)));
        let style = style_with(anchor(AnchorAxis::Width), anchor(AnchorAxis::Height));
        let resolved = resolve(901, &style).unwrap();
        assert_eq!(resolved.width, Some(DimensionValue::Pixels(120.0)));
        assert_eq!(resolved.min_height, Some(DimensionValue::Pixels(30.0)));
        assert_eq!(resolved.opacity, None);
        assert!(uses_anchor_size(901));
        let mut tree = RetainedTree::new();
        tree.create_element(901, "anchored".to_string());
        begin_build(WindowId::from(1), &tree, &tree.elements[&901]);
        assert!(!uses_anchor_size(901));
        forget(901);
    }

    #[test]
    fn resolves_margins_and_insets() {
        record(902, (120, 30));
        let style = StyleDesc {
            margin: Some(Sides::all(DimensionValue::AnchorSize(AnchorAxis::Width))),
            margin_top: anchor(AnchorAxis::Height),
            margin_right: anchor(AnchorAxis::Width),
            margin_bottom: anchor(AnchorAxis::Height),
            margin_left: anchor(AnchorAxis::Width),
            top: anchor(AnchorAxis::Height),
            right: anchor(AnchorAxis::Width),
            bottom: anchor(AnchorAxis::Height),
            left: anchor(AnchorAxis::Width),
            ..Default::default()
        };
        let resolved = resolve(902, &style).unwrap();
        let width = Some(DimensionValue::Pixels(120.0));
        let height = Some(DimensionValue::Pixels(30.0));
        assert_eq!(resolved.margin, width.clone().map(Sides::all));
        assert_eq!(resolved.margin_top, height);
        assert_eq!(resolved.margin_right, width);
        assert_eq!(resolved.margin_bottom, height);
        assert_eq!(resolved.margin_left, width);
        assert_eq!(resolved.top, height);
        assert_eq!(resolved.right, width);
        assert_eq!(resolved.bottom, height);
        assert_eq!(resolved.left, width);
        assert_eq!(resolved.opacity, None);
        forget(902);
    }

    #[test]
    fn the_logical_pairs_take_the_trigger_size_too() {
        use crate::css::logical::Pair;
        paint(904, 120.0, 30.0);
        let style = StyleDesc {
            margin_inline: Some(Pair { start: DimensionValue::AnchorSize(AnchorAxis::Width), end: DimensionValue::Pixels(2.0) }),
            inset_block: Some(Pair::both(DimensionValue::AnchorSize(AnchorAxis::Height))),
            ..Default::default()
        };
        let tree = mounted(905, None);
        record(905, (120, 30));
        begin_build(WindowId::from(1), &tree, &tree.elements[&905]);
        let resolved = resolve(905, &style).unwrap();
        assert_eq!(
            resolved.margin_inline,
            Some(Pair { start: DimensionValue::Pixels(120.0), end: DimensionValue::Pixels(2.0) })
        );
        assert_eq!(resolved.inset_block, Some(Pair::both(DimensionValue::Pixels(30.0))));
        forget(905);
    }

    #[test]
    fn a_margin_alone_makes_the_child_wait_for_the_trigger() {
        let style = StyleDesc {
            margin_left: anchor(AnchorAxis::Width),
            ..Default::default()
        };
        let resolved = resolve(903, &style).unwrap();
        assert_eq!(resolved.margin_left, None);
        assert_eq!(resolved.opacity, Some(0.0));
        assert!(uses_anchor_size(903));
        forget(903);
    }

    fn mounted(anchored: u64, parent_style: Option<&str>) -> RetainedTree {
        let mut tree = RetainedTree::new();
        tree.create_element(anchored - 1, "div".to_string());
        tree.create_element(anchored, "anchored".to_string());
        tree.append_child(anchored - 1, anchored);
        if let Some(style) = parent_style {
            let style = serde_json::from_str(style).expect("a style");
            tree.set_style(anchored - 1, Arc::new(style));
        }
        tree
    }

    #[test]
    fn the_first_build_starts_from_the_trigger_painted_last_frame() {
        let tree = mounted(911, None);
        paint(910, 140.4, 29.6);
        begin_build(WindowId::from(1), &tree, &tree.elements[&911]);
        let resolved = resolve(911, &style_with(anchor(AnchorAxis::Width), anchor(AnchorAxis::Height))).unwrap();
        assert_eq!(resolved.width, Some(DimensionValue::Pixels(140.0)));
        assert_eq!(resolved.min_height, Some(DimensionValue::Pixels(30.0)));
        assert_eq!(resolved.opacity, None);
        // The placement that measures the same size has nothing to rebuild for.
        assert!(!record(911, (140, 30)));
        forget(911);
    }

    #[test]
    fn a_placement_measurement_beats_the_painted_size() {
        let tree = mounted(921, None);
        record(921, (200, 50));
        paint(920, 140.0, 30.0);
        begin_build(WindowId::from(1), &tree, &tree.elements[&921]);
        let resolved = resolve(921, &style_with(anchor(AnchorAxis::Width), None)).unwrap();
        assert_eq!(resolved.width, Some(DimensionValue::Pixels(200.0)));
        forget(921);
    }

    #[test]
    fn the_trigger_is_the_nearest_ancestor_that_is_not_display_contents() {
        let mut tree = mounted(931, Some(r#"{"display":"contents"}"#));
        tree.create_element(929, "div".to_string());
        tree.append_child(929, 930);
        paint(929, 77.0, 20.0);
        paint(930, 5.0, 5.0);
        begin_build(WindowId::from(1), &tree, &tree.elements[&931]);
        let resolved = resolve(931, &style_with(anchor(AnchorAxis::Width), None)).unwrap();
        assert_eq!(resolved.width, Some(DimensionValue::Pixels(77.0)));
        forget(931);
    }

    #[test]
    fn stays_transparent_when_the_trigger_has_not_painted() {
        let tree = mounted(941, None);
        begin_build(WindowId::from(1), &tree, &tree.elements[&941]);
        let resolved = resolve(941, &style_with(anchor(AnchorAxis::Width), None)).unwrap();
        assert_eq!(resolved.opacity, Some(0.0));
        forget(941);
    }
}
