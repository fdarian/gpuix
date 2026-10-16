# AGENTS.md - GPUIX Codebase Guide

**Read [README.md](./README.md) first** to understand what GPUIX is, the architecture, mutation API, supported elements/events/styles, and the test renderer.

This checkout is the `fdarian/gpuix` fork. [FORK.md](./FORK.md) says what the fork changes and why, how to sync with upstream, and how to work on the `zed/` submodule; where it disagrees with this file, FORK.md wins. This file orients you to the code as it stands. Upstream's rule that only remorses and monotykamary open pull requests belongs to `remorses/gpuix`; this fork is a proof of concept and nothing in it is a request to merge.

## README is the public API contract

Document every user-facing feature, element, prop, event, renderer option,
public method, and behavior change in `README.md` in the same change. A
changeset does not replace API documentation.

## Behave like the DOM

Events, focus, scrolling, layout, paint and motion follow DOM and CSS semantics. A new prop, event or style gets the browser's behaviour under the browser's name (`onPointerDown`, `tabIndex`, `position: sticky`), not a gpuix-only rule, and not DOM behaviour hidden behind an opt-in. A deviation is acceptable only where GPUI cannot build the DOM model, and it is written down in the README next to the feature. [FORK.md](./FORK.md#the-principle-dom-and-css-semantics) has the rationale and each area's list of deviations.

- New behaviour goes in its own module (`events/`, `layout/`, `css/`, `motion/`, `scroll.rs`, `js/events/`). `renderer.rs`, `style.rs` and `js/host.ts` get wiring only, because they conflict on every upstream sync.
- macOS is the platform that was exercised. Effects (Metal), pointer lock, synchronous dispatch and the OS reduced-motion read are macOS only. The Windows, Linux and wasm paths exist but were not compiled by the people who wrote them, so do not report them as working. The Quad shader edits (HLSL, WGSL) were never built or rendered either. Each gap's reason and what closing it takes is in FORK.md's per-area deviations.

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

The `zed/` submodule is not upstream GPUI. It carries the fork's additions (DOM hit testing, scroll routing, sticky, containing blocks, grid templates, effect groups, `draw_if_dirty`), so GPUI docs and `zed-industries/zed` issues describe something close but not identical. The source in `zed/crates/gpui` is the truth.

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
| Layout | `layout/`, `custom_elements/anchored.rs` | | `taffy/containing_block.rs`, `grid_template.rs`, `elements/anchored.rs` |
| Paint and style | `style.rs`, `css/`, `color.rs`, `text/inline.rs`, `text/paragraph.rs`, `custom_elements/svg_*.rs` | `svg.ts` | `effects.rs`, `scene.rs`, `style.rs`, `crates/gpui_apple` |
| Motion | `motion/` | `motion-*.ts`, `keyframes.ts`, `presence.ts`, `layout-animation.ts`, `reorder.ts`, `match-media.ts` | |
| Text, selection, search | `text/`, `syntax/`, `markdown/`, `diff/` | `text-selection.ts` | |
| Bounds, observers, automation | `automation.rs`, `observers.rs` | `bounds.ts`, `observers.ts`, `automation/` | `window.rs` (`draw_if_dirty`) |

Adapters: `packages/react/src` (`reconciler/`, `components/`, `hooks/`) and `packages/solid/src` (`host.ts`, `primitives.ts`, `components/`). The rest of the repo: `cli/` (`gpuix new`), `examples/`, `example-app/`, `disktree/`, `docs/`.

**`build_element` resolves a style in a fixed order**, and each stage reads the one before: the base style, then the `hover` / `active` / `groupHover` / `groupActive` / `groupFocusWithin` fold from `PointerState` and the focus state, then CSS transitions, then `motion` `animate`, then motion values, then CSS animations (which skip a property a CSS transition is running on, as a transition beats an animation), then the `display: contents` overlay and `anchor-size()`. A stage that needs to change a style copies it first: `RetainedElement.style` is a shared `Arc<StyleDesc>`.

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

`@gpuix/native/runtime` owns the desktop pump. On macOS it wakes on GPUI's
display link, with a 100 ms fallback when no wake arrives. `tick()` must stay
non-blocking so Node.js and Bun I/O can run. Explicit `frameMs` uses timer polling.

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

**Pointer capture is explicit.** Nothing captures on its own, so an element with `onMouseDown` and `onMouseMove` hears only what happens over it. A gesture that must survive leaving the element, its parent or the window calls `setPointerCapture` on the press. The call is recorded in `PointerRequests` (shared between the renderer handle and the view) and applies at the next native pointer input, so `gotpointercapture` and the first captured move come one event after the handler that asked. `cancelDrag` and `requestPointerLock` work the same way.

**An element drag is gpuix's own state machine** (`events/element_drag.rs`), not GPUI's `on_drag`. GPUI's `AnyDrag` ghost (`App::active_drag`, crate-private in zed) is what a drag image would be built on; gpuix never sets it.

**A press, a release, a wheel tick and a keystroke reach JS synchronously on macOS and in the test renderer**, through `sync_dispatch::emit` (`events/sync_dispatch.rs`). The JS handler runs inside GPUI's input dispatch and returns `{ defaultPrevented, propagationStopped }`, which is how `preventDefault` cancels a native default: `mousedown` / `pointerdown` cancel the focus change and the input editor's own focus (`window.prevent_default()`), `wheel` cancels that input's scroll, `keydown` cancels the editor action and the text insertion, and a `click` inside a `<label>` cancels its activation. The focus events of a script's own call (`focus()`, `blur()`, `focusNext()` and the other focus moves) are synchronous too, from `focus::settle_focus`, so a handler that moves focus hears `blur` and `focus` before its next line. Every other event (moves, enter/leave, `scroll`, transitions, and the focus a press, `autoFocus` or a frame notices) takes the ordinary queue and reaches JS later. On Windows and Linux GPUI runs off the JS thread (`gpui_windows` and `gpui_linux` have no embedded loop, unlike `gpui_macos`), so `emit` queues too and every default proceeds; blocking the UI thread on JS there would deadlock against the JS-to-UI commands that block on a reply (the header of `sync_dispatch.rs`; FORK.md area A lists the two ways out). The test renderer is always synchronous, so a passing test says nothing about those two platforms.

Consequence for anything a handler can call: while a sync call is in flight the dispatching `Window` and `App` are stashed (`with_active_window`). GPUI's input callback (`AnyWindowHandle::update`) does not lease the root view, and the test renderer dispatches the same way (`cx.simulate_event`), so every renderer read and write works from a handler and answers current values. A new napi method must go through `sync_dispatch::route` and the `update_*` helpers (the test renderer's `update_view` / `window_call`), never `with_test_state` or a lease of its own: the app is already borrowed there, and a nested `GpuixView` update panics inside GPUI. `update_root_view` returns `VIEW_BUSY` only when the view is genuinely being updated further up the stack (a nested update); `update_root_view_or_defer` runs a write after that update instead. Only calls that start another input or drive the test app (`simulate_*`, `advance_time`, `capture_screenshot`) cannot run from a handler, and say so.

**A handler that throws does not stop the dispatch.** Errors from `dispatchDomEvent` and from window/document listeners go to the renderer's `onError` option, else to `reportError` (uncaught). Nothing is rethrown into the caller of a simulate method, so a test that expects a throw asserts on `onError`.

Rules that sit in the dispatcher and are easy to miss:

- `inert` is inherited like `pointer-events: none`, and a descendant's `auto` does not undo it (`Inherited::descend` in `renderer.rs`). There is no hover, group state or listener in an inert subtree.
- A disabled form control gets no `click`, `auxclick` or `dblclick` (`RetainedTree::in_disabled_form_control`, checked in `dispatcher::release`). `element.click()` and the accessibility click skip it as well (`events/synthetic_click.rs`). Pointer and mouse move events still reach it.
- `mousedownoutside` is gpuix's own event, delivered to each listener whose logical subtree does not contain the press target, and it does not bubble.
- `contextmenu` fires on mousedown, not mouseup, for the secondary button and, on macOS only, for ctrl+primary.
- **`appRegion` is not hit testing.** It inherits (`Inherited::descend`) and applies only when the window is `titlebarTransparent` (`app_owns_titlebar_drag` is set from the same option, so AppKit does not drag the top strip itself). Each host element with a value registers its box with GPUI, and `Window::window_control_area_at` picks the last registered box under the pointer in paint order; z-order, `pointerEvents`, clipping and `inert` do not take part. A primary press in a drag region never reaches the page: the capture listener starts `Window::start_window_move` (macOS needs the mouse-down being dispatched) and, on Windows, prevents the default but leaves the press unstopped so `WM_NCHITTEST`'s `HTCAPTION` does the drag. With no button held the pointer over a region is outside the window as far as the dispatcher is concerned (`app_region::hides_pointer`). The test renderer records the window actions (`takeSimulatedWindowActions`) instead of performing them, and treats a window as frameless only when it was made with `titlebarTransparent: true`.

Not in the dispatcher: keys (`events/keyboard.rs`), focus events (`GpuixView::observe_focus`), the VoiceOver press (`apply_a11y_click`), `scroll`, `onLayout` and observer reports (paint), and transition and animation events (`emit_bubbling_event` in `renderer.rs`, which gives them `target` and `path` so they bubble).


## Focus, keyboard, forms and accessibility

- **Focus.** GPUI owns who has focus; `focus.rs` owns what the DOM says about it: what is focusable (`tabIndex`, never a listener), tab order, `:focus-visible` and the four focus events. All of it is observed from one place, `GpuixView::observe_focus`, because gpui runs per-element focus listeners in creation order, so per-element subscriptions fire `focus(new)` before `blur(old)`. Do not add one.
- **The element source** (`installElementSource` in `js/events/window-target.ts`) is the id-to-element lookup of a root as the adapter's ref gives it: React `Container.instances`, Solid `HostRoot.elements`. Each adapter installs it beside `syncWindowEventListeners`. `document.activeElement` reads it, `null` with nothing focused; with several roots the one that heard a focus event last and still has a focused element wins. Event dispatch reads it too (see [Events](#events-one-hit-test-one-dispatcher-dom-propagation)). It lives on `globalThis` because a renderer's state outlives a `bun --hot` re-evaluation of the module. The test window is one per thread, so a second `TestRenderer` stops its predecessor reporting focus (`forgetFocusSource`); that root keeps naming its elements.
- **A press moves focus.** `focus::defer_mouse_down_default` runs after the `mousedown` handlers and uses the dispatcher's press path, not the topmost hit. A `preventDefault` from any handler on the path cancels it. A press with nothing focusable on its path blurs.
- **A focusable `<span>` has no GPUI element**, so its paragraph anchors the handle (`text/inline_focus.rs`): a zero-size `div` under the span's `host_element_id` that tracks it, placed after the glyphs in `Paragraph.boxes` order, which gives the dispatch node, the tab stop in tree order and the a11y node. Bounds still come from `text/inline_bounds.rs`; a second recorder under the id would overwrite them. GPUI cannot find the pointer in a zero-size box, so `focus_on_press` focuses the span after the press has been dispatched. A focusable ancestor's own GPUI handler would otherwise win and prevent the default. A focusable nested `<text>` is a block, an element of its own.
- **Keys.** `events/keyboard.rs` sends one `keydown` per keystroke, from a GPUI keystroke interceptor that runs before key bindings, to the focused element. JS bubbles it and then reaches the `window` and `document` listeners (`js/events/window-target.ts`). Defaults (Tab, Escape for the top layer, the scrolling keys) run in JS after every handler, in `runKeyDefault`, and a `preventDefault` skips them. `key` and `code` are DOM names (`events/key_names.rs`). On macOS `code` is the physical key from `NativeKey::key_code` through Chromium's table (`events/key_codes.rs`) and `key` is `NativeKey::layout_char`, the layout's character with shift and option applied, as in Chrome; without a native key (other platforms, a simulated key name) both derive from GPUI's `key` assuming a US layout. A simulated press that names a code (`KeyA`) carries a native key; one that names a key (`a`, `Enter`) does not. On macOS a key the input method takes first (composing, or an IME source taking printable keys) reaches JS as a `keydown` with `key` `"Process"`, sent before the input method gets it from an observe-only zed event (`PlatformInput::ImeKeyDown`, `App::observe_ime_keystrokes`) in `install_key_down`; its outcome is ignored, so `preventDefault` cannot stop the input method. `isComposing` is the focused input's marked text. The input editor fires `compositionstart` / `compositionupdate` / `compositionend` itself from its `EntityInputHandler` calls (`custom_elements/input.rs`), through `emit_bubbling_event` before the `change` of the same edit; they are queued like `change`, so only the test renderer's `simulateComposition` orders them with the sync keydown and keyup.
- **Forms.** `forms.rs` owns validation and submission (`invalid`, then a bubbling cancelable `submit`), both sent through `sync_dispatch`. A `<label>` (`accessibility/label.rs`) activates its control after a `click` whose path contains it, unless the click was prevented or landed inside the control.
- **Accessibility.** `aria-*` props map to AccessKit in `accessibility/aria.rs`. A keyboard or VoiceOver activation is a DOM `click` with `detail` 0, `pointerId` -1 and no pointer (`events/synthetic_click.rs`).


## Scrolling: native routing, and nested scrollers work

Overflow follows CSS (`scroll.rs`): `scroll` and `auto` scroll, `hidden` scrolls only from code, `clip` does not scroll, and overflow never changes `display` or wraps children. `scroll::apply_container` turns a div into a scroll container and attaches the scrollbar (`scrollbar.rs`), which is an overlay or, when macOS "Show scroll bars" is Always (`ScrollerStyle::Classic`), a bar in a gutter the container reserves through the `scrollbar_width` and `without_scrollbar` styles in zed. `ScrollHandle::padding_box` is the scrollport less that gutter. `TestRenderer.setScrollbarStyle` forces the style under test.

The wheel is two separate things.

- **Which scroller moves** is zed's `scroll_routing.rs`. One wheel event scrolls one scroller: every scroller under the pointer is offered the event in bubble order, innermost first (`div` scrollers and `gpui::list()` alike, so `<virtual-list>` and a scrolling `<diff>` take part), and the first that can use any of it, on either axis, takes all of it. What it cannot use is dropped, not handed on in the same event; chaining to the outer scroller happens between events (`TouchPhase::Started`, 500 ms idle, or a phase-less wheel after the pointer moved), so a test that expects an outer scroller to move needs a second gesture (`"started"` phase). A scroller's range is CSS scrollable overflow over all descendants, not only direct children: an absolute descendant counts only if the scroller is its containing block, and fixed, anchored and deferred ones never do. A gesture latches to the scroller that took its first delta, trackpad momentum included (a mouse wheel latches until 500 ms of silence or until the pointer moves), and `overscroll-behavior` decides what happens to delta a scroller cannot use. This is not `stop_propagation`, so `onWheel` listeners above still run. A scroller whose hitbox is `PassThrough` (`pointer-events: none`, inert) is never a candidate, even while something non-pass-through under the pointer is.
- **The `wheel` DOM event** goes through the dispatcher like any pointer event, to the hit target and along its path, and it runs **before** any scroller claims, through sync dispatch. A `preventDefault` in either phase calls `window.prevent_default()` and the routing skips that event. It cancels that input only, not the rest of the gesture, and only where dispatch is synchronous. `onWheel` fires for every tick; `onScroll` is a different event, fired on the scroller that moved for any offset change (wheel, scrollbar drag, keys, `scrollTo`), non-bubbling, and not fired by `<virtual-list>` (`onVisibleRange`).

`<code>` and `<markdown>` code and table bodies are `overflow_x_scroll().restrict_scroll_to_axis()` divs. They do not set `allow_concurrent_scroll`, so `div.rs` takes its older branch for them: horizontal deltas only, no chaining, no latching. Keep `restrict_scroll_to_axis()` on any x-only scroller that native code builds, or a vertical wheel over it moves it sideways instead of the page.

Keys that scroll are a JS default action (`runKeyDefault`, then `renderer.scrollByKey`, `scroll::scroll_by_key`): the innermost scrollable ancestor of the focused element that can move that way, else the scroller under the pointer. A focused input keeps its keys.

`position: sticky` is placed natively in prepaint (zed `sticky.rs`) against the nearest scroll container's scrollport, so it follows the scroll with no JS in the loop and cannot tear.

A `<diff>` flows with its parent by default, so a parent list can be the only scroller. Pass `scroll` plus a bounded height only for a dedicated viewer. Do not give `<diff>` a bounded height inside a parent scroller just so it can virtualize.


## Bounds: every element reports its own box at paint, and reads are never stale

`getByTestId(..).click()`, `getBoundingClientRect`, `onLayout` and the observers all read the border box recorded by `crate::automation::track_host_bounds`. It is the element's own `on_painted_layout`, so it adds nothing to the tree: a canvas child per element would double the nodes gpui lays out, prepaints and paints. `<img>`, `<svg>` and `<anchored>` use `track_own_bounds`, the same hook without the selection-start region. A leaf cannot hold a child, and wrapping it in a div would move the layout box (the wrapper becomes the flex item and the image loses intrinsic sizing and corner clipping). `custom_surface` attaches the right one for a custom element. `on_painted_layout` takes one listener, so paint-driven events (`onLayout`, observers, `onScroll`) share it through a `PaintReporter`, which also gets the padding and border the node was laid out with (`Window::layout_insets`), state styles and percentages resolved. `clientLeft` and the observers read those, never the element's base style.

Both record during **paint**, and `bounds_frame_reset` clears the registry during paint too. Never move any of it to prepaint: `gpui::list()` prepaints a speculative row range, then rolls the window back through `Window::transact` and prepaints a different one, so a prepaint-recorded box can belong to a row that never reached the screen. A `visibility: hidden` element paints nothing but still runs these listeners, so it keeps its box (`Window::visibility` tells a listener which it is, and `track_host_bounds` records no selection region for a hidden one); a box that is missing means `display: none`, a virtual list row outside its window, or a removed element.

**A bounds read is never stale.** `readBoundingClientRect` (`js/bounds.ts`) first sends the mutations the adapter has queued (`flushPendingMutations`: Solid flushes in a microtask, React at commit), then native `GetElementBounds` calls `Window::draw_if_dirty` in zed before it answers. Scroll metrics and `scrollIntoView` do the same. The cost is that a read in the same turn as a mutation is a synchronous draw: batch the reads, do not interleave a read after every write. In a real window `draw_if_dirty` is the only thing that draws early; `flush_effects` draws inline only for test-mode apps ([FORK.md](./FORK.md), patch 1). A read made from inside an input handler (a sync keydown, wheel or mousedown handler) cannot draw: the dispatch is reading the rendered frame. There `draw_if_dirty` is a measure pass (`Window::measure` in zed) that lays out and paints into the next frame without swapping it, so bounds and scroll metrics are current and the window stays dirty for the real draw. Calls made while no input is dispatching are real draws.


## Layout: CSS rules between the retained tree and GPUI

`layout/` holds the DOM and CSS rules that taffy and GPUI do not give for free; `renderer.rs` only calls into it. `layout::style::apply` is the layout half of `apply_styles`.

- **`position` defaults to `static`** (`build_element` sets it when the style has none). `absolute` resolves against the nearest positioned ancestor: zed's `taffy/containing_block.rs` rewrites those nodes after a layout pass and lays out again. `fixed` resolves against the window. So `top` / `left` without `position` do nothing, and code that needs an absolute child needs a positioned parent (`AnimatePresence mode="popLayout"` is one).
- **Clip and scroll follow the containing block.** An `absolute` box is clipped and scrolled only by the ancestors at or inside its containing block, and a `fixed` box by the window alone. zed does it: a positioned `Div` (also `<anchored>`, and a layout root such as a list row) records the content mask and element offset its children see, `Element::escapes_to` makes `Drawable` prepaint and paint the box under that frame instead of the inherited one, and deferred draws carry the frame. Native code that walks ancestors to decide what clips or moves an element (`observers.rs`, `scrollIntoView`, key scrolling, `RetainedTree::elements_at`) uses `layout::containing_block`, not `parent`. A new gpui element that is a styled box needs `escapes_to` (`StyleRefinement::escape`), and a wrapper that places something by its child's bounds forwards it. Residuals: an opacity or filter group clips to the mask at its start, so an escaped box inside one on a clipped element is still clipped by it, and a `layout` (FLIP) glide is dropped for an absolute or fixed box.
- **Paint order is CSS stacking contexts, decided in one place** (`layout/stacking.rs`). zed lifts every positioned `Div`, `<img>`, `<svg>` and `<virtual-list>` (`Element::lift_z`) out of its parent into the nearest `Div` flagged `stacking_context()`, which paints and hit-tests them after its in-flow children by `(zIndex, tree order)` (negative ones above its own background, below the in-flow content); tab order, dispatch and a11y stay in tree order. `build_element` flags exactly what `creates_context` says: the window root, a positioned element with a `zIndex`, `fixed`, `sticky`, and every effect group (`opacity` < 1, `filter`, `backdropFilter`, `maskImage`, `clipPath`: zed cannot see these, so a missed one lets a descendant escape the effect). `<anchored>` content, each virtual-list row and the anonymous block of a root or row `display: contents` are contexts too (`stacking_root` in `BuildCtx`): a deferred layer and a layout root begin with no context around them, and nothing is lifted out of a row. Without a context (below `visibility: hidden`) `paint_rank` orders the children of one parent. `RetainedTree::elements_at` models the same contexts on the retained tree with the same predicates (`lifted_z`, `is_forced_context`, `creates_context`), so a new rule goes in `stacking.rs` and nowhere else. An element gpuix positions only for layout (the text wrapper, the containing block of an absolute canvas, anchored's placement boxes) must be `.unlifted()`, or it rises above its later siblings; a gpui wrapper element forwards `lift_z` with `escapes_to`. Not modelled: `transform` (none exists), a `zIndex` on a static flex or grid item, and `hover` / motion values that move an element into a context (`elements_at` reads the declared style).
- **No box.** `display: none` builds nothing (no box, no hitbox, no a11y node); `display: contents` splices the children into the parent (`layout/contents.rs`, taffy has no such value); a root or virtual-list row has no parent, so `build_contents_container` puts its children in an anonymous block. An empty or whitespace-only text node generates no box and takes no `gap` slot (`text/inline.rs`).
- **Lengths are a linear expression, resolved at apply time.** `css/length.rs` has `LinearLength` (px, %, em, rem, vw, vh, vmin, vmax) and `LengthContext` (font size, root font size, window size), which rides in `Inherited` next to `ColorContext` and is passed to `apply_styles*`; em and rem need it, so never resolve a length at deserialize. `css/dimension.rs` is the parser (`parse_linear_length`, `calc()`), reused by every length field. `layout/style.rs` turns them into GPUI lengths (`definite()`, `linear()`); a length plus a `%` becomes a calc node whose storage is zed's `taffy/calc.rs` `CalcArena` (owned by `taffy.rs`), evaluated by the patched taffy in `zed/third_party/taffy` (which also resolves a block container's percentage padding against the parent's width). `width` / `minWidth` / `maxWidth` / `flexBasis` accept `min-content`, `max-content` and `fit-content`, measured by a pre-pass in zed's `taffy/intrinsic.rs` (`flexBasis` only along a row container's main axis, `auto` elsewhere) (and `taffy/natural.rs` for the natural width of `width: auto` motion), each costing extra layouts per flagged node. Sizes also take `anchor-size()`. Grid tracks take lengths and the keywords (`layout/grid.rs`).
- Grid syntax is parsed once when a style is deserialized (`layout/grid.rs`), so the per-frame `apply_styles` only moves built values. `order` and `aspectRatio` live in `layout/` too.
- `<anchored>` is a floating-ui style popup (zed `elements/anchored.rs`: flip, shift, `onPlacement`). `anchor-size()` (sizes, margins and insets of a direct child; `layout/anchor_size.rs`) starts from the trigger's last painted size, so it is right in the first frame under a trigger that has painted; with none the child is held at opacity 0 until the trigger is measured.


## Paint and style

- **Colours resolve through `ColorContext`** (`color.rs`): the element's computed `color` (what `currentColor` means), its `color-scheme`, and the window appearance. `light-dark()`, `currentColor` and `color-mix()` cannot be resolved without it, so `apply_styles` takes `&ColorContext`, a state refinement uses `for_state`, and children get `descend`. `parse_color_rgba` has no context and none of those functions: do not call it on a style value. Motion colours go through it too: `motion::Colors` (`motion/value.rs`) holds an element and a text context, `parent_color_context` (`renderer.rs`) builds the parent's from declared styles (an ancestor's `hover` colour is not in it), and a `MotionState` re-parses its colours when they change.
- **CSS value strings** (`background`, `boxShadow`, `filter`, `mask`, `clipPath`, colour functions) are parsed in `css/`, one file per concern. `gradient::parse` (every `background` and `mask` image), `filter::parse` and `clip_path::parse` go through a bounded thread-local cache keyed by the text (`css/cache.rs`), so an element built again with the same string does not re-parse it. The key is the text plus the part of the context the text reads (`cache::Inputs`): a gradient with `currentColor`, `light-dark()` or `color-mix()` keeps one result per `ColorContext::resolves_like` class, and a text with a number followed by `em`, `rem` or a viewport unit one per `LengthContext::resolves_like` class (`LengthReads::of` finds the units); a text with neither is one result for every element. A parse that starts reading another part of the context must add it to `Inputs` and to those two `resolves_like`. `boxShadow` is not cached because measurement showed no gain. Everything else is parsed on every build, so keep new parsing out of anything that runs per frame per element; `css/parse_cost.rs` and `examples/css-values.perf.test.tsx` measure it.
- **Effect groups** (`filter`, `backdropFilter`, `mask`, `clipPath`, group `opacity`) are offscreen passes in zed `effects.rs` and `gpui_apple`. Only macOS (Metal) renders them; elsewhere the group draws flat and only opacity applies. A subtree that paints one primitive keeps the cheap per-primitive path. Changing `Quad` means changing the shader and the Rust struct together on every backend, or the stride checks fail. Only the Metal side of those shaders has run: the HLSL is built only by fxc on a Windows host and the WGSL was validated with naga but never rendered.
- **`overflow` clips to the padding box** with rounded inner radii (zed rounded content masks), not to a rectangle.
- **Inline text.** `text::inline::plan_children` is the CSS inline formatting rule: in a block container each maximal run of inline children (raw text, `<span>`, a nested `<text>` that is not focusable) is one paragraph, and a block child ends the run; in a flex or grid container children are blockified. A focusable `<text>` stays a block, because an inline box has no focus handle; listeners and state styles do not make a box a block (the pointer finds an inline box through `text/inline_hit.rs`, and `run_styles` folds a hovered or pressed box's `hover` / `active` / group styles into its run). A span's `cursor` is the exception to "no hitbox": GPUI resolves the cursor from hitboxes on every move, so `text/inline_cursor.rs` inserts one per line fragment, after the glyphs, only for text that asks for one; they carry the container's element id, so `host_hit_chain` reads them as the container. `text/paragraph.rs` turns the inline boxes into per-run styles. A box's `focusVisible` / `focusWithin` are folded the same way (`PointerStates` carries the two focus bits for inline boxes only, set by `FocusStyles::states_of`, since a div applies them in `apply_focus_styles`), and `text/inline_decoration.rs` paints its `backgroundColor`, border and `borderRadius` behind the glyphs, and its `outline` over them, for each line fragment (a box's background is never a `TextRun` background, which would fill its margin box). Every consumer of a box's rectangle (bounds, hit test, cursor hitboxes, the painted quads) takes it from `inline_bounds::border_box_rows`: the font's content area in the line plus the vertical padding and border, not the line box. A box's margin, border and padding are `text/inline_insets.rs`: an `InsetSpec` keeps the lengths as written (`em`, `rem`, `calc()` with the box's own `LengthContext` from `paragraph::box_lengths`, so a span's `em` is its own font size; a `%` is of the width the paragraph is laid out in), `StyledText::with_insets_for_width` asks it for that width at measure, and each consumer reads its border box edges (`BoxEdges::at(layout)`) for the width the layout kept (`TextLayout::inset_width`), never at build time. Text's min-content width is `WindowTextSystem::min_content_width`: the text laid out at wrap width 0 with `TextLayoutOptions::for_min_content`, so it follows the same break rules as the text; do not mirror them elsewhere. A span's `fontSize` and `lineHeight` size its run (`RunStyle.font_size` / `line_height`, `TextRun.font_size` / `line_height` in gpui): the row is as tall as the CSS line box, the container's strut plus each run's own contribution, and `rem` stays the root's.
- **`<svg>`** rasterizes with resvg at device scale in its own colours (`custom_elements/svg_paint.rs`); `currentColor` is the element's `color`, not black. DOM-style shape children (`<path>`…) never become GPUI elements: `svg_tree.rs` serializes them into one image. `SvgPaint::image` holds one image and drops the previous one from the atlas when an input changes, so keep that single entry point.
- **`clipPath` clips hit testing** for `div` hitboxes (zed `Hitbox.clip`). A custom element that inserts its own hitbox outside any `div` is not clipped.


## Motion

`packages/native/src/motion/` resolves animation during GPUI rendering, outside React. A description (`initial`, `animate`, `transition`) becomes one `Track` per animated property, and a retarget restarts only the properties whose target changed (`mod.rs`). Around it: CSS transitions and animations (`css_transition.rs`, `css_animation.rs`), the keyframes registry, motion values (`bindings.rs`), `layout_flip.rs` and `measure.rs`.

- GPUI has no transform for quads, shadows, paths, underlines, images, clips or hitboxes (only SVG and glyph sprites carry a matrix). `layout`, `layoutId` and a dragged `Reorder` item move by prepainting inside `Window::with_element_offset`, which shifts bounds, hitboxes and paint, so position animates and size snaps. Scale and rotate (a size morph, keyframe rotation) need a transform primitive first; FORK.md area E has the route.
- `measure.rs` is what the previous frame's layout produced: `width: "auto"`, `height: "auto"` and `%` targets read it, so they settle one frame late. The natural width comes from an extra layout of the tree that GPUI runs only for nodes flagged `measure_natural_width`; `motion::wants_natural_width` flags an element only when its motion, or its transitions and animations under `interpolateSize: "allow-keywords"`, can run on `width`, so other trees pay nothing.
- Hover and active for an element with a transition come from `PointerState`, folded once in `build_element`, one frame after the pointer moves. Do not add a probe element above a hitbox: it hides the element from the DOM hit test.
- Transition and animation events are lowercase DOM types (`transitionend`, …) with `target` and `path`; they bubble and are not cancelable. `Reorder` (React and Solid) captures the pointer with `setPointerCapture` on `pointerdown`, so a drag works only because capture is explicit.
- A motion value is not tied to an element, so its colours resolve as on a root with `colorScheme: "light dark"` and keep what they resolved to.
- `matchMedia`: JS owns the query language (`js/media-query.ts`); `media.rs` only answers what the OS says, and only macOS has a read.

**Adding a `StyleDesc` field.** `overlay_fields!` in `events/state_styles.rs` must list it: it is a struct literal, so a missing field fails to compile instead of silently never taking part in state styles. If the field is a shorthand, `drop_shadowed_longhands` in the same file must reset the longhands it covers, or a hover shorthand loses to a base longhand. If it animates, it needs an entry in `motion/property.rs`.

## Text rendering: one funnel, no exceptions

Every string GPUIX paints goes through `crate::text`:

- `selectable_text(..)` for content — registers into the per-frame selection
  registry and installs the window mouse and key listeners
- `chrome_text(..)` for line numbers, language tags and file headers — painted
  and logged for tests, but never part of a selection

**Never call `div().child(some_string)` in a new element.** Doing so makes the
text invisible to selection AND to `getPaintedText()`, so it cannot be tested
except by screenshot.

The registry is rebuilt during **paint**, not during build, because paint order
is the only place document order is guaranteed: a `list()` decides at paint time
which rows exist. `selection_frame_reset()` must stay the first child of the
root, or stale entries from the previous frame leak into the next drag.

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

`hover`, `active`, `groupHover`, `groupActive` and `groupFocusWithin` are folded into the style in
`build_element` before any builder runs, so a builder only applies the style it
is given (`apply_interactive_styles`). `focusVisible` and `focusWithin` need the
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
submodule ([FORK.md](./FORK.md#syncing-with-upstream) has the procedure and the
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
from that remote branch, and never reset or detach `zed/` to "undo" work. FORK.md
([Working on the fork](./FORK.md#working-on-the-fork)) has the branch layout and
says which gpuix commit may move the `zed` pointer. Test zed changes where they
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
- [ ] **Transforms** - GPUI has no transform for quads, shadows, paths, underlines, images, clips or hitboxes, so size morphs in layout animations and rotation or scale in keyframes wait on it

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

The test renderer draws offscreen and takes the synchronous dispatch path for every call (see [Events](#events-one-hit-test-one-dispatcher-dom-propagation)), so a passing event test proves the macOS behaviour, not Windows or Linux. Every simulate helper (keys, wheel, pointer, file drag, focus moves) calls `flushIfDirty()` before and after the input, not `flush()`: it draws a pending change and nothing on a clean window, as a live window draws once per dirty input and never unconditionally, so a move costs one draw. Hit testing needs a laid-out frame, which that gives it; the public `flush()` always draws. `nativeSimulateClick` sends the press and the release as two inputs and drains JS between them, because the press's focus change is queued while the release is delivered synchronously. A handler that throws does not fail the simulate call: pass `onError` and assert on it.

The test window opens from the live renderer's `WindowOptions` through the same `to_gpui_window_options` (`TestGpuixRenderer::new`); the README's Testing section lists what each option does there. Simulated moves report the buttons held by simulated downs (`simulateLostMouseUp` forgets one), and `scrollTo` / `getScrollOffset` speak DOM scroll positions (`scroll::gpui_offset` and `scroll::dom_position` are the only sign flip). The appearance override is process-wide (`NSApplication.appearance`), and each new test window sets or clears it.

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
