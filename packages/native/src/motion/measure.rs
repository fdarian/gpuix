//! What layout last produced for the elements that animate.
//!
//! Motion and CSS transitions sometimes need a size the style does not state: a
//! `height: "auto"` target is the height the content wants, a percentage
//! resolves against the parent's box, and an unset start begins from the laid
//! out value. Layout only exists after the frame is built, so the paint of one
//! frame records it here and the build of the next frame reads it.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{App, Bounds, LayoutId, Pixels, Size, Window};

use super::property::{AnimProp, Axis};
use crate::style::StyleDesc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Measurement {
    /// The painted border box.
    pub border_box: Size<Pixels>,
    /// The border-box height the element would have if it sized to its content,
    /// whatever height it was actually laid out at.
    pub natural_height: Pixels,
    /// The content box of the parent, which a percentage length resolves
    /// against. A root has no parent, so it is the window's content size, as
    /// the initial containing block is in a document.
    pub parent_content: Size<Pixels>,
    /// Device pixels per layout pixel.
    pub scale_factor: f32,
}

impl Measurement {
    /// The natural height rounded up to a whole device pixel, so content that
    /// ends part-way through a pixel is not clipped by a height that stops
    /// short of it. The epsilon keeps a height that is already whole, but off
    /// by f32 rounding, from gaining a pixel.
    pub(super) fn natural_height_px(&self) -> f64 {
        let scale = f64::from(self.scale_factor);
        let device = f64::from(self.natural_height.as_f32()) * scale;
        (device - 1e-3).ceil().max(0.0) / scale
    }

    /// What a percentage of `prop` is in px: a fraction of the parent's content
    /// box along the axis `prop` sizes. `None` for a property that is not a size.
    pub(super) fn percent_px(&self, prop: AnimProp, fraction: f64) -> Option<f64> {
        let length = match prop.size_axis()? {
            Axis::Width => self.parent_content.width,
            Axis::Height => self.parent_content.height,
        };
        Some(f64::from(length.as_f32()) * fraction)
    }

    /// The painted border-box size along the axis `prop` sizes.
    pub(super) fn laid_out(&self, prop: AnimProp) -> Option<f64> {
        Some(f64::from(match prop.size_axis()? {
            Axis::Width => self.border_box.width.as_f32(),
            Axis::Height => self.border_box.height.as_f32(),
        }))
    }
}

/// Shared with the paint closures, like the paint reports, so a handle is
/// cheap to clone and every clone sees the same map.
#[derive(Clone, Default)]
pub(crate) struct Measurements {
    by_element: Rc<RefCell<HashMap<u64, Measurement>>>,
    painted_frames: Rc<Cell<u64>>,
}

impl Measurements {
    pub(crate) fn get(&self, id: u64) -> Option<Measurement> {
        self.by_element.borrow().get(&id).copied()
    }

    pub(crate) fn remove(&self, id: u64) {
        self.by_element.borrow_mut().remove(&id);
    }

    pub(crate) fn retain(&self, mut keep: impl FnMut(u64) -> bool) {
        self.by_element.borrow_mut().retain(|id, _| keep(*id));
    }

    /// How many frames have been painted. An element without a measurement
    /// after a frame was painted is not in that frame at all.
    pub(crate) fn painted_frames(&self) -> u64 {
        self.painted_frames.get()
    }

    /// The `on_painted` listener for the root element, which every painted
    /// frame includes.
    pub(crate) fn frame_counter(&self) -> impl Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static {
        let painted_frames = self.painted_frames.clone();
        move |_, _, _| painted_frames.set(painted_frames.get() + 1)
    }

    /// The `on_painted_layout` listener that keeps `id` measured. `border` is
    /// the vertical border of the style the element is built with.
    pub(crate) fn recorder(
        &self,
        id: u64,
        border: VerticalBorder,
    ) -> impl Fn(Bounds<Pixels>, LayoutId, &mut Window, &mut App) + 'static {
        let measurements = self.by_element.clone();
        move |bounds, layout_id, window, _| {
            let content = window.layout_content_size(layout_id);
            let measurement = Measurement {
                border_box: bounds.size,
                natural_height: natural_height(content.height, border),
                parent_content: window
                    .layout_parent_content_size(layout_id)
                    .unwrap_or_else(|| window.viewport_size()),
                scale_factor: window.scale_factor(),
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

    fn measurement(natural_height: f32, scale_factor: f32) -> Measurement {
        Measurement {
            border_box: gpui::size(gpui::px(40.0), gpui::px(30.0)),
            natural_height: gpui::px(natural_height),
            parent_content: gpui::size(gpui::px(800.0), gpui::px(600.0)),
            scale_factor,
        }
    }

    #[test]
    fn the_natural_height_rounds_up_to_the_device_pixel() {
        assert_eq!(measurement(80.2, 1.0).natural_height_px(), 81.0);
        assert_eq!(measurement(80.2, 2.0).natural_height_px(), 80.5);
        assert_eq!(measurement(80.0, 2.0).natural_height_px(), 80.0);
        assert_eq!(measurement(0.0, 2.0).natural_height_px(), 0.0);
        // f32 noise on a whole height is not a pixel of content.
        assert_eq!(measurement(80.000_01, 2.0).natural_height_px(), 80.0);
    }

    #[test]
    fn a_size_property_reads_its_axis_of_the_border_box() {
        let measurement = measurement(50.0, 1.0);
        assert_eq!(measurement.laid_out(AnimProp::Width), Some(40.0));
        assert_eq!(measurement.laid_out(AnimProp::MaxWidth), Some(40.0));
        assert_eq!(measurement.laid_out(AnimProp::Height), Some(30.0));
        assert_eq!(measurement.laid_out(AnimProp::MinHeight), Some(30.0));
        assert_eq!(measurement.laid_out(AnimProp::Top), None);
    }

    #[test]
    fn a_percentage_resolves_against_the_parent_content_box_on_its_own_axis() {
        let nested = Measurement {
            parent_content: gpui::size(gpui::px(200.0), gpui::px(80.0)),
            ..measurement(50.0, 1.0)
        };
        assert_eq!(nested.percent_px(AnimProp::Width, 0.5), Some(100.0));
        assert_eq!(nested.percent_px(AnimProp::MaxWidth, 0.25), Some(50.0));
        assert_eq!(nested.percent_px(AnimProp::Height, 0.5), Some(40.0));
        assert_eq!(nested.percent_px(AnimProp::MinHeight, 1.0), Some(80.0));
        assert_eq!(nested.percent_px(AnimProp::Top, 0.5), None);
        assert_eq!(nested.percent_px(AnimProp::Opacity, 0.5), None);
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
            parent_content: gpui::size(gpui::px(800.0), gpui::px(600.0)),
            scale_factor: 1.0,
        };
        measurements.by_element.borrow_mut().insert(1, measurement);
        measurements.by_element.borrow_mut().insert(2, measurement);
        assert_eq!(measurements.get(1), Some(measurement));
        measurements.remove(1);
        assert_eq!(measurements.get(1), None);
        measurements.retain(|_| false);
        assert_eq!(measurements.get(2), None);
    }
}
