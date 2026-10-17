//! Shared automation host: paint bounds and a controllable motion clock.
//!
//! Record bounds during **paint**, not prepaint. The frame reset canvas
//! clears the map in paint, and GPUI prepaint runs for the whole tree
//! before any paint. A prepaint recorder would be wiped by the reset.
//!
//! TestGpuixRenderer and GpuixRenderer both use this so locators, screenshots,
//! and clock control do not fork between headless tests and a live window.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{
    canvas, point, px, App, Bounds, InputEvent, IntoElement, KeyDownEvent, KeyUpEvent, LayoutInsets,
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput,
    Styled, Window, WindowId,
};
use web_time::Instant;

use crate::events::key_names::Press;
use crate::layout::containing_block::Position;

#[derive(Clone, Copy, Debug)]
pub struct ElementBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// The DOM's `clientLeft`: the left border's width as laid out, so the padding box starts at `x + client_left`.
    pub client_left: f64,
    pub client_top: f64,
    pub border_right: f64,
    pub border_bottom: f64,
    /// The `position` the element was built with once its state styles were folded in. `None`
    /// when the builder did not say, and the declared one stands.
    pub position: Option<Position>,
}

impl ElementBounds {
    fn from_gpui(bounds: Bounds<Pixels>, insets: LayoutInsets) -> Self {
        Self {
            x: f64::from(f32::from(bounds.origin.x)),
            y: f64::from(f32::from(bounds.origin.y)),
            width: f64::from(f32::from(bounds.size.width)),
            height: f64::from(f32::from(bounds.size.height)),
            client_left: f64::from(f32::from(insets.border.left)),
            client_top: f64::from(f32::from(insets.border.top)),
            border_right: f64::from(f32::from(insets.border.right)),
            border_bottom: f64::from(f32::from(insets.border.bottom)),
            position: None,
        }
    }
}

thread_local! {
    /// Keyed by window as well as element id: element ids are only unique within one renderer,
    /// so two windows would otherwise read each other's boxes.
    static BOUNDS: RefCell<HashMap<(WindowId, u64), ElementBounds>> = RefCell::new(HashMap::new());
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

fn forget_window(window: WindowId) {
    BOUNDS.with(|cell| cell.borrow_mut().retain(|key, _| key.0 != window));
}

/// Record this element's own painted box, with no extra element in the tree.
///
/// A leaf such as `gpui::img` cannot have a canvas child to measure it.
/// Wrapping the leaf in a div instead would move the layout box: the wrapper
/// would become the flex item and the image would lose intrinsic sizing and
/// corner clipping.
pub fn track_own_bounds<E: gpui::InteractiveElement + gpui::Styled>(
    el: E,
    id: u64,
    report: Option<PaintReporter>,
) -> E {
    el.on_painted_layout(move |bounds, layout_id, window, cx| {
        let insets = window.layout_insets(layout_id);
        record_bounds(window.window_handle().window_id(), id, bounds, insets);
        if let Some(report) = &report {
            report(bounds, insets, window, cx);
        }
    })
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
/// reports.
///
/// `position` is the `position` the element is built with, state styles folded in; `None` leaves
/// the declared one to stand for a reader of the retained tree.
///
/// `report` sees the painted box right after it is recorded. `on_painted_layout`
/// takes one listener, so the paint-driven events share this one.
pub fn track_host_bounds<E>(
    el: E,
    id: u64,
    selection_start: Option<bool>,
    position: Option<Position>,
    report: Option<PaintReporter>,
) -> E
where
    E: gpui::InteractiveElement + gpui::Styled,
{
    el.on_painted_layout(move |bounds, layout_id, window, cx| {
        let insets = window.layout_insets(layout_id);
        let window_id = window.window_handle().window_id();
        record_bounds(window_id, id, bounds, insets);
        if let Some(position) = position {
            record_position(window_id, id, position);
        }
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
/// out with, on every paint, from inside paint. It is how `onLayout`,
/// `onScroll` and the observers see what the frame produced. The window is the
/// one being painted, so a reporter can read the content mask the element was
/// painted under.
pub type PaintReporter = Rc<dyn Fn(Bounds<Pixels>, LayoutInsets, &mut Window, &mut App)>;

pub fn record_bounds(window: WindowId, id: u64, bounds: Bounds<Pixels>, insets: LayoutInsets) {
    BOUNDS.with(|cell| {
        cell.borrow_mut()
            .insert((window, id), ElementBounds::from_gpui(bounds, insets));
    });
}

fn record_position(window: WindowId, id: u64, position: Position) {
    BOUNDS.with(|cell| {
        if let Some(bounds) = cell.borrow_mut().get_mut(&(window, id)) {
            bounds.position = Some(position);
        }
    });
}

pub fn get_bounds(window: WindowId, id: u64) -> Option<ElementBounds> {
    BOUNDS.with(|cell| cell.borrow().get(&(window, id)).copied())
}

pub fn all_bounds(window: WindowId) -> HashMap<u64, ElementBounds> {
    BOUNDS.with(|cell| {
        cell.borrow()
            .iter()
            .filter(|(key, _)| key.0 == window)
            .map(|(key, bounds)| (key.1, *bounds))
            .collect()
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
