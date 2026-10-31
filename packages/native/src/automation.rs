//! Shared automation host: element bounds and a controllable motion clock.
//!
//! The box recorded is the one layout computed (fractional), which only prepaint can ask for, so
//! [`capture_layout_box`] keeps it for the paint listener that records it.
//!
//! Record bounds during **paint**, not prepaint. The frame reset canvas
//! clears the map in paint, and GPUI prepaint runs for the whole tree
//! before any paint. A prepaint recorder would be wiped by the reset.
//!
//! TestGpuixRenderer and GpuixRenderer both use this so locators, screenshots,
//! and clock control do not fork between headless tests and a live window.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{
    canvas, point, px, App, Bounds, InputEvent, IntoElement, KeyDownEvent, KeyUpEvent, LayoutInsets,
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput, Point,
    Styled, TransformationMatrix, Window, WindowId,
};
use rustc_hash::FxHashMap;
use web_time::Instant;

use crate::events::key_names::Press;
use crate::shrink::shrink_if_sparse;

/// An element's border box as layout computed it: fractional, in 1/64 of a pixel as Chrome's
/// `LayoutUnit` is. The box that is painted and hit is [`snapped`](Self::snapped_origin) to device
/// pixels and is not what these four are.
///
/// `x`, `y`, `width` and `height` are the box before any `transform`, in the space of its
/// ancestors' transforms, which is what `offset*`, `client*`, `scroll*` and the layout reads
/// measure. What `getBoundingClientRect` reports is [`Self::window_box`].
#[derive(Clone, Copy, Debug)]
pub struct ElementBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Where the border box is painted, which the pointer's `offsetX` and `offsetY` are measured from.
    pub painted_x: f64,
    pub painted_y: f64,
    /// The extent of the content from the padding box's origin as layout measured it (padding
    /// included), which `scrollWidth` and `scrollHeight` round.
    pub content_width: f64,
    pub content_height: f64,
    /// The DOM's `clientLeft`: the left border's width as laid out, so the padding box starts at `x + client_left`.
    pub client_left: f64,
    pub client_top: f64,
    pub border_right: f64,
    pub border_bottom: f64,
    /// The padding as laid out, so the content box is what the border box has left of it and of the border.
    pub padding_left: f64,
    pub padding_top: f64,
    pub padding_right: f64,
    pub padding_bottom: f64,
    /// A table with `border-collapse: collapse`. Its `border` is the outer halves of its cells' borders, which Chrome
    /// reports as `clientLeft` and `clientTop` but does not take off `clientWidth` and `clientHeight`.
    pub collapsed_table: bool,
    /// The element was in a culled subtree of the frame that recorded it: gpui walked it under an
    /// empty content mask, so nothing of it was drawn or hit. Retention reads it to leave a
    /// paint-only animation on it dormant (`retain::Sources::dormant`).
    pub culled: bool,
    /// From the space `x` and `y` are in to the window, in logical pixels and exact: the transforms
    /// around the element and its own. The unit matrix for an element no transform touches.
    pub to_window: TransformationMatrix,
    /// The back of the element faces the viewer, so that `backface-visibility: hidden` hides it and what
    /// is in it (`Window::layout_faces_away`).
    pub faces_away: bool,
}

/// Layout resolution, the one of Chrome's `LayoutUnit`: a length is cut down to a multiple of
/// 1/64 of a device pixel (1/128 of a pixel at scale 2), which `getBoundingClientRect` reports as
/// it is. A multiple of it is exact in an `f32` and in a sum with a device-pixel offset, so a box a
/// replay moves by a whole offset is the box a build would have given there.
const LAYOUT_UNITS_PER_DEVICE_PX: f64 = 64.0;

/// Taffy computes in `f32`, so a length that is a multiple of 1/64 (a text's width, `10px`) comes
/// out a rounding error either side of it, an error that depends on how far from the window's
/// origin the box is. Cutting that down would take a whole unit off one side, and a replayed box
/// and a built one would differ. The error is taken off first, by rounding to a finer grid that
/// is still many times coarser than it. Both grids are in device pixels, so a unit is on the
/// finer one whatever the scale.
const NOISE_UNITS_PER_DEVICE_PX: f64 = 512.0;

fn layout_units(value: Pixels, scale: f32) -> f64 {
    let scale = f64::from(scale);
    let device = f64::from(f32::from(value)) * scale;
    let denoised = (device * NOISE_UNITS_PER_DEVICE_PX).round() / NOISE_UNITS_PER_DEVICE_PX;
    (denoised * LAYOUT_UNITS_PER_DEVICE_PX).floor() / LAYOUT_UNITS_PER_DEVICE_PX / scale
}

impl ElementBounds {
    /// `painted` is where the box is when snapped to device pixels.
    fn from_gpui(layout: LaidOut, painted: Point<Pixels>, insets: LayoutInsets) -> Self {
        Self {
            x: layout_units(layout.border_box.origin.x, layout.scale),
            y: layout_units(layout.border_box.origin.y, layout.scale),
            width: layout_units(layout.border_box.size.width, layout.scale),
            height: layout_units(layout.border_box.size.height, layout.scale),
            content_width: layout_units(layout.content.width, layout.scale),
            content_height: layout_units(layout.content.height, layout.scale),
            painted_x: f64::from(f32::from(painted.x)),
            painted_y: f64::from(f32::from(painted.y)),
            client_left: f64::from(f32::from(insets.border.left)),
            client_top: f64::from(f32::from(insets.border.top)),
            border_right: f64::from(f32::from(insets.border.right)),
            border_bottom: f64::from(f32::from(insets.border.bottom)),
            padding_left: f64::from(f32::from(insets.padding.left)),
            padding_top: f64::from(f32::from(insets.padding.top)),
            padding_right: f64::from(f32::from(insets.padding.right)),
            padding_bottom: f64::from(f32::from(insets.padding.bottom)),
            collapsed_table: layout.collapsed_table,
            culled: layout.culled,
            to_window: layout.to_window,
            faces_away: layout.faces_away,
        }
    }

    /// A transform of the window moves or deforms the box, so that it is not where `x` and `y` say.
    pub fn is_transformed(&self) -> bool {
        !self.to_window.is_identity()
    }

    /// The smallest box with sides parallel to the window's that holds the border box after every
    /// transform around it and its own, which is what `getBoundingClientRect` reports.
    pub fn window_box(&self) -> (f64, f64, f64, f64) {
        if !self.is_transformed() {
            return (self.x, self.y, self.width, self.height);
        }
        self.box_through(&self.to_window)
    }

    /// [`Self::window_box`] in the space of `space`, another element's box, which is where a scroll
    /// aligns this one. `None` where `space` flattens the plane.
    pub fn box_in(&self, space: &ElementBounds) -> Option<(f64, f64, f64, f64)> {
        if !self.is_transformed() && !space.is_transformed() {
            return Some((self.x, self.y, self.width, self.height));
        }
        Some(self.box_through(&space.to_window.inverse()?.compose(self.to_window)))
    }

    fn box_through(&self, matrix: &TransformationMatrix) -> (f64, f64, f64, f64) {
        if !matrix.is_affine() {
            // What of the box is behind the eye is cut off first; one that is all behind it is empty.
            let bounds = gpui::Bounds::new(
                point(px(self.x as f32), px(self.y as f32)),
                gpui::size(px(self.width as f32), px(self.height as f32)),
            );
            return match matrix.try_bounding_box(bounds) {
                Some(mapped) => (
                    f64::from(f32::from(mapped.origin.x)),
                    f64::from(f32::from(mapped.origin.y)),
                    f64::from(f32::from(mapped.size.width)),
                    f64::from(f32::from(mapped.size.height)),
                ),
                None => (0.0, 0.0, 0.0, 0.0),
            };
        }
        let corners = [
            (self.x, self.y),
            (self.x + self.width, self.y),
            (self.x, self.y + self.height),
            (self.x + self.width, self.y + self.height),
        ]
        .map(|(x, y)| apply(matrix, x, y).expect("an affine matrix maps every point"));
        let (mut left, mut top) = (f64::INFINITY, f64::INFINITY);
        let (mut right, mut bottom) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (x, y) in corners {
            left = left.min(x);
            right = right.max(x);
            top = top.min(y);
            bottom = bottom.max(y);
        }
        (left, top, right - left, bottom - top)
    }

    /// The window point `(x, y)` in the space `self.x` and `self.y` are in, or `None` where the
    /// transform flattens the plane or the point is at the horizon or behind the eye of a
    /// perspective, and nothing is hit.
    pub fn to_local(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        if !self.is_transformed() {
            return Some((x, y));
        }
        self.to_window.inverse().and_then(|inverse| apply(&inverse, x, y))
    }

    /// [`Self::to_local`] for a point of the window as gpui has it.
    pub fn to_local_point(&self, position: Point<Pixels>) -> Option<Point<Pixels>> {
        window_point_in(&self.to_window, position)
    }

    /// Whether the window point `(x, y)` is inside the border box, tested through the inverse of the
    /// transforms. `horizontal` and `vertical` say which axes count, for a box that clips along one.
    pub fn holds_window_point(&self, x: f64, y: f64, horizontal: bool, vertical: bool) -> bool {
        let Some((x, y)) = self.to_local(x, y) else {
            return false;
        };
        (!horizontal || (x >= self.x && x < self.x + self.width))
            && (!vertical || (y >= self.y && y < self.y + self.height))
    }

    /// A box with no border or padding, painted where it is laid out.
    #[cfg(test)]
    pub(crate) fn plain(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
            painted_x: x,
            painted_y: y,
            content_width: width,
            content_height: height,
            client_left: 0.0,
            client_top: 0.0,
            border_right: 0.0,
            border_bottom: 0.0,
            padding_left: 0.0,
            padding_top: 0.0,
            padding_right: 0.0,
            padding_bottom: 0.0,
            collapsed_table: false,
            culled: false,
            to_window: TransformationMatrix::unit(),
            faces_away: false,
        }
    }

    /// How a verified frame says the box in words. Whether prepaint took the culled walk is left
    /// out: a root kept near a scroller's clip is walked culled when it replays a recording made
    /// far from it and unculled when it is built whole, and nothing the frame shows differs. The
    /// matrix is told to a thousandth: moving a transformed box by a replay and building it there
    /// differ in the last bits of a single precision sum.
    fn described(self) -> String {
        let [[a, c], [b, d]] = self.to_window.linear();
        let [e, f] = self.to_window.translation();
        let perspective = if self.to_window.is_affine() {
            String::new()
        } else {
            let [g, h, i] = self.to_window.rows[2];
            format!(" perspective({g:.6}, {h:.6}, {i:.3})")
        };
        format!(
            "{} matrix({a:.3}, {b:.3}, {c:.3}, {d:.3}, {e:.3}, {f:.3}){perspective} faces_away={}",
            self.described_apart_from_transform(),
            self.faces_away
        )
    }

    fn described_apart_from_transform(self) -> String {
        // Which way a box faces moves with its matrix, and a patch publishes it with the matrix.
        format!("{:?}", Self { culled: false, to_window: TransformationMatrix::unit(), faces_away: false, ..self })
    }

    /// The box `offset` away, in the single precision it is measured in, which is what painting
    /// it there measures.
    fn translated(self, offset: Point<Pixels>) -> Self {
        let moved = |value: f64, by: Pixels| f64::from(f32::from(px(value as f32) + by));
        Self {
            x: moved(self.x, offset.x),
            y: moved(self.y, offset.y),
            painted_x: moved(self.painted_x, offset.x),
            painted_y: moved(self.painted_y, offset.y),
            to_window: moved_matrix(self.to_window, offset),
            ..self
        }
    }
}

/// The window point `position` in the space `to_window` maps into the window, or `None` where the
/// transform flattens the plane or puts the point at the horizon or behind the eye, and nothing is
/// hit.
pub(crate) fn window_point_in(to_window: &TransformationMatrix, position: Point<Pixels>) -> Option<Point<Pixels>> {
    if to_window.is_identity() {
        return Some(position);
    }
    let inverse = to_window.inverse()?;
    let (x, y) = apply(&inverse, f64::from(f32::from(position.x)), f64::from(f32::from(position.y)))?;
    Some(point(px(x as f32), px(y as f32)))
}

/// `(x, y)` after the matrix, or `None` where a matrix with perspective puts it at the horizon or
/// behind the eye.
fn apply(matrix: &TransformationMatrix, x: f64, y: f64) -> Option<(f64, f64)> {
    let [[a, b, e], [c, d, f], [g, h, i]] = matrix.rows.map(|row| row.map(f64::from));
    if matrix.is_affine() {
        return Some((a * x + b * y + e, c * x + d * y + f));
    }
    let w = g * x + h * y + i;
    (w >= f64::from(gpui::MIN_W)).then(|| ((a * x + b * y + e) / w, (c * x + d * y + f) / w))
}

/// The matrix of a box that was moved by `offset` before it was transformed, so that it puts the
/// moved box where it put the box, moved by `offset`. A replay only moves a subtree by a whole
/// offset where the transform around it is a translation, which `offset` is a move in the window
/// of.
pub(crate) fn moved_matrix(matrix: TransformationMatrix, offset: Point<Pixels>) -> TransformationMatrix {
    if matrix.is_identity() {
        return matrix;
    }
    let (dx, dy) = (f32::from(offset.x), f32::from(offset.y));
    if !matrix.is_affine() {
        let to_window = TransformationMatrix::affine([[1.0, 0.0], [0.0, 1.0]], [dx, dy]);
        let from_window = TransformationMatrix::affine([[1.0, 0.0], [0.0, 1.0]], [-dx, -dy]);
        return to_window.compose(matrix).compose(from_window);
    }
    let [[a, b], [c, d]] = matrix.linear();
    let translation = matrix.translation();
    TransformationMatrix::affine(
        matrix.linear(),
        [translation[0] + dx - (a * dx + b * dy), translation[1] + dy - (c * dx + d * dy)],
    )
}

/// The painted boxes of one window.
///
/// A box is written once by the frame that built its element, and read again when a root that
/// holds it is replayed. A replay does not write the box back: it notes which of the lists of
/// writes it puts back (`live`, by the serial [`crate::tape::owner`] gave the write) and how far
/// they are moved, and a box is current if the frame wrote it or the list it was written to is live.
/// So replaying a root costs the same whatever it holds, and a box is read in one lookup.
#[derive(Default)]
struct WindowBounds {
    /// Counts the frames that reset the boxes. A box written by an earlier one is gone.
    frame: u64,
    live: FxHashMap<u64, Point<Pixels>>,
    boxes: FxHashMap<u64, Stored>,
    /// Boxes of elements that no frame wrote or replayed are left in `boxes` until there are this
    /// many.
    purge_at: usize,
    /// The frame of the last forced layout pass of a read, as it left the boxes: see [`Self::get_kept`].
    kept: Option<Kept>,
}

/// What a forced pass wrote and replayed, which the frames that skip the content it forced do not
/// write again nor replay.
struct Kept {
    frame: u64,
    live: FxHashMap<u64, Point<Pixels>>,
}

struct Stored {
    bounds: ElementBounds,
    frame: u64,
    /// The box as a subtree that records wrote it, and the list of writes it is in (see
    /// [`crate::tape::owner`]). A pass that builds the element without recording writes it
    /// again as `bounds`, and leaves this, which the next pass that replays the subtree reads.
    recorded: Option<(ElementBounds, u64)>,
    /// [`Self::recorded`] for a pass that only lays out: the list is one of a layout tape. Kept
    /// apart, so that a pass that lays out does not forget the box of the recording of a frame, which
    /// the draw after it may replay, nor the reverse.
    laid_out: Option<(ElementBounds, u64)>,
}

impl WindowBounds {
    fn get(&self, id: u64) -> Option<ElementBounds> {
        let stored = self.boxes.get(&id)?;
        if stored.frame == self.frame {
            return Some(stored.bounds);
        }
        [&stored.recorded, &stored.laid_out].into_iter().flatten().find_map(|(bounds, owner)| {
            let delta = crate::tape::delta_of(*owner, &self.live)?;
            Some(bounds.translated(delta))
        })
    }

    /// [`Self::get`], and the box the last forced pass left of an element the frames since have not
    /// written: a frame that skips the content that read forced writes none of its boxes, and a
    /// read whose window is still as that pass left it (`Relevance::forced_by_last_read`) takes
    /// them as it left them.
    fn get_kept(&self, id: u64) -> Option<ElementBounds> {
        if let Some(bounds) = self.get(id) {
            return Some(bounds);
        }
        let kept = self.kept.as_ref()?;
        let stored = self.boxes.get(&id)?;
        if stored.frame == kept.frame {
            return Some(stored.bounds);
        }
        [&stored.recorded, &stored.laid_out].into_iter().flatten().find_map(|(bounds, owner)| {
            let delta = crate::tape::delta_of(*owner, &kept.live)?;
            Some(bounds.translated(delta))
        })
    }

    /// Notes the boxes as the pass that just ran left them.
    fn keep(&mut self) {
        self.kept = Some(Kept { frame: self.frame, live: self.live.clone() });
    }

    fn reset(&mut self) {
        if self.boxes.len() > self.purge_at {
            let (frame, live, kept) = (self.frame, &self.live, &self.kept);
            // A box a layout record holds is read again by the pass that forces the content it is in,
            // however many draws come first (`Retain::skip_content`), and no replay put it back in
            // between: it goes with its element (`retain`).
            self.boxes.retain(|_, stored| {
                stored.frame == frame
                    || stored.laid_out.is_some()
                    || stored.recorded.iter().any(|(_, owner)| crate::tape::delta_of(*owner, live).is_some())
                    || kept.as_ref().is_some_and(|kept| {
                        stored.frame == kept.frame
                            || stored.recorded.iter().any(|(_, owner)| crate::tape::delta_of(*owner, &kept.live).is_some())
                    })
            });
            self.purge_at = (self.boxes.len() * 2).max(4096);
            shrink_if_sparse(&mut self.boxes);
        }
        self.frame += 1;
        self.live.clear();
    }

    /// Forgets the boxes of the elements `keep` says are gone.
    fn retain(&mut self, mut keep: impl FnMut(u64) -> bool) {
        self.boxes.retain(|id, _| keep(*id));
        shrink_if_sparse(&mut self.boxes);
    }

    fn insert(&mut self, id: u64, bounds: ElementBounds, owner: u64) {
        let previous = self.boxes.get_mut(&id);
        let (mut recorded, mut laid_out) = previous.map_or((None, None), |stored| (stored.recorded.take(), stored.laid_out.take()));
        match owner {
            0 => {}
            owner if crate::tape::is_laid_out(owner) => laid_out = Some((bounds, owner)),
            owner => recorded = Some((bounds, owner)),
        }
        self.boxes.insert(id, Stored { bounds, frame: self.frame, recorded, laid_out });
    }
}

thread_local! {
    /// Keyed by window as well as element id: element ids are only unique within one renderer,
    /// so two windows would otherwise read each other's boxes.
    static BOUNDS: RefCell<FxHashMap<WindowId, WindowBounds>> = RefCell::new(FxHashMap::default());
}

thread_local! {
    /// The layout node of the window's engine each host element was last painted with, for
    /// [`HostBox`]. A host in the engine of an atomic inline has none.
    static HOST_NODES: RefCell<FxHashMap<WindowId, FxHashMap<u64, (gpui::LayoutId, u64)>>> = RefCell::new(FxHashMap::default());
}

/// Where a host element stands in the window's layout, as far as a later reading can tell whether
/// it has moved or changed size.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostBox {
    /// The stamp of its node in the window's engine: it follows the box of the node and those of the
    /// nodes above it, not a box elsewhere. The last is the engine's id at the time (`Window::layout_engine_id`):
    /// the engine gives its nodes other ids when it moves them to a smaller store, and an id from before is
    /// then another node's.
    Node(gpui::LayoutId, u64, u64),
    /// Its node is not in the window's engine, or was not seen, so any box that moved may be its own.
    Anywhere,
}

impl HostBox {
    /// Where `id` stands now, by the layout node its last paint reported. `Anywhere` for a host that
    /// reported none, or whose node the engine freed or gave another id since.
    pub(crate) fn of(window: &gpui::Window, id: u64) -> Self {
        let node = HOST_NODES.with(|nodes| nodes.borrow().get(&window.window_handle().window_id())?.get(&id).copied());
        let current = node.filter(|(_, noted_in)| *noted_in == window.layout_engine_id());
        match current.and_then(|(node, engine)| Some((node, window.box_stamp(node)?, engine))) {
            Some((node, stamp, engine)) => Self::Node(node, stamp, engine),
            None => Self::Anywhere,
        }
    }

    /// Whether the host is where it was when `self` was taken, given that the window's boxes
    /// are `same_boxes` as they were then.
    pub(crate) fn unmoved(self, window: &gpui::Window, same_boxes: bool) -> bool {
        match self {
            Self::Node(node, stamp, engine) => window.layout_engine_id() == engine && window.box_stamp(node) == Some(stamp),
            Self::Anywhere => same_boxes,
        }
    }
}

/// Where a host element stands in the window: its layout box ([`HostBox`]) and the transform it was
/// last reported through, which a frame that animates a transform of the host or above it changes
/// without moving a box.
#[derive(Clone, Copy)]
pub(crate) struct HostPlace {
    id: u64,
    layout: HostBox,
    to_window: Option<TransformationMatrix>,
}

impl HostPlace {
    pub(crate) fn of(window: &gpui::Window, id: u64) -> Self {
        Self { id, layout: HostBox::of(window, id), to_window: Self::reported_to_window(window, id) }
    }

    /// Whether the host is where and as it was when `self` was taken, given that the window's boxes
    /// are `same_boxes` as they were then.
    pub(crate) fn unmoved(self, window: &gpui::Window, same_boxes: bool) -> bool {
        self.layout.unmoved(window, same_boxes) && self.to_window == Self::reported_to_window(window, self.id)
    }

    fn reported_to_window(window: &gpui::Window, id: u64) -> Option<TransformationMatrix> {
        let window_id = window.window_handle().window_id();
        BOUNDS.with(|cell| Some(cell.borrow().get(&window_id)?.get(id)?.to_window))
    }
}

/// Notes the layout node a host was painted (or published) with, if it is the window's own.
fn note_host_node(window: &gpui::Window, id: u64, node: gpui::LayoutId) {
    let window_id = window.window_handle().window_id();
    HOST_NODES.with(|nodes| {
        let mut nodes = nodes.borrow_mut();
        let nodes = nodes.entry(window_id).or_default();
        if window.is_window_layout_engine() {
            nodes.insert(id, (node, window.layout_engine_id()));
        } else {
            nodes.remove(&id);
        }
    });
}

/// The roots that `live` replays put back have the boxes they were built with, moved by the
/// replay: see [`WindowBounds`].
pub(crate) fn publish(window: WindowId, live: &crate::tape::Live) {
    if !live.has(crate::tape::Kind::Bounds) {
        return;
    }
    BOUNDS.with(|cell| live.mark(&mut cell.borrow_mut().entry(window).or_default().live));
}

/// Zero-size canvas. Keep it ahead of the app subtree under the root.
///
/// Everything here is recorded during **paint**, never prepaint: gpui's
/// `List::prepaint` speculatively prepaints a row range and can roll the window
/// back and prepaint a different one, so a prepaint-recorded box can belong to a
/// row that never reached the screen.
///
/// A pass that only lays out (a bounds read: `Window::draw_if_dirty`) has no paint, so the
/// recording moves to its end, through `Window::publish_layout`, which drops what a rolled-back
/// prepaint queued. The reset is still ahead of everything, and runs in that pass's prepaint.
pub fn bounds_frame_reset() -> impl IntoElement {
    canvas(
        |_, window, _| {
            if window.is_layout_pass() {
                forget_window(window.window_handle().window_id());
            }
        },
        move |_, _, window, _| forget_window(window.window_handle().window_id()),
    )
    .absolute()
    .w(px(0.0))
    .h(px(0.0))
}

/// Forgets the boxes of the elements of `window` that `keep` says are destroyed.
pub(crate) fn retain_bounds(window: WindowId, keep: impl FnMut(u64) -> bool) {
    let mut keep = keep;
    HOST_NODES.with(|nodes| {
        if let Some(nodes) = nodes.borrow_mut().get_mut(&window) {
            nodes.retain(|id, _| keep(*id));
            shrink_if_sparse(nodes);
        }
    });
    BOUNDS.with(|cell| {
        if let Some(bounds) = cell.borrow_mut().get_mut(&window) {
            bounds.retain(keep);
        }
    });
}

pub(crate) fn forget_window(window: WindowId) {
    BOUNDS.with(|cell| cell.borrow_mut().entry(window).or_default().reset());
}

/// Record this element's own painted box, with no extra element in the tree.
///
/// A leaf such as `gpui::img` cannot have a canvas child to measure it.
/// Wrapping the leaf in a div instead would move the layout box: the wrapper
/// would become the flex item and the image would lose intrinsic sizing and
/// corner clipping.
pub fn track_own_bounds<E: gpui::InteractiveElement + gpui::Styled>(el: E, id: u64) -> E {
    let (el, laid_out) = capture_layout_box(el, false);
    el.on_painted_layout(move |bounds, layout_id, window, _| {
        let insets = window.layout_insets(layout_id);
        record_bounds(window.window_handle().window_id(), id, laid_out.get(), bounds.origin, insets);
    })
}

/// The box layout gave an element, as its prepaint saw it.
///
/// Paint cannot ask for it: `Window::layout_unsnapped_bounds` reads the element offset, which only
/// prepaint has. The prepaint listener runs for a culled subtree too, and its box is read by the
/// painted-layout listener of the same element, which paints, or publishes after a pass that only
/// lays out, right after it.
#[derive(Clone, Default)]
pub struct LaidOutBox(Rc<Cell<Option<LaidOut>>>);

/// What layout measured of an element, before any of it is snapped to device pixels.
#[derive(Clone, Copy)]
pub struct LaidOut {
    /// See `Window::layout_unsnapped_bounds`.
    pub border_box: Bounds<Pixels>,
    /// How far the scrollable overflow reaches from the origin of the padding box, the padding
    /// after the content included, which `scrollWidth` and `scrollHeight` round. The border box
    /// for an element that does not scroll.
    pub content: gpui::Size<Pixels>,
    /// The window's scale factor, which sets the unit the box is read in.
    pub scale: f32,
    /// The element is a table with collapsed borders (`ElementBounds::collapsed_table`).
    pub collapsed_table: bool,
    /// Prepaint walked the element as part of a culled subtree (`Window::is_culling`), which only
    /// prepaint knows.
    pub culled: bool,
    /// See `ElementBounds::to_window`.
    pub to_window: TransformationMatrix,
    /// See `ElementBounds::faces_away`.
    pub faces_away: bool,
}

impl LaidOut {
    /// An inline box has no layout node: it holds what it is.
    pub fn of_box(border_box: Bounds<Pixels>, scale: f32) -> Self {
        Self {
            border_box,
            content: border_box.size,
            scale,
            collapsed_table: false,
            culled: false,
            to_window: TransformationMatrix::unit(),
            faces_away: false,
        }
    }

    /// The box as seen through `to_window`, see `ElementBounds::to_window`.
    pub fn through(self, to_window: TransformationMatrix) -> Self {
        Self { to_window, ..self }
    }
}

impl LaidOutBox {
    fn get(&self) -> LaidOut {
        self.0.get().expect("an element is prepainted before its box is reported")
    }
}

/// `scrolls`: the element is a scroller, whose overflow is measured; it walks the subtree, so it is
/// not asked of the others.
pub fn capture_layout_box<E: gpui::InteractiveElement>(el: E, scrolls: bool) -> (E, LaidOutBox) {
    let captured = LaidOutBox::default();
    let slot = captured.0.clone();
    let el = el.on_prepainted_layout(move |_, layout_id, window, _| {
        let border_box = window.layout_unsnapped_bounds(layout_id);
        let content = match scrolls.then(|| window.layout_unsnapped_scrollable_overflow(layout_id)).flatten() {
            Some(reach) => {
                let insets = window.layout_insets(layout_id);
                gpui::size(
                    reach.width - insets.border.left + insets.padding.right,
                    reach.height - insets.border.top + insets.padding.bottom,
                )
            }
            None => border_box.size,
        };
        // Prepaint runs outside the element's own transform, which layout holds: the listener
        // sees the ambient one of its parent.
        let to_window = window.layout_to_window(layout_id, border_box);
        let laid_out = LaidOut {
            border_box,
            content,
            scale: window.scale_factor(),
            collapsed_table: false,
            culled: window.is_culling(),
            to_window,
            faces_away: window.layout_faces_away(layout_id, border_box),
        };
        slot.set(Some(laid_out));
    });
    (el, captured)
}

/// Record a host element's painted box, and its selection-start region, for
/// automation and text selection.
///
/// The element reports its own box through `on_painted_layout`, so tracking it adds
/// nothing to the tree. A canvas child per element would double the number of
/// nodes gpui lays out, prepaints and paints.
///
/// A `visibility: hidden` element paints nothing but keeps its box, so it is
/// recorded and reports like any other. What is not recorded for it is what a
/// reader of the screen would see: the selection-start region.
///
/// The box is the border box, which is what a DOM `getBoundingClientRect`
/// reports. `scrolls` says it is a scroller, whose scrollable overflow is measured too.
/// `collapsed_table` says it is a table with collapsed borders.
///
/// `report` sees the painted box right after it is recorded. `on_painted_layout`
/// takes one listener, so it shares this one. It is for what cannot be read from the recorded
/// boxes afterwards (`onScroll`); `onLayout` and the observers are read from them by
/// [`crate::observers::frame_sweep`], which is what lets a replayed subtree report.
pub fn track_host_bounds<E>(
    el: E,
    id: u64,
    selection_start: Option<bool>,
    scrolls: bool,
    collapsed_table: bool,
    report: Option<PaintReporter>,
) -> E
where
    E: gpui::InteractiveElement + gpui::Styled,
{
    let (el, laid_out) = capture_layout_box(el, scrolls);
    el.on_painted_layout(move |bounds, layout_id, window, cx| {
        let insets = window.layout_insets(layout_id);
        let window_id = window.window_handle().window_id();
        note_host_node(window, id, layout_id);
        let laid_out = LaidOut { collapsed_table, ..laid_out.get() };
        let to_window = laid_out.to_window;
        record_bounds(window_id, id, laid_out, bounds.origin, insets);
        let visible = window.visibility() == gpui::Visibility::Visible;
        if let Some(selectable) = selection_start.filter(|_| visible) {
            crate::text::record_start_region(window_id, id, bounds, selectable, to_window);
        }
        if let Some(report) = &report {
            report(bounds, insets, window, cx);
        }
    })
}

/// Called with an element's painted box and the padding and border it was laid
/// out with, on every paint, from inside paint. It is how `onScroll` sees what the
/// frame produced. The window is the one being painted.
pub type PaintReporter = Rc<dyn Fn(Bounds<Pixels>, LayoutInsets, &mut Window, &mut App)>;

/// `layout` is the box as laid out and `painted` where its snapped box starts.
pub fn record_bounds(
    window: WindowId,
    id: u64,
    layout: LaidOut,
    painted: Point<Pixels>,
    insets: LayoutInsets,
) {
    crate::tape::write_in_layout(
        crate::tape::Kind::Bounds,
        (window, id, ElementBounds::from_gpui(layout, painted, insets)),
        |(window, id, bounds)| insert_bounds(*window, *id, bounds.translated(Point::default())),
        |(_, id, bounds), offset| format!("bounds {id} {}", bounds.translated(offset).described()),
    );
}

/// What a [`record_bounds`] write says, apart from the transforms around the element, which an
/// animation of a transform changes at every tick of a patch.
pub(crate) fn describe_apart_from_transform(data: &dyn std::any::Any, offset: Point<Pixels>) -> String {
    let (_, id, bounds) = bounds_write(data);
    format!("bounds {id} {}", bounds.translated(offset).described_apart_from_transform())
}

/// Makes a [`record_bounds`] write that a patch captured, which painted again without writing it.
pub(crate) fn publish_captured_bounds(data: &dyn std::any::Any, offset: Point<Pixels>) {
    let (window, id, bounds) = bounds_write(data);
    insert_bounds(*window, *id, bounds.translated(offset));
}

/// Gives the box of `id` the scrollable overflow a patch measured for it (`content`, in logical
/// pixels), which the transforms that ticked changed without a frame that records the box.
pub(crate) fn republish_scroll_content(window: WindowId, id: u64, content: gpui::Size<Pixels>, scale: f32) {
    let Some(bounds) = BOUNDS.with(|cell| cell.borrow().get(&window).and_then(|bounds| bounds.get(id))) else {
        return;
    };
    insert_bounds(
        window,
        id,
        ElementBounds {
            content_width: layout_units(content.width, scale),
            content_height: layout_units(content.height, scale),
            ..bounds
        },
    );
}

fn bounds_write(data: &dyn std::any::Any) -> &(WindowId, u64, ElementBounds) {
    data.downcast_ref().expect("a write of the bounds registry holds a window, an element and its box")
}

fn insert_bounds(window: WindowId, id: u64, bounds: ElementBounds) {
    BOUNDS.with(|cell| {
        let mut cell = cell.borrow_mut();
        cell.entry(window).or_default().insert(id, bounds, crate::tape::owner());
    });
}

/// A box for `id`, as a pass that laid it out leaves it.
#[cfg(test)]
pub(crate) fn set_bounds(window: WindowId, id: u64, bounds: ElementBounds) {
    insert_bounds(window, id, bounds);
}

pub fn get_bounds(window: WindowId, id: u64) -> Option<ElementBounds> {
    BOUNDS.with(|cell| {
        let bounds = cell.borrow();
        let bounds = bounds.get(&window)?;
        if is_reading_kept() { bounds.get_kept(id) } else { bounds.get(id) }
    })
}

thread_local! {
    static READING_KEPT: Cell<bool> = const { Cell::new(false) };
}

/// Runs `read` with [`get_bounds`] taking the boxes of the last forced pass for the elements the
/// frames since did not write.
pub(crate) fn reading_kept<R>(read: impl FnOnce() -> R) -> R {
    let was = READING_KEPT.with(|reading| reading.replace(true));
    let result = read();
    READING_KEPT.with(|reading| reading.set(was));
    result
}

/// Whether a [`reading_kept`] is running.
pub(crate) fn is_reading_kept() -> bool {
    READING_KEPT.with(Cell::get)
}

/// Notes the boxes of `window` as the forced pass that just ran left them, for [`reading_kept`].
pub(crate) fn keep_bounds(window: WindowId) {
    BOUNDS.with(|cell| cell.borrow_mut().entry(window).or_default().keep());
}

/// Whether the last frame that painted or laid out `window` walked `id` in a culled subtree. An
/// element it did not reach is not culled: nothing is assumed of what was not seen.
pub(crate) fn was_culled(window: WindowId, id: u64) -> bool {
    get_bounds(window, id).is_some_and(|bounds| bounds.culled)
}

/// Every painted box of `window` as text, in the order of the element ids.
pub(crate) fn describe_bounds(window: WindowId) -> String {
    let mut all: Vec<_> = all_bounds(window).into_iter().collect();
    all.sort_by_key(|(id, _)| *id);
    all.iter().map(|(id, bounds)| format!("bounds {id} {}\n", bounds.described())).collect()
}

pub fn all_bounds(window: WindowId) -> HashMap<u64, ElementBounds> {
    BOUNDS.with(|cell| {
        cell.borrow().get(&window).map_or_else(HashMap::new, |bounds| {
            bounds.boxes.keys().filter_map(|id| Some((*id, bounds.get(*id)?))).collect()
        })
    })
}

enum ClockMode {
    Live,
    Frozen { now: Instant },
}

struct ClockInner {
    origin: Instant,
    mode: ClockMode,
}

#[derive(Clone)]
pub struct AutomationClock {
    inner: Arc<Mutex<ClockInner>>,
}

impl Default for AutomationClock {
    fn default() -> Self {
        Self::new()
    }
}

impl AutomationClock {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ClockInner {
                origin: Instant::now(),
                mode: ClockMode::Live,
            })),
        }
    }

    pub fn now(&self) -> Instant {
        let inner = self.inner.lock().unwrap();
        match inner.mode {
            ClockMode::Live => Instant::now(),
            ClockMode::Frozen { now } => now,
        }
    }

    #[allow(dead_code)]
    pub fn now_ms(&self) -> f64 {
        let inner = self.inner.lock().unwrap();
        current_instant(&inner)
            .saturating_duration_since(inner.origin)
            .as_secs_f64()
            * 1000.0
    }

    /// Milliseconds from the origin of the clock to `instant`.
    pub fn ms_of(&self, instant: Instant) -> f64 {
        let inner = self.inner.lock().unwrap();
        instant.saturating_duration_since(inner.origin).as_secs_f64() * 1000.0
    }

    pub fn pause(&self) -> f64 {
        let mut inner = self.inner.lock().unwrap();
        let now = current_instant(&inner);
        inner.mode = ClockMode::Frozen { now };
        now.saturating_duration_since(inner.origin).as_secs_f64() * 1000.0
    }

    pub fn set_ms(&self, now_ms: f64) -> f64 {
        let mut inner = self.inner.lock().unwrap();
        let now = inner.origin + duration_ms(now_ms);
        inner.mode = ClockMode::Frozen { now };
        now_ms
    }

    pub fn fast_forward_ms(&self, delta_ms: f64) -> f64 {
        let mut inner = self.inner.lock().unwrap();
        let now = current_instant(&inner) + duration_ms(delta_ms);
        inner.mode = ClockMode::Frozen { now };
        now.saturating_duration_since(inner.origin).as_secs_f64() * 1000.0
    }

    pub fn resume(&self) -> f64 {
        let mut inner = self.inner.lock().unwrap();
        let elapsed = current_instant(&inner).saturating_duration_since(inner.origin);
        inner.origin = Instant::now() - elapsed;
        inner.mode = ClockMode::Live;
        elapsed.as_secs_f64() * 1000.0
    }
}

fn current_instant(inner: &ClockInner) -> Instant {
    match inner.mode {
        ClockMode::Live => Instant::now(),
        ClockMode::Frozen { now } => now,
    }
}

fn duration_ms(ms: f64) -> Duration {
    Duration::from_secs_f64((ms / 1000.0).max(0.0))
}

pub fn mouse_button(button: u32) -> MouseButton {
    match button {
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        _ => MouseButton::Left,
    }
}

/// Parse the held modifiers of a simulated mouse event from a hyphenated
/// string: `"cmd"`, `"cmd-shift"`, `"alt"`. `None` and `""` mean no modifier. Unknown names are ignored, so a
/// typo weakens the gesture instead of failing the whole automation call.
///
/// A string, not an object, because the same value has to cross the napi, the
/// wasm and the stdio boundary, and only wasm makes objects awkward.
pub fn parse_modifiers(modifiers: Option<&str>) -> Modifiers {
    let mut parsed = Modifiers::default();
    let Some(text) = modifiers else {
        return parsed;
    };
    for part in text.split('-') {
        match part.trim().to_ascii_lowercase().as_str() {
            "cmd" | "meta" | "super" | "win" | "platform" => parsed.platform = true,
            "ctrl" | "control" => parsed.control = true,
            "alt" | "option" => parsed.alt = true,
            "shift" => parsed.shift = true,
            "fn" | "function" => parsed.function = true,
            _ => {}
        }
    }
    parsed
}

/// Presses a space-separated sequence in Playwright's `keyboard.press` grammar
/// (`"Shift+Tab"`, `"Meta+a"`, `"Enter"`): keydown, then keyup, for each. Typed
/// characters reach the focused input like real typing. Every press is parsed
/// before the first is sent.
pub fn dispatch_keystrokes(
    window: &mut Window,
    cx: &mut App,
    keystrokes: &str,
) -> Result<(), String> {
    for press in crate::events::key_names::parse_presses(keystrokes)? {
        send_key_down(window, cx, press.clone(), false);
        send_key_up(window, cx, press);
    }
    Ok(())
}

/// One press's keydown, with no keyup, in the same grammar.
pub fn dispatch_key_down(
    window: &mut Window,
    cx: &mut App,
    keystroke: &str,
    is_held: bool,
) -> Result<(), String> {
    send_key_down(window, cx, crate::events::key_names::parse_press(keystroke)?, is_held);
    Ok(())
}

pub fn dispatch_key_up(window: &mut Window, cx: &mut App, keystroke: &str) -> Result<(), String> {
    send_key_up(window, cx, crate::events::key_names::parse_press(keystroke)?);
    Ok(())
}

/// What a platform input method does with the text of a key it consumed.
pub enum ImeAction<'a> {
    /// `setMarkedText:`: the text becomes the composition.
    Mark(&'a str),
    /// `insertText:` while composing: the text is final and the composition ends.
    Commit(&'a str),
    /// `unmarkText`: the composition ends with the text it has.
    Unmark,
}

/// A key the input method consumes (one press in the `dispatch_keystrokes`
/// grammar), as macOS delivers it: an `ImeKeyDown` first, then what the input
/// method does with it. Sends no keyup.
pub fn dispatch_ime_key_down(
    window: &mut Window,
    cx: &mut App,
    keystroke: &str,
    action: ImeAction,
) -> Result<(), String> {
    let press = crate::events::key_names::parse_press(keystroke)?;
    let is_composing = window.is_composing(cx);
    window.dispatch_event(
        PlatformInput::ImeKeyDown(KeyDownEvent {
            keystroke: press.keystroke,
            is_held: false,
            prefer_character_input: false,
            native_key: press.native_key,
            is_composing,
        }),
        cx,
    );
    let reached_input = match action {
        ImeAction::Mark(text) => window.dispatch_marked_text(text, cx),
        ImeAction::Commit(text) => window.dispatch_composed_text(text, cx),
        ImeAction::Unmark => window.dispatch_unmark_text(cx),
    };
    if reached_input {
        Ok(())
    } else {
        Err("No text input is focused, so there is nothing for the input method to compose in".to_string())
    }
}

fn send_key_down(window: &mut Window, cx: &mut App, press: Press, is_held: bool) {
    // Only a keystroke with a `key_char` goes through `dispatch_keystroke`,
    // which inserts that text into the focused input unless a handler stopped
    // the keydown. It would also invent a `key_char` for a key that has none
    // (`Shift` types "shift"), so every other key is a bare event.
    if press.keystroke.key_char.is_some() && !is_held {
        window.dispatch_native_keystroke(press.keystroke, press.native_key, cx);
    } else {
        let is_composing = window.is_composing(cx);
        window.dispatch_event(
            KeyDownEvent {
                keystroke: press.keystroke,
                is_held,
                prefer_character_input: false,
                native_key: press.native_key,
                is_composing,
            }
            .to_platform_input(),
            cx,
        );
    }
}

fn send_key_up(window: &mut Window, cx: &mut App, press: Press) {
    let is_composing = window.is_composing(cx);
    window.dispatch_event(
        KeyUpEvent {
            keystroke: press.keystroke,
            native_key: press.native_key,
            is_composing,
        }
        .to_platform_input(),
        cx,
    );
}

/// Every automation mouse dispatcher takes modifiers, so a test can drive
/// cmd-wheel zoom, shift-click range selection, or alt-drag duplication.
///
/// Each one first draws what the tree changed since the last frame, because the pointer is hit
/// tested against a drawn frame. A bounds read only lays out, and the display link that would
/// draw a frame for it may not run (a hidden window, a session with no display); a person's
/// pointer lands on whatever frame was on screen, and automation should land on what it read.
pub fn dispatch_click(
    window: &mut Window,
    cx: &mut App,
    x: f64,
    y: f64,
    button: u32,
    modifiers: Modifiers,
) {
    window.paint_if_dirty(cx);
    let position = point(px(x as f32), px(y as f32));
    let button = mouse_button(button);
    window.dispatch_event(
        MouseDownEvent {
            button,
            position,
            modifiers,
            click_count: 1,
            first_mouse: false,
        }
        .to_platform_input(),
        cx,
    );
    window.dispatch_event(
        MouseUpEvent {
            button,
            position,
            modifiers,
            click_count: 1,
        }
        .to_platform_input(),
        cx,
    );
}

pub fn dispatch_mouse_down(
    window: &mut Window,
    cx: &mut App,
    x: f64,
    y: f64,
    button: u32,
    modifiers: Modifiers,
) {
    window.paint_if_dirty(cx);
    window.dispatch_event(
        MouseDownEvent {
            button: mouse_button(button),
            position: point(px(x as f32), px(y as f32)),
            modifiers,
            click_count: 1,
            first_mouse: false,
        }
        .to_platform_input(),
        cx,
    );
}

pub fn dispatch_mouse_up(
    window: &mut Window,
    cx: &mut App,
    x: f64,
    y: f64,
    button: u32,
    modifiers: Modifiers,
) {
    window.paint_if_dirty(cx);
    window.dispatch_event(
        MouseUpEvent {
            button: mouse_button(button),
            position: point(px(x as f32), px(y as f32)),
            modifiers,
            click_count: 1,
        }
        .to_platform_input(),
        cx,
    );
}

pub fn dispatch_mouse_move(
    window: &mut Window,
    cx: &mut App,
    x: f64,
    y: f64,
    pressed_button: Option<u32>,
    modifiers: Modifiers,
) {
    window.paint_if_dirty(cx);
    window.dispatch_event(
        MouseMoveEvent {
            position: point(px(x as f32), px(y as f32)),
            pressed_button: pressed_button.map(mouse_button),
            modifiers,
        }
        .to_platform_input(),
        cx,
    );
}

/// A wheel delta as GPUI wants it, from the DOM's sign: positive `delta_y`
/// scrolls down (content moves up) and positive `delta_x` scrolls right, as in
/// `WheelEvent.deltaY` / `deltaX`. GPUI's `ScrollDelta` has the opposite sign
/// (it is how far the content moves), so the negation lives here, once, for
/// every simulated wheel.
pub fn wheel_delta(delta_x: f64, delta_y: f64) -> gpui::ScrollDelta {
    gpui::ScrollDelta::Pixels(point(px(-delta_x as f32), px(-delta_y as f32)))
}

/// Dispatches a wheel event. The deltas have the DOM's sign, see `wheel_delta`.
pub fn dispatch_scroll_wheel(
    window: &mut Window,
    cx: &mut App,
    x: f64,
    y: f64,
    delta_x: f64,
    delta_y: f64,
    modifiers: Modifiers,
    touch_phase: gpui::TouchPhase,
) {
    window.paint_if_dirty(cx);
    window.dispatch_event(
        gpui::ScrollWheelEvent {
            position: point(px(x as f32), px(y as f32)),
            delta: wheel_delta(delta_x, delta_y),
            modifiers,
            touch_phase,
        }
        .to_platform_input(),
        cx,
    );
}

#[cfg(test)]
mod host_box_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_length_is_cut_down_to_a_sixty_fourth_of_a_device_pixel_as_chrome_cuts_it() {
        // 33.3333% of 100px, and `getBoundingClientRect` in Chrome for it.
        assert_eq!(layout_units(px(33.3333), 1.0), 33.328125);
        assert_eq!(layout_units(px(100.4), 1.0), 100.390625);
        assert_eq!(layout_units(px(-0.3), 1.0), -0.3125);
        // A unit is 1/128 of a pixel at scale 2: `10.01px` is 10.0078125 there (fractional-lengths-oracle).
        assert_eq!(layout_units(px(10.01), 2.0), 10.0078125);
        assert_eq!(layout_units(px(100.4), 2.0), 100.3984375);
        assert_eq!(layout_units(px(0.4), 2.0), 0.3984375);
    }

    /// A box of 80 x 40 at (10, 20), turned a quarter around its middle (50, 40) and moved 5 right.
    fn turned() -> ElementBounds {
        ElementBounds {
            to_window: TransformationMatrix::affine([[0.0, -1.0], [1.0, 0.0]], [100.0 + 5.0, -10.0]),
            ..ElementBounds::plain(10.0, 20.0, 80.0, 40.0)
        }
    }

    #[test]
    fn a_transformed_box_reports_the_box_that_holds_its_corners() {
        // (x, y) goes to (105 - y, x - 10): the corners are (85, 0), (85, 80), (45, 0) and (45, 80).
        assert_eq!(turned().window_box(), (45.0, 0.0, 40.0, 80.0));
        // The size the layout measured is the one before the turn.
        assert_eq!((turned().width, turned().height), (80.0, 40.0));
        assert_eq!(ElementBounds::plain(10.0, 20.0, 80.0, 40.0).window_box(), (10.0, 20.0, 80.0, 40.0));
    }

    #[test]
    fn a_point_of_the_window_is_in_a_transformed_box_where_the_inverse_puts_it_in_the_box() {
        let turned = turned();
        // (x, y) comes from (X, Y) as (Y + 10, 105 - X): (65, 10) is (20, 40), inside the box; (50, 70) is
        // (80, 55), inside; (84, 79) is (89, 21), inside; and (44, 40) is (50, 61), 1px below it.
        assert_eq!(turned.to_local(65.0, 10.0), Some((20.0, 40.0)));
        assert!(turned.holds_window_point(65.0, 10.0, true, true));
        assert!(turned.holds_window_point(50.0, 70.0, true, true));
        assert!(turned.holds_window_point(84.0, 79.0, true, true));
        assert!(!turned.holds_window_point(44.0, 40.0, true, true));
        // Along the axis that clips, the other is not asked.
        assert!(turned.holds_window_point(44.0, 40.0, true, false));
    }

    #[test]
    fn a_box_that_a_transform_flattens_is_hit_nowhere() {
        let flat = ElementBounds {
            to_window: TransformationMatrix::affine([[0.0, 0.0], [0.0, 1.0]], [0.0, 0.0]),
            ..ElementBounds::plain(10.0, 20.0, 80.0, 40.0)
        };
        assert_eq!(flat.to_local(30.0, 30.0), None);
        assert!(!flat.holds_window_point(30.0, 30.0, true, true));
    }

    /// A box of 100 x 50 at (0, 0) seen along its x axis from 200 away: `W = 1 - x / 200`.
    fn in_perspective() -> ElementBounds {
        ElementBounds {
            to_window: TransformationMatrix { rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [-1.0 / 200.0, 0.0, 1.0]] },
            ..ElementBounds::plain(0.0, 0.0, 100.0, 50.0)
        }
    }

    #[test]
    fn a_box_in_perspective_reports_the_box_of_its_divided_corners() {
        // The far edge, x = 100, is half as far (W = 0.5) and shows at x = 200, y = 100.
        let (x, y, width, height) = in_perspective().window_box();
        assert_eq!((x, y), (0.0, 0.0));
        assert!((width - 200.0).abs() < 1e-3 && (height - 100.0).abs() < 1e-3, "{width} x {height}");
    }

    #[test]
    fn a_box_with_a_corner_behind_the_eye_reports_a_finite_box_and_one_wholly_behind_it_an_empty_one() {
        let partly = ElementBounds { width: 300.0, ..in_perspective() };
        let (x, _, width, height) = partly.window_box();
        assert_eq!(x, 0.0);
        assert!(width.is_finite() && width > 200.0 && height.is_finite(), "{width} x {height}");
        let behind = ElementBounds { x: 250.0, width: 50.0, ..in_perspective() };
        assert_eq!(behind.window_box(), (0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn a_point_in_perspective_maps_back_with_the_divide_and_misses_behind_the_eye() {
        let wide = ElementBounds { width: 800.0, ..in_perspective() };
        // x = 100 shows at x = 200, and y = 30 at 60.
        let (x, y) = wide.to_local(200.0, 60.0).unwrap();
        assert!((x - 100.0).abs() < 1e-3 && (y - 30.0).abs() < 1e-3, "{x} {y}");
        assert!(wide.holds_window_point(200.0, 60.0, true, true));
        // x = 600 is behind the eye and would show at -300: nothing is there.
        assert_eq!(wide.to_local(-300.0, 20.0), None);
        assert!(!wide.holds_window_point(-300.0, 20.0, true, true));
    }

    #[test]
    fn a_replay_moves_a_box_in_perspective_by_the_offset_in_the_window() {
        let moved = in_perspective().translated(point(px(0.0), px(30.0)));
        let (x, y, width, height) = in_perspective().window_box();
        let (moved_x, moved_y, moved_width, moved_height) = moved.window_box();
        assert!((moved_x - x).abs() < 1e-2 && (moved_y - (y + 30.0)).abs() < 1e-2, "{moved_x} {moved_y}");
        assert!((moved_width - width).abs() < 1e-2 && (moved_height - height).abs() < 1e-2);
    }

    #[test]
    fn a_replay_moves_the_transformed_box_and_the_box_it_reports_by_the_same_offset() {
        let offset = point(px(0.0), px(30.0));
        let moved = turned().translated(offset);
        assert_eq!((moved.x, moved.y), (10.0, 50.0));
        let (x, y, width, height) = turned().window_box();
        assert_eq!(moved.window_box(), (x, y + 30.0, width, height));
    }

    #[test]
    fn a_box_is_seen_in_the_space_of_a_transformed_scroller() {
        let scroller = turned();
        // Laid out at (20, 30), 10 x 10, not transformed itself: the scroller aligns it where its turn puts it
        // back, (Y + 10, 105 - X) for its corners.
        let inside = ElementBounds::plain(20.0, 30.0, 10.0, 10.0);
        assert_eq!(inside.box_in(&scroller), Some((40.0, 75.0, 10.0, 10.0)));
        // Turned by the same transform, it is where it is laid out.
        let with_it = ElementBounds { to_window: scroller.to_window, ..inside };
        assert_eq!(with_it.box_in(&scroller), Some((20.0, 30.0, 10.0, 10.0)));
    }

    #[test]
    fn a_multiple_of_a_sixty_fourth_stays_put_whatever_its_rounding_error() {
        for value in [69.3125_f32, 0.015625, 2783.984375] {
            let below = f32::from_bits(value.to_bits() - 1);
            let above = f32::from_bits(value.to_bits() + 1);
            for noisy in [below, value, above] {
                assert_eq!(layout_units(px(noisy), 1.0), f64::from(value), "{noisy}");
            }
        }
    }

    #[test]
    fn a_multiple_of_a_sixty_fourth_of_a_device_pixel_stays_put_at_scale_two() {
        for value in [10.0078125_f32, 0.0078125, 1391.9921875] {
            let below = f32::from_bits(value.to_bits() - 1);
            let above = f32::from_bits(value.to_bits() + 1);
            for noisy in [below, value, above] {
                assert_eq!(layout_units(px(noisy), 2.0), f64::from(value), "{noisy}");
            }
        }
    }

    #[test]
    fn a_box_moved_by_a_device_pixel_offset_is_the_box_laid_out_there() {
        let at = |x: f32| layout_units(px(x), 1.0);
        let moved = ElementBounds::plain(at(33.3333), 0.0, 10.0, 10.0).translated(gpui::point(px(-1021.5), px(0.0)));
        assert_eq!(moved.x, at(33.3333 - 1021.5));
    }

    #[test]
    fn wheel_deltas_have_the_doms_sign() {
        // Positive deltaY scrolls down, which GPUI spells as the content moving up.
        let gpui::ScrollDelta::Pixels(delta) = wheel_delta(7.0, 30.0) else {
            panic!("a wheel delta is in pixels")
        };
        assert_eq!((f32::from(delta.x), f32::from(delta.y)), (-7.0, -30.0));
    }

    #[test]
    fn frozen_clock_holds_and_fast_forwards() {
        let clock = AutomationClock::new();
        clock.set_ms(0.0);
        assert!((clock.now_ms() - 0.0).abs() < 0.001);
        clock.fast_forward_ms(150.0);
        assert!((clock.now_ms() - 150.0).abs() < 0.001);
        let later = clock.now();
        clock.fast_forward_ms(150.0);
        assert_eq!(
            clock.now().saturating_duration_since(later),
            Duration::from_millis(150)
        );
    }

    fn bounds_of(window: u64, id: u64) -> Option<ElementBounds> {
        get_bounds(WindowId::from(window), id)
    }

    #[test]
    fn the_boxes_of_a_page_that_left_give_their_slots_back() {
        const BOXES: u64 = 10_000;
        println!("Stored: {} bytes", std::mem::size_of::<Stored>());
        let (window, id) = (0x5_0000_0001, WindowId::from(0x5_0000_0001));
        for element in 0..BOXES {
            set_bounds(id, element, ElementBounds::plain(element as f64, 0.0, 10.0, 10.0));
        }
        let grown = BOUNDS.with(|cell| cell.borrow()[&id].boxes.capacity());
        assert!(grown as u64 * std::mem::size_of::<Stored>() as u64 > 256 * 1024);

        retain_bounds(id, |element| element % 100 == 0);
        let capacity = BOUNDS.with(|cell| cell.borrow()[&id].boxes.capacity());
        assert!(capacity < grown / 10, "{capacity} of {grown}");
        assert_eq!(bounds_of(window, 300).unwrap().x, 300.0);
        assert!(bounds_of(window, 301).is_none());

        for _ in 0..3 {
            retain_bounds(id, |_| true);
        }
        assert_eq!(BOUNDS.with(|cell| cell.borrow()[&id].boxes.capacity()), capacity);
        forget_window(id);
    }

    #[test]
    fn the_purge_of_boxes_no_frame_wrote_gives_their_slots_back() {
        let mut bounds = WindowBounds::default();
        for id in 0..10_000 {
            bounds.insert(id, ElementBounds::plain(0.0, 0.0, 1.0, 1.0), 0);
        }
        bounds.reset();
        let grown = bounds.boxes.capacity();
        for id in 0..50 {
            bounds.insert(id, ElementBounds::plain(0.0, 0.0, 1.0, 1.0), 0);
        }
        bounds.purge_at = 0;
        bounds.reset();
        assert_eq!(bounds.boxes.len(), 50);
        assert!(bounds.boxes.capacity() < grown / 10);

        let capacity = bounds.boxes.capacity();
        for _ in 0..3 {
            bounds.reset();
        }
        assert_eq!(bounds.boxes.capacity(), capacity);
    }
}
