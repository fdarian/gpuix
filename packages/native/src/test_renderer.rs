/// TestGpuixRenderer — GPU-backed GPUI test renderer exposed to Node.js via napi.
///
/// Uses gpui::VisualTestAppContext with the native Metal or DirectX renderer
/// and TestDispatcher for deterministic scheduling. Runs the SAME GpuixView,
/// build_element(), apply_styles(), and event handlers as production.
///
/// Windows are positioned offscreen at (-10000, -10000) — invisible but
/// fully rendered by the native GPU. This enables capture_screenshot() for visual
/// test validation.
///
/// VisualTestAppContext is !Send, so it is stored in thread-local state.
/// All napi calls happen on the JS main thread.
use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use napi::bindgen_prelude::*;
use napi_derive::napi;

use gpui::AppContext as _;

use crate::element_tree::EventPayload;
use crate::renderer::{
    apply_batch_to_tree, debug_frame_overlay_mode_name, debug_frame_overlay_stats_js,
    parse_debug_frame_overlay_mode, to_element_id, DebugFrameOverlayStats, EventCallback,
    GpuixView, PathPromptOptions, PromptForPathsTask,
};
use crate::retained_tree::RetainedTree;

// ── Thread-local storage for !Send GPUI types ────────────────────────

/// Bundles VisualTestAppContext + window handle + view entity.
/// Stored in thread_local because VisualTestAppContext is !Send (Rc<AppCell>).
/// Field order is load-bearing: Rust drops fields in declaration order, and
/// gpui panics at app teardown if an `Entity` handle outlives its `App`.
/// `view` must therefore be declared before `cx`.
struct VisualTestState {
    view: gpui::Entity<GpuixView>,
    window: gpui::AnyWindowHandle,
    cx: gpui::VisualTestAppContext,
}

/// Release every `Entity` handle the view is holding, while the `App` is alive.
///
/// The test build enables gpui's leak detector, which panics if a handle
/// outlives its `App`. `<input>` keeps an `Entity<TextEditorState>` in the
/// view's custom element registry, so that panic fires from a thread-local
/// destructor at process exit. macOS never runs this destructor, so the panic
/// only appeared once Windows started running the suite: every test file
/// passed and then the vitest worker died with "Worker exited unexpectedly".
///
/// `drop` runs before the fields are dropped, so `view` and `cx` are both
/// still usable here.
impl Drop for VisualTestState {
    fn drop(&mut self) {
        let view = self.view.clone();
        // Unmount, exactly as React would: empty the tree, then paint one more
        // frame. The registry is not the only owner of the entity. `<input>`
        // installs an `ElementInputHandler` during paint, and a clone of that
        // lives in the window's rendered frame and in the platform window. A
        // frame with nothing in it is what drops those, and it has to happen
        // while the `App` is still alive.
        self.cx.update(|cx| {
            view.update(cx, |view, cx| {
                if let Ok(mut tree) = view.tree.lock() {
                    tree.root_id = None;
                }
                view.custom_registry.destroy_all();
                view.focus_handles.clear();
                cx.notify();
            });
        });
        // Err only means the window is already gone, which is the state this
        // is trying to reach.
        self.cx
            .update_window(self.window, |_, window, _| window.refresh())
            .ok();
        self.cx.run_until_parked();
    }
}

thread_local! {
    static TEST_STATE: RefCell<Option<VisualTestState>> = const { RefCell::new(None) };
}

/// Access VisualTestAppContext + window + view mutably within thread_local.
/// The closure receives (&mut cx, window_handle, &view_entity).
/// Returns Err if no TestGpuixRenderer has been created on this thread.
///
/// Also marks the call as running inside a call from JS, so input dispatched by
/// `f` can run JS handlers synchronously. A handler that calls a renderer
/// method that needs the app context gets an error here: the app is already
/// borrowed by the input being dispatched. The methods a handler may call go
/// through `update_view` instead.
fn with_test_state<R>(
    f: impl FnOnce(
        &mut gpui::VisualTestAppContext,
        gpui::AnyWindowHandle,
        &gpui::Entity<GpuixView>,
    ) -> Result<R>,
) -> Result<R> {
    crate::events::sync_dispatch::native_call(|| {
        TEST_STATE.with(|cell| {
            let mut borrow = cell.try_borrow_mut().map_err(|_| {
                Error::from_reason(
                    "This renderer call cannot run inside an event handler that native called synchronously (keyDown, keyUp). Make it from a microtask instead.",
                )
            })?;
            let state = borrow
                .as_mut()
                .ok_or_else(|| Error::from_reason("TestGpuixRenderer not initialized"))?;
            f(&mut state.cx, state.window, &state.view)
        })
    })
}

/// Runs `update` on the view and the window, from anywhere: from inside a sync
/// event handler it borrows nothing new and uses the window being dispatched
/// to. The test context is not run until parked, since the dispatch that is
/// still on the stack does that.
fn update_view<R>(
    update: impl FnOnce(&mut GpuixView, &mut gpui::Window, &mut gpui::Context<GpuixView>) -> R,
) -> Result<R> {
    crate::events::sync_dispatch::native_call(|| {
        crate::events::sync_dispatch::route(
            update,
            crate::events::sync_dispatch::update_root_view,
            Error::from_reason,
            |update| {
                with_test_state(|cx, window, view| {
                    let view = view.clone();
                    let result = cx
                        .update_window(window, |_, window, app| {
                            view.update(app, |view, cx| update(view, window, cx))
                        })
                        .map_err(|error| Error::from_reason(error.to_string()))?;
                    cx.run_until_parked();
                    Ok(result)
                })
            },
        )
    })
}

/// Runs `update` on the window alone, which a leased root view does not block.
fn window_call<R>(update: impl FnOnce(&mut gpui::Window, &mut gpui::App) -> R) -> Result<R> {
    crate::events::sync_dispatch::native_call(|| {
        crate::events::sync_dispatch::route(
            update,
            crate::events::sync_dispatch::update_active_window,
            Error::from_reason,
            |update| {
                with_test_state(|cx, window, _view| {
                    let result = cx
                        .update_window(window, |_, window, app| update(window, app))
                        .map_err(|error| Error::from_reason(error.to_string()))?;
                    cx.run_until_parked();
                    Ok(result)
                })
            },
        )
    })
}

/// `update_view` for a change that returns nothing. When it is called from a
/// handler of an input that holds the root view, it runs after that input.
fn update_view_or_defer(
    update: impl FnOnce(&mut GpuixView, &mut gpui::Window, &mut gpui::Context<GpuixView>) + 'static,
) -> Result<()> {
    crate::events::sync_dispatch::route(
        update,
        crate::events::sync_dispatch::update_root_view_or_defer,
        Error::from_reason,
        update_view,
    )
}

/// Default offscreen window size. Matches gpui's `open_offscreen_window_default`,
/// so a `new TestGpuixRenderer()` with no size behaves exactly as before.
///
/// Note for layout tests: 1280 is wide enough that a centered max-width content
/// column stays capped whether a sidebar is open or closed. A test that needs to
/// observe re-wrapping must pass a narrower width explicitly.
const DEFAULT_WINDOW_WIDTH: f64 = 1280.0;
const DEFAULT_WINDOW_HEIGHT: f64 = 800.0;

/// Validate a caller-supplied window dimension, falling back to `default`.
///
/// Checks the value *after* the `f32` cast: a finite `f64` such as `1e300`
/// saturates to `f32::INFINITY`, which would open a window with no usable size.
fn window_dimension(value: Option<f64>, default: f64, label: &str) -> Result<f32> {
    let Some(value) = value else {
        return Ok(default as f32);
    };
    let pixels = value as f32;
    if !pixels.is_finite() || pixels <= 0.0 {
        return Err(Error::from_reason(format!(
            "TestGpuixRenderer {label} must be a positive, finite number, got {value}"
        )));
    }
    Ok(pixels)
}

/// Convert JS button number (0=left, 1=middle, 2=right) to GPUI MouseButton.
fn u32_to_mouse_button(button: u32) -> gpui::MouseButton {
    match button {
        1 => gpui::MouseButton::Middle,
        2 => gpui::MouseButton::Right,
        _ => gpui::MouseButton::Left,
    }
}

/// Dispatch input the way AppKit does: `WindowHandle<GpuixView>::update` leases
/// the root view for the whole event. `AnyWindowHandle::update` does not, so a
/// nested `GpuixView` update would pass here and abort a live window.
fn simulate_event_with_view_lease(
    cx: &mut gpui::VisualTestAppContext,
    window: gpui::AnyWindowHandle,
    event: impl gpui::InputEvent,
) {
    cx.update_window(window, |root_view, window, cx| {
        let view = root_view
            .downcast::<GpuixView>()
            .expect("test window root is GpuixView");
        view.update(cx, |_view, cx| {
            window.dispatch_event(event.to_platform_input(), cx);
        });
    })
    .ok();
    cx.run_until_parked();
}

fn file_drag_position(x: f64, y: f64) -> gpui::Point<gpui::Pixels> {
    gpui::point(gpui::px(x as f32), gpui::px(y as f32))
}

/// Same SVG the JS img tests write to disk. VisualTestAppContext starts with
/// a 404 FakeHttpClient, and a real ReqwestClient would leave the GPUI
/// test dispatcher parked while Tokio fetches. This answers every request
/// immediately so `<img src="https://…">` exercises GPUI's URI loader.
const TEST_IMAGE_SVG: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="140" viewBox="0 0 240 140">"##,
    r##"<rect x="0" y="0" width="240" height="140" fill="#1e2d59"/>"##,
    r##"<rect x="16" y="16" width="208" height="108" rx="14" fill="#5ca9ff"/>"##,
    r##"<circle cx="68" cy="70" r="24" fill="#ffd166"/>"##,
    r##"<rect x="112" y="50" width="88" height="14" rx="7" fill="#20304f"/>"##,
    r##"<rect x="112" y="74" width="70" height="12" rx="6" fill="#2a3c61"/>"##,
    "</svg>",
);

fn install_test_image_http_client(cx: &mut gpui::App) {
    let body = TEST_IMAGE_SVG.as_bytes().to_vec();
    let client = gpui::http_client::FakeHttpClient::create(move |_req| {
        let body = body.clone();
        async move {
            Ok(gpui::http_client::Response::builder()
                .status(200)
                .header("content-type", "image/svg+xml")
                .body(body.into())
                .unwrap())
        }
    });
    cx.set_http_client(client);
}

// ── TestGpuixRenderer ────────────────────────────────────────────────

/// GPU-backed GPUI test renderer. Uses VisualTestAppContext with the native
/// Metal or DirectX renderer and TestDispatcher for deterministic scheduling.
/// Same GpuixView and rendering pipeline as production.
///
/// Usage from JS:
///   const r = new TestGpuixRenderer()
///   r.applyBatch('[["createElement",1,"div"],["setRoot",1]]')
///   r.flush()                  // triggers GpuixView::render() on the GPU
///   r.simulateClick(50, 50)    // dispatches through GPUI hit testing
///   const events = r.drainEvents()
///   r.captureScreenshot("/tmp/test.png")  // saves rendered UI as PNG
#[napi]
pub struct TestGpuixRenderer {
    sync_handler: crate::events::sync_dispatch::SyncHandlerOwner,
    tree: Arc<Mutex<RetainedTree>>,
    events: Arc<Mutex<Vec<EventPayload>>>,
    /// Same handle GpuixView paints against, so tests can assert on the live
    /// selection after simulating a drag.
    selection: crate::text::SharedSelection,
    pointer_requests: crate::events::state::PointerRequests,
    /// The platform's side of pointer lock, which the offscreen window cannot
    /// provide: it is never key, so a real request is always refused.
    platform: crate::events::pointer_lock::SimulatedPlatform,
    /// What `getMediaFeatures` reports. The test window never receives OS
    /// notifications, so `setMediaFeatures` is the only way it changes.
    media: Arc<Mutex<crate::media::TestMedia>>,
}

impl TestGpuixRenderer {
    fn apply_text_selection(
        &self,
        id: u64,
        request: crate::custom_elements::text_selection::SelectionRequest,
    ) -> Result<()> {
        update_view(|view, window, cx| view.set_text_selection(id, request, window, cx))?
            .map_err(Error::from_reason)
    }
}

#[napi]
impl TestGpuixRenderer {
    #[napi(constructor)]
    pub fn new(width: Option<f64>, height: Option<f64>) -> Result<Self> {
        // Quiet unless RUST_LOG asks, so a suite can be run for its warnings.
        let _ = env_logger::try_init();
        let window_size = gpui::size(
            gpui::px(window_dimension(width, DEFAULT_WINDOW_WIDTH, "width")?),
            gpui::px(window_dimension(height, DEFAULT_WINDOW_HEIGHT, "height")?),
        );
        let tree = Arc::new(Mutex::new(RetainedTree::new()));
        let events: Arc<Mutex<Vec<EventPayload>>> = Arc::new(Mutex::new(Vec::new()));

        // Event callback: push to Vec instead of ThreadsafeFunction.
        let events_clone = events.clone();
        let event_callback: Option<EventCallback> = Some(Arc::new(move |payload: EventPayload| {
            events_clone.lock().unwrap().push(payload);
        }));

        let tree_clone = tree.clone();
        let callback_clone = event_callback.clone();
        let selection = crate::text::SharedSelection::default();
        let selection_clone = selection.clone();
        let pointer_requests = crate::events::state::PointerRequests::default();
        let pointer_requests_clone = pointer_requests.clone();
        let simulated_platform = crate::events::pointer_lock::SimulatedPlatform::default();
        let simulated_platform_clone = simulated_platform.clone();

        let platform = gpui_platform::current_platform(false);
        let mut cx = gpui::VisualTestAppContext::new(platform);
        cx.update(|cx| {
            crate::custom_elements::input::init(cx);
            install_test_image_http_client(cx);
        });

        // Open an offscreen window at (-10000, -10000) with the same GpuixView
        // and native GPU renderer as production.
        let window_handle = cx
            .open_offscreen_window(window_size, |window, app| {
                app.new(|cx| {
                    let mut view = GpuixView::new(
                        tree_clone,
                        callback_clone,
                        "GPUIX Test".to_string(),
                        selection_clone,
                        pointer_requests_clone,
                    );
                    view.observe_appearance(window, cx);
                    view.simulate_platform_pointer_lock(simulated_platform_clone);
                    crate::events::enable_dom_hit_testing(window);
                    view.install_key_interception(cx);
                    view
                })
            })
            .map_err(|e| Error::from_reason(format!("Failed to open test window: {}", e)))?;

        // Get the root entity (Entity<GpuixView>) from the window.
        let view = window_handle
            .entity(&cx)
            .map_err(|e| Error::from_reason(format!("Failed to get root view: {}", e)))?;

        // Convert typed WindowHandle<GpuixView> to AnyWindowHandle for simulation methods.
        let window: gpui::AnyWindowHandle = window_handle.into();

        // Visual tests have no VoiceOver / UIA client, so AccessKit never
        // activates. Pretend one is connected so every flush builds a tree
        // that get_a11y_tree can dump.
        //
        // The offscreen window is also never key. GPUI reports focus paths
        // only for an active window, so without this onFocus / onBlur never
        // fire in tests. Real activation would take the user's keyboard.
        cx.update_window(window, |_, window, cx| {
            window.set_a11y_active_for_tests(true);
            window.set_active_for_tests(true, cx);
        })
        .map_err(|e| Error::from_reason(e.to_string()))?;
        cx.run_until_parked();

        // Store !Send types on the JS main thread.
        TEST_STATE.with(|cell| {
            *cell.borrow_mut() = Some(VisualTestState { cx, window, view });
        });

        let sync_handler = crate::events::sync_dispatch::SyncHandlerOwner::new();
        sync_handler.bind_window(window.window_id());

        Ok(Self {
            sync_handler,
            tree,
            events,
            selection,
            pointer_requests,
            platform: simulated_platform,
            media: Arc::new(Mutex::new(crate::media::TestMedia::default())),
        })
    }

    /// Registers the function native calls synchronously for `keyDown` and
    /// `keyUp`; see `GpuixRenderer::setSyncEventHandler`. Without one, those
    /// events are queued for `drainEvents` like every other.
    #[napi]
    pub fn set_sync_event_handler(
        &self,
        env: &Env,
        handler: Option<Function<EventPayload, crate::events::sync_dispatch::SyncDispatchResult>>,
    ) -> Result<()> {
        crate::events::sync_dispatch::register(&self.sync_handler, env, handler)
    }

    /// How many elements the retained tree holds, reachable from the root or
    /// not. `getTreeJson` walks from the root, so it cannot see a node that was
    /// detached and never destroyed. This is the only way a test can prove a
    /// removal actually freed it.
    #[napi]
    pub fn get_retained_element_count(&self) -> u32 {
        self.tree.lock().unwrap().elements.len() as u32
    }

    /// Apply a batch of mutations in a single FFI call.
    /// Same format as GpuixRenderer::apply_batch (string op names).
    /// Returns accumulated destroyed IDs from all destroyElement ops.
    #[napi]
    pub fn apply_batch(&self, json: String) -> Result<Vec<f64>> {
        let mut tree = self.tree.lock().unwrap();
        apply_batch_to_tree(&mut tree, json.as_bytes()).map_err(Error::from_reason)
    }

    // ── Test-specific methods ────────────────────────────────────────

    /// Notify the view entity and run GPUI until parked.
    /// This triggers GpuixView::render() → build_element() → GPUI layout.
    /// Must be called after mutations and before simulating events (GPUI's
    /// hit testing requires elements to be laid out).
    #[napi]
    pub fn flush(&self) -> Result<()> {
        // From a sync event handler this only marks the view dirty: the frame
        // is drawn once the input that is being dispatched has returned.
        update_view_or_defer(|_view, _window, cx| cx.notify())
    }

    /// Simulate a click at the given window coordinates.
    /// Dispatches MouseDown + MouseUp through GPUI's input pipeline,
    /// which triggers the same event handlers as production.
    /// IMPORTANT: Call flush() before this — hit testing requires laid-out elements.
    /// `modifiers` uses the `press()` syntax: "cmd", "cmd-shift", "alt".
    #[napi]
    pub fn simulate_click(
        &self,
        x: f64,
        y: f64,
        button: Option<u32>,
        modifiers: Option<String>,
    ) -> Result<()> {
        let modifiers = crate::automation::parse_modifiers(modifiers.as_deref());
        let button = button.unwrap_or(0);
        with_test_state(|cx, window, _view| {
            // Not `cx.simulate_click`: that helper hard-codes the left button,
            // so a right click silently became a left click. Lease GpuixView
            // the same way AppKit does, or this path cannot catch a nested
            // root-view update.
            let position = gpui::point(gpui::px(x as f32), gpui::px(y as f32));
            let gpui_button = u32_to_mouse_button(button);
            simulate_event_with_view_lease(
                cx,
                window,
                gpui::MouseDownEvent {
                    position,
                    modifiers,
                    button: gpui_button,
                    click_count: 1,
                    first_mouse: false,
                },
            );
            simulate_event_with_view_lease(
                cx,
                window,
                gpui::MouseUpEvent {
                    position,
                    modifiers,
                    button: gpui_button,
                    click_count: 1,
                },
            );
            Ok(())
        })
    }

    /// Simulate key strokes through GPUI's input pipeline.
    /// Format: space-separated keys, e.g. "a", "enter", "cmd-shift-p".
    /// The focused element receives keyDown/keyUp events.
    #[napi]
    pub fn simulate_keystrokes(&self, keystrokes: String) -> Result<()> {
        with_test_state(|cx, window, _view| {
            cx.simulate_keystrokes(window, &keystrokes);
            Ok(())
        })
    }

    /// Simulate a single key down event through GPUI's input pipeline.
    /// Format: modifier-key string, e.g. "a", "enter", "cmd-s".
    /// Unlike simulate_keystrokes, this dispatches ONLY a KeyDownEvent —
    /// no automatic KeyUpEvent follows. Use with simulate_key_up for
    /// fine-grained key event testing.
    #[napi]
    pub fn simulate_key_down(&self, keystroke: String, is_held: Option<bool>) -> Result<()> {
        with_test_state(|cx, window, _view| {
            let parsed = gpui::Keystroke::parse(&keystroke).map_err(|e| {
                Error::from_reason(format!("Invalid keystroke '{}': {}", keystroke, e))
            })?;

            cx.simulate_event(
                window,
                gpui::KeyDownEvent {
                    keystroke: parsed,
                    is_held: is_held.unwrap_or(false),
                    prefer_character_input: false,
                },
            );

            Ok(())
        })
    }

    /// Simulate a single key up event through GPUI's input pipeline.
    /// Format: modifier-key string, e.g. "a", "enter", "cmd-s".
    /// Pairs with simulate_key_down for fine-grained key event testing.
    #[napi]
    pub fn simulate_key_up(&self, keystroke: String) -> Result<()> {
        with_test_state(|cx, window, _view| {
            let parsed = gpui::Keystroke::parse(&keystroke).map_err(|e| {
                Error::from_reason(format!("Invalid keystroke '{}': {}", keystroke, e))
            })?;

            cx.simulate_event(window, gpui::KeyUpEvent { keystroke: parsed });

            Ok(())
        })
    }

    /// Simulate a mouse move to the given coordinates.
    /// pressed_button: optional mouse button held during move (0=left, 1=middle, 2=right).
    /// Used to simulate drag events.
    /// movement_x, movement_y: the motion the platform reports for this move, which
    /// becomes `movementX`/`movementY`. Zero when not given.
    #[napi]
    pub fn simulate_mouse_move(
        &self,
        x: f64,
        y: f64,
        pressed_button: Option<u32>,
        modifiers: Option<String>,
        movement_x: Option<f64>,
        movement_y: Option<f64>,
    ) -> Result<()> {
        let modifiers = crate::automation::parse_modifiers(modifiers.as_deref());
        self.platform
            .set_movement((movement_x.unwrap_or(0.0), movement_y.unwrap_or(0.0)));
        with_test_state(|cx, window, _view| {
            let button: Option<gpui::MouseButton> = pressed_button.map(u32_to_mouse_button);

            simulate_event_with_view_lease(
                cx,
                window,
                gpui::MouseMoveEvent {
                    position: gpui::point(gpui::px(x as f32), gpui::px(y as f32)),
                    modifiers,
                    pressed_button: button,
                },
            );

            Ok(())
        })
    }

    /// Simulate the pointer leaving the window, as the platform reports it when
    /// the cursor crosses the window edge: there is no final mouse move.
    #[napi]
    pub fn simulate_mouse_exit(&self, x: f64, y: f64) -> Result<()> {
        with_test_state(|cx, window, _view| {
            simulate_event_with_view_lease(
                cx,
                window,
                gpui::MouseExitEvent {
                    position: gpui::point(gpui::px(x as f32), gpui::px(y as f32)),
                    pressed_button: None,
                    modifiers: Default::default(),
                },
            );
            Ok(())
        })
    }

    /// Focus an element by its numeric ID.
    /// The element must be DOM-focusable (an enabled input or textarea, or an
    /// element with a `tabIndex`), which is what gives it a FocusHandle.
    /// Call flush() before this so the element tree and focus handles exist.
    #[napi]
    pub fn focus_element(&self, id: f64) -> Result<()> {
        let id = to_element_id(id)?;
        // Same path as the live renderer: an element whose focus handle does
        // not exist yet (Solid applies its mutations in a microtask) takes
        // focus when its handle is created.
        update_view_or_defer(move |view, window, cx| view.request_focus(id, window, cx))
    }

    #[napi]
    pub fn blur_element(&self, id: f64) -> Result<()> {
        let id = to_element_id(id)?;
        update_view_or_defer(move |view, window, cx| view.blur_element(id, window, cx))
    }

    #[napi]
    pub fn matches_focus(&self, id: f64, selector: String) -> Result<bool> {
        let id = to_element_id(id)?;
        update_view(|view, window, _cx| view.matches_focus(id, &selector, window))?
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn focus_next(&self) -> Result<()> {
        window_call(|window, cx| window.focus_next(cx))
    }

    #[napi]
    pub fn focus_previous(&self) -> Result<()> {
        window_call(|window, cx| window.focus_prev(cx))
    }

    #[napi]
    pub fn prompt_for_paths(
        &self,
        options: Option<PathPromptOptions>,
    ) -> AsyncTask<PromptForPathsTask> {
        let receiver = PathPromptOptions::to_gpui(options)
            .and_then(|options| window_call(|_window, cx| cx.prompt_for_paths(options)));
        AsyncTask::new(PromptForPathsTask::new(receiver))
    }

    /// The test window's own in-memory clipboard, never the system's.
    #[napi]
    pub fn write_clipboard_text(&self, text: String) -> Result<()> {
        window_call(|_window, cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string(text)))
    }

    #[napi]
    pub fn read_clipboard_text(&self) -> Result<Option<String>> {
        window_call(|_window, cx| cx.read_from_clipboard().and_then(|item| item.text()))
    }

    #[napi]
    pub fn get_focused_element_id(&self) -> Result<Option<f64>> {
        update_view(|view, window, _cx| view.focused_element_id(window).map(|id| id as f64))
    }

    /// Makes the offscreen window key or not, as when the user switches apps.
    #[napi]
    pub fn set_window_active(&self, active: bool) -> Result<()> {
        window_call(move |window, cx| window.set_active_for_tests(active, cx))
    }

    #[napi]
    pub fn focus_next_within(&self, element_id: f64) -> Result<()> {
        let id = to_element_id(element_id)?;
        update_view_or_defer(move |view, window, cx| {
            view.focus_next_within(id, window, cx);
            cx.notify();
        })
    }

    #[napi]
    pub fn focus_previous_within(&self, element_id: f64) -> Result<()> {
        let id = to_element_id(element_id)?;
        update_view_or_defer(move |view, window, cx| {
            view.focus_previous_within(id, window, cx);
            cx.notify();
        })
    }

    #[napi]
    pub fn set_window_key_events(&self, key_down: bool, key_up: bool, event_id: f64) -> Result<()> {
        let event_id = to_element_id(event_id)?;
        update_view_or_defer(move |view, window, cx| {
            view.routing.set_window_keys(key_down, key_up, event_id);
            cx.notify();
            window.refresh();
        })
    }

    #[napi]
    pub fn set_window_selection_change(&self, enabled: bool, event_id: f64) -> Result<()> {
        let event_id = to_element_id(event_id)?;
        update_view_or_defer(move |view, window, cx| {
            view.set_selection_change_listener(enabled, event_id);
            cx.notify();
            window.refresh();
        })
    }

    #[napi]
    pub fn set_window_appearance_change(&self, enabled: bool, event_id: f64) -> Result<()> {
        let event_id = to_element_id(event_id)?;
        update_view_or_defer(move |view, _window, _cx| {
            view.set_appearance_change_listener(enabled, event_id);
        })
    }

    #[napi]
    pub fn set_window_media_change(&self, enabled: bool) {
        self.media.lock().unwrap().listening = enabled;
    }

    /// The preferences this window reports, which start as "no preference".
    #[napi]
    pub fn get_media_features(&self) -> crate::media::MediaFeatures {
        self.media.lock().unwrap().features
    }

    /// Change the reported OS preferences. Queues a `mediaChange` event when
    /// something changed and a listener is enabled, like the OS notification.
    #[napi]
    pub fn set_media_features(
        &self,
        reduced_motion: Option<bool>,
        increased_contrast: Option<bool>,
        forced_colors: Option<bool>,
    ) {
        let event = self
            .media
            .lock()
            .unwrap()
            .set(reduced_motion, increased_contrast, forced_colors);
        if let Some(event) = event {
            self.events.lock().unwrap().push(event);
        }
    }

    /// Validates and does nothing. This renderer drives the real offscreen
    /// platform window, which has no run loop here, so AppKit never delivers
    /// the appearance change and `getAppearance` would not follow. Forcing
    /// NSApp's appearance would only change the test process.
    #[napi]
    pub fn set_appearance(&self, appearance: String) -> Result<()> {
        crate::renderer::parse_window_appearance(&appearance).map_err(Error::from_reason)?;
        Ok(())
    }

    #[napi]
    pub fn get_appearance(&self) -> Result<String> {
        let appearance = window_call(|window, _cx| window.appearance())?;
        Ok(crate::renderer::appearance_name(appearance).to_string())
    }

    /// Simulate a mouse down event at the given window coordinates.
    /// Button: 0=left, 1=middle, 2=right. Defaults to left (0).
    /// `click_count` is what the platform reports for the press: 1 for a single
    /// click, 2 for the second press of a double click.
    #[napi]
    pub fn simulate_mouse_down(
        &self,
        x: f64,
        y: f64,
        button: Option<u32>,
        modifiers: Option<String>,
        click_count: Option<u32>,
    ) -> Result<()> {
        let modifiers = crate::automation::parse_modifiers(modifiers.as_deref());
        with_test_state(|cx, window, _view| {
            simulate_event_with_view_lease(
                cx,
                window,
                gpui::MouseDownEvent {
                    position: gpui::point(gpui::px(x as f32), gpui::px(y as f32)),
                    modifiers,
                    button: u32_to_mouse_button(button.unwrap_or(0)),
                    click_count: click_count.unwrap_or(1) as usize,
                    first_mouse: false,
                },
            );
            Ok(())
        })
    }

    /// Simulate a mouse up event at the given window coordinates.
    /// Button: 0=left, 1=middle, 2=right. Defaults to left (0).
    /// `click_count` is the count the platform reports for the release.
    #[napi]
    pub fn simulate_mouse_up(
        &self,
        x: f64,
        y: f64,
        button: Option<u32>,
        modifiers: Option<String>,
        click_count: Option<u32>,
    ) -> Result<()> {
        let modifiers = crate::automation::parse_modifiers(modifiers.as_deref());
        with_test_state(|cx, window, _view| {
            simulate_event_with_view_lease(
                cx,
                window,
                gpui::MouseUpEvent {
                    position: gpui::point(gpui::px(x as f32), gpui::px(y as f32)),
                    modifiers,
                    button: u32_to_mouse_button(button.unwrap_or(0)),
                    click_count: click_count.unwrap_or(1) as usize,
                },
            );
            Ok(())
        })
    }

    /// Simulate a scroll wheel event at the given position.
    /// delta_x and delta_y are in pixels (negative = scroll up/left).
    ///
    /// `phase` is `"started"` (the default, a gesture of its own), `"moved"`, `"ended"` or
    /// `"cancelled"`. The scroller that took a gesture's first delta keeps the rest of it, so
    /// continue one with `"moved"`.
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
        let touch_phase =
            crate::scroll::parse_touch_phase(phase.as_deref()).map_err(Error::from_reason)?;
        with_test_state(|cx, window, _view| {
            cx.simulate_event(
                window,
                gpui::ScrollWheelEvent {
                    position: gpui::point(gpui::px(x as f32), gpui::px(y as f32)),
                    delta: gpui::ScrollDelta::Pixels(gpui::point(
                        gpui::px(delta_x as f32),
                        gpui::px(delta_y as f32),
                    )),
                    modifiers,
                    touch_phase,
                },
            );
            Ok(())
        })
    }

    /// Files from the OS enter the window at `x`, `y` (`FileDropEvent::Entered`).
    #[napi]
    pub fn simulate_file_drag_enter(&self, x: f64, y: f64, paths: Vec<String>) -> Result<()> {
        with_test_state(|cx, window, _view| {
            let paths =
                gpui::ExternalPaths(paths.into_iter().map(std::path::PathBuf::from).collect());
            simulate_event_with_view_lease(
                cx,
                window,
                gpui::FileDropEvent::Entered {
                    position: file_drag_position(x, y),
                    paths,
                },
            );
            Ok(())
        })
    }

    /// The dragged files move to `x`, `y` (`FileDropEvent::Pending`).
    #[napi]
    pub fn simulate_file_drag_move(&self, x: f64, y: f64) -> Result<()> {
        with_test_state(|cx, window, _view| {
            simulate_event_with_view_lease(
                cx,
                window,
                gpui::FileDropEvent::Pending {
                    position: file_drag_position(x, y),
                },
            );
            Ok(())
        })
    }

    /// The dragged files are released at `x`, `y` (`FileDropEvent::Submit`).
    #[napi]
    pub fn simulate_file_drag_submit(&self, x: f64, y: f64) -> Result<()> {
        with_test_state(|cx, window, _view| {
            simulate_event_with_view_lease(
                cx,
                window,
                gpui::FileDropEvent::Submit {
                    position: file_drag_position(x, y),
                },
            );
            Ok(())
        })
    }

    /// The dragged files leave the window (`FileDropEvent::Exited`).
    #[napi]
    pub fn simulate_file_drag_exit(&self) -> Result<()> {
        with_test_state(|cx, window, _view| {
            simulate_event_with_view_lease(cx, window, gpui::FileDropEvent::Exited);
            Ok(())
        })
    }

    /// The platform's drag session ends (`FileDropEvent::Ended`).
    #[napi]
    pub fn simulate_file_drag_end(&self) -> Result<()> {
        with_test_state(|cx, window, _view| {
            simulate_event_with_view_lease(cx, window, gpui::FileDropEvent::Ended);
            Ok(())
        })
    }

    /// A whole Finder-style drop at `x`, `y`, in the order the platform sends
    /// it: Entered, Submit, then Exited and Ended.
    #[napi]
    pub fn simulate_file_drop(&self, x: f64, y: f64, paths: Vec<String>) -> Result<()> {
        self.simulate_file_drag_enter(x, y, paths)?;
        self.simulate_file_drag_submit(x, y)?;
        self.simulate_file_drag_exit()?;
        self.simulate_file_drag_end()
    }

    // ── Selection API ──────────────────────────────────────────────────

    /// The current text selection joined in document order, or null.
    #[napi]
    pub fn get_selected_text(&self) -> Option<String> {
        self.selection.lock().selected_text()
    }

    /// Drop the current selection.
    #[napi]
    pub fn clear_selection(&self) {
        self.selection.lock().clear();
    }

    /// Syntax-cache counters as `[hits, misses, documents]`.
    ///
    /// GPUIX rebuilds its whole element tree every frame, so a `<code>` block
    /// that misses the cache reparses at frame rate. A test can watch the hit
    /// count to catch that regression before a profiler does.
    #[napi]
    pub fn get_syntax_cache_stats(&self) -> Vec<f64> {
        let stats = crate::syntax::cache::stats();
        vec![
            stats.hits as f64,
            stats.misses as f64,
            stats.documents as f64,
        ]
    }

    /// Every string painted in the last frame, in paint order.
    ///
    /// `getAllText()` only sees `<text>` nodes in the retained tree. Native
    /// elements such as `<code>` and `<diff>` draw their text inside gpui, so
    /// this is the only way to assert on what they actually rendered.
    #[napi]
    pub fn get_painted_text(&self) -> Result<Vec<String>> {
        self.flush()?;
        Ok(crate::text::painted_text())
    }

    /// Every highlight wash painted in the last frame, in paint order.
    ///
    /// A quad is invisible to `getPaintedText()`, so this is the only way to
    /// assert on `highlight` without a screenshot. Each entry carries its rects,
    /// so a soft-wrapped match is provably two boxes.
    #[napi]
    pub fn get_painted_highlights(&self) -> Result<Vec<crate::element_tree::HighlightMatch>> {
        self.flush()?;
        Ok(crate::text::painted_highlights()
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Drag-select from one point to another: mouse down, move, up.
    ///
    /// A single helper rather than three calls because the listeners that drive
    /// selection are registered during **paint**, so a flush must sit between
    /// the down and the move. Getting that order wrong silently selects nothing,
    /// which is a miserable thing to debug from JS.
    #[napi]
    pub fn drag_select(&self, x1: f64, y1: f64, x2: f64, y2: f64) -> Result<()> {
        self.flush()?;
        self.simulate_mouse_down(x1, y1, None, None, None)?;
        self.flush()?;
        self.simulate_mouse_move(x2, y2, Some(0), None, None, None)?;
        self.flush()?;
        self.simulate_mouse_up(x2, y2, None, None, None)?;
        self.flush()?;
        Ok(())
    }

    // ── Scroll API ─────────────────────────────────────────────────────

    /// Set the scroll offset of a scrollable element.
    /// x and y are negative pixel values (scroll down = more negative y).
    /// Call flush() after to apply the offset and re-render.
    #[napi]
    pub fn scroll_to(
        &self,
        element_id: f64,
        x: f64,
        y: f64,
        behavior: Option<String>,
    ) -> Result<()> {
        let id = to_element_id(element_id)?;
        let behavior =
            crate::scroll::Behavior::parse(behavior.as_deref()).map_err(Error::from_reason)?;
        update_view_or_defer(move |view, _window, _cx| {
            if view.set_virtual_list_offset(id, x as f32, y as f32) {
                return;
            }
            if let Some(handle) = view.scroll_handles.get(&id) {
                crate::scroll::scroll_to(
                    id,
                    handle,
                    gpui::point(gpui::px(x as f32), gpui::px(y as f32)),
                    behavior,
                );
            }
        })
    }

    /// Scroll a child into view by its index in the children list.
    /// Call flush() after to apply and re-render. For a `<virtual-list>` the
    /// scroll is queued and applied on that flush, after the child splice.
    /// `offset_in_item` is in pixels and may be negative, which anchors the
    /// viewport top above the item.
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
        update_view_or_defer(move |view, _window, _cx| {
            if view.scroll_virtual_list_to_item(id, index, offset) {
                return;
            }
            if let Some(handle) = view.scroll_handles.get(&id) {
                handle.scroll_to_item(index);
            }
        })
    }

    /// The DOM's `scrollIntoView`: scrolls every scrollable ancestor until the element sits where
    /// `block` (default `"start"`) and `inline` (default `"nearest"`) ask.
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
        self.flush()?;
        update_view_or_defer(move |view, window, cx| {
            if view.scroll_element_into_view(id, options) {
                cx.notify();
                window.refresh();
            }
        })
    }

    /// Capture the mouse for `element_id`; takes effect at the next simulated
    /// pointer input. Ignored while no button is down.
    #[napi]
    pub fn set_pointer_capture(&self, element_id: f64, pointer_id: f64) -> Result<()> {
        let id = to_element_id(element_id)?;
        let tree = self.tree.lock().unwrap();
        self.pointer_requests
            .set(&tree, id, pointer_id)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn release_pointer_capture(&self, element_id: f64, pointer_id: f64) -> Result<()> {
        let id = to_element_id(element_id)?;
        self.pointer_requests
            .release(id, pointer_id)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn has_pointer_capture(&self, element_id: f64, pointer_id: f64) -> Result<bool> {
        let id = to_element_id(element_id)?;
        let tree = self.tree.lock().unwrap();
        self.pointer_requests
            .has(&tree, id, pointer_id)
            .map_err(Error::from_reason)
    }

    /// Ends the element drag in progress without a `dragend`; applies at the
    /// next simulated pointer input.
    #[napi]
    pub fn cancel_drag(&self) {
        self.pointer_requests.cancel_drag();
    }

    /// Lock the pointer to `element_id`. The frame it asks for is drawn here,
    /// so the outcome is in `drainEvents` when this returns.
    #[napi]
    pub fn request_pointer_lock(&self, element_id: f64) -> Result<()> {
        let id = to_element_id(element_id)?;
        self.pointer_requests.request_lock(id);
        self.flush()
    }

    #[napi]
    pub fn exit_pointer_lock(&self) -> Result<()> {
        self.pointer_requests.exit_lock();
        self.flush()
    }

    #[napi]
    pub fn pointer_lock_element(&self) -> Option<f64> {
        self.pointer_requests.locked_element().map(|id| id as f64)
    }

    /// Whether the simulated platform grants lock requests, as one that can lock
    /// the pointer does. Off, a request fails the way GPUI's does when the
    /// platform refuses.
    #[napi]
    pub fn simulate_pointer_lock_granting(&self, granting: bool) {
        self.platform.set_granting(granting);
    }

    /// The platform drops the lock, as it does when the window deactivates. The
    /// next frame notices, so this draws one.
    #[napi]
    pub fn simulate_pointer_lock_lost(&self) -> Result<()> {
        self.platform.drop_lock();
        self.flush()
    }

    /// The click an activation that is not a pointer press makes on `element_id`;
    /// collected for `drainEvents` like every other event.
    #[napi]
    pub fn click_element(&self, element_id: f64) -> Result<()> {
        let id = to_element_id(element_id)?;
        let payload = {
            let tree = self.tree.lock().unwrap();
            crate::events::synthetic_click::click_element(&tree, id).map_err(Error::from_reason)?
        };
        self.events.lock().unwrap().push(payload);
        Ok(())
    }

    /// Packed `"rgba"` (default) or `"bgra"` pixels onto an `<img>` host node.
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
        let width = crate::renderer::dimension_u32(width, "width")?;
        let height = crate::renderer::dimension_u32(height, "height")?;
        let format = crate::custom_elements::img::PixelFormat::parse(format.as_deref())
            .map_err(Error::from_reason)?;
        let bytes = pixels.to_vec();
        with_test_state(|cx, window, view| {
            let view = view.clone();
            cx.update_window(window, |_, window, app| {
                view.update(app, |view, cx| {
                    view.set_image_pixels(id, width, height, bytes, format, window, cx)
                })
            })
            .map_err(|e| Error::from_reason(e.to_string()))?
            .map_err(Error::from_reason)
        })
    }

    /// Encoded image bytes onto an `<img>` host node.
    #[napi]
    pub fn set_image(&self, element_id: f64, bytes: Buffer) -> Result<()> {
        let id = to_element_id(element_id)?;
        let bytes = bytes.to_vec();
        with_test_state(|cx, window, view| {
            let view = view.clone();
            cx.update_window(window, |_, window, app| {
                view.update(app, |view, cx| view.set_encoded_image(id, bytes, window, cx))
            })
            .map_err(|e| Error::from_reason(e.to_string()))?
            .map_err(Error::from_reason)
        })
    }

    /// `select()` on an `<input>` or `<textarea>`.
    #[napi]
    pub fn select_text(&self, element_id: f64) -> Result<()> {
        self.apply_text_selection(
            to_element_id(element_id)?,
            crate::custom_elements::text_selection::SelectionRequest::select_all(),
        )
    }

    /// `setSelectionRange(start, end, direction)`; offsets are UTF-16 units.
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

    /// `form.requestSubmit()`: validate, then emit `submit`.
    #[napi]
    pub fn request_submit(&self, id: f64) -> Result<()> {
        let id = to_element_id(id)?;
        window_call(move |window, cx| crate::forms::request_submit(window, cx, id))?
            .map_err(Error::from_reason)
    }

    /// `selectionStart`, `selectionEnd` and `selectionDirection`.
    #[napi]
    pub fn get_text_selection(
        &self,
        element_id: f64,
    ) -> Result<crate::custom_elements::text_selection::TextSelection> {
        let id = to_element_id(element_id)?;
        update_view(|view, _window, cx| view.text_selection(id, cx))?.map_err(Error::from_reason)
    }

    /// Make the fonts in a TrueType or OpenType file usable by `fontFamily`.
    #[napi]
    pub fn register_font(&self, data: Buffer) -> Result<()> {
        let bytes = data.to_vec();
        with_test_state(|cx, window, view| {
            let view = view.clone();
            cx.update_window(window, |_, window, app| {
                view.update(app, |view, cx| view.register_font(bytes, window, cx))
            })
            .map_err(|e| Error::from_reason(e.to_string()))?
            .map_err(Error::from_reason)
        })
    }

    /// The logical scroll anchor of a `<virtual-list>`:
    /// `[itemIndex, offsetInItemPx, viewportHeightPx]`, or null for anything
    /// else. `itemIndex == item count` is gpui's at-end sentinel.
    #[napi]
    pub fn get_list_scroll_top(&self, element_id: f64) -> Result<Option<Vec<f64>>> {
        let id = to_element_id(element_id)?;
        update_view(|view, _window, _cx| view.virtual_list_scroll_top(id).map(|top| top.to_vec()))
    }

    /// `"hidden"` | `"minimal"` | `"full"`.
    #[napi]
    pub fn set_debug_frame_overlay(&self, mode: String) -> Result<String> {
        let mode = parse_debug_frame_overlay_mode(&mode)?;
        with_test_state(|cx, window, _view| {
            cx.update_window(window, |_, window, _app| {
                window.set_debug_frame_overlay_mode(mode);
                debug_frame_overlay_mode_name(window.debug_frame_overlay_mode()).to_string()
            })
            .map_err(|e| Error::from_reason(e.to_string()))
        })
    }

    /// Hidden → minimal → full → hidden.
    #[napi]
    pub fn cycle_debug_frame_overlay(&self) -> Result<String> {
        with_test_state(|cx, window, _view| {
            cx.update_window(window, |_, window, _app| {
                window.cycle_debug_frame_overlay_mode();
                debug_frame_overlay_mode_name(window.debug_frame_overlay_mode()).to_string()
            })
            .map_err(|e| Error::from_reason(e.to_string()))
        })
    }

    #[napi]
    pub fn get_debug_frame_overlay(&self) -> Result<String> {
        with_test_state(|cx, window, _view| {
            cx.update_window(window, |_, window, _app| {
                debug_frame_overlay_mode_name(window.debug_frame_overlay_mode()).to_string()
            })
            .map_err(|e| Error::from_reason(e.to_string()))
        })
    }

    /// Clears the last 1000 draw samples. Frame count stays.
    #[napi]
    pub fn reset_debug_frame_overlay_stats(&self) -> Result<()> {
        with_test_state(|cx, window, _view| {
            cx.update_window(window, |_, window, _app| {
                window.reset_debug_frame_overlay_stats();
            })
            .map_err(|e| Error::from_reason(e.to_string()))?;
            Ok(())
        })
    }

    /// Same numbers as the on-screen overlay: current, p90, p99, max, frames.
    #[napi]
    pub fn get_debug_frame_overlay_stats(&self) -> Result<DebugFrameOverlayStats> {
        with_test_state(|cx, window, _view| {
            cx.update_window(window, |_, window, _app| {
                debug_frame_overlay_stats_js(window.debug_frame_overlay_stats())
            })
            .map_err(|e| Error::from_reason(e.to_string()))
        })
    }

    /// The default action of a scrolling key. See `GpuixRenderer::scroll_by_key`.
    #[napi]
    pub fn scroll_by_key(&self, key: String, shift: bool) -> Result<()> {
        update_view_or_defer(move |view, window, cx| {
            if view.scroll_by_key(&key, shift, window) {
                cx.notify();
                window.refresh();
            }
        })
    }

    /// The DOM scroll geometry of an element:
    /// `[scrollLeft, scrollTop, scrollWidth, scrollHeight, clientWidth, clientHeight]`, or null
    /// if it has not been painted.
    #[napi]
    pub fn get_scroll_metrics(&self, element_id: f64) -> Result<Option<Vec<f64>>> {
        let id = to_element_id(element_id)?;
        self.flush()?;
        with_test_state(|cx, window, view| {
            let view = view.clone();
            let result = cx
                .update_window(window, |_, _window, app| {
                    view.update(app, |view, _cx| {
                        let painted = crate::automation::get_bounds(id)
                            .map(|bounds| (bounds.width as f32, bounds.height as f32));
                        crate::scroll::metrics(view.scroll_handles.get(&id), painted)
                            .map(|metrics| metrics.to_vec())
                    })
                })
                .map_err(|e| Error::from_reason(e.to_string()))?;
            Ok(result)
        })
    }

    /// Get the current scroll offset of a scrollable element.
    /// Returns [x, y] or null if the element has no scroll handle.
    #[napi]
    pub fn get_scroll_offset(&self, element_id: f64) -> Result<Option<Vec<f64>>> {
        let id = to_element_id(element_id)?;
        update_view(|view, _window, _cx| {
            if let Some(offset) = view.virtual_list_offset(id) {
                return Some(offset.to_vec());
            }
            view.scroll_handles.get(&id).map(|handle| {
                let offset = handle.offset();
                vec![
                    f64::from(f32::from(offset.x)),
                    f64::from(f32::from(offset.y)),
                ]
            })
        })
    }

    /// Capture a screenshot of the current rendered state and save as PNG.
    /// Supported on macOS through Metal and Windows through DirectX.
    #[napi]
    pub fn capture_screenshot(&self, path: String) -> Result<()> {
        with_test_state(|cx, window, view| {
            let view = view.clone();

            // Flush: notify view and run until parked so layout/rendering are current.
            cx.update_window(window, |_, _window, app| {
                view.update(app, |_, cx| {
                    cx.notify();
                });
            })
            .map_err(|e| Error::from_reason(e.to_string()))?;

            // Force a window refresh before capture so render_to_image reads
            // the most recent frame scene.
            cx.update_window(window, |_, window, _app| {
                window.refresh();
            })
            .map_err(|e| Error::from_reason(e.to_string()))?;

            cx.run_until_parked();

            // Capture via the platform renderer's render_to_image implementation.
            let image = cx
                .capture_screenshot(window)
                .map_err(|e| Error::from_reason(format!("Screenshot capture failed: {}", e)))?;

            // Save as PNG (format inferred from file extension).
            image
                .save(&path)
                .map_err(|e| Error::from_reason(format!("Failed to save screenshot: {}", e)))?;

            Ok(())
        })
    }

    /// Return and clear all collected events since the last drain.
    /// Events are collected synchronously — no event loop queuing.
    #[napi]
    pub fn drain_events(&self) -> Vec<EventPayload> {
        let mut events = self.events.lock().unwrap();
        events.drain(..).collect()
    }

    // ── Tree inspection ──────────────────────────────────────────────

    /// Get all text content in the tree (depth-first order).
    #[napi]
    pub fn get_all_text(&self) -> Vec<String> {
        let tree = self.tree.lock().unwrap();
        let mut texts = Vec::new();
        if let Some(root_id) = tree.root_id {
            Self::collect_text(root_id, &tree, &mut texts);
        }
        texts
    }

    /// Find element IDs matching the given type (e.g. "div", "text").
    #[napi]
    pub fn find_by_type(&self, element_type: String) -> Vec<f64> {
        let tree = self.tree.lock().unwrap();
        tree.elements
            .values()
            .filter(|e| e.element_type == element_type)
            .map(|e| e.id as f64)
            .collect()
    }

    /// Check if an element has a specific event listener.
    #[napi]
    pub fn has_event_listener(&self, id: f64, event_type: String) -> Result<bool> {
        let id = to_element_id(id)?;
        let tree = self.tree.lock().unwrap();
        Ok(tree
            .elements
            .get(&id)
            .map(|e| e.events().contains(&event_type))
            .unwrap_or(false))
    }

    /// Get the text content of an element.
    #[napi]
    pub fn get_text(&self, id: f64) -> Result<Option<String>> {
        let id = to_element_id(id)?;
        let tree = self.tree.lock().unwrap();
        Ok(tree.elements.get(&id).and_then(|e| e.content.clone()))
    }

    /// Get the full tree as JSON for snapshot testing.
    #[napi]
    pub fn get_tree_json(&self) -> Result<String> {
        let tree = self.tree.lock().unwrap();
        let json = tree.to_json(&std::collections::HashMap::new());
        serde_json::to_string_pretty(&json)
            .map_err(|e| Error::from_reason(format!("JSON serialization failed: {}", e)))
    }

    /// GPUI accessibility dump from the last painted frame.
    /// Empty until a11y is active; the test renderer turns that on at construct.
    #[napi(js_name = "getA11yTree")]
    pub fn get_a11y_tree(&self) -> Result<String> {
        self.flush()?;
        window_call(|window, _cx| {
            window
                .debug_a11y_tree_json()
                .unwrap_or_else(|| "{}".to_string())
        })
    }

    /// Tree JSON with last-paint bounds. Used by the automation locators.
    #[napi]
    pub fn get_automation_tree(&self) -> Result<String> {
        self.flush()?;
        let tree = self.tree.lock().unwrap();
        let json = tree.to_automation_json(&crate::automation::all_bounds());
        serde_json::to_string(&json)
            .map_err(|e| Error::from_reason(format!("JSON serialization failed: {}", e)))
    }

    /// Last painted bounds for an element, or null if it was not painted.
    #[napi]
    pub fn get_element_bounds(&self, id: f64) -> Result<Option<crate::renderer::ElementBounds>> {
        let id = to_element_id(id)?;
        self.flush()?;
        Ok(crate::automation::get_bounds(id).map(crate::renderer::ElementBounds::from_painted))
    }

    /// Ids under the window point, topmost first. See the live renderer.
    #[napi]
    pub fn get_elements_at(&self, x: f64, y: f64) -> Result<Vec<f64>> {
        self.flush()?;
        let tree = self.tree.lock().unwrap();
        Ok(tree
            .elements_at(x, y, &crate::automation::get_bounds)
            .into_iter()
            .map(|id| id as f64)
            .collect())
    }

    #[napi]
    pub fn clock_pause(&self) -> Result<f64> {
        with_test_state(|cx, window, view| {
            let view = view.clone();
            let now_ms = cx
                .update_window(window, |_, _window, app| {
                    view.update(app, |view, cx| {
                        let now_ms = view.clock.pause();
                        cx.notify();
                        now_ms
                    })
                })
                .map_err(|e| Error::from_reason(e.to_string()))?;
            cx.run_until_parked();
            Ok(now_ms)
        })
    }

    #[napi]
    pub fn clock_set(&self, now_ms: f64) -> Result<f64> {
        with_test_state(|cx, window, view| {
            let view = view.clone();
            let now_ms = cx
                .update_window(window, |_, _window, app| {
                    view.update(app, |view, cx| {
                        let now_ms = view.clock.set_ms(now_ms);
                        cx.notify();
                        now_ms
                    })
                })
                .map_err(|e| Error::from_reason(e.to_string()))?;
            cx.run_until_parked();
            Ok(now_ms)
        })
    }

    #[napi]
    pub fn clock_fast_forward(&self, delta_ms: f64) -> Result<f64> {
        with_test_state(|cx, window, view| {
            let view = view.clone();
            let now_ms = cx
                .update_window(window, |_, _window, app| {
                    view.update(app, |view, cx| {
                        let now_ms = view.clock.fast_forward_ms(delta_ms);
                        cx.notify();
                        now_ms
                    })
                })
                .map_err(|e| Error::from_reason(e.to_string()))?;
            cx.run_until_parked();
            Ok(now_ms)
        })
    }

    #[napi]
    pub fn clock_resume(&self) -> Result<f64> {
        with_test_state(|cx, window, view| {
            let view = view.clone();
            let now_ms = cx
                .update_window(window, |_, _window, app| {
                    view.update(app, |view, cx| {
                        let now_ms = view.clock.resume();
                        cx.notify();
                        now_ms
                    })
                })
                .map_err(|e| Error::from_reason(e.to_string()))?;
            cx.run_until_parked();
            Ok(now_ms)
        })
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

    /// Defines the keyframes `animation-name: name` plays. See
    /// `GpuixRenderer.defineKeyframes`.
    #[napi]
    pub fn define_keyframes(&self, name: String, frames_json: String) -> Result<()> {
        self.with_motion_state(false, move |view, _now| {
            view.keyframes.define(&name, &frames_json)
        })
    }

    /// Runs `run` against the window's motion values at the window clock's
    /// instant. A write repaints, which samples it and delivers its events.
    fn with_motion_values<R>(
        &self,
        repaint: bool,
        run: impl FnOnce(&mut crate::motion::MotionValueStore, web_time::Instant) -> std::result::Result<R, String>,
    ) -> Result<R> {
        self.with_motion_state(repaint, move |view, now| run(&mut view.motion_values, now))
    }

    /// Runs `run` against the window's view at the window clock's instant.
    /// Only the motion state of the view is for it to touch.
    fn with_motion_state<R>(
        &self,
        repaint: bool,
        run: impl FnOnce(&mut GpuixView, web_time::Instant) -> std::result::Result<R, String>,
    ) -> Result<R> {
        with_test_state(|cx, window, view| {
            let view = view.clone();
            let result = cx
                .update_window(window, |_, _window, app| {
                    view.update(app, |view, cx| {
                        let now = view.clock.now();
                        let result = run(view, now);
                        if repaint {
                            cx.notify();
                        }
                        result
                    })
                })
                .map_err(|error| Error::from_reason(error.to_string()))?;
            cx.run_until_parked();
            result.map_err(Error::from_reason)
        })
    }

    /// Advance GPUI's deterministic test executor and run due timers.
    #[napi]
    pub fn advance_time(&self, milliseconds: f64) -> Result<()> {
        if !milliseconds.is_finite() || milliseconds < 0.0 {
            return Err(Error::from_reason(format!(
                "advanceTime milliseconds must be finite and non-negative, got {milliseconds}"
            )));
        }
        with_test_state(|cx, _window, _view| {
            cx.advance_clock(Duration::from_secs_f64(milliseconds / 1000.0));
            cx.run_until_parked();
            Ok(())
        })
    }

    /// Get the root element ID, or null if no root is set.
    #[napi]
    pub fn get_root_id(&self) -> Option<f64> {
        self.tree.lock().unwrap().root_id.map(|id| id as f64)
    }

    /// The offscreen window size, so `useWindowSize()` reports the same numbers
    /// under test as in a real window instead of falling back to a default.
    #[napi]
    pub fn get_window_size(&self) -> Result<crate::renderer::WindowSize> {
        let size = window_call(|window, _cx| window.viewport_size())?;
        Ok(crate::renderer::WindowSize {
            width: f32::from(size.width) as f64,
            height: f32::from(size.height) as f64,
        })
    }

    // ── Private helpers ──────────────────────────────────────────────

    fn collect_text(id: u64, tree: &RetainedTree, texts: &mut Vec<String>) {
        if let Some(element) = tree.elements.get(&id) {
            if let Some(ref content) = element.content {
                texts.push(content.clone());
            }
            for &child_id in &element.children {
                Self::collect_text(child_id, tree, texts);
            }
        }
    }
}
