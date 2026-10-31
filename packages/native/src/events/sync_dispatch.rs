//! Calling JavaScript synchronously from inside native input handling, and
//! letting that JavaScript call back into the renderer.
//!
//! The ordinary event path is a queue: native pushes an `EventPayload`, JS
//! handles it later, and nothing JS does can change what native does with the
//! input. That cannot give `preventDefault()` its meaning. A keydown handler has
//! to run before the editor acts on the key, and has to be able to cancel it.
//!
//! A sync handler is a JS function registered with `setSyncEventHandler`. Each
//! renderer owns its own (`SyncHandlerOwner`), and the window the input
//! arrived on picks which one runs. It takes one `EventPayload` and returns `{ defaultPrevented, propagationStopped }`.
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
    /// What a `dragover` handler chose for the drop, when one ran.
    pub drag_operation: Option<gpui::DragOperation>,
}

/// The operation a `dropEffect` stands for.
pub(crate) fn drag_operation(drop_effect: &str) -> Result<gpui::DragOperation, String> {
    match drop_effect {
        "none" => Ok(gpui::DragOperation::None),
        "copy" => Ok(gpui::DragOperation::Copy),
        "move" => Ok(gpui::DragOperation::Move),
        "link" => Ok(gpui::DragOperation::Link),
        other => Err(format!("\"{other}\" is not a dropEffect")),
    }
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

    /// A sync call made with no window or app to give back: `ACTIVE` is emptied, so every call JS
    /// makes while it runs finds nothing to use.
    fn enter_without_window() -> Self {
        IN_SYNC_CALL.with(|depth| depth.set(depth.get() + 1));
        Self { previous: ACTIVE.with(|active| active.take()) }
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
    let window_id = window.window_handle().window_id();
    if backend::available(window_id) {
        let _scope = ScopeGuard::enter(window, app);
        return backend::call(window_id, payload);
    }
    if let Some(callback) = callback {
        callback(payload);
    }
    DispatchOutcome::default()
}

/// Delivers `payload` for `window` the way `emit` does, from code that holds the window or the app
/// and has no `&mut` of them to give: an element's teardown, which runs inside `applyBatch`, a
/// frame, or the drop of a window.
///
/// The handler runs as in a sync call, but nothing is reachable from it: any renderer call it makes
/// fails with [`NESTED`], where reaching for the window or app that the caller holds would be
/// undefined behaviour. Without a sync handler the event goes through `callback`, like any other.
pub(crate) fn emit_without_window(
    window: gpui::WindowId,
    callback: &Option<EventCallback>,
    payload: EventPayload,
) -> DispatchOutcome {
    if backend::available(window) {
        return run_without_window(|| backend::call(window, payload));
    }
    if let Some(callback) = callback {
        callback(payload);
    }
    DispatchOutcome::default()
}

/// Runs `call` as a sync call that no window or app is reachable from.
fn run_without_window<R>(call: impl FnOnce() -> R) -> R {
    let _scope = ScopeGuard::enter_without_window();
    call()
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

pub(crate) fn root_view(window: &Window) -> Result<gpui::Entity<GpuixView>, String> {
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

/// Reports a failure of a change that was deferred past its caller, which can
/// no longer be given the error. It is thrown from the call on the stack when
/// that is still the one that dispatched the input.
pub(crate) fn report_deferred_error(message: String) {
    backend::report_deferred_error(message);
}

/// Marks the root view dirty without leasing it, which `applyBatch` may
/// need while the input holds it.
pub(crate) fn invalidate_root_view(dirty: crate::retained_tree::Dirty) -> Result<(), String> {
    with_active_window(|window, cx| {
        let view = root_view(window)?;
        crate::renderer::invalidate_after_mutation(cx, view.entity_id(), dirty);
        Ok(())
    })
    .unwrap_or_else(|| Err(NESTED.to_string()))
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) use backend::{native_call, register, SyncDispatchResult, SyncHandlerOwner};

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
mod backend {
    use super::*;
    use napi::bindgen_prelude::*;
    use napi_derive::napi;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// What the sync handler returns.
    #[napi(object)]
    pub struct SyncDispatchResult {
        pub default_prevented: bool,
        pub propagation_stopped: bool,
        /// The `dropEffect` a `dragover` ended with.
        pub drop_effect: Option<String>,
    }

    struct Registered {
        handler: FunctionRef<EventPayload, SyncDispatchResult>,
        env: Env,
    }

    thread_local! {
        /// Handlers by owner. Only the JS thread reads them: `available`
        /// needs `NATIVE_DEPTH`, which is zero on GPUI's own thread.
        static HANDLERS: RefCell<HashMap<u64, Registered>> = RefCell::new(HashMap::new());
        /// Which owner's handler an input on a window goes to.
        static WINDOWS: RefCell<HashMap<gpui::WindowId, u64>> = RefCell::new(HashMap::new());
        /// How many calls from JS are on this thread's stack.
        static NATIVE_DEPTH: Cell<usize> = const { Cell::new(0) };
        /// The first error a sync handler threw during the outermost native call.
        static PENDING_ERROR: RefCell<Option<Error>> = const { RefCell::new(None) };
    }

    static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

    /// A renderer's claim on the sync handler registry. Dropping it releases
    /// the handler and its window, so a destroyed renderer neither keeps its
    /// `FunctionRef` alive nor receives input.
    pub(crate) struct SyncHandlerOwner {
        id: u64,
    }

    impl SyncHandlerOwner {
        pub(crate) fn new() -> Self {
            Self {
                id: NEXT_OWNER.fetch_add(1, Ordering::Relaxed),
            }
        }

        /// Routes input on `window` to this owner's handler.
        pub(crate) fn bind_window(&self, window: gpui::WindowId) {
            WINDOWS.with(|windows| windows.borrow_mut().insert(window, self.id));
        }
    }

    impl Drop for SyncHandlerOwner {
        fn drop(&mut self) {
            // `try_with`: this can run while the thread's locals are being torn down.
            let removed = HANDLERS.try_with(|handlers| handlers.borrow_mut().remove(&self.id));
            let _ = WINDOWS.try_with(|windows| windows.borrow_mut().retain(|_, id| *id != self.id));
            // The `FunctionRef` is dropped outside the borrow it was removed under.
            drop(removed);
        }
    }

    /// Installs `owner`'s sync handler, or clears it with `None`.
    pub(crate) fn register(
        owner: &SyncHandlerOwner,
        env: &Env,
        handler: Option<Function<EventPayload, SyncDispatchResult>>,
    ) -> Result<()> {
        let previous = match handler {
            Some(handler) => {
                let registered = Registered {
                    handler: handler.create_ref()?,
                    env: *env,
                };
                HANDLERS.with(|handlers| handlers.borrow_mut().insert(owner.id, registered))
            }
            None => HANDLERS.with(|handlers| handlers.borrow_mut().remove(&owner.id)),
        };
        drop(previous);
        Ok(())
    }

    pub(super) fn available(window: gpui::WindowId) -> bool {
        NATIVE_DEPTH.with(|depth| depth.get() > 0)
            && WINDOWS.with(|windows| {
                windows.borrow().get(&window).is_some_and(|owner| {
                    HANDLERS.with(|handlers| handlers.borrow().contains_key(owner))
                })
            })
    }

    pub(super) fn call(window: gpui::WindowId, payload: EventPayload) -> DispatchOutcome {
        let outcome = (|| {
            let owner = WINDOWS.with(|windows| {
                *windows
                    .borrow()
                    .get(&window)
                    .expect("available() checked that the window has an owner")
            });
            let env = HANDLERS.with(|handlers| {
                handlers
                    .borrow()
                    .get(&owner)
                    .expect("available() checked that a sync handler is registered")
                    .env
            });
            // The registry is not borrowed while JS runs, so the handler can
            // replace itself.
            let function = HANDLERS.with(|handlers| {
                handlers
                    .borrow()
                    .get(&owner)
                    .expect("available() checked that a sync handler is registered")
                    .handler
                    .borrow_back(&env)
            })?;
            function.call(payload)
        })();
        let outcome = outcome.and_then(|result| {
            let drag_operation = result
                .drop_effect
                .as_deref()
                .map(drag_operation)
                .transpose()
                .map_err(Error::from_reason)?;
            Ok(DispatchOutcome {
                default_prevented: result.default_prevented,
                propagation_stopped: result.propagation_stopped,
                drag_operation,
            })
        });
        match outcome {
            Ok(outcome) => outcome,
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

    pub(super) fn report_deferred_error(message: String) {
        if NATIVE_DEPTH.with(|depth| depth.get()) == 0 {
            log::error!("GPUIX deferred call failed: {message}");
            return;
        }
        PENDING_ERROR.with(|slot| {
            slot.borrow_mut().get_or_insert(Error::from_reason(message));
        });
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
    pub(super) fn available(_window: gpui::WindowId) -> bool {
        HANDLER.with(|slot| slot.borrow().is_some())
    }

    pub(super) fn report_deferred_error(message: String) {
        log::error!("GPUIX deferred call failed: {message}");
    }

    pub(super) fn call(_window: gpui::WindowId, payload: EventPayload) -> DispatchOutcome {
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
        let drop_effect = |result: &JsValue| {
            js_sys::Reflect::get(result, &JsValue::from_str("dropEffect"))
                .ok()
                .and_then(|value| value.as_string())
        };
        match result {
            Ok(result) => {
                let drag_operation = drop_effect(&result)
                    .map(|effect| drag_operation(&effect))
                    .transpose();
                match drag_operation {
                    Ok(drag_operation) => DispatchOutcome {
                        default_prevented: flag(&result, "defaultPrevented"),
                        propagation_stopped: flag(&result, "propagationStopped"),
                        drag_operation,
                    },
                    Err(message) => {
                        log::error!("GPUIX browser sync event handler failed: {message}");
                        DispatchOutcome::default()
                    }
                }
            }
            Err(error) => {
                log::error!("GPUIX browser sync event handler failed: {error:?}");
                DispatchOutcome::default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::DragOperation;

    fn event() -> EventPayload {
        EventPayload { element_id: 7.0, event_type: "detach".to_string(), ..Default::default() }
    }

    #[test]
    fn without_a_sync_handler_the_event_goes_through_the_callback() {
        let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = received.clone();
        let callback: Option<EventCallback> = Some(std::sync::Arc::new(move |payload: EventPayload| {
            sink.lock().unwrap().push(payload.event_type);
        }));
        let window = gpui::WindowId::from(1_u64);
        let outcome = emit_without_window(window, &callback, event());
        assert_eq!(outcome, DispatchOutcome::default());
        assert_eq!(*received.lock().unwrap(), ["detach"]);
    }

    #[test]
    fn without_a_sync_handler_and_a_callback_the_event_is_dropped() {
        let window = gpui::WindowId::from(1_u64);
        assert_eq!(emit_without_window(window, &None, event()), DispatchOutcome::default());
    }

    #[test]
    fn a_call_made_without_a_window_is_nested_whatever_it_asks_for() {
        assert!(!in_sync_call());
        run_without_window(|| {
            assert!(in_sync_call());
            assert_eq!(update_active_window(|_, _| ()), Err(NESTED.to_string()));
            assert_eq!(update_root_view(|_, _, _| ()), Err(NESTED.to_string()));
            assert_eq!(update_root_view_or_defer(|_, _, _| ()), Err(NESTED.to_string()));
            assert_eq!(invalidate_root_view(crate::retained_tree::Dirty::default()), Err(NESTED.to_string()));
            assert!(with_active_window(|_, _| ()).is_none());
        });
        assert!(!in_sync_call());
    }

    #[test]
    fn a_renderer_call_routes_to_the_nested_error_inside_it() {
        let outcome: Result<(), String> = run_without_window(|| {
            route((), |()| update_active_window(|_, _| ()), |message| message, |()| Ok(()))
        });
        assert_eq!(outcome, Err(NESTED.to_string()));
    }

    #[gpui::test]
    fn the_window_of_a_sync_call_is_given_back_when_the_call_without_one_ends(cx: &mut gpui::TestAppContext) {
        let window = cx.add_empty_window();
        window.update(|window, app| {
            let _scope = ScopeGuard::enter(window, app);
            assert!(with_active_window(|_, _| ()).is_some());
            run_without_window(|| assert!(with_active_window(|_, _| ()).is_none()));
            assert!(with_active_window(|_, _| ()).is_some());
            assert!(in_sync_call());
        });
        assert!(!in_sync_call());
    }

    #[test]
    fn each_drop_effect_is_its_operation() {
        assert_eq!(drag_operation("none"), Ok(DragOperation::None));
        assert_eq!(drag_operation("copy"), Ok(DragOperation::Copy));
        assert_eq!(drag_operation("move"), Ok(DragOperation::Move));
        assert_eq!(drag_operation("link"), Ok(DragOperation::Link));
    }

    #[test]
    fn a_word_that_is_not_a_drop_effect_is_an_error() {
        assert!(drag_operation("copyMove").is_err());
        assert!(drag_operation("").is_err());
    }
}
