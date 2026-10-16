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

use crate::retained_tree::RetainedTree;
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
    /// False when the element did not paint this frame (a virtual list row outside
    /// its window, removed). Every number is then 0. A `visibility: hidden`
    /// element keeps its box and is present.
    pub present: bool,
    /// Size of the content box: the border box minus border and padding.
    pub content_width: f64,
    pub content_height: f64,
    /// Padding on the left and top, which is where the content box starts
    /// inside the padding box (`ResizeObserverEntry.contentRect.x` and `.y`).
    pub padding_left: f64,
    pub padding_top: f64,
    /// The border widths of an intersection report. The padding box, which
    /// is what an `IntersectionObserver` root exposes, is the border box minus
    /// these.
    pub border_left: f64,
    pub border_top: f64,
    pub border_right: f64,
    pub border_bottom: f64,
    /// The `overflow` clips of the ancestors, nearest first, in window
    /// coordinates. JS applies the ones below its root. The window edge is not
    /// one of them: an `IntersectionObserver` with a `rootMargin` sees past it.
    pub clips: Vec<ObservationClip>,
    /// The window's paintable size, which is the viewport of an
    /// `IntersectionObserver` with no `root`.
    pub viewport_width: f64,
    pub viewport_height: f64,
}

/// One ancestor's clip rect. An axis the ancestor does not clip spans
/// `UNCLIPPED` either way.
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
pub struct ObservationClip {
    /// The clipping ancestor's element id.
    pub id: f64,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

const UNCLIPPED: f32 = 1.0e9;

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

#[derive(Clone)]
struct IntersectionSample {
    present: bool,
    border: Bounds<Pixels>,
    border_insets: Insets,
    clips: Vec<(u64, Bounds<Pixels>)>,
    viewport: gpui::Size<Pixels>,
}

/// Border and padding of one element, in px, from its own style.
///
/// Mirrors how `apply_styles` reads `padding*` and `border*Width`. Not
/// included: `hover` and `active` overrides, which change at paint time.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
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

/// An ancestor that clips its descendants, as gpui's `overflow_mask` does.
struct ClippingAncestor {
    id: u64,
    clip_x: bool,
    clip_y: bool,
    /// The clip edge is the padding box: the border is outside it.
    border: Insets,
}

fn clipping_ancestors(tree: &RetainedTree, id: u64) -> Vec<ClippingAncestor> {
    let mut found = Vec::new();
    let mut next = tree.elements.get(&id).and_then(|element| element.parent);
    while let Some(ancestor) = next {
        let Some(element) = tree.elements.get(&ancestor) else {
            break;
        };
        let style = element.style.as_deref();
        let (mut clip_x, mut clip_y) = style.map_or((false, false), |style| {
            let axes = crate::scroll::Axes::of(style);
            (axes.x.clips(), axes.y.clips())
        });
        if element.element_type == "virtual-list" {
            clip_x = true;
            clip_y = true;
        }
        if clip_x || clip_y {
            found.push(ClippingAncestor {
                id: ancestor,
                clip_x,
                clip_y,
                border: insets(style).1,
            });
        }
        next = element.parent;
    }
    found
}

fn clip_rects(ancestors: &[ClippingAncestor]) -> Vec<(u64, Bounds<Pixels>)> {
    ancestors
        .iter()
        .filter_map(|ancestor| {
            let painted = crate::automation::get_bounds(ancestor.id)?;
            let (x, y) = (painted.x as f32, painted.y as f32);
            let (width, height) = (painted.width as f32, painted.height as f32);
            let (left, right) = if ancestor.clip_x {
                (x + ancestor.border.left, x + width - ancestor.border.right)
            } else {
                (-UNCLIPPED, UNCLIPPED)
            };
            let (top, bottom) = if ancestor.clip_y {
                (y + ancestor.border.top, y + height - ancestor.border.bottom)
            } else {
                (-UNCLIPPED, UNCLIPPED)
            };
            Some((
                ancestor.id,
                Bounds {
                    origin: gpui::point(px(left), px(top)),
                    size: gpui::size(px((right - left).max(0.0)), px((bottom - top).max(0.0))),
                },
            ))
        })
        .collect()
}

pub(crate) type ObserverReporter = Rc<dyn Fn(Bounds<Pixels>, &mut Window, &mut App)>;

/// The reporter for an element that has an observer listener, or `None` for
/// every other element.
pub(crate) fn reporter(
    id: u64,
    events: &HashSet<String>,
    style: Option<&StyleDesc>,
    tree: &RetainedTree,
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
    let ancestors = if intersection {
        clipping_ancestors(tree, id)
    } else {
        Vec::new()
    };
    Some(Rc::new(move |bounds, window, cx| {
        let viewport = window.viewport_size();
        if resize {
            report_resize(id, bounds, padding, border, &reports, &callback, cx);
        }
        if intersection {
            let clips = clip_rects(&ancestors);
            report_intersection(id, bounds, border, clips, viewport, &reports, &callback, cx);
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

#[allow(clippy::too_many_arguments)]
fn report_intersection(
    id: u64,
    bounds: Bounds<Pixels>,
    border: Insets,
    clips: Vec<(u64, Bounds<Pixels>)>,
    viewport: gpui::Size<Pixels>,
    reports: &SharedPaintReports,
    callback: &EventCallback,
    cx: &mut App,
) {
    {
        let mut reports = reports.borrow_mut();
        let unchanged = reports.observers.intersection.get(&id).is_some_and(|last| {
            last.present
                && same_bounds(last.border, bounds)
                && last.border_insets == border
                && last.clips.len() == clips.len()
                && last
                    .clips
                    .iter()
                    .zip(&clips)
                    .all(|(left, right)| left.0 == right.0 && same_bounds(left.1, right.1))
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
                border_insets: border,
                clips: clips.clone(),
                viewport,
            },
        );
    }
    let observation = Observation {
        present: true,
        border_left: f64::from(border.left),
        border_top: f64::from(border.top),
        border_right: f64::from(border.right),
        border_bottom: f64::from(border.bottom),
        clips: clips
            .iter()
            .map(|(id, rect)| ObservationClip {
                id: *id as f64,
                x: f64::from(f32::from(rect.origin.x)),
                y: f64::from(f32::from(rect.origin.y)),
                width: f64::from(f32::from(rect.size.width)),
                height: f64::from(f32::from(rect.size.height)),
            })
            .collect(),
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
///
/// An element that no longer exists keeps its sample until `sweep` has reported
/// it gone, because a destroyed element is one that stopped painting.
pub(crate) fn prune(
    reports: &mut ObserverReports,
    exists: impl Fn(u64) -> bool,
    has_listener: impl Fn(u64, &str) -> bool,
) {
    reports.resize.retain(|id, last| {
        if exists(*id) {
            has_listener(*id, RESIZE_EVENT)
        } else {
            *last != ResizeSample::default()
        }
    });
    reports.intersection.retain(|id, last| {
        if exists(*id) {
            has_listener(*id, INTERSECTION_EVENT)
        } else {
            last.present
        }
    });
}

/// A zero-size child that runs once per frame, after the frame has painted,
/// and reports the observed elements that did not paint: a virtual list row
/// outside its window, an element that was removed. Nothing paints for them, so
/// only a sweep can tell they went away.
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn border_insets_read_the_shorthand_and_the_per_side_override() {
        let style = StyleDesc {
            border_width: Some(10.0),
            border_left_width: Some(2.0),
            padding: Some(4.0),
            ..StyleDesc::default()
        };
        let (padding, border) = insets(Some(&style));
        assert_eq!(
            border,
            Insets { left: 2.0, top: 10.0, right: 10.0, bottom: 10.0 }
        );
        assert_eq!(padding, Insets { left: 4.0, top: 4.0, right: 4.0, bottom: 4.0 });
    }

    #[test]
    fn an_ancestor_clips_at_the_padding_box_whatever_its_border_colour() {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        tree.create_element(2, "div".to_string());
        tree.append_child(1, 2);
        tree.set_style(
            1,
            Arc::new(StyleDesc {
                border_width: Some(6.0),
                overflow: Some("hidden".into()),
                ..StyleDesc::default()
            }),
        );

        let ancestors = clipping_ancestors(&tree, 2);
        assert_eq!(ancestors.len(), 1);
        assert_eq!(
            ancestors[0].border,
            Insets { left: 6.0, top: 6.0, right: 6.0, bottom: 6.0 }
        );
    }
}
