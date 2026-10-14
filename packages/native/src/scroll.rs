//! Scrolling, as a browser does it: which elements scroll, how far they can, and how a
//! script moves them.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use gpui::prelude::*;
use web_time::Instant;

use crate::style::StyleDesc;

/// What one axis of CSS `overflow` does to the element's content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Overflow {
    Visible,
    /// `clip`: the content is cut off and the element is not a scroll container.
    Clip,
    /// `hidden`: cut off, but a scroll container that only code scrolls, as in a browser.
    Hidden,
    /// `scroll` and `auto`. Scrollbars are overlays, so the two differ in nothing.
    Scroll,
}

impl Overflow {
    pub(crate) fn parse(value: Option<&str>) -> Self {
        match value {
            Some("scroll" | "auto") => Self::Scroll,
            Some("hidden") => Self::Hidden,
            Some("clip") => Self::Clip,
            _ => Self::Visible,
        }
    }

    pub(crate) fn clips(self) -> bool {
        self != Self::Visible
    }

    /// Whether the element has a scroll offset: `scroll`, `auto` and `hidden`.
    pub(crate) fn has_offset(self) -> bool {
        matches!(self, Self::Scroll | Self::Hidden)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Axes {
    pub x: Overflow,
    pub y: Overflow,
}

impl Axes {
    /// `overflowX` and `overflowY` override the `overflow` shorthand.
    pub(crate) fn of(style: &StyleDesc) -> Self {
        Self {
            x: Overflow::parse(style.overflow_x.as_deref().or(style.overflow.as_deref())),
            y: Overflow::parse(style.overflow_y.as_deref().or(style.overflow.as_deref())),
        }
    }

    /// Whether the element has a scroll offset on either axis.
    pub(crate) fn scrolls(self) -> bool {
        self.x.has_offset() || self.y.has_offset()
    }
}

fn overscroll(value: Option<&str>) -> gpui::OverscrollBehavior {
    match value {
        Some("contain") => gpui::OverscrollBehavior::Contain,
        Some("none") => gpui::OverscrollBehavior::None,
        _ => gpui::OverscrollBehavior::Auto,
    }
}

/// The vertical `overscrollBehavior` of a style: the axis value, else the shorthand.
pub(crate) fn overscroll_y(style: &StyleDesc) -> gpui::OverscrollBehavior {
    overscroll(
        style
            .overscroll_behavior_y
            .as_deref()
            .or(style.overscroll_behavior.as_deref()),
    )
}

/// Makes `el` a scroll container if its style asks for one, and drops the handle of an
/// element that no longer is.
///
/// The handle outlives the frame in `handles`, which is how GPUI keeps the offset between
/// renders. Overflow never changes `display` or the element's children: a scroller lays out
/// exactly as it would without `overflow`.
pub(crate) fn apply_container(
    mut el: gpui::Stateful<gpui::Div>,
    id: u64,
    style: Option<&StyleDesc>,
    handles: &mut HashMap<u64, gpui::ScrollHandle>,
) -> gpui::Stateful<gpui::Div> {
    let Some(style) = style else {
        handles.remove(&id);
        return el;
    };
    let axes = Axes::of(style);
    if !axes.scrolls() {
        handles.remove(&id);
        return el;
    }
    el = match (axes.x.has_offset(), axes.y.has_offset()) {
        (true, true) => el.overflow_scroll(),
        (true, false) => el.overflow_x_scroll(),
        _ => el.overflow_y_scroll(),
    };
    el = el.programmatic_scroll_only(axes.x == Overflow::Hidden, axes.y == Overflow::Hidden);
    // A browser moves both axes of a diagonal wheel, and gives each axis only its own delta.
    // This is also what makes GPUI hand the unused delta to an ancestor.
    el.style().allow_concurrent_scroll = Some(true);
    let both = style.overscroll_behavior.as_deref();
    el = el.overscroll_behavior(
        overscroll(style.overscroll_behavior_x.as_deref().or(both)),
        overscroll(style.overscroll_behavior_y.as_deref().or(both)),
    );
    SMOOTH_BY_DEFAULT.with(|smooth| {
        if style.scroll_behavior.as_deref() == Some("smooth") {
            smooth.borrow_mut().insert(id);
        }
    });
    let handle = handles.entry(id).or_insert_with(gpui::ScrollHandle::new);
    el.track_scroll(handle)
}

/// `[scrollLeft, scrollTop, scrollWidth, scrollHeight, clientWidth, clientHeight]` as the DOM
/// reports them. `painted` is the border box of an element that does not scroll, whose
/// content is as big as it is.
pub(crate) fn metrics(
    handle: Option<&gpui::ScrollHandle>,
    painted: Option<(f32, f32)>,
) -> Option<[f64; 6]> {
    let Some(handle) = handle else {
        let (width, height) = painted?;
        return Some([0.0, 0.0, width as f64, height as f64, width as f64, height as f64]);
    };
    let client = handle.padding_box().size;
    let max = handle.max_offset();
    let offset = clamp_offset(handle.offset(), max);
    let (width, height) = (f32::from(client.width), f32::from(client.height));
    Some([
        // `0.0 -` rather than negation: an offset of 0 must read +0, not -0.
        0.0 - f64::from(f32::from(offset.x)),
        0.0 - f64::from(f32::from(offset.y)),
        f64::from(width + f32::from(max.x)),
        f64::from(height + f32::from(max.y)),
        f64::from(width),
        f64::from(height),
    ])
}

fn clamp_offset(
    offset: gpui::Point<gpui::Pixels>,
    max: gpui::Point<gpui::Pixels>,
) -> gpui::Point<gpui::Pixels> {
    gpui::point(
        offset.x.clamp(-max.x, gpui::px(0.0)),
        offset.y.clamp(-max.y, gpui::px(0.0)),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Behavior {
    /// Follow the scroller's `scrollBehavior` style.
    Auto,
    Instant,
    Smooth,
}

impl Behavior {
    pub(crate) fn parse(value: Option<&str>) -> Result<Self, String> {
        match value {
            None | Some("auto") => Ok(Self::Auto),
            Some("instant") => Ok(Self::Instant),
            Some("smooth") => Ok(Self::Smooth),
            Some(other) => Err(format!(
                "Unknown scroll behavior \"{other}\", expected \"auto\", \"instant\" or \"smooth\""
            )),
        }
    }
}

thread_local! {
    /// Scrollers whose `scrollBehavior` style is `smooth`, for calls that name no behavior.
    /// Rebuilt every frame by `apply_container`. Thread-local like `SCROLL_HANDLES`, so the
    /// same napi calls can reach it without a window update.
    static SMOOTH_BY_DEFAULT: RefCell<HashSet<u64>> = RefCell::new(HashSet::new());
    static ANIMATIONS: RefCell<HashMap<u64, Animation>> = RefCell::new(HashMap::new());
}

/// Call before building a frame, so a scroller that lost `scrollBehavior: "smooth"` or was
/// removed does not keep it.
pub(crate) fn begin_frame() {
    SMOOTH_BY_DEFAULT.with(|smooth| smooth.borrow_mut().clear());
}

fn is_smooth(id: u64, behavior: Behavior) -> bool {
    match behavior {
        Behavior::Smooth => true,
        Behavior::Instant => false,
        Behavior::Auto => SMOOTH_BY_DEFAULT.with(|smooth| smooth.borrow().contains(&id)),
    }
}

/// Moves a scroller to `target` (an offset: zero, then negative), instantly or by a smooth
/// animation. A new call replaces the animation in flight. The offset is not clamped here: the
/// next layout clamps it against the content as it is by then, which a caller that has just
/// changed the content needs.
pub(crate) fn scroll_to(
    id: u64,
    handle: &gpui::ScrollHandle,
    target: gpui::Point<gpui::Pixels>,
    behavior: Behavior,
) {
    ANIMATIONS.with(|animations| animations.borrow_mut().remove(&id));
    if !is_smooth(id, behavior) {
        handle.set_offset(target);
        return;
    }
    ANIMATIONS.with(|animations| {
        animations.borrow_mut().insert(
            id,
            Animation {
                handle: handle.clone(),
                from: handle.offset(),
                to: target,
                started: None,
                duration: Duration::ZERO,
                written: handle.offset(),
            },
        );
    });
}

struct Animation {
    handle: gpui::ScrollHandle,
    from: gpui::Point<gpui::Pixels>,
    to: gpui::Point<gpui::Pixels>,
    /// Set by the first frame that sees the animation, so it runs on the clock that frame uses.
    started: Option<Instant>,
    duration: Duration,
    /// What this animation last wrote. An offset that differs belongs to someone else (the
    /// wheel, a scrollbar, an instant `scrollTo`), which ends the animation.
    written: gpui::Point<gpui::Pixels>,
}

/// The shortest and longest a smooth scroll takes. A longer distance takes longer, up to the cap.
const SMOOTH_MIN: Duration = Duration::from_millis(300);
const SMOOTH_MAX: Duration = Duration::from_millis(450);
const SMOOTH_MS_PER_PX: f32 = 0.05;
/// Material's standard curve: quick to get going, long to settle, like a browser's.
const SMOOTH_CURVE: [f64; 4] = [0.4, 0.0, 0.2, 1.0];

/// Advances the smooth scrolls to `now`. True while one still has frames to run.
pub(crate) fn advance_animations(now: Instant) -> bool {
    ANIMATIONS.with(|animations| {
        let mut animations = animations.borrow_mut();
        animations.retain(|_, animation| advance(animation, now));
        !animations.is_empty()
    })
}

fn advance(animation: &mut Animation, now: Instant) -> bool {
    let current = animation.handle.offset();
    if animation.started.is_some() && current != animation.written {
        return false;
    }
    let started = match animation.started {
        Some(started) => started,
        None => {
            let distance = f32::from(animation.to.x - animation.from.x)
                .abs()
                .max(f32::from(animation.to.y - animation.from.y).abs());
            let extra = Duration::from_secs_f32(distance * SMOOTH_MS_PER_PX / 1000.0);
            animation.duration = (SMOOTH_MIN + extra).min(SMOOTH_MAX);
            animation.started = Some(now);
            now
        }
    };
    let progress = (now.saturating_duration_since(started).as_secs_f64()
        / animation.duration.as_secs_f64())
    .min(1.0);
    // The curve is solved numerically; the ends are exact so a finished scroll lands on its target.
    let eased = if progress <= 0.0 {
        0.0
    } else if progress >= 1.0 {
        1.0
    } else {
        crate::motion::cubic_bezier(progress, SMOOTH_CURVE) as f32
    };
    let lerp = |from: gpui::Pixels, to: gpui::Pixels| from + (to - from) * eased;
    let next = clamp_offset(
        gpui::point(
            lerp(animation.from.x, animation.to.x),
            lerp(animation.from.y, animation.to.y),
        ),
        animation.handle.max_offset(),
    );
    animation.handle.set_offset(next);
    animation.written = next;
    progress < 1.0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Align {
    Start,
    Center,
    End,
    Nearest,
}

impl Align {
    pub(crate) fn parse(value: Option<&str>, default: Self) -> Result<Self, String> {
        match value {
            None => Ok(default),
            Some("start") => Ok(Self::Start),
            Some("center") => Ok(Self::Center),
            Some("end") => Ok(Self::End),
            Some("nearest") => Ok(Self::Nearest),
            Some(other) => Err(format!(
                "Unknown scroll alignment \"{other}\", expected \"start\", \"center\", \"end\" or \"nearest\""
            )),
        }
    }
}

/// What `scrollIntoView` was asked for, with the DOM's defaults.
#[derive(Clone, Copy, Debug)]
pub(crate) struct IntoView {
    pub block: Align,
    pub inline: Align,
    pub behavior: Behavior,
}

impl IntoView {
    pub(crate) fn parse(
        block: Option<&str>,
        inline: Option<&str>,
        behavior: Option<&str>,
    ) -> Result<Self, String> {
        Ok(Self {
            block: Align::parse(block, Align::Start)?,
            inline: Align::parse(inline, Align::Nearest)?,
            behavior: Behavior::parse(behavior)?,
        })
    }
}

pub(crate) fn parse_touch_phase(value: Option<&str>) -> Result<gpui::TouchPhase, String> {
    match value {
        // Each simulated wheel call is a gesture of its own unless the caller says it
        // continues one, so a test that wheels two scrollers in a row is not latched to the first.
        None | Some("started") => Ok(gpui::TouchPhase::Started),
        Some("moved") => Ok(gpui::TouchPhase::Moved),
        Some("ended") => Ok(gpui::TouchPhase::Ended),
        Some("cancelled") => Ok(gpui::TouchPhase::Cancelled),
        Some(other) => Err(format!(
            "Unknown touch phase \"{other}\", expected \"started\", \"moved\", \"ended\" or \"cancelled\""
        )),
    }
}

/// The change in `scrollTop` (or `scrollLeft`) that puts the target (`target_start..target_end`)
/// in the scrollport (`port_start..port_end`) as `align` asks. All in the same coordinates.
/// `Nearest` follows CSSOM View.
pub(crate) fn align_delta(
    align: Align,
    target_start: f32,
    target_end: f32,
    port_start: f32,
    port_end: f32,
) -> f32 {
    match align {
        Align::Start => target_start - port_start,
        Align::End => target_end - port_end,
        Align::Center => (target_start + target_end) / 2.0 - (port_start + port_end) / 2.0,
        Align::Nearest => {
            let before = target_start < port_start;
            let after = target_end > port_end;
            let fits = target_end - target_start < port_end - port_start;
            match (before, after) {
                (false, false) | (true, true) => 0.0,
                (true, false) if fits => target_start - port_start,
                (true, false) => target_end - port_end,
                (false, true) if fits => target_end - port_end,
                (false, true) => target_start - port_start,
            }
        }
    }
}

/// What a scrolling key asks for, with the amounts a browser uses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct KeyScroll {
    vertical: bool,
    amount: KeyAmount,
    /// Space also activates a focused control, which wins over scrolling.
    pub(crate) activates_controls: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum KeyAmount {
    Pixels(f32),
    /// Fraction of the client size.
    Pages(f32),
    Start,
    End,
}

const ARROW_STEP: f32 = 40.0;
const PAGE_FRACTION: f32 = 0.9;

impl KeyScroll {
    /// `None` for a key that does not scroll. `key` is the GPUI key name.
    pub(crate) fn parse(key: &str, shift: bool) -> Option<Self> {
        let (vertical, amount) = match key {
            "up" => (true, KeyAmount::Pixels(-ARROW_STEP)),
            "down" => (true, KeyAmount::Pixels(ARROW_STEP)),
            "left" => (false, KeyAmount::Pixels(-ARROW_STEP)),
            "right" => (false, KeyAmount::Pixels(ARROW_STEP)),
            "pageup" => (true, KeyAmount::Pages(-PAGE_FRACTION)),
            "pagedown" => (true, KeyAmount::Pages(PAGE_FRACTION)),
            "space" => (true, KeyAmount::Pages(if shift { -PAGE_FRACTION } else { PAGE_FRACTION })),
            "home" => (true, KeyAmount::Start),
            "end" => (true, KeyAmount::End),
            _ => return None,
        };
        Some(Self {
            vertical,
            amount,
            activates_controls: key == "space",
        })
    }

    /// Where `handle` would scroll to, or `None` if it cannot move that way.
    fn target(self, handle: &gpui::ScrollHandle) -> Option<gpui::Point<gpui::Pixels>> {
        let offset = handle.offset();
        let max = handle.max_offset();
        let client = handle.padding_box().size;
        let user = handle.user_scrollable();
        if !(if self.vertical { user.y } else { user.x }) {
            return None;
        }
        let (position, range, size) = if self.vertical {
            (-f32::from(offset.y), f32::from(max.y), f32::from(client.height))
        } else {
            (-f32::from(offset.x), f32::from(max.x), f32::from(client.width))
        };
        let next = match self.amount {
            KeyAmount::Pixels(step) => position + step,
            KeyAmount::Pages(fraction) => position + fraction * size,
            KeyAmount::Start => 0.0,
            KeyAmount::End => range,
        }
        .clamp(0.0, range.max(0.0));
        if (next - position).abs() < 0.01 {
            return None;
        }
        // `0.0 -` rather than negation: an offset of 0 must be +0, not -0.
        Some(if self.vertical {
            gpui::point(offset.x, gpui::px(0.0 - next))
        } else {
            gpui::point(gpui::px(0.0 - next), offset.y)
        })
    }
}

/// Scrolls the first of `scrollers` (innermost first) that can move the way `key` asks.
pub(crate) fn scroll_by_key<'a>(
    scrollers: impl IntoIterator<Item = (u64, &'a gpui::ScrollHandle)>,
    key: KeyScroll,
) -> bool {
    for (id, handle) in scrollers {
        if let Some(target) = key.target(handle) {
            scroll_to(id, handle, target, Behavior::Auto);
            return true;
        }
    }
    false
}

/// A box in window coordinates.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Scrolls one scroll container so `target` sits where `options` ask, and moves `target` to
/// where that leaves it, for the next container out. False if the container could not move.
pub(crate) fn scroll_into_view(
    id: u64,
    handle: &gpui::ScrollHandle,
    target: &mut Rect,
    options: IntoView,
) -> bool {
    let port = handle.padding_box();
    let max = handle.max_offset();
    let offset = handle.offset();
    let along = |scrolls: bool, align, start: f32, length: f32, port_start: gpui::Pixels, port_end: gpui::Pixels| {
        if !scrolls {
            return 0.0;
        }
        align_delta(align, start, start + length, f32::from(port_start), f32::from(port_end))
    };
    let dx = along(
        f32::from(max.x) > 0.0,
        options.inline,
        target.x,
        target.width,
        port.left(),
        port.right(),
    );
    let dy = along(
        f32::from(max.y) > 0.0,
        options.block,
        target.y,
        target.height,
        port.top(),
        port.bottom(),
    );
    let next = clamp_offset(
        gpui::point(offset.x - gpui::px(dx), offset.y - gpui::px(dy)),
        max,
    );
    if next == offset {
        return false;
    }
    target.x += f32::from(next.x - offset.x);
    target.y += f32::from(next.y - offset.y);
    scroll_to(id, handle, next, options.behavior);
    true
}

/// The `offset_in_item` that scrolls a virtual list's row (`top`, `height` in window
/// coordinates, if it is painted) as `align` asks, given the list's viewport. `None` when
/// `Nearest` finds the row already in view. A row that is not painted cannot be measured, so it
/// goes to the top of the viewport.
pub(crate) fn virtual_list_offset(
    align: Align,
    row: Option<(f32, f32)>,
    viewport: (f32, f32),
) -> Option<f32> {
    let Some((top, height)) = row else {
        return Some(0.0);
    };
    let (port_top, port_height) = viewport;
    let delta = align_delta(align, top, top + height, port_top, port_top + port_height);
    if align == Align::Nearest && delta == 0.0 {
        return None;
    }
    Some(port_top + delta - top)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_list_rows_align_like_scroll_container_targets() {
        // A 20px row at y=120 in a viewport at y=20 that is 80 tall.
        let row = Some((120.0, 20.0));
        assert_eq!(virtual_list_offset(Align::Start, row, (20.0, 80.0)), Some(0.0));
        assert_eq!(virtual_list_offset(Align::End, row, (20.0, 80.0)), Some(-60.0));
        assert_eq!(virtual_list_offset(Align::Nearest, Some((40.0, 20.0)), (20.0, 80.0)), None);
        assert_eq!(virtual_list_offset(Align::Start, None, (20.0, 80.0)), Some(0.0));
    }

    fn style(overflow: Option<&str>, x: Option<&str>, y: Option<&str>) -> StyleDesc {
        StyleDesc {
            overflow: overflow.map(str::to_string),
            overflow_x: x.map(str::to_string),
            overflow_y: y.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn auto_scrolls_like_scroll_and_hidden_is_a_scroll_container_the_wheel_ignores() {
        assert_eq!(Overflow::parse(Some("auto")), Overflow::Scroll);
        assert_eq!(Overflow::parse(Some("scroll")), Overflow::Scroll);
        assert_eq!(Overflow::parse(Some("clip")), Overflow::Clip);
        assert_eq!(Overflow::parse(Some("hidden")), Overflow::Hidden);
        assert!(Overflow::Hidden.has_offset());
        assert!(!Overflow::Clip.has_offset());
        assert_eq!(Overflow::parse(Some("visible")), Overflow::Visible);
        assert_eq!(Overflow::parse(None), Overflow::Visible);
    }

    #[test]
    fn axis_overflow_overrides_the_shorthand() {
        let axes = Axes::of(&style(Some("scroll"), None, Some("hidden")));
        assert_eq!((axes.x, axes.y), (Overflow::Scroll, Overflow::Hidden));
        assert!(axes.scrolls());
        assert!(Axes::of(&style(Some("hidden"), None, None)).scrolls());
        assert!(!Axes::of(&style(Some("clip"), None, None)).scrolls());
    }

    #[test]
    fn nearest_does_nothing_for_a_target_in_view_or_covering_the_port() {
        assert_eq!(align_delta(Align::Nearest, 20.0, 40.0, 0.0, 100.0), 0.0);
        assert_eq!(align_delta(Align::Nearest, -10.0, 150.0, 0.0, 100.0), 0.0);
    }

    #[test]
    fn nearest_scrolls_the_shortest_way() {
        assert_eq!(align_delta(Align::Nearest, 130.0, 150.0, 0.0, 100.0), 50.0);
        assert_eq!(align_delta(Align::Nearest, -30.0, -10.0, 0.0, 100.0), -30.0);
        // A target taller than the port lines up with the edge it crossed last.
        assert_eq!(align_delta(Align::Nearest, 50.0, 250.0, 0.0, 100.0), 50.0);
    }

    #[test]
    fn start_center_end_align_the_target() {
        assert_eq!(align_delta(Align::Start, 120.0, 140.0, 20.0, 100.0), 100.0);
        assert_eq!(align_delta(Align::End, 120.0, 140.0, 20.0, 100.0), 40.0);
        assert_eq!(align_delta(Align::Center, 120.0, 140.0, 20.0, 100.0), 70.0);
    }
}
