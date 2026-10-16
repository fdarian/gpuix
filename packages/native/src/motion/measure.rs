//! What layout last produced for the elements that animate.
//!
//! Motion and CSS transitions sometimes need a size the style does not state: a
//! `height: "auto"` target is the height the content wants, a percentage
//! resolves against the parent's box, and an unset start begins from the laid
//! out value. Layout only exists after the frame is built, so the paint of one
//! frame records it here and the build of the next frame reads it.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, Edges, Element, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    Pixels, Size, Window,
};

use super::property::{AnimProp, Axis};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Measurement {
    /// The painted border box.
    pub border_box: Size<Pixels>,
    /// The border-box height the element would have if it sized to its content,
    /// whatever height it was actually laid out at.
    pub natural_height: Pixels,
    /// The border-box width the element would have with `width: auto`. `None`
    /// until a frame was laid out with the element flagged to measure it, which
    /// only an element that can animate `width` to or from `auto` is.
    pub natural_width: Option<Pixels>,
    /// The content box of the parent, which a percentage length resolves
    /// against. A root has no parent, so it is the window's content size, as
    /// the initial containing block is in a document.
    pub parent_content: Size<Pixels>,
    /// Device pixels per layout pixel.
    pub scale_factor: f32,
    /// Whether a percentage `height` of the element has a definite height to
    /// resolve against. CSS treats one that has not as `auto`.
    pub percent_height_resolves: bool,
}

impl Measurement {
    /// The natural size along the axis `prop` sizes, rounded up to a whole
    /// device pixel, so content that ends part-way through a pixel is not
    /// clipped by a size that stops short of it. The epsilon keeps a size that
    /// is already whole, but off by f32 rounding, from gaining a pixel.
    ///
    /// `None` for a property that is not a size, and for a width until it has
    /// been measured.
    pub(super) fn natural_px(&self, prop: AnimProp) -> Option<f64> {
        let length = match prop.size_axis()? {
            Axis::Width => self.natural_width?,
            Axis::Height => self.natural_height,
        };
        let scale = f64::from(self.scale_factor);
        let device = f64::from(length.as_f32()) * scale;
        Some((device - 1e-3).ceil().max(0.0) / scale)
    }

    /// Whether values of `prop` that include an `auto` (`has_auto`) or not can
    /// be resolved in px: an `auto` needs the natural size.
    pub(super) fn resolves(&self, prop: AnimProp, has_auto: bool) -> bool {
        !has_auto || self.natural_px(prop).is_some()
    }

    /// What a percentage of `prop` is in px: a fraction of the parent's content
    /// box along the axis CSS resolves it against (`AnimProp::percent_axis`).
    /// `None` for a property that takes no percentage. A `height` with nothing
    /// definite to resolve against is the natural height, as `height: auto`.
    ///
    /// The parent stands in for the containing block, which an absolute box
    /// resolves against its nearest positioned ancestor's padding box instead.
    pub(super) fn percent_px(&self, prop: AnimProp, fraction: f64) -> Option<f64> {
        let axis = prop.percent_axis()?;
        if prop == AnimProp::Height && !self.percent_height_resolves {
            return self.natural_px(AnimProp::Height);
        }
        let length = match axis {
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
    /// The elements `wrap` was called for since the last `prune`.
    wrapped: Rc<RefCell<HashSet<u64>>>,
    painted_frames: Rc<Cell<u64>>,
}

impl Measurements {
    pub(crate) fn get(&self, id: u64) -> Option<Measurement> {
        self.by_element.borrow().get(&id).copied()
    }

    /// Drops the measurement of every element that `exists` rejects or that was
    /// not wrapped since the last prune, then starts a new round of wrapping.
    ///
    /// An element that stops being measured keeps living, and a measurement
    /// left behind would seed the size it starts from if it gains motion later.
    /// Rows of a virtual list are built after the root render, so the round
    /// runs from one root render to the next.
    pub(crate) fn prune(&self, mut exists: impl FnMut(u64) -> bool) {
        let mut wrapped = self.wrapped.borrow_mut();
        self.by_element
            .borrow_mut()
            .retain(|id, _| wrapped.contains(id) && exists(*id));
        wrapped.clear();
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

    /// `child` wrapped so each paint of it keeps `id` measured. `percent_height_resolves` is
    /// what `Container::resolves_percent_height` said.
    ///
    /// A wrapper rather than `on_painted_layout` on the element: that listener
    /// lives on GPUI's `Interactivity`, which `<img>`, `<svg>`, `<input>` and
    /// the other custom elements build behind an `AnyElement`.
    pub(crate) fn wrap(&self, id: u64, percent_height_resolves: bool, child: AnyElement) -> AnyElement {
        self.wrapped.borrow_mut().insert(id);
        Measured {
            id,
            percent_height_resolves,
            measurements: self.clone(),
            child,
        }
        .into_any_element()
    }

    fn record(
        &self,
        id: u64,
        percent_height_resolves: bool,
        bounds: Bounds<Pixels>,
        layout_id: LayoutId,
        window: &mut Window,
    ) {
        let content = window.layout_content_size(layout_id);
        let measurement = Measurement {
            border_box: bounds.size,
            natural_height: natural_height(content.height, window.layout_insets(layout_id).border),
            natural_width: window.layout_natural_width(layout_id),
            parent_content: window
                .layout_parent_content_size(layout_id)
                .unwrap_or_else(|| window.viewport_size()),
            scale_factor: window.scale_factor(),
            percent_height_resolves,
        };
        self.by_element.borrow_mut().insert(id, measurement);
    }
}

struct Measured {
    id: u64,
    percent_height_resolves: bool,
    measurements: Measurements,
    child: AnyElement,
}

impl IntoElement for Measured {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Measured {
    type RequestLayoutState = LayoutId;
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
    ) -> (LayoutId, LayoutId) {
        // The child's own node, so the content and parent sizes read here are
        // the child's and the bounds painted below are its bounds.
        let layout_id = self.child.request_layout(window, cx);
        (layout_id, layout_id)
    }

    // `record` reads this wrapper's bounds, which must be the child's.
    fn escapes_to(&self) -> Option<gpui::Escape> {
        self.child.escapes_to()
    }

    // The stacking context takes the wrapper, not the child, so the wrapper must say the child's z.
    fn lift_z(&self) -> Option<i32> {
        self.child.lift_z()
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut LayoutId,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout_id: &mut LayoutId,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
        self.measurements
            .record(self.id, self.percent_height_resolves, bounds, *layout_id, window);
    }
}

/// Layout's content extent covers the padding but not the border, so the
/// border-box height is the extent plus the border on both sides.
fn natural_height(content_height: Pixels, border: Edges<Pixels>) -> Pixels {
    content_height + border.top + border.bottom
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_height_adds_both_borders_to_the_content_extent() {
        let border = Edges {
            top: gpui::px(2.0),
            bottom: gpui::px(3.0),
            ..Edges::default()
        };
        assert_eq!(natural_height(gpui::px(80.0), border), gpui::px(85.0));
        assert_eq!(natural_height(gpui::px(80.0), Edges::default()), gpui::px(80.0));
    }

    fn measurement(natural_height: f32, scale_factor: f32) -> Measurement {
        Measurement {
            border_box: gpui::size(gpui::px(40.0), gpui::px(30.0)),
            natural_height: gpui::px(natural_height),
            natural_width: None,
            parent_content: gpui::size(gpui::px(800.0), gpui::px(600.0)),
            scale_factor,
            percent_height_resolves: true,
        }
    }

    #[test]
    fn the_natural_height_rounds_up_to_the_device_pixel() {
        assert_eq!(measurement(80.2, 1.0).natural_px(AnimProp::Height), Some(81.0));
        assert_eq!(measurement(80.2, 2.0).natural_px(AnimProp::Height), Some(80.5));
        assert_eq!(measurement(80.0, 2.0).natural_px(AnimProp::Height), Some(80.0));
        assert_eq!(measurement(0.0, 2.0).natural_px(AnimProp::Height), Some(0.0));
        // f32 noise on a whole height is not a pixel of content.
        assert_eq!(measurement(80.000_01, 2.0).natural_px(AnimProp::Height), Some(80.0));
    }

    #[test]
    fn the_natural_width_rounds_up_like_the_height_and_is_missing_until_measured() {
        let wide = |natural: f32, scale_factor: f32| Measurement {
            natural_width: Some(gpui::px(natural)),
            ..measurement(10.0, scale_factor)
        };
        assert_eq!(wide(120.2, 1.0).natural_px(AnimProp::Width), Some(121.0));
        assert_eq!(wide(120.2, 2.0).natural_px(AnimProp::Width), Some(120.5));
        assert_eq!(wide(120.000_01, 2.0).natural_px(AnimProp::Width), Some(120.0));
        assert_eq!(wide(0.0, 2.0).natural_px(AnimProp::Width), Some(0.0));
        assert_eq!(measurement(10.0, 1.0).natural_px(AnimProp::Width), None);
        assert_eq!(wide(120.0, 1.0).natural_px(AnimProp::Height), Some(10.0));
        assert_eq!(wide(120.0, 1.0).natural_px(AnimProp::Opacity), None);
    }

    #[test]
    fn an_auto_resolves_only_with_the_natural_size_of_its_axis() {
        let unmeasured = measurement(10.0, 1.0);
        assert!(unmeasured.resolves(AnimProp::Height, true));
        assert!(!unmeasured.resolves(AnimProp::Width, true));
        assert!(unmeasured.resolves(AnimProp::Width, false));
        let measured = Measurement {
            natural_width: Some(gpui::px(50.0)),
            ..unmeasured
        };
        assert!(measured.resolves(AnimProp::Width, true));
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
        assert_eq!(nested.percent_px(AnimProp::Opacity, 0.5), None);
        assert_eq!(nested.percent_px(AnimProp::RowGap, 0.5), None);
    }

    #[test]
    fn a_height_percentage_with_no_definite_height_is_the_natural_height() {
        let indefinite = Measurement {
            parent_content: gpui::size(gpui::px(200.0), gpui::px(80.0)),
            percent_height_resolves: false,
            ..measurement(50.0, 1.0)
        };
        assert_eq!(indefinite.percent_px(AnimProp::Height, 0.5), Some(50.0));
        assert_eq!(indefinite.percent_px(AnimProp::Height, 1.0), Some(50.0));
        // Only `height` is auto then: widths and the other height properties keep their share.
        assert_eq!(indefinite.percent_px(AnimProp::Width, 0.5), Some(100.0));
        assert_eq!(indefinite.percent_px(AnimProp::Top, 0.5), Some(40.0));
        assert_eq!(indefinite.percent_px(AnimProp::MinHeight, 0.5), Some(40.0));
    }

    #[test]
    fn a_margin_percentage_is_a_share_of_the_width_on_every_side() {
        let nested = Measurement {
            parent_content: gpui::size(gpui::px(200.0), gpui::px(80.0)),
            ..measurement(50.0, 1.0)
        };
        for margin in [
            AnimProp::MarginTop,
            AnimProp::MarginRight,
            AnimProp::MarginBottom,
            AnimProp::MarginLeft,
        ] {
            assert_eq!(nested.percent_px(margin, 0.25), Some(50.0), "{margin:?}");
        }
    }

    #[test]
    fn an_inset_percentage_is_a_share_of_the_height_for_top_and_bottom_and_the_width_for_left_and_right() {
        let nested = Measurement {
            parent_content: gpui::size(gpui::px(200.0), gpui::px(80.0)),
            ..measurement(50.0, 1.0)
        };
        assert_eq!(nested.percent_px(AnimProp::Top, 0.5), Some(40.0));
        assert_eq!(nested.percent_px(AnimProp::Bottom, 0.25), Some(20.0));
        assert_eq!(nested.percent_px(AnimProp::Left, 0.5), Some(100.0));
        assert_eq!(nested.percent_px(AnimProp::Right, 0.25), Some(50.0));
    }

    fn some_measurement() -> Measurement {
        Measurement {
            border_box: gpui::size(gpui::px(1.0), gpui::px(2.0)),
            natural_height: gpui::px(2.0),
            natural_width: None,
            parent_content: gpui::size(gpui::px(800.0), gpui::px(600.0)),
            scale_factor: 1.0,
            percent_height_resolves: true,
        }
    }

    fn wrap_empty(measurements: &Measurements, id: u64) {
        measurements.wrap(id, true, gpui::Empty.into_any_element());
    }

    #[test]
    fn prune_drops_measurements_of_elements_that_left_the_tree() {
        let measurements = Measurements::default();
        let measurement = some_measurement();
        measurements.by_element.borrow_mut().insert(1, measurement);
        measurements.by_element.borrow_mut().insert(2, measurement);
        wrap_empty(&measurements, 1);
        wrap_empty(&measurements, 2);
        measurements.prune(|id| id == 2);
        assert_eq!(measurements.get(1), None);
        assert_eq!(measurements.get(2), Some(measurement));
    }

    #[test]
    fn prune_drops_the_measurement_of_an_element_that_was_not_wrapped() {
        let measurements = Measurements::default();
        let measurement = some_measurement();
        measurements.by_element.borrow_mut().insert(1, measurement);
        measurements.by_element.borrow_mut().insert(2, measurement);
        wrap_empty(&measurements, 1);
        measurements.prune(|_| true);
        assert_eq!(measurements.get(1), Some(measurement));
        assert_eq!(measurements.get(2), None);
    }

    #[test]
    fn a_wrap_counts_for_one_round_only() {
        let measurements = Measurements::default();
        measurements
            .by_element
            .borrow_mut()
            .insert(1, some_measurement());
        wrap_empty(&measurements, 1);
        measurements.prune(|_| true);
        assert!(measurements.get(1).is_some());
        measurements.prune(|_| true);
        assert_eq!(measurements.get(1), None);
    }
}
