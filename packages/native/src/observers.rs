//! Native half of `ResizeObserver` and `IntersectionObserver`.
//!
//! JS asks for an observation by setting the `resizeObserve` or
//! `intersectObserve` listener on an element, like any other event. The paint
//! of that element then reports what the observers need, and the JS side
//! (`js/observers.ts`) applies the DOM rules: thresholds, `rootMargin`, the
//! `box` option, batching.
//!
//! Reports are queued after the paint that saw the change, never from inside
//! it, for the same reason as `onLayout`: a handler that sets state would
//! re-enter the frame being painted.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui::{canvas, px, App, Bounds, IntoElement, Pixels, Styled, Window};
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use napi_derive::napi;

use crate::renderer::{emit_event_full, EventCallback, SharedPaintReports};
use crate::style::StyleDesc;

pub(crate) const RESIZE_EVENT: &str = "resizeObserve";
pub(crate) const INTERSECTION_EVENT: &str = "intersectObserve";

/// Values closer than this are the same value; absorbs float noise from
/// fractional scale factors.
const EPSILON: f32 = 0.01;

/// What one observation event carries beyond the element's border box, which
/// travels in the payload's `x`, `y`, `width` and `height`.
#[derive(Debug, Clone, Default)]
#[cfg_attr(
    all(target_arch = "wasm32", target_os = "unknown"),
    derive(serde::Serialize)
)]
#[cfg_attr(
    all(target_arch = "wasm32", target_os = "unknown"),
    serde(rename_all = "camelCase")
)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct Observation {
    /// False when the element did not paint this frame (`visibility: hidden`, a
    /// virtual list row outside its window, removed). Every number is then 0.
    pub present: bool,
    /// Size of the content box: the border box minus border and padding.
    pub content_width: f64,
    pub content_height: f64,
    /// Padding on the left and top, which is where the content box starts
    /// inside the padding box (`ResizeObserverEntry.contentRect.x` and `.y`).
    pub padding_left: f64,
    pub padding_top: f64,
    /// The border box clipped by every ancestor's `overflow` and by the
    /// window, in window coordinates. Zero-sized when clipped away entirely.
    pub visible_x: f64,
    pub visible_y: f64,
    pub visible_width: f64,
    pub visible_height: f64,
    /// The window's paintable size, which is the viewport of an
    /// `IntersectionObserver` with no `root`.
    pub viewport_width: f64,
    pub viewport_height: f64,
}

/// What the observers of every element last reported, so a repaint that changed
/// nothing queues nothing.
#[derive(Default)]
pub(crate) struct ObserverReports {
    resize: HashMap<u64, ResizeSample>,
    intersection: HashMap<u64, IntersectionSample>,
}

impl ObserverReports {
    fn is_observing(&self) -> bool {
        !self.resize.is_empty() || !self.intersection.is_empty()
    }
}

/// Starts at zero like the DOM's last reported size: an element that is 0 by 0
/// when first seen has nothing to report.
#[derive(Default, Clone, Copy, PartialEq)]
struct ResizeSample {
    border: (f32, f32),
    content: (f32, f32),
}

#[derive(Clone, Copy)]
struct IntersectionSample {
    present: bool,
    border: Bounds<Pixels>,
    visible: Bounds<Pixels>,
    viewport: gpui::Size<Pixels>,
}

/// Border and padding of one element, in px, from its own style.
///
/// Mirrors how `apply_styles` reads `padding*` and `border*Width`. Not
/// included: `hover` and `active` overrides, which change at paint time.
#[derive(Clone, Copy, Default)]
struct Insets {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

fn insets(style: Option<&StyleDesc>) -> (Insets, Insets) {
    let Some(style) = style else {
        return (Insets::default(), Insets::default());
    };
    let side = |all: Option<f64>, one: Option<f64>| one.or(all).unwrap_or(0.0).max(0.0) as f32;
    let padding = Insets {
        left: side(style.padding, style.padding_left),
        top: side(style.padding, style.padding_top),
        right: side(style.padding, style.padding_right),
        bottom: side(style.padding, style.padding_bottom),
    };
    let border = Insets {
        left: side(style.border_width, style.border_left_width),
        top: side(style.border_width, style.border_top_width),
        right: side(style.border_width, style.border_right_width),
        bottom: side(style.border_width, style.border_bottom_width),
    };
    (padding, border)
}

pub(crate) type ObserverReporter = Rc<dyn Fn(Bounds<Pixels>, &mut Window, &mut App)>;

/// The reporter for an element that has an observer listener, or `None` for
/// every other element.
pub(crate) fn reporter(
    id: u64,
    events: &HashSet<String>,
    style: Option<&StyleDesc>,
    reports: &SharedPaintReports,
    callback: &Option<EventCallback>,
) -> Option<ObserverReporter> {
    let resize = events.contains(RESIZE_EVENT);
    let intersection = events.contains(INTERSECTION_EVENT);
    if !resize && !intersection {
        return None;
    }
    let callback = callback.clone()?;
    let reports = reports.clone();
    let (padding, border) = insets(style);
    Some(Rc::new(move |bounds, window, cx| {
        let viewport = window.viewport_size();
        if resize {
            report_resize(id, bounds, padding, border, &reports, &callback, cx);
        }
        if intersection {
            let mask = window.content_mask().bounds;
            report_intersection(id, bounds, mask, viewport, &reports, &callback, cx);
        }
    }))
}

fn size_of(bounds: Bounds<Pixels>) -> (f32, f32) {
    (f32::from(bounds.size.width), f32::from(bounds.size.height))
}

fn near(left: f32, right: f32) -> bool {
    (left - right).abs() < EPSILON
}

fn same_bounds(left: Bounds<Pixels>, right: Bounds<Pixels>) -> bool {
    near(f32::from(left.origin.x), f32::from(right.origin.x))
        && near(f32::from(left.origin.y), f32::from(right.origin.y))
        && near(f32::from(left.size.width), f32::from(right.size.width))
        && near(f32::from(left.size.height), f32::from(right.size.height))
}

/// `a` clipped to `b`. Empty, not negative, when they do not overlap.
fn clip(a: Bounds<Pixels>, b: Bounds<Pixels>) -> Bounds<Pixels> {
    let left = a.origin.x.max(b.origin.x);
    let top = a.origin.y.max(b.origin.y);
    let right = a.right().min(b.right());
    let bottom = a.bottom().min(b.bottom());
    Bounds {
        origin: gpui::point(left, top),
        size: gpui::size((right - left).max(px(0.0)), (bottom - top).max(px(0.0))),
    }
}

fn report_resize(
    id: u64,
    bounds: Bounds<Pixels>,
    padding: Insets,
    border: Insets,
    reports: &SharedPaintReports,
    callback: &EventCallback,
    cx: &mut App,
) {
    let border_box = size_of(bounds);
    let sample = ResizeSample {
        border: border_box,
        content: (
            (border_box.0 - padding.left - padding.right - border.left - border.right).max(0.0),
            (border_box.1 - padding.top - padding.bottom - border.top - border.bottom).max(0.0),
        ),
    };
    {
        let mut reports = reports.borrow_mut();
        let last = reports.observers.resize.entry(id).or_default();
        if near(last.border.0, sample.border.0)
            && near(last.border.1, sample.border.1)
            && near(last.content.0, sample.content.0)
            && near(last.content.1, sample.content.1)
        {
            return;
        }
        *last = sample;
    }
    let observation = Observation {
        present: true,
        content_width: f64::from(sample.content.0),
        content_height: f64::from(sample.content.1),
        padding_left: f64::from(padding.left),
        padding_top: f64::from(padding.top),
        ..Observation::default()
    };
    queue(cx, callback, id, RESIZE_EVENT, bounds, observation);
}

fn report_intersection(
    id: u64,
    bounds: Bounds<Pixels>,
    mask: Bounds<Pixels>,
    viewport: gpui::Size<Pixels>,
    reports: &SharedPaintReports,
    callback: &EventCallback,
    cx: &mut App,
) {
    let visible = clip(bounds, mask);
    {
        let mut reports = reports.borrow_mut();
        let unchanged = reports.observers.intersection.get(&id).is_some_and(|last| {
            last.present
                && same_bounds(last.border, bounds)
                && same_bounds(last.visible, visible)
                && near(f32::from(last.viewport.width), f32::from(viewport.width))
                && near(f32::from(last.viewport.height), f32::from(viewport.height))
        });
        if unchanged {
            return;
        }
        reports.observers.intersection.insert(
            id,
            IntersectionSample {
                present: true,
                border: bounds,
                visible,
                viewport,
            },
        );
    }
    let observation = Observation {
        present: true,
        visible_x: f64::from(f32::from(visible.origin.x)),
        visible_y: f64::from(f32::from(visible.origin.y)),
        visible_width: f64::from(f32::from(visible.size.width)),
        visible_height: f64::from(f32::from(visible.size.height)),
        viewport_width: f64::from(f32::from(viewport.width)),
        viewport_height: f64::from(f32::from(viewport.height)),
        ..Observation::default()
    };
    queue(cx, callback, id, INTERSECTION_EVENT, bounds, observation);
}

fn queue(
    cx: &mut App,
    callback: &EventCallback,
    id: u64,
    event_type: &'static str,
    bounds: Bounds<Pixels>,
    observation: Observation,
) {
    let callback = Some(callback.clone());
    cx.defer(move |_| {
        emit_event_full(&callback, id, event_type, |payload| {
            payload.x = Some(f64::from(f32::from(bounds.origin.x)));
            payload.y = Some(f64::from(f32::from(bounds.origin.y)));
            payload.width = Some(f64::from(f32::from(bounds.size.width)));
            payload.height = Some(f64::from(f32::from(bounds.size.height)));
            payload.observation = Some(observation);
        });
    });
}

/// Drop what an element reported once it stopped listening, so adding the
/// listener back reports again like a fresh observer.
pub(crate) fn prune(
    reports: &mut ObserverReports,
    has_listener: impl Fn(u64, &str) -> bool,
) {
    reports.resize.retain(|id, _| has_listener(*id, RESIZE_EVENT));
    reports
        .intersection
        .retain(|id, _| has_listener(*id, INTERSECTION_EVENT));
}

/// A zero-size child that runs once per frame, after the frame has painted,
/// and reports the observed elements that did not paint: `visibility: hidden`, a
/// virtual list row outside its window, an element that was removed. Nothing
/// paints for them, so only a sweep can tell they went away.
///
/// Paint runs first-to-last under the root, so this must stay a child of the
/// root next to `bounds_frame_reset`; the sweep itself is deferred until the
/// draw is over, when every element that painted is in the bounds map.
pub(crate) fn frame_sweep(
    reports: SharedPaintReports,
    callback: Option<EventCallback>,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, _, cx| {
            if !reports.borrow().observers.is_observing() {
                return;
            }
            let reports = reports.clone();
            let callback = callback.clone();
            cx.defer(move |cx| sweep(&reports, &callback, cx));
        },
    )
    .absolute()
    .w(px(0.0))
    .h(px(0.0))
}

fn sweep(reports: &SharedPaintReports, callback: &Option<EventCallback>, cx: &mut App) {
    let Some(callback) = callback else {
        return;
    };
    let mut gone_resize = Vec::new();
    let mut gone_intersection = Vec::new();
    {
        let mut reports = reports.borrow_mut();
        for (id, last) in reports.observers.resize.iter_mut() {
            if crate::automation::get_bounds(*id).is_none() && *last != ResizeSample::default() {
                *last = ResizeSample::default();
                gone_resize.push(*id);
            }
        }
        for (id, last) in reports.observers.intersection.iter_mut() {
            if last.present && crate::automation::get_bounds(*id).is_none() {
                last.present = false;
                gone_intersection.push(*id);
            }
        }
    }
    let nothing = Bounds::<Pixels>::default();
    for id in gone_resize {
        queue(cx, callback, id, RESIZE_EVENT, nothing, Observation::default());
    }
    for id in gone_intersection {
        queue(cx, callback, id, INTERSECTION_EVENT, nothing, Observation::default());
    }
}
