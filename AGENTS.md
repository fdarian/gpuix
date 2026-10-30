# AGENTS.md - GPUIX Codebase Guide

**Read [README.md](./README.md) first** to understand what GPUIX is, the architecture, mutation API, supported elements/events/styles, and the test renderer.

This checkout is the `fdarian/gpuix` fork. [FORK.md](./FORK.md) says what the fork changes and why and maps the topic docs in `docs/fork/`, which cover syncing with upstream and working on the `zed/` submodule; where it disagrees with this file, FORK.md wins. This file orients you to the code as it stands. Upstream's rule that only remorses and monotykamary open pull requests belongs to `remorses/gpuix`; this fork is a proof of concept and nothing in it is a request to merge.

## README is the public API contract

Document every user-facing feature, element, prop, event, renderer option,
public method, and behavior change in `README.md` in the same change. A
changeset does not replace API documentation.

## Behave like the DOM

Events, focus, scrolling, layout, paint and motion follow DOM and CSS semantics. A new prop, event or style gets the browser's behaviour under the browser's name (`onPointerDown`, `tabIndex`, `position: sticky`), not a gpuix-only rule, and not DOM behaviour hidden behind an opt-in. A deviation is acceptable only where GPUI cannot build the DOM model, and it is listed in [docs/fork/deviations.md](./docs/fork/deviations.md), the single list, and linked from the README at the feature. [FORK.md](./FORK.md#the-principle-dom-and-css-semantics) has the rationale.

- New behaviour goes in its own module (`events/`, `layout/`, `css/`, `motion/`, `scroll.rs`, `js/events/`). `renderer.rs`, `style.rs` and `js/host.ts` get wiring only, because they conflict on every upstream sync.
- macOS is the platform that was exercised. Effects (Metal), pointer lock, synchronous dispatch and the OS reduced-motion read are macOS only. The Windows, Linux and wasm paths exist but were not compiled by the people who wrote them, so do not report them as working. The Quad shader edits (HLSL, WGSL) were never built or rendered either. Each gap's reason and what closing it takes is in [deviations.md, Other platforms](./docs/fork/deviations.md#other-platforms).

## GPUIX is a thin layer on GPUI

**Read the GPUI docs and the GPUI source before you write native code.** `zed/crates/gpui`
is checked out in this repository. `gpui::ListState`, `gpui::div`, `gpui::Window` and the
rest are the real API; GPUIX only translates a React tree into calls on them.

Do not invent behaviour on top of GPUI. If a GPUIX element needs something GPUI does not
do, the order is:

1. Find the GPUI API that already does it. Search `zed/crates/gpui` for the symbol
2. Search `zed-industries/zed` issues and PRs. Someone may have shipped it already
3. Fix it in the `zed/` submodule (`fdarian/zed`, branch `poc`) as a normal GPUI change, and bump the submodule
4. Only then, add GPUIX code

**Never paper over GPUI in `packages/native`.** A workaround that re-applies state after
GPUI computed it, patches a value GPUI owns, or reaches around a GPUI invariant will break
on the next submodule bump and is very hard to debug. When such a change is unavoidable,
it must state in a comment what GPUI does, why that is not what GPUIX needs, and which
GPUI call makes it safe.

Prefer the smallest translation. Fewer moving parts is more important than matching any
other framework's behaviour.

The `zed/` submodule is not upstream GPUI. It carries the fork's additions (DOM hit testing, scroll routing, sticky, containing blocks, grid templates, effect groups, `draw_if_dirty`, a layout engine that keeps its nodes across frames, retained subtrees that replay and move with a scroll, culling), so GPUI docs and `zed-industries/zed` issues describe something close but not identical. The source in `zed/crates/gpui` is the truth.

## Built-in components follow Base UI

Headless controls in `@gpuix/react` and `@gpuix/solid` (`select`, `combobox`,
`tooltip`, and any new primitive) should match [Base UI](https://base-ui.com/react/components/select)
first: same split between Root data and children.

For Select, `items` on Root is optional. It is only a label lookup for
`SelectValue` while the popup is closed. Keyboard nav and clicks read the
mounted `SelectItem` children. Do not walk `child.type`. Do not require `items`
for the menu to work.

## Project Goal

GPUIX enables building **native GPU-accelerated desktop applications** using
**React or Solid and TypeScript**, powered by
[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) (Zed's
rendering framework).

Instead of Electron/web rendering, your React components render directly to the GPU via Metal/Vulkan.

```
React (TypeScript)  →  napi-rs  →  GPUI (Rust)  →  GPU
     Your code         Bridge      Native render    Metal/Vulkan
```

## How a change reaches the screen, and where the code is

[README Architecture](./README.md#architecture) and [Mutation API](./README.md#mutation-api) have the picture. In short: the adapter queues host mutations, `applyBatch` (`BatchOp` in `renderer.rs`) applies them to the Rust `RetainedTree` (`retained_tree.rs`), and every GPUI frame `GpuixView::render` walks that tree through `build_element` (`renderer.rs`) into GPUI elements. There is no element tree serialized from JS. Events go back as `EventPayload` (`element_tree.rs`), the one napi struct for every event type, with optional fields.

| Area | `packages/native/src/` | `packages/native/js/` | `zed/crates/gpui/src/` |
|---|---|---|---|
| Pointer events, hover/active, drag | `events/` (`dispatcher`, `listeners`, `state`, `path`, `ids`, `element_drag`, `file_drag`, `pointer_lock`, `synthetic_click`) | `events/` (`dispatch`, `route`, `event-props`, `drag`, `window-target`) | `window.rs` (`Hitbox`, `dom_hit_test`) |
| Window drag regions (`appRegion`) | `events/app_region.rs`, the press and release listeners in `events/listeners.rs` | `StyleDesc.appRegion` in `host.ts` | `window.rs` (`window_control_area_at`), `elements/div.rs` (`window_client_area`) |
| Synchronous dispatch | `events/sync_dispatch.rs` | `renderer-state.ts` (`dispatchSync`), `runtime.ts` | |
| Keyboard, focus, forms, a11y | `events/keyboard.rs`, `focus.rs`, `forms.rs`, `accessibility.rs`, `accessibility/` | `events/keyboard-defaults.ts`, `focus.ts`, `forms.ts`, `aria.ts` | `_accessibility.rs` |
| Scrolling | `scroll.rs`, `scrollbar.rs` | `scroll.ts` | `scroll_routing.rs`, `elements/div.rs`, `elements/list.rs`, `sticky.rs` |
| Layout | `layout/` (`table.rs`, `table_grid.rs`, `collapse.rs`, `display.rs`, `container.rs`), `custom_elements/anchored.rs` | | `taffy/containing_block.rs`, `grid_template.rs`, `elements/anchored.rs`, `third_party/taffy/src/compute/table/`, `collapsed_borders.rs` |
| Paint and style | `style.rs`, `css/` (`table.rs`, `container_query.rs`, `vertical_align.rs`), `color.rs`, `text/inline.rs`, `text/paragraph.rs`, `custom_elements/svg_*.rs` | `svg.ts` | `effects.rs`, `scene.rs`, `style.rs`, `crates/gpui_apple` |
| Motion | `motion/` | `motion-*.ts`, `keyframes.ts`, `presence.ts`, `layout-animation.ts`, `reorder.ts`, `match-media.ts` | |
| Text, selection, search, atomic inlines | `text/` (`atomic.rs`, `selection.rs`, `search.rs`), `syntax/`, `markdown/`, `diff/` | `text-selection.ts` | `elements/atomic_inline.rs`, `text_system/inline.rs` |
| Bounds, observers, automation | `automation.rs`, `observers.rs`, `resolved_style.rs` | `bounds.ts`, `observers.ts`, `automation/` | `window.rs` (`draw_if_dirty`, `publish_layout`) |
| Frame cost: skipped subtrees, culling | `retain.rs`, `tape.rs`, `layout/cull.rs`, `perf_counters.rs`, `slot.rs`, `retained_root` / `build_subtree` in `renderer.rs` | `testing.ts` (counters and switches) | `taffy.rs` and `third_party/taffy` (persistent engine), `elements/retained.rs`, `window/retained.rs`, `Element::cull_outset` |

Adapters: `packages/react/src` (`reconciler/`, `components/`, `hooks/`) and `packages/solid/src` (`host.ts`, `primitives.ts`, `components/`). The rest of the repo: `cli/` (`gpuix new`), `examples/`, `example-app/`, `disktree/`, `docs/`.

**`build_element` resolves a style in a fixed order**, and each stage reads the one before: the base style with the `@container` blocks the containers above select folded in (`layout/container.rs`), then the `hover` / `active` / `groupHover` / `groupActive` / `groupFocusWithin` / `hasHover` / `hasActive` / `hasFocus` / `hasEnabled` / `hasDisabled` / `enabled` / `disabled` fold from `PointerState`, the focus state and the form controls (`RetainedElement::control_disabled`; `formControl` marks a `div` that stands in for one) (the order is in `resolve_pointer_states`; `focusWithin`, `hasFocusVisible` and `focusVisible` are applied after it by the focus code), then CSS transitions, then `motion` `animate`, then motion values, then CSS animations (which skip a property a CSS transition is running on, as a transition beats an animation; they advance first, inside `resolve_declared_style`, so a property one drives starts no transition), then the `display: contents` overlay and `anchor-size()`. A stage that needs to change a style copies it first: `RetainedElement.style` is a shared `Arc<StyleDesc>`. The result, with the focus variants laid over it, is reduced to a `Resolved` record in `RetainedTree::resolved` (`resolved_style.rs`). Every walk over the retained tree (`elements_at`, stacking, `containing_block`, observer clips, key scrolling) reads that record through `RetainedTree::resolved_of`, `position_of`, `overflow_of` or `display_of`, never `element.style`, which is only the declared style and the answer for an element that was never built.

## Shared JavaScript runtime

Framework-neutral TypeScript belongs in `@gpuix/native`, not in an adapter.
`@gpuix/native/host` owns host contracts, mutation batching, renderer-local
IDs, root ownership, event routing, observers, and text search.
`@gpuix/native/testing` and `@gpuix/native/automation` own the shared native
test wrapper and automation client. These subpaths must not load the `.node`
addon at import time, except `testing`, which constructs the native test class.

React and Solid keep only framework scheduling, context, JSX integration, and
components. React preserves its public testing and automation subpaths as thin
adapters or re-exports. Do not copy shared runtime code back into an adapter.

`@gpuix/native/runtime` owns the desktop pump. On macOS it wakes when the main run
loop has work (a main-queue block, a run-loop timer, or input: an observer on
AppKit's event thread posts to the main queue, since nothing else wakes a main
loop that libuv owns), and the display link runs only on frame demand, so an idle
window gets no wakes and no timer. `tick()`
clears `pending` and then calls `run_loop_pumped()` to re-arm the watch. `tick()`
must stay non-blocking so Node.js and Bun I/O can run. Explicit `frameMs` uses
timer polling.

The native package is ESM-first. Generate `index.js` with
`napi build --platform --esm --js index.js`. `index.cjs` exists only for the
documented Hermes CJS runtime. Keep both loaders beside the `.node` file.

Solid applications use `jsx: "preserve"`, `jsxImportSource: "@gpuix/solid"`,
and `@gpuix/solid/preload` in `bunfig.toml`. The preload runs the Solid
universal compiler and selects Solid's client runtime. Do not add Vite or a
wrapper CLI.

## Events: one hit test, one dispatcher, DOM propagation

Pointer, mouse, click, drag and wheel events are not wired per element. They travel in three steps.

1. **Hit test, in zed.** `events::enable_dom_hit_testing` turns on `Window::set_dom_hit_testing` for the window (off by default, so other GPUI users are unaffected). The hit is the topmost hitbox that is not `HitboxBehavior::PassThrough`, plus the chain of its parent hitboxes (`dom_hit_test` in `window.rs`). `PassThrough` is `pointer-events: none` and everything inside an `inert` subtree: never the target, but still in the chain when a descendant is hit. A `clipPath` cuts a hitbox's area (`Hitbox.clip`). A deferred layer (`<anchored>`) is its own hitbox root. Every host element's root is a hitbox under `events::ids::host_element_id`, and `host_hit_chain` maps GPUI's chain back to retained element ids. A text node has no hitbox, so GPUI's target is the element holding it. `sample` in `events/listeners.rs` then runs `events::inline_target::refine`, which puts the inline boxes under the pointer in front of the chain: `text::inline_hit` keeps each painted paragraph's `TextLayout` and the boxes around every piece, and a point inside a glyph's cell (`index_for_position` is `Ok`) yields the innermost `<span>` / nested `<text>` and the boxes around it. Blank space in a paragraph belongs to the container. The dispatcher sees ordinary host ids, and the path comes from the retained parents.
2. **Dispatcher, in native.** `events/listeners.rs` registers window-level GPUI listeners once per frame from one canvas, not one per element. Each builds a `PointerSample` and `dispatcher::dispatch` turns it into `DomEvent`s in Chrome's order (boundary events, then pointer before mouse, then click). `dispatch` only reads the `RetainedTree` and writes `PointerState`, so its rules are unit-tested without a window. An event's path is the target and then its retained ancestors (`events/path.rs`), so an `<anchored>` popup bubbles to the element it is declared under, not to the window root. **A type is produced only if something on the path listens** (`path_listens`) **or `window` / `document` does**: a new event type needs its prop in `js/events/event-props.ts` (which sets the native listener) or it is never emitted. The JS `window` and `document` targets keep their listeners in JS, so `syncWindowEventListeners` (`js/events/window-target.ts`, called by both adapters beside `setWindowKeyEvents`) sends the set of types they listen for to `PointerRequests::set_window_events`, and `Emitter::emit` produces those types for any path. Without it a `pointermove` over bare background (target: the root element, which has no listener) never reaches JS. A `pointermove` / `mousemove` over no element at all but inside the window (and not under an `appRegion` area) is a window-aimed `DomEvent` with an empty `path` (`Emitter::emit_to_window`), sent only when the set holds the type; it reaches JS as a payload with no `path` or `target`, like a key with nothing focused (`isWindowMove` in `js/events/keyboard-defaults.ts`). Keys keep their own flag (`set_window_key_events`).
3. **Propagation, in JS.** `dispatchDomEvent` (`js/events/dispatch.ts`) walks `payload.path`: capture from the root down, the target, then bubble. `runRootEvent` (`events/keyboard-defaults.ts`) decides which events take that walk.

**Native payloads carry element ids; handlers get elements.** `EventPayload.target`, `path` and `relatedTarget` are ids on the wire. `createDomEvent` resolves them through the root's element source (`ActiveRoot.elementById`), so a handler's `target`, `currentTarget` and `relatedTarget` are the objects a `ref` gives, `composedPath()` replaces `path`, and `elementId` stays the numeric id. `currentTarget` is the `window` or `document` object during their phases. An event aimed at the window has a `null` target (gpuix has no `body` / `html`), and `relatedTarget` is `null` when there is none or its element is gone. A path whose target the root no longer has is a stale event: `handlesEvent` drops it. Window-level callbacks (`onSelectionChange`, `onPointerLockChange`, ...) get a `RootEvent` built the same way; `onEvent` is the raw tap and keeps ids. A handler type is `DomEvent` / `HostMouseEvent`, never `EventPayload`.

`PointerState` (`events/state.rs`) holds the hovered and active chains, press targets, click count and pointer capture. `build_element` reads it for `:hover` / `:active` and the group variants, and a chain change on an element that declares one of them rebuilds the view (`Output::restyle`). Host elements do not use GPUI's `.hover()` or `.group_hover()`: the dispatcher is the only source of hover for them.

**Pointer capture is explicit.** Nothing captures on its own, so an element with `onMouseDown` and `onMouseMove` hears only what happens over it. A gesture that must survive leaving the element, its parent or the window calls `setPointerCapture` on the press. The call is recorded in `PointerRequests` (shared between the renderer handle and the view), so it is in force when it returns: `hasPointerCapture` answers for it and the next pointer input reads it before it builds an event, firing `gotpointercapture` first, as the DOM does. `cancelDrag` works the same way. `requestPointerLock` and `exitPointerLock` also take effect in the call on macOS (`GpuixRenderer::settle_pointer_lock` runs `pointer_lock::apply` through `update_window_or_defer`, the test renderer through `update_view_or_defer`), so `pointerLockElement` is current in the handler that asked; the `pointerlockchange` and `pointerlockerror` they cause take the ordinary queue and reach `document` and `window` listeners after the handler returns. Elsewhere the next frame applies the lock request.

**An element drag is gpuix's own state machine** (`events/element_drag.rs`), not GPUI's `on_drag`. GPUI's `AnyDrag` ghost (`App::active_drag`, crate-private in zed) is what a drag image would be built on; gpuix never sets it.

**A press, a release, a wheel tick, a keystroke and every event of a drag of files from the OS reach JS synchronously on macOS and in the test renderer**, through `sync_dispatch::emit` (`events/sync_dispatch.rs`). The JS handler runs inside GPUI's input dispatch and returns `{ defaultPrevented, propagationStopped }`, which is how `preventDefault` cancels a native default: `mousedown` cancels the focus change and the input editor's own focus (`window.prevent_default()`; `pointerdown` does not, as in the DOM), `wheel` cancels that input's scroll, `keydown` cancels the editor action and the text insertion, and a `click` inside a `<label>` cancels its activation. A file drag's `dragover` returns its `dropEffect` (`{ dropEffect }`, `"none"` unless cancelled with an allowed effect), which `listeners.rs` hands to `window.set_drag_operation`; gpui_macos answers `draggingEntered` / `draggingUpdated` with it, so the OS cursor and the drop follow the JS rule. All of a drag's events go down this one path (`PointerDispatch::file_drag`), or a queued `dragleave` could end the next drag's session. The focus events of a script's own call (`focus()`, `blur()`, `focusNext()` and the other focus moves) are synchronous too, from `focus::settle_focus`, so a handler that moves focus hears `blur` and `focus` before its next line. The focus a press, `autoFocus` or a frame causes is synchronous too: `defer_mouse_down_default` settles the press default, `GpuixView::commit_auto_focus` (called by `applyBatch` when a batch turned `autoFocus` on) focuses at commit, and a frame schedules `settle_focus` after the draw (`settle_focus_after_frame`), because `render` holds the root view. Every other event (moves, enter/leave, `scroll`, transitions) takes the ordinary queue and reaches JS later. On Windows and Linux GPUI runs off the JS thread (`gpui_windows` and `gpui_linux` have no embedded loop, unlike `gpui_macos`), so `emit` queues too and every default proceeds; blocking the UI thread on JS there would deadlock against the JS-to-UI commands that block on a reply (the header of `sync_dispatch.rs`; [deviations.md](./docs/fork/deviations.md#other-platforms) lists the two ways out). The test renderer is always synchronous, so a passing test says nothing about those two platforms.

Consequence for anything a handler can call: while a sync call is in flight the dispatching `Window` and `App` are stashed (`with_active_window`). GPUI's input callback (`AnyWindowHandle::update`) does not lease the root view, and the test renderer dispatches the same way (`cx.simulate_event`), so every renderer read and write works from a handler and answers current values. A new napi method must go through `sync_dispatch::route` and the `update_*` helpers (the test renderer's `update_view` / `window_call`), never `with_test_state` or a lease of its own: the app is already borrowed there, and a nested `GpuixView` update panics inside GPUI. `update_root_view` returns `VIEW_BUSY` only when the view is genuinely being updated further up the stack (a nested update); `update_root_view_or_defer` runs a write after that update instead. Only calls that start another input or drive the test app (`simulate_*`, `advance_time`, `capture_screenshot`) cannot run from a handler, and say so.

**A handler that throws does not stop the dispatch.** Errors from `dispatchDomEvent` and from window/document listeners go to the renderer's `onError` option, else to `reportError` (uncaught). Nothing is rethrown into the caller of a simulate method, so a test that expects a throw asserts on `onError`.

Rules that sit in the dispatcher and are easy to miss:

- `inert` is inherited like `pointer-events: none`, and a descendant's `auto` does not undo it (`Inherited::descend` in `renderer.rs`). There is no hover, group state or listener in an inert subtree.
- A disabled form control gets no `click`, `auxclick` or `dblclick` (`RetainedTree::in_disabled_form_control`, checked in `dispatcher::release`). `element.click()` and the accessibility click skip it as well (`events/synthetic_click.rs`). Pointer and mouse move events still reach it.
- `mousedownoutside` is gpuix's own event, delivered to each listener whose logical subtree does not contain the press target, and it does not bubble.
- `contextmenu` fires on mousedown, not mouseup, for the secondary button and, on macOS only, for ctrl+primary.
- **A cancelled `pointerdown` suppresses the press's `mousedown`, `mousemove` and `mouseup`** (`PointerState::mouse_events_suppressed`), nothing else: `contextmenu`, `click`, `mousedownoutside`, the focus default and the hover events fire. The dispatcher only returns events, so the flag is set by `PointerDispatch::press` (`listeners.rs`) from the sync outcome of the `pointerdown` it delivers, and that loop skips the `mousedown` after it; `dispatch` skips `mousemove` and `mouseup` from then on. It is per pointer: set by the first button's `pointerdown`, covers buttons that join it, cleared when the last is up, by `cancel_press`, and when an element drag begins (its `pointercancel`). Where `emit` queues the outcome is never "prevented", so nothing is suppressed.
- **`appRegion` is not hit testing.** It inherits (`Inherited::descend`) and applies only when the window is `titlebarTransparent` (`app_owns_titlebar_drag` is set from the same option, so AppKit does not drag the top strip itself). Each host element with a value registers its box with GPUI (an inline `<span>` flattened into a paragraph has no host box, so `text/inline_region.rs` registers one pass-through hitbox per line fragment of its border box, after the container's own), and `Window::window_control_area_at` picks the last registered box under the pointer in paint order; z-order, `pointerEvents`, clipping and `inert` do not take part. A primary press in a drag region never reaches the page: the capture listener starts `Window::start_window_move` (macOS needs the mouse-down being dispatched) and, on Windows, prevents the default but leaves the press unstopped so `WM_NCHITTEST`'s `HTCAPTION` does the drag. With no button held the pointer over a region is outside the window as far as the dispatcher is concerned (`app_region::hides_pointer`). The test renderer records the window actions (`takeSimulatedWindowActions`) instead of performing them, and treats a window as frameless only when it was made with `titlebarTransparent: true`.

Not in the dispatcher: keys (`events/keyboard.rs`), focus events (`GpuixView::observe_focus`), the VoiceOver press (`apply_a11y_click`), `scroll` (queued by the paint listener), `onLayout` and observer reports (read from the published boxes after the draw by `observers::frame_sweep`), and transition and animation events (`emit_bubbling_event` in `renderer.rs`, which gives them `target` and `path` so they bubble).

**The reports of a frame go out in the order of the DOM's rendering steps.** `scroll` events (paint order), then `onLayout` and `ResizeObserver` (`onLayout` is `ResizeObserver` for an element), then `IntersectionObserver`; within a phase in document order, `onLayout` before the observers of one element, and an element that is gone after the ones that are there. `report_scroll` queues into `PaintReports::pending_scrolls` and `observers::sweep` drains it first (a deferred flush does when nothing listens for boxes), so the order is the code's and not the order of two deferrals. Only a painted frame sweeps: a layout-only pass (a bounds read, the `scrollTop` setter's lay out and clamp) reports nothing, as a forced layout in a browser delivers no `ResizeObserver` callbacks, and the next frame's sweep reports what changed against what was last delivered, after its scroll events and with that frame's sizes. `js/observers.ts` then runs the callbacks as the DOM does: every `ResizeObserver` callback, then every `IntersectionObserver` callback, each in observer creation order, with an observer's entries in the order its targets were observed. `frame-event-order.test.tsx` pins it.

## Focus, keyboard, forms and accessibility

- **Focus.** GPUI owns who has focus; `focus.rs` owns what the DOM says about it: what is focusable (`tabIndex`, never a listener), tab order, `:focus-visible` and the four focus events. All of it is observed from one place, `GpuixView::observe_focus`, because gpui runs per-element focus listeners in creation order, so per-element subscriptions fire `focus(new)` before `blur(old)`. Do not add one.
- **The element source** (`installElementSource` in `js/events/window-target.ts`) is the id-to-element lookup of a root as the adapter's ref gives it: React `Container.instances`, Solid `HostRoot.elements`. Each adapter installs it beside `syncWindowEventListeners`. `document.activeElement` reads it, `null` with nothing focused; with several roots the one that heard a focus event last and still has a focused element wins. Event dispatch reads it too (see [Events](#events-one-hit-test-one-dispatcher-dom-propagation)). It lives on `globalThis` because a renderer's state outlives a `bun --hot` re-evaluation of the module. The test window is one per thread, so a second `TestRenderer`, and a live renderer whose frame loop ended, stops its predecessor reporting focus and listing animations in `document.getAnimations()` (`forgetWindowSource`; the native side would answer for the other window); that root keeps naming its elements.
- **A press moves focus.** `focus::defer_mouse_down_default` runs after the `mousedown` handlers and uses the dispatcher's press path, not the topmost hit. A `preventDefault` from any handler on the path cancels it. A press with nothing focusable on its path blurs. It settles the focus events before it returns, also when it was cancelled (the element's own focus transfer may have moved focus already), so a handler that mounts an `autoFocus` element on `mousedown` loses focus to the pressed element unless it calls `preventDefault`, as on the web.
- **Every focus change reports itself.** `settle_focus` is the one place events leave the focus outbox; the sync emit makes them reach JS before the caller returns. Whoever moves focus calls it: `change_focus` (script), the press default, `commit_auto_focus` (the `autoFocus` of a batch, called from `applyBatch`, so JS handlers run inside `applyBatch` and `createMutationQueue` takes its queue before the call), and a frame (`sync_focus_handles` notes the move and defers `settle_focus` past the draw). The outbox dedupes: `FocusState::observe` reports a move once, so the frame after any of them sends nothing.
- **A focusable `<span>` has no GPUI element**, so its paragraph anchors the handle (`text/inline_focus.rs`): a zero-size `div` under the span's `host_element_id` that tracks it, placed after the glyphs in `Paragraph.boxes` order, which gives the dispatch node, the tab stop in tree order and the a11y node. Bounds still come from `text/inline_bounds.rs`; a second recorder under the id would overwrite them. GPUI cannot find the pointer in a zero-size box, so `focus_on_press` focuses the span after the press has been dispatched. A focusable ancestor's own GPUI handler would otherwise win and prevent the default. A focusable nested `<text>` is a block, an element of its own.
- **Keys.** `events/keyboard.rs` sends one `keydown` per keystroke, from a GPUI keystroke interceptor that runs before key bindings, to the focused element. JS bubbles it and then reaches the `window` and `document` listeners (`js/events/window-target.ts`). Defaults (Tab, Escape for the top layer, the scrolling keys) run in JS after every handler, in `runKeyDefault`, and a `preventDefault` skips them. `key` and `code` are DOM names (`events/key_names.rs`). On macOS `code` is the physical key from `NativeKey::key_code` through Chromium's table (`events/key_codes.rs`) and `key` is `NativeKey::layout_char`, the layout's character with shift and option applied, as in Chrome; without a native key (other platforms, a simulated key name) both derive from GPUI's `key` assuming a US layout. The legacy `keyCode` / `which` are computed beside them (`key_names::legacy_key_code`, tables in `key_codes.rs`) from Chromium's `KeyboardCodeFromNSEvent`: the produced character decides letters and punctuation, the physical key decides the digit row, keypad and named keys, and `Process` is 229. There is no `keypress`, so no `charCode`. A simulated press that names a code (`KeyA`) carries a native key; one that names a key (`a`, `Enter`) does not. On macOS a key the input method takes first (composing, or an IME source taking printable keys) reaches JS as a `keydown` with `key` `"Process"`, sent before the input method gets it from an observe-only zed event (`PlatformInput::ImeKeyDown`, `App::observe_ime_keystrokes`) in `install_key_down`; its outcome is ignored, so `preventDefault` cannot stop the input method. `isComposing` is the focused input's marked text. The input editor fires `compositionstart` / `compositionupdate` / `compositionend` itself from its `EntityInputHandler` calls (`custom_elements/input.rs`), through `emit_bubbling_event`, `compositionstart` / `compositionupdate` before the `change` of their edit and `compositionend` after the `change` of the final one, as Chrome orders them (that `change` has `isComposing` true). The `change` carries `inputType`, `data` and `isComposing` from `InputKind` (`custom_elements/input.rs`): every editor action names its kind and `edit` / `replace_and_mark_text_in_range` pick the composition's. There is no `beforeinput` and no `onInput`; `change` is the DOM `input`. They are queued like `change`, so only the test renderer's `simulateComposition` orders them with the sync keydown and keyup.
- **Forms.** `forms.rs` owns validation and submission (`invalid`, then a bubbling cancelable `submit`), both sent through `sync_dispatch`. A `<label>` (`accessibility/label.rs`) activates its control after a `click` whose path contains it, unless the click was prevented or landed inside the control.
- **Accessibility.** `aria-*` props map to AccessKit in `accessibility/aria.rs`. A keyboard or VoiceOver activation is a DOM `click` with `detail` 0, `pointerId` -1 and no pointer (`events/synthetic_click.rs`).


## Scrolling: native routing, and nested scrollers work

Overflow follows CSS (`scroll.rs`): `scroll` and `auto` scroll, `hidden` scrolls only from code, `clip` does not scroll (both axes `clip` is gpui `overflow_x_clip().overflow_y_clip()`, not `overflow_hidden`: taffy treats that as a scroll container with a minimum size of 0, which shrinks flex items), and overflow never changes `display` or wraps children. `scroll::apply_container` turns a div into a scroll container and attaches the scrollbar (`scrollbar.rs`), which is an overlay or, when macOS "Show scroll bars" is Always (`ScrollerStyle::Classic`), a bar in a gutter the container reserves through the `scrollbar_width` and `without_scrollbar` styles in zed. `ScrollHandle::padding_box` is the scrollport less that gutter. `TestRenderer.setScrollbarStyle` forces the style under test.

The wheel is two separate things.

- **Which scroller moves** is zed's `scroll_routing.rs`. One wheel event scrolls one scroller: every scroller under the pointer is offered the event in bubble order, innermost first (`div` scrollers and `gpui::list()` alike, so `<virtual-list>` and a scrolling `<diff>` take part), and the first that can use any of it, on either axis, takes all of it. What it cannot use is dropped, not handed on in the same event; chaining to the outer scroller happens between events (`TouchPhase::Started`, 500 ms idle, or a phase-less wheel after the pointer moved), so a test that expects an outer scroller to move needs a second gesture (`"started"` phase). A scroller's range is CSS scrollable overflow over all descendants, not only direct children: an absolute descendant counts only if the scroller is its containing block, and fixed, anchored and deferred ones never do. A gesture latches to the scroller that took its first delta, trackpad momentum included (a mouse wheel latches until 500 ms of silence or until the pointer moves), and `overscroll-behavior` decides what happens to delta a scroller cannot use. This is not `stop_propagation`, so `onWheel` listeners above still run. A scroller whose hitbox is `PassThrough` (`pointer-events: none`, inert) is never a candidate, even while something non-pass-through under the pointer is.
- **The `wheel` DOM event** goes through the dispatcher like any pointer event, to the hit target and along its path, and it runs **before** any scroller claims, through sync dispatch. A `preventDefault` in either phase calls `window.prevent_default()` and the routing skips that event. It cancels that input only, not the rest of the gesture, and only where dispatch is synchronous. `onWheel` fires for every tick; `onScroll` is a different event, fired on the scroller that moved for any offset change (wheel, scrollbar drag, keys, `scrollTo`), non-bubbling, and not fired by `<virtual-list>` (`onVisibleRange`).

`<code>` and `<markdown>` code and table bodies are `overflow_x_scroll().restrict_scroll_to_axis()` divs. They do not set `allow_concurrent_scroll`, so `div.rs` takes its older branch for them: horizontal deltas only, no chaining, no latching. Keep `restrict_scroll_to_axis()` on any x-only scroller that native code builds, or a vertical wheel over it moves it sideways instead of the page.

Keys that scroll are a JS default action (`runKeyDefault`, then `renderer.scrollByKey`, `scroll::scroll_by_key`): the innermost scrollable ancestor of the focused element that can move that way, else the scroller under the pointer. A focused input keeps its keys.

**Scroll anchoring (`overflowAnchor`)** is zed's `elements/scroll_anchoring.rs`, Blink's `scroll_anchor.cc`. `css/overflow_anchor.rs` parses the key (an invalid value is ignored, not inherited, not animatable) and `layout::style::apply` hands it to every box: descendants need it to be excluded as candidates. A tracked scroller notes itself in the layout engine at `request_layout`; `TaffyLayoutEngine::compute_layout_in` lets each pick or keep its anchor *before* the layout is computed (`ScrollAnchoring::before_layout`, Blink's `NotifyBeforeLayout`), while the nodes still hold the boxes of the pass before and already hold the new tree and styles, and `step` compensates in the scroller's prepaint, after the offset is clamped. That order is what makes a removed, hidden or `none` anchor, a toggled scroller, and a wheel scroll in the frame of a prepend anchor as Chrome does; do not move the pick back to the end of a prepaint. Scroll events of an adjustment come from the painted frame, never a layout-only pass (`scroll_reporter` skips `is_layout_pass`), so a forced read sees the new offset and the event runs a frame later, as in Chrome. `fixtures/scroll-anchoring-{scenes.ts,oracle.*}` and `scroll-anchoring.test.tsx` pin it against headless Chrome; a scene Chrome does differently for a reason that is not worth copying is in `KNOWN_DEVIATIONS` there.

`position: sticky` is placed natively in prepaint (zed `sticky.rs`) against the nearest scroll container's scrollport, so it follows the scroll with no JS in the loop and cannot tear.

A `<diff>` flows with its parent by default, so a parent list can be the only scroller. Pass `scroll` plus a bounded height only for a dedicated viewer. Do not give `<diff>` a bounded height inside a parent scroller just so it can virtualize.


## Bounds: every element reports its own box at paint, and reads are never stale

`getByTestId(..).click()`, `getBoundingClientRect`, `onLayout` and the observers all read the border box recorded by `crate::automation::track_host_bounds`. It is the border box **as layout computed it** (`ElementBounds.x/y/width/height`: unsnapped, cut down to 1/64 px like Chrome's `LayoutUnit`, with `painted_x/y` as the snapped origin that `offsetX` is measured from), not the box that is painted and hit. `Window::layout_unsnapped_bounds` can only be asked in prepaint, so `capture_layout_box` (an `on_prepainted_layout` listener, also run for a culled subtree) keeps it for the painted-layout listener of the same element, which records it in paint through the tape: one write per element per frame, and a replay that moves a subtree by a whole device-pixel offset keeps it exact. A scroller also measures `Window::layout_unsnapped_scrollable_overflow` there (`content_width/height`), which is what `scrollWidth` rounds; `clientWidth` and `scrollWidth` are integers (`scroll::metrics`), the other members fractional. A collapsed table is flagged (`ElementBounds.collapsed_table`, from the renderer through `track_host_bounds`): its `border` is the outer halves, so `scroll::metrics` does not take them off its client size, as Chrome does not. The largest scroll offset (`ScrollHandle::max_offset`, the wheel, `scrollTo`) is not measured from the snapped boxes: `clamp_scroll_position` takes the same unsnapped reach and client size (`Interactivity::scroll_extent`) and rounds each to a device pixel, so it does not depend on where the scroller sits. An inline span's box is its paragraph's rows (measured from where the glyphs are painted) shifted by `TextLayout::unsnapped_offset`, the paragraph's laid-out origin less the glyphs' (`inline_bounds::record`). An atomic inline is placed from the paragraph's laid-out origin in both axes (`TextLayout::atomic_origins`) and keeps the fraction in every box it reports (`Window::with_unsnapped_element_origin`: the element offset is the device pixel the origin rounds half up to, as Chrome paints it, and `layout_unsnapped_bounds` adds the rest back). The listener is the element's own `on_painted_layout`, so it adds nothing to the tree: a canvas child per element would double the nodes gpui lays out, prepaints and paints. `<img>`, `<svg>` and `<anchored>` use `track_own_bounds`, the same hook without the selection-start region. A leaf cannot hold a child, and wrapping it in a div would move the layout box (the wrapper becomes the flex item and the image loses intrinsic sizing and corner clipping). `custom_surface` attaches the right one for a custom element. `on_painted_layout` takes one listener, so `onScroll` shares it through a `PaintReporter`. `onLayout` and the observers have no listener: `observers::frame_sweep` reads their boxes after the draw from the same registry, which also holds the padding and border the node was laid out with (`Window::layout_insets`), state styles and percentages resolved. `clientLeft` and the observers read those, never the element's base style.

Both record during **paint**, and `bounds_frame_reset` clears the registry during paint too. Never move any of it to prepaint: `gpui::list()` prepaints a speculative row range, then rolls the window back through `Window::transact` and prepaints a different one, so a prepaint-recorded box can belong to a row that never reached the screen. A `visibility: hidden` element paints nothing but still runs these listeners, so it keeps its box (`Window::visibility` tells a listener which it is, and `track_host_bounds` records no selection region for a hidden one); a box that is missing means `display: none`, a virtual list row outside its window, or a removed element. A `<virtual-list>` records its own box through the same `track_host_bounds` (zed's `List::paint` and a layout-only pass run `on_painted_layout` as `Div` does), but its rows are separate layout roots stacked at fractional offsets (`prepaint_at_fractional`), so it has no scrollable overflow to measure: its scroll metrics come from `ListState::dom_scroll`.

**A bounds read is never stale, and never paints.** `readBoundingClientRect` (`js/bounds.ts`) first sends the mutations the adapter has queued (`flushPendingMutations`: Solid flushes in a microtask, React at commit), then native `GetElementBounds` calls `Window::draw_if_dirty` in zed before it answers. Scroll metrics and `scrollIntoView` do the same. `draw_if_dirty` is a layout-only pass, as a browser forces layout: it renders, lays out and prepaints, publishes what paint would have published (element and inline-span bounds, scroll metrics) from prepaint through `Window::publish_layout`, which is what the next painted frame's `observers::frame_sweep` reads (a layout-only pass reports no `onLayout` or observer entry itself), throws the frame away and leaves the window dirty, so the display link draws once however many reads came first. It skips the retained subtrees ([below](#frames-skip-what-did-not-change-retained-subtrees)) that the last presented frame recorded as unchanged and republishes their bounds from their tapes, so a read after a write costs layout and prepaint, not a build of the page; it records no frame and takes nothing from the rendered one, so the draw after it still replays. A subtree it builds leaves a *layout record* (gpui `LayoutRecord`, tape in `TapeStore::laid_out`, replayed with `RetainedPhase::LayoutRecorded`) that the next layout-only pass skips with, so a page read many times before its first frame builds each subtree once; a draw ends every layout record. A clean window runs no pass and answers from the last frame, and one pass serves every read until something invalidates. Only a draw advances time: a layout-only pass samples everything time-dependent (motions, springs, transitions, animations, scrollbar fades, virtual rows) at `GpuixView::frame_instant`, the instant of the last draw, so a read from a `change` handler cannot advance the animation it listens to. A scroll offset write on a scroller or a `<virtual-list>` (`App::notify_scroll_only`) leaves the scroll metrics current and the boxes stale: `Window::draw_if_scroll_metrics_dirty` (`getScrollMetrics`, the `scrollTop` setter's lay out and clamp) runs no pass after only scroll writes, `draw_if_dirty` (bounds) does, and the next frame builds a list's rows at the new offset; a list's metrics are read from its `ListState` (`ListState::dom_scroll`). It works the same inside an input handler, where nothing may swap frames. Anything new that a read can observe must be published from prepaint when `window.is_layout_pass()`, or it is stale after a read: `on_painted` and `on_painted_layout` listeners already are, a custom paint-time publisher queues itself with `publish_layout`. `Window::paint_if_dirty` is the draw for a caller that needs painted output (`getPaintedText`), and synthetic pointer input from `automation.rs` draws first because it is hit tested against a drawn frame. Test renderer: a dirty window is drawn inline whenever effects flush, which hides every pass; `setInlineDraws(false)` plus `drawFrame()` give a frame the way the display link does, and `getLayoutPassCount()` counts the passes.


## Clipped content is culled: build and layout stay, prepaint and paint go

A subtree that lies outside the content mask of its clip is not prepainted or painted, as a browser skips paint for clipped content. A browser keeps style and layout, and so does this: the build and the taffy layout of a clipped box still run (a clipped box costs about two thirds of a visible one), because JS can read its box. To cut the build and layout too, cull in JS (see README, "Pannable surfaces must cull") or skip the content with `contentVisibility` (next section).

- **gpui decides from the real layout** (`prepaint_unlifted_child`, `Window::lies_outside_the_mask`, `TaffyLayoutEngine::painted_extent`). A child that says `.cullable(outset)` (`InteractiveElement::cullable`, `Element::cull_outset`) is culled when its border box, and the boxes of what overflows it (descendants of a node that clips an axis contribute nothing on that axis), inflated by `outset` and a pixel, do not touch `content_mask().bounds`. A subtree with a `fixed` or detached node, or an `absolute` node whose containing block is above it, has no extent and is never culled. Never culled either: a child that is lifted, escapes its clip, is sticky, or has an image cache or a prepaint order; any child while a hitbox captures the pointer. An accessibility tree does not stop culling.
- **A culled subtree runs `Element::prepaint_culled` and `paint_culled`** (default: the full `prepaint` and `paint`, so an element nobody adapted is only not faster) under an empty content mask, so whatever an adapted element draws or inserts is dropped and unhittable. `Div` keeps what readers see: `on_painted`, `on_painted_layout`, `on_prepainted_layout` and `on_children_prepainted` listeners, scroll metrics of the scrollers in it (the full style is computed for those only), the tab stops and focus-path entry of focusable elements, layout-pass publication, and the positioned frame an escaping descendant is placed by. The accessibility tree keeps all of it, as clipped and off-screen content stays in the DOM's: nodes with their bounds and relations, focus, active descendant and action listeners are reported as for an unculled box, and culling does not wait for the tree to be off (macOS turns it on for any AX client, for good); an element whose `a11y_synthetic_children` reads its prepaint state must do a full prepaint while `Window::is_a11y_active`. It has no hitbox, listener, tooltip, cursor, style from state, or overlay painter. `StyledText` only sets its bounds; the canvas under a text still runs in full, which is where the selection registry, find and inline-box bounds are recorded.
- **gpuix says how far a subtree inks past its boxes** (`layout/cull.rs`, `BuildCtx::subtree`, filled bottom-up by `build_element`): shadows, outlines, blur and drop-shadow of the style and of every state style, a font size of text. `position: absolute|fixed|sticky` anywhere in the subtree, a custom element, a virtual list, a `layout` (FLIP) wrapper and a collapsed table are `block()`ed: no ancestor is cullable. A clip with both axes clipping drops what is built below it from the reach, not the block. A paragraph with an atomic inline, an inline box that paints or takes space, a cursor, region or focus anchor is blocked. `Measured` forwards the child's `cull_outset`. A new element that paints outside its box, or is placed by something other than its parent, must `block()`.
- **Tests**: `Window::culled_subtree_count` (`getCulledSubtreeCount`) says something was skipped, so a test of what a culled box still reports cannot pass by nothing having been skipped. The test window pretends a screen reader is connected (`setA11yActive`), so the tests run with an accessibility tree and still see subtrees culled; `getA11yTree` has each node's `bounds` (device pixels). gpui's `clipped_content_stays_in_the_accessibility_tree` compares the tree culled and unculled node by node. Guard: `examples/clipped.perf.test.tsx`, with the screen reader on and off.


## `contentVisibility` skips the content of an element, and `skips` is the one place that decides

`layout/content_visibility.rs`. `contained` rewrites the final style of any element whose value is not `visible` (`overflow: clip` on the axes left `visible`, a stacking context, `position: relative` for a static element so it contains its absolute descendants), once in `build_element` and again in `Resolved::of_style`, so the gpui element and the walks over the retained tree (`position_of`, `overflow_of`, `lift_z`, `creates_context`) agree. `skips` says whether the content is built: `hidden` always, `auto` when `Relevance` (`tree.relevance`) says the element is not near and holds no focus. A skipped element is built and painted itself and builds none of its children (`build_host_container`), with size containment from `contain_skipped` (gpui `contain_intrinsic_width` / `height`, zed style): `contains_size` is the part that stays when a read forces the content. Layout containment is the zed style flag `contain_layout`, set by `layout::style::apply` for any value but `visible` (skipped or not): an independent formatting context in taffy (no child margin collapses through it), and the containing block of `position: fixed` descendants, which `layout/containing_block.rs` (`contains_fixed`, `reaching_ancestors`) and the hit walk of `retained_tree.rs` read as zed's `Escape::Viewport` placing by the nearest contained frame. A percentage height under the `contain-intrinsic-height` of an `auto` height is auto (taffy `height_from_containment`), in a block only. `Retain::skip_content` drops the records and dormancy of everything below, so a subtree that shows again is built whole and no loop below keeps an ancestor on the spine.
- **`auto` is decided from the published boxes.** The first answer is the element's prepaint listener (`watch`: bounds and content mask, a near element asks `request_redraw_before_present`); later ones come from `Relevance::sweep`, run by `observers::frame_sweep` after each painted frame. Near is the border box outset by 1.5 viewports, cut by the clips the `IntersectionObserver` uses. Elements whose answer changed are a source of the frame (`Sources::relevance`, `Relevance::pending`) and part of the element's key (`container_key`); the sweep invalidates with `invalidate_after_commit`, never `Window::refresh`, which turns retained replay off for every root. `Relevance::remember` holds the content box for `contain-intrinsic-size: auto`; `contentvisibilityautostatechange` is emitted by `sweep_relevance`: the elements present in the first sweep of a window that has drawn something (`Relevance::drawn`, set by `frame_sweep` for a frame with content and no `auto` element, `tree.has_content()`) are first told `true` (`Swept::locked`, document order) and then their state, as Chrome does for a page at load; later ones once. A subtree that holds an `auto` element is culled only past the margin (`Subtree::reach(proximity_reach)`).
- **A geometry read inside a skipped subtree lays it out for real** (`renderer::read_after_layout`, which `getElementBounds` and `getScrollMetrics` go through, as Chrome's `ScopedForcedUpdate`): the skipped ancestors go in `Relevance::force`, a layout-only pass runs, the boxes are read, the set is cleared and the window invalidated so the next painted frame skips them again. The reads that follow share that pass while `Window::layout_stamp` is the one `Relevance` recorded (`ForcedRead`: a read that needs only forced ancestors already in the set runs no pass, one that needs more forces the union in one pass); a style write, scroll write, resize, a frame in which the box of the outermost skipped ancestor or of one above it moved, a frame in which an element on the chain of the read started or stopped a layout-capable animation, a write to a transform on the chain, or any other layout pass ends it; a paint-only write, a painted frame that moves no box and a layout animation that moves boxes elsewhere do not (`LayoutStamp` counts layout invalidations, animation frames, the taffy `box_generation` and the boxes of nested engines, and the host asks `Window::box_stamp`). A read after a paint-only write to a transform runs `Window::draw_for_transforms` so its boxes carry the transform written. The pass must leave no trace: `built` and `remember` ignore a forced element, the animations the element already ran are sampled for the boxes and rewound, and a run the pass makes stays, held (`is_forced_by_ancestor`, `rewind_point`, `hold`; `animate_style` sets no `animating_opacity` for a run it did not step), nothing else is recorded. The test renderer's `getBuiltElementBounds` is the read without that pass (the box of the last layout, `null` for what is not built), which tests use to tell what is built.
- **Skipped `auto` content is in the accessibility tree and in the tab order, from the retained tree** (Chrome keeps both). A11y: with the tree active, `build_host_container` gives a skipped `auto` element the nodes of everything below it (`accessibility/skipped.rs`: `exposed` walks the children in document order (`contents::flatten`, not `order`), leaves out `display: none`, `aria-hidden`/`inert`, `visibility: hidden` nodes, parts of a paragraph and the content of a nested `hidden`; `host` hands them to gpui as `a11y_synthetic_children`, nested with `A11ySubtreeBuilder::push_nested`). Each node comes from `apply_accessibility` on a throwaway `div` read back through `Element::a11y_role`/`write_a11y_info`, so a role, a name, `aria-*`, a relation or a `<text>`'s value is computed by the code that builds them for a built element; keep it that way and add nothing the built path does not have. The element gets a `Group` node when it has no role (`apply_hosting`); `hidden` and `aria-hidden` expose nothing. Tab: `focus/skipped.rs::target` compares the stop gpui would pick (`Window::next_tab_stop`) with the focusables of skipped `auto` content (`focusable_ids` that have a skipped `auto` ancestor and no `display: none` or `hidden` one), by `(tab-index group, document path)`; `step_focus` focuses the one that comes first through `change_focus`, reads its box with `read_after_layout` and scrolls to it (`scroll_element_into_view_at`). Not built, so it has no tab stop: the frame after builds it, because it holds the focus.
- **Find (`highlight`) searches skipped `auto` content and not `hidden` content** (Chrome's `FindBuffer`). `GroupList::collect` leaves out what is below a `hidden` host element (`content_visibility::found_in`, the declared value: `same_paragraphs` moves `search_revision` when it changes) and tags the groups below an `auto` one with its id, so the matches in skipped content are in the total and in `MatchSet::held` / `autos`. Numbering stays in document order: an `auto` element that skips its content announces its matches in its prepaint listener (`watch`, `search::announce_skipped`, one `MatchId::Skipped` entry of `count` ordinals at its `DocPos`), so the built matches after it keep their numbers. The `auto` elements around the active match (a skipped one's entry, a painted match's `autos`) come out of `search::settle_frame`, which `observers::frame_sweep` calls in the paint of every frame (a frame with nothing to wash numbers nothing otherwise) and hands to `Relevance::set_found`; `wanted_skipped` treats them as it does the focus, so the element builds in the frame after and stays built for as long as its match is active. Matches in `<code>`, `<markdown>` and `<diff>` text exist only while the element renders, so those in skipped content are not counted. Guard: `content-visibility-find.test.tsx`.
- Guards: `content-visibility-a11y.test.tsx` (tree and Tab, `fixtures/content-visibility-a11y-oracle.*`, regenerated with `bun`), `content-visibility.test.tsx` (`hidden` against headless Chrome at 1/64 px, `fixtures/content-visibility-oracle.*`), `content-visibility-auto.test.tsx` (oracle scenes from `fixtures/content-visibility-auto-oracle.ts`, regenerated with `bun`, and counters). Run the suites also with `GPUIX_RETAIN_VERIFY=1`. See [observers-and-content-visibility.md](./docs/fork/observers-and-content-visibility.md#skipped-content-in-the-accessibility-tree-and-in-the-tab-order) (skipped content in the accessibility tree and in the tab order).

## Frames skip what did not change: retained subtrees

A frame does not build a subtree whose inputs did not change; gpui puts back what the subtree prepainted and painted the frame before, on layout nodes that stayed. [retention.md](./docs/fork/retention.md#why-three-layers-37) has the design in three layers, [the invalidation rules](./docs/fork/retention.md#invalidation-rules) and [where a frame differs from a full build](./docs/fork/retention.md#where-a-frame-differs-from-a-full-build). This is where it lives and what not to break.

- **Layout engine** (zed `taffy.rs`, vendored `third_party/taffy`): nodes persist, found by (id scope, ordinal) and compared before they are set, so taffy's cache survives. `NodeMeta::requested` holds the style an element asked for, which every layout rewriter reads and restores. Freed at the end of a presented draw by reachability. A change inside a relayout boundary (Blink's `ObjectIsRelayoutBoundary`: a fixed-size clip box or a box with size and layout containment, `TaffyTree::is_relayout_boundary`) lays out that box again and not the ancestors: `mark_dirty` stops there, but the generations above it still move, so recordings and extent stamps notice. `nodesLaidOut` and `nodesSized` in `getLayoutCounters()` are the cost of a change (a leaf at depth d lays out d + 1 nodes, whatever the fan-out); the engine fuzz grows boundaries, and a boundary that answers differently falls back to the chain to the root.
- **`gpui::retained(id, key, build)`** (zed `elements/retained.rs`, `window/retained.rs`): a frame with the same key does not call `build`, and replays the recorded prepaint and paint if the `ReplayKey` (place, clip, text style, scale, ...) and the root's layout generation match, else builds late in prepaint. Why a recording cannot replay is `NonReusable`, counted by `getRetainedCounters`.
- **The planner** (`retain.rs`): `build_element` calls `Retain::enter`, which skips an element when its last build was eligible, its entry key (`subtree_revision`, `inherited_fingerprint`, shape) is unchanged and the frame's deltas (`find_deltas`: pointer and focus chains, scroll offsets, measurements, links, custom `retain_state`, running motion) do not name it or a descendant. `retained_root` hands gpui a `retained(..)` whose closure is `GpuixView::build_subtree` with a `FrameEnv` snapshot. `Exclusion` lists what a replay cannot repeat (scrollers, virtual lists, `onScroll` listeners, `<code>`, `<markdown>` and `<diff>`). Only the kinds that follow the clock or the arrival of something make a tick of a loop elsewhere decline (`Exclusion::varies_with_time`: highlights, an `<svg>` whose paint would start a raster job, virtual lists, layout animations); `<code>`, `<markdown>` and `<diff>` paint the same in a tick, so a page with one patches its ticks. `onLayout` and the observers are not on it: `observers::frame_sweep` reads the boxes the tape republishes. Motion that runs on an element with no box (`display: none` or below one, `box_generation::is_hidden`) names no unit and blocks no tick: no build reaches it and no frame paints it. A root of one element is skipped only if it is costly to build (`Entry::costly`: text, image, svg, custom element), because replaying a plain box costs what building it does (`MIN_ROOT`, 2). A root may hand boxes to the stacking context around it: a lifted element that is a root of its own is a hole (`retained(..).lift_z(z)`, never `relocatable`) and any other, such as an `absolute` box placed by a positioned box inside the root, is a fragment of the root's recording; `tape.rs` records what the context paints for each (`Phase::Lifted`, from `LiftedBegin/End/Paint`). A root whose absolute box has its containing block above the root is not eligible, and a root that lifts something out of it is never moved by a scroller. Its notes between `enter` and `exit` (`pointer`, `focus`, `scroll`, `exclude`, ...) record what a build read. `exit` settles the element (what it adds up to with what is below it: size, lifts, escapes, holes, the epoch and token its record gets) and says whether gpui is to record the build: an element built whole that `skippable` would take the record of next frame, with no motion running in it, is wrapped by `recorded_root` as `retained(id, token, built element)`, so the frame after replays it where it used to build it again to record it (the first build of a page, a root built because it changed). A scroller that has no layout yet is taken to scroll the axes its style scrolls, with the window for a viewport (`Scroller::of`), so the first frame records its roots to be moved too. The colour a root inherits is in its key only if the build read it (`Record::reads_colors`): `ColorContext` keeps where its `current` was declared, reading it (`current()`, or `note_read()` for what takes the colour from gpui's text style: text, svg, custom elements, motion) notes that place, and `color::reads::Frame`, opened by `enter` and closed by `exit`, says whether a place above the element was noted; a `color` an element declares cuts the read for what is below it, a `currentColor` or `color-mix()` of the inherited one reads it. A root that reads none is wrapped `ignoring_text_color` (gpui builds, records and replays it under a colour of its own), unless it lifts.
- **The tape** (`tape.rs`): a replay runs no listeners, so what they write into gpuix's per-frame registries is recorded by the `Taped` root. The `on_replay_translated` hook does not write it again: it publishes the lists it puts back (`tape::publish`, `delta` away from where they were recorded) and the registries read them when asked (`tape::Layered`, `tape::delta_of`), so a replay costs a step per root. What the stacking context paints for a box the root handed it lies outside the root's paint, so gpui brackets it with `LiftedBegin(rel)` and `LiftedEnd` for every recording open around it, and the tape keeps those writes in a slice that a replay plays at `LiftedPaint(rel)`; a hole plays its own tape (`Paint`, or `LayoutPaint` in a pass that only lays out). Effects of building (measured elements, background image requests, the axes of a query container a style read, which the container compares with its size after layout) are `BuildEffect`s kept with the element's record and repeated by a frame that skips it. A pass that only lays out records a tape of its own (`Tape::laid_out`; the `Taped` root brackets the publications its prepaint queued with two more, so their writes land on it), kept in `TapeStore::laid_out` and `Stored::laid_out` in `automation.rs` so that it never replaces the tape or the box of the recording the draw after it replays.
- **Patch frames and dormancy** (`retain/patch.rs`, zed `window/patch.rs`): a tick whose only change is how running animations paint is drawn by building the *units* that moved (`Retain::begin_unit` and `end_unit`: the outermost element whose animations all paint only, at most 256 elements) alone and having gpui paint them again in the frame shown (`Window::request_tick`, `repaint_in_place`). `Retain::preview` takes only time-driven changes, and any decline draws the tick whole in the same draw. A tick with no unit and nothing to settle (every change is in skipped or sleeping content: `running_motion` leaves out a motion whose element has no box, so a subtree that skips its content has no running source) is an empty tick: the clocks are stepped, nothing is built or presented, and `getPatchCounters().emptyTicks` counts it; one that still has elements to enter the dormant set is drawn whole, as `Dormancy`. An element whose animations paint only and that the last frame walked culled is dormant (`Dormancy::holds`, `Retain::dormant`): not built, its clock advanced without a style (`advance_clock`, `advance_dormant`) and nothing drawn (`dormant_tick`), which runs only when a dormant source has something to tell: `CssAnimations::dormant_wake` (the end of a delay and of an active phase, an iteration only where the event path listens for `animationiteration`), `CssTransitions::dormant_wake`, `MotionState::wake` (a spring wakes at its rest instant) and the motion values' wake, merged by `dormant_clock_wake`; a loop nobody listens to costs nothing until it is seen. A unit may sit under any positioned, translucent or lifted ancestor (gpui records a root prepainted inside an open lift as `Placement::Deferred` and paints it in the surroundings the stacking context gives it), and so may an element that is itself `position: absolute` (`end_unit` only refuses an absolute box inside a unit whose containing block is outside it): gpui records its root for a patch and never skips it (`Recorded::root_escapes`), a whole frame builds it every time and settles its node against its containing block, and a patch repaints it where that frame left it. A `fixed` element is no unit (`Exclusion::Fixed`). A tick is requested whenever something runs, units or not, so every tick drawn whole is counted: `Retain::preview` names the element that is no unit with its `Refusal` (`Record::refusal`, set in `finish_session`). A tick steps only what moves: `GpuixView::step_motions` the motions that moved at the last sync (while the `MotionInputs` a sync reads but the clock are the same) and are drawn, or sleep and are due (a frame built whole does the same through `sync_motions_for_build`, and a gesture reaches only the motions it flipped), `step_values` and `MotionValueStore::step` the values that move or were written (`BindingIndex` holds the bindings parsed once per revision), and zed's `Scene::rebuild_from_operations` takes the draw orders from the scene's `order_log` while the bounds it inserts are the same, so a patched tick costs its units and not the page (`motionsSynced`, `motionValuesSampled`, `boundsInserts` count the exceptions). `getPatchCounters` says how ticks were drawn, `GPUIX_PATCH_OFF` times a run without patching; see [patch-frames.md](./docs/fork/patch-frames.md).
- **Roots under a scroller move with it** (`Scroller` in `retain.rs`): a root of at most two viewports in a scroller (or a clip that script scrolled) is handed to gpui as `retained(..).relocatable(overscan)`, so a scroll puts its recording where the scroll put it instead of building it. A bigger box is not a root; its children are. `Retain::relocation` watches `getRetainedCounters` (`lazy_builds` against `relocations`) and stops planning on moves for a few frames when gpui builds the roots anyway (rounded clips, positions kept by a listener). Roots and the clips around them are gpui's to judge (`NonRelocatable`, `RelocationMiss`). A canvas in a root says `keeps_no_position()` when its closures hold no position, or the root cannot move.

What a contributor must not break:

- **Every input of a build is in the key, in a delta, or excludes the element.** State set from JS moves `subtree_revision` (every mutation goes through `RetainedTree`). State of the window or the app that a build reads is a field of `Inherited` and of `inherited_fingerprint` (a size test fails when one is missing), or a delta in `find_deltas` with a note in the build, or an `Exclusion`. A hole is a stale frame that only verify mode or a user finds.
- **Where a new style field or state goes.** A field of `StyleDesc` is covered by the revision and `StyleDesc` equality; it needs `overlay_fields!` (`events/state_styles.rs`) to take part in state styles. Something inherited goes in `Inherited`, `descend` and `inherited_fingerprint` (custom properties and `direction` are there). A new `PointerStates` flag goes in `pointer_bits` (a test lists every flag) and `any()`. State that is read from the tree during a build (`:disabled` and `:has(:disabled)` through `ControlStyles`) needs no delta, because `set_custom_prop` moves the revision of the control and its ancestors. A new text input to a measure is in the `TextRun`, `TextLayoutOptions` or `InlineBox` that gpui's measure key hashes. What a verified build compares is `Retain::digest`: the host's gpui style and its `Resolved`, the paragraph's runs, insets and decorations; a new build output belongs in one of them.
- **A state laid over an element is applied on the folded style.** `Resolved`, `forms_context` and the lift a retained root hands gpui are made from `FocusStyles::folded`, so `apply_focus_variants` decides the lift again from the folded style (`stacking::apply_folded_lift`), and `overlay` keeps the flag that says the element is a flex or grid item. A focus variant that moves `position`, `zIndex` or an effect group is guarded by `retained-parity.test.tsx` (`elementsFromPoint` and pixels).
- **Every registry write from a prepaint or paint listener goes through `tape::write`**, or `tape::write_in_layout` when a layout-only pass makes it too (bounds). A write that holds a position moves by the offset it is given (`ElementBounds::translated`, start regions); a handle that holds one (a `TextLayout`) is moved by gpui, and custom state registers with `Window::note_relocated`. A build-time effect goes through `tape::note_build`.
- **A new custom element implements `CustomElement::retain_state`**: a hash of everything it keeps for itself, which becomes part of the shape and a delta. `None` excludes it, which is right for anything holding state only gpui knows (a scroller), and for nothing that is merely waiting: an element whose image is on its way (`<svg>`: a box nobody has painted, a job running) gives the word of that state, which stays while it waits, so it is built every frame only for the one frame that finds its word changed, and not by every tick of every loop on the page (`getRetainDebug().exclusionWhy` names why a built element excluded itself). The text editors count their notifications in an epoch (`TextEditorState::changed`, every notify of the editor but the blink's); the blink of a focused caret is `CustomElement::follows_clock`: the element is a unit, and the blink asks for a tick (`Window::request_tick`) when the phase changes instead of notifying, so a page with an input patches its ticks. The tick of a phase marks the caret due (`CustomElement::clock_due`, cleared by the next paint of the editor) and `Retain::preview` touches an element whose only dynamic is `Dynamic::Blink` only while it is due, so a tick of a loop elsewhere does not build the input. The timer of the phase is armed by the prepaint of the editor (`TextEditorState::keep_blinking`) only while the caret's box meets the content mask and the editor's bounds, so a caret nobody sees asks for no frame; the input still `block()`s culling, since its key handlers are registered by its paint.
- **A property that reaches a text only through gpui's `TextStyle` is not in the key**: gpui compares the text style around a retained root before it skips it. The colour is the exception, see the planner. A new reader of the inherited colour calls `ColorContext::current()` or `note_read()`; a read that goes around them is a stale colour that only verify mode finds. A property that is baked into a descendant's own build (an inherited underline offset, a length) goes in `inherited_fingerprint`.
- **A keyed measure hashes every input its closure reads** (`request_keyed_measured_layout`); a key that misses one freezes a stale size. A request without a key is a node no later frame gets back (the editors' measure is keyed by their shaped text, so they can be retained and patched). The insets of a span are in it through `InsetSpec::text_key`. A text with atomic inlines keeps its measure through `AtomicInlines::digest` (the boxes' engine persists on the text's layout node, see `AtomicCarry`); it has none, and is measured every frame, when a box is made anew each frame (a keyless request, a measured leaf as the box's root).
- **A gpui element that a scroller may move keeps no position it can't give back.** Something that keeps where its element was past the frame (a drag, a keyboard click, a tooltip's bounds test) calls `Window::note_positioned_capture()`; a `Canvas` whose closures keep none says `keeps_no_position()`; state outside the frame that holds a position registers with `Window::note_relocated`. Painted geometry rounds translation-covariantly (a shift by whole device pixels moves the result by that shift), or a moved replay differs from a build.
- **A property is in `AnimProp::paints_only` only if it cannot move or size a box.** Units and dormancy rest on it: a wrong entry draws a stale layout. A JS `setStyle` that changes only these keys (and the `borderColor` shorthand, `StyleDesc::differs_only_in_paint`, which compares every other field whole, so a new field is a layout key until it is listed) is `Dirty::Paint` from `RetainedTree::take_dirty` and notifies with `notify_paint_only`; any other mutation is `Dirty::Layout`. A new kind of running animation must say whether it paints only, like `CssAnimations::paints_only` and `MotionState::paints_only`, because `runs_paint_only` asks every one that runs on an element.
- **`accessibility::links::compute` reads only what `RelationRegistry` (on `RetainedTree`) lists**: the `<label>`s, the elements with an `aria-*` id reference, and the elements by DOM `id`. A prop or a mutation it starts to read must reach `RelationRegistry::reads`/`prop_changed` or `text_changed`/`restructured`, or the links stay stale, and it must never walk `tree.elements` (a tree with no label and no reference costs O(1); `relationVisits` in `getLayoutCounters()` pins it).
- **A retained root needs an element id of its own** (`absent_host`, not `Empty`): layout nodes are named by path.
- **Run the suites with `GPUIX_RETAIN_VERIFY=1` before landing** a change to what a build reads or a paint listener writes (`packages/react`, `packages/solid`, `examples`): it builds each replayed frame twice and compares them, and a mismatch is `[retained]` (gpui) or `[retain]` (gpuix) on stderr and fails `dispose()`. `GPUIX_RETAIN_OFF=1` builds everything, for timing. Switches and counters are listed in [testing.md](./docs/fork/testing.md#switches-and-counters); `GPUIX_RETAIN_DEBUG=1` with `getRetainDebug()` says what the last frame built and why.
- Guards: `packages/react/src/__tests__/retained-replay.test.tsx` (the full pipeline) `retained-fuzz.test.tsx` (random interaction against a build) and `retained-round6.test.tsx` (every text, box and motion property against retained frames, on an ancestor, the page, the card and a box in it), `retained-parity.test.tsx` (every field of the parity pass: direction, line boxes, inline boxes and spans, mask-border, flex-basis keywords, control states, collapsed tables, focus variants, `display: contents` wrappers, gestures; its case runner is `retained-cases.tsx`), `retained-scroll.test.tsx` (rows a scroll moves against the same page with retention off), `retained-lifts.test.tsx` (holes and fragments), `retained-spine.test.tsx` (a motion whose way up to a replayed root is never built: under a `display: contents` wrapper, in an svg), `retained-frame-sources.test.tsx` (everything that changes per frame inside a root a frame could replay, retention on against off), packages/solid's `retained-cull-wrapper.test.tsx` (the demo's cull wrapper: a `display: contents` region and a provider around animating cards), `retained-observers.test.tsx` and `frame-event-order.test.tsx`, gpui's `engine_fuzz_tests.rs` (`GPUI_ENGINE_FUZZ_SEEDS=a..b` runs the ignored long run) and the retained tests in `elements/retained/` (`hole_tests`, `fragment_tests`, `relocation_tests`, `regression_tests` with the fuzz of random pages: `RETAINED_FUZZ_OFF=lift,round,...` takes features away to find which a failure needs, `RETAINED_VERIFY_DUMP=dir` writes the first differing frame), and the perf tests below.


## Layout: CSS rules between the retained tree and GPUI

`layout/` holds the DOM and CSS rules that taffy and GPUI do not give for free; `renderer.rs` only calls into it. `layout::style::apply` is the layout half of `apply_styles`.

- **`position` defaults to `static`** (`build_element` sets it when the style has none). `absolute` resolves against the nearest positioned ancestor: zed's `taffy/containing_block.rs` rewrites those nodes after a layout pass and lays out again. `fixed` resolves against the window. So `top` / `left` without `position` do nothing, and code that needs an absolute child needs a positioned parent (`AnimatePresence mode="popLayout"` is one).
- **Custom properties (`--name`) are no `StyleDesc` field names.** `css/covers.rs` collects the keys while a style object is parsed (`Recorder`), `StyleDesc::custom_properties` holds them (layered by `overlay_fields!`), they inherit through `Inherited::custom` (`css/custom_properties.rs`), and two things read them: `style()` container queries, and `var()` in the transform properties (`transform::resolve_vars`, substituted into the text before the parsers; a value that holds `var()` is computed at the element that declares it). No other property substitutes `var()`. A change of a custom property alone is paint-only unless an `@container` rule is in the tree, and a build that read one makes it part of its retained key (`Record::reads_custom`).
- **Clip and scroll follow the containing block.** An `absolute` box is clipped and scrolled only by the ancestors at or inside its containing block, and a `fixed` box by the window alone, or by the nearest layout-contained ancestor (`contentVisibility` other than `visible`). zed does it: a positioned `Div` (also `<anchored>`, and a layout root such as a list row) records the content mask and element offset its children see, `Element::escapes_to` makes `Drawable` prepaint and paint the box under that frame instead of the inherited one, and deferred draws carry the frame. Native code that walks ancestors to decide what clips or moves an element (`observers.rs`, `scrollIntoView`, key scrolling, `RetainedTree::elements_at`) uses `layout::containing_block`, not `parent`. A new gpui element that is a styled box needs `escapes_to` (`StyleRefinement::escape`), and a wrapper that places something by its child's bounds forwards it. A `position` that only a state style sets needs nothing in zed: `build_element` folds `hover` / `active` / group states into the style before the element is built, and `apply_focus_styles` / `apply_focus_within` write the focus variants onto the same `base_style`, so `escapes_to` and `lift_z` read the state's value. The retained tree only has the declared style, so `build_element` passes the folded `position` (`FocusStyles::position` adds the focus variants) to `track_host_bounds`, `ElementBounds::position` carries it, and `elements_at` and the scroll chains read it through `containing_block::PaintedPosition`, one frame behind a state change like the bounds. `escaping_count` counts an element that escapes in any state (`may_escape`), so no clipped subtree is pruned while one exists. Not covered: `IntersectionObserver` builds its chain before the frame paints and reads the declared position, as does an `<input>`. An effect group (opacity, filter, mask, clip-path) around an escaped box composites with the mask it started with widened to the box's: `Drawable::paint` paints an escaped box through `Window::with_escaped_content_mask`, which calls `Scene::widen_groups` (recorded for `replay`). The group's own primitives keep their masks, so only what a filter spreads of the clipped part passes the clip. Residual: a `layout` (FLIP) glide is dropped for an absolute or fixed box.
- **Paint order is CSS stacking contexts, decided in one place** (`layout/stacking.rs`). zed lifts every positioned `Div`, `<img>`, `<svg>` and `<virtual-list>` (`Element::lift_z`) out of its parent into the nearest `Div` flagged `stacking_context()`, which paints and hit-tests them after its in-flow children by `(zIndex, tree order)` (negative ones above its own background, below the in-flow content); tab order, dispatch and a11y stay in tree order. `build_element` flags exactly what `creates_context` says: the window root, a positioned element with a `zIndex`, `fixed`, `sticky`, and every effect group (`opacity` < 1, `filter`, `backdropFilter`, `maskImage`, `clipPath`: zed cannot see these, so a missed one lets a descendant escape the effect) and every element with a transform (`transform`, `translate`, `rotate` or `scale` other than `none`; a non-atomic inline box takes none), which `lift_z` lifts at 0 like an effect group and which is the containing block of its absolute and fixed descendants (`contain_layout`, and `containing_block::contains_fixed` for the retained walks). A non-positioned element with `opacity` < 1, `filter`, `backdropFilter`, `maskImage` or `clipPath` paints in the z=0 layer in tree order and forms a stacking context. A flex or grid item takes its `zIndex` without a position, lifts into the nearest stacking context, and forms one when `zIndex` isn't `auto`. `<anchored>` content, each virtual-list row and the anonymous block of a root or row `display: contents` are contexts too (`stacking_root` in `BuildCtx`): a deferred layer and a layout root begin with no context around them, and nothing is lifted out of a row. Without a context (below `visibility: hidden`) `paint_rank` orders the children of one parent. `RetainedTree::elements_at` models the same contexts on the retained tree with the same predicates (`lifted_z`, `is_forced_context`, `creates_context`), so a new rule goes in `stacking.rs` and nowhere else. An element gpuix positions only for layout (the text wrapper, the containing block of an absolute canvas, anchored's placement boxes) must be `.unlifted()`, or it rises above its later siblings; a gpui wrapper element forwards `lift_z` with `escapes_to`. While an animation or transition of `opacity` is current (CSS animation or transition, `motion`, a moving motion value) the element is an effect group at any sampled value, as Chrome's `will-change: opacity`: `animate_style` sets `StyleDesc::animating_opacity`, `has_effect_group` reads it, gpui paints a group even at 1 and at 0 (`Styled::has_active_opacity_animation`), and `Retain::enter` refuses to skip a record made on the other side of a run's start or end (`Entry::lift_run`), so the lift a retained wrapper carries is the lift of the build that replaces it. A running CSS animation or transition of a transform property is the same for the transform: `animate_style` sets `StyleDesc::animating_transform`, which `Resolved.transformed` reads, so a tick never flips the stacking context or the containing block. A Motion track or motion value does not set it: Motion's keys all at their defaults are `transform: none` (no context, no containing block), and the flip at a run's ends is a layout frame. Not modelled: `hover` / motion values that move an element into a context (`elements_at` reads the declared style).
- **No box.** `display: none` builds nothing (no box, no hitbox, no a11y node); `display: contents` splices the children into the parent (`layout/contents.rs`, taffy has no such value); a root or virtual-list row has no parent, so `build_contents_container` puts its children in an anonymous block (the list measures the row as that block; the block carries the element's `role` and `aria-*` under a non-host id, so it is no hit target; a spliced contents element has no gpui element to carry a node). `RetainedElement::needs_focus` is false for a contents element, as in Chrome. An empty or whitespace-only text node generates no box and takes no `gap` slot (`text/inline.rs`). An inline box that holds no text (an empty `<span>`) in a block container is `Child::EmptyLine`: a childless zero-size div that text, search, a11y and selection never see, which gives a table cell a child with no baseline (Blink's empty line box), so the cell's baseline is the bottom of its content. One with inline-axis margin, border or padding is not an empty line but a U+FFFC placeholder in a paragraph (`text/empty_box.rs`, `Piece::placeholder`, gpui `InlineContent::Empty`): it takes its insets, makes the strut line and has a fragment, see Text rendering.
- **Lengths are a linear expression, resolved at apply time.** `css/length.rs` has `LinearLength` (px, %, em, rem, vw, vh, vmin, vmax, cqw, cqh, cqmin, cqmax) and `LengthContext` (font size, root font size, window size, the content box of the nearest query containers), which rides in `Inherited` next to `ColorContext` and is passed to `apply_styles*`; em and rem need it, so never resolve a length at deserialize. `css/dimension.rs` is the parser (`parse_linear_length`, `calc()`), reused by every length field. `layout/style.rs` turns them into GPUI lengths (`definite()`, `linear()`); a length plus a `%` becomes a calc node whose storage is zed's `taffy/calc.rs` `CalcArena` (owned by `taffy.rs`), evaluated by the patched taffy in `zed/third_party/taffy` (which also resolves a block container's percentage padding against the parent's width). `width` / `minWidth` / `maxWidth` accept `min-content`, `max-content` and `fit-content`, measured by a pre-pass in zed's `taffy/intrinsic.rs` (and `taffy/natural.rs` for the natural width of `width: auto` motion), each costing extra layouts per flagged node. `flexBasis` takes those and `content` too, but not through the pre-pass: `gpui::FlexBasisKeyword` becomes a tag of taffy's `Dimension` (`content()`, `min_content()`, `max_content()`, `fit_content()`), which `determine_flex_base_size` in `zed/third_party/taffy/src/compute/flexbox.rs` reads (every keyword is `content` along a column; `fit-content` needs the container's inner size, known only there). A `width: fit-content` on a row item with an `auto` basis is the basis `fit-content` (the pre-pass sets it); `flexBasis: fit-content(<limit>)` is not CSS and still goes through the pre-pass. Sizes also take `anchor-size()`. Grid tracks take lengths and the keywords (`layout/grid.rs`).
- **Container queries read last frame's layout and check the guess.** `layout/container.rs`: a build evaluates `@container` blocks and `cq` units against the content box the previous layout recorded (`ContainerStore`), each container's `on_prepainted_layout` compares its new box with what was read, and a mismatch calls `Window::request_redraw_before_present`, which zed honours in `Window::draw` by dropping the pass before paint and building again (`set_max_draw_passes`, 4 here). Anything a build publishes outside the frame therefore runs once per pass and must be idempotent; paint-time state is only touched by the presented pass. The `display` a block gives an element is what its parent decides its box by (`display: none`, an inline box, an atomic inline, a block): a host's build settles it for its children and the inline boxes among them (`layout::container::settle_children`, stored in `RetainedElement::container_display`, read through `RetainedElement::display`) before the paragraph planner runs, and a change asks for another pass. The `order` a block gives is settled the same way (`container_order`, read through `RetainedElement::order`), so a flex or grid parent sorts its items by it in the same frame. Planner code and the table role (`layout::table::part`) read `display()`, never `style.display`, and the sort reads `order()`, never `style.order`. `containerType` sets size containment on the taffy node (`contain_inline_size`, `contain_block_size`): the content-based size ignores the content, so a container without a width is as wide as its padding and border. Any element with a `containerType` or a `containerName` is a `ContainerScope`, but only one that CSS can size contain (`layout::container::can_be_sized`: a block, an atomic inline, a caption, a `<virtual-list>`) is `sized` and gets a prepaint listener; a non-atomic `<span>` (scoped in `build_atomics`), a `display: contents` element (carried to its spliced children by `ContentsLevel`, applied in `enter_contents`) and a table part are found by a query but answer no size feature (unknown, and the lookup stops there), and a `cq` unit looks past them. `<img>` and `<svg>` have nothing to query: `contain_declared` only sets containment. `<input>` ignores `containerType` as Chrome does. One container answers a whole query (`Needs`: the axes its size features read, and whether it has `style()`), picked by `Chain` (`Containers::container`); a `style()` query with no name and no size feature asks the parent's computed custom properties (`Inherited::custom`). A window that allows more than one pass skips GPUI's view cache (gpuix builds no cached views).
- **Tables are zed's taffy table algorithm, and gpuix keeps the tree well formed.** `display: table` and the `table-*` values are a `gpui::TableRole` on a block box; the layout is `zed/third_party/taffy/src/compute/table/` (`mod.rs` lists where it differs from Chrome), compared against headless Chrome in its tests. taffy treats a child that is no table part as a cell of its own, so `layout/table.rs` does the CSS wrapping before gpui sees the tree: `part` reads the role (through `display()`), `anonymous_cell` wraps loose children, `wrap_loose_parts` puts an anonymous table around parts outside a table. `border-collapse` is three pieces that must agree: `layout/table_grid.rs` re-derives the slot grid zed's `compute/table/structure.rs` places, `layout/collapse.rs` resolves which border each edge has (CSS 2.1 17.6.2.1) and hands layout the half widths, and zed's `collapsed_borders.rs` paints each edge once from the lines the layout recorded (a mismatch paints nothing). Table styles are `css/table.rs`; `colSpan`, `rowSpan` and `span` are element props read in `layout/table.rs`. Table parts are query containers that cannot be size contained (see Container queries above). A part of a collapsed table is built after the table, so the table reads each part's borders from `ResolvedStyles::borders` (what the last pass painted, transitions and motion included) and `rebuild_if_borders_moved` asks for another pass when a part has since been painted with another border *width or style* (`collapse::same_geometry`). Colour takes no part in which border wins an edge, so each edge keeps its owner (`collapse::Owners`) and `settle_collapsed_borders` recolours the edges with the colours the parts were painted with in the same build (`collapse::recolored`), which is why a `border-color` transition costs no second pass and no rebuilt siblings. The table is the unit of a patched tick for a paint-only animation on one of its parts, and for an anonymous table (no element) the element that holds it is (`Retain::collapsed_table`, `unit_facts`); the parts stay excluded as roots (`Exclusion::CollapsedTable`). Whatever else a collapsed table reads from its parts has to be recorded the same way. Percentage heights are resolved in `compute/table/heights.rs` (`distribute_table_height`, `distribute_to_rows`).
- **`direction` is `css::direction::Inherited`, resolved per element in `Inherited::direction`.** It reaches taffy as `Style.direction` (set only for `rtl`: gpui's default is `Ltr`, and a gpui style is not inherited, so an element built under an rtl parent gets it from `Inherited`, including anonymous cells and tables). `text-align` is kept as its keyword so `start` and `end` resolve against each element's own direction; the resolved alignment is set on an element only when it differs from what the parent's gpui text style hands down. `Inherited::apply` takes `&mut E` on purpose: it runs on every build level and a by-value `Div` made the deepest debug test overflow its stack. `layout/collapse.rs` walks the grid in table order and mirrors only the halves and edges it hands to zed, which keeps `CollapsedBorders` in screen order. Text positions and hit tests in zed (`TextLayout::position_for_index`, `index_for_position`, `align_offset_at`) include text alignment; code that needs the left of a row uses `align_offset_at`, not `bounds.left()`.
- **Atomic inlines (`inline-block`, `inline-flex`, `inline-grid`, `inline-table`) are a U+FFFC placeholder in the paragraph.** `layout/display.rs` is the one place that knows the values and how they blockify (flex or grid item, absolute box). The planner (`text/inline.rs`) puts the box in the paragraph; `text/atomic.rs` strips its margin, resolves its percentage width and reads its baseline off the texts inside; zed's `StyledText::with_atomic_inlines` owns the element and measures it before shaping, in a private layout engine per paragraph because the text measures while the window's engine is taken (so the box is a layout root: absolute descendants resolve against it, and a fitting box is two subtree layouts a frame). Selection and find put the box's text at the byte of its placeholder (see Text rendering). `verticalAlign` is `css/vertical_align.rs` for spans and atomics, and the cell alignment of `table-cell` is a separate gpui enum. An empty span with inline margin, border or padding is a placeholder of the same kind with no element (`InlineContent::Empty`): the line layout treats it as a place no line breaks at (`LineBreaks::new_skipping`) and its glyph carries the span's insets; keep `same_paragraphs` reading `empty_box::style_takes_space` when the planner reads a new style.
- **Text has a first baseline.** A measured text leaf returns `MeasuredLeaf` (size plus the baseline of its first row) to taffy, so baseline alignment in cells, flex, grid and inline lines uses text; a new measured element that has text should report one. A trimmed text reports baselines from the top of its trimmed box (`TextLayoutInner::trim_top` is taken off in `first_baseline` and `last_baseline`), and `text/atomic.rs` reads the unsnapped tops, as glyphs are placed from them.
- Grid syntax is parsed once when a style is deserialized (`layout/grid.rs`), so the per-frame `apply_styles` only moves built values. `order` and `aspectRatio` live in `layout/` too.
- `<anchored>` is a floating-ui style popup (zed `elements/anchored.rs`: flip, shift, `onPlacement`). `anchor-size()` (sizes, margins and insets of a direct child; `layout/anchor_size.rs`) starts from the trigger's last painted size, so it is right in the first frame under a trigger that has painted; with none the child is held at opacity 0 until the trigger is measured.


## Paint and style

- **Colours resolve through `ColorContext`** (`color.rs`): the element's computed `color` (what `currentColor` means), its `color-scheme`, and the window appearance. `light-dark()`, `currentColor` and `color-mix()` cannot be resolved without it, so `apply_styles` takes `&ColorContext`, a state refinement uses `for_state`, and children get `descend`. `parse_color_rgba` has no context and none of those functions: do not call it on a style value. Motion colours go through it too: `motion::Colors` (`motion/value.rs`) holds an element and a text context, `parent_color_context` (`renderer.rs`) builds the parent's from declared styles (an ancestor's `hover` colour is not in it), and a `MotionState` re-parses its colours when they change.
- **CSS value strings** (`background`, `boxShadow`, `filter`, `mask`, `clipPath`, colour functions) are parsed in `css/`, one file per concern. `gradient::parse` (every `background` and `mask` image), `filter::parse` and `clip_path::parse` go through a bounded thread-local cache keyed by the text (`css/cache.rs`), so an element built again with the same string does not re-parse it. The key is the text plus the part of the context the text reads (`cache::Inputs`): a gradient with `currentColor`, `light-dark()` or `color-mix()` keeps one result per `ColorContext::resolves_like` class, and a text with a number followed by `em`, `rem` or a viewport unit one per `LengthContext::resolves_like` class (`LengthReads::of` finds the units); a text with neither is one result for every element. A parse that starts reading another part of the context must add it to `Inputs` and to those two `resolves_like`. `boxShadow` is not cached because measurement showed no gain. Everything else is parsed on every build, so keep new parsing out of anything that runs per frame per element; `css/parse_cost.rs` and `examples/css-values.perf.test.tsx` measure it.
- **`url()` backgrounds and masks** (`background_images.rs`, `css/url.rs`). `css/url.rs` is the one reader of a `url()` token, and `syntax::split_commas` / `split_whitespace` keep a whole token together, since a quoted address can hold `)`, `,` and spaces. `css/background.rs` makes a layer `BackgroundImage::Image(ImageKey)`, the key being a hash of the trimmed address (`background_images::key`, a `DefaultHasher::new()`, so it is stable), for a `mask` layer as for a background one. Parsing has no window, and validity checks run it too, so only the code that turns slots into layers (`into_layers`, which `background::resolve` and `mask::resolve` end in) calls `background_images::request`, which queues the address on a thread-local (deduped, the address interned). `build_element` calls `pump` after every element, which is also where a virtual-list row is built: each queued key goes through the per-App `Store` (Loading, Loaded, Failed; the thread-local cannot be the store, because the test renderer opens an App per window) and, once loaded, into gpui's `BackgroundImages` table, which is what a layer paints from (no entry paints nothing). The source is `img::resolve_src` as for `<img>`: a path or `http(s)` goes through `window.use_asset::<ImageAssetLoader>` (it re-renders the view when the load ends), `data:` is decoded in the pump, and `blob:` waits for `deliver`. A blob URL exists only in JS, so `js/style-images.ts` (wired from `mutations.ts`'s `setStyle` and `setUniversalStyle`) fetches each distinct `blob:` URL a style names, state styles included, and calls `setStyleImage` / `failStyleImage`. The store keeps 128 images and evicts the least recently requested (`BackgroundImages::remove` and `window.drop_image`), so a layer must `request` on every build, not once. A hover image is requested when the hover first applies, because `build_element` folds pointer states into the style before it is resolved. A mask layer waits the same way, and an image that has not loaded or failed is a transparent-black layer (the element is fully masked), as CSS says. A `text`-clipped image layer paints inside the text-mask group: `Window::paint_image` uses `Scene::insert_image` so a text clip does not take the sprite as a glyph.
- **Effect groups** (`filter`, `backdropFilter`, `mask`, `clipPath`, group `opacity`) are offscreen passes in zed `effects.rs` and `gpui_apple`. Only macOS (Metal) renders them; elsewhere the group draws flat and only opacity applies. A mask is `Style.mask_layers`, each a `BackgroundLayer` whose `mask_mode` (`alpha` / `luminance` / `match-source`) and `mask_composite` (`add` / `subtract` / `intersect` / `exclude`, the bottom layer's ignored) are read only there; `css/background.rs` builds them for both grammars and `Kind::HAS_MASK_PARTS` lets only the `mask` shorthand take the keywords. zed draws each layer (gradient quads, image sprites) into its own texture and adds a combine pass only when a layer is not alpha + add. `mask-border` is `Style.mask_border` (`css/mask_border.rs` parses it, zed `mask_border.rs` places the nine pieces and tiles): a `MaskBorder` becomes the top mask layer, composited with `add` over the `mask_layers`, as Chrome does with `-webkit-mask-box-image`; each slice is one image sprite cropped from the atlas (or gradient quads at the slice's scale), and the polychrome sprite shader clamps its lookup to the tile so a crop does not blend in the neighbouring slice. A subtree that paints one primitive keeps the cheap per-primitive path. Changing `Quad` means changing the shader and the Rust struct together on every backend, or the stride checks fail. Only the Metal side of those shaders has run: the HLSL is built only by fxc on a Windows host and the WGSL was validated with naga but never rendered.
- **Text decoration** (`css/text_decoration.rs`) parses one declaration into a style, a colour and a thickness for all its lines, and builds gpui `UnderlineStyle`, `StrikethroughStyle` and `OverlineStyle` from it. zed's `text_decoration.rs` turns a line into `Underline` primitives (a `double` line is two solid ones; dotted and dashed carry their mark length and period), and the three shaders draw the shape. `Underline` is a GPU struct like `Quad`: change the Rust struct, `shaders.metal`, `shaders.wgsl`, `shaders_webgl.wgsl` (stride and loader) and `shaders.hlsl` together. `from-font` is resolved in zed's `paint_line`, where the glyph run's font is known, not in `apply_styles`. `auto` is a tenth of the font size of the box that declares the line (`text_decoration.rs`), kept fractional until the line paints. `css/text_underline_offset.rs` holds `textUnderlineOffset`: the box that declares the line decides the offset, so it is read where the decoration is built and an `em` resolves where the value is set. An underline is drawn at the baseline of the box that declared it: `RunStyle` puts that box's id in `UnderlineStyle::decorator`, which `InlineBox::key` names, and zed's `paint_line` asks `LineLayout::box_raise` how far the box is raised. A line-through is placed by two thirds of the ascent of the decorating box's font (`StrikethroughStyle::decorator`, `PlacedBox::ascent`) from the top of the text it crosses, an overline up from that top, and `paint_line` splits any line where the font or alignment under it moves it. `paragraph::listed_boxes` therefore lists a decorating box when some box in the paragraph is aligned off the baseline, or it strikes through and a box has another font. Dotted, dashed and wavy lines start over per text node: `RunStyle` numbers the pieces into `fragment`, and `DecorationGeometry::fitted` stretches the gaps to the line's length in device pixels. A thickness that is a length rounds half up to a device pixel with a floor of one (`round_thickness_to_device_pixel`); `auto` and `from-font` floor. A negative length is valid (Chrome computes it and paints one device pixel, as for zero), so `parse_thickness` keeps it and `Thickness::fallback` clamps; `motion` does not clamp it, since an animation passes through negative values.
- **An outline paints only with a style** (`css/outline.rs`): `outlineStyle` is `none` until `outline` or `outlineStyle` gives one, the width is then `medium` and the colour `currentColor`. `css::border::outline` is the one resolver, used by `apply_styles` and the inline boxes (`text/inline_decoration.rs`), and the style reaches the painted outline as `gpui::Outline.style`. `auto` paints as `solid`.
- **`textWrap` is `whiteSpace`'s wrap component plus a breaker style** (`css/text_wrap.rs`). `text_wrap::apply` runs after `whiteSpace` in the `apply_styles` family, because `textWrapMode` is the longhand and wins on the same element. The style reaches gpui as `TextStyle::text_wrap_style`, and zed `text_system/text_wrap.rs` picks the breaks among the usual break opportunities with greedy wrapping's own line measure, so the width the text reports to layout does not change. It follows Chromium's `kMaxLinesForBalance` and `kMaxLinesForOptimal`, and skips a text with a tab, `break-spaces` or a clamp. The style is a container's alone (a paragraph is one layout, and Chromium reads it from the block), but the mode is per box: `paragraph::BoxStyle::wraps` becomes `InlineBox::wraps`, zed resolves it per run (`RunWrap` on `InlineRun`, set by `apply_inline`), and `compute_wrap_boundaries` allows a break only where `SpaceRun` and `may_break_apart` say, which follow Chromium (oracle: `fixtures/text-wrap-spans-oracle.html`). `listed_boxes` lists a box that declares a mode, and every box when some box wraps. `layout/contents.rs` carries it through `display: contents`.
- **`textBox`** (`css/text_box.rs`, zed `text_system/text_box.rs`). `textBoxEdge` is inherited text style; `textBoxTrim` is not, so the renderer passes it down as `BuildCtx::text_box_trim` to the first and last in-flow child of a block container (`build_plan`) and to the paragraph, which cuts the row (`with_text_box_trim`). A flex or grid container, and anonymous table cells, start with no trim.
- **`fontVariationSettings`** (`css/font_variations.rs`) is carried like `fontFeatureSettings`: `Inherited::font_variations` is set on the element that declares it, and `RunStyle::variations` on a span. On macOS zed's `varied_font` (`gpui_macos/src/text_system.rs`) applies the axes and also sets `wght` from `fontWeight` on a variable font unless the settings name it, so a `font` with different variations is a different face. A value CSS does not have is dropped when the style parses (`deserialize_ignoring_invalid`, also for the two feature props), so an element keeps what it inherits.
- **Border radius** is parsed when the style arrives, not per frame: `BorderRadiusValue` and `CornerRadiusValue` (`style.rs`) deserialize a number or a CSS string through `css/border_radius.rs` into a `LinearLength` per axis (so `em`, `rem`, the viewport units and `calc()` work), so an invalid value fails `StyleTable::intern` like an invalid `width`, in state styles too. `CornerRadiusValue::resolve` turns it into gpui `CornerRadius` (`x`, `y` as `DefiniteLength`) with the element's `LengthContext` when the style is applied: the px part is resolved, and a percentage stays a `Fraction` (or the `Calc` sum with the px part) that zed resolves against the border box, `x` of the width and `y` of the height. gpui reads a zero `y` as "same as `x`", so `resolve` squares a corner with an explicit zero on either axis. `parse_radius_length` in that file is the one place a radius token is read. Motion reads and writes a corner through `Field for Option<CornerRadiusValue>`: a circular corner is a length, an elliptical one `AnimValue::Ellipse`, so both radii animate. The `round` of `clipPath` shapes shares `border_radius::parse_radii`.
- **Border styles** are per side: gpui `Style.border_styles` is `Edges<BorderStyle>`, and `css/border.rs` merges the shorthands, `borderStyle` (one to four values) and the per-side longhands into it. A side's style starts as `none`, as in CSS, and a `none` or `hidden` side has used width 0 (a visible style with no width is `medium`): `Declared::used_width` is the one place that rule lives, and the div (`apply_widths`, which only touches the sides a declaration names, so the `<img>` fallback keeps its own border), inline boxes (`inline_insets::border_widths`) and state refinements (`apply_state_styles` sizes the border from the base overlaid with the state, since the state alone may not carry the style) all go through it. zed `border_style.rs` turns each side's style and colour into what the shaders get (`Quad.border_styles`, `border_colors`, `border_inner_colors`): `inset`/`outset` become solid with Blink-shaded colours, `groove`/`ridge` carry the outer half in `border_colors` and the inner in `border_inner_colors`, and a `double` under 3 device px becomes solid. The shaders cut dotted, dashed, double and groove/ridge bands out of the border, the style of a point being its owning side's on the colour join line. `Quad` stride lives in the Rust struct, the three shader structs and the `shaders_webgl.wgsl` loader.
- **`overflow` clips to the padding box** with rounded inner radii (zed rounded content masks), not to a rectangle.
- **Nested `overflow` clips are exact.** A `ContentMask` is one rounded rectangle, and `ContentMask::intersect` of two is only that when their corners line up. `ContentMask::intersection_is_exact` says when it is the true intersection (a child's square or rounded corner inside the parent's arc, a shared corner where one mask's radii are at least the other's on both axes). The overflow clip of `div` and `<virtual-list>` paints through `Window::with_exact_content_mask`: exact pairs, the normal case, take the plain `with_content_mask` path; an inexact one paints as an identity group (`GroupEffect::exact_clip`, never folded into one primitive) under the parent's mask, with the child's own mask on its primitives, so the group composite cuts by the parent and the primitives by the child. That costs one offscreen pass, and only on macOS: without effect groups it stays the approximate `intersect`. `intersection_is_exact` runs per overflow element per frame, so its fast path (a clip clear of the other's rounded corner boxes) has to stay cheap. A clip with a box inside that escapes it (`Window::note_escape`, read through `escapes_prepainted` at prepaint) skips the group, since `Scene::widen_groups` would widen it past the parent's mask. Hit testing does the same through `Window::with_exact_hit_mask`: a hitbox under an inexact pair keeps the list of clips it must be inside (`Hitbox::exact_clips`). A positioned box that a clip contains is painted by its stacking context with the clip it had at prepaint, so it is cut approximately. `overflow_mask` rounds only when both axes clip, as in CSS (csswg-drafts#7434): `clip` beside `visible` is a square mask unbounded along the visible axis (`UNBOUNDED_CLIP`), while `hidden` and `scroll` beside `visible` clip both (`scroll::Axes::of` computes the visible axis to `auto`, and `renderer.rs` hands GPUI `Overflow::Clip` only for the `clip` + `visible` pair). A `<virtual-list>` clips and hit-masks its rows with `Style::padding_box_mask`, the both-axes case of `overflow_mask`, so its `borderRadius` less its border rounds them. `examples/overflow-clip.perf.test.tsx` measures the frame cost of plain, aligned and cut pairs.
- **Inline text.** `text::inline::plan_children` is the CSS inline formatting rule: in a block container each maximal run of inline children (raw text, `<span>`, a nested `<text>` that is not focusable) is one paragraph, and a block child ends the run; in a flex or grid container children are blockified. A focusable `<text>` stays a block, because an inline box has no focus handle; listeners and state styles do not make a box a block (the pointer finds an inline box through `text/inline_hit.rs`, and `run_styles` folds a hovered or pressed box's `hover` / `active` / group styles into its run). A span's `cursor` is the exception to "no hitbox": GPUI resolves the cursor from hitboxes on every move, so `text/inline_cursor.rs` inserts one per line fragment, after the glyphs, only for text that asks for one; they carry the container's element id, so `host_hit_chain` reads them as the container. `text/paragraph.rs` turns the inline boxes into per-run styles. A box's `focusVisible` / `focusWithin` are folded the same way (`PointerStates` carries the two focus bits for inline boxes only, set by `FocusStyles::states_of`, since a div applies them in `apply_focus_styles`), and `text/inline_decoration.rs` paints its `backgroundColor`, border and `borderRadius` behind the glyphs, and its `outline` over them, for each line fragment (a box's background is never a `TextRun` background, which would fill its margin box). Every consumer of a box's rectangle (bounds, hit test, cursor hitboxes, the painted quads) takes it from `inline_bounds::border_box_rows`: the font's content area in the line plus the vertical padding and border, not the line box. A box's margin, border and padding are `text/inline_insets.rs`: an `InsetSpec` keeps the lengths as written (`em`, `rem`, `calc()` with the box's own `LengthContext` from `paragraph::box_lengths`, so a span's `em` is its own font size; a `%` is of the width the paragraph is laid out in), `StyledText::with_insets_for_width(spec.text_key(), ..)` asks it for that width at measure (`text_key` hashes what `Insets::text` reads, which is the measure key's share of the insets), and each consumer reads its border box edges (`BoxEdges::at(layout)`) for the width the layout kept (`TextLayout::inset_width`), never at build time. Text's min-content width is `WindowTextSystem::min_content_width`: the text laid out at wrap width 0 with `TextLayoutOptions::for_min_content`, so it follows the same break rules as the text; do not mirror them elsewhere. A span's `fontSize` and `lineHeight` size its run (`RunStyle.font_size` / `line_height`, `TextRun.font_size` / `line_height` in gpui): the row is as tall as the CSS line box, the container's strut plus each run's own contribution, and `rem` stays the root's. A box with an `opacity` below one is an `OpacityGroup` (`paragraph::styles`): its runs carry `TextRun::group`, which `TextLayout::paint` leaves out, and `text/paint.rs` paints the group (its backgrounds and rings from `inline_decoration`, its glyphs and lines through `TextLayout::paint_group`) in a `with_effect_group` after the rest of the paragraph, nested groups inside it. Colours are never pre-faded.
- **Line boxes follow Chrome on macOS.** `FontMetrics::line_box(font_size, scale_factor)` (zed `text_system.rs`) rounds a face's ascent, descent and line gap in device pixels at the used size (Blink zooms the font to the display first, so Menlo 20px is 24px of content at scale 1 and 23px at scale 2), and widens the ascent of Times, Helvetica and Courier (`widens_ascent`, set by `gpui_macos`); `line-height: normal` (`gpui::LineHeight::Normal`, the default of `TextStyle`) is their sum. Half-leading is `extent_in_line(.., scale_factor)`: floored above the text to a whole device pixel (a 24px line around Menlo 16px has its baseline at 17px at scale 1 and 17.5 at scale 2). The scale is the window's, held by `WindowTextSystem` (`Window::apply_scale_factor` sets it with the window's own: creation, `bounds_changed`, the test `set_scale_factor`) and passed to `PlatformTextSystem::layout_line`; a `LineLayout` records the scale it was shaped for (`scale_factor`, which `line_rows` and every reader without a window use), it is part of the line layout cache key and so of the shape store's, and of `TextLayout`'s measure key and reuse check. `TextSystem::line_height` / `baseline_offset` / `line_box` take it too. `gpui_wgpu` and `gpui_windows` accept and ignore it. Text is measured in Chrome's unit at the window's scale, 1/64 of a device pixel (`util.rs`: `ceil_to_layout_unit(width, scale)` for the inline size of a text, which is `ShapeResult::SnappedWidth`, `round_to_layout_unit` for a trimmed height and baseline; `measured_size_in_device_pixels` in `taffy.rs` ceils in device pixels), and a line height is `LineHeight::to_laid_out_pixels` (`TextSystem::line_height`, `RunHeights` in `text/paragraph.rs`): a number is cut to the unit, a length or a percentage (a length once computed) is rounded, as Blink does. The slack that forgives float noise in a fit (`FIT_SLACK`) is a length in logical pixels both in the ceil and in `overflows`, so a width that was measured to fit never wraps in its own box. A measured leaf keeps its fractional height in taffy; `layout_bounds` snaps boxes. The line's strut is the font of the element (`TextLayoutOptions::strut_font`, set by `TextLayout::layout`), and a run whose rounded ascent or descent differs from it (another weight too) is its own box (`LineLayout::is_styled`). Paragraph runs get a `line_height` of their own in `text/paragraph.rs` (`run_line_height`). The oracle is `packages/react/src/__tests__/fixtures/line-box-oracle.*`, asserted by `line-box.test.tsx`.
- **`<svg>`** rasterizes with resvg at device scale in its own colours (`custom_elements/svg_paint.rs`); `currentColor` is the element's `color`, not black. DOM-style shape children (`<path>`…) never become GPUI elements: `svg_tree.rs` serializes them into one image. `SvgPaint::image` holds one image and drops the previous one from the atlas when an input changes, so keep that single entry point. A document over 4096 bytes or 128 x 128 px is rasterised by a job, except while an animation runs on its shapes (`CustomRenderContext::shapes_run`, `Key::in_paint`): its document changes at every frame, so it is rasterised in the paint, which keeps its ticks out of `Exclusion::Svg` and patchable. The image is fitted to the box whatever `overflow` says, so an `<svg>` is cullable like a box (`CustomRenderContext::cull_outset`, from `Subtree::of_element`: its own shadow, filter and outline only; a positioned one still blocks): its canvas makes no image in a culled subtree (`SvgPaint::note_culled`, `retain_state` 1). Its `retain_state` is `SvgPaint::unsettled` turned into a word (0 settled, 2 never painted: no area, 3 a job running, 4 its result waiting to be installed by the next paint) and is `None` (`Exclusion::Svg`, which makes every tick on the page whole) only for a big document or colour that a paint would start a job for: an `<svg>` with no area, or waiting for a job, leaves the page's ticks alone, and the arrival of its raster is the word changing, which builds that element once, and its shapes' animations are the `<svg>`'s to keep on time while it is dormant (`running_motion` names the `<svg>` for a running shape, `with_shapes` hands the clocks of its shapes to `enter_dormant`).
- **`clipPath` clips hit testing** (zed `Hitbox.clip`) for the hitbox of the element it is on and every hitbox inside it (the parent chain). A host element's hitbox comes from a styled `div`, or from the list for `<virtual-list>`, which inserts its own shaped hitbox, paints its box style with `Style::paint_with_effects` and runs the scrollbar through the `prepaint_overlay` / `paint_overlay` hooks so the bar is inside that clip. A hitbox a custom element inserts with `Window::insert_hitbox` outside such an element has no clip. Window drag regions (`appRegion`) are rectangles in paint order and ignore it.
- **`borderRadius` cuts the hit area of the box itself**: a `div` hitbox carries the border-box corner radii it paints with (zed `Hitbox.corner_radii`, from `Style::border_box_radii`, tested by `point_in_rounded_rect` in `hit_test` and `dom_hit_test`), elliptical corners included. It does not reach descendants (unlike `overflow` and `clipPath`), whose hit testing is per element as in CSS. A hitbox a custom element inserts with `Window::insert_hitbox` has square corners; use `insert_hitbox_shaped` for radii (the list does). The wheel and scroll routing candidates come from the same hit test, so the cut applies to them too.


## Motion

`packages/native/src/motion/` resolves animation during GPUI rendering, outside React. A description (`initial`, `animate`, `transition`) becomes one `Track` per animated property, and a retarget restarts only the properties whose target changed (`mod.rs`). Around it: CSS transitions and animations (`css_transition.rs`, `css_animation.rs`), the keyframes registry, motion values (`bindings.rs`), `layout_flip.rs` and `measure.rs`.

- **Transforms are a matrix on every primitive** (`Window::with_transform`, [paint-and-transforms.md](./docs/fork/paint-and-transforms.md#css-transforms-92)). `css/transform.rs` parses `transform`, `translate`, `rotate`, `scale`, `transformOrigin`, `perspective`, `perspectiveOrigin`, `transformStyle`, `backfaceVisibility` and Motion's keys (`x`, `scale` as `motionScale`, `rotate` as `motionRotate`, `z`, `rotateX`, ...) into one `ElementTransform` (a 4x4 `Matrix3d`, the parent's perspective in front, `preserve_3d`, `backface_hidden`) that GPUI resolves from the unsnapped border box. A translation-only ambient transform moves snapped bounds and leaves primitives untransformed; a general one makes quads, shadows, surfaces and underlines carry the matrix, with `content_mask` in window space and a separate `local_clip`, and glyphs are rasterized through the matrix and drawn upright (a bucketed raster scale while the transform runs, `StyleDesc::transform_in_motion`). A `defer_draw` keeps the ambient transforms; a deferred `<anchored>` opts out with `Deferred::in_window_space` and is placed by the trigger's transformed box. A box read is `ElementBounds`: the layout box, plus `to_window` for the painted one, so `getBoundingClientRect` is the bounding box of the corners and layout reads, `ResizeObserver` and `offsetWidth` stay the layout box. Hit spaces (`Hitbox::to_local`) and listener positions follow the ambient transform; a patch copies them from the scratch frame, and its tape compares boxes apart from transforms. `StyleDesc::animating_transform` keeps the stacking context for a whole run. A non-atomic inline box takes no transform. Primitives carry a projective 3x3 (the z=0 plane of the 4x4): a flat parent composes 3x3s, a `preserve-3d` chain multiplies 4x4s and flattens once at the leaf, planes paint in paint order, and `backface-visibility` follows Chromium's `IsBackFaceVisible` (`Window::layout_faces_away` serves the hit test of retained trees). The used `transform-style` is flat under Chromium's grouping properties (`layout/stacking.rs`, `groups_descendants`) while the declared one still makes the stacking context and containing block. A blend with a 3D function pairs as Blink does and decomposes in 3D (`motion/transform_value.rs`). `layout`, `layoutId` and a dragged `Reorder` item move by prepainting inside `Window::with_element_offset`, so position animates and size snaps; `layout="size"` as a FLIP scale is not built.
- `measure.rs` is what the previous frame's layout produced. Only `width: "auto"`, `height: "auto"` and an unset size start read it; the animation waits (`Pending`) and its track starts at the clock of the style change, not of the measuring frame, so the first sample can already be frames in. The natural width comes from an extra layout of the tree that GPUI runs only for nodes flagged `measure_natural_width`; `motion::wants_natural_width` flags an element only when its motion, or its transitions and animations under `interpolateSize: "allow-keywords"`, can run on `width`, so other trees pay nothing.
- Hover and active for an element with a transition come from `PointerState`, folded once in `build_element`, one frame after the pointer moves. Do not add a probe element above a hitbox: it hides the element from the DOM hit test.
- Transition and animation events are lowercase DOM types (`transitionend`, …) with `target` and `path`; they bubble and are not cancelable. `Reorder` (React and Solid) captures the pointer with `setPointerCapture` on `pointerdown`, so a drag works only because capture is explicit.
- **Web Animations objects are the animation half of the spec, run natively and driven by JS reads.** Each CSS animation `Run` and transition `Running` carries a `web_animation::Playback`: hold time, start time, pending play/pause task, pending rate, the finished state and the notes (`Ready`, `Finish`, `Cancel`) the spec makes promises and events of. Its timeline is the renderer's clock at the last frame's instant; `startTime` and `timeline.currentTime` are ms on `AutomationClock`. A play or pause waits for a frame that is *drawn*: a layout pass a read forced (`window.is_layout_pass()`, told to the engines by `note_pass`) creates runs pending and tells the DOM nothing, and the draw that follows resolves them, `Ready` first for a play and after the events for a pause. `emit_css_events` merges the DOM events and the Web Animations events by `seq` (`late` DOM events, those of a play that just ran or a transition a frame made, go after them), which is Chrome's order. `cancel()` keeps the run idle in its engine (a CSS animation stays in `runs`, a transition goes to `ElementState::cancelled`) for `play()` and the like; a style change does not recreate it, and the name leaving the style drops it silently. A transition that finishes at its end moves to `ElementState::completed` and one run back to its start stays in `running`; both stay readable and commandable by id until the style moves off the end value, which cancels them (API cancel only, no `transitioncancel`). `getAnimations(elementId)` lists them in composite order with Chrome's relevance (phase-based). JS (`js/web-animations.ts`) keeps one `Animation` (a `GpuixEventTarget`) per native id per renderer, derives `ready`/`finished`/`pending` from what native says after each command, and takes `webanimationready`, `webanimationfinish` and `webanimationcancel` in `RendererState.dispatch` before any app dispatch; a removed element's animations are cancelled by `createMutationQueue` from the destroyed ids, since native sends nothing. A run that finished or was cancelled is kept in `web_animation::Retired` as it ended until JS has consumed each of its events and calls `releaseAnimation(id)`, so no read throws between the frame and the event. A command names its element in `retain::Sources::commanded`: a finished transition that `play()` revives was not dynamic in its last build, so retention would replay it unchanged. A run the style lets go of (or a cancelled script animation) moves to the element's disowned runs and stays playable while JS holds its `Animation`; `forgetAnimation` (from a `FinalizationRegistry`) drops it. `animation.effect` is `motion/effect.rs`; `getAnimations({subtree})` and `document.getAnimations()` are `getSubtreeAnimations` and `getDocumentAnimations`. The animations Motion 14.1.0 hands to the browser (`opacity`, `backgroundColor`, browser-syntax colours) are listed from `motion/script.rs`, a clock and an effect beside the track's own. A forced read makes the runs of the elements it builds and `CssAnimations::hold` / `CssTransitions::hold` keep them on time without a build (`Beat::Held`) until a build that is not forced releases them. Oracles: `web-animations-oracle.html` (its `apiCases` run on a window that draws only at `settle`, see the header of `web-animations.test.tsx`), `motion-waapi-oracle` and `svg-transitions-oracle` ([testing.md](./docs/fork/testing.md#chrome-oracles)).
- Gestures (`whileHover` / `whileTap` / `whileFocus` / `whileInView`) are fields of the `motion` description for every element, span and SVG shape: `MotionState::set_gestures` runs from `sync_motion_states` with the states `gestures_of` reads (`:hover`, `:active`, `:focus-visible`), the priority is Framer's `variantPriorityOrder` (`Gesture` enum order), and `inView` is written by JS. A target a controlling ancestor's variant name stands for carries `inherit` and answers to that ancestor's gesture (`gesture_controller` finds the nearest ancestor whose description has `controls`, as the React/Solid `VariantContext` nests); `motion-variants.ts` is the shared JS resolver. `listens_for_gesture` keeps the pointer dispatcher restyling for an element whose only hover/press consumer is a gesture. Gestures dropped from a description (a leaving node) hand their properties back.
- A `<span>` in a paragraph has no `build_element`, so `motion/inline.rs` steps its `MotionState` (`motion.span`) with its CSS transitions and animations: transitions, then `motion` (with its `whileHover` / `whileTap` / `whileFocus` / `whileInView` targets, see `MotionState::set_gestures`), then the motion values its `style` binds (`MotionValueStore::apply_in`), then animations, as for an element. It is mounted with `MotionState::new_inline`, which rejects a description naming a property outside `Scope::InlineBox` (the same set the span's CSS transitions and animations move), so the JS adapters' `MOTION_SPAN_KEYS` (`js/motion-span.ts`) must stay in step with `Scope::animates`. Everything that reads a span's style for the frame (`paragraph::styles`, `inline_decoration::of`, `InsetSpec::of_moving`, `paragraph::box_lengths`) goes through `InlineStyles::declared`, never `tree.elements[..].style`: a new reader of an inline box's style must too, or an animated padding or font is not seen. An inline box's `opacity` is an `OpacityGroup`, not a scaled alpha (see Inline text above).
- **A motion length is a px part and a percentage part.** `AnimValue::Number` (px), `Percent` and `Calc { px, percent }` (`Layout::Calc`, two channels) blend part by part, so `px`, `%`, `calc()` and `em` / `rem` / viewport lengths mix; relative units are px by the time an element's track holds them (`Colors::lengths`, `LengthContext::split`) and the percentage stays a share of its basis, which lowers to a taffy calc node each frame, so a parent resize needs no measurement. A spring rests per channel, and `Spring::rest_thresholds(distance, percent)` scales the percentage channel by 0.01 (its `restDelta` and `restSpeed` are percentage points). A new length field reads and writes through `resolved` / `linear_length` in `motion/property.rs`; a field CSS gives no percentage to refuses one there.
- **A shorthand in motion is split before it is parsed.** `split_shorthand` (`motion/property.rs`) turns `padding: "0.5em 0.75em"`, `borderRadius: "10px / 20px"` and the rest into one value per longhand, frame by frame for keyframes, and `expand_shorthand` lists the longhands. `parse_goals` and `Transitions::parse` walk the keys in written order (serde_json `preserve_order`), so the later of a shorthand and a longhand wins. A corner with two radii is `AnimValue::Ellipse`. A motion value holding such a string is a `ListValue` (`motion/list.rs`); bound to a shorthand, `split_for` (`motion/bindings.rs`) gives each longhand its component.
- A motion value is not tied to an element, so its colours resolve as on a root with `colorScheme: "light dark"` and keep what they resolved to, and its `vw` / `vh` against the window when it is written. Its `em` / `rem` stay unresolved (`AnimValue::Relative`, `Layout::Relative`, four channels px / % / em / rem) and `MotionValueStore::apply` resolves them per bound element with `ParentValues::lengths_of` (the font size first, since its `em` is the parent's); an element's own `animate` resolves at parse time as before.
- **An element bound to motion values is touched by a tick only when a value can have changed.** Every bound element carries `Dynamic::MotionValues`; `Retain::between_changes` leaves one whose only dynamic is that out of the touched set unless `Sources::clock_due` says so, which for a binding is `MotionValueStore::binding_due`: a value it binds was moving by itself in the last sample (`frame_moving`), or it or a value it is computed from was written, animated or controlled since (`written`, cleared by `frame`, which a frame built whole and a patcher's `step_clocks` both run; `MotionValues::follows_any` walks the transform and follower inputs). So a page of bound boxes that rest costs a tick of a loop elsewhere nothing, and a width bound to a value that rests is no `LayoutAnimation` refusal for the loop beside it. Anything that changes `frame_moving` or `written` outside those two calls must keep the element's last build and the value it shows the same.
- **Whether a moving motion value asks for a frame is per element.** `MotionValueStore::demand` (renderer.rs `motion_value_demand`) says `Frame` when a value that an element with a box that is not `Retain::is_dormant` (`value_sleeps`) binds, or a drag reads, moves. For the rest the end of a frame, of a patch and of a dormant tick ask `wake_dormant` for `GpuixView::dormant_clock_wake` (`dormant_wake.rs`: `Never`, `NextFrame` or `At`, merged by the earliest), and not `request_next_frame`. `MotionValueStore::dormant_wake` is `NextFrame` for a moving value with a `change` listener (Motion's `change` follows the animation, not the element being seen), the end of its animation for one with only an `animation` listener (`MotionValues::wake`; `NextFrame` for a spring) and `Never` for one nobody listens to, whoever binds it: the frame that draws its element again samples it. `dormant_tick` (a frame callback that `wake_dormant` queues at once for `NextFrame` and from an executor timer for `Wake::At`) runs `MotionValueStore::advance_dormant`, which only finds the events of the listened values, without touching `frame_moving`, `frame_values` or `written`, which a frame that builds an element has to find as it left them. `MotionValueStore::step` leaves a moving value bound only to sleeping elements as it is. An element whose opacity is animated is not culled in a scroller (a CSS opacity loop there is built every tick as well), so a bound opacity is not dormant either.
- **A script's write to a motion value that only paints is a source for a patch.** `Repaint::invalidate` (renderer.rs) asks for a tick (`Window::request_tick`) when `GpuixView::patches_ticks` and `MotionValueStore::writes_paint_only` hold, where it would `notify_paint_only` (a view queued, so `PendingView` declines the patch and the frame is built whole); the write is in `written`, so `binding_due` touches the elements bound to the value and the tick builds them alone. A tick the patcher does not take is drawn whole in the same draw, as for a clock. A write that can move a box (`writes_paint_only` false) still invalidates layout.
- **`auto` in a margin, inset or `flexBasis` (and `content` in `flexBasis`, `AnimValue::content_keyword`) is `AnimValue::Keyword(Some("auto"))`** (`AnimValue::auto_keyword`, `Kind::Offset`), not `AnimValue::Auto`, which is the measured `width` / `height` auto. `align_in` pairs it with a length as `Layout::DiscreteList`, so it is `Pair::is_discrete` (a CSS transition needs `allow-discrete`) and flips at half. Unset insets and `flexBasis` start at it, unset margins at `0`. An unset border width starts from `AnimProp::initial(style, ..)`, which reads `css::border::computed_width` (a `border` shorthand's width, else `medium`; the side's style does not enter, Blink zeroes a `none` side where it is used) and an unset outline width from `medium`. `flex` is a motion shorthand (`expand_shorthand`, `split_flex`) but cannot be bound to a motion value.
- **An unset inherited property starts from the parent's computed value** (`motion::Inheritable`, in `ParentValues` and `Colors::inherited`): `textUnderlineOffset`, the table properties, `pointerEvents`, `visibility` and a unitless `lineHeight` next to what `Colors::font` and the colour contexts hold. It is built from `Inherited` at build time (`Inherited::parent_values`) and from the tree's declared styles in `parent_inherited`; a field added to it needs one in `Inherited`, its `descend` and `inherited_fingerprint`.
- `fontVariationSettings` animates as `AnimValue::Variations`: two lists naming the same axes blend per axis, any other pair is discrete. The element's computed value rides in `Colors::font` (`Font::variations`) with `fontSize` and `fontWeight`, which is why `Colors` and `ParentValues` are not `Copy`.
- `matchMedia`: JS owns the query language (`js/media-query.ts`); `media.rs` only answers what the OS says, and only macOS has a read.

**Adding a `StyleDesc` field.** `overlay_fields!` in `events/state_styles.rs` must list it: it is a struct literal, so a missing field fails to compile instead of silently never taking part in state styles. If the field is a shorthand, it needs a row in the covers table (`css/covers.rs`): that one table makes a shorthand drop the longhands written before it in the same object, and makes `drop_shadowed_longhands` in the same file reset the longhands it covers, or a hover shorthand loses to a base longhand. If it animates, it needs an entry in `motion/property.rs`. `Kind::Keyword` (a string field) and `Kind::TypedKeyword` (an enum field implementing `css::table::KeywordValue`) are discrete: only a CSS transition with `allow-discrete` runs them, and `animate` and keyframes reject them. `zIndex` is `Kind::Integer` (rounded half up where it is written), `borderSpacing` a pair of lengths held as `AnimValue::Ellipse`, and `verticalAlign` a length or a keyword.

**A shorthand stays its own field and holds its expanded parts.** `padding`, `margin`, `inset`, `borderWidth` are `Sides<T>` and `gap` is `Gap` (`css/shorthand.rs`); the logical ones are `Pair<T>` (`css/logical.rs`), `flex` / `flexFlow` are `css/flex.rs`, `placeItems` / `placeContent` / `placeSelf` are `css/place.rs`, `outline` is `css/outline.rs`, `textWrap` and `textBox` follow the same shape. The shorthand is parsed when the style is deserialized (an invalid one fails `StyleTable::intern`), never flattened into the longhands, so a state style can replace the base's shorthand and the longhands it covers. Of a shorthand and a longhand it covers the key written later wins, as in CSS: the deserializer (`css/covers.rs`) drops the longhands a shorthand covers that were written before it, so what remains is ranked by specificity (the shorthand, the logical pair, then the longhand) and a consumer reads the side from `padding_below_longhands()` (the shorthand, then the logical pair) and lets the explicit longhand override it, or through `effective_*` (`effective_flex_grow`, `effective_align_items`). Read a shorthand-covered value through those, never `style.padding_top` alone. The start of a logical inline pair (`marginInline`, `paddingInline`, `insetInline`) is the left in `ltr` and the right in `rtl`: `StyleDesc::mirrored_inline_pairs` swaps the ends for an element whose own direction is `rtl` (in `build_element_unpumped` after `Inherited::descend`, and in `InsetSpec::of_moving` for the spans of a paragraph), so everything below reads `start` as left. `mask` covers `maskBorder` and its longhands (the specification says it resets them). A new shorthand needs its field in `overlay_fields!`, a row in the covers table, a `host.ts` type, and for animation an entry in `motion::property::expand_shorthand`.

## Text rendering: one funnel, no exceptions

Every string GPUIX paints goes through `crate::text`:

- `selectable_text(..)` for content — registers into the per-frame selection
  registry and installs the window mouse and key listeners
- `chrome_text(..)` for line numbers, language tags and file headers — painted
  and logged for tests, but never part of a selection

**Never call `div().child(some_string)` in a new element.** Doing so makes the
text invisible to selection AND to `getPaintedText()`, so it cannot be tested
except by screenshot.

The registry is rebuilt during **paint**, not during build, because a `list()`
decides at paint time which rows exist. It is in paint order, which is NOT
document order (a positioned element is lifted and registers after the siblings
that follow it): hit testing (`registry_point`) uses paint order, so the topmost
text wins, while selection, copy and the wash use `DocPos` (child indices through
the retained tree plus `sub`; tree order, ignoring flex `order`), carried on
`SelectableText::path`. The text of an atomic inline (`inline-block`...) is hosted
by the paragraph the box is in, at the byte of its placeholder (`DocPath::host`,
`BuildCtx::doc_hosts`): the paragraph is one registry entry, so `resolve_drag`
cuts it at its U+FFFC into segments (`selection::segments`) and the box's text
sorts between them; a segment and the box's texts share a `line` so copy puts no
line break between them. Find numbers matches the same way: every run announces
its matches in prepaint (`search::announce_*_run`, at `DocPos::at(byte)`), and
`wash` numbers them in document order. `selection_frame_reset()` must stay the first child of the
root, or stale entries from the previous frame leak into the next drag.

gpui's line layout cache keeps two frame generations and, behind them, a per-window shape store (zed `text_system/line_layout/shape_store.rs`, bounded and emptied when fonts are added), so a page mounted again after other frames shapes nothing; `getLayoutCounters().shapeCacheHits` counts the lines it gave back (guard: `shape-cache.test.tsx`). The macOS text system caches sized `CTFont`s (zed `gpui_macos/src/text_system.rs`).

The registries are thread-locals, but keyed by `WindowId`: every window's frame
reset runs on the one UI thread, so a single list would be cleared by whichever
window reset last. That covers the selection registry, start regions, the
painted-text and highlight logs (`text::paint::Frame`), the match numbering
(`text::search::ORDINALS`) and the span hit registry (`text::inline_hit`). A new
per-frame text registry takes the window id on every register and read, and its
reset clears only that window's entry; `getPaintedText()` and
`getPaintedHighlights()` read the window they serve.

## Text is grouped, because React splits one line into many nodes

`shouldSetTextContent` is false, so `<text>Hello {name}!</text>` becomes **three**
host text nodes. Anything that reasons about a line (paint, copy, `highlight`)
must merge them first, and all of them must agree on where a line starts and ends.

`text::inline::plan_children` is that rule, and it is CSS inline formatting, not
the shape of the tree: in a block container each run of inline children is one
paragraph, a block child ends the run, and a flex or grid container blockifies
its children. Paint builds a paragraph as one `selectable_text` (`paragraph_content`),
and `search::collect_into` walks the same `plan_children`. `search::group_id` is
the matching identity for a lone adjacent text leaf. Never take the group from
`element.parent` for one consumer and adjacency or `display` for another: a
non-text sibling between two leaves ends the run for search but not for copy.
`None` means a run that never merges, which is every native element line.

## `highlight` resolves per subtree, with two caches

`crate::text::search` deliberately builds **no joined document** for a query. It
matches per group, so a 5k-row chat is 5k small strings rather than one megabyte
string rebuilt on every keystroke. A joined document exists only when a spec
supplies explicit `ranges`, because only then does someone index into it.

`HighlightCacheEntry` has two levels and they must stay separate:

- the `GroupList` is keyed by **`search_revision`**
- the `MatchSet` is additionally keyed by `HighlightSet::matcher_hash()`, which
  **excludes** `activeIndex` and the colours; a cursor move swaps only the spec

`search_revision` exists because `highlight` is itself a custom prop, so
`subtree_revision` moves on every keystroke. Key the group list on that and a
find-bar keystroke re-walks and re-folds the whole subtree.

**A timing budget does not catch this.** On the 1000-turn chat the broken version
is 2.7ms against 1.9ms, because most of that text lives in native element props
rather than retained `<text>`. `highlight_cache_tests` in `renderer.rs` compares
`Arc` identity instead, and fails outright.

Native elements (`<code>`, `<markdown>`, `<diff>`) generate text inside
`render()`, so the build-time resolver cannot see it. They match the exact string
they are about to paint, through `washes_for_native_run`. Do **not** add a second
traversal that re-derives their text and `sub` values; markdown assigns `sub`
with a render-time counter and the two would drift.

**Ordinals are allocated during paint, inside `search::wash`, for retained and
native matches alike.** `activeIndex` means the nth match in the document, and
only paint knows that order: retained matches are located before the frame,
native text exists only during it, and a subtree can interleave them. Numbering
each kind separately made `activeIndex: 0` mark the `<text>` match even when a
`<code>` block came first.

Two things that look redundant there are not. `MatchId::Retained` carries the
build-time index so a match split over several interpolated runs takes exactly
one ordinal. The `assigned` memo makes a row gpui paints twice keep its numbers
rather than advance the cursor again.

Paint only sees what is mounted, so a **virtualized subtree must say where it
starts**. `matchIndexOffset` is the number of **matches** above the window, not
a row index, and the sequence begins there. Without it `activeIndex` silently
means "the nth visible match" and a find cursor lands on the wrong row.
`<virtual-list>` already takes `windowStart` and `itemCount` from the app for the
same reason. It is excluded from `matcher_hash`, like `activeIndex`, so scrolling
never rescans text. A malformed value **rejects the whole spec**: a bad offset
only shows up as a cursor on the wrong row, so silence is worse than nothing.

`useTextSearch` takes both numbers as one `matches: { total, indexOffset }`
object. They are never individually correct: native counts and numbers the same
window, so an app that overrides one must override the other.

`onHighlight` is queued during build and flushed **after** the root build
returns, keyed on `MatchSet::identity()` rather than the count. Emitting inline
lets a `setState` in the handler re-enter the build; keying on the count alone
misses a query swap that finds the same number of hits, and including the
colours makes a cursor move look like a new result. `reported` is written only
when an event is really queued, or adding `onHighlight` after the first render
would report nothing forever.

A `<virtual-list>` row is built from `cx.processor` **after** the root render
returned, so `build_virtual_child` re-resolves the declaration against the tree
as it is then. On Windows and Linux the Node thread can commit new text in
between, and a captured range would paint over the wrong glyphs.

Content is searchable even under `userSelect: "none"`, because a browser still
finds it. `chrome_text` cannot paint a wash, so it is only for real chrome:
gutters, language tags, diff file headers.

## Layout numbers live in `Theme::metrics`, not in Rust constants

Row heights, gutter widths, paddings, text sizes and the heading scale are all
fields on `crate::theme::Metrics`, reachable from JS as `theme.metrics`.

**Do not add a new `const` for anything that decides layout.** Put it on
`Metrics`, give it a default, add it to `MetricsOverride`, `hash_into`, and the
`GpuixMetrics` TypeScript interface. The whole point is that a design tweak is a
React re-render, not a native rebuild.

Two things stay constant, because they are paint geometry and cannot move a
glyph: the table hairline, and the inline-code wash overhang.

`<diff>` derives its virtualized height model from the metrics without
measuring, so `DiffElement` re-runs `reset_with_uniform_height` whenever
`Metrics::hash_into` changes. Forget that and the scrollbar drifts from the
content.

## Iterating on the Rust side

There is no hot reload and there cannot be: `require()` of a `.node` calls
`process.dlopen`, Node has no unload, and the event loop, GPU device, window and
selection registry all live in thread-locals of the loaded library.

Use `bun run dev` (see `scripts/dev.ts`). It watches `packages/native/src`,
rebuilds, and re-renders the screenshot tests. **A Rust edit reaches fresh PNGs
in about 4 seconds.** Prefer screenshot mode over `--app`: PNGs in
`packages/react/screenshots/` can be read by an agent, a live window cannot.

**Never ship or start the app on a debug native build.** `bun run build:debug`
and `cargo build` without `--release` produce an unoptimized `.node`. GPUI
paint is then many times slower, and that looks like an app bug. Always use
`bun run build` in `packages/native` (release). Use `build:debug` only when
the user asks, or when a debug-only tool (lldb, sanitizers) cannot run on
release. After any debug build, rebuild release before starting `chat.tsx`
or judging frame time.

## Two Bun modes, only one of them refreshes React

`bun --hot chat.tsx` is the **runtime**. It re-evaluates the module graph in the
same process, so `render()` finds its window on `globalThis.__gpuixRenderHost`
and remounts. There is no bundler, so there is no Fast Refresh transform and no
`import.meta.hot`. Every save loses `useState`. Tracked upstream as
[oven-sh/bun#40179](https://github.com/oven-sh/bun/issues/40179).

`bun scripts/web.ts` is the **bundler dev server**. Bun applies the Fast Refresh
transform and its HMR runtime calls `refreshRuntime.injectIntoGlobalHook(window)`,
which is the only thing our reconciler needs: `injectIntoDevTools()` in
`reconciler.ts` hands the hook `scheduleRefresh` and `setRefreshHandler`, and
`react-refresh` drives updates through them.

Delete that call and you get **no error and no page reload**. Bun still accepts
the update and still calls `performReactRefresh()`, which iterates zero mounted
roots and schedules nothing. The bundle changes and the painted UI stays stale.
`fast-refresh.test.tsx` is the only thing that catches it.

Do **not** assert on the return value of `injectIntoDevTools()`. It ends in
`hook.checkDCE ? true : false`, and `react-refresh` installs no `checkDCE`, so a
working injection still returns `false`. `fast-refresh.test.tsx` asserts the
behaviour instead: `_getMountedRootCount()`, then a component swap that keeps
state.

**Never add `import.meta.hot.accept("./app", ...)` to a browser entry.** Bun runs
an importer's dependency-accept callback even when the imported module already
self-accepted for Fast Refresh. The callback then remounts on top of a
successful refresh and wipes every hook. This looks exactly like Fast Refresh
being broken, and it is not.

**Never run `bun run clean` in `packages/react` while the web dev server is up.**
That folder is inside the dev server's module graph, so removing it under a
running server permanently corrupts the registry: every page load then fails
with `Failed to load bundled module 'packages/react/dist/index.js'`, even after
a hard reload, and only a server restart clears it. This is why `build` is plain
`tsc` and the wipe lives in a separate `clean` script. Clean first, then start
the server.

## A new element needs a host-derived GPUI id, or it has no state

`.id(..)` is not decoration. gpui keys `InteractiveElementState` off the
`GlobalElementId`, so an element without one silently loses **hover, its scroll
offset, its accessibility node, and any element state gpui itself keeps**.
`<img>` had no id, which is why an animated GIF never left frame zero:
`ImgState` holds the frame index.

The root of every host element, built-in or custom, uses
`events::ids::host_element_id(host_id)`, which is `ElementId::NamedInteger("gpuix", host_id)`.
That variant is how the dispatcher maps GPUI's hit chain back to retained
elements (`host_id_of`), so it is reserved for the root. Inner parts (a list
row, a code block's scroll body, an editor's inner view) use
`ElementId::Name("__gpuix_<kind>_<…>")`; they are GPUI details and never a
pointer target. Per-row ids are formatted names too: `__gpuix_diff_line_{ix}`,
`__gpuix_md_table_{id}_{sub}`.

A custom element builds its root through `custom_surface` in
`custom_elements/mod.rs`. It inserts the hitbox (`host_hitbox`: always a hitbox,
`PassThrough` under `pointer-events: none`), applies the style, defaults
`position` to static, records bounds, and wires the accessibility node. Give the
root its `host_element_id` first.

`hover`, `active`, `groupHover`, `groupActive`, `groupFocusWithin`, the relational `hasHover`,
`hasActive` and `hasFocus`, and the control states `disabled`, `enabled`, `hasDisabled` and `hasEnabled` are folded into the style in
`build_element` before any builder runs, so a builder only applies the style it
is given (`apply_interactive_styles`). `focusVisible`, `focusWithin` and `hasFocusVisible` need the
focus state, so `focus::apply_focus_styles` and `apply_focus_within` apply them
where the element calls `track_focus`.

## A macOS menu item owns its shortcut, so the window never sees it

`crate::app_menu` installs the App and Window menus during `init_macos`. GPUI
does not do this on its own: `NSApp.mainMenu` stays nil, macOS paints an empty
menu bar, and `⌘Q`, `⌘H`, `⌘M` and `⌘W` do not exist, because AppKit only
provides them through menu items.

**Never add an Edit menu carrying `⌘C` / `⌘V` / `⌘X` / `⌘A`.** AppKit consumes a
key equivalent before the window sees the key event, so those items would take
the keystroke away from the selection listener in `text::paint` and from the
per-focus clipboard handling in `custom_elements::input`. An Edit menu needs
those handlers moved into GPUI actions first.

`gpui::App::set_menus` reads each shortcut out of the keymap, so bind the keys
**before** you call it. Window-level items (`MinimizeWindow`, `ZoomWindow`,
`CloseWindow`) go through `with_window_menu_actions` on the root element in
`GpuixView::render`, because a `Window` exists nowhere else; app-level ones
(`Quit`, `Hide`, `HideOthers`, `ShowAll`) are `cx.on_action` globals.

Two things real AppKit decides for you. The **title of the application menu is
the executable name**, not the `Menu` name you pass, so `bun app.tsx` shows
`bun`; only a `.app` bundle changes it. And the menu named `Window` is handed to
`setWindowsMenu:`, which prepends AppKit's own tiling items, `Enter Full Screen`
included. Do not add that item yourself.

Verify with the accessibility tree, not a screenshot. The system menu bar is
outside the window, so GPUIX automation cannot see it.

```bash
osascript -e 'tell application "System Events" to tell (first process whose unix id is PID) \
  to get name of every menu bar item of menu bar 1'
```

## Browser keyboard input goes through GPUI's hidden element

A GPUI web app has two event surfaces. The `<canvas>` takes pointer events.
A hidden `[data-gpui-input]` element appended to `<body>` takes **every keyboard
and IME event**: `gpui_web`'s `listen_input` attaches `keydown` / `keyup` there,
not to the window or the canvas. Dispatching a synthetic `KeyboardEvent` at that
element is therefore the only way automation can type into a browser app.

**Match it by attribute only** (`IME_MIRROR_SELECTOR` in `automation/client.ts`).
The element's tag belongs to `gpui_web`: it is a `<textarea>`
([zed-industries/zed#63201](https://github.com/zed-industries/zed/pull/63201))
because a single-line input strips newlines from an assigned value and
desynchronises the mirror from the document. A tag-qualified selector fails every
browser keystroke the next time that tag changes.

## Virtualized React children re-enter through `cx.processor`

`<virtual-list>` does not build its retained children during `GpuixView::render`.
Its `gpui::list()` callback uses `cx.processor` to re-enter the `GpuixView`
entity after the root render has returned, creates a fresh `BuildCtx`, and builds
only the rows GPUI requests. Never capture the root render's tree guard or
`BuildCtx` in that callback.

`<diff>` still owns its parsed Rust data because one native diff node is much
cheaper than retaining one React node per line.

## A prepended row is only visible at the top

`gpui::ListState` anchors on a **logical item**, and `splice_focusable` shifts
that anchor by the number of rows inserted before it. So a prepend keeps the
rows already on screen and pushes the new one above the viewport. That is
correct for a history pane, and wrong for a feed.

A browser anchors the same way and suppresses it at `scrollTop: 0`. GPUIX copies
that: `VirtualListEntry::sync` remembers a top-aligned, non-`followTail` list
whose `logical_scroll_top()` is `{0, 0}` and calls `scroll_to(default)` after the
splice. Do not "simplify" that away.

**Do not trust a short list to prove a prepend works.** While the content is
shorter than the viewport, gpui's "does not fill" branch re-anchors to item 0 on
every layout, so the drift is invisible. It appears on the frame where the list
first overflows. `example-app` looked stuck on two rows for exactly that reason,
and the regression test in `virtual-list.test.tsx` grows a 160px list from 2 rows
to 12 rather than starting tall.

**A loading row is the anchor while the reader waits in it**, so an
infinite-scroll prepend splices the page in *under* it and replaces the screen
the reader was looking at. The splice-shift above only protects an anchor
*below* the insert point. The app owns the correction, because only it knows the
loading row stands for the arriving content: read `getListScrollTop`, commit,
then `scrollToItem(listId, indexOfTheMessageUnderTheVoid, offsetInVoid -
EDGE_HEIGHT)`. The negative offset anchors the viewport top above that row and
gpui resolves it at layout time against the freshly measured new rows, which is
what makes the restore pixel-exact; any pixel math done in JS would trust
`estimatedItemHeight` and still jump. The append twin: a reader waiting at a
trailing loading row usually rests on gpui's **at-end sentinel**
(`itemIndex == item count`, stored `logical_scroll_top` is `None`), not inside
the void, so the offset is meaningless there; convert with the viewport height
from the same tuple (`EDGE_HEIGHT - viewportHeight`). Traps that cost a
session each:

- virtual-list `scrollToItem` is **queued and applied after the next render's
  splice** (`PENDING_VIRTUAL_LIST_SCROLLS` in `renderer.rs`). Applying it
  eagerly let `splice_focusable` shift the just-restored anchor a second time
  on the live renderer, while the test renderer hid it because
  `TestRenderer.scrollToItem` flushes first
- a bottom-aligned list with a trailing loading row starts **scrolled to the
  end**, i.e. showing that loading row. In tests, wheel direction is therefore
  ambiguous at mount: the first wheel tick can trigger a `next` fetch even when
  the test means to scroll up. Start from the latest page (no trailing edge) or
  `scrollToItem` onto content first. `infinite-chat.test.tsx` does both

## A second pane cannot follow native scroll

GPUI moves a scroll container on the wheel frame. The `onScroll` callback that would move a sibling pane arrives a frame later, so a ruler synced that way tears away from its content during a fast pan. A header inside the **same** scroller needs none of this: use `position: sticky`.

When two panes in different containers must stay locked to the pixel, **React owns the offset**: one `onWheel` listener on a non-scrolling parent, `scrollX` / `scrollY` in state, and one absolutely positioned wrapper per pane carrying the translation. Zed does the same; the editor owns its scroll position and paints the gutter and the text from it. `examples/timeline.tsx` is the worked example.

Those wrappers must set `pointerEvents: "none"`. A positioned box is a hit target even with no fill, as in a browser, so otherwise it swallows every press meant for the surface behind it. `pointerEvents: "none"` **inherits**, so anything inside a wrapper that must still be clickable sets `pointerEvents: "auto"`.

Keep the moving subtree in a `memo` component whose props do not change during a pan. Then a wheel costs a handful of style mutations instead of one per row. `examples/timeline.perf.test.tsx` measures both halves.

## Scroll cost

A wheel event calls `cx.notify` on the one `GpuixView`. That rebuilds the
tree. `gpui::list()` then re-renders every **visible** item. Cached heights
only skip overdraw items that are off screen.

```
wheel  ►  notify GpuixView  ►  render()  ►  Taffy on visible rows  ►  paint
```

If scroll is smooth on empty padding and slow or stuck over a child, look at who
claimed the delta. The wheel goes to scrollers that are ancestors of the hit
target, so a nested scroller, `<virtual-list>` or `<diff scroll>` under the pointer
takes its share first, and an `onWheel` handler that calls `preventDefault()`
cancels the scroll outright. Remaining cost is Taffy on fat visible rows.
`<code>` is one flex row per line. Safe-mdx is ~100 host nodes. Flatten paint
before changing the frame loop.

Keep `<virtual-list>` `overdraw` modest. 820px on a short chat kept almost
every row live. Profile with `debugFrameOverlay: 'full'`. The overlay is
draw time, not FPS. `8.3 MS` is about 120 Hz.

A long `{rows.map(...)}` is slow **at start**. `createInstance` runs in the
render phase. The host `<virtual-list>` children API retains every child. Pass
`itemCount` and `windowStart` and render only that slice so React mounts a
window too. After mount, scroll cost is visible Taffy only.

**There is no `VirtualList` wrapper component and there must not be one.** The
window is app state. A generic wrapper cannot know when to widen its own
window, so it silently dropped rows whenever `itemCount` grew without a scroll,
which is exactly what a filter does.

Keep chrome state out of the component that maps the list. `memo(Transcript)`
so a sidebar click or composer keystroke does not remap every row. A 5k-row
chat paid 250ms per click before that. Profile that path with
`INTERACT=1 bun profile-chat-scroll.tsx`. Do not treat a fast wheel flush as
proof that chrome updates are cheap.

## Profiling and optimizing

Load the **profano** skill first. Fetch its README. Do not guess CLI flags.

Separate **first mount**, **scroll**, and **chrome setState**. They are
different paths.

```
first mount
  React maps every child
    ►  createInstance / setStyle / setCustomProp  (queued)
    ►  one applyBatch JSON
    ►  Rust RetainedTree
    ►  first paint (list builds visible rows only)

scroll
  wheel  ►  notify GpuixView  ►  render()  ►  Taffy on visible rows  ►  paint

chrome setState
  sidebar click / composer key
    ►  parent re-render
    ►  {rows.map(...)} again unless memo(list)
    ►  same JS cost as mount if you forget
```

### JS / mount

Write a short script that mounts through `createTestRoot()` and exits. Profile
that, not the live window. The tick loop will drown the mount.

```ts
import React from 'react'
import { createTestRoot } from '@gpuix/react'
import { ChatApp } from './chat'

const root = createTestRoot()
const start = performance.now()
root.render(<ChatApp turnCount={10_000} />)
console.log(`mount ${(performance.now() - start).toFixed(1)}ms`)
```

```bash
cd examples
MOUNT_ONLY=1 bun --cpu-prof --cpu-prof-dir=../tmp/cpu-profiles profile-chat-scroll.tsx
INTERACT=1 bun profile-chat-scroll.tsx
npx profano ../tmp/cpu-profiles/CPU.*.cpuprofile -n 30
npx profano ../tmp/cpu-profiles/CPU.*.cpuprofile --sort total -n 20
```

Read **self** first. That is where the CPU sat. **Total** is the caller chain.

The 10k chat mount was 850ms. profano said:

| Function | Self | What it was |
|---|---|---|
| `applyBatch` | 626ms | Rust parsing the mutation JSON |
| `FiberNode` | 31ms | React |
| `stringify` | 26ms | `JSON.stringify(queue)` |

React was not the problem. The batch **stringified every style and theme**, then
stringified the queue, then Rust parsed each escaped string again.

Queue **raw objects**. `setStyle` and `setCustomProp` both carry raw JSON values.
Never encode either value before the outer batch is stringified. Doing so adds
a second parse and turns strings such as `"top"` into nested JSON.

```ts
queue.push(['setStyle', id, styleObject])
queue.push(['setCustomProp', id, 'side', 'top'])
```

After a JS reconciler change, **build `@gpuix/react`**. `examples/` and
`bun --hot chat.tsx` load `packages/react/dist`, not `src`. packages/react
vitest uses `src`. You will think the fix works in one suite and fail in the
app.

```bash
cd packages/react && bun run build
```

### Scroll / paint

Turn on `debugFrameOverlay: 'full'`. The number is **draw time**, not FPS.
`8.3 MS` is about 120 Hz.

Keep `overdraw` modest. 820px on a short list keeps almost every row live.

Do not flatten the frame loop to hide fat rows. Flatten the rows
(`<markdown>` / `<code>` / `<diff>` as one native node).

### Native

For Rust time, `sample` the bun/node pid, or `samply`. GPUI also has
`ZED_MEASUREMENTS=1`. That is Zed's frame log, not our overlay.

A `.node` cannot unload. After a native rebuild, restart the app. `bun --hot`
only remounts React.

### Mutation wire format and tree memory

Do not swap the `applyBatch` codec before reading
[docs/serialization-benchmark.md](./docs/serialization-benchmark.md). It
measures both halves on the real `ChatApp` queue, and the answer is that the
codec is the **smallest** lever.

```bash
cd examples && TURNS=10000 SAFE_MDX=1 bun run bench:serde
cd packages/native && cargo run --release --example bench_serde
```

Five results that shaped the current code:

- `StyleDesc` is about 1.4 KB (1392 bytes when it was measured, and every field
  added since only grows it). Putting it in an enum variant makes the whole
  `Vec<BatchOp>` as wide, so a 221k-op mount reserved 312 MB before parsing
  anything. Never inline a style in an op: `BatchOp::SetStyle` keeps the raw JSON
- **styles are hash-consed in Rust, not in the protocol.** `StyleTable::intern`
  (`retained_tree.rs`) hashes the raw payload and shares one `Arc<StyleDesc>`. Do not move this into
  JS: `commitUpdate` resends the full style every commit, a dragged element
  produces a distinct style every frame, and a JS-owned table would grow forever
  while sending two ops where it sends one today
- `StyleTable::maybe_sweep` runs after batches, and `sweep` drops entries with
  `Arc::strong_count == 1`. That is the only thing keeping the table bounded
- `RetainedElement.style` is `Option<Arc<StyleDesc>>`. Read it with
  `.as_deref()`. The motion path must copy out before mutating, or one
  element's animation restyles every element that declared the same style
- `RetainedElement.style` is the **effective** style: what the element declared
  (`own_style`) over the universal layer (`setUniversalStyle`, a CSS `*` rule;
  `universal_style.rs`). Only `RetainedTree::assign_style` writes it, for
  `set_style`, `set_universal_style` and a string landing on a `text` element
  (text nodes get no layer), so its bookkeeping lives once. `StyleTable` holds
  the layered styles per own style, and `sweep` releases them with it

No mainstream JS↔Rust codec deduplicates repeated string **values**. msgpackr
`useRecords` deduplicates keys, and its output is not plain MessagePack, so
`rmp_serde` cannot read it. MessagePack measured 1.24x, which does not pay for a
new dependency on both sides.

## Overlays and icons

Menus, tooltips, and dialogs go through **`SelectContent` / `ComboboxContent` /
`<anchored deferred>`**. Never overflow a `position: "absolute"` card out of the
composer into a `<virtual-list>`. A positioned card paints above later siblings of
its own parent only, so the list, a later sibling subtree, paints over the menu and
the clicks hit the text behind it.

Do not paint `#00000000` over a blurred window. A transparent GPUI quad punches
through Metal to the desktop. Omit the fill, or use the parent color. Overlay
rows need a **solid** fill too, not a transparent idle state.

**Pointer rules are the DOM's.** The target is the topmost non-`none` element
where it is painted, whatever it paints or listens for. A filled or positioned
`div` hides nothing from its ancestors: they still get the event as bubbling and
are still hovered. An overlay that must keep clicks from reaching what is behind
it stops them with `stopPropagation()` (or is an `<anchored>` layer, which is its
own hitbox root). The wheel reaches only scrollers that are ancestors of the
target, so a card over an unrelated scroller does not scroll it, and a modal does
not scroll the page it was declared in.

`pointerEvents: "none"` makes the element never the target; it stays in the path
of its descendants' events, so its own listeners still hear them. It
**inherits**: a descendant is also not a target until one sets
`pointerEvents: "auto"`, which is how a clickable control sits inside a `none`
layer. An absolutely positioned wrapper with no fill still takes hits, like an
empty positioned `div` in a browser, so a wrapper that only carries a scroll
offset must set `none`, and its interactive children `auto`.

Text **selection** still uses window mouse events and text bounds, not hitboxes.
A drag on a menu over markdown can still start a selection. Do not skip
selection tests to hide that.

If `<svg>` icons are blank in vitest, `src` is probably a `data:image/svg+xml`
URL from `import … with { type: 'file' }`. Native decodes that URL. Do not write
a temp-file workaround. `currentColor` in the file is the element's `color`:
prefer it to a hard-coded `#000` that relied on `style.color` to tint.

macOS traffic-light clearance is **86px**. The test renderer does not draw
traffic lights, so that gap looks empty in PNGs.

## Ported code

`text/`, `syntax/`, `markdown/`, `diff/`, `theme.rs`, `custom_elements/code.rs`,
`custom_elements/diff.rs`, and the caret blink sections of
`custom_elements/input.rs` are ported from [Comet](https://github.com/zeronsh/comet)
(MIT). Each file names its original in
its header, and `THIRD_PARTY_NOTICES.md` has the full table. When fixing a bug in
one of them, read the Comet original first: it usually documents why the code is
shaped that way.

## Auto-generated files (do NOT edit manually)

The following files in `packages/native/` are auto-generated by napi-rs during `bun run build`. Never edit them by hand — they are regenerated from the Rust `#[napi]` annotations every build:

- `packages/native/index.d.ts` — TypeScript type declarations
- `packages/native/index.js` — ESM Node.js loader/binding glue
- `packages/native/index.cjs` — Hermes CommonJS loader/binding glue
- `packages/native/*.node` — compiled native binary

To update the TypeScript API surface, edit the Rust source files in `packages/native/src/` (add/modify `#[napi]` structs, methods, functions), then run `bun run build` in `packages/native` to regenerate.

## Changesets

**Always** add a `.changeset/*.md` file after a user-facing fix or feature. Do this before you consider the work done. Never skip it. Never edit CHANGELOG.md. Never bump `package.json` version by hand.

Load the `changesets` skill for format and rules. If the change fixes a GitHub issue or should close a PR, put `Fixes #N` / `Closes #N` on its own line. changepub copies those onto the release commit.

## Publishing

**Never publish from a local machine.** CI is the only release path.

Exception: `disktree` ships `tsc` output and no binary, so publish it
locally with `bun publish` in `disktree/` from a clean checkout of `main`.
`prepublishOnly` wipes `dist/` and rebuilds; run `bun run test` first. Before you publish, check that
the `@gpuix/native` and `@gpuix/react` versions in the repo are already on npm,
because `workspace:^` becomes a range on those versions.

`.github/workflows/ci.yml` builds `@gpuix/native` for **four native targets**,
uploads the `.node` artifacts, then the `publish` job downloads them, runs
`napi create-npm-dirs` + `napi artifacts`, and publishes `@gpuix/native`,
`@gpuix/react`, and `@gpuix/solid`. The independent `publish-cli` job tests and
publishes `@gpuix/cli` without waiting for native platform tests.

| OS | Target | Renderer | Features |
|---|---|---|---|
| macOS | `aarch64-apple-darwin` | Metal | `test-support` |
| Linux | `x86_64-unknown-linux-gnu` | Vulkan / wgpu | `--no-default-features` |
| Windows | `x86_64-pc-windows-msvc` | Direct3D | `test-support` |
| Windows ARM64 | `aarch64-pc-windows-msvc` | Direct3D | `test-support` |

Windows ARM64 builds on `windows-11-arm`, with Visual Studio's ARM64 Clang on
`PATH` for `ring`. The Windows test job runs the same binding, renderer, React,
Solid, and example checks on x64 and ARM64. Publishing waits for both.
Every extra target is a full gpui build, and gpui is most of the wall clock here.
**The matrix and `napi.targets` in `packages/native/package.json` must list the
same set.** A target in one and not the other means the published loader looks for
a platform package that CI never built. Add a target back when someone asks for it.

Each build job also compiles `examples/chat.tsx` with `bun build --compile` against
that target's `.node`. A release asset is served as raw bytes, so a download loses
the executable bit and, on macOS and Linux, arrives with no extension. The job packs
those two into `example-chat-<target>.tar.gz`, which keeps the mode and names itself.
Windows ships the `.exe` as it is. On `main`, the publish job attaches them to the
`@gpuix/react@x.y.z` GitHub release.

Publish order is required. Both adapters depend on `@gpuix/native`
(`workspace:^`). If an adapter publishes first, an install in that window
cannot resolve native.

1. `napi pre-publish` publishes the per-platform packages (`darwin-arm64`, `linux-x64-gnu`, …)
2. `npm publish` publishes `@gpuix/native`
3. `npm publish` publishes `@gpuix/react`
4. `npm publish` publishes `@gpuix/solid`

`@gpuix/cli` publishes independently. It resolves the latest published React
version when `gpuix new` runs, so it has no package dependency on this sequence.

A local `npm publish` / `bun publish` would ship only the host binary and break every other platform. `prepublishOnly` exits if `CI` is unset.

### Create the GitHub release before CI gets to the publish job

The example binaries are attached by tag, and **the upload step gives up when that
release does not exist**:

```bash
if ! gh release view "$TAG" >/dev/null 2>&1; then
  echo "No GitHub release for ${TAG}, skipping example upload"
  exit 0
fi
```

So a release created afterwards gets **no binaries**, and the only way to add them
later is to download ~100 MB of artifacts to a laptop and push them back. There is
no API that attaches an artifact to a release: the assets endpoint wants the raw
bytes in the request body, and `gh release upload` cannot read stdin
([cli/cli#5820](https://github.com/cli/cli/issues/5820)). The bytes always move.
Do not let them move through your machine.

Release order:

1. Consume the changesets, bump all three packages, write `CHANGELOG.md`, commit
2. `git push origin HEAD:main`. CI starts
3. Tag and create the releases **now**, while the native builds run:

```bash
git tag '@gpuix/native@0.5.0' && git tag '@gpuix/react@0.5.0' && git tag '@gpuix/solid@0.5.0'
git push origin '@gpuix/native@0.5.0' '@gpuix/react@0.5.0' '@gpuix/solid@0.5.0'
gh release create '@gpuix/react@0.5.0' --title '@gpuix/react@0.5.0' \
  --notes-file /tmp/react-notes.md --latest
gh release create '@gpuix/solid@0.5.0' --title '@gpuix/solid@0.5.0' \
  --notes-file /tmp/solid-notes.md --latest=false
```

4. CI publishes npm, then uploads `example-chat-*` to the React release with `--clobber`

**Each adapter gets its own GitHub release**, because a Solid user never installs
React and will not look under a React tag. Native has no release: users reach it
through an adapter, so its changes go into both adapters' notes. Skip the Solid
release only when nothing in the version affects Solid users.

The React release stays `--latest` because it carries the example binaries; the
upload step looks it up by name. GitHub marks only one release as latest, so Solid
uses `--latest=false`. It is still a normal, visible release.

The `publish` job only runs after every build and both test jobs, which is more than
ten minutes, so step 3 has plenty of slack. Write the notes from the current
`CHANGELOG.md` section, keeping only items relevant to each adapter. Never
`--draft`, never `--prerelease`.

**If CI fails and you push fixes, re-point the tags.** The tag then names an older
commit than the one npm was built from. Delete all three tags locally and on the remote,
recreate them on the commit that published, and push again. The upload step matches
on the tag *name*, so the release itself keeps its notes and its assets.

## Building

### Standalone Build

The `zed/` submodule is `fdarian/zed` on `poc` (`.gitmodules`). Cargo uses path
dependencies from that submodule so the native addon and native platforms always
compile from the same source:

**Keep `zed/` on the local `poc` branch. Never leave the submodule in detached
HEAD state**, including after `git submodule update` or a pointer update. If Git
detaches it, switch back to `poc` before doing any other work. A fresh worktree
gets zed with `git submodule update --init zed`, which clones `fdarian/zed` from
GitHub.

- macOS uses `MacPlatform::new_embedded()` and pumps AppKit on Node's main thread
- Windows and Linux run `gpui_platform::application().run()` on a dedicated UI thread
- `gpui_platform` selects Metal or DirectX for the GPU-backed test renderer
- `core-text = 21.0.0`, `core-graphics = 0.24.0` for macOS

These avoid the core-graphics 0.24 vs 0.25 conflict between `core-text` and Zed's `font-kit` fork.

### Rust toolchain

`rust-toolchain.toml` pins the same channel as `zed/rust-toolchain.toml`. When the
submodule moves, update ours to match or GPUI may not compile.

### Metal toolchain (macOS)

`gpui_apple` compiles `shaders.metal` in its build script. Xcode 26 no longer ships the
Metal compiler by default, so a fresh machine fails with
`cannot execute tool 'metal' due to missing Metal Toolchain`. Install it once:

```bash
xcodebuild -downloadComponent MetalToolchain
```

### Bumping the gpui revision

`scripts/sync-upstream.sh` rebases both repos onto upstream and moves the
submodule ([working-on-the-fork.md](./docs/fork/working-on-the-fork.md#syncing-with-upstream) has the procedure and the
conflict hot spots). After it:

1. Match `rust-toolchain.toml` to `zed/rust-toolchain.toml`.
2. Run `cargo check --all-targets`, `bun run build`, and the test suites, including
   `cargo test -p gpui --lib --features test-support` in `zed/`.

### Search Zed before you touch GPUI

Before you debug a GPUI behaviour, add a GPUIX feature that needs a new GPUI API,
or patch the fork, **search `zed-industries/zed` first**. Zed is a large project
with an active roadmap. The answer is often one of:

- someone already reported the same bug
- an open PR already implements the API, so **wait and bump the submodule**
- a merged PR already added it, so **bump the submodule** instead of writing code
- a closed issue says the Zed team declined it, so plan a fork-only fix

Search issues and PRs together, then search code:

```bash
# issues + PRs, full text
gh search issues --repo zed-industries/zed --include-prs --limit 30 'TransformationMatrix' \
  --json number,title,url,state,isPullRequest \
  --jq '.[] | [.number, .isPullRequest, .state, .title, .url] | @tsv'

# title only, to find the feature rather than every mention
gh search issues --repo zed-industries/zed --include-prs --match title --limit 30 'transform'

# where the API already exists in the tree
gh search code --repo zed-industries/zed --language Rust --limit 30 'TransformationMatrix'
```

Then read the promising ones in full. A closed issue is the important signal, and
its `stateReason` and comments explain whether the idea was rejected or shipped:

```bash
gh issue view 53303 -R zed-industries/zed --json number,title,state,stateReason,body,comments
gh pr view 59413 -R zed-industries/zed --json title,state,body,files,comments,reviews
```

**Use the `--repo` and `--match` flags. Do not put `repo:` or `in:title` inside the
query string.** `gh search` mangles the inline form: `repo:` first fails with
`Invalid search query`, and `in:title ... repo:owner/name` silently drops the repo
filter and returns results from unrelated repositories.

Search the real symbol names, not concepts. `TransformationMatrix`,
`with_element_offset`, and `request_animation_frame` find the discussion.
"animation is slow" does not.

Record the outcome in the changeset or PR body, with issue and PR URLs, so the
next session does not repeat the search.

### Fixing GPUI for GPUIX

The `zed/` fork is part of GPUIX's implementation boundary. Fix GPUI in it when a
reusable GPUI API or platform correction keeps GPUIX simpler and avoids browser,
embedded-platform, or event-routing workarounds. Do not keep a hack in
`packages/native` only because the required API is missing upstream.

Fork-only fixes are normal commits on zed `poc` (`fdarian/zed`), pushed before the
GPUIX submodule points at them. Never pin GPUIX to a commit that is not reachable
from that remote branch, and never reset or detach `zed/` to "undo" work. [Working on the fork](./docs/fork/working-on-the-fork.md#repos-and-branches) has the branch layout and
[the commit stack](./docs/fork/working-on-the-fork.md#the-commit-stack) says which gpuix commit may move the `zed` pointer. Test zed changes where they
live: `cd zed && cargo test -p gpui --lib --features test-support`.

Make a GPUI change that alters behaviour for every user opt-in per window, the way
DOM hit testing is (`Window::set_dom_hit_testing`, default off). It keeps the
change small to review and possible to send upstream.

### PRs to Zed

A "PR to Zed" means **upstream** [`zed-industries/zed`](https://github.com/zed-industries/zed)
`main`, a different thing from a fork fix above. Never open one from this checkout,
and never switch `zed/` to another branch for it: that submodule is what GPUIX
builds against, and a dirty or switched `zed/` breaks the native addon and the test
renderer. Make a separate worktree from `zed-industries/zed` `main` (the zed
clone's `upstream` remote is `remorses/zed`, so add upstream Zed as its own remote),
commit there, and cherry-pick back onto `poc` after the PR merges.

### PRs to GPUIX

When you open a PR with `gh pr create` against **this repo** (`fdarian/gpuix`, or upstream `remorses/gpuix`),
the body must name the **harness**, **agent**, and **model** that wrote the
change. Then put **every user prompt** from the session in a collapsed
`<details>` block. Reviewers use that to judge prompt quality and how much
the agent invented.

Do this for `gh pr create` and for later `gh pr edit` if the first body missed
it. Do not add this block to Zed PRs.

```md
**Harness:** OpenCode / Kimaki
**Agent:** build
**Model:** xai/grok-4.6

<details>
<summary>User prompts</summary>

1. first user message, verbatim

2. second user message, verbatim

</details>
```

- **Harness:** the product that ran the agent. Examples: OpenCode, Kimaki,
  Claude Code, Cursor, Codex.
- **Agent:** the named agent if the harness has one (`build`, `plan`, `opus`).
  Write `none` if there is no named agent.
- **Model:** the exact model id from the session (`xai/grok-4.6`,
  `anthropic/claude-opus-4.6`). Do not guess a shorter marketing name.
- **User prompts:** every user message that drove the PR, in order, verbatim.
  Skip system reminders, tool output, and your own replies. If a prompt is
  huge, keep the full text inside the details block; do not summarize it.

## Current Status

User-facing status is the README **Status** section. This list is only the
remaining engineering work.

- [ ] **Background highlighting** - move Syntect off the frame thread once
      there is a way to request a repaint from a background task
- [ ] **Canvas** - custom drawing element (`<canvas>` is typed, not implemented)
- [ ] **Window resize** - expose GPUI's runtime resize API
- [ ] **Multiple windows** - Support multiple GPUI windows
- [ ] **React Refresh on desktop** - `bun --hot` is the runtime, not the bundler, so it runs no Fast Refresh transform. Tracked as [oven-sh/bun#40179](https://github.com/oven-sh/bun/issues/40179)
- [ ] **Native hot reload** - cannot unload a `.node`. `bun run dev` rebuilds and restarts
- [ ] **DevTools** - React DevTools integration
- [ ] **Size morphs** - `layout="size"` as a FLIP scale with the children's counter-scale

## Testing

### Unit Tests

```bash
# Rust unit tests (selection, syntax, diff parser, markdown parser, theme)
cd packages/native && cargo test --lib

# React reconciler + GPU-backed test renderer
cd packages/react && bun run test

# Solid adapter, and the native JS runtime
cd packages/solid && bun run test
cd packages/native && bun run test

# Example app tests
cd examples && bun run test

# Starter todo app, driven through the automation client
cd example-app && bun run test

# Chat and timeline draw / chrome regressions (excluded from the default run)
cd examples && bun run test:perf

# One guard per perf issue (#36-39) and the retained-subtree report; release addon, NODE_ENV=production
cd examples && bun run test:perf:depth       # nested columns lay out in linear time
cd examples && bun run test:perf:animating   # one animating node over 2042 elements
cd examples && bun run test:perf:clipped     # clipped content is skipped, scrolling pays nothing for retention
cd examples && bun run test:perf:draws       # a read draws nothing, one draw per frame
cd examples && bun run test:perf:retain      # which share of each page a frame skips

# Replay against a whole build for every replayed frame (slow; run before landing a retain change)
GPUIX_RETAIN_VERIFY=1 bun run test

# macOS CPU clamp. E-cores, not Chrome 6x. Do not set in CI.
THROTTLE=utility bun run test:perf
THROTTLE=utility bun profile-chat-scroll.tsx
THROTTLE=utility bun --hot chat.tsx
```

`examples/chat.perf.test.tsx` and `examples/timeline.perf.test.tsx` are the
automated profiles. They use `createTestRoot()`, not the live window. Assert
**p95 draw / flush ms**, not a per-frame FPS floor.

Timeline drag samples are not comparable to timeline pan samples.
`nativeSimulateMouseMove` draws a pending change before and after the event, so a
drag sample includes the paint and `dispatchScrollWheel` does not.

`THROTTLE` re-execs under `taskpolicy -c`. `utility` is an M1/M2 Air CPU proxy.
`background` is harsher, closer to a 2019 Intel Mac. GPU and RAM stay on this
machine. `taskpolicy -c` only works at launch. The vitest config wraps the main
process so workers inherit the clamp. A throttled run **logs** numbers and
skips the default budgets. Those budgets are for an unclamped M-series CPU.

Use `bun run test`, not `bun test`. The suites are vitest, so `bun test` picks the
wrong runner and fails on the `vitest` imports.

`packages/native` and `packages/solid` have their own `bun run test`. The native one has a single test that bundles both adapters for the browser; it needs `packages/react` built and `bun run build:web` (nightly Rust and `wasm-bindgen`), and fails without them. The zed commits are tested in zed: `cd zed && cargo test -p gpui --lib --features test-support`.

The test renderer draws offscreen and takes the synchronous dispatch path for every call (see [Events](#events-one-hit-test-one-dispatcher-dom-propagation)), so a passing event test proves the macOS behaviour, not Windows or Linux. Every simulate helper (keys, wheel, pointer, file drag, focus moves) calls `flushIfDirty()` before and after the input, not `flush()`: it draws a pending change and nothing on a clean window, as a live window draws once per dirty input and never unconditionally, so a move costs one draw. Hit testing needs a laid-out frame, which that gives it; the public `flush()` always draws. `nativeSimulateClick` sends the press and the release as two inputs and drains JS between them, as a person lets go later, so whatever the press queued reaches JS before the release. A handler that throws does not fail the simulate call: pass `onError` and assert on it.

The test window opens from the live renderer's `WindowOptions` through the same `to_gpui_window_options` (`TestGpuixRenderer::new`); the README's Testing section lists what each option does there. Simulated moves report the buttons held by simulated downs (`simulateLostMouseUp` forgets one), and `scrollTo` / `getScrollOffset` speak DOM scroll positions (`scroll::gpui_offset` and `scroll::dom_position` are the only sign flip, and `dom_position` plus the writes in `scroll::scroll_to` / `scroll_list_to` are where a position becomes a whole device pixel, rounded half up, as Chrome's is). The appearance override is process-wide (`NSApplication.appearance`), and each new test window sets or clears it.

Simulated keys take Playwright's `keyboard.press` grammar (`Shift+Tab`, `Space`, `KeyA`), parsed in `events/key_names.rs` from the table that names events, and send a `keydown` then a `keyup`; the simulated wheel takes the DOM sign, with `automation::wheel_delta` as the one negation. `TestRenderer.dispose()` (which `createTestRoot().unmount()` calls) drops the window state while every thread-local is alive, and a new `TestRenderer` disposes its predecessor. State nobody disposed is leaked at thread exit (`ManuallyDrop`), not dropped: a destructor that touches a torn-down thread-local aborts the process.

### Asserting on native elements

`getAllText()` reads the retained tree, so it only sees `<text>` nodes. `<code>`,
`<diff>` and `<markdown>` paint inside gpui and are invisible to it. Use
`renderer.getPaintedText()` (every string painted last frame, in paint order) and
`renderer.dragSelect(x1, y1, x2, y2)` instead.

`dragSelect` exists because selection listeners are registered during **paint**:
calling `simulateMouseDown` / `Move` / `Up` by hand without a flush between each
step silently selects nothing.

Screenshots go to `packages/react/screenshots/` (gitignored), not `/tmp`, so they
can be inspected after a run.

### Integration Test

```bash
cd examples && GPUIX_BACKGROUND=1 bun --hot chat.tsx
```

Use tuistory for the long-running process. Do not use `tsx` or raw `tmux`.
On macOS and Windows, the background flag keeps the real GPU window from taking
the user's keyboard; live paint, clicks, screenshots, and automation still
work. Linux currently ignores `focus`.

### Drive the live window

When the user asks to **open an example so they can look**, start
`bun --hot <file>.tsx` and **leave that session running**. A save remounts
React on the same window. Do not relaunch without `--hot`. Do not kill the
session after a screenshot. `bun --hot` still drops `useState` on save; that
is the desktop remount path, not Fast Refresh.

**Do not use `usecomputer`, `screencapture`, or desktop clicks.** GPUIX has a
Playwright-like automation API. Full docs are in the README **Automation**
section.

Mark targets with `testId`. Then either:

- `connectTest(renderer)` against `createTestRoot()` in vitest
- `launch({ command, args })` against a child process. The app serves commands
  on stdin only when stdin is a **pipe**

**Always pass `focus: false` when you start a window to check your own work.**
The user is doing something else. A window that activates on launch takes the
keyboard mid-sentence, once per iteration, and there is no reason for it:
`click()` and `screenshot()` never need focus. Wire the entry file so the flag
comes from the environment, then set it in `launch({ env })`, so a human run
still behaves normally.

```tsx
render(<App />, { focus: process.env.GPUIX_BACKGROUND !== '1' })
```

`fill()` and `press()` use the live GPUI window input pipeline and do not need
the desktop window to become active.

```ts
import { launch } from '@gpuix/react/automation'

const app = await launch({
  command: 'bun',
  args: ['chat.tsx'],
  cwd: 'examples',
  env: { GPUIX_BACKGROUND: '1' },
})
await app.getByTestId('sidebar-collapse').waitFor({ timeoutMs: 30_000 })
await app.screenshot({ path: 'tmp/chat.png' })

const startedAt = await app.clock.pause()
await app.getByTestId('sidebar-collapse').click()
await app.captureFrames('tmp/sidebar', [startedAt, startedAt + 100, startedAt + 200])
await app.clock.resume()
await app.close()
```

`click()` hits the last painted bounds. `clock.pause` / `set` / `fastForward`
freeze native motion. `captureFrames` writes one PNG per timestamp. That is how
you record a sidebar open/close, not a screen recorder.

## Related Projects

- [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) - Zed's GPU UI framework
- [opentui](https://github.com/anomalyco/opentui) - Terminal UI with React (reconciler reference)
- [create-gpui-app](https://github.com/zed-industries/create-gpui-app) - Official GPUI starter template
- [react-reconciler](https://github.com/facebook/react/tree/main/packages/react-reconciler) - React's custom renderer API

## Examples using same tech as ours. To unblock on issues and compare to our code

For example usage of projects depending on gpui in rust: opensrc https://github.com/zed-industries/create-gpui-app

For examples of NAPI rs native packages: https://github.com/napi-rs/package-template and https://github.com/Brooooooklyn/Image

For reading gpui source code: https://github.com/zed-industries/sed inside crates/gpui

For examples of a custom React renderer: https://github.com/anomalyco/opentui inside packages/react
