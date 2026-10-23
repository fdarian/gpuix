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
    Styled, Window, WindowId,
};
use rustc_hash::FxHashMap;
use web_time::Instant;

use crate::events::key_names::Press;

/// An element's border box as layout computed it, which is what the DOM reports for
/// `getBoundingClientRect`: fractional, in 1/64 of a pixel as Chrome's `LayoutUnit` is. The box
/// that is painted and hit is [`snapped`](Self::snapped_origin) to device pixels and is not what
/// these four are.
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
        }
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
        }
    }

    /// How a verified frame says the box in words. Whether prepaint took the culled walk is left
    /// out: a root kept near a scroller's clip is walked culled when it replays a recording made
    /// far from it and unculled when it is built whole, and nothing the frame shows differs.
    fn described(self) -> String {
        format!("{:?}", Self { culled: false, ..self })
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
            ..self
        }
    }
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
        }
        self.frame += 1;
        self.live.clear();
    }

    /// Forgets the boxes of the elements `keep` says are gone.
    fn retain(&mut self, mut keep: impl FnMut(u64) -> bool) {
        self.boxes.retain(|id, _| keep(*id));
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
}

impl LaidOut {
    /// An inline box has no layout node: it holds what it is.
    pub fn of_box(border_box: Bounds<Pixels>, scale: f32) -> Self {
        Self { border_box, content: border_box.size, scale, collapsed_table: false, culled: false }
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
        let laid_out = LaidOut {
            border_box,
            content,
            scale: window.scale_factor(),
            collapsed_table: false,
            culled: window.is_culling(),
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
        record_bounds(window_id, id, LaidOut { collapsed_table, ..laid_out.get() }, bounds.origin, insets);
        let visible = window.visibility() == gpui::Visibility::Visible;
        if let Some(selectable) = selection_start.filter(|_| visible) {
            crate::text::record_start_region(window_id, bounds, selectable);
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
}
