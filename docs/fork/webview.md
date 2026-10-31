# `<webview>` and window composition

`<webview>` shows a web page inside a window: Electron's tag, on a system `WKWebView`. GPUI draws with Metal into the window's layer and has no way to put another renderer's output into its scene, so the page lives in a native view that the window composes between GPUI layers. `docs/reference.md` ([`<webview>`](../reference.md#webview)) holds what an app sees; this file holds how it works and why. The list of what does not match a browser is in [deviations.md](deviations.md#webview-and-window-composition).

macOS only. Other platforms get an element that makes no page, and a warning once.

The same surface, without the page, is [`<nativeview>`](nativeview.md): `NativeSurfaceStyle` has `overflow` (`Clip`, which `<webview>` passes, or `Visible`) and `captures_pointer` (`true` for `<webview>`) for it.

## Why a window composition, and why WKWebView

A page in a GPUI texture (an offscreen web engine drawn into an image) costs a copy per frame and loses WebKit's scrolling, text input, IME, accessibility and cursor. A page in its own child window cannot be clipped by a scroll container, layered under a popover or moved in the same frame as GPUI content. A native view among the layers keeps the engine's own input and rendering, and what is left is making GPUI draw around it.

WKWebView and not CEF: it ships with the OS, needs no bundled Chromium (hundreds of MB), no helper processes to sign and no message-loop integration; its input (keyboard, IME, scrolling, find) is AppKit's. CEF would add Windows and Linux, which the fork does not build. The embedder side (`packages/native/src/platform/webview_macos.rs`) is the only code that names WebKit, behind the `WebviewBackend` trait (`custom_elements/webview/backend.rs`), which the test renderer also implements with a scripted page (`test_backend.rs`).

## The placeholder and the layers

A `<webview>` paints a **placeholder**, not the page: `Window::paint_native_surface` records a `NativeSurfacePlaceholder` (bounds, content mask, opacity, uniform corner radius, the element's hitbox) as `PaintOperation::NativeSurface` and the scene gives it a `DrawOrder` from `Scene::insert_bounds` as it does a primitive. Recording it as a paint operation is what makes retained replay, translated replay (the relocation of a scroll), patch frames and the order-reuse logs carry it, with nothing in them special-cased.

A `DrawOrder` is one more than the greatest order of the earlier primitives that overlap it, so of two overlapping primitives the later one has the greater order. Cutting the scene at the orders of the placeholders therefore keeps every overlap right, whichever layer draws which part. With `k` visible placeholders sorted by order `W1 <= ... <= Wk`, the frame is, bottom to top:

```
G0 (orders <= W1), N1, G1 (W1 < orders <= W2), N2, ..., Nk, Gk (orders > Wk)
```

`Gi` are GPUI layers (`G0` is the window's own, the others transparent overlays) and `Ni` the native views. `composition/present.rs` builds this in `present`, which `Window::draw` calls in place of `platform_window.draw`: a window that never made a native surface hands its scene to the platform whole, so a window without a `<webview>` pays nothing.

- **Exact for any number of pages.** There is no heuristic about what overlaps what, and the layers keep paint order, including a page painted after a popover (`zIndex`, `position`).
- **Cost.** One window-sized transparent `CAMetalLayer` per visible page (`Gk` above `Nk`), plus its renderer. Layers are pooled by position (`CompositionState::overlays`), so a frame in which a page moves in the order reuses them. An empty segment draws nothing, but its drawable still presents clear.
- **Hidden cases cost nothing extra.** "A page is visible this frame" is "the finished scene has a placeholder for it". `visibility: hidden` drops the placeholder (`Scene::insert_native_surface` returns when the scene is hidden), `display: none`, skipped `content-visibility` and a detached element paint nothing, and a placeholder whose content mask clips it away entirely is recorded (so replay keeps it) but takes no order. The surface of a page without a placeholder is hidden, not destroyed.
- **Effect groups.** `Scene::batches_in(OrderRange)` is what lets a renderer draw the primitives and the effect groups of one range of orders (`gpui_apple` `draw_orders`); a group belongs to the range of its order. A placeholder painted inside an effect group (an ancestor with `opacity < 1`, a filter, a mask) waits in `pending_native_surfaces` and takes its order when the outermost group closes, so the group draws below the page as a whole. The opacity of the opacity-only groups multiplies into the page's; filters and masks of a group do not apply. Without effect groups (a platform that draws them flat) the flat opacities multiply.
- **Opacity and radius.** The placeholder carries `element_opacity()` and a corner radius (the `border-radius` of the element when all four corners are the same circle, `uniform_radius` in `custom_elements/native_surface.rs`, shared with `<nativeview>`; clamped to half the shorter side). They reach the container view's layer (`opacity`, `cornerRadius` through a path mask of the whole surface, not of the clip).

## Geometry: the container and the content view

Per page the window holds a layer-backed container `NSView` (`masksToBounds`) whose frame is the clip, the bounds cut by the content mask, a rectangle. It holds a content view at the full unclipped bounds, relative to the container, and the `WKWebView` is in the content view (autoresizing to fill it). So a page cut by a scroll container is not resized and not shifted: it is cut. `MacNativeSurface::apply` sets the frames, opacity, mask and `hidden` of a frame's `NativePlacement`, and says whether anything moved.

The frame is applied in the same `present` as the GPUI layers are drawn. `draw_composed` (`gpui_macos/src/window/composition.rs`) sorts the window view's subviews in place (adding and removing views would drop the first responder and tracking areas), applies the placements, then draws each GPUI layer with `draw_orders`. When anything changed (a view moved, an overlay was made or removed, the order changed) the drawables present **with the Core Animation transaction** the views change in (`set_presents_with_transaction`), so the page and the GPUI content around it appear together. After every composed frame of a window with native surfaces `draw_composed` also calls `[CATransaction flush]`, outside the window-state lock: AppKit calls into the views, which take that lock, so flushing under it deadlocks.

Same frame is not exact. With the flush, under heavy CPU load (eight busy processes) 0.8% of captures after a scroll (18 of 2160 over nine runs of 240; 0 to 6 per run) show the page at the new offset and the GPUI marker beside it at the previous one, and none do on a quiet machine (0 of 2160 captures). It is always the GPUI side that is late, never the page, and the next frame fixes it. The e2e allows 3% (7 of 240). The numbers, and what was tried, are in [deviations.md](deviations.md#webview-and-window-composition). The cause is not proven; the transaction is what keeps it small (without it, 20 to 40% of captures, idle or not), but presenting every composed frame with the transaction, waiting for the GPU to complete before the present, an explicit `CATransaction` begin and commit instead of the flush, and drawing the layers before the views move all measured the same within the run-to-run spread.

## Input

A window with a native surface sets a flag on its window class (`COMPOSITION_INPUT_IVAR`) that routes pointer events through an `NSWindow` `sendEvent:` override; a window without one takes the ordinary path.

- **Who the pointer belongs to.** Every GPUI layer above a page is a view over the whole window, so it claims the pointer only inside the hitboxes GPUI gave its layer (`GPUIOverlayView hitTest:`, `overlay_hit_test`); elsewhere AppKit's hit test goes on to the container below and the `WKWebView`. A layer's hitboxes are those inserted into `next_frame.hitboxes` **after the page's own hitbox and before the next page's** (`composition/hit.rs`, `overlay_hitboxes`), without the page's hitbox and its ancestors (an ancestor hits wherever a descendant does and is inserted around the page whatever order the frame ended up in, so it would shut the page out). Pass-through hitboxes and hitboxes with nothing left after their content mask are not claimed.
- **GPUI sees a press first.** AppKit would deliver a press over the page to the `WKWebView`, and GPUI would never learn of hover, a click outside a popup or the focus change. `send_event` therefore runs a press, drag, release or move whose AppKit hit view is in a native container through GPUI's handler first (`dispatch_view_event`), then on to AppKit (`pre_dispatched_event` marks it so the view does not hand it to GPUI again). A pressed button keeps its recipient for the drag and release (`native_buttons`, `gpui_buttons`, `withheld_buttons`). The press is withheld from the page only when GPUI's handler stops propagation; `default_prevented` does not withhold it, because GPUI's own focus transfer to a focusable `<webview>` calls `prevent_default`, which had kept every press from the page.
- **The wheel is the page's.** A wheel is not in `send_event`'s path: over a container it goes to AppKit and the page only, so the page scrolls and a GPUI scroll ancestor does not. A wheel over an overlay's hitbox goes to GPUI, and an overlay hitbox that declines it forwards it to the native view below (`forward_unhandled_scroll`).
- **The cursor is WebKit's.** GPUI does not set the cursor while `pointer_over_native_surface` (`Window::native_surface_at`: the topmost visible page under the pointer that no hitbox above it covers), and the window view's cursor rects leave out the native frames (`cursor_regions`).
- **`pointer-events: none` does nothing on a `<webview>`.** The hitbox of the placeholder is a `Normal` hitbox the element's canvas inserts for itself, so `pointerEvents` on the element only changes its host hitbox: the page still gets the events, the element's own handlers still run, and GPUI content under the page still does not (checked with a click on the test renderer).

## Focus

The keyboard goes to the page when its element gains focus, and back to the window's view (`GPUIView`) when it loses it. `GpuixView::observe_focus_in` notices every change (`focus.rs`) and queues a `PageMove`; `settle_focus` applies it with `webview_set_focused` before the focus events go to JS. One place covers a press on the page, `focus()` and `blur()`, Tab and Shift+Tab, `focusElement`, a click elsewhere and the window going inactive (the page gives the keyboard up; coming back focuses it again).

- **Into the page.** A script or a click is `PageFocus::Focused`: the page keeps what it had focused. Tab and Shift+Tab are `FocusedByTab`: `step_focus` settles with the direction, and `WkBackend::set_first_responder` links the window view to the page for the move and calls `selectKeyViewFollowingView:` (`selectKeyViewPrecedingView:`) on the window, so WebKit's `becomeFirstResponder` sees the direction and focuses the first (last) tabbable element, as it does for an iframe.
- **Out of the page.** The page is a `GpuixWebView`, a `WKWebView` subclass. WebKit ends its tab order with `-[NSWindow selectKeyViewFollowingView:]`, which asks the view for `nextValidKeyView` (`previousValidKeyView` backwards). The subclass answers nil, which keeps the keyboard in the page, and calls the element's `FocusExit`, which spawns `step_focus(backwards)` on the foreground executor, a turn of the run loop later: the page asks from WebKit's callbacks, outside any update of the window. No `WKUserScript` and no key-handling override are involved.
- **Keys inside the page.** While the first responder is a view inside a native surface, `GPUIView performKeyEquivalent:` does not run GPUI's key dispatch (`keyboard_is_native`): it asks `-[NSView performKeyEquivalent:]`, so the page and the main menu get the key. The keys of the page do not reach the parent document, as in an iframe. `flagsChanged:` is not seen either, so GPUI's modifier state catches up with the next event.
- `tabFocusesLinks` is set on the page's preferences: Tab reaches links as well as form controls, as in Chromium.
- **No dim.** `apply_focus_styles` dims every other focusable while a control is focus-visible. `<webview>` clears `dim_path` in its own render unless its style sets `keyboardFocusDim: true` (`exempt_from_keyboard_focus_dim`), since a page is content.

Tab stops: a `<webview>` is focusable like a text control (`needs_focus`); `tabIndex={-1}` leaves it out of the order. The test backend (`TestBackend`) lists `setFirstResponder` in `getWebviewCommands` with the argument `"true"`, `"false"`, `"forward"` or `"backward"`.

## Waking JS for WebKit

WebKit's replies reach the main run loop with a wake that gpuix's run-loop watch does not see, so for an idle app the events of a load and the results of `executeJavaScript` would wait for the next input or frame. `platform/webview_pulse.rs` keeps a repeating timer while a page exists (16 ms for 1.5 s after anything asked a page for work, 250 ms otherwise). An app with a `<webview>` is therefore not idle: about 4 wakes a second at rest. The mechanism, the numbers and the reproduction are in [frames-and-wakes.md](frames-and-wakes.md#waking-js-for-a-webview).

## Events and promises

WebKit calls the navigation delegate and the title observer on the main thread, which on macOS is the JS thread (embedded run loop), outside any frame of the window. The element hands its `EventEmitter` the event callback each time it is built; the page's callbacks emit through `emit_event_full`, so the five events (`did-start-loading`, `did-stop-loading`, `did-navigate`, `page-title-updated`, `did-fail-load`) arrive as ordinary events of the element: at-target only, non-bubbling (`PLAIN_NOTIFICATIONS`, `js/events/dispatch.ts`), with the payload fields `url`, `title`, `explicitSet`, `errorCode`, `errorDescription`, `validatedUrl` on `EventPayload`. `errorCode` is the `NSURLError` of the failure; an `NSError` of another domain keeps its own code. A `src` or `loadURL` that is no URL becomes `did-fail-load` with `NSURLErrorUnsupportedURL` (-1002) from `src`, and a thrown error from `loadURL`.

`executeJavaScript` is a napi function that returns a promise: `ScriptPromise` (`custom_elements/webview/js_api.rs`) holds a `JsDeferred`, and `evaluateJavaScript:completionHandler:` settles it at a later turn of the run loop, with the JSON of the result (`undefined` is told apart from `null`) or the message of the script's error (`WKJavaScriptExceptionMessage`). A script asked before the page exists is rejected, as is one on a platform without a backend. `WkBackend::drop` uses `try_with` on its thread locals: `process.exit` with a live page otherwise aborted, because the drop ran after the thread's locals were gone.

## `file:` URLs and load targets

`webview_url::classify` splits a URL into a `file:` URL, loaded with `loadFileURL:allowingReadAccessToURL:` and read access to the file's directory (a trailing slash: the directory itself), and any other, requested by `NSURLRequest`. A URL without a scheme is an error before WebKit sees it.

## The page is made at first paint

A surface needs the window, so the element makes its surface and its `WKWebView` when its canvas first prepaints (`Shared::prepare`), and a `src` that was set earlier waits in `pending_load`. `retain_state` is a constant: what the build produces is the element's box, hitbox and placeholder, which props and style decide, and the page outlives the build. `destroy` drops the backend and the surface; the surface handle removes the container view in the frame that no longer places it (`CompositionState::retired`).

## The zed side

All of it is in `zed/` on `poc`, as small hunks into existing files plus new modules, so it can be dropped as a unit:

| Where | What |
|---|---|
| `crates/gpui/src/composition.rs`, `composition/{tree,present,hit}.rs` | Types (`NativeSurface`, `NativeSurfacePlaceholder`, `ComposedScene`, `GpuiLayer`, `NativeLayer`, `NativePlacement`, `OverlayHitbox`, `OrderRange`), the surface tree, `present`, the layer cut and the overlay hitboxes. `Window::create_native_surface` and `paint_native_surface` are in `present.rs`. |
| `crates/gpui/src/scene.rs` | `PaintOperation::NativeSurface`, `Scene::native_surfaces`, `insert_native_surface`, `Scene::batches_in(OrderRange)`. |
| `crates/gpui/src/window.rs` | The `composition` field; `present(self)` in place of `platform_window.draw`; `Hitbox::parent_id`; `snap_bounds` and `snapped_content_mask` made `pub(crate)`; the cursor rule over a native surface. |
| `crates/gpui/src/platform.rs` | `PlatformWindow::draw_composed`, `create_native_surface` (defaults draw the scene whole / refuse), and the test recorders `composition_frame(s)`. |
| `crates/gpui/src/platform/test*` | The test platform records composed frames and native surface events (`CompositionFrame`, `RecordedLayer`, `NativeSurfaceEvent`). |
| `crates/gpui_apple/src/composition.rs`, `metal_renderer.rs` | `new_overlay_renderer` (a transparent `CAMetalLayer` sharing the device, sprite atlas and colourspace), `draw_orders(scene, range)`, `render_scene_to_image_in`, `new_layer(device, transparent)`. |
| `crates/gpui_macos/src/window/composition.rs`, small hunks in `window.rs` | `GPUIOverlayView`, `GPUINativeSurfaceView` (the container), `draw_composed`, the `sendEvent:` override, `keyboard_is_native`, `cursor_regions`, the transaction and flush. |

It differs from the open zed PR #62379 (and from gpui-fast, which has the same idea) in how layers are decided: **by paint order** through draw-order thresholds, not by a deferred-draw rule that puts everything after a surface above it, which would draw a background painted after a page on top of it. It has no nesting of native surfaces and no external-GPU surfaces; the PR has both. Everything is additive: `draw` is still the default for a window with no surface. If #62379 lands, drop these commits and re-apply the pieces the PR lacks: the order cut and overlay hitboxes, the `sendEvent:` pre-dispatch, key routing to a native first responder, cursor rects, and the transaction flush.

## Breaking changes

None. `<webview>` is new, and a window without one is drawn as before.

## What consumers can drop

gpuix has no consumers of webviews yet, so there is nothing to drop; the fork exists so one that wraps a system browser in its own window or a child process can use the tag.

## Tests

- **zed `gpui`** `composition/tests.rs` (29, `cargo test -p gpui --lib --features test-support composition`): the surface tree, the placeholder in the scene, the layer split, scroll and replay of a surface, hitboxes over it, a surface in an effect group.
- **zed `gpui_apple`**: two tests of `draw_orders` (only the primitives in a range of orders; the effect groups in it and not the others).
- **zed `gpui_macos`** lib: cursor regions leave out the native frames, a wheel is never shown to GPUI before the native view, a frame partly outside the view. `tests/native_surface.rs` (`harness = false`, a real window, `cargo test -p gpui_macos --test native_surface`): the view tree and its order, frames, what AppKit hit-tests to, that a pointer event reaches GPUI and the native view once each, keys to a native first responder, and pixels of the composited window. It needs a display on which its window is not occluded (`occlusionState` must have the visible bit), or it stops at its first scroll step. `cargo run -p gpui --example native_surface` shows two views, a scroll container, a badge and a popover.
- **gpuix Rust** (`cargo test --lib`): `webview/mod.rs` (corner radius, the dim exemption), `view.rs` (parsing of actions), `test_backend.rs`, `platform/webview_url.rs`, `focus.rs` (the page moves), `platform/webview_macos.rs`. `cargo run --example webview_smoke --features test-support` loads a real `WKWebView` through a load, a title, history and scripts.
- **gpuix vitest** `packages/react/src/__tests__/webview.test.tsx` (47) and `packages/solid/src/__tests__/webview.test.tsx` (25): size, surface geometry through scroll, hidden cases, looks, layers, what AppKit was told, loading, methods, events, focus and the keyboard handoff, retention, on the test renderer's scripted pages. The renderer reads the composed frame the window recorded (`getWebviewSurface`, `getCompositionLayers`, `getWebviewNativeView`).
- **The e2e** and how to run it: [testing.md](testing.md#the-webview-e2e).

Deviations: [deviations.md#webview-and-window-composition](deviations.md#webview-and-window-composition).
