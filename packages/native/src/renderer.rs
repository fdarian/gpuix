//! GPUIX retained renderer for napi desktop hosts and GPUI's browser platform.
//!
//! Mutation-based API: React's reconciler sends individual mutations
//! (createElement, appendChild, setStyle, etc.) instead of a full JSON tree.
//! Rust maintains a RetainedTree and rebuilds GPUI elements from it each frame.
//!
//! Desktop lifecycle:
//!   const renderer = new GpuixRenderer(eventCallback)
//!   renderer.init({ title: 'My App', width: 800, height: 600 })
//!   renderer.applyBatch(json)             // one atomic React commit
//!   startFrameLoop(renderer)             // macOS wakes on vsync; Win/Linux polls UI thread
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use futures::channel::oneshot;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
use futures::{channel::mpsc, StreamExt as _};
use gpui::AppContext as _;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use napi::bindgen_prelude::*;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use napi::JsDeferred;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use napi_derive::napi;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
use std::sync::mpsc::{sync_channel, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use wasm_bindgen::JsCast as _;

use crate::accessibility::{apply_a11y_click, apply_accessibility};
use crate::css::length::LengthContext;
use crate::custom_elements::{CustomElementRegistry, CustomRenderContext};
use crate::events::app_region::AppRegion;
use crate::element_tree::EventPayload;
use crate::retained_tree::{RetainedTree, StyleTable};
use crate::style::StyleDesc;
use crate::text::white_space::StyledWhiteSpace as _;
use crate::text::{selectable_text, selection_frame_reset, SharedSelection};
use crate::theme::Theme;

/// The Window menu items act on the focused window, and the root element is the
/// only place in GPUIX that has one. `crate::app_menu` owns everything else.
#[cfg(target_os = "macos")]
fn with_window_menu_actions(root: gpui::Div) -> gpui::Div {
    use crate::app_menu::{CloseWindow, MinimizeWindow, ZoomWindow};
    use gpui::prelude::*;

    root.on_action(|_: &MinimizeWindow, window, _cx| window.minimize_window())
        .on_action(|_: &ZoomWindow, window, _cx| window.zoom_window())
        .on_action(|_: &CloseWindow, window, _cx| window.remove_window())
}

#[cfg(not(target_os = "macos"))]
fn with_window_menu_actions(root: gpui::Div) -> gpui::Div {
    root
}

pub(crate) fn parse_font_style(value: &str) -> Option<gpui::FontStyle> {
    match value.trim().to_ascii_lowercase().as_str() {
        "normal" => Some(gpui::FontStyle::Normal),
        "italic" => Some(gpui::FontStyle::Italic),
        "oblique" => Some(gpui::FontStyle::Oblique),
        _ => None,
    }
}

/// Parse a CSS font-weight value (string or number) into a GPUI FontWeight.
/// Accepts named keywords ("bold", "semibold"), numeric strings ("700"),
/// and raw numbers (700). Falls back to 400 (normal) for unrecognized values.
pub(crate) fn parse_font_weight(value: &crate::style::FontWeightValue) -> gpui::FontWeight {
    match value {
        crate::style::FontWeightValue::Num(n) => gpui::FontWeight((*n as f32).clamp(1.0, 1000.0)),
        crate::style::FontWeightValue::Str(s) => {
            let lower = s.trim().to_ascii_lowercase();
            match lower.as_str() {
                "100" | "thin" => gpui::FontWeight(100.0),
                "200" | "extralight" | "extra-light" => gpui::FontWeight(200.0),
                "300" | "light" => gpui::FontWeight(300.0),
                "400" | "normal" => gpui::FontWeight(400.0),
                "500" | "medium" => gpui::FontWeight(500.0),
                "600" | "semibold" | "semi-bold" => gpui::FontWeight(600.0),
                "700" | "bold" => gpui::FontWeight(700.0),
                "800" | "extrabold" | "extra-bold" => gpui::FontWeight(800.0),
                "900" | "black" => gpui::FontWeight(900.0),
                _ => lower
                    .parse::<f32>()
                    .map(|n| gpui::FontWeight(n.clamp(1.0, 1000.0)))
                    .unwrap_or(gpui::FontWeight(400.0)),
            }
        }
    }
}

/// Abstracted event callback shared by desktop, browser, and test renderers.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) type EventCallback = Arc<dyn Fn(EventPayload) + Send + Sync>;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) type EventCallback = Rc<dyn Fn(EventPayload)>;

fn raw_dimension_u32(value: f64, label: &str) -> std::result::Result<u32, String> {
    if !value.is_finite() || value <= 0.0 || value.fract() != 0.0 || value > u32::MAX as f64 {
        return Err(format!(
            "Image {label} must be a positive integer, got {value}"
        ));
    }
    Ok(value as u32)
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn dimension_u32(value: f64, label: &str) -> Result<u32> {
    raw_dimension_u32(value, label).map_err(Error::from_reason)
}

fn raw_element_id(id: f64) -> std::result::Result<u64, String> {
    if !id.is_finite() || id < 0.0 || id.fract() != 0.0 || id > 9_007_199_254_740_991.0 {
        return Err(format!("Invalid element id: {id}"));
    }
    Ok(id as u64)
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn to_element_id(id: f64) -> Result<u64> {
    raw_element_id(id).map_err(Error::from_reason)
}

thread_local! {
    #[cfg(target_os = "macos")]
    static MAC_PLATFORM: RefCell<Option<Rc<gpui_macos::MacPlatform>>> = const { RefCell::new(None) };
    #[cfg(target_os = "macos")]
    static GPUI_APP: RefCell<Option<gpui::ApplicationHandle>> = const { RefCell::new(None) };
    #[cfg(target_os = "macos")]
    static GPUI_WINDOW: RefCell<Option<gpui::WindowHandle<GpuixView>>> = const { RefCell::new(None) };
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    static WEB_APP: RefCell<Option<gpui::ApplicationHandle>> = const { RefCell::new(None) };
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    static WEB_WINDOW: RefCell<Option<gpui::WindowHandle<GpuixView>>> = const { RefCell::new(None) };
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    static PENDING_DEBUG_OVERLAY: RefCell<Option<gpui::DebugFrameOverlayMode>> =
        const { RefCell::new(None) };
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    static PENDING_WINDOW_KEY_EVENTS: RefCell<Option<(bool, bool, u64)>> =
        const { RefCell::new(None) };
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    static PENDING_WINDOW_SELECTION_CHANGE: RefCell<Option<(bool, u64)>> =
        const { RefCell::new(None) };
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    static PENDING_WINDOW_APPEARANCE_CHANGE: RefCell<Option<(bool, u64)>> =
        const { RefCell::new(None) };
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    static PENDING_FOCUS_ELEMENT: RefCell<Option<u64>> = const { RefCell::new(None) };
    /// Shared scroll handles — GpuixView writes here during render(),
    /// platform-local handlers read from here for programmatic scroll control.
    /// ScrollHandle is Rc<RefCell<...>> so its methods (set_offset, offset,
    /// scroll_to_item) work without an App context.
    ///
    /// NOTE: This is a singleton — if multiple renderers/windows coexist,
    /// the last one to render wins. Acceptable for now (single-window only).
    /// TODO: Scope by renderer/window ID when multi-window support is added.
    static SCROLL_HANDLES: RefCell<HashMap<u64, gpui::ScrollHandle>> = RefCell::new(HashMap::new());
    static VIRTUAL_LIST_STATES: RefCell<HashMap<u64, gpui::ListState>> = RefCell::new(HashMap::new());
    /// Virtual-list scrolls queued for the next `GpuixView::render`, applied
    /// AFTER `VirtualListEntry::sync` splices that frame's child changes.
    ///
    /// Never applied eagerly: JS computes row indices against the child list it
    /// just committed, but that commit only reaches `gpui::ListState` when the
    /// next render splices it in. An eager `scroll_to` would be shifted a
    /// second time by `splice_focusable` and land on the wrong row.
    static PENDING_VIRTUAL_LIST_SCROLLS: RefCell<HashMap<u64, gpui::ListOffset>> =
        RefCell::new(HashMap::new());
}

const SELECTION_SCROLL_TICK_MS: u64 = 24;
const SELECTION_SCROLL_EDGE_PX: f32 = 36.0;
const SELECTION_SCROLL_MAX_STEP_PX: f32 = 24.0;

/// Signed list scroll step for a pointer near a viewport edge.
fn selection_scroll_step(
    bounds: gpui::Bounds<gpui::Pixels>,
    position: gpui::Point<gpui::Pixels>,
) -> f32 {
    let height = f32::from(bounds.size.height);
    if height <= 0.0 {
        return 0.0;
    }
    let edge = SELECTION_SCROLL_EDGE_PX.min(height / 6.0);
    if edge <= 0.0 {
        return 0.0;
    }
    let y = f32::from(position.y);
    let top = f32::from(bounds.top());
    let bottom = f32::from(bounds.bottom());
    let scaled = |penetration: f32| {
        let progress = (penetration / edge).clamp(0.0, 1.0);
        SELECTION_SCROLL_MAX_STEP_PX * progress * progress
    };
    if y < top + edge {
        -scaled(top + edge - y)
    } else if y > bottom - edge {
        scaled(y - (bottom - edge))
    } else {
        0.0
    }
}

/// Queue a virtual-list scroll for the next render. `offset_in_item` may be
/// negative: gpui then anchors the viewport top above the item, which is what
/// keeps a row pixel-stable while unmeasured rows are spliced in above it.
pub(crate) fn queue_virtual_list_scroll(id: u64, index: usize, offset_in_item: f32) {
    PENDING_VIRTUAL_LIST_SCROLLS.with(|cell| {
        cell.borrow_mut().insert(
            id,
            gpui::ListOffset {
                item_ix: index,
                offset_in_item: gpui::px(offset_in_item),
            },
        );
    });
}

/// The DOM scroll position `[scrollLeft, scrollTop]` of the virtual list or scroll container
/// `id`, or `None` for anything else.
fn scroll_element_position(id: u64) -> Option<[f64; 2]> {
    VIRTUAL_LIST_STATES
        .with(|cell| {
            cell.borrow()
                .get(&id)
                .map(|state| state.scroll_px_offset_for_scrollbar())
        })
        .or_else(|| SCROLL_HANDLES.with(|cell| cell.borrow().get(&id).map(|handle| handle.offset())))
        .map(crate::scroll::dom_position)
}

/// Scrolls the virtual list or scroll container `id` to the DOM scroll position `left`, `top`.
fn scroll_element_to(id: u64, left: f64, top: f64, behavior: crate::scroll::Behavior) {
    let offset = crate::scroll::gpui_offset(left, top);
    if VIRTUAL_LIST_STATES.with(|cell| {
        let states = cell.borrow();
        let Some(state) = states.get(&id) else {
            return false;
        };
        state.set_offset_from_scrollbar(offset);
        true
    }) {
        return;
    }
    SCROLL_HANDLES.with(|cell| {
        if let Some(handle) = cell.borrow().get(&id) {
            crate::scroll::scroll_to(id, handle, offset, behavior);
        }
    });
}

/// `Element.scrollTop` and friends, from the last layout. See `scroll::metrics`.
///
/// Callers draw pending changes first (`draw_pending_changes`, or `draw_if_dirty` inside a
/// window command) so the layout is never older than the tree.
pub(crate) fn scroll_metrics(id: u64) -> Option<[f64; 6]> {
    let painted = crate::automation::get_bounds(id).map(|b| (b.width as f32, b.height as f32));
    SCROLL_HANDLES.with(|cell| crate::scroll::metrics(cell.borrow().get(&id), painted))
}

fn parse_debug_frame_overlay_mode_str(
    mode: &str,
) -> std::result::Result<gpui::DebugFrameOverlayMode, String> {
    match mode {
        "hidden" => Ok(gpui::DebugFrameOverlayMode::Hidden),
        "minimal" => Ok(gpui::DebugFrameOverlayMode::Minimal),
        "full" => Ok(gpui::DebugFrameOverlayMode::Full),
        other => Err(format!(
            "Unknown debug frame overlay mode {other:?}. Use hidden, minimal, or full."
        )),
    }
}

#[cfg(test)]
mod selection_scroll_tests {
    use super::*;

    #[test]
    fn selection_scroll_ramps_at_viewport_edges() {
        let bounds = gpui::Bounds::new(
            gpui::point(gpui::px(10.0), gpui::px(20.0)),
            gpui::size(gpui::px(300.0), gpui::px(200.0)),
        );
        assert_eq!(
            selection_scroll_step(bounds, gpui::point(gpui::px(20.0), gpui::px(120.0))),
            0.0
        );
        assert!(selection_scroll_step(bounds, gpui::point(gpui::px(20.0), gpui::px(20.0))) < 0.0);
        assert!(selection_scroll_step(bounds, gpui::point(gpui::px(20.0), gpui::px(220.0))) > 0.0);
        assert!(
            selection_scroll_step(bounds, gpui::point(gpui::px(20.0), gpui::px(220.0)))
                > selection_scroll_step(bounds, gpui::point(gpui::px(20.0), gpui::px(200.0)))
        );
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn parse_debug_frame_overlay_mode(mode: &str) -> Result<gpui::DebugFrameOverlayMode> {
    parse_debug_frame_overlay_mode_str(mode).map_err(Error::from_reason)
}

pub(crate) fn debug_frame_overlay_mode_name(mode: gpui::DebugFrameOverlayMode) -> &'static str {
    match mode {
        gpui::DebugFrameOverlayMode::Hidden => "hidden",
        gpui::DebugFrameOverlayMode::Minimal => "minimal",
        gpui::DebugFrameOverlayMode::Full => "full",
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn debug_frame_overlay_stats_js(
    stats: gpui::DebugFrameOverlayStats,
) -> DebugFrameOverlayStats {
    DebugFrameOverlayStats {
        current_ms: stats.current_ms.map(|ms| ms as f64),
        p90_ms: stats.p90_ms.map(|ms| ms as f64),
        p99_ms: stats.p99_ms.map(|ms| ms as f64),
        max_ms: stats.max_ms.map(|ms| ms as f64),
        frames: stats.frames as f64,
        samples: stats.samples as f64,
    }
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
fn recv_ui_response<T>(receiver: std::sync::mpsc::Receiver<T>, operation: &str) -> Result<T> {
    match receiver.recv_timeout(Duration::from_secs(2)) {
        Ok(response) => Ok(response),
        Err(RecvTimeoutError::Timeout) => Err(Error::from_reason(format!(
            "Timed out after 2 seconds waiting for {operation}"
        ))),
        Err(RecvTimeoutError::Disconnected) => Err(Error::from_reason(format!(
            "The GPUI UI thread stopped during {operation}"
        ))),
    }
}

/// `update_window` for a change that returns nothing. When it is called while the
/// root view is already being updated, it runs once that update has returned.
#[cfg(target_os = "macos")]
fn update_window_or_defer(
    update: impl FnOnce(&mut GpuixView, &mut gpui::Window, &mut gpui::Context<GpuixView>) + 'static,
) -> Result<()> {
    crate::events::sync_dispatch::route(
        update,
        crate::events::sync_dispatch::update_root_view_or_defer,
        Error::from_reason,
        update_window,
    )
}

#[cfg(target_os = "macos")]
fn update_window<R>(
    update: impl FnOnce(&mut GpuixView, &mut gpui::Window, &mut gpui::Context<GpuixView>) -> R,
) -> Result<R> {
    crate::events::sync_dispatch::native_call(|| {
        crate::events::sync_dispatch::route(
            update,
            crate::events::sync_dispatch::update_root_view,
            Error::from_reason,
            |update| {
                let window = GPUI_WINDOW
                    .with(|window| *window.borrow())
                    .ok_or_else(|| Error::from_reason("GPUI window is not initialized"))?;

                GPUI_APP.with(|app| {
                    let app = app.borrow();
                    let app = app.as_ref().ok_or_else(|| {
                        Error::from_reason("GPUI application is not initialized")
                    })?;
                    app.update(|cx| {
                        window
                            .update(cx, update)
                            .map_err(|error| Error::from_reason(error.to_string()))
                    })
                })
            },
        )
    })
}

#[cfg(target_os = "macos")]
// A sync input handler can reach the view through its own renderer calls, so the work that
// only needs the window runs without leasing the root view.
fn update_window_without_view<R>(
    update: impl FnOnce(&mut gpui::Window, &mut gpui::App) -> R,
) -> Result<R> {
    crate::events::sync_dispatch::native_call(|| {
        crate::events::sync_dispatch::route(
            update,
            crate::events::sync_dispatch::update_active_window,
            Error::from_reason,
            |update| {
                let window = GPUI_WINDOW
                    .with(|window| *window.borrow())
                    .ok_or_else(|| Error::from_reason("GPUI window is not initialized"))?;

                GPUI_APP.with(|app| {
                    let app = app.borrow();
                    let app = app.as_ref().ok_or_else(|| {
                        Error::from_reason("GPUI application is not initialized")
                    })?;
                    app.update(|cx| {
                        gpui::AnyWindowHandle::from(window)
                            .update(cx, move |_view, window, cx| update(window, cx))
                            .map_err(|error| Error::from_reason(error.to_string()))
                    })
                })
            },
        )
    })
}

#[cfg(target_os = "macos")]
/// Runs `update` against the app alone. From inside a sync handler the app is
/// already borrowed by the input being dispatched, so it uses that one.
fn update_app<R>(update: impl FnOnce(&mut gpui::App) -> R) -> Result<R> {
    crate::events::sync_dispatch::route(
        update,
        |update| {
            crate::events::sync_dispatch::update_active_window(|_window, cx| update(cx))
        },
        Error::from_reason,
        |update| {
            GPUI_APP.with(|app| {
                let app = app.borrow();
                let app = app
                    .as_ref()
                    .ok_or_else(|| Error::from_reason("GPUI application is not initialized"))?;
                Ok(app.update(update))
            })
        },
    )
}

#[cfg(target_os = "macos")]
fn invalidate_window() -> Result<()> {
    if crate::events::sync_dispatch::in_sync_call() {
        return crate::events::sync_dispatch::invalidate_root_view().map_err(Error::from_reason);
    }
    update_window(|_view, window, cx| {
        cx.notify();
        window.refresh();
    })
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
enum MouseInput {
    Click {
        x: f64,
        y: f64,
        button: u32,
        modifiers: gpui::Modifiers,
    },
    Down {
        x: f64,
        y: f64,
        button: u32,
        modifiers: gpui::Modifiers,
    },
    Up {
        x: f64,
        y: f64,
        button: u32,
        modifiers: gpui::Modifiers,
    },
    Move {
        x: f64,
        y: f64,
        pressed_button: Option<u32>,
        modifiers: gpui::Modifiers,
    },
    Wheel {
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
        modifiers: gpui::Modifiers,
        phase: gpui::TouchPhase,
    },
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
enum KeyInput {
    Keystrokes(String),
    Down { keystroke: String, is_held: bool },
    Up(String),
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
enum ClockControl {
    Pause,
    Set(f64),
    FastForward(f64),
    Resume,
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
enum UiCommand {
    Invalidate,
    ActivateWindow,
    MinimizeWindow,
    ZoomWindow,
    ToggleFullscreen,
    SetWindowTitle(String),
    PromptForPaths {
        options: gpui::PathPromptOptions,
        deferred: PathPromptDeferred,
    },
    WriteClipboardText {
        text: String,
        response: SyncSender<()>,
    },
    ReadClipboardText {
        response: SyncSender<Option<String>>,
    },
    SetDebugFrameOverlay(gpui::DebugFrameOverlayMode),
    CycleDebugFrameOverlay {
        response: SyncSender<String>,
    },
    GetDebugFrameOverlay {
        response: SyncSender<String>,
    },
    GetDebugFrameOverlayStats {
        response: SyncSender<DebugFrameOverlayStats>,
    },
    ResetDebugFrameOverlayStats,
    ScrollTo {
        id: u64,
        left: f64,
        top: f64,
        behavior: crate::scroll::Behavior,
    },
    ScrollToItem {
        id: u64,
        index: usize,
        offset: f32,
    },
    ScrollIntoView {
        id: u64,
        options: crate::scroll::IntoView,
    },
    SetImagePixels {
        id: u64,
        width: u32,
        height: u32,
        bytes: Vec<u8>,
        format: crate::custom_elements::img::PixelFormat,
        response: SyncSender<std::result::Result<(), String>>,
    },
    SetImage {
        id: u64,
        bytes: Vec<u8>,
        response: SyncSender<std::result::Result<(), String>>,
    },
    SetTextSelection {
        id: u64,
        request: crate::custom_elements::text_selection::SelectionRequest,
        response: SyncSender<std::result::Result<(), String>>,
    },
    GetTextSelection {
        id: u64,
        response: SyncSender<
            std::result::Result<crate::custom_elements::text_selection::TextSelection, String>,
        >,
    },
    RequestSubmit {
        id: u64,
        response: SyncSender<std::result::Result<(), String>>,
    },
    RegisterFont {
        bytes: Vec<u8>,
        response: SyncSender<std::result::Result<(), String>>,
    },
    GetScrollOffset {
        id: u64,
        response: SyncSender<Option<[f64; 2]>>,
    },
    GetScrollMetrics {
        id: u64,
        response: SyncSender<Option<[f64; 6]>>,
    },
    ScrollByKey {
        key: String,
        shift: bool,
        response: SyncSender<bool>,
    },
    GetListScrollTop {
        id: u64,
        response: SyncSender<Option<[f64; 3]>>,
    },
    GetAutomationBounds {
        response: SyncSender<HashMap<u64, crate::automation::ElementBounds>>,
    },
    GetWindowSize {
        response: SyncSender<WindowSize>,
    },
    GetElementBounds {
        id: u64,
        response: SyncSender<Option<crate::automation::ElementBounds>>,
    },
    FocusElement(u64),
    ClickElement {
        id: u64,
        response: SyncSender<std::result::Result<(), String>>,
    },
    BlurElement(u64),
    MatchesFocus {
        id: u64,
        selector: String,
        response: SyncSender<std::result::Result<bool, String>>,
    },
    FocusNext,
    FocusPrevious,
    GetFocusedElementId {
        response: SyncSender<Option<u64>>,
    },
    FocusNextWithin(u64),
    FocusPreviousWithin(u64),
    SetWindowKeyEvents {
        key_down: bool,
        key_up: bool,
        event_id: u64,
    },
    SetWindowSelectionChange {
        enabled: bool,
        event_id: u64,
    },
    SetWindowAppearanceChange {
        enabled: bool,
        event_id: u64,
    },
    SetWindowMediaChange {
        enabled: bool,
    },
    SetAppearance(Option<gpui::WindowAppearance>),
    GetAppearance {
        response: SyncSender<String>,
    },
    ControlClock {
        control: ClockControl,
        response: SyncSender<f64>,
    },
    MotionValues {
        run: Box<
            dyn FnOnce(&mut GpuixView, &mut gpui::Window, &mut gpui::Context<GpuixView>) + Send,
        >,
    },
    DispatchMouse {
        input: MouseInput,
        response: SyncSender<std::result::Result<(), String>>,
    },
    DispatchKey {
        input: KeyInput,
        response: SyncSender<std::result::Result<(), String>>,
    },
    #[cfg(all(target_os = "windows", feature = "test-support"))]
    CaptureScreenshot {
        path: String,
        response: SyncSender<std::result::Result<(), String>>,
    },
    Blur,
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
fn refresh_ui_window(
    window: gpui::WindowHandle<GpuixView>,
    cx: &mut gpui::AsyncApp,
) -> anyhow::Result<()> {
    window.update(cx, |_view, window, cx| {
        cx.notify();
        window.refresh();
    })
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
async fn run_ui_commands(
    mut commands: mpsc::UnboundedReceiver<UiCommand>,
    window: gpui::WindowHandle<GpuixView>,
    cx: &mut gpui::AsyncApp,
) {
    while let Some(command) = commands.next().await {
        let result = match command {
            UiCommand::Invalidate => refresh_ui_window(window, cx),
            UiCommand::ActivateWindow => window.update(cx, |_view, window, cx| {
                cx.activate(true);
                window.activate_window();
            }),
            UiCommand::MinimizeWindow => {
                window.update(cx, |_view, window, _cx| window.minimize_window())
            }
            UiCommand::ZoomWindow => window.update(cx, |_view, window, _cx| window.zoom_window()),
            UiCommand::ToggleFullscreen => {
                window.update(cx, |_view, window, _cx| window.toggle_fullscreen())
            }
            UiCommand::SetWindowTitle(title) => window.update(cx, move |view, window, cx| {
                view.window_title = title;
                cx.notify();
                window.refresh();
            }),
            UiCommand::PromptForPaths { options, deferred } => {
                let background_executor = cx.background_executor().clone();
                let receiver = cx.update(|cx| cx.prompt_for_paths(options));
                settle_path_prompt(background_executor, receiver, deferred);
                Ok(())
            }
            UiCommand::WriteClipboardText { text, response } => {
                cx.update(|cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string(text)));
                response.send(()).ok();
                Ok(())
            }
            UiCommand::ReadClipboardText { response } => {
                let text = cx.update(|cx| cx.read_from_clipboard().and_then(|item| item.text()));
                response.send(text).ok();
                Ok(())
            }
            UiCommand::SetDebugFrameOverlay(mode) => {
                window.update(cx, move |_view, window, _cx| {
                    window.set_debug_frame_overlay_mode(mode);
                })
            }
            UiCommand::CycleDebugFrameOverlay { response } => {
                window.update(cx, move |_view, window, _cx| {
                    window.cycle_debug_frame_overlay_mode();
                    response
                        .send(
                            debug_frame_overlay_mode_name(window.debug_frame_overlay_mode()).into(),
                        )
                        .ok();
                })
            }
            UiCommand::GetDebugFrameOverlay { response } => {
                window.update(cx, move |_view, window, _cx| {
                    response
                        .send(
                            debug_frame_overlay_mode_name(window.debug_frame_overlay_mode()).into(),
                        )
                        .ok();
                })
            }
            UiCommand::GetDebugFrameOverlayStats { response } => {
                window.update(cx, move |_view, window, _cx| {
                    response
                        .send(debug_frame_overlay_stats_js(
                            window.debug_frame_overlay_stats(),
                        ))
                        .ok();
                })
            }
            UiCommand::ResetDebugFrameOverlayStats => window.update(cx, |_view, window, _cx| {
                window.reset_debug_frame_overlay_stats();
            }),
            UiCommand::ScrollTo {
                id,
                left,
                top,
                behavior,
            } => {
                scroll_element_to(id, left, top, behavior);
                refresh_ui_window(window, cx)
            }
            UiCommand::ScrollToItem { id, index, offset } => {
                if !VIRTUAL_LIST_STATES.with(|cell| {
                    if !cell.borrow().contains_key(&id) {
                        return false;
                    }
                    queue_virtual_list_scroll(id, index, offset);
                    true
                }) {
                    SCROLL_HANDLES.with(|cell| {
                        if let Some(handle) = cell.borrow().get(&id) {
                            handle.scroll_to_item(index);
                        }
                    });
                }
                refresh_ui_window(window, cx)
            }
            UiCommand::ScrollIntoView { id, options } => {
                // Align by the tree as it is, not the last frame; see `GetElementBounds`.
                gpui::AnyWindowHandle::from(window)
                    .update(cx, |_view, window, cx| window.draw_if_dirty(cx))?;
                window.update(cx, move |view, window, cx| {
                    if view.scroll_element_into_view(id, options) {
                        cx.notify();
                        window.refresh();
                    }
                })
            }
            UiCommand::GetScrollMetrics { id, response } => {
                gpui::AnyWindowHandle::from(window).update(cx, move |_view, window, cx| {
                    window.draw_if_dirty(cx);
                    response.send(scroll_metrics(id)).ok();
                })
            }
            UiCommand::ScrollByKey { key, shift, response } => {
                window.update(cx, move |view, window, cx| {
                    let scrolled = view.scroll_by_key(&key, shift, window);
                    if scrolled {
                        cx.notify();
                        window.refresh();
                    }
                    response.send(scrolled).ok();
                })
            }
            UiCommand::SetImagePixels {
                id,
                width,
                height,
                bytes,
                format,
                response,
            } => window.update(cx, move |view, window, cx| {
                response
                    .send(view.set_image_pixels(id, width, height, bytes, format, window, cx))
                    .ok();
            }),
            UiCommand::SetImage {
                id,
                bytes,
                response,
            } => window.update(cx, move |view, window, cx| {
                response
                    .send(view.set_encoded_image(id, bytes, window, cx))
                    .ok();
            }),
            UiCommand::SetTextSelection {
                id,
                request,
                response,
            } => window.update(cx, move |view, window, cx| {
                response
                    .send(view.set_text_selection(id, request, window, cx))
                    .ok();
            }),
            UiCommand::GetTextSelection { id, response } => {
                window.update(cx, move |view, _window, cx| {
                    response.send(view.text_selection(id, cx)).ok();
                })
            }
            UiCommand::RequestSubmit { id, response } => gpui::AnyWindowHandle::from(window)
                .update(cx, move |_view, window, cx| {
                    response
                        .send(crate::forms::request_submit(window, cx, id))
                        .ok();
                }),
            UiCommand::RegisterFont { bytes, response } => {
                window.update(cx, move |view, window, cx| {
                    response.send(view.register_font(bytes, window, cx)).ok();
                })
            }
            UiCommand::GetScrollOffset { id, response } => {
                response.send(scroll_element_position(id)).ok();
                Ok(())
            }
            UiCommand::GetListScrollTop { id, response } => {
                let top = VIRTUAL_LIST_STATES.with(|cell| {
                    cell.borrow().get(&id).map(|state| {
                        let top = state.logical_scroll_top();
                        [
                            top.item_ix as f64,
                            f64::from(f32::from(top.offset_in_item)),
                            f64::from(f32::from(state.viewport_bounds().size.height)),
                        ]
                    })
                });
                response.send(top).ok();
                Ok(())
            }
            UiCommand::GetWindowSize { response } => {
                window.update(cx, move |_view, window, _cx| {
                    let size = window.viewport_size();
                    response
                        .send(WindowSize {
                            width: f32::from(size.width) as f64,
                            height: f32::from(size.height) as f64,
                        })
                        .ok();
                })
            }
            // Not `window.update`: that call leases the root view, and drawing
            // builds it.
            UiCommand::GetAutomationBounds { response } => {
                gpui::AnyWindowHandle::from(window).update(cx, move |_view, window, cx| {
                    window.draw_if_dirty(cx);
                    response.send(crate::automation::all_bounds()).ok();
                })
            }
            UiCommand::GetElementBounds { id, response } => {
                gpui::AnyWindowHandle::from(window).update(cx, move |_view, window, cx| {
                    window.draw_if_dirty(cx);
                    response.send(crate::automation::get_bounds(id)).ok();
                })
            }
            UiCommand::FocusElement(id) => window.update(cx, move |view, window, cx| {
                view.request_focus(id, window, cx);
                window.refresh();
            }),
            // Not `window.update`: the click's handlers may update the root view.
            UiCommand::ClickElement { id, response } => {
                gpui::AnyWindowHandle::from(window).update(cx, move |_view, window, cx| {
                    response
                        .send(crate::events::activation::click_element(id, window, cx))
                        .ok();
                })
            }
            UiCommand::BlurElement(id) => window.update(cx, move |view, window, cx| {
                view.blur_element(id, window, cx);
                window.refresh();
            }),
            UiCommand::MatchesFocus {
                id,
                selector,
                response,
            } => window.update(cx, move |view, window, _cx| {
                response
                    .send(view.matches_focus(id, &selector, window))
                    .ok();
            }),
            UiCommand::FocusNext => window.update(cx, |_view, window, cx| window.focus_next(cx)),
            UiCommand::FocusPrevious => {
                window.update(cx, |_view, window, cx| window.focus_prev(cx))
            }
            UiCommand::GetFocusedElementId { response } => {
                window.update(cx, |view, window, _cx| {
                    response.send(view.focused_element_id(window)).ok();
                })
            }
            UiCommand::FocusNextWithin(id) => window.update(cx, |view, window, cx| {
                view.focus_next_within(id, window, cx);
                cx.notify();
                window.refresh();
            }),
            UiCommand::FocusPreviousWithin(id) => window.update(cx, |view, window, cx| {
                view.focus_previous_within(id, window, cx);
                cx.notify();
                window.refresh();
            }),
            UiCommand::SetWindowKeyEvents {
                key_down,
                key_up,
                event_id,
            } => window.update(cx, move |view, window, cx| {
                view.routing.set_window_keys(key_down, key_up, event_id);
                cx.notify();
                window.refresh();
            }),
            UiCommand::SetWindowSelectionChange { enabled, event_id } => {
                window.update(cx, move |view, window, cx| {
                    view.set_selection_change_listener(enabled, event_id);
                    cx.notify();
                    window.refresh();
                })
            }
            UiCommand::SetWindowAppearanceChange { enabled, event_id } => {
                window.update(cx, move |view, _window, _cx| {
                    view.set_appearance_change_listener(enabled, event_id);
                })
            }
            UiCommand::SetWindowMediaChange { enabled } => {
                window.update(cx, move |view, _window, _cx| {
                    view.set_media_change_listener(enabled);
                })
            }
            UiCommand::SetAppearance(appearance) => {
                cx.update(|cx| cx.set_window_appearance(appearance));
                Ok(())
            }
            UiCommand::GetAppearance { response } => {
                window.update(cx, move |_view, window, _cx| {
                    response
                        .send(appearance_name(window.appearance()).to_string())
                        .ok();
                })
            }
            UiCommand::ControlClock { control, response } => {
                window.update(cx, move |view, _window, cx| {
                    let now_ms = match control {
                        ClockControl::Pause => view.clock.pause(),
                        ClockControl::Set(now_ms) => view.clock.set_ms(now_ms),
                        ClockControl::FastForward(delta_ms) => view.clock.fast_forward_ms(delta_ms),
                        ClockControl::Resume => view.clock.resume(),
                    };
                    cx.notify();
                    response.send(now_ms).ok();
                })
            }
            UiCommand::MotionValues { run } => {
                window.update(cx, move |view, window, cx| run(view, window, cx))
            }
            UiCommand::DispatchMouse { input, response } => {
                let result =
                    gpui::AnyWindowHandle::from(window).update(cx, move |_view, window, cx| {
                        match input {
                            MouseInput::Click {
                                x,
                                y,
                                button,
                                modifiers,
                            } => {
                                crate::automation::dispatch_click(
                                    window, cx, x, y, button, modifiers,
                                );
                            }
                            MouseInput::Down {
                                x,
                                y,
                                button,
                                modifiers,
                            } => {
                                crate::automation::dispatch_mouse_down(
                                    window, cx, x, y, button, modifiers,
                                );
                            }
                            MouseInput::Up {
                                x,
                                y,
                                button,
                                modifiers,
                            } => {
                                crate::automation::dispatch_mouse_up(
                                    window, cx, x, y, button, modifiers,
                                );
                            }
                            MouseInput::Move {
                                x,
                                y,
                                pressed_button,
                                modifiers,
                            } => {
                                crate::automation::dispatch_mouse_move(
                                    window,
                                    cx,
                                    x,
                                    y,
                                    pressed_button,
                                    modifiers,
                                );
                            }
                            MouseInput::Wheel {
                                x,
                                y,
                                delta_x,
                                delta_y,
                                modifiers,
                                phase,
                            } => {
                                crate::automation::dispatch_scroll_wheel(
                                    window, cx, x, y, delta_x, delta_y, modifiers, phase,
                                );
                            }
                        }
                    });
                response
                    .send(
                        result
                            .as_ref()
                            .map(|_| ())
                            .map_err(|error| format!("{error:#}")),
                    )
                    .ok();
                result
            }
            UiCommand::DispatchKey { input, response } => {
                let result = gpui::AnyWindowHandle::from(window)
                    .update(cx, move |_view, window, cx| match input {
                        KeyInput::Keystrokes(keystrokes) => {
                            crate::automation::dispatch_keystrokes(window, cx, &keystrokes)
                        }
                        KeyInput::Down { keystroke, is_held } => {
                            crate::automation::dispatch_key_down(window, cx, &keystroke, is_held)
                        }
                        KeyInput::Up(keystroke) => {
                            crate::automation::dispatch_key_up(window, cx, &keystroke)
                        }
                    })
                    .and_then(|result| result.map_err(anyhow::Error::msg));
                response
                    .send(
                        result
                            .as_ref()
                            .map(|_| ())
                            .map_err(|error| format!("{error:#}")),
                    )
                    .ok();
                result
            }
            #[cfg(all(target_os = "windows", feature = "test-support"))]
            UiCommand::CaptureScreenshot { path, response } => {
                let error_response = response.clone();
                let result = window.update(cx, move |_view, window, cx| {
                    cx.notify();
                    window.refresh();
                    window.on_next_frame(move |window, _cx| {
                        let result = window
                            .render_to_image()
                            .map_err(|error| format!("Screenshot capture failed: {error}"))
                            .and_then(|image| {
                                image
                                    .save(&path)
                                    .map_err(|error| format!("Failed to save screenshot: {error}"))
                            });
                        response.send(result).ok();
                    });
                });
                if let Err(error) = &result {
                    error_response.send(Err(format!("{error:#}"))).ok();
                }
                result
            }
            UiCommand::Blur => window.update(cx, |_view, window, _cx| window.blur()),
        };
        if let Err(error) = result {
            if cx.update(|cx| cx.windows().is_empty()) {
                break;
            }
            log::error!("Failed to handle GPUI UI command: {error:#}");
        }
    }
    cx.update(|cx| cx.quit());
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string())
}

/// The main GPUI renderer exposed to Node.js.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
#[napi]
pub struct GpuixRenderer {
    event_callback: Mutex<Option<Arc<ThreadsafeFunction<EventPayload>>>>,
    sync_handler: crate::events::sync_dispatch::SyncHandlerOwner,
    tree: Arc<Mutex<RetainedTree>>,
    initialized: Arc<Mutex<bool>>,
    /// Shared with GpuixView so napi methods can read the live selection
    /// without an App context. Paint and napi calls can use different threads.
    selection: SharedSelection,
    pointer_requests: crate::events::state::PointerRequests,
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
    ui_commands: Mutex<Option<mpsc::UnboundedSender<UiCommand>>>,
    /// False after `Platform::run` returns. `tick()` reports that so JS can
    /// `process.exit`, matching macOS where `pump_events` returning false is
    /// the last-window-closed signal. The UI thread owns the Win32/Linux loop,
    /// so `tick()` cannot pump it; it only observes this flag.
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
    ui_running: Arc<AtomicBool>,
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl GpuixRenderer {
    fn apply_text_selection(
        &self,
        id: u64,
        request: crate::custom_elements::text_selection::SelectionRequest,
    ) -> Result<()> {
        #[cfg(target_os = "macos")]
        return update_window(move |view, window, cx| {
            view.set_text_selection(id, request, window, cx)
        })?
        .map_err(Error::from_reason);

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::SetTextSelection {
                id,
                request,
                response,
            })?;
            return recv_ui_response(receiver, "the text selection update")?
                .map_err(Error::from_reason);
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
#[napi]
impl GpuixRenderer {
    fn event_callback_for_view(&self) -> Option<EventCallback> {
        self.event_callback.lock().unwrap().clone().map(|tsf| {
            Arc::new(move |payload: EventPayload| {
                tsf.call(Ok(payload), ThreadsafeFunctionCallMode::NonBlocking);
            }) as EventCallback
        })
    }

    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
    fn send_ui_command(&self, command: UiCommand) -> Result<()> {
        self.ui_commands
            .lock()
            .unwrap()
            .as_ref()
            .ok_or_else(|| Error::from_reason("GPUI application is not initialized"))?
            .unbounded_send(command)
            .map_err(|_| Error::from_reason("The GPUI UI thread is not running"))
    }

    fn begin_path_prompt(&self, options: gpui::PathPromptOptions, deferred: PathPromptDeferred) {
        #[cfg(target_os = "macos")]
        {
            let prompt = update_app(|cx| {
                (
                    cx.background_executor().clone(),
                    cx.prompt_for_paths(options),
                )
            });
            match prompt {
                Ok((background_executor, receiver)) => {
                    settle_path_prompt(background_executor, receiver, deferred)
                }
                Err(error) => deferred.reject(error),
            }
            return;
        }

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            if !self.ui_running.load(Ordering::Acquire) {
                deferred.reject(Error::from_reason("The GPUI UI thread is not running"));
                return;
            }
            let sender = self.ui_commands.lock().unwrap().as_ref().cloned();
            let Some(sender) = sender else {
                deferred.reject(Error::from_reason("GPUI application is not initialized"));
                return;
            };
            if let Err(error) =
                sender.unbounded_send(UiCommand::PromptForPaths { options, deferred })
            {
                if let UiCommand::PromptForPaths { deferred, .. } = error.into_inner() {
                    deferred.reject(Error::from_reason("The GPUI UI thread is not running"));
                }
            }
            return;
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        {
            let _ = options;
            deferred.reject(Error::from_reason("Unsupported operating system"));
        }
    }

    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
    fn dispatch_mouse_input(&self, input: MouseInput) -> Result<()> {
        let (response_sender, response_receiver) = sync_channel(1);
        self.send_ui_command(UiCommand::DispatchMouse {
            input,
            response: response_sender,
        })?;
        recv_ui_response(response_receiver, "the GPUI UI command")?.map_err(Error::from_reason)
    }

    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
    fn dispatch_key_input(&self, input: KeyInput) -> Result<()> {
        let (response_sender, response_receiver) = sync_channel(1);
        self.send_ui_command(UiCommand::DispatchKey {
            input,
            response: response_sender,
        })?;
        recv_ui_response(response_receiver, "the GPUI key command")?.map_err(Error::from_reason)
    }

    /// Lay out and paint any pending change now, so a bounds read answers for
    /// the tree as it is, like `getBoundingClientRect` forcing layout.
    ///
    /// Without this, bounds come from the last display-link frame, which may
    /// not exist yet right after a commit. Call it before locking `tree`: the
    /// draw builds the view from it. The display-link step that follows only
    /// presents.
    #[cfg(target_os = "macos")]
    fn draw_pending_changes(&self) -> Result<()> {
        if !*self.initialized.lock().unwrap() {
            return Ok(());
        }
        update_window_without_view(|window, cx| window.draw_if_dirty(cx)).map(|_| ())
    }

    fn automation_bounds(&self) -> Result<HashMap<u64, crate::automation::ElementBounds>> {
        #[cfg(target_os = "macos")]
        {
            self.draw_pending_changes()?;
            return Ok(crate::automation::all_bounds());
        }

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::GetAutomationBounds { response })?;
            return recv_ui_response(receiver, "the automation bounds query");
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    fn elements_at(&self, x: f64, y: f64) -> Result<Vec<u64>> {
        #[cfg(target_os = "macos")]
        {
            self.draw_pending_changes()?;
            let tree = self.tree.lock().unwrap();
            return Ok(tree.elements_at(x, y, &crate::automation::get_bounds));
        }

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let bounds = self.automation_bounds()?;
            let tree = self.tree.lock().unwrap();
            return Ok(tree.elements_at(x, y, &|id| bounds.get(&id).copied()));
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        {
            let _ = (x, y);
            Err(Error::from_reason("Unsupported operating system"))
        }
    }

    fn element_bounds(&self, id: u64) -> Result<Option<crate::automation::ElementBounds>> {
        #[cfg(target_os = "macos")]
        {
            self.draw_pending_changes()?;
            return Ok(crate::automation::get_bounds(id));
        }

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::GetElementBounds { id, response })?;
            return recv_ui_response(receiver, "the element bounds query");
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        {
            let _ = id;
            Err(Error::from_reason("Unsupported operating system"))
        }
    }

    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
    fn control_clock(&self, control: ClockControl) -> Result<f64> {
        let (response, receiver) = sync_channel(1);
        self.send_ui_command(UiCommand::ControlClock { control, response })?;
        recv_ui_response(receiver, "the automation clock command")
    }

    /// Runs `run` on the UI thread against the window's motion values at the
    /// window clock's instant. A write repaints, so the frame samples it.
    fn with_motion_values<R: Send + 'static>(
        &self,
        repaint: bool,
        run: impl FnOnce(&mut crate::motion::MotionValueStore, web_time::Instant) -> std::result::Result<R, String>
            + Send
            + 'static,
    ) -> Result<R> {
        self.with_motion_state(repaint, move |view, now| run(&mut view.motion_values, now))
    }

    /// Runs `run` on the UI thread against the window's view at the window
    /// clock's instant. Only the motion state of the view is for it to touch.
    fn with_motion_state<R: Send + 'static>(
        &self,
        repaint: bool,
        run: impl FnOnce(&mut GpuixView, web_time::Instant) -> std::result::Result<R, String>
            + Send
            + 'static,
    ) -> Result<R> {
        let apply = move |view: &mut GpuixView,
                          window: &mut gpui::Window,
                          cx: &mut gpui::Context<GpuixView>| {
            let now = view.clock.now();
            view.motion_values
                .set_appearance(color_scheme_of(window.appearance()));
            let result = run(view, now);
            if repaint {
                cx.notify();
                window.refresh();
            }
            result
        };

        #[cfg(target_os = "macos")]
        return update_window(apply)?.map_err(Error::from_reason);

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::MotionValues {
                run: Box::new(move |view, window, cx| {
                    response.send(apply(view, window, cx)).ok();
                }),
            })?;
            return recv_ui_response(receiver, "the motion value command")?
                .map_err(Error::from_reason);
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        {
            let _ = apply;
            Err(Error::from_reason(
                "The production GPUIX renderer does not support this operating system",
            ))
        }
    }

    fn request_invalidate(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        return invalidate_window();

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::Invalidate);

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason(
            "The production GPUIX renderer does not support this operating system",
        ))
    }

    #[napi(constructor)]
    pub fn new(event_callback: Option<ThreadsafeFunction<EventPayload>>) -> Self {
        let _ = env_logger::try_init();
        Self {
            event_callback: Mutex::new(event_callback.map(Arc::new)),
            sync_handler: crate::events::sync_dispatch::SyncHandlerOwner::new(),
            tree: Arc::new(Mutex::new(RetainedTree::new())),
            initialized: Arc::new(Mutex::new(false)),
            selection: SharedSelection::default(),
            pointer_requests: Default::default(),
            #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
            ui_commands: Mutex::new(None),
            #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
            ui_running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Initialize GPUI using the native event-loop architecture for this OS.
    #[napi]
    pub fn init(&self, options: Option<WindowOptions>) -> Result<()> {
        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        {
            let _ = options;
            return Err(Error::from_reason(
                "The production GPUIX renderer does not support this operating system",
            ));
        }

        #[cfg(target_os = "macos")]
        return self.init_macos(options);

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.init_threaded(options);
    }

    #[cfg(target_os = "macos")]
    fn init_macos(&self, options: Option<WindowOptions>) -> Result<()> {
        let options = options.unwrap_or_default();

        {
            let initialized = self.initialized.lock().unwrap();
            if *initialized {
                return Err(Error::from_reason("Renderer is already initialized"));
            }
        }
        if MAC_PLATFORM.with(|platform| platform.borrow().is_some()) {
            return Err(Error::from_reason(
                "A GPUI application already exists on this thread",
            ));
        }

        let width = options.width.unwrap_or(800.0);
        let height = options.height.unwrap_or(600.0);
        let title = options.title.clone().unwrap_or_else(|| "GPUIX".to_string());
        let app_name = options.app_name.clone().unwrap_or_else(|| title.clone());
        // `focus: false` must also skip `cx.activate`: the window flag only
        // decides key status inside the app, activation is what steals focus.
        let activate = options.focus.unwrap_or(true);
        let window_options = options.clone();
        let appearance = appearance_override(&options).map_err(Error::from_reason)?;

        let platform = Rc::new(gpui_macos::MacPlatform::new_embedded());

        let tree = self.tree.clone();
        let callback = self.event_callback_for_view();

        let selection = self.selection.clone();
        let pointer_requests = self.pointer_requests.clone();
        let opened_window = Rc::new(RefCell::new(None));
        let startup_error = Rc::new(RefCell::new(None));
        let opened_window_for_app = opened_window.clone();
        let startup_error_for_app = startup_error.clone();
        // bun/node is not a .app. A Dock icon with no window cannot relaunch.
        // Last window close quits AppKit; tick() returns false and JS exits.
        let app = gpui::Application::with_platform(platform.clone())
            .with_quit_mode(gpui::QuitMode::LastWindowClosed);
        let app_handle = app.run_embedded(move |cx: &mut gpui::App| {
            crate::custom_elements::input::init(cx);
            crate::custom_elements::img::init(cx);
            // After the other bindings: `set_menus` reads key equivalents out of
            // the keymap, so every binding must exist before it runs.
            crate::app_menu::init(&app_name, cx);
            cx.set_window_appearance(appearance);
            let bounds = gpui::Bounds::centered(
                None,
                gpui::size(gpui::px(width as f32), gpui::px(height as f32)),
                cx,
            );

            match cx.open_window(
                to_gpui_window_options(&window_options, bounds),
                |window, cx| {
                    cx.new(|cx| {
                        let mut view = GpuixView::new(
                            tree.clone(),
                            callback.clone(),
                            title,
                            selection.clone(),
                            pointer_requests.clone(),
                        );
                        view.observe_appearance(window, cx);
                        view.observe_viewport(window, cx);
                        view.observe_scroller_style(cx);
                        view.set_frameless(window_options.frameless());
                        crate::events::enable_dom_hit_testing(window);
                        view.install_key_interception(window, cx);
                        view
                    })
                },
            ) {
                Ok(window_handle) => {
                    *opened_window_for_app.borrow_mut() = Some(window_handle);
                    if activate {
                        cx.activate(true);
                    }
                }
                Err(error) => {
                    *startup_error_for_app.borrow_mut() = Some(error.to_string());
                }
            }
        });

        let startup_result = match startup_error.borrow_mut().take() {
            Some(error) => Err(Error::from_reason(format!(
                "Failed to open the GPUI window: {error}"
            ))),
            None => opened_window
                .borrow_mut()
                .take()
                .ok_or_else(|| Error::from_reason("GPUI did not open the application window")),
        };
        let window_handle = match startup_result {
            Ok(window_handle) => window_handle,
            Err(error) => {
                app_handle.update(|cx| cx.quit());
                if platform.pump_events() {
                    MAC_PLATFORM.with(|stored| {
                        *stored.borrow_mut() = Some(platform.clone());
                    });
                }
                return Err(error);
            }
        };

        MAC_PLATFORM.with(|stored| {
            *stored.borrow_mut() = Some(platform);
        });
        GPUI_APP.with(|a| {
            *a.borrow_mut() = Some(app_handle);
        });
        GPUI_WINDOW.with(|w| {
            *w.borrow_mut() = Some(window_handle);
        });
        self.sync_handler.bind_window(window_handle.window_id());

        *self.initialized.lock().unwrap() = true;
        self.event_callback.lock().unwrap().take();
        Ok(())
    }

    // GPUI declares PerMonitorV2 only in the host exe manifest (zed#8936).
    // A napi .node is loaded by node.exe/bun.exe, so that manifest never applies.
    // Call on gpuix-ui only, before WindowsPlatform::new. Never on the Node thread:
    // the V2 fallback is SetThreadDpiAwarenessContext, which would change bun itself.
    #[cfg(target_os = "windows")]
    fn enable_per_monitor_dpi() {
        use windows::Win32::UI::HiDpi::{
            AreDpiAwarenessContextsEqual, GetThreadDpiAwarenessContext,
            SetProcessDpiAwarenessContext, SetThreadDpiAwarenessContext,
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };

        unsafe {
            let current = GetThreadDpiAwarenessContext();
            if AreDpiAwarenessContextsEqual(current, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
                .as_bool()
            {
                return;
            }
            if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_ok() {
                return;
            }
            if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE).is_ok() {
                return;
            }
            // Process awareness is already locked (node/bun manifest). This thread has no HWND yet.
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
    }

    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
    fn init_threaded(&self, options: Option<WindowOptions>) -> Result<()> {
        let options = options.unwrap_or_default();
        if *self.initialized.lock().unwrap() {
            return Err(Error::from_reason("Renderer is already initialized"));
        }

        let width = options.width.unwrap_or(800.0);
        let height = options.height.unwrap_or(600.0);
        let title = options.title.clone().unwrap_or_else(|| "GPUIX".to_string());
        // `focus: false` must also skip `cx.activate`: the window flag only
        // decides key status inside the app, activation is what steals focus.
        let activate = options.focus.unwrap_or(true);
        let window_options = options.clone();
        let appearance = appearance_override(&options).map_err(Error::from_reason)?;
        let tree = self.tree.clone();
        let selection = self.selection.clone();
        let pointer_requests = self.pointer_requests.clone();
        let callback = self.event_callback_for_view();
        let (command_sender, command_receiver) = mpsc::unbounded();
        let (startup_sender, startup_receiver) = sync_channel(1);
        let exit_startup_sender = startup_sender.clone();
        let ui_running = self.ui_running.clone();
        let ui_running_for_run = ui_running.clone();

        std::thread::Builder::new()
            .name("gpuix-ui".to_string())
            .spawn(move || {
                #[cfg(target_os = "windows")]
                Self::enable_per_monitor_dpi();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    // Default is already LastWindowClosed on Windows/Linux.
                    // Set it anyway so a GPUI default change cannot leave bun
                    // running after the last window closes, as on macOS.
                    gpui_platform::application()
                        .with_quit_mode(gpui::QuitMode::LastWindowClosed)
                        .run(move |cx| {
                            crate::custom_elements::input::init(cx);
                            crate::custom_elements::img::init(cx);
                            cx.set_window_appearance(appearance);
                            let size = gpui::size(gpui::px(width as f32), gpui::px(height as f32));
                            // A layer-shell surface is positioned by the compositor from its
                            // anchor, so it opens at the origin; a normal window is centered.
                            let bounds = if window_options.layer_shell.is_some() {
                                gpui::Bounds {
                                    origin: gpui::point(gpui::px(0.0), gpui::px(0.0)),
                                    size,
                                }
                            } else {
                                gpui::Bounds::centered(None, size, cx)
                            };
                            let window = match cx.open_window(
                                to_gpui_window_options(&window_options, bounds),
                                |window, cx| {
                                    cx.new(|cx| {
                                        let mut view = GpuixView::new(
                                            tree,
                                            callback,
                                            title,
                                            selection,
                                            pointer_requests,
                                        );
                                        view.observe_appearance(window, cx);
                                        view.observe_viewport(window, cx);
                                        view.observe_scroller_style(cx);
                                        view.set_frameless(window_options.frameless());
                                        crate::events::enable_dom_hit_testing(window);
                                        view.install_key_interception(window, cx);
                                        view
                                    })
                                },
                            ) {
                                Ok(window) => window,
                                Err(error) => {
                                    startup_sender
                                        .send(Err(format!(
                                            "Failed to open the GPUI window: {error}"
                                        )))
                                        .ok();
                                    cx.quit();
                                    return;
                                }
                            };

                            cx.spawn(async move |cx| {
                                run_ui_commands(command_receiver, window, cx).await;
                            })
                            .detach();
                            if activate {
                                cx.activate(true);
                            }
                            ui_running_for_run.store(true, Ordering::Release);
                            startup_sender.send(Ok(())).ok();
                        });
                }));
                ui_running.store(false, Ordering::Release);

                let error = match result {
                    Ok(()) => {
                        "The GPUI event loop exited before initialization completed".to_string()
                    }
                    Err(payload) => format!(
                        "The GPUI UI thread panicked during initialization: {}",
                        panic_message(payload)
                    ),
                };
                exit_startup_sender.try_send(Err(error)).ok();
            })
            .map_err(|error| {
                Error::from_reason(format!("Failed to spawn the GPUI UI thread: {error}"))
            })?;

        startup_receiver
            .recv()
            .map_err(|_| Error::from_reason("The GPUI UI thread stopped during initialization"))?
            .map_err(Error::from_reason)?;

        *self.ui_commands.lock().unwrap() = Some(command_sender);
        *self.initialized.lock().unwrap() = true;
        self.event_callback.lock().unwrap().take();
        Ok(())
    }

    /// Apply a batch of mutations in a single FFI call.
    ///
    /// Accepts a JSON array of mutation tuples. Each tuple is an array where
    /// the first element is the operation name (string) and remaining elements
    /// are the arguments:
    ///
    ///   ["createElement",    id, "type"]
    ///   ["destroyElement",   id]
    ///   ["appendChild",      parentId, childId]
    ///   ["insertBefore",     parentId, childId, beforeId]
    ///   ["setStyle",         id, { ...style }]
    ///   ["setText",          id, "content"]
    ///   ["setEventListener", id, "eventType", true|false]
    ///   ["setRoot",          id]
    ///   ["setKeyboardFocusDim", true|false]
    ///   ["setCustomProp",    id, "key", value]
    ///
    /// Returns accumulated destroyed IDs from all destroyElement ops.
    /// Acquires the tree mutex ONCE for the entire batch.
    #[napi]
    pub fn apply_batch(&self, json: String) -> Result<Vec<f64>> {
        let mut tree = self.tree.lock().unwrap();
        let destroyed =
            apply_batch_to_tree(&mut tree, json.as_bytes()).map_err(Error::from_reason)?;
        let dirty = tree.take_dirty();
        drop(tree);
        // A re-render that changed nothing sends a batch of identical resends.
        // Waking the window for it makes it rebuild and diff the whole element
        // tree to draw the frame it already has.
        if dirty {
            self.request_invalidate()?;
        }
        Ok(destroyed)
    }

    // ── Frame loop ───────────────────────────────────────────────────

    /// Pump the native event loop. Returns false after the last window closes.
    #[napi]
    pub fn tick(&self) -> Result<bool> {
        let initialized = *self.initialized.lock().unwrap();
        if !initialized {
            return Err(Error::from_reason(
                "Renderer not initialized. Call init() first.",
            ));
        }

        #[cfg(target_os = "macos")]
        {
            // Key input is dispatched while the platform pumps, and JS handlers
            // run synchronously from there.
            return crate::events::sync_dispatch::native_call(|| {
                Ok(MAC_PLATFORM.with(|p| {
                    p.borrow()
                        .as_ref()
                        .map(|platform| platform.pump_events())
                        .unwrap_or(false)
                }))
            });
        }

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let running = self.ui_running.load(Ordering::Acquire);
            if !running {
                self.ui_commands.lock().unwrap().take();
            }
            return Ok(running);
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason(
            "The production GPUIX renderer does not support this operating system",
        ))
    }

    #[napi]
    pub fn is_initialized(&self) -> bool {
        *self.initialized.lock().unwrap()
    }

    /// Registers the function native calls synchronously for events whose
    /// default JS can cancel: `keydown`, `keyup`. It takes the `EventPayload`
    /// and returns `{ defaultPrevented, propagationStopped }`. Pass `null` to
    /// remove it.
    ///
    /// Synchronous on macOS, where GPUI runs on the JS thread inside `tick()`.
    /// Windows and Linux run GPUI on another thread and keep delivering these
    /// events through the queue, so their handlers cannot cancel anything.
    #[napi]
    pub fn set_sync_event_handler(
        &self,
        env: &Env,
        handler: Option<Function<EventPayload, crate::events::sync_dispatch::SyncDispatchResult>>,
    ) -> Result<()> {
        crate::events::sync_dispatch::register(&self.sync_handler, env, handler)
    }

    /// Wake JavaScript after the display link queues a frame. Returns true on macOS,
    /// false on other platforms. Call tick() in the callback; it never blocks.
    /// Wakes coalesce while JavaScript is busy and do not keep the process alive.
    /// Pass null to clear the callback. Already queued callbacks may still arrive.
    #[napi]
    pub fn set_frame_wake_callback(&self, callback: Option<Function<(), ()>>) -> Result<bool> {
        #[cfg(target_os = "macos")]
        {
            use std::sync::atomic::{AtomicBool, Ordering};

            let Some(callback) = callback else {
                gpui_macos::set_frame_wake_hook(None);
                return Ok(true);
            };
            if !self.is_initialized() {
                return Err(Error::from_reason(
                    "Call init() before setting a frame wake callback",
                ));
            }
            let pending = Arc::new(AtomicBool::new(false));
            let handled = pending.clone();
            let wake = callback
                .build_threadsafe_function::<()>()
                .callee_handled::<false>()
                .weak::<true>()
                .build_callback(move |_| {
                    handled.store(false, Ordering::Release);
                    Ok(())
                })?;
            gpui_macos::set_frame_wake_hook(Some(Arc::new(move || {
                if !pending.swap(true, Ordering::AcqRel)
                    && wake.call((), ThreadsafeFunctionCallMode::NonBlocking) != napi::Status::Ok
                {
                    pending.store(false, Ordering::Release);
                }
            })));
            return Ok(true);
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = callback;
            Ok(false)
        }
    }

    /// Whether JavaScript must call tick() until it returns false.
    ///
    /// macOS: tick() pumps AppKit. Windows/Linux: tick() only reports whether
    /// the UI thread is still inside `Platform::run`. Both return false after
    /// the last window closes so the JS frame loop can exit the process.
    #[napi]
    pub fn requires_tick(&self) -> bool {
        cfg!(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        ))
    }

    /// The paintable size of the window in logical pixels, excluding any
    /// platform title bar. This used to answer a hardcoded 800x600, so anything
    /// that turned a mouse position into layout coordinates pointed at the
    /// wrong place on every window that was not exactly that size.
    #[napi]
    pub fn get_window_size(&self) -> Result<WindowSize> {
        #[cfg(target_os = "macos")]
        return update_window(|_view, window, _cx| {
            let size = window.viewport_size();
            WindowSize {
                width: f32::from(size.width) as f64,
                height: f32::from(size.height) as f64,
            }
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::GetWindowSize { response })?;
            return recv_ui_response(receiver, "the window size query");
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason(
            "The production GPUIX renderer does not support this operating system",
        ))
    }

    #[napi]
    pub fn get_window_insets(&self) -> Result<WindowInsets> {
        #[cfg(target_os = "macos")]
        return update_window(|_view, window, _cx| WindowInsets::from_gpui(window.insets()));

        #[cfg(not(target_os = "macos"))]
        Ok(WindowInsets::default())
    }

    /// `"hidden"` | `"minimal"` | `"full"`. Paints into the scene after layout.
    #[napi]
    pub fn set_debug_frame_overlay(&self, mode: String) -> Result<String> {
        let mode = parse_debug_frame_overlay_mode(&mode)?;
        #[cfg(target_os = "macos")]
        return update_window(move |_view, window, _cx| {
            window.set_debug_frame_overlay_mode(mode);
            debug_frame_overlay_mode_name(window.debug_frame_overlay_mode()).to_string()
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            self.send_ui_command(UiCommand::SetDebugFrameOverlay(mode))?;
            return self.debug_frame_overlay_mode();
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// Hidden → minimal → full → hidden.
    #[napi]
    pub fn cycle_debug_frame_overlay(&self) -> Result<String> {
        #[cfg(target_os = "macos")]
        return update_window(move |_view, window, _cx| {
            window.cycle_debug_frame_overlay_mode();
            debug_frame_overlay_mode_name(window.debug_frame_overlay_mode()).to_string()
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::CycleDebugFrameOverlay { response })?;
            return recv_ui_response(receiver, "the debug frame overlay query");
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    #[napi]
    pub fn get_debug_frame_overlay(&self) -> Result<String> {
        self.debug_frame_overlay_mode()
    }

    fn debug_frame_overlay_mode(&self) -> Result<String> {
        #[cfg(target_os = "macos")]
        return update_window(|_view, window, _cx| {
            debug_frame_overlay_mode_name(window.debug_frame_overlay_mode()).to_string()
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::GetDebugFrameOverlay { response })?;
            recv_ui_response(receiver, "the debug frame overlay query")
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// Clears the last 1000 draw samples. Frame count stays.
    #[napi]
    pub fn reset_debug_frame_overlay_stats(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        return update_window(|_view, window, _cx| {
            window.reset_debug_frame_overlay_stats();
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::ResetDebugFrameOverlayStats);

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// Same numbers as the on-screen overlay: current, p90, p99, max, frames.
    #[napi]
    pub fn get_debug_frame_overlay_stats(&self) -> Result<DebugFrameOverlayStats> {
        #[cfg(target_os = "macos")]
        return update_window(|_view, window, _cx| {
            debug_frame_overlay_stats_js(window.debug_frame_overlay_stats())
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::GetDebugFrameOverlayStats { response })?;
            match receiver.recv_timeout(Duration::from_secs(2)) {
                Ok(stats) => Ok(stats),
                Err(RecvTimeoutError::Timeout) => Err(Error::from_reason(
                    "Timed out after 2 seconds waiting for debug frame overlay stats",
                )),
                Err(RecvTimeoutError::Disconnected) => Err(Error::from_reason(
                    "The GPUI UI thread stopped during the debug frame overlay stats query",
                )),
            }
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// Bring the window forward and give it focus. This is how a window opened
    /// with `show: false` or `focus: false` is revealed later.
    #[napi]
    pub fn activate_window(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        return update_window(|_view, window, cx| {
            cx.activate(true);
            window.activate_window();
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::ActivateWindow);

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason(
            "The production GPUIX renderer does not support this operating system",
        ))
    }

    /// Minimize the native window.
    #[napi]
    pub fn minimize_window(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        return update_window(|_view, window, _cx| window.minimize_window());

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::MinimizeWindow);

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason(
            "The production GPUIX renderer does not support this operating system",
        ))
    }

    /// Run the native zoom or maximize operation.
    #[napi]
    pub fn zoom_window(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        return update_window(|_view, window, _cx| window.zoom_window());

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::ZoomWindow);

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason(
            "The production GPUIX renderer does not support this operating system",
        ))
    }

    /// Enter or exit native fullscreen.
    #[napi]
    pub fn toggle_fullscreen(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        return update_window(|_view, window, _cx| window.toggle_fullscreen());

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::ToggleFullscreen);

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason(
            "The production GPUIX renderer does not support this operating system",
        ))
    }

    #[napi]
    pub fn set_window_title(&self, title: String) -> Result<()> {
        #[cfg(target_os = "macos")]
        return update_window(move |view, window, cx| {
            view.window_title = title;
            cx.notify();
            window.refresh();
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::SetWindowTitle(title));

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason(
            "The production GPUIX renderer does not support this operating system",
        ))
    }

    /// Open the platform path picker. Returns null when the user cancels.
    #[napi(ts_return_type = "Promise<Array<string> | null>")]
    pub fn prompt_for_paths<'env>(
        &self,
        env: &'env Env,
        options: Option<PathPromptOptions>,
    ) -> Result<Object<'env>> {
        let (deferred, promise): (PathPromptDeferred, Object<'env>) = env.create_deferred()?;
        match PathPromptOptions::to_gpui(options) {
            Ok(options) => self.begin_path_prompt(options, deferred),
            Err(error) => deferred.reject(error),
        }
        Ok(promise)
    }

    /// Replace the system clipboard's contents with plain text.
    #[napi]
    pub fn write_clipboard_text(&self, text: String) -> Result<()> {
        #[cfg(target_os = "macos")]
        return update_app(|cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string(text)));

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::WriteClipboardText { text, response })?;
            return recv_ui_response(receiver, "the clipboard write");
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// The system clipboard's text, or null when it holds none.
    #[napi]
    pub fn read_clipboard_text(&self) -> Result<Option<String>> {
        #[cfg(target_os = "macos")]
        return update_app(|cx| cx.read_from_clipboard().and_then(|item| item.text()));

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::ReadClipboardText { response })?;
            return recv_ui_response(receiver, "the clipboard read");
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// Whether `openEyeDropper` exists on this platform. macOS only.
    #[napi]
    pub fn supports_eye_dropper(&self) -> bool {
        crate::platform::eyedropper::is_supported()
    }

    /// Run the screen colour picker. Resolves `#rrggbb`, or null when the user
    /// cancels. There is no way to dismiss it from code.
    #[napi(ts_return_type = "Promise<string | null>")]
    pub fn open_eye_dropper<'env>(&self, env: &'env Env) -> Result<Object<'env>> {
        let (deferred, promise): (crate::platform::eyedropper::EyeDropperDeferred, Object<'env>) =
            env.create_deferred()?;
        crate::platform::eyedropper::open(deferred);
        Ok(promise)
    }

    #[napi]
    pub fn focus_element(&self, element_id: f64) -> Result<()> {
        let id = to_element_id(element_id)?;
        #[cfg(target_os = "macos")]
        return update_window_without_view(move |window, cx| {
            crate::focus::change_focus(window, cx, move |view, window, cx| {
                view.request_focus(id, window, cx)
            })
        })?
        .map_err(Error::from_reason);

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::FocusElement(id));

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// `element.blur()`: clear focus when `element_id` is the focused element.
    #[napi]
    pub fn blur_element(&self, element_id: f64) -> Result<()> {
        let id = to_element_id(element_id)?;
        #[cfg(target_os = "macos")]
        return update_window_without_view(move |window, cx| {
            crate::focus::change_focus(window, cx, move |view, window, cx| {
                view.blur_element(id, window, cx)
            })
        })?
        .map_err(Error::from_reason);

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::BlurElement(id));

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// `element.matches(selector)` for `:focus`, `:focus-visible`,
    /// `:focus-within`, `:hover` and `:active`. Any other selector is an error.
    #[napi]
    pub fn matches_focus(&self, element_id: f64, selector: String) -> Result<bool> {
        let id = to_element_id(element_id)?;
        #[cfg(target_os = "macos")]
        {
            let matches = update_window(move |view, window, _cx| {
                view.matches_focus(id, &selector, window)
            })?
            .map_err(Error::from_reason)?;
            update_window_without_view(crate::focus::settle_focus)?.map_err(Error::from_reason)?;
            return Ok(matches);
        }

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::MatchesFocus {
                id,
                selector,
                response,
            })?;
            return recv_ui_response(receiver, "the focus selector query")?
                .map_err(Error::from_reason);
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// Move focus to the next GPUI tab stop.
    #[napi]
    pub fn focus_next(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        return update_window_without_view(|window, cx| {
            window.focus_next(cx);
            crate::focus::settle_focus(window, cx)
        })?
        .map_err(Error::from_reason);

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::FocusNext);

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// Move focus to the previous GPUI tab stop.
    #[napi]
    pub fn focus_previous(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        return update_window_without_view(|window, cx| {
            window.focus_prev(cx);
            crate::focus::settle_focus(window, cx)
        })?
        .map_err(Error::from_reason);

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::FocusPrevious);

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// Host id of the focused element, or null when nothing is focused.
    #[napi]
    pub fn get_focused_element_id(&self) -> Result<Option<f64>> {
        #[cfg(target_os = "macos")]
        return update_window(|view, window, _cx| {
            view.focused_element_id(window).map(|id| id as f64)
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::GetFocusedElementId { response })?;
            return recv_ui_response(receiver, "the focused element id query")
                .map(|id| id.map(|id| id as f64));
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// Move focus to the next tab stop inside `element_id`, wrapping in that subtree.
    #[napi]
    pub fn focus_next_within(&self, element_id: f64) -> Result<()> {
        let id = to_element_id(element_id)?;
        #[cfg(target_os = "macos")]
        return update_window_without_view(move |window, cx| {
            crate::focus::change_focus(window, cx, move |view, window, cx| {
                view.focus_next_within(id, window, cx);
                cx.notify();
            })
        })?
        .map_err(Error::from_reason);

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::FocusNextWithin(id));

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// Move focus to the previous tab stop inside `element_id`, wrapping in that subtree.
    #[napi]
    pub fn focus_previous_within(&self, element_id: f64) -> Result<()> {
        let id = to_element_id(element_id)?;
        #[cfg(target_os = "macos")]
        return update_window_without_view(move |window, cx| {
            crate::focus::change_focus(window, cx, move |view, window, cx| {
                view.focus_previous_within(id, window, cx);
                cx.notify();
            })
        })?
        .map_err(Error::from_reason);

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::FocusPreviousWithin(id));

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// Enable the window key events requested by the React renderer.
    #[napi]
    pub fn set_window_key_events(&self, key_down: bool, key_up: bool, event_id: f64) -> Result<()> {
        let event_id = to_element_id(event_id)?;
        #[cfg(target_os = "macos")]
        return update_window(move |view, window, cx| {
            view.routing.set_window_keys(key_down, key_up, event_id);
            cx.notify();
            window.refresh();
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::SetWindowKeyEvents {
            key_down,
            key_up,
            event_id,
        });

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    #[napi]
    pub fn blur(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        return update_window_without_view(move |window, _cx| window.blur());

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::Blur);

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// Enable the window selectionChange event requested by the React renderer.
    #[napi]
    pub fn set_window_selection_change(&self, enabled: bool, event_id: f64) -> Result<()> {
        let event_id = to_element_id(event_id)?;
        #[cfg(target_os = "macos")]
        return update_window(move |view, window, cx| {
            view.set_selection_change_listener(enabled, event_id);
            cx.notify();
            window.refresh();
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::SetWindowSelectionChange { enabled, event_id });

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// Enable the window appearanceChange event requested by the React renderer.
    #[napi]
    pub fn set_window_appearance_change(&self, enabled: bool, event_id: f64) -> Result<()> {
        let event_id = to_element_id(event_id)?;
        #[cfg(target_os = "macos")]
        return update_window(move |view, _window, _cx| {
            view.set_appearance_change_listener(enabled, event_id);
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::SetWindowAppearanceChange { enabled, event_id });

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason(
            "The production GPUIX renderer does not support this operating system",
        ))
    }

    /// Enable the `mediaChange` event behind `matchMedia`. It fires when the OS
    /// accessibility preferences or the window appearance change.
    #[napi]
    pub fn set_window_media_change(&self, enabled: bool) -> Result<()> {
        #[cfg(target_os = "macos")]
        return update_window(move |view, _window, _cx| {
            view.set_media_change_listener(enabled);
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::SetWindowMediaChange { enabled });

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason(
            "The production GPUIX renderer does not support this operating system",
        ))
    }

    /// The OS accessibility preferences that media queries expose. All false
    /// where the OS has no read (everything but macOS).
    #[napi]
    pub fn get_media_features(&self) -> crate::media::MediaFeatures {
        crate::media::read_system_features()
    }

    /// `"system"` follows the OS light/dark setting, `"light"` and `"dark"`
    /// force it for the whole app. Only macOS applies it; elsewhere GPUI has no
    /// override and the call is accepted and ignored.
    #[napi]
    pub fn set_appearance(&self, appearance: String) -> Result<()> {
        let appearance = parse_window_appearance(&appearance).map_err(Error::from_reason)?;
        #[cfg(target_os = "macos")]
        return update_window_without_view(move |_window, cx| cx.set_window_appearance(appearance));

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::SetAppearance(appearance));

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason(
            "The production GPUIX renderer does not support this operating system",
        ))
    }

    /// The effective appearance of the window, `"light"` or `"dark"`.
    #[napi]
    pub fn get_appearance(&self) -> Result<String> {
        #[cfg(target_os = "macos")]
        return update_window(|_view, window, _cx| {
            appearance_name(window.appearance()).to_string()
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::GetAppearance { response })?;
            return recv_ui_response(receiver, "the appearance query");
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason(
            "The production GPUIX renderer does not support this operating system",
        ))
    }

    // ── Motion values ────────────────────────────────────────────────
    //
    // JS chooses the ids (one counter per process), so a value can exist there
    // before it is bound to a renderer. Values and transitions travel as JSON:
    // a number, a colour or percentage string, a list of numbers, or a
    // `boxShadow` layer or list. Reads sample at the window clock, so they
    // follow `clockSet` and `clockFastForward`. A value that something binds
    // through the `motionValues` prop, or that has a listener, is sampled on
    // every frame; `motionValueChange`, `motionValueAnimationStart` and
    // `motionValueAnimationComplete` events carry the value id as `elementId`.

    /// A value that is written with `set` and `jump` or animated.
    #[napi]
    pub fn create_motion_value(&self, id: f64, initial: serde_json::Value) -> Result<()> {
        self.with_motion_values(false, move |values, _now| values.create_source(id, &initial))
    }

    /// A value mapped from the number value `input` through `inputRange` and
    /// `outputRange`. `options` is `{ clamp?: boolean, ease?: ease | ease[] }`.
    #[napi]
    pub fn create_motion_transform(
        &self,
        id: f64,
        input: f64,
        input_range: Vec<f64>,
        output_range: serde_json::Value,
        options: Option<serde_json::Value>,
    ) -> Result<()> {
        self.with_motion_values(false, move |values, _now| {
            values.create_transform(id, input, input_range, &output_range, options.as_ref())
        })
    }

    /// A spring that chases another value. `config` is a spring transition
    /// without the `type`.
    #[napi]
    pub fn create_motion_follower(
        &self,
        id: f64,
        source: f64,
        config: serde_json::Value,
    ) -> Result<()> {
        self.with_motion_values(false, move |values, now| {
            values.create_follower(id, source, &config, now)
        })
    }

    /// A spring with a target of its own, which `setMotionValue` moves.
    #[napi]
    pub fn create_motion_spring(
        &self,
        id: f64,
        initial: serde_json::Value,
        config: serde_json::Value,
    ) -> Result<()> {
        self.with_motion_values(false, move |values, now| {
            values.create_spring(id, &initial, &config, now)
        })
    }

    /// Whether the value existed. Values that read it fail from then on.
    #[napi]
    pub fn dispose_motion_value(&self, id: f64) -> Result<bool> {
        self.with_motion_values(false, move |values, _now| values.dispose(id))
    }

    #[napi]
    pub fn get_motion_value(&self, id: f64) -> Result<serde_json::Value> {
        self.with_motion_values(false, move |values, now| values.get(id, now))
    }

    /// Units per second. Errors for a value that is not a number.
    #[napi]
    pub fn get_motion_value_velocity(&self, id: f64) -> Result<f64> {
        self.with_motion_values(false, move |values, now| values.velocity(id, now))
    }

    #[napi]
    pub fn is_motion_value_animating(&self, id: f64) -> Result<bool> {
        self.with_motion_values(false, move |values, now| values.is_animating(id, now))
    }

    /// Writes the value. Over a running animation this does not stop it:
    /// call `stopMotionValue` first to take the value from the animation.
    #[napi]
    pub fn set_motion_value(&self, id: f64, value: serde_json::Value) -> Result<()> {
        self.with_motion_values(true, move |values, now| values.set(id, &value, now))
    }

    /// Writes the value, ending any animation and dropping its velocity.
    #[napi]
    pub fn jump_motion_value(&self, id: f64, value: serde_json::Value) -> Result<()> {
        self.with_motion_values(true, move |values, now| values.jump(id, &value, now))
    }

    /// Animates a value from where it is, with the speed it has. `transition`
    /// is the same description `motion` takes, tween by default.
    #[napi]
    pub fn animate_motion_value(
        &self,
        id: f64,
        target: serde_json::Value,
        transition: Option<serde_json::Value>,
    ) -> Result<()> {
        self.with_motion_values(true, move |values, now| {
            values.animate(id, &target, transition.as_ref(), now)
        })
    }

    /// Ends an animation where it is, keeping the speed for the next one.
    #[napi]
    pub fn stop_motion_value(&self, id: f64) -> Result<()> {
        self.with_motion_values(true, move |values, now| values.stop(id, now))
    }

    /// Chooses which events the value delivers. Neither removes the listener.
    #[napi]
    pub fn set_motion_value_listener(&self, id: f64, change: bool, animation: bool) -> Result<()> {
        self.with_motion_values(false, move |values, now| {
            values.listen(id, change, animation, now)
        })
    }

    /// Defines the keyframes `animation-name: name` plays, as the `@keyframes`
    /// rule does. `framesJson` is a JSON object of `from`, `to`, `N%` or comma
    /// list keys to the animatable style properties plus `animationTimingFunction`.
    /// Redefining a name replaces it for running animations too. A bad
    /// definition throws and leaves the earlier one. Use `defineKeyframes` from
    /// `@gpuix/native/host`, which defines it on every renderer.
    #[napi]
    pub fn define_keyframes(&self, name: String, frames_json: String) -> Result<()> {
        self.with_motion_state(true, move |view, _now| {
            view.keyframes.define(&name, &frames_json)
        })
    }

    // ── Selection API ────────────────────────────────────────────────

    /// The current text selection joined in document order, or null.
    #[napi]
    pub fn get_selected_text(&self) -> Option<String> {
        self.selection.lock().selected_text()
    }

    /// Drop the current selection and request a repaint.
    #[napi]
    pub fn clear_selection(&self) -> Result<()> {
        self.selection.lock().clear();
        self.request_invalidate()
    }

    // ── Scroll API ───────────────────────────────────────────────────
    // GpuixView syncs scroll handles and virtual list states to thread-local maps.

    /// Scroll an element to a DOM scroll position: `left` and `top` are `scrollLeft` and
    /// `scrollTop`, zero or more, so scrolling down is a larger `top`.
    ///
    /// `behavior` is `"instant"`, `"smooth"` or `"auto"` (the default), which follows the
    /// element's `scrollBehavior` style. A smooth scroll animates over the next frames.
    #[napi]
    pub fn scroll_to(
        &self,
        element_id: f64,
        left: f64,
        top: f64,
        behavior: Option<String>,
    ) -> Result<()> {
        let id = to_element_id(element_id)?;
        let behavior =
            crate::scroll::Behavior::parse(behavior.as_deref()).map_err(Error::from_reason)?;
        #[cfg(target_os = "macos")]
        {
            scroll_element_to(id, left, top, behavior);
            return invalidate_window();
        }

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::ScrollTo {
            id,
            left,
            top,
            behavior,
        });

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// Scroll a child into view by its index in the children list.
    ///
    /// For a `<virtual-list>` the scroll is queued and applied on the next
    /// render, after that frame's child splice, so indices computed against a
    /// just-committed child list are never shifted twice. `offsetInItem` is in
    /// pixels and may be negative, which anchors the viewport top above the
    /// item and resolves against measured heights at layout time.
    #[napi]
    pub fn scroll_to_item(
        &self,
        element_id: f64,
        index: f64,
        offset_in_item: Option<f64>,
    ) -> Result<()> {
        let id = to_element_id(element_id)?;
        let index = index as usize;
        let offset = offset_in_item.unwrap_or(0.0) as f32;
        #[cfg(target_os = "macos")]
        if !VIRTUAL_LIST_STATES.with(|cell| {
            if !cell.borrow().contains_key(&id) {
                return false;
            }
            queue_virtual_list_scroll(id, index, offset);
            true
        }) {
            SCROLL_HANDLES.with(|cell| {
                let handles = cell.borrow();
                if let Some(handle) = handles.get(&id) {
                    handle.scroll_to_item(index);
                }
            });
        }
        #[cfg(target_os = "macos")]
        return invalidate_window();

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::ScrollToItem { id, index, offset });

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        {
            let _ = offset;
            Err(Error::from_reason("Unsupported operating system"))
        }
    }

    /// The logical scroll anchor of a `<virtual-list>`:
    /// `[itemIndex, offsetInItemPx, viewportHeightPx]`, or null for anything
    /// else. `itemIndex == item count` is gpui's at-end sentinel.
    ///
    /// Unlike `getScrollOffset` this is exact even while row heights are still
    /// estimates, because it is the anchor gpui itself scrolls by.
    #[napi]
    pub fn get_list_scroll_top(&self, element_id: f64) -> Result<Option<Vec<f64>>> {
        let id = to_element_id(element_id)?;
        #[cfg(target_os = "macos")]
        return Ok(VIRTUAL_LIST_STATES.with(|cell| {
            cell.borrow().get(&id).map(|state| {
                let top = state.logical_scroll_top();
                vec![
                    top.item_ix as f64,
                    f64::from(f32::from(top.offset_in_item)),
                    f64::from(f32::from(state.viewport_bounds().size.height)),
                ]
            })
        }));

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::GetListScrollTop { id, response })?;
            return Ok(
                recv_ui_response(receiver, "the GPUI list scroll query")?.map(|top| top.to_vec())
            );
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// The DOM scroll position of a scrollable element, `[scrollLeft, scrollTop]` (zero or
    /// more), or null if it is not a scroll container or a `<virtual-list>`.
    #[napi]
    pub fn get_scroll_offset(&self, element_id: f64) -> Result<Option<Vec<f64>>> {
        let id = to_element_id(element_id)?;
        #[cfg(target_os = "macos")]
        return Ok(scroll_element_position(id).map(|position| position.to_vec()));

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::GetScrollOffset { id, response })?;
            return Ok(
                recv_ui_response(receiver, "the GPUI scroll query")?.map(|[x, y]| vec![x, y])
            );
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// The DOM's `scrollIntoView`: scrolls every scrollable ancestor so the element sits where
    /// `block` (default `"start"`) and `inline` (default `"nearest"`) ask. Each is `"start"`,
    /// `"center"`, `"end"` or `"nearest"`. `behavior` is as for `scrollTo`.
    #[napi]
    pub fn scroll_into_view(
        &self,
        element_id: f64,
        block: Option<String>,
        inline: Option<String>,
        behavior: Option<String>,
    ) -> Result<()> {
        let id = to_element_id(element_id)?;
        let options =
            crate::scroll::IntoView::parse(block.as_deref(), inline.as_deref(), behavior.as_deref())
                .map_err(Error::from_reason)?;
        #[cfg(target_os = "macos")]
        {
            // Before `update_window_or_defer`, which leases the view the draw needs.
            self.draw_pending_changes()?;
            update_window_or_defer(move |view, window, cx| {
                if view.scroll_element_into_view(id, options) {
                    cx.notify();
                    window.refresh();
                }
            })?;
            return Ok(());
        }

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.send_ui_command(UiCommand::ScrollIntoView { id, options });

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// The default action of a scrolling key (`"up"`, `"down"`, `"left"`, `"right"`,
    /// `"pageup"`, `"pagedown"`, `"home"`, `"end"`, `"space"`): scrolls the innermost scrollable
    /// ancestor of the focused element, or the scroller under the pointer if nothing is focused.
    /// A focused text input or control keeps its keys.
    ///
    /// It reports nothing: called while the root view is being updated, it runs once that update
    /// has returned, and whether anything scrolled is not known yet at that point.
    #[napi]
    pub fn scroll_by_key(&self, key: String, shift: bool) -> Result<()> {
        #[cfg(target_os = "macos")]
        return update_window_or_defer(move |view, window, cx| {
            if view.scroll_by_key(&key, shift, window) {
                cx.notify();
                window.refresh();
            }
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::ScrollByKey { key, shift, response })?;
            recv_ui_response(receiver, "the scroll key command")?;
            return Ok(());
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// The DOM scroll geometry of an element:
    /// `[scrollLeft, scrollTop, scrollWidth, scrollHeight, clientWidth, clientHeight]`, or null
    /// if it has not been painted.
    #[napi]
    pub fn get_scroll_metrics(&self, element_id: f64) -> Result<Option<Vec<f64>>> {
        let id = to_element_id(element_id)?;
        #[cfg(target_os = "macos")]
        {
            self.draw_pending_changes()?;
            return Ok(scroll_metrics(id).map(|metrics| metrics.to_vec()));
        }

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::GetScrollMetrics { id, response })?;
            return Ok(recv_ui_response(receiver, "the GPUI scroll query")?.map(|m| m.to_vec()));
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// The event types `window` or `document` listens for. Replaces the set.
    #[napi]
    pub fn set_window_event_listeners(&self, types: Vec<String>) {
        self.pointer_requests.set_window_events(types);
    }

    /// Capture the mouse for `element_id`. Takes effect at the next pointer
    /// input, not before: the dispatcher applies it then and fires
    /// `gotpointercapture`. Ignored while no button is down.
    #[napi]
    pub fn set_pointer_capture(&self, element_id: f64, pointer_id: f64) -> Result<()> {
        let id = to_element_id(element_id)?;
        let tree = self.tree.lock().unwrap();
        self.pointer_requests
            .set(&tree, id, pointer_id)
            .map_err(Error::from_reason)
    }

    /// End `element_id`'s capture, if it holds one, at the next pointer input.
    #[napi]
    pub fn release_pointer_capture(&self, element_id: f64, pointer_id: f64) -> Result<()> {
        let id = to_element_id(element_id)?;
        self.pointer_requests
            .release(id, pointer_id)
            .map_err(Error::from_reason)
    }

    /// Whether `element_id` holds, or has been asked to hold, the mouse capture.
    #[napi]
    pub fn has_pointer_capture(&self, element_id: f64, pointer_id: f64) -> Result<bool> {
        let id = to_element_id(element_id)?;
        let tree = self.tree.lock().unwrap();
        self.pointer_requests
            .has(&tree, id, pointer_id)
            .map_err(Error::from_reason)
    }

    /// End the element drag in progress, without a `dragend`: JS calls it when
    /// `dragstart` was cancelled. Applies at the next pointer input, so a move
    /// or two may already have been delivered.
    #[napi]
    pub fn cancel_drag(&self) {
        self.pointer_requests.cancel_drag();
    }

    /// Lock the pointer to `element_id`: the cursor hides and stops, and
    /// movement is reported as `movementX`/`movementY`. Applies at the next
    /// frame, which this asks for; that frame fires `pointerlockchange` or, when
    /// the element is not connected or the platform refuses, `pointerlockerror`.
    #[napi]
    pub fn request_pointer_lock(&self, element_id: f64) -> Result<()> {
        let id = to_element_id(element_id)?;
        self.pointer_requests.request_lock(id);
        self.request_invalidate()
    }

    /// Release the pointer lock, if held, at the next frame.
    #[napi]
    pub fn exit_pointer_lock(&self) -> Result<()> {
        self.pointer_requests.exit_lock();
        self.request_invalidate()
    }

    /// The element that holds the pointer lock, or null. Follows the frames: it
    /// is still null right after `requestPointerLock`.
    #[napi]
    pub fn pointer_lock_element(&self) -> Option<f64> {
        self.pointer_requests.locked_element().map(|id| id as f64)
    }

    /// Deliver the click an activation that is not a pointer press makes on
    /// `element_id`, such as Enter on a button: a `click` with `detail` 0 that
    /// bubbles along the element's path, followed by the activation behaviour
    /// a label has. A disabled form control gets none, like
    /// `HTMLElement.click()`.
    #[napi]
    pub fn click_element(&self, element_id: f64) -> Result<()> {
        let id = to_element_id(element_id)?;
        #[cfg(target_os = "macos")]
        return update_window_without_view(move |window, cx| {
            crate::events::activation::click_element(id, window, cx)
        })?
        .map_err(Error::from_reason);

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::ClickElement { id, response })?;
            return recv_ui_response(receiver, "the click")?.map_err(Error::from_reason);
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// Paint packed pixels onto an `<img>` host node. `format` is `"rgba"`
    /// (default) or `"bgra"`; BGRA skips the per-pixel swizzle.
    #[napi]
    pub fn set_image_pixels(
        &self,
        element_id: f64,
        width: f64,
        height: f64,
        pixels: Buffer,
        format: Option<String>,
    ) -> Result<()> {
        let id = to_element_id(element_id)?;
        let width = dimension_u32(width, "width")?;
        let height = dimension_u32(height, "height")?;
        let format = crate::custom_elements::img::PixelFormat::parse(format.as_deref())
            .map_err(Error::from_reason)?;
        let bytes = pixels.to_vec();
        #[cfg(target_os = "macos")]
        {
            return update_window(move |view, window, cx| {
                view.set_image_pixels(id, width, height, bytes, format, window, cx)
            })?
            .map_err(Error::from_reason);
        }

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::SetImagePixels {
                id,
                width,
                height,
                bytes,
                format,
                response,
            })?;
            return recv_ui_response(receiver, "the image pixel upload")?
                .map_err(Error::from_reason);
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// Decode PNG, JPEG, WebP, GIF, SVG, BMP, TIFF, ICO, or Netpbm bytes onto
    /// an `<img>` host node. Prefer `setImagePixels` for live waveforms.
    #[napi]
    pub fn set_image(&self, element_id: f64, bytes: Buffer) -> Result<()> {
        let id = to_element_id(element_id)?;
        let bytes = bytes.to_vec();
        #[cfg(target_os = "macos")]
        {
            return update_window(move |view, window, cx| {
                view.set_encoded_image(id, bytes, window, cx)
            })?
            .map_err(Error::from_reason);
        }

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::SetImage {
                id,
                bytes,
                response,
            })?;
            return recv_ui_response(receiver, "the encoded image upload")?
                .map_err(Error::from_reason);
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// `select()` on an `<input>` or `<textarea>`: select all of its text.
    #[napi]
    pub fn select_text(&self, element_id: f64) -> Result<()> {
        self.apply_text_selection(
            to_element_id(element_id)?,
            crate::custom_elements::text_selection::SelectionRequest::select_all(),
        )
    }

    /// `setSelectionRange(start, end, direction)` on an `<input>` or
    /// `<textarea>`. Offsets are UTF-16 code units; `direction` is
    /// `"forward"`, `"backward"` or `"none"`.
    #[napi]
    pub fn set_selection_range(
        &self,
        element_id: f64,
        start: f64,
        end: f64,
        direction: Option<String>,
    ) -> Result<()> {
        self.apply_text_selection(
            to_element_id(element_id)?,
            crate::custom_elements::text_selection::SelectionRequest::from_dom(
                start,
                end,
                direction.as_deref(),
            ),
        )
    }

    /// `selectionStart`, `selectionEnd` and `selectionDirection` of an
    /// `<input>` or `<textarea>`.
    #[napi]
    pub fn get_text_selection(
        &self,
        element_id: f64,
    ) -> Result<crate::custom_elements::text_selection::TextSelection> {
        let id = to_element_id(element_id)?;
        #[cfg(target_os = "macos")]
        return update_window(move |view, _window, cx| view.text_selection(id, cx))?
            .map_err(Error::from_reason);

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::GetTextSelection { id, response })?;
            return recv_ui_response(receiver, "the text selection query")?
                .map_err(Error::from_reason);
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// `form.requestSubmit()`: validate the form's controls, then emit `submit`.
    /// The events are delivered before this returns.
    #[napi]
    pub fn request_submit(&self, element_id: f64) -> Result<()> {
        let id = to_element_id(element_id)?;
        #[cfg(target_os = "macos")]
        return update_window_without_view(move |window, cx| {
            crate::forms::request_submit(window, cx, id)
        })?
        .map_err(Error::from_reason);

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::RequestSubmit { id, response })?;
            return recv_ui_response(receiver, "the form submission")?.map_err(Error::from_reason);
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// Make the fonts in a TrueType or OpenType file (or a collection) usable by
    /// `fontFamily`, under the family names the file declares. Call it before
    /// the text that names the family is first laid out: a family that did not
    /// exist then has already fallen back to another font.
    #[napi]
    pub fn register_font(&self, data: Buffer) -> Result<()> {
        let bytes = data.to_vec();
        #[cfg(target_os = "macos")]
        {
            return update_window(move |view, window, cx| view.register_font(bytes, window, cx))?
                .map_err(Error::from_reason);
        }

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::RegisterFont { bytes, response })?;
            return recv_ui_response(receiver, "the font registration")?
                .map_err(Error::from_reason);
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    #[napi]
    pub fn get_automation_tree(&self) -> Result<String> {
        self.request_invalidate()?;
        let bounds = self.automation_bounds()?;
        let tree = self.tree.lock().unwrap();
        let json = tree.to_automation_json(&bounds);
        serde_json::to_string(&json)
            .map_err(|e| Error::from_reason(format!("JSON serialization failed: {}", e)))
    }

    #[napi]
    pub fn get_element_bounds(&self, id: f64) -> Result<Option<ElementBounds>> {
        let id = to_element_id(id)?;
        Ok(self.element_bounds(id)?.map(ElementBounds::from_painted))
    }

    /// The elements whose last painted box holds the window point `(x, y)`,
    /// topmost first, like the DOM's `elementsFromPoint`. An element that did
    /// not paint is never listed, `overflow: hidden | scroll` clips what lies
    /// outside it, and `pointer-events: none` is skipped.
    #[napi]
    pub fn get_elements_at(&self, x: f64, y: f64) -> Result<Vec<f64>> {
        Ok(self
            .elements_at(x, y)?
            .into_iter()
            .map(|id| id as f64)
            .collect())
    }

    #[napi]
    pub fn get_all_text(&self) -> Vec<String> {
        let tree = self.tree.lock().unwrap();
        let mut texts = Vec::new();
        if let Some(root_id) = tree.root_id {
            collect_text(root_id, &tree, &mut texts);
        }
        texts
    }

    #[napi]
    pub fn get_painted_text(&self) -> Vec<String> {
        crate::text::painted_text()
    }

    /// Every highlight wash painted in the last frame, in paint order.
    ///
    /// A quad is invisible to `getPaintedText()`, so this is the only way to
    /// assert on `highlight` without a screenshot.
    #[napi]
    pub fn get_painted_highlights(&self) -> Vec<crate::element_tree::HighlightMatch> {
        crate::text::painted_highlights()
            .into_iter()
            .map(Into::into)
            .collect()
    }

    /// Press space-separated keys in Playwright's `keyboard.press` grammar
    /// (`"Enter"`, `"Shift+Tab"`, `"Meta+a"`, `"a"`, `"Space"`) through the
    /// focused element's input pipeline, each as a keydown then a keyup.
    #[napi]
    pub fn simulate_keystrokes(&self, keystrokes: String) -> Result<()> {
        #[cfg(target_os = "macos")]
        return update_window_without_view(move |window, cx| {
            crate::automation::dispatch_keystrokes(window, cx, &keystrokes)
        })?
        .map_err(Error::from_reason);

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.dispatch_key_input(KeyInput::Keystrokes(keystrokes));

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        {
            let _ = keystrokes;
            Err(Error::from_reason("Unsupported operating system"))
        }
    }

    /// One keydown (one press in the `simulate_keystrokes` grammar), no keyup.
    #[napi]
    pub fn simulate_key_down(&self, keystroke: String, is_held: Option<bool>) -> Result<()> {
        let is_held = is_held.unwrap_or(false);

        #[cfg(target_os = "macos")]
        return update_window_without_view(move |window, cx| {
            crate::automation::dispatch_key_down(window, cx, &keystroke, is_held)
        })?
        .map_err(Error::from_reason);

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.dispatch_key_input(KeyInput::Down { keystroke, is_held });

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        {
            let _ = (keystroke, is_held);
            Err(Error::from_reason("Unsupported operating system"))
        }
    }

    /// One keyup (one press in the `simulate_keystrokes` grammar).
    #[napi]
    pub fn simulate_key_up(&self, keystroke: String) -> Result<()> {
        #[cfg(target_os = "macos")]
        return update_window_without_view(move |window, cx| {
            crate::automation::dispatch_key_up(window, cx, &keystroke)
        })?
        .map_err(Error::from_reason);

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.dispatch_key_input(KeyInput::Up(keystroke));

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        {
            let _ = keystroke;
            Err(Error::from_reason("Unsupported operating system"))
        }
    }

    /// `modifiers` is hyphenated: "cmd", "cmd-shift", "alt".
    #[napi]
    pub fn simulate_click(
        &self,
        x: f64,
        y: f64,
        button: Option<u32>,
        modifiers: Option<String>,
    ) -> Result<()> {
        let button = button.unwrap_or(0);
        let modifiers = crate::automation::parse_modifiers(modifiers.as_deref());

        #[cfg(target_os = "macos")]
        return update_window_without_view(move |window, cx| {
            crate::automation::dispatch_click(window, cx, x, y, button, modifiers);
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.dispatch_mouse_input(MouseInput::Click {
            x,
            y,
            button,
            modifiers,
        });

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        {
            let _ = (x, y, button);
            Err(Error::from_reason(
                "The production GPUIX renderer does not support this operating system",
            ))
        }
    }

    #[napi]
    pub fn simulate_mouse_down(
        &self,
        x: f64,
        y: f64,
        button: Option<u32>,
        modifiers: Option<String>,
    ) -> Result<()> {
        let button = button.unwrap_or(0);
        let modifiers = crate::automation::parse_modifiers(modifiers.as_deref());

        #[cfg(target_os = "macos")]
        return update_window_without_view(move |window, cx| {
            crate::automation::dispatch_mouse_down(window, cx, x, y, button, modifiers);
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.dispatch_mouse_input(MouseInput::Down {
            x,
            y,
            button,
            modifiers,
        });

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        {
            let _ = (x, y, button);
            Err(Error::from_reason(
                "The production GPUIX renderer does not support this operating system",
            ))
        }
    }

    #[napi]
    pub fn simulate_mouse_up(
        &self,
        x: f64,
        y: f64,
        button: Option<u32>,
        modifiers: Option<String>,
    ) -> Result<()> {
        let button = button.unwrap_or(0);
        let modifiers = crate::automation::parse_modifiers(modifiers.as_deref());

        #[cfg(target_os = "macos")]
        return update_window_without_view(move |window, cx| {
            crate::automation::dispatch_mouse_up(window, cx, x, y, button, modifiers);
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.dispatch_mouse_input(MouseInput::Up {
            x,
            y,
            button,
            modifiers,
        });

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        {
            let _ = (x, y, button);
            Err(Error::from_reason(
                "The production GPUIX renderer does not support this operating system",
            ))
        }
    }

    #[napi]
    pub fn simulate_mouse_move(
        &self,
        x: f64,
        y: f64,
        pressed_button: Option<u32>,
        modifiers: Option<String>,
    ) -> Result<()> {
        let modifiers = crate::automation::parse_modifiers(modifiers.as_deref());

        #[cfg(target_os = "macos")]
        return update_window_without_view(move |window, cx| {
            crate::automation::dispatch_mouse_move(window, cx, x, y, pressed_button, modifiers);
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.dispatch_mouse_input(MouseInput::Move {
            x,
            y,
            pressed_button,
            modifiers,
        });

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        {
            let _ = (x, y, pressed_button);
            Err(Error::from_reason(
                "The production GPUIX renderer does not support this operating system",
            ))
        }
    }

    /// Dispatch a wheel event through the same GPUI hit test the trackpad uses.
    /// Deltas are pixels with the DOM's sign, like `WheelEvent.deltaY`: positive
    /// `delta_y` scrolls down, positive `delta_x` scrolls right, matching
    /// `TestGpuixRenderer::simulate_scroll_wheel`.
    ///
    /// `phase` is `"started"` (the default, a gesture of its own), `"moved"`,
    /// `"ended"` or `"cancelled"`. The scroller that took a gesture's first delta
    /// keeps the rest of it, so continue one with `"moved"`.
    #[napi]
    pub fn simulate_scroll_wheel(
        &self,
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
        modifiers: Option<String>,
        phase: Option<String>,
    ) -> Result<()> {
        let modifiers = crate::automation::parse_modifiers(modifiers.as_deref());
        let phase = crate::scroll::parse_touch_phase(phase.as_deref()).map_err(Error::from_reason)?;

        #[cfg(target_os = "macos")]
        return update_window_without_view(move |window, cx| {
            crate::automation::dispatch_scroll_wheel(
                window, cx, x, y, delta_x, delta_y, modifiers, phase,
            );
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.dispatch_mouse_input(MouseInput::Wheel {
            x,
            y,
            delta_x,
            delta_y,
            modifiers,
            phase,
        });

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        {
            let _ = (x, y, delta_x, delta_y, modifiers, phase);
            Err(Error::from_reason(
                "The production GPUIX renderer does not support this operating system",
            ))
        }
    }

    #[napi]
    pub fn clock_pause(&self) -> Result<f64> {
        #[cfg(target_os = "macos")]
        return update_window(move |view, _window, cx| {
            let now_ms = view.clock.pause();
            cx.notify();
            now_ms
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.control_clock(ClockControl::Pause);

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    #[napi]
    pub fn clock_set(&self, now_ms: f64) -> Result<f64> {
        #[cfg(target_os = "macos")]
        return update_window(move |view, _window, cx| {
            let now_ms = view.clock.set_ms(now_ms);
            cx.notify();
            now_ms
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.control_clock(ClockControl::Set(now_ms));

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        {
            let _ = now_ms;
            Err(Error::from_reason("Unsupported operating system"))
        }
    }

    #[napi]
    pub fn clock_fast_forward(&self, delta_ms: f64) -> Result<f64> {
        #[cfg(target_os = "macos")]
        return update_window(move |view, _window, cx| {
            let now_ms = view.clock.fast_forward_ms(delta_ms);
            cx.notify();
            now_ms
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.control_clock(ClockControl::FastForward(delta_ms));

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        {
            let _ = delta_ms;
            Err(Error::from_reason("Unsupported operating system"))
        }
    }

    #[napi]
    pub fn clock_resume(&self) -> Result<f64> {
        #[cfg(target_os = "macos")]
        return update_window(move |view, _window, cx| {
            let now_ms = view.clock.resume();
            cx.notify();
            now_ms
        });

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        return self.control_clock(ClockControl::Resume);

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    #[napi]
    pub fn capture_screenshot(&self, path: String) -> Result<()> {
        #[cfg(all(target_os = "macos", feature = "test-support"))]
        {
            let image = update_window(move |_view, window, cx| {
                cx.notify();
                window.refresh();
                window.render_to_image()
            })?
            .map_err(|e| Error::from_reason(format!("Screenshot capture failed: {}", e)))?;
            image
                .save(&path)
                .map_err(|e| Error::from_reason(format!("Failed to save screenshot: {}", e)))?;
            Ok(())
        }

        #[cfg(all(target_os = "windows", feature = "test-support"))]
        {
            let (response, receiver) = sync_channel(1);
            self.send_ui_command(UiCommand::CaptureScreenshot { path, response })?;
            return recv_ui_response(receiver, "screenshot capture")?.map_err(Error::from_reason);
        }

        #[cfg(not(all(
            feature = "test-support",
            any(target_os = "macos", target_os = "windows")
        )))]
        {
            let _ = path;
            Err(Error::from_reason(
                "captureScreenshot needs a test-support build on macOS or Windows",
            ))
        }
    }
}

fn collect_text(id: u64, tree: &RetainedTree, texts: &mut Vec<String>) {
    if let Some(element) = tree.elements.get(&id) {
        if let Some(ref content) = element.content {
            texts.push(content.clone());
        }
        for &child_id in &element.children {
            collect_text(child_id, tree, texts);
        }
    }
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn start_web_app(
    tree: Arc<Mutex<RetainedTree>>,
    selection: SharedSelection,
    pointer_requests: crate::events::state::PointerRequests,
    event_callback: EventCallback,
) -> Result<(), wasm_bindgen::JsValue> {
    if WEB_APP.with(|stored| stored.borrow().is_some()) {
        return Err(wasm_bindgen::JsValue::from_str(
            "GPUIX web is already running",
        ));
    }
    gpui_platform::web_init();
    let app = gpui_platform::single_threaded_web().run_embedded(move |cx| {
        crate::custom_elements::input::init(cx);
        let window = cx.open_window(Default::default(), |window, cx| {
            if let Some(mode) = PENDING_DEBUG_OVERLAY.with(|pending| pending.borrow_mut().take()) {
                window.set_debug_frame_overlay_mode(mode);
            }
            cx.new(|cx| {
                let mut view = GpuixView::new(
                    tree,
                    Some(event_callback),
                    "GPUIX Web".to_string(),
                    selection,
                    pointer_requests,
                );
                view.observe_appearance(window, cx);
                crate::events::enable_dom_hit_testing(window);
                view.install_key_interception(window, cx);
                if let Some((key_down, key_up, event_id)) =
                    PENDING_WINDOW_KEY_EVENTS.with(|pending| pending.borrow_mut().take())
                {
                    view.routing.set_window_keys(key_down, key_up, event_id);
                }
                if let Some((enabled, event_id)) =
                    PENDING_WINDOW_SELECTION_CHANGE.with(|pending| pending.borrow_mut().take())
                {
                    view.set_selection_change_listener(enabled, event_id);
                }
                if let Some((enabled, event_id)) =
                    PENDING_WINDOW_APPEARANCE_CHANGE.with(|pending| pending.borrow_mut().take())
                {
                    view.set_appearance_change_listener(enabled, event_id);
                }
                view
            })
        });
        match window {
            Ok(window) => WEB_WINDOW.with(|stored| *stored.borrow_mut() = Some(window)),
            Err(error) => log::error!("Failed to open the GPUIX web window: {error:#}"),
        }
        cx.activate(true);
    });
    WEB_APP.with(|stored| *stored.borrow_mut() = Some(app));
    Ok(())
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn web_element_id(id: f64) -> Result<u64, wasm_bindgen::JsValue> {
    raw_element_id(id).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn web_number_array(values: impl IntoIterator<Item = f64>) -> wasm_bindgen::JsValue {
    let result = js_sys::Array::new();
    for value in values {
        result.push(&value.into());
    }
    result.into()
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn web_string_array(values: impl IntoIterator<Item = String>) -> wasm_bindgen::JsValue {
    let result = js_sys::Array::new();
    for value in values {
        result.push(&value.into());
    }
    result.into()
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn update_web_window<R>(
    update: impl FnOnce(&mut GpuixView, &mut gpui::Window, &mut gpui::Context<GpuixView>) -> R,
) -> Result<R, wasm_bindgen::JsValue> {
    crate::events::sync_dispatch::route(
        update,
        crate::events::sync_dispatch::update_root_view,
        |error| wasm_bindgen::JsValue::from_str(&error),
        |update| {
            WEB_APP.with(|app| {
                let app = app.borrow();
                let app = app.as_ref().ok_or_else(|| {
                    wasm_bindgen::JsValue::from_str("GPUIX web is not initialized")
                })?;
                app.update(|cx| {
                    WEB_WINDOW.with(|window| {
                        let window = (*window.borrow()).ok_or_else(|| {
                            wasm_bindgen::JsValue::from_str("GPUIX web window is not ready")
                        })?;
                        window
                            .update(cx, update)
                            .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))
                    })
                })
            })
        },
    )
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn update_web_window_without_view<R>(
    update: impl FnOnce(&mut gpui::Window, &mut gpui::App) -> R,
) -> Result<R, wasm_bindgen::JsValue> {
    crate::events::sync_dispatch::route(
        update,
        crate::events::sync_dispatch::update_active_window,
        |error| wasm_bindgen::JsValue::from_str(&error),
        |update| {
            WEB_APP.with(|app| {
                let app = app.borrow();
                let app = app.as_ref().ok_or_else(|| {
                    wasm_bindgen::JsValue::from_str("GPUIX web is not initialized")
                })?;
                app.update(|cx| {
                    WEB_WINDOW.with(|window| {
                        let window = (*window.borrow()).ok_or_else(|| {
                            wasm_bindgen::JsValue::from_str("GPUIX web window is not ready")
                        })?;
                        gpui::AnyWindowHandle::from(window)
                            .update(cx, move |_view, window, cx| update(window, cx))
                            .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))
                    })
                })
            })
        },
    )
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn apply_web_text_selection(
    element_id: f64,
    request: crate::custom_elements::text_selection::SelectionRequest,
) -> Result<(), wasm_bindgen::JsValue> {
    let id = web_element_id(element_id)?;
    update_web_window(move |view, window, cx| view.set_text_selection(id, request, window, cx))?
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn notify_web() {
    if let Err(error) = update_web_window(|_view, _window, cx| cx.notify()) {
        if WEB_WINDOW.with(|window| window.borrow().is_some()) {
            log::error!("Failed to invalidate the GPUIX web window: {error:?}");
        }
    }
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn web_event_callback(callback: js_sys::Function) -> EventCallback {
    Rc::new(move |payload| {
        let Ok(json) = serde_json::to_string(&payload) else {
            log::error!("Failed to serialize GPUIX browser event");
            return;
        };
        let Ok(payload) = js_sys::JSON::parse(&json) else {
            log::error!("Failed to create GPUIX browser event object");
            return;
        };
        let callback = callback.clone();
        let task = wasm_bindgen::closure::Closure::once_into_js(move || {
            if let Err(error) = callback.call2(
                &wasm_bindgen::JsValue::UNDEFINED,
                &wasm_bindgen::JsValue::NULL,
                &payload,
            ) {
                log::error!("GPUIX browser event callback failed: {error:?}");
            }
        });
        let task: js_sys::Function = task.unchecked_into();
        if let Some(window) = web_sys::window() {
            window.queue_microtask(&task);
        }
    })
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = GpuixRenderer)]
pub struct WebGpuixRenderer {
    tree: Arc<Mutex<RetainedTree>>,
    selection: SharedSelection,
    pointer_requests: crate::events::state::PointerRequests,
    event_callback: EventCallback,
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[wasm_bindgen::prelude::wasm_bindgen(js_class = GpuixRenderer)]
impl WebGpuixRenderer {
    #[wasm_bindgen::prelude::wasm_bindgen(constructor)]
    pub fn new(event_callback: js_sys::Function) -> Self {
        Self {
            tree: Arc::new(Mutex::new(RetainedTree::new())),
            selection: SharedSelection::default(),
            pointer_requests: Default::default(),
            event_callback: web_event_callback(event_callback),
        }
    }

    pub fn init(&self, _options: wasm_bindgen::JsValue) -> Result<(), wasm_bindgen::JsValue> {
        start_web_app(
            self.tree.clone(),
            self.selection.clone(),
            self.pointer_requests.clone(),
            self.event_callback.clone(),
        )
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = applyBatch)]
    pub fn apply_batch(
        &self,
        json: String,
    ) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
        let mut tree = self.tree.lock().unwrap();
        let destroyed = apply_batch_to_tree(&mut tree, json.as_bytes())
            .map_err(|error| wasm_bindgen::JsValue::from_str(&error))?;
        let dirty = tree.take_dirty();
        drop(tree);
        if dirty {
            notify_web();
        }
        Ok(web_number_array(destroyed))
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = isInitialized)]
    pub fn is_initialized(&self) -> bool {
        WEB_APP.with(|app| app.borrow().is_some())
    }

    /// See `GpuixRenderer::setSyncEventHandler`. Always synchronous here.
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = setSyncEventHandler)]
    pub fn set_sync_event_handler(&self, handler: Option<js_sys::Function>) {
        crate::events::sync_dispatch::register(handler);
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = requiresTick)]
    pub fn requires_tick(&self) -> bool {
        false
    }

    pub fn tick(&self) {}

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = getWindowSize)]
    pub fn get_window_size(&self) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
        let window = web_sys::window()
            .ok_or_else(|| wasm_bindgen::JsValue::from_str("Browser window is unavailable"))?;
        let size = js_sys::Object::new();
        js_sys::Reflect::set(&size, &"width".into(), &window.inner_width()?)?;
        js_sys::Reflect::set(&size, &"height".into(), &window.inner_height()?)?;
        Ok(size.into())
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = getWindowInsets)]
    pub fn get_window_insets(&self) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
        let insets = update_web_window(|_view, window, _cx| window.insets())?;
        window_insets_js(insets)
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = setWindowTitle)]
    pub fn set_window_title(&self, title: String) -> Result<(), wasm_bindgen::JsValue> {
        update_web_window(move |view, _window, cx| {
            view.window_title = title;
            cx.notify();
        })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = promptForPaths)]
    pub fn prompt_for_paths(&self, _options: wasm_bindgen::JsValue) -> js_sys::Promise {
        js_sys::Promise::reject(
            &js_sys::Error::new("promptForPaths is not supported on the web").into(),
        )
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = focusElement)]
    pub fn focus_element(&self, element_id: f64) -> Result<(), wasm_bindgen::JsValue> {
        let id = web_element_id(element_id)?;
        PENDING_FOCUS_ELEMENT.with(|pending| *pending.borrow_mut() = Some(id));
        notify_web();
        Ok(())
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = blurElement)]
    pub fn blur_element(&self, element_id: f64) -> Result<(), wasm_bindgen::JsValue> {
        let id = web_element_id(element_id)?;
        update_web_window(move |view, window, cx| view.blur_element(id, window, cx))
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = matchesFocus)]
    pub fn matches_focus(
        &self,
        element_id: f64,
        selector: String,
    ) -> Result<bool, wasm_bindgen::JsValue> {
        let id = web_element_id(element_id)?;
        update_web_window(move |view, window, _cx| view.matches_focus(id, &selector, window))?
            .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = focusNext)]
    pub fn focus_next(&self) -> Result<(), wasm_bindgen::JsValue> {
        update_web_window(|_view, window, cx| window.focus_next(cx))
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = focusPrevious)]
    pub fn focus_previous(&self) -> Result<(), wasm_bindgen::JsValue> {
        update_web_window(|_view, window, cx| window.focus_prev(cx))
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = getFocusedElementId)]
    pub fn get_focused_element_id(&self) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
        let id = update_web_window(|view, window, _cx| view.focused_element_id(window))?;
        Ok(match id {
            Some(id) => wasm_bindgen::JsValue::from_f64(id as f64),
            None => wasm_bindgen::JsValue::NULL,
        })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = focusNextWithin)]
    pub fn focus_next_within(&self, element_id: f64) -> Result<(), wasm_bindgen::JsValue> {
        let id = web_element_id(element_id)?;
        update_web_window(move |view, window, cx| {
            view.focus_next_within(id, window, cx);
            cx.notify();
        })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = focusPreviousWithin)]
    pub fn focus_previous_within(&self, element_id: f64) -> Result<(), wasm_bindgen::JsValue> {
        let id = web_element_id(element_id)?;
        update_web_window(move |view, window, cx| {
            view.focus_previous_within(id, window, cx);
            cx.notify();
        })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = setWindowKeyEvents)]
    pub fn set_window_key_events(
        &self,
        key_down: bool,
        key_up: bool,
        event_id: f64,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let event_id = web_element_id(event_id)?;
        // Graphics init is async. createRoot() sets this before WEB_WINDOW exists.
        if WEB_WINDOW.with(|window| window.borrow().is_none()) {
            PENDING_WINDOW_KEY_EVENTS.with(|pending| {
                *pending.borrow_mut() = Some((key_down, key_up, event_id));
            });
            return Ok(());
        }
        update_web_window(move |view, _window, cx| {
            view.routing.set_window_keys(key_down, key_up, event_id);
            cx.notify();
        })
    }

    pub fn blur(&self) -> Result<(), wasm_bindgen::JsValue> {
        update_web_window(|_view, window, _cx| window.blur())
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = setWindowSelectionChange)]
    pub fn set_window_selection_change(
        &self,
        enabled: bool,
        event_id: f64,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let event_id = web_element_id(event_id)?;
        if WEB_WINDOW.with(|window| window.borrow().is_none()) {
            PENDING_WINDOW_SELECTION_CHANGE.with(|pending| {
                *pending.borrow_mut() = Some((enabled, event_id));
            });
            return Ok(());
        }
        update_web_window(move |view, _window, cx| {
            view.set_selection_change_listener(enabled, event_id);
            cx.notify();
        })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = setWindowAppearanceChange)]
    pub fn set_window_appearance_change(
        &self,
        enabled: bool,
        event_id: f64,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let event_id = web_element_id(event_id)?;
        if WEB_WINDOW.with(|window| window.borrow().is_none()) {
            PENDING_WINDOW_APPEARANCE_CHANGE.with(|pending| {
                *pending.borrow_mut() = Some((enabled, event_id));
            });
            return Ok(());
        }
        update_web_window(move |view, _window, _cx| {
            view.set_appearance_change_listener(enabled, event_id);
        })
    }

    /// The browser owns light/dark, so this validates and does nothing.
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = setAppearance)]
    pub fn set_appearance(&self, appearance: String) -> Result<(), wasm_bindgen::JsValue> {
        parse_window_appearance(&appearance)
            .map_err(|error| wasm_bindgen::JsValue::from_str(&error))?;
        Ok(())
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = getAppearance)]
    pub fn get_appearance(&self) -> Result<String, wasm_bindgen::JsValue> {
        update_web_window(|_view, window, _cx| appearance_name(window.appearance()).to_string())
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = getSelectedText)]
    pub fn get_selected_text(&self) -> wasm_bindgen::JsValue {
        self.selection
            .lock()
            .selected_text()
            .map_or(wasm_bindgen::JsValue::NULL, |value| {
                wasm_bindgen::JsValue::from_str(&value)
            })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = clearSelection)]
    pub fn clear_selection(&self) {
        self.selection.lock().clear();
        notify_web();
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = scrollTo)]
    pub fn scroll_to(
        &self,
        element_id: f64,
        x: f64,
        y: f64,
        behavior: Option<String>,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let id = web_element_id(element_id)?;
        let behavior = crate::scroll::Behavior::parse(behavior.as_deref())
            .map_err(|message| wasm_bindgen::JsValue::from_str(&message))?;
        scroll_element_to(id, x, y, behavior);
        notify_web();
        Ok(())
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = scrollToItem)]
    pub fn scroll_to_item(
        &self,
        element_id: f64,
        index: f64,
        offset_in_item: Option<f64>,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let id = web_element_id(element_id)?;
        let index = index as usize;
        let offset = offset_in_item.unwrap_or(0.0) as f32;
        if !VIRTUAL_LIST_STATES.with(|states| {
            if !states.borrow().contains_key(&id) {
                return false;
            }
            queue_virtual_list_scroll(id, index, offset);
            true
        }) {
            SCROLL_HANDLES.with(|handles| {
                if let Some(handle) = handles.borrow().get(&id) {
                    handle.scroll_to_item(index);
                }
            });
        }
        notify_web();
        Ok(())
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = scrollIntoView)]
    pub fn scroll_into_view(
        &self,
        element_id: f64,
        block: Option<String>,
        inline: Option<String>,
        behavior: Option<String>,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let id = web_element_id(element_id)?;
        let options = crate::scroll::IntoView::parse(
            block.as_deref(),
            inline.as_deref(),
            behavior.as_deref(),
        )
        .map_err(|message| wasm_bindgen::JsValue::from_str(&message))?;
        update_web_window(move |view, window, cx| {
            if view.scroll_element_into_view(id, options) {
                cx.notify();
                window.refresh();
            }
        })
    }


    #[wasm_bindgen::prelude::wasm_bindgen(js_name = setWindowEventListeners)]
    pub fn set_window_event_listeners(&self, types: Vec<String>) {
        self.pointer_requests.set_window_events(types);
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = setPointerCapture)]
    pub fn set_pointer_capture(
        &self,
        element_id: f64,
        pointer_id: f64,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let id = web_element_id(element_id)?;
        let tree = self.tree.lock().unwrap();
        self.pointer_requests
            .set(&tree, id, pointer_id)
            .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = releasePointerCapture)]
    pub fn release_pointer_capture(
        &self,
        element_id: f64,
        pointer_id: f64,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let id = web_element_id(element_id)?;
        self.pointer_requests
            .release(id, pointer_id)
            .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = hasPointerCapture)]
    pub fn has_pointer_capture(
        &self,
        element_id: f64,
        pointer_id: f64,
    ) -> Result<bool, wasm_bindgen::JsValue> {
        let id = web_element_id(element_id)?;
        let tree = self.tree.lock().unwrap();
        self.pointer_requests
            .has(&tree, id, pointer_id)
            .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = cancelDrag)]
    pub fn cancel_drag(&self) {
        self.pointer_requests.cancel_drag();
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = requestPointerLock)]
    pub fn request_pointer_lock(&self, element_id: f64) -> Result<(), wasm_bindgen::JsValue> {
        let id = web_element_id(element_id)?;
        self.pointer_requests.request_lock(id);
        update_web_window_without_view(|window, _cx| window.refresh())
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = exitPointerLock)]
    pub fn exit_pointer_lock(&self) -> Result<(), wasm_bindgen::JsValue> {
        self.pointer_requests.exit_lock();
        update_web_window_without_view(|window, _cx| window.refresh())
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = pointerLockElement)]
    pub fn pointer_lock_element(&self) -> Option<f64> {
        self.pointer_requests.locked_element().map(|id| id as f64)
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = clickElement)]
    pub fn click_element(&self, element_id: f64) -> Result<(), wasm_bindgen::JsValue> {
        let id = web_element_id(element_id)?;
        update_web_window_without_view(move |window, cx| {
            crate::events::activation::click_element(id, window, cx)
        })?
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = setImagePixels)]
    pub fn set_image_pixels(
        &self,
        element_id: f64,
        width: f64,
        height: f64,
        pixels: js_sys::Uint8Array,
        format: Option<String>,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let id = web_element_id(element_id)?;
        let width = raw_dimension_u32(width, "width")
            .map_err(|error| wasm_bindgen::JsValue::from_str(&error))?;
        let height = raw_dimension_u32(height, "height")
            .map_err(|error| wasm_bindgen::JsValue::from_str(&error))?;
        let format = crate::custom_elements::img::PixelFormat::parse(format.as_deref())
            .map_err(|error| wasm_bindgen::JsValue::from_str(&error))?;
        let bytes = pixels.to_vec();
        update_web_window(move |view, window, cx| {
            view.set_image_pixels(id, width, height, bytes, format, window, cx)
                .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
        })?
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = setImage)]
    pub fn set_image(
        &self,
        element_id: f64,
        bytes: js_sys::Uint8Array,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let id = web_element_id(element_id)?;
        let bytes = bytes.to_vec();
        update_web_window(move |view, window, cx| {
            view.set_encoded_image(id, bytes, window, cx)
                .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
        })?
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = selectText)]
    pub fn select_text(&self, element_id: f64) -> Result<(), wasm_bindgen::JsValue> {
        apply_web_text_selection(
            element_id,
            crate::custom_elements::text_selection::SelectionRequest::select_all(),
        )
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = setSelectionRange)]
    pub fn set_selection_range(
        &self,
        element_id: f64,
        start: f64,
        end: f64,
        direction: Option<String>,
    ) -> Result<(), wasm_bindgen::JsValue> {
        apply_web_text_selection(
            element_id,
            crate::custom_elements::text_selection::SelectionRequest::from_dom(
                start,
                end,
                direction.as_deref(),
            ),
        )
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = getTextSelection)]
    pub fn get_text_selection(
        &self,
        element_id: f64,
    ) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
        let id = web_element_id(element_id)?;
        let selection = update_web_window(move |view, _window, cx| view.text_selection(id, cx))?
            .map_err(|error| wasm_bindgen::JsValue::from_str(&error))?;
        let object = js_sys::Object::new();
        for (key, value) in [
            ("start", wasm_bindgen::JsValue::from_f64(f64::from(selection.start))),
            ("end", wasm_bindgen::JsValue::from_f64(f64::from(selection.end))),
            ("direction", wasm_bindgen::JsValue::from_str(&selection.direction)),
        ] {
            js_sys::Reflect::set(&object, &wasm_bindgen::JsValue::from_str(key), &value)?;
        }
        Ok(object.into())
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = requestSubmit)]
    pub fn request_submit(&self, element_id: f64) -> Result<(), wasm_bindgen::JsValue> {
        let id = web_element_id(element_id)?;
        update_web_window_without_view(move |window, cx| {
            crate::forms::request_submit(window, cx, id)
        })?
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = getListScrollTop)]
    pub fn get_list_scroll_top(
        &self,
        element_id: f64,
    ) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
        let id = web_element_id(element_id)?;
        let top = VIRTUAL_LIST_STATES.with(|states| {
            states.borrow().get(&id).map(|state| {
                let top = state.logical_scroll_top();
                [
                    top.item_ix as f64,
                    f64::from(f32::from(top.offset_in_item)),
                    f64::from(f32::from(state.viewport_bounds().size.height)),
                ]
            })
        });
        let Some(top) = top else {
            return Ok(wasm_bindgen::JsValue::NULL);
        };
        Ok(web_number_array(top))
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = getScrollOffset)]
    pub fn get_scroll_offset(
        &self,
        element_id: f64,
    ) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
        let id = web_element_id(element_id)?;
        let offset = scroll_element_position(id);
        let Some([x, y]) = offset else {
            return Ok(wasm_bindgen::JsValue::NULL);
        };
        Ok(web_number_array([x, y]))
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = getAutomationTree)]
    pub fn get_automation_tree(&self) -> Result<String, wasm_bindgen::JsValue> {
        notify_web();
        let bounds = crate::automation::all_bounds();
        let tree = self.tree.lock().unwrap();
        serde_json::to_string(&tree.to_automation_json(&bounds)).map_err(|error| {
            wasm_bindgen::JsValue::from_str(&format!("JSON serialization failed: {error}"))
        })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = getElementBounds)]
    pub fn get_element_bounds(
        &self,
        element_id: f64,
    ) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
        let Some(bounds) = crate::automation::get_bounds(web_element_id(element_id)?) else {
            return Ok(wasm_bindgen::JsValue::NULL);
        };
        element_bounds_js(bounds)
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = getAllText)]
    pub fn get_all_text(&self) -> wasm_bindgen::JsValue {
        let tree = self.tree.lock().unwrap();
        let mut texts = Vec::new();
        if let Some(root_id) = tree.root_id {
            collect_text(root_id, &tree, &mut texts);
        }
        web_string_array(texts)
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = getPaintedText)]
    pub fn get_painted_text(&self) -> wasm_bindgen::JsValue {
        web_string_array(crate::text::painted_text())
    }

    /// The same array of objects the napi build returns.
    ///
    /// Through `serde_json` and `JSON.parse`, not `serde-wasm-bindgen`: this is
    /// a test-only API, and both crates here are already dependencies. Building
    /// the nested value by hand with `js_sys` is 20 lines of noise.
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = getPaintedHighlights)]
    pub fn get_painted_highlights(&self) -> wasm_bindgen::JsValue {
        let matches: Vec<crate::element_tree::HighlightMatch> = crate::text::painted_highlights()
            .into_iter()
            .map(Into::into)
            .collect();
        serde_json::to_string(&matches)
            .ok()
            .and_then(|json| js_sys::JSON::parse(&json).ok())
            .unwrap_or(wasm_bindgen::JsValue::NULL)
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = simulateClick)]
    pub fn simulate_click(
        &self,
        x: f64,
        y: f64,
        button: Option<u32>,
        modifiers: Option<String>,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let modifiers = crate::automation::parse_modifiers(modifiers.as_deref());
        update_web_window_without_view(move |window, cx| {
            crate::automation::dispatch_click(window, cx, x, y, button.unwrap_or(0), modifiers);
        })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = simulateMouseDown)]
    pub fn simulate_mouse_down(
        &self,
        x: f64,
        y: f64,
        button: Option<u32>,
        modifiers: Option<String>,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let modifiers = crate::automation::parse_modifiers(modifiers.as_deref());
        update_web_window_without_view(move |window, cx| {
            crate::automation::dispatch_mouse_down(
                window,
                cx,
                x,
                y,
                button.unwrap_or(0),
                modifiers,
            );
        })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = simulateMouseUp)]
    pub fn simulate_mouse_up(
        &self,
        x: f64,
        y: f64,
        button: Option<u32>,
        modifiers: Option<String>,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let modifiers = crate::automation::parse_modifiers(modifiers.as_deref());
        update_web_window_without_view(move |window, cx| {
            crate::automation::dispatch_mouse_up(window, cx, x, y, button.unwrap_or(0), modifiers);
        })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = simulateMouseMove)]
    pub fn simulate_mouse_move(
        &self,
        x: f64,
        y: f64,
        pressed_button: Option<u32>,
        modifiers: Option<String>,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let modifiers = crate::automation::parse_modifiers(modifiers.as_deref());
        update_web_window_without_view(move |window, cx| {
            crate::automation::dispatch_mouse_move(window, cx, x, y, pressed_button, modifiers);
        })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = simulateScrollWheel)]
    pub fn simulate_scroll_wheel(
        &self,
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
        modifiers: Option<String>,
        phase: Option<String>,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let modifiers = crate::automation::parse_modifiers(modifiers.as_deref());
        let phase = crate::scroll::parse_touch_phase(phase.as_deref())
            .map_err(|message| wasm_bindgen::JsValue::from_str(&message))?;
        update_web_window_without_view(move |window, cx| {
            crate::automation::dispatch_scroll_wheel(
                window, cx, x, y, delta_x, delta_y, modifiers, phase,
            );
        })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = clockPause)]
    pub fn clock_pause(&self) -> Result<f64, wasm_bindgen::JsValue> {
        update_web_window(|view, _window, cx| {
            let now_ms = view.clock.pause();
            cx.notify();
            now_ms
        })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = clockSet)]
    pub fn clock_set(&self, now_ms: f64) -> Result<f64, wasm_bindgen::JsValue> {
        update_web_window(move |view, _window, cx| {
            let now_ms = view.clock.set_ms(now_ms);
            cx.notify();
            now_ms
        })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = clockFastForward)]
    pub fn clock_fast_forward(&self, delta_ms: f64) -> Result<f64, wasm_bindgen::JsValue> {
        update_web_window(move |view, _window, cx| {
            let now_ms = view.clock.fast_forward_ms(delta_ms);
            cx.notify();
            now_ms
        })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = clockResume)]
    pub fn clock_resume(&self) -> Result<f64, wasm_bindgen::JsValue> {
        update_web_window(|view, _window, cx| {
            let now_ms = view.clock.resume();
            cx.notify();
            now_ms
        })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = setDebugFrameOverlay)]
    pub fn set_debug_frame_overlay(&self, mode: String) -> Result<String, wasm_bindgen::JsValue> {
        let mode = parse_debug_frame_overlay_mode_str(&mode)
            .map_err(|error| wasm_bindgen::JsValue::from_str(&error))?;
        // Graphics init is async. render() sets the overlay before WEB_WINDOW exists.
        if WEB_WINDOW.with(|window| window.borrow().is_none()) {
            PENDING_DEBUG_OVERLAY.with(|pending| *pending.borrow_mut() = Some(mode));
            return Ok(debug_frame_overlay_mode_name(mode).to_string());
        }
        update_web_window(move |_view, window, cx| {
            window.set_debug_frame_overlay_mode(mode);
            cx.notify();
            debug_frame_overlay_mode_name(window.debug_frame_overlay_mode()).to_string()
        })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = getDebugFrameOverlay)]
    pub fn get_debug_frame_overlay(&self) -> Result<String, wasm_bindgen::JsValue> {
        if WEB_WINDOW.with(|window| window.borrow().is_none()) {
            let pending = PENDING_DEBUG_OVERLAY.with(|pending| *pending.borrow());
            return Ok(debug_frame_overlay_mode_name(
                pending.unwrap_or(gpui::DebugFrameOverlayMode::Hidden),
            )
            .to_string());
        }
        update_web_window(|_view, window, _cx| {
            debug_frame_overlay_mode_name(window.debug_frame_overlay_mode()).to_string()
        })
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl Drop for GpuixRenderer {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if *self.initialized.lock().unwrap() {
            gpui_macos::set_frame_wake_hook(None);
        }
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "freebsd"))]
        self.ui_commands.lock().unwrap().take();
    }
}

// ── GPUI View ────────────────────────────────────────────────────────

pub(crate) struct GpuixView {
    pub(crate) tree: Arc<Mutex<RetainedTree>>,
    pub(crate) event_callback: Option<EventCallback>,
    pub(crate) window_title: String,
    /// What the key and mouse listeners read about this view. See `InputRouting`.
    pub(crate) routing: crate::events::keyboard::InputRouting,
    key_down_interception: Option<gpui::Subscription>,
    pub(crate) window_selection_change: bool,
    pub(crate) window_selection_event_id: u64,
    window_appearance_change: bool,
    window_appearance_event_id: u64,
    appearance_subscription: Option<gpui::Subscription>,
    bounds_subscription: Option<gpui::Subscription>,
    media_viewport: gpui::Size<gpui::Pixels>,
    /// Stands in for the platform's pointer lock in the test renderer, whose
    /// offscreen window is never key and so can never take a real one.
    simulated_platform: Option<crate::events::pointer_lock::SimulatedPlatform>,
    /// Stands in for the window when a press in a drag region asks it to move or zoom: the
    /// test renderer's offscreen window has no current mouse event to start a drag from.
    simulated_window_actions: Option<crate::events::app_region::SimulatedWindowActions>,
    /// The window draws its own titlebar (`titlebarTransparent`), so `appRegion` applies.
    frameless: bool,
    window_media_change: bool,
    #[cfg(target_os = "macos")]
    media_observer: Option<crate::media::MediaChangeObserver>,
    /// Rebuilds the window when the "Show scroll bars" setting changes.
    #[cfg(target_os = "macos")]
    scroller_style_observer: Option<crate::scrollbar::ScrollerStyleObserver>,
    /// The scrollbar style to use instead of the OS setting, which the test renderer sets so a
    /// test does not depend on the machine's.
    scroller_style: Option<crate::scrollbar::ScrollerStyle>,
    /// Last identity delivered through `selectionChange`. Only written once an
    /// event is really queued, so adding the listener later still reports.
    reported_selection: Option<u64>,
    /// Persistent FocusHandles keyed by element ID.
    /// Created for every DOM-focusable element (`RetainedElement::needs_focus`).
    /// Handles persist across renders so GPUI maintains focus state.
    pub(crate) focus_handles: HashMap<u64, gpui::FocusHandle>,
    pending_focus_element: Option<PendingFocus>,
    pub(crate) focus: crate::focus::FocusState,
    /// Registry for custom element types (input, editor, diff, etc.).
    /// Stores factories (one per type) and live instances (one per element ID).
    pub(crate) custom_registry: CustomElementRegistry,
    /// Persistent ScrollHandles keyed by element ID.
    /// Created lazily for elements with overflow: "scroll" (or per-axis scroll).
    /// Handles persist across renders so GPUI maintains scroll offset state.
    pub(crate) scroll_handles: HashMap<u64, gpui::ScrollHandle>,
    /// Native animation clocks keyed by retained element ID.
    pub(crate) motion_states: HashMap<u64, crate::motion::MotionState>,
    /// The motion value graph JS creates through `createMotionValue` and friends.
    pub(crate) motion_values: crate::motion::MotionValueStore,
    /// The `@keyframes` analogue: the sets `animation-name` refers to, pushed
    /// here by JS through `defineKeyframes`.
    pub(crate) keyframes: crate::motion::KeyframesRegistry,
    /// CSS transition state of the elements that declare `transition*`.
    css_transitions: crate::motion::CssTransitions,
    /// CSS animation state of the elements that declare `animation*`.
    css_animations: crate::motion::CssAnimations,
    /// What the last paint measured for the elements that animate.
    measurements: crate::motion::measure::Measurements,
    /// Hover and press state for those elements, until the event model owns it.
    /// The glide of the elements that declare `layout`.
    layout_flips: crate::motion::LayoutFlips,
    /// Serialised markup of the `<svg>` elements that have shape children.
    svg_trees: crate::custom_elements::svg_tree::SvgTrees,
    /// Live text selection, shared with the paint closures and the napi methods.
    pub(crate) selection: SharedSelection,
    /// Persistent measurement and scroll state for React-backed virtual lists.
    virtual_lists: HashMap<u64, VirtualListEntry>,
    /// Latest pointer sample and list during selection edge scrolling.
    selection_drag_position: Option<gpui::Point<gpui::Pixels>>,
    selection_scroll_list: Option<u64>,
    selection_scroll_task: Option<gpui::Task<()>>,
    /// Motion / review clock. Live wall time unless automation freezes it.
    pub(crate) clock: crate::automation::AutomationClock,
    /// Resolved `highlight` state, keyed by the element that declared it.
    /// Empty in every app that does not use search.
    highlights: HashMap<u64, HighlightCacheEntry>,
    /// What `onLayout` and `onScroll` last reported, for the elements that
    /// have the listener. Shared with the paint closures that compare against it.
    paint_reports: SharedPaintReports,
    /// Where the pointer is and what it is pressing.
    /// Shared with the window's mouse-down listener, which dispatches a press
    /// without reaching the view.
    pub(crate) pointer: Rc<RefCell<crate::events::state::PointerState>>,
}

#[derive(Default)]
pub(crate) struct PaintReports {
    /// Border-box size last delivered through `onLayout`.
    sizes: HashMap<u64, (f32, f32)>,
    /// Scroll offset last seen for `onScroll`. Recorded on the first paint
    /// without an event, so only movement is reported.
    scrolls: HashMap<u64, (f32, f32)>,
    /// What `ResizeObserver` and `IntersectionObserver` last saw.
    pub(crate) observers: crate::observers::ObserverReports,
}

pub(crate) type SharedPaintReports = Rc<RefCell<PaintReports>>;

/// Two-level cache for one element's `highlight`.
///
/// The group list is keyed by `search_revision`, which a query change does NOT
/// move, so typing in a find bar never re-walks or re-folds text. The matches
/// are additionally keyed by the matcher hash, which excludes `activeIndex` and
/// the colours, so moving the find cursor only re-colours what it already found.
///
/// Do not key the group list on `subtree_revision`: `highlight` is a custom
/// prop, so every keystroke moves that revision and the cache would do nothing.
/// `highlight_cache_tests` at the bottom of this file compares `Arc` identity
/// and fails if either level regresses. A timing budget does not catch it: on
/// the 1000-turn chat the broken version is 2.7ms against 1.9ms.
struct HighlightCacheEntry {
    revision: u64,
    groups: Arc<crate::text::GroupList>,
    matcher_hash: u64,
    /// The spec plus the located matches. Ordinals and colours are decided at
    /// paint, so a colour or `activeIndex` change reuses this whole value.
    context: Arc<crate::text::HighlightContext>,
    /// Last identity delivered through `onHighlight`. Only written once an
    /// event is really queued, so adding the listener later still reports.
    reported: Option<u64>,
}

fn emit_motion_settled(
    callback: &Option<EventCallback>,
    tree: &crate::retained_tree::RetainedTree,
    completions: &[(u64, u64)],
) {
    for &(id, generation) in completions {
        let Some(element) = tree.elements.get(&id) else {
            continue;
        };
        if !element.events().contains("motionComplete") {
            continue;
        }
        emit_event_full(callback, id, "motionComplete", |payload| {
            payload.motion_generation = Some(generation as f64);
        });
    }
}

/// Samples the motion values the tree binds and the ones JS listens to. A
/// virtual row rebuilt between two root frames reads what this sampled.
fn sync_motion_values(
    tree: &crate::retained_tree::RetainedTree,
    store: &mut crate::motion::MotionValueStore,
    now: web_time::Instant,
) -> crate::motion::MotionValueFrame {
    let elements = || {
        tree.motion_value_ids
            .iter()
            .filter_map(|id| tree.elements.get(id))
    };
    store.frame(
        elements().filter_map(|element| element.custom_props().get("motionValues")),
        &elements()
            .filter_map(|element| element.custom_props().get("layout"))
            .flat_map(crate::motion::drag_value_ids)
            .collect::<Vec<_>>(),
        now,
    )
}

/// Samples the values the subtree of a virtual row binds that the root's
/// sample did not reach: a row mounted after it has none sampled, and would
/// render unbound for a frame. Says whether one of them is still moving.
fn sync_row_motion_values(
    tree: &crate::retained_tree::RetainedTree,
    store: &mut crate::motion::MotionValueStore,
    row_id: u64,
    now: web_time::Instant,
) -> bool {
    if tree.motion_value_ids.is_empty() {
        return false;
    }
    let mut bindings = Vec::new();
    let mut read = Vec::new();
    let mut pending = vec![row_id];
    while let Some(id) = pending.pop() {
        let Some(element) = tree.elements.get(&id) else {
            continue;
        };
        if let Some(binding) = element.custom_props().get("motionValues") {
            bindings.push(binding);
        }
        if let Some(source) = element.custom_props().get("layout") {
            read.extend(crate::motion::drag_value_ids(source));
        }
        pending.extend(element.children.iter().copied());
    }
    store.sample_unsampled(bindings, &read, now)
}

/// The element id of these events is the motion value's, so the JS side tells
/// them apart by event type.
fn emit_motion_value_events(
    callback: &Option<EventCallback>,
    events: &[crate::motion::MotionValueEvent],
) {
    for event in events {
        emit_event_full(callback, u64::from(event.id), event.kind.event_type(), |payload| {
            payload.value = event.value.clone();
        });
    }
}

/// What `id`'s parent hands it, from the styles the tree declares: the colour
/// context and the font. Motion resolves its values before the build, which is
/// where these are otherwise made, so a state style (`hover`) that sets an
/// ancestor's `color` or `fontSize` is not in them. `known` holds what was already
/// made this frame, for the element each entry belongs to.
fn parent_inherited(
    tree: &crate::retained_tree::RetainedTree,
    id: u64,
    root: &(crate::color::ColorContext, crate::motion::ParentFont),
    known: &mut HashMap<u64, (crate::color::ColorContext, crate::motion::ParentFont)>,
) -> (crate::color::ColorContext, crate::motion::ParentFont) {
    let Some(parent) = tree.elements.get(&id).and_then(|element| element.parent) else {
        return *root;
    };
    if let Some(inherited) = known.get(&parent) {
        return *inherited;
    }
    let above = parent_inherited(tree, parent, root, known);
    let style = tree.elements.get(&parent).and_then(|element| element.style.as_deref());
    let inherited = match style {
        Some(style) => (above.0.descend(style), above.1.descend(Some(style))),
        None => (above.0, above.1.descend(None)),
    };
    known.insert(parent, inherited);
    inherited
}

fn sync_motion_states(
    tree: &crate::retained_tree::RetainedTree,
    states: &mut HashMap<u64, crate::motion::MotionState>,
    measurements: &crate::motion::measure::Measurements,
    appearance: gpui::WindowAppearance,
    viewport: gpui::Size<gpui::Pixels>,
    now: web_time::Instant,
) -> (bool, Vec<(u64, u64)>) {
    let painted = measurements.painted_frames();
    let mut root_font = crate::motion::ParentFont::initial();
    root_font.lengths = LengthContext::initial(
        f64::from(f32::from(viewport.width)),
        f64::from(f32::from(viewport.height)),
    );
    let root = (crate::color::ColorContext::root(color_scheme_of(appearance)), root_font);
    let mut known_colors = HashMap::new();
    states.retain(|id, _| tree.motion_ids.contains(id));
    let mut active = false;
    let mut settled = Vec::new();

    for &id in &tree.motion_ids {
        let Some(element) = tree.elements.get(&id) else {
            continue;
        };
        let Some(source) = element.custom_props().get("motion") else {
            continue;
        };
        let base = element.style.as_deref();
        let (parent_colors, parent_font) = parent_inherited(tree, id, &root, &mut known_colors);
        let colors = crate::motion::Colors::inheriting(&parent_colors, &parent_font, base);
        let measurement = measurements.get(id);
        // An SVG shape animates its attributes, not a style.
        let svg_attributes = crate::custom_elements::svg_tree::is_svg_element(tree, element)
            .then(|| element.custom_props());
        let state = match states.entry(id) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let created = match svg_attributes {
                    Some(attributes) => {
                        crate::motion::MotionState::new_svg(source, attributes, colors, now)
                    }
                    None => crate::motion::MotionState::new(
                        source,
                        base,
                        colors,
                        measurement.as_ref(),
                        painted,
                        now,
                    ),
                };
                match created {
                    Ok(state) => entry.insert(state),
                    Err(error) => {
                        log::warn!("Invalid motion description for element {id}: {error}");
                        entry.insert(crate::motion::MotionState::invalid(source, colors, now))
                    }
                }
            }
        };
        let synced = match svg_attributes {
            Some(attributes) => state.sync_svg(source, attributes, colors, now),
            None => state.sync(source, base, colors, measurement.as_ref(), painted, now),
        };
        if let Err(error) = synced {
            log::warn!("Invalid motion update for element {id}: {error}");
        }
        let frame = state.frame(now);
        active |= frame.active;
        if frame.just_settled {
            settled.push((id, frame.generation));
        }
    }

    (active, settled)
}

/// Emits a DOM event that bubbles and cannot be cancelled, aimed at `id`, when
/// the element or an ancestor listens for it.
fn emit_bubbling_event(
    callback: &Option<EventCallback>,
    tree: &crate::retained_tree::RetainedTree,
    id: u64,
    event_type: &str,
    build: impl FnOnce(&mut EventPayload),
) {
    if !crate::events::path::path_listens(tree, id, event_type) {
        return;
    }
    emit_event_full(callback, id, event_type, |payload| {
        payload.target = Some(id as f64);
        payload.path = Some(
            crate::events::path::event_path(tree, id)
                .into_iter()
                .map(|id| id as f64)
                .collect(),
        );
        build(payload);
    });
}

fn emit_transition_events(
    callback: &Option<EventCallback>,
    tree: &crate::retained_tree::RetainedTree,
    events: Vec<crate::motion::TransitionEvent>,
) {
    for event in events {
        emit_bubbling_event(callback, tree, event.id, event.kind.event_type(), |payload| {
            payload.property_name = Some(event.property_name());
            payload.elapsed_time = Some(event.elapsed);
            payload.pseudo_element = Some(String::new());
        });
    }
}

fn emit_layout_events(
    callback: &Option<EventCallback>,
    tree: &crate::retained_tree::RetainedTree,
    events: Vec<crate::motion::LayoutEvent>,
) {
    for event in events {
        let event_type = event.kind.event_type();
        let listens = tree
            .elements
            .get(&event.id)
            .is_some_and(|element| element.events().contains(event_type));
        if listens {
            emit_event_full(callback, event.id, event_type, |_| {});
        }
    }
}

fn emit_animation_events(
    callback: &Option<EventCallback>,
    tree: &crate::retained_tree::RetainedTree,
    events: Vec<crate::motion::AnimationEvent>,
) {
    for event in events {
        emit_bubbling_event(callback, tree, event.id, event.kind.event_type(), |payload| {
            payload.animation_name = Some(event.name);
            payload.elapsed_time = Some(event.elapsed);
            payload.pseudo_element = Some(String::new());
        });
    }
}

fn emit_highlight_events(callback: &Option<EventCallback>, events: &[(u64, usize)]) {
    for &(id, total) in events {
        emit_event_full(callback, id, "highlight", |payload| {
            payload.match_count = Some(total as f64);
        });
    }
}

/// Resolve one element's `highlight` prop, reusing both cache levels.
///
/// Returns the context, plus the match count when `has_listener` and the result
/// differs from the last one this element reported. Identity, not count:
/// swapping a query for a different one with the same number of hits is still a
/// new result.
fn resolve_highlight(
    cache: &mut HashMap<u64, HighlightCacheEntry>,
    tree: &RetainedTree,
    id: u64,
    value: &serde_json::Value,
    theme: &Theme,
    has_listener: bool,
) -> Option<(Arc<crate::text::HighlightContext>, Option<usize>)> {
    let set = crate::text::HighlightSet::parse(value, theme)?;
    // `search_revision`, NOT `subtree_revision`: `highlight` is a custom prop,
    // so the general revision moves on every keystroke and this cache would
    // never hit for the one case it exists for.
    let revision = tree.elements.get(&id)?.search_revision;
    let matcher_hash = set.matcher_hash();

    let cached = cache
        .get(&id)
        .filter(|entry| entry.revision == revision && entry.matcher_hash == matcher_hash);
    let context = match cached {
        // Nothing moved at all. Returning the same `Arc` keeps the whole
        // subtree's inherited value identical, which the cache tests assert.
        Some(entry) if entry.context.set == set => entry.context.clone(),
        // Same matches, different colours or find cursor: reuse the located
        // matches and swap only the spec. No text is scanned.
        Some(entry) => {
            let context = Arc::new(crate::text::HighlightContext {
                declaration: id,
                set,
                matches: entry.context.matches.clone(),
            });
            cache.get_mut(&id)?.context = context.clone();
            context
        }
        None => {
            let groups = match cache.get(&id) {
                Some(entry) if entry.revision == revision => entry.groups.clone(),
                _ => Arc::new(crate::text::GroupList::collect(tree, id)),
            };
            let context = Arc::new(crate::text::HighlightContext {
                declaration: id,
                matches: Arc::new(crate::text::search::resolve(&groups, &set)),
                set,
            });
            let reported = cache.get(&id).and_then(|entry| entry.reported);
            cache.insert(
                id,
                HighlightCacheEntry {
                    revision,
                    groups,
                    matcher_hash,
                    context: context.clone(),
                    reported,
                },
            );
            context
        }
    };

    if !has_listener {
        return Some((context, None));
    }
    let identity = context.matches.identity();
    let entry = cache.get_mut(&id)?;
    if entry.reported == Some(identity) {
        return Some((context, None));
    }
    entry.reported = Some(identity);
    let total = context.matches.total;
    Some((context, Some(total)))
}

impl GpuixView {
    pub(crate) fn new(
        tree: Arc<Mutex<RetainedTree>>,
        event_callback: Option<EventCallback>,
        window_title: String,
        selection: SharedSelection,
        pointer_requests: crate::events::state::PointerRequests,
    ) -> Self {
        Self {
            tree,
            event_callback,
            window_title,
            routing: Default::default(),
            key_down_interception: None,
            window_selection_change: false,
            window_selection_event_id: 0,
            window_appearance_change: false,
            window_appearance_event_id: 0,
            appearance_subscription: None,
            bounds_subscription: None,
            media_viewport: gpui::Size::default(),
            simulated_platform: None,
            simulated_window_actions: None,
            frameless: false,
            window_media_change: false,
            #[cfg(target_os = "macos")]
            media_observer: None,
            #[cfg(target_os = "macos")]
            scroller_style_observer: None,
            scroller_style: None,
            reported_selection: None,
            focus_handles: HashMap::new(),
            pending_focus_element: None,
            focus: crate::focus::FocusState::default(),
            custom_registry: CustomElementRegistry::with_defaults(),
            scroll_handles: HashMap::new(),
            motion_states: HashMap::new(),
            motion_values: Default::default(),
            keyframes: Default::default(),
            css_transitions: Default::default(),
            css_animations: Default::default(),
            measurements: Default::default(),
            layout_flips: Default::default(),
            svg_trees: Default::default(),
            selection,
            virtual_lists: HashMap::new(),
            selection_drag_position: None,
            selection_scroll_list: None,
            selection_scroll_task: None,
            clock: crate::automation::AutomationClock::new(),
            highlights: HashMap::new(),
            paint_reports: SharedPaintReports::default(),
            pointer: Rc::new(RefCell::new(crate::events::state::PointerState::with_requests(
                pointer_requests,
            ))),
        }
    }

    /// Hands one pointer input to the dispatcher and delivers the events it
    /// produces. Runs deferred, after GPUI is done with the event. A button
    /// press does not come through here: its listener dispatches it at once.
    pub(crate) fn handle_pointer(
        &mut self,
        mut sample: crate::events::dispatcher::PointerSample,
        cx: &mut gpui::Context<Self>,
    ) {
        if let (Some(platform), crate::events::dispatcher::PointerAction::Move { .. }) =
            (&self.simulated_platform, &sample.action)
        {
            sample.movement = platform.movement();
        }
        let output = crate::events::listeners::dispatch_sample(&self.pointer, &self.tree, &sample);
        if output.restyle {
            cx.notify();
        }
        let Some(callback) = &self.event_callback else {
            return;
        };
        for payload in output.into_payloads() {
            callback(payload);
        }
    }

    pub(crate) fn set_live_image(
        &mut self,
        id: u64,
        image: std::sync::Arc<gpui::RenderImage>,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> std::result::Result<(), String> {
        {
            let tree = self.tree.lock().unwrap();
            let element = tree
                .elements
                .get(&id)
                .ok_or_else(|| format!("Unknown element id {id}"))?;
            if element.element_type != "img" {
                return Err(format!(
                    "setImage is only valid on <img>, got <{}>",
                    element.element_type
                ));
            }
        }
        let previous = self.custom_registry.set_live_image(id, image)?;
        if let Some(previous) = previous {
            window.drop_image(previous).ok();
        }
        cx.notify();
        window.refresh();
        Ok(())
    }

    pub(crate) fn set_image_pixels(
        &mut self,
        id: u64,
        width: u32,
        height: u32,
        bytes: Vec<u8>,
        format: crate::custom_elements::img::PixelFormat,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> std::result::Result<(), String> {
        let image =
            crate::custom_elements::img::render_image_from_pixels(width, height, bytes, format)?;
        self.set_live_image(id, image, window, cx)
    }

    pub(crate) fn set_encoded_image(
        &mut self,
        id: u64,
        bytes: Vec<u8>,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> std::result::Result<(), String> {
        let image = crate::custom_elements::img::render_image_from_encoded(
            bytes,
            cx.svg_renderer(),
        )?;
        self.set_live_image(id, image, window, cx)
    }

    /// Adds the fonts in `bytes` to the text system and repaints, so text that
    /// names one of them picks it up.
    pub(crate) fn register_font(
        &mut self,
        bytes: Vec<u8>,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> std::result::Result<(), String> {
        check_font(&bytes)?;
        window
            .text_system()
            .add_fonts(vec![std::borrow::Cow::Owned(bytes)])
            .map_err(|error| error.to_string())?;
        cx.notify();
        window.refresh();
        Ok(())
    }

    /// Scrolls for a key nothing consumed. See `GpuixRenderer::scroll_by_key`.
    pub(crate) fn scroll_by_key(&self, key: &str, shift: bool, window: &gpui::Window) -> bool {
        let Some(key) = crate::scroll::KeyScroll::parse(key, shift) else {
            return false;
        };
        let tree = self.tree.lock().unwrap();
        let mut scrollers: Vec<(u64, &gpui::ScrollHandle)> = Vec::new();
        if let Some(focused) = self.focused_element_id(window) {
            let Some(element) = tree.elements.get(&focused) else {
                return false;
            };
            if matches!(element.element_type.as_str(), "input" | "textarea") {
                return false;
            }
            if key.activates_controls && element.events().contains("click") {
                return false;
            }
            let reaching = crate::layout::containing_block::reaching_ancestors(&tree, focused);
            for id in std::iter::once(focused).chain(reaching) {
                if let Some(handle) = self.scroll_handles.get(&id) {
                    scrollers.push((id, handle));
                }
            }
        } else {
            let pointer = window.mouse_position();
            let mut under_pointer: Vec<(f32, u64, &gpui::ScrollHandle)> = self
                .scroll_handles
                .iter()
                .filter_map(|(id, handle)| {
                    let port = handle.padding_box();
                    port.contains(&pointer).then(|| {
                        (f32::from(port.size.width) * f32::from(port.size.height), *id, handle)
                    })
                })
                .collect();
            under_pointer.sort_by(|a, b| a.0.total_cmp(&b.0));
            scrollers.extend(under_pointer.into_iter().map(|(_, id, handle)| (id, handle)));
        }
        crate::scroll::scroll_by_key(scrollers, key)
    }

    /// The DOM's `scrollIntoView`. Walks the scrollers around the element from the inside out
    /// and scrolls each until the element sits where `options` ask, using the element's painted
    /// box rather than its index among its siblings, so a descendant of any depth works.
    /// An element that has not been painted has no box to align, which only a virtual list's row
    /// can do without (it goes to the top). False if nothing scrolled.
    pub(crate) fn scroll_element_into_view(&self, id: u64, options: crate::scroll::IntoView) -> bool {
        let mut target = crate::automation::get_bounds(id).map(|painted| crate::scroll::Rect {
            x: painted.x as f32,
            y: painted.y as f32,
            width: painted.width as f32,
            height: painted.height as f32,
        });
        let tree = self.tree.lock().unwrap();
        let mut scrolled = false;
        let mut child = id;
        // Only the ancestors that move the element: an absolute one stays put under a static
        // scroller, and a fixed one under all of them.
        for ancestor_id in crate::layout::containing_block::reaching_ancestors(&tree, id) {
            if let Some(entry) = self.virtual_lists.get(&ancestor_id) {
                if let Some(index) = entry.logical_index_of(child) {
                    let viewport = entry.state.viewport_bounds();
                    let row = crate::automation::get_bounds(child);
                    let offset = crate::scroll::virtual_list_offset(
                        options.block,
                        row.map(|row| (row.y as f32, row.height as f32)),
                        (f32::from(viewport.origin.y), f32::from(viewport.size.height)),
                    );
                    if let Some(offset) = offset {
                        scrolled |= self.scroll_virtual_list_to_item(ancestor_id, index, offset);
                        // The row moves when the list does, and the list applies that on the
                        // next frame, so the scrollers around it have no box to align yet.
                        target = None;
                    }
                }
            } else if let (Some(handle), Some(target)) =
                (self.scroll_handles.get(&ancestor_id), target.as_mut())
            {
                scrolled |= crate::scroll::scroll_into_view(ancestor_id, handle, target, options);
            }
            child = ancestor_id;
        }
        scrolled
    }

    pub(crate) fn set_selection_change_listener(&mut self, enabled: bool, event_id: u64) {
        self.window_selection_change = enabled;
        self.window_selection_event_id = event_id;
        self.reported_selection = None;
    }

    pub(crate) fn set_appearance_change_listener(&mut self, enabled: bool, event_id: u64) {
        self.window_appearance_change = enabled;
        self.window_appearance_event_id = event_id;
    }

    pub(crate) fn set_media_change_listener(&mut self, enabled: bool) {
        self.window_media_change = enabled;
        #[cfg(target_os = "macos")]
        {
            if !enabled {
                self.media_observer = None;
            } else if self.media_observer.is_none() {
                let callback = self.event_callback.clone();
                self.media_observer = Some(crate::media::MediaChangeObserver::new(move || {
                    crate::media::emit_media_change(&callback, "system");
                }));
            }
        }
    }

    /// The appearance half of `set_media_change_listener`: `prefers-color-scheme` changes
    /// with the window appearance. The test window has no OS notifications to observe, so it
    /// does not take the other half.
    #[cfg(feature = "test-support")]
    pub(crate) fn set_appearance_media_listener(&mut self, enabled: bool) {
        self.window_media_change = enabled;
    }

    /// How many frames have been painted, for a test that counts the draws a read caused.
    #[cfg(feature = "test-support")]
    pub(crate) fn painted_frame_count(&self) -> u64 {
        self.measurements.painted_frames()
    }

    /// Emit `appearanceChange` from GPUI's window appearance observer, not from
    /// render: the platform reports the change, so there is nothing to diff.
    pub(crate) fn observe_appearance(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.appearance_subscription =
            Some(cx.observe_window_appearance(window, |view, window, cx| {
                // `colorScheme: "light dark"` resolves against the appearance
                // while building, so a change has to rebuild even when no JS
                // listener asked for the event.
                cx.notify();
                if view.window_media_change {
                    crate::media::emit_media_change(&view.event_callback, "appearance");
                }
                if !view.window_appearance_change {
                    return;
                }
                let value = appearance_name(window.appearance()).to_string();
                emit_event_full(
                    &view.event_callback,
                    view.window_appearance_event_id,
                    "appearanceChange",
                    |payload| {
                        payload.value = Some(value);
                    },
                );
            }));
    }

    /// Emits `mediaChange` when the content size changes, which is what the size features of
    /// `matchMedia` read. The size is tracked whether or not anyone listens, so enabling the
    /// listener later does not report a resize that already happened.
    pub(crate) fn observe_viewport(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.media_viewport = window.viewport_size();
        self.bounds_subscription = Some(cx.observe_window_bounds(window, |view, window, _cx| {
            if crate::media::viewport_resized(&mut view.media_viewport, window.viewport_size())
                && view.window_media_change
            {
                crate::media::emit_media_change(&view.event_callback, "resize");
            }
        }));
    }

    /// Rebuilds the window when the OS "Show scroll bars" setting changes, since the style
    /// decides whether bars take layout space.
    pub(crate) fn observe_scroller_style(&mut self, cx: &mut gpui::Context<Self>) {
        #[cfg(target_os = "macos")]
        {
            let view = cx.weak_entity();
            let app = cx.to_async();
            self.scroller_style_observer =
                Some(crate::scrollbar::ScrollerStyleObserver::new(move || {
                    if let Err(error) = view.update(&mut app.clone(), |_, cx| cx.notify()) {
                        eprintln!("gpuix: could not rebuild after the scroll bar style changed: {error}");
                    }
                }));
        }
        #[cfg(not(target_os = "macos"))]
        let _ = cx;
    }

    /// Uses `style` instead of the OS setting, for a renderer that must not depend on the
    /// machine it runs on.
    #[cfg(feature = "test-support")]
    pub(crate) fn force_scroller_style(&mut self, style: crate::scrollbar::ScrollerStyle) {
        self.scroller_style = Some(style);
    }

    pub(crate) fn set_frameless(&mut self, frameless: bool) {
        self.frameless = frameless;
    }

    pub(crate) fn simulate_window_actions(
        &mut self,
        actions: crate::events::app_region::SimulatedWindowActions,
    ) {
        self.simulated_window_actions = Some(actions);
    }

    pub(crate) fn simulate_platform_pointer_lock(
        &mut self,
        platform: crate::events::pointer_lock::SimulatedPlatform,
    ) {
        self.simulated_platform = Some(platform);
    }

    /// Applies the pointer lock JS asked for and reports what came of it as
    /// `pointerlockchange` and `pointerlockerror`. Document-level events: the
    /// element, when there is one, is the payload's `target`.
    fn apply_pointer_lock(&mut self, window: &mut gpui::Window, cx: &mut gpui::App) {
        use crate::events::pointer_lock::{apply, GpuiLock};
        let events = {
            let tree = self.tree.lock().unwrap();
            match self.simulated_platform.clone() {
                Some(mut platform) => apply(&mut self.pointer.borrow_mut(), &tree, &mut platform),
                None => apply(&mut self.pointer.borrow_mut(), &tree, &mut GpuiLock { window, cx }),
            }
        };
        for event in events {
            emit_event_full(&self.event_callback, 0, event.event_type(), |payload| {
                payload.target = event.element().map(|id| id as f64);
            });
        }
    }

    /// Send `keydown` for every keystroke, before GPUI binds it to an action.
    /// Call once, right after the view is created.
    pub(crate) fn install_key_interception(&mut self, window: &gpui::Window, cx: &mut gpui::App) {
        self.key_down_interception = Some(crate::events::keyboard::install_key_down(
            cx,
            window.window_handle().window_id(),
            self.tree.clone(),
            self.event_callback.clone(),
            self.routing.clone(),
        ));
    }

    /// Emit `selectionChange` when the selected range set changed this frame.
    ///
    /// Reads the same `SelectionState` as `getSelectedText`. Keyed on identity
    /// so an unchanged frame does not emit. Empty on mount is not a change.
    /// `reported_selection` is written only when an event is queued, so adding
    /// `onSelectionChange` later still reports a live selection.
    fn emit_selection_change(&mut self, callback: &Option<EventCallback>) {
        if !self.window_selection_change {
            return;
        }
        let selection = self.selection.lock();
        let identity = selection.identity();
        if self.reported_selection == Some(identity) {
            return;
        }
        if identity == 0 && self.reported_selection.is_none() {
            return;
        }
        let value = selection.selected_text();
        drop(selection);
        self.reported_selection = Some(identity);
        emit_event_full(
            callback,
            self.window_selection_event_id,
            "selectionChange",
            |payload| {
                payload.value = value;
            },
        );
    }

    fn build_virtual_child(
        &mut self,
        list_id: u64,
        index: usize,
        expected_child_id: u64,
        inherited: Inherited,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        use gpui::prelude::*;

        let row_focus_handle = self.virtual_lists.get_mut(&list_id).and_then(|entry| {
            entry.seen_rows.insert(expected_child_id);
            (entry.child_at(index) == Some(expected_child_id))
                .then(|| {
                    index
                        .checked_sub(entry.window_start)
                        .and_then(|offset| entry.row_focus_handles.get(offset).cloned())
                })
                .flatten()
                .flatten()
        });

        let tree_arc = self.tree.clone();
        let tree = tree_arc.lock().unwrap();
        let window_start = self
            .virtual_lists
            .get(&list_id)
            .map(|entry| entry.window_start)
            .unwrap_or(0);
        let child_matches = tree.elements.get(&list_id).and_then(|list| {
            index
                .checked_sub(window_start)
                .and_then(|offset| list.children.get(offset))
        }) == Some(&expected_child_id);
        if !child_matches {
            let height = self
                .virtual_lists
                .get(&list_id)
                .and_then(|entry| entry.config.estimated_item_height)
                .unwrap_or(1.0);
            return unmounted_virtual_row(height);
        }

        let callback = self.event_callback.clone();
        let now = self.clock.now();
        let (motion_active, motion_settled) =
            sync_motion_states(
                &tree,
                &mut self.motion_states,
                &self.measurements,
                window.appearance(),
                window.viewport_size(),
                now,
            );
        let motion_values_active =
            sync_row_motion_values(&tree, &mut self.motion_values, expected_child_id, now);
        let mut highlight_events = Vec::new();

        // Re-resolve against the tree as it is NOW. gpui calls this during
        // layout and prepaint, after the root render returned, and on Windows
        // and Linux the Node thread can commit new text in between. Reusing the
        // captured ranges would paint a wash over the wrong glyphs, or at a byte
        // offset that is no longer a character boundary.
        let mut inherited = inherited;
        let viewport = window.viewport_size();
        inherited.lengths = inherited
            .lengths
            .with_viewport(f64::from(f32::from(viewport.width)), f64::from(f32::from(viewport.height)));
        if let Some(declaration) = inherited.highlight.as_ref().map(|ctx| ctx.declaration) {
            inherited.highlight = tree
                .elements
                .get(&declaration)
                .and_then(|element| element.custom_props().get("highlight"))
                .and_then(|value| {
                    resolve_highlight(
                        &mut self.highlights,
                        &tree,
                        declaration,
                        value,
                        &Theme::dark(),
                        false,
                    )
                })
                .map(|(context, _)| context);
        }

        let focus_styles = self.focus.styles(&tree, tree.keyboard_focus_dim());
        crate::accessibility::links::sync(&tree);
        let pointer_state = self.pointer.clone();
        let pointer = pointer_state.borrow();
        let activation = self.activation();
        crate::layout::box_generation::hold_display(self.css_transitions.held_displays());
        let mut build_ctx = BuildCtx {
            tree: &tree,
            event_callback: &callback,
            activation: &activation,
            focus_handles: &self.focus_handles,
            scroll_handles: &mut self.scroll_handles,
            custom_registry: &mut self.custom_registry,
            virtual_lists: &mut self.virtual_lists,
            motion_states: &mut self.motion_states,
            motion_values: &self.motion_values,
            css_transitions: &mut self.css_transitions,
            css_animations: &mut self.css_animations,
            keyframes: &self.keyframes,
            measurements: &self.measurements,
            layout_flips: &self.layout_flips,
            svg_trees: &mut self.svg_trees,
            now,
            selection: self.selection.clone(),
            inherited,
            highlights: &mut self.highlights,
            paint_reports: &self.paint_reports,
            highlight_events: &mut highlight_events,
            focus_styles,
            pointer: &pointer,
            contents_overlay: None,
            anchor_scope: None,
            stacking_root: true,
        };
        let child = build_element(expected_child_id, &mut build_ctx, window, cx);
        drop(pointer);
        emit_highlight_events(&callback, &highlight_events);
        emit_motion_settled(&callback, &tree, &motion_settled);
        emit_transition_events(&callback, &tree, self.css_transitions.take_events());
        emit_animation_events(&callback, &tree, self.css_animations.take_events());
        emit_layout_events(&callback, &tree, self.layout_flips.take_events());
        if self.css_transitions.take_active()
            || self.css_animations.take_active()
            || motion_active
            || motion_values_active
        {
            window.request_animation_frame();
        }
        let Some(focus_handle) = row_focus_handle else {
            return child;
        };
        gpui::div()
            .id(gpui::SharedString::from(format!(
                "__gpuix_virtual_row_{}_{}",
                list_id, expected_child_id
            )))
            .w_full()
            .track_focus(&focus_handle)
            .child(child)
            .into_any_element()
    }

    pub(crate) fn scroll_virtual_list_to_item(
        &self,
        id: u64,
        index: usize,
        offset_in_item: f32,
    ) -> bool {
        if !self.virtual_lists.contains_key(&id) {
            return false;
        }
        queue_virtual_list_scroll(id, index, offset_in_item);
        emit_event_full(&self.event_callback, id, "visibleRange", |payload| {
            payload.start_index = Some(index as f64);
            payload.end_index = Some((index + 1) as f64);
        });
        true
    }

    /// The list's logical scroll anchor as
    /// `[item_ix, offset_in_item_px, viewport_height_px]`.
    ///
    /// `item_ix == item count` is gpui's at-end sentinel (a bottom-aligned
    /// list resting at its very end); the viewport height is what lets a
    /// caller convert that into a position relative to the trailing rows.
    pub(crate) fn virtual_list_scroll_top(&self, id: u64) -> Option<[f64; 3]> {
        let state = &self.virtual_lists.get(&id)?.state;
        let top = state.logical_scroll_top();
        Some([
            top.item_ix as f64,
            f64::from(f32::from(top.offset_in_item)),
            f64::from(f32::from(state.viewport_bounds().size.height)),
        ])
    }

    pub(crate) fn set_virtual_list_offset(
        &self,
        id: u64,
        offset: gpui::Point<gpui::Pixels>,
    ) -> bool {
        let Some(entry) = self.virtual_lists.get(&id) else {
            return false;
        };
        entry.state.set_offset_from_scrollbar(offset);
        true
    }

    pub(crate) fn virtual_list_offset(&self, id: u64) -> Option<gpui::Point<gpui::Pixels>> {
        Some(
            self.virtual_lists
                .get(&id)?
                .state
                .scroll_px_offset_for_scrollbar(),
        )
    }

    pub(crate) fn reveal_virtual_list_ancestor(&self, id: u64) -> bool {
        let tree_arc = self.tree.clone();
        let tree = tree_arc.lock().unwrap();
        let mut current = id;
        let location = loop {
            let Some(parent_id) = tree
                .elements
                .get(&current)
                .and_then(|element| element.parent)
            else {
                break None;
            };
            if let Some(entry) = self.virtual_lists.get(&parent_id) {
                break entry.logical_index_of(current).map(|index| (parent_id, index));
            }
            current = parent_id;
        };
        drop(tree);

        let Some((list_id, index)) = location else {
            return false;
        };
        self.scroll_virtual_list_to_item(list_id, index, 0.0)
    }
}

/// Everything `build_element` threads through the tree.
///
/// Split into a struct because the recursion needs eight-plus shared references
/// and adding one more to every call site is how this file rots. `window` and
/// `cx` stay separate parameters: they are `&mut` and gpui reborrows them.
pub(crate) struct BuildCtx<'a> {
    pub tree: &'a RetainedTree,
    pub event_callback: &'a Option<EventCallback>,
    pub activation: &'a crate::events::activation::Activation,
    pub focus_handles: &'a HashMap<u64, gpui::FocusHandle>,
    pub scroll_handles: &'a mut HashMap<u64, gpui::ScrollHandle>,
    pub custom_registry: &'a mut CustomElementRegistry,
    virtual_lists: &'a mut HashMap<u64, VirtualListEntry>,
    pub motion_states: &'a mut HashMap<u64, crate::motion::MotionState>,
    motion_values: &'a crate::motion::MotionValueStore,
    css_transitions: &'a mut crate::motion::CssTransitions,
    css_animations: &'a mut crate::motion::CssAnimations,
    keyframes: &'a crate::motion::KeyframesRegistry,
    measurements: &'a crate::motion::measure::Measurements,
    layout_flips: &'a crate::motion::LayoutFlips,
    svg_trees: &'a mut crate::custom_elements::svg_tree::SvgTrees,
    pub now: web_time::Instant,
    pub selection: SharedSelection,
    /// Inherited text state, resolved the way CSS inherits it. The renderer's
    /// own theme only seeds the root selection wash; custom elements resolve
    /// their own theme from their `theme` prop.
    pub inherited: Inherited,
    /// Persistent `highlight` caches, keyed by the declaring element.
    highlights: &'a mut HashMap<u64, HighlightCacheEntry>,
    paint_reports: &'a SharedPaintReports,
    /// `onHighlight` payloads queued during the build.
    ///
    /// Never emitted inline: a handler that calls `setState` repaints, which
    /// would re-enter the build and emit again. They are flushed once the root
    /// build has returned.
    highlight_events: &'a mut Vec<(u64, usize)>,
    /// See `crate::focus::FocusState::styles`.
    pub focus_styles: crate::focus::FocusStyles,
    /// Which elements are hovered or pressed, for their `hover` / `active` styles.
    pub pointer: &'a crate::events::state::PointerState,
    /// Inheritable style of the `display: contents` ancestors of the child about to be
    /// built, taken by `build_element`.
    pub contents_overlay: Option<StyleDesc>,
    /// Set to an `<anchored>` element's id while building its direct children, taken by
    /// `build_element` to resolve `anchor-size()`.
    pub anchor_scope: Option<u64>,
    /// Set while building the first element of a build (the window root, a virtual-list row),
    /// which has no stacking context around it. Taken by `build_element`.
    pub stacking_root: bool,
}

impl BuildCtx<'_> {
    /// The states of `id` that `style` has a refinement for. Nothing applies in
    /// an inert subtree: nothing in it can be hovered, and it also ignores a
    /// group outside it.
    fn pointer_states(
        &self,
        id: u64,
        style: &StyleDesc,
        inert: bool,
    ) -> crate::events::state_styles::PointerStates {
        if inert || self.inherited.inert {
            return Default::default();
        }
        let followed = |refinement: &Option<Box<StyleDesc>>| {
            refinement
                .as_deref()
                .map(|refinement| self.inherited.group_state(&refinement.followed_group()))
        };
        crate::events::state_styles::PointerStates {
            hovered: style.hover.is_some() && self.pointer.is_hovered(id),
            active: style.active.is_some() && self.pointer.is_active(id),
            group_hovered: followed(&style.group_hover).is_some_and(|group| group.hovered),
            group_active: followed(&style.group_active).is_some_and(|group| group.active),
            group_focused_within: followed(&style.group_focus_within)
                .is_some_and(|group| group.focused_within),
            ..Default::default()
        }
    }
}

/// Style properties that cascade into descendants.
///
/// Not `Copy`: `highlight` holds an `Arc`. Every call site must clone
/// explicitly, including the deferred `build_virtual_child` callback, which gpui
/// may run more than once per frame.
#[derive(Clone)]
pub(crate) struct Inherited {
    /// False once an ancestor sets `userSelect: "none"`.
    pub selectable: bool,
    /// True below an element with the `inert` prop, which wins over any
    /// `pointerEvents` or `userSelect` of its descendants.
    pub inert: bool,
    /// Selection wash colour for this subtree.
    pub selection_wash: gpui::Hsla,
    /// The nearest ancestor's `highlight`, resolved. `None` in every app that
    /// does not use search. It carries the declaring element id, which is what
    /// a virtual-list row re-resolves against: that row is built after the root
    /// render returns, and on Windows and Linux the Node thread can edit text
    /// in between, so a stale range would paint over the wrong glyphs.
    pub highlight: Option<Arc<crate::text::HighlightContext>>,
    /// True below an element with `pointerEvents: "none"` and above any
    /// descendant that sets `auto` again, as in CSS where it is inherited.
    /// Always true in an inert subtree: `auto` does not reopen it.
    pub pointer_events_none: bool,
    /// The nearest `appRegion` above, itself included. Always `None` unless the window is
    /// frameless: a window with a titlebar of its own has nothing to drag.
    pub app_region: Option<AppRegion>,
    /// Whether this window has regions at all (`titlebarTransparent`).
    frameless: bool,
    /// Computed `color` and `colorScheme`, which `currentColor` and
    /// `light-dark()` resolve against.
    pub colors: crate::color::ColorContext,
    /// Computed `font-size`, root font size and viewport, which `em`, `rem` and
    /// the viewport units resolve against. The viewport is read again on every
    /// build, including a virtual list's rows.
    pub lengths: LengthContext,
    /// Computed `font-weight`, which an unset one of a transition or motion starts from.
    pub font_weight: f64,
    /// No element has been built yet: the next one is the root, whose computed
    /// font size becomes the `rem`.
    above_root: bool,
    /// `fontVariantNumeric` and `fontFeatureSettings` as inherited so far.
    pub font_state: crate::css::font_features::FontFeatureState,
    /// The combined features, only on the element whose style declares one of the
    /// two: gpui inherits a text style on its own, so descendants add nothing.
    pub font_features: Option<gpui::FontFeatures>,
    /// The nearest ancestor's `interpolateSize`.
    pub interpolate_size: crate::motion::InterpolateSize,
    /// What the parent is as a containing block, which decides whether a
    /// percentage `height` of a child has anything to resolve against.
    pub container: crate::motion::containing::Container,
    /// The `group` declarations above this element, nearest first, with the
    /// state their `groupHover` / `groupActive` / `groupFocusWithin` followers read.
    pub groups: Option<Arc<GroupScope>>,
}

/// One `group` declaration on the way down the tree.
pub(crate) struct GroupScope {
    name: gpui::SharedString,
    state: GroupState,
    parent: Option<Arc<GroupScope>>,
}

#[derive(Clone, Copy, Default)]
struct GroupState {
    hovered: bool,
    active: bool,
    /// The group element or a descendant has focus, the source `focusWithin` reads.
    focused_within: bool,
}

impl Inherited {
    /// The font the element being built inherits.
    fn parent_font(&self) -> crate::motion::ParentFont {
        crate::motion::ParentFont { lengths: self.lengths, above_root: self.above_root, weight: self.font_weight }
    }

    /// The state of the nearest ancestor `group` called `name`; all false when
    /// no ancestor declares it.
    fn group_state(&self, name: &gpui::SharedString) -> GroupState {
        let mut scope = self.groups.as_deref();
        while let Some(group) = scope {
            if group.name == *name {
                return group.state;
            }
            scope = group.parent.as_deref();
        }
        GroupState::default()
    }

    fn root(
        theme: &Theme,
        appearance: gpui::WindowAppearance,
        frameless: bool,
        viewport: gpui::Size<gpui::Pixels>,
    ) -> Self {
        let mut wash = theme.accent;
        wash.a = 0.35;
        Self {
            selectable: true,
            inert: false,
            selection_wash: wash,
            highlight: None,
            pointer_events_none: false,
            app_region: None,
            frameless,
            colors: crate::color::ColorContext::root(color_scheme_of(appearance)),
            lengths: LengthContext::initial(
                f64::from(f32::from(viewport.width)),
                f64::from(f32::from(viewport.height)),
            ),
            font_weight: crate::motion::INITIAL_FONT_WEIGHT,
            above_root: true,
            font_state: Default::default(),
            font_features: None,
            interpolate_size: Default::default(),
            container: crate::motion::containing::Container::WINDOW,
            groups: None,
        }
    }

    /// Apply the inheritable parts of `style` for the subtree below it.
    fn descend(mut self, style: Option<&StyleDesc>, inert: bool) -> Self {
        self.lengths = self
            .lengths
            .descend(style.and_then(|style| style.font_size.as_ref()), self.above_root);
        self.above_root = false;
        self.font_features = None;
        self.container = self.container.within(style.unwrap_or(&EMPTY_STYLE));
        self.inert |= inert;
        if self.inert {
            self.selectable = false;
            self.pointer_events_none = true;
        }
        let Some(style) = style else { return self };
        if let Some(weight) = &style.font_weight {
            self.font_weight = f64::from(parse_font_weight(weight).0);
        }
        (self.font_state, self.font_features) = self.font_state.descend(
            style.font_variant_numeric.as_deref(),
            style.font_feature_settings.as_deref(),
        );
        match style.user_select.as_deref() {
            Some("none") => self.selectable = false,
            Some("text") | Some("auto") if !self.inert => self.selectable = true,
            _ => {}
        }
        self.colors = self.colors.descend(style);
        if let Some(color) = style
            .selection_color
            .as_deref()
            .and_then(|value| self.colors.resolve(value))
        {
            self.selection_wash = color.into();
        }
        match style.pointer_events.as_deref() {
            Some("none") => self.pointer_events_none = true,
            Some("auto") => self.pointer_events_none = self.inert,
            _ => {}
        }
        if self.frameless {
            self.app_region = style
                .app_region
                .as_deref()
                .and_then(AppRegion::parse)
                .or(self.app_region);
        }
        if let Some(interpolate_size) = crate::motion::InterpolateSize::declared(style) {
            self.interpolate_size = interpolate_size;
        }
        self
    }
}

fn json_usize(value: &serde_json::Value) -> Option<usize> {
    value
        .as_u64()
        .map(|n| n as usize)
        .or_else(|| {
            value
                .as_f64()
                .filter(|n| *n >= 0.0 && n.is_finite())
                .map(|n| n as usize)
        })
        .or_else(|| value.as_i64().filter(|n| *n >= 0).map(|n| n as usize))
}

fn window_start_from_element(element: &crate::retained_tree::RetainedElement) -> usize {
    element
        .custom_props()
        .get("windowStart")
        .and_then(json_usize)
        .unwrap_or(0)
}

#[derive(Clone, Copy, PartialEq)]
struct VirtualListConfig {
    alignment: gpui::ListAlignment,
    follow_tail: bool,
    overdraw: f32,
    estimated_item_height: Option<f32>,
    item_count: Option<usize>,
}

impl VirtualListConfig {
    fn from_element(element: &crate::retained_tree::RetainedElement) -> Self {
        let prop = |key: &str| element.custom_props().get(key);
        let alignment = match prop("alignment").and_then(serde_json::Value::as_str) {
            Some("bottom") => gpui::ListAlignment::Bottom,
            _ => gpui::ListAlignment::Top,
        };
        let follow_tail = prop("followTail")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let overdraw = prop("overdraw")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(512.0)
            .max(0.0) as f32;
        let estimated_item_height = prop("estimatedItemHeight")
            .and_then(serde_json::Value::as_f64)
            .filter(|height| *height > 0.0)
            .map(|height| height as f32);
        let item_count = estimated_item_height.and_then(|_| prop("itemCount").and_then(json_usize));
        Self {
            alignment,
            follow_tail,
            overdraw,
            estimated_item_height,
            item_count,
        }
    }

    fn logical_count(self, child_len: usize) -> usize {
        self.item_count.unwrap_or(child_len)
    }

    fn make_state(
        self,
        item_count: usize,
        focus_handles: &[Option<gpui::FocusHandle>],
    ) -> gpui::ListState {
        let mut state = gpui::ListState::new(item_count, self.alignment, gpui::px(self.overdraw));
        if focus_handles.len() == item_count {
            state.splice_focusable(0..item_count, focus_handles.iter().cloned());
        } else {
            state.splice_focusable(0..item_count, (0..item_count).map(|_| None));
        }
        if let Some(height) = self.estimated_item_height {
            state = state.with_uniform_item_height(gpui::px(height));
        }
        if self.follow_tail {
            state.set_follow_mode(gpui::FollowMode::Tail);
        }
        state
    }
}

struct VirtualListEntry {
    state: gpui::ListState,
    config: VirtualListConfig,
    window_start: usize,
    child_ids: Vec<u64>,
    child_revisions: Vec<u64>,
    row_focus_handles: Vec<Option<gpui::FocusHandle>>,
    seen_rows: HashSet<u64>,
}

impl VirtualListEntry {
    fn new(
        config: VirtualListConfig,
        window_start: usize,
        child_ids: Vec<u64>,
        child_revisions: Vec<u64>,
        row_focus_handles: Vec<Option<gpui::FocusHandle>>,
    ) -> Self {
        let item_count = config.logical_count(child_ids.len());
        let state = config.make_state(item_count, &row_focus_handles);
        if row_focus_handles.len() != item_count {
            for (offset, handle) in row_focus_handles.iter().enumerate() {
                if handle.is_some() {
                    let logical = window_start + offset;
                    if logical < item_count {
                        state.splice_focusable(
                            logical..logical + 1,
                            std::iter::once(handle.clone()),
                        );
                    }
                }
            }
        }
        Self {
            state,
            config,
            window_start,
            child_ids,
            child_revisions,
            row_focus_handles,
            seen_rows: HashSet::new(),
        }
    }

    fn child_at(&self, logical_index: usize) -> Option<u64> {
        logical_index
            .checked_sub(self.window_start)
            .and_then(|offset| self.child_ids.get(offset).copied())
    }

    fn logical_index_of(&self, child_id: u64) -> Option<usize> {
        self.child_ids
            .iter()
            .position(|id| *id == child_id)
            .map(|offset| self.window_start + offset)
    }

    fn sync(
        &mut self,
        config: VirtualListConfig,
        window_start: usize,
        child_ids: Vec<u64>,
        child_revisions: Vec<u64>,
        focusable_rows: &HashSet<u64>,
        cx: &mut gpui::Context<GpuixView>,
    ) {
        let focus_unchanged = self.child_ids == child_ids
            && self.row_focus_handles.len() == child_ids.len()
            && self
                .child_ids
                .iter()
                .zip(&self.row_focus_handles)
                .all(|(id, handle)| handle.is_some() == focusable_rows.contains(id));
        if self.config == config
            && self.window_start == window_start
            && focus_unchanged
            && self.child_revisions == child_revisions
        {
            return;
        }

        let old_rows: HashMap<u64, (u64, Option<gpui::FocusHandle>)> = self
            .child_ids
            .iter()
            .copied()
            .zip(self.child_revisions.iter().copied())
            .zip(self.row_focus_handles.iter().cloned())
            .map(|((id, revision), focus_handle)| (id, (revision, focus_handle)))
            .collect();
        let row_focus_handles: Vec<Option<gpui::FocusHandle>> = child_ids
            .iter()
            .map(|id| {
                focusable_rows.contains(id).then(|| {
                    old_rows
                        .get(id)
                        .and_then(|(_, focus_handle)| focus_handle.clone())
                        .unwrap_or_else(|| cx.focus_handle())
                })
            })
            .collect();
        if self.config != config {
            let scroll_top = self.state.logical_scroll_top();
            let should_follow =
                config.follow_tail && (!self.config.follow_tail || self.state.is_following_tail());
            let mut replacement = Self::new(
                config,
                window_start,
                child_ids,
                child_revisions,
                row_focus_handles,
            );
            replacement.seen_rows = std::mem::take(&mut self.seen_rows);
            replacement
                .seen_rows
                .retain(|id| replacement.child_ids.contains(id));
            if !should_follow {
                replacement.state.scroll_to(scroll_top);
            }
            *self = replacement;
            return;
        }

        // gpui anchors a list on a logical item, so splicing rows in at the
        // front keeps the rows already on screen and pushes the new ones above
        // the viewport. A browser anchors too, but suppresses it at scrollTop 0,
        // so a prepend is visible. Match the browser: remember a list pinned to
        // the top and put it back after the splice.
        //
        // While the content is shorter than the viewport gpui re-anchors to
        // item 0 every layout, so the drift only appears once the list
        // overflows. That is why `example-app` looked stuck at two rows.
        //
        // The guard is `is_following_tail()`, not `config.follow_tail`: a
        // following list that does not fill its viewport also ends layout
        // anchored at {0, 0}, and `scroll_to` would call `stop_following` on it.
        // Once the user scrolls up to the top, following is already stopped, so
        // a top-aligned `followTail` list still gets the browser behaviour.
        let top = self.state.logical_scroll_top();
        let was_pinned_to_top = matches!(config.alignment, gpui::ListAlignment::Top)
            && !self.state.is_following_tail()
            && top.item_ix == 0
            && top.offset_in_item <= gpui::px(0.0);

        // A windowed list's children are a sliding viewport. Splicing by
        // child position would treat a scroll as a rewrite of items 0..N.
        if config.item_count.is_none() && self.child_ids != child_ids {
            let prefix = self
                .child_ids
                .iter()
                .zip(&child_ids)
                .take_while(|(old, new)| old == new)
                .count();
            let suffix = self.child_ids[prefix..]
                .iter()
                .rev()
                .zip(child_ids[prefix..].iter().rev())
                .take_while(|(old, new)| old == new)
                .count();
            self.state.splice_focusable(
                prefix..self.child_ids.len().saturating_sub(suffix),
                row_focus_handles[prefix..row_focus_handles.len().saturating_sub(suffix)]
                    .iter()
                    .cloned(),
            );
            if let Some(height) = config.estimated_item_height {
                self.state = self
                    .state
                    .clone()
                    .with_uniform_item_height(gpui::px(height));
            }
        }

        for (offset, (&id, focus_handle)) in child_ids.iter().zip(&row_focus_handles).enumerate() {
            let logical = window_start + offset;
            let focusability_changed = old_rows
                .get(&id)
                .is_some_and(|(_, old_handle)| old_handle.is_some() != focus_handle.is_some());
            if focusability_changed {
                self.state
                    .splice_focusable(logical..logical + 1, std::iter::once(focus_handle.clone()));
            }
        }

        let mut changed_start = None;
        for (offset, (&id, &revision)) in child_ids.iter().zip(&child_revisions).enumerate() {
            let logical = window_start + offset;
            let changed = old_rows
                .get(&id)
                .is_some_and(|(old_revision, _)| *old_revision != revision);
            match (changed_start, changed) {
                (None, true) => changed_start = Some(logical),
                (Some(start), false) => {
                    self.state.remeasure_items(start..logical);
                    changed_start = None;
                }
                _ => {}
            }
        }
        if let Some(start) = changed_start {
            self.state
                .remeasure_items(start..window_start + child_ids.len());
        }
        self.remeasure_unknown_rows(window_start, &child_ids, &old_rows);
        if was_pinned_to_top {
            self.state.scroll_to(gpui::ListOffset::default());
        }

        self.window_start = window_start;
        self.child_ids = child_ids;
        self.child_revisions = child_revisions;
        self.row_focus_handles = row_focus_handles;
    }

    fn remeasure_unknown_rows(
        &mut self,
        window_start: usize,
        child_ids: &[u64],
        known: &HashMap<u64, (u64, Option<gpui::FocusHandle>)>,
    ) {
        let mut range_start = None;
        for (offset, id) in child_ids.iter().enumerate() {
            let logical = window_start + offset;
            let is_new = !known.contains_key(id);
            match (range_start, is_new) {
                (None, true) => range_start = Some(logical),
                (Some(start), false) => {
                    self.state.remeasure_items(start..logical);
                    range_start = None;
                }
                _ => {}
            }
        }
        if let Some(start) = range_start {
            self.state
                .remeasure_items(start..window_start + child_ids.len());
        }
    }
}

impl GpuixView {
    pub(crate) fn request_focus(&mut self, id: u64, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        self.reveal_virtual_list_ancestor(id);
        if let Some(handle) = self.focus_handles.get(&id) {
            self.pending_focus_element = None;
            self.focus.note_programmatic();
            handle.focus(window, cx);
            self.observe_focus(window);
        } else if self.focus_can_still_arrive(id) {
            self.pending_focus_element = Some(PendingFocus::new(id, window, cx));
        }
        cx.notify();
    }

    /// Whether `id` will get a focus handle on a coming frame: it is not in the
    /// tree yet, or it is focusable and has not been rendered. An element that
    /// exists and is not focusable ignores `focus()`, like the DOM.
    fn focus_can_still_arrive(&self, id: u64) -> bool {
        let tree = self.tree.lock().unwrap();
        !tree.elements.contains_key(&id) || tree.focusable_ids.contains(&id)
    }

    pub(crate) fn focused_element_id(&self, window: &gpui::Window) -> Option<u64> {
        self.focus_handles
            .iter()
            .find_map(|(id, handle)| handle.is_focused(window).then_some(*id))
    }

    fn descendant_ids(&self, ancestor: u64) -> HashSet<u64> {
        let tree = self.tree.lock().unwrap();
        let mut ids = HashSet::new();
        fn walk(tree: &RetainedTree, id: u64, ids: &mut HashSet<u64>) {
            ids.insert(id);
            let Some(element) = tree.elements.get(&id) else {
                return;
            };
            for child in &element.children {
                walk(tree, *child, ids);
            }
        }
        walk(&tree, ancestor, &mut ids);
        ids
    }

    fn focus_among(
        &self,
        ancestor: u64,
        forward: bool,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        let ids = self.descendant_ids(ancestor);
        let allowed: HashSet<gpui::FocusId> = self
            .focus_handles
            .iter()
            .filter_map(|(id, handle)| ids.contains(id).then_some(handle.id()))
            .collect();
        if forward {
            window.focus_next_among(|id| allowed.contains(id), cx);
        } else {
            window.focus_prev_among(|id| allowed.contains(id), cx);
        }
    }

    pub(crate) fn focus_next_within(
        &self,
        ancestor: u64,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        self.focus_among(ancestor, true, window, cx);
    }

    pub(crate) fn focus_previous_within(
        &self,
        ancestor: u64,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        self.focus_among(ancestor, false, window, cx);
    }

    fn on_selection_mouse_move(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.selection.lock().is_dragging() {
            self.stop_selection_scroll();
            return;
        }
        let list_id = self
            .selection_scroll_list
            .filter(|id| {
                self.virtual_lists.get(id).is_some_and(|entry| {
                    let bounds = entry.state.viewport_bounds();
                    position.x >= bounds.left() && position.x <= bounds.right()
                })
            })
            .or_else(|| {
                self.virtual_lists
                    .iter()
                    .find(|(_, entry)| entry.state.viewport_bounds().contains(&position))
                    .map(|(id, _)| *id)
            });
        let Some(list_id) = list_id else {
            self.stop_selection_scroll();
            return;
        };
        self.selection_drag_position = Some(position);
        self.selection_scroll_list = Some(list_id);
        self.schedule_selection_scroll(cx);
    }

    fn stop_selection_scroll(&mut self) {
        self.selection_drag_position = None;
        self.selection_scroll_list = None;
        self.selection_scroll_task = None;
    }

    fn schedule_selection_scroll(&mut self, cx: &mut gpui::Context<Self>) {
        if self.selection_scroll_task.is_some() || !self.selection.lock().is_dragging() {
            return;
        }
        let (Some(position), Some(list_id)) =
            (self.selection_drag_position, self.selection_scroll_list)
        else {
            return;
        };
        let Some(entry) = self.virtual_lists.get(&list_id) else {
            return;
        };
        if selection_scroll_step(entry.state.viewport_bounds(), position) == 0.0 {
            return;
        }
        self.selection_scroll_task = Some(cx.spawn(async move |view, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(SELECTION_SCROLL_TICK_MS))
                .await;
            if let Err(error) = view.update(cx, |view, cx| {
                view.selection_scroll_task = None;
                view.step_selection_scroll(cx);
            }) {
                log::debug!("selection scroll stopped after view teardown: {error}");
            }
        }));
    }

    fn step_selection_scroll(&mut self, cx: &mut gpui::Context<Self>) {
        if !self.selection.lock().is_dragging() {
            self.stop_selection_scroll();
            return;
        }
        let (Some(position), Some(list_id)) =
            (self.selection_drag_position, self.selection_scroll_list)
        else {
            return;
        };
        let Some(entry) = self.virtual_lists.get(&list_id) else {
            self.stop_selection_scroll();
            return;
        };
        let step = selection_scroll_step(entry.state.viewport_bounds(), position);
        if step == 0.0 {
            return;
        }

        let before = entry.state.logical_scroll_top();
        let selection_moved = crate::text::paint::update_drag_at(&self.selection, position);
        entry.state.scroll_by(gpui::px(step));
        let after = entry.state.logical_scroll_top();
        let list_moved =
            after.item_ix != before.item_ix || after.offset_in_item != before.offset_in_item;
        if !selection_moved && !list_moved {
            self.stop_selection_scroll();
            return;
        }
        cx.notify();
        self.schedule_selection_scroll(cx);
    }

    /// Give every DOM-focusable element a gpui focus handle in tab order, report
    /// focus changes, and drop the handles of elements that are no longer
    /// focusable.
    fn sync_focus_handles(
        &mut self,
        tree: &RetainedTree,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        // Checked before this frame's autoFocus runs: an explicit request
        // outranks autoFocus, but not a Tab or click that came after it.
        if self.pending_focus_element.is_some_and(|pending| {
            window.focused(cx).map(|handle| handle.id()) != pending.focused_at_request
        }) {
            self.pending_focus_element = None;
        }
        // Only the focusable elements are visited. This runs every frame, and
        // the rest of the tree is orders of magnitude larger.
        for &id in &tree.focusable_ids {
            let Some(element) = tree.elements.get(&id) else {
                continue;
            };
            // A text control without a `tabIndex` is a tab stop at 0.
            let (tab_index, tab_stop) = element
                .tab_index()
                .map_or((0, true), crate::focus::gpui_tab_stop);

            if let Some(handle) = self.focus_handles.get(&id) {
                handle.clone().tab_index(tab_index).tab_stop(tab_stop);
                continue;
            }
            let handle = cx.focus_handle().tab_index(tab_index).tab_stop(tab_stop);
            // Focus once, at creation. Re-focusing every frame would
            // steal focus back from whatever the user clicked next.
            if element.auto_focus {
                self.focus.note_programmatic();
                handle.focus(window, cx);
            }
            self.focus_handles.insert(id, handle);
        }

        // Keep the request until the handle exists. A frame can render before
        // the batch that creates the element arrives (Solid flushes in a
        // microtask, Windows and Linux render on their own thread).
        if let Some(handle) = self
            .pending_focus_element
            .and_then(|pending| self.focus_handles.get(&pending.id).cloned())
        {
            self.focus.note_programmatic();
            handle.focus(window, cx);
            self.pending_focus_element = None;
        }

        // An element that stopped being focusable while focused (`disabled`, or
        // its `tabIndex` removed) must give up focus; its handle is dropped below.
        // A destroyed element is not blurred: DOM removes it without an event.
        let lost_focusability = self.focus_handles.iter().any(|(id, handle)| {
            !tree.focusable_ids.contains(id)
                && tree.elements.contains_key(id)
                && handle.is_focused(window)
        });
        if lost_focusability {
            window.blur();
        }

        self.observe_focus_in(tree, window);
        self.queue_focus_events();

        // Clean up handles for elements that no longer exist.
        self.focus_handles
            .retain(|id, _| tree.focusable_ids.contains(id));
        self.routing.set_focusables(
            self.focus_handles
                .iter()
                .map(|(id, handle)| (*id, handle.downgrade())),
        );
    }
}

/// A `focusElement` request for an element whose focus handle does not exist
/// yet. It is dropped when focus moves any other way first (Tab, a click, a
/// newer request), so a late element never steals focus from the user.
#[derive(Clone, Copy)]
struct PendingFocus {
    id: u64,
    focused_at_request: Option<gpui::FocusId>,
}

impl PendingFocus {
    fn new(id: u64, window: &gpui::Window, cx: &gpui::App) -> Self {
        Self {
            id,
            focused_at_request: window.focused(cx).map(|handle| handle.id()),
        }
    }
}

impl GpuixView {
    pub(crate) fn activation(&self) -> crate::events::activation::Activation {
        crate::events::activation::Activation {
            tree: self.tree.clone(),
            callback: self.event_callback.clone(),
            routing: self.routing.clone(),
        }
    }
}

impl gpui::Render for GpuixView {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui::IntoElement;

        window.set_window_title(&self.window_title);
        self.apply_pointer_lock(window, cx);

        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        if let Some(id) = PENDING_FOCUS_ELEMENT.with(|pending| pending.borrow_mut().take()) {
            self.pending_focus_element = Some(PendingFocus::new(id, window, cx));
        }

        // Clone Arc so we don't borrow self.tree — frees self for focus_handles access.
        let tree_arc = self.tree.clone();
        let tree = tree_arc.lock().unwrap();
        let callback = self.event_callback.clone();

        // Sync focus handles before building elements.
        self.sync_focus_handles(&tree, window, cx);

        // Ensure custom element instances are destroyed when their IDs disappear.
        self.custom_registry
            .prune_missing(|id| tree.elements.contains_key(&id), window);

        // Clean up scroll handles for destroyed elements (IDs removed from tree).
        // Scrollability-based cleanup (element still exists but style changed
        // from scroll to non-scroll) is handled inside build_host_container().
        self.scroll_handles
            .retain(|id, _| tree.elements.contains_key(id));
        self.virtual_lists
            .retain(|id, _| tree.elements.contains_key(id));
        // Pruned by listener, not existence: dropping `onLayout` and adding it
        // back reports the size again, like a fresh observer.
        {
            let mut reports = self.paint_reports.borrow_mut();
            if !reports.sizes.is_empty() {
                reports.sizes.retain(|id, _| {
                    tree.elements
                        .get(id)
                        .is_some_and(|element| element.events().contains("layout"))
                });
            }
            if !reports.scrolls.is_empty() {
                reports.scrolls.retain(|id, _| {
                    tree.elements
                        .get(id)
                        .is_some_and(|element| element.events().contains("scroll"))
                });
            }
            crate::observers::prune(
                &mut reports.observers,
                |id| tree.elements.contains_key(&id),
                |id, event| {
                    tree.elements
                        .get(&id)
                        .is_some_and(|element| element.events().contains(event))
                },
            );
        }
        // Build the element tree. custom_registry, focus_handles, and scroll_handles
        // are different fields of self, so Rust allows borrowing all simultaneously.
        let theme = Theme::dark();
        let now = self.clock.now();
        crate::scroll::begin_frame();
        crate::scrollbar::begin_frame(
            now,
            self.scroller_style
                .unwrap_or_else(|| crate::scrollbar::ScrollerStyle::of_system(cx)),
        );
        let scroll_animating = crate::scroll::advance_animations(now);
        // Before the motion sync reads them, so an element that stopped being
        // measured does not seed its motion from a size laid out long ago.
        self.measurements
            .prune(|id| tree.elements.contains_key(&id));
        let (motion_active, motion_settled) =
            sync_motion_states(
                &tree,
                &mut self.motion_states,
                &self.measurements,
                window.appearance(),
                window.viewport_size(),
                now,
            );
        self.motion_values
            .set_appearance(color_scheme_of(window.appearance()));
        let motion_value_frame = sync_motion_values(&tree, &mut self.motion_values, now);
        self.css_transitions
            .retain(|id| tree.elements.contains_key(&id));
        self.css_animations
            .retain(|id| tree.elements.contains_key(&id));
        self.svg_trees.retain(|id| tree.elements.contains_key(&id));
        self.layout_flips.retain(|id| {
            tree.elements
                .get(&id)
                .is_some_and(|element| element.custom_props().contains_key("layout"))
        });
        // Pruned by DECLARATION, not existence: an element that drops its
        // `highlight` prop keeps living, and its cached group list holds a copy
        // of every string in its subtree.
        self.highlights.retain(|id, _| {
            tree.elements
                .get(id)
                .is_some_and(|element| element.custom_props().contains_key("highlight"))
        });
        let mut highlight_events = Vec::new();
        let focus_styles = self.focus.styles(&tree, tree.keyboard_focus_dim());
        crate::accessibility::links::sync(&tree);
        let pointer_state = self.pointer.clone();
        let pointer = pointer_state.borrow();
        let activation = self.activation();
        crate::layout::box_generation::hold_display(self.css_transitions.held_displays());
        let result = match tree.root_id {
            Some(root_id) => {
                let mut ctx = BuildCtx {
                    tree: &tree,
                    event_callback: &callback,
                    activation: &activation,
                    focus_handles: &self.focus_handles,
                    scroll_handles: &mut self.scroll_handles,
                    custom_registry: &mut self.custom_registry,
                    virtual_lists: &mut self.virtual_lists,
                    motion_states: &mut self.motion_states,
                    motion_values: &self.motion_values,
                    css_transitions: &mut self.css_transitions,
                    css_animations: &mut self.css_animations,
                    keyframes: &self.keyframes,
                    measurements: &self.measurements,
                    layout_flips: &self.layout_flips,
                    svg_trees: &mut self.svg_trees,
                    now,
                    selection: self.selection.clone(),
                    inherited: Inherited::root(
                        &theme,
                        window.appearance(),
                        self.frameless,
                        window.viewport_size(),
                    ),
                    highlights: &mut self.highlights,
                    paint_reports: &self.paint_reports,
                    highlight_events: &mut highlight_events,
                    focus_styles,
                    pointer: &pointer,
                    contents_overlay: None,
                    anchor_scope: None,
                    stacking_root: true,
                };
                build_element(root_id, &mut ctx, window, cx)
            }
            None => gpui::Empty.into_any_element(),
        };
        drop(pointer);
        self.custom_registry
            .load_hidden_images(&tree, &callback, window, cx);
        // Flushed after the root build so a `setState` in the handler cannot
        // re-enter this build.
        emit_highlight_events(&callback, &highlight_events);
        emit_motion_settled(&callback, &tree, &motion_settled);
        emit_motion_value_events(&callback, &motion_value_frame.events);
        emit_transition_events(&callback, &tree, self.css_transitions.take_events());
        emit_animation_events(&callback, &tree, self.css_animations.take_events());
        emit_layout_events(&callback, &tree, self.layout_flips.take_events());
        let transitions_active =
            self.css_transitions.take_active() | self.css_animations.take_active();
        self.emit_selection_change(&callback);

        // The frame reset must paint BEFORE any text, so it is the first child of
        // the root wrapper. Without it the selection registry accumulates stale
        // entries across frames and a drag resolves against elements that are no
        // longer on screen.
        let result = {
            use gpui::prelude::*;
            let drag_move_view = cx.weak_entity();
            let drag_end_view = drag_move_view.clone();
            let mut root = gpui::div()
                .size_full()
                .on_painted(self.measurements.frame_counter());
            root.text_style().tab_size = Some(crate::css::tab_size::DEFAULT);
            crate::css::word_break::set_defaults(root.text_style());
            root.text_style().hang_trailing_spaces =
                Some(crate::text::white_space::WhiteSpace::default().hangs_trailing_spaces());
            with_window_menu_actions(root)
                .child(crate::events::listeners::pointer_frame(
                    cx.weak_entity(),
                    crate::events::listeners::PointerDispatch {
                        pointer: self.pointer.clone(),
                        activation: activation.clone(),
                        window_actions: self.simulated_window_actions.clone(),
                    },
                    self.pointer.borrow().observed(),
                ))
                .child(crate::events::keyboard::window_input_listeners(
                    tree_arc.clone(),
                    callback.clone(),
                    self.routing.clone(),
                ))
                .child(selection_frame_reset(
                    self.selection.clone(),
                    move |position, app| {
                        // This listener can run inside an update of GpuixView (an
                        // accessibility activation does). A nested update panics, so wait.
                        let drag_move_view = drag_move_view.clone();
                        app.defer(move |app| {
                            drag_move_view
                                .update(app, |view, cx| view.on_selection_mouse_move(position, cx))
                                .ok();
                        });
                    },
                    move |app| {
                        let drag_end_view = drag_end_view.clone();
                        app.defer(move |app| {
                            drag_end_view
                                .update(app, |view, _cx| view.stop_selection_scroll())
                                .ok();
                        });
                    },
                ))
                .child(crate::automation::bounds_frame_reset())
                .child(crate::observers::frame_sweep(
                    self.paint_reports.clone(),
                    callback.clone(),
                ))
                .child(result)
                .into_any_element()
        };

        // Sync scroll handles to thread_local so napi methods (scrollTo,
        // getScrollOffset) can access them without an App context.
        SCROLL_HANDLES.with(|cell| {
            let mut handles = cell.borrow_mut();
            handles.clear();
            for (&id, handle) in &self.scroll_handles {
                handles.insert(id, handle.clone());
            }
        });
        VIRTUAL_LIST_STATES.with(|cell| {
            let mut states = cell.borrow_mut();
            states.clear();
            for (&id, entry) in &self.virtual_lists {
                states.insert(id, entry.state.clone());
            }
        });
        // One-shot: a queued scroll for a list that did not build this frame
        // would otherwise fire on some later frame, against child indices that
        // no longer match what JS meant.
        PENDING_VIRTUAL_LIST_SCROLLS.with(|cell| cell.borrow_mut().clear());

        if motion_active || transitions_active || motion_value_frame.active || scroll_animating {
            window.request_animation_frame();
        }

        result
    }
}

// ── Element builders ─────────────────────────────────────────────────

static EMPTY_STYLE: std::sync::LazyLock<StyleDesc> = std::sync::LazyLock::new(StyleDesc::default);

pub(crate) fn build_element(
    id: u64,
    ctx: &mut BuildCtx,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<GpuixView>,
) -> gpui::AnyElement {
    use gpui::IntoElement;

    // Copy the tree reference out of `ctx`, so `element` borrows the tree and
    // not the build context, which the children below need mutably.
    let tree = ctx.tree;
    let stacking_root = std::mem::take(&mut ctx.stacking_root);
    let Some(element) = tree.elements.get(&id) else {
        return gpui::Empty.into_any_element();
    };
    if crate::layout::box_generation::is_display_none(element) {
        ctx.css_transitions.hide(tree, id, ctx.now);
        ctx.css_animations.hide(tree, id, ctx.now);
        return gpui::Empty.into_any_element();
    }
    ctx.css_transitions.show(id);
    ctx.css_animations.show(id);

    // `hover`, `active`, `groupHover`, `groupActive` and `groupFocusWithin` are
    // folded into the base here, before motion, so the GPUI element never sees them: the dispatcher
    // decides who is hovered.
    let pointer_states = element
        .style
        .as_deref()
        .map(|style| ctx.pointer_states(id, style, element.has_inert_prop()))
        .unwrap_or_default();
    let state_style = element.style.as_deref().and_then(|style| {
        pointer_states
            .any()
            .then(|| crate::events::state_styles::resolve_pointer_states(style, pointer_states))
    });
    let base_style = state_style.as_ref().or(element.style.as_deref());

    // State styles and CSS transitions come before motion: an `animate` value
    // is an inline style and beats a transition. CSS animations come last of
    // all, as animations beat every normal declaration in CSS, except on a
    // property a transition is running on: a transition beats an animation.
    let props = element.custom_props();
    let has_motion = props.contains_key("motion");
    let motion_binding = props.get("motionValues");
    let layout_source = props.get("layout");
    let transitions_wanted = ctx.css_transitions.wants(id, element.style.as_deref());
    let animations_wanted = ctx.css_animations.wants(id, element.style.as_deref());
    // A plain element has no measurement to read: it is not wrapped below, and
    // the root render prunes the measurement of any element that was not.
    let measured = has_motion || transitions_wanted || animations_wanted;
    let measurement = measured.then(|| ctx.measurements.get(id)).flatten();
    let transitioned = (transitions_wanted || animations_wanted).then(|| {
        let focus_visible = ctx
            .focus_handles
            .get(&id)
            .is_some_and(|handle| handle.is_focused(window))
            && window.last_input_was_keyboard();
        crate::motion::resolve_declared_style(
            transitions_wanted.then_some(&mut *ctx.css_transitions),
            element.style_before_transition(),
            &ctx.inherited.colors,
            &ctx.inherited.parent_font(),
            pointer_states,
            ctx.motion_states.get(&id),
            id,
            element.style.as_deref().unwrap_or(&EMPTY_STYLE),
            focus_visible,
            measurement.as_ref(),
            ctx.inherited.interpolate_size,
            ctx.now,
        )
    });
    // A `display` transition with `allow-discrete` kept the box until now.
    if crate::layout::box_generation::declares_display_none(element)
        && transitioned
            .as_ref()
            .is_none_or(|declared| declared.style.display.as_deref() == Some("none"))
    {
        ctx.css_transitions.hide(tree, id, ctx.now);
        ctx.css_animations.hide(tree, id, ctx.now);
        return gpui::Empty.into_any_element();
    }
    let declared_style = transitioned
        .as_ref()
        .map(|declared| &declared.style)
        .or(base_style);
    let animated_style = if has_motion {
        ctx.motion_states
            .get(&id)
            .and_then(|state| state.visible_style(ctx.now))
            .map(|style| {
                // `Arc<StyleDesc>` is shared, so the animated frame is applied to a
                // copy. Mutating through the pointer would restyle every element
                // that declared the same style.
                let mut resolved = declared_style.cloned().unwrap_or_default();
                style.apply_to(&mut resolved);
                resolved
            })
    } else {
        None
    };
    // Motion values are inline style as well, and come after `motion`.
    let bound_style = motion_binding.map(|binding| {
        let mut resolved = animated_style
            .as_ref()
            .or(declared_style)
            .cloned()
            .unwrap_or_default();
        ctx.motion_values.apply(binding, &mut resolved);
        resolved
    });
    let underlying_style = bound_style
        .as_ref()
        .or(animated_style.as_ref())
        .or(declared_style);
    let css_animated = animations_wanted
        .then(|| {
            let declared = declared_style.unwrap_or(&EMPTY_STYLE);
            ctx.css_animations
                .update(
                    id,
                    declared,
                    underlying_style.unwrap_or(&EMPTY_STYLE),
                    &ctx.inherited.colors,
                    &ctx.inherited.parent_font(),
                    ctx.keyframes,
                    measurement.as_ref(),
                    ctx.inherited.interpolate_size,
                    ctx.now,
                )
                .map(|values| {
                    let mut resolved = underlying_style.cloned().unwrap_or_default();
                    values.apply_to(
                        &mut resolved,
                        transitioned
                            .as_ref()
                            .and_then(|declared| declared.transitions.as_ref()),
                    );
                    resolved
                })
        })
        .flatten();
    let current_style = css_animated.as_ref().or(underlying_style);
    let overlaid_style = ctx.contents_overlay.take().map(|overlay| {
        current_style
            .cloned()
            .unwrap_or_default()
            .with_inherited_overlay(&overlay)
    });
    let style = overlaid_style.as_ref().or(current_style);
    let anchored_style = ctx.anchor_scope.take().and_then(|anchored_id| {
        style.and_then(|style| crate::layout::anchor_size::resolve(anchored_id, style))
    });
    let style = anchored_style.as_ref().or(style);
    let natural_width_style = (measured
        && crate::motion::wants_natural_width(
            ctx.motion_states.get(&id),
            declared_style,
            ctx.inherited.interpolate_size,
            ctx.keyframes,
            ctx.now,
        ))
    .then(|| StyleDesc {
        measure_natural_width: true,
        ..style.cloned().unwrap_or_default()
    });
    let style = natural_width_style.as_ref().or(style);

    // Inheritable style resolves once here so both built-ins and custom
    // elements see the same cascade.
    let parent_inherited = ctx.inherited.clone();
    ctx.inherited = parent_inherited
        .clone()
        .descend(style, element.has_inert_prop());
    // After the fold above: a group is its descendants' to follow, so the
    // element's own `groupHover` reads the groups outside it.
    if let Some(name) = style.and_then(StyleDesc::declared_group) {
        ctx.inherited.groups = Some(Arc::new(GroupScope {
            name,
            state: GroupState {
                hovered: ctx.pointer.is_hovered(id),
                active: ctx.pointer.is_active(id),
                focused_within: ctx.focus_styles.within.contains(&id),
            },
            parent: ctx.inherited.groups.take(),
        }));
    }

    // A `highlight` here replaces any ancestor's: the nearest declaration wins,
    // and `GroupList::collect` skips nested declarations so an ancestor never
    // resolves or counts matches that will not paint.
    if let Some(value) = element.custom_props().get("highlight") {
        let has_listener = element.events().contains("highlight");
        let resolved = resolve_highlight(
            ctx.highlights,
            ctx.tree,
            id,
            value,
            &Theme::dark(),
            has_listener,
        );
        if let Some((_, Some(total))) = &resolved {
            ctx.highlight_events.push((id, *total));
        }
        ctx.inherited.highlight = resolved.map(|(context, _)| context);
    }

    let built = match element.element_type.as_str() {
        // `<text>` is a `<div>` that happens to carry a string. Giving it its
        // own builder meant every interaction prop on the shared `Props` type
        // (onClick, hover, focus, tabIndex) type-checked, registered a JS
        // listener, and then silently did nothing. `<span>` is the same box;
        // it only differs in being inline inside a paragraph.
        host if crate::text::inline::is_host_type(host) => {
            ctx.custom_registry.destroy(id);
            if crate::layout::contents::is_display_contents(element) {
                build_contents_container(element, style, stacking_root, ctx, window, cx)
            } else {
                build_host_container(element, style, stacking_root, ctx, window, cx)
            }
        }
        "virtual-list" => {
            ctx.custom_registry.destroy(id);
            build_virtual_list(element, ctx, window, cx)
        }

        // Polymorphic dispatch for all custom elements.
        custom_type => {
            // An `<svg>` paints its shape children as one image, so they are
            // serialised instead of built.
            let is_svg = custom_type == "svg";
            let svg_markup = is_svg
                .then(|| ctx.svg_trees
                    .markup(ctx.tree, id, ctx.motion_states, &ctx.inherited.colors, ctx.now))
                .flatten();
            let sized_style = is_svg
                .then(|| crate::custom_elements::svg_tree::sized_style(style, element.custom_props()))
                .flatten();
            let style = sized_style.as_ref().or(style);
            if custom_type == "anchored" {
                crate::layout::anchor_size::begin_build(tree, element);
            }
            let child_ids = crate::layout::contents::flatten(ctx.tree, &element.children);
            let custom_children: Vec<gpui::AnyElement> = if is_svg {
                Vec::new()
            } else {
                child_ids
                    .iter()
                    .copied()
                    .filter(|child_id| {
                        crate::layout::box_generation::generates_box(ctx.tree, *child_id)
                    })
                    .map(|child_id| {
                        ctx.contents_overlay =
                            crate::layout::contents::inherited_overlay(ctx.tree, element, child_id);
                        if custom_type == "anchored" {
                            ctx.anchor_scope = Some(id);
                        }
                        build_element(child_id, ctx, window, cx)
                    })
                    .collect()
            };
            let inherited = ctx.inherited.clone();
            let render_ctx = CustomRenderContext {
                id,
                events: element.events(),
                event_callback: ctx.event_callback,
                focus_handle: ctx.focus_handles.get(&id),
                focus_styles: ctx.focus_styles.clone(),
                click_path: click_path(ctx.tree, element),
                activation: ctx.activation,
                style,
                colors: inherited.colors,
                lengths: inherited.lengths,
                font_features: inherited.font_features.clone(),
                children: custom_children,
                svg_markup,
                selection: ctx.selection.clone(),
                selectable: inherited.selectable,
                inert: inherited.inert,
                selection_wash: inherited.selection_wash,
                highlight_set: inherited.highlight.clone(),
                props: element.custom_props(),
                pointer_events_none: inherited.pointer_events_none,
                app_region: inherited.app_region,
                stacking_root,
                // Built from every listener: the registry hands the element only
                // the events it declares support for.
                observe: crate::observers::reporter(
                    id,
                    element.events(),
                    ctx.tree,
                    None,
                    ctx.paint_reports,
                    ctx.event_callback,
                ),
            };
            ctx.custom_registry
                .render(custom_type, element.custom_props(), render_ctx, window, cx)
        }
    };

    ctx.inherited = parent_inherited;
    // Motion and CSS transitions read layout back one frame later, so only the
    // elements they drive pay for the measurement. It wraps every kind of
    // element here, because the custom ones and `virtual-list` build their
    // GPUI element out of reach of `build_host_container`.
    let built = if measured {
        ctx.measurements.wrap(
            id,
            crate::motion::measure::Containment {
                percent_height_resolves: ctx.inherited.container.resolves_percent_height(style),
                flex_main: ctx.inherited.container.flex_main(),
            },
            built,
        )
    } else {
        built
    };
    match layout_source {
        Some(source) => ctx.layout_flips.wrap(id, source, built, ctx.now, ctx.motion_values),
        None => built,
    }
}

fn build_virtual_list(
    element: &crate::retained_tree::RetainedElement,
    ctx: &mut BuildCtx,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<GpuixView>,
) -> gpui::AnyElement {
    use gpui::prelude::*;

    let child_ids: Vec<u64> = element
        .children
        .iter()
        .copied()
        .filter(|child_id| ctx.tree.elements.contains_key(child_id))
        .collect();
    let child_revisions: Vec<u64> = child_ids
        .iter()
        .filter_map(|child_id| {
            ctx.tree
                .elements
                .get(child_id)
                .map(|child| child.subtree_revision)
        })
        .collect();
    let focusable_rows: HashSet<u64> = ctx
        .focus_handles
        .keys()
        .filter_map(|element_id| virtual_row_ancestor(ctx.tree, element.id, *element_id))
        .collect();
    let focused_row = ctx
        .focus_handles
        .iter()
        .find_map(|(element_id, handle)| {
            handle
                .is_focused(window)
                .then(|| virtual_row_ancestor(ctx.tree, element.id, *element_id))
                .flatten()
        })
        .or_else(|| {
            ctx.focus_handles.keys().find_map(|element_id| {
                ctx.tree
                    .elements
                    .get(element_id)
                    .is_some_and(|element| element.auto_focus)
                    .then(|| virtual_row_ancestor(ctx.tree, element.id, *element_id))
                    .flatten()
            })
        });
    let config = VirtualListConfig::from_element(element);
    let window_start = if config.item_count.is_some() {
        window_start_from_element(element)
    } else {
        0
    };
    let list_state = match ctx.virtual_lists.entry(element.id) {
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            entry.get_mut().sync(
                config,
                window_start,
                child_ids.clone(),
                child_revisions,
                &focusable_rows,
                cx,
            );
            let entry = entry.into_mut();
            if let Some(row_id) = focused_row.filter(|row_id| !entry.seen_rows.contains(row_id)) {
                if let Some(index) = entry.logical_index_of(row_id) {
                    entry.state.scroll_to(gpui::ListOffset {
                        item_ix: index,
                        offset_in_item: gpui::px(0.0),
                    });
                }
            }
            entry.state.clone()
        }
        std::collections::hash_map::Entry::Vacant(entry) => {
            let row_focus_handles = child_ids
                .iter()
                .map(|id| focusable_rows.contains(id).then(|| cx.focus_handle()))
                .collect();
            let entry = entry.insert(VirtualListEntry::new(
                config,
                window_start,
                child_ids.clone(),
                child_revisions,
                row_focus_handles,
            ));
            if let Some(row_id) = focused_row {
                if let Some(index) = entry.logical_index_of(row_id) {
                    entry.state.scroll_to(gpui::ListOffset {
                        item_ix: index,
                        offset_in_item: gpui::px(0.0),
                    });
                }
            }
            entry.state.clone()
        }
    };

    // Queued scrolls apply here, after `sync` spliced this frame's child
    // changes, so the indices JS computed against its committed child list are
    // the indices the splice-adjusted ListState sees.
    if let Some(offset) =
        PENDING_VIRTUAL_LIST_SCROLLS.with(|cell| cell.borrow_mut().remove(&element.id))
    {
        list_state.scroll_to(offset);
    }

    if element.events().contains("visibleRange") {
        let callback = ctx.event_callback.clone();
        let list_id = element.id;
        list_state.set_scroll_handler(move |event, _window, _cx| {
            emit_event_full(&callback, list_id, "visibleRange", |payload| {
                payload.start_index = Some(event.visible_range.start as f64);
                payload.end_index = Some(event.visible_range.end as f64);
            });
        });
    }

    let list_id = element.id;
    // Cloned, not copied: gpui runs this processor once per requested row, so
    // the captured value must survive every call.
    let inherited = ctx.inherited.clone();
    let render_item = cx.processor(move |view, index: usize, window, cx| {
        let Some(entry) = view.virtual_lists.get(&list_id) else {
            return unmounted_virtual_row(1.0);
        };
        let Some(child_id) = entry.child_at(index) else {
            // Empty measures as 0 and poisons ListState. Keep the estimate.
            return unmounted_virtual_row(entry.config.estimated_item_height.unwrap_or(1.0));
        };
        view.build_virtual_child(list_id, index, child_id, inherited.clone(), window, cx)
    });
    let scroll_state = list_state.clone();
    // The list inserts its own hitbox under this id, so a host id makes it a
    // target. Under `pointer-events: none` it stays a GPUI detail: the list's
    // hitbox cannot pass the pointer through.
    let list_element_id = if ctx.inherited.pointer_events_none {
        gpui::ElementId::Name(gpui::SharedString::from(format!(
            "__gpuix_virtual_list_{}",
            element.id
        )))
    } else {
        crate::events::ids::host_element_id(element.id)
    };
    let mut list = gpui::list(list_state, render_item)
        .with_sizing_behavior(gpui::ListSizingBehavior::Auto)
        .id(list_element_id);
    if let Some(style) = element.style.as_deref() {
        list = apply_styles(list, style, &ctx.inherited.colors, &ctx.inherited.lengths).overscroll_behavior(
            gpui::OverscrollBehavior::Auto,
            crate::scroll::overscroll_y(style),
        );
        if let Some(features) = ctx.inherited.font_features.clone() {
            list = list.font_features(features);
        }
    }
    let list = apply_accessibility(list, element.id, element.custom_props(), None).into_any_element();
    let look = element
        .style
        .as_deref()
        .map(|style| crate::scrollbar::Look::of(style, &ctx.inherited.colors))
        .unwrap_or_else(|| {
            crate::scrollbar::Look::of(&StyleDesc::default(), &ctx.inherited.colors)
        });
    if look.hidden() {
        return list;
    }
    ListWithScrollbar {
        list,
        source: crate::scrollbar::Source::List(scroll_state),
        look,
        state: crate::scrollbar::state_for(element.id),
    }
    .into_any_element()
}

fn unmounted_virtual_row(height: f32) -> gpui::AnyElement {
    use gpui::prelude::*;
    gpui::div().h(gpui::px(height.max(1.0))).w_full().into_any()
}

fn virtual_row_ancestor(tree: &RetainedTree, list_id: u64, element_id: u64) -> Option<u64> {
    let mut current = element_id;
    loop {
        let parent = tree.elements.get(&current)?.parent?;
        if parent == list_id {
            return Some(current);
        }
        current = parent;
    }
}

/// A text instance: a bare string with no style, children or listeners.
fn is_text_node(element: &crate::retained_tree::RetainedElement) -> bool {
    element.element_type == "text"
        && element.content.is_some()
        && element.style.is_none()
        && element.children.is_empty()
        && element.events().is_empty()
}

/// The one builder for `<div>`, `<text>` and `<span>`.
///
/// Both get the same stable GPUI id, so gpui keeps their interactive element
/// state (hover, active, pointer capture, scroll, accessibility node) across
/// frames, and both wire the whole shared `Props` surface.
pub(crate) fn build_host_container(
    element: &crate::retained_tree::RetainedElement,
    style: Option<&StyleDesc>,
    stacking_root: bool,
    ctx: &mut BuildCtx,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<GpuixView>,
) -> gpui::AnyElement {
    use gpui::prelude::*;

    // Every host element has a hitbox under an id that is its retained id and
    // nothing else, so the dispatcher can map GPUI's hit chain back to elements.
    // Text instances are the exception: the DOM has no box for a text node, so the
    // pointer is over the element holding it.
    let mut el = gpui::div().id(crate::events::ids::host_element_id(element.id));
    if !is_text_node(element) {
        el = crate::events::ids::host_hitbox(el, ctx.inherited.pointer_events_none);
        el = crate::events::app_region::mark(el, ctx.inherited.app_region);
    }

    // An inert subtree takes no pointer input at all: no listener, no hover
    // or active style (both need a hitbox), no occluding hitbox.
    let inert = ctx.inherited.inert;
    let inert_style;
    let style = match style {
        Some(style) if inert => {
            inert_style = style.without_pointer_states();
            Some(&inert_style)
        }
        style => style,
    };

    if let Some(style) = style {
        el = apply_interactive_styles(el, style, &ctx.inherited.colors, &ctx.inherited.lengths);

        if let Some(features) = ctx.inherited.font_features.clone() {
            el = el.font_features(features);
        }
    }

    el = crate::scroll::apply_container(el, element.id, style, &ctx.inherited.colors, ctx.scroll_handles);

    // If a FocusHandle was pre-created for this element (by sync_focus_handles),
    // attach it via track_focus. This makes the element focusable — clicking it
    // or tabbing to it gives it keyboard focus. The handle persists across renders
    // because it's stored in GpuixView::focus_handles.
    if style.and_then(|style| style.position.as_deref()).is_none() {
        el.style().position = Some(gpui::Position::Static);
    }
    el = crate::layout::stacking::apply_context(
        el,
        style,
        stacking_root,
        &ctx.inherited.colors,
        &ctx.inherited.lengths,
    );
    // The inert root claims its box as a non-selectable start region; below it,
    // a descendant's `userSelect: "text"` must not reopen one.
    let selection_start = if element.has_inert_prop() {
        Some(false)
    } else if inert {
        selection_start_flag(style).map(|_| false)
    } else {
        selection_start_flag(style)
    };
    el = crate::automation::track_host_bounds(
        el,
        element.id,
        selection_start,
        paint_reporter(element.id, element.events(), None, ctx),
    );
    if let Some(handle) = ctx.focus_handles.get(&element.id) {
        el = el.track_focus(handle);
        el = crate::focus::apply_focus_styles(
            el,
            element.id,
            style,
            &ctx.inherited.colors,
            &ctx.inherited.lengths,
            &ctx.focus_styles,
        );
    } else {
        el = crate::focus::apply_focus_within(
            el,
            element.id,
            style,
            &ctx.inherited.colors,
            &ctx.inherited.lengths,
            &ctx.focus_styles,
        );
    }

    // React text instances (`createTextInstance`) are also type `text` and
    // hold `content`. Only the host `<text>` (no content of its own) gets
    // Label. The inner nodes stay out of the AX tree so VoiceOver does not
    // hear the same string twice.
    let is_text_host = element.element_type == "text" && element.content.is_none();
    let is_label = element.element_type == "label";
    let default_role = if element.element_type == "form" {
        Some(gpui::Role::Form)
    } else {
        (is_text_host || is_label).then_some(gpui::Role::Label)
    };
    el = apply_accessibility(el, element.id, element.custom_props(), default_role);
    // Planned once: the label below and the children further down read the same
    // paragraphs. A node with no children paints its own `content` instead.
    let plan = if element.children.is_empty() {
        Vec::new()
    } else {
        crate::text::inline::plan_children(ctx.tree, element)
    };
    if is_text_host && element.custom_props().get("aria-valuetext").is_none() {
        if let Some(content) = crate::text::inline::accessible_text(&plan) {
            el = el.aria_value(content);
        }
    }
    if is_label && !inert {
        el = crate::accessibility::label::describe(el, element.id);
    }

    // Pointer, mouse, click and wheel events are not wired here: the window-level
    // dispatcher (`crate::events`) finds the target and runs them along the
    // retained tree. What is left are the events GPUI still owns.
    if !inert {
        el = apply_a11y_click(el, click_path(ctx.tree, element), ctx.activation);
    }

    // Key and focus events are not wired per element either: one event per keystroke
    // goes to the focused element and bubbles along its path in JS (`events::keyboard`),
    // and focus events come from `GpuixView::observe_focus`. The wheel is a window-level
    // input like the pointer's (`events::listeners`).

    let (content, paint_ranks) = build_content(element, plan, ctx, window, cx);
    el.extend(content);
    if paint_ranks.iter().any(|rank| *rank != 0) {
        el = el.paint_ranks(paint_ranks);
    }

    el.into_any_element()
}

/// What a host element holds, in paint order, with the paint rank of each.
fn build_content(
    element: &crate::retained_tree::RetainedElement,
    plan: Vec<crate::text::inline::Child>,
    ctx: &mut BuildCtx,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<GpuixView>,
) -> (Vec<gpui::AnyElement>, Vec<i32>) {
    // Text content — selectable, same as a <text> leaf.
    // An empty string paints no glyphs, so it must take no space: gpui's text
    // layout still reserves a full line for "", and Solid's universal renderer
    // creates an empty placeholder text node for every dynamic hole (`<Show>`,
    // `{cond && x}`). Emitting those as full-height lines inflated every
    // conditional row. React never sends empty text, so this is a no-op there.
    let mut content_elements = Vec::new();
    let mut paint_ranks = Vec::new();
    if element.children.is_empty() {
        if let Some(content) = crate::text::search::leaf_text(ctx.tree, element) {
            content_elements.push(text_content(element, &content, ctx));
            paint_ranks.push(0);
        }
    }

    // Children. Consecutive inline children are one paragraph, see
    // `text::inline`; the rest are elements of their own.
    if !element.children.is_empty() {
        for child in plan {
            match child {
                crate::text::inline::Child::Block(child_id) => {
                    paint_ranks.push(crate::layout::stacking::paint_rank(
                        ctx.tree
                            .elements
                            .get(&child_id)
                            .and_then(|child| child.style.as_deref()),
                    ));
                    ctx.contents_overlay =
                        crate::layout::contents::inherited_overlay(ctx.tree, element, child_id);
                    content_elements.push(build_element(child_id, ctx, window, cx));
                }
                crate::text::inline::Child::Paragraph(paragraph) => {
                    paint_ranks.push(0);
                    content_elements.push(paragraph_content(&paragraph, ctx));
                }
            }
        }
    }
    (content_elements, paint_ranks)
}

/// A `display: contents` element that has no parent to splice into: the root, or
/// a virtual-list row. It generates no box, so it has no id, hitbox or bounds.
/// Its children lay out in an anonymous block in its place, and keep the text
/// style it passes down.
fn build_contents_container(
    element: &crate::retained_tree::RetainedElement,
    style: Option<&StyleDesc>,
    stacking_root: bool,
    ctx: &mut BuildCtx,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<GpuixView>,
) -> gpui::AnyElement {
    use gpui::prelude::*;

    let mut el = gpui::div();
    el.style().position = Some(gpui::Position::Static);
    if stacking_root {
        el = el.stacking_context();
    }
    if let Some(style) = style {
        el = apply_styles(el, &style.inheritable_part(), &ctx.inherited.colors, &ctx.inherited.lengths);
    }
    if let Some(features) = ctx.inherited.font_features.clone() {
        el = el.font_features(features);
    }
    let plan = if element.children.is_empty() {
        Vec::new()
    } else {
        crate::text::inline::plan_children(ctx.tree, element)
    };
    let (content, paint_ranks) = build_content(element, plan, ctx, window, cx);
    el.extend(content);
    if paint_ranks.iter().any(|rank| *rank != 0) {
        el = el.paint_ranks(paint_ranks);
    }
    el.into_any_element()
}

/// A virtual list with its scrollbar.
///
/// A list element takes no children, and wrapping it in a box would change the layout its style
/// asks for. This lays out, prepaints and paints as the list does (it answers with the list's
/// own layout node), and puts the bar over it from what its `ListState` has measured: the same
/// totals the list scrolls by, so the thumb and the scroll position cannot disagree.
struct ListWithScrollbar {
    list: gpui::AnyElement,
    source: crate::scrollbar::Source,
    look: crate::scrollbar::Look,
    state: std::rc::Rc<std::cell::RefCell<crate::scrollbar::State>>,
}

impl gpui::IntoElement for ListWithScrollbar {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl gpui::Element for ListWithScrollbar {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> (gpui::LayoutId, ()) {
        (self.list.request_layout(window, cx), ())
    }

    // The scrollbar is placed by this wrapper's bounds, which must be the list's.
    fn escapes_to(&self) -> Option<gpui::Escape> {
        self.list.escapes_to()
    }

    // The stacking context takes the wrapper, not the list, so the wrapper must say the list's z.
    fn lift_z(&self) -> Option<i32> {
        self.list.lift_z()
    }

    fn prepaint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        _: &mut (),
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        self.list.prepaint(window, cx);
        crate::scrollbar::prepaint(&self.state, &self.source, self.look, bounds, window);
    }

    fn paint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        self.list.paint(window, cx);
        crate::scrollbar::paint(&self.state, &self.source, self.look, bounds, window, cx);
    }
}

/// A selectable text run owned by `element`. Runs are left to gpui so the
/// text keeps inheriting colour, weight and family from ancestor styles.
///
/// The run's group is its parent host element, because React makes a separate
/// host node for every interpolated string. `<text>Hello {name}!</text>` is one
/// logical line painted as three runs that all share the parent's id.
/// A `userSelect: "none"` run still paints highlight washes, because a browser
/// still finds that text with Ctrl+F. Element chrome that must never be found,
/// such as a code gutter, uses `chrome_text` instead.
fn text_content(
    element: &crate::retained_tree::RetainedElement,
    content: &str,
    ctx: &BuildCtx,
) -> gpui::AnyElement {
    selectable_text(crate::text::SelectableText {
        group: crate::text::search::group_id(ctx.tree, element.id),
        selectable: ctx.inherited.selectable,
        highlight: ctx
            .inherited
            .highlight
            .clone()
            .map(crate::text::HighlightSource::Resolved),
        ..crate::text::SelectableText::new(
            element.id,
            0,
            gpui::SharedString::from(content.to_string()),
            None,
            ctx.selection.clone(),
            ctx.inherited.selection_wash,
        )
    })
}

/// One paragraph of inline content: a single string with a run per text node,
/// selected, searched and copied as one line of text. It has no group, so two
/// paragraphs never join into one copied line.
fn paragraph_content(
    paragraph: &crate::text::inline::Paragraph,
    ctx: &BuildCtx,
) -> gpui::AnyElement {
    let states = |id: u64, style: &StyleDesc| {
        let inert = ctx
            .tree
            .elements
            .get(&id)
            .is_some_and(crate::retained_tree::RetainedElement::has_inert_prop);
        ctx.focus_styles
            .states_of(id, style, ctx.pointer_states(id, style, inert))
    };
    selectable_text(crate::text::SelectableText {
        group: None,
        selectable: ctx.inherited.selectable,
        highlight: ctx
            .inherited
            .highlight
            .clone()
            .map(crate::text::HighlightSource::Resolved),
        run_styles: crate::text::paragraph::run_styles(
            ctx.tree,
            paragraph,
            &ctx.inherited.colors,
            &ctx.inherited.font_state,
            &states,
        ),
        inline_targets: crate::text::inline_hit::Targets::of(
            ctx.tree,
            paragraph,
            ctx.inherited.pointer_events_none,
            ctx.inherited.inert,
        )
        .map(Arc::new),
        inline_boxes: inline_boxes(paragraph, ctx),
        inline_focus: focusable_inline_boxes(paragraph, ctx),
        inline_decorations: crate::text::inline_decoration::of(
            ctx.tree,
            paragraph,
            &ctx.inherited.colors,
            &ctx.inherited.lengths,
            &states,
        ),
        inline_cursors: crate::text::inline_cursor::of(
            ctx.tree,
            paragraph,
            ctx.inherited.pointer_events_none,
            ctx.inherited.inert,
            &states,
        ),
        ..crate::text::SelectableText::new(
            paragraph.container,
            paragraph.index,
            gpui::SharedString::from(paragraph.text.clone()),
            None,
            ctx.selection.clone(),
            ctx.inherited.selection_wash,
        )
    })
}

/// The inline elements of a paragraph that report their own box. They have no
/// padding or border to take off a content box, so their style is not read.
fn inline_boxes(
    paragraph: &crate::text::inline::Paragraph,
    ctx: &BuildCtx,
) -> Vec<crate::text::inline_bounds::InlineBox> {
    paragraph
        .boxes
        .iter()
        .map(|(id, range)| crate::text::inline_bounds::InlineBox {
            id: *id,
            range: range.clone(),
            report: ctx.tree.elements.get(id).and_then(|element| {
                paint_reporter(*id, element.events(), Some(paragraph.container), ctx)
            }),
        })
        .collect()
}

/// The inline elements of a paragraph that are focusable, with the handle
/// `sync_focus_handles` made for them.
fn focusable_inline_boxes(
    paragraph: &crate::text::inline::Paragraph,
    ctx: &BuildCtx,
) -> Vec<crate::text::inline_focus::FocusableBox> {
    crate::text::inline_focus::in_tree_order(paragraph, |id| ctx.focus_handles.get(&id))
        .into_iter()
        .filter_map(|(id, handle)| {
            let props = ctx.tree.elements.get(&id)?.custom_props().clone();
            Some(crate::text::inline_focus::FocusableBox {
                id,
                handle: handle.clone(),
                accessibility: Box::new(move |anchor| {
                    crate::accessibility::apply_accessibility(anchor, id, &props, None)
                }),
            })
        })
        .collect()
}

/// Explicit `userSelect` on this node. `None` means inherit; the ancestor
/// that set the value already owns the start region.
fn selection_start_flag(style: Option<&StyleDesc>) -> Option<bool> {
    match style.and_then(|style| style.user_select.as_deref()) {
        Some("none") => Some(false),
        Some("text") | Some("auto") => Some(true),
        _ => None,
    }
}

// ── Style application ────────────────────────────────────────────────

/// The lines of `decoration` on the text style `el` passes down. A line that
/// is not declared leaves the inherited one alone, as in CSS, where `none` on a
/// descendant takes nothing off.
fn apply_text_decoration<E: gpui::Styled>(
    mut el: E,
    decoration: &crate::css::text_decoration::TextDecoration,
    colors: &crate::color::ColorContext,
) -> E {
    let color = decoration.resolve_color(colors);
    let text_style = el.text_style();
    if decoration.underline {
        text_style.underline = Some(gpui::UnderlineStyle {
            thickness: gpui::px(1.0),
            color: Some(color),
            wavy: decoration.wavy(),
        });
    }
    if decoration.line_through {
        text_style.strikethrough = Some(gpui::StrikethroughStyle {
            thickness: gpui::px(1.0),
            color: Some(color),
        });
    }
    el
}

/// The base styles of a stateful GPUI root.
///
/// `hover`, `active`, `groupHover`, `groupActive` and `groupFocusWithin` are not
/// applied here: `build_element` folds them into `style` before any builder
/// runs, for the elements the pointer dispatcher says are hovered or pressed and
/// those in a focused group. `focusVisible` and `focusWithin` need the focus state, so
/// `apply_focus_styles` applies it where the element calls `track_focus`.
pub(crate) fn apply_interactive_styles<E>(
    el: E,
    style: &StyleDesc,
    colors: &crate::color::ColorContext,
    lengths: &LengthContext,
) -> E
where
    E: gpui::Styled + gpui::StatefulInteractiveElement,
{
    apply_styles(el, style, colors, lengths)
}

/// A state refinement: the base colours that follow `currentColor` first, so a
/// state that changes `color` repaints them, then the state's own styles.
/// `colors` and `lengths` are the refinement's, from `for_state`.
pub(crate) fn apply_state_styles<E: gpui::Styled>(
    mut refinement: E,
    base: &StyleDesc,
    state: &StyleDesc,
    colors: &crate::color::ColorContext,
    lengths: &LengthContext,
) -> E {
    if let Some(followers) = base.colors_following_current(state, lengths) {
        refinement = apply_styles(refinement, &followers, colors, lengths);
    }
    apply_styles(refinement, state, colors, lengths)
}

/// `colors` and `lengths` are the context of the element `style` belongs to: its
/// own computed `color`, so `currentColor` in a border or shadow means what CSS
/// says, and its computed `font-size`, which is the text size `style.font_size`
/// declares once resolved.
pub(crate) fn apply_styles<E: gpui::Styled>(
    el: E,
    style: &StyleDesc,
    colors: &crate::color::ColorContext,
    lengths: &LengthContext,
) -> E {
    let fill = style.resolved_fill(colors, lengths);
    apply_styles_with_fill(el, style, colors, lengths, fill)
}

/// `fill` must be `style.resolved_fill(colors, lengths)`.
pub(crate) fn apply_styles_with_fill<E: gpui::Styled>(
    mut el: E,
    style: &StyleDesc,
    colors: &crate::color::ColorContext,
    lengths: &LengthContext,
    fill: crate::style::Fill,
) -> E {
    el = crate::layout::style::apply(el, style, lengths);
    match style.flex_wrap.as_deref() {
        Some("wrap") => el = el.flex_wrap(),
        Some("wrap-reverse") => el = el.flex_wrap_reverse(),
        Some("nowrap") => el = el.flex_nowrap(),
        _ => {}
    }
    if let Some(grow) = style.flex_grow {
        el.style().flex_grow = Some(grow as f32);
    }
    if let Some(shrink) = style.flex_shrink {
        el.style().flex_shrink = Some(shrink as f32);
    }
    match style.align_items.as_deref() {
        Some("center") => el = el.items_center(),
        Some("start") | Some("flex-start") => el = el.items_start(),
        Some("end") | Some("flex-end") => el = el.items_end(),
        Some("stretch") => el = el.items_stretch(),
        Some("baseline") => el = el.items_baseline(),
        _ => {}
    }
    match style.align_content.as_deref() {
        Some("center") => el = el.content_center(),
        Some("start") | Some("flex-start") => el = el.content_start(),
        Some("end") | Some("flex-end") => el = el.content_end(),
        Some("between") | Some("space-between") => el = el.content_between(),
        Some("around") | Some("space-around") => el = el.content_around(),
        Some("evenly") | Some("space-evenly") => el = el.content_evenly(),
        Some("stretch") => el = el.content_stretch(),
        Some("normal") => el = el.content_normal(),
        _ => {}
    }
    match style.justify_content.as_deref() {
        Some("center") => el = el.justify_center(),
        Some("start") | Some("flex-start") => el = el.justify_start(),
        Some("end") | Some("flex-end") => el = el.justify_end(),
        Some("between") | Some("space-between") => el = el.justify_between(),
        Some("around") | Some("space-around") => el = el.justify_around(),
        Some("evenly") | Some("space-evenly") => el = el.justify_evenly(),
        _ => {}
    }
    match style.align_self.as_deref() {
        Some("center") => {
            el.style().align_self = Some(gpui::AlignItems::Center);
        }
        Some("start") | Some("flex-start") => {
            el.style().align_self = Some(gpui::AlignItems::FlexStart);
        }
        Some("end") | Some("flex-end") => {
            el.style().align_self = Some(gpui::AlignItems::FlexEnd);
        }
        Some("stretch") => {
            el.style().align_self = Some(gpui::AlignItems::Stretch);
        }
        Some("baseline") => {
            el.style().align_self = Some(gpui::AlignItems::Baseline);
        }
        _ => {}
    }
    if let Some(ratio) = style.aspect_ratio.and_then(|ratio| ratio.ratio()) {
        el = el.aspect_ratio(ratio as f32);
    }
    if let Some(background) = fill.color {
        el = el.bg(background);
    }
    // An empty list is `background-image: none`, which a state style needs in
    // order to clear the base layers.
    if !fill.layers.is_empty() || style.declares_background_layers() {
        el.style().background_layers = Some(fill.layers);
    }
    if let Some(color) = colors.declared {
        el = el.text_color(color);
    }
    if style.font_size.is_some() {
        el = el.text_size(gpui::px(lengths.font_size as f32));
    }
    if let Some(ref family) = style.font_family {
        el = el.font_family(family.clone());
    }
    if let Some(ref weight) = style.font_weight {
        el = el.font_weight(parse_font_weight(weight));
    }
    if let Some(font_style) = style.font_style.as_deref().and_then(parse_font_style) {
        el.text_style().font_style = Some(font_style);
    }
    // `textAlign` was in the style type but implemented nowhere.
    match style.text_align.as_deref() {
        Some("center") => el = el.text_center(),
        Some("right") => el = el.text_right(),
        Some("left") | Some("start") => el = el.text_left(),
        _ => {}
    }
    // Collapsing is the paragraph builder's (`text::inline`); gpui wraps and hangs.
    if let Some(white_space) = style
        .white_space
        .as_deref()
        .and_then(crate::text::white_space::WhiteSpace::parse)
    {
        el = el.white_space(white_space);
    }
    if let Some(tab_size) = style.tab_size {
        el.text_style().tab_size = Some(tab_size.to_gpui());
    }
    crate::css::word_break::apply(el.text_style(), style.word_break, style.overflow_wrap);
    match style.text_overflow.as_deref() {
        Some("ellipsis") => el = el.text_ellipsis(),
        Some("ellipsis-start") => el = el.text_ellipsis_start(),
        _ => {}
    }
    if let Some(clamp) = style.line_clamp {
        if clamp >= 1.0 {
            el = el.line_clamp(clamp as usize);
        }
    }
    if let Some(decoration) = crate::css::text_decoration::of(style) {
        el = apply_text_decoration(el, &decoration, colors);
    }
    if let Some(line_height) = &style.line_height {
        el.text_style().line_height = Some(line_height.text_style_value(lengths));
    }
    if let Some(radius) = style.border_radius {
        el = el.rounded(gpui::px(radius as f32));
    }
    // Apply corner longhands after the shorthand so the explicit corner wins.
    if let Some(radius) = style.border_top_left_radius {
        el = el.rounded_tl(gpui::px(radius as f32));
    }
    if let Some(radius) = style.border_top_right_radius {
        el = el.rounded_tr(gpui::px(radius as f32));
    }
    if let Some(radius) = style.border_bottom_left_radius {
        el = el.rounded_bl(gpui::px(radius as f32));
    }
    if let Some(radius) = style.border_bottom_right_radius {
        el = el.rounded_br(gpui::px(radius as f32));
    }
    el = crate::css::border::apply_shorthands(el, style, colors, lengths);
    // `borderWidth: 0` must clear a border, not be ignored: an element that
    // draws its own border needs a way for the caller to remove it.
    let width = |width: &crate::css::length::LinearLength| crate::css::border::resolve_width(width, lengths);
    if let Some(value) = &style.border_width {
        el = el.border(width(value));
    }
    if let Some(value) = &style.border_top_width {
        el = el.border_t(width(value));
    }
    if let Some(value) = &style.border_right_width {
        el = el.border_r(width(value));
    }
    if let Some(value) = &style.border_bottom_width {
        el = el.border_b(width(value));
    }
    if let Some(value) = &style.border_left_width {
        el = el.border_l(width(value));
    }
    el = crate::css::border::apply_colors(el, style, colors);
    match style.border_style.as_deref() {
        Some("dashed") => el = el.border_dashed(),
        Some("solid") => el.style().border_style = Some(gpui::BorderStyle::Solid),
        _ => {}
    }
    if let Some(ref shadows) = style.box_shadow {
        // A shadow whose colour does not parse is dropped on its own, so one
        // bad entry does not take the rest of a list with it.
        // CSS paints the first layer on top; gpui paints later ones on top.
        let shadows: Vec<gpui::BoxShadow> = shadows
            .as_slice()
            .iter()
            .rev()
            .filter_map(|shadow| {
                let color = colors.resolve(&shadow.color)?;
                // The CSS blur radius is two standard deviations; gpui's shadow
                // shaders take the standard deviation itself.
                let length = |length: &crate::css::length::LinearLength| lengths.resolve(length, 0.0);
                let built = gpui::BoxShadow::new(
                    gpui::px(length(&shadow.offset_x) as f32),
                    gpui::px(length(&shadow.offset_y) as f32),
                    color.into(),
                )
                .blur_radius(gpui::px((length(&shadow.blur_radius).max(0.0) / 2.0) as f32))
                .spread_radius(gpui::px(length(&shadow.spread_radius) as f32));
                Some(if shadow.inset { built.inset() } else { built })
            })
            .collect();
        if !shadows.is_empty() {
            el = el.shadow(shadows);
        }
    }
    if let Some(outline) = crate::css::border::outline(style, colors, lengths) {
        el = el.outline(outline);
    }
    match style.visibility.as_deref() {
        Some("hidden") => el = el.invisible(),
        Some("visible") => el = el.visible(),
        _ => {}
    }
    if let Some(opacity) = style.opacity {
        el = el.opacity(opacity as f32);
    }
    // Some(empty) is `none`, which a state style needs in order to clear the base filter.
    if let Some(filters) = style
        .filter
        .as_deref()
        .and_then(|value| crate::css::filter::parse_resolved(value, colors, lengths))
    {
        el.style().filter = Some(filters);
    }
    if let Some(filters) = style
        .backdrop_filter
        .as_deref()
        .and_then(|value| crate::css::filter::parse_resolved(value, colors, lengths))
    {
        el.style().backdrop_filter = Some(filters);
    }
    if let Some(layers) = style.resolved_mask(colors, lengths) {
        el.style().mask_layers = Some(layers);
    }
    // `Some(None)` is `none`, which a state style needs in order to clear the base clip path.
    if let Some(clip_path) = style
        .clip_path
        .as_deref()
        .and_then(|value| crate::css::clip_path::parse(value, lengths))
    {
        el.style().clip_path = Some(clip_path.into());
    }
    if let Some(cursor) = style.cursor.as_deref().and_then(crate::style::parse_cursor) {
        el = el.cursor(cursor);
    }
    // `hidden` and `clip` are on the Styled trait; scrolling needs a stateful
    // element and is applied by `scroll::apply_container`.
    {
        let axes = crate::scroll::Axes::of(style);
        let hides = |overflow| {
            matches!(
                overflow,
                crate::scroll::Overflow::Clip | crate::scroll::Overflow::Hidden
            )
        };
        if hides(axes.x) && hides(axes.y) {
            el = el.overflow_hidden();
        } else if hides(axes.x) {
            el = el.overflow_x_hidden();
        } else if hides(axes.y) {
            el = el.overflow_y_hidden();
        }
    }

    el
}

// ── Event emission ───────────────────────────────────────────────────

/// Convert GPUI MouseButton to our u32 encoding: 0=left, 1=middle, 2=right.
pub(crate) fn mouse_button_to_u32(button: gpui::MouseButton) -> u32 {
    match button {
        gpui::MouseButton::Left => 0,
        gpui::MouseButton::Middle => 1,
        gpui::MouseButton::Right => 2,
        gpui::MouseButton::Navigate(_) => 3,
    }
}

/// The paint-driven events of one element, as one reporter for `on_painted`.
///
/// `onLayout`: its border box after the first paint with the listener, then again
/// each time its size changes. A move without a resize reports nothing, so
/// scrolling a list does not queue an event per row.
///
/// `onScroll`: its scroll offset each time it differs from the last paint,
/// whatever moved it. The wheel is only one cause; a scrollbar drag or
/// `scrollTo` moves it too, and gpui raises no event for those.
///
/// Events are queued after the paint that saw the change, never from inside it.
/// The browser build calls the listener synchronously, and a handler that sets
/// state would re-enter the frame that is being painted.
fn paint_reporter(
    id: u64,
    events: &HashSet<String>,
    clips_from: Option<u64>,
    ctx: &BuildCtx,
) -> Option<crate::automation::PaintReporter> {
    let layout = events.contains("layout");
    let scroll = events
        .contains("scroll")
        .then(|| ctx.scroll_handles.get(&id).cloned())
        .flatten();
    let observe = crate::observers::reporter(
        id,
        events,
        ctx.tree,
        clips_from,
        ctx.paint_reports,
        ctx.event_callback,
    );
    if !layout && scroll.is_none() && observe.is_none() {
        return None;
    }
    let callback = ctx.event_callback.clone()?;
    let reports = ctx.paint_reports.clone();
    Some(Rc::new(move |bounds, insets, window, cx| {
        if layout {
            report_layout(id, bounds, &reports, &callback, cx);
        }
        if let Some(handle) = &scroll {
            report_scroll(id, handle.offset(), &reports, &callback, cx);
        }
        if let Some(observe) = &observe {
            observe(bounds, insets, window, cx);
        }
    }))
}

fn report_layout(
    id: u64,
    bounds: gpui::Bounds<gpui::Pixels>,
    reports: &SharedPaintReports,
    callback: &EventCallback,
    cx: &mut gpui::App,
) {
    let size = (
        f32::from(bounds.size.width),
        f32::from(bounds.size.height),
    );
    {
        let mut reports = reports.borrow_mut();
        let unchanged = reports.sizes.get(&id).is_some_and(|last| {
            (last.0 - size.0).abs() < PAINT_REPORT_EPSILON
                && (last.1 - size.1).abs() < PAINT_REPORT_EPSILON
        });
        if unchanged {
            return;
        }
        reports.sizes.insert(id, size);
    }
    let callback = Some(callback.clone());
    cx.defer(move |_| {
        emit_event_full(&callback, id, "layout", |payload| {
            payload.x = Some(f64::from(f32::from(bounds.origin.x)));
            payload.y = Some(f64::from(f32::from(bounds.origin.y)));
            payload.width = Some(f64::from(size.0));
            payload.height = Some(f64::from(size.1));
        });
    });
}

fn report_scroll(
    id: u64,
    offset: gpui::Point<gpui::Pixels>,
    reports: &SharedPaintReports,
    callback: &EventCallback,
    cx: &mut gpui::App,
) {
    let offset = (f32::from(offset.x), f32::from(offset.y));
    {
        let mut reports = reports.borrow_mut();
        match reports.scrolls.get(&id).copied() {
            // The first paint only sets the baseline.
            None => {
                reports.scrolls.insert(id, offset);
                return;
            }
            Some(last)
                if (last.0 - offset.0).abs() < PAINT_REPORT_EPSILON
                    && (last.1 - offset.1).abs() < PAINT_REPORT_EPSILON =>
            {
                return;
            }
            Some(_) => {
                reports.scrolls.insert(id, offset);
            }
        }
    }
    let callback = Some(callback.clone());
    cx.defer(move |_| {
        emit_event_full(&callback, id, "scroll", |payload| {
            payload.scroll_x = Some(f64::from(offset.0));
            payload.scroll_y = Some(f64::from(offset.1));
        });
    });
}

/// The retained path a non-pointer click on `element` bubbles along. Empty when
/// nothing would receive it, which is also when it is never read: nothing
/// listens for a click and the element is not a label that activates a control,
/// or the element is in a disabled form control, which an accessibility click
/// does nothing on.
pub(crate) fn click_path(tree: &RetainedTree, element: &crate::retained_tree::RetainedElement) -> Vec<u64> {
    let listens = element.events().contains("click");
    if (!listens && element.element_type != "label") || tree.in_disabled_form_control(element.id) {
        return Vec::new();
    }
    let path = crate::events::path::event_path(tree, element.id);
    if listens || crate::accessibility::label::activated_control(tree, &path).is_some() {
        path
    } else {
        Vec::new()
    }
}

/// Values closer than this are the same value. Layout is deterministic, so this
/// only absorbs float noise from fractional scale factors.
const PAINT_REPORT_EPSILON: f32 = 0.01;

/// General-purpose event emitter. Builds a default EventPayload, lets the
/// caller customize it via a closure, then sends it through the callback.
/// Production: queues on Node.js event loop via ThreadsafeFunction.
/// Tests: pushes to a synchronous Vec for drainEvents().
pub(crate) fn emit_event_full(
    callback: &Option<EventCallback>,
    element_id: u64,
    event_type: &str,
    build: impl FnOnce(&mut EventPayload),
) {
    if let Some(cb) = callback {
        let mut payload = EventPayload {
            element_id: element_id as f64,
            event_type: event_type.to_string(),
            ..Default::default()
        };
        build(&mut payload);
        cb(payload);
    }
}

// ── Batch processing ─────────────────────────────────────────────

/// Parsed batch operation — typed enum for atomic validation.
/// All ops are parsed and validated BEFORE any tree mutation occurs.
/// This prevents partial application on malformed batches.
enum BatchOp<'a> {
    CreateElement {
        id: u64,
        element_type: String,
    },
    DestroyElement {
        id: u64,
    },
    AppendChild {
        parent_id: u64,
        child_id: u64,
    },
    InsertBefore {
        parent_id: u64,
        child_id: u64,
        before_id: u64,
    },
    /// The payload stays as raw JSON until apply time.
    ///
    /// Two reasons. A parsed `StyleDesc` is ~1.4 KB, and a `Vec<BatchOp>` is as
    /// wide as its widest variant, so inlining one made a 220k-op mount reserve
    /// over 300 MB before it parsed a single op. And the tree hash-conses
    /// styles by content, so it needs the bytes: hashing ~110 bytes is far
    /// cheaper than building 80 `Option` fields and throwing 99.8% of them away.
    SetStyle {
        id: u64,
        style: &'a serde_json::value::RawValue,
    },
    SetText {
        id: u64,
        content: String,
    },
    SetEventListener {
        id: u64,
        event_type: String,
        has_handler: bool,
    },
    SetRoot {
        id: u64,
    },
    SetKeyboardFocusDim {
        enabled: bool,
    },
    SetCustomProp {
        id: u64,
        key: String,
        value: serde_json::Value,
    },
}

/// A batch failure. The message names the op index, so it survives the trip
/// back to JS as a plain `Error`.
pub type BatchResult<T> = std::result::Result<T, String>;

/// Decode the batch straight from its JSON bytes into `Vec<BatchOp>`.
///
/// There is deliberately no `Vec<serde_json::Value>` in between. That tree cost
/// a `String` per key and per value, every payload was then deep-cloned out of
/// it, and `from_value` parsed the clone a second time, so one style was
/// allocated three times. A 220k-op mount made 1.5M allocations that way.
///
/// Everything the `Value` version guaranteed still holds, and each one is
/// load-bearing:
///
/// * an unknown opcode is a hard error, not a skipped op. Silently ignoring one
///   would let a JS/Rust version skew desync the tree instead of throwing
/// * ids go through `raw_element_id`, so non-finite, negative, fractional and
///   out-of-safe-range values are still rejected
/// * `hasHandler` is accepted as a bool or a number
/// * errors still name the op index. `serde_json` reports a byte offset, which
///   is useless when you are chasing a desync
fn parse_batch_ops(bytes: &[u8]) -> BatchResult<Vec<BatchOp<'_>>> {
    serde_json::from_slice::<BatchOps>(bytes)
        .map(|batch| batch.0)
        .map_err(|error| format!("Failed to parse batch: {error}"))
}

struct BatchOps<'a>(Vec<BatchOp<'a>>);

impl<'de> serde::Deserialize<'de> for BatchOps<'de> {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct OpsVisitor;

        impl<'de> serde::de::Visitor<'de> for OpsVisitor {
            type Value = BatchOps<'de>;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an array of mutation tuples")
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<BatchOps<'de>, A::Error> {
                let mut ops = Vec::with_capacity(seq.size_hint().unwrap_or(64));
                loop {
                    // The index is attached here because this is the only place
                    // that knows it.
                    let index = ops.len();
                    match seq.next_element::<BatchOp<'de>>() {
                        Ok(Some(op)) => ops.push(op),
                        Ok(None) => break,
                        Err(error) => {
                            return Err(serde::de::Error::custom(format!(
                                "Batch op {index}: {error}"
                            )))
                        }
                    }
                }
                Ok(BatchOps(ops))
            }
        }

        deserializer.deserialize_seq(OpsVisitor)
    }
}

/// A string argument, borrowed from the input when the JSON has no escapes.
///
/// The owned copy happens exactly once, on the way into the `BatchOp`. The
/// `Value` path allocated twice: into `Value::String`, then into the op.
struct StrArg<'a>(std::borrow::Cow<'a, str>);

impl<'de> serde::Deserialize<'de> for StrArg<'de> {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        use std::borrow::Cow;
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = StrArg<'de>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a string")
            }
            fn visit_borrowed_str<E: serde::de::Error>(
                self,
                v: &'de str,
            ) -> std::result::Result<StrArg<'de>, E> {
                Ok(StrArg(Cow::Borrowed(v)))
            }
            fn visit_str<E: serde::de::Error>(
                self,
                v: &str,
            ) -> std::result::Result<StrArg<'de>, E> {
                Ok(StrArg(Cow::Owned(v.to_owned())))
            }
            fn visit_string<E: serde::de::Error>(
                self,
                v: String,
            ) -> std::result::Result<StrArg<'de>, E> {
                Ok(StrArg(Cow::Owned(v)))
            }
        }
        deserializer.deserialize_str(V)
    }
}

fn next_arg<'de, A, T>(seq: &mut A, what: &str) -> std::result::Result<T, A::Error>
where
    A: serde::de::SeqAccess<'de>,
    T: serde::Deserialize<'de>,
{
    seq.next_element()?
        .ok_or_else(|| serde::de::Error::custom(format!("missing {what}")))
}

/// Read an element id. Ids cross napi as JS numbers, so they are read as `f64`
/// and validated exactly as `batch_id` did.
fn next_id<'de, A: serde::de::SeqAccess<'de>>(
    seq: &mut A,
    what: &str,
) -> std::result::Result<u64, A::Error> {
    let raw: f64 = next_arg(seq, what)?;
    raw_element_id(raw).map_err(serde::de::Error::custom)
}

impl<'de> serde::Deserialize<'de> for BatchOp<'de> {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct V;

        impl<'de> serde::de::Visitor<'de> for V {
            type Value = BatchOp<'de>;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a [opcode, ...args] mutation tuple")
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<BatchOp<'de>, A::Error> {
                let name: StrArg<'de> = next_arg(&mut seq, "op name")?;
                let op = match name.0.as_ref() {
                    "createElement" => BatchOp::CreateElement {
                        id: next_id(&mut seq, "id")?,
                        element_type: next_arg::<A, StrArg>(&mut seq, "element type")?
                            .0
                            .into_owned(),
                    },
                    "destroyElement" => BatchOp::DestroyElement {
                        id: next_id(&mut seq, "id")?,
                    },
                    "appendChild" => BatchOp::AppendChild {
                        parent_id: next_id(&mut seq, "parent id")?,
                        child_id: next_id(&mut seq, "child id")?,
                    },
                    "insertBefore" => BatchOp::InsertBefore {
                        parent_id: next_id(&mut seq, "parent id")?,
                        child_id: next_id(&mut seq, "child id")?,
                        before_id: next_id(&mut seq, "before id")?,
                    },
                    "setStyle" => BatchOp::SetStyle {
                        id: next_id(&mut seq, "id")?,
                        style: next_arg(&mut seq, "style")?,
                    },
                    "setText" => BatchOp::SetText {
                        id: next_id(&mut seq, "id")?,
                        content: next_arg::<A, StrArg>(&mut seq, "text")?.0.into_owned(),
                    },
                    "setEventListener" => BatchOp::SetEventListener {
                        id: next_id(&mut seq, "id")?,
                        event_type: next_arg::<A, StrArg>(&mut seq, "event type")?
                            .0
                            .into_owned(),
                        has_handler: next_arg(&mut seq, "hasHandler")?,
                    },
                    "setRoot" => BatchOp::SetRoot {
                        id: next_id(&mut seq, "id")?,
                    },
                    "setKeyboardFocusDim" => BatchOp::SetKeyboardFocusDim {
                        enabled: next_arg(&mut seq, "enabled")?,
                    },
                    "setCustomProp" => BatchOp::SetCustomProp {
                        id: next_id(&mut seq, "id")?,
                        key: next_arg::<A, StrArg>(&mut seq, "prop key")?.0.into_owned(),
                        value: next_arg(&mut seq, "custom prop value")?,
                    },
                    other => {
                        return Err(serde::de::Error::custom(format!(
                            "unknown operation: {other:?}"
                        )))
                    }
                };
                // Trailing arguments are tolerated, as they were when the op was
                // an indexed array.
                while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
                Ok(op)
            }
        }

        deserializer.deserialize_seq(V)
    }
}

/// Turn one raw `setStyle` payload into a shared style.
///
fn intern_style_payload(
    styles: &mut StyleTable,
    payload: &serde_json::value::RawValue,
) -> BatchResult<Arc<StyleDesc>> {
    let raw = payload.get().trim();
    styles.intern(raw.as_bytes())
}

/// Resolve every `setStyle` payload in the batch, in op order.
///
/// This is the last fallible step, so it runs before the apply loop and borrows
/// only the style table. The borrow checker then proves no element was touched
/// when it returns `Err`, which is what makes a batch atomic. An earlier
/// version interned inside the apply loop, so a malformed style at the end of a
/// batch left everything before it applied and then threw.
fn resolve_styles(
    styles: &mut StyleTable,
    ops: &[BatchOp<'_>],
) -> BatchResult<Vec<Arc<StyleDesc>>> {
    let mut resolved = Vec::new();
    for (index, op) in ops.iter().enumerate() {
        if let BatchOp::SetStyle { style, .. } = op {
            let shared = intern_style_payload(styles, style)
                .map_err(|error| format!("Batch op {index} setStyle parse error: {error}"))?;
            resolved.push(shared);
        }
    }
    Ok(resolved)
}

/// Apply a batch of mutation tuples to a RetainedTree.
/// Shared between GpuixRenderer::apply_batch and TestGpuixRenderer::apply_batch.
/// Returns accumulated destroyed IDs (as f64) from all destroyElement ops.
///
/// ATOMIC: the batch is decoded and every style is resolved before a single
/// element is touched. If any op is malformed the tree is left unchanged and an
/// error is returned. Nothing after that point can fail, so JS and Rust cannot
/// desync when a batch is retried.
///
/// Batch format: JSON array of tuples [opcode, ...args].
/// See GpuixRenderer::apply_batch for opcode documentation.
///
/// Public so `examples/bench_serde.rs` times this exact function. A replica in
/// the bench would drift, and the numbers would then describe code nobody runs.
pub fn apply_batch_to_tree(tree: &mut RetainedTree, bytes: &[u8]) -> BatchResult<Vec<f64>> {
    // Phase 1: decode. No mutation.
    let parsed = parse_batch_ops(bytes)?;

    // Phase 2: resolve styles. Touches the style table only; a failure here
    // sweeps back out whatever this call interned.
    let styles = resolve_styles(&mut tree.styles, &parsed).inspect_err(|_| tree.styles.sweep())?;
    let mut styles = styles.into_iter();

    // Phase 3: apply. Cannot fail.
    let mut destroyed_ids: Vec<f64> = Vec::new();
    for batch_op in parsed {
        match batch_op {
            BatchOp::CreateElement { id, element_type } => {
                tree.create_element(id, element_type);
            }
            BatchOp::DestroyElement { id } => {
                let destroyed = tree.destroy_element(id);
                destroyed_ids.extend(destroyed.iter().map(|&id| id as f64));
            }
            BatchOp::AppendChild {
                parent_id,
                child_id,
            } => {
                tree.append_child(parent_id, child_id);
            }
            BatchOp::InsertBefore {
                parent_id,
                child_id,
                before_id,
            } => {
                tree.insert_before(parent_id, child_id, before_id);
            }
            BatchOp::SetStyle { id, .. } => {
                let shared = styles.next().expect("one resolved style per setStyle op");
                tree.set_style(id, shared);
            }
            BatchOp::SetText { id, content } => {
                tree.set_text(id, content);
            }
            BatchOp::SetEventListener {
                id,
                event_type,
                has_handler,
            } => {
                tree.set_event_listener(id, event_type, has_handler);
            }
            BatchOp::SetRoot { id } => {
                tree.set_root(id);
            }
            BatchOp::SetKeyboardFocusDim { enabled } => {
                tree.set_keyboard_focus_dim(enabled);
            }
            BatchOp::SetCustomProp { id, key, value } => {
                tree.set_custom_prop(id, key, value);
            }
        }
    }

    // Release styles nothing references any more. Without this a dragged
    // element, which produces a distinct style every frame, would grow the
    // table for as long as the app runs. The element count is what catches the
    // opposite case, a batch that destroyed most of the tree.
    let live_elements = tree.elements.len();
    tree.styles.maybe_sweep(live_elements);

    Ok(destroyed_ids)
}

// ── Types ────────────────────────────────────────────────────────────

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
type PathPromptReceiver = oneshot::Receiver<anyhow::Result<Option<Vec<std::path::PathBuf>>>>;

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
type PathPromptResolver = Box<dyn FnOnce(Env) -> Result<Option<Vec<String>>> + Send>;

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
type PathPromptDeferred = JsDeferred<Option<Vec<String>>, PathPromptResolver>;

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn path_prompt_result(
    result: anyhow::Result<Option<Vec<std::path::PathBuf>>>,
) -> Result<Option<Vec<String>>> {
    result
        .map_err(|error| Error::from_reason(error.to_string()))?
        .filter(|paths| !paths.is_empty())
        .map(|paths| {
            paths
                .into_iter()
                .map(|path| {
                    path.into_os_string()
                        .into_string()
                        .map_err(|_| Error::from_reason("The selected path is not valid Unicode"))
                })
                .collect()
        })
        .transpose()
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn settle_path_prompt(
    executor: gpui::BackgroundExecutor,
    receiver: PathPromptReceiver,
    deferred: PathPromptDeferred,
) {
    executor
        .spawn(async move {
            let result = receiver
                .await
                .map_err(|_| Error::from_reason("The path prompt closed without a result"))
                .and_then(path_prompt_result);
            match result {
                Ok(paths) => deferred.resolve(Box::new(move |_env| Ok(paths))),
                Err(error) => deferred.reject(error),
            }
        })
        .detach();
}

#[derive(Debug, Clone, Default)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct PathPromptOptions {
    /// Select files. Defaults to true unless `directories` is true.
    pub files: Option<bool>,
    /// Select directories. Defaults to false.
    pub directories: Option<bool>,
    /// Allow several paths. Defaults to false.
    pub multiple: Option<bool>,
    /// Label for the picker confirmation button.
    pub prompt: Option<String>,
    /// Restrict the selectable files with the HTML `accept` syntax, such as
    /// ".png,.jpg,image/*,application/pdf". Directories are never filtered.
    pub accept: Option<String>,
}

impl PathPromptOptions {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    pub(crate) fn to_gpui(options: Option<Self>) -> Result<gpui::PathPromptOptions> {
        let options = options.unwrap_or_default();
        let directories = options.directories.unwrap_or(false);
        let files = options.files.unwrap_or(!directories);
        if !files && !directories {
            return Err(Error::from_reason(
                "promptForPaths requires files or directories",
            ));
        }
        if files && directories && !cfg!(target_os = "macos") {
            return Err(Error::from_reason(
                "Selecting files and directories together is only supported on macOS",
            ));
        }
        let filters = match options.accept {
            Some(accept) => crate::platform::file_filter::parse_accept(&accept)
                .map_err(Error::from_reason)?,
            None => Vec::new(),
        };
        Ok(gpui::PathPromptOptions {
            files,
            directories,
            multiple: options.multiple.unwrap_or(false),
            prompt: options.prompt.map(Into::into),
            filters,
        })
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub struct PromptForPathsTask {
    receiver: Option<Result<PathPromptReceiver>>,
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl PromptForPathsTask {
    pub(crate) fn new(receiver: Result<PathPromptReceiver>) -> Self {
        Self {
            receiver: Some(receiver),
        }
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
#[napi]
impl Task for PromptForPathsTask {
    type Output = Option<Vec<String>>;
    type JsValue = Option<Vec<String>>;

    fn compute(&mut self) -> Result<Self::Output> {
        let receiver = self
            .receiver
            .take()
            .ok_or_else(|| Error::from_reason("Path prompt was already awaited"))??;
        let result = futures::executor::block_on(receiver)
            .map_err(|_| Error::from_reason("The path prompt closed without a result"))?;
        path_prompt_result(result)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct WindowSize {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Default)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct EdgeInsets {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

#[derive(Debug, Clone, Default)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct WindowInsets {
    pub safe_area: EdgeInsets,
    pub ime: EdgeInsets,
    pub effective: EdgeInsets,
}

impl WindowInsets {
    fn from_gpui(insets: gpui::WindowInsets) -> Self {
        let effective = insets.effective();
        Self {
            safe_area: EdgeInsets::from_gpui(insets.safe_area),
            ime: EdgeInsets::from_gpui(insets.ime),
            effective: EdgeInsets::from_gpui(effective),
        }
    }
}

impl EdgeInsets {
    fn from_gpui(insets: gpui::Edges<gpui::Pixels>) -> Self {
        Self {
            top: f32::from(insets.top) as f64,
            right: f32::from(insets.right) as f64,
            bottom: f32::from(insets.bottom) as f64,
            left: f32::from(insets.left) as f64,
        }
    }
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn edge_insets_js(
    insets: gpui::Edges<gpui::Pixels>,
) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
    let object = js_sys::Object::new();
    for (key, value) in [
        ("top", insets.top),
        ("right", insets.right),
        ("bottom", insets.bottom),
        ("left", insets.left),
    ] {
        js_sys::Reflect::set(
            &object,
            &wasm_bindgen::JsValue::from_str(key),
            &wasm_bindgen::JsValue::from_f64(f32::from(value) as f64),
        )?;
    }
    Ok(object.into())
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn window_insets_js(
    insets: gpui::WindowInsets,
) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
    let effective = insets.effective();
    let object = js_sys::Object::new();
    for (key, value) in [
        ("safeArea", edge_insets_js(insets.safe_area)?),
        ("ime", edge_insets_js(insets.ime)?),
        ("effective", edge_insets_js(effective)?),
    ] {
        js_sys::Reflect::set(&object, &wasm_bindgen::JsValue::from_str(key), &value)?;
    }
    Ok(object.into())
}

/// Last painted box for a host element.
#[derive(Debug, Clone)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct ElementBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// The DOM's `clientLeft` and `clientTop`: the left and top border widths.
    /// The padding box starts at `(x + clientLeft, y + clientTop)`.
    pub client_left: f64,
    pub client_top: f64,
}

impl ElementBounds {
    pub(crate) fn from_painted(bounds: crate::automation::ElementBounds) -> Self {
        Self {
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
            client_left: bounds.client_left,
            client_top: bounds.client_top,
        }
    }
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn element_bounds_js(
    bounds: crate::automation::ElementBounds,
) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
    let object = js_sys::Object::new();
    for (key, value) in [
        ("x", bounds.x),
        ("y", bounds.y),
        ("width", bounds.width),
        ("height", bounds.height),
        ("clientLeft", bounds.client_left),
        ("clientTop", bounds.client_top),
    ] {
        js_sys::Reflect::set(
            &object,
            &wasm_bindgen::JsValue::from_str(key),
            &wasm_bindgen::JsValue::from_f64(value),
        )?;
    }
    Ok(object.into())
}

/// Recorded draw times from the debug frame overlay.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
#[derive(Debug, Clone)]
#[napi(object)]
pub struct DebugFrameOverlayStats {
    pub current_ms: Option<f64>,
    pub p90_ms: Option<f64>,
    pub p99_ms: Option<f64>,
    pub max_ms: Option<f64>,
    pub frames: f64,
    pub samples: f64,
}

/// Wayland `wlr-layer-shell` surface options. Linux/Wayland only; ignored on
/// every other platform. When present on `WindowOptions`, the window is opened
/// as a compositor-anchored surface (a bar, dock, notification overlay or
/// wallpaper) with no native titlebar instead of a normal floating window.
/// `width` / `height` then only constrain the axis the surface is not
/// stretched along by its `anchor`.
#[derive(Debug, Clone, Default)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct LayerShellOptions {
    /// Compositor surface namespace, used for window rules. Cannot be changed
    /// after the surface is created. Defaults to the empty string.
    pub namespace: Option<String>,
    /// `"background"` | `"bottom"` | `"top"` | `"overlay"`. Defaults to `"top"`.
    pub layer: Option<String>,
    /// Screen edges to anchor to: any combination of `"top"`, `"bottom"`,
    /// `"left"`, `"right"`. Anchoring two opposite edges stretches the surface
    /// across that axis. Defaults to `["top", "left", "right"]` (a top bar).
    pub anchor: Option<Vec<String>>,
    /// Logical pixels to reserve along the anchored edge so other windows do
    /// not overlap the surface. `0` lets the compositor decide, a negative
    /// value asks it not to reserve any space.
    pub exclusive_zone: Option<f64>,
    /// Which edge the exclusive zone applies to when it cannot be inferred from
    /// a single-edge `anchor`. Same values as one `anchor` entry.
    pub exclusive_edge: Option<String>,
    /// Gap between the surface and its anchor edge(s), in CSS order:
    /// `[top, right, bottom, left]`. Must have exactly four entries or it is
    /// ignored.
    pub margin: Option<Vec<f64>>,
    /// `"none"` | `"on-demand"` | `"exclusive"`. Defaults to `"none"`: a bar or
    /// overlay that never takes keyboard focus.
    pub keyboard_interactivity: Option<String>,
}

#[derive(Debug, Clone)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct WindowOptions {
    pub title: Option<String>,
    /// The name used inside the macOS "Hide" and "Quit" menu items. Defaults to
    /// `title`. It does NOT set the title of the application menu itself: macOS
    /// takes that from the executable, and only a `.app` bundle changes it.
    pub app_name: Option<String>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub min_width: Option<f64>,
    pub min_height: Option<f64>,
    pub resizable: Option<bool>,
    pub fullscreen: Option<bool>,
    /// Plain alpha transparency. Prefer `window_background` when you need blur.
    pub transparent: Option<bool>,
    /// Hide the native titlebar so the app can draw chrome under the traffic lights.
    pub titlebar_transparent: Option<bool>,
    /// `"opaque"` | `"transparent"` | `"blurred"`. `transparent: true` is the
    /// same as `"transparent"` when this is unset.
    pub window_background: Option<String>,
    /// `"system"` | `"light"` | `"dark"`. Defaults to `"system"`: follow the OS
    /// setting. Applies to the whole app on macOS and is ignored elsewhere.
    pub appearance: Option<String>,
    pub traffic_light_x: Option<f64>,
    pub traffic_light_y: Option<f64>,
    /// Give the window key focus when it opens. `false` opens it behind the
    /// active app, like `open -g`. Ignored on Linux.
    pub focus: Option<bool>,
    /// Show the window when it opens. `false` opens it hidden; call
    /// `activateWindow()` to reveal it. Ignored on Linux.
    pub show: Option<bool>,
    /// Application id: Wayland `app_id` / X11 `WM_CLASS`. Desktop environments
    /// use it to group windows and match window rules. Required in practice for
    /// a `layerShell` surface a compositor is meant to target by rule.
    pub app_id: Option<String>,
    /// Open the window as a Wayland `wlr-layer-shell` surface instead of a
    /// normal window. Linux/Wayland only; ignored elsewhere.
    pub layer_shell: Option<LayerShellOptions>,
}

impl WindowOptions {
    /// The window has no titlebar of its own, so the app marks what drags it (`appRegion`).
    pub(crate) fn frameless(&self) -> bool {
        self.titlebar_transparent.unwrap_or(false)
    }
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            title: Some("GPUIX".to_string()),
            app_name: None,
            width: Some(800.0),
            height: Some(600.0),
            min_width: None,
            min_height: None,
            resizable: Some(true),
            fullscreen: Some(false),
            transparent: Some(false),
            titlebar_transparent: Some(false),
            window_background: None,
            appearance: None,
            traffic_light_x: None,
            traffic_light_y: None,
            focus: Some(true),
            show: Some(true),
            app_id: None,
            layer_shell: None,
        }
    }
}

#[cfg(target_os = "linux")]
fn layer_shell_anchor_bit(name: &str) -> gpui::layer_shell::Anchor {
    use gpui::layer_shell::Anchor;
    match name.trim().to_ascii_lowercase().as_str() {
        "top" => Anchor::TOP,
        "bottom" => Anchor::BOTTOM,
        "left" => Anchor::LEFT,
        "right" => Anchor::RIGHT,
        _ => Anchor::empty(),
    }
}

#[cfg(target_os = "linux")]
fn to_layer_shell_options(options: &LayerShellOptions) -> gpui::layer_shell::LayerShellOptions {
    use gpui::layer_shell::{Anchor, KeyboardInteractivity, Layer};

    let layer = match options.layer.as_deref() {
        Some("background") => Layer::Background,
        Some("bottom") => Layer::Bottom,
        Some("overlay") => Layer::Overlay,
        _ => Layer::Top,
    };
    let anchor = match options.anchor.as_deref() {
        Some(names) if !names.is_empty() => names.iter().fold(Anchor::empty(), |acc, name| {
            acc | layer_shell_anchor_bit(name)
        }),
        _ => Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
    };
    let keyboard_interactivity = match options.keyboard_interactivity.as_deref() {
        Some("on-demand") => KeyboardInteractivity::OnDemand,
        Some("exclusive") => KeyboardInteractivity::Exclusive,
        _ => KeyboardInteractivity::None,
    };
    let margin = options.margin.as_ref().and_then(|m| match m.as_slice() {
        [top, right, bottom, left] => Some((
            gpui::px(*top as f32),
            gpui::px(*right as f32),
            gpui::px(*bottom as f32),
            gpui::px(*left as f32),
        )),
        _ => None,
    });

    gpui::layer_shell::LayerShellOptions {
        namespace: options.namespace.clone().unwrap_or_default(),
        layer,
        anchor,
        exclusive_zone: options.exclusive_zone.map(|z| gpui::px(z as f32)),
        exclusive_edge: options
            .exclusive_edge
            .as_deref()
            .map(layer_shell_anchor_bit),
        margin,
        keyboard_interactivity,
    }
}

/// `Ok(None)` is `"system"`: no override, GPUI tracks the OS setting.
pub(crate) fn parse_window_appearance(
    value: &str,
) -> std::result::Result<Option<gpui::WindowAppearance>, String> {
    match value {
        "system" => Ok(None),
        "light" => Ok(Some(gpui::WindowAppearance::Light)),
        "dark" => Ok(Some(gpui::WindowAppearance::Dark)),
        other => Err(format!(
            "Invalid appearance {other:?}: expected \"system\", \"light\" or \"dark\""
        )),
    }
}

/// The vibrant variants are the same light/dark choice with a translucent
/// material, which JS has no use for.
pub(crate) fn color_scheme_of(appearance: gpui::WindowAppearance) -> crate::color::Scheme {
    match appearance {
        gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight => {
            crate::color::Scheme::Light
        }
        gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark => {
            crate::color::Scheme::Dark
        }
    }
}

pub(crate) fn appearance_name(appearance: gpui::WindowAppearance) -> &'static str {
    match appearance {
        gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight => "light",
        gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark => "dark",
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn appearance_override(
    options: &WindowOptions,
) -> std::result::Result<Option<gpui::WindowAppearance>, String> {
    match options.appearance.as_deref() {
        Some(value) => parse_window_appearance(value),
        None => Ok(None),
    }
}

pub(crate) fn to_gpui_window_options(
    options: &WindowOptions,
    bounds: gpui::Bounds<gpui::Pixels>,
) -> gpui::WindowOptions {
    let title = options.title.clone().unwrap_or_else(|| "GPUIX".to_string());
    let titlebar_transparent = options.frameless();
    let traffic_light_position = match (options.traffic_light_x, options.traffic_light_y) {
        (Some(x), Some(y)) => Some(gpui::point(gpui::px(x as f32), gpui::px(y as f32))),
        _ => None,
    };
    let window_background = match options.window_background.as_deref() {
        Some("transparent") => gpui::WindowBackgroundAppearance::Transparent,
        Some("blurred") => gpui::WindowBackgroundAppearance::Blurred,
        Some("opaque") => gpui::WindowBackgroundAppearance::Opaque,
        _ if options.transparent.unwrap_or(false) => gpui::WindowBackgroundAppearance::Transparent,
        _ => gpui::WindowBackgroundAppearance::Opaque,
    };
    let window_min_size = match (options.min_width, options.min_height) {
        (Some(width), Some(height)) => {
            Some(gpui::size(gpui::px(width as f32), gpui::px(height as f32)))
        }
        _ => None,
    };
    let window_bounds = if options.fullscreen.unwrap_or(false) {
        gpui::WindowBounds::Fullscreen(bounds)
    } else {
        gpui::WindowBounds::Windowed(bounds)
    };
    #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
    let mut gpui_options = gpui::WindowOptions {
        window_bounds: Some(window_bounds),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some(title.into()),
            appears_transparent: titlebar_transparent,
            traffic_light_position,
        }),
        // AppKit would otherwise drag the window from the top strip itself and hold back clicks
        // there to tell a double click apart, and Electron only drags `appRegion` boxes.
        app_owns_titlebar_drag: titlebar_transparent,
        is_resizable: options.resizable.unwrap_or(true),
        window_background,
        window_min_size,
        focus: options.focus.unwrap_or(true),
        show: options.show.unwrap_or(true),
        app_id: options.app_id.clone(),
        ..Default::default()
    };

    // A layer-shell surface has no titlebar and is placed by the compositor
    // from its anchor, not by `window_bounds.origin`. The `WindowKind` variant
    // only exists on a Wayland-enabled Linux gpui build.
    #[cfg(target_os = "linux")]
    if let Some(layer_shell) = &options.layer_shell {
        gpui_options.titlebar = None;
        gpui_options.kind = gpui::WindowKind::LayerShell(to_layer_shell_options(layer_shell));
    }

    gpui_options
}

#[cfg(test)]
mod highlight_cache_tests {
    use super::*;

    fn tree_with_text() -> RetainedTree {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        tree.create_element(2, "text".to_string());
        tree.append_child(1, 2);
        tree.set_text(2, "a fox and a fox".to_string());
        tree
    }

    fn query(text: &str) -> serde_json::Value {
        serde_json::json!({ "query": text })
    }

    fn declare(tree: &mut RetainedTree, value: &serde_json::Value) {
        tree.set_custom_prop(1, "highlight".to_string(), value.clone());
    }

    /// The whole reason `search_revision` exists. `highlight` is a custom prop,
    /// so keying the group list on `subtree_revision` means every keystroke
    /// re-walks and re-folds the subtree. The pointer comparison is the proof;
    /// a timing budget over a realistic app is far too coarse to catch it.
    #[test]
    fn a_query_change_reuses_the_group_list() {
        let theme = Theme::dark();
        let mut tree = tree_with_text();
        let mut cache = HashMap::new();

        declare(&mut tree, &query("f"));
        resolve_highlight(&mut cache, &tree, 1, &query("f"), &theme, false).expect("resolves");
        let first = Arc::as_ptr(&cache[&1].groups);

        declare(&mut tree, &query("fo"));
        resolve_highlight(&mut cache, &tree, 1, &query("fo"), &theme, false).expect("resolves");
        assert_eq!(
            Arc::as_ptr(&cache[&1].groups),
            first,
            "a query change must not rebuild the group list"
        );
    }

    /// Moving a find cursor changes no text and no matcher, so it must re-use
    /// the located matches. Colours and ordinals are decided at paint.
    #[test]
    fn a_cursor_move_reuses_the_located_matches() {
        let theme = Theme::dark();
        let mut tree = tree_with_text();
        let mut cache = HashMap::new();
        let spec = |active: u64| serde_json::json!({ "query": "fox", "activeIndex": active });

        declare(&mut tree, &spec(0));
        resolve_highlight(&mut cache, &tree, 1, &spec(0), &theme, true).expect("resolves");
        let matches = Arc::as_ptr(&cache[&1].context.matches);

        declare(&mut tree, &spec(1));
        let (context, changed) =
            resolve_highlight(&mut cache, &tree, 1, &spec(1), &theme, true).expect("resolves");
        assert_eq!(Arc::as_ptr(&context.matches), matches, "no rescan");
        assert_eq!(changed, None, "a cursor move is not a new result");
        assert_eq!(
            context.set.specs[0].active_index,
            Some(1),
            "spec still swapped"
        );
    }

    /// Editing the text must invalidate, or the wash paints over stale offsets.
    #[test]
    fn a_text_change_rebuilds_the_group_list() {
        let theme = Theme::dark();
        let mut tree = tree_with_text();
        let mut cache = HashMap::new();

        declare(&mut tree, &query("fox"));
        resolve_highlight(&mut cache, &tree, 1, &query("fox"), &theme, true).expect("resolves");
        let first = Arc::as_ptr(&cache[&1].groups);

        tree.set_text(2, "one fox only".to_string());
        let (_, changed) =
            resolve_highlight(&mut cache, &tree, 1, &query("fox"), &theme, true).expect("resolves");
        assert_ne!(Arc::as_ptr(&cache[&1].groups), first);
        assert_eq!(changed, Some(1), "two matches became one");
    }

    /// A review caught this: `reported` used to be written even with no
    /// listener, so mounting without `onHighlight` and adding it later reported
    /// nothing, forever.
    #[test]
    fn adding_the_listener_later_still_reports() {
        let theme = Theme::dark();
        let mut tree = tree_with_text();
        let mut cache = HashMap::new();

        declare(&mut tree, &query("fox"));
        let (_, changed) = resolve_highlight(&mut cache, &tree, 1, &query("fox"), &theme, false)
            .expect("resolves");
        assert_eq!(changed, None, "nothing to report without a listener");

        let (_, changed) =
            resolve_highlight(&mut cache, &tree, 1, &query("fox"), &theme, true).expect("resolves");
        assert_eq!(changed, Some(2), "the listener gets the current count");

        let (_, changed) =
            resolve_highlight(&mut cache, &tree, 1, &query("fox"), &theme, true).expect("resolves");
        assert_eq!(changed, None, "and only once");
    }
}

/// The `applyBatch` protocol. This is the surface JS talks to, so every rule it
/// relies on is asserted here against real JSON bytes rather than through a
/// hand-built `Vec<BatchOp>`.
#[cfg(test)]
mod batch_tests {
    use super::*;

    fn apply(tree: &mut RetainedTree, json: &str) -> BatchResult<Vec<f64>> {
        apply_batch_to_tree(tree, json.as_bytes())
    }

    /// Everything a mutation can reach, so an unwanted partial apply shows up
    /// as a diff instead of hiding in a field the test forgot to read.
    fn describe(tree: &RetainedTree) -> String {
        let mut ids: Vec<_> = tree.elements.keys().copied().collect();
        ids.sort_unstable();
        let mut out = format!("root={:?}\n", tree.root_id);
        for id in ids {
            let element = &tree.elements[&id];
            let mut events: Vec<_> = element.events().iter().cloned().collect();
            events.sort();
            let mut props: Vec<_> = element.custom_props().iter().collect();
            props.sort_by(|(a, _), (b, _)| a.cmp(b));
            out += &format!(
                "{id} type={} text={:?} style={:?} children={:?} parent={:?} events={events:?} props={props:?} rev={}/{}\n",
                element.element_type,
                element.content,
                element.style.as_deref(),
                element.children,
                element.parent,
                element.subtree_revision,
                element.search_revision,
            );
        }
        out
    }

    /// Whether the batch left something a frame would draw differently. The
    /// window is only woken for these, so a wrong `false` is a stale screen and
    /// a wrong `true` is a wasted frame.
    fn dirty_after(tree: &mut RetainedTree, json: &str) -> bool {
        apply(tree, json).expect("valid batch");
        tree.take_dirty()
    }

    const MOUNT: &str = r##"[
        ["createElement",1,"div"],["createElement",2,"text"],["createElement",3,"div"],
        ["appendChild",1,2],["appendChild",1,3],["setRoot",1],
        ["setStyle",1,{"width":100,"backgroundColor":"#112233"}],
        ["setStyle",3,{"height":10}],
        ["setText",2,"hello"],
        ["setEventListener",3,"click",true],
        ["setCustomProp",3,"aria-label","row"],
        ["setCustomProp",3,"testId","row"],
        ["setCustomProp",3,"autoFocus",true]
    ]"##;

    #[test]
    fn mounting_marks_the_tree_dirty() {
        let mut tree = RetainedTree::new();
        assert!(dirty_after(&mut tree, MOUNT));
    }

    #[test]
    fn resending_everything_unchanged_is_not_dirty() {
        let mut tree = RetainedTree::new();
        apply(&mut tree, MOUNT).expect("valid batch");
        tree.take_dirty();

        // The same ops again, with the style keys reordered: interning keys on
        // raw bytes, so this reaches `set_style` as a different `Arc`.
        let resend = r##"[
            ["setRoot",1],
            ["setStyle",1,{"backgroundColor":"#112233","width":100}],
            ["setStyle",3,{"height":10}],
            ["setText",2,"hello"],
            ["setEventListener",3,"click",true],
            ["setCustomProp",3,"aria-label","row"],
            ["setCustomProp",3,"testId","row"],
            ["setCustomProp",3,"autoFocus",true],
            ["setKeyboardFocusDim",true],
            ["setCustomProp",3,"missing",null]
        ]"##;
        assert!(!dirty_after(&mut tree, resend));
    }

    #[test]
    fn each_kind_of_change_is_dirty() {
        let changes = [
            r#"[["setStyle",3,{"height":11}]]"#,
            r#"[["setText",2,"changed"]]"#,
            r#"[["setEventListener",3,"click",false]]"#,
            r#"[["setEventListener",3,"mouseDown",true]]"#,
            r#"[["setCustomProp",3,"aria-label","other"]]"#,
            r#"[["setCustomProp",3,"aria-label",null]]"#,
            r#"[["setCustomProp",3,"testId","other"]]"#,
            r#"[["setCustomProp",3,"autoFocus",false]]"#,
            r#"[["setKeyboardFocusDim",false]]"#,
            r#"[["createElement",4,"div"]]"#,
            r#"[["destroyElement",3]]"#,
            r#"[["insertBefore",1,3,2]]"#,
            r#"[["setRoot",3]]"#,
        ];
        for change in changes {
            let mut tree = RetainedTree::new();
            apply(&mut tree, MOUNT).expect("valid batch");
            tree.take_dirty();
            assert!(dirty_after(&mut tree, change), "{change} must repaint");
        }
    }

    #[test]
    fn a_rejected_batch_does_not_leave_the_tree_dirty() {
        let mut tree = RetainedTree::new();
        apply(&mut tree, MOUNT).expect("valid batch");
        tree.take_dirty();

        apply(&mut tree, r#"[["setText",2,"changed"],["setStyle",2,123]]"#)
            .expect_err("malformed style");

        assert!(!tree.take_dirty());
    }

    /// The regression test for batch atomicity. `intern_style_payload` used to
    /// run inside the apply loop, so this batch created the element, set its
    /// text, and only then threw — leaving JS to retry against a tree that had
    /// already moved.
    #[test]
    fn a_malformed_style_applies_nothing_at_all() {
        let mut tree = RetainedTree::new();
        apply(&mut tree, r#"[["createElement",1,"div"],["setRoot",1]]"#).expect("valid batch");
        let before = describe(&tree);
        let styles_before = tree.styles.len();

        let error = apply(
            &mut tree,
            r#"[["createElement",2,"div"],["setText",2,"changed"],["setStyle",2,123]]"#,
        )
        .expect_err("a malformed style must reject the batch");

        assert_eq!(describe(&tree), before, "the tree must be untouched");
        assert_eq!(
            tree.styles.len(),
            styles_before,
            "the failed batch must not leave styles interned"
        );
        assert!(error.contains("setStyle"), "{error}");
    }

    /// A style that fails halfway through a long batch is unfindable without
    /// its index; serde reports a byte offset, which names nothing.
    #[test]
    fn a_style_error_names_its_op_index() {
        let mut tree = RetainedTree::new();
        let error = apply(
            &mut tree,
            r#"[["createElement",1,"div"],["setStyle",1,{"color":"red"}],["setStyle",1,{"color":5}]]"#,
        )
        .expect_err("a bad style rejects the batch");
        assert!(
            error.starts_with("Batch op 2 setStyle parse error:"),
            "{error}"
        );
    }

    /// `null` is not "no style". Treating it as `{}` would silently clear every
    /// declared property instead of telling JS it sent something wrong.
    #[test]
    fn a_null_style_is_an_error() {
        let mut tree = RetainedTree::new();
        let error = apply(
            &mut tree,
            r#"[["createElement",1,"div"],["setStyle",1,null]]"#,
        )
        .expect_err("null is not a style");
        assert!(
            error.contains("Batch op 1 setStyle parse error:"),
            "{error}"
        );
        assert!(tree.elements.is_empty(), "and the batch stays atomic");
    }

    /// Skipping an unknown opcode would let a JS/Rust version skew desync the
    /// tree quietly. It has to throw.
    #[test]
    fn an_unknown_opcode_is_an_error() {
        let mut tree = RetainedTree::new();
        let error = apply(&mut tree, r#"[["teleportElement",1]]"#).expect_err("unknown opcode");
        assert!(error.contains("unknown operation"), "{error}");
        assert!(tree.elements.is_empty());
    }

    /// Every op that takes an id must validate it. A fractional or oversized id
    /// would truncate into a *different* element, which is a silent desync.
    #[test]
    fn an_invalid_id_is_rejected_in_every_id_position() {
        let templates = [
            r#"[["createElement",ID,"div"]]"#,
            r#"[["destroyElement",ID]]"#,
            r#"[["appendChild",ID,2]]"#,
            r#"[["appendChild",1,ID]]"#,
            r#"[["insertBefore",ID,2,3]]"#,
            r#"[["insertBefore",1,ID,3]]"#,
            r#"[["insertBefore",1,2,ID]]"#,
            r#"[["setStyle",ID,{}]]"#,
            r#"[["setText",ID,"x"]]"#,
            r#"[["setEventListener",ID,"click",true]]"#,
            r#"[["setRoot",ID]]"#,
            r#"[["setCustomProp",ID,"k",1]]"#,
        ];
        // 1e999 overflows f64, 9007199254740992 is Number.MAX_SAFE_INTEGER + 1.
        let bad_ids = ["-1", "1.5", "9007199254740992", "1e999"];

        for template in templates {
            for bad in bad_ids {
                let json = template.replace("ID", bad);
                let mut tree = RetainedTree::new();
                let error = apply(&mut tree, &json).expect_err(&format!("{json} must be rejected"));
                assert!(error.contains("Batch op 0"), "{json}: {error}");
                assert!(tree.elements.is_empty(), "{json} mutated the tree");
                assert_eq!(tree.root_id, None, "{json} mutated the root");
            }
        }
    }

    #[test]
    fn has_handler_requires_a_bool() {
        for (payload, expected) in [("true", true), ("false", false)] {
            let mut tree = RetainedTree::new();
            let json =
                format!(r#"[["createElement",1,"div"],["setEventListener",1,"click",{payload}]]"#);
            apply(&mut tree, &json).expect("boolean handler state");
            assert_eq!(
                tree.elements[&1].events().contains("click"),
                expected,
                "hasHandler {payload}"
            );
        }

        for payload in ["0", "1", "0.5", r#""true""#] {
            let mut tree = RetainedTree::new();
            let json =
                format!(r#"[["createElement",1,"div"],["setEventListener",1,"click",{payload}]]"#);
            apply(&mut tree, &json).expect_err(&format!("hasHandler {payload} is not a boolean"));
        }
    }

    #[test]
    fn a_malformed_op_tuple_is_an_error() {
        let cases = [
            (r#"[42]"#, "a non-array op"),
            (r#"[["createElement",1]]"#, "a missing argument"),
            (r#"[[7,1,"div"]]"#, "a non-string op name"),
        ];
        for (json, what) in cases {
            let mut tree = RetainedTree::new();
            let error = apply(&mut tree, json).expect_err(what);
            assert!(
                error.starts_with("Failed to parse batch:"),
                "{what}: {error}"
            );
            assert!(tree.elements.is_empty(), "{what} mutated the tree");
        }
    }

    /// Interning keys on raw bytes, so re-ordered keys are two `Arc`s. They are
    /// still the same style, and a repaint per key order would be a real cost
    /// on any app that builds style objects conditionally.
    #[test]
    fn a_reordered_style_does_not_repaint() {
        let mut tree = RetainedTree::new();
        apply(
            &mut tree,
            r#"[["createElement",1,"div"],["setStyle",1,{"color":"red","left":10}]]"#,
        )
        .expect("valid batch");
        let revision = tree.elements[&1].subtree_revision;

        apply(&mut tree, r#"[["setStyle",1,{"left":10,"color":"red"}]]"#).expect("valid batch");
        assert_eq!(
            tree.elements[&1].subtree_revision, revision,
            "the same style in another key order is not a change"
        );
    }

    /// Three ways an interned style loses its last element reference.
    #[test]
    fn a_style_is_released_when_nothing_references_it() {
        let mut tree = RetainedTree::new();
        apply(&mut tree, r#"[["createElement",1,"div"]]"#).expect("valid batch");

        // Set on an id that does not exist: nothing keeps the style alive.
        apply(&mut tree, r#"[["setStyle",99,{"color":"red"}]]"#).expect("missing ids are ignored");
        tree.styles.sweep();
        assert_eq!(tree.styles.len(), 0, "a style nobody took must be released");

        apply(&mut tree, r#"[["setStyle",1,{"color":"red"}]]"#).expect("valid batch");
        tree.styles.sweep();
        assert_eq!(tree.styles.len(), 1);

        // Replaced.
        apply(&mut tree, r#"[["setStyle",1,{"color":"blue"}]]"#).expect("valid batch");
        tree.styles.sweep();
        assert_eq!(tree.styles.len(), 1, "the replaced style must be released");

        // Destroyed.
        apply(&mut tree, r#"[["destroyElement",1]]"#).expect("valid batch");
        tree.styles.sweep();
        assert_eq!(tree.styles.len(), 0);
    }
}

#[cfg(test)]
mod path_prompt_options_tests {
    use super::*;

    #[test]
    fn defaults_to_one_file_and_maps_directory_prompts() {
        let defaults = PathPromptOptions::to_gpui(None).expect("default prompt");
        assert!(defaults.files);
        assert!(!defaults.directories);
        assert!(!defaults.multiple);
        assert!(defaults.prompt.is_none());
        assert!(defaults.filters.is_empty());

        let directories = PathPromptOptions::to_gpui(Some(PathPromptOptions {
            directories: Some(true),
            multiple: Some(true),
            prompt: Some("Choose folder".to_string()),
            ..PathPromptOptions::default()
        }))
        .expect("directory prompt");
        assert!(!directories.files);
        assert!(directories.directories);
        assert!(directories.multiple);
        assert_eq!(directories.prompt.as_deref(), Some("Choose folder"));
    }

    #[test]
    fn accept_becomes_filters_and_a_typo_is_an_error() {
        let options = PathPromptOptions::to_gpui(Some(PathPromptOptions {
            accept: Some(".png,image/*".to_string()),
            ..PathPromptOptions::default()
        }))
        .expect("accept prompt");
        assert_eq!(
            options.filters,
            vec![
                gpui::PathPromptFilter::Extension("png".into()),
                gpui::PathPromptFilter::MimeType("image/*".into())
            ]
        );

        let error = PathPromptOptions::to_gpui(Some(PathPromptOptions {
            accept: Some("png".to_string()),
            ..PathPromptOptions::default()
        }))
        .expect_err("a bare word is not an accept token");
        assert!(error.reason.contains("Invalid accept token"));
    }

    #[test]
    fn rejects_a_prompt_that_selects_nothing() {
        let error = PathPromptOptions::to_gpui(Some(PathPromptOptions {
            files: Some(false),
            directories: Some(false),
            ..PathPromptOptions::default()
        }))
        .expect_err("an empty path prompt must fail");
        assert!(error.reason.contains("requires files or directories"));
    }

    #[test]
    fn mixed_selection_matches_platform_support() {
        let result = PathPromptOptions::to_gpui(Some(PathPromptOptions {
            files: Some(true),
            directories: Some(true),
            ..PathPromptOptions::default()
        }));
        if cfg!(target_os = "macos") {
            assert!(result.is_ok());
        } else {
            assert!(result
                .expect_err("mixed selection must fail")
                .reason
                .contains("only supported on macOS"));
        }
    }

    #[test]
    fn an_empty_platform_selection_is_cancellation() {
        assert_eq!(path_prompt_result(Ok(Some(Vec::new()))).unwrap(), None);
    }
}

#[cfg(test)]
mod window_options_tests {
    use super::*;

    fn mapped(options: WindowOptions) -> gpui::WindowOptions {
        let bounds = gpui::Bounds {
            origin: gpui::point(gpui::px(0.0), gpui::px(0.0)),
            size: gpui::size(gpui::px(800.0), gpui::px(600.0)),
        };
        to_gpui_window_options(&options, bounds)
    }

    #[test]
    fn defaults_open_a_focused_visible_window() {
        let gpui_options = mapped(WindowOptions::default());
        assert!(gpui_options.focus);
        assert!(gpui_options.show);
    }

    #[test]
    fn unset_focus_and_show_still_default_to_true() {
        let gpui_options = mapped(WindowOptions {
            focus: None,
            show: None,
            ..WindowOptions::default()
        });
        assert!(gpui_options.focus);
        assert!(gpui_options.show);
    }

    #[test]
    fn focus_false_leaves_the_window_visible() {
        let gpui_options = mapped(WindowOptions {
            focus: Some(false),
            ..WindowOptions::default()
        });
        assert!(!gpui_options.focus);
        assert!(gpui_options.show);
    }

    #[test]
    fn show_false_keeps_focus_independent() {
        let gpui_options = mapped(WindowOptions {
            show: Some(false),
            ..WindowOptions::default()
        });
        assert!(!gpui_options.show);
        assert!(gpui_options.focus);
    }

    #[test]
    fn existing_options_are_still_mapped() {
        let gpui_options = mapped(WindowOptions {
            title: Some("Background".to_string()),
            resizable: Some(false),
            window_background: Some("blurred".to_string()),
            min_width: Some(320.0),
            min_height: Some(240.0),
            focus: Some(false),
            ..WindowOptions::default()
        });
        let titlebar = gpui_options.titlebar.expect("titlebar options");
        assert_eq!(titlebar.title.as_deref(), Some("Background"));
        assert!(!gpui_options.is_resizable);
        assert_eq!(
            gpui_options.window_background,
            gpui::WindowBackgroundAppearance::Blurred
        );
        assert_eq!(
            gpui_options.window_min_size,
            Some(gpui::size(gpui::px(320.0), gpui::px(240.0)))
        );
    }

    #[test]
    fn appearance_parses_system_light_and_dark() {
        assert_eq!(parse_window_appearance("system"), Ok(None));
        assert_eq!(
            parse_window_appearance("light"),
            Ok(Some(gpui::WindowAppearance::Light))
        );
        assert_eq!(
            parse_window_appearance("dark"),
            Ok(Some(gpui::WindowAppearance::Dark))
        );
    }

    #[test]
    fn appearance_rejects_anything_else() {
        for value in ["", "Dark", "auto", "vibrantDark"] {
            assert!(parse_window_appearance(value).is_err(), "{value:?}");
        }
    }

    #[test]
    fn missing_appearance_option_follows_the_system() {
        assert_eq!(appearance_override(&WindowOptions::default()), Ok(None));
        let invalid = WindowOptions {
            appearance: Some("sepia".to_string()),
            ..WindowOptions::default()
        };
        assert!(appearance_override(&invalid).is_err());
    }

    #[test]
    fn vibrant_appearances_collapse_to_light_and_dark() {
        use gpui::WindowAppearance::*;
        assert_eq!(appearance_name(Light), "light");
        assert_eq!(appearance_name(VibrantLight), "light");
        assert_eq!(appearance_name(Dark), "dark");
        assert_eq!(appearance_name(VibrantDark), "dark");
    }

    #[test]
    fn app_id_is_forwarded() {
        let gpui_options = mapped(WindowOptions {
            app_id: Some("kite-panel".to_string()),
            ..WindowOptions::default()
        });
        assert_eq!(gpui_options.app_id.as_deref(), Some("kite-panel"));
    }

    #[test]
    fn without_layer_shell_the_window_is_normal_with_a_titlebar() {
        let gpui_options = mapped(WindowOptions::default());
        assert_eq!(gpui_options.kind, gpui::WindowKind::Normal);
        assert!(gpui_options.titlebar.is_some());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn layer_shell_options_map_to_a_layer_shell_window_kind() {
        use gpui::layer_shell::{Anchor, KeyboardInteractivity, Layer};

        let gpui_options = mapped(WindowOptions {
            app_id: Some("kite-panel".to_string()),
            layer_shell: Some(LayerShellOptions {
                namespace: Some("kite-panel".to_string()),
                layer: Some("top".to_string()),
                anchor: Some(vec![
                    "top".to_string(),
                    "left".to_string(),
                    "right".to_string(),
                ]),
                exclusive_zone: Some(34.0),
                exclusive_edge: Some("top".to_string()),
                margin: Some(vec![0.0, 0.0, 0.0, 0.0]),
                keyboard_interactivity: Some("none".to_string()),
            }),
            ..WindowOptions::default()
        });

        assert!(gpui_options.titlebar.is_none());
        match gpui_options.kind {
            gpui::WindowKind::LayerShell(opts) => {
                assert_eq!(opts.namespace, "kite-panel");
                assert_eq!(opts.layer, Layer::Top);
                assert_eq!(opts.anchor, Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
                assert_eq!(opts.exclusive_zone, Some(gpui::px(34.0)));
                assert_eq!(opts.exclusive_edge, Some(Anchor::TOP));
                assert_eq!(
                    opts.margin,
                    Some((gpui::px(0.0), gpui::px(0.0), gpui::px(0.0), gpui::px(0.0)))
                );
                assert_eq!(opts.keyboard_interactivity, KeyboardInteractivity::None);
            }
            other => panic!("expected a layer-shell window kind, got {other:?}"),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn layer_shell_anchor_defaults_to_a_top_bar() {
        use gpui::layer_shell::Anchor;

        let gpui_options = mapped(WindowOptions {
            layer_shell: Some(LayerShellOptions::default()),
            ..WindowOptions::default()
        });
        match gpui_options.kind {
            gpui::WindowKind::LayerShell(opts) => {
                assert_eq!(opts.anchor, Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
            }
            other => panic!("expected a layer-shell window kind, got {other:?}"),
        }
    }
}

/// Rejects fonts GPUI's text system cannot load. It unwraps the family, full and
/// PostScript name records and aborts the whole process when one is missing, and
/// skips a font without an "m" glyph, which would leave the family unresolvable.
fn check_font(bytes: &[u8]) -> std::result::Result<(), String> {
    use ttf_parser::name_id::{FAMILY, FULL_NAME, POST_SCRIPT_NAME};

    let face = ttf_parser::Face::parse(bytes, 0)
        .map_err(|error| format!("not a TrueType or OpenType font: {error}"))?;
    for (id, what) in [
        (FAMILY, "family"),
        (FULL_NAME, "full"),
        (POST_SCRIPT_NAME, "PostScript"),
    ] {
        let named = face
            .names()
            .into_iter()
            .any(|name| name.name_id == id && name.to_string().is_some());
        if !named {
            return Err(format!("font has no {what} name"));
        }
    }
    if face.glyph_index('m').is_none() {
        return Err("font has no glyph for \"m\"".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod inherited_pointer_events_tests {
    use super::*;

    fn style(pointer_events: &str) -> StyleDesc {
        StyleDesc {
            pointer_events: Some(pointer_events.to_string()),
            ..Default::default()
        }
    }

    fn root() -> Inherited {
        Inherited::root(&Theme::dark(), gpui::WindowAppearance::Dark, true, TEST_VIEWPORT)
    }

    #[test]
    fn auto_reopens_a_pointer_events_none_subtree() {
        let below_none = root().descend(Some(&style("none")), false);
        assert!(below_none.pointer_events_none);
        assert!(!below_none.descend(Some(&style("auto")), false).pointer_events_none);
    }

    #[test]
    fn auto_does_not_reopen_an_inert_subtree() {
        let below_inert = root().descend(None, true);
        assert!(below_inert.pointer_events_none);
        let below_auto = below_inert.descend(Some(&style("auto")), false);
        assert!(below_auto.pointer_events_none);
        assert!(below_auto.descend(None, false).pointer_events_none);
    }

    #[test]
    fn an_inert_element_with_pointer_events_auto_is_not_hit_itself() {
        assert!(root().descend(Some(&style("auto")), true).pointer_events_none);
    }
}

#[cfg(test)]
mod inherited_app_region_tests {
    use super::*;

    fn style(app_region: &str) -> StyleDesc {
        StyleDesc {
            app_region: Some(app_region.to_string()),
            ..Default::default()
        }
    }

    fn frameless() -> Inherited {
        Inherited::root(&Theme::dark(), gpui::WindowAppearance::Dark, true, TEST_VIEWPORT)
    }

    #[test]
    fn a_region_reaches_every_descendant_and_the_nearest_one_wins() {
        let drag = frameless().descend(Some(&style("drag")), false);
        assert_eq!(drag.app_region, Some(AppRegion::Drag));
        let no_drag = drag.descend(Some(&style("no-drag")), false);
        assert_eq!(no_drag.app_region, Some(AppRegion::NoDrag));
        assert_eq!(
            no_drag.clone().descend(None, false).app_region,
            Some(AppRegion::NoDrag)
        );
        assert_eq!(
            no_drag.descend(Some(&style("drag")), false).app_region,
            Some(AppRegion::Drag)
        );
    }

    #[test]
    fn an_invalid_value_leaves_the_inherited_one() {
        let drag = frameless().descend(Some(&style("drag")), false);
        assert_eq!(
            drag.descend(Some(&style("none")), false).app_region,
            Some(AppRegion::Drag)
        );
        assert_eq!(
            frameless().descend(Some(&style("grab")), false).app_region,
            None
        );
    }

    #[test]
    fn a_window_with_a_titlebar_has_no_regions() {
        let framed = Inherited::root(&Theme::dark(), gpui::WindowAppearance::Dark, false, TEST_VIEWPORT);
        assert_eq!(framed.descend(Some(&style("drag")), false).app_region, None);
    }

    #[test]
    fn an_inert_subtree_keeps_its_region() {
        // `inert` is about input to the page; the window still moves from it, as in Electron.
        let inert = frameless().descend(Some(&style("drag")), true);
        assert_eq!(inert.app_region, Some(AppRegion::Drag));
    }
}


#[cfg(test)]
const TEST_VIEWPORT: gpui::Size<gpui::Pixels> = gpui::Size {
    width: gpui::px(800.0),
    height: gpui::px(600.0),
};

/// The computed `font-size` through a real build, read back as the height of a
/// line of text: gpui's line height is a fixed multiple of the font size.
#[cfg(test)]
mod font_size_cascade_tests {
    use super::*;

    const CALIBRATION: u64 = 9999;
    /// Line heights are rounded to the pixel grid.
    const TOLERANCE: f64 = 0.5;

    struct Node {
        id: u64,
        element_type: &'static str,
        parent: Option<u64>,
        style: &'static str,
    }

    fn node(id: u64, parent: Option<u64>, style: &'static str) -> Node {
        Node { id, element_type: "div", parent, style }
    }

    fn virtual_list(id: u64, parent: Option<u64>, style: &'static str) -> Node {
        Node { id, element_type: "virtual-list", parent, style }
    }

    fn probe(id: u64) -> u64 {
        id + 20_000
    }

    struct Page {
        tree: Arc<Mutex<RetainedTree>>,
        window: gpui::WindowHandle<GpuixView>,
        height_per_px: f64,
    }

    impl Page {
        /// The first node is the root. Each node gets a style-less line of text
        /// that inherits whatever size the node computes.
        fn new(cx: &mut gpui::TestAppContext, nodes: &[Node]) -> Self {
            let tree = Arc::new(Mutex::new(RetainedTree::new()));
            let root = nodes[0].id;
            {
                let mut tree = tree.lock().unwrap();
                let calibration = node(CALIBRATION, Some(root), r#"{"fontSize": 100}"#);
                for node in nodes.iter().chain([&calibration]) {
                    let style: StyleDesc = serde_json::from_str(node.style).unwrap();
                    tree.create_element(node.id, node.element_type.to_owned());
                    tree.set_style(node.id, Arc::new(style));
                    tree.create_element(probe(node.id), "div".to_owned());
                    tree.create_element(probe(node.id) + 10_000, "text".to_owned());
                    tree.set_text(probe(node.id) + 10_000, "m".to_owned());
                    tree.append_child(probe(node.id), probe(node.id) + 10_000);
                    tree.append_child(node.id, probe(node.id));
                    if let Some(parent) = node.parent {
                        tree.append_child(parent, node.id);
                    }
                }
                tree.set_root(root);
            }
            let view_tree = tree.clone();
            let window = cx.add_window(|window, _| {
                crate::events::enable_dom_hit_testing(window);
                GpuixView::new(
                    view_tree,
                    None,
                    "test".to_owned(),
                    SharedSelection::default(),
                    crate::events::state::PointerRequests::default(),
                )
            });
            let mut page = Self { tree, window, height_per_px: 0.0 };
            page.draw(cx);
            page.height_per_px = page.height(probe(CALIBRATION)) / 100.0;
            assert!(page.height_per_px > 0.0, "the calibration text has no height");
            page
        }

        fn draw(&self, cx: &mut gpui::TestAppContext) {
            cx.update_window(self.window.into(), |_, window, app| {
                window.refresh();
                window.draw(app).clear(app);
            })
            .unwrap();
        }

        fn height(&self, id: u64) -> f64 {
            crate::automation::get_bounds(id)
                .unwrap_or_else(|| panic!("element {id} was not painted"))
                .height
        }

        /// The computed font size of node `id`, from its probe.
        fn font_size(&self, id: u64) -> f64 {
            self.height(probe(id)) / self.height_per_px
        }

        fn assert_font_sizes(&self, expected: &[(u64, f64)]) {
            for (id, size) in expected {
                let measured = self.font_size(*id);
                assert!(
                    (measured - size).abs() <= TOLERANCE,
                    "node {id} should compute {size}px but measures {measured}px"
                );
            }
        }

        fn set_style(&self, cx: &mut gpui::TestAppContext, id: u64, style: &str) {
            let style: StyleDesc = serde_json::from_str(style).unwrap();
            self.tree.lock().unwrap().set_style(id, Arc::new(style));
            self.draw(cx);
        }
    }

    #[gpui::test]
    fn a_page_without_a_font_size_uses_the_default(cx: &mut gpui::TestAppContext) {
        let page = Page::new(cx, &[node(1, None, "{}"), node(2, Some(1), "{}")]);
        page.assert_font_sizes(&[(2, crate::css::length::INITIAL_FONT_SIZE)]);
    }

    #[gpui::test]
    fn em_is_relative_to_the_parent_and_compounds(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                node(1, None, r#"{"fontSize": 10}"#),
                node(2, Some(1), r#"{"fontSize": "2em"}"#),
                node(3, Some(2), r#"{"fontSize": "1.5em"}"#),
                node(4, Some(3), "{}"),
            ],
        );
        // A child that declares nothing inherits the computed size.
        page.assert_font_sizes(&[(1, 10.0), (2, 20.0), (3, 30.0), (4, 30.0)]);
    }

    #[gpui::test]
    fn a_percentage_is_a_fraction_of_the_parents_size(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                node(1, None, r#"{"fontSize": 20}"#),
                node(2, Some(1), r#"{"fontSize": "150%"}"#),
                node(3, Some(2), r#"{"fontSize": "50%"}"#),
            ],
        );
        page.assert_font_sizes(&[(2, 30.0), (3, 15.0)]);
    }

    #[gpui::test]
    fn rem_is_the_initial_size_until_the_root_sets_one(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                node(1, None, "{}"),
                node(2, Some(1), r#"{"fontSize": "2rem"}"#),
                node(3, Some(2), r#"{"fontSize": "0.5rem"}"#),
            ],
        );
        page.assert_font_sizes(&[(2, 32.0), (3, 8.0)]);
    }

    #[gpui::test]
    fn rem_follows_the_font_size_the_root_sets(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                node(1, None, r#"{"fontSize": "10px"}"#),
                node(2, Some(1), r#"{"fontSize": "3rem"}"#),
                node(3, Some(2), r#"{"fontSize": "1rem"}"#),
            ],
        );
        page.assert_font_sizes(&[(2, 30.0), (3, 10.0)]);
    }

    #[gpui::test]
    fn the_roots_own_rem_is_the_initial_size(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[node(1, None, r#"{"fontSize": "2rem"}"#), node(2, Some(1), r#"{"fontSize": "1rem"}"#)],
        );
        page.assert_font_sizes(&[(1, 32.0), (2, 32.0)]);
    }

    #[gpui::test]
    fn viewport_units_in_a_font_size_follow_a_resize(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                node(1, None, "{}"),
                node(2, Some(1), r#"{"fontSize": "calc(1rem + 2vw)"}"#),
                node(3, Some(2), r#"{"fontSize": "10vh"}"#),
            ],
        );
        let width = cx
            .update_window(page.window.into(), |_, window, _| f64::from(f32::from(window.viewport_size().width)))
            .unwrap();
        page.assert_font_sizes(&[(2, 16.0 + width * 0.02)]);
        cx.simulate_window_resize(page.window.into(), gpui::size(gpui::px(500.0), gpui::px(300.0)));
        page.draw(cx);
        page.assert_font_sizes(&[(2, 16.0 + 500.0 * 0.02), (3, 30.0)]);
    }

    #[gpui::test]
    fn a_new_style_changes_the_size_children_resolve_against(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[node(1, None, r#"{"fontSize": 20}"#), node(2, Some(1), r#"{"fontSize": "2em"}"#)],
        );
        page.assert_font_sizes(&[(2, 40.0)]);
        page.set_style(cx, 1, r#"{"fontSize": 10}"#);
        page.assert_font_sizes(&[(2, 20.0)]);
    }

    #[gpui::test]
    fn a_hover_font_size_changes_what_the_children_resolve_against(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                node(1, None, r#"{"fontSize": 20}"#),
                node(2, Some(1), r#"{"fontSize": "1em", "hover": {"fontSize": "2em"}}"#),
                node(3, Some(2), r#"{"fontSize": "1.5em"}"#),
            ],
        );
        page.assert_font_sizes(&[(2, 20.0), (3, 30.0)]);
        let bounds = crate::automation::get_bounds(2).unwrap();
        let inside = gpui::point(
            gpui::px((bounds.x + bounds.width / 2.0) as f32),
            gpui::px((bounds.y + 1.0) as f32),
        );
        gpui::VisualTestContext::from_window(page.window.into(), cx).simulate_mouse_move(
            inside,
            None,
            Default::default(),
        );
        page.draw(cx);
        page.assert_font_sizes(&[(2, 40.0), (3, 60.0)]);
    }

    /// The height of node `id`'s one-line probe, which is its line height.
    fn assert_line_heights(page: &Page, expected: &[(u64, f64)]) {
        for (id, height) in expected {
            let measured = page.height(probe(*id));
            assert!(
                (measured - height).abs() <= TOLERANCE,
                "node {id} should have a {height}px line height but measures {measured}px"
            );
        }
    }

    #[gpui::test]
    fn a_line_height_number_multiplies_the_font_size(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                node(1, None, "{}"),
                node(2, Some(1), r#"{"fontSize": 10, "lineHeight": 2}"#),
                node(3, Some(1), r#"{"fontSize": 10, "lineHeight": "1.5"}"#),
                node(4, Some(1), r#"{"fontSize": 10, "lineHeight": 0.5}"#),
            ],
        );
        assert_line_heights(&page, &[(2, 20.0), (3, 15.0), (4, 5.0)]);
    }

    #[gpui::test]
    fn a_line_height_length_resolves_against_the_elements_own_font_size(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                node(1, None, r#"{"fontSize": 40}"#),
                node(2, Some(1), r#"{"fontSize": 10, "lineHeight": "24px"}"#),
                node(3, Some(1), r#"{"fontSize": 10, "lineHeight": "150%"}"#),
                node(4, Some(1), r#"{"fontSize": 10, "lineHeight": "1.5em"}"#),
                node(5, Some(1), r#"{"fontSize": "0.5em", "lineHeight": "2em"}"#),
                node(6, Some(1), r#"{"fontSize": "0.5em", "lineHeight": "200%"}"#),
                node(7, Some(1), r#"{"lineHeight": "1.5em"}"#),
                node(8, Some(1), r#"{"fontSize": 10, "lineHeight": "calc(1em + 2rem)"}"#),
            ],
        );
        assert_line_heights(&page, &[(2, 24.0), (3, 15.0), (4, 15.0), (5, 40.0), (6, 40.0), (7, 60.0), (8, 90.0)]);
    }

    #[gpui::test]
    fn a_multiplier_is_inherited_as_a_multiplier_and_a_length_as_px(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                node(1, None, "{}"),
                node(2, Some(1), r#"{"fontSize": 10, "lineHeight": 2}"#),
                node(3, Some(2), r#"{"fontSize": 20}"#),
                node(4, Some(1), r#"{"fontSize": 10, "lineHeight": "24px"}"#),
                node(5, Some(4), r#"{"fontSize": 20}"#),
                node(6, Some(1), r#"{"fontSize": 10, "lineHeight": "150%"}"#),
                node(7, Some(6), r#"{"fontSize": 20}"#),
                node(8, Some(1), r#"{"fontSize": 10, "lineHeight": "1.5em"}"#),
                node(9, Some(8), r#"{"fontSize": "3em"}"#),
            ],
        );
        assert_line_heights(&page, &[(3, 40.0), (5, 24.0), (7, 15.0), (9, 15.0)]);
    }

    #[gpui::test]
    fn a_normal_line_height_undoes_what_a_parent_set(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                node(1, None, r#"{"fontSize": 10}"#),
                node(2, Some(1), r#"{"lineHeight": "24px"}"#),
                node(3, Some(2), r#"{"lineHeight": "normal"}"#),
                node(4, Some(1), "{}"),
                node(5, Some(1), r#"{"lineHeight": 3}"#),
                node(6, Some(5), r#"{"lineHeight": "normal"}"#),
            ],
        );
        let default = page.height(probe(4));
        assert_line_heights(&page, &[(2, 24.0), (3, default), (5, 30.0), (6, default)]);
    }

    #[gpui::test]
    fn changing_a_line_height_relays_the_text_out(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[node(1, None, "{}"), node(2, Some(1), r#"{"fontSize": 10, "lineHeight": 2}"#)],
        );
        assert_line_heights(&page, &[(2, 20.0)]);
        page.set_style(cx, 2, r#"{"fontSize": 10, "lineHeight": "30px"}"#);
        assert_line_heights(&page, &[(2, 30.0)]);
        page.set_style(cx, 2, r#"{"fontSize": 10}"#);
        assert_line_heights(&page, &[(2, 16.0)]);
    }

    #[gpui::test]
    fn a_hover_line_height_applies_while_hovered(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                node(1, None, "{}"),
                node(
                    2,
                    Some(1),
                    r#"{"fontSize": 10, "lineHeight": 2, "hover": {"lineHeight": "3em"}}"#,
                ),
            ],
        );
        assert_line_heights(&page, &[(2, 20.0)]);
        let bounds = crate::automation::get_bounds(2).unwrap();
        let inside = gpui::point(
            gpui::px((bounds.x + bounds.width / 2.0) as f32),
            gpui::px((bounds.y + 1.0) as f32),
        );
        gpui::VisualTestContext::from_window(page.window.into(), cx).simulate_mouse_move(
            inside,
            None,
            Default::default(),
        );
        page.draw(cx);
        assert_line_heights(&page, &[(2, 30.0)]);
    }

    fn custom(id: u64, parent: u64, element_type: &'static str, style: &'static str) -> Node {
        Node { id, element_type, parent: Some(parent), style }
    }

    fn element_height(id: u64) -> f64 {
        crate::automation::get_bounds(id)
            .unwrap_or_else(|| panic!("element {id} was not painted"))
            .height
    }

    #[gpui::test]
    fn a_code_block_rows_follow_the_line_height(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                node(1, None, "{}"),
                custom(2, 1, "code", r#"{"fontSize": 10, "lineHeight": 2}"#),
                custom(3, 1, "code", r#"{"fontSize": 10, "lineHeight": "30px"}"#),
                custom(4, 1, "code", r#"{"fontSize": 10, "lineHeight": "3em"}"#),
            ],
        );
        for id in [2, 3, 4] {
            page.tree.lock().unwrap().set_custom_prop(id, "code".to_owned(), serde_json::json!("a\nb"));
        }
        page.draw(cx);
        assert_eq!(element_height(2), 40.0);
        assert_eq!(element_height(3), 60.0);
        assert_eq!(element_height(4), 60.0);
    }

    #[gpui::test]
    fn a_textarea_row_follows_the_line_height(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                node(1, None, "{}"),
                custom(2, 1, "textarea", r#"{"fontSize": 10, "lineHeight": 2}"#),
                custom(3, 1, "textarea", r#"{"fontSize": 10, "lineHeight": "30px"}"#),
                custom(4, 1, "textarea", r#"{"fontSize": 10, "lineHeight": "3em"}"#),
                custom(5, 1, "textarea", r#"{"fontSize": 10}"#),
                custom(6, 1, "textarea", r#"{"fontSize": 10, "lineHeight": "normal"}"#),
            ],
        );
        assert_eq!(element_height(2), 20.0);
        assert_eq!(element_height(3), 30.0);
        assert_eq!(element_height(4), 30.0);
        assert_eq!(element_height(6), element_height(5));
    }

    #[gpui::test]
    fn a_virtual_list_row_resolves_viewport_units_in_the_current_window(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                node(1, None, "{}"),
                virtual_list(2, Some(1), r#"{"height": 300}"#),
                node(3, Some(2), r#"{"fontSize": "10vh"}"#),
            ],
        );
        let height = cx
            .update_window(page.window.into(), |_, window, _| f64::from(f32::from(window.viewport_size().height)))
            .unwrap();
        page.assert_font_sizes(&[(3, height * 0.1)]);
        cx.simulate_window_resize(page.window.into(), gpui::size(gpui::px(500.0), gpui::px(300.0)));
        page.draw(cx);
        page.assert_font_sizes(&[(3, 30.0)]);
    }
}

/// Lengths of every unit and `calc()` on the size, margin and inset properties,
/// resolved through a real layout and read back as bounds.
#[cfg(test)]
mod dimension_length_tests {
    use super::*;

    struct Page {
        tree: Arc<Mutex<RetainedTree>>,
        window: gpui::WindowHandle<GpuixView>,
    }

    /// `nodes` are `(id, parent, style)`; the first is the root.
    impl Page {
        fn new(cx: &mut gpui::TestAppContext, nodes: &[(u64, Option<u64>, &str)]) -> Self {
            Self::with_events(cx, nodes, &[], None)
        }

        /// `listeners` are `(id, event type)`; `callback` receives what they emit.
        fn with_events(
            cx: &mut gpui::TestAppContext,
            nodes: &[(u64, Option<u64>, &str)],
            listeners: &[(u64, &str)],
            callback: Option<EventCallback>,
        ) -> Self {
            let tree = Arc::new(Mutex::new(RetainedTree::new()));
            {
                let mut tree = tree.lock().unwrap();
                for (id, parent, style) in nodes {
                    let style: StyleDesc = serde_json::from_str(style).unwrap();
                    tree.create_element(*id, "div".to_owned());
                    tree.set_style(*id, Arc::new(style));
                    if let Some(parent) = parent {
                        tree.append_child(*parent, *id);
                    }
                }
                for (id, event_type) in listeners {
                    tree.set_event_listener(*id, (*event_type).to_owned(), true);
                }
                tree.set_root(nodes[0].0);
            }
            let view_tree = tree.clone();
            let window = cx.add_window(|window, _| {
                crate::events::enable_dom_hit_testing(window);
                GpuixView::new(
                    view_tree,
                    callback,
                    "test".to_owned(),
                    SharedSelection::default(),
                    crate::events::state::PointerRequests::default(),
                )
            });
            let page = Self { tree, window };
            page.draw(cx);
            page
        }

        fn draw(&self, cx: &mut gpui::TestAppContext) {
            cx.update_window(self.window.into(), |_, window, app| {
                window.refresh();
                window.draw(app).clear(app);
            })
            .unwrap();
        }

        fn set_style(&self, cx: &mut gpui::TestAppContext, id: u64, style: &str) {
            let style: StyleDesc = serde_json::from_str(style).unwrap();
            self.tree.lock().unwrap().set_style(id, Arc::new(style));
            self.draw(cx);
        }

        /// A text node `id` with `content` inside `parent`.
        fn add_text(&self, cx: &mut gpui::TestAppContext, id: u64, parent: u64, content: &str) {
            {
                let mut tree = self.tree.lock().unwrap();
                tree.create_element(id, "text".to_owned());
                tree.set_text(id, content.to_owned());
                tree.append_child(parent, id);
            }
            self.draw(cx);
        }

        fn bounds(&self, id: u64) -> crate::automation::ElementBounds {
            crate::automation::get_bounds(id).unwrap_or_else(|| panic!("element {id} was not painted"))
        }

        /// `(width, height)` of `id`.
        fn size(&self, id: u64) -> (f64, f64) {
            let bounds = self.bounds(id);
            (bounds.width, bounds.height)
        }

        /// `(x, y)` of `id` from the top left of `from`.
        fn offset(&self, id: u64, from: u64) -> (f64, f64) {
            let (bounds, origin) = (self.bounds(id), self.bounds(from));
            (bounds.x - origin.x, bounds.y - origin.y)
        }
    }

    fn near(actual: (f64, f64), expected: (f64, f64)) {
        assert!(
            (actual.0 - expected.0).abs() < 0.01 && (actual.1 - expected.1).abs() < 0.01,
            "expected {expected:?} but got {actual:?}"
        );
    }

    const PARENT: &str = r#"{"width": 200, "height": 200}"#;

    #[gpui::test]
    fn a_percentage_with_a_length_is_resolved_against_the_containing_block(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                (1, None, PARENT),
                (2, Some(1), r#"{"width": "calc(100% - 20px)", "height": "calc(50% + 1rem)"}"#),
            ],
        );
        near(page.size(2), (180.0, 116.0));
    }

    #[gpui::test]
    fn em_follows_the_elements_font_size(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                (1, None, r#"{"width": 400, "height": 400, "fontSize": 20}"#),
                (2, Some(1), r#"{"width": "10em", "height": "calc(2em + 1rem)"}"#),
                (3, Some(1), r#"{"fontSize": "50%", "width": "10em", "height": "1em"}"#),
            ],
        );
        // The root's own 20px is the `rem` too.
        near(page.size(2), (200.0, 60.0));
        near(page.size(3), (100.0, 10.0));
    }

    #[gpui::test]
    fn rem_is_the_roots_size_and_the_absolute_units_ignore_the_font_size(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                (1, None, r#"{"width": 400, "height": 400, "fontSize": 30}"#),
                (2, Some(1), r#"{"fontSize": 10, "width": "2rem", "height": "1in"}"#),
                (3, Some(1), r#"{"width": "72pt", "height": "calc(2.54cm + 1in)"}"#),
            ],
        );
        near(page.size(2), (60.0, 96.0));
        near(page.size(3), (96.0, 192.0));
    }

    #[gpui::test]
    fn viewport_units_follow_a_resize(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[(1, None, "{}"), (2, Some(1), r#"{"width": "50vw", "height": "calc(10vh + 5vmin)"}"#)],
        );
        let viewport = cx
            .update_window(page.window.into(), |_, window, _| window.viewport_size())
            .unwrap();
        let (width, height) = (f64::from(f32::from(viewport.width)), f64::from(f32::from(viewport.height)));
        near(page.size(2), (width * 0.5, height * 0.1 + width.min(height) * 0.05));
        cx.simulate_window_resize(page.window.into(), gpui::size(gpui::px(500.0), gpui::px(300.0)));
        page.draw(cx);
        near(page.size(2), (250.0, 30.0 + 15.0));
    }

    #[gpui::test]
    fn min_and_max_sizes_clamp_with_a_calc(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                (1, None, PARENT),
                (2, Some(1), r#"{"width": 190, "height": 10, "flexShrink": 0, "maxWidth": "calc(100% - 50px)", "minHeight": "calc(25% + 1em)"}"#),
                (3, Some(1), r#"{"width": 10, "height": 400, "flexShrink": 0, "minWidth": "calc(50% + 10px)", "maxHeight": "calc(50% - 1rem)"}"#),
                (4, Some(1), r#"{"width": 400, "height": 10, "flexShrink": 0, "maxWidth": "10em"}"#),
            ],
        );
        near(page.size(2), (150.0, 66.0));
        near(page.size(3), (110.0, 84.0));
        near(page.size(4), (160.0, 10.0));
    }

    #[gpui::test]
    fn a_calc_margin_shifts_the_sibling_by_its_px_and_percentage(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                (1, None, r#"{"display": "flex", "flexDirection": "row", "width": 400, "height": 200}"#),
                (2, Some(1), r#"{"width": 50, "height": 20, "margin": "calc(5% + 4px)"}"#),
                (3, Some(1), r#"{"width": 50, "height": 20}"#),
            ],
        );
        // 5% of the containing block's width (400) is 20, so each margin is 24.
        near(page.offset(2, 1), (24.0, 24.0));
        near(page.offset(3, 1), (24.0 + 50.0 + 24.0, 0.0));
        page.set_style(cx, 2, r#"{"width": 50, "height": 20, "marginLeft": "2em", "marginTop": "2rem"}"#);
        near(page.offset(2, 1), (32.0, 32.0));
    }

    #[gpui::test]
    fn an_absolute_child_resolves_calc_insets_against_its_containing_block(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                (1, None, r#"{"position": "relative", "width": 200, "height": 200}"#),
                (
                    2,
                    Some(1),
                    r#"{"position": "absolute", "width": 10, "height": 10, "top": "calc(100% - 30px)", "left": "calc(50% + 1em)"}"#,
                ),
            ],
        );
        near(page.offset(2, 1), (116.0, 170.0));
    }

    #[gpui::test]
    fn a_hover_style_resolves_its_lengths_too(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                (1, None, PARENT),
                (2, Some(1), r#"{"width": 20, "height": 20, "hover": {"width": "calc(100% - 20px)"}}"#),
            ],
        );
        near(page.size(2), (20.0, 20.0));
        let bounds = page.bounds(2);
        gpui::VisualTestContext::from_window(page.window.into(), cx).simulate_mouse_move(
            gpui::point(gpui::px((bounds.x + 5.0) as f32), gpui::px((bounds.y + 5.0) as f32)),
            None,
            Default::default(),
        );
        page.draw(cx);
        near(page.size(2), (180.0, 20.0));
    }

    /// Element 1 is a 400px root and 2 a grid with `style` and two 10px-high items, 3 and 4.
    fn grid(cx: &mut gpui::TestAppContext, root: &str, style: &str) -> Page {
        Page::new(
            cx,
            &[
                (1, None, root),
                (2, Some(1), style),
                (3, Some(2), r#"{"height": 10}"#),
                (4, Some(2), r#"{"height": 10}"#),
            ],
        )
    }

    const GRID_ROOT: &str = r#"{"width": 400, "height": 400}"#;

    #[gpui::test]
    fn grid_tracks_in_em_follow_the_font_size_of_the_grid(cx: &mut gpui::TestAppContext) {
        let page = grid(
            cx,
            GRID_ROOT,
            r#"{"display": "grid", "width": 400, "fontSize": 10, "gridTemplateColumns": "repeat(2, 5em)"}"#,
        );
        near(page.size(3), (50.0, 10.0));
        near(page.offset(4, 2), (50.0, 0.0));
        page.set_style(
            cx,
            2,
            r#"{"display": "grid", "width": 400, "fontSize": 20, "gridTemplateColumns": "repeat(2, 5em)"}"#,
        );
        near(page.size(3), (100.0, 10.0));
        near(page.offset(4, 2), (100.0, 0.0));
    }

    #[gpui::test]
    fn grid_tracks_in_rem_follow_the_root_font_size(cx: &mut gpui::TestAppContext) {
        let page = grid(
            cx,
            r#"{"width": 400, "height": 400, "fontSize": 30}"#,
            r#"{"display": "grid", "width": 400, "fontSize": 10, "gridTemplateColumns": "minmax(2rem, 1fr) 1em"}"#,
        );
        near(page.size(3), (390.0, 10.0));
        near(page.size(4), (10.0, 10.0));
        page.set_style(cx, 2, r#"{"display": "grid", "width": 400, "gridTemplateColumns": "minmax(2rem, 1fr) 1em"}"#);
        near(page.size(4), (30.0, 10.0));
    }

    #[gpui::test]
    fn a_grid_track_calc_with_a_percentage_is_of_the_grid_width(cx: &mut gpui::TestAppContext) {
        let page = grid(
            cx,
            GRID_ROOT,
            r#"{"display": "grid", "width": 400, "gridTemplateColumns": "calc(100% - 20px) 20px"}"#,
        );
        near(page.size(3), (380.0, 10.0));
        near(page.size(4), (20.0, 10.0));
        page.set_style(cx, 2, r#"{"display": "grid", "width": 200, "gridTemplateColumns": "calc(100% - 20px) 20px"}"#);
        near(page.size(3), (180.0, 10.0));
    }

    #[gpui::test]
    fn a_grid_track_in_viewport_units_follows_a_resize(cx: &mut gpui::TestAppContext) {
        let page = grid(
            cx,
            GRID_ROOT,
            r#"{"display": "grid", "width": 400, "gridTemplateColumns": "10vw calc(5vh + 1px)"}"#,
        );
        let viewport = cx
            .update_window(page.window.into(), |_, window, _| window.viewport_size())
            .unwrap();
        let (width, height) = (f64::from(f32::from(viewport.width)), f64::from(f32::from(viewport.height)));
        near(page.size(3), (width * 0.1, 10.0));
        near(page.size(4), (height * 0.05 + 1.0, 10.0));
        cx.simulate_window_resize(page.window.into(), gpui::size(gpui::px(500.0), gpui::px(300.0)));
        page.draw(cx);
        near(page.size(3), (50.0, 10.0));
        near(page.size(4), (16.0, 10.0));
    }

    #[gpui::test]
    fn grid_auto_tracks_resolve_relative_units_but_a_number_stays_px(cx: &mut gpui::TestAppContext) {
        let page = grid(
            cx,
            r#"{"width": 400, "height": 400, "fontSize": 30}"#,
            r#"{"display": "grid", "width": 400, "fontSize": 10, "gridAutoRows": "2rem"}"#,
        );
        near(page.offset(4, 2), (0.0, 60.0));
        page.set_style(
            cx,
            2,
            r#"{"display": "grid", "width": 400, "fontSize": 10, "gridAutoFlow": "column", "gridAutoColumns": 30}"#,
        );
        near(page.size(3), (30.0, 10.0));
        near(page.offset(4, 2), (30.0, 0.0));
        page.set_style(
            cx,
            2,
            r#"{"display": "grid", "width": 400, "fontSize": 10, "gridAutoFlow": "column", "gridAutoColumns": "3em"}"#,
        );
        near(page.size(3), (30.0, 10.0));
    }

    /// Element 1 is a 200px root; element 2 has `style` and a 10px child, 3.
    fn padded(cx: &mut gpui::TestAppContext, style: &str) -> Page {
        Page::new(
            cx,
            &[
                (1, None, PARENT),
                (2, Some(1), style),
                (3, Some(2), r#"{"width": 10, "height": 10}"#),
            ],
        )
    }

    #[gpui::test]
    fn padding_with_a_percentage_is_of_the_containing_block_width(cx: &mut gpui::TestAppContext) {
        let page = padded(cx, r#"{"width": 100, "height": 100, "padding": "calc(10% + 4px)"}"#);
        near(page.offset(3, 2), (24.0, 24.0));
        page.set_style(cx, 2, r#"{"width": 100, "height": 100, "padding": "10%", "paddingTop": "calc(50% - 1em)"}"#);
        near(page.offset(3, 2), (20.0, 84.0));
    }

    #[gpui::test]
    fn padding_in_em_and_rem_follows_the_font_sizes(cx: &mut gpui::TestAppContext) {
        let page = padded(cx, r#"{"width": 100, "height": 100, "fontSize": 20, "padding": "2em"}"#);
        near(page.offset(3, 2), (40.0, 40.0));
        page.set_style(
            cx,
            2,
            r#"{"width": 100, "height": 100, "fontSize": 20, "paddingLeft": "1rem", "paddingTop": "0.5em"}"#,
        );
        near(page.offset(3, 2), (16.0, 10.0));
    }

    #[gpui::test]
    fn each_padding_side_beats_the_shorthand(cx: &mut gpui::TestAppContext) {
        let page = padded(
            cx,
            r#"{"width": 100, "height": 100, "padding": "1in", "paddingLeft": 3, "paddingTop": "2px"}"#,
        );
        near(page.offset(3, 2), (3.0, 2.0));
        near(page.size(2), (100.0, 100.0));
    }

    #[gpui::test]
    fn a_hover_padding_resolves_in_the_hover_font_size(cx: &mut gpui::TestAppContext) {
        let page = padded(
            cx,
            r#"{"width": 100, "height": 100, "fontSize": 20, "hover": {"padding": "1em"}}"#,
        );
        near(page.offset(3, 2), (0.0, 0.0));
        let bounds = page.bounds(2);
        gpui::VisualTestContext::from_window(page.window.into(), cx).simulate_mouse_move(
            gpui::point(gpui::px((bounds.x + 90.0) as f32), gpui::px((bounds.y + 90.0) as f32)),
            None,
            Default::default(),
        );
        page.draw(cx);
        near(page.offset(3, 2), (20.0, 20.0));
    }

    /// Element 2 is a 100px wide flex container with `style` and children 3, 4, 5
    /// of 40 by 20.
    fn flowing(cx: &mut gpui::TestAppContext, style: &str) -> Page {
        let item = r#"{"width": 40, "height": 20}"#;
        Page::new(
            cx,
            &[
                (1, None, PARENT),
                (2, Some(1), style),
                (3, Some(2), item),
                (4, Some(2), item),
                (5, Some(2), item),
            ],
        )
    }

    #[gpui::test]
    fn gap_is_a_length_between_the_children(cx: &mut gpui::TestAppContext) {
        let page = flowing(
            cx,
            r#"{"display": "flex", "flexDirection": "column", "width": 100, "fontSize": 10, "gap": "calc(1em + 2px)"}"#,
        );
        near(page.offset(4, 2), (0.0, 20.0 + 12.0));
        near(page.offset(5, 2), (0.0, 2.0 * (20.0 + 12.0)));
    }

    #[gpui::test]
    fn row_gap_and_column_gap_beat_gap(cx: &mut gpui::TestAppContext) {
        let page = flowing(
            cx,
            r#"{"display": "flex", "flexWrap": "wrap", "alignContent": "flex-start", "width": 100, "fontSize": 10,
                "gap": 100, "rowGap": "1rem", "columnGap": "2em"}"#,
        );
        near(page.offset(4, 2), (60.0, 0.0));
        near(page.offset(5, 2), (0.0, 20.0 + 16.0));

        page.set_style(
            cx,
            2,
            r#"{"display": "flex", "flexWrap": "wrap", "alignContent": "flex-start", "width": 100, "fontSize": 10,
                "columnGap": "2em", "gap": "1em"}"#,
        );
        near(page.offset(4, 2), (60.0, 0.0));
        near(page.offset(5, 2), (0.0, 30.0));
    }

    fn basis(cx: &mut gpui::TestAppContext, item: &str) -> Page {
        Page::new(
            cx,
            &[
                (1, None, r#"{"display": "flex", "width": 200, "height": 50}"#),
                (2, Some(1), item),
            ],
        )
    }

    #[gpui::test]
    fn flex_basis_takes_a_length_percentage_or_calc(cx: &mut gpui::TestAppContext) {
        let page = basis(cx, r#"{"flexBasis": "50%", "flexShrink": 0}"#);
        near(page.size(2), (100.0, 50.0));
        page.set_style(cx, 2, r#"{"flexBasis": "calc(100% - 40px)", "flexShrink": 0}"#);
        near(page.size(2), (160.0, 50.0));
        page.set_style(cx, 2, r#"{"flexBasis": "2em", "fontSize": 15}"#);
        near(page.size(2), (30.0, 50.0));
        page.set_style(cx, 2, r#"{"flexBasis": 70}"#);
        near(page.size(2), (70.0, 50.0));
    }

    #[gpui::test]
    fn flex_basis_auto_uses_the_main_size(cx: &mut gpui::TestAppContext) {
        let page = basis(cx, r#"{"width": 30, "flexBasis": "auto"}"#);
        near(page.size(2), (30.0, 50.0));
        page.set_style(cx, 2, r#"{"width": 30, "flexBasis": 80, "flexShrink": 0}"#);
        near(page.size(2), (80.0, 50.0));
    }

    #[gpui::test]
    fn a_resize_observer_content_box_excludes_the_resolved_padding(cx: &mut gpui::TestAppContext) {
        let payloads = Arc::new(Mutex::new(Vec::<EventPayload>::new()));
        let sink = payloads.clone();
        let page = Page::with_events(
            cx,
            &[
                (1, None, PARENT),
                (2, Some(1), r#"{"width": 100, "height": 100, "fontSize": 20, "padding": "1em", "paddingLeft": "calc(10% + 1em)"}"#),
            ],
            &[(2, crate::observers::RESIZE_EVENT)],
            Some(Arc::new(move |payload| sink.lock().unwrap().push(payload))),
        );
        page.draw(cx);
        cx.run_until_parked();
        let payloads = payloads.lock().unwrap();
        let observation = payloads
            .iter()
            .find_map(|payload| payload.observation.as_ref())
            .expect("the observer reported");
        assert_eq!((observation.padding_left, observation.padding_top), (40.0, 20.0));
        assert_eq!((observation.content_width, observation.content_height), (100.0 - 40.0 - 20.0, 60.0));
    }

    /// `padded` with the fields of `style` added to element 2's 100px box.
    fn bordered(cx: &mut gpui::TestAppContext, style: &str) -> Page {
        padded(cx, &format!(r#"{{"width": 100, "height": 100, {style}}}"#))
    }

    fn border_of(page: &Page, id: u64) -> (f64, f64, f64, f64) {
        let bounds = page.bounds(id);
        (bounds.client_left, bounds.client_top, bounds.border_right, bounds.border_bottom)
    }

    #[gpui::test]
    fn a_border_width_in_em_shrinks_the_content_box_by_the_font_size(cx: &mut gpui::TestAppContext) {
        let page = bordered(cx, r#""fontSize": 20, "borderWidth": "0.5em""#);
        near(page.offset(3, 2), (10.0, 10.0));
        near(page.size(2), (100.0, 100.0));
        assert_eq!(border_of(&page, 2), (10.0, 10.0, 10.0, 10.0));
    }

    #[gpui::test]
    fn each_border_side_takes_its_own_length_and_beats_the_shorthand(cx: &mut gpui::TestAppContext) {
        let page = bordered(
            cx,
            r#""fontSize": 10, "borderWidth": "1in", "borderTopWidth": "calc(1px + 1rem)", "borderLeftWidth": 2,
               "borderRightWidth": "2em", "borderBottomWidth": "calc(1em - 2px)""#,
        );
        assert_eq!(border_of(&page, 2), (2.0, 17.0, 20.0, 8.0));
        near(page.offset(3, 2), (2.0, 17.0));
    }

    #[gpui::test]
    fn a_negative_border_width_is_none(cx: &mut gpui::TestAppContext) {
        let page = bordered(cx, r#""borderWidth": 3, "borderTopWidth": "-1em", "borderLeftWidth": -2"#);
        assert_eq!(border_of(&page, 2), (0.0, 0.0, 3.0, 3.0));
    }

    #[gpui::test]
    fn the_keywords_are_one_three_and_five_px(cx: &mut gpui::TestAppContext) {
        let page = bordered(
            cx,
            r#""borderLeftWidth": "thin", "borderTopWidth": "medium", "borderRightWidth": "thick", "borderBottomWidth": "Thick""#,
        );
        assert_eq!(border_of(&page, 2), (1.0, 3.0, 5.0, 5.0));
        page.set_style(cx, 2, r#"{"width": 100, "height": 100, "border": "thick solid red", "borderTop": "thin dashed blue"}"#);
        assert_eq!(border_of(&page, 2), (5.0, 1.0, 5.0, 5.0));
        page.set_style(cx, 2, r#"{"width": 100, "height": 100, "border": "solid"}"#);
        assert_eq!(border_of(&page, 2), (3.0, 3.0, 3.0, 3.0));
    }

    #[gpui::test]
    fn a_border_shorthand_resolves_em_in_the_elements_font_size(cx: &mut gpui::TestAppContext) {
        let page = bordered(cx, r#""fontSize": 20, "border": "0.25em solid red", "borderBottom": "calc(1em + 1px) dashed""#);
        assert_eq!(border_of(&page, 2), (5.0, 5.0, 5.0, 21.0));
        near(page.offset(3, 2), (5.0, 5.0));
    }

    #[gpui::test]
    fn a_hover_border_resolves_in_the_hover_font_size(cx: &mut gpui::TestAppContext) {
        let page = bordered(
            cx,
            r#""fontSize": 20, "border": "0.25em solid red", "hover": {"fontSize": 40}"#,
        );
        assert_eq!(border_of(&page, 2), (5.0, 5.0, 5.0, 5.0));
        let bounds = page.bounds(2);
        gpui::VisualTestContext::from_window(page.window.into(), cx).simulate_mouse_move(
            gpui::point(gpui::px((bounds.x + 90.0) as f32), gpui::px((bounds.y + 90.0) as f32)),
            None,
            Default::default(),
        );
        page.draw(cx);
        assert_eq!(border_of(&page, 2), (10.0, 10.0, 10.0, 10.0));
    }

    #[gpui::test]
    fn a_hover_border_width_in_em_replaces_the_base_one(cx: &mut gpui::TestAppContext) {
        let page = bordered(
            cx,
            r#""fontSize": 20, "borderLeftWidth": "0.5em", "borderWidth": 1, "hover": {"borderWidth": "1em"}"#,
        );
        assert_eq!(border_of(&page, 2), (10.0, 1.0, 1.0, 1.0));
        let bounds = page.bounds(2);
        gpui::VisualTestContext::from_window(page.window.into(), cx).simulate_mouse_move(
            gpui::point(gpui::px((bounds.x + 90.0) as f32), gpui::px((bounds.y + 90.0) as f32)),
            None,
            Default::default(),
        );
        page.draw(cx);
        assert_eq!(border_of(&page, 2), (20.0, 20.0, 20.0, 20.0));
    }

    #[test]
    fn the_outline_resolves_in_the_elements_font_size() {
        let style: StyleDesc =
            serde_json::from_str(r#"{"outlineWidth": "0.1em", "outlineOffset": "calc(0.5em + 1px)", "outlineColor": "red"}"#).unwrap();
        let lengths = LengthContext::initial(800.0, 600.0).descend(Some(&crate::css::length::LinearLength::px(20.0)), true);
        let refinement = apply_styles(
            gpui::StyleRefinement::default(),
            &style,
            &crate::color::ColorContext::root(crate::color::Scheme::Light),
            &lengths,
        );
        let outline = refinement.outline.expect("the outline is applied");
        assert_eq!((outline.width, outline.offset), (gpui::px(2.0), gpui::px(11.0)));
    }

    fn refine_in_20px_font(style: &str) -> gpui::StyleRefinement {
        let style: StyleDesc = serde_json::from_str(style).unwrap();
        let lengths = LengthContext::initial(800.0, 600.0)
            .descend(Some(&crate::css::length::LinearLength::px(10.0)), true)
            .descend(Some(&crate::css::length::LinearLength::px(20.0)), false);
        apply_styles(
            gpui::StyleRefinement::default(),
            &style,
            &crate::color::ColorContext::root(crate::color::Scheme::Light),
            &lengths,
        )
    }

    #[test]
    fn string_values_resolve_in_the_elements_font_size_and_the_viewport() {
        let refinement = refine_in_20px_font(
            r#"{
                "boxShadow": "0 0 1em red, 1rem -10vh 0 2px blue",
                "filter": "blur(0.5em) drop-shadow(1em 0 calc(1rem + 2px) red)",
                "backgroundImage": "linear-gradient(red calc(1em + 10%), blue 2rem)",
                "backgroundSize": "calc(100% - 1em) 3rem",
                "backgroundPosition": "1em 10vw",
                "clipPath": "inset(1em calc(2rem + 5%))"
            }"#,
        );

        // CSS paints the first shadow on top, so gpui has them reversed, and takes half the blur.
        let shadows = refinement.box_shadow.expect("the shadows are applied");
        assert_eq!(shadows.len(), 2);
        assert_eq!((shadows[0].offset.x, shadows[0].offset.y), (gpui::px(10.0), gpui::px(-60.0)));
        assert_eq!(shadows[0].spread_radius, gpui::px(2.0));
        assert_eq!(shadows[1].blur_radius, gpui::px(10.0));

        let filters = refinement.filter.expect("the filter is applied");
        assert_eq!(filters[0], gpui::FilterFunction::Blur(gpui::px(10.0)));
        let gpui::FilterFunction::DropShadow { offset, blur, .. } = filters[1] else {
            panic!("not a drop shadow");
        };
        assert_eq!((offset.x, blur), (gpui::px(20.0), gpui::px(12.0)));

        let layers = refinement.background_layers.expect("the layers are applied");
        let gpui::BackgroundImage::Gradient(gradient) = &layers[0].image;
        let positions: Vec<_> = gradient
            .items
            .iter()
            .map(|item| match item {
                gpui::GradientItem::Stop(stop) => stop.position,
                gpui::GradientItem::Hint(_) => panic!("a hint"),
            })
            .collect();
        assert_eq!(
            positions,
            vec![
                Some(gpui::StopPosition::Calc { length: gpui::px(20.0), fraction: 0.1 }),
                Some(gpui::StopPosition::Length(gpui::px(20.0))),
            ]
        );
        assert_eq!(
            layers[0].size,
            gpui::BackgroundSize::Explicit {
                width: Some(gpui::LengthPercentage::Calc { length: gpui::px(-20.0), percent: 1.0 }),
                height: Some(gpui::LengthPercentage::Length(gpui::px(30.0))),
            }
        );
        assert_eq!(layers[0].position.x.offset, gpui::LengthPercentage::Length(gpui::px(20.0)));
        assert_eq!(layers[0].position.y.offset, gpui::LengthPercentage::Length(gpui::px(80.0)));

        let clip_path = refinement.clip_path.expect("the clip path is applied");
        let gpui::ClipShape::Inset { top, right, .. } = clip_path.shape().expect("a shape").shape else {
            panic!("not an inset");
        };
        assert_eq!(top, gpui::LengthPercentage::Length(gpui::px(20.0)));
        assert_eq!(right, gpui::LengthPercentage::Calc { length: gpui::px(20.0), percent: 0.05 });
    }

    #[test]
    fn a_percentage_where_a_value_has_no_basis_leaves_it_unapplied() {
        let refinement = refine_in_20px_font(
            r#"{"boxShadow": "10% 0 red", "filter": "blur(calc(1em + 10%))", "clipPath": "inset(1ch)"}"#,
        );
        assert!(refinement.box_shadow.is_none());
        assert!(refinement.filter.is_none());
        assert!(refinement.clip_path.is_none());
    }

    #[gpui::test]
    fn a_resize_observer_content_box_excludes_the_resolved_border(cx: &mut gpui::TestAppContext) {
        let payloads = Arc::new(Mutex::new(Vec::<EventPayload>::new()));
        let sink = payloads.clone();
        let page = Page::with_events(
            cx,
            &[
                (1, None, PARENT),
                (2, Some(1), r#"{"width": 100, "height": 100, "fontSize": 20, "borderWidth": "0.5em", "borderLeftWidth": "calc(1em + 1px)", "padding": 5}"#),
            ],
            &[(2, crate::observers::RESIZE_EVENT)],
            Some(Arc::new(move |payload| sink.lock().unwrap().push(payload))),
        );
        page.draw(cx);
        cx.run_until_parked();
        let payloads = payloads.lock().unwrap();
        let observation = payloads
            .iter()
            .find_map(|payload| payload.observation.as_ref())
            .expect("the observer reported");
        assert_eq!(
            (observation.content_width, observation.content_height),
            (100.0 - 21.0 - 10.0 - 10.0, 100.0 - 20.0 - 10.0)
        );
    }

    // The test renderer's font is monospace: a character is 0.6em wide, so 9.6px at the default
    // 16px, and a line is 26px high. `min-content` is the widest word with the space after it,
    // which is what the line wrapper counts.
    const CHAR: f64 = 9.6;
    const LINE: f64 = 26.0;
    const HELLO_WORLD: &str = "hello world";
    const LONG: &str = "hello wonderful world";
    const COLUMN: &str = r#"{"width": 600}"#;

    fn text_width(characters: usize) -> f64 {
        (characters as f64 * CHAR).ceil()
    }

    /// Element 1 is `parent`, 2 a child with `style` holding the text `content`.
    fn text_box(cx: &mut gpui::TestAppContext, parent: &str, style: &str, content: &str) -> Page {
        let page = Page::new(cx, &[(1, None, parent), (2, Some(1), style)]);
        page.add_text(cx, 3, 2, content);
        page
    }

    fn hover(page: &Page, cx: &mut gpui::TestAppContext, id: u64) {
        let bounds = page.bounds(id);
        gpui::VisualTestContext::from_window(page.window.into(), cx).simulate_mouse_move(
            gpui::point(gpui::px((bounds.x + 2.0) as f32), gpui::px((bounds.y + 2.0) as f32)),
            None,
            Default::default(),
        );
        page.draw(cx);
    }

    #[gpui::test]
    fn max_content_is_the_text_on_one_line_in_a_wider_column(cx: &mut gpui::TestAppContext) {
        let page = text_box(cx, COLUMN, r#"{"width": "max-content"}"#, LONG);
        near(page.size(2), (text_width(21), LINE));
        page.set_style(cx, 2, "{}");
        near(page.size(2), (600.0, LINE));
    }

    #[gpui::test]
    fn min_content_is_the_widest_word(cx: &mut gpui::TestAppContext) {
        let page = text_box(cx, COLUMN, r#"{"width": "min-content"}"#, LONG);
        near(page.size(2), (text_width(10), 3.0 * LINE));
    }

    #[gpui::test]
    fn fit_content_is_the_available_width_between_min_and_max_content(cx: &mut gpui::TestAppContext) {
        let short = text_box(cx, r#"{"width": 300}"#, r#"{"width": "fit-content"}"#, HELLO_WORLD);
        near(short.size(2), (text_width(11), LINE));
        let long = text_box(cx, r#"{"width": 150}"#, r#"{"width": "fit-content"}"#, LONG);
        near(long.size(2), (150.0, 2.0 * LINE));
        let narrow = text_box(cx, r#"{"width": 40}"#, r#"{"width": "fit-content"}"#, LONG);
        near(narrow.size(2), (text_width(10), 3.0 * LINE));
    }

    #[gpui::test]
    fn fit_content_with_a_limit_clamps_it_between_min_and_max_content(cx: &mut gpui::TestAppContext) {
        let page = text_box(cx, COLUMN, r#"{"width": "fit-content(100px)"}"#, LONG);
        near(page.size(2), (100.0, 3.0 * LINE));
        page.set_style(cx, 2, r#"{"width": "fit-content(10px)"}"#);
        near(page.size(2), (text_width(10), 3.0 * LINE));
        page.set_style(cx, 2, r#"{"width": "fit-content(500px)"}"#);
        near(page.size(2), (text_width(21), LINE));
        page.set_style(cx, 2, r#"{"width": "fit-content(calc(50px + 50px))"}"#);
        near(page.size(2), (100.0, 3.0 * LINE));
    }

    #[gpui::test]
    fn a_fit_content_limit_in_percent_is_of_the_containing_block(cx: &mut gpui::TestAppContext) {
        let page = text_box(cx, r#"{"width": 300}"#, r#"{"width": "fit-content(50%)"}"#, LONG);
        near(page.size(2), (150.0, 2.0 * LINE));
        page.set_style(cx, 1, r#"{"width": 500}"#);
        near(page.size(2), (text_width(21), LINE));
    }

    #[gpui::test]
    fn a_fit_content_limit_in_em_follows_the_elements_font_size(cx: &mut gpui::TestAppContext) {
        let page = text_box(cx, COLUMN, r#"{"fontSize": 20, "width": "fit-content(15em)"}"#, LONG);
        // 20px: a character is 12px, min-content 120px, max-content 252px, and 15em is 300px.
        near(page.size(2), (252.0, 32.5));
        page.set_style(cx, 2, r#"{"fontSize": 20, "width": "fit-content(10em)"}"#);
        near(page.size(2), (200.0, 65.0));
        page.set_style(cx, 2, r#"{"fontSize": 10, "width": "fit-content(10em)"}"#);
        near(page.size(2), (100.0, 32.0));
    }

    #[gpui::test]
    fn min_width_min_content_keeps_a_narrow_box_from_squeezing_its_text(cx: &mut gpui::TestAppContext) {
        let page = text_box(cx, COLUMN, r#"{"width": 50, "minWidth": "min-content"}"#, LONG);
        near(page.size(2), (text_width(10), 3.0 * LINE));
    }

    #[gpui::test]
    fn max_width_max_content_caps_a_stretched_block(cx: &mut gpui::TestAppContext) {
        let page = text_box(cx, COLUMN, r#"{"maxWidth": "max-content"}"#, LONG);
        near(page.size(2), (text_width(21), LINE));
        let narrower = text_box(cx, r#"{"width": 100}"#, r#"{"maxWidth": "max-content"}"#, LONG);
        near(narrower.size(2), (100.0, 3.0 * LINE));
    }

    #[gpui::test]
    fn the_keywords_are_auto_in_the_block_axis(cx: &mut gpui::TestAppContext) {
        let plain = text_box(cx, COLUMN, r#"{"width": 120}"#, LONG);
        let expected = plain.size(2);
        for style in [
            r#"{"width": 120, "height": "max-content"}"#,
            r#"{"width": 120, "height": "min-content"}"#,
            r#"{"width": 120, "height": "fit-content(10px)"}"#,
            r#"{"width": 120, "minHeight": "max-content"}"#,
            r#"{"width": 120, "maxHeight": "fit-content"}"#,
        ] {
            let page = text_box(cx, COLUMN, style, LONG);
            near(page.size(2), expected);
        }
    }

    #[gpui::test]
    fn a_hover_width_replaces_a_base_keyword_and_a_hover_keyword_replaces_a_base_width(cx: &mut gpui::TestAppContext) {
        let page = text_box(
            cx,
            COLUMN,
            r#"{"width": "max-content", "hover": {"width": 50}}"#,
            LONG,
        );
        near(page.size(2), (text_width(21), LINE));
        hover(&page, cx, 2);
        assert_eq!(page.size(2).0, 50.0);

        let reverse = text_box(
            cx,
            COLUMN,
            r#"{"width": 50, "hover": {"width": "max-content"}}"#,
            LONG,
        );
        assert_eq!(reverse.size(2).0, 50.0);
        hover(&reverse, cx, 2);
        near(reverse.size(2), (text_width(21), LINE));

        let auto = text_box(
            cx,
            COLUMN,
            r#"{"width": "max-content", "hover": {"width": "auto"}}"#,
            LONG,
        );
        hover(&auto, cx, 2);
        assert_eq!(auto.size(2).0, 600.0);
    }

    fn refine(styles: &[&str]) -> gpui::StyleRefinement {
        let lengths = LengthContext::initial(800.0, 600.0);
        let colors = crate::color::ColorContext::root(crate::color::Scheme::Light);
        styles.iter().fold(gpui::StyleRefinement::default(), |refinement, style| {
            let style: StyleDesc = serde_json::from_str(style).unwrap();
            apply_styles(refinement, &style, &colors, &lengths)
        })
    }

    #[test]
    fn a_later_apply_replaces_a_keyword_in_the_same_slot_either_way() {
        let keyword = refine(&[r#"{"width": "max-content", "minWidth": "fit-content(2em)", "maxWidth": "min-content"}"#]);
        assert_eq!(keyword.intrinsic_widths.width, Some(Some(gpui::IntrinsicSize::MaxContent)));
        assert_eq!(
            keyword.intrinsic_widths.min_width,
            Some(Some(gpui::IntrinsicSize::FitContentLimit(gpui::px(32.0).into())))
        );
        assert_eq!(keyword.intrinsic_widths.max_width, Some(Some(gpui::IntrinsicSize::MinContent)));

        let definite = refine(&[
            r#"{"width": "max-content", "minWidth": "max-content", "maxWidth": "max-content"}"#,
            r#"{"width": 50, "minWidth": "10%", "maxWidth": "calc(1em + 1px)"}"#,
        ]);
        assert_eq!(definite.intrinsic_widths.width, Some(None));
        assert_eq!(definite.intrinsic_widths.min_width, Some(None));
        assert_eq!(definite.intrinsic_widths.max_width, Some(None));
        assert_eq!(definite.size.width, Some(gpui::px(50.0).into()));

        let auto = refine(&[r#"{"width": "min-content"}"#, r#"{"width": "auto"}"#]);
        assert_eq!(auto.intrinsic_widths.width, Some(None));
        assert_eq!(auto.size.width, Some(gpui::Length::Auto));

        let back = refine(&[r#"{"width": 50}"#, r#"{"width": "min-content"}"#]);
        assert_eq!(back.intrinsic_widths.width, Some(Some(gpui::IntrinsicSize::MinContent)));
        assert_eq!(back.size.width, Some(gpui::Length::Auto));

        let untouched = refine(&[r#"{"width": "max-content"}"#, r#"{"height": 5}"#]);
        assert_eq!(untouched.intrinsic_widths.width, Some(Some(gpui::IntrinsicSize::MaxContent)));
    }

    #[test]
    fn a_height_keyword_applies_as_auto() {
        let height = refine(&[r#"{"height": "max-content", "minHeight": "min-content", "maxHeight": "fit-content"}"#]);
        assert_eq!(height.size.height, Some(gpui::Length::Auto));
        assert_eq!(height.min_size.height, Some(gpui::Length::Auto));
        assert_eq!(height.max_size.height, Some(gpui::Length::Auto));
        assert_eq!(height.intrinsic_widths, Default::default());
    }

    #[gpui::test]
    fn a_keyword_node_in_a_grid_cell_sizes_to_its_content(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                (1, None, r#"{"width": 600, "display": "grid", "gridTemplateColumns": "300px 300px", "alignItems": "start"}"#),
                (2, Some(1), r#"{"width": "max-content"}"#),
                (4, Some(1), r#"{"width": "min-content"}"#),
            ],
        );
        page.add_text(cx, 3, 2, LONG);
        page.add_text(cx, 5, 4, LONG);
        near(page.size(2), (text_width(21), LINE));
        near(page.size(4), (text_width(10), 3.0 * LINE));
        near(page.offset(4, 1), (300.0, 0.0));
    }

    #[gpui::test]
    fn a_keyword_node_in_a_flex_row_leaves_the_rest_to_its_sibling(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                (1, None, r#"{"width": 600, "display": "flex", "flexDirection": "row"}"#),
                (2, Some(1), r#"{"width": "max-content", "flexShrink": 0}"#),
                (4, Some(1), r#"{"flexGrow": 1}"#),
            ],
        );
        page.add_text(cx, 3, 2, HELLO_WORLD);
        near(page.size(2), (text_width(11), LINE));
        near(page.size(4), (600.0 - text_width(11), LINE));
        near(page.offset(4, 1), (text_width(11), 0.0));
    }

    /// A row of `container`, holding element 2 with `item` and text, then a growing element 4.
    fn basis_row(cx: &mut gpui::TestAppContext, container: &str, item: &str, content: &str) -> Page {
        let page = Page::new(
            cx,
            &[(1, None, container), (2, Some(1), item), (4, Some(1), r#"{"flexGrow": 1}"#)],
        );
        page.add_text(cx, 3, 2, content);
        page
    }

    const ROW: &str = r#"{"width": 600, "display": "flex", "flexDirection": "row"}"#;

    #[gpui::test]
    fn a_max_content_flex_basis_beats_the_width_and_leaves_the_rest_to_a_sibling(cx: &mut gpui::TestAppContext) {
        let page = basis_row(
            cx,
            ROW,
            r#"{"width": 20, "flexBasis": "max-content", "flexShrink": 0}"#,
            HELLO_WORLD,
        );
        near(page.size(2), (text_width(11), LINE));
        near(page.size(4), (600.0 - text_width(11), LINE));
        near(page.offset(4, 1), (text_width(11), 0.0));
    }

    #[gpui::test]
    fn a_min_content_flex_basis_wraps_the_text_to_its_widest_word(cx: &mut gpui::TestAppContext) {
        let page = basis_row(cx, ROW, r#"{"flexBasis": "min-content", "flexShrink": 0}"#, LONG);
        near(page.size(2), (text_width(10), 3.0 * LINE));
        near(page.offset(4, 1), (text_width(10), 0.0));
    }

    #[gpui::test]
    fn a_flex_basis_keyword_works_in_a_reversed_row_and_beats_a_width_keyword(cx: &mut gpui::TestAppContext) {
        let reversed = r#"{"width": 600, "display": "flex", "flexDirection": "row-reverse"}"#;
        let page = basis_row(cx, reversed, r#"{"flexBasis": "min-content", "flexShrink": 0}"#, LONG);
        near(page.size(2), (text_width(10), 3.0 * LINE));
        let both = basis_row(
            cx,
            ROW,
            r#"{"width": "min-content", "flexBasis": "max-content", "flexShrink": 0}"#,
            LONG,
        );
        near(both.size(2), (text_width(21), LINE));
    }

    #[gpui::test]
    fn a_fit_content_flex_basis_is_max_content_that_shrinks_and_a_limit_is_clamped(cx: &mut gpui::TestAppContext) {
        let page = basis_row(cx, ROW, r#"{"flexBasis": "fit-content", "flexShrink": 0}"#, LONG);
        near(page.size(2), (text_width(21), LINE));
        let narrow = basis_row(cx, r#"{"width": 150, "display": "flex"}"#, r#"{"flexBasis": "fit-content"}"#, LONG);
        near(narrow.size(2), (150.0, 2.0 * LINE));

        let limited = basis_row(cx, ROW, r#"{"flexBasis": "fit-content(100px)", "flexShrink": 0}"#, LONG);
        near(limited.size(2), (100.0, 3.0 * LINE));
        limited.set_style(cx, 2, r#"{"flexBasis": "fit-content(10px)", "flexShrink": 0}"#);
        near(limited.size(2), (text_width(10), 3.0 * LINE));
        limited.set_style(cx, 2, r#"{"flexBasis": "fit-content(500px)", "flexShrink": 0}"#);
        near(limited.size(2), (text_width(21), LINE));
        limited.set_style(cx, 2, r#"{"flexBasis": "fit-content(25%)", "flexShrink": 0}"#);
        near(limited.size(2), (150.0, 2.0 * LINE));
    }

    #[gpui::test]
    fn a_flex_basis_keyword_is_auto_along_a_column_and_in_a_block(cx: &mut gpui::TestAppContext) {
        let column = r#"{"width": 600, "display": "flex", "flexDirection": "column"}"#;
        let reversed = r#"{"width": 600, "display": "flex", "flexDirection": "column-reverse"}"#;
        for parent in [column, reversed, r#"{"width": 600, "display": "block"}"#] {
            for keyword in ["min-content", "max-content", "fit-content", "fit-content(50px)"] {
                let item = format!(r#"{{"flexBasis": "{keyword}"}}"#);
                let page = text_box(cx, parent, &item, LONG);
                near(page.size(2), (600.0, LINE));
            }
        }
    }

    #[gpui::test]
    fn a_hover_flex_basis_replaces_a_base_keyword_and_the_other_way_round(cx: &mut gpui::TestAppContext) {
        let page = basis_row(
            cx,
            ROW,
            r#"{"flexBasis": "max-content", "flexShrink": 0, "hover": {"flexBasis": 120}}"#,
            LONG,
        );
        near(page.size(2), (text_width(21), LINE));
        hover(&page, cx, 2);
        assert_eq!(page.size(2).0, 120.0);

        let reverse = basis_row(
            cx,
            ROW,
            r#"{"flexBasis": 120, "flexShrink": 0, "hover": {"flexBasis": "max-content"}}"#,
            LONG,
        );
        assert_eq!(reverse.size(2).0, 120.0);
        hover(&reverse, cx, 2);
        near(reverse.size(2), (text_width(21), LINE));
    }

    #[gpui::test]
    fn set_style_swaps_a_flex_basis_keyword_for_a_length_and_back(cx: &mut gpui::TestAppContext) {
        let page = basis_row(cx, ROW, r#"{"flexBasis": "min-content", "flexShrink": 0}"#, LONG);
        near(page.size(2), (text_width(10), 3.0 * LINE));
        page.set_style(cx, 2, r#"{"flexBasis": 120, "flexShrink": 0}"#);
        near(page.size(2), (120.0, 3.0 * LINE));
        page.set_style(cx, 2, r#"{"flexBasis": "max-content", "flexShrink": 0}"#);
        near(page.size(2), (text_width(21), LINE));
        page.set_style(cx, 2, r#"{"flexBasis": "auto", "flexShrink": 0}"#);
        near(page.size(2), (text_width(21), LINE));
        page.set_style(cx, 2, r#"{"flexBasis": "min-content", "flexShrink": 0}"#);
        page.set_style(cx, 2, r#"{"flexShrink": 0}"#);
        near(page.size(2), (text_width(21), LINE));
    }

    #[test]
    fn a_later_apply_replaces_a_flex_basis_keyword_either_way() {
        let keyword = refine(&[r#"{"flexBasis": "fit-content(2em)"}"#]);
        assert_eq!(
            keyword.intrinsic_widths.flex_basis,
            Some(Some(gpui::IntrinsicSize::FitContentLimit(gpui::px(32.0).into())))
        );
        assert_eq!(keyword.flex_basis, Some(gpui::Length::Auto));

        for replacement in [r#"{"flexBasis": 50}"#, r#"{"flexBasis": "auto"}"#, r#"{"flexBasis": "calc(1em + 1%)"}"#] {
            let cleared = refine(&[r#"{"flexBasis": "max-content"}"#, replacement]);
            assert_eq!(cleared.intrinsic_widths.flex_basis, Some(None), "{replacement}");
        }
        let back = refine(&[r#"{"flexBasis": 50}"#, r#"{"flexBasis": "min-content"}"#]);
        assert_eq!(back.intrinsic_widths.flex_basis, Some(Some(gpui::IntrinsicSize::MinContent)));
        assert_eq!(back.flex_basis, Some(gpui::Length::Auto));

        let untouched = refine(&[r#"{"flexBasis": "max-content"}"#, r#"{"width": 5}"#]);
        assert_eq!(untouched.intrinsic_widths.flex_basis, Some(Some(gpui::IntrinsicSize::MaxContent)));
        assert_eq!(untouched.intrinsic_widths.width, Some(None));
    }

    #[gpui::test]
    fn a_resize_observer_and_the_bounds_report_the_resolved_keyword_width(cx: &mut gpui::TestAppContext) {
        let payloads = Arc::new(Mutex::new(Vec::<EventPayload>::new()));
        let sink = payloads.clone();
        let page = Page::with_events(
            cx,
            &[(1, None, COLUMN), (2, Some(1), r#"{"width": "max-content", "padding": 5}"#)],
            &[(2, crate::observers::RESIZE_EVENT)],
            Some(Arc::new(move |payload| sink.lock().unwrap().push(payload))),
        );
        page.add_text(cx, 3, 2, LONG);
        cx.run_until_parked();
        near(page.size(2), (text_width(21) + 10.0, LINE + 10.0));
        let payloads = payloads.lock().unwrap();
        let observation = payloads
            .iter()
            .rev()
            .find_map(|payload| payload.observation.as_ref())
            .expect("the observer reported");
        assert_eq!(
            (observation.content_width, observation.content_height),
            (text_width(21), LINE)
        );
    }

    mod natural_width {
        use super::*;

        const ROW: &str = r#"{"display": "flex", "width": 400, "height": 100}"#;
        const CONTENT: &str = r#"{"width": 120, "height": 10, "flexShrink": 0}"#;
        const OPEN_KEYWORDS: &str = r#""interpolateSize": "allow-keywords""#;

        /// Element 2 holds a 120px box; the clock stands still until `step`.
        fn page(cx: &mut gpui::TestAppContext, style: &str) -> Page {
            let page = Page::new(cx, &[(1, None, ROW), (2, Some(1), style), (3, Some(2), CONTENT)]);
            page.window.update(cx, |view, _, _| view.clock.pause()).unwrap();
            page
        }

        fn set_motion(page: &Page, cx: &mut gpui::TestAppContext, description: serde_json::Value) {
            page.tree.lock().unwrap().set_custom_prop(2, "motion".to_owned(), description);
            page.draw(cx);
        }

        /// Draws a frame `millis` later.
        fn step(page: &Page, cx: &mut gpui::TestAppContext, millis: f64) {
            page.window.update(cx, |view, _, _| view.clock.fast_forward_ms(millis)).unwrap();
            page.draw(cx);
        }

        fn natural_width(page: &Page, cx: &mut gpui::TestAppContext, id: u64) -> Option<f32> {
            page.window
                .update(cx, |view, _, _| {
                    view.measurements
                        .get(id)
                        .expect("the element is measured")
                        .natural_width
                        .map(f32::from)
                })
                .unwrap()
        }

        fn open_width() -> serde_json::Value {
            serde_json::json!({
                "initial": { "width": 0 },
                "animate": { "width": "auto" },
                "transition": { "duration": 1.0, "ease": "linear" }
            })
        }

        #[gpui::test]
        fn a_motion_width_runs_from_zero_to_the_content_width_and_ends_on_auto(cx: &mut gpui::TestAppContext) {
            let page = page(cx, r#"{"display": "flex"}"#);
            set_motion(&page, cx, open_width());
            // The first frame has nothing measured: it holds the start.
            near(page.size(2), (0.0, 100.0));
            // The next one has the width, and the clock starts there.
            step(&page, cx, 500.0);
            near(page.size(2), (0.0, 100.0));
            assert_eq!(natural_width(&page, cx, 2), Some(120.0));
            step(&page, cx, 500.0);
            near(page.size(2), (60.0, 100.0));
            step(&page, cx, 500.0);
            near(page.size(2), (120.0, 100.0));
            step(&page, cx, 500.0);
            near(page.size(2), (120.0, 100.0));
        }

        #[gpui::test]
        fn auto_ends_as_the_content_width_when_the_content_changes(cx: &mut gpui::TestAppContext) {
            let page = page(cx, r#"{"display": "flex"}"#);
            set_motion(&page, cx, open_width());
            step(&page, cx, 0.0);
            step(&page, cx, 1000.0);
            near(page.size(2), (120.0, 100.0));
            page.set_style(cx, 3, r#"{"width": 200, "height": 10, "flexShrink": 0}"#);
            near(page.size(2), (200.0, 100.0));
        }

        #[gpui::test]
        fn only_an_element_that_animates_width_toward_auto_is_measured_for_it(cx: &mut gpui::TestAppContext) {
            let page = page(cx, r#"{"display": "flex"}"#);
            set_motion(
                &page,
                cx,
                serde_json::json!({ "initial": { "opacity": 0 }, "animate": { "opacity": 1 }, "transition": { "duration": 1.0 } }),
            );
            step(&page, cx, 10.0);
            assert_eq!(natural_width(&page, cx, 2), None);

            set_motion(
                &page,
                cx,
                serde_json::json!({ "animate": { "width": 100 }, "transition": { "duration": 1.0 } }),
            );
            step(&page, cx, 10.0);
            assert_eq!(natural_width(&page, cx, 2), None);

            set_motion(&page, cx, open_width());
            step(&page, cx, 10.0);
            assert_eq!(natural_width(&page, cx, 2), Some(120.0));

            // Settled on auto, nothing follows it any more.
            step(&page, cx, 2000.0);
            step(&page, cx, 10.0);
            step(&page, cx, 10.0);
            assert_eq!(natural_width(&page, cx, 2), None);
        }

        #[gpui::test]
        fn a_transition_toward_an_auto_width_needs_allow_keywords_to_measure_and_run(cx: &mut gpui::TestAppContext) {
            let closed = format!(r#"{{"display": "flex", "width": 0, {OPEN_KEYWORDS}, "transition": "width 1s linear"}}"#);
            let open = format!(r#"{{"display": "flex", {OPEN_KEYWORDS}, "transition": "width 1s linear"}}"#);
            let page = page(cx, &closed);
            assert_eq!(natural_width(&page, cx, 2), Some(120.0));
            near(page.size(2), (0.0, 100.0));

            page.set_style(cx, 2, &open);
            near(page.size(2), (0.0, 100.0));
            step(&page, cx, 500.0);
            near(page.size(2), (60.0, 100.0));
            step(&page, cx, 500.0);
            near(page.size(2), (120.0, 100.0));
            step(&page, cx, 500.0);
            near(page.size(2), (120.0, 100.0));

            let numeric = r#"{"display": "flex", "width": 0, "transition": "width 1s linear"}"#;
            let page = self::page(cx, numeric);
            assert_eq!(natural_width(&page, cx, 2), None);
            page.set_style(cx, 2, r#"{"display": "flex", "transition": "width 1s linear"}"#);
            step(&page, cx, 500.0);
            near(page.size(2), (120.0, 100.0));
        }

        #[gpui::test]
        fn a_transition_of_another_property_does_not_measure_the_width(cx: &mut gpui::TestAppContext) {
            let page = page(
                cx,
                &format!(r#"{{"display": "flex", {OPEN_KEYWORDS}, "transition": "opacity 1s linear"}}"#),
            );
            assert_eq!(natural_width(&page, cx, 2), None);
        }

        #[gpui::test]
        fn a_css_animation_of_width_runs_to_the_content_width(cx: &mut gpui::TestAppContext) {
            let page = page(cx, r#"{"display": "flex"}"#);
            page.window
                .update(cx, |view, _, _| {
                    view.keyframes
                        .define("open", r#"{ "from": { "width": 0 }, "to": { "width": "auto" } }"#)
                        .unwrap();
                })
                .unwrap();
            page.set_style(
                cx,
                2,
                &format!(r#"{{"display": "flex", {OPEN_KEYWORDS}, "animation": "open 1s linear forwards"}}"#),
            );
            step(&page, cx, 500.0);
            assert_eq!(natural_width(&page, cx, 2), Some(120.0));
            step(&page, cx, 0.0);
            near(page.size(2), (60.0, 100.0));
            step(&page, cx, 600.0);
            near(page.size(2), (120.0, 100.0));
        }
    }
}

/// The flags `build_element` sets for stacking, through a real build and paint: the order the
/// text of each box is painted in is the paint order.
#[cfg(test)]
mod stacking_build_tests {
    use super::*;

    fn labelled(tree: &mut RetainedTree, id: u64, parent: u64, style: &str, label: &str) {
        let style: StyleDesc = serde_json::from_str(style).unwrap();
        tree.create_element(id, "div".to_owned());
        tree.set_style(id, Arc::new(style));
        tree.append_child(parent, id);
        tree.create_element(id + 100, "text".to_owned());
        tree.set_text(id + 100, label.to_owned());
        tree.append_child(id, id + 100);
    }

    /// `1` is the window root with `a`, a static 200 box holding `b`, and `c` after it.
    fn paint_order(cx: &mut gpui::TestAppContext, root: &str, b: &str, c: &str) -> Vec<String> {
        let tree = Arc::new(Mutex::new(RetainedTree::new()));
        {
            let mut tree = tree.lock().unwrap();
            let root: StyleDesc = serde_json::from_str(root).unwrap();
            tree.create_element(1, "div".to_owned());
            tree.set_style(1, Arc::new(root));
            tree.set_root(1);
            labelled(&mut tree, 2, 1, r#"{"width": 200, "height": 200}"#, "a");
            labelled(&mut tree, 3, 2, b, "b");
            labelled(&mut tree, 4, 1, c, "c");
        }
        let window = cx.add_window(|window, _| {
            crate::events::enable_dom_hit_testing(window);
            GpuixView::new(
                tree,
                None,
                "test".to_owned(),
                SharedSelection::default(),
                crate::events::state::PointerRequests::default(),
            )
        });
        cx.update_window(window.into(), |_, window, app| {
            window.refresh();
            window.draw(app).clear(app);
        })
        .unwrap();
        crate::text::painted_text()
    }

    const ROOT: &str = r#"{"width": 400, "height": 400}"#;
    const C: &str = r#"{"width": 200, "height": 200, "marginTop": -100, "marginLeft": 100}"#;

    #[gpui::test]
    fn a_z_indexed_descendant_paints_after_a_later_sibling(cx: &mut gpui::TestAppContext) {
        let b = r#"{"position": "absolute", "zIndex": 5, "left": 120, "top": 120, "width": 100, "height": 100}"#;
        assert_eq!(paint_order(cx, ROOT, b, C), ["a", "c", "b"]);
    }

    #[gpui::test]
    fn a_static_box_with_a_z_index_stays_in_tree_order(cx: &mut gpui::TestAppContext) {
        let b = r#"{"zIndex": 5, "width": 100, "height": 100}"#;
        assert_eq!(paint_order(cx, ROOT, b, C), ["a", "b", "c"]);
    }
}
