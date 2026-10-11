# fdarian/gpuix, a proof-of-concept fork

This is a long-lived proof of concept of what [remorses/gpuix](https://github.com/remorses/gpuix)
could be if scrolling and input latency on macOS were a goal, and if its elements behaved like the web
DOM. It is not published to npm and nothing here is a request to merge. It is rebased on upstream
regularly (see [Working on the fork](#working-on-the-fork)), so commit hashes change.

The fork carries three kinds of change:

1. **Scrolling and latency patches (1 to 3).** It started as a demo that renders a large
   syntax-highlighted diff with `@gpuix/solid` and had visibly worse scrolling than Zed itself. Three
   causes turned up. Upstream has since landed the fix for one of them (hidden bounds canvases,
   [remorses/gpuix `d8cf51a`](https://github.com/remorses/gpuix/commit/d8cf51a), `track_host_bounds`),
   so the fork's own on-demand bounds patch was dropped in favor of it. The other two are patches 1
   and 2. Patch 3 is not about scrolling: it adds the spring transitions the fork's UI work needed.
2. **DOM parity, in five areas (A to E).** Porting a component library built for the web showed that
   the gaps were not about speed. Hover, focus, scrolling, clipping and animation behaved differently
   from the DOM in ways each component had to work around. Each area closes one family of those gaps.
   See [The principle](#the-principle-dom-and-css-semantics).
3. **Three small features** that are not patches against a problem: following the macOS system
   appearance, a `fonts` window option, and `appRegion` window drag regions for frameless windows. See
   [Appearance and fonts](#appearance-and-fonts) and [Window drag regions](#window-drag-regions).

| Change | Repo | What it does |
|---|---|---|
| Patch 1: `test-support` no longer draws inline in real windows | zed (`poc`) | Wheel events and `applyBatch`/`scrollTo` stop paying for a full synchronous draw. |
| Patch 2: display-link frame pump | upstream, plus gpuix and zed (`poc`) | GPUI is serviced by the display, not by an 8 ms JS timer. Upstream provides the pump; the fork adds the Solid `frameLoop` option, the frame-loop tests and the present log. |
| Patch 3: spring transitions | gpuix | `motion` gets springs and carries velocity across a retarget. |
| A: pointer events | gpuix + zed | One DOM hit test, capture/target/bubble dispatch, pointer capture, click family, drag and drop, pointer lock. |
| B: focus, keyboard, forms, a11y, platform APIs | gpuix + zed | DOM focus order and `tabIndex`, DOM keyboard events, form controls, `aria-*`, `inert`, clipboard, file picker, `EyeDropper`. |
| C: layout, measurement, scrolling | gpuix + zed | Never-stale bounds, observers, CSS positioning, grid, tables, atomic inlines (`inline-block` and kin), container queries, nested scrolling with scrollbars, floating-ui placement. |
| D: paint and style | gpuix + zed | Rounded clipping, gradients, filters, masks, `clipPath`, group opacity, inline text, CSS colours, full-colour `<svg>`, group and relational (`:has()`) state styles, selection in document order. |
| E: motion | gpuix + zed | Colour and per-property transitions, keyframes, CSS `animation`, motion values, layout animations, `matchMedia`. |
| Appearance | gpuix | `appearance` window option, `getAppearance()`/`setAppearance()`, `appearanceChange`. |
| Fonts | gpuix | `fonts` window option that loads app-supplied font files. |
| Window drag regions | gpuix + zed | `appRegion: "drag" \| "no-drag"` moves a `titlebarTransparent` window, as Electron's `-webkit-app-region`. |

Areas A to E and the drag regions break existing gpuix apps on purpose. Each section lists what an app author has to
change. The README documents the resulting behavior; this file records why.

## Patch 1: test-support does not draw inline in real windows

**Problem.** The macOS addon is built with GPUI's `test-support` feature. It is the crate's default
feature (`packages/native/Cargo.toml:99`), and CI builds the release artifact with `bun run
build`, not `build:release` (`.github/workflows/ci.yml:33`). With that feature, `App::flush_effects`
in `zed/crates/gpui/src/app.rs` draws every dirty window synchronously at the end of every
`App::update`. Test contexts need that because they never run a display link. A real window does
not, but the feature is a Cargo feature, so the check was compiled in for it too.

**Evidence.** Each wheel event cost a full draw, about 3 ms, inline in the input handler, and every
`applyBatch` and `scrollTo` drew synchronously as well. Timed in a real (non-offscreen) window with
the demo's 2.7k-node diff, 300 calls each:

| Call | Unguarded `test-support` | Guarded (this fork) | Addon built without `test-support` |
|---|---|---|---|
| `simulateScrollWheel` | 2.34 ms p50 (2.52 p95) | 0.001 ms p50 | 0.03 ms |
| `scrollTo` | 2.33 ms p50 (2.47 p95) | 0.000 ms p50 | 0.01 ms |

The last column was measured earlier with a different method (the wheel path through the real
event handler, 2.7 ms down to 0.03 ms), so compare it only loosely. It is the floor: the guarded
build is as cheap as a build with no `test-support` at all.

**Design.** `flush_effects` now draws inline only when `App::mode` is `GpuiMode::Test`. `App::new_app`
starts in `GpuiMode::Production`, and only the test contexts (`TestAppContext`,
`VisualTestAppContext`, the headless one) switch it. `TestGpuixRenderer` is built on
`VisualTestAppContext`, so it keeps drawing inline and `flush()` still returns a drawn frame. The
real `GpuixRenderer` runs on `gpui_platform::application()`, stays in `Production`, and is drawn by
the display link. The change is one commit in `fdarian/zed` (`gpui: draw inline in flush_effects
only for test-mode apps`) and gpuix needs no change.

Area C later added a deliberate exception: a bounds or scroll read in the same turn as a mutation
flushes and draws synchronously through `Window::draw_if_dirty`, so the read is never stale. That is
an explicit call, not the `flush_effects` path this patch guards.

Inside input dispatch that call would swap the frame the dispatch is reading: a key's dispatch path
indexes a different dispatch tree, and mouse listeners the dispatch holds out of the frame are written
back over the new one, so scrollers stop scrolling and bubble handlers see `is_hovered` false.
`Window::dispatch_event` therefore raises a depth counter, and `draw_if_dirty` at depth above zero runs
`Window::measure`. The measure builds the next frame with the same `draw_roots` as a draw, so painted
bounds, scroll handle metrics and element states are current, then throws the frame away: element
states are handed back to `rendered_frame` and the hit test, pointer capture and tooltip bounds are
restored. It skips focus listeners, the input handler, entity observation, text-layout frame
rotation, the accessibility tree and `needs_present`, renders every view in full instead of reusing
its cached prepaint and paint, and leaves the window dirty. It runs again only when something was
invalidated since the previous measure. The accessibility and IME paths are therefore not covered:
inside a handler, bounds and scroll reads are current, but the accessibility tree and the input
handler (IME caret bounds) still describe the previous frame until the next draw. Making them current
would mean swapping frames, which is what the pass exists to avoid.

**Alternatives rejected.**

- **Dropping `test-support` from the release build.** It fixes the draw cost, but the feature also
  gates `TestGpuixRenderer`, `createTestRoot` and `captureScreenshot` on macOS
  (`packages/native/src/renderer.rs`, `capture_screenshot`), so a plain release addon loses them.
  The next entry is the attempt to keep both.
- **Two addons: a release addon without `test-support`, and a separate test addon with it, loaded
  only by `@gpuix/native/testing`.** Built and tried; it cannot work in one process. Both addons
  embed GPUI, and a load-time constructor registers the Objective-C window class
  (`gpui_macos/src/window.rs:366`, `ClassDecl::new(name, superclass).unwrap()` in
  `build_window_class`, reached from `build_classes::__ctor_private`). The second `dlopen` finds
  the class already registered, `unwrap()` panics inside an `extern "C"` constructor, and the
  process aborts ("panic in a function that cannot unwind"). Minimal repro: `require` the two
  `.node` files in one `bun -e`. In the react suite, six test files abort their vitest worker:
  `styles`, `render`, `events`, `floating-controls`, `animate-presence` and
  `debug-frame-overlay`. Each imports the adapter's public entry (which loads the release addon
  through `@gpuix/native`) and `@gpuix/native/testing` (which loads the test addon). The other 21
  files, 300 tests, passed. Making the two coexist needs either lazily loaded adapter addons or a
  zed change that tolerates an already-registered class, which would share Objective-C method
  implementations between two dylibs that each have their own statics. Neither was worth it, and
  the two-addon build also loses `captureScreenshot` in the release addon.

**Trade-offs.** The shipped addon still contains the `test-support` code, so it is larger than a
build without it. `TestGpuixRenderer` is also still part of the production binary. The guard
trusts `GpuiMode`: an embedder that builds its own `App` and sets the mode to `Test` gets the old
behavior, which is the point.

**Upstreaming.** Two routes. Land the guard in `remorses/zed` (one commit, no gpuix change), or
build the release addon without `test-support` and make the adapters load the addon lazily so a
test process that also uses `@gpuix/native/testing` never loads two. The first is smaller. The
second also shrinks the binary, but it needs the lazy load and a decision about
`captureScreenshot`, and CI would build and publish a second artifact for the test addon.

## Patch 2: display-link frame pump

**Problem.** On macOS, GPUI runs on the JavaScript thread and was only serviced by an 8 ms
`setTimeout` around `renderer.tick()`. AppKit input and GPUI's display-link dispatch source
therefore waited for the next JS timer: frames were not paced by the display, and input could wait
most of a timer period.

**Why upstream used a timer.** It was deliberate. Bun owns the event loop, and an earlier blocking
pump starved Bun's I/O ([remorses/gpuix#39](https://github.com/remorses/gpuix/issues/39)).
Latency was not what the timer was chosen for.

**Design.** `tick()` stays non-blocking. Upstream's zed `gpuix` branch has a hook,
`gpui_macos::set_frame_wake_hook`, that runs on the display link's io thread right after a vsync's
frame step has been dispatched to the main queue. `renderer.setFrameWakeCallback(fn)` registers a
non-blocking, weak N-API threadsafe function behind that hook and returns whether the platform has
one (`true` on macOS); wakes that arrive while one is still pending are dropped, and dropping the
renderer clears the hook. `startFrameLoop` (`packages/native/js/runtime.ts`) calls `tick()` from
that callback, and a 100 ms fallback timer (`FRAME_WAKE_FALLBACK_MS`) covers the case where the
display link is stopped (an occluded window). `frameMs` opts into the timer loop, and Windows and
Linux use it too.

The fork adds to upstream's pump:

- `FrameLoopOptions` is exported from `@gpuix/native/runtime`.
- Solid's `render()` takes `frameLoop: { frameMs }` and passes it to `startFrameLoop`.
- `packages/native/js/__tests__/frame-loop.test.ts` runs the loop against a fake renderer: the tick
  at start and on every wake, the fallback timer, teardown, termination, tick errors, and timer
  polling.
- A zed commit adds a `GPUIX_PRESENT_LOG` debug log of when each `MTLDrawable` is presented, which
  is how the table below was measured.

**Alternatives rejected.**

- **Keep the 8 ms timer.** It is the baseline column below: 2 skipped refreshes, a 16.7 ms worst
  gap between frames on screen, 77% of scroll steps drawn.
- **Block in the run loop** (`CFRunLoopRunInMode` for up to `waitMs` inside `tick`, the first
  version of this patch). Input delay dropped to about 0.2 ms, but Bun's I/O ran up to `waitMs`
  late on every tick: 100 sequential awaited file reads took 810 ms, against 3.8 ms with the timer.
  That is the regression #39 fixed, so the blocking wait was dropped.

**Results.** 120 Hz display, every column built so that scrolling does not draw inline
(patch 1 or no `test-support`):

| | 8 ms timer | Blocking wait | Display link |
|---|---|---|---|
| Skipped refreshes while scrolling | 2 | 0 | 0 |
| Worst gap between frames on screen | 16.7 ms | 8.33 ms | 8.33 ms |
| Scroll steps that got drawn | 77% | 99% | 100% |
| Input delay, p50 | 4.1 ms | 0.24 ms | 3.9 ms |
| 100 file reads awaited one at a time | 3.8 ms | 810 ms | 8.7 ms |
| Idle CPU (20 s runs) | 4.42% | not measured | 4.46% |

The display link is the only version that is good on every row, and it costs no extra idle CPU. Its
lead over the timer is small; patch 1 is the large practical win.

**Trade-offs.** Input delay is still up to one frame (3.9 ms p50 at 120 Hz). Only macOS has a
display link in GPUI's path.

## Patch 3: spring transitions for native motion

**Problem.** `motion` had `duration` plus a named ease or `cubicBezier`, nothing else, and a
retarget (`MotionState::sync`) restarted from the visible value at zero velocity. That cannot express
a Framer-style spring, and the second gap matters more: velocity carried through an interruption is
what makes a spring feel fluid, for example a hover highlight gliding between rows as the pointer
moves. The motivation is porting the components on fluidfunctionalism.com, whose springs are
`fast` (0.08 s, critically damped), `moderate` (0.16 s) and `slow` (0.24 s, bounce 0.12).

**Design.** `transition: { type: "spring", ... }` in two forms, both Framer-shaped: `stiffness` /
`damping` / `mass` (defaults 100 / 10 / 1), or `duration` + `bounce`. The second is converted with a
port of motion-dom's `findSpring` and `bounceToDampingRatio` (`animation/generators/spring.mjs`, the
engine behind Framer Motion), checked in a Rust test against values taken from that module's
`spring()`. `restDelta` / `restSpeed` use Framer's defaults, tighter for moves under 5 units. Tween
transitions (no `type`, or `"tween"`) are untouched.

`MotionState` now keeps a position and a velocity for the segment start. A spring is the closed-form
damped oscillator (under-, critically and over-damped) evaluated from that start, so it is exact and
does not depend on frame rate; there is no numeric integration. On `sync` the new segment starts at
the sampled position and sampled velocity. A tween interrupted by a spring hands over its
instantaneous slope (a central difference of the easing curve); a spring interrupted by a tween
drops the velocity. The segment settles when every property is within its rest thresholds, then
snaps to the target, and `frame().active` / `just_settled` / `motionComplete` work as before.
Springs overshoot, so the applied value is clamped (`opacity` to 0..1, sizes and `borderRadius` to
zero or more) while the unclamped value keeps driving the physics. `AnimatePresence` needed no
change; the React `motionKey` now hashes the whole transition, so changing only a spring option
starts a new generation.

**Why native, and not Framer Motion.** Framer runs its physics in JavaScript every frame and writes
the DOM. In gpuix that would be a JS-to-native style mutation per frame per animated element, with no
`requestAnimationFrame` to pace it (only `setTimeout`, which is not synced with the display link),
and most of Framer has nothing to attach to: DOM measurement, layout animations, gestures and
MotionValues bound to CSS. Rust already resolves motion per frame inside the GPUI render, with no
React render or N-API call. The API stays Framer-shaped (`initial` / `animate` / `transition`), so
components port without a rewrite.

**Alternatives rejected.**

- **Numeric integration per frame** (what Framer's generators do). It needs a stable step, so it
  depends on frame timing; the closed form is exact and stateless, which also keeps frozen-clock
  frame capture deterministic.
- **Porting motion-dom's "time-defined springs ignore inherited velocity".** motion-dom drops the
  velocity for `duration` / `bounce` springs because it overshoots small moves. Carrying it is the
  point here, and a critically damped spring cannot oscillate from it. The cost is that an
  underdamped duration spring interrupted at high speed can overshoot a short move.
- **Negative `bounce` (overdamped) and `visualDuration`.** motion-dom has both; the first needs
  `overdampedFreqScale`. Neither was needed for the target components, and physics-form springs can
  already be overdamped.

**Trade-offs.** `StyleDesc` has no `transform` (see [area E](#area-e-motion) for why), so an enter
animation such as `scaleY` still cannot be expressed. The settle snap is at most `restDelta`
(0.5 units, or 0.005 under 5). `duration` is where the spring has decayed to 0.1% of the move, as in
motion-dom, not a hard stop. Springs with a very large `sqrt(stiffness / mass)` or
`damping / (2 * mass)` (over 1e6) are rejected, because the closed form would overflow. Setting
`stiffness` and `duration` together, or a spring option on a tween, is an error where motion-dom would
silently prefer one.

**Upstreaming.** Self-contained in `packages/native/src/motion.rs` (now `src/motion/`, see
[area E](#area-e-motion)), the transition types in `packages/native/js/host.ts`, the React
`motionKey`, tests and the README section, plus a changeset. `remorses/gpuix` requires an approved
issue first.

## The principle: DOM and CSS semantics

Areas A to E follow one rule: **behave like the web DOM and CSS.** Anything a web developer can do
works the same way here, with DOM semantics and DOM-shaped names (`onPointerDown`, `tabIndex`,
`scrollIntoView`, `position: sticky`). The work is not allowed to add gpuix-only behavior, or to keep
the old default and put DOM behavior behind an opt-in. A deviation is accepted only where the DOM
model cannot be built on GPUI, and each one is listed in its area's "Deviations" section with the
reason.

Why this and not a compatibility layer: the fork's consumer is a port of web components. Every
place gpuix differed from the DOM (an element that blocks hover on its parent, a `scroll` event that
meant wheel, a `position` that defaulted to `absolute`) became a workaround in the component, and
the workarounds were what made the port expensive. Upstream tests that asserted the non-DOM behavior
were rewritten to the DOM behavior instead of being kept alive.

Consequences that apply to every area:

- **Breaking changes are taken, not shimmed.** The fork has no users to protect, so there are no
  compatibility flags. Each area lists its breaking changes so an app author can find what to edit.
- **New code lives in new modules** (`packages/native/src/events/`, `scroll.rs`, `layout/`, `css/`,
  `motion/`, `packages/native/js/events/` and so on). `renderer.rs`, `style.rs` and `host.ts` get
  wiring only, which keeps the rebase onto upstream tractable.
- **The README is the API contract**, as upstream's `AGENTS.md` requires. Behavior is documented
  there, not here.
- **macOS is the platform that was exercised.** Several features have a macOS-only path (Metal
  effects, pointer lock, the synchronous dispatch that lets `preventDefault` cancel a native default,
  the OS reduced-motion read). The Windows, Linux and wasm paths were written but not compiled by the
  people who wrote them, and the zed shader edits for them were never built or rendered (see area D).
  A pull request on the fork runs `.github/workflows/ci.yml`, whose matrix compiles Linux, Windows and
  Windows ARM64; that is the cheapest first check. The wasm build (`bun run build:web`) needs nightly
  Rust and `wasm-bindgen`. Each area names its own gaps, with what closing each would take.

## Area A: pointer events

**Problem.** GPUI decides hover per element and hands gpuix one listener per node. That gave
gpuix-only rules: a filled or absolute box blocked hover and clicks to its ancestors, an element with
both `onMouseDown` and `onMouseMove` silently captured the pointer, `click` was synthesized from
mouse-up with no press-origin check, `pointerEvents: "auto"` meant "block the wheel", and a popup
could not tell whether a press was inside its trigger. There was no capture phase, no `target`,
no pointer events and no drag and drop.

**Design.**

- **One hit test per pointer event.** zed gets an opt-in DOM hit-testing mode per window
  (`Window::set_dom_hit_testing`, default off, so other GPUI users are unaffected). Each hitbox records
  its parent. The hit is the topmost hitbox that is not `HitboxBehavior::PassThrough`, plus its
  parent chain. `PassThrough` is `pointer-events: none`: never a target, but still in the chain when a
  descendant is hit. A deferred (anchored) layer is its own hitbox root, so hover, focus and wheel
  over a popup follow layout, as a portal does in `body`.
- **A native dispatcher** in `packages/native/src/events/` owns the events. One set of window-level
  GPUI listeners per view turns each input into a path (target first, then logical ancestors; an
  anchored layer continues to its tree parent) and emits DOM events. Per-view `PointerState`
  (`events/state.rs`) holds the hovered and active chains, press targets, click count and pointer
  capture. A type is emitted only if some element on the path, or `window` or `document`, listens to
  it. JS keeps the window and document listeners, so it tells native which types they hear
  (`syncWindowEventListeners`); a pointer move over bare background, or over no element at all inside
  the window, therefore reaches them (the second as an event with no target).
- **JS propagation** is `dispatchDomEvent` in `packages/native/js/events/dispatch.ts`: capture, target,
  bubble along `payload.path`, with `stopPropagation`, `stopImmediatePropagation`, `preventDefault`
  and `on<Event>Capture` props. Native sends element ids; JS resolves them through the adapter's
  element source, so `target`, `currentTarget` and `relatedTarget` are the objects a `ref` gives.
- **Events.** Pointer and mouse events (pointer first); `over`/`out` bubble, `enter`/`leave` fire per
  element in DOM order; `click` fires on the nearest common ancestor of press and release targets,
  with `auxclick`, `dblclick`, `detail` and `contextmenu`; `wheel`; `mousedownoutside` for every
  listener whose logical subtree does not contain the press target (popups and submenus in anchored
  layers count as inside their trigger). Payload: `target`, `currentTarget`, `relatedTarget` (elements), `composedPath()`,
  `elementId` (the numeric id), `clientX/Y`, `offsetX/Y` (the target's padding edge), `button`, `buttons`, modifiers, `detail`,
  `pointerId`, `pointerType`, and `movementX/Y` (the motion on a move, 0 on every other event). A
  `pointerdown` a handler cancelled suppresses the press's compatibility events: its `mousedown`, the
  `mousemove`s while it is held and its `mouseup`. `click` still fires, as in the DOM.
- **Pointer capture is explicit**: `setPointerCapture` / `releasePointerCapture` /
  `hasPointerCapture` with `gotpointercapture` and `lostpointercapture`. Implicit capture is gone.
  The call is in force when it returns (`PointerRequests`): the next pointer input reads it and fires
  `gotpointercapture` first. `cancelDrag` works the same way.
- **State styles come from `PointerState`.** `:hover` applies to the target and every ancestor,
  `:active` to the primary press chain. `resolve_state_styles` (`events/state_styles.rs`) folds hover
  and active into the base style before motion runs. A hovered `color` reaches descendants'
  `currentColor`.
- **Drag and drop.** File drags fire `dragenter`/`dragover`/`dragleave`/`drop` with
  `dataTransfer.files` (`path`, `name`, `size`, `type`, `lastModified`). `draggable` elements start
  a session after 4 px of movement and fire `dragstart`, `drag`, `dragend` and in-app drops through
  the same code. `dataTransfer` is a JS object per session. During a drag no pointer or mouse events
  fire, as in the DOM. Escape during an element drag cancels it and reaches no `keydown` listener
  (`events/keyboard.rs` does not send it), as in Chrome.
- **The OS drop operation of dragged-in files follows `dragover`** (macOS). A `dragover` that calls
  `preventDefault` with a `dropEffect` of `copy`, `move` or `link` sets the drag cursor through
  `Window::set_drag_operation` (a zed commit; `gpui_macos` answers `draggingEntered` and
  `draggingUpdated` with it, within what the source allows). Anything else refuses the drop at the OS,
  and a window with no `dragover` listener refuses file drops. The file-drag events are delivered
  synchronously for this, from inside the OS drag callback.
- **`cursor: "none"`, `requestPointerLock` / `exitPointerLock`** (macOS), backed by zed commits for
  `CursorStyle::None` and pointer lock. The lock is taken in the call, from an input handler too
  (`GpuixRenderer::settle_pointer_lock`), so `pointerLockElement` is current and the next pointer
  event is aimed at the lock element. `pointerlockchange` and `pointerlockerror` are events at
  `document` (`event.target` is `document`, they bubble to `window`, the element is
  `document.pointerLockElement`). `cursor: none` hides the cursor per window (`CursorHide` in
  `gpui_macos`): it ends when the pointer leaves the window, the window stops being key or it closes.

Cross-area decisions that touch A:

- **A press, a wheel and the file-drag events reach JS synchronously.** On macOS and in the test
  renderer, `mousedown`/`pointerdown` and `wheel` run in the capture phase through
  `events/sync_dispatch.rs`. `preventDefault` on a `mousedown` cancels the focus default and the input
  editor's focus (it calls `window.prevent_default()`); `pointerdown` does not, as in Chrome. On a wheel
  it cancels that input's scroll. This is how an event handler gets to veto GPUI's native defaults (with
  B and C).
- **Handler errors go to `onError`** if the renderer has one, else `reportError`. Dispatch continues
  after a throw and simulate calls no longer rethrow.
- **`inert` joins the hit test** (with B and C): it behaves like inherited `pointer-events: none`.
  A pass-through scroller never claims the wheel, including a latched gesture.
- **Disabled controls get no `click`, `auxclick` or `dblclick`** (HTML). Pointer and mouse move
  events still reach them. `element.click()` and accessibility clicks skip them too.
- **`clipPath` clips hit testing** (with D): a point outside the clip falls through to what is below,
  and the clip cuts the element's subtree with it.
- **`borderRadius` cuts the hit area of the box itself** (with D), whatever its `overflow`: a pointer
  over a rounded-away corner reaches what is below, as in a browser. A child covering the corner is
  still hit through its own box.
- **Label activation follows the click rule** (with B): see [area B](#area-b-focus-keyboard-forms-a11y-and-platform-apis).
- **`Reorder` (React and Solid) calls `setPointerCapture` on `pointerdown`** and ends the drag on
  `pointerup`, `pointercancel` or `lostpointercapture`. On Windows and Linux (where `pointerdown`
  reaches JS through a queue) a move that arrives before the handler runs is not captured; on macOS the
  press is dispatched synchronously and no move is lost.
- **Transition and animation events go through this dispatcher** with `target` and `path`, and bubble
  (they are not cancelable). Native emits the lowercase DOM names (`transitionrun`, `animationend`,
  ...) and each has an `on…Capture` twin.

**Breaking changes.**

- A filled or positioned element no longer blocks hover or clicks to its ancestors. `click`,
  `mousedown`, `mousemove` and `mouseup` bubble, so code that relied on occlusion needs
  `stopPropagation`. The anchored `occlude` prop is removed; use `pointerEvents`.
- `pointerEvents: "none"` is inherited (a descendant sets `auto` to restore it), and an element with
  `none` is never a target even if it has listeners. `auto` no longer blocks the wheel.
- The wheel goes only to scrollers that are ancestors of the target, so a modal or popup no longer
  scrolls the container it was declared in. `onWheel` and `onWheelCapture` are pointer-event props
  (`HostWheelEvent`: payload fields plus `deltaX/Y` and `deltaMode`).
- Implicit capture (a node with `onMouseDown` and `onMouseMove`) is removed. Call `setPointerCapture`.
- `onFileDrop` and the `paths` field are removed. Use `onDrop` with `dataTransfer.files`.
- Event type strings are lowercase DOM names (`mousemove`, not `mouseMove`).
- `pointerlockchange` and `pointerlockerror` are `document` events; the root `onPointerLockChange` and
  `onPointerLockError` are removed. `movementX/Y` are required fields of `HostMouseEvent` (0 on every
  event but a move), and `HostMouseMoveEvent` and `HostPointerMoveEvent` are removed.
- Escape during a `draggable` element drag no longer reaches `keydown` listeners or closes a dialog.
- On macOS a window with no `dragover` listener refuses files dragged in, and a `dragover` that does
  not call `preventDefault` refuses the drop.
- Clicks from the keyboard or VoiceOver are DOM clicks with `detail` 0, `pointerId` -1 and
  `pointerType` `""`. `buttonProps(behavior, renderer)` takes the renderer.
  `createPointerCaptureMethods` is renamed `createElementMethods`.
- A handler that throws no longer ends the dispatch, and nothing rethrows into the caller of a
  simulate method. Errors reach `onError`.
- `target`, `currentTarget` and `relatedTarget` are the elements a `ref` gives, not numeric ids;
  `path` is `composedPath()`, `elementId` keeps the id, and `target` is `null` for an event aimed at
  the window. A handler type is `DomEvent` / `HostMouseEvent`. Window-level callbacks get the same
  resolution; the raw `onEvent` tap keeps ids.
- A `window` or `document` listener hears every event of its type, not only those an element on the
  path listens for.
- A `mousedown`/`pointerdown`, `mouseup`/`pointerup`/`click` or `wheel` handler runs inside the
  synchronous dispatch on macOS (and in the test renderer), as key handlers do. The input does not lease
  the root view (GPUI's input callback is `AnyWindowHandle::update`, and the test renderer dispatches
  the same way), so every renderer call works from a handler and reads current values.
  `document.activeElement` is the focused element. Only a call that starts another input or drives the
  test app (`simulate*`, `advanceTime`, `captureScreenshot`) cannot run from a handler. A new napi
  method goes through `sync_dispatch::route` and the `update_*` helpers.
- Disabled controls get no `click`/`auxclick`/`dblclick`.
- A press whose `pointerdown` was cancelled sends no `mousedown`, `mousemove` or `mouseup`; code that
  cancels `pointerdown` and still listens for `mousedown` stops hearing it.
- Text with no element of its own has no hitbox; the element that holds it is the target.

**Deviations from the DOM.**

- **Windows and Linux dispatch asynchronously.** `preventDefault` cannot cancel a native default
  there (a press's focus, a wheel scroll), a label always activates its control, and the first move
  of a `Reorder` drag can be lost (it captures the pointer from a queued `pointerdown`). Not
  infeasible, a consequence of the threading: on macOS GPUI runs on the JS thread
  (`MacPlatform::new_embedded`), so an input callback is already inside a napi call and
  `sync_dispatch::emit` can call the handler directly. `gpui_windows` and `gpui_linux` have no
  embedded loop, so gpuix runs `Platform::run` on its own thread, and a blocking round trip can
  deadlock: several JS-to-UI calls (clipboard, scroll metrics) block the JS thread on a reply, so a
  UI thread waiting on a JS handler and a JS thread waiting on the UI thread stall together. Two
  ways out. A request/reply bridge in gpuix: the UI thread waits on a channel for the handler's
  `{defaultPrevented, propagationStopped}` while servicing the commands that handler issues, with
  a timeout and the queued path as the fallback; a busy JS thread then stalls input. Or embedded run
  loops for Windows (a `PeekMessage` pump) and Linux (a calloop dispatch with a zero timeout) in zed,
  after which the macOS design applies unchanged and the second thread and its command enum go.
  Either must be built and tested on a Windows or Linux host. The OS drop operation of dragged-in
  files is macOS only.
- **`preventDefault` on a `wheel` cancels that one input.** A browser that sees the first wheel of a
  gesture cancelled cancels the whole gesture; here the next input scrolls unless its own handler
  cancels it too, so a handler that wants a gesture cancelled calls `preventDefault()` on each input.
- **No drag image, and no drag to or from other apps.** `setDragImage` is a no-op, and a dragged
  element's `dataTransfer` stays in the window. GPUI is not the obstacle: it has an in-window drag
  ghost (`AnyDrag`, painted last at the pointer) and, on macOS and Wayland, an OS drag session that
  carries files. gpuix runs its own drag state machine (`events/element_drag.rs`) and uses neither,
  and `App::active_drag` is crate-private in zed. What a ghost would take: a public `App::start_drag`
  in zed (small), and in gpuix a ghost view that rebuilds the dragged subtree with no listeners at
  the pointer under its own element-id scope, a `setDragImage` napi call
  and the default semi-transparent copy. GPUI cannot snapshot an element to
  a bitmap (`render_to_image` is test-only), so the ghost is a live second build of the subtree per
  drag frame. Dragging out to other apps needs more: the OS payload is files only
  (`ExternalDragPayload::Files`), so text and URL payloads need a pasteboard writer in `gpui_macos`,
  `DoDragDrop` and `IDataObject` on Windows, and XDND on X11; a custom image on macOS needs an
  offscreen render to an `NSImage`. Dropping text or URLs in from other apps needs each platform's
  drag-enter handler to read more than file URLs.
- **Event paths and `:hover` follow the logical element tree** (an anchored popup is inside its
  trigger, as with a React portal), while GPUI's hit chain follows layout. A trigger therefore keeps
  `:hover` while the pointer is in its popup.
- **`cursor: none` and pointer lock are macOS only.** Only `gpui_macos` implements pointer lock
  (`CGAssociateMouseAndMouseCursorPosition`); the platform trait's default refuses it. `cursor: none`
  has no Windows implementation that was tested (`CursorStyle::None` maps to no cursor there) and
  Linux shows the default cursor. Lock needs `ClipCursor`, a hidden cursor and raw-input deltas on
  Windows, `zwp_pointer_constraints_v1` with `zwp_relative_pointer_v1` on Wayland, and XInput2 raw
  motion with a grab on X11; Wayland `cursor: none` is `wl_pointer.set_cursor(null)`. Elsewhere
  `movementX/Y` of a move are 0.
- **`contextmenu` fires on mousedown** (macOS timing) on every platform. Moving it to mouseup is a
  small gpuix change.
- **`clipPath` and `borderRadius` hit testing cover every host element**, whose hitbox comes from a
  styled box (`<virtual-list>` inserts its own shaped hitbox, and its scrollbar sits inside the clip).
  A hitbox a custom element inserts itself with `Window::insert_hitbox` has no clip and square
  corners; `insert_hitbox_shaped` takes both.
- **Window drag regions** (`appRegion`) are a separate feature that builds on this dispatcher; see
  [Window drag regions](#window-drag-regions).

**Upstreaming.** The zed side is opt-in and independent: the DOM hit-testing mode, `CursorStyle::None`,
pointer lock, the per-window cursor hide, the drag operation of dragged-in files
(`Window::set_drag_operation`), `active_drag_value` and the pass-through hitbox can go to `remorses/zed`
as separate commits. The gpuix side removes behavior upstream apps depend on (occlusion, implicit
capture, `onFileDrop`), so it is a major-version discussion, and `remorses/gpuix` wants an approved
issue first.

## Area B: focus, keyboard, forms, a11y and platform APIs

**Problem.** Focus fired in the wrong order (the old element's `blur` before the new element's
`focus`), listening for a key or focus event made an element focusable, `event.key` was lowercase
(`"enter"`), editors ran their built-in key actions before JS could cancel them, there was no `tabIndex`,
`<input>` lacked the selection API, only a few `aria-*` attributes mapped to AccessKit, and a web
form could not submit, copy or pick a file the usual way.

**Design.**

- **Focus.** DOM order (`blur`, `focusout`, `focus`, `focusin`), `focusin`/`focusout` bubble,
  `relatedTarget`, `:focus-visible` with the browser's keyboard-modality heuristic (readable with
  `matches(':focus-visible')`), a `focusWithin` state style, `tabIndex` as in the DOM (`-1` is
  focusable by script only, positive values follow DOM order), `focus({ preventScroll })` and
  `blur()`. A listener no longer makes an element focusable. The focus events of a press, of
  `autoFocus` and of a frame reach JS before the call that caused them returns (`settle_focus`, macOS
  and the test renderer), so `document.activeElement` is current in the next handler. `autoFocus`
  focuses in the commit that mounts the element. `focusNext()` and `focusPrevious()` scroll the new
  focus into view with `nearest`, as Tab does.
- **Keyboard.** `event.key` and `event.code` are DOM values, with `repeat` and modifier fields
  (zed reports key auto-repeat). On macOS `code` comes from the virtual key code (`NativeKey::key_code`
  through Chromium's table in `events/key_codes.rs`) and `key` is the layout's character with shift and
  option applied (`NativeKey::layout_char`), as in Chrome. `keydown` and `keyup` also carry the legacy
  `keyCode` and `which` with Chromium's macOS values (NumLock is 12, an input method's `Process` 229).
  There is no `keypress` and no `charCode`. An input method composing in an `<input>` or
  `<textarea>` gives `isComposing`, a `Process` `keydown` for a key it takes first (observe-only: an
  `ImeKeyDown` that `preventDefault` cannot stop) and `compositionstart`/`compositionupdate`/
  `compositionend`. Each keystroke is one `keydown` that goes through capture and
  bubble. Adapters export `window` and `document` objects for window-level listeners; no globals are
  installed. In text editors, `keydown` runs before the built-in action, and `preventDefault`
  cancels both the action and the text insertion.
- **Inputs and forms.** `select()`, `setSelectionRange`, `selectionStart/End/Direction`, `disabled`
  (never focusable), `type`, `maxLength`, number `min/max/step`. `<form onSubmit>`, `name`,
  `required`, Enter to submit, `requestSubmit()` and `invalid` events. The per-edit event is `onChange`
  with `inputType`, `data` and `isComposing`, once per edit like the DOM `input`; a composition's last
  `change` comes before `compositionend`.
- **Accessibility.** Every `aria-*` attribute is accepted and mapped to AccessKit where a mapping
  exists; `id` replaces `aria-id`; relations resolve by author id (a zed commit); `<label htmlFor>`
  names and activates its control; `inert`. The test renderer's tree dump reports a descendant of an
  `aria-hidden` or `inert` node as hidden (a zed commit: AccessKit's flag covers descendants, the dump
  read only the node that carries it). A referenced element with no role becomes a `group` node
  so `aria-labelledby` can name it from its text.
- **Platform APIs.** `navigator.clipboard.writeText`/`readText` (attached to an existing `navigator`,
  never creating one; `@gpuix/native` declares the global `Clipboard` type once for both adapters), file picker `accept` filters (a zed commit), `EyeDropper` on macOS.
- Solid `DialogPortal` built its children twice; fixed, along with the same pattern in the
  floating components.

Cross-area decisions that touch B:

- **Mousedown focus follows A's path.** The focus default (`focus.rs`, `defer_mouse_down_default`)
  uses the dispatcher's target path, not the topmost hit, and blurs when nothing on the path is
  focusable. A `preventDefault` from any `mousedown` handler on the path cancels both the focus move
  and the blur, including an input editor's own focus; `pointerdown` does not cancel it, as in Chrome.
  The order is `pointerdown`, `mousedown`, `blur`, `focusout`, `focus`, `focusin`, pinned in
  `focus.test.tsx`. There is no second mousedown emission.
- **Label activation follows the click rule.** After a `click` whose path contains a label (and whose
  target is not inside the labelled control or other interactive content in the label), if it was not
  default-prevented, gpuix focuses the control and dispatches a `click` on it. A pointer release,
  `element.click()` and the VoiceOver Press all run it (`events/activation.rs`). The release goes through the synchronous dispatch from a deferred
  step, so queued focus handlers run first.
- **`inert` is in the hit test**, inherited like `pointer-events: none`, and a descendant's `auto`
  does not undo it. An inert scroller never takes the wheel.
- **Disabled controls** get no click-family events or activation (see A).
- **Handler errors** from window and document listeners go to `onError`, like A's.

**Breaking changes.**

- Key, `onFocus` and `onBlur` listeners no longer make an element focusable; give it a `tabIndex`. A
  disabled input is never focusable.
- `event.key` uses DOM names (`"Enter"`, `" "`, `"ArrowLeft"`).
- `windowKeyDown`, `windowKeyUp` and `keystrokeId` are removed; one `keydown` per keystroke propagates.
- React `onFocus`/`onBlur` bubble, as in React DOM. Solid's do not, as on the web.
- `onSubmit` is removed from `<input>` and `<textarea>`; use `onKeyDown` or wrap the input in a
  `<form>`. In a textarea, Enter always inserts a newline. `TextareaProps` no longer extends
  `InputProps`.
- `aria-id` is replaced by `id`.
- Positive `tabIndex` follows DOM order instead of GPUI's raw ordering.
- Mousedown on content with no focusable ancestor now blurs the focused element.
- `autoFocus` focuses in the commit, not on the next frame, so a `mousedown` handler that mounts an
  `autoFocus` popup must call `preventDefault()` to keep focus there; the press takes focus after the
  handlers, as on the web.
- On macOS `event.code` is the physical key and `event.key` the layout's character (`"ß"` for Option+s,
  `code` `"KeyC"` for the C position on a Cyrillic layout, `"NumpadEnter"`). `keydown` can carry key
  `"Process"`, and `isComposing` is true while text is marked.
- Handlers throwing, the focus default and `click` on disabled controls: see A.

**Deviations from the DOM.**

- **Windows and Linux** dispatch asynchronously, so `preventDefault` cannot cancel editor actions
  there, a label always activates its control, and form `invalid`/`submit`
  events arrive after `requestSubmit()` returns. The focus events of a press, `autoFocus` and a frame are
  queued there too. Why and what it would take: see area A.
- **Input methods other than macOS's.** Windows (TSF or IMM32) and Linux (ibus, text-input-v3) report
  `isComposing` false and fire no `Process` keydown or composition events, and their composition keys
  go to the input method before GPUI sees them. On macOS a value an app sets while a composition is open
  ends it without a `compositionend`.
- **`beforeinput`, `onInput`, `keypress` and `charCode` do not exist.** Text editing reports through
  `onChange` only, after the edit, so a handler cannot cancel one edit by type; `keydown`'s
  `preventDefault` cancels the key's action and its text.
- **`code`, `key`, `keyCode` and `which` off macOS** come from GPUI's key name assuming a US layout, because the other
  platforms leave `NativeKey` empty. They have the physical key at event creation (the Windows scan
  code, the xkb keycode) and each needs a table like `events/key_codes.rs`.
- **File picker `accept`** works on macOS and goes through the portal filters on Linux, which are
  implemented and untested. Windows ignores it: `file_open_dialog` never calls `SetFileTypes` for an
  open dialog. It needs a `COMDLG_FILTERSPEC` list built from `PathPromptOptions.filters`.
- **`EyeDropper`** exists on macOS only. Its abort has no cancel call (`NSColorSampler` has no dismiss
  API), so the magnifier stays up until the user clicks or presses Escape; nothing to build there.
  Windows has no system picker (it would be a custom overlay reading the screen), and Linux could use
  the portal's `Screenshot.PickColor`.
- **`readOnly` controls** are submitted but not validated, and `select()` does not move focus.
  Password inputs get the password role and expose no value; number inputs get the spin-button role.

**Upstreaming.** The zed commits are small and independent (a11y relations by author id,
`PathPromptOptions` accept filters, key auto-repeat on `KeystrokeEvent`, `Entity::is_updating`, the
`NativeKey` (virtual key code and layout character) and `is_composing` of key events, the observe-only
`ImeKeyDown` and `Window`'s input-method driving calls) and can go to `remorses/zed` one by one. The
gpuix side depends on A's `dispatchDomEvent` and `sync_dispatch`, so it cannot land before A.

## Area C: layout, measurement and scrolling

**Problem.** `getElementBounds` could be stale until the next paint and reported an inconsistent box.
A padded scroller scrolled by twice its padding, a both-axis scroller was forced to `display: block`,
whitespace Solid text nodes became flex items and added a `gap` slot, `position` defaulted to
absolute-like behavior, there was no `sticky`, `aspectRatio`, `order`, `margin: "auto"` or real grid
syntax, no overlay scrollbars, nested scrollers fought over the wheel (the full delta applied to
every scroller under the pointer), `scrollIntoView` failed for nested descendants, and
`<anchored align="center">` offset twice. `display: table` did not exist, `inline-block`, `inline-flex` and `inline-grid` were
ignored (the box stayed a block), and a component could not change with the size of its container.

**Design.**

- **Bounds are never stale.** `getElementBounds` and the new `getBoundingClientRect` return the border
  box. A read in the same turn as a mutation flushes the JS queue and draws synchronously
  (`Window::draw_if_dirty` in zed), as a browser forces layout. Same-turn reads therefore cost a draw.
  `ResizeObserver` and `IntersectionObserver` follow DOM semantics.
- **Scrolling engine** (`packages/native/src/scroll.rs`, `scrollbar.rs`; zed `scroll_routing.rs`).
  Scrollable overflow follows CSS: `auto` behaves like `scroll`, `hidden` can be scrolled only from
  code, `clip` does not scroll, and overflow never changes `display` or wraps children. macOS-style
  scrollbars (drag, track paging) are on by default: overlay bars, or with "Show scroll bars:
  Always" (which macOS also picks while a mouse is attached) classic bars that take a 15 px gutter out of the
  container, read from the OS and refreshed on its notification. One wheel event goes to the
  innermost scroller that can use any of it, which takes it whole; the rest is dropped and chaining
  happens between gestures. A scroller's range is scrollable overflow over all descendants. A trackpad gesture,
  momentum included, stays latched to the scroller that started it. `overscrollBehavior`,
  `scrollIntoView({ block, inline, behavior })` through every ancestor, `scrollBehavior: smooth`, and
  arrow, Page, Home, End and Space scroll the nearest scroller as a default action.
  `scrollTop`/`scrollLeft`/`scrollWidth`/`scrollHeight`/`clientWidth`/`clientHeight`/`scrollTo`/`scrollBy`
  are readable. Virtual lists take part in the same routing.
- **Events.** `onScroll` fires on any offset change (wheel, drag, keys, `scrollTo`) and does not
  bubble. The wheel is a separate event, `onWheel` (see A).
- **Layout.** `position` defaults to `static`; `absolute` resolves against the nearest positioned
  ancestor (zed `taffy/containing_block.rs`); `fixed` against the window; `sticky`. An absolute box is
  clipped and scrolled only by its containing block and what is outside it, a fixed one by the window
  alone (see C2 below). Positioned elements paint above in-flow ones and `zIndex` orders them across
  stacking contexts as CSS 2 Appendix E (see C3 below). Also `aspectRatio` (and
  `<img>` sizing like a replaced element), `order`, `row-reverse`/`column-reverse`,
  `margin: "auto"`, full CSS grid syntax (tracks, placement, areas, auto flow; zed
  `grid_template.rs`), `display: none` and `display: contents`. An empty or whitespace text node
  produces no box and no `gap` slot (D's inline builder owns this rule).
- **Anchored popups** (gpuix component, not DOM). Floating-ui style placement: `align="center"` offset
  fixed, flip clears the trigger and re-flips live, `onPlacement` reports available size and anchor
  size, and `anchor-size()` is supported.
- Wheel is a DOM event through A's dispatcher, and `preventDefault` cancels it (see A). A pass-through
  scroller never claims the wheel.
- **Tables are a table algorithm in taffy, not a grid mapping.** Mapping a table onto grid (`repeat(N, auto)`
  columns, row groups `display: contents`) would have been a few days, and would have left out
  `table-layout: fixed`, `%` column distribution and `border-collapse`. The fork already carries taffy, so
  `third_party/taffy/src/compute/table/` implements CSS Tables 3 as Chrome lays it out (structure, column
  widths, row heights, `colSpan`/`rowSpan` distribution, captions, header and footer groups, cell
  `vertical-align` on the row baseline), compared against headless Chrome in its tests. gpui carries
  `Display::Table`, a `TableRole` per node and the table styles. taffy makes a child that is no table part a
  cell of its own; gpuix does the CSS wrapping instead (anonymous cells and anonymous tables,
  `layout/table.rs`), so the tree gpui lays out is always well formed. `border-collapse` is split three
  ways: gpuix resolves which border an edge has at build (CSS 2.1 17.6.2.1, `layout/collapse.rs`, over the
  slot grid `layout/table_grid.rs` re-derives from taffy's placement), layout gets Chrome's exact half
  widths, and the table paints each edge once, centred on the grid lines layout recorded and above the cell
  backgrounds (zed `collapsed_borders.rs`); cells paint no borders. Painting by the recorded lines means
  that when the two grids disagree nothing is drawn, not a wrong border. Copied text has a tab between the
  cells of a row (see D's selection order). `css/table.rs` parses the table styles.
- **Atomic inlines are a placeholder in gpui text.** `inline-block`, `inline-flex`, `inline-grid` and
  `inline-table` put one U+FFFC run in the paragraph whose advance is the box's margin-box width and whose
  ascent and descent come from the box's baseline (zed `text_system/inline.rs`, `vertical-align` on the line
  box as CSS 2.1 10.8). The box is a real element owned by `StyledText` (`elements/atomic_inline.rs`):
  measured before shaping (min and max-content, shrink-to-fit against the line), laid out again at the used
  width, then prepainted and painted at the placeholder's position, so events, hit testing, focus, bounds and
  observers work on it as on any block. The text keeps a private layout engine per paragraph and each box is
  a layout root in it, because the text measures the boxes while the window's engine is taken (it is in the
  middle of laying the window out). Two consequences: a box that fits costs two subtree layouts per frame, and
  an absolute descendant resolves against the atomic inline. A box with a `min-width` or `max-width` in `%` is
  laid out with the width of the paragraph as the parent size its percentages are of (taffy
  `compute_root_layout_with_parent_size`), measured again per paragraph width. gpuix side: `layout/display.rs` (the
  inline-level values, blockified where CSS blockifies them), `text/atomic.rs`, the planner in
  `text/inline.rs`, `css/vertical_align.rs`.
- **Container queries are same-frame.** A container's size is known only after layout, and the tree is built
  from the style that size picks. A lag of one frame (query against the last measured size) was the cheap
  option and was not taken: a mount or resize that flips a query would show the old style for a frame. The
  build reads the content box the last layout recorded (`ContainerStore`, `layout/container.rs`), each
  container's prepaint listener (`Div::on_prepainted_layout`) compares its new box with what the build
  assumed for the axes something read, and a mismatch calls `Window::request_redraw_before_present`. zed's
  `Window::draw` then drops the pass before paint, as it drops a measure pass, and builds again, up to
  `set_max_draw_passes` (4, one per level of containers that flip each other); at the cap the last pass is
  presented and the window draws once more, so a loop costs a frame per step and never hangs. Only the
  presented pass paints, so bounds, observers, `onLayout`, registries and a11y never see a dropped one;
  what a build publishes outside the frame therefore has to be idempotent. Cost: a steady frame pays one
  comparison per read container; a flip of 500 containers (4.5k elements,
  `examples/container-queries.perf.test.tsx`) takes 1.9 to 2.0 times a steady frame. A window that allows
  more than one pass does not use GPUI's view cache; gpuix never builds cached views, so that costs
  nothing. `containerType` is size containment in taffy (`contain_inline_size`, `contain_block_size`: the
  content-based size ignores the content on those axes) plus a formatting context of its own; it is no
  containing block and no stacking context, as in Chrome. `cq` units ride in `LengthContext`. The grammar
  is `css/container_query.rs`, three-valued, unknown is false. The `display` a block gives is settled
  before the paragraph planner and the table role read it (`settle_children`).
- **Text has a first baseline.** A measured leaf reports the first baseline of its first row to taffy
  (`MeasuredLeaf`), so a `baseline`-aligned table cell, an `inline-table` and flex and grid items with
  `alignItems: baseline` line up by their text, as in Chrome. Two grid bugs showed up against Chrome and
  are fixed in the vendored taffy (an item with another `alignSelf` was shifted to the others' baseline;
  a grid's baseline was its first item's bottom).

**Breaking changes.**

- `position` defaults to `static`. `top`/`left` without a `position` do nothing, and an absolute box
  resolves against its nearest positioned ancestor, not its parent.
- Positioned elements paint and hit-test above in-flow ones, and `zIndex` orders them across stacking
  contexts (a descendant can reach over its parent's later sibling). `getElementsAt` lists elements in
  paint order. `DialogPopup` defaults to `position: relative`, or its absolute backdrop would cover it.
- A static `overflow` ancestor no longer clips or scrolls an absolute box; give the ancestor a
  `position` to clip it.
- `onScroll` is now the offset-change event, the wheel handler is `onWheel`, and `onScrollChange` is
  removed. The wheel delta sign flipped: positive is down, and the simulated wheel
  (`nativeSimulateScrollWheel`, `dispatchScrollWheel`, the automation `wheel`) takes that sign too.
  `nativeSimulateScrollWheel` without a phase starts a new gesture.
- `overflowX: "scroll"` no longer forces flex and `min-width: 0` or wraps children. Children need an
  explicit width or `flexShrink: 0` to overflow. The vertical-wheel-onto-x remap is gone, and a
  horizontal-only scroller no longer sizes its children automatically.
- `scrollIntoView()` with no arguments aligns to the top.
- Arrow, Page, Home/End and Space scroll the nearest scroller unless `preventDefault`ed. The built-in
  Select and Combobox do; app listboxes must too. Space on a focused element with `onClick` activates
  instead of scrolling.
- `scrollbarWidth` is `"auto" | "thin" | "none"` and `scrollbarColor` is `"auto"` or `"<thumb> <track>"`.
  Scrollbars show by default. With the classic style they take layout space, so `clientWidth` and the
  content box shrink.
- Virtual lists take the wheel from, and give it to, surrounding scrollers between gestures.
- A wheel event no longer chains to the outer scroller in the same event: the first scroller that can
  use any of it takes it whole, and the outer one moves on the next gesture. A test that expects the
  outer scroller to move sends a second gesture (`"started"` phase).
- A scroller's range counts every descendant that overflows, down to one that clips, not only direct
  children. An absolute one counts only for its containing block; fixed, anchored and deferred
  layers never count.
- `<input>` bounds are the border box. A same-turn bounds or scroll read costs a synchronous draw.
- Anchored popups flip (and shift) instead of snapping over the trigger.
- An `<img>` with a `%` width or `maxWidth` gets its height from its intrinsic ratio.
- `margin*` and the inset props are `DimensionValue` (number, `%` or `"auto"`).
- Empty and whitespace text no longer takes a gap slot.
- Scroll geometry on a renderer that lacks it, half of a `scrollTo` pair, and an unknown
  `behavior` or alignment string throw.
- `display: "inline-block"`, `"inline-flex"` and `"inline-grid"` are atomic inlines that sit on a line of
  text, where they were ignored and left the box a block. A flex or grid item, or an absolute box, blockifies
  them as CSS does. `display: "table"` and the `table-*` values, ignored before, lay out as tables.
- `alignItems` / `alignSelf: "baseline"` align by the first line of the text, where they aligned by the
  bottom of it. Grid baseline alignment changed with it.
- A query container (`containerType: "inline-size"` or `"size"`) is size contained: with no `width` in a
  flex row or an absolute box it is as wide as its padding and border, whatever it holds.
- A window that allows several draw passes (every DOM window) does not use GPUI's view cache.
- Table styles and props (`tableLayout`, `borderSpacing`, `colSpan`, ...) throw on an invalid value, as the
  other typed styles do.

**Deviations from the DOM.**

- **Clip and scroll follow the containing block (C2).** Content masks and element offsets are inherited
  down the paint tree, so a positioned `Div` (also `<anchored>` and a layout root such as a list row)
  records the mask and offset its children see on a `Window` stack, and `Element::escapes_to` makes
  `Drawable` prepaint and paint an absolute (`AbsoluteInAncestor`) box under the nearest recorded frame
  (`Window::with_replaced_content_mask`, which replaces instead of intersecting) and a fixed one under
  the viewport with no offset. Hitboxes take the mask at insert time, so hit testing follows; deferred
  draws carry the frame. gpuix's `IntersectionObserver`, `scrollIntoView`, key scrolling and
  `getElementsAt` use `layout::containing_block` instead of `parent`. A `position` that only a hover or
  focus style sets escapes the clips too: the retained tree records the position the element was built
  with, state styles folded in, one frame late. `IntersectionObserver` and `<input>` bounds still read the
  declared position. An effect group widens its composite mask to an escaped box
  (`Scene::widen_groups`); with a `filter`, the spread of the part that stays clipped can pass the clip.
  Residual: a `layout` (FLIP) glide is dropped for an absolute or fixed box. Zed's parent-relative
  `Position::Absolute` is unchanged.
- **`zIndex` across stacking contexts (C3).** gpuix flags the `Div`s that create a context (the window
  root, a positioned element with a `zIndex`, `fixed`, `sticky`, and every effect group: `opacity` < 1,
  `filter`, `backdropFilter`, `maskImage`, `clipPath`; also `<anchored>` content and each virtual-list
  row). zed (`Div::stacking_context`, `Element::lift_z`, `Window::lift_lists`) takes every positioned
  descendant out of its parent at prepaint (its hitboxes and its `AnyElement`, leaving an `Empty`
  placeholder) into the nearest context's list, and the context paints them by (z, tree order): negative
  z after its own background and before its in-flow children, the rest after them, then its scrollbar
  overlay. Prepaint runs in place, so the dispatch tree, the a11y tree and element state stay in tree
  order; hitboxes are re-inserted in paint order (a parent hitbox can now follow its child, so
  `dom_hit_test` and `ClipIndex` no longer assume otherwise); tab stops are rewritten into document
  order (`TabStopMap::restore_lifted_order`); `RetainedTree::elements_at` models the same contexts with
  the same predicates (`layout/stacking.rs`, also used by `build_element`), and now agrees with paint
  where it used to list tree order. Barriers where nothing lifts and the old per-parent `paint_rank`
  applies: layout roots (`prepaint_at`, `prepaint_as_root`), deferred layers, `visibility: hidden`
  subtrees. Deviations: a virtual-list row is its own context, so a row's content never rises above
  the next row; a cached view prepainted inside a context is not reused the next frame (its recorded
  hitbox and tab-stop ranges would be stale); `transform` (none in this branch) creates no context; an element that is not a `Div`, `Img`, `Svg`, `List`,
  `UniformList`, `Canvas` or `Surface` is never lifted; gpuix elements positioned
  only for layout (text wrapper, anchored placement boxes) are `.unlifted()`. A flex or grid item takes its
  `zIndex` without a `position`, and a non-positioned element with `opacity` < 1, a filter, a backdrop
  filter, a mask or a clip path paints in the `z-index: 0` layer in tree order and forms a context
  (`Styled::lifted`, `layout/stacking.rs`). Two residuals: `getElementsAt` and the retained-tree walk read
  declared styles, so an `opacity` or `zIndex` that only a state style or a motion value sets is not seen
  there; and the children of a custom element are never flex or grid items, so they take no `zIndex`
  without a `position`.
- A bounds or scroll read inside an input handler is measured, not drawn, and does not cover the
  accessibility and IME paths (see [patch 1](#patch-1-test-support-does-not-draw-inline-in-real-windows)).
- A `display: contents` virtual-list row lays out as an anonymous block inside its one list item, not
  spliced into the list's items (the list's item, focus-handle and index bookkeeping is per row).
- `anchor-size()` resolves on the sizes, margins and insets of a direct child of `<anchored>`. It
  starts from the trigger's last painted size, so it is right in the first frame under a trigger that
  has already painted; otherwise the child is held at opacity 0 until the trigger is measured.
- A virtual list truncates a single-frame scroll delta above the 512 px overdraw, and unmounted
  placeholder rows count as 1 px without `estimatedItemHeight`.
- A virtual list's classic scrollbar (macOS "Show scroll bars: Always") overlays its rows instead of
  taking layout space. An `overflow: auto` scroller reserves its classic gutter one frame after its
  content starts to scroll.
- A `visibility: hidden` subtree is still painted, with its primitives dropped, so a big hidden pane
  costs CPU; a "no descendant is visible" hint would skip it.
- **Tables** (README "Tables" lists them all). Collapsed borders are resolved from the declared style with the
  pointer states folded in, so a transition or `motion` on a neighbour's border is not seen until it ends,
  and a `tableLayout: "fixed"` table whose `width` is a `%` of a parent with no width draws none (the borders
  are predicted before layout). Percentage heights of rows, groups and cells are `auto`, `direction: rtl`
  does not mirror columns, and some `rowSpan` share-outs and columns past 100% differ from Chrome. Sums are
  floats where Chrome counts in 1/64 px. Copied text keeps the tab after a cell that ends in a block, where
  Chrome drops it. Table styles are not animatable, and `table-*` on a custom element is a block.
- **Atomic inlines** (README "Inline-block..."). gpui measures text to the next whole pixel, so a box as wide
  as its text is up to 1px wider than Chrome's, and fractional font metrics move `middle`, `text-top` and
  `text-bottom` by up to a pixel. An absolute descendant resolves against the atomic inline, which is also a
  stacking context. `textOverflow` and `lineClamp` ignore a paragraph with an atomic in it. A span holding
  only atomics reports its line box, a match cannot cross the box, and the decoration of a raised span that
  wraps is drawn at the row's baseline.
- **Container queries** (README "Container queries"). `style()` and `scroll-state()` are unknown (false). Only
  host boxes are containers (not an inline `<span>`, `<img>`, `<svg>`, `<input>`, a `display: contents`
  element, a `<virtual-list>` itself), and table parts ignore `containerType` as Chrome does. A block is
  evaluated where the element is built, so `elementsAt`, stacking and the paragraph planner read the style
  without it; `display` is the exception, since it decides what box a child is in its parent. No vertical
  writing modes, so `cqi` is `cqw`.
- **`:has()`** is the four relational states, not a selector. There is no `:has(.error)`, no `:has(> x)` and
  no `hasChecked` or `hasDisabled`, because gpuix has neither a selector engine nor `checked` and `disabled`
  state styles.
- **Intrinsic widths are a pre-pass, not taffy.** `width`, `minWidth`, `maxWidth` and `flexBasis` take `min-content`,
  `max-content`, `fit-content` and `fit-content(<length-percentage>)`. taffy 0.13 has no such
  keywords, so gpui keeps them in a `Style::intrinsic_widths` field (the real width slots stay `auto`)
  and `TaffyLayoutEngine::compute_layout` measures each keyword node (deepest first) as a sub-root
  with `MinContent` / `MaxContent` available width, then rewrites its taffy style to definite values
  (`fit-content` becomes `auto` with `min-width` and `max-width` set to the content sizes kept between the
  author's own limits, which is the same as clamping to the content sizes and then to the limits, `min-width`
  winning; a limit in `%` or `calc()` makes those a taffy `Calc` that holds a value between a floor and a
  ceiling, which layout resolves against the containing block, and a cyclic `%` is none while that width is
  unknown). The engine is rebuilt every frame, so a
  keyword node costs one or two extra subtree layouts per frame and nesting multiplies it. Height
  keywords are `auto`, as CSS Sizing 3 says of the block axis. As `minWidth` / `maxWidth`,
  `fit-content` is `min-content` / `max-content`. Text `min-content` wraps at the widest stretch
  between break opportunities and counts the trailing space, so it is one space wider than a
  browser's. `flexBasis` is measured like a width only along a row or row-reverse flex container's main
  axis (the parent is looked up where the pre-pass runs, since children are created first) and is
  `auto` in a column, block or grid; it wins over `width`, and its `fit-content` is `max-content`
  (the container's size is not known there; with `flexShrink: 0` it overflows where a browser clamps)
  and `fit-content(<limit>)` is the limit clamped between min and max-content for a length.

**Upstreaming.** The zed side is the larger part: scroll routing with latching, CSS scrollable overflow,
`Position::Sticky`, containing blocks, grid templates, floating-ui `Anchored`, paint ranks and
`draw_if_dirty`. It changes GPUI's scroll behavior for every user, so expect review on the
`div.rs` scroll listener. The gpuix side is breaking (`position`, `onScroll`, the wheel sign).

## Area D: paint and style

**Problem.** `overflow: hidden` clipped to a rectangle, backgrounds were a solid colour, `boxShadow`
and borders had one colour and radius, `opacity` applied per primitive, nested text was a block, CSS
colour syntaxes other than a few were rejected, `<svg>` was a monochrome tinted mask, `<img>` had no
`onLoad`/`onError`, and there was no group state (`groupHover`) because there is no stylesheet to
write `.group:hover .child`.

**Design.**

- **Clipping and borders.** `overflow` clips to the padding box with rounded inner radii (zed rounded
  content masks). Per-side border colours, widths and line styles (`solid`, `dashed`, `dotted`,
  `double`, `groove`, `ridge`, `inset`, `outset`), `border*` shorthands, per-corner radii on every
  painted part. Radii are elliptical: `a / b`, a two-value corner longhand, a percentage per axis
  (quads, shadows, content masks, effect-group clips and hit testing). A corner's inner edge is the
  outer ellipse less the adjacent border widths per axis, as in CSS. A side no declaration gives a line
  style is `none`, as in CSS: a `none` or `hidden` side has width 0, so a width alone draws nothing, and
  a visible style with no width is `medium` (3px). `borderColor` is the shorthand of four side colours,
  which `motion` and CSS transitions animate separately.
- **Backgrounds and shadows.** Linear, radial and conic gradients (plus the repeating forms) with any
  number of stops, colour hints and `in oklab`; multiple layers; `backgroundImage`, `Size`,
  `Position`, `Repeat`, `Origin`, `Clip` and the `background` shorthand; `boxShadow` as a CSS string
  with multiple layers and `inset`; `backgroundClip: "text"`. CSS strings are parsed in
  `packages/native/src/css/`.
- **Effect groups** (zed `effects.rs`, `gpui_apple`): `filter`, `backdropFilter`, `maskImage`
  (gradients and images, `maskMode`, `maskComposite`), `clipPath` (`inset()`, `rect()`, `xywh()`, `circle()`, `ellipse()`, `polygon()`, `path()`), and `opacity`
  that composites the element as a group. A subtree that paints one primitive keeps the cheap
  per-primitive path; `opacity: 0` paints nothing. A mask is a list of `BackgroundLayer`s, each with a
  `mask_mode` and a `mask_composite`; zed draws each layer into its own texture and runs a combine pass
  only when some layer is not alpha + add.
- **Inline text.** `<span>` and a nested `<text>` are inline runs with their own colour, background,
  weight, family, `fontStyle`, decoration and font features, in one paragraph (`src/text/inline.rs`).
  `fontVariantNumeric` and `fontFeatureSettings`. A span's `fontSize` and `lineHeight` size its own run,
  and its `em` is its own font size. A row is as tall as its CSS line box, the line's strut plus every
  run's size and line height; text with no sized run keeps the rows it always had. Paint, decorations,
  hit testing, selection and highlight rows, and a span's content area use each row's own baseline and
  height. A run with listeners or `hover`/`active`/`groupHover`/
  `groupActive` styles stays inline: the pointer is hit-tested by glyph (`text/inline_hit.rs`, which
  keeps each painted paragraph's `TextLayout`), `events/inline_target.rs` puts the inline boxes under
  the glyph in front of the container's hit chain, so the span is the event target and the dispatcher
  needs no change, and `run_styles` folds the pointer state into the run. The hit registry and the
  painted text, start regions, highlights and match ordinals are keyed by window (`WindowId`), so a second
  window's frame reset keeps the first one's entries.
  Selection and find follow DOCUMENT order while hit testing follows paint order. zed lifts a positioned
  element, so its text registers after the siblings that follow it, and the registry is in that order.
  Each run carries a `DocPos` (child indices through the retained tree, then `sub`; flex `order` and
  `display: contents` do not move it, as in the DOM), and `resolve_drag` sorts by it, so the range, the
  wash and the copied text read in the tree's order. Find numbers matches in the same order: runs
  announce their matches in prepaint and the first wash of a frame numbers them all. The press and the
  drag head still land on the topmost text under the pointer. (Demo divergence 29.) The text of an atomic
  inline (`inline-block`, `inline-flex`, `inline-grid`) sorts at the byte of the box's placeholder in the
  paragraph around it: the paragraph is cut there for the drag, and copy puts no line break between the
  text of the line and the box's inline content.
  A span has bounds: its border box per line fragment (`text/inline_bounds.rs`, recorded at paint),
  `getBoundingClientRect` and the observers read the union, and a span that stops painting is gone.
  `cursor` is a hitbox per fragment (`inline_cursor.rs`), `tabIndex` a zero-size anchor div under the
  span's host id that tracks its focus handle (`inline_focus.rs`), and `focusVisible` / `focusWithin`
  fold into the run and ring the fragments with `outline*` (`inline_decoration.rs`). Margin, border and
  padding along the line are a `TextInset` side channel in gpui (`StyledText::with_insets`; later glyphs
  shift like after a tab, wrapping and hit mapping count it), and `backgroundColor`, borders, radii and
  vertical padding paint per fragment with `box-decoration-break: slice` semantics (`inline_decoration.rs`,
  `inline_insets.rs`). `motion.span` (React and Solid) animates a span's colours and `opacity` and
  nothing else: it is an inline box (`Scope::InlineBox`), so a layout key, a `while*` prop or a motion
  value in its `style` throws, and `variants` work on `motion.span` only. The `opacity` of an inline box
  is not a compositing group. A span's `color`, `backgroundColor`, `textDecorationColor`,
  `outlineColor` and border side colours also run CSS transitions and keyframe animations; its other
  properties apply at once.
- **White space.** `whiteSpace` is inherited and processed as CSS Text 3 §4: `normal`, `nowrap`, `pre`,
  `pre-wrap`, `pre-line` and `break-spaces`, collapsing across spans and stripping at paragraph and
  line edges (`src/text/white_space.rs`, applied by the paragraph builder, so paint, search and copy
  share the text; adjacent text leaves of an `anchored` or `virtual-list` parent go through the same
  builder). A tab stays a tab: gpui lays it out on tab stops (`TextStyle::tab_size`, from `tabSize`).
  Spaces at a soft wrap hang (`hang_trailing_spaces`), `break-spaces` breaks after every space
  (`break_spaces`), and `wordBreak` / `overflowWrap` map to `WordBreak` / `OverflowWrap`. All four are
  `TextStyle` fields carried by `TextLayoutOptions` into the layout cache key and
  `LineLayout::compute_wrap_boundaries`. gpui's defaults leave Zed's text as it was (no tab stops, spaces
  stay on the line, a word that does not fit is broken), and gpuix sets the CSS ones on the root view.
- **Colours.** `currentColor`, `light-dark()` (through an inherited `colorScheme`), `color-mix()`,
  `color()` in all CSS colour spaces, and named colours. The default text colour follows the scheme.
- **`<svg>` and `<img>`.** SVG rasterizes with resvg at device scale in its own colours, and
  `currentColor` resolves to the element's `color`. `<img>` gets `onLoad` (with
  `naturalWidth`/`naturalHeight`), `onError`, and `file://` and `blob:` URLs.
- **Group state.** `group` with `groupHover`, `groupActive` and `groupFocusWithin`. The group's
  `focused_within` is read from B's focus-within path (`FocusStyles::within`), the source `focusWithin` uses.
- **`:has()` is relational state, not a selector.** `hasHover`, `hasActive`, `hasFocus` and `hasFocusVisible`
  are nested style objects like `hover`, true while a strict descendant (logical tree, as the hover chain and
  `focusWithin` use) is hovered, pressed, focused or `:focus-visible`. Without a stylesheet there is no
  selector engine to build `:has(.error)` on, and the four states are the ones gpuix already tracks: each is
  read from the chain it already keeps (the hover chain and the active chain minus their target, the
  focus-within set minus the focused element, the focus-visible element's strict ancestors), so it costs O(1)
  per element and no tree walk. The restyle gating that decides which elements a hover or focus change
  restyles counts the `has*` keys. Transitions, animations, `<UniversalStyle>`, `motion`, inert gating and
  inline spans treat them like their plain counterparts.
- **Universal style.** `setUniversalStyle(style | null)` (and `<UniversalStyle>` in the React and Solid
  adapters) is a fork addition equivalent to `* { ... }`: one style applied under every element, at the
  lowest precedence per property, to elements mounted later as well. It is not inherited, text nodes are
  not matched, and it is not a stylesheet (no selectors). Edge: when the layer is the first to add
  `transition` to an element that has no style of its own, a colour set in that same change applies at
  once instead of transitioning.

Cross-area decisions that touch D:

- **Group state comes from A's `PointerState`.** `groupHover`/`groupActive` apply when the nearest
  ancestor with that `group` name is hovered or active. A group applies to descendants only, and the
  nearest same name wins. State layers fold in the order base, `groupHover`, `hasHover`, `hover`,
  `groupActive`, `hasActive`, `active`, `groupFocusWithin`, `hasFocus`; `focusWithin`, `hasFocusVisible` and
  `focusVisible` are applied to the element after that fold, so focus layers beat the pointer ones,
  `groupFocusWithin` included. An element's own state beats what its descendants cause, and that beats a group. GPUI's `.group()` and `.group_hover()` are no longer called on host elements. There is no
  group state inside an inert subtree.
- **A state style's shorthand resets the longhands it covers** (`background`, `overscrollBehavior`,
  `gridArea`/`gridColumn`/`gridRow`, `border*`, `borderColor`, `mask`), so a hover `background`
  replaces every layer longhand, as in CSS. `overlay_fields!` in `events/state_styles.rs` is the list
  of every `StyleDesc` field that takes part in state styles.
- **`clipPath` clips hit testing** (zed `Hitbox.clip`) and `borderRadius` cuts the hit area of its box
  (zed `Hitbox.corner_radii`); see A.
- **A hovered `color` reaches descendants' `currentColor`.**
- **Motion colours resolve through `ColorContext`.** Targets and keyframes (motion, CSS transitions,
  CSS animation keyframes, `<svg>` `fill`/`stroke`) take `currentColor`, `light-dark()`, `color-mix()`
  and `color()`, resolved in the element's own context (`currentColor` for `color` itself is the
  parent's); a new `color`, `colorScheme` or window appearance resolves them again and the track runs
  to the new colour. A static `<svg>` `fill`/`stroke` that uses those functions is written as `rgba()`
  in the svg's context. An animated `<svg>` shape changes hue through `SvgPaint::image`, which keeps
  one image and drops the previous one from the atlas, so an `<svg>` holds one atlas entry, not one per
  frame. Deviations: a motion value is not tied to an element, so its colours resolve as on a root with
  `colorScheme: "light dark"` and keep what they resolved to (they are not re-resolved on an appearance
  change), and an ancestor's `hover`/`active` colour is not in a descendant's motion colour context.

**Breaking changes.**

- Sibling text nodes in a block `div` flow on one line instead of stacking, and `getPaintedText()`
  returns the whole paragraph. A nested `<text>` with listeners or state styles stays a block.
- `<svg>` no longer gets the `#e2e2e2` tint and shapes with no fill paint black. Icons that used
  `#000` and relied on `color` to tint them need `currentColor`. Its content clips to its border
  radius.
- `overflow` clips to the padding box even when the border is transparent.
- Hover and active states that set `color` now reach `currentColor` in descendants.
- GPUI `Style.border_color` became `border_colors` and `Style.clip_path` became `ClipPathValue`.
- An appearance change always rebuilds the view.
- An undeclared border style is `none`, as in CSS: a `borderWidth` or `borderColor` alone draws
  nothing. Add `borderStyle: "solid"` (or a style in the `border` shorthand).
- A text node has no box, so `Locator.bounds()` on a raw text node falls back to the box of the
  element that holds it (the whole paragraph, when several text nodes share one).
- Selection, the copied text, the highlighted range and find's match numbers follow document order, not
  paint order. A `position: relative` (lifted) element no longer reorders them. Hit testing still follows
  paint order.

**Deviations from the DOM.**

- **Effects exist on macOS (Metal) only.** On wgpu and DirectX the groups draw flat and only opacity
  applies (`Platform::supports_effect_groups()` is false there). It is a port, not a limit:
  the group pass (texture pool, resample, colour matrix, blur, drop shadow, mask, clip-path and
  composite) is about 660 lines of Rust and 235 of Metal in `gpui_apple`. wgpu needs the same passes
  in WGSL (WebGL2 takes its own shader variant), DirectX 11 in HLSL, roughly a thousand lines of Rust
  and a few hundred of shader per backend, written on that platform.
- **The Quad shaders are unverified off macOS, and that is more than effects.** The fork rewrote
  them on every backend for rounded content masks, per-side border colours and styles, elliptical
  corners, gradients and background tiles: about +300 lines of WGSL (plus 76 for WebGL and 19 for the
  subpixel shader) and +300 of HLSL.
  The HLSL is compiled by `gpui_windows/build.rs` (fxc) only on a Windows host and has never been
  built; the WGSL was validated with naga and never rendered. Gradients, per-side borders and
  their styles, elliptical corners and rounded `overflow` clipping are as unproven on Windows and
  Linux as effects are. Checking them needs a Windows host (fxc build, then GPUI's render tests) and a wgpu run.
- **Masks take gradients and `url()` images.** Images (a path, `file://`, `data:`, `http(s):`, `blob:`,
  loaded like `url()` backgrounds) are read by `maskMode` (`alpha`, `luminance`, `match-source`) and combined by
  `maskComposite` (`add`, `subtract`, `intersect`, `exclude`), as longhands or in the `mask` shorthand. An
  unloaded or failed image is a transparent layer, so the element is hidden whole, as in CSS. Not built:
  `image-set()` (skipped), `mask-border`, and the legacy `-webkit-mask-composite` keywords; `no-clip` and
  `margin-box` invalidate a value, `mask` is not animatable, and an image mask's tiles are culled to the
  content mask at group start.
- **`clip-path` has no `url()`, and `path()` is flattened.** `url()` and `filter: url(#id)` reference
  an SVG `<clipPath>` or `<filter>`, and gpuix has no SVG DOM, so they stay out of scope. `path()`
  becomes polygon contours, one per subpath, with curves flattened to 0.25px and a limit of 4096
  vertices for the whole path (coarser, up to 4px, before it is invalid). A `mask` or `clip-path` with
  an unsupported value is invalid, as in CSS, instead of falling back.
- **Limits of elliptical corners.** A dashed or dotted border on an elliptical corner is laid out
  along the arc of the corner's mean radius. A radius in `motion` is a circular number of px: a
  percentage or elliptical corner has no start value, so the animation starts at 0, and a written
  value sets a circular px corner. A radius string is a CSS length like any other (`px`, the absolute
  units, `%`, `em`, `rem`, the viewport units and `calc()`). An invalid or negative
  radius throws when the style is committed, as an invalid width does, where `boxShadow` and
  `clipPath` ignore an invalid string.
- **`url()` background images.** `url()` in `background` / `backgroundImage` paints, mixed with
  gradients (position, size, repeat, origin, clip, `backgroundClip: text`). It loads as an `<img>` `src` does
  and paints nothing until loaded, or if the load fails (logged once). At most 128 images are kept and at
  most 4096 tiles per layer; animated images draw frame 0; `blob:` URLs are fetched in JS and handed over
  with `setStyleImage` / `failStyleImage`; a hover image starts loading when the hover first applies; an
  inline span box paints `backgroundColor` only.
- **White space.** In `normal` and `pre-line` a hanging space at a soft wrap stays in the text, so
  selection, copy and search see it where CSS removes it. `textOverflow` with `lineClamp` or a width cuts where the text
  lays out (`WindowTextSystem::truncate_end`, `text_system/line_truncation.rs`): it keeps the layout's
  own rows, so tabs, hanging spaces, `break-spaces`, `wordBreak` and `overflowWrap` apply. `ellipsis-start`
  and the middle cut still walk flat character advances, each measured at its run's size, and the ellipsis width has no kerning. `<input>` and `<textarea>` shape text without a style and have no tab stops, and
  keep breaking words. `tabSize`, `wordBreak` and `overflowWrap` on a `<span>` do nothing, because a
  paragraph is one string with one layout. See README "White space".
- **Text decoration.** A `TextRun` carries one underline, strikethrough and overline plus extra lines of
  each kind (`extra_decorations`, `TextStyle::refine_nested`), so nested boxes accumulate as CSS draws
  them. Dotted and dashed marks keep Chrome's proportions and do not stretch to end on a whole mark, and
  an outer dotted or dashed line restarts its pattern where a nested box changes the run.
  `text-decoration-thickness` takes `em`, `rem`, the viewport units, `calc()` and a `%` of 1em, resolved
  at the element that declares it.
- **CSS lengths are linear expressions.** Every length field (sizes, margins, insets, padding, gap,
  `flexBasis`, border and outline widths, `fontSize`, `lineHeight`, shadows, the lengths inside
  filters, gradients, masks, clip paths and grid tracks) takes a number or a string with `px`, the
  absolute units, `%`, `em`, `rem`, `vw`, `vh`, `vmin`, `vmax` or `calc()` (`+` and `-` between any
  of them, `*` and `/` by numbers). It is parsed once into `LinearLength` (`css/length.rs`) and resolved
  at apply time with a `LengthContext` carried in `Inherited` next to `ColorContext`: the font size is
  cascaded during the build (`em` of `fontSize` is the parent's, `rem` the root element's, initial
  16px) and the viewport is the window size. A length plus a percentage reaches layout as a taffy
  calc value (vendored taffy, `DefiniteLength::Calc`) or, for backgrounds and gradient stops, as
  `LengthPercentage::Calc` / `StopPosition::Calc`. Limits: `calc()`, `em`, `rem` and the viewport units are not
  animatable in `animate` or keyframes (an error names the property; a transition to or from one
  jumps), and `%` is only where motion has a basis for it (sizes, margins, insets, padding and
  `flexBasis`; the other lengths take a number of px); a focus-visible or focus-within `fontSize` changes only the element's own text, not its
  children's `em`, as colour does; two nested `display: contents` ancestors with `em` in `fontSize` do
  not compound; a `calc()` that comes out negative clamps to 0 for `fontSize` and track sizes;
  `fit-content(calc(length + %))` as a grid track is rejected (taffy has no calc `fit-content`); a
  repeating gradient with a length-plus-percentage stop skips the zero-length averaging special case.
- **`lineHeight` is CSS `line-height`.** A number is a multiplier of the element's font size (it was
  px), a string is a length, `"normal"` is gpui's default (`phi`).
- **`<svg>` rasterises off the frame by size.** A document up to 4 KB drawn up to 128x128 device px
  rasterises in paint (`svg_paint.rs`: `INLINE_BYTES`, `INLINE_PIXELS`); anything bigger rasterises on the
  background executor, one job at a time per `<svg>`, drawing the previous raster stretched until the new
  one lands. A running resvg job cannot be cancelled, so a changed key waits for it, and a big `<svg>`
  that has not rasterised yet draws nothing on its first frames.
- **`<svg>` children** serialise the shapes, what they refer to by id (gradients, `clipPath`, `mask`,
  `use`, `symbol`, `pattern`, `marker`, `filter` with its `fe*` primitives), `image` and
  `text`/`tspan`/`textPath`; the vocabulary is `custom_elements/svg_names.rs`. `<text>` inside an
  `<svg>` is told from the host `text` by position. `<image>` takes `data:` URLs, `file://` URLs and
  paths, not `blob:` or `http(s):`, because the svg rasterises without waiting for a load.
- **`TextRun` has its own `font_size` and `line_height`** (`Option`s; `None` is the line's) for per-run
  `fontSize` in inline text. Sizes go through CoreText only: the macOS shaper builds a font per run, and
  the other shapers still shape every run at the line size. `FontRun` and `ShapedRun` carry the size,
  `TextLayout` gains `content_extent` and `row_for_index`, wrapped rows take the height of their CSS line
  box, and paint, hit testing and start and middle truncation follow each row and run. It changes the
  text path Zed's editor uses (`text_system`, `line_layout`, `line_wrapper`, `line.rs`,
  `elements/text.rs`), so it is the least upstreamable change here.
- **Inline boxes.** A span that contains a block child is treated as a block. Padding, border and
  margin along the line, and vertical padding and border, take any CSS length
  (resolved through the span's own `LengthContext`); `auto` counts as 0. Only `backgroundColor`,
  borders, radii and `opacity` paint on an inline box: no gradients, `boxShadow`, and no
  `box-decoration-break: clone`. A box's content area is that of the span's own font. A span that starts on a `\n` or is cut by
  `textOverflow` / `lineClamp` loses its inline space. A focused span does not dim (`keyboardFocusDim`
  is not read on it) and its accessibility node has no size. A `groupHover` on a span follows a group
  declared by the container or above, not one declared by another span. A hit is decided by the glyph
  cell under the pointer, so vertical padding that reaches into a neighbouring line is on the span
  only where that cell is the span's. A hanging space at a soft wrap is part of the fragment's rect.
- **Font features need a font that has them**, as in a browser. The macOS system font has `smcp`,
  `frac`, `tnum` and `pnum` but no `zero` or `onum`, so those paint like the default there; Avenir
  Next and Baskerville have `smcp` and `onum`. Windows and Linux were not tested.
- **Nested `overflow` clips are exact on macOS where they can be.** An inexact pair (a child's corner inside
  the parent's cut-away corner, or mixed elliptical radii at a shared corner) paints through an identity effect
  group under the parent's mask, one offscreen pass; hit testing keeps each clip separately. Still
  approximate: platforms without effect groups, a box inside the clip that escapes it, and a positioned box
  the clip contains (painted by its stacking context with the prepaint clip). A one-axis clip is a plain
  rectangle, as in Chromium (csswg-drafts#7434). `overflow: clip` beside `visible` cuts only the clipping
  axis (square, unbounded on the other); `hidden`, `scroll` or `auto` beside `visible` compute it to `auto`
  (CSS Overflow 3).
- **Cost.** A `Quad` is 452 bytes where upstream's is 160 (gradient data in `Background`, per-side
  border colours, styles and inner colours, and the vertical corner radii) and path vertices grew from
  34 to 56 words. The impact is upload bandwidth only: 292 bytes more per quad, so 5,000 quads add
  about 1.5 MB a frame, with no shader cliff. Tile culling and the 4096-tile cap bound image layers. Gradient (so background and mask images), filter and
  clip-path strings go through a bounded per-thread parse cache keyed by the text and, when the text
  reads `currentColor`, `light-dark()` or `color-mix()`, by the colour context, and when it reads
  `em`, `rem` or a viewport unit, by the length context (`css/cache.rs`): a
  500-element grid of three-layer gradients builds its frame in 3.9 ms instead of 6.7 ms. `boxShadow`
  is parsed when a style is deserialized, and caching it measured no gain.
- **The zed crates outside `gpui` that this touched** (`editor`, `ui`, `terminal_view`, `onboarding`,
  `keymap_editor`) were not built. `cargo check -p editor -p ui -p terminal_view` would; it matters
  only to someone building Zed from this submodule.

**Upstreaming.** The zed commits touch the scene, the Metal pipeline and the `Quad` layout, so they are
the hardest to send: they change every GPUI renderer. Moving gradients into their own primitive would
win back the `Quad` size and is the change to make first. The gpuix side is breaking mainly for `<svg>`
colour and inline text.

## Area E: motion

**Problem.** Patch 3 gave `motion` springs, but only for a handful of numeric properties. Colours did
not animate, there was no CSS `transition`, `animation` or keyframes, no per-property transitions,
no `height: "auto"` target, no motion values, no layout animation, no `popLayout`, SVG path drawing
could not animate, and an app could not read `prefers-reduced-motion`.

**Design.** The design record is `packages/native/src/motion/` and its README section; what matters
here:

- **Colours animate** in Oklab (premultiplied) with springs per channel and velocity carry-over:
  `color`, `backgroundColor`, `borderColor`, `outlineColor`, `boxShadow`, SVG `fill` and `stroke`.
  `borderColor` is the shorthand of four side colours, so a `border-color` transition fires one event
  per side.
- **CSS transitions**: `transition`, `transitionProperty`, `transitionDuration`,
  `transitionTimingFunction` and `transitionDelay` (all timing functions, including `steps()` and
  `linear()`), with transition events. They apply to plain style changes and state-style changes.
- **Per-property transitions** (`transition: { default, opacity: {...} }`) for every numeric
  `StyleDesc` property, including margin, padding, gap and per-corner radii. `height: "auto"` and
  px to `%` targets work through a zed commit that exposes a layout node's content size.
- **Keyframes** with `times`, `repeat`, `repeatType` and `repeatDelay`; CSS `animation` with
  `defineKeyframes`.
- **Motion values** in React and Solid (`useMotionValue`, `useTransform`, `useSpring`,
  `useVelocity`, imperative `animate()`), native-driven so linked layers cannot drift. Grabbing a
  value mid-flight continues from where it is.
- **Layout animations**: `layout` (FLIP), `layoutId` shared-element morph, `LayoutGroup`, and a
  `Reorder` drag-reorder primitive. `AnimatePresence` gets `mode="wait"`, `"popLayout"` and `custom`.
- **SVG**: `<svg>` takes DOM-style shape children (`<path>` and friends); `pathLength`,
  `strokeDasharray` and `strokeDashoffset` draw, and `d` morphs.
- **`matchMedia`** with change events (`prefers-reduced-motion` from the OS, `prefers-color-scheme`
  from the appearance, `hover`, `pointer`, size queries) and `MotionConfig reducedMotion="user"`.

Cross-area decisions that touch E:

- **Hover and active read A's `PointerState`.** The `StateSource` probe is gone: it sat above an
  element's hitbox and hid it from A's hit testing. The transition fold reads `PointerState` once.
- **Prop types follow C and A**: animatable properties take C's dimension margins and insets and A's
  aspect ratio type, and `popLayout` needs a positioned parent, because absolute children resolve
  against the containing block.
- **`Reorder` calls `setPointerCapture`** on `pointerdown`, and transition and animation events
  dispatch through A with a path and bubble (not cancelable); see area A.
- **Motion colours go through D's `ColorContext`**, and animated `<svg>` colours repaint through
  `SvgPaint::image`; see area D.
- **A style native rejects no longer freezes React.** An invalid animation, transition or grid track
  made `applyBatch` fail inside `resetAfterCommit`, which left react-reconciler's `isFlushingWork`
  set, so no root in the process committed a sync update again. The batch is now resent without the
  offending `setStyle`, and the error goes to the root's `onUncaughtError`.

**Breaking changes.**

- An unknown or non-animatable key in `motion` `animate`, `initial`, `exit` or `transition` throws
  (it was ignored). Invalid `transition*` and `animation*` values throw when the batch flushes.
- A cubic-bezier `ease` can overshoot (it was clamped to 0..1). Applied values still clamp per property.
- A property dropped from `animate` keeps animating to its old target, as in Framer. Unchanged
  properties keep their timeline on re-render instead of restarting.
- An `<svg>` with shape children ignores `source`/`src`, and its `width`/`height` props size the svg box.
- `strokeDasharray: ["4","2"]` in `motion` is now two keyframes.
- `layout` is a universal prop on host elements, and `animate` is optional on motion components.
- Solid's host keeps an element alive when Solid moves its node (a removed node's element is destroyed
  at flush, and inserting a node before itself is a no-op). This fixed Solid `Reorder` and
  `AnimatePresence` exits.
- Solid `AnimatePresence` keeps exiting nodes in place.

**Deviations from the DOM and CSS.**

- **`width: "auto"` as a motion target** works like `height: "auto"`, one frame late. Taffy's content
  width is not the width `auto` would give, so a node that can animate width carries
  `Style::measure_natural_width`: `compute_layout` lays the tree out once with those nodes' widths
  `auto`, records their border-box widths (`Window::layout_natural_width`), restores the styles and
  lays out again (taffy's cache redoes only the dirty chain). The flag is set only while a motion
  has a pending `width` goal containing `auto`, or a transition list covering `width` or `all`, or
  keyframes using `width`, with `interpolateSize: "allow-keywords"` for the last two; any such element
  pays the extra layout every frame. `animate` is not gated by `interpolateSize`, as for height.
  Inside the pass an inner flagged node is `auto` while an outer one is measured.
- **Not built: size morphs, scale and rotate.** Layout animations glide position and size snaps,
  because GPUI has no transform primitive. Only the SVG and glyph sprites carry a
  `TransformationMatrix`; quads, shadows, paths, underlines, images, content masks and hitboxes do
  not, so there is no CSS `transform` at all. Translation exists (`Window::with_element_offset` shifts
  paint, bounds and hitboxes), which is why `layout`, `layoutId` and `Reorder` can move a box,
  `Reorder`'s dragged item included. The cheapest route is the effect-group machinery: an affine
  matrix on `GroupEffect` applied in the composite vertex stage, a `Window::with_element_transform`,
  and the inverse matrix per hitbox in `dom_hit_test` for hit testing and bounds, then `transform`
  and `transformOrigin` styles, motion tracks, and `layout="size"` as a FLIP scale with child
  correction in gpuix. That makes transforms macOS-only until wgpu and DirectX get groups, and a
  group renders at its own resolution, so scaling up blurs. A matrix in every primitive's shader
  avoids both and costs several times as much.
- **`transition-behavior: allow-discrete`** works for `display`, `visibility` and the flex and
  alignment keywords (`flexDirection`, `flexWrap`, `alignItems`, ...), with the spec's `display` and
  `visibility` exceptions. `position`, `overflow`, `pointerEvents` and `cursor` are not
  transitionable, and a transition into or out of `display: contents` is unreliable (the box
  generation reads the declared style). An element held by an X-to-`none` display transition is
  listed in a per-frame thread-local read by `is_display_none`. There is no `@starting-style`: the
  first style an element is seen with never transitions (`motion`'s `initial` is the equivalent), so
  `none` to X shows X at once.
- **Margin and inset (`top`/`right`/`bottom`/`left`) to or from `auto` throw** in `motion`, like
  `width: "auto"`. That matches CSS, where `auto` is not interpolable, so it is not a gap.
- **`popLayout` needs a positioned parent** (`position: relative`): the leaving child becomes
  `position: absolute`, which resolves against its containing block. Framer's rule is the same.
- **Colour space.** CSS transitions and animations blend two legacy-syntax colours (hex, named,
  `rgb()`, `hsl()`) in sRGB and every other pair in Oklab, as browsers do; `currentColor`, an
  inherited colour and `color-mix()` count as modern syntax. `motion` springs and motion values stay
  in Oklab (per-channel springs with velocity carry-over).
- **`transitionProperty` needs kebab-case** (accepting camelCase is a few lines). A transition
  declared in the same `setStyle` as the value change runs: the retained tree keeps the style before
  the one that adds the declaration (`style_before_transition`). If two `setStyle` calls land before
  one render, the intermediate style is the start.
- **Cascade.** A running CSS transition beats a CSS animation on the same property. A declared change
  of a property that a running or filling CSS animation drives starts no transition (CSS Transitions
  §3). Deviation: an animation that has not produced a value yet already counts as covering the
  property, so a change then starts no transition either. A redefined `@keyframes` updates running animations
  on the next frame. An implicit 0% and 100% keyframe is the underlying value (re-read each frame);
  where the underlying value cannot be read (inherited `fontSize`) the nearest keyframe holds.
  Keyframe properties are limited to the animatable set.
- **`%` in motion.** Margins and insets resolve against the parent's content box, width for left,
  right and every margin side, height for top and bottom. CSS uses the nearest positioned ancestor's
  padding box for insets and absolute margins (needs a public `containing_block` and its padding-box
  size from zed). A `%` height under a parent with no definite height is `auto` (`motion/containing.rs`;
  grid items never count as definite, relative `top`/`bottom` and `minHeight`/`maxHeight` `%` in an
  auto parent are not zeroed). Padding resolves against the parent's width on every side, as in CSS, and
  `flexBasis` against the main size of its flex container.
- **Unset starts.** An unset `color` starts from the inherited colour (a parent's colour change does
  not start a transition on the child, where a browser would). `top`/`right`/`bottom`/`left`,
  `flexBasis`, `aspectRatio` and `lineHeight: normal` take the target at once, as CSS does for
  non-interpolable keywords. An unset `fontSize` and `fontWeight` start from the inherited computed values.
  An unset border width starts at 0, where CSS computes `medium` for a visible border style.
  `letterSpacing` is not in `StyleDesc`.
- **Reduced motion** follows framer's `positionalKeys` minus the transforms: `width`, `height`, `top`,
  `right`, `bottom` and `left`, and `layout`, `layoutId` and `Reorder` glides, jump; everything else
  animates. Deviation: a bare `animate(motionValue)` bound to one of those six keys is reduced too,
  which framer does not do. SVG geometry attributes are not reduced. The OS
  setting is read on macOS only; Windows has `SPI_GETCLIENTAREAANIMATION` and `SPI_GETHIGHCONTRAST`
  with `WM_SETTINGCHANGE`, and Linux the portal or gsettings `enable-animations`, a small read each.
- **`matchMedia`**: `hover` is fixed to `hover` and `pointer` to `fine`, because GPUI exposes no
  input-device signal; size queries re-evaluate when the window is resized (`mediaChange` from
  `observe_viewport`); there is no DOM `resize` event; a bare `matchMedia()` binds to the most recently mounted root.
- **`layoutId`**: an old holder that is still mounted is not hidden, the hand-off window is one
  render, and layout events arrive one frame late. `layout={false}` also disables `layoutId`
  (whether Framer does the same is unverified).
- **Motion values.** A function-form transform and `useVelocity` are computed in JS and run one frame
  behind. A spring follower replays missed frames in 16 ms steps, capped at 1000, before each read or
  write. Hook-created motion values are released but not destroyed on cleanup.
- **Solid `useIsPresent`** cannot see presence from descendants, and cannot: Solid disposes the scope
  of a child that leaves, with every effect the descendant made. Parity needs `AnimatePresence` to
  own each child's scope (keyed items and a render function, over 300 lines, breaks the `Show` / `For`
  usage). The test window gained `setWindowSize` (AppKit `setContentSize`) for the resize tests.

**Upstreaming.** Almost all of it is gpuix (`packages/native/src/motion/`, `js/motion-*.ts`,
`keyframes.ts`, `presence.ts`, `layout-animation.ts`, `match-media.ts`, the adapters); the zed side is
one commit that exposes layout content size. It builds on patch 3, and on C's prop types, D's colour
resolver and A's `PointerState`, so it cannot be sent alone.

## Appearance and fonts

**Appearance.** The `appearance` window option follows the macOS system appearance by default, with
`getAppearance()`, `setAppearance()` and an `appearanceChange` event. D's `light-dark()` and E's
`prefers-color-scheme` read it. The test renderer applies it too: it opens its window from the same
`WindowOptions` through `to_gpui_window_options`, and a zed commit makes `VisualTestPlatform` forward
`set_window_appearance` to the real platform (the trait default was a no-op), so a headless window follows
the override and delivers the change. Conflicts live in `packages/native/js/host.ts`, `src/renderer.rs` and
`src/test_renderer.rs`.

**Fonts.** The `fonts` window option takes a list of font files, reads them and passes them to
upstream's `registerFont` right after the renderer is created, so `fontFamily` resolves to them. It has
no Rust (`js/fonts.ts`, `js/runtime.ts`, `js/testing.ts`).

**Upstreaming.** Both are small and independent of everything above. If upstream grows its own font
option, drop the fonts commit.

## Window drag regions

**Problem.** A `titlebarTransparent` window has no titlebar, and nothing in gpuix said what moves it.
AppKit dragged such a window from a fixed strip at the top of the view and held clicks in that strip
back to tell a double click apart, so an app's own bar had to sit exactly under the strip and its
buttons paid the delay. GPUI had the parts (`Window::start_window_move`,
`window_control_area(Drag)`, the `app_owns_titlebar_drag` window option) and nothing connected them to
a page. Electron's answer is `-webkit-app-region`, which the apps this fork ports already use.

**Design.** The press is handled natively, not through a JS handler. macOS
`performWindowDragWithEvent:` needs the `NSEvent` of the mouse-down that is being dispatched, so the
move has to start inside that dispatch, and Windows only moves a window from `WM_NCHITTEST` answering
`HTCAPTION`, which no JS round trip can answer in time.

- **Electron's model, not hit testing.** Regions are layout rectangles. Every box with a value
  contributes its border box in tree order and the last box that holds the point wins: `drag` adds,
  `no-drag` cuts. Z-order, overlap, `pointerEvents`, `opacity`, `overflow` and `borderRadius` do not
  take part, so an overlay with no value over a bar does not stop it dragging. The value inherits
  (Chromium has since about 150, as `window-drag`, CSSWG #13101), so `drag`, a `no-drag` inside it and a
  `drag` inside that all work, and a child that overflows a drag box drags.
- **zed, opt-in through DOM hit-testing mode** (other GPUI users are unchanged). A hitbox can be
  registered as a client area (`.window_client_area()`, `Window::insert_window_client_hitbox`). In DOM
  mode `Window::window_control_area_at(position)` takes the last registered entry, in paint order, whose
  unclipped hitbox bounds contain the point, and a client entry answers `None`; a fullscreen window
  has no areas. Outside DOM mode it keeps the old rule (the first area whose hitbox is under the mouse).
  The platform's own hook (what Windows asks on `WM_NCHITTEST`) calls it with the point: `PlatformWindow::on_hit_test_window_control`'s callback takes the position in window logical pixels (Windows converts the message's screen `lparam` with `ScreenToClient`), because `mouse_position` lags the hit test. The cursor is the arrow in a
  Drag area with no button held, and stays the arrow for a press that began in one. `Window` now tracks
  the held buttons for that (`pressed_mouse_buttons`, private). `gpui_macos` `start_window_move`
  copies the native window and drops the state lock before calling AppKit (the call returns right
  away, but a `windowDidMove` could reach `window_did_move`, which locks the same mutex); it then
  clears the left-button bookkeeping, since Apple documents that the mouse-up of a window drag may
  not be sent.
- **gpuix.** `appRegion: "drag" | "no-drag"` is a `StyleDesc` field that inherits through
  `Inherited::descend`, and it is read only when the window is frameless (`titlebarTransparent`, in a test window
  too). Each host element with an effective value registers its host
  hitbox as a Drag area or a client area (`events/app_region.rs`, `mark`, called wherever
  `host_hitbox` is). An inline span in a paragraph is covered by `text/inline_region.rs`. `app_owns_titlebar_drag` is set from the same option, so AppKit stops dragging the
  top strip itself.
- **The press.** The capture-phase mouse-down listener in `events/listeners.rs` asks
  `window_control_area_at` before the dispatcher sees the press, and `press_outcome` (pure, per
  platform) decides. On macOS and Linux a left press (not ctrl on macOS, no other button down) is
  swallowed: no DOM event, no focus default, GPUI's propagation stopped. It starts the move, or on
  `click_count` 2 does `titlebar_double_click()` (macOS, which follows the user's "double-click a
  window's title bar" setting) or `zoom_window()` (Linux). A Linux right press opens the window
  menu. On Windows gpuix asks for no action: GPUI receives the caption's `WM_NC*BUTTONDOWN` as a mouse
  down, and a stopped one makes `DefWindowProc` skip the drag, the maximise and the system menu, so the
  press is taken for the window (`PointerState::window_presses`) and never stopped. It only calls
  `Window::prevent_default()`, which keeps GPUI's focus, the active state, the input editor's caret,
  the text selection (`text/paint.rs` returns on a prevented mouse-down, as the DOM does for
  `preventDefault`) from running; a scrollbar skips a press inside a Drag area (Electron: the region
  covers the scrollbar). `window_presses`
  also makes the release the window's, and until it comes a move is not the page's. A release that never
  arrives (the OS may not send one after a window move) cannot leave it stuck: a press of that button that reaches the page, a pointer exit
  or a move that reports no button held ends it.
- **The resync after a paint** (a layout that moved under a still pointer) is deferred until the draw
  has swapped frames, because `window_control_area_at` reads the rendered frame; a bar that appears
  under the pointer is a region from its first frame, and hover leaves once.
- **What the page sees.** No `pointerdown`, `mousedown`, `mouseup`, `click`, `dblclick`, focus change
  or `mousedownoutside` for a press in a region, as in Electron. With no button held, a pointer over a
  Drag region is outside the window as far as the dispatcher is concerned (`hides_pointer`):
  hover leaves with the usual leave events, no moves fire, and leaving the region brings hover back.
  A button that went down outside the region keeps the pointer through it. The wheel is untouched.
  Right click and ctrl-click reach the page on macOS.
- **Test renderer.** The offscreen window has no current mouse event to start a move from, so the
  actions (`move`, `titlebarDoubleClick`, `zoom`, `showMenu`) are recorded and read with
  `takeSimulatedWindowActions()`. Nothing is recorded for Windows, where the OS owns the region.

**Platform status.** macOS is built and tested offscreen only: the test renderer's recorded actions,
Rust unit tests for `press_outcome` and `hides_pointer`, and zed's `gpui` tests for
`window_control_area_at`. The real drag and the real double click were not exercised in a live
window. `performWindowDragWithEvent:` hands the drag to the WindowServer and returns right away
(Apple's `NSWindow.performDrag(with:)` docs: a mouse-up may not be sent; a probe here returned in
about 0.1 ms), so nothing runs nested inside GPUI's mouse dispatch, and starting the move there is what
Chromium, winit, tao, Qt and Zed's own title bar do. Whether the mouse-up arrives is unverified, and
the bookkeeping reset is right either way. Windows and Linux were written, not compiled.

**Breaking changes.**

- A `titlebarTransparent` window no longer drags from AppKit's top strip, and clicks there are no
  longer held back for double-click detection. An app puts `appRegion: "drag"` on its bar, and
  `"no-drag"` on the controls inside it.
- zed: `Window::start_window_move` takes `cx` (`start_window_move(cx)`), and in DOM mode
  `window_control_area_at` follows bounds and paint order, not hit testing.

**Deviations from Electron.**

- **Paint order, not Blink's pre-order.** zed registers a box when it paints, so "last wins" is paint
  order. Where gpuix paints out of tree order (positioned boxes with a `zIndex`, deferred `<anchored>`
  layers), a region follows the paint.
- **A `<virtual-list>`'s own box registers no region.** Its rows do, by inheritance, so the region
  is the rows' boxes, not the list's.
- **An inline `<span>` flattened into a paragraph has no host box, so the paragraph registers its regions**
  (`text/inline_region.rs`): one rectangle per line fragment of the span's border box, as `PassThrough`
  hitboxes registered in the paragraph's paint after the container's own region, so a span wins over its
  container and a nested span over its parent. A span registers only where its effective value (inherited,
  `display: contents` included) differs from the enclosing box's; `pointerEvents` and `inert` do not take
  part, hover is folded in as for the cursor, and a span's margin is not in its region. A span that is a
  flex or grid item still registers as a host box.
- **A double click is decided on the second mouse-down**, where Electron and AppKit decide on the
  second click.
- **Linux has no drag threshold:** the window menu or the move starts on the press, with no
  wait for movement.
- **No `system-context-menu` event.** Electron lets a page cancel the system menu on Windows and Linux;
  here the OS owns it on Windows, and on Linux a right press opens the window menu with no hook.
- **`appRegion` is not a DOM or CSS property.** It is Electron's `-webkit-app-region`, and the
  standards-track `window-drag` has a different value set; the exception to the principle above is
  taken because a web port needs it and nothing in the DOM does the job.

**Upstreaming.** The zed side is opt-in and small: client areas, `window_control_area_at`, the cursor
rule and the macOS lock fix go to `remorses/zed` as separate commits (the lock fix is a deadlock fix
that stands alone; `start_window_move(cx)` changes a public signature). The gpuix side needs A's
dispatcher and `PointerState`, so it goes with A.

## Results summary

- **Offscreen scroll step** (wheel dispatch plus `flush()` in `TestGpuixRenderer`, 2.7k-node diff):
  React 0.9 with a single scroll div 39.4 ms; Solid 0.10 with a scroll div 41 ms;
  `<virtual-list>` 6.6 ms; this fork 5.3-5.6 ms. Moving to Solid alone changed nothing, because
  GPUI rebuilt every element each frame. The fork's gain over `<virtual-list>` came from the
  on-demand bounds patch, since dropped; upstream's `d8cf51a` removes the same hidden canvases. These
  numbers predate the DOM-parity areas and were not re-measured with them.
- **Real window input cost**: patch 1 table above.
- **Frame pacing at 120 Hz**: patch 2 table above.
- **Motion**: patch 3 adds springs and velocity carry-over; it does not change frame cost.
- **A page of clipped and scrolled-out menus** (`examples/command-menus.perf.test.tsx`, run with
  `bun run test:perf:menus` in `examples/`; profiling, no budgets). A frame spends about 70% in build,
  `request_layout` and taffy and about 27% in prepaint and paint, so culling paint is not where the
  time is: a paint cull for content outside the clip measured about 1% and was not carried. Release
  build, p50 CPU per frame: 10.6 ms with every menu laid out, 4.1 ms with the far ones hidden, 2.8 ms
  with all hidden. `GlobalElementId` now hashes its id path once, at construction (zed `1f55cd8cd9`):
  10.3 -> 9.6 ms with every menu laid out, 3.9 -> 3.7 ms with the far ones hidden, 2.7 -> 2.7 ms with
  all hidden. Not done: skipping build and layout under a fully clipped fixed-size node, because the
  descendants' bounds stay readable from JS (`getBoundingClientRect`, scroll metrics, observers). A lead:
  `memmove` is about 19% of a frame, from by-value moves of `Div` in `apply_styles` and `build_element`.
- **DOM parity**: areas A to E are behavior, not speed. Two costs are known: a same-turn bounds or
  scroll read draws synchronously (C), and a `Quad` is larger (D).
- **Layout features** (tables, atomic inlines, container queries, `:has()`): tables and `:has()` cost only
  where used. A fitting atomic inline is two subtree layouts per frame. Container queries cost nothing in a
  steady frame, and a frame in which 500 containers cross a query takes 1.9 to 2.0 times a steady frame
  (`bun run test:perf:cq` in `examples/`).

## Working on the fork

### Layout

Two repos, same shape:

| | `fdarian/gpuix` | `fdarian/zed` |
|---|---|---|
| `main` / `gpuix` | `main`: mirror of `remorses/gpuix` `main`, no patches | `gpuix`: mirror of `remorses/zed` `gpuix`, no patches |
| `poc` (GitHub default) | upstream `main` plus the fork's commits | upstream `gpuix` plus the fork's commits |
| `upstream` remote | `remorses/gpuix` | `remorses/zed` |

`gpuix/zed` is a submodule; `.gitmodules` points at `https://github.com/fdarian/zed` on `poc`. The
submodule's origin is `fdarian/zed` on GitHub, a fresh worktree gets it with `git submodule update
--init zed`, and zed feature branches are pushed there. `main` in `fdarian/zed` is not part of the fork.

gpuix `poc`, bottom to top:

1. Upstream `main`.
2. Export `FrameLoopOptions`, the Solid `frameLoop` option and the frame-loop tests (patch 2).
3. Spring transitions for native motion (patch 3).
4. Follow the macOS system appearance and expose it to JS: the `appearance` window option,
   `getAppearance()` / `setAppearance()` and the `appearanceChange` event.
5. The `fonts` window option.
6. The DOM-parity commits, grouped by area in the order A, B, C, D, E. Each area's commits are
   followed by its own `Integrate <area>: …` commits, the fixes that adapt it to the areas below it.
7. The cross-area `Integrate …` commits, named by the pair or triple they reconcile (`Integrate A×B:
   …`, `Integrate A×B×C: …`, `Integrate A×C: …`, `Integrate A×D: …`, `Integrate A×E: …`, `Integrate D×E: …`). Their subjects are the changelog of the cross-area
   decisions above.
8. The docs commits: `README.md` and `AGENTS.md` are edited for the fork's behavior (events, scrolling,
   bounds and the like), so expect them to conflict on sync wherever upstream edits the same sections.
9. Window drag regions: the `appRegion` style and its registration, the press and hover rules, the
   test renderer's recorded window actions and tests, the titlebar example and the drag bars on the
   other examples, and the README and `AGENTS.md` text.
10. The layout features, on `layout-features` until they land on `poc`: `:has()` (`hasHover` and the other
   relational states), selection and find in document order, container queries, atomic inlines, tables
   (with border-collapse and anonymous tables), text baselines, and the `Integrate <feature> × <feature>:
   …` commits that reconcile each pair (tables × :has(), tables × container queries, inline × selection
   order, and so on). Their README and `AGENTS.md` text is in the same commits, and the `FORK.md` text is
   `fixup!` commits of the fork commit.
11. The fork commit: `.gitmodules`, the submodule bump to zed `poc`, `FORK.md`, `scripts/sync-upstream.sh`,
   the README notice and the `CLAUDE.md` pointer. It must stay last; the sync script folds a new
   zed tip into it.

**Only the fork commit touches the `zed` gitlink.** A feature commit never carries a submodule bump:
if it did, every sync where upstream moves the pin would conflict on it, and the commit would not be
reproducible from the area's own history. The cost is that a commit in the middle of the stack builds
only against the zed commit its area needs. Check out `zed/` at the tip of that area's zed commits (see
below) before building a mid-stack state, and never `git add zed` or `git commit -a` while it differs
from the fork commit's pin.

zed `poc`, on top of upstream `gpuix`:

1. The present log (patch 2) and the `flush_effects` guard (patch 1).
2. The DOM-parity zed commits, by area, in the same order:
   - A: DOM hit testing mode, `CursorStyle::None` (hidden per window in `gpui_macos`), pointer lock,
     `active_drag_value`, deferred hitbox root, and the drag operation of dragged-in files
     (`Window::set_drag_operation`).
   - B: a11y relations by author id, picker accept filters, key auto-repeat, `Entity::is_updating`,
     the a11y debug dump reporting a descendant of a hidden node as hidden, the native key and
     `is_composing` on key events, the observe-only `ImeKeyDown`, and `Window` driving the input
     handler like an input method.
   - C: `draw_if_dirty`, CSS scrollable overflow and wheel routing (one wheel event scrolls one
     scroller; a scroller's range counts all descendants, never anchored or deferred layers),
     programmatic-only scrollers, `prepaint_overlay`, `Position::Sticky`, containing blocks, `<img>` sizing, grid templates,
     floating-ui `Anchored`, paint ranks, list visible range, inherited `visibility`
     (`Window::with_visibility`, hidden elements keep their layout listeners), `Window::layout_insets`
     (the padding and border a node was laid out with) and a scroller's `scrollbar_width`, which takes
     room from its scrollport.
   - D: rounded content masks, per-side border colours, gradient layers, effect groups (`gpui` and
     `gpui_apple`).
   - Layout values (after E): vendored taffy 0.13.0 in `third_party/taffy` (`[patch.crates-io]` in
     `zed/Cargo.toml` and `packages/native/Cargo.toml`; patches: `LinearCalc` evaluated by
     `resolve_calc_value`, a block container's percentage padding and border resolve against its
     parent), `DefiniteLength::Calc`, `LengthPercentage::Calc`, `StopPosition::Calc`,
     `GridTrackSize::Calc`, `Window::layout_padding` / `layout_border`, intrinsic width keywords,
     `measure_natural_width`, text min-content in `LineWrapper`.
   - E: layout content size, parent content box and `on_painted_layout`.
3. The integration commits: a pass-through hitbox never takes the wheel, `preventDefault` in the
   capture phase cancels a wheel's scroll, and a clip-path cuts the hit area of its element and its
   subtree, and a box's `borderRadius` cuts its own hit area.
4. Window drag regions: client areas and `window_control_area_at`, the cursor rule, the held-button
   tracking, the `gpui_macos` `start_window_move` lock fix, and `start_window_move(cx)`.
5. Test platform appearance: `VisualTestPlatform` forwards `set_window_appearance` to the real
   platform, so the test renderer's window follows the `appearance` option.
6. The layout features (zed `layout-features` until they land on `poc`):
   - Container queries: `Window::request_redraw_before_present` and `set_max_draw_passes` (the pass loop in
     `Window::draw`, `Div::on_prepainted_layout`), and size containment (`contain_inline_size`,
     `contain_block_size` in taffy and gpui, plus the formatting context of a contained box).
   - Atomic inlines: `text_system/inline.rs` (U+FFFC placeholders, `vertical-align` on the line box),
     `line_layout.rs`, `elements/atomic_inline.rs` (`StyledText::with_atomic_inlines`, the private layout
     engine), and the platform shapers (`gpui_macos`, the test text system).
   - Tables: the taffy table module (`third_party/taffy/src/compute/table/`, `style/table.rs`),
     `Display::Table`, `TableRole` and the table fields on gpui `Style`, the table painting hook in `div.rs`,
     `position: relative` on table parts, and collapsed borders (`collapsed_borders.rs`).
   - Text baselines: taffy `MeasuredLeaf` and `request_measured_layout`, `TextLayout`'s first baseline,
     the two grid baseline fixes.

### Syncing with upstream

```bash
scripts/sync-upstream.sh             # rebase both repos and push
scripts/sync-upstream.sh --no-push   # rebase only, to inspect first
```

It requires clean working trees and then:

1. Fetches `upstream` in gpuix and in `zed/`.
2. Rebases zed `poc` onto upstream zed's `gpuix` branch. The target is its tip, not the commit
   upstream gpuix pins, which may lag it; the test suites check that the fork builds against the tip.
3. Rebases gpuix `poc` onto `upstream/main`. If the only conflict is the `zed` pointer (upstream
   moved the pin and the fork commit moves it too), it points `zed` at the rebased zed tip and
   continues.
4. Folds a changed zed tip into the fork commit with `git commit --amend`.
5. Pushes upstream gpuix's `main` to gpuix `main` and upstream zed's `gpuix` to zed `gpuix`
   (fast-forward only), then `poc` in zed, then in gpuix, with `--force-with-lease`.

The rebase is linear: a plain `git rebase` drops merge commits and replays everything, so `poc` has
no merges. The stack is long, so a sync after upstream touches `renderer.rs` can stop several times.

### Resolving conflicts

The script stops on any conflict other than the `zed` pointer and prints the files. Resolve, run
`git rebase --continue`, and run the script again. Four files collect conflicts from every area:
`packages/native/src/renderer.rs` (wiring for each area's JS bindings, written three times: macOS,
wasm and the Windows/Linux command path), `packages/native/js/host.ts`, `README.md`, and
`AGENTS.md`. The generated `index.d.ts`, `index.js` and `index.cjs` in `packages/native` are never
resolved by hand: take upstream's and regenerate with `bun run build` in `packages/native`.

- **Present log (zed).** `GPUIX_PRESENT_LOG` is a debug log in `gpui_apple`'s `metal_renderer.rs`. If
  upstream restructures how drawables are presented, re-add the log around the present call.
- **`flush_effects` guard (zed).** If `GpuiMode` or the inline draw block moved, keep the draw
  under `cfg(any(test, feature = "test-support", feature = "bench"))` and add the
  `self.mode.is_test()` check. If upstream gated this some other way, drop the commit.
- **Pump (gpuix).** Upstream owns `startFrameLoop`, `setFrameWakeCallback` and the zed frame hook;
  take its side of `packages/native/js/runtime.ts` and `src/renderer.rs`. Keep the exported
  `FrameLoopOptions`, `windowClosed` (it releases the sync key handler when the last window closes)
  and the Solid `frameLoop` option on top of it, and keep `frame-loop.test.ts` passing against
  upstream's `setFrameWakeCallback` return value.
- **Springs (gpuix).** Conflicts are in `src/motion/`, `js/host.ts` and the React `motionKey`.
  `MotionState::sample` returns position, velocity and `active` together; keep `sync` taking the
  sampled velocity, not just the visible style.
- **Appearance and fonts (gpuix).** Appearance conflicts are in `js/host.ts` (the `NativeRenderer`
  method pick-list must keep `setAppearance` and `getAppearance`), `src/renderer.rs` and
  `src/test_renderer.rs`. The fonts option lives in `js/fonts.ts`, `js/runtime.ts` (`init` wrapper)
  and `js/testing.ts`; it has no Rust. If upstream adds its own font option, drop the commit.
- **A: pointer events.**
  - gpuix: `renderer.rs` (the per-element mouse listeners and `wire_standard_events` are gone;
    keep them gone, not re-added by an upstream change), `src/element_tree.rs` (`EventPayload`
    fields), `js/mutations.ts` (`EVENT_PROPS`, capture props), `js/renderer-state.ts` (B rewrote it
    too), `src/events/` and `js/events/`. Upstream edits to hover or click wiring belong in the
    dispatcher.
  - zed: `window.rs` (`Hitbox` parent tracking, `hit_test`, `dom_hit_test`, `clipped_out`),
    `div.rs` (hitbox insertion, `Style::hit_clip_path`), `HitboxBehavior::PassThrough`, and the
    cursor and pointer-lock code in `gpui_macos`. DOM mode must stay opt-in per window.
- **B: focus, keyboard, forms, a11y.** `src/focus.rs`, `src/forms.rs`, `src/accessibility/`
  (`aria.rs`, `label.rs`), `src/events/sync_dispatch.rs` and `keyboard.rs`, `js/focus.ts`, `js/forms.ts`,
  `js/events/keyboard-defaults.ts`, and `renderer-state.ts` and `EventPayload` again. In zed:
  `_accessibility.rs` and the `PathPromptOptions` / `KeystrokeEvent` changes. If upstream adds its own
  `tabIndex` or key-name handling, the DOM key names (`"Enter"`) win.
- **C: layout and scrolling.** `src/scroll.rs`, `scrollbar.rs`, `src/layout/` (`box_generation.rs`: keep
  `display: none` handling and drop the text-node branch if D's inline builder owns it),
  `custom_elements/anchored.rs`, and `style.rs` (`DimensionValue` margins and insets). In zed:
  `scroll_routing.rs`, the scroll listener in `div.rs`, `taffy/containing_block.rs`, `grid_template.rs`,
  `sticky.rs`, `elements/anchored.rs`, `list.rs`. Wheel candidates go through
  `scroll_routing::is_candidate`, which rejects `PassThrough` hitboxes: keep that.
- **D: paint and style.** `src/css/`, `src/color.rs`, `src/text/inline.rs` and the `<svg>` code under
  `custom_elements/` (`svg_paint.rs`), plus `style.rs`. **`overlay_fields!` in
  `src/events/state_styles.rs` must list every `StyleDesc` field**; it is a struct literal, so a field
  missing from it fails to compile, and every area that adds a style field (C, D, E, drag regions) touches it. In zed:
  `scene.rs`, `effects.rs`, `style.rs` (`border_colors`, `ClipPathValue`), and the `gpui_apple`
  Metal pipeline. A change to `Quad` needs the shader and the Rust struct to move together, on every
  backend, or the stride checks fail.
- **E: motion.** `src/motion/`, `js/motion-*.ts`, `keyframes.ts`, `presence.ts`, `reorder.ts`,
  `layout-animation.ts`, `match-media.ts`, the React `motionKey` and the Solid host. The zed commit
  is `on_painted_layout` and content-size exposure. Hover and active come from `PointerState`; do not
  reintroduce a probe element above a hitbox.
- **Window drag regions.**
  - gpuix: `src/events/app_region.rs`; `events/listeners.rs` (the press and release listeners:
    the region check comes before the dispatcher sees a press, and Windows must call `prevent_default` and
    return without stopping it); `renderer.rs` (`Inherited` carries `app_region` and `frameless`, and
    `to_gpui_window_options` sets `app_owns_titlebar_drag`); `events/state_styles.rs`
    (`overlay_fields!` lists `app_region`).
  - zed: `window.rs` (`window_control_area_at` and the platform hook that calls it,
    `pressed_mouse_buttons`, the cursor rule `drag_area_overrides_cursor`), `div.rs`
    (`window_client_area`, `WindowRegion`), and `gpui_macos` `start_window_move` (the state lock
    must be dropped before the call into AppKit). Every caller of `start_window_move` passes `cx`.
- **Layout features.**
  - gpuix tables: `src/layout/table.rs` (roles, anonymous cells and tables), `layout/table_grid.rs` and
    `layout/collapse.rs` (the two must keep the placement of zed's `compute/table/structure.rs`),
    `css/table.rs`; `layout/display.rs` is the one place that knows the `inline-*` values and their
    blockification. `layout/contents.rs` keeps the inherited table styles (`borderSpacing` and the rest) through `display: contents`, and `box_generation.rs` reads `display()` for `display: none`.
  - gpuix atomic inlines: `src/text/atomic.rs`, `text/inline.rs` (the planner), `text/paragraph.rs`,
    `text/paint.rs`, `css/vertical_align.rs`. The planner and the table role read `RetainedElement::display()`,
    never `style.display`, or an `@container` display switch stops working.
  - gpuix container queries and `:has()`: `src/layout/container.rs` and `css/container_query.rs`;
    `css/length.rs` (`LengthContext` carries the container boxes); `events/state_styles.rs` (`overlay_fields!`
    lists `container_type`, `container_name`, `container` and the `@container` blocks too, and the `has*`
    fold order lives in `resolve_pointer_states`); `events/listeners.rs` and `dispatcher.rs` (restyle gating
    counts `has*`). Anything a build publishes must be idempotent, since a pass can be dropped.
  - gpuix selection order: `text/selection.rs`, `text/search.rs`, `text/paint.rs` (`DocPos`,
    `SelectableText::path`).
  - zed: `taffy` vendored in `third_party/taffy` is the biggest conflict surface (`compute/table/`,
    `compute/leaf.rs`, `style/mod.rs`, `tree/traits.rs`, `tree/taffy_tree.rs`, the grid alignment files); an
    upstream taffy bump means re-applying the fork's patches there. In gpui: `window.rs` (the draw pass
    loop), `elements/div.rs` (prepainted-layout listeners, the table painting hook), `elements/text.rs`,
    `text_system/` and the platform shapers, `style.rs`, `taffy.rs`, `view.rs`.
- **`AGENTS.md`.** Take upstream's side for sections the fork did not rewrite and keep the fork's for
  events, scrolling, bounds and the other behavior listed in this file.
- **Fork commit.** `.gitmodules` conflicts mean upstream changed the submodule URL or branch; keep
  `fdarian/zed` on `poc`.

If a change lands upstream, delete it from `poc` instead of resolving.

### Building

The addon is one build with `test-support`, as upstream:

```bash
cd packages/native
bun run build         # release addon, same flags as upstream's `bun run build`
bun run build:debug   # debug addon; never use it to judge frame time
```

The release profile sets `strip = "none"`: rustc's post-link `llvm-strip` drops the linker's 4-byte
pad, so the string table offset is 4 mod 8 whenever the indirect-symbol count is odd, and macOS 27 dyld
refuses to load the addon. Finish with a release build: scratch apps link this checkout, and a leftover debug addon looks like
lag. Set `GPUIX_PRESENT_LOG=<file>` to append when each frame reaches the screen to that file. There
is no separate test addon (see the rejected two-addon design in patch 1), so `TestGpuixRenderer` is in
the same file.

Some machines need workarounds that do not belong in the fork: no `metal` compiler (enable
`gpui_macos/runtime_shaders`), or an Xcode license that has
not been accepted (point `DEVELOPER_DIR` at the Command Line Tools). The demo that uses this fork
keeps its own build script for them.

### Tests

```bash
cd packages/native && cargo test --lib          # Rust unit tests
cd packages/native && bun run test              # vitest; needs react and the wasm build for one test
cd packages/react && bun run test               # vitest, needs the addon built
cd packages/solid && bun run test               # builds, then bun test
cd examples && bun run test                     # the examples suite; *.perf.test.tsx is excluded
cd zed && cargo test -p gpui --lib --features test-support   # GPUI's own tests, where the zed commits are tested
```

`packages/native` has one test that bundles both adapters for the browser; it needs
`packages/react` built and `bun run build:web` (nightly Rust and `wasm-bindgen`), so it fails on a
machine without them. That is unrelated to the fork. The `*.perf.test.tsx` files in `examples/` run
with `bun run test:perf`.

The test renderer is shaped like a live window, not like a harness. The window opens from the live
`WindowOptions` (appearance and frameless included). Keys take Playwright's `keyboard.press` grammar and
send a `keydown` then a `keyup`; the wheel and `scrollTo` / `getScrollOffset` take the DOM sign; a move
carries the buttons held by earlier downs; and every simulate helper draws once if the window is dirty,
before and after the input, never otherwise. `TestRenderer.setScrollbarStyle("classic" | "overlay")` forces the scroll bar style (the test window
starts as overlay, whatever the host's setting). The hover resync that follows a paint
(`events/listeners.rs`) applies at most once per frame in the test renderer: a state style that moves the
element from under the pointer would otherwise flip hover once per flush, with no frame between, and never
return. `TestRenderer.dispose()` tears the window down
explicitly (`createTestRoot().unmount()` calls it), and state nobody disposed is leaked at thread exit
rather than dropped, because the drop ran after other thread-locals were gone and aborted the process.

Each area added tests next to its code; none of them need a real window, because the test renderer
(`createTestRoot`) draws offscreen. That also means the macOS-only pieces in the areas above (pointer
lock, Metal effects, the OS reduced-motion read) were checked against a simulated platform, not a
live one.
