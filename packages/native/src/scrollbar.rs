//! Overlay scrollbars, painted the way macOS does.
//!
//! A scroll container's bar is hidden at rest, appears whenever its scroll offset changes, and
//! fades out after a second without movement. While the pointer is over the bar it stays, the
//! thumb thickens and a faint track shows. The thumb drags, and pressing the track pages toward
//! the pointer.
//!
//! The bar is an overlay (`paint_overlay`), so it never takes part in what the container can
//! scroll. Its hitbox exists only where the bar is visible, so it blocks nothing otherwise.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui::prelude::*;
use web_time::Instant;

use crate::style::StyleDesc;

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
        Self { width, colors }
    }

    pub(crate) fn hidden(self) -> bool {
        self.width == Width::None
    }

    /// Thumb thickness at rest and under the pointer.
    fn thickness(self) -> (f32, f32) {
        match self.width {
            Width::Thin => (4.0, 7.0),
            _ => (7.0, 11.0),
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

fn default_colors(window: &gpui::Window) -> (gpui::Rgba, gpui::Rgba) {
    let dark = matches!(
        window.appearance(),
        gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark
    );
    let (shade, thumb, track) = if dark { (1.0, 0.5, 0.08) } else { (0.0, 0.5, 0.06) };
    (
        gpui::Rgba { r: shade, g: shade, b: shade, a: thumb },
        gpui::Rgba { r: shade, g: shade, b: shade, a: track },
    )
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
}

#[derive(Default)]
pub(crate) struct State {
    last_offset: Option<gpui::Point<gpui::Pixels>>,
    last_active: Option<Instant>,
    hover: [bool; 2],
    drag: Option<Drag>,
    layout: [Option<AxisLayout>; 2],
    hitboxes: [Option<gpui::Hitbox>; 2],
    alpha: f32,
    seen: u64,
}

thread_local! {
    static STATES: RefCell<HashMap<u64, Rc<RefCell<State>>>> = RefCell::new(HashMap::new());
    static FRAME: Cell<u64> = const { Cell::new(0) };
    static NOW: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// Call before building a frame, with the time its animations run on. Drops the bars of
/// scrollers that were not built last frame.
pub(crate) fn begin_frame(now: Instant) {
    NOW.set(Some(now));
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

/// Gives a scroll container its bars, if its style does not turn them off.
pub(crate) fn attach(
    el: gpui::Stateful<gpui::Div>,
    id: u64,
    source: Source,
    style: &StyleDesc,
    colors: &crate::color::ColorContext,
) -> gpui::Stateful<gpui::Div> {
    let look = Look::of(style, colors);
    if look.hidden() {
        return el;
    }
    let state = state_for(id);
    el.prepaint_overlay({
        let (state, source) = (state.clone(), source.clone());
        move |bounds, window, _| prepaint(&state, &source, look, bounds, window)
    })
    .paint_overlay(move |bounds, window, cx| paint(&state, &source, look, bounds, window, cx))
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
    let (_, hover_thickness) = look.thickness();
    let bar = hover_thickness + EDGE_INSET;
    let [user_x, user_y] = source.axes();
    let show_x = user_x && f32::from(max.x) > 0.5;
    let show_y = user_y && f32::from(max.y) > 0.5;
    let corner = if show_x && show_y { bar } else { 0.0 };

    let mut layout: [Option<AxisLayout>; 2] = [None, None];
    for (index, vertical) in [(0, false), (1, true)] {
        if !(if vertical { show_y } else { show_x }) {
            continue;
        }
        let (port_start, port_len) = axis_of(port, vertical);
        let track_len = port_len - corner;
        let (max_scroll, scrolled) = if vertical {
            (f32::from(max.y), f32::from(offset.y))
        } else {
            (f32::from(max.x), f32::from(offset.x))
        };
        let Some((thumb_start, thumb_len)) = scrollbar_thumb(track_len, max_scroll, scrolled)
        else {
            continue;
        };
        let strip = if vertical {
            gpui::Bounds::new(
                gpui::point(port.right() - gpui::px(bar), port.top()),
                gpui::size(gpui::px(bar), gpui::px(track_len)),
            )
        } else {
            gpui::Bounds::new(
                gpui::point(port.left(), port.bottom() - gpui::px(bar)),
                gpui::size(gpui::px(track_len), gpui::px(bar)),
            )
        };
        layout[index] = Some(AxisLayout {
            strip,
            track_start: port_start,
            track_len,
            thumb_start,
            thumb_len,
            max: max_scroll,
            port_len,
        });
    }

    let held = st.hover.iter().any(|hovered| *hovered) || st.drag.is_some();
    if held {
        st.last_active = Some(now);
    }
    let alpha = match st.last_active {
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
    };

    st.layout = layout;
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
    }
    if alpha > 0.0 {
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
    let (layout, hitboxes, alpha) = {
        let mut st = state.borrow_mut();
        for index in 0..2 {
            if let Some(hitbox) = &st.hitboxes[index] {
                st.hover[index] = hitbox.is_hovered(window);
            }
        }
        (st.layout, st.hitboxes.clone(), st.alpha)
    };
    if alpha <= 0.0 {
        return;
    }

    let (thumb_color, track_color) = look
        .colors
        .unwrap_or_else(|| default_colors(window));
    let (rest, hover) = look.thickness();
    let scaled = |color: gpui::Rgba| -> gpui::Hsla {
        gpui::Rgba { a: color.a * alpha, ..color }.into()
    };
    {
        let st = state.borrow();
        for (index, vertical) in [(0, false), (1, true)] {
            let Some(axis) = layout[index] else { continue };
            let hovered = st.hover[index] || st.drag.is_some_and(|drag| drag.axis == index);
            let thickness = if hovered { hover } else { rest };
            let strip = axis.strip;
            let rect = |start: f32, len: f32, thickness: f32| {
                if vertical {
                    gpui::Bounds::new(
                        gpui::point(
                            strip.right() - gpui::px(EDGE_INSET + thickness),
                            gpui::px(axis.track_start + start),
                        ),
                        gpui::size(gpui::px(thickness), gpui::px(len)),
                    )
                } else {
                    gpui::Bounds::new(
                        gpui::point(
                            gpui::px(axis.track_start + start),
                            strip.bottom() - gpui::px(EDGE_INSET + thickness),
                        ),
                        gpui::size(gpui::px(len), gpui::px(thickness)),
                    )
                }
            };
            let paint_rect = |window: &mut gpui::Window, bounds, thickness: f32, color| {
                window.paint_quad(gpui::quad(
                    bounds,
                    gpui::px(thickness / 2.0),
                    color,
                    gpui::px(0.0),
                    gpui::transparent_black(),
                    gpui::BorderStyle::default(),
                ));
            };
            if hovered {
                paint_rect(window, rect(0.0, axis.track_len, hover), hover, scaled(track_color));
            }
            paint_rect(
                window,
                rect(axis.thumb_start, axis.thumb_len, thickness),
                thickness,
                scaled(thumb_color),
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
            if crate::events::listeners::in_drag_region(window, event.position) {
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
}
