//! Calling JavaScript synchronously from inside native input handling, and
//! letting that JavaScript call back into the renderer.
//!
//! The ordinary event path is a queue: native pushes an `EventPayload`, JS
//! handles it later, and nothing JS does can change what native does with the
//! input. That cannot give `preventDefault()` its meaning. A keydown handler has
//! to run before the editor acts on the key, and has to be able to cancel it.
//!
//! A sync handler is a JS function registered with `setSyncEventHandler`. It
//! takes one `EventPayload` and returns `{ defaultPrevented, propagationStopped }`.
//! It is only called when this thread is inside a call from JS: `tick()` on
//! macOS, where GPUI runs on the JS thread, every method of the test renderer,
//! and any browser event handler on the web. On Windows and Linux GPUI runs on
//! its own thread, and a blocking round trip to the JS thread is not worth the
//! latency and the deadlocks, so `emit` queues the event as before and reports
//! the default as not prevented.
//!
//! # Re-entrancy
//!
//! GPUI dispatches input with the `App` borrowed and the `Window` taken out of
//! the app, so a handler that calls `applyBatch`, `focusElement` or a getter
//! would normally panic on the double borrow. While a sync call is in flight the
//! dispatching `&mut Window` and `&mut App` are stashed here, and the napi
//! helpers (`update_window`, `with_test_state`) use them through
//! `with_active_window` instead of borrowing again.
//!
//! The stash is not enough for everything. `GpuixView` can be leased by whoever
//! dispatched the input, and a nested update of a leased entity panics inside
//! GPUI. The helpers turn that into an error that names the call, instead of
//! aborting a live window.

use std::cell::{Cell, RefCell};

use gpui::{App, Window};

use crate::element_tree::EventPayload;
use crate::renderer::{EventCallback, GpuixView};

/// What a JS handler decided about an event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DispatchOutcome {
    pub default_prevented: bool,
    pub propagation_stopped: bool,
}

#[derive(Clone, Copy)]
struct ActiveContext {
    window: *mut Window,
    app: *mut App,
}

thread_local! {
    /// The dispatching window and app, present between `emit` calling JS and JS
    /// returning. Taken for the length of each re-entrant call so that a nested
    /// one cannot create a second `&mut` to the same value.
    static ACTIVE: Cell<Option<ActiveContext>> = const { Cell::new(None) };
    /// How many sync calls are in flight on this thread.
    static IN_SYNC_CALL: Cell<usize> = const { Cell::new(0) };
}

/// True while a JS handler called through `emit` has not returned.
pub(crate) fn in_sync_call() -> bool {
    IN_SYNC_CALL.with(|depth| depth.get() > 0)
}

struct ScopeGuard {
    previous: Option<ActiveContext>,
}

impl ScopeGuard {
    fn enter(window: &mut Window, app: &mut App) -> Self {
        IN_SYNC_CALL.with(|depth| depth.set(depth.get() + 1));
        let previous = ACTIVE.with(|active| {
            active.replace(Some(ActiveContext {
                window,
                app,
            }))
        });
        Self { previous }
    }
}

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.set(self.previous.take()));
        IN_SYNC_CALL.with(|depth| depth.set(depth.get() - 1));
    }
}

/// Runs `f` against the window and app that are dispatching the in-flight sync
/// call. `None` when there is none, or when `f` is itself running inside this
/// function.
pub(crate) fn with_active_window<R>(f: impl FnOnce(&mut Window, &mut App) -> R) -> Option<R> {
    struct Restore(ActiveContext);
    impl Drop for Restore {
        fn drop(&mut self) {
            ACTIVE.with(|active| active.set(Some(self.0)));
        }
    }

    let context = ACTIVE.with(|active| active.take())?;
    let _restore = Restore(context);
    // SAFETY: the pointers come from the `&mut Window` and `&mut App` that
    // `emit` holds for as long as the guard lives, and `emit` does not touch
    // them while JS runs. `ACTIVE` is emptied for the length of this call, so
    // no second reference to either can be made from inside `f`.
    Some(f(unsafe { &mut *context.window }, unsafe {
        &mut *context.app
    }))
}

/// Delivers `payload` to JS and returns what its handlers decided.
///
/// Synchronous when a sync handler is registered and this thread is inside a
/// call from JS. Otherwise the event goes through `callback` like any other and
/// the outcome is the default.
pub(crate) fn emit(
    window: &mut Window,
    app: &mut App,
    callback: &Option<EventCallback>,
    payload: EventPayload,
) -> DispatchOutcome {
    if backend::available() {
        let _scope = ScopeGuard::enter(window, app);
        return backend::call(payload);
    }
    if let Some(callback) = callback {
        callback(payload);
    }
    DispatchOutcome::default()
}

/// `emit_event_full`, but delivered with `emit`.
pub(crate) fn emit_full(
    window: &mut Window,
    app: &mut App,
    callback: &Option<EventCallback>,
    element_id: u64,
    event_type: &str,
    build: impl FnOnce(&mut EventPayload),
) -> DispatchOutcome {
    let mut payload = EventPayload {
        element_id: element_id as f64,
        event_type: event_type.to_string(),
        ..Default::default()
    };
    build(&mut payload);
    emit(window, app, callback, payload)
}

/// Why a re-entrant update of the root view could not run.
pub(crate) const VIEW_BUSY: &str = "The root view is being updated by the input that is being dispatched, so this call cannot read it from the event handler. Calls that only change state (focus, blur, selection) are deferred until the input is done; reads like getFocusedElementId are not available.";
pub(crate) const NESTED: &str =
    "A renderer call cannot be nested inside another one made from an event handler";

fn root_view(window: &Window) -> Result<gpui::Entity<GpuixView>, String> {
    window
        .root::<GpuixView>()
        .flatten()
        .ok_or_else(|| "The window has no GpuixView".to_string())
}

/// Updates the root view from inside a sync call and returns what `update`
/// returned. Fails when the input being dispatched holds the view, which only
/// its own handlers can release.
pub(crate) fn update_root_view<R>(
    update: impl FnOnce(&mut GpuixView, &mut Window, &mut gpui::Context<GpuixView>) -> R,
) -> Result<R, String> {
    with_active_window(|window, cx| {
        let view = root_view(window)?;
        if view.is_updating(cx) {
            return Err(VIEW_BUSY.to_string());
        }
        Ok(view.update(cx, |view, cx| update(view, window, cx)))
    })
    .unwrap_or_else(|| Err(NESTED.to_string()))
}

/// Like `update_root_view` for a change that returns nothing: when the view is
/// held it runs once the input being dispatched has returned it.
pub(crate) fn update_root_view_or_defer(
    update: impl FnOnce(&mut GpuixView, &mut Window, &mut gpui::Context<GpuixView>) + 'static,
) -> Result<(), String> {
    with_active_window(|window, cx| {
        let view = root_view(window)?;
        if view.is_updating(cx) {
            window.defer(cx, move |window, cx| {
                view.update(cx, |view, cx| update(view, window, cx));
            });
            return Ok(());
        }
        view.update(cx, |view, cx| update(view, window, cx));
        Ok(())
    })
    .unwrap_or_else(|| Err(NESTED.to_string()))
}

/// Runs `update` against the window being dispatched, which a leased root view
/// does not block. Fails when called from inside another such call.
pub(crate) fn update_active_window<R>(
    update: impl FnOnce(&mut Window, &mut App) -> R,
) -> Result<R, String> {
    with_active_window(update).ok_or_else(|| NESTED.to_string())
}

/// Sends a renderer call down one of two paths: `inside` while a sync call is in
/// flight, where the app is already borrowed by the input being dispatched, and
/// `outside` otherwise. `inside` is one of the `update_*` functions above and
/// reports its error as a `String`, which `map_error` turns into the caller's.
pub(crate) fn route<U, R, E>(
    update: U,
    inside: impl FnOnce(U) -> Result<R, String>,
    map_error: impl FnOnce(String) -> E,
    outside: impl FnOnce(U) -> Result<R, E>,
) -> Result<R, E> {
    if in_sync_call() {
        inside(update).map_err(map_error)
    } else {
        outside(update)
    }
}

/// Redraws the window without leasing the root view, which `applyBatch` may
/// need while the input holds it.
pub(crate) fn invalidate_root_view() -> Result<(), String> {
    with_active_window(|window, cx| {
        let view = root_view(window)?;
        cx.notify(view.entity_id());
        window.refresh();
        Ok(())
    })
    .unwrap_or_else(|| Err(NESTED.to_string()))
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) use backend::{native_call, register, SyncDispatchResult};

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
mod backend {
    use super::*;
    use napi::bindgen_prelude::*;
    use napi_derive::napi;

    /// What the sync handler returns.
    #[napi(object)]
    pub struct SyncDispatchResult {
        pub default_prevented: bool,
        pub propagation_stopped: bool,
    }

    struct Registered {
        handler: FunctionRef<EventPayload, SyncDispatchResult>,
        env: Env,
    }

    thread_local! {
        static HANDLER: RefCell<Option<Registered>> = const { RefCell::new(None) };
        /// How many calls from JS are on this thread's stack.
        static NATIVE_DEPTH: Cell<usize> = const { Cell::new(0) };
        /// The first error a sync handler threw during the outermost native call.
        static PENDING_ERROR: RefCell<Option<Error>> = const { RefCell::new(None) };
    }

    /// Installs the sync handler, or clears it with `None`.
    pub(crate) fn register(
        env: &Env,
        handler: Option<Function<EventPayload, SyncDispatchResult>>,
    ) -> Result<()> {
        let registered = match handler {
            Some(handler) => Some(Registered {
                handler: handler.create_ref()?,
                env: *env,
            }),
            None => None,
        };
        HANDLER.with(|slot| *slot.borrow_mut() = registered);
        Ok(())
    }

    pub(super) fn available() -> bool {
        NATIVE_DEPTH.with(|depth| depth.get() > 0) && HANDLER.with(|slot| slot.borrow().is_some())
    }

    pub(super) fn call(payload: EventPayload) -> DispatchOutcome {
        let outcome = (|| {
            let env = HANDLER.with(|slot| {
                slot.borrow()
                    .as_ref()
                    .expect("available() checked that a sync handler is registered")
                    .env
            });
            // The registry is not borrowed while JS runs, so the handler can
            // replace itself.
            let function = HANDLER.with(|slot| {
                slot.borrow()
                    .as_ref()
                    .expect("available() checked that a sync handler is registered")
                    .handler
                    .borrow_back(&env)
            })?;
            function.call(payload)
        })();
        match outcome {
            Ok(result) => DispatchOutcome {
                default_prevented: result.default_prevented,
                propagation_stopped: result.propagation_stopped,
            },
            Err(error) => {
                // Native cannot throw through GPUI, so the first error waits for
                // the napi method that is on the stack and is thrown from there.
                PENDING_ERROR.with(|slot| {
                    slot.borrow_mut().get_or_insert(error);
                });
                DispatchOutcome::default()
            }
        }
    }

    /// Marks `f` as running inside a call from JS, which is what lets `emit`
    /// call back into JS. Surfaces an error thrown by a sync handler when the
    /// outermost call returns.
    pub(crate) fn native_call<R>(f: impl FnOnce() -> Result<R>) -> Result<R> {
        NATIVE_DEPTH.with(|depth| depth.set(depth.get() + 1));
        let result = f();
        let outermost = NATIVE_DEPTH.with(|depth| {
            depth.set(depth.get() - 1);
            depth.get() == 0
        });
        if outermost {
            if let Some(error) = PENDING_ERROR.with(|slot| slot.borrow_mut().take()) {
                return Err(error);
            }
        }
        result
    }
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) use backend::register;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod backend {
    use super::*;
    use wasm_bindgen::JsValue;

    thread_local! {
        static HANDLER: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
    }

    pub(crate) fn register(handler: Option<js_sys::Function>) {
        HANDLER.with(|slot| *slot.borrow_mut() = handler);
    }

    /// Browser input arrives through wasm callbacks from JS, so JS can always
    /// be called back.
    pub(super) fn available() -> bool {
        HANDLER.with(|slot| slot.borrow().is_some())
    }

    pub(super) fn call(payload: EventPayload) -> DispatchOutcome {
        let handler = HANDLER.with(|slot| slot.borrow().clone());
        let Some(handler) = handler else {
            return DispatchOutcome::default();
        };
        let result = serde_json::to_string(&payload)
            .map_err(|error| JsValue::from_str(&error.to_string()))
            .and_then(|json| js_sys::JSON::parse(&json))
            .and_then(|payload| handler.call1(&JsValue::UNDEFINED, &payload));
        let flag = |result: &JsValue, name: &str| {
            js_sys::Reflect::get(result, &JsValue::from_str(name))
                .map(|value| value.is_truthy())
                .unwrap_or(false)
        };
        match result {
            Ok(result) => DispatchOutcome {
                default_prevented: flag(&result, "defaultPrevented"),
                propagation_stopped: flag(&result, "propagationStopped"),
            },
            Err(error) => {
                log::error!("GPUIX browser sync event handler failed: {error:?}");
                DispatchOutcome::default()
            }
        }
    }
}
