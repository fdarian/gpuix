//! What layout last produced for the elements that animate.
//!
//! Motion and CSS transitions sometimes need a size the style does not state: a
//! `height: "auto"` target is the height the content wants, a percentage
//! resolves against the parent's box, and an unset start begins from the laid
//! out value. Layout only exists after the frame is built, so the paint of one
//! frame records it here and the build of the next frame reads it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{App, Bounds, LayoutId, Pixels, Size, Window};

use crate::style::StyleDesc;

// Recorded ahead of the engine: nothing reads a measurement until `height: "auto"`
// and unset starts land.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Measurement {
    /// The painted border box.
    pub border_box: Size<Pixels>,
    /// The border-box height the element would have if it sized to its content,
    /// whatever height it was actually laid out at.
    pub natural_height: Pixels,
    /// The content box of the parent, which a percentage length resolves
    /// against. `None` for a root.
    pub parent_content: Option<Size<Pixels>>,
}

/// Shared with the paint closures, like the paint reports, so a handle is
/// cheap to clone and every clone sees the same map.
#[derive(Clone, Default)]
pub(crate) struct Measurements(Rc<RefCell<HashMap<u64, Measurement>>>);

impl Measurements {
    #[allow(dead_code)]
    pub(crate) fn get(&self, id: u64) -> Option<Measurement> {
        self.0.borrow().get(&id).copied()
    }

    pub(crate) fn remove(&self, id: u64) {
        self.0.borrow_mut().remove(&id);
    }

    pub(crate) fn retain(&self, mut keep: impl FnMut(u64) -> bool) {
        self.0.borrow_mut().retain(|id, _| keep(*id));
    }

    /// The `on_painted_layout` listener that keeps `id` measured. `border` is
    /// the vertical border of the style the element is built with.
    pub(crate) fn recorder(
        &self,
        id: u64,
        border: VerticalBorder,
    ) -> impl Fn(Bounds<Pixels>, LayoutId, &mut Window, &mut App) + 'static {
        let measurements = self.0.clone();
        move |bounds, layout_id, window, _| {
            let content = window.layout_content_size(layout_id);
            let measurement = Measurement {
                border_box: bounds.size,
                natural_height: natural_height(content.height, border),
                parent_content: window.layout_parent_content_size(layout_id),
            };
            measurements.borrow_mut().insert(id, measurement);
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct VerticalBorder {
    pub top: f32,
    pub bottom: f32,
}

impl VerticalBorder {
    /// A side width beats the `borderWidth` shorthand, as it does when the
    /// renderer applies them.
    pub(crate) fn of(style: Option<&StyleDesc>) -> Self {
        let Some(style) = style else {
            return Self::default();
        };
        let width = |side: Option<f64>| {
            side.or(style.border_width)
                .map_or(0.0, |width| width.max(0.0) as f32)
        };
        Self {
            top: width(style.border_top_width),
            bottom: width(style.border_bottom_width),
        }
    }
}

/// Layout's content extent covers the padding but not the border, so the
/// border-box height is the extent plus the border on both sides.
fn natural_height(content_height: Pixels, border: VerticalBorder) -> Pixels {
    content_height + gpui::px(border.top + border.bottom)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(json: &str) -> StyleDesc {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn natural_height_adds_both_borders_to_the_content_extent() {
        let border = VerticalBorder {
            top: 2.0,
            bottom: 3.0,
        };
        assert_eq!(natural_height(gpui::px(80.0), border), gpui::px(85.0));
        assert_eq!(
            natural_height(gpui::px(80.0), VerticalBorder::default()),
            gpui::px(80.0)
        );
    }

    #[test]
    fn a_side_width_beats_the_shorthand() {
        let border = VerticalBorder::of(Some(&style(
            r#"{ "borderWidth": 4, "borderTopWidth": 1 }"#,
        )));
        assert_eq!(
            border,
            VerticalBorder {
                top: 1.0,
                bottom: 4.0
            }
        );
    }

    #[test]
    fn no_border_is_zero_and_a_negative_width_clamps() {
        assert_eq!(VerticalBorder::of(None), VerticalBorder::default());
        assert_eq!(
            VerticalBorder::of(Some(&style(r#"{ "borderBottomWidth": -3 }"#))),
            VerticalBorder::default()
        );
    }

    #[test]
    fn retain_and_remove_drop_measurements() {
        let measurements = Measurements::default();
        let measurement = Measurement {
            border_box: gpui::size(gpui::px(1.0), gpui::px(2.0)),
            natural_height: gpui::px(2.0),
            parent_content: None,
        };
        measurements.0.borrow_mut().insert(1, measurement);
        measurements.0.borrow_mut().insert(2, measurement);
        assert_eq!(measurements.get(1), Some(measurement));
        measurements.remove(1);
        assert_eq!(measurements.get(1), None);
        measurements.retain(|_| false);
        assert_eq!(measurements.get(2), None);
    }
}
