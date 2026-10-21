//! Native half of `ResizeObserver` and `IntersectionObserver`, and `onLayout`.
//!
//! JS asks for an observation by setting the `resizeObserve` or
//! `intersectObserve` listener on an element, like any other event. What the
//! observers need is read from the boxes the frame published (the registry
//! `automation::get_bounds` reads), by [`frame_sweep`] after the draw, and the JS
//! side (`js/observers.ts`) applies the DOM rules: thresholds, `rootMargin`, the
//! `box` option, batching.
//!
//! Reading the registry and not listening to each element's paint is what lets an
//! observed element sit in a subtree that a frame replays: a replay publishes the
//! same boxes the build did (see `tape`), and nothing about the element has to run.
//!
//! Reports are emitted after the draw that saw the change, never from inside
//! it: a handler that sets state would re-enter the frame being painted. A frame's reports
//! go out in the order of the DOM's rendering steps: the frame's scroll events, then
//! `onLayout` and resize reports, then intersection reports (see `sweep`).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use gpui::{canvas, px, App, Bounds, IntoElement, Pixels, Styled, WindowId};
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use napi_derive::napi;

use crate::automation::ElementBounds;
use crate::renderer::{emit_event_full, EventCallback, SharedPaintReports};
use crate::retained_tree::RetainedTree;

pub(crate) const LAYOUT_EVENT: &str = "layout";
pub(crate) const RESIZE_EVENT: &str = "resizeObserve";
pub(crate) const INTERSECTION_EVENT: &str = "intersectObserve";

/// Whether a listener of this type is answered from the published boxes.
pub(crate) fn reports_geometry(event: &str) -> bool {
    matches!(event, LAYOUT_EVENT | RESIZE_EVENT | INTERSECTION_EVENT)
}

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

/// What `onLayout` and the observers of every element last reported, so a draw that changed
/// nothing queues nothing.
#[derive(Default)]
pub(crate) struct ObserverReports {
    /// Border-box size last delivered through `onLayout`.
    layout: HashMap<u64, (f32, f32)>,
    resize: HashMap<u64, ResizeSample>,
    intersection: HashMap<u64, IntersectionSample>,
    /// The box an inline element with an `IntersectionObserver` paints in, as the build that
    /// made its paragraph saw it. The tree cannot say: which ancestors are inline boxes is up to
    /// the paragraph planner.
    clip_origins: HashMap<u64, u64>,
    /// Moves when `clip_origins` changes, for `targets`.
    origins_epoch: u64,
    targets: Option<Targets>,
}

impl ObserverReports {
    pub(crate) fn is_observing(&self) -> bool {
        !self.layout.is_empty() || !self.resize.is_empty() || !self.intersection.is_empty()
    }

    /// Says where `id`, which a build has just made, paints, for its clipping ancestors: `None`
    /// for an element with a box of its own, the container of its paragraph for an inline one.
    pub(crate) fn note_clip_origin(&mut self, id: u64, events: &HashSet<String>, origin: Option<u64>) {
        let origin = origin.filter(|_| events.contains(INTERSECTION_EVENT));
        let changed = match origin {
            Some(origin) => self.clip_origins.insert(id, origin) != Some(origin),
            None => self.clip_origins.remove(&id).is_some(),
        };
        if changed {
            self.origins_epoch += 1;
        }
    }

    /// The targets of the tree as it is, which only change when the tree does.
    fn targets(&mut self, tree: &RetainedTree) -> Targets {
        let stamp = (tree.id(), tree.revision(), self.origins_epoch);
        if let Some(targets) = self.targets.take_if(|targets| targets.stamp == stamp) {
            return targets;
        }
        let list = tree
            .geometry_ids
            .iter()
            .filter_map(|id| {
                let events = tree.elements.get(id)?.events();
                let intersection = events.contains(INTERSECTION_EVENT);
                Some(Target {
                    id: *id,
                    layout: events.contains(LAYOUT_EVENT),
                    resize: events.contains(RESIZE_EVENT),
                    clips: intersection
                        .then(|| clipping_ancestors(tree, *id, self.clip_origins.get(id).copied())),
                })
            })
            .collect();
        Targets { stamp, list }
    }
}

/// Which elements are read for their boxes, and the ancestors that clip each one.
struct Targets {
    /// The tree, its revision and `ObserverReports::origins_epoch` these were made from.
    stamp: (u64, u64, u64),
    list: Vec<Target>,
}

struct Target {
    id: u64,
    layout: bool,
    resize: bool,
    /// `Some` for an element with an `IntersectionObserver`.
    clips: Option<Vec<ClippingAncestor>>,
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

/// An ancestor that clips its descendants, as gpui's `overflow_mask` does.
struct ClippingAncestor {
    id: u64,
    clip_x: bool,
    clip_y: bool,
}

/// Only the ancestors that reach `id` clip it: an absolute element is clipped by its containing
/// block and what clips that, a fixed one by nothing. `first` is the box `id` paints in when that
/// is not its parent, an inline element's paragraph container, which clips it and what reaches it.
fn clipping_ancestors(tree: &RetainedTree, id: u64, first: Option<u64>) -> Vec<ClippingAncestor> {
    let mut found = Vec::new();
    // The ancestors were built before this element, so their resolved style is this frame's.
    let reaching = match first {
        None => crate::layout::containing_block::reaching_ancestors(tree, id),
        Some(first) => std::iter::once(first)
            .chain(crate::layout::containing_block::reaching_ancestors(tree, first))
            .collect(),
    };
    for ancestor in reaching {
        let Some(element) = tree.elements.get(&ancestor) else {
            break;
        };
        let axes = tree.overflow_of(element);
        let (mut clip_x, mut clip_y) = (axes.x.clips(), axes.y.clips());
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
fn clip_rects(window: WindowId, ancestors: &[ClippingAncestor]) -> Vec<(u64, Bounds<Pixels>)> {
    ancestors
        .iter()
        .filter_map(|ancestor| {
            let painted = crate::automation::get_bounds(window, ancestor.id)?;
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

/// The border box a published element has, which is what the payload of every report carries.
fn border_box(painted: &ElementBounds) -> Bounds<Pixels> {
    Bounds {
        origin: gpui::point(px(painted.x as f32), px(painted.y as f32)),
        size: gpui::size(px(painted.width as f32), px(painted.height as f32)),
    }
}

fn padding_of(painted: &ElementBounds) -> Insets {
    Insets {
        left: painted.padding_left as f32,
        top: painted.padding_top as f32,
        right: painted.padding_right as f32,
        bottom: painted.padding_bottom as f32,
    }
}

fn border_of(painted: &ElementBounds) -> Insets {
    Insets {
        left: painted.client_left as f32,
        top: painted.client_top as f32,
        right: painted.border_right as f32,
        bottom: painted.border_bottom as f32,
    }
}

/// One event to emit. `kind` orders the events of one element: `onLayout`, then the observers.
struct Report {
    id: u64,
    kind: u8,
    event_type: &'static str,
    bounds: Bounds<Pixels>,
    observation: Option<Observation>,
}

fn report_layout(
    reports: &mut ObserverReports,
    id: u64,
    bounds: Bounds<Pixels>,
) -> Option<Report> {
    let size = size_of(bounds);
    let unchanged = reports
        .layout
        .get(&id)
        .is_some_and(|last| near(last.0, size.0) && near(last.1, size.1));
    if unchanged {
        return None;
    }
    reports.layout.insert(id, size);
    Some(Report { id, kind: 0, event_type: LAYOUT_EVENT, bounds, observation: None })
}

fn report_resize(
    reports: &mut ObserverReports,
    id: u64,
    bounds: Bounds<Pixels>,
    padding: Insets,
    border: Insets,
) -> Option<Report> {
    let sample = ResizeSample::of(size_of(bounds), padding, border);
    let last = reports.resize.entry(id).or_default();
    if last.is_same_as(&sample) {
        return None;
    }
    *last = sample;
    let observation = Observation {
        present: true,
        content_width: f64::from(sample.content.0),
        content_height: f64::from(sample.content.1),
        padding_left: f64::from(padding.left),
        padding_top: f64::from(padding.top),
        ..Observation::default()
    };
    Some(Report { id, kind: 1, event_type: RESIZE_EVENT, bounds, observation: Some(observation) })
}

fn report_intersection(
    reports: &mut ObserverReports,
    id: u64,
    bounds: Bounds<Pixels>,
    border: Insets,
    clips: Vec<(u64, Bounds<Pixels>)>,
    viewport: gpui::Size<Pixels>,
) -> Option<Report> {
    let unchanged = reports.intersection.get(&id).is_some_and(|last| {
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
        return None;
    }
    reports.intersection.insert(
        id,
        IntersectionSample {
            present: true,
            border: bounds,
            border_insets: border,
            clips: clips.clone(),
            viewport,
        },
    );
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
    Some(Report { id, kind: 2, event_type: INTERSECTION_EVENT, bounds, observation: Some(observation) })
}

fn emit(callback: &Option<EventCallback>, report: Report) {
    emit_event_full(callback, report.id, report.event_type, |payload| {
        payload.x = Some(f64::from(f32::from(report.bounds.origin.x)));
        payload.y = Some(f64::from(f32::from(report.bounds.origin.y)));
        payload.width = Some(f64::from(f32::from(report.bounds.size.width)));
        payload.height = Some(f64::from(f32::from(report.bounds.size.height)));
        payload.observation = report.observation;
    });
}

/// `Report::kind` of an intersection report: the last phase of a frame.
const INTERSECTION_KIND: u8 = 2;

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
    reports.layout.retain(|id, _| has_listener(*id, LAYOUT_EVENT));
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
    let before = reports.clip_origins.len();
    reports.clip_origins.retain(|id, _| has_listener(*id, INTERSECTION_EVENT));
    if reports.clip_origins.len() != before {
        reports.origins_epoch += 1;
    }
}

/// A zero-size child that reports, after each draw, what `onLayout` and the observers of the
/// elements that listen see in the boxes the draw published, and the observed elements that
/// did not paint: a virtual list row outside its window, an element that was removed. Nothing
/// paints for them, so only a sweep can tell they went away.
///
/// The reads are deferred until the draw is over, when every box is in the registry. A pass
/// that only lays out publishes the same registry at its end (see `automation::bounds_frame_reset`),
/// and reports what is there; it does not report an element gone, because a bounds read
/// is not a promise that everything painted.
///
/// `active` is whether anything listens, so a page without observers pays nothing.
pub(crate) fn frame_sweep(
    reports: SharedPaintReports,
    tree: Arc<Mutex<RetainedTree>>,
    callback: Option<EventCallback>,
    active: bool,
) -> impl IntoElement {
    let sweep_after = move |painted: bool| {
        let reports = reports.clone();
        let tree = tree.clone();
        let callback = callback.clone();
        move |window: &mut gpui::Window, cx: &mut App| {
            if !active {
                return;
            }
            let id = window.window_handle().window_id();
            let viewport = window.viewport_size();
            cx.defer(move |_| sweep(id, viewport, painted, &reports, &tree, &callback));
        }
    };
    let after_layout = sweep_after(false);
    let after_paint = sweep_after(true);
    canvas(
        move |_, window, cx| {
            if window.is_layout_pass() {
                after_layout(window, cx);
            }
        },
        move |_, _, window, cx| after_paint(window, cx),
    )
    .absolute()
    .w(px(0.0))
    .h(px(0.0))
}

fn sweep(
    window: WindowId,
    viewport: gpui::Size<Pixels>,
    painted: bool,
    reports: &SharedPaintReports,
    tree: &Mutex<RetainedTree>,
    callback: &Option<EventCallback>,
) {
    if callback.is_none() {
        return;
    }
    // The scrolls of the frame come first, as the DOM runs its scroll steps before the observers.
    crate::renderer::emit_pending_scrolls(reports, callback);
    let tree = tree.lock().unwrap();
    let mut shared = reports.borrow_mut();
    let observers = &mut shared.observers;
    let targets = observers.targets(&tree);
    let mut found = Vec::new();
    for target in &targets.list {
        let Some(published) = crate::automation::get_bounds(window, target.id) else {
            continue;
        };
        let bounds = border_box(&published);
        if target.layout {
            found.extend(report_layout(observers, target.id, bounds));
        }
        if target.resize {
            found.extend(report_resize(observers, target.id, bounds, padding_of(&published), border_of(&published)));
        }
        if let Some(ancestors) = &target.clips {
            let clips = clip_rects(window, ancestors);
            found.extend(report_intersection(observers, target.id, bounds, border_of(&published), clips, viewport));
        }
    }
    observers.targets = Some(targets);
    // Sorted into the order of the DOM's rendering steps: `onLayout` and `ResizeObserver`, then
    // `IntersectionObserver`; in document order within them, `onLayout` before the observers of
    // one element. The JS side puts the callbacks of the observers in the order of the DOM
    // (creation order of the observers, observation order of their targets): this is the order
    // the elements are told in, which `onLayout` handlers see as it is.
    if found.len() > 1 {
        found.sort_by_cached_key(|report| {
            (report.kind == INTERSECTION_KIND, document_order(&tree, report.id), report.kind)
        });
    }

    let mut gone_resize = Vec::new();
    let mut gone_intersection = Vec::new();
    if painted {
        for (id, last) in observers.resize.iter_mut() {
            if crate::automation::get_bounds(window, *id).is_none() && *last != ResizeSample::default() {
                *last = ResizeSample::default();
                gone_resize.push(*id);
            }
        }
        for (id, last) in observers.intersection.iter_mut() {
            if last.present && crate::automation::get_bounds(window, *id).is_none() {
                last.present = false;
                gone_intersection.push(*id);
            }
        }
    }
    gone_resize.sort_unstable();
    gone_intersection.sort_unstable();
    drop(shared);
    drop(tree);
    let nothing = Bounds::<Pixels>::default();
    let gone = |ids: Vec<u64>, event_type, kind| {
        ids.into_iter().map(move |id| Report {
            id,
            kind,
            event_type,
            bounds: nothing,
            observation: Some(Observation::default()),
        })
    };
    // A removed element is reported after the ones that are there, in the phase of its observer.
    let (resizing, intersecting): (Vec<_>, Vec<_>) = found.into_iter().partition(|report| report.kind != INTERSECTION_KIND);
    for report in resizing
        .into_iter()
        .chain(gone(gone_resize, RESIZE_EVENT, 1))
        .chain(intersecting)
        .chain(gone(gone_intersection, INTERSECTION_EVENT, INTERSECTION_KIND))
    {
        emit(callback, report);
    }
}

/// Where `id` is in the document: the index of each ancestor and of the element among its
/// siblings, outermost first, which sorts the way the tree is walked.
fn document_order(tree: &RetainedTree, id: u64) -> Vec<usize> {
    let mut path = Vec::new();
    let mut current = id;
    while let Some(parent) = tree.elements.get(&current).and_then(|element| element.parent) {
        let index = tree
            .elements
            .get(&parent)
            .and_then(|parent| parent.children.iter().position(|child| *child == current));
        path.push(index.expect("an element is among the children of its parent"));
        current = parent;
    }
    path.reverse();
    path
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
                border_width: Some(crate::css::shorthand::Sides::all(crate::css::length::LinearLength::px(2.0))),
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
            gpui::WindowId::from(1),
            1,
            crate::automation::LaidOut::of_box(bounds, 1.0),
            bounds.origin,
            LayoutInsets {
                padding: gpui::Edges::default(),
                border: gpui::Edges::all(px(6.0)),
            },
        );

        let ancestors = clipping_ancestors(&tree, 2, None);
        assert_eq!(ancestors.len(), 1);
        let clips = clip_rects(gpui::WindowId::from(1), &ancestors);
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
