//! CSS `anchor-size(width)` / `anchor-size(height)` on the six size properties of an
//! `<anchored>` layer's direct children.
//!
//! The trigger is only measured once layout has run, and the child's style is needed
//! to run it, so a child is built with the size the previous placement recorded and
//! the renderer is notified when that size changes. Until a size exists the child is
//! built transparent, so it never shows at a guessed size.

use std::cell::RefCell;
use std::collections::HashMap;

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

pub(crate) fn begin_build(anchored_id: u64) {
    ANCHORS.with(|anchors| {
        anchors.borrow_mut().entry(anchored_id).or_default().used = false;
    });
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
    let uses_anchor_size = [
        &style.width,
        &style.min_width,
        &style.max_width,
        &style.height,
        &style.min_height,
        &style.max_height,
    ]
    .into_iter()
    .any(|value| matches!(value, Some(DimensionValue::AnchorSize(_))));
    if !uses_anchor_size {
        return None;
    }

    ANCHORS.with(|anchors| {
        anchors.borrow_mut().entry(anchored_id).or_default().used = true;
    });
    let size = ANCHORS.with(|anchors| anchors.borrow().get(&anchored_id).and_then(|info| info.size));

    let mut resolved = style.clone();
    for slot in [
        &mut resolved.width,
        &mut resolved.min_width,
        &mut resolved.max_width,
        &mut resolved.height,
        &mut resolved.min_height,
        &mut resolved.max_height,
    ] {
        let axis = match slot {
            Some(DimensionValue::AnchorSize(axis)) => *axis,
            _ => continue,
        };
        *slot = size.map(|(width, height)| {
            DimensionValue::Pixels(match axis {
                AnchorAxis::Width => width as f64,
                AnchorAxis::Height => height as f64,
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

    fn style_with(width: Option<DimensionValue>, min_height: Option<DimensionValue>) -> StyleDesc {
        StyleDesc {
            width,
            min_height,
            ..Default::default()
        }
    }

    #[test]
    fn leaves_styles_without_anchor_size_alone() {
        assert_eq!(resolve(1, &style_with(Some(DimensionValue::Pixels(5.0)), None)), None);
    }

    #[test]
    fn is_transparent_until_the_trigger_has_been_measured() {
        let style = style_with(Some(DimensionValue::AnchorSize(AnchorAxis::Width)), None);
        let resolved = resolve(900, &style).unwrap();
        assert_eq!(resolved.width, None);
        assert_eq!(resolved.opacity, Some(0.0));
        forget(900);
    }

    #[test]
    fn resolves_either_axis_against_the_recorded_size() {
        assert!(record(901, (120, 30)));
        assert!(!record(901, (120, 30)));
        let style = style_with(
            Some(DimensionValue::AnchorSize(AnchorAxis::Width)),
            Some(DimensionValue::AnchorSize(AnchorAxis::Height)),
        );
        let resolved = resolve(901, &style).unwrap();
        assert_eq!(resolved.width, Some(DimensionValue::Pixels(120.0)));
        assert_eq!(resolved.min_height, Some(DimensionValue::Pixels(30.0)));
        assert_eq!(resolved.opacity, None);
        assert!(uses_anchor_size(901));
        begin_build(901);
        assert!(!uses_anchor_size(901));
        forget(901);
    }
}
