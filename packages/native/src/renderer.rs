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
use std::cell::{Cell, RefCell};
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
pub(crate) fn scroll_metrics(window: gpui::WindowId, id: u64) -> Option<[f64; 6]> {
    let painted = crate::automation::get_bounds(window, id);
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

/// Marks the root view dirty for a change JS made to the tree, a scroll offset or a motion
/// value, and wakes the platform. Both renderers go through it, so the test renderer draws the
/// frame the live window does.
///
/// It must not call `Window::refresh`: that sets `refreshing`, and a refreshing frame skips no
/// replay, so every element is built (5403 for one hover write over 3000 nodes, against 5).
/// Retention sees all of these: a mutation moves a `subtree_revision`, and a scroll offset and a
/// running motion are frame deltas. `refresh` is for what it cannot see, as `set_live_image` and
/// `register_font` do.
pub(crate) fn invalidate_after_commit(cx: &mut gpui::App, view: gpui::EntityId) {
    cx.notify(view);
}

/// What a write to the motion state asks of the window.
#[derive(Clone, Copy)]
pub(crate) enum Repaint {
    None,
    /// Something a frame samples changed in a way not known to leave the boxes alone.
    Layout,
    /// A write to this motion value, which leaves the boxes alone when nothing that can move
    /// one is driven by it (`MotionValueStore::writes_paint_only`).
    MotionWrite(f64),
}

impl Repaint {
    /// Invalidates the window as the write asked, after `run` made it. A write whose
    /// classification fails is a layout one.
    pub(crate) fn invalidate(
        self,
        motion_values: &crate::motion::MotionValueStore,
        cx: &mut gpui::App,
        view: gpui::EntityId,
    ) {
        match self {
            Self::None => {}
            Self::MotionWrite(id) if matches!(motion_values.writes_paint_only(id), Ok(true)) => {
                // The frame that follows still draws; only a read of bounds before it learns
                // that no layout is due.
                cx.notify_paint_only(view);
            }
            Self::MotionWrite(_) | Self::Layout => invalidate_after_commit(cx, view),
        }
    }
}

#[cfg(target_os = "macos")]
fn invalidate_window() -> Result<()> {
    if crate::events::sync_dispatch::in_sync_call() {
        return crate::events::sync_dispatch::invalidate_root_view().map_err(Error::from_reason);
    }
    update_window(|_view, _window, cx| {
        let view = cx.entity_id();
        invalidate_after_commit(cx, view);
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
fn invalidate_ui_window(
    window: gpui::WindowHandle<GpuixView>,
    cx: &mut gpui::AsyncApp,
) -> anyhow::Result<()> {
    window.update(cx, |_view, _window, cx| {
        let view = cx.entity_id();
        invalidate_after_commit(cx, view);
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
            UiCommand::Invalidate => invalidate_ui_window(window, cx),
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
                invalidate_ui_window(window, cx)
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
                invalidate_ui_window(window, cx)
            }
            UiCommand::ScrollIntoView { id, options } => {
                // Align by the tree as it is, not the last frame; see `GetElementBounds`.
                gpui::AnyWindowHandle::from(window)
                    .update(cx, |_view, window, cx| window.draw_if_dirty(cx))?;
                window.update(cx, move |view, window, cx| {
                    if view.scroll_element_into_view(window, id, options) {
                        cx.notify();
                        window.refresh();
                    }
                })
            }
            UiCommand::GetScrollMetrics { id, response } => {
                gpui::AnyWindowHandle::from(window).update(cx, move |_view, window, cx| {
                    window.draw_if_dirty(cx);
                    response.send(scroll_metrics(window.window_handle().window_id(), id)).ok();
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
                    response.send(crate::automation::all_bounds(window.window_handle().window_id())).ok();
                })
            }
            UiCommand::GetElementBounds { id, response } => {
                gpui::AnyWindowHandle::from(window).update(cx, move |_view, window, cx| {
                    window.draw_if_dirty(cx);
                    response.send(crate::automation::get_bounds(window.window_handle().window_id(), id)).ok();
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
            UiCommand::FocusNext => window.update(cx, |_view, window, cx| {
                if let Err(error) = crate::focus::step_focus(window, cx, false) {
                    crate::events::sync_dispatch::report_deferred_error(error);
                }
            }),
            UiCommand::FocusPrevious => window.update(cx, |_view, window, cx| {
                if let Err(error) = crate::focus::step_focus(window, cx, true) {
                    crate::events::sync_dispatch::report_deferred_error(error);
                }
            }),
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

#[cfg(target_os = "macos")]
type FrameWakeHook = Arc<dyn Fn() + Send + Sync>;

/// The display-link wake path. `pending` is set by the wake that queues JS work and
/// cleared only when `tick()` returns, so a wake that lands during a frame is dropped
/// instead of queued behind it.
#[cfg(target_os = "macos")]
#[derive(Default)]
struct FrameWake {
    pending: Arc<std::sync::atomic::AtomicBool>,
    /// The closure the display link calls, kept here so the test thread calls the same one.
    hook: Arc<Mutex<Option<FrameWakeHook>>>,
    #[cfg(feature = "test-support")]
    simulator: Mutex<Option<WakeSimulator>>,
    #[cfg(feature = "test-support")]
    tick_delay_ms: std::sync::atomic::AtomicU32,
}

#[cfg(target_os = "macos")]
impl FrameWake {
    fn clear(&self) {
        gpui_macos::set_frame_wake_hook(None);
        self.hook.lock().unwrap().take();
        self.pending
            .store(false, std::sync::atomic::Ordering::Release);
        #[cfg(feature = "test-support")]
        self.stop_simulator();
    }

    #[cfg(feature = "test-support")]
    fn stop_simulator(&self) {
        let simulator = self.simulator.lock().unwrap().take();
        if let Some(simulator) = simulator {
            simulator.stop();
        }
    }
}

/// Calls the wake closure from a Rust thread at a fixed interval, as the display link does.
#[cfg(all(target_os = "macos", feature = "test-support"))]
struct WakeSimulator {
    stop: std::sync::mpsc::Sender<()>,
    thread: std::thread::JoinHandle<()>,
}

#[cfg(all(target_os = "macos", feature = "test-support"))]
impl WakeSimulator {
    fn start(interval: Duration, hook: Arc<Mutex<Option<FrameWakeHook>>>) -> Self {
        let (stop, stopped) = std::sync::mpsc::channel::<()>();
        let thread = std::thread::spawn(move || {
            // Deadlines, not a sleep per wake, so a busy machine does not stretch the period.
            let mut next = std::time::Instant::now() + interval;
            while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = stopped
                .recv_timeout(next.saturating_duration_since(std::time::Instant::now()))
            {
                next += interval;
                let wake = hook.lock().unwrap().clone();
                if let Some(wake) = wake {
                    wake();
                }
            }
        });
        Self { stop, thread }
    }

    fn stop(self) {
        drop(self.stop);
        self.thread.join().expect("the wake simulator thread panicked");
    }
}

/// The main GPUI renderer exposed to Node.js.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
#[napi]
pub struct GpuixRenderer {
    #[cfg(target_os = "macos")]
    frame_wake: FrameWake,
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

    /// Lay out any pending change now, so a bounds read answers for the tree as it is, like
    /// `getBoundingClientRect` forcing layout. Nothing is painted: the next display-link step
    /// draws and presents once, however many reads came before it.
    ///
    /// Without this, bounds come from the last display-link frame, which may
    /// not exist yet right after a commit. Call it before locking `tree`: the
    /// pass builds the view from it.
    ///
    /// Returns the window that was laid out, which is where its boxes are. `None` before
    /// `init`: nothing was laid out.
    #[cfg(target_os = "macos")]
    fn draw_pending_changes(&self) -> Result<Option<gpui::WindowId>> {
        self.with_initialized_window(|window, cx| window.draw_if_dirty(cx))
    }

    /// Like `draw_pending_changes`, for a read of what a frame painted (the painted text), which
    /// a layout does not produce.
    #[cfg(target_os = "macos")]
    fn paint_pending_changes(&self) -> Result<Option<gpui::WindowId>> {
        self.with_initialized_window(|window, cx| window.paint_if_dirty(cx))
    }

    #[cfg(target_os = "macos")]
    fn with_initialized_window(
        &self,
        pass: impl FnOnce(&mut gpui::Window, &mut gpui::App) -> bool,
    ) -> Result<Option<gpui::WindowId>> {
        if !*self.initialized.lock().unwrap() {
            return Ok(None);
        }
        update_window_without_view(|window, cx| {
            pass(window, cx);
            window.window_handle().window_id()
        })
        .map(Some)
    }

    fn automation_bounds(&self) -> Result<HashMap<u64, crate::automation::ElementBounds>> {
        #[cfg(target_os = "macos")]
        {
            return Ok(self
                .draw_pending_changes()?
                .map(crate::automation::all_bounds)
                .unwrap_or_default());
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
            let window = self.draw_pending_changes()?;
            let tree = self.tree.lock().unwrap();
            return Ok(tree.elements_at(x, y, &|id| {
                window.and_then(|window| crate::automation::get_bounds(window, id))
            }));
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
            return Ok(self
                .draw_pending_changes()?
                .and_then(|window| crate::automation::get_bounds(window, id)));
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
        repaint: Repaint,
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
        repaint: Repaint,
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
            let viewport = window.viewport_size();
            view.motion_values
                .set_viewport(f64::from(f32::from(viewport.width)), f64::from(f32::from(viewport.height)));
            let result = run(view, now);
            let entity = cx.entity_id();
            repaint.invalidate(&view.motion_values, cx, entity);
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

    /// Applies the pointer lock request now where the window is on this thread,
    /// else at the next frame. `Window::request_pointer_lock` is a platform call
    /// that only macOS implements, and there GPUI runs on the JS thread.
    fn settle_pointer_lock(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        return update_window_or_defer(|view, window, cx| view.apply_pointer_lock(window, cx));

        #[cfg(not(target_os = "macos"))]
        self.request_invalidate()
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
            #[cfg(target_os = "macos")]
            frame_wake: FrameWake::default(),
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
                        crate::layout::container::allow_redraw_passes(window);
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
                                        crate::layout::container::allow_redraw_passes(window);
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
    ///   ["setUniversalStyle", { ...style } | null]
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
        let auto_focus = tree.take_auto_focus_request();
        drop(tree);
        // A re-render that changed nothing sends a batch of identical resends.
        // Waking the window for it makes it rebuild and diff the whole element
        // tree to draw the frame it already has.
        if dirty {
            self.request_invalidate()?;
        }
        if auto_focus {
            self.commit_auto_focus()?;
        }
        Ok(destroyed)
    }

    /// Windows and Linux render on their own thread, which cannot call JS
    /// synchronously; there the next frame focuses the element.
    fn commit_auto_focus(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        return update_window_without_view(|window, cx| {
            crate::focus::change_focus(window, cx, |view, window, cx| {
                view.commit_auto_focus(window, cx)
            })
        })?
        .map_err(Error::from_reason);

        #[cfg(not(target_os = "macos"))]
        Ok(())
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
            let result = crate::events::sync_dispatch::native_call(|| {
                let running = MAC_PLATFORM.with(|p| {
                    p.borrow()
                        .as_ref()
                        .map(|platform| platform.pump_events())
                        .unwrap_or(false)
                });
                #[cfg(feature = "test-support")]
                {
                    let delay = self
                        .frame_wake
                        .tick_delay_ms
                        .load(std::sync::atomic::Ordering::Relaxed);
                    if delay > 0 {
                        std::thread::sleep(Duration::from_millis(delay.into()));
                    }
                }
                Ok(running)
            });
            // After the frame, on every path: a wake that landed during it was dropped.
            self.frame_wake
                .pending
                .store(false, std::sync::atomic::Ordering::Release);
            return result;
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
    /// false on other platforms. The callback should schedule the tick rather than run
    /// it inline, and tick() never blocks. A wake that lands while a tick is running, or
    /// after another wake that has not been ticked yet, is dropped: the next frame waits
    /// for the next vsync. Wakes do not keep the process alive.
    /// Pass null to clear the callback. Already queued callbacks may still arrive.
    #[napi]
    pub fn set_frame_wake_callback(&self, callback: Option<Function<(), ()>>) -> Result<bool> {
        #[cfg(target_os = "macos")]
        {
            use std::sync::atomic::Ordering;

            let Some(callback) = callback else {
                self.frame_wake.clear();
                return Ok(true);
            };
            if !self.is_initialized() {
                return Err(Error::from_reason(
                    "Call init() before setting a frame wake callback",
                ));
            }
            let pending = self.frame_wake.pending.clone();
            pending.store(false, Ordering::Release);
            let wake = callback
                .build_threadsafe_function::<()>()
                .callee_handled::<false>()
                .weak::<true>()
                .build_callback(|_| Ok(()))?;
            let hook: FrameWakeHook = Arc::new(move || {
                if !pending.swap(true, Ordering::AcqRel)
                    && wake.call((), ThreadsafeFunctionCallMode::NonBlocking) != napi::Status::Ok
                {
                    pending.store(false, Ordering::Release);
                }
            });
            *self.frame_wake.hook.lock().unwrap() = Some(hook.clone());
            gpui_macos::set_frame_wake_hook(Some(hook));
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
        return update_window_without_view(|window, cx| crate::focus::step_focus(window, cx, false))?
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
        return update_window_without_view(|window, cx| crate::focus::step_focus(window, cx, true))?
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
        self.with_motion_values(Repaint::None, move |values, _now| values.create_source(id, &initial))
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
        self.with_motion_values(Repaint::None, move |values, _now| {
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
        self.with_motion_values(Repaint::None, move |values, now| {
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
        self.with_motion_values(Repaint::None, move |values, now| {
            values.create_spring(id, &initial, &config, now)
        })
    }

    /// Whether the value existed. Values that read it fail from then on.
    #[napi]
    pub fn dispose_motion_value(&self, id: f64) -> Result<bool> {
        self.with_motion_values(Repaint::None, move |values, _now| values.dispose(id))
    }

    #[napi]
    pub fn get_motion_value(&self, id: f64) -> Result<serde_json::Value> {
        self.with_motion_values(Repaint::None, move |values, now| values.get(id, now))
    }

    /// Units per second. Errors for a value that is not a number.
    #[napi]
    pub fn get_motion_value_velocity(&self, id: f64) -> Result<f64> {
        self.with_motion_values(Repaint::None, move |values, now| values.velocity(id, now))
    }

    #[napi]
    pub fn is_motion_value_animating(&self, id: f64) -> Result<bool> {
        self.with_motion_values(Repaint::None, move |values, now| values.is_animating(id, now))
    }

    /// Writes the value. Over a running animation this does not stop it:
    /// call `stopMotionValue` first to take the value from the animation.
    #[napi]
    pub fn set_motion_value(&self, id: f64, value: serde_json::Value) -> Result<()> {
        self.with_motion_values(Repaint::MotionWrite(id), move |values, now| values.set(id, &value, now))
    }

    /// Writes the value, ending any animation and dropping its velocity.
    #[napi]
    pub fn jump_motion_value(&self, id: f64, value: serde_json::Value) -> Result<()> {
        self.with_motion_values(Repaint::MotionWrite(id), move |values, now| values.jump(id, &value, now))
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
        self.with_motion_values(Repaint::MotionWrite(id), move |values, now| {
            values.animate(id, &target, transition.as_ref(), now)
        })
    }

    /// Ends an animation where it is, keeping the speed for the next one.
    #[napi]
    pub fn stop_motion_value(&self, id: f64) -> Result<()> {
        self.with_motion_values(Repaint::MotionWrite(id), move |values, now| values.stop(id, now))
    }

    /// Holds the value's animation where it is. The value stays, its velocity
    /// reads 0 and no frame is asked for; `isMotionValueAnimating` stays true.
    #[napi]
    pub fn pause_motion_value(&self, id: f64) -> Result<()> {
        self.with_motion_values(Repaint::MotionWrite(id), move |values, now| values.pause(id, now))
    }

    /// Continues a held animation from the time it holds. A finished one plays
    /// again from its start, or from its end when its speed is negative.
    #[napi]
    pub fn play_motion_value(&self, id: f64) -> Result<()> {
        self.with_motion_values(Repaint::MotionWrite(id), move |values, now| values.play(id, now))
    }

    /// Moves the value's animation to `seconds` after its delay ended. A
    /// finished animation is held there; one seeked past its end finishes.
    #[napi]
    pub fn seek_motion_value(&self, id: f64, seconds: f64) -> Result<()> {
        self.with_motion_values(Repaint::MotionWrite(id), move |values, now| {
            values.seek(id, seconds, now)
        })
    }

    /// Sets the pace of the value's animation: 2 is twice as fast, a negative
    /// speed runs it backwards to its first keyframe, 0 holds it while it stays
    /// in effect.
    #[napi]
    pub fn set_motion_value_speed(&self, id: f64, speed: f64) -> Result<()> {
        self.with_motion_values(Repaint::MotionWrite(id), move |values, now| {
            values.set_speed(id, speed, now)
        })
    }

    /// Jumps the value's animation to its last keyframe (the first when it runs
    /// backwards) and finishes it.
    #[napi]
    pub fn complete_motion_value(&self, id: f64) -> Result<()> {
        self.with_motion_values(Repaint::MotionWrite(id), move |values, now| values.complete(id, now))
    }

    /// What the controls of the value's animation read, or null if it has none:
    /// it was never animated, or was stopped or jumped.
    #[napi]
    pub fn get_motion_value_playback(&self, id: f64) -> Result<Option<MotionValuePlayback>> {
        self.with_motion_values(Repaint::None, move |values, now| {
            values.playback(id, now).map(|info| info.map(MotionValuePlayback::from))
        })
    }

    /// Chooses which events the value delivers. Neither removes the listener.
    #[napi]
    pub fn set_motion_value_listener(&self, id: f64, change: bool, animation: bool) -> Result<()> {
        self.with_motion_values(Repaint::None, move |values, now| {
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
        self.with_motion_state(Repaint::Layout, move |view, _now| {
            view.keyframes.define(&name, &frames_json)
        })
    }

    /// The CSS transitions, then the CSS animations, of an element that are
    /// running, paused or in effect through a fill, in the order of the DOM's
    /// `getAnimations()`. Animations of the motion engine are not listed.
    #[napi]
    pub fn get_animations(&self, element_id: f64) -> Result<Vec<WebAnimationInfo>> {
        let id = to_element_id(element_id)?;
        self.with_motion_state(Repaint::None, move |view, _now| Ok(view.web_animations(id)))
    }

    /// One CSS animation or transition by the id `getAnimations` gave. A run
    /// that finished or was cancelled answers as it ended until
    /// `releaseAnimation` says JS consumed its `webanimationfinish` or
    /// `webanimationcancel` event; after that, and for a transition whose style
    /// moved off its end value, it throws.
    #[napi]
    pub fn get_animation(&self, animation_id: f64) -> Result<WebAnimationInfo> {
        let id = to_element_id(animation_id)?;
        self.with_motion_state(Repaint::None, move |view, _now| view.web_animation(id))
    }

    /// JS consumed one `webanimationfinish` or `webanimationcancel` event of the
    /// run: native stops answering for what it ended as. Call it for every such
    /// event, whether or not an `Animation` object exists for the run.
    #[napi]
    pub fn release_animation(&self, animation_id: f64) -> Result<()> {
        let id = to_element_id(animation_id)?;
        self.with_motion_state(Repaint::None, move |view, _now| {
            view.release_web_animation(id);
            Ok(())
        })
    }

    /// `animation.pause()`: holds the current time at once, and from here the
    /// style's `animation-play-state` no longer decides.
    #[napi]
    pub fn pause_animation(&self, animation_id: f64) -> Result<()> {
        self.command_animation(animation_id, crate::motion::web_animation::Command::Pause)
    }

    /// `animation.play()`: continues from the held time at the next frame, or
    /// from the start (the end when running backwards) if it has finished.
    #[napi]
    pub fn play_animation(&self, animation_id: f64) -> Result<()> {
        self.command_animation(animation_id, crate::motion::web_animation::Command::Play)
    }

    /// `animation.currentTime = milliseconds`.
    #[napi]
    pub fn set_animation_current_time(&self, animation_id: f64, milliseconds: f64) -> Result<()> {
        self.command_animation(
            animation_id,
            crate::motion::web_animation::Command::SetCurrentTime(milliseconds),
        )
    }

    /// `animation.playbackRate = rate`, keeping the current time.
    #[napi]
    pub fn set_animation_playback_rate(&self, animation_id: f64, rate: f64) -> Result<()> {
        self.command_animation(
            animation_id,
            crate::motion::web_animation::Command::SetPlaybackRate(rate),
        )
    }

    fn command_animation(
        &self,
        animation_id: f64,
        command: crate::motion::web_animation::Command,
    ) -> Result<()> {
        let id = to_element_id(animation_id)?;
        self.with_motion_state(Repaint::Layout, move |view, _now| view.command_web_animation(id, command))
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
                if view.scroll_element_into_view(window, id, options) {
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
            return Ok(self
                .draw_pending_changes()?
                .and_then(|window| scroll_metrics(window, id))
                .map(|metrics| metrics.to_vec()));
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

    /// Capture the mouse for `element_id`. The request is recorded before this
    /// returns, so `hasPointerCapture` answers true and the next pointer input
    /// is already aimed at the element; that input fires `gotpointercapture`
    /// before its own events, as the DOM does. Ignored while no button is down.
    #[napi]
    pub fn set_pointer_capture(&self, element_id: f64, pointer_id: f64) -> Result<()> {
        let id = to_element_id(element_id)?;
        let tree = self.tree.lock().unwrap();
        self.pointer_requests
            .set(&tree, id, pointer_id)
            .map_err(Error::from_reason)
    }

    /// End `element_id`'s capture, if it holds one: recorded now, and the next
    /// pointer input fires `lostpointercapture`.
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
    /// `dragstart` was cancelled. Recorded now and read first by the next
    /// pointer input, before it builds an event; the moves delivered since
    /// `dragstart` were already queued, and JS drops them.
    #[napi]
    pub fn cancel_drag(&self) {
        self.pointer_requests.cancel_drag();
    }

    /// Lock the pointer to `element_id`: the cursor hides and stops, and
    /// movement is reported as `movementX`/`movementY`. On macOS the lock is
    /// taken before this returns, from a handler too, and it fires
    /// `pointerlockchange` or, when the element is not connected or the platform
    /// refuses, `pointerlockerror`. Elsewhere the next frame applies it.
    #[napi]
    pub fn request_pointer_lock(&self, element_id: f64) -> Result<()> {
        let id = to_element_id(element_id)?;
        self.pointer_requests.request_lock(id);
        self.settle_pointer_lock()
    }

    /// Release the pointer lock, if held. Applied like `requestPointerLock`.
    #[napi]
    pub fn exit_pointer_lock(&self) -> Result<()> {
        self.pointer_requests.exit_lock();
        self.settle_pointer_lock()
    }

    /// The element that holds the pointer lock, or null. On macOS it is the
    /// element right after `requestPointerLock`.
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

    /// Encoded bytes for the `blob:` URL a `url()` background image names. The
    /// JS layer fetches the blob, because a blob URL only exists in its process.
    #[napi]
    pub fn set_style_image(&self, url: String, bytes: Buffer) -> Result<()> {
        let bytes = bytes.to_vec();
        #[cfg(target_os = "macos")]
        {
            return update_window(move |_, window, cx| {
                crate::background_images::deliver_to_view(&url, bytes, window, cx)
            })?
            .map_err(Error::from_reason);
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (url, bytes);
            Err(Error::from_reason("setStyleImage is only implemented on macOS"))
        }
    }

    /// The JS layer could not read the `blob:` URL a `url()` background names.
    #[napi]
    pub fn fail_style_image(&self, url: String, message: String) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            return update_window(move |_, window, cx| {
                crate::background_images::fail_in_view(&url, message, window, cx)
            })?
            .map_err(Error::from_reason);
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (url, message);
            Err(Error::from_reason("failStyleImage is only implemented on macOS"))
        }
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
    pub fn get_painted_text(&self) -> Result<Vec<String>> {
        #[cfg(target_os = "macos")]
        {
            return Ok(self
                .paint_pending_changes()?
                .map(crate::text::painted_text)
                .unwrap_or_default());
        }

        #[cfg(not(target_os = "macos"))]
        Err(Error::from_reason("Unsupported operating system"))
    }

    /// Every highlight wash painted in the last frame, in paint order.
    ///
    /// A quad is invisible to `getPaintedText()`, so this is the only way to
    /// assert on `highlight` without a screenshot.
    #[napi]
    pub fn get_painted_highlights(&self) -> Result<Vec<crate::element_tree::HighlightMatch>> {
        #[cfg(target_os = "macos")]
        {
            return Ok(self
                .paint_pending_changes()?
                .map(crate::text::painted_highlights)
                .unwrap_or_default()
                .into_iter()
                .map(Into::into)
                .collect());
        }

        #[cfg(not(target_os = "macos"))]
        Err(Error::from_reason("Unsupported operating system"))
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

#[cfg(all(feature = "test-support", not(all(target_arch = "wasm32", target_os = "unknown"))))]
#[napi]
impl GpuixRenderer {
    /// Calls the registered frame wake closure from a Rust thread every `intervalMs`, the
    /// function the display link calls, so a hidden window gets deterministic wakes.
    /// `null` stops it; clearing the wake callback stops it too.
    #[napi]
    pub fn simulate_frame_wakes(&self, interval_ms: Option<f64>) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            self.frame_wake.stop_simulator();
            let Some(interval_ms) = interval_ms else {
                return Ok(());
            };
            if !(interval_ms > 0.0) {
                return Err(Error::from_reason("intervalMs must be greater than 0"));
            }
            let simulator = WakeSimulator::start(
                Duration::from_secs_f64(interval_ms / 1000.0),
                self.frame_wake.hook.clone(),
            );
            *self.frame_wake.simulator.lock().unwrap() = Some(simulator);
            Ok(())
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = interval_ms;
            Err(Error::from_reason("simulateFrameWakes needs macOS"))
        }
    }

    /// Makes every `tick()` sleep `ms` inside its native call, a forced-slow frame. 0 turns it off.
    #[napi]
    pub fn set_tick_delay(&self, ms: u32) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            self.frame_wake
                .tick_delay_ms
                .store(ms, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = ms;
            Err(Error::from_reason("setTickDelay needs macOS"))
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
                crate::layout::container::allow_redraw_passes(window);
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

/// The web window, where its painted boxes are. `None` before it exists: nothing was painted.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn web_window_id() -> Option<gpui::WindowId> {
    WEB_WINDOW.with(|window| window.borrow().as_ref().map(|window| window.window_id()))
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
        update_web_window(|view, window, cx| {
            window.focus_next(cx);
            view.scroll_focus_into_view(window, cx);
        })
    }

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = focusPrevious)]
    pub fn focus_previous(&self) -> Result<(), wasm_bindgen::JsValue> {
        update_web_window(|view, window, cx| {
            window.focus_prev(cx);
            view.scroll_focus_into_view(window, cx);
        })
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
            if view.scroll_element_into_view(window, id, options) {
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
        let bounds = web_window_id()
            .map(crate::automation::all_bounds)
            .unwrap_or_default();
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
        let id = web_element_id(element_id)?;
        let Some(bounds) = web_window_id().and_then(|window| crate::automation::get_bounds(window, id))
        else {
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
        web_string_array(web_window_id().map(crate::text::painted_text).unwrap_or_default())
    }

    /// The same array of objects the napi build returns.
    ///
    /// Through `serde_json` and `JSON.parse`, not `serde-wasm-bindgen`: this is
    /// a test-only API, and both crates here are already dependencies. Building
    /// the nested value by hand with `js_sys` is 20 lines of noise.
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = getPaintedHighlights)]
    pub fn get_painted_highlights(&self) -> wasm_bindgen::JsValue {
        let matches: Vec<crate::element_tree::HighlightMatch> = web_window_id()
            .map(crate::text::painted_highlights)
            .unwrap_or_default()
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
        {
            if *self.initialized.lock().unwrap() {
                gpui_macos::set_frame_wake_hook(None);
            }
            self.frame_wake.hook.lock().unwrap().take();
            #[cfg(feature = "test-support")]
            self.frame_wake.stop_simulator();
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
    resync_by_frame: bool,
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
    pub(crate) css_transitions: crate::motion::CssTransitions,
    /// CSS animation state of the elements that declare `animation*`.
    pub(crate) css_animations: crate::motion::CssAnimations,
    /// Runs that finished or were cancelled, until JS has consumed the event.
    pub(crate) retired_animations: crate::motion::web_animation::Retired,
    /// What the `display: contents` elements handed down in the frame being built, see `splice_overlay`.
    contents_styles: HashMap<u64, Arc<ContentsFrame>>,
    /// What the last paint measured for the elements that animate.
    pub(crate) measurements: crate::motion::measure::Measurements,
    /// Hover and press state for those elements, until the event model owns it.
    /// The glide of the elements that declare `layout`.
    pub(crate) layout_flips: crate::motion::LayoutFlips,
    /// Serialised markup of the `<svg>` elements that have shape children.
    pub(crate) svg_trees: crate::custom_elements::svg_tree::SvgTrees,
    /// Live text selection, shared with the paint closures and the napi methods.
    pub(crate) selection: SharedSelection,
    /// Persistent measurement and scroll state for React-backed virtual lists.
    virtual_lists: HashMap<u64, VirtualListEntry>,
    /// Latest pointer sample and list during selection edge scrolling.
    selection_drag_position: Option<(gpui::WindowId, gpui::Point<gpui::Pixels>)>,
    selection_scroll_list: Option<u64>,
    selection_scroll_task: Option<gpui::Task<()>>,
    /// Motion / review clock. Live wall time unless automation freezes it.
    pub(crate) clock: crate::automation::AutomationClock,
    /// The instant the last pass that was not a verifier's reference was built at, or the last
    /// dormant tick ran at.
    pub(crate) built_at: Option<web_time::Instant>,
    /// A frame callback that carries the dormant animations on is waiting, see `dormant_tick`.
    dormant_tick_armed: bool,
    /// Whether a tick of paint-only animations is drawn by patching the frame before it.
    pub(crate) patch: crate::retain::Patching,
    /// Resolved `highlight` state, keyed by the element that declared it.
    /// Empty in every app that does not use search.
    highlights: HashMap<u64, HighlightCacheEntry>,
    /// What `onLayout` and `onScroll` last reported, for the elements that
    /// have the listener. Shared with the paint closures that compare against it.
    paint_reports: SharedPaintReports,
    /// The content box layout last produced for each query container, which the next build
    /// reads: see `layout::container`.
    containers: crate::layout::container::ContainerStore,
    /// What the last build recorded of each element, for deciding what a replay could reuse.
    pub(crate) retain: crate::retain::Retain,
    /// What the retained roots wrote into the registries the last time they were built.
    pub(crate) tapes: crate::tape::Tapes,
    /// Where the pointer is and what it is pressing.
    /// Shared with the window's mouse-down listener, which dispatches a press
    /// without reaching the view.
    pub(crate) pointer: Rc<RefCell<crate::events::state::PointerState>>,
}

#[derive(Default)]
pub(crate) struct PaintReports {
    /// Scroll offset last seen for `onScroll`. Recorded on the first paint
    /// without an event, so only movement is reported.
    scrolls: HashMap<u64, (f32, f32)>,
    /// The scrolls the frame painted, in paint order, until the events are emitted: by the sweep of
    /// the frame (`observers::frame_sweep`), which emits them before it reports the boxes, or by a
    /// deferred flush if nothing listens for boxes.
    pending_scrolls: Vec<(u64, (f32, f32))>,
    scroll_flush_queued: bool,
    /// What `onLayout`, `ResizeObserver` and `IntersectionObserver` last saw.
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

pub(crate) fn emit_motion_settled(
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
pub(crate) fn sync_motion_values(
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
pub(crate) fn emit_motion_value_events(
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
        return root.clone();
    };
    if let Some(inherited) = known.get(&parent) {
        return inherited.clone();
    }
    let above = parent_inherited(tree, parent, root, known);
    let style = tree.elements.get(&parent).and_then(|element| element.style.as_deref());
    let inherited = match style {
        Some(style) => (above.0.descend(style), above.1.descend(Some(style))),
        None => (above.0, above.1.descend(None)),
    };
    known.insert(parent, inherited.clone());
    inherited
}

/// Which gestures are on an element: the states its `hover`, `active` and `focusVisible` styles
/// answer to, whether or not it declares them, in a subtree that takes input. Targets that name
/// a variant of a controlling ancestor answer to that ancestor's gestures.
pub(crate) fn gestures_of(
    tree: &crate::retained_tree::RetainedTree,
    pointer: &crate::events::state::PointerState,
    focus: &crate::focus::FocusStyles,
    id: u64,
) -> crate::motion::GestureStates {
    let states = |id: u64| {
        let live = !tree.elements.get(&id).is_some_and(crate::retained_tree::RetainedElement::has_inert_prop);
        crate::motion::Gestures {
            hover: live && pointer.is_hovered(id),
            tap: live && pointer.is_active(id),
            focus: focus.is_focus_visible(id),
            ..Default::default()
        }
    };
    crate::motion::GestureStates {
        own: states(id),
        controller: crate::motion::gesture_controller(tree, id).map(states).unwrap_or_default(),
    }
}

pub(crate) fn sync_motion_states(
    tree: &crate::retained_tree::RetainedTree,
    states: &mut HashMap<u64, crate::motion::MotionState>,
    measurements: &crate::motion::measure::Measurements,
    appearance: gpui::WindowAppearance,
    viewport: gpui::Size<gpui::Pixels>,
    gestures: &dyn Fn(u64) -> crate::motion::GestureStates,
    now: web_time::Instant,
) -> (Vec<u64>, Vec<(u64, u64)>) {
    let painted = measurements.painted_frames();
    let mut root_font = crate::motion::ParentFont::initial();
    root_font.lengths = LengthContext::initial(
        f64::from(f32::from(viewport.width)),
        f64::from(f32::from(viewport.height)),
    );
    let root = (crate::color::ColorContext::root(color_scheme_of(appearance)), root_font);
    let mut known_colors = HashMap::new();
    states.retain(|id, _| tree.motion_ids.contains(id));
    let mut animating = Vec::new();
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
        let inline = element.element_type == "span";
        let state = match states.entry(id) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let created = match svg_attributes {
                    Some(attributes) => {
                        crate::motion::MotionState::new_svg(source, attributes, colors.clone(), now)
                    }
                    None if inline => crate::motion::MotionState::new_inline(source, base, colors.clone(), now),
                    None => crate::motion::MotionState::new(
                        source,
                        base,
                        colors.clone(),
                        measurement.as_ref(),
                        painted,
                        now,
                    ),
                };
                match created {
                    Ok(state) => entry.insert(state),
                    Err(error) => {
                        log::warn!("Invalid motion description for element {id}: {error}");
                        entry.insert(crate::motion::MotionState::invalid(source, colors.clone(), now))
                    }
                }
            }
        };
        let synced = match svg_attributes {
            Some(attributes) => state.sync_svg(source, attributes, colors.clone(), now),
            None if inline => state.sync_inline(source, base, colors.clone(), now),
            None => state.sync(source, base, colors, measurement.as_ref(), painted, now),
        };
        if let Err(error) = synced {
            log::warn!("Invalid motion update for element {id}: {error}");
        }
        let gesture_base = match svg_attributes {
            Some(attributes) => crate::motion::GestureBase::Svg(attributes),
            None => crate::motion::GestureBase::Style(base),
        };
        let gesturing = state.set_gestures(gestures(id), gesture_base, measurement.as_ref(), painted, now);
        if let Err(error) = gesturing {
            log::warn!("Invalid motion gesture for element {id}: {error}");
        }
        let frame = state.frame(now);
        if frame.active {
            animating.push(id);
        }
        if frame.just_settled {
            settled.push((id, frame.generation));
        }
    }

    (animating, settled)
}

/// Emits a DOM event that bubbles and cannot be cancelled, aimed at `id`, when
/// the element or an ancestor listens for it.
pub(crate) fn emit_bubbling_event(
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

pub(crate) fn emit_transition_events(
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

pub(crate) fn emit_layout_events(
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

pub(crate) fn emit_animation_events(
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

/// Tells JS a CSS animation or transition finished or was cancelled. The
/// payload has no path, so it reaches the element's own handlers only and no
/// ancestor, and it is not subject to a listener check: the `Animation` object
/// of `getAnimations()` listens by existing.
///
/// The run is retired as it showed then, so JS can still read it until it has
/// consumed the event (`releaseAnimation`).
pub(crate) fn emit_web_animation_events(
    callback: &Option<EventCallback>,
    retired: &mut crate::motion::web_animation::Retired,
    events: Vec<crate::motion::web_animation::ApiEvent>,
) {
    if callback.is_none() {
        return;
    }
    for event in events {
        retired.retire(event.info);
        emit_event_full(callback, event.element, event.kind.event_type(), |payload| {
            payload.animation_id = Some(event.run as f64);
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
    /// The Web Animations of `element`: its CSS transitions, then its CSS
    /// animations, each in composite order, as of the last frame.
    pub(crate) fn web_animations(&self, element: u64) -> Vec<WebAnimationInfo> {
        let Some(now) = self.built_at else {
            return Vec::new();
        };
        let transitions = self.css_transitions.list(element, now);
        let animations = self.css_animations.list(element, now);
        transitions.into_iter().chain(animations).map(WebAnimationInfo::from).collect()
    }

    /// The run `id` as it is now, or as it showed when its finish or cancel
    /// was reported while JS has not consumed that event.
    pub(crate) fn web_animation(&self, id: u64) -> std::result::Result<WebAnimationInfo, String> {
        self.built_at
            .and_then(|now| {
                self.css_transitions.info(id, now).or_else(|| self.css_animations.info(id, now))
            })
            .or_else(|| self.retired_animations.get(id).cloned())
            .map(WebAnimationInfo::from)
            .ok_or_else(|| unknown_web_animation(id))
    }

    /// JS consumed an event of the run `id`.
    pub(crate) fn release_web_animation(&mut self, id: u64) {
        self.retired_animations.release(id);
    }

    /// Applies `command` at the time of the last frame, which is the time the
    /// animation reads as. The caller requests the frame that paints it.
    pub(crate) fn command_web_animation(
        &mut self,
        id: u64,
        command: crate::motion::web_animation::Command,
    ) -> std::result::Result<(), String> {
        let outcome = self.built_at.and_then(|now| {
            self.css_transitions
                .command(id, command, now)
                .or_else(|| self.css_animations.command(id, command, now))
        });
        outcome.unwrap_or_else(|| Err(unknown_web_animation(id)))
    }

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
            resync_by_frame: false,
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
            retired_animations: Default::default(),
            contents_styles: Default::default(),
            measurements: Default::default(),
            layout_flips: Default::default(),
            svg_trees: Default::default(),
            selection,
            virtual_lists: HashMap::new(),
            selection_drag_position: None,
            selection_scroll_list: None,
            selection_scroll_task: None,
            clock: crate::automation::AutomationClock::new(),
            built_at: None,
            dormant_tick_armed: false,
            patch: crate::retain::Patching::new(),
            highlights: HashMap::new(),
            paint_reports: SharedPaintReports::default(),
            containers: Default::default(),
            retain: crate::retain::Retain::new(),
            tapes: Default::default(),
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
    pub(crate) fn scroll_element_into_view(
        &self,
        window: &gpui::Window,
        id: u64,
        options: crate::scroll::IntoView,
    ) -> bool {
        let window_id = window.window_handle().window_id();
        let mut target = crate::automation::get_bounds(window_id, id).map(|painted| crate::scroll::Rect {
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
                    let row = crate::automation::get_bounds(window_id, child);
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

    pub(crate) fn resync_hover_by_frame(&mut self) {
        self.resync_by_frame = true;
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
    /// `pointerlockchange` and `pointerlockerror`. Document-level events, so
    /// they name no element: JS reads the lock element from the renderer.
    pub(crate) fn apply_pointer_lock(&mut self, window: &mut gpui::Window, cx: &mut gpui::App) {
        use crate::events::pointer_lock::{apply, GpuiLock};
        let events = {
            let tree = self.tree.lock().unwrap();
            match self.simulated_platform.clone() {
                Some(mut platform) => apply(&mut self.pointer.borrow_mut(), &tree, &mut platform),
                None => apply(&mut self.pointer.borrow_mut(), &tree, &mut GpuiLock { window, cx }),
            }
        };
        for event in events {
            emit_event_full(&self.event_callback, 0, event.event_type(), |_| {});
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
            self.pointer.clone(),
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
        list_path: Vec<u32>,
        list_hosts: Arc<[crate::text::selection::Host]>,
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

        let mut row_path = list_path;
        row_path.push(index as u32);
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
        let focus_styles = self.focus.styles(&tree, tree.keyboard_focus_dim());
        let pointer_state = self.pointer.clone();
        let pointer = pointer_state.borrow();
        let (motion_animating, motion_settled) =
            sync_motion_states(
                &tree,
                &mut self.motion_states,
                &self.measurements,
                window.appearance(),
                window.viewport_size(),
                &|id| gestures_of(&tree, &pointer, &focus_styles, id),
                now,
            );
        let motion_active = !motion_animating.is_empty();
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

        crate::accessibility::links::sync(&tree);
        let activation = self.activation();
        crate::layout::box_generation::hold_display(self.css_transitions.held_displays());
        let mut build_ctx = self.build_ctx(
            &tree,
            &callback,
            &activation,
            &pointer,
            &mut highlight_events,
            BuildStart {
                inherited,
                doc_path: row_path,
                doc_hosts: list_hosts,
                now,
                stacking_root: true,
                stacking_item: false,
                text_box_trim: Default::default(),
                focus_styles,
                control_styles: Default::default(),
                scroller: None,
            },
        );
        build_ctx.retain.enter_row();
        let child = build_element(expected_child_id, &mut build_ctx, window, cx);
        build_ctx.retain.leave_row();
        drop(pointer);
        emit_highlight_events(&callback, &highlight_events);
        emit_motion_settled(&callback, &tree, &motion_settled);
        emit_transition_events(&callback, &tree, self.css_transitions.take_events());
        emit_animation_events(&callback, &tree, self.css_animations.take_events());
        emit_web_animation_events(&callback, &mut self.retired_animations, self.css_transitions.take_api_events());
        emit_web_animation_events(&callback, &mut self.retired_animations, self.css_animations.take_api_events());
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

    /// The scroll handle of `id` unless the element has no box (`display: none` on it or above):
    /// there is nothing to scroll, and a script's `scrollTo` or `scrollTop` does nothing.
    pub(crate) fn scrollable_handle(&self, id: u64) -> Option<&gpui::ScrollHandle> {
        let handle = self.scroll_handles.get(&id)?;
        crate::scroll::has_scrolling_box(&self.tree.lock().unwrap(), id).then_some(handle)
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
    contents_styles: &'a mut HashMap<u64, Arc<ContentsFrame>>,
    keyframes: &'a crate::motion::KeyframesRegistry,
    measurements: &'a crate::motion::measure::Measurements,
    layout_flips: &'a crate::motion::LayoutFlips,
    svg_trees: &'a mut crate::custom_elements::svg_tree::SvgTrees,
    pub now: web_time::Instant,
    pub selection: SharedSelection,
    /// Child indices from the root to the element being built, in the retained tree: the document
    /// position selection and find order text by. A virtual-list row starts at its list's.
    pub doc_path: Vec<u32>,
    /// The paragraphs whose atomic inlines the element being built is inside, see
    /// [`crate::text::selection::DocPath`].
    pub doc_hosts: Arc<[crate::text::selection::Host]>,
    /// The line of text the inline content of the atomic inline being built continues, see
    /// [`crate::text::selection::RegisteredText::line`]. The first paragraph the box builds takes it.
    pub inline_line: Option<Arc<str>>,
    /// Inherited text state, resolved the way CSS inherits it. The renderer's
    /// own theme only seeds the root selection wash; custom elements resolve
    /// their own theme from their `theme` prop.
    pub inherited: Inherited,
    /// Persistent `highlight` caches, keyed by the declaring element.
    highlights: &'a mut HashMap<u64, HighlightCacheEntry>,
    paint_reports: &'a SharedPaintReports,
    containers: &'a crate::layout::container::ContainerStore,
    /// `onHighlight` payloads queued during the build.
    ///
    /// Never emitted inline: a handler that calls `setState` repaints, which
    /// would re-enter the build and emit again. They are flushed once the root
    /// build has returned.
    highlight_events: &'a mut Vec<(u64, usize)>,
    /// See `crate::focus::FocusState::styles`.
    pub focus_styles: crate::focus::FocusStyles,
    /// The ancestors of the form controls, for `hasDisabled` and `hasEnabled`.
    pub control_styles: crate::retained_tree::ControlStyles,
    /// Which elements are hovered or pressed, for their `hover` / `active` styles.
    pub pointer: &'a crate::events::state::PointerState,
    /// Inheritable style of the `display: contents` ancestors of the child about to be
    /// built, taken by `build_element`.
    pub contents_overlay: Option<Box<StyleDesc>>,
    /// `layout::contents::Overlay::key` of the overlay, set with it.
    pub contents_key: u64,
    /// What runs on the `display: contents` elements of the overlay (`retain::Dynamic` bits), set
    /// with it.
    pub contents_dynamic: u8,
    /// Set to an `<anchored>` element's id while building its direct children, taken by
    /// `build_element` to resolve `anchor-size()`.
    pub anchor_scope: Option<u64>,
    /// Set while building the first element of a build (the window root, a virtual-list row),
    /// which has no stacking context around it. Taken by `build_element`.
    pub stacking_root: bool,
    /// Set while building a child of a flex or grid container, which its `z-index` applies to
    /// without a `position`. Taken by `build_element`.
    pub stacking_item: bool,
    /// Set while building an atomic inline, taken by `build_element`, which takes the
    /// margin and a percentage width off its style into `atomic_extracted`.
    pub atomic_root: bool,
    pub atomic_extracted: Option<crate::text::atomic::Extracted>,
    /// Whether the block container the atomic inline is in has a definite height, which a
    /// percentage `top` or `bottom` of its `position: relative` is of.
    pub atomic_height_definite: bool,
    /// The cells the atomic inlines of the paragraphs of the host being built read the
    /// content height of their containing block from, which the host writes in prepaint.
    pub containing_heights: Vec<Rc<Cell<gpui::Pixels>>>,
    /// The flex or grid container whose first item `build_content` is to watch, for the
    /// baseline of an `inline-flex` or `inline-grid` box.
    pub baseline_probe: Option<crate::text::atomic::Probe>,
    /// What the tables with collapsed borders built so far gave their parts.
    pub collapsed: crate::layout::table::Collapsed,
    /// The ends of the lines of the next element built that an ancestor's `text-box-trim`
    /// reaches through the blocks between. Taken by `build_element`.
    pub text_box_trim: gpui::TextBoxTrim,
    /// What the element being built has under it so far: how far it draws past its boxes, and
    /// whether gpui may skip it outside its clip. See `layout::cull`.
    pub subtree: crate::layout::cull::Subtree,
    /// The scroller the element being built is inside of, see `retain::Scroller`.
    pub scroller: Option<crate::retain::Scroller>,
    /// What this build is recording for the next, see `retain`.
    pub retain: &'a mut crate::retain::Retain,
    /// See `GpuixView::tapes`.
    tapes: &'a crate::tape::Tapes,
}

impl BuildCtx<'_> {
    /// Where the element being built sits in the document, which orders its text in selection and find.
    pub fn doc_path(&self) -> crate::text::selection::DocPath {
        crate::text::selection::DocPath::new(self.doc_hosts.clone(), self.doc_path.as_slice().into())
    }

    /// The states of `id` that `style` has a refinement for. Nothing applies in
    /// an inert subtree: nothing in it can be hovered, and it also ignores a
    /// group outside it.
    fn pointer_states(
        &self,
        id: u64,
        style: &StyleDesc,
        inert: bool,
    ) -> crate::events::state_styles::PointerStates {
        pointer_states_of(
            &self.inherited,
            self.pointer,
            &self.focus_styles,
            ControlSource { tree: self.tree, styles: &self.control_styles },
            id,
            style,
            inert,
        )
    }

    /// The states of an inline box, for the style it shows.
    fn inline_states(&self) -> impl Fn(u64, &StyleDesc) -> crate::events::state_styles::PointerStates + '_ {
        move |id: u64, style: &StyleDesc| {
            let inert = self
                .tree
                .elements
                .get(&id)
                .is_some_and(crate::retained_tree::RetainedElement::has_inert_prop);
            self.focus_styles
                .states_of(id, style, self.pointer_states(id, style, inert))
        }
    }

    /// The transitions and animations of the colours of the inline boxes of
    /// `paragraph`, stepped to this frame.
    fn inline_styles(&mut self, paragraph: &crate::text::inline::Paragraph) -> crate::motion::InlineStyles {
        // Field by field: the closure reads what the timeline does not borrow.
        let tree = self.tree;
        let inherited = &self.inherited;
        let pointer = self.pointer;
        let focus_styles = &self.focus_styles;
        let controls = &self.control_styles;
        let states = |id: u64, style: &StyleDesc| {
            let inert = tree
                .elements
                .get(&id)
                .is_some_and(crate::retained_tree::RetainedElement::has_inert_prop);
            focus_styles.states_of(
                id,
                style,
                pointer_states_of(
                    inherited,
                    pointer,
                    focus_styles,
                    ControlSource { tree, styles: controls },
                    id,
                    style,
                    inert,
                ),
            )
        };
        crate::motion::InlineStyles::resolve(
            tree,
            paragraph,
            &inherited.colors,
            &inherited.parent_font(),
            &states,
            crate::motion::Timeline {
                values: &*self.motion_values,
                transitions: &mut *self.css_transitions,
                animations: &mut *self.css_animations,
                keyframes: self.keyframes,
                motion: &*self.motion_states,
                now: self.now,
            },
        )
    }
}

/// Where `:disabled` and `:enabled` are read from.
struct ControlSource<'a> {
    tree: &'a crate::retained_tree::RetainedTree,
    styles: &'a crate::retained_tree::ControlStyles,
}

impl ControlSource<'_> {
    /// `disabled`, `enabled`, `hasDisabled` and `hasEnabled` of `id`, for the ones `style` declares.
    /// Not pointer state: an `inert` subtree keeps them, as `:disabled` still matches in one.
    fn states(&self, id: u64, style: &StyleDesc) -> crate::events::state_styles::PointerStates {
        let control = (style.disabled.is_some() || style.enabled.is_some())
            .then(|| self.tree.elements.get(&id)?.control_disabled())
            .flatten();
        crate::events::state_styles::PointerStates {
            disabled: style.disabled.is_some() && control == Some(true),
            enabled: style.enabled.is_some() && control == Some(false),
            has_disabled: style.has_disabled.is_some() && self.styles.has_disabled(self.tree, id),
            has_enabled: style.has_enabled.is_some() && self.styles.has_enabled(self.tree, id),
            ..Default::default()
        }
    }
}

fn pointer_states_of(
    inherited: &Inherited,
    pointer: &crate::events::state::PointerState,
    focus: &crate::focus::FocusStyles,
    controls: ControlSource,
    id: u64,
    style: &StyleDesc,
    inert: bool,
) -> crate::events::state_styles::PointerStates {
    let controls = controls.states(id, style);
    if inert || inherited.inert {
        return controls;
    }
    let followed = |refinement: &Option<Box<StyleDesc>>| {
        refinement
            .as_deref()
            .map(|refinement| inherited.group_state(&refinement.followed_group()))
    };
    crate::events::state_styles::PointerStates {
        hovered: style.hover.is_some() && pointer.is_hovered(id),
        active: style.active.is_some() && pointer.is_active(id),
        group_hovered: followed(&style.group_hover).is_some_and(|group| group.hovered),
        group_active: followed(&style.group_active).is_some_and(|group| group.active),
        group_focused_within: followed(&style.group_focus_within)
            .is_some_and(|group| group.focused_within),
        has_hovered: style.has_hover.is_some() && pointer.has_hovered(id),
        has_active: style.has_active.is_some() && pointer.has_active(id),
        has_focused: style.has_focus.is_some() && focus.has_focus(id),
        ..controls
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
    pub(crate) frameless: bool,
    /// Computed `color` and `colorScheme`, which `currentColor` and
    /// `light-dark()` resolve against.
    pub colors: crate::color::ColorContext,
    /// Computed `font-size`, root font size and viewport, which `em`, `rem` and
    /// the viewport units resolve against. The viewport is read again on every
    /// build, including a virtual list's rows.
    pub lengths: LengthContext,
    /// Computed `font-weight`, which an unset one of a transition or motion starts from.
    pub font_weight: f64,
    /// Computed `text-underline-offset`, which an underline takes from the box
    /// that declares it.
    pub underline_offset: crate::css::text_underline_offset::TextUnderlineOffset,
    /// Computed `visibility` is hidden, which an unset one of a transition or motion starts from.
    pub visibility_hidden: bool,
    /// The multiplier of the computed `line-height` when it is unitless, which an unset one of a
    /// transition or motion starts from.
    pub line_height: Option<f64>,
    /// The farthest an underline of an element above, or of this one, is put from its baseline
    /// and thick, in px, which a text below inks past the box of its line by (culling).
    pub underline_reach: f32,
    /// No element has been built yet: the next one is the root, whose computed
    /// font size becomes the `rem`.
    pub(crate) above_root: bool,
    /// `fontVariantNumeric` and `fontFeatureSettings` as inherited so far.
    pub font_state: crate::css::font_features::FontFeatureState,
    /// Computed `font-variation-settings`, which an unset one of a transition or motion starts from.
    pub computed_variations: gpui::FontVariations,
    /// The combined features, only on the element whose style declares one of the
    /// two: gpui inherits a text style on its own, so descendants add nothing.
    pub font_features: Option<gpui::FontFeatures>,
    /// `fontVariationSettings`, only on the element whose style declares it, as with
    /// `font_features`.
    pub font_variations: Option<gpui::FontVariations>,
    /// The nearest ancestor's `interpolateSize`.
    pub interpolate_size: crate::motion::InterpolateSize,
    /// `border-spacing`, `border-collapse`, `caption-side` and `empty-cells`, which inherit.
    pub table: crate::css::table::TableInherited,
    /// `direction` and `text-align`, which resolves against it.
    pub direction: crate::css::direction::Inherited,
    /// The row and cell of a table the element is in, for copied text.
    pub table_row: Option<u64>,
    pub table_cell: Option<crate::text::selection::TableCell>,
    /// What the parent is as a containing block, which decides whether a
    /// percentage `height` of a child has anything to resolve against.
    pub container: crate::motion::containing::Container,
    /// The `group` declarations above this element, nearest first, with the
    /// state their `groupHover` / `groupActive` / `groupFocusWithin` followers read.
    pub groups: Option<Arc<GroupScope>>,
    /// The query containers above this element, nearest first, which its `@container`
    /// blocks are evaluated against and its container units are fractions of.
    pub containers: Option<Arc<crate::layout::container::ContainerScope>>,
    /// The container this element is, once it is built. Its children's `containers` begin
    /// with it (`enter_children`); the element's own style never sees it.
    pub(crate) self_container: Option<Arc<crate::layout::container::ContainerScope>>,
    /// The custom properties (`--name`) this element computes to, which `style()` queries read.
    pub custom: crate::css::custom_properties::CustomProperties,
    /// Counts the clones of this struct for the test renderer. Carries no data.
    clone_probe: crate::perf_counters::CloneProbe<{ crate::perf_counters::INHERITED }>,
}

/// One `group` declaration on the way down the tree.
pub(crate) struct GroupScope {
    pub(crate) name: gpui::SharedString,
    pub(crate) state: GroupState,
    pub(crate) parent: Option<Arc<GroupScope>>,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct GroupState {
    pub(crate) hovered: bool,
    pub(crate) active: bool,
    /// The group element or a descendant has focus, the source `focusWithin` reads.
    pub(crate) focused_within: bool,
}

impl Inherited {
    /// The font the element being built inherits.
    fn parent_font(&self) -> crate::motion::ParentFont {
        crate::motion::ParentFont {
            lengths: self.lengths,
            above_root: self.above_root,
            weight: self.font_weight,
            variations: self.computed_variations.clone(),
            inheritable: crate::motion::Inheritable {
                underline_offset: self.underline_offset,
                table: self.table,
                pointer_events_none: self.pointer_events_none,
                visibility_hidden: self.visibility_hidden,
                line_height: self.line_height,
            },
        }
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

    pub(crate) fn root(
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
            underline_offset: Default::default(),
            visibility_hidden: false,
            line_height: None,
            underline_reach: 0.0,
            above_root: true,
            font_state: Default::default(),
            font_features: None,
            font_variations: None,
            computed_variations: Default::default(),
            interpolate_size: Default::default(),
            table: Default::default(),
            direction: Default::default(),
            table_row: None,
            table_cell: None,
            container: crate::motion::containing::Container::WINDOW,
            groups: None,
            containers: None,
            self_container: None,
            clone_probe: Default::default(),
            custom: Default::default(),
        }
    }

    /// The containers the children of the element this is the context of are in: the element
    /// itself, if it is one, then the ones above it.
    fn container_chain(&self) -> Option<&crate::layout::container::ContainerScope> {
        self.self_container.as_deref().or(self.containers.as_deref())
    }

    /// The containers a query of the element being built is evaluated against. The element this is
    /// the context of is their parent, so `style()` with no name asks what it computes to.
    fn chain(&self) -> crate::layout::container::Chain<'_> {
        crate::layout::container::Chain {
            scopes: self.container_chain(),
            parent_custom: &self.custom,
            parent_lengths: self.lengths,
        }
    }

    /// What the children of the element this is the context of inherit as far as query
    /// containers go: the element itself, if it is one.
    fn enter_children(mut self) -> Self {
        if let Some(scope) = self.self_container.take() {
            self.lengths = scope.descendant_lengths(self.lengths);
            self.containers = Some(scope);
        }
        self
    }

    /// Apply the inheritable parts of `style` for the subtree below it.
    fn descend(mut self, style: Option<&StyleDesc>, inert: bool) -> Self {
        self = self.enter_children();
        self.lengths = self
            .lengths
            .descend(style.and_then(|style| style.font_size.as_ref()), self.above_root);
        self.above_root = false;
        self.font_features = None;
        self.font_variations = None;
        self.container = self.container.within(style.unwrap_or(&EMPTY_STYLE));
        self.direction = self.direction.descend(style.unwrap_or(&EMPTY_STYLE));
        self.inert |= inert;
        if self.inert {
            self.selectable = false;
            self.pointer_events_none = true;
        }
        let Some(style) = style else { return self };
        if let Some(declared) = &style.custom_properties {
            self.custom = self.custom.descend(declared);
        }
        self.underline_offset = self
            .underline_offset
            .inherit(style.text_underline_offset.as_ref(), &self.lengths);
        match style.visibility.as_deref() {
            Some("hidden") => self.visibility_hidden = true,
            Some("visible") => self.visibility_hidden = false,
            _ => {}
        }
        match &style.line_height {
            Some(crate::css::line_height::LineHeight::Multiplier(multiplier)) => self.line_height = Some(*multiplier),
            Some(_) => self.line_height = None,
            None => {}
        }
        self.underline_reach = self.underline_reach.max(crate::layout::cull::line_reach(
            style,
            self.underline_offset,
            &self.lengths,
        ));
        if let Some(weight) = &style.font_weight {
            self.font_weight = f64::from(parse_font_weight(weight).0);
        }
        (self.font_state, self.font_features) = self.font_state.descend(
            style.font_variant_numeric.as_deref(),
            style.font_feature_settings.as_deref(),
        );
        self.font_variations = style.font_variation_settings.as_ref().map(|settings| settings.variations());
        if let Some(variations) = &self.font_variations {
            self.computed_variations = variations.clone();
        }
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
        self.table = self.table.descend(style, &self.lengths);
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

    /// Scrolls the focused element to its nearest edge if it is out of view, as a browser does
    /// when focus moves to it.
    pub(crate) fn scroll_focus_into_view(
        &self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(id) = self.focused_element_id(window) else {
            return;
        };
        if self.scroll_element_into_view(window, id, crate::scroll::IntoView::NEAREST) {
            cx.notify();
            window.refresh();
        }
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
        window: gpui::WindowId,
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
        self.selection_drag_position = Some((window, position));
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
        let (Some((_, position)), Some(list_id)) =
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
        let (Some((window, position)), Some(list_id)) =
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
        let selection_moved = crate::text::paint::update_drag_at(window, &self.selection, position);
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

    /// Creates the focus handle of a focusable element that has none. Focus
    /// goes to it once, here, if it asks for `autoFocus`: re-focusing every
    /// frame would steal focus back from whatever the user clicked next.
    fn create_focus_handle(
        &mut self,
        id: u64,
        element: &crate::retained_tree::RetainedElement,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let (tab_index, tab_stop) = focus_handle_tab(element);
        let handle = cx.focus_handle().tab_index(tab_index).tab_stop(tab_stop);
        if element.auto_focus {
            self.focus.note_programmatic();
            handle.focus(window, cx);
        }
        self.focus_handles.insert(id, handle);
    }

    /// Focuses the elements that mounted with `autoFocus` in the batch that was
    /// just applied, as React DOM does in its commit. The next frame would find
    /// them too, but only after JS has run on with nothing focused.
    pub(crate) fn commit_auto_focus(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let tree_arc = self.tree.clone();
        let tree = tree_arc.lock().unwrap();
        for &id in &tree.focusable_ids {
            let Some(element) = tree.elements.get(&id) else {
                continue;
            };
            if element.auto_focus && !self.focus_handles.contains_key(&id) {
                self.create_focus_handle(id, element, window, cx);
            }
        }
        cx.notify();
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
            if let Some(handle) = self.focus_handles.get(&id) {
                let (tab_index, tab_stop) = focus_handle_tab(element);
                handle.clone().tab_index(tab_index).tab_stop(tab_stop);
                continue;
            }
            self.create_focus_handle(id, element, window, cx);
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
        self.settle_focus_after_frame(window, cx);

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

/// A text control without a `tabIndex` is a tab stop at 0.
fn focus_handle_tab(element: &crate::retained_tree::RetainedElement) -> (isize, bool) {
    element
        .tab_index()
        .map_or((0, true), crate::focus::gpui_tab_stop)
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
    /// The context a build of elements starts in, with `start` handed down to the first of them.
    fn build_ctx<'a>(
        &'a mut self,
        tree: &'a RetainedTree,
        callback: &'a Option<EventCallback>,
        activation: &'a crate::events::activation::Activation,
        pointer: &'a crate::events::state::PointerState,
        highlight_events: &'a mut Vec<(u64, usize)>,
        start: BuildStart,
    ) -> BuildCtx<'a> {
        BuildCtx {
            tree,
            event_callback: callback,
            activation,
            focus_handles: &self.focus_handles,
            scroll_handles: &mut self.scroll_handles,
            custom_registry: &mut self.custom_registry,
            virtual_lists: &mut self.virtual_lists,
            motion_states: &mut self.motion_states,
            motion_values: &self.motion_values,
            css_transitions: &mut self.css_transitions,
            css_animations: &mut self.css_animations,
            contents_styles: &mut self.contents_styles,
            keyframes: &self.keyframes,
            measurements: &self.measurements,
            layout_flips: &self.layout_flips,
            svg_trees: &mut self.svg_trees,
            now: start.now,
            selection: self.selection.clone(),
            doc_path: start.doc_path,
            doc_hosts: start.doc_hosts,
            inline_line: None,
            inherited: start.inherited,
            highlights: &mut self.highlights,
            paint_reports: &self.paint_reports,
            containers: &self.containers,
            highlight_events,
            focus_styles: start.focus_styles,
            control_styles: start.control_styles,
            pointer,
            contents_overlay: None,
            contents_key: 0,
            contents_dynamic: 0,
            anchor_scope: None,
            stacking_root: start.stacking_root,
            stacking_item: start.stacking_item,
            atomic_root: false,
            atomic_extracted: None,
            atomic_height_definite: false,
            containing_heights: Vec::new(),
            baseline_probe: None,
            collapsed: Default::default(),
            text_box_trim: start.text_box_trim,
            subtree: crate::layout::cull::Subtree::empty(),
            scroller: start.scroller,
            retain: &mut self.retain,
            tapes: &self.tapes,
        }
    }

    /// Builds the subtree of `id` for gpui, which asked for it while it laid out or prepainted the
    /// frame: it could not use what it recorded of the subtree, or never had. `env` is the build
    /// of the frame around the subtree as it was when the frame skipped it.
    pub(crate) fn build_subtree(
        &mut self,
        id: u64,
        env: FrameEnv,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        use gpui::IntoElement;

        let tree_arc = self.tree.clone();
        let tree = tree_arc.lock().unwrap();
        let callback = self.event_callback.clone();
        let activation = self.activation();
        let pointer_state = self.pointer.clone();
        let pointer = pointer_state.borrow();
        let mut highlight_events = Vec::new();
        self.retain.begin_deferred(env.relocatable);
        // The parent that spliced the element out of its wrappers is not there to say what they hand down.
        let splice = crate::layout::contents::splice_parent(&tree, id);
        let element = {
            let mut ctx = self.build_ctx(&tree, &callback, &activation, &pointer, &mut highlight_events, env.start);
            if let Some(parent) = splice {
                splice_overlay(&mut ctx, window, parent, id);
            }
            build_element_in(id, &mut ctx, window, cx, Some(env.enter))
        };
        self.retain.finish_deferred();
        drop(pointer);
        debug_assert!(highlight_events.is_empty(), "a highlight is never inside a retained subtree");
        emit_transition_events(&callback, &tree, self.css_transitions.take_events());
        emit_animation_events(&callback, &tree, self.css_animations.take_events());
        emit_web_animation_events(&callback, &mut self.retired_animations, self.css_transitions.take_api_events());
        emit_web_animation_events(&callback, &mut self.retired_animations, self.css_animations.take_api_events());
        emit_layout_events(&callback, &tree, self.layout_flips.take_events());
        if self.css_transitions.take_active() || self.css_animations.take_active() {
            window.request_animation_frame();
        }
        if self.css_transitions.take_dormant_active() | self.css_animations.take_dormant_active() {
            self.arm_dormant_tick(window, cx);
        }
        // Only a build that gpui can record has a tape that goes with the recording. A pass that
        // only lays out records nothing, and must leave the tape of the recording that stands.
        if !window.retention_records() {
            return element;
        }
        let tape = crate::tape::SharedTape::default();
        self.tapes.set(id, tape.clone());
        crate::tape::Taped::new(element, tape).into_any_element()
    }

    /// Builds the unit `id` of the last frame built whole (see `Retain::begin_unit`) as it is at
    /// `now`, to be painted again in place of what that frame made of it (`Window::repaint_in_place`).
    /// It is an element to prepaint and paint onto the nodes the unit has, whose tape holds what it
    /// would write into the registries without writing it: `Tapes::same_as_recorded` says whether
    /// that is what the frame wrote. None if `id` is not a unit.
    ///
    /// Nothing of the frames is touched: not the records, the dynamic elements, the elements not yet
    /// built, the tapes, and no frame is asked for (the animations that still run are noted in
    /// `take_active`, for the caller). What the build does to the clocks of the animations and the
    /// events it emits are those of a frame, and `built_at` is `now`.
    pub(crate) fn build_unit(
        &mut self,
        id: u64,
        now: web_time::Instant,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> Option<ScratchUnit> {
        use gpui::IntoElement;

        let mut env = self.retain.unit(id)?.env.clone();
        env.start.now = now;
        let effects = crate::tape::build_effects_len();
        let tree_arc = self.tree.clone();
        let tree = tree_arc.lock().unwrap();
        let callback = self.event_callback.clone();
        let activation = self.activation();
        let pointer_state = self.pointer.clone();
        let pointer = pointer_state.borrow();
        let mut highlight_events = Vec::new();
        let recording = self.retain.suspend();
        let splice = crate::layout::contents::splice_parent(&tree, id);
        let element = {
            let mut ctx = self.build_ctx(&tree, &callback, &activation, &pointer, &mut highlight_events, env.start);
            if let Some(parent) = splice {
                splice_overlay(&mut ctx, window, parent, id);
            }
            build_element_in(id, &mut ctx, window, cx, None)
        };
        self.retain.resume(recording);
        crate::tape::truncate_build_effects(effects);
        drop(pointer);
        debug_assert!(highlight_events.is_empty(), "a highlight is never inside a unit");
        self.built_at = Some(now);
        emit_transition_events(&callback, &tree, self.css_transitions.take_events());
        emit_animation_events(&callback, &tree, self.css_animations.take_events());
        emit_web_animation_events(&callback, &mut self.retired_animations, self.css_transitions.take_api_events());
        emit_web_animation_events(&callback, &mut self.retired_animations, self.css_animations.take_api_events());
        emit_layout_events(&callback, &tree, self.layout_flips.take_events());
        let tape = crate::tape::Tape::capturing();
        Some(ScratchUnit {
            id: retained_id(id),
            element: crate::tape::Taped::new(element, tape.clone()).into_any_element(),
            tape,
        })
    }

    /// Whether the tick at `now` can be drawn by building the units that hold what changed, and
    /// which they are, see `Retain::preview`. Changes nothing: what the frame takes when it reads
    /// the measurements, links and commands of the animations stays for it.
    ///
    /// The motions run as `sync_motion_states` last left them: a patcher steps them before it
    /// builds a unit, which nothing here depends on, since a motion starts or retargets only
    /// with a change of the tree or the pointer.
    pub(crate) fn preview_tick(
        &self,
        window: &gpui::Window,
        now: web_time::Instant,
    ) -> std::result::Result<Vec<u64>, crate::retain::Unpatchable> {
        let tree_arc = self.tree.clone();
        let tree = tree_arc.lock().unwrap();
        let focus_styles = self.focus.styles(&tree, tree.keyboard_focus_dim());
        let pointer_state = self.pointer.clone();
        let pointer = pointer_state.borrow();
        let dormancy = Dormancy {
            tree: &tree,
            window: window.window_handle().window_id(),
            animations: &self.css_animations,
            transitions: &self.css_transitions,
            motion_states: &self.motion_states,
            keyframes: &self.keyframes,
        };
        let commanded: Vec<u64> = self
            .css_transitions
            .peek_commanded()
            .iter()
            .chain(self.css_animations.peek_commanded())
            .copied()
            .collect();
        let sources = crate::retain::Sources {
            tree: &tree,
            hovered: pointer.chains().0,
            active: pointer.chains().1,
            focus: &focus_styles,
            focusable: &self.focus_handles,
            keyboard: window.last_input_was_keyboard(),
            scrolls: &self.scroll_handles,
            measured: self.measurements.peek_changed(),
            links: crate::accessibility::links::peek_changed(),
            commanded,
            customs: self.custom_registry.retain_states(),
            motion: running_motion(&self.motion_states, &self.css_animations, &self.keyframes, now),
            dormant: &|id| dormancy.holds(id),
            selection: self.selection.lock().identity(),
            pass: window.draw_pass(),
            lazy_builds: window.retained_counters().lazy_builds,
            relocations: window.retained_counters().relocations,
            reference: false,
            layout_only: false,
        };
        self.retain
            .preview(&sources, crate::scroll::animating() || crate::scrollbar::animating())
    }

    pub(crate) fn activation(&self) -> crate::events::activation::Activation {
        crate::events::activation::Activation {
            tree: self.tree.clone(),
            callback: self.event_callback.clone(),
            routing: self.routing.clone(),
        }
    }
}

/// The elements with a motion that runs at `now`, see `retain::Sources::motion`.
fn running_motion(
    motion_states: &HashMap<u64, crate::motion::MotionState>,
    animations: &crate::motion::CssAnimations,
    keyframes: &crate::motion::KeyframesRegistry,
    now: web_time::Instant,
) -> Vec<u64> {
    motion_states
        .iter()
        .filter(|(_, state)| state.is_animating(now))
        .map(|(id, _)| *id)
        .chain(animations.svg_shapes(keyframes))
        .collect()
}

/// A unit built to be painted again, see `GpuixView::build_unit`.
pub(crate) struct ScratchUnit {
    /// The id the retained root of the unit has in the frame.
    pub id: gpui::ElementId,
    pub element: gpui::AnyElement,
    /// What the element writes into the registries when it is prepainted and painted, see
    /// `Tapes::same_as_recorded`.
    pub tape: crate::tape::SharedTape,
}

/// Whether an element can be left alone while nobody sees it, see `retain::Sources::dormant`.
struct Dormancy<'a> {
    tree: &'a crate::retained_tree::RetainedTree,
    window: gpui::WindowId,
    animations: &'a crate::motion::CssAnimations,
    transitions: &'a crate::motion::CssTransitions,
    motion_states: &'a HashMap<u64, crate::motion::MotionState>,
    keyframes: &'a crate::motion::KeyframesRegistry,
}

impl Dormancy<'_> {
    fn holds(&self, id: u64) -> bool {
        crate::automation::was_culled(self.window, id)
            && self.tree.elements.get(&id).is_some_and(|element| {
                runs_paint_only(
                    self.tree,
                    element,
                    self.animations,
                    self.transitions,
                    self.motion_states,
                    self.keyframes,
                )
            })
    }
}

/// Whether everything that runs on the element only changes what it paints, so that a frame
/// that builds it again in place of the one before changes nothing it lays out.
fn runs_paint_only(
    tree: &crate::retained_tree::RetainedTree,
    element: &crate::retained_tree::RetainedElement,
    animations: &crate::motion::CssAnimations,
    transitions: &crate::motion::CssTransitions,
    motion_states: &HashMap<u64, crate::motion::MotionState>,
    keyframes: &crate::motion::KeyframesRegistry,
) -> bool {
    let id = element.id;
    // A span is painted by its paragraph, a shape of an `<svg>` by the raster of the `<svg>`, and a
    // `layout` element glides between boxes: none is an element whose frame is only paint.
    if element.element_type == "span"
        || crate::custom_elements::svg_tree::is_svg_element(tree, element)
        || element.custom_props().contains_key("layout")
    {
        return false;
    }
    // What runs on the shapes of an `<svg>` changes its picture, never its box. An `<svg>` blocks
    // culling around it (`Subtree::block`), so it is a unit but never dormant.
    let shapes_paint_only = || {
        crate::custom_elements::svg_tree::shapes(tree, element).all(|shape| {
            animations.paints_only(shape.id, keyframes)
                && motion_states.get(&shape.id).is_none_or(crate::motion::MotionState::paints_only)
        })
    };
    animations.paints_only(id, keyframes)
        && transitions.paints_only(id)
        && motion_states.get(&id).is_none_or(crate::motion::MotionState::paints_only)
        && element
            .custom_props()
            .get("motionValues")
            .is_none_or(crate::motion::MotionValueStore::binding_paints_only)
        && shapes_paint_only()
}

/// What runs on the shapes of the `<svg>` `element` at `now`, as `retain::Dynamic` bits. A shape is
/// never built, so its `<svg>` is the element a running shape is a reason to build again.
fn shapes_running(
    tree: &crate::retained_tree::RetainedTree,
    element: &crate::retained_tree::RetainedElement,
    animations: &crate::motion::CssAnimations,
    motion_states: &HashMap<u64, crate::motion::MotionState>,
    keyframes: &crate::motion::KeyframesRegistry,
    now: web_time::Instant,
) -> u8 {
    use crate::retain::Dynamic;

    let mut running = 0;
    for shape in crate::custom_elements::svg_tree::shapes(tree, element) {
        if animations.shape_moves(shape.id, keyframes) {
            running |= Dynamic::Animation as u8;
        }
        if motion_states.get(&shape.id).is_some_and(|state| state.is_animating(now)) {
            running |= Dynamic::Motion as u8;
        }
    }
    running
}

impl GpuixView {
    /// Asks for a frame callback that carries the dormant animations on without drawing: Chrome
    /// runs the animations of content it does not show without painting a frame for them.
    pub(crate) fn arm_dormant_tick(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        if std::mem::replace(&mut self.dormant_tick_armed, true) {
            return;
        }
        let view = cx.weak_entity();
        window.on_next_frame(move |window, cx| {
            view.update(cx, |view, cx| view.dormant_tick(window, cx)).ok();
        });
    }

    /// A frame in which only dormant animations run: no build, no layout, no draw. What a frame
    /// would have done for them with the clock is done here: runs and transitions move to `now`,
    /// motion is stepped, `animationiteration` and its kin are emitted, and `getAnimations` reads
    /// the instant of the tick.
    fn dormant_tick(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        self.dormant_tick_armed = false;
        let tree_arc = self.tree.clone();
        let tree = tree_arc.lock().unwrap();
        let callback = self.event_callback.clone();
        let now = self.clock.now();
        self.built_at = Some(now);
        for &id in self.retain.dormant() {
            self.css_animations.advance_clock(id, now);
            self.css_transitions.advance_clock(id, now);
        }
        let (animating, settled) = if tree.motion_ids.is_empty() {
            (Vec::new(), Vec::new())
        } else {
            let focus_styles = self.focus.styles(&tree, tree.keyboard_focus_dim());
            let pointer_state = self.pointer.clone();
            let pointer = pointer_state.borrow();
            sync_motion_states(
                &tree,
                &mut self.motion_states,
                &self.measurements,
                window.appearance(),
                window.viewport_size(),
                &|id| gestures_of(&tree, &pointer, &focus_styles, id),
                now,
            )
        };
        emit_motion_settled(&callback, &tree, &settled);
        emit_transition_events(&callback, &tree, self.css_transitions.take_events());
        emit_animation_events(&callback, &tree, self.css_animations.take_events());
        emit_web_animation_events(&callback, &mut self.retired_animations, self.css_transitions.take_api_events());
        emit_web_animation_events(&callback, &mut self.retired_animations, self.css_animations.take_api_events());
        let dormant = self.retain.dormant();
        let dormant_active = self.css_transitions.take_dormant_active() | self.css_animations.take_dormant_active();
        if animating.iter().any(|id| !dormant.contains(id)) {
            // A motion that no frame has seen run: a frame of its own sets it going.
            cx.notify();
        } else if dormant_active || !animating.is_empty() {
            self.arm_dormant_tick(window, cx);
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
        self.install_patcher(window, cx);
        let verifies = self.retain.verifies_frames();
        self.tapes.verify(verifies);
        // A build is not the same twice in a frame here (anchor-size() reads the size the trigger
        // painted a pass ago), so outside the subtrees that replayed the two passes differ, and
        // every wrapper makes a dispatch node.
        window.set_retention_whole_frame(false);
        window.set_retention_verifier(verifies.then(|| {
            let tapes = self.tapes.clone();
            Rc::new(move |window: &gpui::Window| {
                // A pass that only lays out records no tape. What it publishes is the registry.
                if window.is_layout_pass() {
                    return crate::automation::describe_bounds(window.window_handle().window_id());
                }
                tapes.describe_replays(window.is_retention_reference())
            }) as Rc<dyn Fn(&gpui::Window) -> String>
        }));
        if !window.is_retention_reference() {
            self.tapes.forget_replays();
        }

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
        self.containers.retain(|id| tree.elements.contains_key(&id));
        self.tapes.retain(|id| tree.elements.contains_key(&id));
        // Pruned by listener, not existence: dropping `onLayout` and adding it
        // back reports the size again, like a fresh observer.
        {
            let mut reports = self.paint_reports.borrow_mut();
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
        // Everything time-dependent in a pass (motions, springs, transitions, animations, scroll
        // animations, scrollbar fades) reads this one instant. The reference pass of a verified
        // frame takes the instant of the pass it is compared with: on a clock that runs on it
        // would be a pass ahead, and a replayed subtree placed by something that animates would
        // differ from it by the distance that moved. Those are all sampled at `now` and not
        // stepped, so building again at the same instant gives the same state.
        let now = if window.is_retention_reference() {
            self.built_at
                .expect("a reference pass is built right after the pass it is compared with")
        } else {
            let now = self.clock.now();
            self.built_at = Some(now);
            now
        };
        crate::scroll::begin_frame(|id| tree.elements.contains_key(&id));
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
        let focus_styles = self.focus.styles(&tree, tree.keyboard_focus_dim());
        let pointer_state = self.pointer.clone();
        let pointer = pointer_state.borrow();
        let (motion_animating, motion_settled) =
            sync_motion_states(
                &tree,
                &mut self.motion_states,
                &self.measurements,
                window.appearance(),
                window.viewport_size(),
                &|id| gestures_of(&tree, &pointer, &focus_styles, id),
                now,
            );
        self.motion_values
            .set_appearance(color_scheme_of(window.appearance()));
        let motion_value_frame = sync_motion_values(&tree, &mut self.motion_values, now);
        self.css_transitions
            .retain(|id| tree.elements.contains_key(&id));
        self.css_animations
            .retain(|id| tree.elements.contains_key(&id));
        self.contents_styles.clear();
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
        crate::accessibility::links::sync(&tree);
        let activation = self.activation();
        crate::layout::box_generation::hold_display(self.css_transitions.held_displays());
        let commanded: Vec<u64> = self
            .css_transitions
            .take_commanded()
            .into_iter()
            .chain(self.css_animations.take_commanded())
            .collect();
        let dormancy = Dormancy {
            tree: &tree,
            window: window.window_handle().window_id(),
            animations: &self.css_animations,
            transitions: &self.css_transitions,
            motion_states: &self.motion_states,
            keyframes: &self.keyframes,
        };
        self.retain.begin_frame(crate::retain::Sources {
            tree: &tree,
            hovered: pointer.chains().0,
            active: pointer.chains().1,
            focus: &focus_styles,
            focusable: &self.focus_handles,
            keyboard: window.last_input_was_keyboard(),
            scrolls: &self.scroll_handles,
            measured: self.measurements.take_changed(),
            links: crate::accessibility::links::take_changed(),
            commanded,
            customs: self.custom_registry.retain_states(),
            motion: running_motion(&self.motion_states, &self.css_animations, &self.keyframes, now),
            dormant: &|id| dormancy.holds(id),
            selection: self.selection.lock().identity(),
            pass: window.draw_pass(),
            lazy_builds: window.retained_counters().lazy_builds,
            relocations: window.retained_counters().relocations,
            reference: window.is_retention_reference(),
            layout_only: window.is_layout_pass(),
        });
        // The elements left dormant keep their animations on time without a build: their clocks
        // move here, and what they ask of the window goes to a tick that draws nothing.
        self.css_animations.set_dormant(self.retain.dormant().iter().copied());
        self.css_transitions.set_dormant(self.retain.dormant().iter().copied());
        if !window.is_retention_reference() {
            for &id in self.retain.dormant() {
                self.css_animations.advance_clock(id, now);
                self.css_transitions.advance_clock(id, now);
            }
        }
        let result = match tree.root_id {
            Some(root_id) => {
                let mut ctx = self.build_ctx(
                    &tree,
                    &callback,
                    &activation,
                    &pointer,
                    &mut highlight_events,
                    BuildStart {
                        inherited: Inherited::root(
                            &theme,
                            window.appearance(),
                            self.frameless,
                            window.viewport_size(),
                        ),
                        doc_path: Vec::new(),
                        doc_hosts: Arc::from([]),
                        now,
                        stacking_root: true,
                        stacking_item: false,
                        text_box_trim: Default::default(),
                        focus_styles,
                        control_styles: Default::default(),
                        scroller: None,
                    },
                );
                build_element(root_id, &mut ctx, window, cx)
            }
            None => gpui::Empty.into_any_element(),
        };
        self.retain.finish_frame();
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
        emit_web_animation_events(&callback, &mut self.retired_animations, self.css_transitions.take_api_events());
        emit_web_animation_events(&callback, &mut self.retired_animations, self.css_animations.take_api_events());
        emit_layout_events(&callback, &tree, self.layout_flips.take_events());
        let transitions_active =
            self.css_transitions.take_active() | self.css_animations.take_active();
        let dormant_active = self.css_transitions.take_dormant_active() | self.css_animations.take_dormant_active();
        let motion_active = motion_animating.iter().any(|id| !self.retain.dormant().contains(id));
        let dormant_motion = motion_animating.iter().any(|id| self.retain.dormant().contains(id));
        self.emit_selection_change(&callback);

        // The frame reset must paint BEFORE any text, so it is the first child of
        // the root wrapper. Without it the selection registry accumulates stale
        // entries across frames and a drag resolves against elements that are no
        // longer on screen.
        let observing = !tree.geometry_ids.is_empty() || self.paint_reports.borrow().observers.is_observing();
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
                        resync_by_frame: self.resync_by_frame,
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
                    move |window, position, app| {
                        // This listener can run inside an update of GpuixView (an
                        // accessibility activation does). A nested update panics, so wait.
                        let drag_move_view = drag_move_view.clone();
                        app.defer(move |app| {
                            drag_move_view
                                .update(app, |view, cx| view.on_selection_mouse_move(window, position, cx))
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
                    tree_arc.clone(),
                    callback.clone(),
                    observing,
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
                if !crate::scroll::has_scrolling_box(&tree, id) {
                    continue;
                }
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
            self.request_next_frame(window, scroll_animating);
        } else if dormant_active || dormant_motion {
            self.arm_dormant_tick(window, cx);
        }

        result
    }
}

// ── Element builders ─────────────────────────────────────────────────

static EMPTY_STYLE: std::sync::LazyLock<StyleDesc> = std::sync::LazyLock::new(StyleDesc::default);

/// What an element that has no box in the layout is built as. It has the id of the element, like
/// every host does, and not the id-less `Empty`: a retained root names its layout node by its
/// path, and an element without an id of its own is named by its parent's, which is the node of
/// whatever else under that parent has none. It is `display: none` all the same: a block would be
/// an item of a flex or grid parent, with the gap around it.
fn absent_host(id: u64) -> gpui::AnyElement {
    use gpui::{InteractiveElement, IntoElement, Styled};
    gpui::div().id(crate::events::ids::host_element_id(id)).hidden().into_any_element()
}

pub(crate) fn build_element(
    id: u64,
    ctx: &mut BuildCtx,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<GpuixView>,
) -> gpui::AnyElement {
    build_element_in(id, ctx, window, cx, None)
}

/// `build_element`, for the subtree gpui asked for after the frame was built if `prekeyed`: the
/// key the frame that deferred it entered it with.
fn build_element_in(
    id: u64,
    ctx: &mut BuildCtx,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<GpuixView>,
    prekeyed: Option<crate::retain::EnterKey>,
) -> gpui::AnyElement {
    let mut unit = None;
    if let Some(known) = ctx.tree.elements.get(&id) {
        let plain = ctx.baseline_probe.is_none()
            && ctx.inline_line.is_none()
            && ctx.doc_hosts.is_empty()
            && ctx.containing_heights.is_empty()
            && ctx.atomic_extracted.is_none()
            && !crate::text::atomic::is_collecting();
        let boxed = !matches!(
            crate::layout::table::part(known),
            Some(
                crate::layout::table::Part::RowGroup(_)
                    | crate::layout::table::Part::Row
                    | crate::layout::table::Part::Column
                    | crate::layout::table::Part::ColumnGroup
            )
        );
        let skip = ctx.retain.enter(crate::retain::Entry {
            id,
            revision: known.subtree_revision,
            inherited: &ctx.inherited,
            at: ctx.doc_path.last().copied().unwrap_or(0),
            stacking_root: ctx.stacking_root,
            stacking_item: ctx.stacking_item,
            atomic_root: ctx.atomic_root,
            contents_overlay: ctx.contents_overlay.is_some().then_some(ctx.contents_key),
            anchor_scope: ctx.anchor_scope.is_some(),
            text_box_trim: ctx.text_box_trim,
            plain,
            container: known
                .style
                .as_deref()
                .filter(|style| style.container_rules.is_some() || crate::layout::container::declared(style).is_some())
                .map(|_| ctx.containers.key(id)),
            boxed,
            prekeyed,
            scroller: ctx.scroller,
            costly: known.content.is_some()
                || !crate::text::inline::is_host_type(&known.element_type)
                || known.element_type == "text",
        });
        if let Some(skip) = skip {
            ctx.contents_overlay = None;
            ctx.contents_dynamic = 0;
            ctx.anchor_scope = None;
            return retained_root(id, skip, ctx, window, cx);
        }
        unit = ctx.retain.begin_unit(plain && boxed && prekeyed.is_none()).map(|enter| FrameEnv {
            start: BuildStart::of(ctx),
            enter,
            relocatable: false,
        });
    }
    // After the retained decision: what the `display: contents` ancestors pass down is covered by
    // `contents_key`, and a deferred build of the subtree enters it again.
    let outside = enter_contents(ctx);
    let element = build_element_unpumped(id, ctx, window, cx);
    if let Some(outside) = outside {
        ctx.inherited = outside;
    }
    let unit = unit.and_then(|env| ctx.retain.end_unit().map(|subtree| (env, subtree)));
    if ctx.tree.elements.contains_key(&id) {
        ctx.retain.exit();
    }
    // Every style resolves while its element is built, and a virtual-list row is
    // built in list layout, so this is the one place that sees both.
    crate::background_images::pump(window, cx);
    match unit {
        Some((env, subtree)) => unit_root(id, env, subtree, element, ctx),
        None => element,
    }
}

/// What the build of an element starts from, as the build around it hands it down.
#[derive(Clone)]
pub(crate) struct BuildStart {
    inherited: Inherited,
    doc_path: Vec<u32>,
    doc_hosts: Arc<[crate::text::selection::Host]>,
    now: web_time::Instant,
    stacking_root: bool,
    stacking_item: bool,
    text_box_trim: gpui::TextBoxTrim,
    focus_styles: crate::focus::FocusStyles,
    control_styles: crate::retained_tree::ControlStyles,
    scroller: Option<crate::retain::Scroller>,
}

impl BuildStart {
    /// What `ctx` hands the element it is about to build, left as it is: the element takes the
    /// flags itself.
    fn of(ctx: &BuildCtx) -> Self {
        Self {
            inherited: ctx.inherited.clone(),
            doc_path: ctx.doc_path.clone(),
            doc_hosts: ctx.doc_hosts.clone(),
            now: ctx.now,
            stacking_root: ctx.stacking_root,
            stacking_item: ctx.stacking_item,
            text_box_trim: ctx.text_box_trim,
            focus_styles: ctx.focus_styles.clone(),
            control_styles: ctx.control_styles.clone(),
            scroller: ctx.scroller,
        }
    }
}

/// What a build of a subtree after the frame needs of the build around the element, which is
/// over by then.
#[derive(Clone)]
pub(crate) struct FrameEnv {
    start: BuildStart,
    enter: crate::retain::EnterKey,
    /// Whether the subtree is recorded to be put elsewhere by its scroller.
    relocatable: bool,
}

#[cfg(test)]
impl FrameEnv {
    pub(crate) fn for_test(inherited: Inherited, enter: crate::retain::EnterKey, now: web_time::Instant) -> Self {
        Self {
            start: BuildStart {
                inherited,
                doc_path: Vec::new(),
                doc_hosts: Arc::from([]),
                now,
                stacking_root: false,
                stacking_item: false,
                text_box_trim: Default::default(),
                focus_styles: Default::default(),
                control_styles: Default::default(),
                scroller: None,
            },
            enter,
            relocatable: false,
        }
    }
}

/// The element standing for a subtree the frame found unchanged. gpui builds the subtree from
/// `GpuixView::build_subtree` if it cannot use what it recorded of it, and until then it is as if
/// it had been built: its effects outside the frame are repeated, and the registries it writes
/// are filled from its tape when gpui replays it.
fn retained_root(
    id: u64,
    skip: crate::retain::Skip,
    ctx: &mut BuildCtx,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<GpuixView>,
) -> gpui::AnyElement {
    use gpui::IntoElement;

    for effect in skip.effects.iter().flat_map(|effects| effects.iter()) {
        match effect {
            crate::tape::BuildEffect::Measured(id) => ctx.measurements.keep(*id),
            crate::tape::BuildEffect::Image(key, url) => crate::background_images::request(*key, url),
            crate::tape::BuildEffect::ContainerRead(container, axes) => {
                if let Some(scope) = ctx.inherited.container_chain() {
                    scope.repeat_read(*container, *axes);
                }
            }
        }
    }
    crate::background_images::pump(window, cx);
    ctx.subtree.merge(skip.subtree);
    let start = BuildStart::of(ctx);
    ctx.stacking_root = false;
    ctx.stacking_item = false;
    ctx.text_box_trim = Default::default();
    let env = FrameEnv { start, enter: skip.enter, relocatable: skip.overscan.is_some() };
    let view = cx.weak_entity();
    let mut retained = gpui::retained(
        retained_id(id),
        skip.token,
        move |window, cx| {
            view.update(cx, |view, cx| view.build_subtree(id, env, window, cx))
                .expect("the view of a window lives as long as the window draws")
        },
    )
    .on_replay_translated(replay_hook(id, ctx.tapes.clone(), ctx.measurements.clone()));
    if let Some(overscan) = skip.overscan {
        retained = retained.relocatable(overscan);
    }
    retained = retained.cull_outset(skip.subtree.outset());
    if let Some(z) = skip.lift_z {
        retained = retained.lift_z(z);
    }
    retained.into_any_element()
}

pub(crate) fn retained_id(id: u64) -> gpui::ElementId {
    gpui::ElementId::NamedInteger(gpui::SharedString::new_static("gpuix-retained"), id)
}

/// What a replay of the retained root `id` does outside the frame: the registries it wrote are
/// written again from its tape.
fn replay_hook(
    id: u64,
    tapes: crate::tape::Tapes,
    measurements: crate::motion::measure::Measurements,
) -> impl Fn(gpui::RetainedPhase, gpui::Point<gpui::Pixels>, &mut gpui::Window, &mut gpui::App) + 'static {
    move |phase, delta, window, _| {
        let phase = match phase {
            gpui::RetainedPhase::Prepaint => {
                measurements.republish_parent_content(id, window);
                crate::tape::Phase::Prepaint
            }
            gpui::RetainedPhase::Paint => {
                measurements.republish_parent_content(id, window);
                crate::tape::Phase::Paint
            }
            gpui::RetainedPhase::Layout => crate::tape::Phase::Layout,
            gpui::RetainedPhase::LayoutPaint => crate::tape::Phase::LayoutPaint,
            gpui::RetainedPhase::LiftedBegin(rel) => return tapes.begin_lifted(id, rel),
            gpui::RetainedPhase::LiftedEnd => return tapes.end_lifted(id),
            gpui::RetainedPhase::LiftedPaint(rel) => crate::tape::Phase::Lifted(rel),
        };
        tapes.replay(window.window_handle().window_id(), id, phase, delta);
    }
}

/// `element`, just built whole as the unit `id` (see `Retain::begin_unit`), as a patchable retained
/// root that is recorded in every frame and replayed in none, for gpui to leave what a tick needs
/// to repaint it alone: its key is not the one of the frame before. The root hands over the
/// element built, and keeps `env` for building it again.
fn unit_root(
    id: u64,
    env: FrameEnv,
    subtree: crate::layout::cull::Subtree,
    element: gpui::AnyElement,
    ctx: &mut BuildCtx,
) -> gpui::AnyElement {
    use gpui::IntoElement;

    let tapes = ctx.tapes.clone();
    let hook = replay_hook(id, tapes.clone(), ctx.measurements.clone());
    let retained = gpui::retained(retained_id(id), ctx.retain.unit_key(), move |window, _| {
        if !window.retention_records() {
            return element;
        }
        let tape = crate::tape::SharedTape::default();
        tapes.set(id, tape.clone());
        crate::tape::Taped::new(element, tape).into_any_element()
    })
    .on_replay_translated(hook)
    .cull_outset(subtree.outset())
    .patchable();
    ctx.retain.add_unit(crate::retain::Unit { id, env });
    retained.into_any_element()
}

/// Takes the query containers and custom properties of the `display: contents` ancestors a
/// spliced child comes from into `ctx.inherited`, before its own `@container` blocks are
/// evaluated, and returns what to put back afterwards. The rest of what such an ancestor passes
/// down is the overlay the child's style is filled from.
fn enter_contents(ctx: &mut BuildCtx) -> Option<Inherited> {
    let levels = ctx.contents_overlay.as_ref()?.contents_levels.clone()?;
    let outside = ctx.inherited.clone();
    for level in levels.iter() {
        if let Some(custom) = &level.custom {
            ctx.inherited.custom = ctx.inherited.custom.descend(custom);
        }
        if let Some((id, declaration)) = &level.declaration {
            ctx.inherited = ctx.inherited.clone().enter_children();
            ctx.inherited.self_container = Some(Arc::new(crate::layout::container::ContainerScope::without_size(
                *id,
                declaration.clone(),
                ctx.inherited.lengths,
                ctx.inherited.custom.clone(),
                ctx.inherited.containers.clone(),
            )));
        }
    }
    Some(outside)
}

/// What `animate_style` starts from.
struct StyleInputs<'a> {
    /// The style with the `@container` blocks folded in: what the stages ask whether they apply to.
    declared: Option<&'a StyleDesc>,
    /// `declared` with the state styles (`hover`, `focus`, ...) folded in.
    base: Option<&'a StyleDesc>,
    before_transition: Option<&'a StyleDesc>,
    pointer_states: crate::events::state_styles::PointerStates,
    /// Until every container above has been laid out once, the style is a guess that the next
    /// pass replaces. It must not seed a transition or an animation: the correct style would
    /// then animate in from the guess.
    provisional: bool,
    /// Whether a `display: none` that a transition produces ends the build: only an element with
    /// a box has one to end.
    may_hide: bool,
}

/// An element's style in this frame, after the stages that move with time. Owned, so that the
/// build of an element and the overlay of a `display: contents` element that is never built read
/// the one result.
struct Animated {
    /// A `display` transition has hidden the element: no stage after it ran.
    hidden: bool,
    transitioned: Option<crate::motion::DeclaredStyle>,
    moved: Option<StyleDesc>,
    bound: Option<StyleDesc>,
    animated: Option<StyleDesc>,
    /// What the animations of an `<svg>` write on its root, for its shapes to inherit.
    svg_css: Vec<(&'static str, String)>,
    /// Whether the element is measured (and wrapped to be).
    measured: bool,
    measurement: Option<crate::motion::measure::Measurement>,
    focus_visible_own: Option<bool>,
    /// What runs on the element now, as `retain::Dynamic` bits.
    dynamic: u8,
}

impl Animated {
    /// The style before `motion`, motion values and keyframes.
    fn declared<'s>(&'s self, base: Option<&'s StyleDesc>) -> Option<&'s StyleDesc> {
        self.transitioned.as_ref().map(|declared| &declared.style).or(base)
    }

    fn current<'s>(&'s self, base: Option<&'s StyleDesc>) -> Option<&'s StyleDesc> {
        self.animated
            .as_ref()
            .or(self.bound.as_ref())
            .or(self.moved.as_ref())
            .or_else(|| self.declared(base))
    }

    /// Whether `current` is another style than `base`.
    fn restyled(&self) -> bool {
        self.transitioned.is_some() || self.moved.is_some() || self.bound.is_some() || self.animated.is_some()
    }
}

/// Steps what moves an element's style to `ctx.now`, in the order the cascade has it: CSS
/// transitions on the state-folded style, then `motion` (an `animate` value is an inline style and
/// beats a transition), then motion values, then CSS animations, which beat every normal
/// declaration except on a property a transition is running on, as a transition beats an
/// animation.
///
/// It advances the state of the element (`CssTransitions`, `CssAnimations`), so it runs once per
/// frame for an element, and says what it found to the caller instead of noting it in `ctx.retain`:
/// the element being built is not always the one that was stepped.
fn animate_style(
    ctx: &mut BuildCtx,
    window: &gpui::Window,
    element: &crate::retained_tree::RetainedElement,
    input: StyleInputs,
) -> Animated {
    use crate::retain::Dynamic;

    let id = element.id;
    let props = element.custom_props();
    let has_motion = props.contains_key("motion");
    let motion_binding = props.get("motionValues");
    let transitions_wanted = !input.provisional && ctx.css_transitions.wants(id, input.declared);
    let animations_wanted = !input.provisional && ctx.css_animations.wants(id, input.declared);
    // A plain element has no measurement to read: it is not wrapped below, and
    // the root render prunes the measurement of any element that was not.
    let measured = has_motion || transitions_wanted || animations_wanted;
    let measurement = measured.then(|| ctx.measurements.get(id)).flatten();
    let mut dynamic = 0;
    if has_motion && ctx.motion_states.get(&id).is_some_and(|state| state.is_animating(ctx.now)) {
        dynamic |= Dynamic::Motion as u8;
    }
    if motion_binding.is_some() {
        dynamic |= Dynamic::MotionValues as u8;
    }
    if animations_wanted {
        dynamic |= Dynamic::Animation as u8;
    }
    let mut focus_visible_own = None;
    let transitioned = (transitions_wanted || animations_wanted).then(|| {
        let focus_visible = crate::motion::FocusVisible {
            own: ctx
                .focus_handles
                .get(&id)
                .is_some_and(|handle| handle.is_focused(window))
                && window.last_input_was_keyboard(),
            descendant: ctx.focus_styles.has_focus_visible(id),
        };
        focus_visible_own = Some(focus_visible.own);
        crate::motion::resolve_declared_style(
            transitions_wanted.then_some(&mut *ctx.css_transitions),
            animations_wanted.then_some((&mut *ctx.css_animations, ctx.keyframes)),
            input.before_transition,
            &ctx.inherited.colors,
            &ctx.inherited.parent_font(),
            input.pointer_states,
            ctx.motion_states.get(&id),
            id,
            input.declared.unwrap_or(&EMPTY_STYLE),
            focus_visible,
            measurement.as_ref(),
            ctx.inherited.interpolate_size,
            crate::motion::Scope::Element,
            ctx.now,
        )
    });
    // A `display` transition with `allow-discrete` kept the box until now.
    if input.may_hide
        && crate::layout::box_generation::declares_display_none(element)
        && transitioned
            .as_ref()
            .is_none_or(|declared| declared.style.display.as_deref() == Some("none"))
    {
        return Animated {
            hidden: true,
            transitioned: None,
            moved: None,
            bound: None,
            animated: None,
            svg_css: Vec::new(),
            measured,
            measurement,
            focus_visible_own,
            dynamic,
        };
    }
    if transitioned.as_ref().is_some_and(|declared| declared.transitions.is_some()) {
        dynamic |= Dynamic::Transition as u8;
    }
    let declared_style = transitioned.as_ref().map(|declared| &declared.style).or(input.base);
    let moved = if has_motion {
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
    let bound = motion_binding.map(|binding| {
        let mut resolved = moved.as_ref().or(declared_style).cloned().unwrap_or_default();
        ctx.motion_values.apply(binding, &mut resolved, &ctx.inherited.parent_font());
        resolved
    });
    let underlying_style = bound.as_ref().or(moved.as_ref()).or(declared_style);
    let css_values = animations_wanted
        .then(|| {
            let beneath = underlying_style.unwrap_or(&EMPTY_STYLE);
            let subject = if element.element_type == "svg" {
                crate::motion::Subject::svg_host(beneath, element.custom_props())
            } else {
                crate::motion::Subject::element(beneath)
            };
            ctx.css_animations.update(
                id,
                declared_style.unwrap_or(&EMPTY_STYLE),
                &subject,
                &ctx.inherited.colors,
                &ctx.inherited.parent_font(),
                measurement.as_ref(),
                ctx.inherited.interpolate_size,
                crate::motion::Scope::Element,
                ctx.now,
            )
        })
        .flatten();
    // What the animations of an `<svg>` write on its root, for its shapes to inherit.
    let svg_css = css_values.as_ref().map(crate::motion::AnimationStyle::svg_attributes).unwrap_or_default();
    let animated = css_values.map(|values| {
        let mut resolved = underlying_style.cloned().unwrap_or_default();
        values.apply_to(
            &mut resolved,
            transitioned
                .as_ref()
                .and_then(|declared| declared.transitions.as_ref()),
        );
        resolved
    });
    Animated {
        hidden: false,
        transitioned,
        moved,
        bound,
        animated,
        svg_css,
        measured,
        measurement,
        focus_visible_own,
        dynamic,
    }
}

/// What a `display: contents` element handed down in a frame, kept for the other children spliced
/// out of it.
struct ContentsFrame {
    now: web_time::Instant,
    /// The style it showed, for the elements below it that resolve against it.
    style: Option<Arc<StyleDesc>>,
    handed: crate::layout::contents::Handed,
}

/// Sets the overlay of `child_id`, a child of `parent` spliced out of the `display: contents`
/// elements between them, and what runs on those elements, in `ctx`. They are never built, so
/// their motion, motion values, transitions and keyframes are stepped here, once a frame for each
/// (`ctx.contents_styles`), by the same `animate_style` as an element that is built. What they
/// pass down is the style they show now, and a `display: contents` element that runs something
/// marks the build of each child it hands that to as dynamic, so a frame builds it, and the way
/// up to the `display: contents` element, while it runs.
fn splice_overlay(
    ctx: &mut BuildCtx,
    window: &gpui::Window,
    parent: &crate::retained_tree::RetainedElement,
    child_id: u64,
) {
    ctx.contents_overlay = None;
    ctx.contents_key = 0;
    ctx.contents_dynamic = 0;
    let Some(wrappers) = crate::layout::contents::contents_between(ctx.tree, parent, child_id) else {
        return;
    };
    if wrappers.is_empty() {
        return;
    }
    let mut frames: Vec<Arc<ContentsFrame>> = Vec::with_capacity(wrappers.len());
    for (ix, wrapper) in wrappers.iter().enumerate() {
        let frame = match ctx.contents_styles.get(&wrapper.id).filter(|frame| frame.now == ctx.now) {
            Some(frame) => frame.clone(),
            None => {
                let frame = Arc::new(contents_frame(ctx, window, &wrappers[..ix], &frames, wrapper));
                ctx.contents_styles.insert(wrapper.id, frame.clone());
                frame
            }
        };
        frames.push(frame);
    }
    if let Some(overlay) = crate::layout::contents::overlay_of(frames.iter().map(|frame| &frame.handed)) {
        ctx.contents_overlay = Some(overlay.style);
        ctx.contents_key = overlay.key;
        ctx.contents_dynamic = overlay.dynamic;
    }
}

/// `wrapper`, a `display: contents` element inside `above` (outermost first, with the frames they
/// showed), as it shows in this frame.
fn contents_frame(
    ctx: &mut BuildCtx,
    window: &gpui::Window,
    above: &[&crate::retained_tree::RetainedElement],
    frames: &[Arc<ContentsFrame>],
    wrapper: &crate::retained_tree::RetainedElement,
) -> ContentsFrame {
    let declared = wrapper.style.as_deref();
    let moves = wrapper.custom_props().contains_key("motion")
        || wrapper.custom_props().contains_key("motionValues")
        || ctx.css_transitions.wants(wrapper.id, declared)
        || ctx.css_animations.wants(wrapper.id, declared);
    if !moves {
        return ContentsFrame {
            now: ctx.now,
            style: wrapper.style.clone(),
            handed: crate::layout::contents::Handed::new(wrapper.id, declared, 0),
        };
    }
    // It inherits from the elements around it as they show now: `em`, `currentColor` and the
    // colours a transition starts from.
    let inside = above.iter().zip(frames).fold(ctx.inherited.clone(), |inherited, (above, frame)| {
        inherited.descend(frame.style.as_deref(), above.has_inert_prop())
    });
    let outside = std::mem::replace(&mut ctx.inherited, inside);
    let animated = animate_style(
        ctx,
        window,
        wrapper,
        StyleInputs {
            declared,
            base: declared,
            before_transition: wrapper.style_before_transition(),
            pointer_states: Default::default(),
            provisional: false,
            may_hide: false,
        },
    );
    ctx.inherited = outside;
    let shown = animated.current(declared);
    ContentsFrame {
        now: ctx.now,
        style: if animated.restyled() { shown.map(|style| Arc::new(style.clone())) } else { wrapper.style.clone() },
        handed: crate::layout::contents::Handed::new(wrapper.id, shown, animated.dynamic),
    }
}

fn build_element_unpumped(
    id: u64,
    ctx: &mut BuildCtx,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<GpuixView>,
) -> gpui::AnyElement {
    // Copy the tree reference out of `ctx`, so `element` borrows the tree and
    // not the build context, which the children below need mutably.
    let tree = ctx.tree;
    let stacking_root = std::mem::take(&mut ctx.stacking_root);
    let stacking_item = std::mem::take(&mut ctx.stacking_item);
    let atomic_root = std::mem::take(&mut ctx.atomic_root);
    let passed_trim = std::mem::take(&mut ctx.text_box_trim);
    // Meant for this element, and not for the next one built when it returns early.
    let contents_overlay = ctx.contents_overlay.take();
    let contents_dynamic = std::mem::take(&mut ctx.contents_dynamic);
    let anchor_scope = ctx.anchor_scope.take();
    let Some(element) = tree.elements.get(&id) else {
        return absent_host(id);
    };
    crate::perf_counters::element_built();
    if contents_dynamic != 0 {
        ctx.retain.dynamic_bits(contents_dynamic | crate::retain::Dynamic::Spliced as u8);
    }

    // `@container` blocks fold into the declared style before anything reads it, so state
    // styles, transitions and motion all see the style the containers above select.
    let containers = ctx.inherited.chain();
    let container_style = element
        .style
        .as_deref()
        .and_then(|style| crate::layout::container::fold(style, &containers));
    let declared = container_style.as_ref().or(element.style.as_deref());
    // The style it had before it declared a transition went through the same containers.
    let before_transition = element
        .style_before_transition()
        .and_then(|before| crate::layout::container::fold(before, &containers));
    // A parent builds its children by whether this element has a box and by its `order`, before
    // this element is built, so what it assumed has to be what the folded style says.
    if element.style.as_deref().is_some_and(|style| style.container_rules.is_some())
        && crate::layout::container::settle(element, declared)
    {
        window.request_redraw_before_present();
    }
    // Its container units read the axes their containers contain.
    let reads_containers = element.style.as_deref().is_some_and(StyleDesc::reads_containers);
    if let (Some(style), Some(scope)) = (element.style.as_deref(), ctx.inherited.container_chain()) {
        scope.note_reads_from(style.container_unit_axes);
    }
    // Until every container above has been laid out once, the style is a guess that the next
    // pass replaces. It must not seed a transition or an animation: the correct style would
    // then animate in from the guess.
    let provisional = reads_containers
        && ctx.inherited.container_chain().is_some_and(|scope| !scope.is_known());
    if crate::layout::box_generation::is_display_none(element) {
        tree.resolved.borrow_mut().forget(id);
        ctx.css_transitions.hide(tree, id, ctx.now);
        ctx.css_animations.hide(tree, id, ctx.now);
        ctx.retain.absent();
        return absent_host(id);
    }
    ctx.css_transitions.show(id);
    ctx.css_animations.show(id);

    // `hover`, `active`, `groupHover`, `groupActive` and `groupFocusWithin` are
    // folded into the base here, before motion, so the GPUI element never sees them: the dispatcher
    // decides who is hovered.
    let pointer_states = declared
        .map(|style| ctx.pointer_states(id, style, element.has_inert_prop()))
        .unwrap_or_default();
    let state_style = declared.and_then(|style| {
        pointer_states
            .any()
            .then(|| crate::events::state_styles::resolve_pointer_states(style, pointer_states))
    });
    let base_style = state_style.as_ref().or(declared);
    ctx.retain.pointer(pointer_states);
    ctx.retain.focus(&ctx.focus_styles, id, ctx.focus_handles.contains_key(&id));
    if let Some(links) = crate::accessibility::links_key(id) {
        ctx.retain.shape(links);
    }

    let layout_source = element.custom_props().get("layout");
    let animated = animate_style(
        ctx,
        window,
        element,
        StyleInputs {
            declared,
            base: base_style,
            before_transition: before_transition.as_ref().or(element.style_before_transition()),
            pointer_states,
            provisional,
            may_hide: true,
        },
    );
    ctx.retain.measurement(animated.measurement.as_ref());
    ctx.retain.dynamic_bits(animated.dynamic);
    if animated.dynamic & crate::retain::Dynamic::OWN != 0 {
        ctx.retain.paints_only(runs_paint_only(
            tree,
            element,
            ctx.css_animations,
            ctx.css_transitions,
            ctx.motion_states,
            ctx.keyframes,
        ));
    }
    if layout_source.is_some() {
        ctx.retain.exclude(crate::retain::Exclusion::LayoutAnimation);
    }
    if let Some(own) = animated.focus_visible_own {
        ctx.retain.focus_visible_own(own);
    }
    if animated.hidden {
        tree.resolved.borrow_mut().forget(id);
        ctx.css_transitions.hide(tree, id, ctx.now);
        ctx.css_animations.hide(tree, id, ctx.now);
        ctx.retain.absent();
        return absent_host(id);
    }
    let measured = animated.measured;
    let declared_style = animated.declared(base_style);
    let current_style = animated.current(base_style);
    let overlaid_style = contents_overlay
        .map(|overlay| current_style.unwrap_or(&EMPTY_STYLE).with_inherited_overlay(&overlay));
    let style = overlaid_style.as_deref().or(current_style);
    let anchored_style = anchor_scope.and_then(|anchored_id| {
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
    // Only a z-index differs for an item, and it is the rare style that has one, in the style or in
    // a focus variant, which is laid over the element when it is built.
    let item_style = (stacking_item && style.is_some_and(StyleDesc::declares_z_index)).then(|| {
        StyleDesc {
            flex_or_grid_item: true,
            ..style.cloned().unwrap_or_default()
        }
    });
    let style = item_style.as_ref().or(style);
    // An underline sits where the box that declares it says, and that box inherits
    // its offset.
    let offset_style = style.and_then(|style| {
        crate::css::text_underline_offset::with_inherited(style, ctx.inherited.underline_offset)
    });
    let style = offset_style.as_ref().or(style);

    if style.is_some_and(StyleDesc::lays_out_unstably) {
        ctx.retain.exclude(crate::retain::Exclusion::UnstableLayout);
    }
    let position = style.and_then(|style| style.position.as_deref());
    ctx.retain.containing_block(position.is_some_and(|position| position != "static"), position == Some("absolute"));
    match position {
        Some("sticky") => ctx.retain.exclude(crate::retain::Exclusion::Sticky),
        Some("fixed") => ctx.retain.exclude(crate::retain::Exclusion::Fixed),
        _ => {}
    }

    // Inheritable style resolves once here so both built-ins and custom
    // elements see the same cascade.
    let descended = ctx.inherited.clone().descend(style, element.has_inert_prop());
    let parent_inherited = std::mem::replace(&mut ctx.inherited, descended);
    crate::layout::table::enter(&mut ctx.inherited.table_row, &mut ctx.inherited.table_cell, element);
    // The start of a logical inline pair is the right side in an rtl element.
    let rtl_style = style
        .filter(|_| ctx.inherited.direction.direction.is_rtl())
        .and_then(StyleDesc::mirrored_inline_pairs);
    let style = rtl_style.as_ref().or(style);
    // An atomic inline's margin and percentage width are the paragraph's to lay out. They
    // resolve against the font of the box, which is known now.
    let atomic_style = atomic_root.then(|| {
        let (stripped, extracted) = crate::text::atomic::strip(
            style.unwrap_or(&EMPTY_STYLE),
            &ctx.inherited.lengths,
            ctx.atomic_height_definite,
        );
        ctx.atomic_extracted = Some(extracted);
        stripped
    });
    let style = atomic_style.as_ref().or(style);
    // What the walks over the retained tree read (`resolved_style`): the style as painted, with
    // the focus variants, which the builders lay over the element, folded in.
    let focus_folded = style.map(|style| {
        ctx.focus_styles
            .folded(id, style, ctx.focus_handles.contains_key(&id))
    });
    let resolved = crate::resolved_style::Resolved::of(
        element,
        focus_folded.as_deref(),
        &ctx.inherited.colors,
        &ctx.inherited.lengths,
    );
    tree.resolved.borrow_mut().record(id, resolved);
    let (colors, lengths) = (&ctx.inherited.colors, &ctx.inherited.lengths);
    ctx.retain.stacking(|| {
        (
            crate::layout::stacking::lift_z(&resolved, stacking_item),
            crate::layout::stacking::forms_context(focus_folded.as_deref(), stacking_root, colors, lengths),
        )
    });
    // The table this is a part of reads these to share the edges of its grid, and it was built first.
    if ctx.inherited.table.collapse && crate::layout::table::part(element).is_some() {
        tree.resolved.borrow_mut().record_borders(
            id,
            crate::css::border::sides(
                focus_folded.as_deref().unwrap_or(&EMPTY_STYLE),
                &ctx.inherited.colors,
                &ctx.inherited.lengths,
            ),
        );
    }
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

    // An element with a `containerType` or a `containerName` is a query container for what is built
    // below it. Its children see it, its own style does not (`Inherited::enter_children`).
    if let (Some(declaration), Some(sized)) = (
        style.and_then(crate::layout::container::declared),
        crate::layout::container::can_be_sized(element),
    ) {
        let parent_lengths = ctx.inherited.lengths;
        let custom = ctx.inherited.custom.clone();
        let above = ctx.inherited.containers.clone();
        ctx.inherited.self_container = Some(Arc::new(if sized {
            crate::layout::container::ContainerScope::new(
                id,
                declaration,
                ctx.containers.get(id),
                parent_lengths,
                custom,
                above,
            )
        } else {
            crate::layout::container::ContainerScope::without_size(id, declaration, parent_lengths, custom, above)
        }));
    }

    // A `highlight` here replaces any ancestor's: the nearest declaration wins,
    // and `GroupList::collect` skips nested declarations so an ancestor never
    // resolves or counts matches that will not paint.
    if let Some(value) = element.custom_props().get("highlight") {
        ctx.retain.exclude(crate::retain::Exclusion::Highlight);
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

    // What the element holds is its own to say; the element above hears it once it is built.
    let enclosing = std::mem::replace(&mut ctx.subtree, crate::layout::cull::Subtree::empty());
    let built = match element.element_type.as_str() {
        // `<text>` is a `<div>` that happens to carry a string. Giving it its
        // own builder meant every interaction prop on the shared `Props` type
        // (onClick, hover, focus, tabIndex) type-checked, registered a JS
        // listener, and then silently did nothing. `<span>` is the same box;
        // it only differs in being inline inside a paragraph.
        host if crate::text::inline::is_host_type(host) => {
            ctx.custom_registry.destroy(id);
            if crate::layout::contents::is_display_contents(element) {
                ctx.retain.exclude(crate::retain::Exclusion::DisplayContents);
                build_contents_container(element, style, stacking_root, ctx, window, cx)
            } else {
                ctx.text_box_trim = crate::css::text_box::requested(
                    style,
                    passed_trim,
                    &ctx.inherited.colors,
                    &ctx.inherited.lengths,
                );
                build_host_container(element, style, stacking_root, ctx, window, cx)
            }
        }
        "virtual-list" => {
            ctx.custom_registry.destroy(id);
            ctx.subtree.block();
            ctx.retain.exclude(crate::retain::Exclusion::VirtualList);
            build_virtual_list(element, ctx, window, cx)
        }

        // Polymorphic dispatch for all custom elements.
        custom_type => {
            // An `<svg>` paints its shape children as one image, so they are
            // serialised instead of built.
            let is_svg = custom_type == "svg";
            if custom_type == "anchored" {
                ctx.retain.exclude(crate::retain::Exclusion::Anchored);
            }
            let svg_markup = is_svg
                .then(|| {
                    let font = ctx.inherited.parent_font();
                    ctx.svg_trees.markup(
                        ctx.tree,
                        id,
                        crate::custom_elements::svg_tree::Frame {
                            motion_states: ctx.motion_states,
                            animations: &mut *ctx.css_animations,
                            keyframes: ctx.keyframes,
                            colors: &ctx.inherited.colors,
                            font: &font,
                            host: animated.svg_css.clone(),
                            now: ctx.now,
                        },
                    )
                })
                .flatten();
            let running_shapes = if is_svg {
                let running = shapes_running(
                    ctx.tree,
                    element,
                    ctx.css_animations,
                    ctx.motion_states,
                    ctx.keyframes,
                    ctx.now,
                );
                if running != 0 {
                    ctx.retain.dynamic_bits(running);
                    ctx.retain.paints_only(runs_paint_only(
                        ctx.tree,
                        element,
                        ctx.css_animations,
                        ctx.css_transitions,
                        ctx.motion_states,
                        ctx.keyframes,
                    ));
                }
                running != 0
            } else {
                false
            };
            let sized_style = is_svg
                .then(|| crate::custom_elements::svg_tree::sized_style(style, element.custom_props()))
                .flatten();
            let style = sized_style.as_ref().or(style);
            if custom_type == "anchored" {
                crate::layout::anchor_size::begin_build(window.window_handle().window_id(), tree, element);
            }
            crate::layout::container::settle_children(
                ctx.tree,
                element,
                &ctx.inherited.chain(),
                window,
            );
            let child_ids = crate::layout::contents::flatten(ctx.tree, &element.children);
            let custom_children: Vec<gpui::AnyElement> = if is_svg {
                Vec::new()
            } else {
                child_ids
                    .iter()
                    .copied()
                    .enumerate()
                    .filter(|(_, child_id)| {
                        crate::layout::box_generation::generates_box(ctx.tree, *child_id)
                    })
                    .map(|(ix, child_id)| {
                        splice_overlay(ctx, window, element, child_id);
                        if custom_type == "anchored" {
                            ctx.anchor_scope = Some(id);
                            ctx.stacking_item = resolved.lays_out_items;
                        }
                        ctx.doc_path.push(ix as u32);
                        let child = build_element(child_id, ctx, window, cx);
                        ctx.doc_path.pop();
                        child
                    })
                    .collect()
            };
            let svg_drawn = svg_markup.clone();
            let click_path = click_path(ctx.tree, element);
            if !ctx.inherited.inert {
                ctx.retain.shape(click_path.len() as u64);
            }
            ctx.paint_reports
                .borrow_mut()
                .observers
                .note_clip_origin(id, element.events(), None);
            let inherited = &ctx.inherited;
            let render_ctx = CustomRenderContext {
                id,
                doc_path: ctx.doc_path(),
                events: element.events(),
                event_callback: ctx.event_callback,
                focus_handle: ctx.focus_handles.get(&id),
                focus_styles: ctx.focus_styles.clone(),
                click_path,
                activation: ctx.activation,
                style,
                colors: inherited.colors,
                lengths: inherited.lengths,
                font_features: inherited.font_features.clone(),
                font_variations: inherited.font_variations.clone(),
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
                resolved,
            };
            let built = ctx
                .custom_registry
                .render(custom_type, element.custom_props(), render_ctx, window, cx);
            // What the adapter keeps for itself is the part of its build the tree cannot show. An
            // adapter that cannot name it is rebuilt every frame.
            match ctx.custom_registry.retain_state(id) {
                Some(state) => ctx.retain.shape(state),
                None => ctx.retain.exclude(if is_svg {
                    crate::retain::Exclusion::Svg
                } else {
                    crate::retain::Exclusion::CustomElement
                }),
            }
            if is_svg {
                // A big document that moves is drawn a paint late, by a job: a tick cannot show it.
                if running_shapes && !ctx.custom_registry.draws_changes_at_once(id) {
                    ctx.retain.exclude(crate::retain::Exclusion::Svg);
                }
                ctx.retain.shape(ctx.svg_trees.generation(id));
                ctx.retain.digest(|| format!("{svg_drawn:?}"));
            }
            // It draws and is hit as it likes, and paints and prepaints for itself.
            ctx.subtree.block();
            built
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
    let built = match layout_source {
        Some(source) => {
            // It moves the element by an offset of its own.
            ctx.subtree.block();
            ctx.layout_flips.wrap(id, source, built, ctx.now, ctx.motion_values)
        }
        None => built,
    };
    let below = std::mem::replace(&mut ctx.subtree, enclosing);
    ctx.retain.subtree(below);
    ctx.subtree.merge(below);
    built
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
    let list_path = ctx.doc_path.clone();
    let list_hosts = ctx.doc_hosts.clone();
    let render_item = cx.processor(move |view, index: usize, window, cx| {
        let Some(entry) = view.virtual_lists.get(&list_id) else {
            return unmounted_virtual_row(1.0);
        };
        let Some(child_id) = entry.child_at(index) else {
            // Empty measures as 0 and poisons ListState. Keep the estimate.
            return unmounted_virtual_row(entry.config.estimated_item_height.unwrap_or(1.0));
        };
        view.build_virtual_child(list_id, index, child_id, list_path.clone(), list_hosts.clone(), inherited.clone(), window, cx)
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
        if let Some(variations) = ctx.inherited.font_variations.clone() {
            list = list.font_variations(variations);
        }
    }
    if let Some(scope) = ctx.inherited.self_container.clone().filter(|scope| scope.is_sized()) {
        list = crate::layout::container::contain(list, scope, ctx.containers.clone());
    }
    let list = apply_accessibility(list, element.id, element.custom_props(), None);
    let default_style = StyleDesc::default();
    crate::scrollbar::attach_to_list(
        list,
        element.id,
        crate::scrollbar::Source::List(scroll_state),
        element.style.as_deref().unwrap_or(&default_style),
        &ctx.inherited.colors,
    )
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
    let mut host = gpui::div().id(crate::events::ids::host_element_id(element.id));
    // Everything up to the children goes through the slot: the element is 1.6 KB, and a helper
    // that took and returned it copied it twice.
    let mut el = crate::slot::Slot::new(&mut host);
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
            Some(&*inert_style)
        }
        style => style,
    };

    if let Some(style) = style {
        el = apply_interactive_styles(el, style, &ctx.inherited.colors, &ctx.inherited.lengths);

        if let Some(features) = ctx.inherited.font_features.clone() {
            el = el.font_features(features);
        }
        if let Some(variations) = ctx.inherited.font_variations.clone() {
            el = el.font_variations(variations);
        }
    }
    ctx.inherited.direction.apply(&mut el);
    if let Some(scope) = ctx.inherited.self_container.clone().filter(|scope| scope.is_sized()) {
        el = crate::layout::container::contain(el, scope, ctx.containers.clone());
    }

    let collapsed_table = ctx.inherited.table.collapse
        && crate::layout::table::part(element) == Some(crate::layout::table::Part::Table);
    let used_from = ctx.collapsed.used_len();
    if collapsed_table {
        resolve_collapsed_borders(ctx, CollapsedTable::Element(element, style), window.scale_factor());
    }
    el = crate::layout::table::apply(el, ctx.tree, element, style, &ctx.inherited.table, &ctx.collapsed);
    el = crate::scroll::apply_container(el, element.id, style, &ctx.inherited.colors, ctx.scroll_handles);
    let outer_scroller = ctx.scroller;
    if let Some(handle) = ctx.scroll_handles.get(&element.id) {
        // Its offset is part of what the build read (the key sees it move), and so is where its
        // children are. A hidden one is only moved by script, and that is a change like any other.
        let offset = handle.offset();
        ctx.retain.scroll(offset);
        let user = style.map(crate::scroll::Axes::of).is_some_and(|axes| axes.x.scrolls_for_user() || axes.y.scrolls_for_user());
        if user {
            ctx.retain.exclude(crate::retain::Exclusion::Scroller);
        }
        ctx.scroller = crate::retain::Scroller::of(handle, window.window_handle().window_id()).or(ctx.scroller);
    }

    // If a FocusHandle was pre-created for this element (by sync_focus_handles),
    // attach it via track_focus. This makes the element focusable — clicking it
    // or tabbing to it gives it keyboard focus. The handle persists across renders
    // because it's stored in GpuixView::focus_handles.
    if style.and_then(|style| style.position.as_deref()).is_none() {
        el.style().position = Some(gpui::Position::Static);
    }
    // The inert root claims its box as a non-selectable start region; below it,
    // a descendant's `userSelect: "text"` must not reopen one.
    let selection_start = if element.has_inert_prop() {
        Some(false)
    } else if inert {
        selection_start_flag(style).map(|_| false)
    } else {
        selection_start_flag(style)
    };
    let scroll_report = scroll_reporter(element.id, element.events(), ctx);
    if scroll_report.is_some() {
        ctx.retain.exclude(crate::retain::Exclusion::PaintReporter);
    }
    ctx.paint_reports
        .borrow_mut()
        .observers
        .note_clip_origin(element.id, element.events(), None);
    el = crate::automation::track_host_bounds(
        el,
        element.id,
        selection_start,
        ctx.scroll_handles.contains_key(&element.id),
        scroll_report,
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
    crate::layout::container::settle_children(
        ctx.tree,
        element,
        &ctx.inherited.chain(),
        window,
    );
    let plan = if element.children.is_empty() {
        Vec::new()
    } else {
        crate::text::inline::plan_children_at(ctx.tree, element)
    };
    if is_text_host && element.custom_props().get("aria-valuetext").is_none() {
        if let Some(content) = crate::text::inline::accessible_text(plan.iter().map(|(child, _)| child)) {
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
        let path = click_path(ctx.tree, element);
        ctx.retain.shape(path.len() as u64);
        el = apply_a11y_click(el, path, ctx.activation);
    }

    // Key and focus events are not wired per element either: one event per keystroke
    // goes to the focused element and bubbles along its path in JS (`events::keyboard`),
    // and focus events come from `GpuixView::observe_focus`. The wheel is a window-level
    // input like the pointer's (`events::listeners`).

    let (content, placements, containing_heights) = build_content(element, plan, ctx, window, cx);
    if collapsed_table {
        rebuild_if_borders_moved(ctx, used_from, window);
    }
    ctx.scroller = outer_scroller;
    crate::text::atomic::report_containing_height(el, containing_heights);
    let focus_folded = style.map(|style| {
        ctx.focus_styles
            .folded(element.id, style, ctx.focus_handles.contains_key(&element.id))
    });
    if crate::layout::stacking::forms_context(
        focus_folded.as_deref(),
        stacking_root,
        &ctx.inherited.colors,
        &ctx.inherited.lengths,
    ) {
        host = host.stacking_context();
    }
    ctx.subtree = crate::layout::cull::Subtree::of_element(
        ctx.subtree,
        style,
        &ctx.inherited.colors,
        &ctx.inherited.lengths,
    );
    // The borders of a collapsed table are painted by the table over its cells.
    if ctx.inherited.table.collapse {
        ctx.subtree.block();
    }
    if let Some(outset) = ctx.subtree.outset() {
        host = host.cullable(outset);
    }
    // The record the walks over the retained tree read is part of what the build made.
    let absent_children = ctx.retain.absent_children();
    ctx.retain.digest(|| {
        let resolved = ctx.tree.resolved_of(element, &ctx.inherited.colors, &ctx.inherited.lengths);
        let present: Vec<Placement> = placements
            .iter()
            .enumerate()
            .filter(|(index, _)| !absent_children.contains(index))
            .map(|(_, placement)| *placement)
            .collect();
        format!(
            "{:?} {} {:?} {:?} {:?}",
            host.style(),
            content.len() - absent_children.len(),
            present.iter().map(|placement| placement.rank).collect::<Vec<_>>(),
            Placement::document_order(&present),
            resolved
        )
    });
    host.extend(content);
    if placements.iter().any(|placement| placement.rank != 0) {
        host = host.paint_ranks(placements.iter().map(|placement| placement.rank).collect());
    }
    if let Some(positions) = Placement::document_order(&placements) {
        host = host.document_order(positions);
    }

    host.into_any_element()
}

/// The table a collapsed border is resolved for.
enum CollapsedTable<'a> {
    Element(&'a crate::retained_tree::RetainedElement, Option<&'a StyleDesc>),
    /// The anonymous table around these table parts, which has no style of its own.
    Anonymous(crate::layout::table_grid::BoxKey, &'a [u64]),
}

/// Resolves the collapsed borders of `table` for its cells and itself, and hands them to `ctx.collapsed`.
fn resolve_collapsed_borders(ctx: &mut BuildCtx, table: CollapsedTable, scale: f32) {
    use crate::css::border::{LineStyle, SideBorder};
    use crate::layout::table_grid::{BoxKey, Grid};

    /// The borders of `element` and, below a table, row group, row or column group, of its children, each read
    /// with the colours and lengths it inherits. A child is built after the table, so its borders are the ones
    /// its last build painted, transitions, animations and `motion` included, else the ones it declares with the
    /// pointer's states folded in. The table builds again when a child is painted with others.
    fn walk(
        ctx: &BuildCtx,
        element: &crate::retained_tree::RetainedElement,
        style: Option<&StyleDesc>,
        inherited: &Inherited,
        is_table: bool,
        sides: &mut HashMap<u64, [SideBorder; 4]>,
    ) {
        sides.insert(
            element.id,
            crate::css::border::sides(style.unwrap_or(&EMPTY_STYLE), &inherited.colors, &inherited.lengths),
        );
        use crate::layout::table::Part;
        if !is_table && !matches!(crate::layout::table::part(element), Some(Part::RowGroup(_) | Part::Row | Part::ColumnGroup)) {
            return;
        }
        let children = crate::layout::order::children_in_order(ctx.tree, element);
        for child in children.iter().filter_map(|id| ctx.tree.elements.get(id)) {
            walk_child(ctx, child, inherited, sides);
        }
    }

    fn walk_child(
        ctx: &BuildCtx,
        child: &crate::retained_tree::RetainedElement,
        inherited: &Inherited,
        sides: &mut HashMap<u64, [SideBorder; 4]>,
    ) {
        if crate::layout::box_generation::is_display_none(child) {
            return;
        }
        let states = child
            .style
            .as_deref()
            .map(|style| ctx.pointer_states(child.id, style, child.has_inert_prop()))
            .unwrap_or_default();
        let folded = child
            .style
            .as_deref()
            .filter(|_| states.any())
            .map(|style| crate::events::state_styles::resolve_pointer_states(style, states));
        let child_style = folded.as_ref().or(child.style.as_deref());
        let child_inherited = inherited.clone().descend(child_style, child.has_inert_prop());
        walk(ctx, child, child_style, &child_inherited, false, sides);
        if let Some(painted) = ctx.tree.resolved.borrow().borders_of(child.id) {
            sides.insert(child.id, painted);
        }
    }

    let none = || SideBorder { style: LineStyle::None, width: gpui::px(0.0), color: gpui::transparent_black() };
    let mut sides = HashMap::new();
    let (key, grid, own) = match table {
        CollapsedTable::Element(element, style) => {
            walk(ctx, element, style, &ctx.inherited, true, &mut sides);
            // A table whose width is definite lays its columns out fixed when it says so, and keeps the columns no
            // cell starts in.
            let fixed = style.is_some_and(|style| {
                style.table_layout == Some(crate::css::table::TableLayout::Fixed)
                    && style.width.as_ref().is_some_and(|width| !matches!(width, crate::style::DimensionValue::Auto))
            });
            (
                BoxKey::Element(element.id),
                Grid::build(ctx.tree, element, !fixed),
                sides[&element.id],
            )
        }
        CollapsedTable::Anonymous(key, parts) => {
            for part in parts.iter().filter_map(|id| ctx.tree.elements.get(id)) {
                walk_child(ctx, part, &ctx.inherited, &mut sides);
            }
            (key, Grid::build_anonymous(ctx.tree, parts, true), [none(); 4])
        }
    };
    let resolved = crate::layout::collapse::resolve(grid, own, scale, ctx.inherited.direction.direction.is_rtl(), &|id| sides.get(&id).copied().unwrap_or([none(); 4]));
    if let Some(resolved) = resolved {
        ctx.collapsed.insert(key, resolved);
    }
    let table_id = match table {
        CollapsedTable::Element(element, _) => Some(element.id),
        CollapsedTable::Anonymous(..) => None,
    };
    ctx.collapsed.note_used(sides.into_iter().filter(|(id, _)| Some(*id) != table_id));
}

/// Builds the table again before it is presented when a part of it, built after it, was painted with other borders than
/// the ones the table shared the edges of the grid by: `used_from` is [`Collapsed::used_len`](crate::layout::table::Collapsed)
/// before the table resolved them.
fn rebuild_if_borders_moved(ctx: &mut BuildCtx, used_from: usize, window: &mut gpui::Window) {
    let used = ctx.collapsed.used_since(used_from);
    let resolved = ctx.tree.resolved.borrow();
    if used.iter().any(|(id, used)| resolved.borders_of(*id).is_some_and(|painted| painted != *used)) {
        window.request_redraw_before_present();
    }
}

/// Where a child of a host element paints, and where it sits in the document.
#[derive(Clone, Copy, Debug)]
struct Placement {
    /// The `z-index` order of the child among its siblings.
    rank: i32,
    /// The child's position among the children of its parent in the document, which flex and
    /// grid `order` moves it away from in the built list. Tab order follows it.
    document: u32,
}

impl Placement {
    /// The document positions of `placements`, or `None` when they are in the order the children
    /// were built in.
    fn document_order(placements: &[Placement]) -> Option<Vec<u32>> {
        placements
            .windows(2)
            .any(|pair| pair[0].document > pair[1].document)
            .then(|| placements.iter().map(|placement| placement.document).collect())
    }
}

/// What a host element holds, in paint order, with the placement of each, and the cells its
/// atomic inlines read the content height of the element from.
fn build_content(
    element: &crate::retained_tree::RetainedElement,
    plan: Vec<(crate::text::inline::Child, u32)>,
    ctx: &mut BuildCtx,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<GpuixView>,
) -> (Vec<gpui::AnyElement>, Vec<Placement>, Vec<Rc<Cell<gpui::Pixels>>>) {
    let outer_heights = std::mem::take(&mut ctx.containing_heights);
    // Text content — selectable, same as a <text> leaf.
    // An empty string paints no glyphs, so it must take no space: gpui's text
    // layout still reserves a full line for "", and Solid's universal renderer
    // creates an empty placeholder text node for every dynamic hole (`<Show>`,
    // `{cond && x}`). Emitting those as full-height lines inflated every
    // conditional row. React never sends empty text, so this is a no-op there.
    let mut content_elements = Vec::new();
    let mut placements = Vec::new();
    let trim = std::mem::take(&mut ctx.text_box_trim);
    if element.children.is_empty() {
        if let Some(content) = crate::text::search::leaf_text(ctx.tree, element) {
            ctx.retain.text();
            ctx.retain.digest(|| content.to_string());
            content_elements.push(text_content(element, &content, trim, ctx));
            ctx.subtree.merge(crate::layout::cull::Subtree::of_text(
                ctx.inherited.lengths.font_size as f32,
                ctx.inherited.underline_reach,
            ));
            placements.push(Placement { rank: 0, document: 0 });
        }
    }

    // Children. Consecutive inline children are one paragraph, see
    // `text::inline`; the rest are elements of their own.
    if !element.children.is_empty() {
        // The baseline of an `inline-flex` or `inline-grid` box is its first item's.
        let probe = ctx
            .baseline_probe
            .as_ref()
            .filter(|probe| probe.container == element.id)
            .map(crate::text::atomic::Probe::clone_handle);
        build_plan(element, plan, trim, probe.as_ref(), &mut 0, &mut content_elements, &mut placements, ctx, window, cx);
    }
    let containing_heights = std::mem::replace(&mut ctx.containing_heights, outer_heights);
    (content_elements, placements, containing_heights)
}

/// Builds the children `plan` of `element` into `content_elements`, with the placement of each.
/// `anonymous` counts the anonymous cells and tables of `element` built so far.
fn build_plan(
    element: &crate::retained_tree::RetainedElement,
    plan: Vec<(crate::text::inline::Child, u32)>,
    trim: gpui::TextBoxTrim,
    probe: Option<&crate::text::atomic::Probe>,
    anonymous: &mut usize,
    content_elements: &mut Vec<gpui::AnyElement>,
    placements: &mut Vec<Placement>,
    ctx: &mut BuildCtx,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<GpuixView>,
) {
    use gpui::prelude::*;

    let in_flow = |child: &crate::text::inline::Child| match child {
        crate::text::inline::Child::Block(child_id) => !ctx
            .tree
            .elements
            .get(child_id)
            .and_then(|child| child.style.as_deref())
            .and_then(|style| style.position.as_deref())
            .is_some_and(|position| matches!(position, "absolute" | "fixed")),
        _ => true,
    };
    let first_in_flow = plan.iter().position(|(child, _)| in_flow(child));
    let last_in_flow = plan.iter().rposition(|(child, _)| in_flow(child));
    for (index, (child, at)) in plan.into_iter().enumerate() {
        let child_trim = gpui::TextBoxTrim {
            start: trim.start && Some(index) == first_in_flow,
            end: trim.end && Some(index) == last_in_flow,
        };
        // The cell is no element: what it holds sits at its own document positions.
        match child {
            crate::text::inline::Child::AnonymousCell(cell) => {
                let mut inside = Vec::new();
                let mut inside_placements = Vec::new();
                let key = crate::layout::table_grid::BoxKey::Anonymous { container: element.id, ordinal: *anonymous };
                *anonymous += 1;
                let outside = ctx.inherited.table_cell;
                ctx.inherited.table_cell = ctx
                    .inherited
                    .table_row
                    .map(|row| crate::text::selection::TableCell { row, cell: key.token() });
                build_plan(element, cell, gpui::TextBoxTrim::default(), probe, &mut 0, &mut inside, &mut inside_placements, ctx, window, cx);
                ctx.inherited.table_cell = outside;
                let halves = ctx.inherited.table.collapse.then(|| ctx.collapsed.cell(key));
                let mut cell = crate::layout::table::anonymous_cell(halves);
                ctx.inherited.direction.apply(&mut cell);
                if inside_placements.iter().any(|placement| placement.rank != 0) {
                    cell = cell.paint_ranks(inside_placements.iter().map(|placement| placement.rank).collect());
                }
                if let Some(positions) = Placement::document_order(&inside_placements) {
                    cell = cell.document_order(positions);
                }
                placements.push(Placement { rank: 0, document: at });
                content_elements.push(cell.children(inside).into_any_element());
                continue;
            }
            crate::text::inline::Child::AnonymousTable(parts) => {
                let key = crate::layout::table_grid::BoxKey::Anonymous { container: element.id, ordinal: *anonymous };
                *anonymous += 1;
                let used_from = ctx.collapsed.used_len();
                if ctx.inherited.table.collapse {
                    let ids: Vec<u64> = parts.iter().map(|(id, _)| *id).collect();
                    resolve_collapsed_borders(ctx, CollapsedTable::Anonymous(key, &ids), window.scale_factor());
                }
                let blocks = parts.into_iter().map(|(id, at)| (crate::text::inline::Child::Block(id), at)).collect();
                let mut inside = Vec::new();
                let mut inside_placements = Vec::new();
                let outside = (ctx.inherited.table_row, ctx.inherited.table_cell);
                (ctx.inherited.table_row, ctx.inherited.table_cell) = (Some(key.token()), None);
                build_plan(element, blocks, gpui::TextBoxTrim::default(), probe, &mut 0, &mut inside, &mut inside_placements, ctx, window, cx);
                (ctx.inherited.table_row, ctx.inherited.table_cell) = outside;
                rebuild_if_borders_moved(ctx, used_from, window);
                let mut table = crate::layout::table::anonymous_table(&ctx.inherited.table, ctx.collapsed.table(key));
                ctx.inherited.direction.apply(&mut table);
                if inside_placements.iter().any(|placement| placement.rank != 0) {
                    table = table.paint_ranks(inside_placements.iter().map(|placement| placement.rank).collect());
                }
                if let Some(positions) = Placement::document_order(&inside_placements) {
                    table = table.document_order(positions);
                }
                placements.push(Placement { rank: 0, document: at });
                content_elements.push(table.children(inside).into_any_element());
                continue;
            }
            _ => {}
        }
        ctx.doc_path.push(at);
        match child {
            crate::text::inline::Child::Block(child_id) => {
                let item = crate::layout::stacking::is_flex_or_grid_container(ctx.tree, element);
                let child_element = ctx.tree.elements.get(&child_id);
                placements.push(Placement {
                    document: at,
                    rank: crate::layout::stacking::paint_rank(
                        &child_element
                            .and_then(|child| child.style.as_deref())
                            .map_or(crate::resolved_style::Resolved::UNSTYLED, |style| {
                                crate::resolved_style::Resolved::of_style(style, &ctx.inherited.colors, &ctx.inherited.lengths)
                            }),
                        item,
                    ),
                });
                splice_overlay(ctx, window, element, child_id);
                ctx.stacking_item = item;
                ctx.text_box_trim = child_trim;
                let out_of_flow = child_element.is_some_and(|child| {
                    matches!(
                        child.style.as_deref().and_then(|style| style.position.as_deref()),
                        Some("absolute" | "fixed")
                    )
                });
                // A block of a box is a line of its own.
                let outer_line = ctx.inline_line.take();
                let built = if out_of_flow {
                    crate::text::atomic::out_of_flow(|| build_element(child_id, ctx, window, cx))
                } else {
                    build_element(child_id, ctx, window, cx)
                };
                ctx.inline_line = outer_line;
                if ctx.retain.take_exited_absent() {
                    ctx.retain.absent_child(content_elements.len());
                }
                content_elements.push(match probe {
                    Some(probe) if !out_of_flow && probe.is_open() => probe.watch(child_id, built),
                    _ => built,
                });
            }
            crate::text::inline::Child::Paragraph(paragraph) => {
                if let Some(probe) = probe {
                    probe.first_is_text();
                }
                placements.push(Placement { rank: 0, document: at });
                // A span's container units are fractions of the container it sits in, which
                // may be this element.
                // Only a query container changes what the paragraph's boxes inherit.
                let outside = ctx.inherited.self_container.is_some().then(|| {
                    let entered = ctx.inherited.clone().enter_children();
                    std::mem::replace(&mut ctx.inherited, entered)
                });
                // A span is not built by `build_element`, so the paragraph reports the
                // containers its boxes' units read.
                if let Some(scope) = ctx.inherited.containers.as_deref() {
                    for id in paragraph.pieces.iter().flat_map(|piece| piece.boxes.iter()) {
                        if let Some(style) = ctx.tree.elements.get(id).and_then(|span| span.style.as_deref()) {
                            scope.note_reads_from(style.container_unit_axes);
                        }
                    }
                }
                let (paragraph_element, subtree) = paragraph_content(&paragraph, child_trim, ctx, window, cx);
                content_elements.push(paragraph_element);
                ctx.subtree.merge(subtree);
                if let Some(outside) = outside {
                    ctx.inherited = outside;
                }
            }
            crate::text::inline::Child::AnonymousCell(_) | crate::text::inline::Child::AnonymousTable(_) => {
                unreachable!("handled before")
            }
        }
        ctx.doc_path.pop();
    }
}

/// A `display: contents` element that has no parent to splice into: the root, or
/// a virtual-list row. It generates no box, so it has no hitbox or bounds.
/// Its children lay out in an anonymous block in its place, and keep the text
/// style it passes down. The block carries the element's accessibility node
/// (role, `aria-*`), as Chrome keeps one for a `display: contents` element: its
/// id is not a host id, so the block is never a pointer target.
fn build_contents_container(
    element: &crate::retained_tree::RetainedElement,
    style: Option<&StyleDesc>,
    stacking_root: bool,
    ctx: &mut BuildCtx,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<GpuixView>,
) -> gpui::AnyElement {
    use gpui::prelude::*;

    let mut el = gpui::div().id(gpui::ElementId::Name(gpui::SharedString::from(format!(
        "__gpuix_contents_{}",
        element.id
    ))));
    el.style().position = Some(gpui::Position::Static);
    if stacking_root {
        el = el.stacking_context();
    }
    if let Some(style) = style {
        el = apply_styles(el, &style.inheritable_part(), &ctx.inherited.colors, &ctx.inherited.lengths);
    }
    el = apply_accessibility(el, element.id, element.custom_props(), None);
    ctx.inherited.direction.apply(&mut el);
    if let Some(features) = ctx.inherited.font_features.clone() {
        el = el.font_features(features);
    }
    crate::layout::container::settle_children(
        ctx.tree,
        element,
        &ctx.inherited.chain(),
        window,
    );
    if let Some(variations) = ctx.inherited.font_variations.clone() {
        el = el.font_variations(variations);
    }
    let plan = if element.children.is_empty() {
        Vec::new()
    } else {
        crate::text::inline::plan_children_at(ctx.tree, element)
    };
    let (content, placements, containing_heights) = build_content(element, plan, ctx, window, cx);
    el = crate::text::atomic::report_containing_height(el, containing_heights);
    if ctx.inherited.table.collapse {
        ctx.subtree.block();
    }
    if let Some(outset) = ctx.subtree.outset() {
        el = el.cullable(outset);
    }
    el.extend(content);
    if placements.iter().any(|placement| placement.rank != 0) {
        el = el.paint_ranks(placements.iter().map(|placement| placement.rank).collect());
    }
    if let Some(positions) = Placement::document_order(&placements) {
        el = el.document_order(positions);
    }
    el.into_any_element()
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
    text_box_trim: gpui::TextBoxTrim,
    ctx: &BuildCtx,
) -> gpui::AnyElement {
    selectable_text(crate::text::SelectableText {
        text_box_trim,
        group: crate::text::search::group_id(ctx.tree, element.id),
        line: ctx.inline_line.clone(),
        cell: ctx.inherited.table_cell,
        selectable: ctx.inherited.selectable,
        highlight: ctx
            .inherited
            .highlight
            .clone()
            .map(crate::text::HighlightSource::Resolved),
        cull_outset: crate::layout::cull::Subtree::of_text(
            ctx.inherited.lengths.font_size as f32,
            ctx.inherited.underline_reach,
        )
        .outset(),
        ..crate::text::SelectableText::new(
            element.id,
            0,
            ctx.doc_path(),
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
    text_box_trim: gpui::TextBoxTrim,
    ctx: &mut BuildCtx,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<GpuixView>,
) -> (gpui::AnyElement, crate::layout::cull::Subtree) {
    let motion = ctx.inline_styles(paragraph);
    if ctx.retain.recording() {
        ctx.retain.text();
        let inline_pointer = {
            let states = ctx.inline_states();
            let mut mixed = 0;
            for (index, id) in paragraph.pieces.iter().flat_map(|piece| piece.boxes.iter()).enumerate() {
                if let Some(style) = ctx.tree.elements.get(id).and_then(|span| span.style.as_deref()) {
                    mixed = crate::retain::mix_pointer(mixed, index, states(*id, style));
                }
            }
            mixed
        };
        ctx.retain.inline_pointer(inline_pointer);
        if motion.is_active(ctx.tree, &ctx.inline_states()) {
            ctx.retain.dynamic(crate::retain::Dynamic::InlineMotion);
        }
    }
    let styles = {
        let ctx = &*ctx;
        crate::text::paragraph::styles(
            ctx.tree,
            paragraph,
            &ctx.inherited.colors,
            &ctx.inherited.lengths,
            &ctx.inherited.font_state,
            ctx.inherited.underline_offset,
            &ctx.inline_states(),
            &motion,
        )
    };
    ctx.retain.digest(|| format!("{} {:?}", paragraph.text, styles.runs));
    // A box in the paragraph, or the paragraph being inline content of a box, puts the texts around
    // and inside the boxes on one line for copy.
    let line = ctx
        .inline_line
        .take()
        .or_else(|| paragraph.atomics().next().is_some().then(|| paragraph.key()));
    let (aligned_boxes, atomics) = build_atomics(paragraph, &styles, &motion, line.as_ref(), ctx, window, cx);
    let verifying = ctx.retain.verifying();
    let (paragraph_element, subtree, boxes_digest) = {
        let ctx = &*ctx;
        let states = ctx.inline_states();
        let insets = crate::text::inline_insets::InsetSpec::of_moving(
            ctx.tree,
            paragraph,
            &ctx.inherited.colors,
            &ctx.inherited.lengths,
            &states,
            &motion,
            ctx.inherited.direction.direction.is_rtl(),
        );
        let inline_decorations = crate::text::inline_decoration::of(
            ctx.tree,
            paragraph,
            &ctx.inherited.colors,
            &ctx.inherited.lengths,
            &states,
            &motion,
            &insets,
        );
        // The edges of a decoration share the spec and its memo of the last width, which is no part of what was built.
        let boxes_digest = verifying.then(|| {
            let decorations: Vec<_> = inline_decorations
                .iter()
                .map(|decoration| (&decoration.range, decoration.group, &decoration.outline, &decoration.paint))
                .collect();
            format!("{} {decorations:?}", insets.digest())
        });
        let inline_cursors = crate::text::inline_cursor::of(
            ctx.tree,
            paragraph,
            ctx.inherited.pointer_events_none,
            ctx.inherited.inert,
            &states,
            &insets,
        );
        let inline_regions = if ctx.inherited.frameless {
            crate::text::inline_region::of(ctx.tree, paragraph, ctx.inherited.app_region, &states, &insets)
        } else {
            Vec::new()
        };
        let inline_focus = focusable_inline_boxes(paragraph, ctx);
        // Plain text draws inside the boxes of its lines. What a box around part of it paints, an
        // inline element in the line and its tab stop, is not bounded by them.
        let plain = atomics.is_empty()
            && inline_decorations.is_empty()
            && inline_cursors.is_empty()
            && inline_regions.is_empty()
            && inline_focus.is_empty()
            && !insets.takes_space_along_the_line();
        let subtree = if plain {
            crate::layout::cull::Subtree::of_text(
                ctx.inherited.lengths.font_size as f32,
                ctx.inherited.underline_reach.max(crate::text::paragraph::line_reach(&styles.runs)),
            )
        } else {
            crate::layout::cull::Subtree::blocked()
        };
        let cull_outset = subtree.outset();
        let paragraph_element = selectable_text(crate::text::SelectableText {
            text_box_trim,
            group: None,
            line,
            cell: ctx.inherited.table_cell,
            selectable: ctx.inherited.selectable,
            highlight: ctx
                .inherited
                .highlight
                .clone()
                .map(crate::text::HighlightSource::Resolved),
            run_styles: styles.runs,
            opacity_groups: styles.groups,
            aligned_boxes,
            atomics,
            inline_targets: crate::text::inline_hit::Targets::of(
                ctx.tree,
                paragraph,
                ctx.inherited.pointer_events_none,
                ctx.inherited.inert,
                &insets,
            )
            .map(Arc::new),
            inline_boxes: inline_boxes(paragraph, ctx, &insets),
            inline_focus,
            inline_decorations,
            inline_cursors,
            inline_regions,
            inline_insets: insets.takes_space_along_the_line().then(|| insets.clone()),
            cull_outset,
            ..crate::text::SelectableText::new(
                paragraph.container,
                paragraph.index,
                ctx.doc_path(),
                gpui::SharedString::from(paragraph.text.clone()),
                None,
                ctx.selection.clone(),
                ctx.inherited.selection_wash,
            )
        });
        (paragraph_element, subtree, boxes_digest)
    };
    if let Some(boxes) = boxes_digest {
        ctx.retain.digest(|| boxes);
    }
    (paragraph_element, subtree)
}

/// The atomic inlines of `paragraph`, built as the elements they are, and the inline boxes the
/// line layout aligns them and the text around them to.
fn build_atomics(
    paragraph: &crate::text::inline::Paragraph,
    styles: &crate::text::paragraph::Styles,
    motion: &crate::motion::InlineStyles,
    line: Option<&Arc<str>>,
    ctx: &mut BuildCtx,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<GpuixView>,
) -> (Vec<crate::text::paragraph::BoxStyle>, Vec<gpui::AtomicInline>) {
    use crate::css::vertical_align::ResolvedVerticalAlign;

    let tree = ctx.tree;
    // The block container the paragraph is in, which `position: relative` offsets are of; the
    // boxes around the atomic below it are inline and make no containing block.
    let height_definite = ctx.inherited.container.height_is_definite();
    // What each atomic inherits from the boxes around it, and how it is aligned.
    struct Around {
        inherited: Inherited,
        overlay: Option<Box<StyleDesc>>,
        align: ResolvedVerticalAlign,
        line_height: Option<gpui::Pixels>,
    }
    let around: Vec<(u64, &crate::text::inline::Piece, Around)> = paragraph
        .atomics()
        .map(|(id, piece)| {
            let states = ctx.inline_states();
            let chain: Vec<StyleDesc> = piece
                .boxes
                .iter()
                .filter_map(|box_id| {
                    let base = motion.declared_by(tree, *box_id)?;
                    Some(motion.declared(*box_id, base, states(*box_id, base)).into_owned())
                })
                .collect();
            let mut inherited = ctx.inherited.clone();
            for (box_id, declared) in piece
                .boxes
                .iter()
                .filter(|box_id| motion.declared_by(tree, **box_id).is_some())
                .zip(&chain)
            {
                let inert = tree
                    .elements
                    .get(box_id)
                    .is_some_and(crate::retained_tree::RetainedElement::has_inert_prop);
                inherited = inherited.descend(Some(declared), inert);
                // A box that is not atomic cannot be size contained, but a query still finds it.
                if let Some(declaration) = crate::layout::container::declared(declared) {
                    inherited.self_container = Some(Arc::new(crate::layout::container::ContainerScope::without_size(
                        *box_id,
                        declaration,
                        inherited.lengths,
                        inherited.custom.clone(),
                        inherited.containers.clone(),
                    )));
                }
            }
            let overlay = (!chain.is_empty()).then(|| {
                let mut overlay = StyleDesc::inherited_through(&chain);
                if overlay.font_size.is_some() {
                    // Resolved here: `em` means the font of the box that declared it.
                    overlay.font_size = Some(crate::css::length::LinearLength::px(inherited.lengths.font_size));
                }
                Box::new(overlay)
            });
            let own = tree.elements.get(&id).and_then(|element| element.style.as_deref());
            let declared = own.map(|base| crate::events::state_styles::folded(base, states(id, base)));
            let lengths = inherited
                .lengths
                .descend(declared.as_ref().and_then(|style| style.font_size.as_ref()), false);
            let line_height = declared
                .as_ref()
                .and_then(|style| style.line_height)
                .or(overlay.as_ref().and_then(|overlay| overlay.line_height));
            let align = declared
                .as_ref()
                .and_then(|style| style.vertical_align.as_ref())
                .map_or(ResolvedVerticalAlign::Baseline, |align| align.resolve(&lengths));
            (
                id,
                piece,
                Around {
                    inherited: inherited.clone(),
                    overlay,
                    align,
                    line_height: crate::text::atomic::line_height(line_height.as_ref(), &lengths),
                },
            )
        })
        .collect();

    let shifted: HashSet<u64> = around
        .iter()
        .filter(|(_, _, around)| around.align != ResolvedVerticalAlign::Baseline)
        .map(|(id, _, _)| *id)
        .collect();
    let listed = crate::text::paragraph::listed_boxes(paragraph, &styles.boxes, &shifted);

    let mut atomics = Vec::with_capacity(around.len());
    let containing_height = Rc::new(Cell::new(gpui::Pixels::ZERO));
    for (id, piece, around) in around {
        let atomic_element = tree.elements.get(&id);
        let kind = crate::text::atomic::kind(
            atomic_element.and_then(|element| element.display()).as_deref(),
            atomic_element.and_then(|element| element.style.as_deref()),
        );
        let probe = (kind == crate::text::atomic::Kind::FirstItem)
            .then(|| crate::text::atomic::Probe::new(id));
        let outer = (
            std::mem::replace(&mut ctx.inherited, around.inherited),
            std::mem::replace(&mut ctx.baseline_probe, probe.as_ref().map(crate::text::atomic::Probe::clone_handle)),
        );
        ctx.contents_overlay = around.overlay;
        // An atomic inline is never a root, so nothing reads the key of its boxes' styles.
        ctx.contents_key = 0;
        ctx.atomic_root = true;
        ctx.atomic_extracted = None;
        ctx.atomic_height_definite = height_definite;
        // The text in the box falls in the paragraph at its placeholder, for selection and find.
        let hosts = ctx.doc_path().host(paragraph.index, piece.range.start);
        let outer_hosts = std::mem::replace(&mut ctx.doc_hosts, hosts);
        ctx.inline_line = line.cloned();
        let (built, texts) = crate::text::atomic::collecting(|| build_element(id, ctx, window, cx));
        ctx.inline_line = None;
        ctx.doc_hosts = outer_hosts;
        ctx.inherited = outer.0;
        ctx.baseline_probe = outer.1;
        // A box that built nothing (a `display` transition is hiding it) has no margin.
        let extracted = ctx
            .atomic_extracted
            .take()
            .unwrap_or_else(crate::text::atomic::Extracted::none);
        let mut inline = gpui::AtomicInline::new(piece.range.start, built);
        inline.margin = extracted.margin;
        inline.width = extracted.width;
        inline.offset = extracted.offset;
        if crate::text::atomic::is_of_containing_height(inline.offset.y) {
            inline.containing_height = containing_height.clone();
            if !ctx.containing_heights.iter().any(|cell| Rc::ptr_eq(cell, &containing_height)) {
                ctx.containing_heights.push(containing_height.clone());
            }
        }
        inline.vertical_align = crate::text::paragraph::vertical_align(around.align);
        inline.line_height = around.line_height;
        inline.parent = piece
            .boxes
            .iter()
            .rev()
            .find_map(|box_id| listed.iter().position(|listed| listed.id == *box_id));
        inline.baseline = crate::text::atomic::baseline(tree, kind, &texts, probe.as_ref());
        atomics.push(inline);
    }
    (listed, atomics)
}

/// The inline elements of a paragraph that report their own box.
///
/// An inline element paints in its paragraph's container, which clips it and what reaches it,
/// and the spans in between have no box to clip it, whatever their `overflow`: that is what
/// an `IntersectionObserver` on it needs to know of the paragraph.
fn inline_boxes(
    paragraph: &crate::text::inline::Paragraph,
    ctx: &BuildCtx,
    insets: &Rc<crate::text::inline_insets::InsetSpec>,
) -> Vec<crate::text::inline_bounds::InlineBox> {
    let mut reports = ctx.paint_reports.borrow_mut();
    paragraph
        .boxes
        .iter()
        .map(|(id, range)| {
            if let Some(element) = ctx.tree.elements.get(id) {
                reports.observers.note_clip_origin(*id, element.events(), Some(paragraph.container));
            }
            crate::text::inline_bounds::InlineBox {
                id: *id,
                range: range.clone(),
                edges: insets.edges_of(*id),
            }
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
    lengths: &crate::css::length::LengthContext,
) -> E {
    let color = decoration.resolve_color(colors);
    let text_style = el.text_style();
    if decoration.underline {
        text_style.underline = Some(decoration.underline_style(color, lengths));
    }
    if decoration.line_through {
        text_style.strikethrough = Some(decoration.strikethrough_style(color, lengths));
    }
    if decoration.overline {
        text_style.overline = Some(decoration.overline_style(color, lengths));
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
    let fill = state.resolved_fill(colors, lengths);
    let mut refined = if crate::css::border::declares_widths(state, colors) {
        let merged = crate::events::state_styles::overlay(base, state);
        apply_styles_taking_borders_from(refinement, state, &merged, colors, lengths, fill)
    } else {
        apply_styles_with_fill(refinement, state, colors, lengths, fill)
    };
    // An outline needs a style, a width and a colour, and the state may give only some of them
    // (a focus ring that sets `outlineWidth` over a base `outline`), or change the `color` the
    // base's colour follows. So it is the merged style's outline that the state refines with.
    // A refinement cannot unset the base's, so "none" is a zero-width outline.
    if state.declares_outline() || (state.color.is_some() && base.declares_outline()) {
        let merged = crate::events::state_styles::overlay(base, state);
        refined.style().outline = Some(crate::css::border::outline(&merged, colors, lengths).unwrap_or(gpui::Outline {
            width: gpui::px(0.0),
            color: gpui::transparent_black(),
            offset: gpui::px(0.0),
            style: gpui::BorderStyle::Solid,
        }));
    }
    refined
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
    el: E,
    style: &StyleDesc,
    colors: &crate::color::ColorContext,
    lengths: &LengthContext,
    fill: crate::style::Fill,
) -> E {
    apply_styles_taking_borders_from(el, style, style, colors, lengths, fill)
}

/// [`apply_styles_with_fill`] with the border widths read from `widths_from`. A state
/// refinement has to size its border from the base it sits on: the used width of a
/// side needs its style, which the state may leave to the base.
fn apply_styles_taking_borders_from<E: gpui::Styled>(
    mut el: E,
    style: &StyleDesc,
    widths_from: &StyleDesc,
    colors: &crate::color::ColorContext,
    lengths: &LengthContext,
    fill: crate::style::Fill,
) -> E {
    el = crate::layout::style::apply(el, style, colors, lengths);
    match style.effective_flex_wrap() {
        Some("wrap") => el = el.flex_wrap(),
        Some("wrap-reverse") => el = el.flex_wrap_reverse(),
        Some("nowrap") => el = el.flex_nowrap(),
        _ => {}
    }
    if let Some(grow) = style.effective_flex_grow() {
        el.style().flex_grow = Some(grow as f32);
    }
    if let Some(shrink) = style.effective_flex_shrink() {
        el.style().flex_shrink = Some(shrink as f32);
    }
    match style.effective_align_items() {
        Some("center") => el = el.items_center(),
        Some("start") | Some("flex-start") => el = el.items_start(),
        Some("end") | Some("flex-end") => el = el.items_end(),
        Some("stretch") => el = el.items_stretch(),
        Some("baseline") => el = el.items_baseline(),
        _ => {}
    }
    match style.effective_align_content() {
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
    match style.effective_justify_content() {
        Some("center") => el = el.justify_center(),
        Some("start") | Some("flex-start") => el = el.justify_start(),
        Some("end") | Some("flex-end") => el = el.justify_end(),
        Some("between") | Some("space-between") => el = el.justify_between(),
        Some("around") | Some("space-around") => el = el.justify_around(),
        Some("evenly") | Some("space-evenly") => el = el.justify_evenly(),
        _ => {}
    }
    match style.effective_align_self() {
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
        Some("left") => el = el.text_left(),
        // `start` and `end` depend on the direction of the element: `Inherited::direction`.
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
        el.text_style().tab_size = Some(tab_size.to_gpui(lengths));
    }
    if let Some(edge) = crate::css::text_box::of(style).edge {
        el.text_style().text_box_edge = Some(edge.to_gpui());
    }
    crate::css::word_break::apply(el.text_style(), style.word_break, style.overflow_wrap);
    // After `white-space`: the mode is its longhand and wins on the same element.
    crate::css::text_wrap::apply(
        el.text_style(),
        style.text_wrap,
        style.text_wrap_mode,
        style.text_wrap_style,
    );
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
    // CSS draws the decoration of a box over the inline boxes in it, and not into an atomic inline.
    if style.atomic_inline {
        el.text_style().reset_decorations = Some(true);
    }
    if let Some(decoration) = crate::css::text_decoration::of(style) {
        el = apply_text_decoration(el, &decoration, colors, lengths);
    }
    if let Some(line_height) = &style.line_height {
        el.text_style().line_height = Some(line_height.text_style_value(lengths));
    }
    if let Some(radii) = style.border_radius {
        el = el
            .rounded_tl(radii.0.top_left.resolve(lengths))
            .rounded_tr(radii.0.top_right.resolve(lengths))
            .rounded_br(radii.0.bottom_right.resolve(lengths))
            .rounded_bl(radii.0.bottom_left.resolve(lengths));
    }
    // Apply corner longhands after the shorthand so the explicit corner wins.
    if let Some(radius) = style.border_top_left_radius {
        el = el.rounded_tl(radius.resolve(lengths));
    }
    if let Some(radius) = style.border_top_right_radius {
        el = el.rounded_tr(radius.resolve(lengths));
    }
    if let Some(radius) = style.border_bottom_left_radius {
        el = el.rounded_bl(radius.resolve(lengths));
    }
    if let Some(radius) = style.border_bottom_right_radius {
        el = el.rounded_br(radius.resolve(lengths));
    }
    el = crate::css::border::apply_colors(el, style, colors);
    el = crate::css::border::apply_widths(el, widths_from, colors, lengths);
    el = crate::css::border::apply_styles(el, style, colors);
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
    if let Some(border) = style.resolved_mask_border(colors, lengths) {
        el.style().mask_border = Some(border);
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
        use crate::scroll::Overflow;
        let axes = crate::scroll::Axes::declared(style);
        let hides = |overflow| matches!(overflow, Overflow::Clip | Overflow::Hidden);
        if hides(axes.x) && hides(axes.y) {
            el = el.overflow_hidden();
        } else if axes.x == Overflow::Clip && axes.y == Overflow::Visible {
            // `clip` beside `visible` leaves the visible axis visible, which GPUI's `Hidden`
            // does not: it computes the visible axis to `auto`, as CSS does for `hidden`.
            el = el.overflow_x_clip();
        } else if axes.y == Overflow::Clip && axes.x == Overflow::Visible {
            el = el.overflow_y_clip();
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

/// `onScroll`: the scroll offset of the element each time it differs from the last paint,
/// whatever moved it. The wheel is only one cause; a scrollbar drag or `scrollTo` moves it
/// too, and gpui raises no event for those. `None` for an element that listens for nothing
/// or has no scroll handle.
///
/// Events are queued after the paint that saw the change, never from inside it.
/// The browser build calls the listener synchronously, and a handler that sets
/// state would re-enter the frame that is being painted. They go out before the frame's
/// `onLayout` and observer reports, as the DOM's scroll events precede its observers: see
/// [`emit_pending_scrolls`].
///
/// `onLayout` and the observers are not here: they are read from the published boxes after
/// the draw, see `observers::frame_sweep`.
fn scroll_reporter(
    id: u64,
    events: &HashSet<String>,
    ctx: &BuildCtx,
) -> Option<crate::automation::PaintReporter> {
    if !events.contains("scroll") {
        return None;
    }
    let handle = ctx.scroll_handles.get(&id)?.clone();
    let callback = ctx.event_callback.clone()?;
    let reports = ctx.paint_reports.clone();
    Some(Rc::new(move |_, _, _, cx| {
        report_scroll(id, handle.offset(), &reports, &callback, cx);
    }))
}

fn report_scroll(
    id: u64,
    offset: gpui::Point<gpui::Pixels>,
    reports: &SharedPaintReports,
    callback: &EventCallback,
    cx: &mut gpui::App,
) {
    let offset = (f32::from(offset.x), f32::from(offset.y));
    let mut shared = reports.borrow_mut();
    match shared.scrolls.get(&id).copied() {
        // The first paint only sets the baseline.
        None => {
            shared.scrolls.insert(id, offset);
            return;
        }
        Some(last)
            if (last.0 - offset.0).abs() < PAINT_REPORT_EPSILON
                && (last.1 - offset.1).abs() < PAINT_REPORT_EPSILON =>
        {
            return;
        }
        Some(_) => {
            shared.scrolls.insert(id, offset);
        }
    }
    shared.pending_scrolls.push((id, offset));
    if !shared.scroll_flush_queued {
        shared.scroll_flush_queued = true;
        let reports = reports.clone();
        let callback = Some(callback.clone());
        cx.defer(move |_| emit_pending_scrolls(&reports, &callback));
    }
}

/// Emits the scroll events the frame painted. Whichever runs first after the draw, the frame's
/// sweep or this flush, emits them all; the other finds none. The sweep goes first when anything
/// listens for boxes, which is what puts scroll before `onLayout`, `ResizeObserver` and
/// `IntersectionObserver` in a frame. Scrollers are in paint order.
pub(crate) fn emit_pending_scrolls(reports: &SharedPaintReports, callback: &Option<EventCallback>) {
    let pending = {
        let mut shared = reports.borrow_mut();
        shared.scroll_flush_queued = false;
        std::mem::take(&mut shared.pending_scrolls)
    };
    for (id, offset) in pending {
        emit_event_full(callback, id, "scroll", |payload| {
            payload.scroll_x = Some(f64::from(offset.0));
            payload.scroll_y = Some(f64::from(offset.1));
        });
    }
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
    /// Raw for the same reasons as `SetStyle`. `None` clears the layer.
    SetUniversalStyle {
        style: Option<&'a serde_json::value::RawValue>,
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
                    "setUniversalStyle" => BatchOp::SetUniversalStyle {
                        style: next_arg(&mut seq, "style")?,
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

/// Turn one raw `setStyle` or `setUniversalStyle` payload into a shared style.
fn intern_style_payload(
    styles: &mut StyleTable,
    payload: &serde_json::value::RawValue,
) -> BatchResult<Arc<StyleDesc>> {
    let raw = payload.get().trim();
    styles.intern(raw.as_bytes())
}

/// The styles a batch's payloads resolved to, in op order.
struct ResolvedStyles {
    /// One per `setStyle`.
    elements: Vec<Arc<StyleDesc>>,
    /// One per `setUniversalStyle`; `None` clears the layer.
    universal: Vec<Option<Arc<StyleDesc>>>,
}

/// Resolve every `setStyle` and `setUniversalStyle` payload in the batch, in op
/// order. The two are one grammar: the layer is a style like any element's.
///
/// This is the last fallible step, so it runs before the apply loop and borrows
/// only the style table. The borrow checker then proves no element was touched
/// when it returns `Err`, which is what makes a batch atomic. An earlier
/// version interned inside the apply loop, so a malformed style at the end of a
/// batch left everything before it applied and then threw.
fn resolve_styles(styles: &mut StyleTable, ops: &[BatchOp<'_>]) -> BatchResult<ResolvedStyles> {
    let mut resolved = ResolvedStyles {
        elements: Vec::new(),
        universal: Vec::new(),
    };
    for (index, op) in ops.iter().enumerate() {
        match op {
            BatchOp::SetStyle { style, .. } => {
                let shared = intern_style_payload(styles, style)
                    .map_err(|error| format!("Batch op {index} setStyle parse error: {error}"))?;
                resolved.elements.push(shared);
            }
            BatchOp::SetUniversalStyle { style } => {
                let shared = style
                    .map(|style| intern_style_payload(styles, style))
                    .transpose()
                    .map_err(|error| {
                        format!("Batch op {index} setUniversalStyle parse error: {error}")
                    })?;
                resolved.universal.push(shared);
            }
            _ => {}
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
    let resolved =
        resolve_styles(&mut tree.styles, &parsed).inspect_err(|_| tree.styles.sweep())?;
    let mut styles = resolved.elements.into_iter();
    let mut universal_styles = resolved.universal.into_iter();

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
            BatchOp::SetUniversalStyle { .. } => {
                let shared = universal_styles
                    .next()
                    .expect("one resolved style per setUniversalStyle op");
                tree.set_universal_style(shared);
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

/// Last box of a host element, as layout computed it: fractional, in 1/64 of a pixel, as
/// `getBoundingClientRect` reports it, and not the box snapped to device pixels that is painted.
#[derive(Debug, Clone)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct ElementBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// The DOM's `clientLeft` and `clientTop`: the left and top border widths, rounded to whole
    /// pixels as the DOM does.
    pub client_left: f64,
    pub client_top: f64,
}

fn unknown_web_animation(id: u64) -> String {
    format!("No running CSS animation or transition has the id {id}")
}

/// A CSS animation or transition as `Animation.currentTime`, `playbackRate` and
/// `playState` show it. Exactly one of `animationName` and `transitionProperty`
/// is set.
#[derive(Debug, Clone)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct WebAnimationInfo {
    /// Unique among the animations and transitions of the process, and the same
    /// for one run from its start to its end.
    pub id: f64,
    /// `"animation"` or `"transition"`.
    pub kind: String,
    /// The `animation-name`.
    pub animation_name: Option<String>,
    /// The CSS name of the transitioned property, `background-color`.
    pub transition_property: Option<String>,
    /// Milliseconds from the start of the animation, the delay inside it, as of
    /// the last frame.
    pub current_time: f64,
    /// Milliseconds where the effect ends, the delay inside it: what
    /// `currentTime` is at the end of a transition that finished. `Infinity`
    /// for an animation that repeats forever.
    pub end_time: f64,
    pub playback_rate: f64,
    /// `"running"`, `"paused"`, `"finished"`, or `"idle"` for a run that was
    /// cancelled and has not been released yet.
    pub play_state: String,
}

/// What the controls of a motion value's animation read, in seconds.
#[derive(Debug, Clone)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct MotionValuePlayback {
    /// Since the delay ended, up to the end of the last repeat.
    pub time: f64,
    /// The pace: 1 is real time, negative runs backwards.
    pub speed: f64,
    /// One iteration, without the delay or the repeats. `Infinity` for a spring
    /// that has not come to rest in 20 seconds.
    pub duration: f64,
    /// `duration` plus the delay.
    pub iteration_duration: f64,
    /// `"running"`, `"paused"` or `"finished"`.
    pub state: String,
}

impl From<crate::motion::PlaybackInfo> for MotionValuePlayback {
    fn from(info: crate::motion::PlaybackInfo) -> Self {
        Self {
            time: info.time,
            speed: info.speed,
            duration: info.duration,
            iteration_duration: info.iteration_duration,
            state: info.phase.name().to_string(),
        }
    }
}

impl From<crate::motion::web_animation::Info> for WebAnimationInfo {
    fn from(info: crate::motion::web_animation::Info) -> Self {
        use crate::motion::web_animation::Kind;
        let (animation_name, transition_property) = match info.kind {
            Kind::Animation => (Some(info.name), None),
            Kind::Transition => (None, Some(info.name)),
        };
        Self {
            id: info.id as f64,
            kind: info.kind.name().to_string(),
            animation_name,
            transition_property,
            current_time: info.current_time,
            end_time: info.end_time,
            playback_rate: info.playback_rate,
            play_state: info.status.name().to_string(),
        }
    }
}

impl ElementBounds {
    pub(crate) fn from_painted(bounds: crate::automation::ElementBounds) -> Self {
        Self {
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
            client_left: bounds.client_left.round(),
            client_top: bounds.client_top.round(),
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
        ("clientLeft", bounds.client_left.round()),
        ("clientTop", bounds.client_top.round()),
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
    fn giving_an_element_autofocus_is_requested_once_per_batch() {
        let mut tree = RetainedTree::new();
        apply(&mut tree, MOUNT).expect("valid batch");
        assert!(tree.take_auto_focus_request());
        assert!(!tree.take_auto_focus_request());

        // A re-render resends the prop; only a prop that turns on is a request.
        apply(&mut tree, r#"[["setCustomProp",3,"autoFocus",true]]"#).expect("valid batch");
        assert!(!tree.take_auto_focus_request());
        apply(&mut tree, r#"[["setCustomProp",3,"autoFocus",false]]"#).expect("valid batch");
        assert!(!tree.take_auto_focus_request());
        apply(&mut tree, r#"[["setCustomProp",3,"autoFocus",true]]"#).expect("valid batch");
        assert!(tree.take_auto_focus_request());
    }

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
    /// on any app that builds style objects conditionally. A shorthand and a
    /// longhand it covers are the exception: the later one wins, so the order is
    /// part of the style.
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

    #[test]
    fn reordering_a_shorthand_and_a_longhand_it_covers_repaints() {
        let mut tree = RetainedTree::new();
        apply(
            &mut tree,
            r#"[["createElement",1,"div"],["setStyle",1,{"padding":8,"paddingLeft":4}]]"#,
        )
        .expect("valid batch");
        let revision = tree.elements[&1].subtree_revision;

        apply(&mut tree, r#"[["setStyle",1,{"paddingLeft":4,"padding":8}]]"#).expect("valid batch");
        assert_ne!(
            tree.elements[&1].subtree_revision, revision,
            "the later of a shorthand and a longhand wins, so another order is another style"
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

/// The computed `font-size` through a real build, read back as the height of a line of text: with
/// `line-height: normal` the test renderer's font makes a line as tall as its ascent and descent,
/// 1.025em and 0.275em, each rounded to a whole pixel.
#[cfg(test)]
mod font_size_cascade_tests {
    use super::*;

    /// A height is rounded to the pixel grid.
    const TOLERANCE: f64 = 0.5;

    /// The height of a `normal` line of the test renderer's font at `size`.
    fn normal_line(size: f64) -> f64 {
        let rounded = |length: f64| (length + 0.5).floor();
        rounded(1025.0 * size / 1000.0) + rounded(275.0 * size / 1000.0)
    }

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
    }

    impl Page {
        /// The first node is the root. Each node gets a style-less line of text
        /// that inherits whatever size the node computes.
        fn new(cx: &mut gpui::TestAppContext, nodes: &[Node]) -> Self {
            let tree = Arc::new(Mutex::new(RetainedTree::new()));
            let root = nodes[0].id;
            {
                let mut tree = tree.lock().unwrap();
                for node in nodes {
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
                crate::layout::container::allow_redraw_passes(window);
                GpuixView::new(
                    view_tree,
                    None,
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

        fn bounds(&self, id: u64) -> crate::automation::ElementBounds {
            crate::automation::get_bounds(self.window.window_id(), id)
                .unwrap_or_else(|| panic!("element {id} was not painted"))
        }

        fn height(&self, id: u64) -> f64 {
            self.bounds(id).height
        }

        /// Each node computes its size if its probe is as tall as a line of text of that size.
        fn assert_font_sizes(&self, expected: &[(u64, f64)]) {
            for (id, size) in expected {
                let (measured, line) = (self.height(probe(*id)), normal_line(*size));
                assert!(
                    (measured - line).abs() <= TOLERANCE,
                    "node {id} should compute {size}px, a line of {line}px, but its probe is {measured}px"
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
        let bounds = crate::automation::get_bounds(page.window.window_id(), 2).unwrap();
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
        // No line height is `normal`: the font is 13px tall at 10px, its ascent of 10.25px and
        // descent of 2.75px rounded to whole pixels.
        page.set_style(cx, 2, r#"{"fontSize": 10}"#);
        assert_line_heights(&page, &[(2, 13.0)]);
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
        let bounds = crate::automation::get_bounds(page.window.window_id(), 2).unwrap();
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

    fn element_height(page: &Page, id: u64) -> f64 {
        crate::automation::get_bounds(page.window.window_id(), id)
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
        assert_eq!(element_height(&page, 2), 40.0);
        assert_eq!(element_height(&page, 3), 60.0);
        assert_eq!(element_height(&page, 4), 60.0);
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
        assert_eq!(element_height(&page, 2), 20.0);
        assert_eq!(element_height(&page, 3), 30.0);
        assert_eq!(element_height(&page, 4), 30.0);
        assert_eq!(element_height(&page, 6), element_height(&page, 5));
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
                crate::layout::container::allow_redraw_passes(window);
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
            crate::automation::get_bounds(self.window.window_id(), id).unwrap_or_else(|| panic!("element {id} was not painted"))
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
    fn the_later_of_gap_and_row_gap_column_gap_wins(cx: &mut gpui::TestAppContext) {
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
        near(page.offset(4, 2), (50.0, 0.0));
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
        let page = bordered(cx, r#""fontSize": 20, "borderWidth": "0.5em", "borderStyle": "solid""#);
        near(page.offset(3, 2), (10.0, 10.0));
        near(page.size(2), (100.0, 100.0));
        assert_eq!(border_of(&page, 2), (10.0, 10.0, 10.0, 10.0));
    }

    #[gpui::test]
    fn each_border_side_takes_its_own_length_and_beats_the_shorthand(cx: &mut gpui::TestAppContext) {
        let page = bordered(
            cx,
            r#""fontSize": 10, "borderWidth": "1in", "borderTopWidth": "calc(1px + 1rem)", "borderLeftWidth": 2,
               "borderRightWidth": "2em", "borderBottomWidth": "calc(1em - 2px)", "borderStyle": "solid""#,
        );
        assert_eq!(border_of(&page, 2), (2.0, 17.0, 20.0, 8.0));
        near(page.offset(3, 2), (2.0, 17.0));
    }

    #[gpui::test]
    fn a_negative_border_width_is_none(cx: &mut gpui::TestAppContext) {
        let page = bordered(cx, r#""borderWidth": 3, "borderTopWidth": "-1em", "borderLeftWidth": -2, "borderStyle": "solid""#);
        assert_eq!(border_of(&page, 2), (0.0, 0.0, 3.0, 3.0));
    }

    #[gpui::test]
    fn the_keywords_are_one_three_and_five_px(cx: &mut gpui::TestAppContext) {
        let page = bordered(
            cx,
            r#""borderLeftWidth": "thin", "borderTopWidth": "medium", "borderRightWidth": "thick", "borderBottomWidth": "Thick", "borderStyle": "solid""#,
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
            r#""fontSize": 20, "borderWidth": 1, "borderLeftWidth": "0.5em", "borderStyle": "solid", "hover": {"borderWidth": "1em"}"#,
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

    #[gpui::test]
    fn a_border_with_no_style_takes_no_space(cx: &mut gpui::TestAppContext) {
        for style in [
            r#""borderWidth": 4"#,
            r#""borderWidth": 4, "borderColor": "red""#,
            r#""borderLeftWidth": 4, "borderTopWidth": "1em""#,
            r#""border": "4px red""#,
            r#""borderLeft": "thick", "borderBottom": "2px red""#,
            r#""borderWidth": 4, "borderStyle": "none""#,
            r#""borderWidth": 4, "borderStyle": "hidden""#,
        ] {
            let page = bordered(cx, style);
            assert_eq!(border_of(&page, 2), (0.0, 0.0, 0.0, 0.0), "{style}");
            near(page.offset(3, 2), (0.0, 0.0));
        }
    }

    #[gpui::test]
    fn a_border_style_gives_the_width_its_space_side_by_side(cx: &mut gpui::TestAppContext) {
        let page = bordered(cx, r#""borderWidth": 4, "borderStyle": "solid""#);
        assert_eq!(border_of(&page, 2), (4.0, 4.0, 4.0, 4.0));
        near(page.offset(3, 2), (4.0, 4.0));

        let page = bordered(cx, r#""borderBottomWidth": 2, "borderBottomStyle": "solid""#);
        assert_eq!(border_of(&page, 2), (0.0, 0.0, 0.0, 2.0));

        let page = bordered(cx, r#""borderWidth": 4, "borderStyle": "solid none""#);
        assert_eq!(border_of(&page, 2), (0.0, 4.0, 0.0, 4.0));
    }

    #[gpui::test]
    fn a_visible_border_style_without_a_width_is_medium(cx: &mut gpui::TestAppContext) {
        let page = bordered(cx, r#""borderStyle": "solid""#);
        assert_eq!(border_of(&page, 2), (3.0, 3.0, 3.0, 3.0));
        let page = bordered(cx, r#""borderTopStyle": "dashed""#);
        assert_eq!(border_of(&page, 2), (0.0, 3.0, 0.0, 0.0));
    }

    #[gpui::test]
    fn a_hover_width_sizes_the_border_the_base_style_makes_visible(cx: &mut gpui::TestAppContext) {
        let page = bordered(cx, r#""borderStyle": "solid", "borderWidth": 1, "hover": {"borderWidth": 3}"#);
        assert_eq!(border_of(&page, 2), (1.0, 1.0, 1.0, 1.0));
        hover(&page, cx, 2);
        assert_eq!(border_of(&page, 2), (3.0, 3.0, 3.0, 3.0));
    }

    #[gpui::test]
    fn a_hover_style_makes_a_border_visible_or_not(cx: &mut gpui::TestAppContext) {
        let page = bordered(cx, r#""borderWidth": 2, "hover": {"borderStyle": "solid"}"#);
        assert_eq!(border_of(&page, 2), (0.0, 0.0, 0.0, 0.0));
        hover(&page, cx, 2);
        assert_eq!(border_of(&page, 2), (2.0, 2.0, 2.0, 2.0));

        let page = bordered(cx, r#""border": "2px solid red", "hover": {"borderStyle": "none"}"#);
        assert_eq!(border_of(&page, 2), (2.0, 2.0, 2.0, 2.0));
        hover(&page, cx, 2);
        assert_eq!(border_of(&page, 2), (0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn a_state_refinement_sizes_its_border_from_the_base_it_sits_on() {
        fn widths(base: &str, state: &str) -> [Option<f32>; 4] {
            let base: StyleDesc = serde_json::from_str(base).unwrap();
            let state: StyleDesc = serde_json::from_str(state).unwrap();
            let colors = crate::color::ColorContext::root(crate::color::Scheme::Light);
            let lengths = LengthContext::initial(800.0, 600.0);
            let refinement = apply_state_styles(gpui::StyleRefinement::default(), &base, &state, &colors, &lengths);
            let edges = refinement.border_widths;
            [edges.top, edges.right, edges.bottom, edges.left]
                .map(|width| width.map(|width| f32::from(width.to_pixels(gpui::px(16.0)))))
        }
        assert_eq!(widths(r#"{"borderStyle": "solid"}"#, r#"{"borderWidth": 2}"#), [Some(2.0); 4]);
        assert_eq!(widths(r#"{"borderWidth": 1}"#, r#"{"borderWidth": 2}"#), [Some(0.0); 4]);
        assert_eq!(widths(r#"{"borderWidth": 1}"#, r#"{"borderStyle": "solid"}"#), [Some(1.0); 4]);
        assert_eq!(
            widths(r#"{"border": "4px solid red"}"#, r#"{"borderLeftStyle": "none"}"#),
            [Some(4.0), Some(4.0), Some(4.0), Some(0.0)]
        );
        assert_eq!(widths(r#"{"border": "4px solid red"}"#, r#"{"borderColor": "blue"}"#), [None; 4]);
    }

    #[test]
    fn the_outline_resolves_in_the_elements_font_size() {
        let style: StyleDesc =
            serde_json::from_str(r#"{"outlineWidth": "0.1em", "outlineStyle": "solid", "outlineOffset": "calc(0.5em + 1px)", "outlineColor": "red"}"#).unwrap();
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
        let gpui::BackgroundImage::Gradient(gradient) = &layers[0].image else { panic!("an image layer") };
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
                (2, Some(1), r#"{"width": 100, "height": 100, "fontSize": 20, "borderWidth": "0.5em", "borderLeftWidth": "calc(1em + 1px)", "borderStyle": "solid", "padding": 5}"#),
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
    // 16px, and a line is 20px high: `line-height: normal` is the ascent and descent of the font,
    // 1.025em and 0.275em, which are 16.4px and 4.4px at 16px and each round to whole pixels.
    // `min-content` is the widest word: the space after it hangs and is not counted, as the text
    // is laid out.
    const CHAR: f64 = 9.6;
    const LINE: f64 = 20.0;
    const HELLO_WORLD: &str = "hello world";
    const LONG: &str = "hello wonderful world";
    const COLUMN: &str = r#"{"width": 600}"#;

    /// The width of the text up to 1/64 of a pixel, as layout has it and as a box reports it.
    fn text_width(characters: usize) -> f64 {
        (characters as f64 * CHAR * 64.0).ceil() / 64.0
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
        near(page.size(2), (text_width(9), 3.0 * LINE));
    }

    #[gpui::test]
    fn fit_content_is_the_available_width_between_min_and_max_content(cx: &mut gpui::TestAppContext) {
        let short = text_box(cx, r#"{"width": 300}"#, r#"{"width": "fit-content"}"#, HELLO_WORLD);
        near(short.size(2), (text_width(11), LINE));
        let long = text_box(cx, r#"{"width": 150}"#, r#"{"width": "fit-content"}"#, LONG);
        near(long.size(2), (150.0, 2.0 * LINE));
        let narrow = text_box(cx, r#"{"width": 40}"#, r#"{"width": "fit-content"}"#, LONG);
        near(narrow.size(2), (text_width(9), 3.0 * LINE));
    }

    #[gpui::test]
    fn fit_content_with_a_limit_clamps_it_between_min_and_max_content(cx: &mut gpui::TestAppContext) {
        let page = text_box(cx, COLUMN, r#"{"width": "fit-content(100px)"}"#, LONG);
        near(page.size(2), (100.0, 3.0 * LINE));
        page.set_style(cx, 2, r#"{"width": "fit-content(10px)"}"#);
        near(page.size(2), (text_width(9), 3.0 * LINE));
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
        // 20px: a character is 12px, min-content 120px, max-content 252px, and 15em is 300px. A line
        // is 27px: the ascent 20.5px and the descent 5.5px round up to 21px and 6px.
        near(page.size(2), (252.0, 27.0));
        page.set_style(cx, 2, r#"{"fontSize": 20, "width": "fit-content(10em)"}"#);
        near(page.size(2), (200.0, 54.0));
        // 10px: 10.25px and 2.75px round to 10px and 3px, a line of 13px.
        page.set_style(cx, 2, r#"{"fontSize": 10, "width": "fit-content(10em)"}"#);
        near(page.size(2), (100.0, 26.0));
    }

    #[gpui::test]
    fn min_width_min_content_keeps_a_narrow_box_from_squeezing_its_text(cx: &mut gpui::TestAppContext) {
        let page = text_box(cx, COLUMN, r#"{"width": 50, "minWidth": "min-content"}"#, LONG);
        near(page.size(2), (text_width(9), 3.0 * LINE));
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
        near(page.size(4), (text_width(9), 3.0 * LINE));
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
        near(page.size(2), (text_width(9), 3.0 * LINE));
        near(page.offset(4, 1), (text_width(9), 0.0));
    }

    #[gpui::test]
    fn a_flex_basis_keyword_works_in_a_reversed_row_and_beats_a_width_keyword(cx: &mut gpui::TestAppContext) {
        let reversed = r#"{"width": 600, "display": "flex", "flexDirection": "row-reverse"}"#;
        let page = basis_row(cx, reversed, r#"{"flexBasis": "min-content", "flexShrink": 0}"#, LONG);
        near(page.size(2), (text_width(9), 3.0 * LINE));
        let both = basis_row(
            cx,
            ROW,
            r#"{"width": "min-content", "flexBasis": "max-content", "flexShrink": 0}"#,
            LONG,
        );
        near(both.size(2), (text_width(21), LINE));
    }

    #[gpui::test]
    fn a_fit_content_flex_basis_is_the_container_width_between_the_content_sizes(cx: &mut gpui::TestAppContext) {
        let page = basis_row(cx, ROW, r#"{"flexBasis": "fit-content", "flexShrink": 0}"#, LONG);
        near(page.size(2), (text_width(21), LINE));

        // Narrower than the longest line it is the container, and an item that cannot shrink stays there.
        let narrow = r#"{"width": 150, "display": "flex"}"#;
        near(basis_row(cx, narrow, r#"{"flexBasis": "fit-content", "flexShrink": 0}"#, LONG).size(2), (150.0, 2.0 * LINE));
        near(basis_row(cx, narrow, r#"{"flexBasis": "fit-content"}"#, LONG).size(2), (150.0, 2.0 * LINE));
        let narrower = r#"{"width": 20, "display": "flex"}"#;
        near(
            basis_row(cx, narrower, r#"{"flexBasis": "fit-content", "flexShrink": 0}"#, LONG).size(2),
            (text_width(9), 3.0 * LINE),
        );

        // The container's content box less the item's margins.
        let padded = r#"{"width": 190, "paddingLeft": 20, "paddingRight": 20, "display": "flex"}"#;
        near(
            basis_row(cx, padded, r#"{"flexBasis": "fit-content", "flexShrink": 0, "marginLeft": 10}"#, LONG).size(2),
            (140.0, 3.0 * LINE),
        );
    }

    #[gpui::test]
    fn a_fit_content_width_is_the_flex_base_size_of_a_row_item_without_a_basis(cx: &mut gpui::TestAppContext) {
        let narrow = r#"{"width": 150, "display": "flex"}"#;
        let page = basis_row(cx, narrow, r#"{"width": "fit-content", "flexShrink": 0}"#, LONG);
        near(page.size(2), (150.0, 2.0 * LINE));

        // The width is no limit: the item grows from it, sharing the free space with the sibling.
        let growing = basis_row(cx, ROW, r#"{"width": "fit-content", "flexGrow": 1}"#, HELLO_WORLD);
        near(growing.size(2), ((600.0 + text_width(11)) / 2.0, LINE));

        // A basis of its own wins, and the item is held up by its content only.
        let basis = basis_row(cx, narrow, r#"{"width": "fit-content", "flexBasis": 20, "flexShrink": 0}"#, LONG);
        near(basis.size(2), (text_width(9), 3.0 * LINE));
    }

    #[gpui::test]
    fn a_flex_basis_limit_is_clamped(cx: &mut gpui::TestAppContext) {
        let limited = basis_row(cx, ROW, r#"{"flexBasis": "fit-content(100px)", "flexShrink": 0}"#, LONG);
        near(limited.size(2), (100.0, 3.0 * LINE));
        limited.set_style(cx, 2, r#"{"flexBasis": "fit-content(10px)", "flexShrink": 0}"#);
        near(limited.size(2), (text_width(9), 3.0 * LINE));
        limited.set_style(cx, 2, r#"{"flexBasis": "fit-content(500px)", "flexShrink": 0}"#);
        near(limited.size(2), (text_width(21), LINE));
        limited.set_style(cx, 2, r#"{"flexBasis": "fit-content(25%)", "flexShrink": 0}"#);
        near(limited.size(2), (150.0, 2.0 * LINE));
    }

    #[gpui::test]
    fn a_content_flex_basis_ignores_the_width_and_is_clamped_by_the_limits(cx: &mut gpui::TestAppContext) {
        let page = basis_row(cx, ROW, r#"{"width": 50, "flexBasis": "content", "flexShrink": 0}"#, LONG);
        near(page.size(2), (text_width(21), LINE));
        page.set_style(cx, 2, r#"{"width": 50, "flexBasis": "content", "flexShrink": 0, "maxWidth": 60}"#);
        near(page.size(2), (60.0, 3.0 * LINE));
        page.set_style(cx, 2, r#"{"width": 50, "flexBasis": "content", "flexShrink": 0, "minWidth": 300}"#);
        near(page.size(2), (300.0, LINE));
        // The basis of a flex item is also read from `flex`.
        page.set_style(cx, 2, r#"{"width": 50, "flex": "0 0 content"}"#);
        near(page.size(2), (text_width(21), LINE));
    }

    #[gpui::test]
    fn a_flex_basis_keyword_is_content_along_a_column_and_nothing_in_a_block(cx: &mut gpui::TestAppContext) {
        let column = r#"{"width": 600, "display": "flex", "flexDirection": "column"}"#;
        let reversed = r#"{"width": 600, "display": "flex", "flexDirection": "column-reverse"}"#;
        for parent in [column, reversed] {
            for keyword in ["content", "min-content", "max-content", "fit-content", "fit-content(50px)"] {
                // The height is not the basis: the content height at the width the item gets is.
                let item = format!(r#"{{"flexBasis": "{keyword}", "height": 100, "flexShrink": 0}}"#);
                let page = text_box(cx, parent, &item, LONG);
                near(page.size(2), (600.0, LINE));
            }
            let auto = text_box(cx, parent, r#"{"flexBasis": "auto", "height": 100, "flexShrink": 0}"#, LONG);
            near(auto.size(2), (600.0, 100.0));
        }
        for keyword in ["content", "min-content", "max-content", "fit-content", "fit-content(50px)"] {
            let item = format!(r#"{{"flexBasis": "{keyword}"}}"#);
            near(text_box(cx, r#"{"width": 600, "display": "block"}"#, &item, LONG).size(2), (600.0, LINE));
        }
    }

    #[gpui::test]
    fn a_column_item_with_a_content_basis_wraps_to_its_width(cx: &mut gpui::TestAppContext) {
        let column = r#"{"width": 150, "display": "flex", "flexDirection": "column"}"#;
        near(text_box(cx, column, r#"{"flexBasis": "content", "flexShrink": 0}"#, LONG).size(2), (150.0, 2.0 * LINE));
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
        near(page.size(2), (text_width(9), 3.0 * LINE));
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
            Some(Some(gpui::FlexBasisKeyword::FitContentLimit(gpui::px(32.0).into())))
        );
        assert_eq!(keyword.flex_basis, Some(gpui::Length::Auto));

        for replacement in [r#"{"flexBasis": 50}"#, r#"{"flexBasis": "auto"}"#, r#"{"flexBasis": "calc(1em + 1%)"}"#] {
            let cleared = refine(&[r#"{"flexBasis": "max-content"}"#, replacement]);
            assert_eq!(cleared.intrinsic_widths.flex_basis, Some(None), "{replacement}");
        }
        let back = refine(&[r#"{"flexBasis": 50}"#, r#"{"flexBasis": "min-content"}"#]);
        assert_eq!(back.intrinsic_widths.flex_basis, Some(Some(gpui::FlexBasisKeyword::MinContent)));
        assert_eq!(back.flex_basis, Some(gpui::Length::Auto));

        let untouched = refine(&[r#"{"flexBasis": "max-content"}"#, r#"{"width": 5}"#]);
        assert_eq!(untouched.intrinsic_widths.flex_basis, Some(Some(gpui::FlexBasisKeyword::MaxContent)));
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
            // The next one has the width, and the clock started at the style change.
            step(&page, cx, 500.0);
            near(page.size(2), (60.0, 100.0));
            assert_eq!(natural_width(&page, cx, 2), Some(120.0));
            step(&page, cx, 500.0);
            near(page.size(2), (120.0, 100.0));
            step(&page, cx, 500.0);
            near(page.size(2), (120.0, 100.0));
        }

        /// The width at each of two frames 16ms apart after the style changed.
        fn first_frames(page: &Page, cx: &mut gpui::TestAppContext, description: serde_json::Value) -> [f64; 3] {
            set_motion(page, cx, description);
            let mut widths = [page.size(2).0, 0.0, 0.0];
            for width in &mut widths[1..] {
                step(page, cx, 16.0);
                *width = page.size(2).0;
            }
            widths
        }

        fn linear_width(from: serde_json::Value, to: serde_json::Value) -> serde_json::Value {
            serde_json::json!({
                "initial": { "width": from },
                "animate": { "width": to },
                "transition": { "duration": 0.1, "ease": "linear" }
            })
        }

        #[gpui::test]
        fn an_auto_width_animation_is_as_far_along_as_a_px_one_on_the_frame_after_the_change(
            cx: &mut gpui::TestAppContext,
        ) {
            let px = first_frames(&page(cx, r#"{"display": "flex"}"#), cx, linear_width(0.into(), 120.into()));
            let auto = first_frames(&page(cx, r#"{"display": "flex"}"#), cx, linear_width(0.into(), "auto".into()));
            assert_eq!(auto[0], 0.0);
            for (frame, (auto, px)) in auto.iter().zip(px).enumerate() {
                assert!((auto - px).abs() < 0.01, "frame {frame}: auto {auto}, px {px}");
            }
            assert!(auto[1] > 0.0, "the first frame after the change already moved");
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

    #[gpui::test]
    fn a_container_query_applies_in_the_first_frame(cx: &mut gpui::TestAppContext) {
        let page = Page::new(
            cx,
            &[
                (1, None, r#"{"width": 500}"#),
                (2, Some(1), r#"{"containerType": "inline-size", "width": "100%"}"#),
                (3, Some(2), r#"{"height": 20, "width": 100, "@container": {"(min-width: 400px)": {"width": 300}}}"#),
            ],
        );
        assert_eq!(page.size(3), (300.0, 20.0));
    }
}

/// A span's `fontSize` and `lineHeight` through a real layout: the height of the
/// container is the sum of the heights of its rows.
#[cfg(test)]
mod inline_run_size_tests {
    use super::*;

    const TOLERANCE: f64 = 0.5;

    /// A container (id 1) of `style` holding `text`, then a span (id 3) of
    /// `span_style` holding `span_text`. Returns the container's height.
    fn height_of(
        cx: &mut gpui::TestAppContext,
        style: &str,
        text: &str,
        span_style: &str,
        span_text: &str,
    ) -> f64 {
        let tree = Arc::new(Mutex::new(RetainedTree::new()));
        {
            let mut tree = tree.lock().unwrap();
            let container: StyleDesc = serde_json::from_str(style).unwrap();
            tree.create_element(1, "div".to_owned());
            tree.set_style(1, Arc::new(container));
            tree.set_root(1);
            tree.create_element(2, "text".to_owned());
            tree.set_text(2, text.to_owned());
            tree.append_child(1, 2);
            tree.create_element(3, "span".to_owned());
            let span: StyleDesc = serde_json::from_str(span_style).unwrap();
            tree.set_style(3, Arc::new(span));
            tree.append_child(1, 3);
            tree.create_element(4, "text".to_owned());
            tree.set_text(4, span_text.to_owned());
            tree.append_child(3, 4);
        }
        let window = cx.add_window(|window, _| {
            crate::events::enable_dom_hit_testing(window);
            crate::layout::container::allow_redraw_passes(window);
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
        crate::automation::get_bounds(window.window_id(), 1).expect("the container was painted").height
    }

    const CONTAINER: &str = r#"{"width": 120, "fontSize": 10, "lineHeight": 1.5}"#;

    fn assert_height(measured: f64, expected: f64) {
        assert!(
            (measured - expected).abs() <= TOLERANCE,
            "expected a height of {expected}px but measured {measured}px"
        );
    }

    #[gpui::test]
    fn a_larger_span_makes_its_row_as_tall_as_its_own_line_box(cx: &mut gpui::TestAppContext) {
        let plain = height_of(cx, CONTAINER, "a ", r#"{}"#, "b");
        assert_height(plain, 15.0);
        let sized = height_of(cx, CONTAINER, "a ", r#"{"fontSize": 30}"#, "b");
        assert_height(sized, 45.0);
    }

    #[gpui::test]
    fn a_span_line_height_without_a_font_size_sets_its_rows_height(cx: &mut gpui::TestAppContext) {
        let tall = height_of(cx, CONTAINER, "a ", r#"{"lineHeight": 3}"#, "b");
        assert_height(tall, 30.0);
        let length = height_of(cx, CONTAINER, "a ", r#"{"lineHeight": "4em"}"#, "b");
        assert_height(length, 40.0);
    }

    #[gpui::test]
    fn sized_text_wraps_where_it_ends_and_the_rows_add_up(cx: &mut gpui::TestAppContext) {
        let words = "word word word word word word";
        let plain = height_of(cx, CONTAINER, "", r#"{}"#, words);
        let sized = height_of(cx, CONTAINER, "", r#"{"fontSize": 30}"#, words);
        let plain_rows = (plain / 15.0).round();
        let sized_rows = (sized / 45.0).round();
        assert_height(plain, plain_rows * 15.0);
        assert_height(sized, sized_rows * 45.0);
        assert!(
            sized_rows > plain_rows,
            "the large text wraps earlier: {sized_rows} rows against {plain_rows}"
        );
    }

    /// What `spans_painted` measured.
    struct Spans {
        container: crate::automation::ElementBounds,
        spans: Vec<crate::automation::ElementBounds>,
        /// The ascent and descent of the font of the text at each of the sizes asked for, each
        /// rounded to a whole pixel as Chrome does.
        metrics: Vec<(f64, f64)>,
    }

    /// A container (id 1) of `style` that holds `spans`, each a span of its style
    /// around its text, and the bounds the painted spans report.
    fn spans_painted(
        cx: &mut gpui::TestAppContext,
        style: &str,
        spans: &[(&str, &str)],
        sizes: &[f32],
    ) -> Spans {
        let tree = Arc::new(Mutex::new(RetainedTree::new()));
        {
            let mut tree = tree.lock().unwrap();
            let container: StyleDesc = serde_json::from_str(style).unwrap();
            tree.create_element(1, "div".to_owned());
            tree.set_style(1, Arc::new(container));
            tree.set_root(1);
            for (ix, (span_style, text)) in spans.iter().enumerate() {
                let (span, text_id) = (10 + 2 * ix as u64, 11 + 2 * ix as u64);
                tree.create_element(span, "span".to_owned());
                let span_style: StyleDesc = serde_json::from_str(span_style).unwrap();
                tree.set_style(span, Arc::new(span_style));
                tree.append_child(1, span);
                tree.create_element(text_id, "text".to_owned());
                tree.set_text(text_id, (*text).to_owned());
                tree.append_child(span, text_id);
            }
        }
        let window = cx.add_window(|window, _| {
            crate::events::enable_dom_hit_testing(window);
            crate::layout::container::allow_redraw_passes(window);
            GpuixView::new(
                tree,
                None,
                "test".to_owned(),
                SharedSelection::default(),
                crate::events::state::PointerRequests::default(),
            )
        });
        let metrics = cx
            .update_window(window.into(), |_, window, app| {
                window.refresh();
                window.draw(app).clear(app);
                let text_system = app.text_system().clone();
                let font = text_system.resolve_font(&window.text_style().font());
                sizes
                    .iter()
                    .map(|size| {
                        let size = gpui::px(*size);
                        let line_box = text_system.line_box(font, size);
                        (
                            f64::from(f32::from(line_box.ascent)),
                            f64::from(f32::from(line_box.descent)),
                        )
                    })
                    .collect()
            })
            .unwrap();
        Spans {
            container: crate::automation::get_bounds(window.window_id(), 1).expect("the container was painted"),
            spans: (0..spans.len())
                .map(|ix| {
                    crate::automation::get_bounds(window.window_id(), 10 + 2 * ix as u64)
                        .expect("the span was painted")
                })
                .collect(),
            metrics,
        }
    }

    fn assert_near(measured: f64, expected: f64, what: &str) {
        assert!(
            (measured - expected).abs() <= TOLERANCE,
            "expected {what} of {expected}px but measured {measured}px"
        );
    }

    #[gpui::test]
    fn a_span_with_a_font_size_has_the_content_area_of_its_own_font(cx: &mut gpui::TestAppContext) {
        let painted = spans_painted(
            cx,
            CONTAINER,
            &[(r#"{"fontSize": 30}"#, "big")],
            &[10.0, 30.0],
        );
        let (ascent, descent) = painted.metrics[1];
        let span = painted.spans[0];
        assert_near(span.height, ascent + descent, "a height");
        // The only thing on the row, so its line box is 45px and its font is
        // centred in it, with the half-leading above it rounded down.
        assert_near(
            span.y - painted.container.y,
            ((45.0 - ascent - descent) / 2.0_f64).floor(),
            "a top",
        );
    }

    #[gpui::test]
    fn spans_of_different_sizes_in_one_row_each_keep_their_own_content_area_on_one_baseline(
        cx: &mut gpui::TestAppContext,
    ) {
        let painted = spans_painted(
            cx,
            CONTAINER,
            &[(r#"{"fontSize": 30}"#, "big"), (r#"{}"#, "s")],
            &[10.0, 30.0],
        );
        let ((small_ascent, small_descent), (big_ascent, big_descent)) =
            (painted.metrics[0], painted.metrics[1]);
        let (big, small) = (painted.spans[0], painted.spans[1]);
        assert_near(big.height, big_ascent + big_descent, "the height of the big span");
        assert_near(small.height, small_ascent + small_descent, "the height of the small span");
        assert_near(
            (small.y + small_ascent) - (big.y + big_ascent),
            0.0,
            "the baselines to differ",
        );
        assert_near(
            painted.container.height,
            45.0,
            "the height of the row, which the big span sets",
        );
    }

    /// A container (id 1) of three spans (ids 3, 5 and 7), a large one and two of the
    /// size of the container, each of which is alone on its row after the first.
    /// Returns what `boxes_under` says for each of the points `probe` is given, with the
    /// layout the paragraph was painted in.
    fn hits_over(
        cx: &mut gpui::TestAppContext,
        probe: impl Fn(&gpui::TextLayout, gpui::WindowId) -> Vec<gpui::Point<gpui::Pixels>>,
    ) -> (gpui::TextLayout, Vec<Vec<u64>>) {
        let tree = Arc::new(Mutex::new(RetainedTree::new()));
        {
            let mut tree = tree.lock().unwrap();
            let container: StyleDesc = serde_json::from_str(CONTAINER).unwrap();
            tree.create_element(1, "div".to_owned());
            tree.set_style(1, Arc::new(container));
            tree.set_root(1);
            for (span, style, text) in [
                (3, r#"{"fontSize": 30}"#, "Big "),
                (5, r#"{}"#, "bbbbbbbbbbbbbbbb "),
                (7, r#"{}"#, "cccccccccccccccc"),
            ] {
                tree.create_element(span, "span".to_owned());
                let style: StyleDesc = serde_json::from_str(style).unwrap();
                tree.set_style(span, Arc::new(style));
                tree.append_child(1, span);
                tree.create_element(span + 1, "text".to_owned());
                tree.set_text(span + 1, text.to_owned());
                tree.append_child(span, span + 1);
            }
        }
        let window = cx.add_window(|window, _| {
            crate::events::enable_dom_hit_testing(window);
            crate::layout::container::allow_redraw_passes(window);
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
            let id = window.window_handle().window_id();
            let layout = crate::text::inline_hit::painted_layout(id, 1);
            let hits = probe(&layout, id)
                .into_iter()
                .map(|point| {
                    crate::text::inline_hit::boxes_under(id, 1, point)
                        .into_iter()
                        .map(|(id, _)| id)
                        .collect()
                })
                .collect();
            (layout, hits)
        })
        .unwrap()
    }

    #[gpui::test]
    fn the_pointer_is_on_the_span_of_the_row_it_is_in_when_rows_differ_in_height(
        cx: &mut gpui::TestAppContext,
    ) {
        let top = |window, span: u64| crate::automation::get_bounds(window, span).unwrap().y as f32;
        let height = |window, span: u64| crate::automation::get_bounds(window, span).unwrap().height as f32;
        // The pointer over the first glyph of a row, `y` below the top of the window.
        let at = |layout: &gpui::TextLayout, y: f32| {
            gpui::point(layout.bounds().left() + gpui::px(3.0), gpui::px(y))
        };
        let (layout, hits) = hits_over(cx, |layout, window| {
            vec![
                // The large span's content area is in the first, 45px, row. Under
                // rows of 15px, these points are in the rows that follow it.
                at(layout, top(window, 3) + height(window, 3) / 2.0),
                at(layout, top(window, 3) + height(window, 3) - 1.0),
                // Under the content area of the large span, in the line box of its row.
                at(layout, 44.0),
                // The content area of the span of the next row, just past the end of the
                // tall one, and the line box over it.
                at(layout, top(window, 5) + 0.5),
                at(layout, top(window, 5) - 0.5),
                at(layout, top(window, 7) + height(window, 7) / 2.0),
            ]
        });
        let text = layout.text();
        let row_of = |word: &str| {
            // The byte after the first of a row: the first is also the end of the row before.
            layout.row_for_index(text.find(word).unwrap() + 1).unwrap()
        };
        let rows = [row_of("Big"), row_of("bbbb"), row_of("cccc")];
        assert_height(f64::from(f32::from(rows[0].height)), 45.0);
        assert_height(f64::from(f32::from(rows[1].height)), 15.0);
        assert_height(f64::from(f32::from(rows[2].height)), 15.0);
        assert_near(
            f64::from(f32::from(rows[1].top - rows[0].top)),
            45.0,
            "the second row to start under the first",
        );
        assert_near(
            f64::from(f32::from(rows[2].top - rows[1].top)),
            15.0,
            "the third row to start under the second",
        );
        assert_eq!(hits, [vec![3], vec![3], vec![], vec![5], vec![], vec![7]]);
    }

    #[gpui::test]
    fn a_container_line_height_in_px_is_not_scaled_by_a_larger_span(cx: &mut gpui::TestAppContext) {
        let container = r#"{"width": 120, "fontSize": 10, "lineHeight": "24px"}"#;
        let sized = height_of(cx, container, "a ", r#"{"fontSize": 30}"#, "b");
        // The span's 24px line box is centred on its own, larger, font, so the
        // row is taller than 24px by the difference of the baselines, and far
        // from the 45px that a multiplier would give.
        assert!(
            sized >= 24.0 - TOLERANCE && sized < 40.0,
            "expected a row near 24px but measured {sized}px"
        );
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

    /// `1` is the window root with `a`, a 200 box holding `b`, and `c` after it, which overlaps
    /// `a` where `b` is also: the point (150, 150).
    struct Scene {
        window: gpui::WindowHandle<GpuixView>,
        tree: Arc<Mutex<RetainedTree>>,
        painted: Vec<String>,
    }

    impl Scene {
        fn new(cx: &mut gpui::TestAppContext, root: &str, a: &str, b: &str, c: &str) -> Scene {
            Scene::with(cx, root, a, b, c, |_| {})
        }

        /// `extra` adds more nodes under `b` (id 3).
        fn with(
            cx: &mut gpui::TestAppContext,
            root: &str,
            a: &str,
            b: &str,
            c: &str,
            extra: impl FnOnce(&mut RetainedTree),
        ) -> Scene {
            let tree = Arc::new(Mutex::new(RetainedTree::new()));
            {
                let mut tree = tree.lock().unwrap();
                let root: StyleDesc = serde_json::from_str(root).unwrap();
                tree.create_element(1, "div".to_owned());
                tree.set_style(1, Arc::new(root));
                tree.set_root(1);
                labelled(&mut tree, 2, 1, a, "a");
                labelled(&mut tree, 3, 2, b, "b");
                labelled(&mut tree, 4, 1, c, "c");
                extra(&mut tree);
            }
            let window = cx.add_window(|window, _| {
                crate::events::enable_dom_hit_testing(window);
                crate::layout::container::allow_redraw_passes(window);
                GpuixView::new(
                    tree.clone(),
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
            Scene { window, tree, painted: crate::text::painted_text(window.window_id()) }
        }

        /// The elements under (150, 150), topmost first, as the hit test of the painted frame
        /// finds them.
        fn hit_chain(&self, cx: &mut gpui::TestAppContext) -> Vec<u64> {
            gpui::VisualTestContext::from_window(self.window.into(), cx).simulate_mouse_move(
                gpui::point(gpui::px(150.0), gpui::px(150.0)),
                None,
                Default::default(),
            );
            cx.update_window(self.window.into(), |_, window, _| {
                crate::events::ids::host_hit_chain(window)
                    .into_iter()
                    .map(|hit| hit.id)
                    .collect()
            })
            .unwrap()
        }

        /// Every element under the same point, topmost first, from the retained tree. Its first
        /// is the target the frame hit tests.
        fn elements_at(&self) -> Vec<u64> {
            let id = self.window.window_id();
            let tree = self.tree.lock().unwrap();
            tree.elements_at(150.0, 150.0, &|element| crate::automation::get_bounds(id, element))
        }
    }

    const ROOT: &str = r#"{"width": 400, "height": 400}"#;
    const A: &str = r#"{"width": 200, "height": 200}"#;
    const FLEX_A: &str = r#"{"display": "flex", "width": 200, "height": 200}"#;
    const GRID_A: &str = r#"{"display": "grid", "width": 200, "height": 200}"#;
    const C: &str = r#"{"width": 200, "height": 200, "marginTop": -100, "marginLeft": 100}"#;

    #[gpui::test]
    fn a_z_indexed_descendant_paints_after_a_later_sibling(cx: &mut gpui::TestAppContext) {
        let b = r#"{"position": "absolute", "zIndex": 5, "left": 120, "top": 120, "width": 100, "height": 100}"#;
        assert_eq!(Scene::new(cx, ROOT, A, b, C).painted, ["a", "c", "b"]);
    }

    #[gpui::test]
    fn a_static_box_with_a_z_index_stays_in_tree_order(cx: &mut gpui::TestAppContext) {
        let b = r#"{"zIndex": 5, "width": 100, "height": 100}"#;
        assert_eq!(Scene::new(cx, ROOT, A, b, C).painted, ["a", "b", "c"]);
    }

    const ITEM: &str = r#"{"zIndex": 5, "width": 180, "height": 180}"#;

    #[gpui::test]
    fn a_flex_item_with_a_z_index_paints_and_hits_above_a_later_sibling_of_its_container(
        cx: &mut gpui::TestAppContext,
    ) {
        let scene = Scene::new(cx, ROOT, FLEX_A, ITEM, C);
        assert_eq!(scene.painted, ["a", "c", "b"]);
        assert_eq!(scene.hit_chain(cx), [3, 2, 1]);
        assert_eq!(scene.elements_at(), [3, 4, 2, 1]);
    }

    #[gpui::test]
    fn a_grid_item_with_a_z_index_paints_and_hits_above_a_later_sibling_of_its_container(
        cx: &mut gpui::TestAppContext,
    ) {
        let scene = Scene::new(cx, ROOT, GRID_A, ITEM, C);
        assert_eq!(scene.painted, ["a", "c", "b"]);
        assert_eq!(scene.hit_chain(cx), [3, 2, 1]);
        assert_eq!(scene.elements_at(), [3, 4, 2, 1]);
    }

    #[gpui::test]
    fn a_flex_item_with_z_index_auto_or_in_a_block_stays_in_tree_order(cx: &mut gpui::TestAppContext) {
        let auto = r#"{"width": 180, "height": 180}"#;
        let scene = Scene::new(cx, ROOT, FLEX_A, auto, C);
        assert_eq!(scene.painted, ["a", "b", "c"]);
        assert_eq!(scene.hit_chain(cx), [4, 1]);
        assert_eq!(scene.elements_at(), [4, 3, 2, 1]);

        let scene = Scene::new(cx, ROOT, A, ITEM, C);
        assert_eq!(scene.painted, ["a", "b", "c"]);
        assert_eq!(scene.hit_chain(cx), [4, 1]);
    }

    #[gpui::test]
    fn a_flex_item_with_a_negative_z_index_paints_below_the_in_flow_boxes(cx: &mut gpui::TestAppContext) {
        let below = r#"{"zIndex": -1, "width": 180, "height": 180}"#;
        let scene = Scene::new(cx, ROOT, FLEX_A, below, C);
        assert_eq!(scene.painted, ["b", "a", "c"]);
        assert_eq!(scene.hit_chain(cx), [4, 1]);
        assert_eq!(scene.elements_at(), [4, 2, 3, 1]);
    }

    #[gpui::test]
    fn a_flex_item_with_a_z_index_traps_what_is_inside_it(cx: &mut gpui::TestAppContext) {
        // x is positioned with the highest index, but belongs to b, which is a context.
        let scene = Scene::with(cx, ROOT, FLEX_A, ITEM, C, |tree| {
            labelled(
                tree,
                5,
                3,
                r#"{"position": "absolute", "zIndex": 100, "left": 0, "top": 0, "width": 20, "height": 20}"#,
                "x",
            );
        });
        assert_eq!(scene.painted, ["a", "c", "b", "x"]);
    }

    const FADED: &str = r#"{"opacity": 0.5, "width": 180, "height": 180}"#;

    #[gpui::test]
    fn a_translucent_static_box_paints_and_hits_in_the_positioned_layer(cx: &mut gpui::TestAppContext) {
        let scene = Scene::new(cx, ROOT, A, FADED, C);
        assert_eq!(scene.painted, ["a", "c", "b"]);
        assert_eq!(scene.hit_chain(cx), [3, 2, 1]);
        assert_eq!(scene.elements_at(), [3, 4, 2, 1]);
    }

    #[gpui::test]
    fn a_box_with_full_opacity_stays_in_tree_order(cx: &mut gpui::TestAppContext) {
        let opaque = r#"{"opacity": 1, "width": 180, "height": 180}"#;
        let scene = Scene::new(cx, ROOT, A, opaque, C);
        assert_eq!(scene.painted, ["a", "b", "c"]);
        assert_eq!(scene.hit_chain(cx), [4, 1]);
    }

    #[gpui::test]
    fn a_translucent_box_is_a_context_of_its_own(cx: &mut gpui::TestAppContext) {
        let scene = Scene::with(cx, ROOT, A, FADED, C, |tree| {
            labelled(
                tree,
                5,
                3,
                r#"{"position": "absolute", "zIndex": 100, "left": 0, "top": 0, "width": 20, "height": 20}"#,
                "x",
            );
        });
        // x comes right after b, not after c's positioned peers: it is inside b.
        assert_eq!(scene.painted, ["a", "c", "b", "x"]);
    }

    #[gpui::test]
    fn a_translucent_flex_item_takes_its_z_index(cx: &mut gpui::TestAppContext) {
        // z-index applies to the item, so it is not at 0 with the positioned boxes: c has 1.
        let b = r#"{"opacity": 0.5, "zIndex": -1, "width": 180, "height": 180}"#;
        let c = r#"{"position": "relative", "zIndex": 1, "width": 200, "height": 200, "marginTop": -100, "marginLeft": 100}"#;
        let scene = Scene::new(cx, ROOT, FLEX_A, b, c);
        assert_eq!(scene.painted, ["b", "a", "c"]);
    }

    #[gpui::test]
    fn the_layout_of_a_lifted_box_is_unchanged(cx: &mut gpui::TestAppContext) {
        let plain = Scene::new(cx, ROOT, FLEX_A, r#"{"width": 180, "height": 180}"#, C);
        let id = plain.window.window_id();
        let before = crate::automation::get_bounds(id, 3).unwrap();
        let lifted = Scene::new(cx, ROOT, FLEX_A, ITEM, C);
        let after = crate::automation::get_bounds(lifted.window.window_id(), 3).unwrap();
        assert_eq!((before.x, before.y, before.width, before.height), (after.x, after.y, after.width, after.height));
    }
}

/// A `position` that only a state refinement declares escapes the overflow clip of the ancestors
/// before its containing block, as a base `position` does, and an effect group around an escaped
/// box does not cut it to the clip it started in. Painted through a real view.
#[cfg(test)]
mod state_position_escape_tests {
    use super::*;

    const RED: u32 = 0xff0000;

    /// `nodes` are `(id, parent, style)`; the first is the root, a positioned 400 box.
    struct Page {
        window: gpui::WindowHandle<GpuixView>,
        tree: Arc<Mutex<RetainedTree>>,
    }

    impl Page {
        fn new(cx: &mut gpui::TestAppContext, nodes: &[(u64, Option<u64>, &str)]) -> Self {
            let tree = Arc::new(Mutex::new(RetainedTree::new()));
            let view_tree = tree.clone();
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
                tree.set_root(nodes[0].0);
            }
            let window = cx.add_window(|window, _| {
                crate::events::enable_dom_hit_testing(window);
                crate::layout::container::allow_redraw_passes(window);
                GpuixView::new(
                    view_tree,
                    None,
                    "test".to_owned(),
                    SharedSelection::default(),
                    crate::events::state::PointerRequests::default(),
                )
            });
            let page = Self { window, tree };
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

        fn move_mouse(&self, cx: &mut gpui::TestAppContext, x: f32, y: f32) {
            gpui::VisualTestContext::from_window(self.window.into(), cx).simulate_mouse_move(
                gpui::point(gpui::px(x), gpui::px(y)),
                None,
                Default::default(),
            );
            self.draw(cx);
        }

        /// How far right the red quad reaches and how far right its clip lets it, in px.
        fn red_extent(&self, cx: &mut gpui::TestAppContext) -> (f32, f32) {
            cx.update_window(self.window.into(), |_, window, _| {
                let scale = window.scale_factor();
                let red: gpui::Hsla = gpui::rgb(RED).into();
                let quad = window
                    .painted_quads()
                    .into_iter()
                    .find(|quad| quad.background.as_solid().is_some_and(|color| color == red))
                    .expect("the red box was painted");
                (quad.bounds.right().0 / scale, quad.content_mask.bounds.right().0 / scale)
            })
            .unwrap()
        }

        fn assert_unclipped(&self, cx: &mut gpui::TestAppContext) {
            let (right, clip) = self.red_extent(cx);
            assert_eq!(right, 400.0);
            assert!(clip >= right, "the box reaches {right} but is clipped at {clip}");
        }

        fn assert_clipped_at_the_card(&self, cx: &mut gpui::TestAppContext) {
            let (right, clip) = self.red_extent(cx);
            assert_eq!(clip, 200.0, "the card ends at 200 (box reaches {right})");
        }
    }

    const ROOT: &str = r#"{"position": "relative", "width": 400, "height": 400}"#;
    const CARD: &str = r#"{"marginLeft": 100, "width": 100, "height": 100, "overflow": "hidden"}"#;
    const CARD_GROUP: &str = r#"{"group": true, "marginLeft": 100, "width": 100, "height": 100, "overflow": "hidden"}"#;
    const BOX: &str = r##""width": 50, "height": 50, "backgroundColor": "#ff0000""##;

    fn card_with(cx: &mut gpui::TestAppContext, card: &str, child: &str) -> Page {
        Page::new(cx, &[(1, None, ROOT), (2, Some(1), card), (3, Some(2), child)])
    }

    #[gpui::test]
    fn a_hover_only_absolute_box_escapes_a_static_overflow_clip(cx: &mut gpui::TestAppContext) {
        let child = format!(
            r#"{{{BOX}, "hover": {{"position": "absolute", "left": 100, "top": 0, "width": 300}}}}"#
        );
        let page = card_with(cx, CARD, &child);
        page.assert_clipped_at_the_card(cx);
        page.move_mouse(cx, 102.0, 2.0);
        page.assert_unclipped(cx);
    }

    #[gpui::test]
    fn a_group_hover_only_absolute_box_escapes_a_static_overflow_clip(cx: &mut gpui::TestAppContext) {
        let child = format!(
            r#"{{{BOX}, "groupHover": {{"position": "absolute", "left": 100, "top": 0, "width": 300}}}}"#
        );
        let page = card_with(cx, CARD_GROUP, &child);
        page.move_mouse(cx, 190.0, 90.0);
        page.assert_unclipped(cx);
    }

    #[gpui::test]
    fn a_hover_only_fixed_box_escapes_a_positioned_overflow_clip(cx: &mut gpui::TestAppContext) {
        let card = r#"{"position": "relative", "marginLeft": 100, "width": 100, "height": 100, "overflow": "hidden"}"#;
        let child = format!(
            r#"{{{BOX}, "hover": {{"position": "fixed", "left": 100, "top": 0, "width": 300}}}}"#
        );
        let page = card_with(cx, card, &child);
        page.assert_clipped_at_the_card(cx);
        page.move_mouse(cx, 102.0, 2.0);
        page.assert_unclipped(cx);
    }

    #[gpui::test]
    fn a_hover_that_makes_the_box_static_puts_the_clip_back(cx: &mut gpui::TestAppContext) {
        let child = r##"{"width": 300, "height": 50, "backgroundColor": "#ff0000", "position": "absolute", "left": 100, "top": 0,
            "hover": {"position": "static"}}"##;
        let page = card_with(cx, CARD, child);
        page.assert_unclipped(cx);
        page.move_mouse(cx, 102.0, 2.0);
        page.assert_clipped_at_the_card(cx);
    }

    #[gpui::test]
    fn a_focus_within_only_absolute_box_escapes_a_static_overflow_clip(cx: &mut gpui::TestAppContext) {
        let child = format!(
            r#"{{{BOX}, "focusWithin": {{"position": "absolute", "left": 100, "top": 0, "width": 300}}}}"#
        );
        let page = card_with(cx, CARD, &child);
        page.tree.lock().unwrap().set_custom_prop(3, "tabIndex".to_owned(), serde_json::json!(0));
        page.draw(cx);
        page.assert_clipped_at_the_card(cx);
        assert_eq!(hit_outside_the_card(&page, cx), [1]);
        page.window.update(cx, |_, window, _| window.activate_window()).unwrap();
        cx.run_until_parked();
        page.window.update(cx, |view, window, cx| view.request_focus(3, window, cx)).unwrap();
        page.draw(cx);
        page.draw(cx);
        page.assert_unclipped(cx);
        assert_eq!(hit_outside_the_card(&page, cx), [3, 1]);
    }

    /// How far right each effect group composites, in px.
    fn group_clips(page: &Page, cx: &mut gpui::TestAppContext) -> Vec<f32> {
        cx.update_window(page.window.into(), |_, window, _| {
            let scale = window.scale_factor();
            window.painted_group_masks().iter().map(|mask| mask.bounds.right().0 / scale).collect()
        })
        .unwrap()
    }

    #[gpui::test]
    fn an_opacity_group_does_not_clip_an_escaped_box_to_the_clip_it_started_in(
        cx: &mut gpui::TestAppContext,
    ) {
        let clip = r#"{"marginLeft": 100, "width": 100, "height": 100, "overflow": "hidden"}"#;
        let card = r##"{"opacity": 0.5, "width": 100, "height": 100, "backgroundColor": "#0000ff"}"##;
        let child = r##"{"position": "absolute", "left": 100, "top": 0, "width": 300, "height": 50, "backgroundColor": "#ff0000"}"##;
        let page = Page::new(cx, &[(1, None, ROOT), (2, Some(1), clip), (3, Some(2), card), (4, Some(3), child)]);
        page.window.update(cx, |_, window, _| window.force_effect_groups()).unwrap();
        page.draw(cx);

        page.assert_unclipped(cx);
        let clips = group_clips(&page, cx);
        assert_eq!(clips.len(), 1);
        assert!(clips[0] >= 400.0, "the group composites up to {} but the box reaches 400", clips[0]);
    }

    #[gpui::test]
    fn an_opacity_group_keeps_the_clip_when_nothing_escapes(cx: &mut gpui::TestAppContext) {
        let clip = r#"{"marginLeft": 100, "width": 100, "height": 100, "overflow": "hidden"}"#;
        let card = r##"{"opacity": 0.5, "width": 100, "height": 100, "backgroundColor": "#0000ff"}"##;
        let child = r##"{"width": 300, "height": 50, "backgroundColor": "#ff0000"}"##;
        let page = Page::new(cx, &[(1, None, ROOT), (2, Some(1), clip), (3, Some(2), card), (4, Some(3), child)]);
        page.window.update(cx, |_, window, _| window.force_effect_groups()).unwrap();
        page.draw(cx);

        assert_eq!(group_clips(&page, cx), [200.0]);
    }

    /// The hit test of the retained tree, at a point outside the card.
    fn hit_outside_the_card(page: &Page, _cx: &mut gpui::TestAppContext) -> Vec<u64> {
        let window = page.window.window_id();
        page.tree
            .lock()
            .unwrap()
            .elements_at(350.0, 20.0, &|id| crate::automation::get_bounds(window, id))
    }

    #[gpui::test]
    fn the_retained_hit_test_follows_a_position_a_state_sets(cx: &mut gpui::TestAppContext) {
        let child = format!(
            r#"{{{BOX}, "groupHover": {{"position": "absolute", "left": 100, "top": 0, "width": 300}}}}"#
        );
        let page = card_with(cx, CARD_GROUP, &child);
        assert_eq!(hit_outside_the_card(&page, cx), [1]);
        page.move_mouse(cx, 190.0, 90.0);
        assert_eq!(hit_outside_the_card(&page, cx), [3, 1]);
        page.move_mouse(cx, 390.0, 390.0);
        assert_eq!(hit_outside_the_card(&page, cx), [1]);
    }

    #[gpui::test]
    fn the_retained_hit_test_clips_a_box_a_state_makes_static(cx: &mut gpui::TestAppContext) {
        let child = r##"{"width": 300, "height": 50, "backgroundColor": "#ff0000", "position": "absolute", "left": 100, "top": 0,
            "groupHover": {"position": "static"}}"##;
        let page = card_with(cx, CARD_GROUP, child);
        assert_eq!(hit_outside_the_card(&page, cx), [3, 1]);
        page.move_mouse(cx, 190.0, 90.0);
        assert_eq!(hit_outside_the_card(&page, cx), [1]);
    }
}

/// Border radii in the units of lane 3, painted.
#[cfg(test)]
mod paint_integration_tests {
    use super::*;

    /// A container with `style`, holding `a ` and then a `<span>` of `span_style` around `b`.
    pub(super) fn painted(
        cx: &mut gpui::TestAppContext,
        container_style: &str,
        span_style: &str,
    ) -> gpui::WindowHandle<GpuixView> {
        let tree = Arc::new(Mutex::new(RetainedTree::new()));
        {
            let mut tree = tree.lock().unwrap();
            let container: StyleDesc = serde_json::from_str(container_style).unwrap();
            tree.create_element(1, "div".to_owned());
            tree.set_style(1, Arc::new(container));
            tree.set_root(1);
            tree.create_element(2, "text".to_owned());
            tree.set_text(2, "a ".to_owned());
            tree.append_child(1, 2);
            tree.create_element(3, "span".to_owned());
            let span: StyleDesc = serde_json::from_str(span_style).unwrap();
            tree.set_style(3, Arc::new(span));
            tree.append_child(1, 3);
            tree.create_element(4, "text".to_owned());
            tree.set_text(4, "bbbbbbbb".to_owned());
            tree.append_child(3, 4);
        }
        shown(cx, tree)
    }

    pub(super) fn shown(
        cx: &mut gpui::TestAppContext,
        tree: Arc<Mutex<RetainedTree>>,
    ) -> gpui::WindowHandle<GpuixView> {
        let window = cx.add_window(|window, _| {
            crate::events::enable_dom_hit_testing(window);
            crate::layout::container::allow_redraw_passes(window);
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
        window
    }

    fn quads(window: gpui::WindowHandle<GpuixView>, cx: &mut gpui::TestAppContext) -> Vec<gpui::Quad> {
        cx.update_window(window.into(), |_, window, _| window.painted_quads()).unwrap()
    }

    /// The radii of the first quad of the container, in px.
    fn container_radii(cx: &mut gpui::TestAppContext, style: &str) -> ([f32; 4], [f32; 4]) {
        let window = painted(cx, style, "{}");
        let scale = cx.update_window(window.into(), |_, window, _| window.scale_factor()).unwrap();
        let quad = quads(window, cx)
            .into_iter()
            .find(|quad| quad.background.as_solid().is_some())
            .expect("the container paints a background");
        let corners = |radii: gpui::Corners<gpui::ScaledPixels>| {
            [radii.top_left, radii.top_right, radii.bottom_right, radii.bottom_left].map(|radius| radius.0 / scale)
        };
        (corners(quad.corner_radii), corners(quad.corner_radii_y))
    }

    const BOX: &str = r#""width": 400, "height": 400, "fontSize": 20, "backgroundColor": "red""#;

    #[gpui::test]
    fn a_border_radius_in_em_rem_and_viewport_units_resolves_against_the_element(cx: &mut gpui::TestAppContext) {
        let (x, y) = container_radii(cx, &format!(r#"{{{BOX}, "borderRadius": "1em 0.5rem 1vh 1vmax"}}"#));
        let viewport = cx
            .update_window(cx.windows()[0], |_, window, _| window.viewport_size())
            .unwrap();
        let (width, height) = (f32::from(viewport.width), f32::from(viewport.height));
        assert_eq!(x[0], 20.0);
        // The container is the root, so its rem is its own font size.
        assert_eq!(x[1], 10.0);
        assert!((x[2] - height * 0.01).abs() < 0.01, "{x:?}");
        assert!((x[3] - width.max(height) * 0.01).abs() < 0.01, "{x:?}");
        // A corner without a vertical radius is circular: the same on both axes, or zero for "same".
        assert!(y.iter().zip(x).all(|(y, x)| *y == 0.0 || *y == x), "{y:?} {x:?}");
    }

    #[gpui::test]
    fn a_calc_radius_mixing_a_length_and_a_percentage_keeps_the_percentage_per_axis(cx: &mut gpui::TestAppContext) {
        // 1em is 20px; the box is 400 wide and 200 high.
        let (x, y) = container_radii(
            cx,
            r#"{"width": 400, "height": 200, "fontSize": 20, "backgroundColor": "red", "borderRadius": "calc(1em + 10%)"}"#,
        );
        assert_eq!(x, [60.0; 4]);
        assert_eq!(y, [40.0; 4]);
    }

    #[gpui::test]
    fn an_elliptical_radius_with_font_relative_units_paints_the_ellipse(cx: &mut gpui::TestAppContext) {
        let (x, y) = container_radii(cx, &format!(r#"{{{BOX}, "borderRadius": "2em / 0.5em"}}"#));
        assert_eq!(x, [40.0; 4]);
        assert_eq!(y, [10.0; 4]);
    }

    #[gpui::test]
    fn a_larger_font_makes_the_same_em_radius_larger(cx: &mut gpui::TestAppContext) {
        let (small, _) = container_radii(cx, &format!(r#"{{{BOX}, "borderTopLeftRadius": "1em"}}"#));
        let (large, _) = container_radii(
            cx,
            r#"{"width": 400, "height": 400, "fontSize": 30, "backgroundColor": "red", "borderTopLeftRadius": "1em"}"#,
        );
        assert_eq!((small[0], large[0]), (20.0, 30.0));
    }
}

/// Lane 1's text decorations, painted by the elements that lane 2 and lane 8 changed: text in a
/// block, in an inline box and with a font size of its own.
#[cfg(test)]
mod decoration_paint_tests {
    use super::paint_integration_tests::{painted, shown};
    use super::*;

    const SOLID: u32 = 0;

    fn decorations(window: gpui::WindowHandle<GpuixView>, cx: &mut gpui::TestAppContext) -> Vec<gpui::Underline> {
        cx.update_window(window.into(), |_, window, _| window.painted_underlines()).unwrap()
    }

    /// The lines of `span` in `container`, moved down from the top of the window: an overline
    /// is drawn above the top of its text.
    fn lines(container: &str, span: &str, cx: &mut gpui::TestAppContext) -> Vec<gpui::Underline> {
        let container = below_the_top(container);
        let window = painted(cx, &container, span);
        decorations(window, cx)
    }

    fn below_the_top(container: &str) -> String {
        let mut style: serde_json::Value = serde_json::from_str(container).unwrap();
        style
            .as_object_mut()
            .unwrap()
            .entry("paddingTop")
            .or_insert(serde_json::json!(20));
        style.to_string()
    }

    #[gpui::test]
    fn an_overline_on_a_block_paints_above_the_text_where_an_underline_paints_below(cx: &mut gpui::TestAppContext) {
        let over = lines(r#"{"textDecoration": "overline"}"#, "{}", cx);
        let under = lines(r#"{"textDecoration": "underline"}"#, "{}", cx);
        assert_eq!((over.len(), under.len()), (1, 1));
        assert_eq!(over[0].style, SOLID);
        assert!(over[0].bounds.origin.y < under[0].bounds.origin.y);
        assert_eq!(over[0].bounds.size.width, under[0].bounds.size.width);
        assert!(over[0].bounds.size.width.0 > 0.0);
    }

    #[gpui::test]
    fn an_overline_on_an_inline_box_covers_the_text_of_that_box_only(cx: &mut gpui::TestAppContext) {
        let whole = lines(r#"{"textDecoration": "overline"}"#, "{}", cx);
        let span = lines("{}", r#"{"textDecoration": "overline"}"#, cx);
        assert_eq!((whole.len(), span.len()), (1, 1));
        assert!(span[0].bounds.size.width.0 > 0.0);
        assert!(span[0].bounds.size.width < whole[0].bounds.size.width);
        assert!(span[0].bounds.origin.x > whole[0].bounds.origin.x);
        assert_eq!(span[0].bounds.origin.y, whole[0].bounds.origin.y);
    }

    #[gpui::test]
    fn an_overline_follows_the_font_size_of_its_inline_box(cx: &mut gpui::TestAppContext) {
        let plain = lines("{}", r#"{"textDecoration": "overline"}"#, cx);
        let large = lines("{}", r#"{"textDecoration": "overline", "fontSize": 40}"#, cx);
        let underlined = lines("{}", r#"{"textDecoration": "underline", "fontSize": 40}"#, cx);
        assert_eq!((plain.len(), large.len(), underlined.len()), (1, 1, 1));
        // A larger font is wider, and its overline is above its underline.
        assert!(large[0].bounds.size.width > plain[0].bounds.size.width);
        assert!(large[0].bounds.origin.y < underlined[0].bounds.origin.y);
    }

    fn width(lines: &[gpui::Underline]) -> f32 {
        lines.iter().map(|line| line.bounds.size.width.0).sum()
    }

    #[gpui::test]
    fn an_inline_box_underline_inside_an_underlined_block_paints_both(cx: &mut gpui::TestAppContext) {
        let alone = lines(r#"{"textDecoration": "underline #ff0000"}"#, "{}", cx);
        let both = lines(
            r#"{"textDecoration": "underline #ff0000"}"#,
            r##"{"textDecoration": "underline dotted 3px #0000ff"}"##,
            cx,
        );
        let red: Vec<_> = both.iter().filter(|line| line.color == alone[0].color).collect();
        let blue: Vec<_> = both.iter().filter(|line| line.color != alone[0].color).collect();
        assert_eq!(blue.len(), 1, "the span's own underline");
        assert_ne!(blue[0].style, SOLID);
        let three_px = lines(r#"{"textDecoration": "underline 3px"}"#, "{}", cx);
        assert_eq!(blue[0].thickness, three_px[0].thickness);
        assert!(blue[0].bounds.size.width < alone[0].bounds.size.width);
        assert!(!red.is_empty(), "the block's, before the box and under it");
        assert!(red.iter().all(|line| line.style == SOLID));
        let red_width: f32 = red.iter().map(|line| line.bounds.size.width.0).sum();
        assert!((red_width - width(&alone)).abs() < 0.01, "{red_width} {}", width(&alone));
    }

    #[gpui::test]
    fn a_none_inside_an_underline_does_not_take_it_off(cx: &mut gpui::TestAppContext) {
        let alone = lines(r#"{"textDecoration": "underline"}"#, "{}", cx);
        let with_none = lines(r#"{"textDecoration": "underline"}"#, r#"{"textDecoration": "none"}"#, cx);
        assert_eq!(width(&with_none), width(&alone));
    }

    #[gpui::test]
    fn a_nested_block_adds_its_underline_to_the_block_around_it(cx: &mut gpui::TestAppContext) {
        let tree = Arc::new(Mutex::new(RetainedTree::new()));
        {
            let mut tree = tree.lock().unwrap();
            tree.create_element(1, "div".to_owned());
            tree.set_style(1, Arc::new(serde_json::from_str(r##"{"textDecoration": "underline #ff0000"}"##).unwrap()));
            tree.set_root(1);
            tree.create_element(2, "div".to_owned());
            tree.set_style(2, Arc::new(serde_json::from_str(r##"{"textDecoration": "underline wavy 2px #0000ff"}"##).unwrap()));
            tree.append_child(1, 2);
            tree.create_element(3, "text".to_owned());
            tree.set_text(3, "nested".to_owned());
            tree.append_child(2, 3);
        }
        let window = shown(cx, tree);
        let lines = decorations(window, cx);
        assert_eq!(lines.len(), 2);
        assert_ne!(lines[0].color, lines[1].color);
        assert_ne!(lines[0].style, lines[1].style);
        assert_ne!(lines[0].thickness, lines[1].thickness);
        assert_eq!(lines[0].bounds.size.width, lines[1].bounds.size.width);
    }

    /// A line of `auto` thickness is a tenth of the font size of its box, floored to a
    /// device pixel, whichever line it is.
    #[gpui::test]
    fn an_auto_thickness_follows_the_font_size_for_every_line(cx: &mut gpui::TestAppContext) {
        for font_size in [16.0_f32, 32.0, 40.0] {
            let container = format!(
                r#"{{"fontSize": {font_size}, "textDecoration": "underline overline line-through"}}"#
            );
            let window = painted(cx, &below_the_top(&container), "{}");
            let scale = cx.update_window(window.into(), |_, window, _| window.scale_factor()).unwrap();
            let lines = decorations(window, cx);
            assert_eq!(lines.len(), 3, "{font_size}");
            let expected = (font_size / 10.0 * scale).floor().max(1.0);
            for line in lines {
                assert_eq!(line.thickness.0, expected, "{font_size}");
            }
        }
    }

    #[gpui::test]
    fn the_style_and_thickness_of_an_overline_reach_the_scene(cx: &mut gpui::TestAppContext) {
        let solid = lines("{}", r#"{"textDecoration": "overline"}"#, cx);
        let dotted = lines("{}", r#"{"textDecoration": "overline dotted 3px"}"#, cx);
        // The outer band of a double overline is above the top of the line, which in a line as
        // tall as its font is the top of the window unless the text is moved down.
        let double = lines(r#"{"paddingTop": 10}"#, r#"{"textDecoration": "overline double"}"#, cx);
        assert_eq!(dotted.len(), 1);
        assert_ne!(dotted[0].style, solid[0].style);
        let three_px = lines("{}", r#"{"textDecoration": "overline 3px"}"#, cx);
        assert_eq!(dotted[0].thickness, three_px[0].thickness);
        assert_eq!(double.len(), 2, "a double line is two");
    }
}

#[cfg(test)]
mod web_animation_event_tests {
    use super::*;
    use crate::motion::web_animation::{ApiEvent, ApiEventKind, Info, Kind, Retired, Status};

    fn event(run: u64, kind: ApiEventKind, status: Status) -> ApiEvent {
        ApiEvent {
            element: 5,
            run,
            kind,
            info: Info {
                id: run,
                kind: Kind::Transition,
                name: "width".to_string(),
                current_time: 1000.0,
                end_time: 1000.0,
                playback_rate: 1.0,
                status,
            },
        }
    }

    fn heard() -> (Option<EventCallback>, Arc<Mutex<Vec<EventPayload>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let callback: EventCallback = Arc::new(move |payload| sink.lock().unwrap().push(payload));
        (Some(callback), seen)
    }

    #[test]
    fn an_event_retires_its_run_until_it_is_released() {
        let (callback, seen) = heard();
        let mut retired = Retired::default();
        emit_web_animation_events(
            &callback,
            &mut retired,
            vec![event(7, ApiEventKind::Finish, Status::Finished), event(8, ApiEventKind::Cancel, Status::Idle)],
        );
        let seen = seen.lock().unwrap();
        assert_eq!(
            seen.iter().map(|payload| (payload.event_type.as_str(), payload.animation_id)).collect::<Vec<_>>(),
            [("webanimationfinish", Some(7.0)), ("webanimationcancel", Some(8.0))]
        );
        assert_eq!(retired.get(7).unwrap().status, Status::Finished);
        assert_eq!(retired.get(8).unwrap().status, Status::Idle);
        retired.release(7);
        assert!(retired.get(7).is_none());
        assert!(retired.get(8).is_some());
    }

    #[test]
    fn nothing_is_retired_when_no_one_hears_the_events() {
        let mut retired = Retired::default();
        emit_web_animation_events(&None, &mut retired, vec![event(7, ApiEventKind::Finish, Status::Finished)]);
        assert_eq!(retired.len(), 0, "no release would ever come for it");
    }
}
