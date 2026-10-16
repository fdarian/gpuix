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

use gpui::{canvas, px, App, Bounds, IntoElement, Pixels, Styled};
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use napi_derive::napi;

use crate::automation::PaintReporter;
use crate::retained_tree::RetainedTree;
use crate::renderer::{emit_event_full, EventCallback, SharedPaintReports};

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

impl ResizeSample {
    /// The content box is the border box minus the padding and border the element
    /// has now. Padding alone can move it while the border box stays put, as with
    /// a fixed `box-sizing: border-box` size.
    fn of(border_box: (f32, f32), padding: Insets, border: Insets) -> Self {
        Self {
            border: border_box,
            content: (
                (border_box.0 - padding.left - padding.right - border.left - border.right)
                    .max(0.0),
                (border_box.1 - padding.top - padding.bottom - border.top - border.bottom)
                    .max(0.0),
            ),
        }
    }

    fn is_same_as(&self, other: &Self) -> bool {
        near(self.border.0, other.border.0)
            && near(self.border.1, other.border.1)
            && near(self.content.0, other.content.0)
            && near(self.content.1, other.content.1)
    }
}

#[derive(Clone)]
struct IntersectionSample {
    present: bool,
    border: Bounds<Pixels>,
    border_insets: Insets,
    clips: Vec<(u64, Bounds<Pixels>)>,
    viewport: gpui::Size<Pixels>,
}

/// Border or padding widths of one element, in px.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
struct Insets {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Insets {
    fn of(edges: gpui::Edges<Pixels>) -> Self {
        Self {
            left: f32::from(edges.left),
            top: f32::from(edges.top),
            right: f32::from(edges.right),
            bottom: f32::from(edges.bottom),
        }
    }
}

/// An ancestor that clips its descendants, as gpui's `overflow_mask` does.
struct ClippingAncestor {
    id: u64,
    clip_x: bool,
    clip_y: bool,
}

/// Only the ancestors that reach `id` clip it: an absolute element is clipped by its containing
/// block and what clips that, a fixed one by nothing.
fn clipping_ancestors(tree: &RetainedTree, id: u64) -> Vec<ClippingAncestor> {
    let mut found = Vec::new();
    for ancestor in crate::layout::containing_block::reaching_ancestors(tree, id) {
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
            });
        }
    }
    found
}

/// The clip edge is the padding box: the border is outside it. The border is the
/// one the ancestor painted with, so a state style that changes it moves the clip.
fn clip_rects(ancestors: &[ClippingAncestor]) -> Vec<(u64, Bounds<Pixels>)> {
    ancestors
        .iter()
        .filter_map(|ancestor| {
            let painted = crate::automation::get_bounds(ancestor.id)?;
            let (x, y) = (painted.x as f32, painted.y as f32);
            let (width, height) = (painted.width as f32, painted.height as f32);
            let (left, right) = if ancestor.clip_x {
                (
                    x + painted.client_left as f32,
                    x + width - painted.border_right as f32,
                )
            } else {
                (-UNCLIPPED, UNCLIPPED)
            };
            let (top, bottom) = if ancestor.clip_y {
                (
                    y + painted.client_top as f32,
                    y + height - painted.border_bottom as f32,
                )
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

/// The reporter for an element that has an observer listener, or `None` for
/// every other element.
pub(crate) fn reporter(
    id: u64,
    events: &HashSet<String>,
    tree: &RetainedTree,
    reports: &SharedPaintReports,
    callback: &Option<EventCallback>,
) -> Option<PaintReporter> {
    let resize = events.contains(RESIZE_EVENT);
    let intersection = events.contains(INTERSECTION_EVENT);
    if !resize && !intersection {
        return None;
    }
    let callback = callback.clone()?;
    let reports = reports.clone();
    let ancestors = if intersection {
        clipping_ancestors(tree, id)
    } else {
        Vec::new()
    };
    Some(Rc::new(move |bounds, layout_insets, window, cx| {
        let viewport = window.viewport_size();
        let padding = Insets::of(layout_insets.padding);
        let border = Insets::of(layout_insets.border);
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
    let sample = ResizeSample::of(size_of(bounds), padding, border);
    {
        let mut reports = reports.borrow_mut();
        let last = reports.observers.resize.entry(id).or_default();
        if last.is_same_as(&sample) {
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

    use gpui::LayoutInsets;

    use super::*;
    use crate::style::StyleDesc;

    fn insets(width: f32) -> Insets {
        Insets { left: width, top: width, right: width, bottom: width }
    }

    #[test]
    fn the_content_box_is_the_border_box_minus_padding_and_border() {
        let sample = ResizeSample::of((100.0, 60.0), insets(10.0), insets(2.0));
        assert_eq!(sample.content, (76.0, 36.0));
        let squeezed = ResizeSample::of((10.0, 10.0), insets(10.0), insets(2.0));
        assert_eq!(squeezed.content, (0.0, 0.0));
    }

    #[test]
    fn padding_alone_is_a_change_while_the_border_box_stays_put() {
        let rest = ResizeSample::of((100.0, 60.0), insets(4.0), insets(0.0));
        let hovered = ResizeSample::of((100.0, 60.0), insets(10.0), insets(0.0));
        assert!(!rest.is_same_as(&hovered));
        assert!(rest.is_same_as(&ResizeSample::of((100.0, 60.0), insets(4.0), insets(0.0))));
    }

    #[test]
    fn an_ancestor_clips_at_the_border_it_painted_with() {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        tree.create_element(2, "div".to_string());
        tree.append_child(1, 2);
        tree.set_style(
            1,
            Arc::new(StyleDesc {
                border_width: Some(2.0),
                overflow: Some("hidden".into()),
                ..StyleDesc::default()
            }),
        );
        let bounds = gpui::Bounds {
            origin: gpui::point(px(10.0), px(20.0)),
            size: gpui::size(px(100.0), px(80.0)),
        };
        // The element's style says 2px; a state style made it 6px at paint.
        crate::automation::record_bounds(
            1,
            bounds,
            LayoutInsets {
                padding: gpui::Edges::default(),
                border: gpui::Edges::all(px(6.0)),
            },
        );

        let ancestors = clipping_ancestors(&tree, 2);
        assert_eq!(ancestors.len(), 1);
        let clips = clip_rects(&ancestors);
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].0, 1);
        assert_eq!(
            clips[0].1,
            gpui::Bounds {
                origin: gpui::point(px(16.0), px(26.0)),
                size: gpui::size(px(88.0), px(68.0)),
            }
        );
    }
}
