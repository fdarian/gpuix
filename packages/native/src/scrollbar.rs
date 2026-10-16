//! Scrollbars, painted the way macOS does, in either of its two styles.
//!
//! With "Show scroll bars" on Automatically or When scrolling (`ScrollerStyle::Overlay`), a scroll
//! container's bar is hidden at rest, appears whenever its scroll offset changes, and fades out
//! after a second without movement. While the pointer is over the bar it stays, and the thumb
//! thickens over 150 ms and a faint track shows. The bar takes no layout space.
//!
//! With it on Always (`ScrollerStyle::Classic`, which macOS also picks while a mouse is
//! attached), the bar is always there: an opaque track and a thumb that does not change, in a
//! gutter the container reserves out of its own box, so `clientWidth` and the content box shrink
//! by it.
//!
//! In both styles the thumb drags, and pressing the track pages toward the pointer. The bar is
//! an overlay (`paint_overlay`), so it never takes part in what the container can scroll. Its
//! hitbox exists only where the bar is visible, so it blocks nothing otherwise.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui::prelude::*;
use web_time::Instant;

use crate::scroll::Overflow;
use crate::style::StyleDesc;

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)] // objc 0.2's msg_send! expands to cfg(feature = "cargo-clippy")
mod observer {
    use block::ConcreteBlock;
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    #[link(name = "AppKit", kind = "framework")]
    extern "C" {
        static NSPreferredScrollerStyleDidChangeNotification: *mut Object;
    }

    /// Calls back when "Show scroll bars" changes, and stops when dropped.
    pub(crate) struct ScrollerStyleObserver {
        token: *mut Object,
    }

    impl ScrollerStyleObserver {
        /// `on_change` runs on the main queue, which is the thread GPUI runs on.
        pub(crate) fn new(on_change: impl Fn() + 'static) -> Self {
            // SAFETY: the block is copied to the heap, so the center keeps it alive until
            // `removeObserver:`. `token` is retained until drop.
            unsafe {
                let center: *mut Object = msg_send![class!(NSNotificationCenter), defaultCenter];
                let main_queue: *mut Object = msg_send![class!(NSOperationQueue), mainQueue];
                let block = ConcreteBlock::new(move |_notification: *mut Object| on_change());
                let block = block.copy();
                let token: *mut Object = msg_send![center,
                    addObserverForName: NSPreferredScrollerStyleDidChangeNotification
                    object: std::ptr::null_mut::<Object>()
                    queue: main_queue
                    usingBlock: block
                ];
                let token: *mut Object = msg_send![token, retain];
                Self { token }
            }
        }
    }

    impl Drop for ScrollerStyleObserver {
        fn drop(&mut self) {
            // SAFETY: `token` came from `addObserverForName:` and was retained in `new`.
            unsafe {
                let center: *mut Object = msg_send![class!(NSNotificationCenter), defaultCenter];
                let _: () = msg_send![center, removeObserver: self.token];
                let _: () = msg_send![self.token, release];
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) use observer::ScrollerStyleObserver;

/// How long a bar stays fully visible after the offset last moved.
const IDLE: Duration = Duration::from_millis(1000);
/// How long it then takes to fade out.
const FADE: Duration = Duration::from_millis(200);
/// Gap between the thumb and the edge of the scrollport.
const EDGE_INSET: f32 = 2.0;
/// The shortest a thumb gets, however long the content.
const MIN_THUMB: f32 = 20.0;
/// How much of the viewport pressing the track pages by.
const PAGE_FRACTION: f32 = 0.9;
/// How long the thumb takes to thicken under the pointer, and to thin again.
const HOVER_GROW: Duration = Duration::from_millis(150);
/// The width of a classic bar's gutter: `auto`, and `thin`.
const CLASSIC_GUTTER: f32 = 15.0;
const CLASSIC_GUTTER_THIN: f32 = 11.0;
/// How many times in a row an `overflow: auto` scroller may add or drop its gutter, so content
/// that only fits with the gutter dropped (and overflows with it) cannot flip for ever.
const MAX_GUTTER_FLIPS: u8 = 4;

/// The macOS "Show scroll bars" setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollerStyle {
    /// Automatically based on mouse or trackpad (with a trackpad), or When scrolling.
    Overlay,
    /// Always, and what Automatically gives while a mouse is attached.
    Classic,
}

impl ScrollerStyle {
    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "overlay" => Ok(Self::Overlay),
            "classic" => Ok(Self::Classic),
            other => Err(format!(
                "Unknown scrollbar style \"{other}\", expected \"overlay\" or \"classic\""
            )),
        }
    }

    /// What the OS asks for. Only macOS has the setting; elsewhere bars are overlays.
    pub(crate) fn of_system(cx: &gpui::App) -> Self {
        if cfg!(target_os = "macos") && !cx.should_auto_hide_scrollbars() {
            Self::Classic
        } else {
            Self::Overlay
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Width {
    Auto,
    Thin,
    None,
}

/// What a style asks the bar to look like.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Look {
    width: Width,
    /// `None` follows the system appearance.
    colors: Option<(gpui::Rgba, gpui::Rgba)>,
    scroller: ScrollerStyle,
}

impl Look {
    /// `scrollbarWidth` is `auto`, `thin` or `none`, and `scrollbarColor` is `auto` or
    /// `<thumb> <track>`. A value that is none of those is ignored, as CSS ignores it.
    pub(crate) fn of(style: &StyleDesc, context: &crate::color::ColorContext) -> Self {
        let width = match style.scrollbar_width.as_deref() {
            Some("thin") => Width::Thin,
            Some("none") => Width::None,
            _ => Width::Auto,
        };
        let colors = style
            .scrollbar_color
            .as_deref()
            .and_then(|value| parse_colors(value, context));
        Self { width, colors, scroller: SCROLLER.get() }
    }

    pub(crate) fn hidden(self) -> bool {
        self.width == Width::None
    }

    fn classic(self) -> bool {
        self.scroller == ScrollerStyle::Classic && !self.hidden()
    }

    /// The room a classic bar takes from its container, zero for an overlay.
    pub(crate) fn gutter(self) -> f32 {
        match (self.classic(), self.width) {
            (false, _) => 0.0,
            (true, Width::Thin) => CLASSIC_GUTTER_THIN,
            (true, _) => CLASSIC_GUTTER,
        }
    }

    /// Gap between the thumb and the edge of its bar.
    fn inset(self) -> f32 {
        match (self.classic(), self.width) {
            (false, _) => EDGE_INSET,
            (true, Width::Thin) => 2.0,
            (true, _) => 3.0,
        }
    }

    /// Thumb thickness at rest and under the pointer. A classic thumb does not change.
    fn thickness(self) -> (f32, f32) {
        if self.classic() {
            let thumb = self.gutter() - 2.0 * self.inset();
            return (thumb, thumb);
        }
        match self.width {
            Width::Thin => (4.0, 7.0),
            _ => (7.0, 11.0),
        }
    }

    /// How far across the container the bar's hit area and track reach.
    fn bar(self) -> f32 {
        if self.classic() {
            self.gutter()
        } else {
            self.thickness().1 + EDGE_INSET
        }
    }
}

/// `"<thumb> <track>"`: two colours, each a CSS colour that may itself contain spaces inside
/// parentheses.
fn parse_colors(
    value: &str,
    context: &crate::color::ColorContext,
) -> Option<(gpui::Rgba, gpui::Rgba)> {
    let mut tokens = Vec::new();
    let (mut depth, mut start) = (0usize, None);
    for (index, character) in value.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        let space = character.is_whitespace() && depth == 0;
        match (space, start) {
            (false, None) => start = Some(index),
            (true, Some(from)) => {
                tokens.push(&value[from..index]);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        tokens.push(&value[from..]);
    }
    let [thumb, track] = tokens.as_slice() else {
        return None;
    };
    Some((context.resolve(thumb)?, context.resolve(track)?))
}

fn default_colors(window: &gpui::Window, classic: bool) -> (gpui::Rgba, gpui::Rgba) {
    let dark = matches!(
        window.appearance(),
        gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark
    );
    let (shade, thumb, track) = if dark { (1.0, 0.5, 0.08) } else { (0.0, 0.5, 0.06) };
    let thumb = gpui::Rgba { r: shade, g: shade, b: shade, a: thumb };
    if classic {
        // A classic track covers whatever scrolls under it, so it is solid.
        let ground = if dark { 0.16 } else { 0.97 };
        return (thumb, gpui::Rgba { r: ground, g: ground, b: ground, a: 1.0 });
    }
    (thumb, gpui::Rgba { r: shade, g: shade, b: shade, a: track })
}

fn ease_in_out(progress: f32) -> f32 {
    progress * progress * (3.0 - 2.0 * progress)
}

/// How far the pointer has thickened one bar, between 0 (at rest) and 1 (under the pointer).
#[derive(Clone, Copy, Default)]
struct Grow {
    from: f32,
    target: f32,
    started: Option<Instant>,
}

impl Grow {
    fn value(self, now: Instant) -> f32 {
        let Some(started) = self.started else {
            return self.target;
        };
        let progress = now.saturating_duration_since(started).as_secs_f32()
            / HOVER_GROW.as_secs_f32();
        self.from + (self.target - self.from) * ease_in_out(progress.min(1.0))
    }

    /// Heads for `target` from wherever it is now, starting at `now`.
    fn aim(&mut self, target: f32, now: Instant) {
        if target != self.target {
            self.from = self.value(now);
            self.target = target;
            self.started = Some(now);
        }
    }

    fn animating(self, now: Instant) -> bool {
        self.started
            .is_some_and(|started| now.saturating_duration_since(started) < HOVER_GROW)
    }
}

/// Whether a classic scroller reserves a gutter on an axis. `overflow: auto` does once something
/// scrolls, which `needed` says it did when last laid out.
fn wants_gutter(overflow: Overflow, needed: bool) -> bool {
    match overflow {
        Overflow::Scroll => true,
        Overflow::Auto => needed,
        _ => false,
    }
}

/// The thumb of a bar along one axis: where it starts and how long it is, in the track's own
/// coordinates. `track` is the bar's length, `max_scroll` how far the content can move and
/// `offset` how far it has (zero or negative). `None` when the content fits.
pub(crate) fn scrollbar_thumb(track: f32, max_scroll: f32, offset: f32) -> Option<(f32, f32)> {
    if track <= 0.0 || max_scroll <= 1.0 {
        return None;
    }
    let length = (track * track / (track + max_scroll)).clamp(MIN_THUMB.min(track), track);
    let progress = (-offset / max_scroll).clamp(0.0, 1.0);
    Some((progress * (track - length), length))
}

/// What a bar scrolls.
#[derive(Clone)]
pub(crate) enum Source {
    Container { id: u64, handle: gpui::ScrollHandle },
    List(gpui::ListState),
}

impl Source {
    /// Which axes the user scrolls, so which ones get a bar.
    fn axes(&self) -> [bool; 2] {
        match self {
            Source::Container { handle, .. } => {
                let user = handle.user_scrollable();
                [user.x, user.y]
            }
            Source::List(_) => [false, true],
        }
    }

    /// How far the content can move and how far it has (zero or negative).
    fn extent(&self) -> (gpui::Point<gpui::Pixels>, gpui::Point<gpui::Pixels>) {
        match self {
            Source::Container { handle, .. } => (handle.max_offset(), handle.offset()),
            Source::List(state) => match state.averaged_scroll_extent() {
                // Rows not yet measured count as the average measured row, so the thumb does
                // not jump as rows come into view.
                Some((max, offset)) => (
                    gpui::point(gpui::px(0.0), max),
                    gpui::point(gpui::px(0.0), -offset),
                ),
                None => (
                    state.max_offset_for_scrollbar(),
                    state.scroll_px_offset_for_scrollbar(),
                ),
            },
        }
    }

    /// The box the bars sit in, given the element's own bounds.
    fn port(&self, bounds: gpui::Bounds<gpui::Pixels>) -> gpui::Bounds<gpui::Pixels> {
        match self {
            Source::Container { handle, .. } => handle.padding_box(),
            Source::List(_) => bounds,
        }
    }

    /// The room the container reserved for its bars: a list's are overlays.
    fn gutter(&self) -> gpui::Size<gpui::Pixels> {
        match self {
            Source::Container { handle, .. } => handle.scrollbar_size(),
            Source::List(_) => gpui::size(gpui::px(0.0), gpui::px(0.0)),
        }
    }

    fn set_offset(&self, offset: gpui::Point<gpui::Pixels>) {
        match self {
            Source::Container { id, handle } => {
                crate::scroll::scroll_to(*id, handle, offset, crate::scroll::Behavior::Instant)
            }
            Source::List(state) => state.set_offset_from_scrollbar(offset),
        }
    }

    /// A list keeps its totals steady while a thumb is held, so the thumb does not run away.
    fn drag(&self, started: bool) {
        if let Source::List(state) = self {
            if started {
                state.scrollbar_drag_started();
            } else {
                state.scrollbar_drag_ended();
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Drag {
    axis: usize,
    /// How far into the thumb the pointer took hold.
    grab: f32,
}

/// Where one axis' bar is, in window coordinates.
#[derive(Clone, Copy)]
struct AxisLayout {
    strip: gpui::Bounds<gpui::Pixels>,
    track_start: f32,
    track_len: f32,
    thumb_start: f32,
    thumb_len: f32,
    max: f32,
    port_len: f32,
    inset: f32,
}

#[derive(Default)]
pub(crate) struct State {
    last_offset: Option<gpui::Point<gpui::Pixels>>,
    last_active: Option<Instant>,
    hover: [bool; 2],
    drag: Option<Drag>,
    layout: [Option<AxisLayout>; 2],
    /// Tracks with no thumb: a classic bar with nothing to scroll, and the corner where two meet.
    empty_tracks: Vec<gpui::Bounds<gpui::Pixels>>,
    hitboxes: [Option<gpui::Hitbox>; 2],
    grow: [Grow; 2],
    alpha: f32,
    seen: u64,
    /// Per axis, whether the content scrolled when last laid out. An `overflow: auto` scroller
    /// reserves its gutter from it, one frame later.
    needed: [bool; 2],
    /// Per axis, whether the scroller was built with a gutter.
    reserved: [bool; 2],
    /// Per axis, whether the gutter follows `needed`.
    follows_need: [bool; 2],
    flips: u8,
}

thread_local! {
    static STATES: RefCell<HashMap<u64, Rc<RefCell<State>>>> = RefCell::new(HashMap::new());
    static FRAME: Cell<u64> = const { Cell::new(0) };
    static NOW: Cell<Option<Instant>> = const { Cell::new(None) };
    static SCROLLER: Cell<ScrollerStyle> = const { Cell::new(ScrollerStyle::Overlay) };
}

/// Call before building a frame, with the time its animations run on and the style the bars
/// take. Drops the bars of scrollers that were not built last frame.
pub(crate) fn begin_frame(now: Instant, scroller: ScrollerStyle) {
    NOW.set(Some(now));
    SCROLLER.set(scroller);
    let frame = FRAME.get() + 1;
    FRAME.set(frame);
    STATES.with(|states| {
        states
            .borrow_mut()
            .retain(|_, state| state.borrow().seen + 1 >= frame);
    });
}

fn now() -> Instant {
    NOW.get().unwrap_or_else(Instant::now)
}

pub(crate) fn state_for(id: u64) -> Rc<RefCell<State>> {
    STATES.with(|states| states.borrow_mut().entry(id).or_default().clone())
}

/// Gives a scroll container its bars, if its style does not turn them off, and the room for them
/// that a classic bar takes. `overflow` is the container's, per axis (x, then y).
pub(crate) fn attach(
    el: gpui::Stateful<gpui::Div>,
    id: u64,
    source: Source,
    style: &StyleDesc,
    colors: &crate::color::ColorContext,
    overflow: [Overflow; 2],
) -> gpui::Stateful<gpui::Div> {
    let look = Look::of(style, colors);
    if look.hidden() {
        return el;
    }
    let state = state_for(id);
    let el = reserve(el, &state, look, overflow);
    el.prepaint_overlay({
        let (state, source) = (state.clone(), source.clone());
        move |bounds, window, _| prepaint(&state, &source, look, bounds, window)
    })
    .paint_overlay(move |bounds, window, cx| paint(&state, &source, look, bounds, window, cx))
}

/// Makes the container leave room for its classic bars. A bar for the vertical axis takes width
/// and one for the horizontal axis takes height, and a bar the user cannot move (`hidden`) takes
/// nothing.
fn reserve(
    el: gpui::Stateful<gpui::Div>,
    state: &Rc<RefCell<State>>,
    look: Look,
    overflow: [Overflow; 2],
) -> gpui::Stateful<gpui::Div> {
    let mut st = state.borrow_mut();
    let gutter = look.gutter();
    if gutter == 0.0 {
        st.reserved = [false; 2];
        st.follows_need = [false; 2];
        return el;
    }
    let wanted = [0, 1].map(|axis| wants_gutter(overflow[axis], st.needed[axis]));
    if wanted == st.reserved {
        st.flips = 0;
    } else if st.flips < MAX_GUTTER_FLIPS {
        st.flips += 1;
        st.reserved = wanted;
    }
    st.follows_need = [0, 1].map(|axis| overflow[axis] == Overflow::Auto);
    let reserved = st.reserved;
    el.scrollbar_width(gpui::px(gutter))
        .without_scrollbar(!reserved[0], !reserved[1])
}

fn axis_of(bounds: gpui::Bounds<gpui::Pixels>, vertical: bool) -> (f32, f32) {
    if vertical {
        (f32::from(bounds.origin.y), f32::from(bounds.size.height))
    } else {
        (f32::from(bounds.origin.x), f32::from(bounds.size.width))
    }
}

/// Works out whether and where the bars show, and inserts their hitboxes. Prepaint, because a
/// hitbox can only be inserted there.
pub(crate) fn prepaint(
    state: &Rc<RefCell<State>>,
    source: &Source,
    look: Look,
    bounds: gpui::Bounds<gpui::Pixels>,
    window: &mut gpui::Window,
) {
    let mut st = state.borrow_mut();
    st.seen = FRAME.get();
    let now = now();
    let (max, offset) = source.extent();

    if let Some(last) = st.last_offset {
        let moved = (f32::from(last.x - offset.x)).abs() > 0.01
            || (f32::from(last.y - offset.y)).abs() > 0.01;
        if moved {
            st.last_active = Some(now);
        }
    }
    st.last_offset = Some(offset);

    let port = source.port(bounds);
    let gutter = source.gutter();
    let classic = look.classic();
    let bar = look.bar();
    let [user_x, user_y] = source.axes();
    let scrolls = [user_x && f32::from(max.x) > 0.5, user_y && f32::from(max.y) > 0.5];
    st.needed = scrolls;
    // The bar of the x axis lies along the bottom, so it takes the gutter's height.
    let room = [f32::from(gutter.height), f32::from(gutter.width)];
    let shown = [scrolls[0] || room[0] > 0.0, scrolls[1] || room[1] > 0.0];
    let corner = if shown[0] && shown[1] && room == [0.0, 0.0] { bar } else { 0.0 };

    let mut layout: [Option<AxisLayout>; 2] = [None, None];
    let mut empty_tracks = Vec::new();
    for (index, vertical) in [(0, false), (1, true)] {
        if !shown[index] {
            continue;
        }
        let (port_start, port_len) = axis_of(port, vertical);
        let track_len = port_len - corner;
        let strip = match (vertical, room[index] > 0.0) {
            (true, true) => gpui::Bounds::new(
                gpui::point(port.right(), port.top()),
                gpui::size(gutter.width, gpui::px(track_len)),
            ),
            (false, true) => gpui::Bounds::new(
                gpui::point(port.left(), port.bottom()),
                gpui::size(gpui::px(track_len), gutter.height),
            ),
            (true, false) => gpui::Bounds::new(
                gpui::point(port.right() - gpui::px(bar), port.top()),
                gpui::size(gpui::px(bar), gpui::px(track_len)),
            ),
            (false, false) => gpui::Bounds::new(
                gpui::point(port.left(), port.bottom() - gpui::px(bar)),
                gpui::size(gpui::px(track_len), gpui::px(bar)),
            ),
        };
        let (max_scroll, scrolled) = if vertical {
            (f32::from(max.y), f32::from(offset.y))
        } else {
            (f32::from(max.x), f32::from(offset.x))
        };
        let thumb = scrollbar_thumb(track_len, max_scroll, scrolled).filter(|_| scrolls[index]);
        let Some((thumb_start, thumb_len)) = thumb else {
            if room[index] > 0.0 {
                empty_tracks.push(strip);
            }
            continue;
        };
        layout[index] = Some(AxisLayout {
            strip,
            track_start: port_start,
            track_len,
            thumb_start,
            thumb_len,
            max: max_scroll,
            port_len,
            inset: look.inset(),
        });
    }
    if room[0] > 0.0 && room[1] > 0.0 {
        empty_tracks.push(gpui::Bounds::new(
            gpui::point(port.right(), port.bottom()),
            gpui::size(gutter.width, gutter.height),
        ));
    }

    let held = st.hover.iter().any(|hovered| *hovered) || st.drag.is_some();
    if held {
        st.last_active = Some(now);
    }
    let alpha = if classic {
        1.0
    } else {
        match st.last_active {
            None => 0.0,
            Some(active) => {
                let elapsed = now.saturating_duration_since(active);
                if elapsed <= IDLE {
                    1.0
                } else if elapsed < IDLE + FADE {
                    1.0 - (elapsed - IDLE).as_secs_f32() / FADE.as_secs_f32()
                } else {
                    0.0
                }
            }
        }
    };

    st.layout = layout;
    st.empty_tracks = empty_tracks;
    st.alpha = alpha;
    for index in 0..2 {
        st.hitboxes[index] = match layout[index] {
            Some(axis) if alpha > 0.0 => {
                Some(window.insert_hitbox(axis.strip, gpui::HitboxBehavior::BlockMouseExceptScroll))
            }
            _ => None,
        };
        if st.hitboxes[index].is_none() {
            st.hover[index] = false;
        }
        let dragged = st.drag.is_some_and(|drag| drag.axis == index);
        let thickened = !classic && (st.hover[index] || dragged);
        st.grow[index].aim(if thickened { 1.0 } else { 0.0 }, now);
    }
    let growing = st.grow.iter().any(|grow| grow.animating(now));
    // An `overflow: auto` gutter follows the layout it caused, so it takes one more frame to settle.
    let settling = st.flips < MAX_GUTTER_FLIPS
        && (0..2).any(|axis| st.follows_need[axis] && st.needed[axis] != st.reserved[axis]);
    if (alpha > 0.0 && !classic) || growing || settling {
        window.request_animation_frame();
    }
}

fn along(position: gpui::Point<gpui::Pixels>, vertical: bool) -> f32 {
    if vertical {
        f32::from(position.y)
    } else {
        f32::from(position.x)
    }
}

/// Moves `axis` to a scroll position (pixels from the start) without touching the other one.
fn scroll_axis_to(source: &Source, axis: usize, position: f32, max: f32) {
    let (_, mut offset) = source.extent();
    let target = gpui::px(0.0 - position.clamp(0.0, max));
    if axis == 1 {
        offset.y = target;
    } else {
        offset.x = target;
    }
    source.set_offset(offset);
}

/// Draws the bars and listens for the pointer.
pub(crate) fn paint(
    state: &Rc<RefCell<State>>,
    source: &Source,
    look: Look,
    _bounds: gpui::Bounds<gpui::Pixels>,
    window: &mut gpui::Window,
    _cx: &mut gpui::App,
) {
    let (layout, empty_tracks, hitboxes, alpha) = {
        let mut st = state.borrow_mut();
        for index in 0..2 {
            if let Some(hitbox) = &st.hitboxes[index] {
                st.hover[index] = hitbox.is_hovered(window);
            }
        }
        (st.layout, st.empty_tracks.clone(), st.hitboxes.clone(), st.alpha)
    };
    if alpha <= 0.0 {
        return;
    }

    let classic = look.classic();
    let (thumb_color, track_color) = look
        .colors
        .unwrap_or_else(|| default_colors(window, classic));
    let (rest, hover) = look.thickness();
    let scaled = |color: gpui::Rgba, share: f32| -> gpui::Hsla {
        gpui::Rgba { a: color.a * alpha * share, ..color }.into()
    };
    let paint_rect = |window: &mut gpui::Window, bounds, radius: f32, color| {
        window.paint_quad(gpui::quad(
            bounds,
            gpui::px(radius),
            color,
            gpui::px(0.0),
            gpui::transparent_black(),
            gpui::BorderStyle::default(),
        ));
    };
    for track in &empty_tracks {
        paint_rect(window, *track, 0.0, scaled(track_color, 1.0));
    }
    {
        let st = state.borrow();
        let now = now();
        for (index, vertical) in [(0, false), (1, true)] {
            let Some(axis) = layout[index] else { continue };
            let grown = st.grow[index].value(now);
            let thickness = rest + (hover - rest) * grown;
            let strip = axis.strip;
            let rect = |start: f32, len: f32, thickness: f32| {
                if vertical {
                    gpui::Bounds::new(
                        gpui::point(
                            strip.right() - gpui::px(axis.inset + thickness),
                            gpui::px(axis.track_start + start),
                        ),
                        gpui::size(gpui::px(thickness), gpui::px(len)),
                    )
                } else {
                    gpui::Bounds::new(
                        gpui::point(
                            gpui::px(axis.track_start + start),
                            strip.bottom() - gpui::px(axis.inset + thickness),
                        ),
                        gpui::size(gpui::px(len), gpui::px(thickness)),
                    )
                }
            };
            if classic {
                paint_rect(window, strip, 0.0, scaled(track_color, 1.0));
            } else if grown > 0.0 {
                paint_rect(
                    window,
                    rect(0.0, axis.track_len, hover),
                    hover / 2.0,
                    scaled(track_color, grown),
                );
            }
            paint_rect(
                window,
                rect(axis.thumb_start, axis.thumb_len, thickness),
                thickness / 2.0,
                scaled(thumb_color, 1.0),
            );
        }
    }

    let view = window.current_view();
    window.on_mouse_event({
        let (state, source, hitboxes) = (state.clone(), source.clone(), hitboxes.clone());
        move |event: &gpui::MouseMoveEvent, phase, window, cx| {
            if phase != gpui::DispatchPhase::Capture {
                return;
            }
            let mut st = state.borrow_mut();
            if let Some(drag) = st.drag {
                let Some(axis) = st.layout[drag.axis] else { return };
                let travel = axis.track_len - axis.thumb_len;
                if travel > 0.0 {
                    let pointer = along(event.position, drag.axis == 1);
                    let fraction = (pointer - drag.grab - axis.track_start) / travel;
                    scroll_axis_to(&source, drag.axis, fraction.clamp(0.0, 1.0) * axis.max, axis.max);
                }
                cx.notify(view);
                return;
            }
            let hover = [0, 1].map(|index| {
                hitboxes[index]
                    .as_ref()
                    .is_some_and(|hitbox| hitbox.is_hovered(window))
            });
            if hover != st.hover {
                st.hover = hover;
                cx.notify(view);
            }
        }
    });
    window.on_mouse_event({
        let (state, source) = (state.clone(), source.clone());
        move |event: &gpui::MouseDownEvent, phase, window, cx| {
            if phase != gpui::DispatchPhase::Bubble || event.button != gpui::MouseButton::Left {
                return;
            }
            // A drag region covers the scrollbar under it, as in Electron.
            if crate::events::app_region::in_drag_region(window, event.position) {
                return;
            }
            let mut st = state.borrow_mut();
            for index in 0..2 {
                let (Some(hitbox), Some(axis)) = (&hitboxes[index], st.layout[index]) else {
                    continue;
                };
                if !hitbox.is_hovered(window) {
                    continue;
                }
                let pointer = along(event.position, index == 1);
                let thumb = axis.track_start + axis.thumb_start;
                if pointer >= thumb && pointer <= thumb + axis.thumb_len {
                    st.drag = Some(Drag { axis: index, grab: pointer - thumb });
                    source.drag(true);
                } else {
                    // Page toward the pointer.
                    let (_, offset) = source.extent();
                    let position = 0.0
                        - if index == 1 { f32::from(offset.y) } else { f32::from(offset.x) };
                    let page = axis.port_len * PAGE_FRACTION;
                    let next = if pointer < thumb { position - page } else { position + page };
                    scroll_axis_to(&source, index, next, axis.max);
                }
                cx.stop_propagation();
                cx.notify(view);
                return;
            }
        }
    });
    window.on_mouse_event({
        let (state, source) = (state.clone(), source.clone());
        move |_: &gpui::MouseUpEvent, phase, _, cx| {
            if phase == gpui::DispatchPhase::Capture && state.borrow_mut().drag.take().is_some() {
                source.drag(false);
                cx.notify(view);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thumb_is_the_viewports_share_of_the_content() {
        // 100 of 300: a third of the track.
        let (start, length) = scrollbar_thumb(100.0, 200.0, 0.0).unwrap();
        assert_eq!(start, 0.0);
        assert!((length - 100.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn a_thumb_runs_the_whole_track_as_the_content_runs_its_distance() {
        let (_, length) = scrollbar_thumb(100.0, 200.0, 0.0).unwrap();
        let (start, same_length) = scrollbar_thumb(100.0, 200.0, -200.0).unwrap();
        assert_eq!(length, same_length);
        assert!((start + length - 100.0).abs() < 0.01);
        let (halfway, _) = scrollbar_thumb(100.0, 200.0, -100.0).unwrap();
        assert!((halfway - (100.0 - length) / 2.0).abs() < 0.01);
    }

    #[test]
    fn a_thumb_stays_inside_the_track_when_the_offset_overshoots() {
        let (start, length) = scrollbar_thumb(100.0, 200.0, -450.0).unwrap();
        assert!((start + length - 100.0).abs() < 0.01);
        assert_eq!(scrollbar_thumb(100.0, 200.0, 30.0).unwrap().0, 0.0);
    }

    #[test]
    fn a_thumb_is_never_shorter_than_a_grabbable_length_nor_longer_than_the_track() {
        assert_eq!(scrollbar_thumb(100.0, 100_000.0, 0.0).unwrap().1, 20.0);
        assert_eq!(scrollbar_thumb(10.0, 100_000.0, 0.0).unwrap().1, 10.0);
    }

    #[test]
    fn content_that_fits_has_no_thumb() {
        assert_eq!(scrollbar_thumb(100.0, 0.0, 0.0), None);
        assert_eq!(scrollbar_thumb(100.0, 0.5, 0.0), None);
        assert_eq!(scrollbar_thumb(0.0, 200.0, 0.0), None);
    }

    #[test]
    fn colors_are_two_css_colors_with_spaces_allowed_inside_parentheses() {
        let context = crate::color::ColorContext::root(crate::color::Scheme::Light);
        let parse_colors = |value| parse_colors(value, &context);
        let (thumb, track) = parse_colors("rgb(255 0 0 / 50%)   #00ff00").unwrap();
        assert_eq!((thumb.r, thumb.a), (1.0, 0.5));
        assert_eq!(track.g, 1.0);
        assert!(parse_colors("#ff0000").is_none(), "one colour is not enough");
        assert!(parse_colors("red green blue").is_none());
        assert!(parse_colors("auto").is_none());
    }

    #[test]
    fn width_is_auto_thin_or_none() {
        let look = |width: &str| {
            Look::of(
                &StyleDesc { scrollbar_width: Some(width.to_string()), ..Default::default() },
                &crate::color::ColorContext::root(crate::color::Scheme::Light),
            )
        };
        assert_eq!(look("thin").thickness(), (4.0, 7.0));
        assert!(look("none").hidden());
        assert_eq!(look("auto").thickness(), (7.0, 11.0));
        assert_eq!(look("nonsense").thickness(), (7.0, 11.0));
    }

    fn classic_look(width: Option<&str>) -> Look {
        let mut look = Look::of(
            &StyleDesc { scrollbar_width: width.map(str::to_string), ..Default::default() },
            &crate::color::ColorContext::root(crate::color::Scheme::Light),
        );
        look.scroller = ScrollerStyle::Classic;
        look
    }

    #[test]
    fn a_classic_bar_takes_15_pixels_or_11_when_thin_and_an_overlay_takes_none() {
        assert_eq!(classic_look(None).gutter(), 15.0);
        assert_eq!(classic_look(Some("thin")).gutter(), 11.0);
        assert_eq!(classic_look(Some("none")).gutter(), 0.0);
        let overlay = Look::of(
            &StyleDesc::default(),
            &crate::color::ColorContext::root(crate::color::Scheme::Light),
        );
        assert_eq!(overlay.gutter(), 0.0);
    }

    #[test]
    fn a_classic_thumb_fills_its_gutter_less_the_inset_and_does_not_thicken() {
        assert_eq!(classic_look(None).thickness(), (9.0, 9.0));
        assert_eq!(classic_look(Some("thin")).thickness(), (7.0, 7.0));
        assert_eq!(classic_look(None).bar(), 15.0);
    }

    #[test]
    fn the_scrollbar_style_is_overlay_or_classic() {
        assert_eq!(ScrollerStyle::parse("overlay"), Ok(ScrollerStyle::Overlay));
        assert_eq!(ScrollerStyle::parse("classic"), Ok(ScrollerStyle::Classic));
        assert!(ScrollerStyle::parse("always").is_err());
    }

    #[test]
    fn scroll_reserves_a_gutter_and_auto_only_once_something_scrolls() {
        assert!(wants_gutter(Overflow::Scroll, false));
        assert!(!wants_gutter(Overflow::Auto, false));
        assert!(wants_gutter(Overflow::Auto, true));
        assert!(!wants_gutter(Overflow::Hidden, true));
        assert!(!wants_gutter(Overflow::Visible, true));
    }

    #[test]
    fn thickening_eases_over_150_ms_from_wherever_it_was() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut grow = Grow::default();
        assert_eq!(grow.value(at(0)), 0.0);
        grow.aim(1.0, at(0));
        assert_eq!(grow.value(at(0)), 0.0);
        assert!((grow.value(at(75)) - 0.5).abs() < 0.001);
        assert!(grow.value(at(30)) < 0.2, "eases in");
        assert_eq!(grow.value(at(150)), 1.0);
        assert_eq!(grow.value(at(10_000)), 1.0);
        assert!(grow.animating(at(149)) && !grow.animating(at(150)));

        // The pointer leaves halfway: it thins from where it got to, not from fully grown.
        let mut grow = Grow::default();
        grow.aim(1.0, at(0));
        grow.aim(0.0, at(75));
        assert!((grow.value(at(75)) - 0.5).abs() < 0.001);
        assert_eq!(grow.value(at(225)), 0.0);
    }

    #[test]
    fn aiming_at_the_same_target_does_not_restart_the_ease() {
        let start = Instant::now();
        let mut grow = Grow::default();
        grow.aim(1.0, start);
        grow.aim(1.0, start + Duration::from_millis(100));
        assert_eq!(grow.value(start + Duration::from_millis(150)), 1.0);
    }
}
