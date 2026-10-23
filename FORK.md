# fdarian/gpuix, a proof-of-concept fork

This is a long-lived proof of concept of what [remorses/gpuix](https://github.com/remorses/gpuix)
could be if scrolling and input latency on macOS were a goal, and if its elements behaved like the web
DOM. It is not published to npm and nothing here is a request to merge. It is rebased on upstream
regularly (see [Working on the fork](#working-on-the-fork)), so commit hashes change.

The fork carries four kinds of change:

1. **Scrolling and latency patches (1 to 3).** It started as a demo that renders a large
   syntax-highlighted diff with `@gpuix/solid` and had visibly worse scrolling than Zed itself. Three
   causes turned up. Upstream has since landed the fix for one of them (hidden bounds canvases,
   [remorses/gpuix `d8cf51a`](https://github.com/remorses/gpuix/commit/d8cf51a), `track_host_bounds`),
   so the fork's own on-demand bounds patch was dropped in favor of it. The other two are patches 1
   and 2. Patch 3 is not about scrolling: it adds the spring transitions the fork's UI work needed.
2. **DOM parity, in five areas (A to E).** Porting a component library built for the web showed that
   the gaps were not about speed. Hover, focus, scrolling, clipping and animation behaved differently
   from the DOM in ways each component had to work around. Each area closes one family of those gaps.
   See [The principle](#the-principle-dom-and-css-semantics). Round 19 (#92) adds CSS transforms to areas D and E
   (`transform`, `translate`, `rotate`, `scale`, `transformOrigin`, and Motion's `x`, `y`, `scale`, `rotate`, `skew`
   keys), see [Round 19](#round-19-92-css-transforms).
3. **Four small features** that are not patches against a problem: following the macOS system
   appearance, a `fonts` window option, `appRegion` window drag regions for frameless windows, and
   `x` / `y` window placement with Electron's `screen` displays. See
   [Appearance and fonts](#appearance-and-fonts), [Window drag regions](#window-drag-regions) and
   [Window placement and displays](#window-placement-and-displays).
4. **The perf pass (#36 to #42).** Frame cost on large pages: nested columns that laid out in
   exponential time, a frame that builds and paints everything when one node animates, clipped content
   that is painted anyway, bounds reads that draw, and texts measured again every frame. It changes
   little behavior on purpose (the order of a frame's reports, fractional text sizes). Round 9 (#44 to
   #49, #57, #58) belongs to it: a hover frame no longer pays for content that did not change and cannot
   be seen, and a bounds read is in fractions of a pixel, as Chrome's is. Round 12 (#62, #63) is about
   animation: one looping animation no longer costs the page, and animations nobody can see cost it nothing.
   Round 14a (#81, #74, #75, #76) closes the pages that still drew every tick of a loop whole: an opacity run
   through 1 or 0, an `<input>`, a positioned or translucent ancestor, a motion value, an `<svg>` (also one that
   has no area to paint, or motion under `display: none`, on FF's showcase); a patched tick no longer pays for
   the motions, the primitives and the motion values of the page; and it records a root in the frame that first
   builds it.
   Round 14b (#77 to #79) is about a quiet window: an idle one wakes JS and the CPU not at all, and a window
   that is not key is not throttled. Round 14c (#93) makes input to that idle window wake JS. Round 16 (#83) makes a forced layout read cost what changed, and with #82 (scroll
   anchoring) and the rest of `content-visibility` closes the gaps to Chrome that Round 13b left. See
   [The perf pass](#the-perf-pass-cheaper-frames-36-to-42) and [Round 16](#round-16-83-82-and-the-content-visibility-leftovers).

| Change | Repo | What it does |
|---|---|---|
| Patch 1: `test-support` no longer draws inline in real windows | zed (`poc`) | Wheel events and `applyBatch`/`scrollTo` stop paying for a full synchronous draw. |
| Patch 2: display-link frame pump | upstream, plus gpuix and zed (`poc`) | GPUI is serviced by the display, not by an 8 ms JS timer. Upstream provides the pump; the fork adds the Solid `frameLoop` option, the frame-loop tests and the present log. Round 14b (#77) wakes JS only when the main run loop has work and runs the display link only on frame demand, so an idle window costs no wakes. Round 14c (#93) adds a run-loop observer on AppKit's event thread, because input does not wake the main run loop by itself. |
| Patch 3: spring transitions | gpuix | `motion` gets springs and carries velocity across a retarget. |
| A: pointer events | gpuix + zed | One DOM hit test, capture/target/bubble dispatch, pointer capture, click family, drag and drop, pointer lock. |
| B: focus, keyboard, forms, a11y, platform APIs | gpuix + zed | DOM focus order and `tabIndex`, DOM keyboard events, form controls, `aria-*`, `inert`, clipboard, file picker, `EyeDropper`. |
| C: layout, measurement, scrolling | gpuix + zed | Never-stale bounds, observers, CSS positioning, grid, tables, atomic inlines (`inline-block` and kin), container queries, the box shorthands (`padding: "0 8px"`, `flex`, `place*`), nested scrolling with scrollbars, floating-ui placement. |
| D: paint and style | gpuix + zed | Rounded clipping, gradients, filters, masks, `clipPath`, group opacity, inline text, CSS colours, full-colour `<svg>`, group and relational (`:has()`) state styles, selection in document order, outlines with a style, `textWrap` and `textBox`, `fontVariationSettings`, `WebkitFontSmoothing`, line breaks where Chrome's breaker puts them. Round 19: 2D CSS `transform`, `translate`, `rotate`, `scale` and `transformOrigin`, drawn through every primitive, hit and measured through the matrix. |
| E: motion | gpuix + zed | Colour and per-property transitions, keyframes (SVG properties included), CSS `animation`, `getAnimations()`, motion values, layout animations, `matchMedia`, calc and mixed-unit lengths. Round 19: Motion's transform keys and CSS transitions and animations of the transform properties, with patched ticks. |
| Appearance | gpuix | `appearance` window option, `getAppearance()`/`setAppearance()`, `appearanceChange`. |
| Fonts | gpuix | `fonts` window option that loads app-supplied font files. |
| Window drag regions | gpuix + zed | `appRegion: "drag" \| "no-drag"` moves a `titlebarTransparent` window, as Electron's `-webkit-app-region`. |
| Window placement and displays | gpuix + zed | `x` / `y` window options open a window on a chosen display; `screen.getAllDisplays()` / `getPrimaryDisplay()` list them, as in Electron. macOS only. |
| Perf pass (#36-42, rounds 9, 12, 14a and 16) | gpuix + zed | Linear nested-column layout, a layout engine that persists across frames, retained subtrees that a frame replays (and moves with a scroll) instead of building, culling of clipped content, bounds reads that lay out and do not draw, text in 1/64 px. Round 9: JS commits, motion writes and scrolls that no longer disable replay, `opacity: 0` that paints nothing, text that is not measured again for a colour, bounds read in fractions of a pixel, `display: contents` that passes an animated inherited value on. Round 12: a tick of paint-only animations repaints only the roots it changes in the frame shown (patch frames), animations on culled content are dormant (a clock, no build and no draw), and a replay costs per root and not per element. Round 14a: those ticks are patched under a positioned or translucent ancestor, on an absolute element, beside an `<input>`, through opacity 1 and 0, for motion values and `<svg>` of any size, and a root is recorded by the frame that builds it. Round 16: a forced read after a change costs the dirty chain (paint-only writes lay nothing out, relayout boundaries, layout records, reads that share a pass), `overflowAnchor` scroll anchoring, and `content-visibility` layout containment, accessibility nodes, Tab, find and first-frame events. |

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
lays out synchronously through `Window::draw_if_dirty`, so the read is never stale. That is
an explicit call, not the `flush_effects` path this patch guards, and it does not draw (see area C).

Inside input dispatch a draw would swap the frame the dispatch is reading: a key's dispatch path
indexes a different dispatch tree, and mouse listeners the dispatch holds out of the frame are written
back over the new one, so scrollers stop scrolling and bubble handlers see `is_hovered` false. A read
therefore never swaps frames, in or out of dispatch. `Window::draw_if_dirty` runs `Window::layout_pass`:
it builds the next frame with the same `draw_roots` as a draw, up to and including prepaint, so scroll
handle metrics and element states are current, queues what paint listeners publish and runs it once the
tree is prepainted (`Window::publish_layout`), then throws the frame away: element states are handed back
to `rendered_frame` and the hit test, pointer capture and tooltip bounds are restored. It skips paint,
focus listeners, the input handler, entity observation, text-layout frame rotation, the accessibility tree
and `needs_present`, renders every view in full instead of reusing its cached prepaint and paint, and
leaves the window dirty. It runs again only when something was invalidated since the previous pass.
A pass does not advance time: `GpuixView::render` samples every animation at the instant of the last draw
(`frame_instant`, stamped once if no draw came yet), because a browser's animation clock stands still
between frames and a forced layout never runs them. A pass that read the clock moved a spring, which fired
its `change` event, whose handler read layout again (#70).
`Window::paint_if_dirty` is the variant for a caller that needs painted output (`getPaintedText`):
a draw outside dispatch, the same pass with paint inside it. The accessibility and IME paths are not
covered by a pass: the accessibility tree and the input handler (IME caret bounds) describe the previous
frame until the next draw. Making them current would mean swapping frames.

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
one (`true` on macOS); a wake that arrives while one is still pending, or while `tick()` runs, is
dropped, and dropping the renderer clears the hook. `startFrameLoop` (`packages/native/js/runtime.ts`)
schedules `tick()` from that callback (see Frames and JS tasks below). A 100 ms fallback timer
covered the case where the display link is stopped (an occluded window). `frameMs` opts into the
timer loop, and Windows and Linux use it too.

That was the hook until Round 14b ([#77](#round-14b-77-78-79)), which replaced it because the display link
ran for as long as a window was visible: an idle window woke JS at the display rate, input waited for the
next vsync, and the fallback timer ticked an idle app besides. Now:

- **The wake is "the main run loop has work".** `gpui_macos::set_run_loop_wake_hook` starts a helper thread
  that watches the ports the main run loop waits on and calls the hook when either fires: a block on the main
  dispatch queue (which includes the display link's frame step) or a run-loop timer. An input event reaches
  that queue through an observer on AppKit's event thread (Round 14c, [#93](#round-14c-93)).
  `run_loop_pumped()` re-arms the watch, and a message still pending fires it at once. `setFrameWakeCallback`
  installs the hook through it, `tick()` clears `pending` and then calls `run_loop_pumped()` on every path,
  and clearing the callback passes `None`. See Round 14b for how the watch works.
- **The display link runs on frame demand.** `MacWindow` implements `schedule_frame` and `frame_waker`, which
  start the link if the window is visible; it stops after `IDLE_STEPS_BEFORE_STOP` = 2 steps in which
  nobody asked for a frame. A draw outside a frame request asks for one (`Window::draw` calls
  `schedule_frame`), since it leaves `needs_present` for the next request to present.
- **No fallback timer.** `startFrameLoop` arms none on the wake path (`FRAME_WAKE_FALLBACK_MS` is gone): an
  idle window arms no timer and gets no wakes. `frameMs` and Windows and Linux keep the timer loop.

**Frames and JS tasks.** A frame is one task of the JS event loop, as a rendering update is in the
HTML event loop: every task that was queued while it ran gets its turn before the next frame
starts. The wake reaches JS as a threadsafe-function call whose callback only schedules `tick()`
with `setImmediate`. The tick runs in the immediates phase, and Bun then polls I/O and runs due
timers before it takes the next wake (in Node, the check phase is followed by timers and poll).
The renderer's pending flag stays set from the wake until `tick()` returns, so wakes that arrive
during a long frame are dropped, not queued. Work that arrived meanwhile is not: the frame step that
the display link merged into the main queue during a long tick is still there when `tick()` returns and
`run_loop_pumped()` re-arms the watch, so it fires at once and the step runs in the next tick, right after
the JS tasks that were queued. Two steps can run in one pump when a step outlasts a vsync. (Before #77 the
wake was the vsync itself, and a step merged during a long tick was dropped until the next vsync; Chrome
would run `requestAnimationFrame` at the first vsync after the frame.) A frame that ends before the next
vsync takes every wake, so fast frames keep the display rate. Before #56 the callback ticked
inline and the flag cleared before the tick; Bun's `ThreadSafeFunction::on_dispatch` drains its
queue without yielding, so once a frame outlasted a vsync each wake queued the next, and timers,
promises waiting on I/O and IPC stopped for as long as an animation ran.

What the frame of a running animation costs is not the pump's, but it decides how long the task is. A frame
that built something asks for the next one of an animation that runs with `Window::request_animation_frame`,
or, when the page has units (paint-only animations it can repaint alone), with
`Window::on_next_frame(request_tick)`. The draw that answers is a tick: gpui offers it to gpuix's patcher
before building anything, which builds the units that changed and paints them again in the frame shown, so
the task costs what the units cost (0.24 ms on 30000 nodes in the test renderer, where a tick drawn whole took
85.8 ms; 0.6 to 1.0 ms in a live window), and a tick the patcher declines is drawn whole in the same draw
([#62](#62-one-loop-costs-the-page)). When everything that runs is dormant (paint-only animations on
content a frame walked culled), the continuation is a frame callback that moves their clocks, emits their
events and draws and presents nothing, because the window is not dirty
([#63](#63-content-nobody-sees-costs-the-page-nothing)). A scroll animation is always asked for as
`request_animation_frame` and drawn whole.

The fork adds to upstream's pump:

- `FrameLoopOptions` is exported from `@gpuix/native/runtime`.
- Solid's `render()` takes `frameLoop: { frameMs }` and passes it to `startFrameLoop`.
- `packages/native/js/__tests__/frame-loop.test.ts` (10) runs the loop against a fake renderer: the tick
  at start and on every wake, teardown, termination, tick errors, and timer polling. An idle wake loop
  ticks once at start and then never, and ticks once per wake and not in between.
- `frame-loop-yields.test.ts` (2) runs the real loop against a hidden window with synthetic wakes from a
  Rust thread (`simulateFrameWakes`, `setTickDelay`, test-support builds only) and checks that timers
  and file reads keep running when every frame outlasts a vsync.
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
lead over the timer is small; patch 1 is the large practical win. That table was measured with the
per-vsync wake, which Round 14b replaced. An idle one-text window at 75 Hz went from 75 wakes/s, 6.9 tick
ms/s and 1.45% CPU to 0 wakes, 0 ticks and 0.29%; the numbers are in
[Round 14b](#round-14b-77-78-79).

**Windows that are not key (#78).** gpui throttles a window that is visible but not key with
`inactive_frame_interval`, 30 fps by default. A window of this fork animates at the display rate, so
`to_gpui_window_options` sets it to `None`: 75 to 76 draws/s were measured with another app frontmost, where
gpui capped the same window at about 30 (FF saw about 26). A window AppKit reports as occluded draws nothing: its
link stops on the occlusion notification (`start_display_link` refuses a window that is not visible) and
with #77 nothing else asks it for a frame. Under a cover that was 0 draws, presents and wakes, and the
display rate again within about 1 s after the cover left. Animations keep time with the clock and catch up
when the window shows, as the animations of Chrome's hidden pages do. AppKit counts a window whose shadow is
uncovered as visible, so a cover that matches the window's top-left edge does not occlude it; Chrome on
macOS, which reads the same `occlusionState`, behaves the same.

**Trade-offs.** Input no longer waits for a vsync: it wakes JS as it arrives, once per event batch (batches
are not coalesced to the display rate). The 3.9 ms p50 at 120 Hz above is the per-vsync wake's; with the
observer of #93 an event reaches its handler 0.4 to 7.7 ms after it is posted. A tick costs an estimated 0.2 to 0.3 ms of CPU, so a stream of input costs
a tick per batch. Only macOS has a display link in GPUI's path.

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

**Trade-offs.** A spring runs on any `motion` key, the transform keys (`x`, `scale`, `rotate`, ...) included
([Round 19](#round-19-92-css-transforms)). The settle snap is at most `restDelta`
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
  box. A read in the same turn as a mutation flushes the JS queue and lays out synchronously
  (`Window::draw_if_dirty` in zed), as a browser forces layout, and paints nothing. The pass builds,
  lays out and prepaints, publishes bounds, inline span bounds and scroll metrics from
  prepaint (`Window::publish_layout`; a `List` that rolls a prepaint back takes its queued reports with
  it) and leaves the window dirty, so the display link draws and presents once however many reads came
  before it. A clean window runs no pass. The pass skips retained subtrees the last presented frame
  recorded as unchanged (or an earlier pass built, see #83 slice C and #88a) and publishes their boxes from their tapes, so a read after a write costs
  layout, prepaint and the spine's build, not a build of the page (`write-then-read` 1.6 ms against 6.7 ms
  on the 1000-element page of `draws-vs-presents.perf.test.tsx`, one move per frame). The pass samples
  time where the last draw did, so it advances no animation. A scroll offset write leaves the scroll
  metrics current (`App::notify_scroll_only`, `Window::draw_if_scroll_metrics_dirty`) and the bounds stale,
  as a browser's `scrollTop` setter changes paint properties and never layout (#70).
  `ResizeObserver` and `IntersectionObserver` follow DOM semantics. They read the boxes the pass
  published, once the frame is presented ("Observers and `onLayout` are read from the published boxes", below).
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
  `scrollTo` on a scroller with no box (`display: none` on it or above) does nothing, instant or smooth, and
  it reads as 0: the DOM ignores a scroll position set on an element without a layout box. The handle of
  such a scroller is kept (its offset is back when it shows again) but `GpuixView::scrollable_handle` and
  the `SCROLL_HANDLES` sync leave it out. poc jumped to the offset of a hidden scroller; the first
  retained build animated it, because `SMOOTH_BY_DEFAULT` outlives the frames that do not build the scroller.
- **Events.** `onScroll` fires on any offset change (wheel, drag, keys, `scrollTo`) and does not
  bubble. The wheel is a separate event, `onWheel` (see A).
- **Layout.** `position` defaults to `static`; `absolute` resolves against the nearest positioned
  ancestor (zed `taffy/containing_block.rs`); `fixed` against the window; `sticky`, which stays inside its nearest scroll container's scrollport less
  that container's padding on every side (Chrome's constraining rectangle, zed `sticky.rs`; percentage insets are
  of it), inset by its own insets and never out of its parent's content box. An absolute box is
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
  `Window::draw` then drops the pass before paint, as a layout pass is dropped, and builds again, up to
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
  before the paragraph planner and the table role read it (`settle_children`). Which element is a container
  follows CSS and was checked against Chrome 156 (`container-hosts-oracle.html`): one container answers a
  whole query (the nearest that contains every axis its size features read; `style()` alone asks the
  parent); an element that cannot be size contained (a span that is not atomic, `display: contents`, a table
  or internal table box) is still the container a query finds, with its size unknown, and a `cq` unit skips
  it. `<img>` and `<svg>` only get the containment (zed `Img` drops the natural size and ratio on the
  contained axes), `<input>` is untouched as in Chrome, and a `<virtual-list>` is a container for its rows
  (zed `List` runs `on_prepainted_layout` listeners after its rows are built, since they are built in
  prepaint). `--name` keys are custom properties (`css/custom_properties.rs`, collected by the parser in
  `css/covers.rs`, inherited through `Inherited::custom`), read only by `style()`.
- **Text has a first baseline.** A measured leaf reports the first baseline of its first row to taffy
  (`MeasuredLeaf`), so a `baseline`-aligned table cell, an `inline-table` and flex and grid items with
  `alignItems: baseline` line up by their text, as in Chrome. Two grid bugs showed up against Chrome and
  are fixed in the vendored taffy (an item with another `alignSelf` was shifted to the others' baseline;
  a grid's baseline was its first item's bottom).
  A trimmed text (`textBox`) reports its baselines from the top of its trimmed box (Round 12, #66): the
  lines start `trim_top` above that box, and counting from the lines seated a cap-trimmed label in an
  `inline-grid`, or a flex or grid row, that much too high.
- **Box shorthands keep their parts.** `padding`, `margin`, `inset` and `borderWidth` take one to four
  values (`Sides<T>`), `gap` one or two (`Gap`), `paddingBlock` / `paddingInline`, `marginBlock` /
  `marginInline` and `insetBlock` / `insetInline` one or two (`Pair<T>`, block is top and bottom, inline left and
  right), `borderBlock` / `borderInline` are `border` strings, and `flex`, `flexFlow`, `placeItems`,
  `placeContent` and `placeSelf` take the full CSS grammar. Each is its own `StyleDesc` field holding the parts it
  expanded to (`css/shorthand.rs`, `logical.rs`, `flex.rs`, `place.rs`), parsed when the style is deserialized.
  Flattening a shorthand into its longhands there would lose which ones the author wrote, and the state overlay
  needs that: a state's shorthand resets the base longhands it covers (`drop_shadowed_longhands`), a state's
  longhand changes only its own. Consumers read a side through `*_below_longhands()` or `effective_*`, which
  put the explicit longhand over the shorthand. The later key wins, as in Chrome: the deserializer notes the order of
  the keys it reads, and a shorthand drops the longhands it covers that were written before it, so the rank
  (shorthand, logical pair, longhand) gives Chrome's answer for what is left. One table (`css/covers.rs`) says what a
  shorthand covers, for that and for `drop_shadowed_longhands`. Tests assert Chrome's boxes at
  rest, in a hover state and along an inline span.

**Breaking changes.**

- `flex` has its CSS meaning (a number is `n 1 0%`); it was silently ignored. `flexFlow` and `place*` are new.
- `padding`, `margin`, `borderWidth` and `gap` take several values and `inset` exists; a multi-value string that
  was ignored now applies. `DimensionValue` serializes `auto` and a percentage as CSS (it was `null` and a bare
  fraction), and the shorthand fields of `StyleDesc` are `Sides<T>` or `Gap`, not one value, for anyone reading them
  in Rust.
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
- `<input>` bounds are the border box. A same-turn bounds or scroll read costs a synchronous layout pass.
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

- **Shorthand grammar.** A unitless number token inside a string reads as px in `margin` and `inset` (as their
  longhands do) and is rejected elsewhere. A `flexBasis` of `content` is
  rejected. The logical longhands (`marginBlockStart`, ...) do not exist, only the block and inline pairs, which assume
  a horizontal layout: the start of an inline pair is the left, and the right in an rtl element
  (`StyleDesc::mirrored_inline_pairs`, where `build_element_unpumped` and `InsetSpec::of_moving` read the
  style). A span in a paragraph takes the direction of the paragraph's container, not its own `direction`.
  `borderInline` sets both sides to one value, so it has nothing to swap. A numeric `border: 1` throws; write `"1px solid"`.
- **Clip and scroll follow the containing block (C2).** Content masks and element offsets are inherited
  down the paint tree, so a positioned `Div` (also `<anchored>` and a layout root such as a list row)
  records the mask and offset its children see on a `Window` stack, and `Element::escapes_to` makes
  `Drawable` prepaint and paint an absolute (`AbsoluteInAncestor`) box under the nearest recorded frame
  (`Window::with_replaced_content_mask`, which replaces instead of intersecting) and a fixed one under
  the viewport with no offset. Hitboxes take the mask at insert time, so hit testing follows; deferred
  draws carry the frame. gpuix's `IntersectionObserver`, `scrollIntoView`, key scrolling and
  `getElementsAt` use `layout::containing_block` instead of `parent`. A `position` that only a hover or
  focus style sets escapes the clips too: `build_element` leaves a `Resolved` record per element in
  `RetainedTree::resolved` (`resolved_style.rs`), made from the style it was built with, and every walk
  over the retained tree (`getElementsAt`, `IntersectionObserver`, `scrollIntoView`, key scrolling) reads
  it, so a `position`, `overflow`, `zIndex`, `opacity`, `visibility` or `pointerEvents` set by a state, an
  `@container` block, a transition, an animation or a motion value counts. An effect group widens its composite mask to an escaped box
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
  order (`TabStopMap::restore_lifted_order` for lifted elements, `restore_children_order` for children a
  parent paints in another order: `paint_rank`s, and `Div::document_order` for flex and grid `order`); `RetainedTree::elements_at` models the same contexts with
  the same predicates (`layout/stacking.rs`, also used by `build_element`), and now agrees with paint
  where it used to list tree order. Barriers where nothing lifts and the old per-parent `paint_rank`
  applies: layout roots (`prepaint_at`, `prepaint_as_root`), deferred layers, `visibility: hidden`
  subtrees. Deviations: a virtual-list row is its own context, so a row's content never rises above
  the next row; a cached view prepainted inside a context is not reused the next frame (its recorded
  hitbox and tab-stop ranges would be stale); `transform` creates a context like an effect group (Round 19); an element that is not a `Div`, `Img`, `Svg`, `List`,
  `UniformList`, `Canvas` or `Surface` is never lifted; gpuix elements positioned
  only for layout (text wrapper, anchored placement boxes) are `.unlifted()`. A flex or grid item takes its
  `zIndex` without a `position`, and a non-positioned element with `opacity` < 1, a filter, a backdrop
  filter, a mask or a clip path paints in the `z-index: 0` layer in tree order and forms a context
  (`Styled::lifted`, `layout/stacking.rs`). Residuals: the children of a custom element other than
  `<anchored>` are never flex or grid items (they draw themselves and have no laid-out children), so they
  take no `zIndex` without a `position`; `zIndex` cannot be transitioned or animated. An element whose
  `opacity` is animated is a stacking context for the whole run, even at 1, as in Chrome
  ([#81](#81-an-opacity-run-is-a-compositor-layer-from-its-start-to-its-end)).
- **Tab order is document order whatever `order` does.** The builder sorts the children of a flex or grid
  container by `order` (`plan_children_at`), so the sorted position is not the document position: it hands gpui
  each child's document index (`Placement`, `build_content`), and `Div::document_order` makes the tab stops
  they register follow it, with a positive `tabIndex` still first and Shift+Tab walking the same order back.
  Nested containers, `display: contents` items, anonymous cells and a change of `order` follow the document
  (oracle `tab-order-oracle`).
- A bounds or scroll read inside an input handler is measured, not drawn, and does not cover the
  accessibility and IME paths (see [patch 1](#patch-1-test-support-does-not-draw-inline-in-real-windows)).
- A `display: contents` virtual-list row (C7) lays out as an anonymous block inside its one list item, not
  spliced into the list's items (the list's item, focus-handle and index bookkeeping is per row), and the
  list measures the row as that block, its children stacked (the DOM has no such list). Its listeners fire
  (the event path holds it), `getBoundingClientRect` is a zero rect, an `IntersectionObserver` reports not
  intersecting, it is not focusable and takes no Tab stop (`needs_focus`), and the anonymous block carries its
  `role` and `aria-*` (oracle `display-contents-oracle.html`, which also records the Tab order and the
  accessibility tree). Residuals: a `display: contents` element spliced into a parent keeps no accessibility
  node (a node needs a gpui element, and taffy has no `contents`), so its `role` and `aria-*` are ignored
  there, `aria-hidden` on it hides nothing, whereas Chrome keeps both; `ResizeObserver` reports nothing for the
  0 by 0 (Chrome delivers one 0 by 0 entry at `observe()`, as for `display: none`, which the README documents).
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
- **Tables** (README "Tables" lists them all). A `tableLayout: "fixed"` table whose `width` is a `%` of a
  parent with no width draws no collapsed borders (they are predicted before layout). The
  widths shared out are floats where Chrome counts in 1/64 px. Copied text keeps the tab
  after a cell that ends in a block, where Chrome drops it. A `table-*` on a custom element is a block.
  Heights are shared as Chrome shares them, in 1/64 px (`taffy/compute/table/heights.rs`, Blink's
  `DistributeExcessBlockSizeToRows` and `DistributeTableBlockSizeToSections`: percentages of groups against the
  groups' height without the spacing around them, of a group's rows against that group, the rows' clamped to 100%
  and the groups' not, what is short shared out in proportion to it, the rest to the rows without a height). The animatable table styles:
  `borderSpacing` blends and is cut to whole device pixels where it is laid out (gpui `snap_border_spacing_to_device_pixel`: 2.5 is 2px at scale 1 and 2.5px at scale 2, 0.99 counts as 1), as Chrome's `short` does it; an
  unset `borderSpacing` and the other inherited table styles start a transition from the value they inherit
  (`motion::Inheritable`, as do an unset `pointerEvents`, `visibility`, `textUnderlineOffset` and a unitless
  `lineHeight`; a `lineHeight` that is `normal` or a length is no number, so it starts at its target, as in Chrome);
  `motion` and keyframes take none of the discrete ones. A part of a collapsed table is
  built after the table, so the table reads its borders from the last pass's record
  (`ResolvedStyles::borders`) and builds again before the frame is presented when a part was painted with
  another width or style since (`rebuild_if_borders_moved`); the first pass of a changed width costs a second
  build. A colour is not read from the record: the table paints the colours its parts are built with in the same
  frame (`settle_collapsed_borders`, #68), so a `border-color` transition costs no second pass.
- **Direction** (README "Direction"). `direction` is inherited in `Inherited::direction` and reaches taffy as
  `Style.direction`, which flex, grid and block already honoured; tables mirror their columns about the grid
  (`DetailedTableInfo::column_lines` stay in order from the left), and `layout/collapse.rs` walks the grid in
  table order and mirrors only what it hands out. `start` and `end` of `textAlign` resolve per element
  (`css::direction::Inherited`), and the text a paragraph lays out is positioned and hit by its alignment
  (`TextLayout::position_for_index`, `index_for_position`, `align_offset_at`; before that the bounds of a
  centred or right-aligned span, selection washes and hit tests were at the left). Not done: the base
  direction of a paragraph is CoreText's (the first strong character), not the element's, so mixed text is
  ordered by it, and a reordered run has approximate bounds and hit tests (gpui's glyph indexes are taken as
  increasing along a line); `direction` is not animatable; the scrollbar of an rtl scroller is not compared with Chrome.
- **Atomic inlines** (README "Inline-block..."). An absolute descendant resolves against the atomic inline, which is also a
  stacking context. `textOverflow` and `lineClamp` ignore a paragraph with an atomic in it. A span holding
  only atomics reports its line box, a match cannot cross the box, and the decoration of a raised span that
  wraps is drawn at the row's baseline.
- **Container queries** (README "Container queries"). `scroll-state()` is unknown (false). There is no `var()`
  substitution: custom properties are read only by `style()` queries, and `style(--x: inherit | unset |
  revert)` is unknown. There is no `@property`, and `""` declares nothing. `<anchored>`, `<code>`, `<markdown>`
  and `<diff>` take no `containerType`. A block is evaluated where the element is built; hit testing, stacking
  and observers read the painted style, and `display` is settled from the blocks before the paragraph planner
  reads it, and `order` the same way (`RetainedElement::container_order`, set by `container::settle` and read
  through `order()`), so a flex or grid parent sorts by the folded order in the frame that lays it out, both
  when a container shrinks past a query and when it grows back (oracle `container-order-oracle`). `display: contents` is the exception for `display`, since the parent splices a
  child in before it is built.
  Vertical writing modes are infeasible: the vendored taffy lays out horizontal-tb only
  (`third_party/taffy/src/style/alignment.rs` and the flexbox and grid code carry TODOs for the other modes)
  and gpui has no `writing-mode` or vertical text, so `cqi` is `cqw`, `cqb` is `cqh` and `inline-size` is
  `width`.
- **`:has()`** is the six relational states (`hasHover`, `hasActive`, `hasFocus`, `hasFocusVisible`,
  `hasDisabled`, `hasEnabled`), not a selector. There is no `:has(.error)` and no `:has(> x)`, because gpuix has no
  selector engine. There is no `hasChecked` (nor `checked`): no element can be checked, since every `<input type>`
  is text-like and there is no checkbox, radio or `<option>`, so the state has no source until a checkbox control
  exists.
- **Intrinsic widths are a pre-pass, not taffy.** `width`, `minWidth`, `maxWidth` and `flexBasis` take `min-content`,
  `max-content`, `fit-content` and `fit-content(<length-percentage>)`. taffy 0.13 has no such
  keywords, so gpui keeps them in a `Style::intrinsic_widths` field (the real width slots stay `auto`)
  and `TaffyLayoutEngine::compute_layout` measures each keyword node (deepest first) as a sub-root
  with `MinContent` / `MaxContent` available width, then rewrites its taffy style to definite values
  (`fit-content` becomes `auto` with `min-width` and `max-width` set to the content sizes kept between the
  author's own limits, which is the same as clamping to the content sizes and then to the limits, `min-width`
  winning; a limit in `%` or `calc()` makes those a taffy `Calc` that holds a value between a floor and a
  ceiling, which layout resolves against the containing block, and a cyclic `%` is none while that width is
  unknown). A keyword node costs one or two extra subtree layouts when it is measured, and
  nesting multiplies it; the node is measured again only when something changed (its cache is
  empty, its dirty generation moved since it was resolved, or its keywords came out differently),
  since the two rewrites and the probe layouts it makes bump the layout generation of what holds
  it, and a retained root is replayed only while its generation is the one it was recorded under
  (a root under a keyword node was built again every idle frame). A measured leaf is always
  resolved again. Height
  keywords are `auto`, as CSS Sizing 3 says of the block axis. As `minWidth` / `maxWidth`,
  `fit-content` is `min-content` / `max-content`. Text `min-content` wraps at the widest stretch
  between break opportunities and counts the trailing space, so it is one space wider than a
  browser's. `flexBasis` is measured like a width only along a row or row-reverse flex container's main
  axis (the parent is looked up where the pre-pass runs, since children are created first) and is
  `auto` in a column, block or grid; it wins over `width`, and its `fit-content` is `max-content`
  (the container's size is not known there; with `flexShrink: 0` it overflows where a browser clamps)
  and `fit-content(<limit>)` is the limit clamped between min and max-content for a length.
  In a flex column the width `fit-content` fits into is the container's content width less the item's own
  margins, whether it is aligned to a side or stretched. The vendored taffy clamped the space an item gets with
  the container's margin (`flexbox.rs`), so a column with a negative margin laid its `fit-content` item out as one
  without, and a margin on the item itself did not narrow it; `leaf.rs` also took a leaf's margin off a second
  time (oracle `fit-content-margin-oracle`).

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
  per-primitive path; `opacity: 0` paints nothing, unless an animation or transition of `opacity` is
  current ([#81](#81-an-opacity-run-is-a-compositor-layer-from-its-start-to-its-end)). A mask is a list of `BackgroundLayer`s, each with a
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
  `inline_insets.rs`). `motion.span` (React and Solid) animates what an inline box shows
  (`Scope::InlineBox`: colours, `opacity`, the font of its text, padding, margin, border, radii, outline and the
  decoration lengths) with `animate`, `initial`, `exit`, `variants`, `whileHover`, `whileTap`, `whileFocus` and
  `whileInView`: the paragraph reads the style `motion/inline.rs` sampled for the frame (`InlineStyles`, which also
  lays the motion values bound in its `style` over it) in `paragraph::styles`, `inline_decoration::of` and
  `InsetSpec::of_moving`, so a width-affecting key lays the paragraph out again every frame. The gestures are
  states of the span's `PointerStates` sources, read whether or not the span declares a `hover` entry
  (`MotionState::set_gestures`, called from `sync_motion_states`); `whileInView` is a gesture JS turns on
  (`inView`) from an `IntersectionObserver`. A key that is not an inline box's (`width`, `display`, ...) throws,
  and a transform key, `layout*` and `drag*` are accepted and do nothing, as a transform does nothing to a
  non-atomic inline box in Chrome. The gestures, `variants` and the variant names a controlling component hands down
are those of every `motion.*` (`motion-variants.ts`; deviations: no `staggerChildren` / `delayChildren` / `when`, no
keyboard tap or auto `tabindex` for `whileTap`, SVG shapes take no gesture of their own, and a leaving node answers to
none where Framer lets active gestures go on with the properties `exit` does not name). The `opacity` of an inline box
  is a compositing group (`OpacityGroup`, painted by `text/paint.rs` through `TextLayout::paint_group`) that
  paints after the text around it; selection and find washes stay under it, and an atomic inline in it is not
  faded. A span's CSS transitions and keyframe animations move the same properties; its other
  properties apply at once. Letter and word spacing and `textShadow` do not exist on an inline box, so they
  do not animate.
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
  selector engine to build `:has(.error)` on, and the states are the ones gpuix already tracks: each is
  read from the chain it already keeps (the hover chain and the active chain minus their target, the
  focus-within set minus the focused element, the focus-visible element's strict ancestors), so it costs O(1)
  per element and no tree walk. The restyle gating that decides which elements a hover or focus change
  restyles counts the `has*` keys. Transitions, animations, `<UniversalStyle>`, `motion`, inert gating and
  inline spans treat them like their plain counterparts.
- **`:disabled` / `:enabled` and `hasDisabled` / `hasEnabled`.** The form controls are `<input>`, `<textarea>`
  and the `div`s that stand in for a `<button>` or `<select>` (`Button`, `buttonProps` users such as the dialog
  triggers, `SelectTrigger`, the `Select` root, `ComboboxTrigger`). Those `div`s say so with the internal
  `formControl` prop (`"enabled"` | `"disabled"`, forwarded for every host type), because `disabled` itself is not
  forwarded for a `div`, so a plain `<div disabled>` matches neither, as in React DOM; `RetainedElement::control_disabled`
  is the one reader, and `in_disabled_form_control` (click suppression) uses it too, so a disabled `Button` no
  longer receives `click()` or the VoiceOver press. The strict ancestors of the controls are worked out the first
  time a style that declares `hasDisabled` or `hasEnabled` asks in a frame (`ControlStyles`, from the tree's
  `control_ids`, walking `parent`), not every frame. The four layers sit after `active` and before the focus ones
  (`hasEnabled`, `hasDisabled`, `enabled`, `disabled`, as Tailwind orders its variants), so `disabled` wins a field
  over `hover`/`active` while the control still matches them, as in CSS. Unlike `hover` they are not pointer
  state: an `inert` subtree keeps them. Deviations: no `<fieldset disabled>` or disabled `<form>` (neither exists),
  `:enabled`/`:disabled` for an `<option>` or `<optgroup>` do not exist, and `:checked`/`hasChecked`/`:indeterminate`
  are not available (no checkable element; they need a native checkbox control).
- **Universal style.** `setUniversalStyle(style | null)` (and `<UniversalStyle>` in the React and Solid
  adapters) is a fork addition equivalent to `* { ... }`: one style applied under every element, at the
  lowest precedence per property, to elements mounted later as well. It is not inherited, text nodes are
  not matched, and it is not a stylesheet (no selectors). Edge: when the layer is the first to add
  `transition` to an element that has no style of its own, a colour set in that same change applies at
  once instead of transitioning.
- **Outline.** `outline` (`width || style || colour`, any order) and `outlineStyle` are CSS's: the style is
  `none` unless one is given, the width `medium` and the colour `currentColor`, so a ring needs a style. The
  style reaches the painted outline (`gpui::Outline.style`, a zed commit), so `dashed`, `dotted` and `double` draw.
  `outlineOffset` is not part of the shorthand, and a state's `outline` resets the base's width, style and colour.
- **Text wrapping.** `textWrap` (shorthand), `textWrapMode` and `textWrapStyle` are inherited. The mode is the
  longhand of `whiteSpace`'s wrap component and wins over it on the same element (`nowrap` beats `pre-wrap`;
  `whiteSpace: "pre"` with `textWrapMode: "wrap"` wraps and hangs like `pre-wrap`). `balance` and `pretty` choose the
  breaks (zed `text_system/text_wrap.rs`) among the same break opportunities with greedy wrapping's own line measure,
  so the line count and the width the text reports to layout are greedy's and `textAlign` aligns within it.
  `stable` is `auto`. The style is a block container's alone, as in Chrome; the mode is also per inline box
  (a `nowrap` span is not broken, a `wrap` span in a `nowrap` block is).
- **Text box trim.** `textBoxTrim`, `textBoxEdge` and `textBox` cut the space above the first line and below the
  last of a block container down to the cap height, x-height or baseline (zed `text_system/text_box.rs`). The trim
  is not inherited: the renderer hands it down block children to the first and last in-flow one and into the
  paragraph.
- **Font variations.** `fontVariationSettings` is a comma list of `"tag" number`, inherited, on elements, spans
  (per run) and inputs, like `fontFeatureSettings`. On a variable font `fontWeight` sets `wght` as Chrome's font
  matching does, so 650 is its own weight and 600 is 600 (it was the 590 semibold instance); a `wght` in the settings
  wins. Runs of one family with different variations keep their own face (zed `Font` and `TextStyle` carry the axes).
  macOS only.
- **Font smoothing.** `WebkitFontSmoothing` (`auto`, `antialiased`, `subpixel-antialiased`, which is `auto`) is
  CSS `-webkit-font-smoothing` as Chrome has it on macOS, and the first vendor-prefixed style key. gpuix drew
  every glyph with CoreGraphics smoothing on, 9-17 % heavier than Chrome's `antialiased` for dark text and 22-42 %
  for light (#67). It is inherited like `fontWeight`, through the zed `TextStyle` (`font_smoothing`, in the
  refinement `apply_styles` writes), on spans (`RunStyle`), inputs (the editor's text style) and through
  `display: contents` (`inheritable_fields!`), and it can be set in a state style. In zed the value rides on the
  `TextRun` and the `DecorationRun` with the colour to `Window::paint_glyph`, and is part of `RenderGlyphParams`
  (equality, hash), so one glyph drawn both ways has two atlas tiles. `Antialiased` is
  `set_should_smooth_fonts(false)` for every colour. `Auto` is Skia's recipe for smoothed text (`SkScalerContext_mac_ct.cpp`):
  CoreGraphics draws the glyph black on white with smoothing whatever the text colour, its gamma of 2.0 is taken out
  of each pixel (`sk_pow2_table`), and the coverage goes through Skia's mask gamma table (`SkTMaskGamma_build_correcting_lut`,
  sRGB device, contrast 0) for the colour's luminance, with each channel halved and cut to 3 bits, as `onFilterRec` does
  for "some" smoothing. The bucket (`RenderGlyphParams::luminance_bucket`, `glyph_luminance_bucket_for_color`) replaces
  the grey fill per luminance level the rasteriser had (`dilation`). The system's own `AppleFontSmoothing = 0` still
  turns it off for `Auto` (the plain coverage, as for `antialiased`). The fork did not read or write that preference
  to get the effect: it is a user setting, not the page's. Not animatable.
- **Underline position and thickness.** `textUnderlineOffset` (inherited) sets the distance from the baseline, and
  the default is Chrome's: the line's top edge is `max(1, ceil(thickness / 2))` under the baseline. `auto`
  thickness, and `from-font` when the font has none, is a tenth of the font size, not 1px. A line is drawn at the
  baseline of the box that declared it, wherever the boxes inside it are raised to, and a thickness that is a length
  rounds half up to a device pixel, at least one.

Cross-area decisions that touch D:

- **Group state comes from A's `PointerState`.** `groupHover`/`groupActive` apply when the nearest
  ancestor with that `group` name is hovered or active. A group applies to descendants only, and the
  nearest same name wins. State layers fold in the order base, `groupHover`, `hasHover`, `hover`,
  `groupActive`, `hasActive`, `active`, `hasEnabled`, `hasDisabled`, `enabled`, `disabled`, `groupFocusWithin`, `hasFocus`; `focusWithin`, `hasFocusVisible` and
  `focusVisible` are applied to the element after that fold, so focus layers beat the pointer ones,
  `groupFocusWithin` included. An element's own state beats what its descendants cause, and that beats a group. GPUI's `.group()` and `.group_hover()` are no longer called on host elements. There is no
  group state inside an inert subtree.
- **A state style's shorthand resets the longhands it covers** (`background`, `overscrollBehavior`,
  `gridArea`/`gridColumn`/`gridRow`, `border*`, `borderColor`, `mask`, `outline`, `textBox`, and C's box
  shorthands), so a hover `background`
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
- An outline paints only with a style (`outline` or `outlineStyle`): `outlineWidth` and `outlineColor` alone draw
  nothing, as in CSS. A focus ring written `focusVisible: { outlineWidth: 2, outlineColor: "..." }` needs
  `outlineStyle: "solid"` or `outline: "2px solid ..."`.
- The default underline sits at the baseline plus `max(1, ceil(thickness / 2))`, and `auto` thickness is a tenth
  of the font size (it was 1px), so underlines are lower and thicker on big text than they were.
- `fontWeight` on a variable font renders the exact `wght`: 600 on the macOS system font is 600, no longer the
  590 semibold instance.
- `textWrapMode` wins over the wrap component of `whiteSpace` on the same element.

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
  `image-set()` (skipped) and the legacy `-webkit-mask-composite` keywords; `no-clip` and
  `margin-box` invalidate a value, `mask` is not animatable, and an image mask's tiles are culled to the
  content mask at group start.
- **`mask-border` is the top mask layer, added.** `maskBorder` and its six longhands draw a nine-slice of a
  gradient or `url()` image (`Style.mask_border`, zed `mask_border.rs`) as one more mask layer composited
  with `add`: Chrome's `-webkit-mask-box-image` unions with the `mask-image` layers, it does not intersect.
  The oracle is that prefixed property, because Chrome has no unprefixed `mask-border-*`; the names and
  initial values follow the spec where the two differ (`slice` starts at `0` and the shorthand does not imply
  `fill`; `mode` starts at `alpha`, which is also all Chrome reads). A slice is cropped from the atlas, so a
  vector source scaled up is as soft as its raster; Chrome draws it sharp. Space gaps are whole device pixels
  where Chrome antialiases them. A gradient source is laid out for each slice's scaled size, not warped with
  it, so a radial one keeps its circle and an angled one its angle. The Metal polychrome sprite shader clamps
  its lookup to the sprite's tile to keep a crop from blending in its neighbour; the HLSL and WGSL do not. The
  `mask` shorthand resets `maskBorder` and its longhands as CSS Masking says it does; Chrome has no `mask-border`,
  and its `mask` leaves `-webkit-mask-box-image` alone, so that follows the specification and not the oracle.
- **`clip-path` has no `url()`, and `path()` is flattened.** `url()` and `filter: url(#id)` reference
  an SVG `<clipPath>` or `<filter>`, and gpuix has no SVG DOM, so they stay out of scope. `path()`
  becomes polygon contours, one per subpath, with curves flattened to 0.25px and a limit of 4096
  vertices for the whole path (coarser, up to 4px, before it is invalid). A `mask` or `clip-path` with
  an unsupported value is invalid, as in CSS, instead of falling back.
- **Limits of elliptical corners.** A dashed or dotted border on an elliptical corner is laid out
  along the arc of the corner's mean radius. A radius string is a CSS length like any other (`px`, the
  absolute units, `%`, `em`, `rem`, the viewport units and `calc()`), and `motion` animates both radii of a
  corner (area E). An invalid or negative radius throws when the style is committed, as an invalid width
  does, where `boxShadow` and `clipPath` ignore an invalid string.
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
  them. A dotted or dashed line is fitted to the line it is on as Blink's `StrokeData` does (`DecorationGeometry::fitted`:
  the gaps stretch so the last mark ends on the line's end, a thin dotted line is square marks), and starts
  over with every text fragment (`UnderlineStyle::fragment`, set per text node by `text/paragraph.rs`); Chrome's
  wavy line is its own curve (a wavelength of 1 + 2 * round(2 * thickness + 0.5)) that gpuix's sine does not
  match, it only starts over per text node too. A decoration the paragraph's own block declares restarts per
  text node only in a paragraph where some box restyles something: one in which no box does is plain text, one
  fragment. `text-decoration-thickness` takes `em`, `rem`, the viewport units, `calc()` and a `%` of 1em, resolved
  at the element that declares it. `from-font` reads the run's font. An underline is drawn at the
  baseline of its decorating box, a line-through by two thirds of the ascent of the decorating box's font from
  the top of the text it crosses, an overline up from the top of that text, as Chrome does (the oracle
  `text-decoration-lines-oracle`); the decorating box is only known to the line layout when it is listed
  (`paragraph::listed_boxes`: one aligned off the baseline, or a strike-through in another font than a box's
  inside it). Lines that a vertical-align shift puts on a rounding tie are a row from Chrome's. Chrome interrupts an underline where
  it crosses a glyph's ink (`text-decoration-skip-ink`); gpuix draws it whole. `textUnderlineOffset` and
  `textDecorationThickness` animate, from the offset an element inherits when it leaves it unset (an unset
  thickness is `auto`, which is not inherited), and through a negative thickness, which is valid and paints one
  device pixel as zero does, all against Chrome. Under `display: contents` the `em` of `textUnderlineOffset` resolves against the child's font. `outlineStyle: auto`
  paints as `solid`.
- **CSS lengths are linear expressions.** Every length field (sizes, margins, insets, padding, gap,
  `flexBasis`, border and outline widths, `fontSize`, `lineHeight`, shadows, the lengths inside
  filters, gradients, masks, clip paths and grid tracks) takes a number or a string with `px`, the
  absolute units, `%`, `em`, `rem`, `vw`, `vh`, `vmin`, `vmax` or `calc()` (`+` and `-` between any
  of them, `*` and `/` by numbers). It is parsed once into `LinearLength` (`css/length.rs`) and resolved
  at apply time with a `LengthContext` carried in `Inherited` next to `ColorContext`: the font size is
  cascaded during the build (`em` of `fontSize` is the parent's, `rem` the root element's, initial
  16px) and the viewport is the window size. A length plus a percentage reaches layout as a taffy
  calc value (vendored taffy, `DefiniteLength::Calc`) or, for backgrounds and gradient stops, as
  `LengthPercentage::Calc` / `StopPosition::Calc`. `motion` animates every length, `calc()` and the relative units
  included, as a px and a percentage part (area E). Limits: a focus-visible or focus-within `fontSize` changes only
  the element's own text, not its children's `em`, as colour does; two nested `display: contents` ancestors with `em` in `fontSize` do
  not compound; a `calc()` that comes out negative clamps to 0 for `fontSize` and track sizes;
  `fit-content(calc(length + %))` as a grid track is rejected (taffy has no calc `fit-content`); a
  repeating gradient with a length-plus-percentage stop skips the zero-length averaging special case.
- **`lineHeight` is CSS `line-height`.** A number is a multiplier of the element's font size (it was
  px), a string is a length, and `"normal"` (also the default of an element that sets none) is the height
  of the font: `TextStyle.line_height` is a `gpui::LineHeight`, `Normal` or a `DefiniteLength`, no longer
  a `DefiniteLength` that defaults to `phi`.
- **Line boxes are Chrome's on macOS.** `FontMetrics::line_box(font_size, scale_factor)` rounds a face's
  ascent, descent and line gap in device pixels at the used size and widens the ascent of the families
  Times, Helvetica and Courier (the `widens_ascent` field, set by `gpui_macos` from the face's family name;
  the 15% is of the device-pixel content area), `normal` is their sum, the half-leading above the text is
  floored to a device pixel (`extent_in_line`), and a line height is laid out in sixty-fourths of a device
  pixel instead of snapped to the device pixel (a number cut, a length or percentage rounded). Blink zooms the font to the display
  before it measures it, so Menlo 20px is 24px of content at scale 1 and 23px at scale 2, and a 24px line
  around Menlo 16px has its baseline at 17px and 17.5px. The scale is the window's: `WindowTextSystem`
  holds it (set with the window's own scale, so a window that changes display shapes again),
  `PlatformTextSystem::layout_line` takes it, a `LineLayout` records the scale it was shaped for (its rows
  and every reader without a window use it), and it is part of the line layout cache key, the shape store's
  and the text element's measure key; `gpui_wgpu` and `gpui_windows` accept it and ignore it. The strut is the font of the element
  (`TextLayoutOptions::strut_font`), not the largest font among the runs, and a run whose rounded ascent or
  descent differs from it, a weight change included, is a box of its own. The model was fitted to
  `packages/react/src/__tests__/fixtures/line-box-oracle.json` (1080 boxes), which `line-box.test.tsx`
  asserts. It changes `TextStyle`, `TextLayoutOptions` (it is `Hash` and in the cache key), `FontMetrics`
  and the platform shapers' ascent and descent, so Zed's editor sees it too; the `gpui_windows` and
  `gpui_wgpu` shapers still report unrounded metrics, and the rest of the Zed workspace is not updated
  for `LineHeight`. Limits: the CSS keywords `system-ui` and `sans-serif` are not font families here, and a
  glyph that falls back to another font does not grow the box, as it does in Chrome.
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
- **Text wrap.** `balance` and `pretty` are a scored breaker that approximates Chromium's. `pretty` re-breaks at most
  the last four lines, and only when the last line is a single short word. `balance` is limited to 6 lines per
  forced-break segment. Both are skipped for text with a tab, `break-spaces` or `lineClamp`. Only block containers
  take `textWrapStyle`; on a span it does nothing, as in Chrome. The mode is per box (`white-space` of a span
  too, which `text-wrap-mode` on the same span wins over). Where a line breaks at the edge of a span follows
  Chromium's rules for spaces and for the places between letters, which were read off its behaviour and not its
  source; they are checked for a word, a space and a word each in a box of its own, for `break-all` and for CJK.
  The other places a line may break are Blink's `LazyLineBreakIterator` (zed `text_system/line_break.rs`): its fast
  pair table for two characters in `!`..`ÿ` (`FillAscii` for ASCII, Chrome's measured table for the rest) and the
  Unicode line breaking algorithm over the whole text for every other pair, from ICU4X with the Unicode 17 data
  (`unstable` feature), `Normal` strictness and its dictionaries for Thai, Lao, Khmer and Burmese (the release addon
  grows by 1.9 MB). `keep-all` is the same places minus two letters or numbers in a row, and `break-all` the same
  plus Blink's `kBreakAllLineBreakClassTable` (`breaks_after_break_all`: a break between the UAX #14 classes the table
  lists, asked before the fast table's refusal, never before a space or a `BA` character but `|`); the min-content width
  follows them. Checked against Chrome in `line-break-oracle` and `hyphen-break-oracle`, each case in a box of its own.
  Deviations: ICU4X is not ICU, so a quote next to a small kana or `ー` and Lao and Khmer word boundaries can differ
  from Chrome; there are no `hyphens` or `line-break` styles (a soft hyphen breaks without drawing a hyphen, though the
  hyphen's width counts in the fit as Blink's does, and the strictness is Chrome's `auto`). `/` is not a break and `?` is, as in Chrome. `LineWrapper::wrap_line`, the
  editor's own wrapper, is not touched and does not break after hyphens. `hang`ing of the
  spaces is the paragraph's, so a `nowrap` block with a `wrap` span hangs them everywhere. A plain box is not listed
  to the line layout unless some box of the paragraph wraps.
- **Text box trim.** A lone `cap` or `ex` edge is `cap text` or `ex text` as the spec says, where Chrome currently
  rejects it. The trim does not pass through anonymous table cells, and the `text` edge uses the line strut's
  metrics.
- **Font smoothing.** `none`, which Safari has, is an invalid value here, as are all but the three Chrome's
  macOS build draws. Only the macOS rasteriser reads `Antialiased`; the Windows and wgpu text systems ignore it,
  as they ignore the luminance bucket. `auto` is Chrome's to 0.07 % of the ink for black on white and white on black,
  and to 0.62 % (scale 1) and 0.32 % (scale 2) for #a1a1a1 on #0a0a0a, the same grey-on-near-black residue `antialiased`
  has (0.9 % and 0.4 %), which is in how gpui's shader blends against Skia's, not in the mask.
- **Font variations.** An invalid `fontVariationSettings`, `fontFeatureSettings` or `fontVariantNumeric` is dropped
  when the style parses (a `motion` description or keyframe with one is still an error). The automatic mappings of `opsz` (from the font size) and `wdth` (from
  `font-stretch`) are not done. A keyframe track of three or more values whose tag sets differ flips as a whole,
  where Chrome decides each pair of keyframes on its own.
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

- **Lengths are a px and a percentage part.** A motion length is `AnimValue::Number`, `Percent` or `Calc { px, percent }`
  (`motion/value.rs`), whatever unit it is written in, so `px`, `%`, `calc()` and `em` / `rem` / `vw` / `vh` / `vmin` /
  `vmax` mix and blend part by part, as Chrome blends a `calc()` pair. Relative units are px when a track holds them
  and the percentage stays a share of its basis, which lowers to a taffy calc node every frame: a parent that resizes
  mid-animation is followed exactly with no measurement, and a spring carries its speed on each part. Measurement
  still serves `auto` and unset starts only. Number-kind fields (`fontSize`, gaps, border widths, outline width and
  offset, corner radii) take length strings too, and refuse a percentage where CSS gives none. CSS transitions and
  animations start from a calc value instead of treating it as unset.
- **Multi-value shorthands split before they parse.** `animate`, `initial`, `exit`, keyframes and CSS `@keyframes`
  split `padding`, `margin`, `inset`, `borderWidth`, `borderColor`, `gap` and `borderRadius` (the `/` form included)
  into one value per longhand (`split_shorthand`), frame by frame for keyframes, so each side, axis or corner has its
  own track. An elliptical corner is one `AnimValue::Ellipse` of four channels, so both radii animate.
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
  value mid-flight continues from where it is. `animate()` returns Motion's playback controls (`time`,
  `speed`, `duration`, `iterationDuration`, `state`, `pause()`, `play()`, `stop()`, `cancel()`,
  `complete()`, `finished`, `then`), run natively: a source keeps its animation as a track on a `Playback`
  clock (`motion/web_animation.rs`, shared with CSS animations), `Sample.moving` separates a held animation
  (in effect, no frame asked for, no velocity) from one that moves, and the renderer has
  `pauseMotionValue`, `playMotionValue`, `seekMotionValue`, `setMotionValueSpeed`, `completeMotionValue` and
  `getMotionValuePlayback`. Checked against Motion 14.1.0 itself on a stepped clock
  (`motion-controls-oracle.mjs`, `motion-controls-native.test.ts`), and through React and Solid in
  `motion-controls.test.tsx`.
- **Layout animations**: `layout` (FLIP), `layoutId` shared-element morph, `LayoutGroup`, and a
  `Reorder` drag-reorder primitive. `AnimatePresence` gets `mode="wait"`, `"popLayout"` and `custom`.
- **SVG**: `<svg>` takes DOM-style shape children (`<path>` and friends); `pathLength`,
  `strokeDasharray` and `strokeDashoffset` draw, and `d` morphs.
- **CSS keyframes drive the SVG properties Chrome animates** (`strokeDashoffset`, the other stroke and paint
  properties, geometry, `d`), on the shapes in an `<svg>` and on the `<svg>`. Keyframes carry
  `PropKey::{Style, Svg}` (`motion/keyframes.rs`) and a name resolves per target (`PropTarget` in
  `css_animation.rs`: shape, host, element): on a shape to the SVG property first (`opacity`, `width`, `height` and
  `fontSize` included), elsewhere to the style, where an SVG-only property is ignored because Chrome computes it
  and never paints it. A shape is never built as an element, so `svg_tree.rs` advances its CSS animations while the
  `<svg>` writes its markup; their values overlay the shape's props and its `motion`, and a shape with animation
  state joins `Sources.motion` while it moves or its keyframes changed, so the host is built again. The `<svg>`'s own
  values go on the root node, which is how inherited properties reach the shapes. A missing endpoint is the
  shape's own attribute, else the nearest ancestor's for an inherited property, else CSS's initial value
  (`SvgAttr::initial_css`). `defineKeyframes` refuses the attributes that are not CSS properties (`pathLength`,
  `x1`, `stdDeviation`, `dx`, `refX`). Oracle `svg-keyframes-oracle`.
- **Web Animations objects.** `element.getAnimations()` returns `CSSTransition` then `CSSAnimation` objects
  (`js/web-animations.ts`), one per native run id per renderer. Native carries the clock
  (`motion/web_animation.rs`: id, rate, the API's pause and play override, seek, end time); a run that ended stays
  readable until JS releases it (`releaseAnimation`), `RendererState.dispatch` consumes
  `webanimationfinish` and `webanimationcancel` before any app dispatch, and the mutation queue idles the animations
  of destroyed elements. A command names its element to the next frame (`Sources::commanded` in `retain.rs`), since a
  transition that had finished runs again after `play()` and nothing in its last build said it could. Relevance
  follows Chrome by phase and rate: a paused animation at its end without a fill is not listed. Oracle
  `web-animations-oracle`.
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
- A `motion` or keyframe target written with `calc()`, `em`, `rem`, a viewport unit or a mixed length no longer
  throws, and a CSS transition from or to one no longer jumps. `borderRadius` and the corner longhands are
  not circular-only: a percentage or elliptical corner animates.
- A `width`, `height` or `px`-to-`%` animation that waits for a measurement starts its clock at the change, not at
  the frame that measured, so its first moving sample is one frame earlier than it was.
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

- **`width: "auto"` as a motion target** works like `height: "auto"`: the natural width is measured one frame after
  the first paint, and the clock starts at the style change. Taffy's content
  width is not the width `auto` would give, so a node that can animate width carries
  `Style::measure_natural_width`: `compute_layout` lays the tree out once with those nodes' widths
  `auto`, records their border-box widths (`Window::layout_natural_width`), restores the styles and
  lays out again (taffy's cache redoes only the dirty chain). The flag is set only while a motion
  has a pending `width` goal containing `auto`, or a transition list covering `width` or `all`, or
  keyframes using `width`, with `interpolateSize: "allow-keywords"` for the last two; any such element
  pays the extra layout every frame. `animate` is not gated by `interpolateSize`, as for height.
  Inside the pass an inner flagged node is `auto` while an outer one is measured.
- **Not built: size morphs.** `layout="size"` does not scale a box with a transform, so a layout animation
  glides a position and a size snaps. The glide is `Window::with_element_offset`, which shifts paint, bounds
  and hitboxes, and is why `layout`, `layoutId` and `Reorder` can move a box, `Reorder`'s dragged item
  included. The FLIP scale would need the children's counter-scale in gpuix on top of the transforms
  Round 19 added. Scale, rotate and skew themselves exist, as `transform` and Motion's keys, drawn by a
  matrix in every primitive's shader and not by an effect group, so scaling up does not blur
  ([Round 19](#round-19-92-css-transforms)).
- **`transition-behavior: allow-discrete`** works for `display`, `visibility`, the flex and
  alignment keywords (`flexDirection`, `flexWrap`, `alignItems`, ...), `position`, `overflowX`, `overflowY`
  (and `overflow` for both), `pointerEvents`, `tableLayout`, `borderCollapse`, `captionSide` and `emptyCells`,
  with the spec's `display` and `visibility` exceptions; `cursor` is not transitionable, and a
  transition into or out of `display: contents` is unreliable (the box
  generation reads the declared style). Like `display`, they are not targets of `motion` or keyframes
  (Chrome flips them at 50% there). `zIndex` transitions as an integer rounded half up, and
  `verticalAlign` blends a length or a percentage of the line height; a keyword against a length is
  discrete. An element held by an X-to-`none` display transition is
  listed in a per-frame thread-local read by `is_display_none`. There is no `@starting-style`: the
  first style an element is seen with never transitions (`motion`'s `initial` is the equivalent), so
  `none` to X shows X at once.
- **`auto` in a margin, an inset or `flexBasis` is discrete**, as in CSS where `auto` does not interpolate with a
  length: `motion`, keyframes and `@keyframes` flip at 50% of the eased progress, and a CSS transition runs it only
  under `allow-discrete` (`AnimValue::Keyword("auto")` paired by `Layout::DiscreteList`). Unset insets and
  `flexBasis` start at `auto`, unset margins at 0. `flex` is a target (`expand_shorthand` splits it into
  `flexGrow`, `flexShrink` and `flexBasis` with the static grammar of `css/flex.rs`) but a motion value cannot be
  bound to it. The logical shorthands (`paddingBlock`, ...) are not targets. One Chrome quirk is not copied: a CSS
  transition of a margin from a length to `auto` (under `allow-discrete`) holds the length until the end in Chrome
  (headless shell 1248), where Web Animations, the insets and the sizes flip at 50% as the spec says, and gpuix flips
  at 50% for all of them.
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
  where the underlying value cannot be read (inherited `fontSize`) the nearest keyframe holds. On an SVG shape
  it is the attribute, else the inherited value, else CSS's initial value (`0` for `strokeDashoffset` and `r`, `1`
  for `fillOpacity`, black for `stopColor`, `none` for `strokeDasharray` and `d`, `auto` for `width` and `rx`).
  Keyframe properties are limited to the animatable set.
- **`%` in motion.** The percentage part of a length is a share of the parent's content box (the border box
  for a corner radius), so it follows the parent's size exactly. Margins and insets resolve against the
  parent's content box, width for left, right and every margin side, height for top and bottom. CSS uses the
  nearest positioned ancestor's padding box for insets and absolute margins (needs a public `containing_block`
  and its padding-box size from zed). A `%` height under a parent with no definite height is `auto`
  (`motion/containing.rs`; grid items never count as definite, relative `top`/`bottom` and `minHeight`/`maxHeight`
  `%` in an auto parent are not zeroed). Padding resolves against the parent's width on every side, as in CSS, and
  `flexBasis` against the main size of its flex container.
- **Auto-size animations start at the style change.** An animation that waits for a measurement (`auto`, an unset
  size) takes its numbers from it but its clock from the frame of the change (`Pending::started`), as an
  `interpolate-size` transition does in a browser. A measurement that arrives frames after the commit therefore
  starts the animation that far in.
- **Relative units in motion.** A length in `em`, `rem` or a viewport unit is px by the time an element's track holds
  it, resolved in the element's `LengthContext`. A motion value is not tied to an element: it resolves the viewport
  units against the window at write time and keeps `em` and `rem` (`AnimValue::Relative`), which
  `MotionValueStore::apply` resolves against each element it is bound to, so one value is two sizes on two fonts.
  A CSS animation that moves an element's `fontSize` does not move the `em` of a bound motion value (motion values
  apply before CSS animations). A percentage in a motion value
  is a share of whatever element it is bound to. The percentage part of a spring rests at a hundredth of the tight
  px thresholds, so `restDelta` and `restSpeed` are in percentage points there. Multi-value motion values whose
  counts differ fill out by the shorthand's side rule (`"8px"` to `"8px 16px"`) instead of jumping; counts no
  shorthand could pair flip at the halfway point.
- **`fontVariationSettings` in motion.** Two settings that name the same axes blend per axis, as Chrome's
  `CSSFontVariationSettingsInterpolationType` does. Any other pair, `normal` against a list included, is discrete
  and flips at 50%; a CSS transition runs that only under `allow-discrete`, where `animate`, keyframes and
  `@keyframes` flip with no opt-in. A span animates it, the paragraph being laid out each frame.
- **Unset starts.** An unset `color` starts from the inherited colour (a parent's colour change does
  not start a transition on the child, where a browser would). `aspectRatio` and `lineHeight: normal` take the
  target at once, as CSS does for non-interpolable keywords. An unset `fontSize` and `fontWeight` start from the
  inherited computed values. An unset border or outline width starts at the `border` shorthand's width or `medium`
  (3px), whatever the style, as Chrome's animations do (checked against `fixtures/motion-values-oracle.html`:
  Blink zeroes a `none` border where it is used, so `getComputedStyle` reads 0 but a style that appears starts no
  transition). Chrome also floors a fractional border width to a whole device pixel, so an animated width steps
  where gpuix lays out the blended value. `letterSpacing` is not in `StyleDesc`.
- **Reduced motion** follows framer's `positionalKeys`: `width`, `height`, `top`,
  `right`, `bottom` and `left`, Motion's transform keys (`x`, `y`, `scale`, `rotate`, `skew`, the origins; the CSS
  `transform` properties are not reduced), and `layout`, `layoutId` and `Reorder` glides, jump; everything else
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
- **Motion value playback controls.** The `time` setter reads back as set (Motion counts the delay in it); a
  `speed` other than 0 after 0 runs the animation again without `play()`; `isAnimating()` is false after the
  controls' `stop()` and `cancel()` (Motion leaves it true) and true while a seeked or replayed animation runs;
  `time` stops at the end of the last repeat; `duration` and `iterationDuration` throw before the value is bound
  to a renderer. `play()` after `cancel()` runs the animation again, as in Motion; only `stop()` is final.
  A paused animation holds its value's node until it is stopped. `startTime` and `attachTimeline` are not built.
  `finished` no longer resolves on stop (gpuix used to); the `(from, to, {host})` overload of `animate` settles
  its temporary value on an internal signal.
- **SVG keyframes.** CSS transitions of SVG properties are not built (a changed prop jumps; `motion` and
  animations move them). A paint of `none` against a colour swaps halfway, as in Chrome; `none` or `auto` against a
  value is discrete, so a property with a `none` keyframe is discrete throughout. A `%` `width` or `height` means
  nothing on a shape, and paths with different commands flip halfway.
- **Web Animations.** Built: `pause()`, `play()`, `currentTime`, `playbackRate`, `playState`, `finished`,
  `animationName` and `transitionProperty`. Not built: `cancel()`, `finish()`, `reverse()`, `updatePlaybackRate()`,
  `ready`, `effect`, `timeline`, `startTime`, `pending`, `onfinish`, `oncancel`, `getAnimations({ subtree })` and
  `document.getAnimations()`. Animations of `motion`, `animate` and motion values are not listed: the Motion engine
  drives them, as Motion's JS loop does in a browser. `currentTime` reads the time of the last frame, not the
  instant of the call.
- The test window gained `setWindowSize` (AppKit `setContentSize`) for the resize tests.

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

## Window placement and displays

**Problem.** A window could only open centred on the primary display. An app could neither put a
window on a second monitor nor learn where one was. Electron's answer is `x` / `y` in the window options
and `screen.getAllDisplays()` / `getPrimaryDisplay()`, which the apps this fork ports already use.

**API.** `WindowOptions.x` and `.y` (CSS px) put the window's top-left there when both are set; with
one missing the window is centred as before. The space is global: origin at the primary display's
top-left, y down, so a display to the left of or above it is negative. `screen` (`packages/native/js/screen.ts`,
`./screen` in `@gpuix/native`, re-exported by `@gpuix/react` and `@gpuix/solid`) lists the displays with
Electron's `Display` fields (`id`, `label`, `bounds`, `workArea`, `size`, `workAreaSize`, `scaleFactor`,
`rotation`, `internal`). `fullscreen` with a point goes fullscreen on that display. The functions are read
off the native namespace, as `validateKeyframes` is: the browser entry has none, and a named import
would fail the adapters' browser bundles.

**What it maps to in gpui.**

- **GPUI reads a window's origin relative to its display, not globally.** `MacWindow::open` takes
  `WindowParams.bounds.origin` against the top-left of the display named by `WindowOptions.display_id`
  (points, y down). `to_gpui_window_options` is the only place that builds `window_bounds` and
  `display_id`: it picks the display for (x, y) and passes `DisplayId` and `(x - display.x, y - display.y)`.
  `WindowBounds::Fullscreen` wraps the same bounds, so a fullscreen window opens on that display.
- **The display comes first by containment, else by distance.** The point is in a display's bounds
  half-open, so a right or bottom edge belongs to the neighbour, as in a DOM rect. A point on no display
  takes the nearest, measured to the rect and not the origin, the first of equals. The window's size is
  the one asked for (or the centred size); it is not clamped to the display.
- **zed makes `MacDisplay` public** (`gpui_macos`, one commit). `PlatformDisplay` gives only local
  bounds (origin 0, 0) and no name, scale, rotation or built-in flag, and `MacDisplay` was
  crate-private. `MacDisplay::screen() -> Option<MacScreen>` returns the display's `NSScreen` data
  (`localized_name`, `frame`, `visible_frame`, `backing_scale_factor`) with frames in global points: Cocoa's
  y is flipped with the height of the first `NSScreen`, which is what `CGDisplayBounds` is.
  `rotation()` is `CGDisplayRotation`, `is_builtin()` is `CGDisplayIsBuiltin`. The trait is unchanged,
  and `bounds()`, `visible_bounds()` and the window's local placement are as they were (Zed relies on
  them). `screen()` is `None` for a display AppKit has no screen for (a mirrored one); `getAllDisplays()` leaves
  it out and `getPrimaryDisplay()` throws if the primary has none.
- **It works before the window opens.** On macOS no GPUI `App` exists until `init` opens the first
  window, so the screen API cannot go through `App::displays()`. `screen.rs` reads `MacDisplay`
  directly, and `init_macos` reads the displays before it builds the window options. `init_threaded`
  passes none: with no displays the window is placed at (x, y) with no `display_id`.

**Deviations from Electron.**

- **macOS only.** Both functions throw "only implemented on macOS" elsewhere (and in the browser build).
  Windows and Linux place the window at (x, y) in whatever space their platform uses.
- **The test renderer ignores `x` / `y`.** Its window stays offscreen (origin -10000), so the options
  are forced to `None`.
- **A point on no display opens on the nearest display.** The display whose rect is nearest is used.
- **Ids are `CGDirectDisplayID`s** (32-bit), not Electron's. An id that does not fit 32 bits throws.
- **A mirrored display is not listed**, since it has no frame of its own.

**Tests.** The mapping is Rust unit tests with fake displays, so any layout runs on one machine:
`window_options_tests` (18) and `screen.rs` (10) cover a point on a second display at (1512, -200), a point
on the primary, a point on no display, one of `x` / `y` only, fullscreen with a point, no displays,
and the edges of `display_for_point`. The real displays are read in `screen.test` (7) and the Rust
`screen` tests on macOS, which hold what must be true for any setup: the primary is in the list and starts
at (0, 0), ids are unique, each work area lies inside its bounds, `size` is the rectangle's, the scale
factor is at least 1, the rotation is a quarter turn, the label is not empty. zed has
`gpui_macos` `display` (2): `screen().frame` equals `CGDisplayBounds`. **Multiple displays are not
exercised in CI** (the tests run on whatever displays the machine has); a window opened on a second display, a rotated one and a
mirrored one were not checked live.

**Upstreaming.** The zed side is one commit that adds exports and accessors to `gpui_macos`, with no
behavior change, and can go alone. The gpuix side (`screen.rs`, the two options, `to_gpui_window_options`)
is independent of the areas above.

## The perf pass: cheaper frames (#36 to #42)

**Problem.** Seven issues, found by profiling the demo on large pages:

- **#36**: nested auto-width columns laid out in exponential time. One level cost 11 µs, twelve cost
  6174 µs.
- **#37**: a frame builds every element, lays out every node and paints every quad, whether or not
  anything changed. An idle page of 2042 elements cost 16.8 ms (about 8.3 µs an element), and one node
  animating on it cost the same, 17.0 ms.
- **#38**: content outside an `overflow` clip was prepainted and painted like the content inside it.
- **#39**: a bounds or scroll read after a write drew the window. A pointer handler that wrote and read
  on every move paid 3 draws per frame the display shows once.
- **#40**: the demo reported `offsetX` and `offsetY` as missing.
- **#41**: a text was laid out to the next whole pixel, so a box as wide as its text was up to 1 px
  wider than Chrome's and a trimmed label was taller than its font metric.
- **#42**: texts whose measure depended on things the layout key did not hold (the padding of a span,
  atomic inline boxes) were measured and shaped again in every frame, hidden copies and `opacity: 0`
  ghosts included.

Round 9 added eight more, found by the same demo on larger pages and by comparing reads with Chrome 155
([below](#round-9-44-to-49-57-and-58)): **#44** the baseline of a trimmed label, **#45** bounds in whole
pixels, **#46** hover frames that grow with the page, **#47** a bounds read that costs a pass after a
write that cannot move a box, **#48** a panic under verify, **#49** a motion on a `display: contents`
element that does not reach its children, **#57** one animating text, and **#58** hidden text on rebuilt
frames.

Round 12 added two about animation ([below](#round-12-62-and-63)): **#62** one looping animation cost the
whole page, a tick of a colour loop on 30000 nodes taking 85.8 ms where it built 2 elements, and **#63**
animations on content nobody sees cost the page a build each, and a hover frame still paid for every
replayed element.

Apart from the list that follows this pass is not behavior. A frame must be the frame a full build of
the same state gives, and where it is not that is a bug, except for the
[deviations](#deviations-from-a-full-build) below. The numbers are in [Results summary](#results-summary).

**Behavior that did change**, each because the DOM does it that way:

- A frame's reports reach the app in the order of the DOM's rendering steps: scroll events, then
  `onLayout` and `ResizeObserver`, then `IntersectionObserver`
  ([below](#layer-3-gpuix-decides-what-to-skip-retainrs-tapers)).
- `onLayout` fires on every element an observer works on, custom elements included.
- An element that is removed is reported gone by the next presented frame, never by a layout read.
- `scrollTo` on a scroller with no box (under `display: none`) does nothing, and a smooth
  `scrollBehavior` no longer animates it.
- Text is laid out in fractions of a pixel ([#41](#41-text-in-fractions-of-a-pixel)).
- A box is read back in fractions of a pixel, as layout computed it, and not snapped to the device pixel
  it is painted at ([#45](#45-bounds-in-fractions-of-a-pixel)). Code that read whole pixels gets
  fractions.
- An `opacity: 0` element paints no primitive. It keeps its box and its hit area and runs its listeners,
  text registry, selection and find ([#46](#46-57-58-what-a-hover-frame-costs)). While an animation or
  transition of its `opacity` is current it paints its group at 0 so that its operations keep their shape
  ([#81](#81-an-opacity-run-is-a-compositor-layer-from-its-start-to-its-end)).
- A motion, transition or keyframes animation of an inherited property on a `display: contents` element
  reaches its children
  ([#49](#49-an-animated-inherited-value-on-display-contents)).
- A paint-only animation on content a frame walked culled does not build or draw: its clock, its events and
  `getAnimations()` move at ticks that present nothing, as Chrome runs the animations of content it does not
  show ([#63](#63-content-nobody-sees-costs-the-page-nothing)). `animationiteration` and the other events
  come at the frames a per-frame tick gives them, and `getAnimations()` reads the instant of the tick.

### #36: taffy's measure cache

The cause is in the vendored taffy. A node caches its size measurements in slots chosen from the known
dimensions and the available space, but a lookup also compares the parent size. A stretched column child
is measured once under the container's width and once with an unknown parent width; both measurements
chose the same slot and evicted each other, so every level re-measured the level below it twice
(24547 flexbox computations at depth 12, 82 after the fix).

`third_party/taffy/src/tree/cache.rs` now keeps the measure entries fully associative with exact-key
lookup and clock (second-chance) replacement. That is the eviction change of upstream
[DioxusLabs/taffy#1010](https://github.com/DioxusLabs/taffy/pull/1010), and `Cache::get` takes `&mut
self` for the second-chance bit. The key is the fork's own, because a cache that survives from one frame
to the next is asked more kinds of question than a per-frame one: it tells apart everything the answer of
a node can depend on. Upstream's packs the requested axis into the sign bits of the parent size (a
negative parent size, an inner size below its padding, was the same key as the positive one) and leaves
out the sizing mode and whether the node's top and bottom margins may collapse with its parent's, so a
node moved to a parent that asks the other way answered with the old parent's entries. Here a `modes`
byte holds the sizing mode, the two collapsing flags, the axes asked and which dimensions are known. A
size measurement is cached with the margins that collapse through the node, which a block container reads
(upstream cached the size alone, so a container's height depended on how many times it had asked). And a
block container no longer adds a child's offset to the height its floats take when the child holds
none, which made a scroll container as tall as the top of its last box when the box overflowed it, and as
tall as the box when the box came from the cache. The engine fuzz found all of these (below).

Guards: a taffy unit test counts flexbox computations at depth 14, unit tests of the cache key, of the
size-measurement entries and of the scroll container, and `examples/depth-col.perf.test.tsx`
(`bun run test:perf:depth`), which fails when depth 12 costs more than 3 times the linear extrapolation of
depth 1.

### #37: frames that skip what did not change

**Why three layers.** gpui consumes its elements every frame: the element arena is cleared and a
`Drawable` runs once. A frame that does not build a subtree therefore has to put back what the subtree
prepainted and painted in the frame before, and that only works if its layout nodes are still there. So
the layout engine must persist (layer 1), gpui needs an element that can be replayed (layer 2), and
gpuix has to decide which subtrees a frame may leave unbuilt, and repeat what building them did outside
the frame (layer 3). Each layer is useful alone and was committed and measured on its own.

#### Layer 1: a layout engine that keeps its nodes

zed `crates/gpui/src/taffy.rs` and the vendored taffy. The engine is not cleared between frames.

- **Keyed nodes.** A request is named by a scope (the hash of the `GlobalElementId` of the element
  asking, or of the nearest ancestor that has one) and its ordinal among the requests in that scope.
  The key only finds a candidate node. The request then compares its style, children and measure key
  with the node's and sets what differs, so taffy's per-node cache survives for every node a frame
  leaves alone. Requests without a key (`request_layout`, `request_measured_layout`) make a node that
  no later frame gets back; the engine counts them (`unstable_requests`).
- **Measured leaves.** A text, image or list measures through a closure that writes its result into a
  captured `TextLayout` (or the list's state). A keyed measured request
  (`request_keyed_measured_layout`) passes a *measure key*, which must hash every input the closure
  reads, and a state handle. With an equal key, equal style, rem size and scale factor, the node keeps
  the sizes taffy cached for it, the new closure replaces the old one, and the request gets the
  previous state back, because the closure that filled it will not run again. Taffy tells a measure
  function whether it is placing the leaf (`RunMode::PerformLayout`) or being probed
  (`ComputeSize`), and the engine records the call that placed each keyed leaf and calls it again if a
  later probe overwrote the state, so the state is always the one of the placement. Keyed today:
  `StyledText` (text, runs with their fonts and variation axes, font size, line height, white-space,
  overflow, line clamp, layout options such as the wrap style, `text-box-trim` and `text-box-edge`,
  inline boxes, the insets key, the atomic inlines' digest, font generation, and the font of the style
  when a trim cuts the box by its cap height or x-height), `img`, `list`, `uniform_list`. gpuix's
  `<input>` is keyed by its shaped text, runs, sizes, rows, font generation and entity
  (`TextEditorState::measure_key`, Round 14a), and a text with atomic inlines is keyed only while its boxes persist
  ([#42](#42-texts-that-keep-their-measure)).
- **Rewritten styles.** gpui's layout rewrites some boxes after they are requested: width keywords
  (`min-content` and kin), natural widths, absolute boxes whose containing block is not their parent,
  forced widths, window roots stretched to the viewport. `NodeMeta::requested` keeps the style the
  element asked for, so a request compares with that and not with what taffy holds. These nodes are
  requested again and put back every frame, except that a keyword node whose content did not change is
  not measured again, so that what holds it keeps its `layout_generation`.
- **Collection.** Nodes are freed in `end_frame`, at the end of a presented draw, by reachability from
  the roots laid out in that pass (a mark in the taffy node, no map). A pass that ends without a frame
  (`end_pass`: a layout-only read, an abandoned container-query pass) collects only when the engine holds
  more than twice the nodes the last collection left plus 4096. `calc()` values are interned by value
  and swept when they outnumber what the last sweep left.
- **Vendored taffy patches** (each marked `fdarian fork patch` in the source): the fully associative
  measure cache with its key and its margin-carrying size entries (#36); `set_style` to an equal style is
  a no-op; `remove` and `clear` drop the node context (a leak before); `set_children` tolerates parent
  pointers that no longer match the child lists, in both directions, and adopting a child unlinks it
  from its old parent; `mark_dirty` walks to the root, because a node under `display: none` stores no
  cache and stopping at an empty cache left stale layouts above it; a `dirty_generation` per node, and a
  `layout_generation` that also moves when the node is laid out with another size, padding, border,
  scrollbar or content size (a percentage padding changes the inside of a node whose box stays);
  `mark_reached` and `unreached_nodes` for collection; `RunMode` passed to measure functions; the
  block-container scroll height fix.
- **Guard.** `engine_fuzz_tests.rs` lays out random edit sequences in an engine kept across frames and
  in a fresh one and compares every node. Seeds 1 to 10000 pass; the suite runs 80 and
  `GPUI_ENGINE_FUZZ_SEEDS=first..last cargo test --release -p gpui --lib --features test-support --
  --ignored a_long_run_of_edits` runs any range.

#### Layer 2: `gpui::retained`

zed `elements/retained.rs` and `window/retained.rs`. `retained(id, key, build)` wraps a subtree with a
key its owner computes from everything the subtree is built from. The owner promises that equal keys
mean the subtree draws, lays out and reacts the same, including what its elements read without being
passed it (hover, focus, running animations). gpui checks only what it owns.

A frame whose key is the one the frame before recorded does not call `build`. It lays out the nodes the
engine kept (the retained element numbers its requests under a scope of its own, so id-less siblings
after it keep their ordinals, and it keeps the element states of everything inside it, see
"What a replay costs" below). After layout it
replays the recorded prepaint and paint if the surroundings are the recorded ones, and otherwise builds
the subtree then, in prepaint, onto the nodes it already has.

- **What a replay puts back**: hitboxes and tab stops (by value), the dispatch tree, focus, tooltips,
  line layouts, element states, the entities the subtree read, accessibility nodes (with relations, focus
  ids, bounds and action listeners), the scene, listeners, cursor styles and input handlers.
- **What must match to replay** (`ReplayKey`): the subtree's bounds (so its origin), its unsnapped size
  and the sub-pixel phase of its unsnapped origin, the content mask and exact clips, text style,
  visibility, element opacity, rem size, scale factor, viewport size, the positioned frame and element
  offset, the current view and image cache, whether a11y is active, whether culling and lifting are on, DOM
  hit testing, and how many of the contexts around the subtree are
  stacking contexts and how many of the elements around it are handed to one (a subtree is not replayed
  after an ancestor starts to be painted from its stacking context). And the root's `layout_generation` is
  the one recorded. A mismatch builds the subtree lazily; if its layout requests do not land on the nodes
  it has (nodes made, styles or children set), it is laid out as a root of its own and not recorded
  (`LayoutMismatch`). The size and the phase are in the key because the boxes inside snap edge by edge
  from the root's origin: a root that keeps its snapped box while its origin settles at another fraction
  (a sibling that animates to a width in percent) has them at other sizes. Two phases are equal within
  1/256 of a device pixel, on the circle (0.999 and 0.001 are close): above the
  rounding of the `f32` sum of offsets that gives the origin, below the 1/64 that sizes measured from text
  are quantized to.
- **What stops a frame from skipping at all** (`request_layout`): a draw pass above 1 (container queries
  draw several), a measuring pass that is not layout-only, the inspector, `Window::refresh` (which
  `set_rem_size` calls when the size changes), an entity the subtree read being notified (the view
  rendering it excepted, since its notify is what draws the frame), a different engine or a dead root
  node, no recording painted in the frame that is rendered (and, for a subtree that holds holes or
  fragments, none of them painted in it), or a text style, rem size or scale factor around the subtree
  that is not the recorded one. The last is checked here and not only in prepaint because the text style
  is not in gpuix's key (`white-space`, `text-wrap-*`, `text-box-edge` and the rest reach a text through
  gpui's inherited `TextStyle`), and a text that wraps differently moves the boxes that layout has placed
  by the old size by the time prepaint could build it.
- **What makes a recording non-reusable** (`NonReusable`, counted by `getRetainedCounters`): a box lifted
  into a stacking context that holds neither a hole nor a fragment for it (`Lift`); a box drawn outside
  its clip (`Escape`); a deferred draw; a nested layout tree (a virtual list's rows); layout nodes that
  are new every frame or rewritten (`UnstableLayout`); a root with no layout result; `LayoutMismatch`; a
  redraw request, an autoscroll request, a frame callback, an asset still loading (including image
  caches and custom sources), a window control area; a cached view drawn from its last frame
  (`CachedView`); and a paint listener that reports bounds ran with no `on_replay` hook on the element
  (`Publication`).
- **Requirements on the caller.** The element needs an element id of its own that is unique among the
  elements around it; it is not on the id stack, so wrapping a subtree changes no id below it. The root
  of the subtree must not escape its clip. If it is lifted, say so with `lift_z`; `cull_outset` forwards
  how far the subtree inks, for culling.

**Boxes handed to a stacking context: holes and fragments.** gpui lifts a positioned element out of
its parent into the nearest stacking context, which paints it in z order with the others, so the paint
of a root is not one run and a root that lifts something is not replayable by copying its run. Three
rules decide what a recording does about the boxes it hands over:

- **Lifts that stay inside the recording are not lifts.** The recording takes a mark of the ambient
  lift list when it starts and, when it ends, looks only at the entries it added to it, which are the
  boxes that left the recording for a context outside. A stacking context root whose positioned
  descendants are all consumed by it adds none, and replays.
- **A hole** is a lifted element that is a retained root of its own (`retained(..).lift_z(z)`). The
  recording of the root around it keeps a stub for it (its id, z, its ordinal relative to the first box
  the recording handed over, its hitboxes by value, mask, paint context and image cache), and a replay
  reserves ordinals in the live lift list, pushes one entry per stub and has the context paint the hole
  from its own recording. The hole is not asked for alone: the key of the root around it holds the hole's
  token, so a hole that changes or animates builds the root around it (it stays on the spine). A hole
  that read entities replays only while they are unchanged, and observes them again.
- **A fragment** is any other lifted element, such as an `absolute` box placed by a positioned box inside
  the root. While a recording is open, the context records what it paints for the element into a cell
  (`PlainPaint`: the paint record, the retained subtrees nested in it and the frame serial) shared by
  every recording open around it (the *owners*: the open recordings whose lift list is the one the
  element joined). A replay of an owner puts the cell's record at the live ordinal. A cell whose paint is
  missing or from another frame counts as a missing hole.

Both bracket what they paint with `RetainedPhase::LiftedBegin(rel)` and `LiftedEnd` for each owner's
hook, and a replay gives the owner `LiftedPaint(rel)` where the context paints the element (after the
owner's `Paint` for `z >= 0`, before it for `z < 0`), which is how gpuix's tape plays the writes the
context made for it. A pass that only lays out gives a hole `LayoutPaint` and the owner `Layout`. A
recording that holds holes or fragments is never moved by a scroller (`NonRelocatable::Hole`). An
ambient lift that is neither is `NonReusable::Lift`.

An `absolute` box whose containing block is inside the subtree used to make every such subtree unstable,
because the engine rewrites an absolute box against its containing block after layout. The engine now
counts it unstable only when the containing block lies outside the retained subtree
(`escapes_subtree`), and gpuix's eligibility is the other half: a root holding an `absolute` box whose
containing block is above the root is not eligible.

**Moving a recording: translated replay.** A scroll moves the content of a scroller by a vector, and a
recording of the content can be put there instead of built again. `retained(..).relocatable(overscan)`
asks for that:

- **Recording.** The subtree is prepainted and painted into a region of its own, the content mask
  around it inflated by `overscan` on each side, into a recorder scene that keeps the operations. It
  emits them into the frame through `Scene::replay_translated(ops, Δ, clip)`, and a build emits with
  Δ = 0 through the same path, so the verifier compares like with like. Culling runs against the region,
  so what the overscan holds is recorded but not painted before it shows. A subtree that is culled where
  it stands is recorded under the empty mask with no region, and can be put anywhere that is culled too.
- **Δ is exact.** It is the difference of the scroll offsets as they are painted at, snapped to device
  pixels; it is not what `scrollTop` reads. `Window::layout_bounds` and `layout_unsnapped_bounds` both add
  the snapped offset, so the bounds an owner keeps are the ones a build reports under a fractional scroll.
  An atomic inline's origin is an element offset too, rounded half up to a device pixel; its fraction is
  kept beside the offset (`element_origin_phase`) and only `layout_unsnapped_bounds` adds it back, so a move
  by whole device pixels keeps it, as it keeps the phase in the key.
  The rounding of painted geometry (box edges, underline origins, glyph subpixel quantisation, emoji
  and svg sprites) was made translation-covariant: a shift by whole device pixels moves the result by
  that shift, with the bytes of positive results unchanged. That is exact only at a scale factor that is a
  power of two (1, 2, 4), where a position shifted by whole device pixels is exactly the shifted
  position, so any other scale factor does not relocate.
- **When it is legal** (`ReplayKey::relocation`): every field of the replay key equal but the origin,
  the content mask, the exact clips and the positioned frame (the unsnapped size and the phase are
  equal: a move by whole device pixels keeps the phase, and with it the side every edge inside snaps
  to); the origin differs by exactly Δ; the
  engine's bounds before the offset and the `layout_generation` are the recorded ones; the region shifted by Δ still
  covers the clip around the subtree now; and each rounded clip inside the subtree meets the clip around
  it as it did (`translated(Δ).intersection_is_exact(ambient)`), because painting cuts a rounded corner
  that a clip crosses in a way a shifted replay does not. The misses are counted as
  `relocationMisses` (`Place`, `Region`, `Clip`). A rounded row that the edge of its scroller cuts is
  built.
- **What moves by value**: scene operations (sprites also move their `translation`), hitboxes, accessibility
  node bounds, and the gpuix tape's geometry writes (each entry is the effect and the offset it was made
  at). **What moves through a handle**: a `TextLayout`'s bounds and a `ScrollHandle`'s bounds, which a
  recording registers with `Window::note_relocated(apply, describe)`; `apply` sets the handle to its
  recorded value plus Δ, so a replay is idempotent. **What has no geometry** and is put back as it is:
  cursors, tab stops, dispatch, focus, line layouts and element states.
- **Never moved** (`NonRelocatable`): a scale factor that is not a power of two, a hole, `position:
  sticky`, an escape to a containing block outside the subtree, a position kept past the frame
  (`Window::note_positioned_capture()`: a drag, a keyboard click, a tooltip's bounds test; a `Canvas`
  says `keeps_no_position()` when its closures keep none), an input handler, a scrolled scroller inside,
  a listener that reports to its owner when the owner does not take the translation
  (`on_replay_translated`), and a rounded clip cut by the clip around. Deferred draws and virtual lists
  are not reusable at all. Two relocatable subtrees do not nest: the inner one is built every frame that
  builds the outer.

**What a replay costs.** It is a step per root, not per element inside it
([#63](#63-content-nobody-sees-costs-the-page-nothing) has the numbers):

- A recording names the retained roots inside it by the id their element state is stored under
  (`RetainedState` keeps the id of the first map the state was stored with), so a replay finds them by pointer
  and not by comparing ids element by element.
- Element states are one map of the window that outlives the frames (`Window::element_states`), each stamped
  with the pass that last accessed it and with the retained root that was being built, prepainted or painted
  when it did (`StateOwner`). A frame that is presented drops the states that it neither accessed nor kept
  (`release_unused_element_states`): a subtree that layout skips marks its owner kept, so a replay copies none
  of the states of its range to the frame. A state lives as long as its root is replayed or built; a root that
  leaves the page loses its states and starts over when it comes back. Ranges copied by anything else (a cached
  view, a deferred draw) stamp the states they name.
- A recording has an `Origin` that says where its prepaint and its paint started and in which frame. The
  origin of a recording made or replayed inside another is placed within the outer one's, relative to where
  that one started, so a replay places its own origin and the roots inside it follow
  (`PrepaintRecord::range_now`, `Recorded::is_placed_in`): a replay does not visit the roots inside the root
  it replays. A rolled-back prepaint (`Window::transact`) drops the placement of the recordings it made.
- A dispatch node is small and shared: its listeners and key context are `NodeExtras` behind an `Rc`, and
  `DispatchTree::reuse_subtree` copies the nodes of a range in one pass, the parent of each worked out from its
  place in the range, taking the extras by pointer from the tree of the frame before (a node that gets a
  listener or a context after it was shared copies them first). A range with a node that names a view is still
  copied a node at a time.

**Patching a frame.** A frame that draws is a new `Frame` and copies every root it replays, so it costs the
page. A tick that changes only how some roots paint repaints those roots in the frame that is shown and leaves
the rest as it is ([#62](#62-one-loop-costs-the-page)):

- **`retained(..).patchable()`** makes a root leave a `PatchUnit` in a per-frame map keyed by its id when it
  records and its recording is plain: not relocatable, not culled. The unit holds what the replay key lacks of
  the surroundings: the id stack, text styles, rem size, view, content mask, exact clips, offset, opacity,
  positioned frame, image cache and stacking contexts. A root that is lifted into a stacking context
  (`lift_z`, a hole), or prepainted inside a lift that is open (a positioned or translucent box around it),
  keeps a second set, captured when the context paints it (`Placement::Deferred { paint }`; `InPlace` when
  no lift is open). A root placed by a containing block outside it is recorded as a source for a patch
  only, never skipped (`Recorded::root_escapes`), see [#75](#75-a-loop-under-a-positioned-or-translucent-ancestor-or-on-an-absolute-element-is-a-patch).
- **`Window::request_tick`** asks for a draw with no view queued and no layout invalidated. A draw for a tick
  first asks the hook of `Window::set_frame_patcher`, which builds the roots the tick changes and calls
  `Window::repaint_in_place(units, cx, accept)`. That restores the surroundings of each unit, lays the new
  element out onto the nodes the recording has (the counters of a build that is skipped must not move),
  prepaints and paints it into a scratch frame with the recorded hitbox ids handed out in order, and compares
  the result with the recording: the replay key, hitboxes by value, cursors, tab stops, listener, input
  handler, tooltip and dispatch node counts, and the kinds of the operations. Then `accept` runs (the embedder
  compares its tape). On success the operations of the rendered scene are overwritten in place,
  `Scene::rebuild_from_operations` makes its primitives again from the log, and the frame is presented only
  if an operation changed. Any failure puts the element states back, ends the layout pass and declines
  (`PatchDecline`: pending view, refresh, unknown unit, stale recording, surroundings, layout mismatch,
  unstable layout, effects, hitboxes, registrations, operations, lifts, rejected), and gpui draws the tick whole in
  the same draw.
- **Counters.** `Window::patch_counters` reports patched frames, patched units and the declines by reason.
  `frame_work_counters` counts the elements and primitives the patch painted.

**Verify.** `Window::set_retention_verifier` builds every frame that replayed once more with every
subtree built, and compares per replayed root the scene primitives, hitboxes, cursors, listener
counts, tab stops and accessibility nodes, plus whatever text the embedder's `describe` returns (gpuix
returns its tapes, and the handles a recording registered). Both sides of a root are put in one form
first: draw orders, and groups and masks that hold nothing, are stripped (including the widenings in the
text of a lifted element, which sits on a line of its own), and when either side was recorded in a
region both are compared without what only a region has. It also compares the frame as a whole (the
order of the scene, the hitboxes in hit-test order, the tab order and cursors, the dispatch tree and the
accessibility tree), because per-root comparison with the draw order stripped cannot see an error in the
order between roots, which is what a hole or a fragment can get wrong. The frame shown is the one built
whole. Embedders whose build is not identical twice within a frame turn the whole-frame comparison off
with `Window::set_retention_whole_frame(false)`: gpuix does, since `anchor-size()` reads the size the
trigger painted a pass ago, so outside the subtrees that replayed the two passes differ. The per-root
comparison stays on. gpuix reads the clock once per pass, and the reference pass takes the instant of the
pass it is compared with (`Window::is_retention_reference`): on a running clock it would be a pass ahead,
and a root that something animating places would differ from its build by the distance that moved.
A patch is verified the same way: with a verifier set, a patched draw is followed by a frame built whole at the
instant of the patch, and the operations of each patched root are compared with the patched ones (a difference
is a `[retained]` mismatch). That frame is the one shown, so the next tick patches against it.

#### Layer 3: gpuix decides what to skip (`retain.rs`, `tape.rs`)

**Entry key.** `Retain::enter` runs for every element before it is built. The key has three parts:

- the element's `subtree_revision`, which every mutation to the element or anything below it moves (an
  event listener change too);
- a fingerprint of `Inherited` hashed by content, not by pointer: group, container and highlight
  scopes are fresh `Arc`s every frame. A field added to `Inherited` must be added to
  `inherited_fingerprint`; a size test fails when one is missing;
- the shape: the element's id, its index among its siblings, its parent's shape, whether it is a
  stacking root or item, an atomic-inline root, the `display: contents` elements it was spliced out of
  and the style each had when it last changed (`contents::overlay_key`), an anchor scope, the
  `text-box-trim` an ancestor hands down, the size layout last gave it if it is a query container or
  has `@container` blocks (`ContainerStore::key`; the sizes of the containers above it are in the
  `Inherited` fingerprint), plus words the build adds as it reads things the tree cannot show: the
  accessibility links an element reads (`links_key`: `<label>`, `aria-*` references), the length of the
  click path, a custom element's `retain_state`.

**What the key cannot see is volatile**: pointer and focus state, scroll offsets, measurements, the
selection, running motion, transitions and animations. The build notes what it read (`pointer`,
`focus`, `scroll`, `measurement`, `text`, `dynamic`) into a `Vol` kept with the key in the element's
*record*. The record also holds the subtree's size, its lift, its culling reach and its build effects. A
skipped element does not enter its children, so what a build folds up from them (epoch, token, holes and
whether it lifts) is taken from its record: without `Record::lifts` a root with a skipped child that lifts
would be recorded as lifting nothing, be relocatable the frame after and be built inline then.

**Frame deltas and the spine.** `find_deltas` compares the sources with the frame before: hover and
active chains (and the target each moved to or from, for `:has()`), the focus-within chain, the
`:focus-visible` element and the keyboard flag, the dim path, scroll offsets, custom `retain_state`
values, motions running now or in the frame before, elements whose measurement or links changed, and
the elements whose last build was dynamic. Those elements and all their ancestors are the *spine*: the
upward closure of the changed ids and `unbuilt` (below). A root is skipped when it is off the spine, so an
element on it whose parent is not would be replayed with it frozen; under verify `find_deltas` counts each
one as a mismatch (`[retain] element N is on the spine and its parent M is not`).
A new selection identity, or a frame with nothing known from before, sets `all`: nothing is skipped.
`find_deltas` is `compute_deltas`, which changes nothing and has peeks of what a frame takes
(`Measurements::peek_changed`, `links::peek_changed`, `peek_commanded`), and `commit_deltas`, so a tick can
ask what would have changed without taking it. An element whose animations only change what it paints and that
the last frame walked culled is *dormant* and is left out of `changed` ([#63](#63-content-nobody-sees-costs-the-page-nothing)).

A pass that only lays out is a frame to `find_deltas` (it updates `seen` and drains
`Measurements::take_changed` and `links::take_changed`) but does not visit an element under `display: none`.
So the spine is not forgotten when it is not built: `Retain::unbuilt` carries every spine element a pass did
not build (it is removed in `finish_session` when the element is built, and with the destroyed elements in
`prune`), and the next `find_deltas` walks up from it with the changed ids. It carries only elements a
build can enter. A `display: contents` element (but its tree's root, which builds as a host) is spliced
out of its parent, an `<svg>` paints what is below it as one image, and an inline span or text is a part of
its container's paragraph (`RetainedElement::is_built_by_ancestor`): no build enters them, so they would
stay in `unbuilt` for good and keep their ancestors on the spine. The spine is a set of its own, closed
under parent, which is what makes the walk right in stopping at the first id already in it; seeding it
with `unbuilt` as it stands would not be, since the built ancestors of an unbuilt element have left it.
A new selection reaches an element the same
way: a `Record` keeps the selection it was built under and `skippable` compares it, since `all` is a
property of the pass and not of the elements it visited. Guards: `layout-pass-deltas.test.tsx` (hover), the
unit tests of `retain.rs` (active, focus, measurement, links, selection, the spine and what `unbuilt`
carries, the lifts of a skipped root), and for a motion that runs where the way up to a replayed root is
never built (below a `display: contents` wrapper, inside an `<svg>`, in an inline span): `retained-spine`,
`retained-frame-sources` (what else changes per frame without a prop, three ways around a wrapper) and
solid's `retained-cull-wrapper` (the shape of the Fluid Functionalism demo cards).

**When a frame skips an element.** All of these:

1. planning is on and it is not `all`;
2. the element has a layout node of its own (`boxed`: table rows, row groups and columns do not) and is
   built in a plain context (no baseline probe, inline line, document host list, containing-height scope,
   atomic extraction); the root of a subtree that gpui asked to have built late is built, though
   elements below it may be skipped;
3. nothing is excluded (below), it is not on the spine and no scroller above it moved, except as a
   root that gpui moves with the scroller (below);
4. its last build left a record, the record says eligible, it is a root worth the replay, and the key it
   was entered with is equal to the key the record was entered with.

**A root worth the replay (`MIN_ROOT`).** Replaying a root has a cost of its own, about what building a
plain box costs: 2000 childless boxes in a scroller that nothing changed took 1.24 to 1.31 times as long
with retention on as off when every box was a root. So a root is skipped when it has at least `MIN_ROOT`
(2) elements, or when it is one element that is costly to build: it carries text, or is an image, an
`<svg>`, a `<text>` or a custom element. With the rule a scroller of plain, rounded or shadowed boxes
costs 1.00 times what it costs with retention off, and a scroller of texts 0.4. Measured on the pages
of the fluid-functionalism demo, the texts and icons that are roots of one element are 0.5 to 0.8 ms of
the 3 to 6 ms an idle frame costs, which is why the exception exists. A verified run takes every root, and
`GPUIX_RETAIN_MIN_ROOT=1` takes every box, to measure.

A skipped element becomes `retained_root`: `gpui::retained(token, build_subtree)` with an `on_replay`
hook. The token is a hash of the entry key, the record's epoch and the tokens of the holes in it, so
rebuilding an element on its own changes its token and the recording of that build is not mistaken for
another. Closing over a `FrameEnv` (inherited state, document path, clock, focus styles, entry key) lets
`build_subtree` build the element later, if gpui cannot replay it, exactly as the frame would have.

**Units.** A subtree that only a running paint-only animation changes is built whole as a *unit*, so that a
tick can build it alone and have gpui paint it again in the frame shown ([#62](#62-one-loop-costs-the-page)).
`Retain::begin_unit` decides at `enter` from the record of the build before: everything that runs on the
element paints only (`Record::paints_only`, noted by `runs_paint_only`), it is eligible, not dormant, built in
a plain context and holds at most `UNIT_MAX_ELEMENTS` (256) elements, and nothing inside it is skipped
(`Kept::InUnit`). `Retain::end_unit` confirms from the build: no exclusion, no scroller, no absolute
descendant placed against a block outside it (the root itself may be absolute: its own containing block is
outside by definition), not absent, and nothing lifted out of it unless the element is a stacking context
that takes it in (an effect group, a positioned element with a `z-index`). An element that is no unit says
why in `Record::refusal` (`Retain::refusal`), which the preview turns into the `unpatchable` key. The outermost such element on a
path is wrapped as `gpui::retained(id, key of this frame, prebuilt).patchable()`, with `.lift_z(z)` when it is
lifted (`unit_root`) and the `Taped` and `on_replay_translated` wiring of a `retained_root`, and keeps its
`FrameEnv` in `Retain::units()`. A layout-only pass and a deferred build make none; the reference pass of a
verified frame does, since after a patch that pass is the frame shown. An element over the cap is built
whole as before and counted (`unitsOversized`).

**The patcher** (`retain/patch.rs`) is installed once per window from `render`. For a tick it takes `now` from
the clock and asks `Retain::preview`, which changes nothing: since the last frame built whole, only time-driven
ids changed (running `dynamic`, motion that runs or ended, not yet built), each in a unit or dormant, the root
revision is the same, no layout-only pass ran, no scroll animation or scrollbar fade is active, and that frame
built no element that follows the clock (`Exclusion::varies_with_time`). The reasons it says no are
`Unpatchable`: `NoFrame`, `LayoutPass`, `Unsteady`, `ScrollAnimation`, `Edited`, `Everything`, `Sourced` (the
pointer, focus, a scroll, a custom element, a measurement, a link or a command changed), `Outside` (a
time-driven change outside every unit that is not dormant, on an element that has a box and the last build gives no reason for),
`Refused` with the reason the element is no unit (`Excluded:<kind>`, `ExcludedBelow:<kind>`, `Escapes`,
`Lifts`, `Scrolls`, `LayoutAnimation`, `TooLarge`) and `Ended` (a unit whose element the last
frame no longer found a paint-only animation running on, though a source still lists it, as an SVG shape
that holds its end value is: the frame built whole would not make it a unit, so it is not repainted as
one). Then it steps what a frame steps before it builds
(`sync_motion_states`, `sync_motion_values`, the clocks of the dormant animations, their events) and builds each
unit with `GpuixView::build_unit` from its kept env at `now`, with the retention closed (nothing entered,
recorded or tapped, `BUILD_EFFECTS` truncated, events drained, `built_at` set, no frame asked for), inside a
tape that captures: `Tape::capturing` notes the writes of a prepaint and paint without making them. gpui's
`accept` is `Tapes::same_as_recorded` for each scratch tape. On success it takes the active flags and asks for
the next tick; on any decline it returns false and gpui draws the tick whole in the same draw. The clocks were
sampled at an instant and not stepped, so that frame sees the state it would have, and each event was emitted
once. The frame that ends a build asks for the next one with `request_next_frame`: `on_next_frame(request_tick)`
when units were made (`Retain::has_units`) and patching is on, `request_animation_frame` otherwise or while a
scroll animation runs. Patching is off with the retention, and with `GPUIX_PATCH_OFF` or `setPatchEnabled(false)`.

**Eligibility and exclusions.** After a build, an element is eligible when nothing in it is excluded
and no `absolute` box in it has its containing block above it (`Below::escapes` is folded up from each
absolute box and cleared by a positioned box). An element that is itself `absolute` is not eligible for a
skip, since gpui rewrites its layout against a block outside it, but it may be a unit (Round 14a). What it lifts into the stacking context around it does
not matter: a lifted element that is a root of its own is a hole in the recording of the root around it
(`retained(..).lift_z(z)`, never `relocatable`) and any other is a fragment of it (Layer 2). Container queries do not exclude: the
size a container was last laid out at is in its key, the sizes of the containers above in the fingerprint,
and the axes that a skipped reader read are a build effect. The children spliced out of a `display:
contents` element are retained, with the wrappers' style in their shape; a subtree gpui builds late under
such a root keeps the style the wrappers hand down.

Excluded (`Exclusion`): a paint reporter (`onScroll` on an element that scrolls; `onLayout`,
`ResizeObserver` and `IntersectionObserver` are not one, see below); a `highlight`; a custom element that
has no `retain_state` (`<code>`, `<markdown>` and `<diff>` hold scrollers whose offset only gpui knows; an
`<svg>` whose raster is not settled). `<input>` and `<textarea>` have one, an epoch of everything but their
blink, and are retained ([#74](#74-an-input-is-retained-and-its-caret-is-a-tick)); a
virtual list and the rows it builds; a table with collapsed borders; a `display: contents` element built as
an element of its own; `<anchored>`; `position: sticky` or `fixed`; a width keyword or a natural width
(`UnstableLayout`); an `overflow: scroll` or `auto` element (a scroller is a boundary, its children are
the roots); a `layout` (FLIP) element; an atomic inline. A running `<svg>` is rasterised in the paint
whatever its size, so it is a unit ([the `<svg>` item of Round 14a](#an-svg-is-culled-like-a-box-and-its-loops-sleep-out-of-view-and-patch-in-view));
`Exclusion::Svg` is left for the one frame before a paint that would start a raster job
([the showcase item](#ffs-showcase-an-svg-waiting-for-its-raster-and-motion-under-display-none) of Round 14a).
`overflow: hidden`, labels, aria links,
settled `<img>` and `<svg>`, `onLayout` and observers, container queries, and lifted and `absolute` boxes
that a root contains are not excluded: their scroll offset, links and state are in the key or the record,
or the tape republishes what they report.

**Observers and `onLayout` are read from the published boxes.** An element with `onLayout`, a
`ResizeObserver` or an `IntersectionObserver` has no paint listener of its own. `observers::frame_sweep`
runs after each painted frame (a layout-only pass reports nothing, as a forced layout delivers no `ResizeObserver`
callbacks in a browser, and the frame that follows reports the change against what was last delivered) and reads the
box of every such element from the registry `automation::get_bounds` serves: the border box, the border
and padding (`ElementBounds`), and for an intersection the boxes of its clipping ancestors. A replayed
subtree republishes the same boxes (see `tape`), so an observed element can be replayed, and the report is
the one a build gives: same draw, same entries, one event per change. The targets come from
`RetainedTree::geometry_ids`, which holds the elements with any of the three listeners, custom elements
included; their clipping ancestors are computed from the tree when its revision moves. An inline span
paints in its paragraph's container, which the build that made the paragraph notes
(`ObserverReports::note_clip_origin`). Guards: `retained-observers.test.tsx`.

**The order of a frame's reports** is the DOM's rendering steps, a decision (the paint listeners of poc
gave `onLayout` before `onScroll` for one element; here the order is stated, not an effect of how many
times each was deferred). Scroll events come first, then `onLayout` and `ResizeObserver` (gpuix's
`onLayout` is `ResizeObserver` for an element, so it is delivered with it), then
`IntersectionObserver`, as in "update the rendering": run the scroll steps, then the resize observation
loop, then the intersection observation steps. `report_scroll` queues into `PaintReports::pending_scrolls`
at paint, in paint order (the DOM's is the order the scrolls happened in, which is the same for the
scroller one input moves); `observers::sweep` drains the queue first and a deferred flush drains it when
nothing listens for boxes. The sweep emits `onLayout` and resize reports in document order (`onLayout`
before the resize report of one element), then intersection reports, and an element that is gone after the
ones that are there of its phase. The callbacks of the observers are ordered on the JS side
(`js/observers.ts`) as the specs do: the "gather active observations" loop walks the document's observers in
creation order and each observer's targets in the order `observe()` added them (a `ResizeObserver`
re-observing a target moves it to the end, `IntersectionObserver.observe` of a target it has changes
nothing); every resize callback runs before any intersection callback. Pinned by
`frame-event-order.test.tsx`. Two frames merged into one delivery keep the entries of one target in the
order they arose.

**An element that was removed is reported gone by the next presented frame**, not by a layout read, and
this is kept on purpose: in the DOM a forced layout (`getBoundingClientRect`) does not deliver
observations, which wait for the next rendering step. A bounds read in poc drew, and so reported; it
is a layout-only pass here, which reports nothing, neither an absence nor what is there (an earlier
version reported what was there, so a `scrollTop` write after a style change delivered `onLayout` ahead of
the scroll event of its frame; Chrome delivers the scroll, then the resize observation, and a forced layout
delivers none: `fixtures/scroll-order-oracle.json`). `sweep` runs after painted frames only and compares
against the sizes last delivered, which a layout-only pass does not move.
`observer-gone-reports.test.tsx` and `forced-layout.test.tsx` pin both halves.

**The effect tape.** A replay puts the scene back but runs none of the listeners and canvases that
write gpuix's own per-frame registries, and gpuix resets them at the top of every painted frame. Every
write that a retained subtree has to repeat goes through `tape::write` (or `tape::write_in_layout`, for
registries a layout-only pass writes too): bounds, selection and start regions, the painted-text log,
inline hit paragraphs, `Measurements::by_element`. A `Taped` element around the root of the subtree
records the writes of its prepaint and paint. An entry is the write and the offset it was made at, so a
moved replay (`on_replay_translated`) shifts the geometry it holds (`ElementBounds::translated`, the
start regions).

A replay does not write the tape into the registries. It tells them which lists of writes it puts back
(`tape::publish` with a `Live`: the list, how far it is moved, whether a pass that only lays out makes it),
and they read it when something asks, so a replay costs the same whatever the subtree holds
([#63](#63-content-nobody-sees-costs-the-page-nothing)). A tape is an `Rc<Entries>` per phase, a list that is
never changed once recorded (a new recording starts a list of its own, so a replay that was not read yet
keeps what the subtree wrote then), with writes of a kind (`tape::Kind`) and nested lists by reference: a
write belongs to the innermost recording, the subtree around it holds the finished list as
`Entry::Nested(list, by)`, and a subtree replayed inside a recorded one is nested the same way, so recording a
replay is O(1) too. The registries that keep an order (`text::paint::Frame::{registry, start_regions,
painted}`, `inline_hit::PARAGRAPHS`) are `tape::Layered`: a write is a slot and a replay is a slot, and the
first read after them expands the replays in place, in the order they came, and keeps the result until the
next write. The painted boxes (`automation::BOUNDS`) answer by element: a write notes the serial of the list
it was recorded into (`tape::owner`), and a box is the one the frame wrote or, from `Stored::recorded`, the
one a live list recorded moved by its delta at read. Marking a replay live is one insert whatever is nested in
it: a list notes the list it was last nested in and by how much (`tape::PARENTS`), and a box is looked up by
going up from the list it was recorded into until a live replay is found (`tape::delta_of`). A frame reset
bumps a counter and clears the live set instead of walking the map, and stale boxes are purged when the map
doubles. `Measurements::by_element` is not reset by a frame, so a replay leaves it as the build wrote it
(`Kind::Measured` stays on the tape for `same_writes` and the verified frame). There is no eager apply of a
relocated replay: the delta is applied at read.

What the stacking context paints for an element the root handed to it is outside the paint of the root:
gpui tells the hook `LiftedBegin(rel)` and `LiftedEnd` around the paint of the frame that builds it, and
the tape keeps those writes in a slice by `rel` (`Phase::Lifted`); a replay of the root gets
`LiftedPaint(rel)` where the context paints the element and plays the slice. A hole plays its own tape
(`Paint`, or `LayoutPaint` in a pass that only lays out), and the root around it plays the writes of its
prepaint. A pass that only lays out plays the in-layout writes of the slices too. The one write that
depends on what is outside the root is the parent content box in the measurement of the root itself (a
percentage that meets `auto` in motion resolves against it, and a sibling or the parent's padding changes
it with the root's box and key as they were): the hook republishes it from the current layout
(`Window::replayed_root`, `Measurements::republish_parent_content`) before it publishes the tape, and a change
marks the root's measurement changed, so the next frame builds it as it would have.

Effects of *building*, not of painting, are `BuildEffect`s kept with the record and applied when a frame
skips the element: marking elements as measured (`Measurements::wrapped`), asking for background images
(`background_images::REQUESTS`) and the axes of a query container that a style read (`ContainerRead`).
The last is what keeps a container honest: after layout it compares its content box with the one the
build assumed, for the axes something read, and asks for another pass if they differ; a skipped reader
has to say what it read, on the container's scope of this frame, or a container that is resized with the
reader's box unchanged would not ask. A reader inside the container's own subtree has no scope to say it
to, and does not need to, since its layout did not change.

**Scroll back-off.** Recording a frame costs about 5% of building it. A page whose frames find nothing a
later frame could skip (a scroller whose rows move every frame, boxes too small to be a subtree) only
pays that. After a recorded frame that found no skippable element, the next 1, 2, 4 and 8 frames (capped
at 8) are built unrecorded, and one frame in nine looks again. Recording starts again at once when the
scrollers stop moving or the element count changes. `GPUIX_RETAIN_VERIFY` turns the back-off off. A
scroller whose roots gpui can put where the scroll moved them (below) is not a page with nothing to skip,
and is recorded every frame.

**Roots under a scroller move with it.** A root of at most two viewports (per axis, by its last painted
box) inside a scroller, or a clip that script scrolled, is handed to gpui as
`retained(..).relocatable(overscan)`, with the overscan half a viewport of the scroller (`Scroller` in
`retain.rs`); what gpui does with it is under Layer 2. A box that is bigger than two viewports is built
and its children are the roots, so a scroller's single content wrapper is not one giant root. The scroller
that moved is on the spine, but what is below it is not built when its roots are moved: `moved` rows are a
loss only when gpui builds them anyway. `Retain::relocation` compares `lazy_builds` with `relocations`
per frame and stops planning on moves for 1, 2, 4 and 8 frames when the builds are four times the moves.
Measured, test renderer, release, one scroll per frame, 200 cards in a 400x300 scroller: 5.1 ms with
retention off, 1.0 ms with it on at 3 px a frame (0.19), 0.35 of the frame at 400 px and 0.5 at 2000 px.

#### Invalidation rules

What follows is the contract between a build and the skipping. Every row is a way for the frame to
stay correct; a build input that is in none of them is a hole, and verify mode is how holes are found.

**What changes the entry key, so the element is built.**

- A mutation to the element or anything below it (`subtree_revision`), including attributes, styles,
  text, children and event listeners.
- A change in anything `Inherited` carries that a descendant reads: colours (only for a root that read a
  colour above it, `Record::reads_colors`; see the retention item of Round 14a), lengths and font state,
  selectability, `inert`, `pointer-events: none`, table settings, container and group scopes with their
  hover, active and focus-within state, the highlight declaration, `appRegion`, `frameless`, the
  underline offset, the `text-box-trim` handed down, `direction` with the `text-align` it resolves
  (and what the parent's text style already resolved it to), and the custom properties (`--x`) that
  `style()` queries read, hashed in sorted order.
- A Web Animations command on the element since the frame before (`Sources::commanded`): `play()` on a
  transition that had finished, a seek, a rate change. Nothing in the last build says that a run which had ended
  can move again.
- A change in the element's shape: its place among its siblings, its parent's shape, being a stacking
  root or item, an accessibility link, the length of the click path, the size of a query container, the
  `display: contents` wrappers it was spliced out of, the `retain_state` of a custom element.

**Fields of the parity pass, and why each is covered.** Pinned by `retained-parity.test.tsx`, run with
and without `GPUIX_RETAIN_VERIFY=1`:

- An own, non-inherited style field (`mask-border-*`, the `flex-basis` keywords, `direction` on the element,
  `:disabled` and the other state styles' bodies, custom properties on a state style) is in the revision and
  `StyleDesc` equality. A state style that carries `--x` reaches the children through `Inherited::custom`, and a
  hover on the element is a pointer delta.
- `:disabled`, `:enabled`, `:has(:disabled)` and `:has(:enabled)` read the tree (`RetainedElement::control_disabled`,
  `ControlStyles`, which is worked out once per frame). A flip is a prop mutation, which moves the revision of the
  control and of every ancestor, which are exactly the elements `:has()` can name, so there is no delta. Their
  `PointerStates` flags have bits in `pointer_bits`, which only attributes a rebuild in the counters.
- A table with collapsed borders resolves the winners and widths of its edges when it is built, from the borders its
  parts were painted with in the last build (`ResolvedStyles::borders`), paints the colours its parts are built with
  in this frame (`settle_collapsed_borders`), and asks for another pass only when a width or a style moved (#68).
  Every part is `Exclusion::CollapsedTable` (the parts are under `Inherited::table.collapse`), so neither the parts
  nor the table around them are ever replayed, whatever transitions or animates in a cell. The table itself is the
  one unit such a page has: paint-only motion on any part of it makes the table the unit
  (`Retain::collapsed_table`, `Retain::unit_facts`: the motion of the table and of what is in it, and no exclusion
  below it but the parts), since it is the table that paints their borders, and a tick builds the table alone. The
  parts are never units, and a part excluded for another reason (`Retain::exclude` replaces `CollapsedTable` by it)
  or one that lays out keeps the table from being one. An anonymous table has no element, so `build_plan` marks the
  element that builds it as holding the table, and that element is the unit (a container over the cap stays
  whole and is counted `TooLarge`). A patched tick still lays the unit out onto its own nodes (one pass, no node made).
- `Resolved` (`resolved_style.rs`) is written on each build and kept by a replay, which is right because nothing it is
  made from changed; a destroyed element forgets its record, and a `display: none` build does too. It is made from the
  focus-folded style, and the focus variants are on the spine (`:focus-within`, `:focus-visible`, the keyboard flag).
  The gpui element must say the same lift as the record: `apply_focus_variants` decides it again from the folded style
  (`stacking::apply_folded_lift`), because a variant applied on its own could not see the base `position` or the item
  flag. `Retain::digest` of a host includes the `Resolved` record, so a verified build sees a stale one.
- `display: contents` wrappers hand a container declaration and custom properties to the children spliced out of them
  (`contents_levels`); the wrapper's style serial is in the child's `contents_key`, so a change in the wrapper is a
  change in the key of each spliced child. A wrapper that toggles `display: contents` re-tracks the focusable
  elements below it (`track_focusable_subtree`).
- Text: `strut_font`, `nowrap`, `TextRun.group`, `UnderlineStyle.decorator` and `.fragment`, `InlineBox.wraps` and
  `.key` are in the gpui measure key through `TextLayoutOptions`, `TextRun` and `InlineBox`, which hash all their
  fields, and `LineHeight::Normal` is the line height measured from the text style's font. The sampled insets of a span
  that moves (`InsetSpec::text_key`) are in it, and a span or element gesture is a pointer delta, with a motion's
  controller named through `gesture_controller`. The digest of a verified paragraph includes the insets and the
  decorations.

**What puts an element on the spine, so it and its ancestors are built, and its other children may
still be skipped.**

- Entering or leaving hover, active, focus-within, focus-visible or the dim path, including the targets
  `:has(:hover)` reads; the keyboard flag changing.
- A scroll offset changing (the scroller and its ancestors; what is below the scroller is *moved*, see
  next list), a scroller appearing or disappearing.
- A custom element's `retain_state` changing; a measurement it reads (`motion/measure.rs`) changing;
  its accessibility links changing.
- A motion, transition, CSS animation, motion value or inline-span motion running now, and any element
  whose last build found one running. A dormant element is not one: it sleeps (`Retain::dormant`) until a
  build reaches it or what `Dormancy::holds` reads that no build tells moves (`Sources::verdicts`: the
  keyframes registry's epoch, the tree's count of re-parentings, the viewport), and a root that shows again is
  built with the clock of that frame, since a culled recording cannot replay at a place that is not culled
  (`ReplayKey::culling`).
- A tick that patches takes only the time-driven sources of this list (running animations, motion, motion that
  ended). A delta of any other source (the pointer, focus, a scroll, a custom `retain_state`, a measurement, a
  link, a command) declines the patch and the tick is drawn whole
  ([The patcher](#layer-3-gpuix-decides-what-to-skip-retainrs-tapers)).
- Anything above that a pass which only lays out saw but did not build and a build can enter
  (`Retain::unbuilt`), the parents of everything on the spine whether or not a build enters them, and a hole
  that changed or animates, which puts the root around it on the spine through the token.

**What makes the whole frame build everything (`all`, or no planning).**

- The first frame, a new text selection identity, and any call that forgets the records
  (`setRetainPlanning`, `setRetainEnabled`, `setRetainLift`, `GPUIX_RETAIN_OFF`).
- A draw pass above 1, `Window::refresh` (a rem size change calls it), a measuring pass that is not a
  layout-only one (`paint_if_dirty` inside input dispatch), the inspector.
  **The refresh rule:** `Window::refresh` is for state the planner cannot see, which a key or a delta
  would have to carry and none does: `set_live_image` and `register_font` (the bytes behind an id),
  background images (`background_images.rs`), the rem size, and a screenshot. Everything JS does to the
  tree is seen: a mutation moves a `subtree_revision`, a scroll offset and a running motion are frame
  deltas. So a commit from JS, a motion write, a programmatic scroll and a read of the automation tree
  dirty the window with `invalidate_after_commit` (`renderer.rs`), which both renderers use and which
  calls `cx.notify` and not `refresh`: with a refresh, one hover write over 3000 nodes built 5403
  elements (about 222 ms under load) instead of 5, and a hover sweep that committed from JS built 616 a
  frame instead of 7.5. The test renderer goes through the same function, so a `refresh` put back in it
  shows up in `live-invalidation.test.tsx`. A write that only drives paint-only properties notifies with
  `notify_paint_only` ([#47](#47-a-bounds-read-after-a-paint-only-write)).
  **Call sites that still refresh, with no reason found that they must.** They were left alone because
  nothing showed them safe without it, and each is a place to try `notify` and run the retained suites
  with `GPUIX_RETAIN_VERIFY=1`: focus changes (`focus.rs` `change_focus`, and the focus, blur and
  `focusNextWithin` commands in `run_ui_commands`), the window title, the setup of window key events and of
  selection-change events, the `scrollIntoView` family (`scrollIntoView`, `scrollByKey` and
  `scroll_focus_into_view`), `custom_elements/text_selection.rs` (`set_text_selection`), the selection
  listeners in `text/paint.rs`, `custom_elements/svg_paint.rs` (a landed raster), the test helpers in
  `retain.rs`, and the wasm pointer lock. Each costs a full build of the frame that follows it.
- A frame in a back-off stretch, which also records nothing.

**What makes a subtree that the frame skipped build late, in prepaint.**

- Its `ReplayKey` differs: the subtree moved by something other than a scroll that a relocatable root
  is put after, was laid out at another unsnapped size or with its origin at another sub-pixel phase
  (its snapped box can be the same), was clipped differently, or the opacity, text style, scale, viewport
  or the number of stacking contexts around it changed.
- Its root's `layout_generation` differs, which includes percentage padding resolved against a parent
  that was resized.
- An entity it read was notified, or an asset it drew was still loading.
- A hole or fragment in it was not painted in the rendered frame.

**What makes a recording non-reusable (`NonReusable`) or not movable (`NonRelocatable`)**: the lists in
layer 2. Those subtrees are built every frame they are asked for, or where they are moved to, and
`getRetainedCounters().nonReusable`, `.nonRelocatable` and `.relocationMisses` say why.

**What records again.** Every build of an element writes a new record (key, `Vol`, size, lift, reach,
build effects) with the epoch advanced, except a build gpui asked for late in the frame that already built
it, which keeps the epoch; and gpui records the prepaint and paint of a root whenever it builds it, for
layout or late, so a root that failed to replay records again with that build. A root built whole is
wrapped as `gpui::retained` by that build (`Retain::exit` returns the `Recording`), so the frame that
builds a page or a changed root is the one gpui records, and the frame after replays. A relocatable root that was
built because the move was not legal records again in a region around where it is now. A frame in a
back-off stretch records nothing and leaves the records as they were. Records go when the element is
destroyed (`prune`) or when planning is turned off or on.

**What a new build input has to do.** If it is state on the element, it goes through the revision. If
it is state of the window or the app that the build reads, it either enters `Inherited` and its
fingerprint, becomes a delta in `find_deltas` and a note in the build, or excludes the element. If it
is a registry the paint writes, it goes through the tape (with its geometry through the offset). If it
is a new custom element, it implements `CustomElement::retain_state` (`None` excludes it, `Some(hash)` of
everything it keeps for itself puts that hash in the shape and in the deltas). A key that does not hash
every input of a measure function makes a layout node keep a size it should not. A new gpui element
that keeps a position past the frame says so (`note_positioned_capture`, `keeps_no_position`,
`note_relocated`), and a new painted geometry rounds translation-covariantly.

### #38: culling clipped content

A browser keeps style and layout for content a clip hides and skips its paint. The fork does the same:
the build and the layout of a clipped box stay, because JS can read its box, and the prepaint and paint
go. A clipped box costs about two thirds of a visible one; a page that wants better culls in JS (README,
"Pannable surfaces must cull").

**gpui decides, from the real layout.** A child that says `.cullable(outset)` (`Element::cull_outset`)
is culled when its border box, and the boxes of what overflows it, inflated by `outset` and a pixel, do
not touch `content_mask().bounds`. The overflow is the engine's `painted_extent` (memoised per node;
descendants of a node that clips an axis contribute nothing on that axis). A subtree with a `fixed` or
detached node, or an `absolute` node whose containing block is above it, has no extent and is not
culled. Never culled either: a child that is lifted, escapes its clip, is sticky, or has an image cache
or a prepaint order; any child while a hitbox captures the pointer; anything inside a culled subtree.

**gpuix says how far a subtree inks** (`layout/cull.rs`), bottom-up as `build_element` returns: the
shadows, outlines, blur and drop shadow of the style and of every state style, and the font size of
text plus how far its underlines are put from the baseline and how thick they are
(`Inherited::underline_reach`: the largest `text-underline-offset` of the elements above and of the
spans of a paragraph). Absolute, fixed and sticky positions, custom elements (an `<svg>` that is not
positioned is the exception: its ink is its box), virtual lists, `layout` (FLIP) wrappers,
collapsed tables, paragraphs with an atomic or painted inline box, and anything with a cursor, region
or focus anchor `block()` the subtree: no ancestor of it is cullable. A box that clips both axes drops
what is built below it from the reach. A retained root forwards the reach recorded for its subtree.

**What a culled subtree still does** (`prepaint_culled`, `paint_culled`, under an empty content mask):
`on_painted`, `on_painted_layout`, `on_prepainted_layout` and `on_children_prepainted` listeners, so
bounds, observers and `onLayout` are as for a painted box; the scroll metrics of the scrollers in it
and the prepaint overlay of a classic scrollbar (so a scroller outside the clip takes its gutter); tab
stops and the focus path; the layout-pass publication; the positioned frame an escaping descendant is
placed by; and the accessibility nodes, with bounds and relations. The accessibility tree is not a
reason to stop culling: macOS turns it on for the first query from any assistive client and never turns
it off. It has no hitbox, listener, tooltip, cursor, style from state or overlay painter, which nothing
could reach outside the clip. `StyledText` only sets its bounds; the canvas under a text runs in full,
which is where the selection registry, the find ranges, the inline-box bounds and the painted-text log
are written, so selecting, searching and `getPaintedText()` see clipped text. A retained recording of a
culled subtree is made under the empty mask and moves to any place that is culled too.

### #39: reads that lay out and do not draw

A browser forces layout on a read and paints once at the frame. A bounds or scroll read here drew the
window synchronously, and that draw was not a frame the display showed. `Window::draw_if_dirty` is now
`layout_pass(paint = false)`: build, layout and prepaint, then throw the frame away. It leaves the
window dirty, so the frame that follows is the only draw, and a clean window runs no pass.

What paint used to publish is published from prepaint instead. `Window::publish_layout(f)` queues `f`
and runs it after the whole tree is prepainted, because an element's own prepaint does not yet have the
bounds of what comes after it. A `List` that prepaints a row range and rolls it back takes the entries
its rolled-back rows queued with it. Published this way: element bounds, borders, inline span bounds,
scroll metrics. `Window::paint_if_dirty` keeps the painting variant for
`getPaintedText`, `getPaintedHighlights` and the synthetic input that `automation.rs` injects (it is hit
tested against a drawn frame). Inside input dispatch a pass is still a pass: the dispatch is reading the
rendered frame and nothing may swap it (see patch 1). Time is the draw's: a pass reads
`GpuixView::frame_instant` and never the clock (#70).

A publication runs under the layout engine that was current when it was queued, and so does a deferred
draw (`Window::defer_draw`). An atomic inline lays its boxes out in an engine of its own, swapped in only
while it works, and a layout id from it means nothing in the window's engine: a box inside an
`inline-block` read its insets from an unrelated node, or panicked with "invalid SlotMap key used", on a
layout-only read.

The pass uses the retained subtrees the last presented frame recorded. It skips them
(`RetainedPhase::Layout`, `LayoutPaint` for a hole), publishes their bounds from the tape with
`tape::write_in_layout`, builds the spine and what moved, records no frame and takes nothing from the
rendered frame, so the draw after it still replays. What it builds it leaves a layout record of (#83 slice C, and from its first build on, #88a). A write followed by a read therefore costs the layout
and prepaint of the page and the build of the spine, and not a build of the page. The pass is a frame to
the deltas (see `Retain::unbuilt`), or a hover that a read consumed would stay stuck.

A write that cannot move a box does not make the next read run a pass
([#47](#47-a-bounds-read-after-a-paint-only-write)).

The test renderer draws a dirty window inline whenever effects flush, which hides every pass.
`setInlineDraws(false)` stops that and `drawFrame()` ends a frame the way the display link does;
`getLayoutPassCount()` counts the passes and `getPaintedFrameCount()` the real paints.

### #40: offsetX and offsetY

Already correct: `offsetX` and `offsetY` are filled from the target's padding edge by the dispatcher
(`dispatcher.rs`, since `bacf35f8`), and the demo that reported them missing was most likely running an
older addon. This pass adds the tests: through the React and Solid handlers, on a bordered target and a nested
one.

### #41: text in fractions of a pixel

Chrome lays text out in 1/64 of a pixel and snaps when it paints; gpui measured to the next whole pixel,
so a box as wide as its text was up to a pixel wider than Chrome's and what followed it later, and a
text-box-trimmed height was rounded up to whole pixels. Now a measured width is the advance of the text
rounded up to 1/64 of a device pixel (Blink's `ShapeResult::SnappedWidth`, `LayoutUnit::FromFloatCeil`: 1/128 px at
scale 2, the window's, round 15a item 4), a trimmed height is the font metric to the nearest 1/64 of one, a number
line height is cut to it and a length or percentage rounded to it (`LineHeight::to_laid_out_pixels`), the leaf in taffy no longer
rounds a measured size up to a device pixel, and forced and atomic-inline widths are not snapped.
Wrapping and truncation tolerate the float a measured width comes back as (the start and middle ellipsis
use the tolerance the end ellipsis and wrapping have, where they cut a text by the floor of its width
or not at all). Positions snap to device pixels when painted, and not in `getBoundingClientRect`
since [#45](#45-bounds-in-fractions-of-a-pixel); `Window::layout_unsnapped_bounds` reads a box before it
is snapped, which a moved replay needs. The
baseline of a run and the origins of atomic inlines start where the lines do, above the bounds by what
`text-box-trim` cut. An atomic inline is placed from the unsnapped origin of its paragraph in both axes and
keeps the fraction in every box it reports (`getBoundingClientRect`, `onLayout`, baselines); it is painted
at the device pixel its place rounds half up to, as Chrome does (`floor(position + 0.5)`, #80), and so is
what is inside it. A span's box is the rows of its paragraph measured from where the glyphs are painted,
moved by `TextLayout::unsnapped_offset`. Guards: gpui `chrome_layout_units` (cases measured in Chrome 155, equal to 1/64) and
`text-layout-units.test.tsx` (what an app reads back with the system fonts), `text-measure.test.tsx` (the rows of
headless Chrome at scale 1 and 2). Round 15c: every width is Chrome's per text node, and a legacy kern is split as
HarfBuzz does (see Round 15c, text positions). A percentage line height is rounded like a length, as Chrome does
(it computes to a length), and not cut like a number.

### #42: texts that keep their measure

A text whose measure depends on more than its string was measured and shaped again every frame, which
costs about 18 µs a text, and a page keeps hidden copies and `opacity: 0` ghosts of every label. Plain,
hidden and `opacity: 0` texts already had a measure key (the keyed measured leaves of layer 1). Two shapes did not:

- **A padded span** gives the text's key the insets that depend on the width (`InsetSpec::text_key`,
  `StyledText::with_insets_for_width(key, ..)`): what `Insets::text` reads is the measure key's share of
  the insets, and each consumer still reads the edges for the width the layout kept.
- **Atomic inline boxes** (an `inline-grid` label, an `inline-block`): their private layout engine now
  persists on the text's layout node (`AtomicCarry`), the frame asks it for the same nodes, and the text's
  key holds what the boxes asked for (`AtomicInlines::digest`) and the width they were measured at, so a
  min-content probe is not answered with the size of a max-content one. A text whose boxes are made anew
  each frame (a keyless request, a measured leaf as a box's root) has no key and is measured every frame.

A 200-text page with padded spans went from 4.73 to 0.28 ms and an inline-grid page from 11.3 to 8.6 ms.
The line layout cache counts what it did (`getLayoutCounters().shapes`, `wrappedLayouts`,
`lineLayoutHits`, `shapeCacheHits`). Behind its two frame generations the window keeps a store of what
it shaped (`shape_store.rs`, 8 MiB estimated, least recently used out, emptied when fonts are added), so
a page mounted again after other frames finds its lines (`shapeCacheHits`) and shapes nothing; guard
`shape-cache.test.tsx`. Guard: `text-idle-measures.test.tsx` (an idle and an animating frame cost no measure and
no shaping, retention on and off). A text with atomic boxes is still built every frame, because an atomic
inline is excluded from retention (~43 µs each).

### Round 9: #44 to #49, #57 and #58

A design-system demo run against the fork reported frame costs that grew with the size of the page and
not with what changed, and boxes that Chrome 155 reads differently. Each item below is a cause found
by counting what a frame did (`getBuildCounters`, `getLayoutCounters` and `getFrameWorkCounters`), and
pinned by a test that counts it again.

#### #44: the baseline of a trimmed label

A cap-trimmed label centred in a pill sat a pixel off Chrome. Taffy snapped the top of the box to a
device pixel, and the glyph baseline was placed from that snapped top and rounded again, so a label whose
top fell at 9.27 and whose baseline was 18.73 below it painted at 18 where Chrome rounds once, to 19.
A text now keeps how far the unsnapped top of its box lies below the snapped one, and its lines, their
positions and the boxes placed in them start from the unsnapped top. A relocation by whole pixels leaves
the phase as it is. Guard: a gpui test with the metrics of Inter, a 13px label in a 28px pill on row 19
and a 12px label in a 24px pill on row 16, as painted, and a label at the top of a box where it was.

Round 12 #66 is the same label in a place whose baseline comes from it. A cap-trimmed label in an
`inline-block`, `inline-flex` or `inline-grid` sat 5.33 px too high in a 16px/24px Menlo block (the cap top
at 0, Chrome has it at 5.328125, on the strut's baseline): `first_baseline` and `last_baseline` of a text
counted from the top of the untrimmed lines, and an atomic's ascent (and a flex or grid item's baseline)
is read from them, so it was `trim_top` too big. They count from the trimmed box now, and
`text/atomic.rs` reads the unsnapped tops that glyphs are placed from. (Round 15a: an atomic inline keeps
the 5.328125 Chrome keeps and the box of a `<span>` in a flex or grid item at a fractional top is measured
once from the unsnapped origin, see [#41](#41-text-in-fractions-of-a-pixel); and the half-leading of a line is
shared out in device pixels, baseline 17.5 at scale 2 as Chrome has it. The trimmed baseline is rounded to the
layout unit its box is in, so an atomic around a cap-trimmed label sits on Chrome's baseline.)

#### #45: bounds in fractions of a pixel

**Problem.** `getBoundingClientRect` and the reads that go with it returned the box snapped to device
pixels for painting. Chrome returns layout's own box, in 1/64 px (`LayoutUnit`): a `33.3333%` share of
100 px is 33.328125 wide, and a box after a text starts where the text ends, say at 69.3125.

**What changed.** These are the unsnapped border box cut down to a multiple of 1/64:
`getBoundingClientRect`, `getElementBounds`, the `ResizeObserver` sizes, the `IntersectionObserver`
rects, `onLayout`, the bounds of a `<span>` (its paragraph's rows shifted by the wrapper's laid-out
origin) and the boxes `getElementsAt` tests. Painting, hitboxes, hit testing and the pointer's
`clientX`, `offsetX` and `offsetY` keep the snapped box (`ElementBounds.painted_x/y` is the snapped
origin). These are integers, as in Chrome, rounded half up from what layout measured: `clientWidth`,
`clientHeight` (the padding box less the scrollbar), `scrollWidth` and `scrollHeight` (the scrollable
overflow, `layout_unsnapped_scrollable_overflow` in zed), and `clientLeft` and `clientTop` (the snapped
border widths, rounded). `scrollTop` and `scrollLeft` are as they were. The values are checked against Chrome 155.

Round 12 #65 is the unit of those reads and where they come from. Layout used to round every authored px
length to a device pixel (`AbsoluteLength::to_taffy`), so `10.01px` was 10, a `0.4px` box was 0 and
`33.3 + 33.3 + 33.4` painted 33, 33 and 33 with a gap. Now a length is cut toward zero to 1/64 of a
*device* pixel as Chrome's `LayoutUnit(float)` is (`100.4px` is `100.390625`, `-10.01px` is `-10` at scale 1
and `-10.0078125` at scale 2), a `calc()` is cut after it is resolved (`Calc::resolve` in vendored taffy, the
one place it is, so `calc(2rem - 0.3px)` is `31.6875`), and the reads are cut to the same unit
(`automation::layout_units` takes the window's scale, `LaidOut::scale`; the `f32` noise is taken off on a
1/512 device pixel grid first). The edges of a box snap at the end, half up, to the pixel it paints on, for
the start and for start plus size, and a box with a size never snaps to none (Blink's `SnapSizeToPixel`):
33.3, 33.3 and 33.4 paint 33, 34 and 33. A border width is Chrome's "snap as a border width", in layout, in
paint and for collapsed borders (`layout/collapse.rs` `snap`): floored to whole device pixels, a width under
one is one device pixel, none is none (`1.6px` is 1 at scale 1 and 3 device pixels at scale 2). A collapsed
border keeps its exact half widths. The oracle is `fractional-lengths-oracle.html` (135 cases at both scales,
boxes, client sizes, painted extents and scans), read by `fractional-lengths.test.tsx`; gpuix has no
`offsetLeft` and the like, so the oracle's `offset*` and computed style are not read.

Round 15a finishes the unit. A percentage is cut toward zero to 1/64 device px where taffy turns it into a
length (`resolve_percentage` in `util/resolve.rs`, called at every site that resolves one, the table module
included), as Blink's `LayoutUnit(float)` cuts `basis * pct`: `25%` of 101.296875 is 25.3125, not 25.324219. A
flex item's share of the free space is rounded to 1/64 device px the way Blink's `ResolveFlexibleLengths` rounds
it (`flexbox.rs`): the items are walked from the last, each takes the free space times its fraction of the
weights still to come, rounded half away from zero, and the first item takes what is left, so three equal items
shrinking are 1098, 1099 and 1099 at scale 1 and not three equal thirds; a sum of weights under one no longer
takes the gap twice. The rows are in `fractional-lengths-oracle` (`fractional-lengths.test.tsx`). A collapsed
table's `clientWidth` and `clientHeight` are its border box and its `clientLeft` and `clientTop` the outer half
border rounded half up (`ElementBounds.collapsed_table`), where the halves were taken off; its cells' client
size still leaves their own halves. An atomic inline keeps the fraction of its place in the line in every box it
reports and is painted where that place rounds half up to, and so is what is inside it
(`Window::with_unsnapped_element_origin`, which keeps the difference in `origin_phase_stack`): the Priority badge
of Beacon sat half a pixel high at a line top on a device-pixel tie (#80). The scroll reads are Chrome's too:
a `<virtual-list>` write is scroll-only and its `scrollTop` reads the list's offset (`ListState::dom_scroll`),
the largest offset of a list is the rule of a `div` (`max_scroll_offset`), and a scroll value that is not a
finite number is 0.

What this costs: an animated length now moves the boxes after it by fractions, as in Chrome, where it used
to move them only when the rounded value changed. A spring's tail moves boxes until the spring rests (0.5 px
of `restDelta` for a move over 5 px, as Motion), so `onLayout`, observer geometry and the frames that carry
them last longer: FF's thinking-steps page, whose heights are springs retargeted from `onLayout`, sends
about 40% more layout events and 35% more intersection geometry while its cards play and draws about 9%
more frames, with retention unchanged (no non-reusable reasons, replays as before). A root moved by a
fraction is built, not relocated, since a relocated replay moves by whole device pixels.

`Window::layout_unsnapped_bounds` only answers in prepaint, and the bounds are recorded in paint (never
move that, see AGENTS.md). So `capture_layout_box`, an `on_prepainted_layout` listener that also runs
for a culled subtree, keeps the box for the `on_painted_layout` listener of the same element, which
writes it in paint through the tape: still one write per element per frame. A replay or a relocated
replay moves the value by a whole device-pixel offset, which stays exact because the value is a
multiple of 1/64 once the `f32` noise is taken off.

**Deviations from Chrome.**

- A grid `repeat(n, fr)` whose tracks are not separated by an item is one set in Blink, sized as a whole and divided
  over its tracks; taffy sizes the tracks one by one, so the carried fraction of such a set can differ by a 64th
  (untested). `safe` and `unsafe` alignment cannot be written in the API.
- The scroll offset a box is placed at is the one it is painted at, a whole device pixel: the fraction of
  a scroll is dropped, so a box in a scroller scrolled by a fraction is where it is painted and not where
  a browser puts it.
- A box that is not a scroller has no scroll handle: `clientWidth` is its padding box, and `scrollWidth`
  is the same number, so what overflows it is not counted.

**Guards.** `fractional-bounds.test.tsx` (Chrome 155 values for a box after a text, 33.3333% shares, an
inline span, client and scroll sizes, the observers, `onLayout` and `getElementsAt`, and a replay and a
relocated replay against a build, also under `GPUIX_RETAIN_VERIFY`), and `text-layout-units`,
`inline-bounds` and `tables`, which read the exact values.

#### #46, #57, #58: what a hover frame costs

**Problem.** A hover over a 60-row sidebar costs the sidebar, and should not cost the 200 cards beside
it, whether they are below the window, in a clipped scroller or hidden behind `opacity: 0`. In a real
window with the demo's pages it did: the frame grew with the page. #57 saw one animating text on 3000
nodes as a full rebuild every frame (about 28 ms), and #58 saw a hidden copy of every label cost about
19 µs a text on a rebuilt frame.

**Causes, in the order of what they cost.**

- **A refresh.** A JS commit, a motion write, a programmatic scroll and a read of the automation tree
  called `Window::refresh`, and a refreshing frame replays nothing: one hover write over 3000 nodes
  built 5403 elements instead of 5. They notify now (see [the refresh rule](#invalidation-rules)). #57
  was this and not a rebuild of the page: an animating text builds 2 elements a frame, and a native
  keyframes animation never refreshed.
- **`opacity: 0` painted.** The contents were painted into a group that was thrown away at its end,
  which rasterised and looked up every glyph (about 7.5 µs a text on a built frame, 2 to 4 µs when
  replayed). The contents are painted with the scene hidden, so no primitive is inserted or recorded and
  a replay inserts none. Hitboxes, listeners, cursor styles, tab stops, accessibility nodes, canvases,
  the text registry, selection and find run as before, so the element keeps its box and is hit, as in
  Chrome. The fade composes with `visibility` (a visible element inside an `opacity: 0` one stays hidden,
  and the one after it shows again), and prepaint sees it, so a subtree recorded faded out is not
  replayed once it shows.
- **A colour that re-measured the page.** The measure key of a text hashed every field of its runs, so
  an inherited `color` that changed measured and shaped every text without a colour of its own, about
  five measures a text a frame. The key now holds each run's length, font, font size, line height and
  whether the decoration changes there. When the key hits and the paint hash differs, the carried lines
  are decorated again (`redecorate_lines`, which shares their layouts) and painted with the new colours.
  A text that truncates still hashes its runs, because the lines it cut cannot be decorated from them.
- **A line layout cache that forgot.** A frame in which nothing consulted the cache still swapped its
  generations, so a few frames of replay (nothing looked a layout up) dropped every label's layout and
  the next frame that measured shaped them all again: a spike of 60 shapes, about 15 ms. The swap now
  waits for a frame that used the cache.
- **Walks of every descendant.** The painted extent of a subtree (what culling reads) and the
  scrollable overflow of a scroller were worked out from the children on every pass: a hover frame on
  1000 clipped cards walked all 7000 nodes twice. Both are kept from one pass and frame to the next,
  stamped with the node's `layout_generation`, its absolute origin and scale, and an epoch that moves
  when a node becomes or stops being static, escapable or detached, and are worked out again only for a
  node whose layout changed. The unsnapped overflow that `scrollWidth` reads is kept the same way, in a map
  of its own. `getLayoutCounters()` counts the walks (`extentWalks`, `overflowWalks`,
  `unsnappedOverflowWalks`).
- **Replay per element.** An element state is kept under the id it was first stored under, so the ids
  compared when states move between frames and when the states a recording holds are found are equal by
  pointer, where each was compared element by element. `collect_garbage` walks the layout trees only when
  a node was made, left a tree or the roots changed since the last collection. In a debug build a frame
  that replays 7000 elements takes 8.4 ms instead of 10.1 to 11.0 (9.4 to 9.9 instead of 10.8 when they are
  in one root). The per-window bounds map that every painted or replayed element writes into hashes with
  FxHasher instead of SipHash, which was 0.5 to 1.5 ms at 7000 replayed elements. This made a replay cheaper per
  element; Round 12 made it a step per root ([#63](#63-content-nobody-sees-costs-the-page-nothing)).

**What a hover frame skips now.** Unchanged content that is below the window, clipped or `opacity: 0`
is not built, measured, shaped, styled, laid out again, prepainted or painted, and its primitives are not
inserted: a page of 200 cards makes the same frame as a page of none, in every counter above and for
the a11y tree on and off. What stays is the replay of recorded roots, which scales with the roots a frame
replays and not with the cards behind them. Cards below the window are not reached. In a scroller each
card is a retained root that is replayed (and, outside the viewport, culled): the replay key and root
lookup, the replay and the state take and put back cost about 5.5 ms a frame at 1000 cards in a debug
build (1.4, 1.3, about 2 and 0.8 ms for the root lookup, the replay, the element state and the element
id). No second prepaint of a culled root exists: the parent's culled prepaint is the call that reaches
the replay.

**What is left.** The cost was per replayed element and not per root, because the state of each element was
moved between frames. Round 12 did the range-based storage this needed: element states are kept for the root
that owns them and a replay places the roots inside it without visiting them
([#63](#63-content-nobody-sees-costs-the-page-nothing)). A text with atomic inline
boxes is still built every frame (see #42). The cost in a real window (the display link, the GPU) was not
measured: the numbers are counts and CPU time in the test renderer.

**Guards.** `live-invalidation.test.tsx` (a `setState` on one row and a motion value set on a page of 600
rows build under 30 elements a frame and replay the rest; a `refresh` put back shows up), and
`hover-frame-work.test.tsx` (the 60-row sidebar beside 200 cards below the window or in a scroller, with
and without an `opacity: 0` copy of each label, a11y on and off, makes the frames of 0 cards in elements
built, measures, shapes, styles and children set, nodes created, extent and overflow walks, prepainted and
painted elements and painted primitives; an `opacity: 0` text and box add no primitive to the frame that
builds them or to one with retention off, and keep their box and are hit; one keyframes text on 3000
nodes builds at most 3 elements a frame; an inherited colour changed over 30 labels measures and shapes
nothing; a re-measure after 10 frames shapes nothing; also under `GPUIX_RETAIN_VERIFY`). gpui tests pin
each cause: the `opacity: 0` scene (nested with `visibility`, and inside a retained root), the measure
key (a colour change measures and shapes nothing, a weight change measures again), the cache ageing, the
memo (a second frame of 50 rows works out no extent and no overflow, a row that grows works out itself and
the root only, and the engine fuzz compares every extent with a new engine's), and replay cost (ids
compared element by element, no layout walk on a replay frame).

#### #47: a bounds read after a paint-only write

A JS style write of the same keys is paint-only too since [#83](#83-a-forced-read-costs-what-changed).

**Problem.** A script that read a rect on every frame, after a motion value moved something, paid about
6 ms of layout pass at 3000 nodes for a change that cannot move a box. Chrome runs no layout for it.

**What changed.** A motion value write notifies with `notify_paint_only` when nothing a layout reads is
driven by the value (`MotionValueStore::writes_paint_only`). zed's `App::notify_paint_only` invalidates
as `notify` does and keeps the window's layout current when it was current before: the invalidator
records the `update_count` at which a draw or layout pass began, and `draw_if_dirty` returns false while
the count has not moved past it. Any other notify, `set_dirty` or refresh moves the count, and the next
read runs the pass. `AnimProp::paints_only` is the one list of what cannot move a box: `opacity`,
`boxShadow`, `color`, `backgroundColor`, the four border colours, `outlineColor`, `textDecorationColor`,
`scrollbarColor` and `selectionColor`. A value that a transform or a follower computes a width from, a
drag offset and an inset are layout, as is every JS commit, hover, scroll and resize. The frame after
the write still draws.

**Guard.** `read-after-paint-change.test.tsx`: no pass for an opacity or a colour write, and the rect is
the one before; one pass for a width, an inset, a derived width, a JS commit or a hover after a
paint-only write.

#### #70: a `scrollTop` write from a spring's `change` handler

**Problem.** FF's command menu wrote `scrollTop` from a spring's `change` handler, one arrow key ran
40 to 60 layout passes (about 90 ms) before a frame drew. The setter read the metrics to fill the axis it
leaves alone, the read laid out the window the write had just invalidated, the pass sampled the clock and
moved the spring, the spring's next `change` wrote again, and so on until the spring ended. Chrome runs
none of this: its animation clock is fixed within a frame and a forced layout runs no animation, and a
scroll offset is a paint property, so N writes with nothing dirty are zero layouts.

**What changed.** Only a draw takes a new time. `GpuixView::pass_instant` reads the clock in a draw and
`frame_instant` (the instant of the last draw, stamped once before the first) in a pass that only lays
out, for the scrollbar fades, scroll animations, motion states, motion values and virtual rows; the
reference pass of a verified frame already took the draw's. A scroll write on a `div` scroller notifies
with zed's `App::notify_scroll_only`: the window keeps a second record (`scroll_metrics_current_at`) next to
the layout's, a scroll write and a paint-only write keep it current, and `Window::draw_if_scroll_metrics_dirty`
(`getScrollMetrics`, and the setter's layout before its clamp) runs no pass while it holds. A read of
bounds uses `draw_if_dirty` and the layout record, so it lays out, because the children moved with the
offset. A `<virtual-list>` offset write is scroll-only too: the list clamps it from its last layout (before its first layout it keeps
it and the first layout holds it to the rows), the next frame builds the rows at the new offset, and its metrics are read from the
`ListState` (`ListState::dom_scroll`: `scrollTop`, the rows with their padding as `scrollHeight`, the list as laid out as the client size). Every maximum offset of a list (scrollbar range, averaged range, wheel, write) is div.rs's
`max_scroll_offset` of the rows with their padding, and the fill test of `layout_items` rounds the same way, so no layout pulls back an offset the range allows.
`scroll_to` lays out what is dirty, then `scroll::scroll_to` clamps to `ScrollHandle::max_offset` (it
relied on the next layout to clamp) and writes; a `scrollTo` that names both axes reads nothing back. The
test renderer's `scrollTo` no longer flushes (`flush` notifies and draws) before and after: that made every
scroll write a layout invalidation that only the test harness had.

**Guard.** `scroll-write-layout.test.tsx`: a spring's `change` handler writing `scrollTop` for 30 frames
runs no layout-only pass and at most one layout a frame; 50 writes with nothing dirty run none and clamp to
the maximum; a write after a style change in the same task lays out once and clamps to the new extent;
a bounds read after a write sees the scrolled position; a layout pass between frames fires no `change`
event and the box it lays out has the last frame's width. gpui
`test_a_scroll_write_only_makes_the_boxes_stale`.

#### #48: a size probe does not replace a placed layout

**Problem.** Under `GPUIX_RETAIN_VERIFY=1` a page panicked reading the bounds of a text (and hit tests
read the same handle). A container that asks a retained root for a size it is never laid out at (a flex
row asking the min-content size of its item) ran the measure function of a replayed root, which is not
prepainted, and left the text's handle holding the layout of the question and no bounds.

**What changed.** A measure function is told whether layout places the leaf or only asks how large it
would be (`MeasureRun::Place` or `Probe`, from taffy's `PerformLayout` and `ComputeSize`: a placement is
a call or the final-layout entry of the node, never an entry of a size probe), and a text keeps what it
measured only for a placement. `request_measured_layout` and `request_keyed_measured_layout` take the
`MeasureRun` in their closures, and so does the multi-line input's, which lays its text out on either.
**Guard.** A root replayed in a row that turns flex keeps the bounds and the wrapped lines of its text
(gpui `probe_tests.rs`).

#### #49: an animated inherited value on `display: contents`

**Problem.** A motion, transition or keyframes animation of `color` or `fontSize` on a `display:
contents` element did not reach its children, with retention off as well. The element is never built, its
children are spliced into its parent, and they were handed the inheritable part of its declared style.

**What changed.** `splice_overlay` steps the wrapper with `animate_style`, the function `build_element`
also steps an element's style with (transitions, `motion`, motion values, keyframes), once a frame for
each (`contents_styles`), and hands the children the inheritable part of what it shows. The overlay key
is a hash of those values and of what runs, not the wrapper's `style_serial`, so a child is built again
exactly when a value it inherits changes. A child under a running element notes the motion in its own
record, which keeps it and the way up to the wrapper on the spine while the wrapper runs. A property that
needs a box (`opacity`, a background, a size) still does nothing, because there is no box, as in the DOM.
**Guards.** `display-contents.test.tsx` (motion, transition, keyframes, `fontSize` laid out, `opacity` left
alone) and `retained-frame-sources.test.tsx`, which follow the wrapper with retention on and off, and
Solid's `display-contents-motion`.

### Round 12: #62 and #63

Two costs of animation that grew with the page and not with the animation. Each item below is a cause found
by counting what a tick did (`getBuildCounters`, `getFrameWorkCounters`, `getRetainedCounters`) and by
profiling the test renderer, and pinned by a test that counts it again. A *tick* is a frame whose only cause
is the clock. "r7" is the page of #62: one text with a colour keyframes loop above N/5 rows of bordered
cells. Times are CPU per tick in the test renderer, release addon built without LTO, 60 ticks 16 ms apart,
unless they say otherwise.

#### #62: one loop costs the page

**Problem.** One colour loop on one text, with nothing else changing, cost a tick 0.94 ms on 500 nodes,
5.84 ms on 3000, 30.5 ms on 12000 and 85.8 ms on 30000, though a tick built 2 elements and painted 8
primitives. The same page drawn once with nothing changed (a forced draw) took 49.2 ms at 30000.

**Cause.** A draw builds a new `Frame`, and everything a frame holds is copied into it once for each retained
root it replays: scene operations, hitboxes, dispatch nodes, element states, line layouts, accessibility
nodes, and in gpuix what the tape republishes. The profile at 30000 nodes was the bookkeeping of replaying
6000 roots: the element-state map and the id compares about 45%, moving `RetainedState` 17%, the dispatch
tree 7%, gpuix's `retained_root` closure and the bounds and text registries 9%. Layout was constant and scene
and paint were under 1%. A cheaper replay (#63) is still a step per root, so the page still pays for what did
not change.

**What changed.** A tick whose only change is how some animations paint is drawn as Chrome repaints the
display items that were damaged: the frame that is shown is kept, the units that moved are built alone and
painted again in it.

- **gpui** (`window/patch.rs`): `retained(..).patchable()`, `Window::request_tick`,
  `Window::set_frame_patcher` and `Window::repaint_in_place`, which restores the surroundings of each unit,
  lays out, prepaints and paints it into a scratch frame, checks it against the recording, and overwrites
  the unit's operations in the frame shown, or declines. See Layer 2, "Patching a frame".
- **gpuix** (`retain.rs`, `retain/patch.rs`): which subtrees are units, the preview that says whether a tick
  can be patched, the build of a unit alone in a tape that only captures, and the continuation that asks
  for the next tick. See Layer 3, "Units" and "The patcher".
- **What runs on a unit has to paint only.** Every running animation can say whether it only changes what its
  element paints: `CssAnimations::paints_only` (every property its keyframes animate is in
  `AnimProp::paints_only`, an SVG attribute counts, and a name whose keyframes are undefined or were
  redefined since the run read them does not), `CssTransitions::paints_only`, `MotionState::paints_only` (its
  tracks, none waiting for a measurement) and `MotionValueStore::binding_paints_only`. Dormancy (#63) uses the
  same classification.
- **A lifted element can be a unit.** Below 1 an opacity makes an effect group, which gpuix lifts at z 0 into
  the context around it, and a positioned element is lifted at its z-index, so a skeleton pulse, an opacity
  loop and the colour loop of a positioned box are units. `Retain::end_unit` refuses an element that lifts
  something out of it unless the element is a stacking context that takes it in, and `unit_root` wraps the
  unit with the `.lift_z(z)` a retained root has. gpui patches such a unit when the new element is lifted as
  the recording was: a unit keeps a second set of surroundings, captured when the context paints it
  (`Placement::Lifted`, `Deferred` since Round 14a), and a patch prepaints the new element directly, so it
  is not lifted again.
- **An `<svg>` whose shapes run a paint-only animation is a unit.** The shapes of an `<svg>` are never built,
  so the svg host notes what runs on them (`shapes_running`) as bits of its own, and `runs_paint_only` of the
  host asks every shape below it. `SvgPaint::paints_in_place` says whether painting the document of this
  render into the box of the last paint draws the image shown (same document and colour, or a document that
  is inline-sized, 4096 bytes and 128 x 128 px at most); only such a document was a unit. Round 14a
  rasterises a running document in the paint whatever its size, so any `<svg>` is one
  ([the item](#an-svg-is-culled-like-a-box-and-its-loops-sleep-out-of-view-and-patch-in-view)).
- **A tick declines safely.** gpui draws the tick whole in the same draw; gpuix leaves its state as it was
  except for the clocks, which are sampled at an instant and not stepped, so the frame that follows finds the
  state it would have and each event was emitted once.

**Measured** (r7, `animation-frame-work`, the test renderer's `clockAdvance` between ticks):

| nodes | tick drawn whole, before | tick patched | patching off |
|---|---|---|---|
| 500 | 0.94 ms | 0.22 ms | - |
| 3000 | 5.84 ms | 0.23 ms | 2.51 ms |
| 12000 | 30.5 ms | 0.22 ms | - |
| 30000 | 85.8 ms | 0.24 ms | 36.0 ms |

A patched tick builds 1 element, prepaints and paints 7, paints 8 primitives and replays no root, at every
size; before it built 2, prepainted and painted 15, replayed 100 to 6000 roots and 800 primitives. "Patching
off" is the same page with the replay costs of #63 and every tick drawn whole. The forced draw of the 30000
node page that changed nothing: 49.2 ms before, 3.75 ms after (the replay costs of #63). An svg stroke dash
loop like the button spinner of the demo: 0.079 and 0.099 ms a tick at 500 and 5000 nodes (0.98 and 13.2 with
patching off). In a live window with
the display link (`startFrameLoop`, 75 Hz, 8 s each at 3000, 12000 and 30000 nodes) every tick after the
first full frame was patched (`getPatchCounters`: `patchedFrames` equal to the ticks, no decline of either
kind) and cost 0.6 to 1.0 ms at every size. A few ticks at 3000 nodes took 6 ms in earlier runs; they were
patched ticks, did not depend on the size of the page, came and went within a run, and were not seen in about
60 later runs. The live window was not measured before the pass.

**Guards.** `animation-frame-work.test.tsx`: at 500 and 5000 nodes a keyframes colour, a native `motion` colour
loop, a colour transition, an svg stroke dash loop, a `motion` opacity loop (also holding a positioned child
with a z-index), a keyframes opacity loop and the colour loop of a relative box with and without a z-index do
the same work per tick (elements built, prepainted, painted, primitives, retained and layout counters equal)
and `patchedFrames` equals the ticks, also with a screen reader on; the picture after 21 ticks equals the
frame built whole at the same instant; hover and a React commit draw a full frame and patching resumes;
`animationiteration` comes once per iteration and `currentTime` moves per tick across 300 patched ticks; a
native loop between 1 and 0.4 and a keyframes skeleton pulse draw the two ticks around opacity 1 whole and
patch the 196 others, with the picture equal to the frame built whole at every instant around opacity 1; an
absolute badge in a relative card, a `width` loop and a 200 x 200 svg are drawn whole. (Round 14a patches the
ticks around opacity 1 and the absolute badge and the svg; see [#81](#81-an-opacity-run-is-a-compositor-layer-from-its-start-to-its-end)
and [#75](#75-a-loop-under-a-positioned-or-translucent-ancestor-or-on-an-absolute-element-is-a-patch).) gpui `window/patch_tests.rs`
(a page of 10 and of 1000 static retained roots beside one patchable root whose colour moves does the same
work, patch counters and (zero) replays and builds, and the patched scene equals the frame built whole, also
in a scrolled scroller, with the accessibility tree on, and with the pointer on a hitbox in the root; each
decline reason; a tick that changes no operation presents nothing; a wrong patch is a mismatch under the
verifier; lifted roots, alone, between lifted boxes, positioned in their parent, and in a root that a frame
replays). Unit tests in `retain.rs` of the unit choice (outermost, paint-only, size cap, exclusion, scroll,
lift, absolute, nothing skipped inside), the preview (each decline, dormant, no effect on the next frame), the
capturing tape, and a patched tick and a tick declined after its clocks stepped under the verifier. Run with
`GPUIX_RETAIN_VERIFY=1` as well.

#### #63: content nobody sees costs the page nothing

**Problem.** 200 cards clipped out of view, each with a colour loop, cost an idle tick 4.86 ms against 0.09
ms for the same page with no loops, and built 403 elements. Chrome paints nothing for them. A hover over a
sidebar beside 5000 such cards took 37 to 43 ms against 0.4 to 1.5 ms with none.

**Cause.** Two. A CSS animation steps only in a build, so every culled animated card was built (2 elements)
with the spine above it each tick, for a result that nothing draws. And a hover frame replays every card of
a scroller, at a cost per element and not per root: about 8 µs for each card nobody sees (1.2 µs an element),
with flat counters. The profile put it in ids that missed the pointer compare and fell to `SmolStr::eq` (about
30%), the loop that moves nested roots, `Frame::finish` moving the element states, `reuse_subtree`, and
gpuix's tape writing each replayed root into the registries.

**What changed: nothing is built or drawn for an animation nobody sees.**

- **An element is dormant** when everything that runs on it only changes what it paints and the last frame that
  reached it walked it culled (`ElementBounds::culled`, taken from `Window::is_culling` in the prepaint
  listener of `capture_layout_box` and written through the tape like the rest of the box, so a replayed root
  republishes it; a verified frame leaves it out of what it compares, since a root kept near a scroller's
  clip is walked culled when it replays an old recording and unculled when it is built whole). `Dormancy::holds`
  decides it from live state: culled, no span, no `layout`, and the animations, transitions,
  `motion` and `motionValues` of the element paint only. An `<svg>` can be culled and so can sleep, with its shapes
  (Round 14a). `Dynamic::Spliced` keeps the children of an animating `display: contents` element out of it. A dormant element is left out of `changed`, so its ancestors leave
  the spine and the root around it replays. A culled recording cannot replay at a place that is not culled
  (`ReplayKey::culling`), so a card that shows again is built late with the clock of that frame, and the
  colour it shows is the current instant's.
- **A dormant element costs a frame nothing while nothing about it changed.** `Retain::dormant` is a
  persistent map (element, the kinds running on it) kept out of `dynamic`, so `compute_deltas` asks
  `Dormancy::holds` only of what is not asleep. An element sleeps until something can have changed its
  answer: a build that reaches it (`finish_session` notes it in `woken`, and the next frame that begins asks
  again, so the frame that built it still sees what the frame before found), or what `holds` reads that no
  build of the element tells (`Sources::verdicts`: the keyframes registry's epoch, the tree's count of
  re-parentings, the viewport), which asks every sleeper again. A frame that does not record (retention off
  or resting, planning off) lets them all go, and a destroyed element is let go by `prune`. What `holds`
  reads of the tree (a span, an SVG element, a `layout` element, a `motionValues` binding that moves a box,
  the shapes an `<svg>` draws) is `PaintStatics`, kept per element while its `subtree_revision` and its
  parent are the same and pruned when the map doubles; a `<text>` in a `<text>` is read every time, since
  what makes it an SVG element is above its parent.
- **Their clocks advance without a build.** `CssAnimations::advance_clock(id, now)` and the same for
  transitions move the runs to `now` as the first half of a frame does (`Run::tick`, shared with
  `advance`), report iterations, ends and finishes, and note whether a run still moves. A dormant run is
  not ticked until something can be due: `Run::quiet_until` is the next of the start, the next iteration
  boundary, the end and the finish, less a millisecond (an hour for a held clock; nothing for a run going
  backwards or a `play()` waiting for a frame). `CssAnimations::advance_dormant` ticks from a due queue (a
  heap by `quiet_until`, one live entry per element) and from a list of elements to look at in the next
  tick whatever it says (new, touched by a build or a command, a run that cannot say when it is due), so a
  tick visits the runs whose boundary has come. `animationiteration`, `animationend` and the finish events come
  at the frames a tick of every dormant element would give them. Dormant transitions are looked at only while
  one runs. What a tick is for is `dormant_wake` (Round 18, #90).
  `Retain::take_dormant_moves` tells the renderer who fell asleep and who woke, so the two stores keep their
  dormant set by `enter_dormant` and `leave_dormant` instead of being given the whole set every frame.
- **A tick that has only dormant work draws and presents nothing.** `GpuixView::dormant_tick` is a
  `Window::on_next_frame` callback, armed when a dormant source has something to tell (Round 18, #90). It moves the runs and
  transitions to `now`, steps native motion, emits the events and sets `built_at`, so `getAnimations()` reads
  the instant of the tick, and it does not notify the view. A patched tick moves the dormant clocks the same
  way before it builds its units.
- **The maps per element are swept when something left.** The tapes and the CSS animation and transition
  states of a destroyed element are dropped in the frame after it was destroyed and not in every frame
  (`GpuixView::swept_at`; the tapes alone were 11% of the hover frame).

**What changed: a replay is a step per root.** In gpui, replaying an unchanged root costs 3.4 µs and 7 ns for
each element in it, where it was 14.8 µs and 0.15 µs (`state_carry_frame_time`: roots of 100 elements with a
nested root per 10, clipped). Five changes, each measured on its own page (50 roots of 100 or of 400
elements): the recordings name the roots inside them by the id their state is stored under, so a replay
finds them by pointer (14.8 to 10.6 µs a root at 100); element states are one window-level map owned by the
retained roots (10.6 to 6.5 µs at 100); a replay places a recording by its `Origin` and the roots inside
follow (21.9 to 12.8 µs at 400); dispatch nodes are copied in one pass (6.3 to 5.4 µs at 100); and a dispatch
node is small and shared behind an `Rc` (12.0 to 5.6 µs at 400). See Layer 2, "What a replay costs". In
gpuix, a replay no longer writes its tape into the registries: they read the lists a replay put back when
asked (see "The effect tape"). On the hover frame at 5000 cards that alone took the clipped page from 34.6
to 27 ms, the page below the window from 36.0 to 29 ms and the clipped page with colour loops from 42.9 to
37.7 ms (on the zed before these changes), and the cached tree facts and the clocks looked at only when due
took the last to 32.5 ms. With the zed changes it was 8.5 ms, and the persistent dormant set took it to 3.05.

**Measured** (hover frame, 5000 cards, clipped out of view, with colour loops, test renderer, release): 37 to
43 ms before the round, 3.05 ms after, against 0.4 ms with none (1000 cards: 0.9 ms). The same page without
loops costs 2.6 to 3.0 ms, so the loops cost nothing. An idle tick of 5000 such cards takes 0.00 ms (0.65 ms with the first dormancy, 0.24 ms
with the clocks looked at only when due), and `compute_deltas` is 0.4% of the hover frame where it was 46%. 200
clipped cards with loops: idle tick 4.86 ms before, 0.16 ms with patching and dormancy, against 0.08 ms with
none. What is left in the hover frame is gpui's `StateOwner::is_kept` (30%) and the replay itself.

**Guards.** `offscreen-animation-work.test.tsx`: 0 and 200 cards with a colour keyframes label and swatch, or a
`motion` opacity loop, below the window or clipped out of an `overflow: hidden` scroller, make the same idle
tick and the same hover frame (0 elements built, 8 prepainted, 0 painted primitives, 1 painted frame, where
200 cards built 403 elements); `animationiteration` fires at the tick that passes each second and the
iterations and ends of five loops of other lengths come at the same ticks culled and shown; `currentTime`
is the instant of every tick, and live-clock `drawFrame`s build and paint nothing for 2.3 s while it moves;
a card scrolled into view shows the same pixel as a build with retention off; a `width` loop is still built each
tick; a redefinition of the keyframes to move a box builds the culled loops again, and a redefinition back lets
them sleep. Also with `GPUIX_RETAIN_VERIFY=1`. Unit tests of the sleepers (asked once, woken by a build for the
frame after, asked again when `verdicts` moves, let go with the planning and with the element), of the due queue
(the events of four loops of other lengths, one retuned by a build, at the frames a tick of every one reports
them, looked at under 60 times in 375 ticks; an element that leaves is not looked at), of the tree facts kept per
element, of a clock ticked only at the frames that pass a boundary, and of the tape (a replay is read among the
writes around it in order and holds what the subtree wrote then; a replayed or built subtree inside another is
part of its tape either way; a box of a subtree inside another, put back by the replay of either; a box
of a recorded subtree moved by a replay and kept across a pass that builds it unrecorded). gpui tests: the
roots inside a replayed root are found by pointer (the compares of a replay frame equal for roots with 1 and
with 40 nested roots), the states of a replayed root live as long as it is replayed (a state counted across 8
replays, a rebuild of the root around, a rebuild of both, roots that leave the page and come back),
a replay keeps the states it does not visit without naming them (the accessed states are the same for roots of
10 and of 400 elements), a replay places each root twice whatever is inside it, roots inside roots follow
the ones replayed around them, and a node-by-node reuse of a subtree equals the whole one. `state_carry_frame_time`
(ignored) prints the frame time per root and per element.

### Round 13b: `content-visibility`

`contentVisibility: "visible" | "auto" | "hidden"` and `containIntrinsicSize` (with `containIntrinsicWidth`,
`containIntrinsicHeight` and the `InlineSize`/`BlockSize` spellings, which are the width and the height; the
physical longhand beats the logical one, both beat the shorthand). `hidden` always skips; `auto` skips while the
element is not relevant to the user (below). Keys: `layout/content_visibility.rs`, `css/contain_intrinsic.rs` (`none`,
a length, `auto <length>`, `auto none`; a number is px; width then height).

- **Containment** (any value but `visible`, on every element type, in `content_visibility::contained`, a style
  rewrite applied once the final style of an element is known, and again by `Resolved::of_style` so the walks over
  the retained tree agree): `overflow: clip` on each axis the author left `visible` (an `overflow` that already
  clips wins), a stacking context (`stacking::creates_context`), and `position: relative` with the insets dropped
  and no `z-index` (unless an item) when the author's position is static, which makes it the containing block of
  its `position: absolute` descendants (`containing_block::is_positioned` reads the resolved position). The box also
  has layout containment (`contain_layout`, set by `layout::style::apply`; Round 16, "Layout containment"): an
  independent formatting context and the containing block of its `position: fixed` descendants.
- **Skipped** (host elements `div`, `text`, `label`, `form`, block `span`; another element type is contained and
  never skipped): the element is built and painted (background, border, hit area), its children are not built, planned
  or counted (`build_host_container`), and it has size containment on both axes with `contain-intrinsic-size` as the
  content box (`contain_skipped`: `none` is 0, `auto <length>` is the remembered size once there is one, else the
  length), and an automatic minimum size of nothing as a flex or grid item (Chrome shrinks a skipped item to its
  padding). Nothing is hidden or forgotten as under `display: none`:
  the DOM, the transition and animation state and the resolved styles stay.
  `Retain::skip_content` runs when an element begins to skip: the records (but the layout of content a read forced,
  which is kept: Round 17), `dynamic`, `dormant`, `unbuilt` and `woken`
  entries of everything below go, so a subtree that shows again is built whole, no ancestor is kept in the spine by a
  loop nobody sees, and the dormant clocks (`leave_dormant`) stop: no animation or transition below ticks or fires an
  event while skipped. The timeline goes on: on show the runs sample the instant they find, `currentTime` is where the
  clock is, and the iterations missed are one `animationiteration`, not replayed. `RetainedElement::is_built_by_ancestor`
  and `box_generation::is_hidden` count an element below a skipped one, so a change there is not a spine of its own
  (and an `<img>` there is loaded like one under `display: none`, by `load_hidden_images`).
- **`auto`: relevant to the user** (`content_visibility::Relevance`, one per tree, `tree.relevance`). An `auto`
  element builds its content while it is *near* the viewport or holds the focus, and skips it (as `hidden` does) while
  it is not. Near is as in Chrome (measured against headless Chrome, not the spec): the border box outset by 150% of the
  viewport on each side (1.5 times the width to the left and right, the height above and below), cut by the padding box of
  every ancestor that clips it and by the viewport, still has an area. An element at the very edge of the margin is not
  near. Focus: the focused element and its ancestors (`FocusStyles::within`, given to the store at the start of every
  render) are relevant, so focusing an element inside a skipped one builds what holds it, in the same frame.
- **How it is decided.** The first answer comes in the frame that builds the element: an `auto` element nothing has
  answered for is built skipped, and its prepaint listener (`content_visibility::watch`) asks whether it is near from the
  bounds it is prepainted at and the content mask (the ancestors' clips and the window); an element in a culled subtree is
  not near (a subtree that holds an `auto` element is culled only past the margin: `Subtree::reach(proximity_reach)`,
  and a box that clips both axes drops what is below it from the reach, as the margin stops at its clip). A near answer
  for an element built skipped is `request_redraw_before_present` (the container-query pass: `MAX_PASSES` 4), and the
  next pass builds it (a page whose `contain-intrinsic-size` is not the size its entries have is laid out again by that
  pass, and what is near moves: the first draw presents the first answers, and the frames after it settle the rest,
  invisibly, 150% of a viewport away). After that the answers come from the sweep that follows
  each painted frame (`Relevance::sweep`, from `observers::frame_sweep`, which runs when any `auto` element exists
  though nothing observes): it reads the published boxes with the clips the `IntersectionObserver` uses
  (`clipping_ancestors`, `clip_rects`), and asks for a frame when an element is to show or skip otherwise than it was
  built, as a commit does (`invalidate_after_commit`, not `Window::refresh`, which would turn replay off for every root:
  a scroll that shows three entries of a page of 190 built 227 subtrees, against 6). Those changes take effect in the
  next frame, with no hysteresis, as Chrome's do. The elements that differ are a source of the frame
  (`Sources::relevance`, `Relevance::pending`), so only the roots above them that are replayed are built, and part of
  the key of the element (`container_key`).
- **A read of geometry forces the skipped ancestors** (`getBoundingClientRect` / `getElementBounds` and
  `getScrollMetrics`, in both renderers: `renderer::read_after_layout`; Chrome's
  `DisplayLockUtilities::ScopedForcedUpdate`). The skipped strict ancestors of the
  element (`content_visibility::skipped_ancestors`: those skipped as the last build did or as the next would, `hidden` or
  `auto`, outermost first; the element's own box needs no forcing) are put in `Relevance::force` before the layout the
  read always does, the window is invalidated and a layout-only pass runs, which is the one pass the read costs (Round 17,
  "The first read after a change is one pass"): `skips` answers no for a forced element, so its content is built and laid out
  in the box it keeps (`contains_size`: the size containment and the intrinsic size stay, as in Chrome, where the
  locked element keeps its size while its subtree is laid out) and its boxes are published; the read takes them; the set
  is cleared and the window invalidated again, so the next painted frame skips the content as before (the reads that
  follow share the pass, #83 slice D). Forced elements
  are a source of the frame (`Relevance::pending`) and part of the key of the element, so the roots above them are
  built and the ones beside them are replayed. A layout-only pass records no frame, which is why the frame after finds
  the painted records of the skipped build (it leaves layout records of what it builds, #83 slice C, and they outlast the
  draws that skip the content: Round 17). The pass leaves no trace: `built` and `remember` ignore a forced element (its state,
  its notified state and its remembered size are not touched), `answer` is not asked, and the animations and
  transitions below it are neither started nor stepped (`animate_style`, `is_forced_by_ancestor`), so no
  `animationstart` fires for a loop that is never shown. A read of an element outside any skipped subtree costs the
  walk up its ancestors and nothing else. The test renderer's `getBuiltElementBounds` is the old read: the box of the
  last layout, null for what is not built, which is how tests tell that the content of an element is skipped.
- **The first read after a change is one pass, and the layout it builds is kept** (Round 17, below). **Observers on an
  element below a skipped one** do not gather it (Round 17, below).
- **Remembered size.** The content box (border box less padding and border) of an element that is not skipped, from
  its prepaint and from the sweep, for `contain-intrinsic-size: auto`: `auto` is forced on both axes of an `auto` element,
  so a skipped one keeps the size it had, and the declared length (none is 0) is for one never laid out. Any element
  with `auto` in its `contain-intrinsic-size` remembers, so a `hidden` one that was visible keeps the size it had.
- **`contentvisibilityautostatechange`** (`onContentVisibilityAutoStateChange`, `event.skipped`; bubbles, cancelable,
  with a capture twin): sent for `auto` elements only, after the frame that applied the state (`sweep_relevance`, in
  document order), once for the first determination (`skipped: false` for an element built whole in the frame that
  mounted it, `true` for a far one) and then each time the state differs from the one last told. The elements of the
  window's first frame are told as Chrome tells a page at load (Round 16, "The first frame's events"). Going from `auto` to `visible` and
  back forgets what was last told, so the first answer is told again.
- **Deviations.** A change below a skipped element still rebuilds the elements above it (its `subtree_revision`
  moves) though nothing below is built; the `motion` states and SVG shapes of a skipped subtree are not paused; focus handles of the
  subtree are not removed. Where gpuix still differs from Chrome in the accessibility nodes, the Tab order, the search and the
  first frame's events, the deviations are listed with each (Round 16).
- **`overflow: clip`.** A box with `overflow: clip` on both axes is not a scroll container, so as a flex or grid
  item it keeps its content-based minimum size (an `overflow: hidden` one shrinks to nothing in a flex column).
  Every `auto` and `hidden` element has that overflow.


**Guards.** `content-visibility.test.tsx`: 93 cases against headless Chrome (`fixtures/content-visibility-oracle.*`,
regenerate with `sh fixtures/content-visibility-oracle.sh`) of a `hidden` element with a child much taller, in a block,
flex row and column, grid, `inline-block`, `inline-flex`, `fit-content`, `min-content`, `max-content` and absolute
parent, with padding and border, a definite size, `min`/`max`, every spelling of `containIntrinsicSize`; and `auto`
elements on screen (children laid out, an absolutely positioned child placed against the element); 1/64 px. Pixels: the
content clipped at the padding box and not without `contentVisibility`, an `overflow` that clips kept, the background and
border of a skipped element painted and hit, a `z-index` inside it stays inside. Counters: 200 rows under a `hidden`
element shape, measure and create nothing (2 elements built, 2 layout nodes: the page and the element) and rows added
below it neither; the rows are laid out when it shows and again gone when it hides. A loop below it, on screen or out of
the window (dormant), fires no event and builds nothing for 3.5 s, and continues from the timeline. Also with
`GPUIX_RETAIN_VERIFY=1`.

`content-visibility-auto.test.tsx`: eight scenes against headless Chrome (`fixtures/content-visibility-auto-oracle.*`,
regenerate with `bun fixtures/content-visibility-auto-oracle.ts`, which drives `chrome-headless-shell` over the DevTools
protocol, one page per scene, a page settled before the scene is added since the first frame of a page is told differently).
Each scene is a list of steps (mount, scrolls, a focus, a style change) read after the page settles: which elements
are skipped, the scroll extent and offset, the size of the boxes, which element has the focus, and what each element
was told (`exact` per step where nothing moves under a scroll, the last state told where the layout moves as the entries
are laid out). `list` (190 entries of 37 px with an intrinsic size of 40 px, 900x780 window, scrolled to 1000, 4000,
7000 and 0), `exact` (the intrinsic size is right), `bare` (no `contain-intrinsic-size`: `auto` is forced, an entry keeps
the size it had), `clipped` (a scroller of 400 px: what clips an entry cuts the margin), `horizontal` (a row scrolled
along x), `nested` (24 sections of 20 rows: a row is clipped by its section, so only those of the sections on screen are
built), `focus` (an entry far away that holds the focus stays built through scrolls and is skipped when it is blurred),
`remember` (a `hidden` element that was visible keeps its size, a never-shown one has the length, a far `auto` one has
the length until it is near and then its size). Counters: the first draw of the page of 190 entries with text builds the
near ones (`first draw` in the test log), idle frames build nothing; a far element runs and builds nothing below it, and
goes on from the timeline when it comes near; the value changed to `visible`, `hidden` and `auto` again. Also with
`GPUIX_RETAIN_VERIFY=1`.

Forced layout: `content-visibility.test.tsx` has six `hidden-read-*` cases (padding and border, an intrinsic size, a flex
row, a percentage width, an absolutely positioned child, and the example of the specification, a child at (11, 22) of
100x200) whose child is read against Chrome at 1/64 px; `content-visibility-auto.test.tsx` has two scenes of the oracle,
`forced` (the bars of four of the 190 entries, near and far, read at mount and after a scroll to 4000 and back; the
entries stay skipped) and `forcedNested` (a read through two `hidden` elements), and counter tests: a read of a far entry
creates the 11 nodes and the one line of text of that entry and no more, paints nothing, tells nothing, remembers no size
and starts no animation, the frame after it builds and paints what any frame does and not the text of that entry, and a
read of an element that is built on a clean window runs no pass.

Also in `content-visibility-auto.test.tsx`: a scroll that shows three of 190 entries beside a sidebar of 60 text rows
builds the roots of those entries in the frame after the sweep (6 subtrees, the same with and without the sidebar) and
nothing in the frame after that.

### Round 14a: #81, #74, #75, #76 and the patch leftovers

Round 12 made a tick of a paint-only animation a patch ([#62](#62-one-loop-costs-the-page)). The repros that
FF ran against it found the pages that still drew every tick whole: a loop under a positioned or translucent
wrapper, an `<input>` beside it, a motion value bound to a box, an `<svg>`, an opacity loop that passes
through 1 or 0. This round fixes each by its cause, and fixes the leftovers of Round 13b's retention (a first
draw that recorded nothing, a `color` that keyed every root, anonymous tables). Each item is found with
`getPatchCounters().unpatchable` and `declines`, `getBuildCounters` and `getFrameWorkCounters`, and pinned by
a test that counts it again. "r11", "r13", "r17" and "r18" are FF's repro pages. Times are CPU per draw, in
the headless test renderer with a 75 Hz timer for r13, r17 and r18 (10 s a run, 2 s of settling), and in the
offscreen test renderer for r11 (medians of 7 rounds after one warm-up); the release addon, on a machine
that was shared (loadavg 4 to 18), so compare numbers taken together.

#### #81: an opacity run is a compositor layer from its start to its end

**Problem.** A loop on `opacity` was lifted into an effect group only below 1, and painted nothing at 0 (the
scene was hidden). A loop through exactly 1 or 0 therefore changed the shape of what its element paints:
the tick at 1 flipped the lift and declined the patch (`Surroundings`), and the tick at 0 dropped the
operations (`Operations`), so those ticks and the one after were drawn whole. The same fact was a bug in
retention: a record made at opacity 1 (not lifted) and skipped (a dormant loop below the window) was handed
to gpui as a wrapper with no lift, and the build that replaced the replay when the card came into view was
lifted at 0.25. zed's `retained.rs` `request_subtree_layout` ("the root of a retained subtree is lifted as
its element says") panicked on a debug addon (the motion cases of `offscreen-animation-work`), and the
release addon kept the stale `None` and painted the element in the wrong place of the stacking context.

**What changed.** In Web Animations an element with a current animation of a property acts as
`will-change` of it, and Chrome keeps its compositor layer. So an element whose `opacity` is animated is an
effect group for the whole run, at whatever value it samples.

- **gpuix** (`animate_style`): `StyleDesc::animating_opacity` is set while a CSS animation that is current
  (active, or before its active phase going forward, not one that only fills), a running CSS transition, a
  `motion` track or a moving motion value targets `opacity` (`opacity_runs`). `has_effect_group` reads it, so
  `lift_z`, `creates_context` and `Resolved` all see an effect group at any sampled value.
- **zed**: `Style::has_active_opacity_animation` makes `group_effect` return a group at 1 and keeps the
  element's scene at 0, so the operations keep their shape from one tick to the next. A static opacity
  is as it was: 1 has no group, 0 paints nothing.
- **A run's start and end are whole frames**, as Chrome re-layers there. The invariant behind the panic:
  `Record::lift_run` says whether a build was in a run, and `skippable` refuses a record that disagrees
  with the run state now (`Kept::LiftRun`), so the wrapper of a skipped root never carries a lift that the
  build replacing it does not. A state style that changes the opacity (`hover`, `focus`, `focusWithin`,
  `groupHover`, `hasHover`) or a write changes the element's record, so it falls under the same rule with or
  without a transition. Round 19 runs the same rule for transforms: `lift_run` is true while an animation of
  either `opacity` or a transform property is current ([Round 19](#round-19-92-css-transforms)).

**Measured.** A native loop and a CSS animation through exactly 1 and through exactly 0, at 500 and 5000
nodes, are patched at every tick: the two ticks around 1 that were drawn whole are gone, and nothing is
declined or counted unpatchable.

**Guards.** `animation-frame-work.test.tsx`: loops through exactly 1 and exactly 0 (native motion and a CSS
animation) patch every tick with the picture of a frame built whole at each instant, and a transition that
sits at 0 for its run is patched. `retained-opacity-states.test.tsx` (36 cases): one card of a replaying page
goes between 1 and 0.5, 0.5 and 1, and 1 and 0 through each state style and a React write, with no
transition and with `opacity 240ms linear` stepped on the test clock, and equals retention off at every
step, plain and under `GPUIX_RETAIN_VERIFY=1`. `retain.rs` has the record made on the other side of a run.
gpui `patch_tests.rs`: a lifted unit patched at 0.5, 1, 0, 0.25, 0, 1, 0.5 keeps the operations of every
frame. `offscreen-animation-work` passes on the debug addon, which aborted at its first motion case.

#### #74: an `<input>` is retained and its caret is a tick

**Problem.** A page with an `<input>` or `<textarea>`, focused or not, in view or not, drew every tick of a
loop elsewhere whole (`Unsteady` at the tick rate): 4.0 ms a draw at 7.8k elements, 7.4 ms at 13k. A page
with a `<code>`, `<markdown>` or `<diff>` did the same.

**Cause.** The input had no `retain_state`, so it was `Exclusion::CustomElement`, which was in
`varies_with_time`, and `Plan::unsteady` declined every tick of a page that built one. The kind was listed
for the input, whose caret follows the clock; `<code>`, `<markdown>` and `<diff>` have no `retain_state` either
but do not follow it (the highlight is made on the frame thread and nothing of them arrives late). The
input's measure was a keyless request, which made every recording of it `UnstableLayout`.

**What changed.**

- **The editor is retained like any element.** `TextEditorElement::retain_state` is an epoch
  (`TextEditorState::changed` counts every notify of the editor but the blink's), so what is typed,
  selected or scrolled rebuilds the input and nothing else does; focus and the window going key are already
  in the planner's focus bits. Its measure is keyed (`request_keyed_measured_layout`,
  `TextEditorState::measure_key`: the shaped text, runs, sizes, rows, the font generation and the entity).
- **The caret is the clock's only.** `CustomElement::follows_clock` notes `Dynamic::Blink`, an own,
  paint-only kind, so a focused input is a unit unless a layout animation also runs on it. The blink is a
  timer aimed at the end of the phase (`arm_blink`, from the activity anchor on the executor's clock, not a
  500 ms loop) that calls `Window::request_tick` instead of notifying, and the caret quad is painted in both
  phases (in no colour while off) so a patch has the operations of the recording. An unfocused input, or one
  in a window that is not key, has no source at all.
- **A tick of another loop does not build the input.** `CustomElement::clock_due` says whether what follows
  the clock changed since it was painted: the phase's timer sets it and the paint clears it.
  `Retain::between_changes` leaves an element whose only dynamic is `Blink` out of the touched set while it is
  not due.
- **A caret nobody sees asks for no frame.** The timer is armed by the prepaint of the editor
  (`TextEditorState::keep_blinking`), only while the window is key, the caret is shown and its box meets the
  content mask and the bounds of the editor; `render` only notes the focus coming. An input scrolled back
  into view arms again in the frame that moves it.
- **`<code>`, `<markdown>` and `<diff>` left `varies_with_time`.** Nothing else read it for custom elements.
- **zed**: a unit that holds the focused element is patched (`registrations_are` declined every one, as the
  scratch frame sets `Frame::focus`); the unit is declined only when it gains or loses the focus.
  `TextSystem::font_generation` is public for the measure key.

**Measured** (r17, 7806 elements, one colour loop beside the input; `input` focused or not, `textarea`, and
an input out of the clip): 4.0 ms and 27.5% of a core, `Unsteady` 67.5/s, to 0.8 to 0.95 ms and 6% (the page
without an input measured 0.97 in the same run) with every tick patched. At 13k elements the input page
went from 7.4 to 0.34 ms and at 1306 from 1.16 to 0.34 ms, what the page without an input costs. The blink
is a patched frame that builds the input alone.

**Guards.** `animation-frame-work`: a colour loop beside an input, a textarea, a focused input, a selection
and an input outside the clip patches every tick at 500 and 5000 nodes with equal work and the picture of a
frame built whole; across 20 ticks within a phase the focused input is built 0 times and once across a change
of phase; a focused input scrolled out of its scroller asks for no frame in 2 s of clock, takes the text
typed there and blinks again when scrolled back; a loop beside a `<code>`, a `<markdown>` and a `<diff>`
patches every tick. `retain.rs`: a blinking element is a unit unless a layout animation also runs, a caret
is touched by the tick of its phase and no other. gpui `patch_tests.rs`: a unit that holds the focused
element.

#### #75: a loop under a positioned or translucent ancestor, or on an absolute element, is a patch

**Problem.** A colour or stroke loop on a text or an `<svg>` under a `position: relative`, `absolute` or
`opacity < 1` wrapper drew every tick whole (`UnknownUnit` at the tick rate), and so did a loop on an
element that is itself `position: absolute`. 3.1 to 3.9 ms a draw at 7.8k elements, 5.3 to 6 ms at 13k.

**Cause.** In gpui. `capture_patch_unit` recorded a unit only for a static root with no lift open, and for a
lifted root with exactly one open lift in a lifting context, so a root prepainted inside any open lift
(gpuix lifts a positioned box or an opacity group, `lifts_open >= 1` while it is prepainted) recorded
nothing, and `try_repaint_in_place` declined it. It is "not recorded", none "recorded twice": a
`patch_tests` case over eight holders pinned it before the fix. For the element that is itself absolute,
gpuix refused it as `Escapes`, since gpui kept no root placed by a box outside it.

**What changed.**

- **A root under an open lift is a unit.** `Placement` is `InPlace` when no lift is open and `Deferred {
  paint }` otherwise (the root, or an element around it, is painted by the stacking context later, under the
  surroundings captured when the root paints), and the unit keeps the `lift_z` the root declares. A root that
  says it is lifted under a parent that lifts nothing is painted in place and patched as one.
- **An absolute root is a unit, recorded and never skipped.** Its node is rewritten against a block outside
  it by `settle_containing_blocks`, which a frame that skips the subtree does not run, so gpui keeps such a
  recording as a source for a patch only (`Recorded::root_escapes`; every frame builds the root, as
  retention already did). `Window::root_escape` names the root's layout node while its recording or patch is
  made so that its own escapes do not count against it under a clip smaller than its block. The patch runs no
  layout pass; a moved block is caught by the bounds key. `end_unit` judges only what is below the root for
  an escape, so an absolute box inside it against the root is fine and an absolute descendant whose
  containing block is outside the unit still refuses it as `Escapes`. `position: fixed` stays
  `Excluded:fixed` (`Escape::Viewport` is declined `Surroundings`).
- **A tick drawn whole says why.** `Record::refusal` (`Retain::refusal`) names why an element something runs
  on is no unit, and `Retain::preview` returns it as `Unpatchable::Refused` where it returned `Outside`:
  `Excluded:<kind>`, `ExcludedBelow:<kind>`, `Escapes`, `Lifts`, `Scrolls`, `LayoutAnimation`, `TooLarge`.
  What runs on the element is looked at first, so an absolute element whose `top` runs is `LayoutAnimation`,
  and a tick made `Sourced` by the measurement of a layout animation is `LayoutAnimation` too. `patches_ticks`
  no longer needs a unit, so a page with none asks for ticks and counts them (the keys are strings now). See
  "Switches and counters".
- **The one-animating-node budget counts units.** A unit root keeps the start of its build (`BuildStart`, one
  `Inherited` clone) for a patch to build from again, so the bound of `motion-opacity` is `elementsBuilt +
  retainableRoots + unitsBuilt`.

**Measured** (r18, 7805 elements, release addon, draw ms, loop on a text or an `<svg>`; before to after):

| 3000 nodes | before | after |
|---|---|---|
| text under `relative` | 3.29 | 0.36 |
| text under `absolute` | 3.10 | 0.40 |
| text under `opacity < 1` | 3.22 | 0.40 |
| text that is itself `absolute` | 4.36 | 0.38 |
| `<svg>` under `relative` | 3.44 | 0.23 |
| `<svg>` under `absolute` | 3.85 | 0.23 |

The text that is itself `absolute` was 30% of a core and is 3%. At 5000 nodes (13005 elements) a text under
`relative` and one that is itself `absolute` went from 5.28 to 0.34 ms. r13 with the header wrapper
`relative`: a colour loop 3.45 to 0.67 ms at 500 nodes, 2.59 to 0.41 at 2000 and 8.89 to 0.43 at 8000
(64% of a core to 3%); the stroke loops of the same page likewise. Every tick is patched and `declines` is
empty; before, `UnknownUnit` came at the tick rate. A `width` or `top` loop in r13 is still drawn whole, and
now says `LayoutAnimation` at the tick rate where it said nothing.

**Guards.** `animation-frame-work`: r18's shapes at 500 and 5000 nodes (a text and an `<svg>` under
`relative`, `absolute` and `opacity < 1` wrappers, nested, and `position: absolute` themselves, also a badge
in a relative card) patch every tick with equal work, no decline of either kind and the picture of patching
off; the containing block moved by a click mid-loop (the text follows it, every tick patched); a fixed text
counted `Excluded:fixed`; a loop on an element that holds an absolute box against a block outside counted
`Escapes`; each refusal on a page with no unit. `retain.rs`: each refusal and its key, an absolute element and
one holding a box against it are units. gpui `patch_tests.rs`: eight holders registered and repainted in
place, the verifier, a scroller, replay and hover over them, an absolute root under each holder with the
block moving between ticks, and one that escapes the clip around it.

#### #76: a motion value is touched when it can have changed

**Problem.** FF's pages with a motion value bound to a box drew whole for four reasons, none of them a
visible paint-only binding (that already patched, with an `<svg>` stroke reached through the `color` of the
`<svg>` and `currentColor` strokes, since a motion value binds to a style property and never to an attribute
of a shape):

- A box bound to a value that rests was built by every tick of a loop beside it (50 resting boxes: 51
  elements built a tick, 1 alone).
- A width, left or top bound to a value that rests made every tick of a loop elsewhere whole
  (`LayoutAnimation`, 8 of 8).
- A paint-only value bound to a culled element requested a frame every vsync, drawn whole (`NoUnits`, 764
  primitives replayed), where a CSS loop there asks for none.
- A script's `mv.set(v)` from a `requestAnimationFrame` loop, or from the `change` of another value, was a
  whole frame at each write (`PendingView`).

**Cause.** Every element with a `motionValues` binding carries `Dynamic::MotionValues`, and `Retain::preview`
touched all of them at every tick, moving or resting. The end of a frame asked for a tick whenever any bound
value moved (`MotionValueFrame::active`, one tree-wide bool), which never reached the dormant branch. A write
went through `Repaint::MotionWrite` to `notify_paint_only`, which queues the view.

**What changed.**

- **A resting binding is not touched.** `MotionValueStore::binding_due` says whether a binding can show
  otherwise than the frame that built its element did: a value it binds was moving by itself when that frame
  sampled it (`frame_moving`), or it, or a value it is computed from through transforms and followers
  (`MotionValues::follows_any`), was set, jumped, animated, stopped or controlled since (`written`, cleared by
  `frame`, which a frame built whole and the patcher's `step_clocks` both run). It goes through
  `Sources::clock_due` beside the caret's, and `Retain::between_changes` leaves an element whose only
  dynamic is `Blink` or `MotionValues` out of the touched set while it is not due. One that also runs an
  animation is touched by every tick as before.
- **What the values ask of the frames is per element.** `MotionValueStore::demand` is `Frame` when a value
  bound by an element that is not dormant (or read by a drag) moves, `Dormant` when only dormant elements
  bind it or only a listener reads it, `Idle` otherwise. `Dormant` arms the dormant tick when a listener needs one (Round 18, #90), which runs
  `MotionValueStore::advance_dormant`: it samples the listened values at its instant and sends their events, so
  `change` still comes at the display rate, `animationStart` and `animationComplete` when they happen, and a
  `finished` promise resolves out of view. It leaves `frame_moving`, `frame_values` and
  `written` as the last build left them, which the frame that builds an element after it wakes has to find.
- **A paint-only write is a tick.** `Repaint::invalidate` asks for one (`Window::request_tick`) when
  `GpuixView::patches_ticks` and `MotionValueStore::writes_paint_only` hold; `animate`, `stop` and the
  controls take the same road, so the frame that starts and the frame that ends a run are patches too. The
  layout stays current, so a read of bounds before the frame lays nothing out. A write that can move a box
  still invalidates layout and is drawn whole, and with `GPUIX_PATCH_OFF` it notifies as before.

**Measured.** Counted, not timed: 50 resting bound boxes beside a colour loop build what the loop builds
alone (1 element a tick, from 51); a loop beside a resting width, left or top patches every tick (from 8 of 8
whole); a bound colour, background colour, spring or range transform out of view in a scroller, or among 5000
nodes below the window, draws no frame in 24 ticks and sends one `change` per tick; a `color` written every
frame at 500 and 5000 nodes is patched at every write with the work of the loop.

**Guards.** `animation-frame-work` (the cases above; the picture of a frame built whole at every tick of a
tween, a spring and a paused-then-played tween, up to the tick the value stops being a source; a write to a
scroller's colour or to a width is drawn whole with the value written; the tests that screenshot between ticks
look at every third tick, since the tick after a screenshot is whole), `bindings.rs` (`binding_due` across a
run, its end, a write, a transform, a control and an unparsable binding; `demand`; `advance_dormant` leaving
the last frame's findings), `retain.rs` (a bound element is touched when due and not otherwise).

#### An `<svg>` is culled like a box, and its loops sleep out of view and patch in view

**Problem.** Every custom element called `Subtree::block`, so an `<svg>` and everything above it could never
be culled, `Dormancy::holds` never saw it culled, and a stroke loop on a card nobody sees was built every
tick. A document over 4096 bytes or 128 x 128 px was rasterised by a job, which a patched tick would start
and then show the image of the tick before, so a running one was `Exclusion::Svg` and every tick of it was
drawn whole.

**What changed.**

- **The ink of an `<svg>` is its box.** It draws one image fitted to its box, `overflow: visible` included, so
  `build_element` takes `Subtree::of_element(empty, style)` for it in place of `block` (the box plus what
  its own style adds; a positioned `<svg>` still blocks) and the element calls `cullable`. In a culled
  subtree its canvas makes no image and starts no job (`SvgPaint::note_culled`; `retain_state` is 1 after such
  a paint, so an `<svg>` never shown is not an unsettled raster).
- **The shapes sleep with it.** A shape is never built, so a running shape named the shape in
  `Sources::motion`, which `holds` could never find culled. `running_motion` names the `<svg>`
  (`svg_tree::host_of`), and the clocks of its shapes enter and leave dormancy with it (`with_shapes`), so
  `animationiteration`, `animationend` and `getAnimations()` of a shape come at the ticks of a shown one.
- **A running document is rasterised in the paint, whatever its size** (`CustomElement::shapes_run`,
  `Key::in_paint`), so `draws_changes_at_once` holds and its ticks are patched. A static large `<svg>`
  keeps the job.

**Measured** (per tick, test renderer, release, median of 30; before to after): a 200 x 200 `<svg>` with one
running circle 0.53 to 0.11 ms on 500 nodes and 4.30 to 0.12 ms on 5000; 400 x 400 0.73 to 0.29 and 5.85 to
0.28; 400 x 400 with 40 running shapes 4.11 to 3.80 and 7.83 to 3.73; 800 x 800 with 40 shapes 7.36 to 5.94
and 10.97 to 5.91. None was patched before; 39 of 40 ticks are after (the first records the unit). The
rasterisation moves from a background thread to the frame, which the whole frame it replaces paid in waiting
for the job as well. Below the window or clipped out of a scroller, 200 cards with a stroke loop on an
`<svg>` of any size make the idle tick and the hover frame of 0 cards.

**Guards.** `offscreen-animation-work` (200 cards with a stroke loop on a small and on a more than 4 KB
`<svg>`, below the window and clipped; the iterations and ends of five loops of other lengths come at the
ticks of a shown one; `currentTime` of a shape moves; a card scrolled into view has the pixels of a build
with retention off), `animation-frame-work` (a stroke loop on a 200 x 200 `<svg>` and on a 24 x 24 document of
more than 4 KB patches every tick with equal work; the tests that pinned a large `<svg>` as drawn whole are
gone), `retain.rs` (an `<svg>` too big to raster at rest is a unit), `svg_paint.rs` (the in-paint key, the
culled note).

#### Retention: a root is recorded by the frame that builds it, and a colour is read where it is read

Four leftovers of Round 13b's retention ("r11": 190 entries, 1902 elements, in a scroller; "r11b" is its
sequence of draws).

**A root built whole is recorded by that frame** (`Retain::exit` returns a `Recording`). `Retain::enter` handed
gpui a `retained(..)` wrapper only for an element it skipped, which needs the record of a last build, so the
first build of a page and the build of a root because it changed were plain builds. The record they left
said "skippable under token T", the next frame skipped under T, gpui had no recording under it and built the
root again, late, in prepaint to record it, and only the frame after that replayed. Now `exit` returns the
token, lift, culling reach and overscan for a build that `skippable` takes the record of in the frame after
(planned, plain, boxed, not prekeyed, outside a unit and a deferred build, no exclusion, not escaped, worth
its size, not moved by a scroller nor too big for one, and no motion running in or below it), and
`build_element_in` wraps the element it built as `gpui::retained(id, token, built)` (`recorded_root`). The
two passes over the nodes in `finish_session` are `Retain::settle`, run by `exit`, so the epoch, token,
holes, escapes, lifts, size and digest of a node exist when its build ends. Three things the first recording
met that a later one does not: the epoch of an element with no record was 1 again after `forget` and a build
took a token gpui still held a recording of (it is the frame serial now); a background image still loading
is asked of the loader during the build, outside the recording (a build that left one loading is not
recorded); a scroller had no `Scroller` before its first layout, so its first frame recorded its roots where
they stand (a provisional one takes the style's axes and the window for a viewport). zed's `Recorded::is_loose`
(a recording made in an overscan no region was made for, as under a rounded clip) is compared as loose by the
verifier, and `Window::is_recording_any_relocation` keeps a root nested in the reference build of a verified
frame from planning a relocation of its own.

**Numbers** (r11b, 190 entries, 1902 elements): draw 2 14.3 to 1.8 ms (1902 to 12 elements built, 190 subtree
builds to 0, 190 hits and one lazy build at the edge of the overscan band); a text change 13.9 to 2.1 ms, and
the frame after replays it; the culled variant 2.2 to 1.1 ms. **Cost:** the first draw is about 15% more
(30 to 35 ms): the entries within the scroller's overscan are prepainted and painted to be recorded (735
prepainted and 2587 painted primitives against 303 and 1616), which draw 2 paid before.

**A `color` is a change to the roots that read it** (`ColorContext`, `color::reads`). The key of a root
held all of `Inherited`, colours included, so a `color` on the page keyed every root below it as changed
though the cards set their own text colour, and past that key gpui's `ReplayKey` holds the whole text style
around a root. `ColorContext` now keeps where its `current` was declared, `Retain::enter` / `exit` open and
close a read frame per built element, and `Record::reads_colors` says whether something in it read a place
above it. A `color` an element declares cuts the read for what is below it; `currentColor` and
`color-mix(.. currentColor ..)` (also in a state, and in a parse-cache hit of a text that names it), text,
custom elements, `<svg>` markup, `motion` and bound values read the colour they inherit. `Key::colors` is
compared by `skippable`, `Cause::Inherited` and the token only for a record that read it. A root that reads
none is wrapped `ignoring_text_color` (zed `Retained::ignoring_text_color`): gpui lays it out, prepaints,
paints, records and replays it under a colour of its own (`FIXED_TEXT_COLOR`, magenta, so a reader that was
missed shows), not for one that lifts. The default colour of an outline and a text decoration was a read for
every element and is lazy now. r11b with a `color` changed on the page (draws 5 to 7): 1902 elements built
to 2, 17 ms to 1.6 ms (after the first-build change alone made these frames 60% dearer, since each now
recorded every root). r11 does not move: its entries read nothing and its warm step changes a background.
**Only colour has a read set**; the other inherited fields (lengths, font state, direction, custom
properties, groups, containers, scheme) still key every root.

**The parts of an anonymous collapsed table are a unit by their container.** `Child::AnonymousTable` is built
by `build_plan`, never through `build_element_in`, and `Retain::collapsed_table` was only called for a
`display: table` element, so no node was the table's and a colour transition on a cell drew every tick whole
(`Outside`). `build_plan` now marks the element it builds as holding the table, and that element is the unit
of a paint-only motion on any part, under the rules of every unit (at most 256 elements, no exclusion below
it but the parts, nothing that scrolls or lifts out of it). A container too big to be one stays whole and is
counted `TooLarge`. The anonymous table's borders are keyed `BoxKey::Anonymous { container, ordinal }` and
resolved from the children of the container, so a build of the container alone resolves the same borders.
Per tick: 0 patched frames, 458 nodes reused, 472 painted before; 1 unit, 51 nodes reused, 64 painted after.

**The walk of a culled subtree stays** (reported, not changed). A culled prepaint and paint still serve the
bounds registry (`getBoundingClientRect`, observers, container queries, `content-visibility`), the text
registries (selection, find, painted text), tab stops and the focus path, accessibility nodes and nested
scroll metrics. A retained root already skips it on a hover or an idle frame (`culledPrepainted` 0 at 0, 1000
and 5000 cards); only a build that goes through it walks it (18 prepainted and 18 painted per card). Skipping
it would take the walk from about 60 to 75% of a built frame (hover at 5000 cards with retention off, 584 to
232 ms), but the reads and the tab order would break; it needs a lazy or movable registry.

**Guards.** `retained-first-build.test.tsx` (draw 2 after a root background change builds no card and every
card is a hit; a changed card is built once and the frame after replays it with no lazy build; the pictures,
text, boxes and accessibility tree equal retention off through six steps; a card beside a running motion is
recorded and the spine is not; a rounded, bordered scroller of sections scrolled with the wheel fails on a
verifier mismatch). `retained-inherited-color` (cards with their own text colour build 0 and replay 12;
cards whose text inherits; a `currentColor` border with its own text colour; a nested declaration cutting
only below it; `color: currentColor` and a `color-mix` of it not cutting; 15 readers drawn as a build
draws them; a hover state), `color::tests` (reads per depth), `retain::tests` (a card that sets its colour is
not changed by the page's). `collapsed-border-transition` (anonymous table: every tick patched with one
unit, no node created or removed, no root of the page replayed, the same work with 60 and 240 siblings, the
colour of ticks 2, 5, 10 and 12 equal to patching off, a container of 200 extra rows whole and counted
`TooLarge`, a border widened mid-transition lays out and goes on patched). Two expectations changed with the
first-build change: `content-visibility-auto` (the frame that shows three entries builds their roots once and
the frames after build none) and `retained-scroll`, which counts relocations under the verifier again: the
reference build of a verified frame leaves its recordings direct and unmovable, so any idle frame between the
frame that records rows to be moved and the scroll that moves them makes that scroll build them where it
puts them (`lazyBuilds` 40 per scroll, `relocations` 0), and `run` now ends with four pairs of scrolls, the
verifier off for the frame that records and on for the frame that moves. Plain and under
`GPUIX_RETAIN_VERIFY=1`, with no `[retained]` mismatch.

#### FF's showcase: an `<svg>` waiting for its raster, and motion under `display: none`

**Problem.** FF's showcase (2358 elements, with a thinking indicator whose text shimmers and a glyph that
morphs its `d`) drew every tick whole, and the tile of the indicator alone (47 elements) patched 63 of 68. `r19-showcase-frame-kinds`, 1240 x 780, 75 Hz, 8 s, release addon: 68 draws a
second, none patched, 6.3 ms a draw, 40 to 44% of a core. Turning off either loop alone changed nothing,
turning off both drew nothing, so the page held every tick whole for as long as anything on it ticked. It
was two things on the page that nothing draws.

**Cause.**

- **An `<svg>` that had not painted an image was unsteady.** `SvgElement::retain_state` was `None`
  (`Exclusion::Svg`, one of the kinds that `varies_with_time`, so `plan.unsteady` and
  `Unpatchable::Unsteady` for every tick) whenever `SvgPaint::paints_in_place` was false: before any paint
  had made an image, while a job ran, with a result in the inbox, and with a big document or a colour that
  the image shown was not. A box with no area (`width: 0`, a big document with `height: 0`) is never painted
  (`paint_svg` returns before `SvgPaint::image`), so its state stayed `None` for as long as it lived and
  every frame that built the page recorded the page as unsteady (the frame-kind test: Unsteady 8 of 8).
- **Motion under `display: none` was outside every unit.** `Retain::preview` named an element something runs
  on that no unit holds as `Outside`, also when it has no box (`display: none` or below one: no build
  reaches it and no frame paints it). The closed dropdown and the cards that are not shown in the showcase
  (`card-dropdown`, `card-slider`, `card-thinking-steps`, found with `getRetainDebug().outside`) ran a motion
  or had been built with a value, so every tick of the page was declined for them (`Outside` 64 a second).

**What changed.**

- **A waiting `<svg>` has a word in its state.** `SvgPaint::unsettled` says which of
  `Unsettled::{NeverPainted, JobRunning, ResultWaiting, Stale}` holds, and `retain_state` gives a word per
  waiting state (2 never painted, 3 job running, 4 its result waiting for the next paint). Nothing a replay of
  the last paint draws differs while it waits, and the arrival changes the word, which `compute_deltas` finds
  as a changed custom element and builds that `<svg>` once: the arrival draws one frame that is not a tick,
  and ticks go on after it. Only `Stale` (a document or colour a paint would start a job for, which a replay
  would not) stays `None`, for the one frame before that paint.
- **An element with no box is not outside a unit.** `Retain::preview` skips it as it skips a dormant one
  (`box_generation::is_hidden`).
- **`getRetainDebug` says why.** `exclusionWhy` names why a built element excluded itself
  (`CustomElement::unretained_because`; for an `<svg>` the `Unsettled` name), and `outside` lists the
  elements ticks were declined for as outside every unit, with their record, their refusal and whether they
  are hidden or unbuilt (`[d:none]` in their labels).

**Measured** (release addon, `r19-showcase-frame-kinds`, window scale 2): 68 draws a second, all whole,
`Outside` 64 a second, 6.1 ms and 42% of a core, to 63 of 68 patched (the rest are the word roll's layout
animation, `LayoutAnimation` 4.2 a second, and `Edited` 0.4 a second), 4.0 to 4.2 ms and 28% (the machine
was at a load of 3 to 12, which inflates the CPU column).

**Guards.** `animation-frame-work`: a colour loop beside an `<svg>` of no area, a big one with no height, one
in a box of opacity 0, `visibility: hidden` and clipped to nothing, beside an svg that a job rasterises, an
icon and an svg of shapes, and under `display: none` (a motion loop, a keyframes loop) is patched at every
tick at 500 and 5000 nodes; a `motion.path` that morphs `d` in a 24 x 24 svg beside a colour loop is patched
at every tick (it was, and is pinned); the raster of a new or a changed document lands as a frame of its own
and the ticks go on; the pictures equal patching off. Rust: `svg_paint` (`unsettled`), `retain` (motion under
no box, an `<svg>` waiting for its raster).

**What did not reproduce.** The first tables of this page said `Unsteady` 68 a second, and later the same
day the same addon said `Outside` 64 a second and no `Unsteady`, with the same page, the same window and
the same release build. The cause was not found (a different display state of the machine was suspected and
not shown). The `<svg>` mechanism above is the class that reproduces `Unsteady` in a test, and the
`display: none` motion is the one the page has now, so both are fixed and pinned; whether the first was the
one the showcase had at 17:00 is not known.

#### What a patched tick still walks

**Problem.** A patched tick should cost what its units cost, and the showcase's did not: 4.11 ms on average
(median 4.27) against 1.92 (median 1.55) for the tile alone in the same run, where the tile and the
showcase patch the same units (10.5 built a tick, with 10.5 elements, 69.5 prepainted and painted, 10.5
primitives and 40 layout nodes reused). A sample of the release addon over the patched ticks of the showcase found the
rest in three walks over the page, each in something that did not move.

| Walk | Share of a patched tick | Grows with |
|---|---|---|
| `sync_motion_states` (half of it `parent_inherited` walking each motion's ancestors, the rest comparing each motion's source with its last) and `running_motion` | 26% and 5% | the motions on the page |
| `Scene::rebuild_from_operations` (every primitive of the frame put in the scene's bounds tree again) | 22% (95% with 1000 boxes in view) | the primitives in view |
| `step_clocks` sampling and parsing the bindings of every element that binds a motion value, and `preview` parsing those of every element last built with one (`binding_due`) | 52% with 1000 bound boxes, 18% in `preview` | the bound values on the page |

**What changed.**

- **A tick steps the motions that move.** `GpuixView::sync_motions` (every frame) notes the motions that
  moved and a hash of what a sync reads but the clock (the tree and its revision, appearance, viewport, the
  hover and press chains, the focus-visible element), and `step_motions` (a tick) steps only those while the
  hash is the same, and all of them when it is not or when a measurement waits for a frame to take it. A
  motion that moves at a tick moved at the one before, since an animation is active from its start (its delay
  included) until it ends, and only a change of what a sync reads starts one. `preview_tick` takes its
  running motions from the same list. `getBuildCounters().motionsSynced` counts the motion elements a sync read.
- **A tick makes its scene again from the draw orders it had.** A draw order is one more than the greatest
  of those of the bounds inserted before it that intersect it, so it is a function of the sequence of bounds
  a scene took, and a patch changes how operations paint, seldom where. A scene keeps the bounds and the
  order of each insert since `clear` (`order_log`, and the log of each group it ended, by the id it has in its
  build), and `rebuild_from_operations` compares the bounds it inserts with that log: while they are the
  same, in the same sequence, it takes the orders from the log and does not ask the tree (zed
  `Scene::insert_bounds`). A sequence that is not the one reused (something moved, a quad that gained a
  background and lost its strips, a group that faded out) is rebuilt with the tree, as before, after the
  insert that found it, and the tree a rebuild did not fill is made from the log when something is inserted
  afterwards. `getFrameWorkCounters().boundsInserts` counts the orders worked out with a tree. The
  comparison of what an operation paints with its recording (`PaintOperation::differs_from`) formatted both
  through `Debug` to two strings for every operation of every unit, 8% of a tick of the tile; it formats the
  recorded one once and writes the other through a comparison that stops at the first character that
  differs, which is early for what changes (a colour, an opacity). The answer is the one of comparing the
  two strings.
- **A tick samples the values that move and parses no binding.** `MotionValueStore::step` samples again what
  moved by itself when it was last sampled (the values elements bind and the ones read for a drag) and the
  values a write since can have changed, and leaves the rest as the frame left them, since only time and a
  write move a value. A tick does that while the bindings, appearance and viewport are the ones the last
  frame sampled (`GpuixView::step_values`, keyed on the tree and its revision as `step_motions` is), and a
  frame samples them all as before (`sync_values`). `BindingIndex` holds the bindings parsed once per
  revision of the tree, by element and by value: `value_demand` asks only the elements bound to a value that
  moves, and `preview` and a frame ask `element_due` of it. `getBuildCounters().motionValuesSampled` counts
  the values sampled for the elements that bind them.

**Measured** (release addon, patched ticks alone, `r20-patched-tick-cost`, window scale 1): FF's showcase,
mean 4.11 to 1.82 ms (median 4.27 to 1.18), against 1.92 (median 1.55) for the tile alone in the same run, so
nothing of the page is left in it; at window scale 2, which is what the machine gave for the sweep, 3.05 ms
against 3.06 for the tile. The full sweep (`r19-showcase-frame-kinds`, window scale 2): the showcase from 4.0 to 4.2 ms
a draw and 28% of a core to 3.3 to 3.7 ms and 21 to 22%, `showcase-no-shimmer` from 2.8 to 3.1 ms and 19 to
20% to 2.0 ms and 13%, the tile unchanged. Beside
a loop, in a scratch run of 300 ticks, median per patched tick: 1000 plain boxes in view 1.87 to 0.125 ms;
1000 resting bound boxes 2.39 to 0.21 ms (none 0.05).

**Guards.** `animation-frame-work`: a loop beside 400 resting `motion.div`s syncs one motion a tick as the
loop alone does and does the same work, and the frame built whole after a resize syncs all 401, then the
ticks one again; a loop bound to a colour beside 400 values that rest samples one value a tick and does the
same work, the frame after a resize samples 401 or more, and the ticks after it one again; a colour loop
beside 120 boxes in view works out the orders of the loop alone (5 or fewer a tick) and the frame built whole
after a resize works out those of the 120 again. `scene::tests` (a scene made again with other colours is
the scene painted so, groups, a layer and glyphs included, with no order worked out; after something moved
it is the scene painted so; a rebuild twice, and an insert after one),
`patch_tests::a_tick_that_changes_one_root_costs_the_same_whatever_the_page_holds` (`bounds_inserts` 10 for 5
ticks of the unit on 10 and on 1000 rows), `cargo test -p gpui --lib --features test-support scene:: patch`:
92 passed.

**Left.** The cost of the units themselves, which the showcase and the tile now share. The glyph unit parses
its svg (usvg `parse_svg`, 5% of a tick), rasterises it, and makes a Metal texture and releases the old one
(`PlatformAtlas::get_or_insert_with` and `drop_image`, 14%, IOGPU calls), since the morph of `d` is a new
image every tick; and what a unit paints is still compared with its tape through strings (see the
deviations).

### Round 14b: #77, #78, #79

What an idle or quiet window costs. #77 and #78 are about frames and wakes (see also Patch 2), #79 about
memory. All numbers are from a live window with the release addon, on a 75 Hz display unless they say
otherwise.

#### #77: JS is woken when the main run loop has work

**Problem.** The wake was the display link's output callback, and the link ran for as long as a window was
visible. An idle one-text window woke JS 75 times a second (FF measured 120 on a 120 Hz display, about 2.3%
CPU), `tick()` ran each time, input waited for the next vsync because AppKit events are only drained inside
`tick()`, and a 100 ms fallback timer ticked an idle app besides.

**What changed.**

- **A run-loop watch (zed, `gpui_macos/src/run_loop_wake.rs`).** A helper thread with its own kqueue holds two
  `EVFILT_MACHPORT` knotes (no message is received; `EV_DISPATCH`) on the ports the main run loop waits on: the
  main dispatch queue's (`_dispatch_get_main_queue_port_4CF`; CF puts it in the run loop's set only while
  inside `CFRunLoopRun`) and the port set of `kCFRunLoopDefaultMode`. That set is found once on the main thread
  by adding a sentinel `CFMachPort` source to that mode alone, taking the port set that holds it and removing
  the sentinel (a panic if there is not exactly one). When a knote fires the helper calls the hook, so a wake
  is input, a main-queue block (the display link's frame step included) or a run-loop timer.
  `run_loop_pumped()` enables the knotes again, and a message still pending fires at once, so nothing is lost.
  `set_run_loop_wake_hook(None)` stops the thread through an `EVFILT_USER` knote and joins it.
- **The display link runs on frame demand.** `schedule_frame` and `frame_waker` start it for a visible
  window and it stops after 2 steps in which nobody asked for a frame (`IDLE_STEPS_BEFORE_STOP`; about 17 ms at
  120 Hz). An occluded window never starts it. The occlusion, screen and key-status starts stay, and the idle
  count stops them again.
- **A draw outside a frame request asks for a frame** (gpui `Window::draw`). It leaves `needs_present` for the
  next request to present, and a link that is stopped had nothing to wake it for that request. Not covered:
  `needs_present` set by `patch_frame` in a draw outside a frame request (`patch.rs`, left as it is).
- **gpuix.** `setFrameWakeCallback` installs the hook through `set_run_loop_wake_hook`; `tick()` clears
  `pending` and then calls `run_loop_pumped()` on every path; `startFrameLoop` arms no timer on the wake path
  (`FRAME_WAKE_FALLBACK_MS` is gone, `frameMs` keeps the timer loop).

Changed from Round 11 (#56): a frame step that the display link merged during a long tick runs in the next
tick right after the JS tasks, and two steps can run in one pump when a step outlasts a vsync, where it was
dropped until the next vsync. One frame per JS task still holds, and `pending` still clears when `tick()`
returns.

**Measured** (75 Hz display, one-text window):

| | before | after |
|---|---|---|
| Idle: JS wakes per second | 75 | 0 |
| Idle: `tick()` ms per second | 6.9 | 0 |
| Idle: CPU | 1.45% | 0.29% |
| A DOM write after idle | | first draw 20 to 25 ms after the write, 3 wakes |
| Focused input, caret blinking | | 2 draws/s, 8 wakes/s |
| Color animation (r16), draws and presents | 75/s | 75/s |

On a 120 Hz display FF measured 120 wakes/s and about 2.3% CPU idle before. For comparison, Chrome with an idle
one-text window has the renderer and the GPU process at 0% (its browser process was about 9.5% on a fresh
profile, which is not frame production).

**Known.**

- Input events wake JS once per event batch; they are not coalesced to the vsync. A tick costs an estimated
  0.2 to 0.3 ms of CPU.
- The watch alone does not see input: AppKit hands an event to the main thread through a run-loop block and a
  wake-up that CoreFoundation drops while nothing runs the loop. [#93](#round-14c-93) added what #77 lacked.
- Apple's app-launch measurement (`libapp_launch_measurement`) polls the main queue every 3 ms, for up to
  about 500 polls after the first present when the startup frames are slow. Each poll is main-queue work and
  wakes JS, so a few seconds after launch cost extra ticks.

**Guards.** zed: `gpui_macos` `run_loop_wake` lib (1: the set is found twice alike, is not the main queue's
port and has members) and `tests/run_loop_wake.rs` (`harness = false`, it pumps the main run loop on the main
thread): a `dispatch_async` from another thread and a `CFRunLoopTimer` each wake the hook while nothing runs
the run loop, neither runs before the pump, the hook stays quiet for 300 ms after a pump, work that arrives
while the knotes are disabled wakes at the re-arm, and `None` stops the wakes; gpui `window::tests` (a draw
outside a frame request schedules a frame before `flush_effects` can, `on_next_frame` schedules and wakes).
The idle count of the link is not tested: a window needs a display, and it was checked by the measurements
above. gpuix: `frame-loop` (10) and `frame-loop-yields` (2).

#### #78: a window that is not key animates at the display rate

`to_gpui_window_options` sets `inactive_frame_interval: None`; see "Windows that are not key" under Patch 2.
Verified: 75 to 76 draws/s while another app was frontmost, where gpui capped it at about 30 (FF saw about 26);
0 draws, presents and wakes under a cover that occludes the window, and the display rate again within about
1 s after. `window_options_tests` has the unfocused case (an unfocused window is not throttled).

#### #79: the footprint of a window with one text

One window and one text, 1x display, footprint: 70 to 65 MB with fonts, 68 to 63 MB without.

**Cut.**

- **gpui's profiler journal ring** (4 MiB). Every `App` allocated it and wrote to it on each task poll, draw
  and present, though only Zed's hang detection and `bench_context` read it. Its slots are now allocated by
  the first collector, and nothing is recorded before one exists. gpuix never reads it. Zed asks at startup, so
  its behavior is unchanged.
- **A registered font is held once.** font-kit's `Handle::from_memory` kept the bytes in an `Arc` and copied
  them into a `CFData` for the validation and again for every `load()`, so a font was held two to three times.
  `add_fonts` now builds the `CTFont` for an owned single TrueType or OpenType file from a no-copy
  `CFData::from_arc` over the bytes. Collections and resource-fork fonts keep font-kit's path, which unpacks
  them into a new buffer. On Arial Unicode.ttf (23.3 MB), registering it and laying out a line added 47.9 MB
  before and 1.1 MB now.

**Left, and why.**

- **`CAMetalLayer` drawables**: 2 at rest, 3 while animating. At 1280 x 780 BGRA each is 3.9 MB at 1x and
  15.2 MiB at 2x, so about 30 MB on a Retina display. `maximumDrawableCount` stays 3, because with 2
  `nextDrawable` blocks when the GPU and the compositor together take longer than a vsync.
- **The JavaScriptCore heap** (App-Specific Tag 1, 17 to 20 MB). It is there with no window: it is the JS that
  was loaded.
- **ColorSync transform tables**, about 11 MB in Malloc Small, from AppKit and CoreAnimation colour matching
  (the Dock icon draw, `CGColor` conversion on commit).
- **gpui's instance buffer** (2 MB) and **A8 glyph atlas** (1 MB).
- **The Objective-C and AppKit runtime**, under 4 MB.

Floors measured for comparison: a bare `bun` 4.4 MB, with the addon loaded 7.8 MB, a compiled app with its JS
and no window 26 MB.

**Guards.** zed: `gpui_macos` lib with font-kit (45: an owned TTF gives a native handle and a collection a
memory one, and the opsz test, Chrome's advances for the Inter subset registered as owned bytes, passes on the
new path), gpui journal tests (3 new: no slots, no offered entries and no sequence before a collector; entries
offered after one are collected; a writer without a collector records nothing, then records once one exists).

### Round 14c: #93

#### #93: input to an idle window wakes JS

**Problem.** After #77 an idle window did not wake on input: a key down, a mouse move, a click or a scroll
sat in AppKit's queue until something else pumped, and FF's Command Menu looked frozen after about 4 s idle
(15 of 15 trials without a wake in 600 ms). AppKit receives every WindowServer event on
`com.apple.NSEventThread` (a source1 into HIToolbox `MessageHandler`, `PullEventsFromWindowServerOnConnection`
and `PushToCGEventQueue`, or SkyLight `CGSSnarfAndDispatchDatagrams`) and hands it to the main thread with
`CFRunLoopPerformBlock(mainRL, kCFRunLoopCommonModes)` and `CFRunLoopWakeUp(mainRL)`. Nothing runs the main run
loop under gpuix (the main thread sits in libuv), so its `ignoreWakeUps` is set and CoreFoundation sends no
message. Neither port the watch holds (the main dispatch queue's and the default mode's set) gets one, and the
event enters AppKit's queue only when a pump runs the loop. Before #77 the 100 ms fallback timer hid it.

**What changed (zed `gpui_macos::run_loop_wake`).** A `kCFRunLoopBeforeWaiting` observer in the common modes
on NSEventThread's run loop (found by its name, with `task_threads` and `_CFRunLoopGet0`) calls
`dispatch_async_f` with a no-op on the main queue. That is a message on a port the watch already holds, sent
after the source callout that queued the event. It is level-triggered, so a wake that lands while a tick has
the knotes disabled is not lost. It is not a timer: NSEventThread ran 0 iterations over 5 to 15 s idle.

**Measured** (post to the wake of JS / post to its handler):

| | wake (ms) | handler (ms) |
|---|---|---|
| keyDown | 0.5 to 1.2 | 4.1 to 7.7 |
| mouseMove | 0.4 to 0.9 | 3.3 to 6.2 |
| click | 0.1 to 1.4 | 0.7 to 5.1 |
| scroll | 0.1 to 0.5 | 0.4 to 1.2 |

Before: no wake at all.

**Known.**

- An event posted with `-[NSApp postEvent:atStart:]` from a thread that is neither NSEventThread nor the main
  thread still does not wake. It queues a block on the main run loop with an ignored wake-up and never touches
  NSEventThread. No real input takes that path; FF's `poster.ts` Worker trials do.
- The observer uses the private `_CFRunLoopGet0` and the thread name `com.apple.NSEventThread`.
- Apple's launch measurement still wakes JS about 2.5 s and 5.3 s after start.

**Guards.** zed `tests/run_loop_wake.rs` (an NSApplication and a window that are never active): 2 s idle with
no call of the hook; a key down, a mouse move, a click and a scroll posted on NSEventThread each wake the hook
within 20 ms (0.1 to 3.5 ms) and are dequeued; a window moved with `setFrameOrigin:` comes back from
WindowServer as an AppKitDefined event that wakes the hook. Without the observer it fails at `key down: no wake
within 20ms`. gpuix: the test-support `GpuixRenderer.postNativeEvent(kind, x?, y?)`
(`packages/native/src/native_event.rs`) builds the NSEvent for the window and posts it from NSEventThread with
`gpui_macos::post_event_on_event_thread`; nothing reaches the OS and the app is not activated. A scroll has no
NSEvent factory, so it is a CGEvent that needs the window number in field 51 and the private
`CGEventSetWindowLocation` (test only). The react `events.test.tsx` frame-loop test opens a visible window
with `focus: false` on the wake path: 0 wakes over 2 s idle once the launch measurement ends, then each kind
wakes JS 0.1 to 1.4 ms after the post and reaches its handler 0.4 to 7.7 ms after it (keyDown through the
window-level `onKeyDown`, nothing focused). It fails at `keyDown woke` without the observer.

### Round 21: #95

#### #95: the test renderer wakes JS for input

**Problem.** `TestGpuixRenderer` had no wake: no `postNativeEvent`, no `setFrameWakeCallback`, and nothing drained
AppKit for its window, so a downstream app could not check #93 (input to an idle window wakes JS and reaches its
handler) headlessly with `createTestRoot`.

**What changed (gpuix only; zed is unchanged).** `TestGpuixRenderer` has the live renderer's surface, same names
and semantics: `setFrameWakeCallback(cb)` (the same `FrameWake` as `GpuixRenderer`: `gpui_macos::set_run_loop_wake_hook`,
true on macOS), `postNativeEvent(kind, x?, y?)` (the same kinds, through `native_event::post` on the test
window's NSView), `requiresTick()` and `tick()`. The test platform is `MacPlatform::new(false)`, whose
`pump_events` panics, so `tick()` drains AppKit itself (`native_event::drain_app_events`): `-[NSApp
nextEventMatchingMask:NSEventMaskAny untilDate:distantPast inMode:NSDefaultRunLoopMode dequeue:YES]` and
`sendEvent:` until none are left, then `CFRunLoopRunInMode(default, 0)` once so the wake's no-op block on the
main dispatch queue is drained, then
`run_until_parked`, then `pending` is cleared and `run_loop_pumped()` re-arms the watch as the live tick does.
The NSEvent reaches the window's own input callback, so the JS events are the ones the `simulate*` helpers
produce. The window stays at (-10000, -10000), is not made key and is not ordered on screen. JS: `TestRenderer`
exposes the four, and its `tick()` also hands the queued native events to JS as `dispatchNativeEvents` does, so
`startFrameLoop(testRoot.renderer)` runs on the wake path. `tick()` draws nothing: frames stay `flush()` and
`drawFrame()`.

**Measured** (react `events.test.tsx`, post to the wake of JS / post to its handler, ms): keyDown 2.5 / 14.0,
mouseMove 0.8 / 7.9, click 1.3 / 7.8, scroll 0.3 / 0.6.

**Known.** The window's startup and the process's launch measurements wake the loop in bursts for about 5 s
after it starts (wakes at 0 to 330 ms, then 2.1 s and 4.8 s), as a live window does, so a check waits for a
quiet second before it posts, and 0 wakes over 2 s idle holds after that. An event posted with `-[NSApp
postEvent:atStart:]` from another thread still does not wake (see #93).

**Guards.** react `events.test.tsx` "wakes an idle test-renderer wake loop for input that AppKit receives, and
runs its handler in that tick": `createTestRoot`, `startFrameLoop(testRoot.renderer)`, 0 wakes over 2 s idle, then
keyDown (window-level `onKeyDown`, nothing focused), mouseMove, click and scroll each wake JS within 20 ms of the
post and reach their handler within 50 ms. With `setFrameWakeCallback` not installed it fails at `keyDown woke`.


### Round 16: #83, #82 and the content-visibility leftovers

Three threads. A forced layout read costs what changed (#83); a scroller keeps the box in view where it is (#82); and the
parts of `content-visibility` that Round 13b left out (layout containment, skipped content in the accessibility tree and
the tab order, find, the first frame's events). zed: relayout boundaries and layout counters in taffy and the layout
engine, `Window::layout_stamp`, layout records, `elements/scroll_anchoring.rs`, `contain_layout`, nested synthetic
accessibility nodes, `Window::next_tab_stop` and `prev_tab_stop`, `TextLayout::place` keeping its bounds, and the flex
percentage base.

#### #83: a forced read costs what changed

**Problem.** A scroll write or a bounds read after any mutation, a colour included, ran a layout-only pass
over the whole retained tree: 0.46 ms at 1k elements and 3.5 ms at 8k, where Chrome lays out nothing for a
paint-only change and the dirty chain for a layout one, and a `scrollTop` write that leaves the position
alone does nothing. Five slices: A (what is dirty, and the counters), B (link resolution), C (layout records for a tree
that was not presented), D (reads share a forced pass) and E (relayout boundaries).

**Slice A: paint-only writes, scroll writes that move nothing, and the counters.**

- **`RetainedTree::take_dirty` says `Clean`, `Paint` or `Layout`**, not a bool. `Paint` is a batch whose only
  changes are style changes by keys of `AnimProp::paints_only` (`opacity`, `boxShadow`, `color`,
  `backgroundColor`, the four `border*Color`, `outlineColor`, `textDecorationColor`, `scrollbarColor`,
  `selectionColor`) and the `borderColor` shorthand (four colours and nothing else). `StyleDesc::differs_only_in_paint`
  copies those fields over a clone of the old style and compares the whole: a field that is not listed, a state
  style (`hover`), a `@container` block and a custom property make it `Layout`. Left out on purpose: `border`,
  `outline`, `outlineWidth` and `outlineOffset` (widths), `filter`, `mask*`, `clipPath` and `transition`
  (a stacking context, a clip, a motion), and the `*Color` of anything inside `hover` and the like. A universal
  layer that only changes these is `Paint` too.
- **`Paint` notifies with `notify_paint_only`** (`invalidate_after_mutation`, and the test renderer's
  `UNFLUSHED_MUTATIONS`, which now holds the strongest kind), so a window whose layout was current stays
  current: a bounds read or a scroll write before the frame lays out nothing, and the frame still builds the
  changed element. `boxShadow` and the outline reach the culling outset (`layout/cull.rs`), but that is worked
  out in the build from the style, and gpui's painted extent is made of boxes, so a stale extent is never read.
  `opacity` crossing 1 makes or drops an effect group, which only paints and stacks.
- **A scroll write that does not move the scroller does nothing**: `scroll::scroll_to` and `scroll_element_to`
  return whether the scroller moved (or will, for a smooth scroll) and the callers invalidate only then.
  It still ends a smooth scroll in flight, as `ScrollableArea::InitiateScrollAnimation` does for a no-op offset,
  and a smooth scroll to where the scroller is starts none. The layout the write needs when the tree is dirty
  still runs.
- **`getLayoutCounters().nodesLaidOut` and `nodesSized`**: the cache misses of taffy's `compute_cached_layout`
  by run mode (`TaffyTree::layout_runs`, `size_runs`). They are two counters because a parent sizes a child
  before it places it, and `measures` already names the calls of a measure function. A hidden node
  (`display: none`) is in neither. Changing the height of one leaf at depth d lays out d + 1 nodes and sizes d,
  whatever the fan-out (`engine_fuzz_tests`); the same read in a list of 8000 elements lays out 7, as in a list
  of 100.

**Guard.** `forced-layout-incremental.test.tsx`, and `batch_tests` in `renderer.rs` for the key list. zed `engine_fuzz_tests`
(the node counts).

**Slice B: link resolution starts from the relations.** `accessibility::links::compute` walked every entry of
`tree.elements` (`display: none` subtrees included) on every layout pass after any mutation: 8 to 11% of a warm forced
pass at 8 to 10k elements, whatever the accessibility state.

- **`getLayoutCounters().relationVisits`** (test renderer): the elements `accessibility::links::compute` read since
  the tree was made. `compute` starts from `RelationRegistry` (labels, elements with an `aria-*` id reference,
  elements by DOM `id`) and is worked out again only when something it reads changed: a relation or `htmlFor`
  prop, an `id` or `role` that is named or on a label, text under a label or a named element, structure
  while a source exists. A tree with neither label nor reference reads none, whatever it holds.
- **Guard.** `accessibility::links` unit tests (document order of duplicate ids and of a control's labels, retargeting
  an id, a reference or `htmlFor`, role, text, detached labels, no visits without sources or after unrelated changes)
  and `forced-layout-incremental.test.tsx` (0 visits in an 8000-element tree with no label, 0 for a width change with
  a label, 20 for a rename at 100 and at 8000 elements).

**Slice C: a layout-only pass leaves a layout record.** A layout-only pass could skip a retained subtree only with
the painted recording of a presented frame, so on a tree that had not been presented since it changed (a page
that mounts, with a `scrollTop` write or a bounds read after each step and no frame between: FF mounts 11 lists
that way) every pass built every element from the JS tree and threw it away, ~28 ms a pass at 5000 elements,
5 us per element against 0.3 warm.

- **The record** (zed `LayoutRecord` on `RetainedState`, next to the painted `Recorded` and never touching it): the
  gpuix token, the engine and root of the build, the entities it read, the `ReplayKey` and the layout generation of the
  root after prepaint, `rendered_serial` when it was made, and a flag the last publication of the build sets. The next
  layout-only pass skips with it (`RetainedLayout::SkippedLaidOut`) under the conditions of a recording (same token,
  no notified entity, node alive, same engine, `shapes_layout_like`), puts the build on the nodes it has when the
  generation or the place differs (`SkipBasis::LaidOut`; a place that is only a translation of the recorded one is skipped
  with a delta: Round 17, "A layout record is skipped at a translated place"), and
  republishes with `RetainedPhase::LayoutRecorded`. A draw ends the records it visits (`rendered_serial` moves, and a draw
  that hits drops it; a draw that builds makes the state anew), builds what it has no painted recording of and
  records that: a draw ignores layout records. (A record made inside the content of a skipped `content-visibility`
  host is not ended by the draws that skip the host, which do not visit it, and a layout-only pass wraps what it builds:
  Round 17.) A pass that is abandoned or whose prepaint a `List` rolls back
  never runs the last publication, so its records are not skipped with.
- **Not recorded:** a prepaint that did anything a skip cannot repeat (`Effects::reasons`, as for a painted recording,
  but not lifts: a pass that only lays out paints nothing, so where a subtree's `position: relative` boxes went in
  the stacking context around it is not repeated, and the publications of the lifted content are in the subtree's
  own tape; `finish_layout_recording` leaves `NonReusable::Lift` out), a subtree that published without an `on_replay` listener, and one that holds a subtree skipped from a
  presented frame (`window.retained.layout_painted_skips`: its tape is a painted one, which cannot nest into a layout
  tape without corrupting `tape::PARENTS`).
- **The tape.** `Tape::laid_out` is a tape of two lists: the prepaint writes, and the writes of the publications the
  prepaint queued (`Taped` queues one publication before and one after its child prepaints, which run in order
  and bracket the `Recorder` of the paint list). Kept in `TapeStore::laid_out`, not `tapes`, and its lists carry
  `tape::LAID_OUT` in the serial, so `automation::Stored::laid_out` holds the boxes it wrote beside `recorded`
  and a pass that lays out can neither forget the box of the recording a draw replays nor be forgotten by one.
  Nested subtrees nest into it as into a painted tape.
- **A refresh and a reference pass end records.** `Window::refresh` leaves `refreshing` set until a frame is
  drawn, which stops a painted recording from being skipped; a layout record made after the last refresh is
  skipped with (`refresh_epoch`), one made before it is not. A verified pass is followed by a pass that builds
  every subtree inline, whose writes go to the lists of the roots around them; a layout record made before such
  a pass (`reference_passes`) is not skipped with after it, since the box a root's tape holds is no longer the
  last one written.
- **An `<svg>` no frame has painted does not exclude its subtree from a layout-only pass.** Its box is its style
  and its document, both in the key; the raster state (`retain_state` is `None` until a paint shows it) only matters
  to a pass that paints, which still excludes it. FF's command-menu has 130 icons, 315 of the 460 elements a steady
  mount pass built.
- **Round 17 builds on the records**: a layout-only pass wraps what it builds (no more inline first build), the records
  of forced content outlast the draws, and a record is skipped at a translated place. See Round 17.
- **Guard.** zed `elements/retained/layout_record_tests.rs` (skip, change inside, moved, draw ends, painted
  recording untouched, abandoned, no listener, nested, a held painted skip, verified, a lifted box inside the cards);
  `forced-layout-incremental.test.tsx` "a forced read on a page that has not been presented since it changed" (with
  and without svg icons; "a forced read on a page whose scroller appeared since it was presented"), `content-visibility-auto.test.tsx` "forced reads inside hidden content that is laid out once
  and kept" (never drawn, drawn visible then hidden, drawn hidden; oracle scene `firstReadKept`),
  plain and with `GPUIX_RETAIN_VERIFY=1`.

**Slice D: consecutive forced reads share one pass.** Every read of an element inside a skipped
`content-visibility` subtree ran a layout pass of its own (and one before it, for a dirty window): ten reads
of the children of one skipped entry were ten forced passes with nothing changed between them. Chrome keeps the
forced layout of a locked subtree clean until something dirties it, so later reads cost nothing there.

- **The record.** `read_after_layout` leaves in `Relevance` (`ForcedRead`) the set of skipped ancestors it forced and
  the window's `gpui::LayoutStamp` read after its trailing invalidation, so that invalidation is not a change. The stamp
  is the invalidator's count of the invalidations not declared paint-only (`notify`, scroll-only ones included, a
  resize, a mutation that can move a box) and the layout engine's count of the boxes laid out to another place or size
  (zed `Window::layout_stamp`, taffy `TaffyTree::box_generation`; it replaced the count of the frames drawn and layout
  passes built in Round 21, #98). The two together say that nothing was invalidated and no box moved, which is what
  Chrome's locked subtree needs to stay clean. The frames that came in between wrote the boxes of what they built and
  skipped the forced content, so the pass's boxes and scroll handles are kept apart for the read that shares it
  (`automation::keep_bounds`, `renderer::keep_scroll_state`, see "A read after a painted frame shares the forced pass").
- **A read while the stamp is the record's** skips the layout it would run first and the forced pass when every skipped
  ancestor of its element is in the set (an element outside any skipped subtree has none), and reads the boxes as they
  stand. One that needs an ancestor outside the set forces the union with the set in one pass, so the boxes read before
  are those of the same pass. `getScrollMetrics` shares the same way.
- **What ends it:** a mutation or any other invalidation that can move a box (a style write, a scroll write, a resize; a
  paint-only one, a colour or an opacity, does not, as Chrome's layout stays clean for it), a frame in which a box
  moved or changed size without an invalidation (a running layout animation; a painted frame that moved none does not,
  Round 21), and a layout pass of anything else that moves a box. `getBuiltElementBounds`, which runs a pass for a dirty
  window, does not end it by itself. The read after a change costs the one pass that lays the change out and forces the
  skipped ancestors (Round 17, #88b: it cost two, because which ancestors skip is read from the last build, so the pass
  that laid the change out had to come first; Round 17, "The first read after a change is one pass").
- **Guard.** `content-visibility-auto.test.tsx`, "consecutive reads ...": ten reads in one entry are one pass (and
  built elements and scroll metrics read after it none), two entries are two passes (the second forces both), a change
  and a scroll write end the share with the values of the new layout, and a painted frame that moved no box keeps it
  with the entries skipped in the frames after it; a paint-only write leaves the share ("consecutive reads and a change
  that moves no box"), a frame in which an animation moved a box ends it. zed
  `test_the_layout_stamp_moves_with_layout_invalidations_and_boxes` and taffy
  `the_box_generation_follows_the_boxes_the_nodes_are_laid_out_to`.

**Slice E: relayout boundaries.** `TaffyTree::mark_dirty` cleared the cache of every ancestor to the root, so a leaf
change inside a 100x100 scroller re-ran the algorithm of each ancestor over its children. Chrome stops the dirty chain
at a relayout boundary (`ObjectIsRelayoutBoundary`, laid out again from `LayoutSubtreeRoot`).

- **Boundary** (vendored taffy, `is_relayout_boundary`; Blink's list for layout correctness, the self-painting-layer
  condition left out as a paint-system reason): placed by a parent and not hidden; a formatting context root (flex,
  grid, flow-root, a block that is absolute, a scroll container, has layout containment or has no children); not a
  flex, grid or table item, not a table, and not an absolute box that takes its place from the static position on an
  axis; and either size and layout containment, or a width and a height that are lengths, no `min`/`max` by content, and
  `overflow` other than `visible` on both axes. A change to the style of the node itself is never the end of the
  chain, since padding, margin and size are the parent's to resolve; the child list and the context are.
- **Walk.** `mark_dirty` stops at the first boundary and registers it as pending. `compute_layout` lays the root out as
  before, then each pending boundary under it, nearest first, whose layout is not current (`Cache::has_final_layout`; a
  size probe does not make it so), with the input it was last placed with (`SavedLayout`, kept by every `PerformLayout`
  call, dropped by a hidden layout). Boundaries not under the computed root stay pending. A boundary that answers
  otherwise than it did (size, baselines, margins, and the content size when it shows its overflow) falls back: the
  chain to the root is dirtied and the layout goes on from there.
- **The nodes above a boundary keep their caches**, but their `dirty_generation` and `layout_generation` are bumped, so
  the retained subtrees, request signatures, painted extent and scroll reach stamps and the intrinsic-width memo see the
  change. A style that `settle_containing_blocks`, the natural-width probe or `detach_for_relayout` writes goes through
  `set_style` and the child-list methods, which end the chain at the first boundary and leave its layout to the
  `compute_layout` that follows in the same pass.
- **Counters.** `TaffyTree::boundary_relayouts` and `boundary_fallbacks`, and gpui's `LayoutCounters::boundaries_laid_out`
  and `boundary_fallbacks`. They are not in `getLayoutCounters()`: the test renderer's `nodesLaidOut` shows the effect.
  `TaffyTree::set_relayout_boundaries(false)` turns them off (the fuzz compares both).
- **Guard.** taffy `relayout_boundary_tests` (the predicate, 3 nodes laid out where the chain was 5, nested and pending
  boundaries, hidden, fallback, and a differential fuzz of random trees and edits against a tree built new: 4000 seeds,
  120000 layouts, 80075 boundary relayouts, 15491 fallbacks); gpui `engine_fuzz_tests` (random trees with absolute,
  fixed and sticky nodes, keywords, natural widths and abandoned passes, with boxes that clip or are contained: seeds
  1 to 1500) and a leaf change inside a 100x100 scroller of blocks (4 nodes laid out, 6 with boundaries off, an idle
  frame lays out nothing).
- **Deviation.** Floats are left out of the fuzz: a block laid out from its cache does not hand the floats that escape
  it to the block formatting context around it, boundary or not.

#### Layout containment of `content-visibility`

**Problem.** Chrome gives `content-visibility: auto` and `hidden` layout containment whether or not the content is
skipped (`EffectiveContainment` keys off the computed value): the box is a block formatting context root, so no margin of
a child collapses through its top or bottom edge, and the containing block of `position: fixed` descendants as well as of
absolute ones. Size containment applies `contain-intrinsic-size` after the children are laid out, so a child's percentage
height under an `auto`-height skipped box is `auto` (0 if empty; 100 under an authored `height: 200px`, which is definite).

**Design.**

- **Margins and fixed boxes.** `layout::style::apply` sets zed's `contain_layout` on every box with a `contentVisibility`
  other than `visible` (a non-atomic inline is part of a paragraph and builds no box, so it gets none). taffy lays a
  `Display::Block` with it out as a formatting context of its own, as one with size containment already was; its own
  margins still collapse with its siblings'. gpui's `NodeMeta::contains_layout` makes `containing_block` resolve a fixed
  node to its nearest layout-contained ancestor instead of the root, and such a box counts as positioned for absolute
  descendants; at prepaint `Window::with_frame` pushes a frame for it, so `Escape::Viewport` is placed and clipped by
  the contained box, and a deferred draw or patched unit keeps it. gpuix `layout/containing_block.rs` (`contains_fixed`,
  `reaching_ancestors`) and the hit walk of `retained_tree.rs` read the same box.
- **Percentage heights.** `LayoutInput::height_from_containment` is set by `contained_inputs` when `contain_block_size`
  gave the known height (an authored or parent-given one leaves it false). Block layout takes the percentage base of the
  children, and the sizes they resolve against, from the known height only without it. A flex container carries it as
  `AlgoConstants::percentage_base`, which every place that resolves an item's size, min and max, margin, padding,
  flex-basis or inset, or passes a parent size to an item, reads; the container's own size, free space, stretch, line
  sizes and absolute children still read the contained height. A grid resolves against its area, where Chrome gives the
  same 100. A flex column whose height was not given to it (`AlgoConstants::column_height_indefinite`) passes the flag
  to its items too, since the height it flexes them to is not definite in Blink either: the `flex: 1 1 0%` item of a
  card that is itself an item of an auto-height column has the content for its basis. The flag is part of the cache key.
- **The height of a flex column is the sum of its items' hypothetical main sizes**, as in Blink
  (`max_sum_hypothetical_main_size`, flex_layout_algorithm.cc): the content of an item enters only through its
  automatic minimum size, so `flex: 1 1 0; min-height: 0` in a column of automatic height is as high as its padding
  and border, not as its content (`longest_line_main_size` in taffy `flexbox.rs`; a row keeps its flex-basis
  logic). A percentage `flex-basis` of a column height that is not known (`flex: 1`, `50%` in an auto-height column) is
  the content, not the `height` of the item (`determine_flex_base_size`, Blink `ResolveBlockLengthInternal` `kMain`); a
  row keeps the `width`, as Blink sizes a shrink-to-fit row from the contributions of its items and taffy from their
  bases. `flex-column-size-oracle` pins 563 rows at 1x and 2x.
- **A scroller in forced content.** The scroll handles a read sees (`SCROLL_HANDLES`) are synced at the end of a build
  for the elements that have a box, and `scroll::has_scrolling_box` asked `is_hidden`, which counts a skipped ancestor as
  built last time. `is_hidden_in_pass` and `is_skipped_by_unforced_ancestor` leave out an ancestor that the pass forces,
  so the handle of a scroller in forced content is synced and `scrollHeight` is its extent. A write still finds no
  handle, as the pass that follows a read is not forced.

**Guards.** `content-visibility.test.tsx`: 37 `contain-*` cases of `fixtures/content-visibility-cases.json` against
headless Chrome (margins that stay inside a `hidden`, `auto` or padded target and collapse through a `visible` one; a
target's own margins collapsing with its siblings'; a fixed child with `top/left`, insets and `bottom/right` placed in the
padding box of a target with a margin, padding and border, and at the viewport under `visible`; `height: 50%` in a block,
a flex column and row, and a grid under `hidden` with an intrinsic size, an authored height and padding, and an on-screen
`auto`), and the `hidden`-element test that holds `scrollHeight` at 200. `content-visibility-auto.test.tsx`: the oracle
scene `forcedScroller` (a scroller in a `hidden` element and in an `auto` one far below, before and after a scroll:
`[0, 0, 40, 200, 40, 50]` both). taffy `containment` (a child margin through a plain and a contained block,
a percentage child under a contained auto height in a block, a flex column and a flex row, a stretched and a growing
item still filling the contained height); gpui `div` tests (margins of a layout-contained box, a fixed box placed and
clipped by a contained ancestor, a positioned box between them skipped, a static contained box containing fixed and
absolute boxes); gpuix `containing_block` and `content_visibility`. The oracle page asks for a frame and checks
`checkVisibility({ contentVisibilityAuto: true })` after the read, since about one run in six Chrome had made its first
rendering update before the case was on the page.

**Deviations.** None known for containment. 24 cases of `content-visibility.test.tsx` fail against Chrome for the test
window's scale, with or without it.

#### #82: scroll anchoring (`overflow-anchor`)

**Problem.** A scroller that was scrolled away from its start showed other content when the layout above the
view changed: a row prepended, a row above that grew or was removed, a `content-visibility: auto` entry above
that took its real height when it was scrolled to. Chrome moves the offset by what moved the first box in view
(CSS scroll anchoring), and a list that loads older pages above the reader, or a virtualised-by-CSS feed, needs it.

**Design**, after Chrome's `scroll_anchor.cc`.

- **zed, `elements/scroll_anchoring.rs`.** `Style::overflow_anchor` (`OverflowAnchor { Auto, None }`, `Styled::overflow_anchor`).
  The anchor is the first box of the scroller's content, in layout order, that is fully in view, else the first partly
  in view with nothing better inside; a box with `overflow-anchor: none` (and its subtree), a zero-size or `display: none`
  box, a box not in view, a fixed box, a sticky box with an inset and an absolute box whose containing block is outside
  the scroller are skipped. After the layout the offset moves by `round(new block start) - round(saved block start)`,
  clamped, and the saved start is not measured again. A different offset from the one the step left, an anchor that left
  the scroller, and a scroller at offset 0 drop the anchor. The taffy engine flags a node whose insets, margin, padding,
  size, min/max size, position or `contain-intrinsic-size` changed in the pass, and a flagged node from the anchor to the
  scroller with a nonzero delta drops the anchor without scrolling. It runs in layout-only passes too, so a forced read
  sees the new offset.
- **The pick is before the layout.** `Interactivity::request_layout` notes a tracked scroller in the engine and
  `compute_layout_in` calls `ScrollAnchoring::before_layout` for each before it computes the layout, while the nodes hold
  the boxes of the pass before and the tree and styles of this one: Blink's `NotifyBeforeLayout`. Picked at the end of the
  prepaint before, a scroll in the frame of a prepend (every wheel frame), a removed or hidden
  anchor, a `none` set on the anchor and a scroller turned on in the frame of the change would compensate nothing. A node
  created in the pass has no box and is not picked.
- **gpuix.** `overflowAnchor: "auto" | "none"` (`css/overflow_anchor.rs`, an invalid value is ignored; not inherited, not
  animatable; a state style overlays it) is given to every box by `layout::style::apply`, since a descendant needs it to
  be left out as a candidate. `scroll_reporter` reports nothing in a layout-only pass, so the `scroll` event of an
  adjustment runs in the frame after the one that laid it out, as in Chrome, and a write after a mutation fires once.

**Guards.** `scroll-anchoring.test.tsx`: 32 scenes of `fixtures/scroll-anchoring-scenes.ts` against headless Chrome
(`fixtures/scroll-anchoring-oracle.*`, regenerate with `bun fixtures/scroll-anchoring-oracle.ts [scene...]`), each
step compared on `scrollTop`, the first row in view with its top, and the frames the `scroll` events run in: rows
inserted above, inside, below and at the end; resized, collapsed (`height: 0`, `display: none`) and removed above;
a partly visible anchor; fractional heights (50.4 to 50.6 is a pixel); margin, top, height and padding of the anchor,
its parent and the scroller; `none` on the scroller, the first row and a parent, and toggled in the frame of a
change; offset 0; a write before and after the mutation in one task; the end of the range and a clamp; a removed, a
hidden and a vanished view; forced reads; content-visibility lists of 60 entries scrolled to the end and back in steps of
150, 60 and 400px. zed `scroll_anchoring` (18); `css::overflow_anchor` and the state-style overlay in gpuix.

**Deviations.**

- A nested scroller is anchored as one box and is not looked into, there is no priority candidate for the focused
  editable or a find-in-page match, and `scroll-padding` does not shrink the area a box must be in. None is exercised.
- Chrome compensates nothing in the layout after a second change of `top` of a relative anchor, as if the flag stayed set
  until the box is laid out (scene `anchorTopStale`, `it.fails`).
- A frame reports the offset it paints when it differs from the last painted, so an offset written away and
  compensated back in one frame is no `scroll` event (Chrome fires two), and the event of a write runs a frame after
  Chrome's, as for every write.
- `<virtual-list>` keeps its own row-index anchoring.

#### Skipped content in the accessibility tree and in the tab order

**Problem.** Chrome keeps the content of a skipped `content-visibility: auto` element in the accessibility tree
(roles and names come from the DOM, no layout is needed, and `aria-labelledby` into it still names) and lets Tab
reach a focusable element in it, scrolling to it and making the element relevant; `hidden` content is neither (Chrome
156: `Accessibility.getFullAXTree` with a screen reader on lists every heading and button of the page but those of the
`hidden` entry; Tab from the last built button lands on the first button of the skipped entry). Nothing below a skipped
element is built here, so both come from the retained tree.

**Accessibility.** With the tree active, `build_host_container` gives a skipped `auto` element the nodes of everything
below it (`accessibility/skipped.rs`). `exposed` walks the retained tree in document order (`contents::flatten`, so a
flex `order` does not move a node, as in Chrome) and
leaves out `display: none`, `aria-hidden` and `inert` subtrees, nodes under `visibility: hidden` (a descendant that is
`visible` again is kept), the parts of a paragraph (`is_part_of_paragraph`: an inline `span` has no node when built
either) and everything below a nested `hidden` element, and goes down through a nested `auto` one. The node of an
element is made by the code that makes it for a built element: `apply_accessibility` runs on a throwaway `div`, and the
role, `write_a11y_info` and the relations are read back from it (`Element::a11y_role` and the rest), so a role, an
`aria-*` prop, an `id`, a name from a `<label>` or from a reference, the text of a `<text>` (`describe_text`, shared with
the built path, and the same for a skipped `<text>` itself, whose plan is made only for it) and the default roles
(`accessibility::default_role`, now also read by `build_host_container` and the editor) are never written twice. The
skipped element hosts the nodes with gpui's `a11y_synthetic_children`, nested (`A11ySubtreeBuilder::push_nested`,
`push_relation`; the id of a node is the hash of the host's and the element's), and gets a node even with no role of its
own, a `Group` (`apply_hosting`), which it has only while it hosts something. The nodes are recorded and replayed with
the element; they are made when the tree is active at the build, and gpui's replay key already tells a window whose
tree turned on or off. A change below the element moves its `subtree_revision`, and a link that changed
(`links::take_changed`) makes its ancestors build, so a name that comes from a label or a reference follows.

**Tab.** gpui moves focus along the tab stops it painted. `focus::step_focus` first asks `focus/skipped.rs::target`:
the focusable elements (`tree.focusable_ids`, so `inert`, `visibility: hidden` and a disabled control are out already,
and a negative `tabIndex` is no stop) that have a skipped `auto` ancestor and no `display: none` or `hidden` one are
put in tab order (the `tabIndex` group, then the path of child indices from the root) with the stop gpui would pick
(`Window::next_tab_stop` / `prev_tab_stop`, which only read) and the focused element, and the one that is met first
wins, wrapping like gpui does. The element has a focus handle already (`sync_focus_handles` makes them for every
focusable element, built or not), so `change_focus` focuses it and the events run. It is not built, so there is no
box to scroll to: `read_after_layout` lays out its skipped ancestors as a read of geometry does and the scrollers
around it scroll to its nearest edge from that box (`scroll_element_into_view_at`). The frame after builds the element,
which holds the focus (`wanted_skipped`). A tree without an `auto` element costs one check per Tab.

**Guards.** `content-visibility-a11y.test.tsx` and `fixtures/content-visibility-a11y-oracle.*` (regenerate with `bun
fixtures/content-visibility-a11y-oracle.ts`): the headings and buttons of the page with a screen reader on against
Chrome (40 entries, one `hidden`); the nodes and their order, the role of the entry kept, `aria-hidden`, `display:
none`, `visibility`, `hidden` and nested `auto` content, `aria-labelledby` both ways, a change below the entry, one
node when the entry is built and when it is skipped again, the tree turned off and on, a skipped `<text>`; Tab and
Shift+Tab from the last built button over `auto` entries and over `hidden` ones against the focus order of Chrome,
the entry built and the button scrolled to the bottom edge, wrapping, and no stop in `display: none`. Also with
`GPUIX_RETAIN_VERIFY=1`. gpuix `focus::skipped` (7 unit tests of the choice).

**Deviations.**

- Chrome centers a focus target that is out of view; gpuix scrolls it to the nearest edge, as it does for a built
  target (Tab, `focus()`): the first Tab into the entry is scrolled by about half a window less than Chrome's.
- The nodes have no bounds and no actions (no Click, no Focus): a screen reader finds them, reads them and cannot press
  them until the element is built. Chrome activates the element for any of them.
- A skipped element with no role is a `Group` here and a `generic` there. An `aria-hidden` or `inert` element leaves its
  whole subtree out, where a built one keeps its node, hidden; the same for a name that comes from one.
- A custom element is described by its props (`role`, `aria-*`, the default role of `<img>`, `<input>` and
  `<textarea>`, an input's `value` and `placeholder`); the state an editor keeps for itself (typed text, a number
  field's range) is not known until it is built. `<virtual-list>` rows and `<anchored>` content below a skipped element
  have no node of their own.

#### Find in skipped content

**Problem.** Chrome's find buffer (`FindBuffer::ShouldIgnoreContents`) leaves out the content of a locked element that
find-in-page cannot activate, which is a `hidden` one, and searches the content of an `auto` element whether it is
skipped or not; a match that becomes the active one activates the lock (`ActivateFindInPageMatchRangeIfNeeded`, kept
unlocked for two lifecycles) and the page scrolls to it. `GroupList::collect` walked the retained tree with no such
check: `matchCount` counted the matches below a `hidden` element, only the built runs got washes and ordinals, so a
skipped match before built ones shifted their numbers, and a match in skipped content could never show as the active one.

**Design.** `collect` leaves out what is below a `hidden` host element (`content_visibility::found_in`, the declared
value, and `same_paragraphs` moves `search_revision` when `content-visibility` changes, so the cached groups follow) and
tags each group below an `auto` element with its id (`Group::autos`, innermost first). `MatchSet` keeps, from that, the
`auto` elements around each match (`autos`) and how many matches each element holds (`held`). Skipped `auto` content
is in the total. Numbering stays in document order: an `auto` element that skips its content announces its matches
in its prepaint listener (`watch`, `search::announce_skipped`) as one entry of `count` ordinals at its `DocPos`, which
`number` (split out of `wash`) sorts in with the runs that announced theirs, so the built matches after it keep the
numbers they have in the document. The reveal: numbering notes the `auto` elements around the active match (the
element of the skipped entry that holds it, or the `autos` of a painted match), `search::settle_frame` returns them, and
`observers::frame_sweep` calls it in the paint of every frame (a frame with no wash to paint, because every match is
skipped, would number nothing otherwise) and hands them to `Relevance::set_found`. `wanted_skipped` treats them as it
does the focus within: the sweep finds the element wants to build, the frame after builds it, its match paints as the
active one, and it stays built for as long as the match is active. A reveal that lasted only the frame that found it
would build and skip the element on alternate frames when the active match is far from the viewport.

**Rejected.** Numbering skipped matches by their index among the collected ones alone (`ordinal = offset + index`): the
native runs (`<code>`, `<markdown>`, `<diff>`) are numbered between the retained ones, and only the sort by `DocPos`
puts them in place; the skipped element has a `DocPos` where its matches have none. Taking the `activeIndex` for the index
of a collected match to find the element to reveal: the same reason.

**Guards.** `content-visibility-find.test.tsx` (5): the total counts 39 of 40 entries, the far ones skipped and the
`hidden` one out; text only in the `hidden` entry finds nothing and builds nothing; stepping `activeIndex` to a far
entry builds it, paints its match as the active one and keeps it built over six frames, and the entry of a match that
stops being active is skipped again; past the `hidden` entry the numbering skips it; scrolled to the end with the first
entries skipped, the built matches keep their document-order numbers; a change of `contentVisibility` follows. Also with
`GPUIX_RETAIN_VERIFY=1`. gpuix `text::search` (3 unit tests: collect, numbering of skipped matches, settle) and
`content_visibility` (`set_found`).

**Deviations.** Matches in `<code>`, `<markdown>` and `<diff>` text inside skipped content are not counted (that text
exists only while the element renders). Scrolling to the active match is the app's (`createTextSearchController` and
`useTextSearch` do not scroll): a skipped element has the box `contain-intrinsic-size` gives it, so scrolling to the
element works before it is built; Chrome scrolls to the match itself. The `ranges` of a spec index the text of the
collected groups, which have none of the text of `hidden` content. The reveal lasts as long as the match is active;
Chrome unlocks for two lifecycles and then lets the viewport decide, but it has scrolled to the match by then.

#### The first frame's events

**Problem.** Chrome, for an `auto` element that is there before the first rendering update (in the markup, or added in
`DOMContentLoaded`): on screen, `skipped: true` before the first animation frame callback and `skipped: false` after it;
off screen, `skipped: true` only; an element added after the first frame, one event with its state. A style update before
the first lifecycle locks the element, the first full lifecycle makes the first intersection observation synchronously
and unlocks it (`fixtures/content-visibility-auto-oracle.ts`, scene `load`, three runs, the same: `event 0 true`, `event 1
true`, `raf 1`, `event 0 false`, `raf 2`, `event 2 false`, `event 3 true`). Here an element of the first frame was told
only its final state.

**Design.** `Relevance` knows whether a frame with something in it has been drawn (`drawn`: set by the first sweep, or by
`frame_sweep` for a frame whose root has content (`RetainedTree::has_content`) and no `auto` element; the container a
renderer mounts into is drawn, empty, before the first element is). The first sweep tells every `auto` element it has not
told `skipped: true` first (`Swept::locked`, in document order, and each is recorded as told `true`), and then the state it
is in (`changes`): `false` for the near ones, nothing more for the others. There is no `requestAnimationFrame` here, so
both come after the first frame, in Chrome's order and not around a callback; an element that comes after that frame is
told once, as before. An app whose window is drawn empty before its tree is mounted tells its elements once, as
an element added after the first frame.

**Guards.** `content-visibility-auto.test.tsx`: the oracle gets the scene `load`; "the first frame of a window" (the three
events in Chrome's order, nothing more over idle frames; two elements added later told once each; an element after a
frame with other content told once); the scenes of the oracle, which Chrome adds to a page that has settled, are mounted
after a first frame of their own; the bubbling test counts the first frame's two events. Also with
`GPUIX_RETAIN_VERIFY=1`.

### Round 15c: #86 screenshot scale

**Problem.** The test window's scale was the platform window's: 2 on a machine whose primary display is 1x, because a
window on no screen (the test renderer opens it at (-10000, -10000)) took `2.0` from `get_scale_factor`. A screenshot
is the Metal layer's drawable, sized by that scale and not by `Window::scale_factor`, so `setWindowScaleFactor(1)` left
the content in the top-left quadrant of a PNG twice the window, and `setWindowSize` read the platform's scale again
and dropped an explicit factor. Headless Chrome is 1x unless `--force-device-scale-factor`, which wins over the
display and stays.

**Design.** zed: `PlatformWindow::force_scale_factor` (test-support) is that flag. `MacWindowState::forced_scale_factor`
is what `scale_factor()` returns when set, and forcing sets the layer's contentsScale and drawable size, so the
capture is the window size times the factor and `set_frame_size`, `update_window_scale_factor` and screen changes keep
it; `TestWindow` sets its field; `Window::set_scale_factor` calls it before applying the factor. A window on no screen
(or a factor of 0) now takes `NSScreen::screens[0]`'s scale, 1.0 with no screens, as a window nobody placed is on the
primary display in Chrome. gpuix: the `TestRenderer` option `scaleFactor` (the native constructor's second argument, not
a `WindowOptions` field, so a live window has none; default 1, whatever the display) is applied in the open-window
callback before the first frame; `setWindowScaleFactor` keeps its factor across `setWindowSize`.

**Window size.** The window was never off screen: AppKit's `constrainFrameRect:toScreen:` pulled it onto a display when
`orderFront` showed it (a probe that mirrors the open path: asked at (-10000, 10000), it was moved to (-2372, 189) on the
built-in display), and that capped every later `setContentSize` to the visible frame, 921 logical px on this machine
(an 1022 px window was 921; 900x1400 was 949 high). Headless Chrome's window is the size asked whatever the displays.
zed: `GPUIWindow` overrides `constrainFrameRect:toScreen:` (lock-free `keepsFrame` ivar, set at creation) to return the
frame unchanged for a window opened by a platform that called `MacPlatform::keep_window_frames`; every other window
calls super. `gpui_platform::current_offscreen_platform` is that platform, used only by the `TestRenderer`. The test
window is now really on no screen, which is what the scale fallback above is for. Test: react screenshot-scale (900x1400
and 2000x600, `getWindowSize` and PNG size at scale 1 and 2).

**#94: a window bigger than the screen.** FF's `check-home-grid` failed on a 1512 pt display (`1600px: the viewport container is the window wide: 1512 (expected 1600)`) and passed on 1920 pt: its checks open `createTestRoot` windows (`scripts/test-window.ts`, size from `createTestRoot({ width, height })`), and AppKit's `constrainFrameRect:toScreen:` capped the window to the screen when it was shown, which the `keepsFrame` window above no longer does for the test renderer. A live window keeps AppKit's constraint on show and when the user drags it. gpuix has no resize API for a live window (only `TestRenderer.setWindowSize`), but gpui has `Window::resize`, and `setContentSize:` asks `constrainFrameRect:toScreen:` as well (a probe: 900x1400 on this display's 949 pt visible frame gave 900x949). zed: `MacWindow::resize` sets `keepsFrame` around its `setContentSize:` and puts back the value it found, so an explicit programmatic resize is the size asked on any window and nothing else changes. Tests: react screenshot-scale (a window 300 px wider and taller than the primary display read through `screen`, 1812x1282 here: `100vw` x `100vh` box, `getWindowSize` and PNG size at scale 1 and 2, at creation and after `setWindowSize`); gpui_macos `window_resize` (`harness = false`, a shown live window: AppKit's own `setContentSize:` to 3000x4000 is capped, `Window::resize` to it is 3000x4000, and once small again AppKit's is capped as before; 3000x4000 stays under Metal's 16384 px texture limit at 2x, 9000 high aborted).

**Deviations.** The fallback for a screenless window has no unit test: libtest's threads abort creating an NSWindow.

### Round 15c: #84 half-pixel glyph origin

A glyph is painted at its origin rounded half up on both axes, as Skia rounds a glyph position (`floor(position + 1 / (2 * steps))`), and not half down: a text line whose baseline lands on a half device pixel (a label at y 20.5 at scale 1, at 20.25 or 20.75 at scale 2) was one row above Chrome's. zed: `split_glyph_origin` (x in quarters, y whole) and the pixel `paint_emoji` sits in use `round_half_up`; the mono svg sprite stays half down. `glyph-origin.test.tsx` holds the first and last ink rows of 68 labels, tops at every 1/16 from 20 to 21, to headless Chrome's at scale 1 and 2 (`fixtures/glyph-origin-oracle.*`).

### Round 15c: #85 cap height and x-height

The cap height and x-height of a font are the OS/2 table's (version 2 or later, nonzero, below two ems) of its default instance, as Skia's `SkScalerContext_Mac::generateFontMetrics` takes them and Chrome measures `ex` and the `ex` and `cap` edges of `text-box-edge` from them, and not `CTFontGetCapHeight` and `CTFontGetXHeight`: Inter Variable's x-height is 1118 of 2048 units in OS/2 and 1056 in CoreText (1097 at `wght` 700), so an `ex`-trimmed label was 0.36 px short at 12px. zed: `os2_heights` in `gpui_macos/src/text_system.rs`, read through font-kit's `load_font_table`. The cap height of Inter is 1490 either way: the 0.704 em of FF's `tabs-subtle` check is the system font, because `createCheck` renders a bare view without `fontFamily: 'Inter Variable'`. `text-box-metrics.test.tsx` holds 240 trims per scale against headless Chrome's (`fixtures/x-height-oracle.*`), in five instances of `inter-subset.ttf` that Chrome measures alike. gpuix has no `ex` unit.

### Round 15c: text positions (15a leftovers: runs ceiled per run, min-content kerning)

Blink measures every text width from the cumulative advances of an item (a text node), counted from the item's start and ceiled to 1/64 of a device pixel: an item is `ceil(P_end)` wide, a piece of one (a min-content word, the part of an item on a wrapped line) `ceil(P_end) - ceil(P_start)`, a line the sum of its pieces and an element the widest line. zed: `SnapMarks::width` (`gpui/src/text_system/line_layout/snap.rs`) is that model, over the glyph x of a `LineLayout`, the glyph each item starts at (the font runs the line was shaped as, and the `TextRun`s `shape_text_inline` passes `layout_wrapped_line` as `item_starts`) and the kerns; `WrappedLineLayout` keeps the width of each line (`line_widths`, min-content) and of the whole text (`width`). Alignment still reads the CoreText positions, and so do wrap decisions, with the fit test of the next paragraph. A font with no GPOS table (`FontMetrics::legacy_kerning`, found with font-kit's `load_font_table`) kerns through a `kern`/`kerx` table, which HarfBuzz splits: `k >> 1` goes into the advance before the pair and `k - (k >> 1)` into the glyph after it (and its offset, so it is drawn where CoreText draws it). A glyph after a kern is measured `k - (k >> 1)` earlier than CoreText has it, which makes a word after a kerned space (`a To` in Helvetica) and one before it narrower in the measure only. It fits the 51 DOM rows of the first sweep and the 152 kerned rows of `text-measure-oracle` exactly, and only with the split at both edges of a word. The `ceil_to_layout_unit` of the element's size stays, for tab stops and insets that are not whole units. `text-measure.test.tsx` asks min-content and the multi-run row for Chrome's exact numbers (the one-sided bands are gone), with kerned `w`, `mc` and `wrap` rows of Helvetica and Times at 13 and 16px.

**Text nodes.** An item is a text node, not a font run: `Quick brown ` + `fox jumps` is 8601/64 of a pixel as two nodes in Chrome and 8600/64 as one (`Hello {name}` in React is two). `paragraph::styles` keeps a run per text node (it merged neighbours of one style, which only saved runs; a paragraph of one node with no restyling is still plain), `shape_text_inline` passes the byte where each `TextRun` after the first of a line starts, and the cache key of a wrapped line holds them. Shaping is unchanged: runs of one font are one font run, so ligatures and kerning across the edge of two nodes stay, and a kern pair across it is split between the two items as one inside a node is (Blink's split applies within the shaped run): the item before ends `k - (k >> 1)` earlier and the one after is that much wider, so `SnapMarks` measures every glyph, an item edge included, without the share of the kern before it and from the same measure of its item's start. `text-measure-oracle` has the `nodes` rows (two to four nodes, an edge between words, inside a word, and around a kerned space, `w`, `mc` and `wrap`, 984 per scale), pinned exactly.

**Deviations.** A font that kerns through a `kern` table is found by its missing GPOS table; an AAT font with `kerx` has none either, and HarfBuzz splits its pairs the same way (untested here).

### Round 20: line breaking (the fit test, `break-all`, soft hyphens)

Where a line breaks was measured against Chrome on 596 texts (Helvetica Neue, Menlo, Times, Helvetica, Inter Variable with Fluid Functionalism's text styles; the UAX #14, punctuation, `break-all`, `keep-all`, `break-word` and soft hyphen cases) at about 65,500 widths per scale (a sweep of every width from the min-content width to the natural one, the widths at which one line is within three 1/64 of fitting, and fixed widths), 1x and 2x. The places a line may break already were Blink's for `normal` and `keep-all`; what differed was the fit test. gpuix fitted a line when its float width was at most the box plus 1/1024; Blink fits it when the sum over the items of the line of `ceil` to 1/64 of a device pixel of each piece's float width from where it starts on the line (`ShapeResultView::SnappedWidth`) is at most the box cut to 1/64 of a device pixel plus one 1/64 (`LineBreaker::AvailableWidthToFit`, `LayoutUnit::AddEpsilon`). That differs for lines within a hair of fitting: 2167 of about 10,400 edge widths, and 33 (1x) and 18 (2x) of 28,153 real ones. zed: `SnapMarks::fit_units` (the pieces and their kern shares as the width has them, except that the first glyph of a line has no share: Blink shapes the start of a line again, with nothing before it) against `util::fit_layout_units`, in `compute_wrap_boundaries` for greedy wrapping, the min-content pass (a width of nothing) and the overflow-wrap cut, and in `text_wrap.rs` for `balance` and `pretty`, which keep their algorithms. The width a line is reported to have (`line_widths`, min-content, `width`) is `SnapMarks::width` as before: `text-measure` is unchanged. `break-all` breaks as `kBreakAllLineBreakClassTable` says (`line_break.rs`), where gpui's own `may_break_between` broke after any letter that was not punctuation. A soft hyphen is a place to break, and the width of the hyphen Blink draws there (U+2010 if the font has it, else `-`, ceiled to a unit) counts in the fit (`LineBreaker::BreakText`: a hyphen that does not fit sends the line back to the last place that fits a hyphen narrower), which is the only part gpuix has of the hyphen: it still paints none. Counting the width alone puts every break of the 552 soft hyphen layouts where Chrome has it.

`line-fit-oracle` pins 322 (1x) and 307 (2x) texts and widths of that sweep with the lines Chrome puts them in (edge widths where the old rule and Blink's differ, real widths, rows both agreed on, kerned Helvetica and Times, Inter, `break-all`, cut words, soft hyphens), in `inter-text-subset.ttf` (Inter Variable cut to the characters of the texts). The 16 rows per scale that gpuix still lays out differently (below) are left out.

Layouts that differ from Chrome, out of those of a font Chrome and gpuix measure alike, before and after (1x; 2x is the same to 1 %): all 3589 of 52,294 (6.9 %) to 262 (0.5 %); `normal` text of the other fonts 578 of 23,803 to 8; Inter 1623 of 16,816 to 9; soft hyphens 73 of 552 to 0; `break-all` 956 of 4226 to 105; `break-word` and `anywhere` 125 of 2128 to 65 (each). 2x: 3478 of 52,340 to 269. Not fixed:

- **A line shaped again at its end.** Blink shapes the end of a line again when it breaks between two letters that kern, which `break-all` and a cut word do all the time (`ShapingLineBreaker::ShapeLine`, `reshape_line_end`; not at a space, which hangs). The line is then as wide as it would be alone, without the kern with the glyph that starts the next line: `well-kno|wn` is 56.67 in `well-known` and 56.94 alone, in a box of 56.89. gpuix measures the glyphs where they are. Of the 262 layouts left at 1x, 163 have a line that does not fit when it is measured alone in Chrome (86 cut words, 70 `break-all`, 7 other). A fix needs the kern of each pair at a break (`advance` of the glyph against the distance to the next), and the same where a `-` is followed by a digit; not built.
- **Advances of a fallback font.** The CJK, emoji and Thai in the texts are measured by Chrome's fallback font and by CoreText's, which differ (CJK by 2.3 px at 13 px, emoji by 1 px each): 17 % to 60 % of those layouts differ (by set) and did before.
- **Inter at `wght` 550 and above**: its advances in CoreText and HarfBuzz differ by a few 1/64 for some glyphs, which flips a line at the edge (9 layouts of 16,816).

### Round 20: a virtual list's border and top padding (15c leftovers c and d)

A `<virtual-list>` took its border box for its client box: the rows were placed at the list's origin plus its padding (not its border), laid out and clipped against the border box's height, hit-tested through it, and `clientWidth`, `clientHeight`, the largest `scrollTop` and the end of `visibleRange` came from it. The rows were also built from the row the scroll offset is in, so a row that ends in the top padding of a scrolled list (the padding box starts `padding-top` above the scroll offset in the rows' coordinates) was not built, though Chrome paints it. zed: `List::prepaint` takes the padding box (`padding_box`: the unsnapped and the snapped box less `layout_insets(..).border`) as the list's client box, as `ScrollExtent` in `div.rs` does: rows start at the unsnapped origin plus border plus padding, are clipped and hit-tested at the snapped padding box (`ListPrepaintState::viewport`, what `overflow_mask` clips a div to), `ScrollClient` (the range, `fills_viewport`, `dom_scroll().client`) and `viewport_bounds` are the padding box, and the width rows get is the padding box less the left and right padding. The build walk starts at the row holding `max(0, scroll - padding.top)` (`first_visible_row`, no earlier than the scroll top's row, and only when the scroll top is less than the padding into its row), so the rows above the scroll top that show in the top padding are built, painted and counted by `visibleRange`; `LayoutItemsResponse::first_row_offset` is how far the scroll top is below the first row built, which places the rows. A list that does not fill its viewport walks upward until the row above the first would be out of the top padding too (`rendered_height < available_height + padding.top`), so a bottom-aligned list at its end builds the row in its top padding. The overdraw is counted from the scroll top as before; a list without top padding builds the rows it did. The `visibleRange` end is the bottom of the padding box (`scroll - padding.top + height`), which was `scroll + height`.

`virtual-list-oracle` has 8 scenes more by 2 scales, and the 2 bordered scenes compare their rows (they compared the box only): `bordered with text rows`, `borders of each side`, `a fractional border` (1.5px: 1 device px at 1x, 3 at 2x), `a row in the top padding`, `... with a fractional padding`, `... of text rows`, `a bordered row ...` and `... below a border of each side`. `virtual-list-rows.test.tsx` compares `clientWidth`, `clientHeight`, `scrollHeight` and every row with a device pixel in view in the padding box, and the device pixel rows that rows with backgrounds paint inside the padding box. gpui `list.rs` tests: `test_a_list_scrolls_in_its_padding_box` (client box, range, viewport, row origin and width with an asymmetric border at 1x and 2x), `test_a_row_in_the_top_padding_is_built`, `test_a_row_above_the_padding_box_is_not_built`, `test_a_bottom_aligned_list_builds_the_row_in_its_top_padding` and `test_the_visible_range_starts_in_the_top_padding`.

**Deviations.** The clip of the rows is square: a list's `border-radius` does not round it (a div's `overflow_mask` does).

`ListState::bounds_for_item` (and so `item_is_above_viewport` and `item_is_below_viewport`) is where the row is placed: inside the border and the padding (`last_rows_origin`), as wide as the list is between its padding, and scrolled to a device pixel, so a row that shows only in the top padding is neither above nor below the viewport. gpui `test_bounds_for_item_are_where_the_rows_are_placed` compares it with the box the row was prepainted at, at 1x and 2x with a scroll between two device pixels.

### Round 20: a scroll offset is a device pixel (15c leftover e)

`scrollTop = 10.5` read back 10.5 and a div painted it at 10 (`pixel_snap` rounds a tie toward zero); a list kept the fraction too and painted at 11. Chrome (`FractionalScrollOffsetsForWebAPI` off) snaps the offset to a device pixel, half up, before it clamps and stores it, and reads that back. `scroll-fraction-oracle` (10 scenes by 2 scales: `scrollTop`, `scrollLeft`, `scrollTo(x, y)` and `scrollBy` with fractions, negative writes, writes past a range whose ends are fractions, a bordered and padded list) pins it: 10.5 reads 11 at 1x, 10.4 and 10.25 read 10 at 1x and 10.5 at 2x (10.25 rounds up there), `scrollTo(12.5, 7.25)` reads 13, 7 at 1x and 12.5, 7.5 at 2x, and `scrollBy(0, 0.5)` three times reads 1, 2, 3 at 1x because it adds to the position as it reads back. The range is `scrollHeight - clientHeight`, each rounded to a device pixel, which a div's `max_offset` already was (`writes past the maximum of a list that rounds up` stops at 61 where the layout's 61.8 would say 62).

gpuix (`scroll.rs`): `scroll_to` (a div, from `scrollTop`, `scrollTo`, `scrollBy`, `scrollIntoView`, a key or the scrollbar) and `scroll_list_to` round the target half up to a device pixel of the scroller's own scale (`ScrollHandle::scale_factor`, `ListState::scale_factor`, kept from the last layout) before they clamp; `dom_position` rounds the other way for `scrollLeft`, `scrollTop`, `getScrollOffset`, `getScrollMetrics` and so for a wheel or a trackpad that left the offset between two device pixels. A scroller that was not laid out has no scale and is read as stored. zed: a div hands its children `snap_scroll_offset` of its offset, so what is painted is what reads back (`pixel_snap` and the element offset are unchanged); `ListDomScroll::top` is the device pixel. The smooth scroll's frames in between are fractions and read rounded.

`scroll-fraction.test.tsx` renders every scene as a div, and as a `<virtual-list>` when it scrolls along y only, and compares `scrollLeft`, `scrollTop`, `getScrollOffset`, the box of the list and the boxes of its rows after each operation, and the device pixel rows that rows with backgrounds paint (scenes without a border or padding). gpui: `a_scroller_between_device_pixels_paints_at_the_position_that_reads_back`, `test_bounds_for_item_are_where_the_rows_are_placed`; native `scroll.rs`: `a_scroll_position_is_rounded_half_up_to_a_device_pixel`, `a_written_offset_is_the_one_that_reads_back`.


### Round 20: a scroll container's baseline (2b)

A box with `overflow` other than `visible` or `clip` (either axis: a scroll container) gives its parent its baseline kept between the top and the bottom of its border box: `LogicalBoxFragment::FirstBaseline` and `LastBaseline` (logical_box_fragment.h:40-70) clamp to `[0, BlockSize()]` when `IsScrollContainer()`, wherever a parent reads one off a child fragment (a block, a flex or grid container, a table cell, a line). A 10px block holding a 20px line has the baseline 10, and `margin-top: -30px` on the line (baseline -10) gives 0. Blink also forces the *last* baseline of an `inline-block` (the `kInlineBlock` algorithm): a block child that scrolls vertically (`ForceInlineBaselineSynthesis`, physical_box_fragment.h:170) gives the bottom of its margin box, whatever it holds (`BlockLayoutAlgorithm::PropagateBaselineFromChild`, block_layout_algorithm.cc:3840), and an `inline-block` that scrolls itself has no baseline (`BaselineMetrics`, logical_box_fragment.cc:25), which gpuix already had.

taffy: `compute_child_layout` (taffy_tree.rs) clamps `first_baselines.y` of the output of every node whose `overflow` is not `visible`/`clip` on either axis to `0..size.height`, so block, flex, grid, table and leaf layouts all read the clamped value from a child; the clamp that `flexbox.rs` had for a row's baseline-aligned items (`overflow.y` only) is that one now, and a flex column's baseline is clamped too. gpuix: an `inline-flex` or `inline-grid` that scrolls keeps its baseline in its border box, so does a first item that scrolls (`text/atomic.rs`, `Kind::FirstItem { scrolls }`, `kept_in_box`, `in_scrolling_item`); a block child that scrolls vertically, of a parent that lays out blocks, and that ends the content of an `inline-block` is its last baseline at the bottom of its margin box (`atomic::scroll_block`, `Collected::scroll_blocks`; zed `Window::layout_margin_bottom` reads the margin taffy resolved).

`scroll-baseline-oracle` (345 rows by 2 scales: `overflow` visible, clip, hidden, auto, scroll; a short box, a tall box, a line below the box, a line above it, padding and border, two lines, no content; as a cell beside a cell of text, an inline-table, a table, flex row, flex column, block, grid in a flex row or a grid, an `inline-block` with the box in it alone, after or before text, with a margin, in a block, an `inline-flex` and an `inline-grid` with the box first or second, and the `inline-block`, `inline-flex` and `inline-grid` that scrolls itself) is read back by `scroll-baseline.test.tsx`; the 12 `table-baseline-cell` rows for a scroll container are off its known gaps. taffy `compute::leaf::tests`: `a_block_around_a_scroll_container_has_its_baseline_clamped_to_the_box` (visible, clip, hidden, scroll), `a_baseline_above_a_scroll_container_is_clamped_to_its_top`, `a_scroll_container_on_one_axis_clamps_the_baseline`, `a_leaf_that_scrolls_has_its_baseline_clamped_to_its_box` (flex and grid), `a_flex_column_around_a_scroll_container_has_its_baseline_clamped` and `a_cell_takes_the_clamped_baseline_of_a_scroll_container`.

**Left.** 12 rows of `scroll-baseline-oracle` (`it.fails`, none a scroll container): a `visible` or `clip` box in a table cell whose line is below the box (a 10px block, a 20px line) or above it (`margin-top: -30px`) makes the row taller than in Chrome (20 for 10, the inline-table and the table in a flex row with it), and a `visible` or `clip` box with its line above it in a block in a flex row or grid makes the line 60 high where Chrome's is 50. A scroll block in a block that is itself in a flex, grid or table container inside an `inline-block` is also taken as the last baseline (Blink forces it only through blocks all the way up), and a flex or grid container that scrolls inside an `inline-block` does not keep the baseline of its text in its box (the text is read off directly).

### Round 15c: table rows and relative insets (15a leftovers: table row rest, relative top %)

The height of a table is shared out as Blink shares it, in `LayoutUnit`s (1/64 device px in taffy), where the floats left the row that takes the rest a 64th short. `taffy/compute/table/heights.rs` has the steps by name: `distribute_excess_block_size_to_rows` (`DistributeExcessBlockSizeToRows`: percent rows by deficit, the rows a spanning cell starts in, auto rows in proportion, empty rows equally, the rest in proportion; `MulDiv` in raw units, what the cuts leave to the last row of the step) for a cell that spans rows (`Caller::RowSpan`, in Blink's `RowspanCell::operator<` order, no percentage resolves) and for a row group with a length (`Caller::Section`), and `distribute_table_height` (`DistributeTableBlockSizeToSections`: the percent phase by distance from the percentage, then the tbody-auto, tbody-fixed or tbody-percent groups, the remainder to the last; every group that grew shares again over its rows). `SectionHeight::min_extra` is gone: a group with a length grows its rows when they are measured. `fractional-lengths-oracle` has 112 `table ...` rows (auto, empty, percent and length rows, two tbodys, thead/tbody/tfoot, a group with a length or a percentage, `rowspan`s, in tables of 100.3, 97.7, 50 and 60.3px and with spacing) at 1x and 2x, and `table-percent-heights.test.tsx` compares exactly where it had a 0.75px tolerance. A `div` cell has no baseline in Chrome, so the rows use `vertical-align: top` cells; the divergence for a baseline-aligned one is in Deviations. The spacing of those rows is whole pixels (an existing `border-spacing` rule: Chrome keeps it in a `short`, gpuix cuts it before the scale, so a fraction differs at 2x).

A `position: relative` block child resolves `top`/`bottom` percentages against the container's percentage resolution height (`block.rs`), `auto` when it is indefinite, then the other side negated, the top over the bottom and the left over the right (the right in rtl), cut to 1/64. Flex, grid and table items already followed those rules (flex `percentage_base`, the grid area, the table's `auto`). The same height fed `height: 50%` of a child and carried upstream's `.or(min_size.height)`, so a child of a `min-height: 100px` box with an auto height got 50px; it is `auto` as in Chrome now, a height a `min-height` clamps up is still definite. 42 `relative ...` rows (definite, auto and min-height parents, padding, rtl, nested, flex column and row, grid) and two percent-height rows in the oracle.

### Round 15c: fr tracks, flex lines and centring (15a leftovers: minmax(%, 1fr) beside 1fr, align-content stretch at 2x, centre offset)

Free space is a `LayoutUnit` in Blink and is divided as an integer, toward zero. `util/resolve.rs` has `half_layout_unit` and `divide_layout_unit`; `common/alignment.rs` `LayoutUnitDiffuser` (layout_unit_diffuser.h: Bresenham, the remainder one 64th at a time over the buckets, the call that gets it chosen by `x >= y`), `initial_alignment_offset` and `alignment_spacing`. Flexbox diffuses the free space of `align-content: stretch` over the lines in layout order (the last line first in `wrap-reverse`), of `space-between`/`around`/`evenly` over the lines and over the items (visual order, so reversed for `*-reverse`), and of main-axis auto margins over the margins (start before end, items in visual order), and the auto margins leave `justify-content` nothing to align. CSS Grid does not diffuse (`grid_track_sizing_algorithm.cc` divides and drops the remainder, with a TODO to change it), so `alignment_spacing` takes `diffuse_remainder`; `space-around`'s half share is cut twice, which equals `free / 2n`. Centring (`justify-content`, `align-items`, `align-self`, `align-content`, a table cell's `vertical-align: middle` and block `align-content`, `-webkit-center`, grid self alignment) is `half_layout_unit` of the free space, negative too. Auto margins: two cross-axis ones in flex are each half of the free space clamped to zero (so a flex item that overflows its line has no negative margin), in grid half the free space (the margins are zero beside the centring Blink applies), in a block the left is half and the right the rest (`ResolveAutoMargins`). An absolute child with no inset and a centring static position is placed by `centred_on_static_position` (flex and grid): the static point is half the content box cut to 1/64, the child is centred in the span of the padding box that is as far from the point as the nearer edge (`ComputeUnclampedIMCBInOneAxis`, `ComputeInsets`).

Grid `fr` tracks: `expand_flexible_tracks` takes the share `fr * 64 * flex + carried` as a float, cuts `share + FLT_EPSILON` to a whole unit, and carries `share - cut` to the next flexible track; a track whose share is below its base size keeps the base and the carry (`ExpandFlexibleTracks`). The fixed length of a track (and the argument of `fit-content()`) is cut to 1/64 where taffy reads it. `find_size_of_fr` already divided in the same float units.

`fractional-lengths-oracle` has 386 rows more at 1x and 2x: `grid fr ...` (10 templates by 6 widths, gaps, rows), `flex lines ...` (3, 5, 7 lines by 3 heights by 8 `align-content`, gaps, wrap-reverse, overflow, column), `flex justify ...`, `flex align-items ...`, `flex auto margins ...`, `flex absolute child ...`, `grid justify-content/align-content/self center/absolute child ...`, table cell `vertical-align` and block auto margins. The badge in a table cell of `atomic-trim-baseline` and Menlo "Hello" of `text-layout-units` are exact now. Found, not fixed: `safe center` and `unsafe center` cannot be written (no `safe` in the API); a `repeat(n, 1fr)` set (see Deviations).

### Round 15c: virtual-list rows and box (15a leftovers: virtual-list rows device px, virtual-list getBoundingClientRect)

A list measured its rows with `layout_as_root`, whose size is the snapped box (a row of `20.3px` was 20), and recorded no bounds: `List::paint` never ran the `on_painted` and `on_painted_layout` listeners, so `getBoundingClientRect` of a `<virtual-list>` was zeros and `onLayout`, `ResizeObserver` and `IntersectionObserver` (target, root, and the clip of its rows) read nothing for it. zed: `AnyElement::layout_as_root_unsnapped` (the size layout computed, through `Window::layout_unsnapped_size`, which keeps no bounds so it can be asked before a root is placed) is how `List` measures every row; rows stack from `layout_unsnapped_bounds` of the list plus the padding `layout_insets` resolved (cut to 1/64, not the style's), at the unsnapped width; each row is prepainted with `AnyElement::prepaint_at_fractional` (`Window::with_fractional_element_origin`): it is painted at the device pixel its origin rounds half up to, and `TaffyLayoutEngine::set_origin_seed` gives the root the fraction, so the edges of the row and of everything in it snap from that origin as the children of a block do (two rows touch when painted where the unsnapped ones touch; `layout_unsnapped_bounds` reports from the seeded origin, and the enclosing atomic inline's phase is zeroed so it is not added twice). The scroll offset a row is placed by is the device pixel the total rounds half up to (the state keeps the fraction, so a small wheel delta still accumulates). `Interactivity::run_paint_listeners` is the one place the painted listeners run (div, culled div, list); a list publishes them in a layout-only pass (`publish_layout`) and notes its prepaint listeners as publications. gpuix: `build_virtual_list` records the box with `track_host_bounds`, no selection region, no scrolls (rows are not in its layout, its scroll metrics are `ListState::dom_scroll`).

`virtual-list-oracle` (12 scenes by 2 scales: rows of 20.3px, 13px text at 1.15, mixed fractions, a list of 100.3px and 99.7px, padding, a positioned parent, a border) is read back by `virtual-list-rows.test.tsx` as the box of the list, `scrollHeight`, `clientHeight` and the box of every row in view after scrolls to 0, a fraction, the end, exactly, and the device pixel rows that rows with backgrounds paint are compared with Chrome's snapped edges. `virtual-list-observers.test.tsx` holds what the other readers saw before (nothing). The border and the top padding are Round 20.

### Round 17: #88, a read inside skipped content

A forced read inside `content-visibility` skipped content cost two layout passes after a change, built the content from
scratch every time a draw had come between two reads, and told the observers the content was gone. Five changes (#88a is
the layout kept, #88b the read and the observers; K1 is the zed half of the lock):

- the first read after a change is one layout pass, as Chrome's (#88b);
- a layout-only pass wraps what it builds, so a cold tree is built once (#88a, slice 1);
- the layout a read forced is kept across the draws that skip the content (#88a, K1 and K2: a content lock, parked taffy
  children and layout records that outlast a draw);
- a layout record is skipped where a change elsewhere moved it, not only where it was made (#88a, F4, zed
  `ReplayKey::translation`);
- the observers leave skipped content alone (#88b).

What is left is under [Deviations from a full build](#deviations-from-a-full-build).

#### The first read after a change is one pass

Which ancestors skip is read from the
value each element was last *built* with, which is stale for the pass that lays the change out: an ancestor the change
turned `hidden`, or inserted. So the ancestors that `skipped_ancestors` finds are forced before the pass, and every
strict ancestor of the element is registered with it (`Relevance::force_for_read`, `ancestors_of`): one whose style
says it skips and whose build, in that pass, resolves it to a skipping one (`contains_size`: `hidden`, or an `auto` that
is not near) is forced as it is built (`Relevance::join_read`, from `build_host_container`, before `skips`) and joins
the forced set, exactly as if it had been forced before the pass. Its key is the forced one (`Some(8)`) from the
planner on (`joins_read`), or the pass would leave a layout record, made with the content built, under the key of a
frame's. An ancestor that does not contain size keeps its key. After the pass the chain is dropped and the ancestors
that still skip (`skipped_ancestors` with the fresh values) are looked up in the forced set; a pass is run again only if
one is missing, which the registration leaves no case of but a `contentVisibility` that a state style sets (`hover`),
which the declared value does not show.

#### A layout-only pass wraps what it builds

The planner used to wrap a node only in a pass that
paints (`Node::records` had `!layout_only`), so the first build of a cold or changed subtree in a layout-only pass ran
inline, outside any gpui Retained, and left no layout record: the second pass wrapped it and built it again, and
only the third skipped. Now a node built in a layout-only pass is wrapped with the token `finish_session` records for
it, gpui leaves the layout record under that key, and the next layout-only pass skips it. `Plan::recorded` (the
patch preview) and `begin_unit` still ignore a layout-only pass. A cold tree is built once, in its first pass; a page
of 5500 elements that mounts its list and writes `scrollTop` 11 times with no frame builds 5502, 14, then 13 elements
a round, the changed entry and the spine from the second pass on (it was 5500, 5500, then 13). Three checks refused the
second pass at first, and each is fixed: the pass that wraps the entries of a list laid out empty finds no scroller
(`Scroller::of`), so it records them to stay where they are, and the pass after, which finds the scroller and gives
the skip an overscan, was refused by gpui (`ReplayKey`: content mask and culling differ), so a skip in a layout-only
pass uses the overscan its record was wrapped with (`Node::wrapped`, `Record::wrapped`; 124 elements before); the
`<svg>` icons a pass created were new ids to the delta of the next (`seen.customs` is committed before the build), so
the 500 icons of the page made the entries they are in the spine (2582 of 6002 elements): the build tells the planner the state it
saw (`Retain::custom_built`) and a paint that moves it is a change as before; and a scroll handle seen for the first
time was taken as scrolled, which gave the content under it no layout record, so a scroller that appears in the same
pass as its content had the content built whole twice more (152, 152, 152, then 12 for a scroller of 15 entries): a
handle that a frame first finds at its origin, a pass that only lays out or a frame that paints (round 21), is not
scrolled, and the content is built once (167, then 13) and recorded to be moved by the scroller, so its first scroll puts
it elsewhere (`Scroller::of`). A scroller first found away from its origin is scrolled, and its content is built again
where it is. A record wrapped to be moved by a scroller is built again, in a painted frame, where its scroller no longer
moves it (`Kept::Unmoved`, e.g. an `overflow: scroll` box whose content does not overflow): gpui keeps the overscan and
relocation handles of such a recording, which a build there has none of (a verified frame showed text bounds the build
lacked). Guard: `retain.rs` `a_scroller_first_found_at_its_origin_is_not_scrolled` and
`the_content_under_a_scroller_first_found_in_a_painted_frame_is_still_recorded_to_be_moved`. A subtree that a change
moves (a box above it grows) is skipped where it is now, if the move is a translation (F4, below).

#### The layout of content a read forced is kept across the draws

A forced read
inside `content-visibility: hidden` content builds it in a layout-only pass, and the next draw threw all of it away
(the host was built with no children, `release_unused_element_states` dropped the states, `collect_garbage` freed the
detached layout nodes, a draw moved `rendered_serial` so no layout record was current, and `skip_content` dropped
every Retain record below the host), so the next read after any draw built it again from scratch: a read of a line in
a hidden element of 300 cards of 3 lines (1203 elements) after a paint-only write and a draw built 1203 elements in
6 to 27 ms each time, and Chrome keeps a locked subtree's layout. Now `build_host_container` calls
`InteractiveElement::locked_content(skipped)` on the host of every element that `contains_size` (`skipped` is whether
this build skips; a forced host is built with its children and `skipped: false`), which is zed's half: the lock
keeps the element states, the layout nodes (parked under the host while it skips) and the layout records the content
left, and a layout record made under a lock with no entity read is current in the next pass that forces the content
however many draws came between. The planner's half: `Retain::forced_host` marks the node of a forced host, a node
built inside one (or in a late build whose `FrameEnv::forced` says it is) is `Node::under_forced` and its record
`Record::forced`; `skip_content` keeps the forced records (still dropping `dynamic`, `dormant`, `unbuilt`, `woken`,
so no clock runs and no spine is held) and drops the others, and any record in `deltas.spine`;
`commit_deltas` drops the record of every spine id that has a skipping host above it (`forget_changed_under_skipped`;
the spine is closed under parent, so the path up to the host goes), so a pointer, a focus, a motion value or a
measurement that changed under the host while it skips is not replayed stale by the pass that forces it. A host that
shows builds the content whole: a forced host's key is `Some(8)` and a node's shape includes its parent's, so the
forced records fail with `Kept::Shape` (guard: "builds the content whole when the host shows again"). The tapes
behind `LayoutRecorded` are kept by `TapeStore::laid_out`, which a draw does not touch, and the boxes a layout record
holds survive `WindowBounds::reset`'s purge (a page of more than 4096 boxes had a read after a draw return null for
an element in skipped content) until the sweep of destroyed elements removes them (`automation::retain_bounds`).
A read inside a hidden element after a draw that changed something is one layout pass that builds the host and the
spine: 4 elements and 8 layout skips for the hidden element of 8 cards of the scene `firstReadKept` (36 to build it from
scratch), and the 300-card probe builds 3 elements in 0.5 to 0.8 ms (1203, 6 to 27 ms). A read after a draw that moved
no box is not a pass at all: see "A read after a painted frame shares the forced pass" (Round 21), which replaced the
statement here that sharing across a painted frame is not done. What a change moves is skipped where it is now (F4,
below: a line that grows moves the cards after it, and their layout records are put at the new place). Guard:
`content-visibility-auto.test.tsx` "content
a read forced keeps its layout across the draws that skip it" (oracle scenes `firstReadKeptInside`, `...Nested` and
`...Shown`, and the page of 1100 cards), `retain.rs` `a_frame_that_skips_content_keeps_the_records_of_what_a_read_forced`
and `what_changes_under_content_a_frame_skips_is_not_replayed_when_a_read_forces_it`, `tape.rs`
`the_box_of_a_layout_tape_outlasts_the_frames_of_a_page_of_many_boxes`, plain and with `GPUIX_RETAIN_VERIFY=1`. The tests
that read after a draw end the share with `renderer.flush()` (a layout invalidation that moves nothing) to keep covering
the layout records.

#### A read after a painted frame shares the forced pass

**Problem (#98).** A read of an element in skipped content after a paint-only write and a drawn frame was one layout
pass (3.2 ms median for a line in a hidden element of 300 cards of 3 lines, with a title and a tint beside it), where
Chrome needs none: it keeps a locked subtree's layout clean until something dirties it. `LayoutStamp` counted the
frames and passes built (`builds`), so any draw ended the share, and `builds` was there because a frame can move a box
without an invalidation (a tick whose animation drives a layout value).

**What changed.** The taffy fork counts the boxes laid out to another box: `TaffyTree::box_generation` is bumped by
`set_unrounded_layout` when the stored layout of a node differs in location, size, border, padding or scrollbar size (not
content size or order: the content size of a host flips each time its children are parked and given back, and
`layout_generation` keeps leaving location out for gpui's `ExtentStamp`). It is one counter for the tree, and never reset.
`LayoutStamp { layout_invalidations, boxes }` takes it from the layout engine in place of `builds`, so a frame that
gives every node the box it had leaves the stamp and a tick that moves a box moves it.
The frames that come between do not write the forced content, so the read that shares the pass takes its
boxes and scroll state as the pass left them: `WindowBounds::keep` notes the frame the pass wrote and the replays
it put back (`live`), `get_kept` falls back to them for an element no later frame wrote (`automation::reading_kept`, only
for an element with skipped ancestors), the purge keeps what they need, and `renderer::keep_scroll_state` keeps
`SCROLL_HANDLES` and `VIRTUAL_LIST_STATES`, which every build rebuilds without the scrollers in skipped content. A read of
a built element is of what the frame published, as it always was.

**What a frame or a pass can change without an invalidation or a box.** Read through for the share: element offsets (a
scroll write, a wheel or a drag, a smooth scroll's frame request and a clamp all invalidate or move a box; anchoring
moves an offset only for a box that moved), layout-record translations and relocations (the delta is the move of a box
or of an offset), virtual-list rows (`layout_as_root_unsnapped` lays them out in the window's engine, so a row that
changes size is a box change), text re-measure and `restore_placed_measures` (they put the measure's state back for the
size already placed), the window's size and scale (a refresh, and every box changes) and motion values written
paint-only (nothing a bound reads). The content a read forced is not built by a frame, so nothing moves inside it.
Not covered: a nested layout engine (an atomic inline's own tree) is not counted. A CSS transform moves a box without
a layout pass; the stamp counts layout boxes, which a transform does not change, and `getBoundingClientRect` adds the
transforms around the box on top of it (Round 19).

**Left.** The counter is the tree's: a box that moves anywhere (a running layout animation elsewhere on the page) ends the
share for every frame it moves in, where Chrome dirties the animated subtree only. The read costs one pass, as before,
in those frames.

Numbers (the 300-card probe of `content-visibility-auto.test.tsx`, "a read of a line in 300 hidden cards"): a read
after a paint-only write and a drawn frame, 1 pass and 3.21 ms median before, 0 passes and under 0.01 ms after. Guard:
that test, "consecutive reads and a change that moves no box" (a frame that moved no box keeps the share, a frame in which a
running animation moved a box ends it with fresh boxes) and "consecutive reads of geometry below skipped elements",
plain and with `GPUIX_RETAIN_VERIFY=1`.

#### A layout record is skipped at a translated place

A retained
subtree that a change elsewhere only moves (a box before it grows, a card above it gets a line) was refused by
`prepaint_skipped_laid_out` and built again where it is (the read after a line grew in `firstReadKeptInside` built 28
elements: the card, the five after it, the host and the page), where Chrome lays out nothing for it. The record keeps the key of its build, and each skip works
out its delta from that (not from the skip before, so a subtree that is moved back is at its own place with no drift):
the box, the origin the layout engine gave it and the content mask around it moved by one whole number of device pixels
(the engine's origin to within the 1/256 of a device pixel the phase is compared to), and the size, the phase of the
origin, the scroll offset around it and everything else the subtree reads equal; a power-of-two scale factor and no exact
clip. Else the place is not one, and the subtree is built again where it is, as before. A mask that did not move with the
subtree (the cards of a scroller viewport, moved by a box above that grows) leaves what is culled inside the same only
when culling cannot decide differently: it depends on where the subtree is in the mask, and a culled box is told so
(`LaidOut::culled`, read for dormancy). `Window::lies_outside_the_mask` notes the rects it compares with the mask (the
inflated box, and the inflated painted extent when it gets that far, also while an element holds the capture) into the layout
recording open around it, a subtree skipped inside the build adds its own moved by its delta, and the union
(`LayoutOutcome::cull_reach`) is kept in `LayoutReplay` with the key. A key whose mask did not move with the subtree is
accepted when nothing was tested, or when the union lies strictly inside the mask before and after the move (by the 1/256
device pixel the engine's origin is compared to; strictly, as the mask of a clip inside that misses the one around is empty
at a corner of it); one that crosses an edge, or was culled, is built again where it is, and a culled subtree root keeps its
rule (it is under the empty mask). The corner radii are not asked of the union: culling compares the bounds only, and equal
radii are still required. No decision is recomputed per candidate. (What culling compares includes the outset of the
candidate, 15 px beyond a text, so a card has to be that far inside the viewport: a card flush with it is built.) The positioned
frame around is not asked, as for a scroll relocation: a box placed by
a frame outside the subtree is a blocker. What stays in the way is kept when the record is made (`LayoutOutcome::blockers`,
sticky, positioned capture such as a canvas or a scroll anchor, a scroller that is scrolled, an input handler, a box
that escapes to a frame outside the subtree, a scale that is not a power of two, a publication whose owner does not take
the translation), and a subtree skipped inside the build of another hands its blockers to it. What the build holds outside
the frame (`Window::note_relocated`: text bounds, scroll handle bounds and the bounds of its children) is a `HandleList`
per record, held by the list of the build around it as the tapes are, and a skip puts it where the delta says (it knows
where it was last put). The hook of the owner is given the delta (`RetainedPhase::LayoutRecorded`), and `TapeStore::replay_laid_out`
replays the tape with it, which moves what is nested too. A guard for each: the counters `layout_translations` (zed
`RetainedCounters`) and, in gpuix, the numbers below. `firstReadKept` step 2 (the hidden element is moved by a box above
it) reads in 4 elements with 8 skips where it built 36; `firstReadKeptInside` after a line grew builds 8 with 7 skips
where it built 28; a page of cards with text and a scroller (`overflow: hidden`) moved by a box above builds the page and the host
(3 elements, 6 skips) where it built 33, and the boxes and scroll metrics are those of a page built where it is. Cards of a scroller viewport that clips (the mask they are under does not move) moved by a box above it: 8 cards of a
box and a text, the box above 30 to 50 grew: 7 are put where they are (the five inside the viewport, and the two outside it
that are culled both times), the sixth reaches the edge and is built; 6 elements built and 7 translations where it was 21
built and 2 translations (the culled two), the same boxes as a page built where it is (`content-visibility-auto.test.tsx`
"skips the cards a box above them moved in a viewport that did not..."). (A card with
a scroller that was drawn since the scroller appeared is skipped like any other: a draw that finds a new handle at its
origin does not take it as a change, so the record of what is around it stays; the first move of the page of cards of the
`content-visibility-auto` test builds 3 elements and skips the 6 cards, where it built 21. A frame that replays a
record skips nothing new, so `clip-cull.test.tsx` counts what the frames since the mount skipped, not the one frame
after. A scroller the user scrolls, `overflow: scroll`, is excluded from retention, and so is the card around it.)
Guard: zed `layout_record_tests.rs` (`Moving`: moved by a box before it, by a fraction of a device pixel,
each blocker, a mask that did not move (inside it both times is translated, across its edge is built, the culled state equal to a
build's), a subtree inside one that is moved, a verified pass, a fuzz of random pages and
moves against a page that builds everything, a locked record put where a box before its host moved it), `tape.rs`
`a_root_skipped_with_its_layout_record_is_put_back_where_the_skip_says`, and `content-visibility-auto.test.tsx`
("builds the hidden subtree...", "skips the cards a line that grew moved", "puts the text and the scrollers of cards the
page moved where a build puts them"), plain and with `GPUIX_RETAIN_VERIFY=1`.

#### Observers on an element below a skipped one

Measured against headless Chrome 156 (`fixtures/content-visibility-observers-oracle.*`;
`hidden` and `auto` alike). The content of a skipped element is not gathered: a `ResizeObserver` tells
nothing of a target in it while it is skipped (no 0 by 0 when it hides), keeps the size it last told, and on show
compares the target with that size and fires only if it differs; a target never rendered fires once when it shows. The
skipped element itself is a normal box (a `hidden` one reports its `contain-intrinsic-size`). An `IntersectionObserver`
tells a target in skipped content (`hidden`, and `auto` while it is skipped) "not intersecting" with `boundingClientRect`,
`intersectionRect` and `rootBounds` all zero: as its first report (a target never laid out), and when the skip lands (one
frame after the host moved away or `content-visibility` changed; the frame that moves the host still reports the real
geometry, it is not skipped yet), only if the target was intersecting. While it is skipped nothing is told, whatever moves
(the host, the content inside, a `rootMargin` of 4000px that reaches it), and on show a fresh entry comes from the new
layout. A read of geometry forces the content for the read and changes none of this: it delivers nothing and moves no
size told. `observers::sweep` leaves a target without a box because an ancestor skips (`inside_skipped_content`) alone for
the `ResizeObserver` (`display: none` and a removed element are still 0 by 0, as in Chrome); for an `IntersectionObserver`
it reports a target that is inside skipped content, with a box kept from a layout or none, gone (`present: false`, the zero
entry above) and does not read its box. The first probe of an observed element is `probeElementBounds`,
`read_after_layout` with `SkippedContent::Leave`: the same pass as `getElementBounds` but the skipped content stays
unbuilt and a target in it has no box, so observing in hidden content builds none of it. Guards:
`content-visibility-observers.test.tsx` (the 39 scenes, frame for frame).

**The root of an `IntersectionObserver`** is its own report. An element root has the `intersectObserve` listener like a
target, and its report carries the border box, the border and the room a classic scrollbar reserves
(`Observation::scrollbar_width`, `scrollbar_height`, from `renderer::reserved_scrollbar`, the handle's size as of the
last layout), so `js/observers.ts` takes the client box (the padding box less the scrollbar, which is what Chrome
calls `rootBounds` before `rootMargin`; a percentage margin is of that box, width for left and right, height for top
and bottom) from it and reads nothing. The clip a target is cut by at an `overflow` ancestor is the same client box
(`clip_rects`). Reports are not applied as they arrive: each marks its record, and `flushObservations` evaluates the
marked records once the frame's reports are all in, so a root that moves or resizes in the frame its target changes
gives one entry, from the geometry of that frame (a root that moves with no threshold crossed gives none; one that
resizes so a ratio changes gives its entry in that frame). Guards: the `io8` scenes, and "evaluates an observer with an
explicit root without reading layout" (no layout pass, no `getElementBounds`). `io8b2` (a classic scrollbar on the
root, `setScrollbarStyle("classic")`) is Chrome's frame for frame except the `rootBounds` of its first entries, 15px
wider than Chrome's: an `overflow: auto` scroller takes its classic gutter one frame after its content starts to scroll
(see "Deviations from a full build"), and the test pins that (`DEVIATIONS`, in a plain frame and under
`GPUIX_RETAIN_VERIFY=1` alike: a verified frame no longer finds the gutter before the plain one does).

#### Guards

The first read (#88b) is the oracle scenes `firstRead...` (`fixtures/content-visibility-auto-oracle.html`,
regenerated alone with `bun fixtures/content-visibility-auto-oracle.ts firstReadInside ...`) in the describe "the first read
after a change": each step changes the page and reads the boxes the scene names, the first of which is inside skipped content.
`firstReadInside` (a layout write, a margin, an intrinsic size and padding in a `hidden` element), `firstReadTurns` (the
read's ancestor turned `hidden` and `visible`, alone and with a write inside it), `firstReadInserted` (a node into `hidden`
content, a `hidden` element inserted with the node read in it, a node into that), `firstReadNested` (two `hidden` elements,
only the inner one changing, turned `visible` and `hidden` again), `firstReadAuto` (190 `auto` entries: all change intrinsic
size, a far one grows, a near one grows) and `firstReadScroller` (the scroll metrics of a scroller in `hidden` content): the
first read is one `getLayoutPassCount` pass and one engine pass, the reads after it in the same skipped element are none, the
boxes are Chrome's at 1/64 px, and the frame after skips the content again. Reading into a second skipped element costs
the union pass of slice D (two entries are two passes).

The wrap (slice 1): the unit test `a_layout_only_pass_builds_the_spine_and_what_changed` (the page and the changed card are
built in roots of their own: `layout_builds` 0 -> 2 and 14 -> 16), `retained-replay` (`layoutBuilds` 0 -> 2),
`forced-layout-incremental.test.tsx` (a cold page that mounts and writes `scrollTop` with no frame builds 5502, then 14
and 13 a round, where the second pass built 5500 again; the svg variant, whose 500 icons made 2582 of 6002 elements the
spine) and
`content-visibility-auto.test.tsx` "forced reads inside hidden content that is laid out once and kept" (never drawn, drawn
visible then hidden, drawn hidden: the first read 36 elements and 10 layout records, the read after an unrelated change 4
elements and 8 skips, the geometry Chrome's; oracle scene `firstReadKept`).

What a read forced is kept (K2): the describe "content a read forced keeps its layout across the draws that skip it" reads,
draws and reads: Chrome's boxes at 1/64 px, 4 elements built with 8 layout skips (9 and 2 for the nested scene) against 36
from scratch; a paint-only change builds one card; a host never drawn; a host that shows builds its content whole; a page
of 1100 cards (more than 4096 boxes) keeps its boxes across draws; a pointer and a motion value that change under a
skipped host give the boxes of a fresh build once it shows. A read of a line in a hidden element of 300 cards (1203
elements) after a paint-only write and a draw builds 3 elements in 0.5 to 0.8 ms (1203 in 6 to 27 ms). The translated
place (F4) has its numbers above. `handler-calls` covers `probeElementBounds`. All with and without
`GPUIX_RETAIN_VERIFY=1`.

### Deviations from a full build

Where a frame is not what a full build of the same state draws or reports, or where the pass leaves a
gap.

- **Never retained.** `<code>`, `<markdown>` and `<diff>` (until they implement `retain_state`; they no
  longer make a tick of a loop elsewhere whole); an `<svg>` for the one frame before a paint that would
  start a raster job (`Unsettled::Stale`; one that waits for its raster is retained); virtual lists and their rows (a chat page
  tops out near 60% skipped); scrollers themselves (what is inside them is retained); elements with an
  `onScroll` listener that scroll; highlights; anchored layers; sticky and fixed boxes; width keywords and
  natural widths; `layout` (FLIP) elements; a `display: contents` element that is built as an element of
  its own; collapsed tables (a paint-only motion on a part of one is still a patch, with the table, or the
  container of an anonymous table, as its unit); atomic inlines (a text with atomic inline boxes keeps its measure, but it
  and its boxes are built every frame, about 43 µs each); and a root that holds an `absolute` box placed
  from outside it (an escaping absolute), and an element that is itself `absolute` (it is built by every
  frame, and may be a patch unit). A page made of these is built as before and pays the 4 to 5%
  of bookkeeping that the back-off limits.
- **Roots are boxes with a layout node.** A table row, row group or column is never a root; a root may
  be one element, but only one that is costly to build (a plain box is not worth its replay).
- **Scroll frames replay what gpui can move.** A scroller's offset is volatile, and what is below it
  moves with it. A root that gpui records to be moved is replayed at the new place; one it cannot is
  built. These are built: a root that holds a hole or a fragment, a rounded clip cut through its corner
  (so the rows that the rounded top and bottom of a scroller cut are built, the rest of the scroller is
  moved), a root inside a transform that is more than a translation (Round 19: `ReplayKey` holds the ambient
  transform, so it is built when that differs, and not moved), a box placed by something outside it, a position kept past the frame (a drag, a keyboard click,
  a tooltip), an input handler, a scrolled scroller inside, a root nested in another root that moves
  (the inner is built every frame the outer is), and every root at a scale factor that is not a power of
  two. A page that scrolls costs at most 1.05 times what it cost without retention (guarded), and a list
  of small rows a fifth (guarded).
- **A read on a dirty window still builds the spine** and does layout and prepaint of the page; only
  `paint_if_dirty` inside dispatch builds every subtree. After a write that drives only paint-only
  properties it does nothing at all ([#47](#47-a-bounds-read-after-a-paint-only-write)).
- **A hover frame still scales with the roots it replays**, now at about 0.5 µs a card: 5000 cards clipped
  out of view cost 3.0 ms against 0.4 ms with none, with colour loops or without. A replay is a step per
  root (3.4 µs in gpui's bench), and gpui's `StateOwner::is_kept` is 30% of what is left
  ([#63](#63-content-nobody-sees-costs-the-page-nothing)). Unchanged content below the window or
  `opacity: 0` costs a frame nothing, as before.
- **Layout-driving loops are full frames.** Only animations that change what their element paints are
  patched or dormant. A `width` or inset loop, a layout (FLIP) animation and a motion value that drives a size
  are built every tick with the spine above them, culled or shown, as before, and counted as
  `LayoutAnimation` in `getPatchCounters().unpatchable`. A binding of such a value that rests does not make
  a tick of a loop elsewhere whole.
- **A tick that gpui or gpuix cannot patch is a full frame, and says why.** A subtree of more than 256
  elements (`UNIT_MAX_ELEMENTS`) is built whole and counted in `unitsOversized` and `TooLarge`. A
  `position: fixed` element is no unit (`Excluded:fixed`), where an absolute one is: gpui has no root
  placed by the viewport. A loop on an element that holds an absolute box against a block outside it is
  `Escapes`. A scroll animation or scrollbar fade, a layout-only pass since the last full frame, the
  pointer, focus, a scroll, a measurement or any change that is not the clock make the tick a full frame
  (`getPatchCounters().unpatchable`, the keys under "Switches and counters"). The run of an `opacity`
  animation starts and ends with a whole frame, as Chrome re-layers; the ticks inside it, at 1 and at 0
  included, are patched.
- **The first blink after a focus is drawn whole.** The frame that notes the blink cannot also be the unit
  (counted `Outside`); the blinks after it are patched.
- **An `<input>` still blocks culling.** Its key and input handlers are registered by its prepaint and
  paint, which a culled subtree does not run, so a focused input scrolled out of view would stop taking
  text. It is never dormant, and an input outside the clip is prepainted, but it arms no blink and asks for
  no frame while its caret cannot be seen.
- **A positioned `<svg>` still blocks culling**, so it is never dormant and the offscreen cost of its loop
  is what it was. An `<svg>` that is not positioned is culled by its box.
- **The glyph `d` morph costs a parse, a raster and a texture a tick.** A `motion.path` that morphs `d` is a
  new document every tick: it is parsed again (usvg, about 5% of a patched tick of FF's tile), rasterised in
  the paint, and the atlas makes a Metal texture for it and releases the one of the tick before
  (`PlatformAtlas::get_or_insert_with` and `drop_image`, about 14%). The tick is patched, and costs what the
  unit does. Not changed.
- **A tape is still compared through strings.** What a patch paints is compared with its recording operation
  by operation and tape entry by tape entry; the operations are compared by a comparison that stops at the
  first character that differs (`PaintOperation::differs_from`), and the entries of the tape are still
  formatted whole (about 14% of a tick of the tile with the operations, before that change).
- **An `<svg>` whose paint would start a job is whole for one frame.** `Unsettled::Stale` (a document or
  colour that a paint would start a job for) is `None` in `retain_state`, so the
  frame before that paint is unsteady and the tick in it is drawn whole. The job's result arrives as a frame
  of its own and the ticks go on after it.
- **`Unsteady` from FF's showcase stopped reproducing** with the same release addon (it was 68 a second, then
  `Outside` 64 a second, with no change to the build or the page). The `<svg>` that has no area is the class
  that reproduces it in a test and is fixed; the cause of the showcase's own is not found.
- **What is dormant is narrower than what is culled.** A span, a `layout` element and a child spliced out
  of an animating `display: contents` element are not dormant, and neither is a card in the overscan of a
  scroller: it is recorded in a region wider than the clip and is not walked culled, so its loop is built as
  a visible one's. An element whose `opacity` is animated is not culled in a scroller either (it is built
  every tick there as well), so a bound or animated `opacity` is not dormant. A motion value bound only by
  dormant elements, or read only by a listener, asks for no frame: a listened one has a dormant tick that sends
  its events and draws nothing, and one that is not has none.
- **Culled subtrees are still walked by a build.** A build that goes through one runs its prepaint and paint
  under an empty mask, for the bounds registry, the text registries, the tab stops and the focus path, the
  accessibility nodes and the scroll metrics (18 prepainted and 18 painted per card in the page of #63); a
  replay skips the walk, so a hover or idle frame does not pay it. Skipping it for a build too would need a
  registry that is lazy or movable. Not done.
- **The first draw costs about 15% more.** A root built whole is recorded by the frame that builds it, so
  the entries within a scroller's overscan are prepainted and painted to be recorded in draw 1 (r11b: 30 to
  35 ms for 1902 elements) where draw 2 paid for it before and was 14.3 ms against 1.8 now.
- **Only `color` has an inherited read set.** A change to the lengths, font state, direction, custom
  properties, groups, containers or scheme still keys every root below it as changed; a `color` keys only
  those that read one.
- **A motion value binds no SVG attribute.** It binds style properties; the way to move the stroke of an
  `<svg>` with one is its `color` with `currentColor` strokes.
- **The verifier's reference build leaves recordings unmovable.** A frame that only replays is built a
  second time and shown as that build, whose recordings are direct and cannot be moved, so a scroll after an
  idle frame under `GPUIX_RETAIN_VERIFY=1` builds the rows where it puts them (`lazyBuilds` 40 per scroll,
  `relocations` 0). `retained-scroll` scrolls in pairs, the verifier off for the frame that records and on
  for the one that moves, to count relocations under it.
- **Fractional movement is built.** A box that an animation moves by a fraction of a pixel is built and not
  replayed, since a replay moves by whole device pixels (Round 12 #65,
  [#45](#45-bounds-in-fractions-of-a-pixel)), and the tail of a spring moves boxes until it rests, so a page
  that retargets springs from `onLayout` sends more layout events than before #65. Not addressed here.
- **Bounds are fractional with a few differences from Chrome**: the fraction of a scroll offset is dropped,
  a box that does not scroll reports its padding box for both `clientWidth` and `scrollWidth`, and the cases
  of a percentage, a flex share or a text that are still a 64th off are listed under the deviations of
  [#45](#45-bounds-in-fractions-of-a-pixel).
- **Call sites that refresh** (focus, the window title, key-event and selection-change setup, the
  `scrollIntoView` family, text selection, svg rasters, the wasm pointer lock) build the whole frame that
  follows; see [the refresh rule](#invalidation-rules).
- **The verifier is weaker where a recording was made in a region.** When either side of a root's
  comparison was recorded in one, both are compared without what a build culls and nothing can hit but
  the region holds: the key, action, modifier and mouse listeners of the boxes between the clip and the
  region, groups that hold nothing, and masks that only widen the groups around them. It cannot see a range that moves across
  frames, which a test with the verifier off covers. The whole-frame comparison is off in gpuix (its
  reference build is not identical to the first one, see Layer 2) and the per-root comparison is on.
- **A page with `display` transitions that use `allow-discrete`** (`hold_display`) can build a different
  set of children with an unchanged key. The verify mode finds it only where a test reaches it.
- **A display: none flex child's `order`** differs between a layout cache hit and a recompute. GPUI does
  not read it.
- **A frame that moves a box ends the shared forced pass, wherever the box is.** A forced read after a painted frame that
  moved no box is no pass (Round 21, #98), but the stamp counts the boxes of the whole tree, so a running layout animation
  elsewhere on the page makes every frame it moves in one where the read is a pass again (0.5 to 0.8 ms for a hidden
  element of 300 cards), where Chrome dirties only the animated subtree.
- **A subtree under a content mask that did not move with it is built when it is moved and something culled inside it is
  near the mask's edge.** A layout record is skipped at a translated place under a mask that did not move only when every
  rect culling tested inside it lies strictly inside the mask before and after (so a card across the edge of its scroller's
  viewport, or one whose child is within its cull outset and a pixel of it, is laid out again); the union is of what was tested when the record
  was made, not recomputed per candidate, so a subtree with one child near the edge is built whole.
- **A scroller the user scrolls (`overflow: scroll`) is never retained, nor the card around it.**
- **Observers.** The spec repeats the resize observation loop at deeper depths until no observation is
  shallower than the last; this does not. An `IntersectionObserver` reports one entry per target per
  frame, where the spec queues one per threshold crossing between deliveries.
- **Pixels.** Retention against a whole build is exact (verify mode). Against the fork before the pass,
  a few randomly generated pages differed by 1/255 in some pixels; the cause was not found and it has not
  been looked at since #41 changed the text layout units, which differ on purpose.
- **A culled subtree draws nothing, so it has no hitbox.** The clip hides it, so that cannot be seen.
  While an element captures the pointer nothing is culled.
- **Accessibility in the test window.** The test renderer pretends a screen reader is connected
  (`setA11yActive(false)` removes it), so tests run with an accessibility tree and still see subtrees
  culled and replayed.
- **`timeline.perf.test.tsx` fails its pan budgets** on poc as well as here (see
  [Results summary](#results-summary)).

### Switches and counters

| Switch | Effect |
|---|---|
| `GPUIX_RETAIN_OFF=1` | Nothing is recorded or skipped: the baseline for timing a build. `setRetainEnabled(false)` does it at runtime. |
| `GPUIX_RETAIN_VERIFY=1` | Every frame that replayed is built once more, whole, and compared (a frame that recorded roots to be moved is not: the frame after it moves them and is compared). gpui's mismatches print `[retained]` on stderr, are counted in `getRetainedCounters().mismatches` and listed by `getRetentionMismatches()`. gpuix's own checks print `[retain]` on stderr and are listed by the window's `getRetainMismatches()`: an element judged unchanged that built differently (also counted in `retainMismatches`), and a spine that is not closed under parent. Both kinds fail `dispose()` (`createTestRoot().unmount()` calls it). The reference pass reads the instant of the pass it is compared with. It also skips every subtree it can (minimum root 1) and turns the back-off off, so small test pages exercise replay. `setRetainVerify(true)` does it per window. |
| `GPUIX_RETAIN_VERIFY_REPORT=1` | With verify, prints the verified frames, gpui's mismatches, `gpuix-mismatches=N` (gpuix's own findings), replays, hits and non-reusable reasons for each window at dispose. |
| `GPUIX_RETAIN_MIN_ROOT=n` | The smallest subtree that is skipped unless it is costly to build (a text, an image, an `<svg>` or a custom element). Default 2; 1 takes every box, to measure. |
| `GPUIX_RETAIN_DEBUG=1` | Keeps what the last recorded frame built and why; `getRetainDebug()` returns it as JSON: elements built by reason (`slots`, largest first, named by type, `testId` and ancestors), what named them (`sources`, `causes`), the ones a record said could be skipped and were not (`notSkippedWhy`), the subtrees gpui built late (`deferred`), why an element excluded itself where the exclusion has more than one cause (`exclusionWhy`: for an `<svg>` the `Unsettled` name), and the elements ticks were declined for as outside every unit (`outside`: label, record, refusal, whether it is hidden or unbuilt; `[d:none]` in a label). Not wrapped by `TestRenderer`; call it on `renderer.native`. |
| `GPUIX_PATCH_OFF=1` | A tick of paint-only animations is drawn whole, like any frame, and not by patching the frame shown: the baseline for timing a tick. `setPatchEnabled(false)` does it per window on the test renderer (the live renderer has the variable only). Patching is also off while the retention is. |
| `setRetainPlanning(false)` | Elements are recorded and judged, and all are built: what `retain.perf` counts. |
| `setRetainLift(names)` | Counts as if the named exclusions were lifted, to see which to lift first. Nothing is reused. |

Counters on the test renderer, each since the window was made (subtract two reads for a span of frames)
unless it says otherwise:

- `getRetainedCounters()`: `hits` (subtrees a frame did not build), `replays`, `lazyBuilds`, `builds`,
  `layoutFallbacks`, `layoutSkips`, `layoutReplays`, `layoutBuilds` and `layoutRecords` (the same for layout-only passes, and the builds that left a layout record: a layout-only pass wraps what it builds, so the first build of a root is one of them; a skip at a translated place is a `layoutSkips` too, zed counts it as `layout_translations` and JS does not see it),
  `verified` and `mismatches` (under verify), `relocations` (subtrees a scroller moved without building
  them), and three JSON objects keyed by reason: `nonReusable` (`Lift`, `Escape`, `DeferredDraw`,
  `NestedLayout`, `UnstableLayout`, `UnsettledLayout`, `LayoutMismatch`, `RedrawRequest`, `Autoscroll`,
  `FrameCallback`, `PendingAsset`, `WindowControl`, `CachedView`, `Publication`), `nonRelocatable`
  (`Scale`, `Hole`, `Sticky`, `Escape`, `PositionedCapture`, `InputHandler`, `NestedScroll`, `Publication`,
  `Clip`) and `relocationMisses` (`Place`, `Region`, `Clip`).
- `getBuildCounters()` (`resetBuildCounters()` starts over): `elementsBuilt`, `elementsSkipped`,
  `styleDescClones`, `inheritedClones`, the retainable share (`retainableRoots`, `retainableElements`,
  `retainableElementsLarge`, `retainReasons` as JSON), and `retainMismatches` with
  `getRetainMismatches()` (elements judged unchanged that built differently, under `setRetainVerify`), and
  `unitsBuilt` (subtrees built whole as units) and `unitsOversized` (units over 256 elements, built whole),
  `motionsSynced` (the motion elements a sync read: all of them in a frame built whole, the ones that move in
  a patched tick) and `motionValuesSampled` (the values sampled for the elements that bind them: all of them
  in a frame built whole, the ones that move or were written in a patched tick).
- `getLayoutCounters()`: `nodesCreated`, `nodesReused`, `nodesRemoved`, `stylesSet`, `childrenSet`,
  `passes`, `measures`, `nodesLaidOut` and `nodesSized` (taffy's cache misses by run mode: a leaf change at depth d
  lays out d + 1 nodes and sizes d, whatever the fan-out; a relayout boundary stops the chain below it),
  `relationVisits` (the elements `accessibility::links::compute` read since the tree was made; 0 without a label or
  an `aria-*` reference), the text system's
  `shapes`, `wrappedLayouts` and `lineLayoutHits`, and the subtrees
  whose painted extent (`extentWalks`), scrollable overflow (`overflowWalks`) and unsnapped overflow
  (`unsnappedOverflowWalks`) were worked out from their children and not found as an earlier pass left
  them.
- `getFrameWorkCounters()`, which says what a frame did to content that did not change, from gpui's
  `Window::frame_work_counters`: `prepainted` and `painted` (elements whose own prepaint and paint ran,
  outside a culled subtree), `culledPrepainted` and `culledPainted`, `paintedPrimitives` (what painting
  put in the scene, with what is hidden or clipped away left out) and `replayedPrimitives` (what a replay
  put in again), and `boundsInserts` (the draw orders worked out with the scene's bounds tree; a scene a
  patch makes again from the orders it had pays none). A replayed root counts none of the elements under it, and a root's wrapper is not
  counted. It sits beside `getRetainedCounters()`, which counts the replays.
- `getPatchCounters()`, on the test renderer and on the live `GpuixRenderer` (so a run in a real window with
  the display link can tell a patched tick from one drawn whole): `patchedFrames` and `patchedUnits`, `emptyTicks` (ticks that
  repainted no root and drew nothing, #97), and two
  JSON objects keyed by reason: `declines`, the ticks gpui drew whole after the patch was tried
  (`PendingView`, `Refreshing`, `UnknownUnit`, `StaleRecording`, `Surroundings`, `LayoutMismatch`,
  `UnstableLayout`, `Effects`, `Hitboxes`, `Registrations`, `Operations`, `Lifts` (a unit whose root stopped being
  a stacking context while it holds lifted children, as a fade that ends at 1 on a box with an absolute child
  does: the tick is drawn whole), `Rejected`), and `unpatchable`, the
  ticks gpuix did not try to patch (`NoFrame`, `LayoutPass`, `Unsteady`, `ScrollAnimation`, `Edited`,
  `Everything`, `Sourced`, `Outside` (an element with a box that the last build gives no reason for; the first blink of a
  caret is one), `Ended`, `Dormancy` (nothing to repaint, but elements are to enter the dormant set; a tick that has no unit
  and nothing to settle is no tick drawn at all, see `emptyTicks` and #97), and `NotAUnit` for a unit that could not be
  built), and the reason an element something runs on is no unit: `Excluded:<kind>` (the kinds of
  `Exclusion`: `paint-reporter`, `highlight`, `custom-element`, `svg`, `virtual-list`, `virtual-row`,
  `collapsed-table`, `display-contents`, `anchored`, `sticky`, `fixed`, `scroller`, `layout-animation`,
  `atomic-inline`, `unstable-layout`), `ExcludedBelow:<kind>` (something in it is), `Escapes` (it holds an
  absolute box placed against a block outside it), `Lifts` (something in it is lifted out of it into a
  stacking context it does not form), `Scrolls`, `LayoutAnimation` (what runs on it moves what it lays
  out; looked at first) and `TooLarge` (over 256 elements). A page with no unit counts its ticks too. Since
  the window was made, as the others. A run is patched when `patchedFrames` equals its ticks.
- The test clock: `clockFastForward(ms)` moves a paused clock and notifies the window, so the frame after it
  is built whole; `clockAdvance(ms)` moves it without telling the window, as time passes in a live one, so
  the frame after it is a tick, which patches when only paint-only animations ran. Use `clockAdvance` with
  `setInlineDraws(false)` and `drawFrame()` to measure or test a tick.
- `getLayoutPassCount()`, `getPaintedFrameCount()` and `getCulledSubtreeCount()`;
  `getRetainedElementCount()` and `getRetainRecordCounts()`, which stay bounded by the elements alive.
- `setInlineDraws(false)` with `drawFrame()`, a frame the way the display link ends one;
  `setA11yActive(false)`, no pretend screen reader.

Run a suite with `GPUIX_RETAIN_VERIFY=1` before landing anything that changes what a build reads, what
a paint listener writes, a custom element, an exclusion, or anything gpui records. Zero mismatches across
`packages/react`, `packages/solid` and `examples` is the bar. It builds each replayed frame twice, so it
is not for timing. The gpui side has its own fuzz, which runs inside `cargo test -p gpui --lib --features
test-support`:

- **Retained fuzz** (`elements/retained/regression_tests.rs`): random pages with every feature on, played
  as scripts of frames in two windows and verified (100 seeds of each scrolled fuzz, 300 of each of the
  others). `RETAINED_FUZZ_OFF=lift,round,lifts,toggle,nested,scale,abandon,cull` takes features away to
  find which one a failure needs; a failure is shrunk to the Rust of a test that plays it unless
  `RETAINED_FUZZ_NOSHRINK` is set, which also lets a panic through. `ITERATIONS` and `SEED` run long
  (about 1.7 s a seed; `SEED=$((n * 190)) ITERATIONS=190 cargo test -p gpui --lib --features test-support
  scrolled_` is one eighth of 1500). `RETAINED_VERIFY_DUMP=dir` writes the texts of the first frame that
  differs from its build, and the registries of that frame.
- **Engine fuzz** (`taffy/engine_fuzz_tests.rs`): edits in a kept engine against a fresh one.
  `GPUI_ENGINE_FUZZ_SEEDS=first..last` and `-- --ignored a_long_run_of_edits_lays_out_like_a_new_engine`
  run a long range.

**Alternatives rejected.**

- **Memoising the resolved style per element.** Style resolution is about 3% of a
  frame; what remained of the build was gpui-side moves of `Div` and `AnyElement`.
- **Excluding frames when the accessibility tree is on**, for culling and for replay. The tree is on for
  good once any assistive client has asked, so the gate would be no optimisation for those users. Both
  record and replay the tree's nodes.
- **Skipping with a per-element dirty flag in JS.** The key is computed in Rust from the tree; JS knows
  nothing about hover, focus or scroll.
- **One recording per scroller, translated as a layer.** A hover or an animation inside the scroller would
  rebuild all of it. The unit is a retained root, so a change rebuilds its own root, and a root bigger than
  two viewports is split.
- **Making the replay cheaper and nothing else, for #62.** It stays a step per root: with every cost of #63 in,
  a tick of the 30000 node page drawn whole still takes 36.0 ms, against 0.24 ms patched.
- **Painting lifted boxes with their parents.** A `position: relative` element with `z-index: auto` is
  painted at the context's z = 0 layer in tree order (CSS Appendix E), so a recording of the parent
  cannot hold it in place; holes and fragments keep the context's order exactly.

**Upstreaming.** The #36 cache change is upstream's own #1010 (the fork's key adds to it) and is the one
clean candidate. The rest is one engine change in taffy and gpui, and a gpuix side that is specific to
this fork's element builder; none of it is separable without the persistent engine.

### Round 18: #90

#### #90: a loop nobody sees costs nothing until it is seen or has something to tell

**Problem.** A looping `motion` animation on culled content ran `dormant_tick` at every display frame, and each
run synced every motion on the page (`sync_motions`, `parent_inherited`) though nothing was drawn: about 3.5% CPU
per loop in FF. A loop under `display: none` was worse, since it is not dormant: every tick was a whole frame (60
built frames and 4800 motions synced over 60 ticks of 40 cards). A culled CSS loop and a culled bound motion value
ran a tick at every frame too, and a patched tick beside a visible loop stepped every culled motion.

**What changed.** Chrome runs an animation on content it does not show without painting for it, and one that
reports nothing (a loop with no listener) is not run at all until it is visible: what it shows is a function of
the time. So a dormant source says when it next has something to tell and the window asks for no more.

- **A dormant source says when it next has something to tell** (`dormant_wake.rs`): a `Wake` is `Never`,
  `NextFrame` or `At(instant)`, merged by the earliest, and `GpuixView::wake_dormant` asks for no more than that.
  `NextFrame` (and an `At` that has passed) queues the frame callback of `dormant_tick`; a later `At` holds an
  executor timer for the wait that then queues it; `Never` drops the timer and asks for nothing. A new wake replaces
  the timer in place. The end of a frame, of a patch and of a dormant tick all go through it
  (`GpuixView::dormant_clock_wake` merges the CSS animations, transitions and motion values; the motions' wake comes
  from the split below).
- **`MotionState::wake`** is `At(end)` for a timed tween or keyframes (delay and finite repeats counted), `Never` for a
  repeat without a count, `NextFrame` for a spring and for a measurement that is pending.
- **`CssAnimations::dormant_wake`** is the earliest instant a dormant run starts (the end of its delay) or ends, taken
  from a heap of them (`Sleepers::boundaries`), and its next iteration only for an element whose event path
  listens for `animationiteration` (`path_listens`, which is also what gates the emission; Blink's
  `RequiresIterationEvents`). Which iterating elements listen is asked again when the tree's revision moves. A run
  that cannot say (a `play()` waiting for a frame, a negative rate, an element just built or commanded) is
  `NextFrame`; a held one, and a loop nobody listens to, `Never`. `Run::events` holds the two instants beside
  `quiet_until`, which still decides which runs a tick looks at.
- **`CssTransitions::dormant_wake`** is the end of the delay and the end of each dormant running transition
  (`Playback::wake_at`).
- **`MotionValueStore::dormant_wake`**: a moving value with a `change` listener is `NextFrame` (Motion's `change`
  follows the animation, not the element being seen); one with only an `animation` listener is the end of its
  animation when that is known (`MotionValues::wake`: a tween's clock, a repeat's end, `NextFrame` for a spring); one
  nobody listens to is `Never`, whoever binds it. `advance_dormant` only finds the events of the listened values and
  no longer samples the bound ones: the frame that draws an element again does, and a spring follower catches up
  (`MAX_CATCH_UP_FRAMES`).
- **A motion with no box sleeps like a dormant one** (`GpuixView::motion_sleeps`: dormant, or hidden by `display: none`
  or a `content-visibility` ancestor), and so does a value bound only to such elements (`value_sleeps`), so neither
  asks for a frame. CSS animations and transitions under `display: none` are cancelled and their state dropped, as
  they were; that is not changed.
- **A tick steps what is awake or due.** `step_motions` splits the motions that move into the ones that are drawn
  and the ones that sleep (`MotionSplit`, taken at the end of a frame and after a step), steps the first, and the
  second only when the earliest of their wakes is due; `MotionStep` returns the awake ones and that wake.
  `MotionValueStore::step` leaves a moving value bound only to sleeping elements as it is (`BindingIndex::bound_only_to`).
  A frame still syncs every motion.
- **`getAnimations()` and the commands of an element that is dormant read the clock**, not the instant of the last
  frame: no frame or tick need have come for it for any time (`GpuixView::api_instant`).
- **`getFrameWorkCounters().dormantTicks`** counts the runs of `dormant_tick` (it is not cleared by
  `resetBuildCounters`).

**Measured** (`offscreen-animation-work` and `animation-frame-work`, test renderer, 16 ms ticks). Dormant ticks over
60 ticks of 40 culled cards, clipped by an `overflow: hidden` scroller: a motion loop 60 before part 1, 0 after; a
CSS colour or stroke loop with no listener 60 -> 0, and the page costs what an empty one costs (painted frames,
patched frames, elements built, motions synced all 0); 20 culled bound motion values with no listener 60 -> 0, with a
`change` listener one a tick as before. With an `onAnimationIteration` listener 200 ticks of 40 loops of 1 s take 200
dormant ticks before and 3 after (one per iteration), each delivered in the tick that passes its boundary. Culled
tweens, CSS animations and CSS transitions of 500 ms end in the tick that passes 500 ms, with 32 dormant ticks before
and at most 2 after. A patched tick of one visible motion loop beside 40 culled motion loops and 40 culled CSS loops
syncs 41 motions before and 1 after, the work of the visible loop alone. Under `display: none` 60 painted frames and
4800 motions synced before part 1, 0 after.

**Open.** The iteration listeners are asked again when the tree's revision moves, so a page that commits at every
frame walks the path of each iterating dormant element at each. `animationstart` and `animationend` wake whether or
not anything listens, since the `Animation` objects of `getAnimations()` listen by existing.

#### #97: skipped content costs the window no frame, and a tick that changes nothing drawn draws nothing

**Report.** In FF a ThinkingIndicator (`MorphingGlyph`, a `motion.path` `d` loop, and `Shimmer`, a CSS colour loop per letter) in skipped
cards took an idle page on poc from 4 to 6% CPU and 0 whole frames a second to 20 to 23% and about 72.

**Found.** Not reproduced on this branch in the test renderer, so what poc did was not measured here: after #90 a page of cards that each
run a `motion.div` loop, a `motion.path` `d` loop and a CSS colour loop (and FF's indicator: eight letters of colour keyframes, a 6 s `d`
morph, a `top`/`opacity` roll on a `position: relative` `motion.div`) draws nothing over 60 ticks, whether the cards are skipped by
`contentVisibility: hidden` or by `auto` scrolled far away, and whether the loops started in skipped content or were running when it
began to be skipped: 0 painted frames, 0 patched, 0 dormant ticks, 0 motions synced, the same as 0 cards (`content-visibility.test.tsx`,
`content-visibility-auto.test.tsx`; the `auto` ones wait 200 ticks, since the scrollbar fades and draws after a scroll). Two things still
cost, found with the same cards:

- **A whole frame rebuilt the path to every skipped host.** `running_motion` named a motion by `is_animating` alone, so a motion under a host
  that skips (no box, nothing built) was a running source: `compute_deltas` put it in `changed`, the spine of the frame took its host
  and every ancestor, and a hover frame over a sidebar rebuilt the page around eight skipped hosts instead of replaying it (300 elements
  built over 20 hovers, 120 for cards that run nothing). It leaves out a motion whose host has no box (`is_hidden`: `display: none` or a
  skipping ancestor; an `<svg>` shape counts as its `<svg>`), so a skipped subtree has no running source, as `Retain::skip_content`
  says. A motion on the skipping host itself is not skipped content and runs as before.
- **A tick that changed nothing drawn was drawn whole.** A paint-only write to a motion value bound inside skipped content requests a
  tick (`Repaint::MotionWrite`); the preview found no unit (the element is skipped), and `build_patch` declined it as `NoUnits`, so
  gpui drew the whole frame: 20 writes, 20 painted frames. A preview with no unit now steps the clocks and calls
  `repaint_in_place` with no root, which builds, lays out and presents nothing: gpui counts it in `PatchCounters::empty_ticks`
  (zed `bfd7dc43f3`), `getPatchCounters().emptyTicks`, and not in `patchedFrames`. `step_clocks` emits the transition and animation
  events the units would have, and a preview with no unit but with elements about to enter the dormant set (or whose verdicts
  moved) is still drawn whole, counted as `Dormancy`, since the frame is what settles that: left to an empty tick, culled
  transitions never became dormant and their `transitionend` never came (the culled-transition test found it). `NoUnits` is no longer
  a key of `unpatchable`; `Dormancy` is.

**Measured** (test renderer, 16 ms ticks, release addon). 20 hovers with 8 skipped hosts of looping cards: 300 elements built before, 120
after (the number for hosts that run nothing). 20 writes of a motion value bound in 8 hidden hosts: 20 painted frames before; 0 painted,
0 patched, 0 declines and 20 `emptyTicks` after. Unchanged by this: 60 ticks of skipped loops 0 frames, and beside one visible loop the
ticks of the loop alone.

**Open.** (1) FF's 72 frames a second was not reproduced here; it needs a measurement on this branch. (2) An `auto` host that is not
skipped (inside the 150% margin) but clipped by an `overflow: hidden` scroller, with a CSS loop that began visible, draws a whole frame
at every tick (`declines.UnknownUnit` 60 of 60 ticks, no dormancy); it is not skipped content. (3) A skipped spring reports `NextFrame` and gets a dormant tick (no draw) at every frame. (4) A whole frame still
syncs every motion, skipped ones too (`motionsSynced` counts them): 720 over 60 whole frames of 12 skipped motions.

**Part 2: what skipped content tells and shows when it is shown.** Against Chrome 156
(`fixtures/content-visibility-animation-oracle.{html,py,sh,json}`; scenes `q1`..`q3`, `hidden` and `auto`, regenerate with
`sh fixtures/content-visibility-animation-oracle.sh`, which needs `uv` and the cached headless shell) the events and values of
CSS animations and transitions in skipped content, played in the test renderer in ticks of 16 ms
(`skipped-animation-events-fixture.tsx`, called by `content-visibility.test.tsx` for `hidden` and
`content-visibility-auto.test.tsx` for `auto`). Chrome tells nothing for a run while its content skips; at the reveal it tells what the
run owes (one coalesced `animationiteration` with the `elapsedTime` of the iteration reached, `animationstart` and `animationend` of a run
that began and ended meanwhile, `animationend`, `transitionend`), two or three frames later; the first frame shows the value of the instant;
an animation or class added, or a style changed, while skipped starts (its transition) at the reveal from the start; content skipped from its
first frame is styled at the reveal and starts then; `getAnimations()` of a running animation reads the live time and tells nothing. The
tests allow gpuix the first three frames after the reveal and none before.

- **Already as Chrome, no change** (the 9 scenes x 2 modes of the tests): a loop skipped 1.35 s tells one `animationiteration` (1.5) and
  nothing earlier, its first frame is the control's colour and time; a finite 300 ms animation and a delayed one that ended while
  skipped tell `animationend` (0.3), `animationstart` and `animationend` at the reveal, with the end value; a class added while skipped
  starts at the reveal from 0; a transition started before the skip tells `transitionend` at the reveal with the end value; a style
  change while skipped (no read) starts its transition at the reveal (from 0, also when the reveal comes mid-way); content skipped from
  load starts its animations at the reveal (`auto`: a host that is far at the first frame); `getAnimations()` while skipped reads the live
  `currentTime` and tells nothing; a motion value written at every tick into skipped content shows the last value when it is shown. `auto`
  draws the content one frame after the reveal (it is built in the frame that finds it near), the events still come in the first.
- **Fixed: a crash.** `animate_style` marked an element as animating opacity when a run of its `opacity` was current, but
  an element built as forced content (a geometry read inside skipped content, `is_forced_by_ancestor`) is not stepped, has no animated
  style, and the `expect("an opacity that runs has the style it runs on")` aborted the process. It reproduced as soon as a scene
  read the bounds of an element with a looping `opacity` in an `auto` host being scrolled back (q1/q2 `auto`); it now marks only an
  element it stepped (`steps`). The test reads the bounds of such an element while skipped, at the reveal before any frame, and after.
- **Differs, left**: a `getAnimations()` read of content that gpuix never built (loaded skipped, or a class added while skipped) lists
  nothing, where Chrome's read styles the element and creates the animation, which then runs while skipped and tells `animationstart` and
  `animationend` at the reveal (`q1_load_read`, `q2_class_read`). gpuix has no style for never-styled skipped content; a forced read starts
  no animation on purpose (`a read of geometry below a skipped element ... starts no animation`). The tests pin gpuix's answer (no
  animation listed, started at the reveal). Chrome's `getComputedStyle` read in skipped content (`q3_inside_read`, `readStyle`) has no gpuix
  counterpart and is not played. Motion's completion callbacks, which are JS in Chrome and fire on time, keep #90's `At(end)` wake.

### Round 19: #92 CSS transforms

**Problem.** GPUI drew only translations (`Window::with_element_offset`) and gave only SVG and glyph sprites a matrix, so gpuix had no
`transform`, `scale` or `rotate`, and a motion component could not slide, scale or turn. FF's components faked each of them with
`top`, `height` and insets, which lay out on every tick, or with a JS tween that wrote a style per frame. The table below says what is
supported; the rest of the section is how GPUI draws, hits and animates it and where it differs from Chrome.

**Scope.** macOS and Metal only (no wgpu, HLSL or WGSL). Rotated and scaled text may be soft; a pure
translation stays pixel-snapped and sharp. Motion's transform keys and CSS transitions and animations of `transform` ship with it, and
their ticks patch. Not built: `layout="size"` as a FLIP scale (the layout glide is still an offset, so a size change snaps), 3D, and
`element.animate()`.

#### What is supported

| Surface | What it takes |
|---|---|
| `transform` | `none`, or a list of `translate()`, `translateX()`, `translateY()`, `scale()`, `scaleX()`, `scaleY()`, `rotate()`, `skew()`, `skewX()`, `skewY()` and `matrix()`. Lengths are px, `%` (of the element's own border box), em, rem, vw, vh and `calc()` of them; angles are `deg`, `rad`, `grad`, `turn` and a bare `0`. The z forms (`translate3d()`, `scale3d()`, `translateZ()`, `scaleZ()`, `rotateZ()`) are accepted and draw in 2D. `rotateX()`, `rotateY()`, `perspective()` and `matrix3d()` are invalid, and an invalid value is ignored as a whole, as in CSS. |
| `translate`, `rotate`, `scale` | The individual properties. `rotate` is a string angle or a number of degrees; `scale` is one or two numbers or percentages, or a number. |
| `transformOrigin` | One or two of `left`, `center`, `right`, `top`, `bottom`, lengths and percentages; `50% 50%` by default. |
| Motion keys | `x`, `y`, `translateX`, `translateY`, `scale`, `scaleX`, `scaleY`, `rotate`, `rotateZ`, `skew`, `skewX`, `skewY`, `originX`, `originY` as `style`, `animate`, `initial`, `exit`, `variants`, keyframe lists, springs, gestures and motion values. `x` and `y` take a number (px) or a CSS length; `rotate`, `skew` take degrees or a CSS angle; `originX` / `originY` a fraction of the border box or a CSS length. |
| 3D keys | `z`, `translateZ`, `rotateX`, `rotateY`, `transformPerspective` and `perspective` are accepted on motion components and ignored. |

**Composition.** CSS Transforms 2: around the origin, `translate`, then `rotate`, then `scale`, then the list (matrices multiplied in
that order, each applied to what the ones after it placed). The list is `transform`, or Motion's transform when any Motion key is
present; Motion's replaces `transform`, as `buildTransform` writes into it. Motion's order is `translateX(x) translateY(y) translateX
translateY scale(scale) scaleX scaleY rotate(rotate) rotateZ skew skewX skewY` (motion-dom 14.1.0). `originX` / `originY` override
`transformOrigin` per axis. A transform other than `none` makes the element a stacking context (painted in the `z-index: 0` layer) and
the containing block of its absolute and fixed descendants; an identity value (`translateZ(0)`, `scale(1)`) does both too, and `none` or
an invalid value does neither. It changes no layout: margins collapse through a transformed box as without it. A non-atomic inline box
takes no transform, as in CSS; the keys on a `<span>` are accepted and ignored. State styles (`hover`, `focus`, ...) register all the
keys.

**Wire names.** The JS motion components rename a static `scale` and `rotate` in `style` to `motionScale` and `motionRotate`
(`motionComponentStyle`), so Motion's `scale: 1.2` does not meet the CSS property of the same name. A motion value keeps its key, and
the native motion contexts (`animate`, `initial`, `exit`, variants, keyframe arrays, motion-value bindings) read `scale` and `rotate` as
Motion's. On a plain element `scale` and `rotate` are CSS. CSS keyframes (`defineKeyframes`) and transitions take the CSS properties
(`transform`, `translate`, `rotate`, `scale`, `transformOrigin`); Motion's keys are not CSS properties, and `transition: all` leaves
them alone.

#### GPUI

- **`Window::with_transform(matrix, f)`** composes onto the ambient transform (logical px, local space to window space), and every
  painter and prepaint inside it reads it. Each push is classified: identity, translation only, or general.
  - *Translation only* (the composed transform is a pure move): every `paint_*` adds the translation, rounded to whole device pixels,
    to the logical bounds before snapping, and the primitive keeps an identity matrix. Content masks pushed inside are mapped into
    window space. The output is byte-identical to the same content placed there, so text stays sharp (`transform-paint.test.tsx`,
    "draws a translated line of text exactly as the same text placed by an offset").
  - *General*: quads, shadows, underlines, sprites and paths carry the matrix in device px (the scale factor folded in); their
    bounds, radii and borders stay in local device px. Subpixel text is off under a matrix. Paths bake the matrix into their vertices.
- **Clip model.** A primitive's `content_mask` stays in window space (hardware `clip_distance` on the transformed vertex, rounded
  coverage at the window fragment position), as for untransformed content. `with_content_mask` under a general transform goes into a
  separate `local_clip` in the primitive's local space, which the fragment evaluates with the local position. Entering a general
  transform inside another one with a local clip maps that clip exactly into the new space when the relative matrix only scales and
  moves; otherwise the clip is replaced by its bounding box in window space, intersected into `content_mask`, with its corner radii
  dropped. That is the one loose place: a rotated child of a rotated `overflow: hidden` parent clips to the parent's bounding box,
  not to its rotated shape. The window mask is never intersected with the bounding box of a local clip, so hitbox and primitive masks
  stay the same from one tick to the next.
- **Shaders** (`shaders.metal`: quads, shadows, underlines, monochrome and polychrome sprites). Under a non-identity matrix the vertex
  stage grows the local quad by one device px (in local units, `1 / scale_factor`) and maps it through the matrix; the fragment
  computes the box SDF, rounded corners, borders, dashes and the local clip in local coordinates and antialiases over
  `0.5 / scale_factor` local px. A transformed quad never takes the pixel-aligned fast path, so straight edges are antialiased. An
  untransformed primitive runs the same code with an identity matrix.
- **Surfaces and groups.** Effect groups (opacity, filter, mask, `clipPath`) cover the bounding box of their transformed content;
  `clipPath` polygons are mapped through the matrix (a rounded rectangle or an ellipse under a rotation becomes a polygon); mask
  layers rotate exactly. A `mask-image` or `backdrop-filter` under a general transform uses the bounding box, and a surface is drawn
  exactly under a translation and as its bounding box otherwise.
- **Draw order and culling.** `insert_primitive_local`, the bounds tree and `primitive_bounds` use the transformed bounding box
  intersected with the content mask. `lies_outside_the_mask` and `prepaint_unlifted_child` cull a child by its transformed bounding
  box, and a culled element keeps its transform (`Window::layout_own_transform`) so the boxes of its descendants stay right. Every
  `translate` of a primitive (retained relocation) moves `transformation.translation` too.
- **`Style::transform`** (`Option<ElementTransform>`, `None` = unset, `none` = `Some` with no functions so a state style can clear the
  base) is resolved by `Div`, `List` and the elements on `Interactivity` from the **unsnapped** border box layout computed, not from
  its pixel snap, in prepaint and in paint, and wraps the element's hitbox, background, border, shadow and children in
  `with_transform`. `Style::contain_positioned` makes a box the containing block of fixed and absolute descendants without the rest
  of layout containment (what a transform needs). gpuix decides stacking contexts itself; GPUI infers none. The layout engine keeps
  each node's transform (stored at `request_layout`, compared in the node's meta so a change bumps the extent epoch) for scrollable
  overflow.
- **Hit testing and events.** A `Hitbox` keeps the space it was inserted in (`to_local`, the window-to-local matrix of the ambient
  transform, rounded to device px for a translation; none for a matrix that flattens the plane, which hits nothing) and the local
  clip. `contains_point`, `Frame::hit_test`, `dom_hit_test` and the clip-path test of `ClipIndex` test each hitbox in its own space;
  `Hitbox::to_local`, `window_bounds` and `contains_window_point` are public, and `Window::hit_chain_entries` gives the space of each
  link. A mouse listener keeps the space it was registered in and is given the event with its position mapped
  (`MouseEvent::with_position_mapped`: move, down, up, wheel, pinch, drop), so `clientX` is in the window and `offsetX` in the
  target. `Div` and `InteractiveText` compare the pointer with their hitbox through the space. `Window::mouse_position` stays in the
  window.
- **Patch and retained replay.** A patch's `Surroundings` and a retained `ReplayKey` carry the ambient transform and local clip (as
  they carry `element_opacity`). A recording is not replayed under a different ambient transform, and is not moved by a scroll or a
  layout when the ambient is more than a translation: it is built. A patch compares hitboxes ignoring `to_local`, `content_mask`, the
  local clip and the exact clips, and after a successful patch copies those, and the spaces of the listeners, from the scratch frame
  into the rendered one (`Hitbox::relocated` moves the space, not the bounds, inside a transform that is more than a translation).
  So a tick that only changes a transform is a patch with no declines (`patch_tests.rs`: a card that turns, a root and a card that
  turn, a card that slides, a listener given the position in the card as it is now, a subtree replayed inside another transform
  built again).
- **Operations do not depend on the clip.** `patch_unit` declines a unit whose operation count changed (`Operations`). A box or a line
  that a transform slides out of or into a clipping parent used to change it, because the scene dropped a clipped primitive before
  recording it, `paint_layer` pushed no layer for clipped-away bounds and text left out the glyphs a font away from the mask. Now a
  primitive, quad or layer clipped away is still recorded, and a patchable root paints every glyph (see the deviations).
- **SkewX and SkewY** are their own `TransformFunction`s that pair only with their own kind; the 2D matrix decomposition used by
  `Interpolated` orthogonalizes the second column (Gram-Schmidt), so a shear blends as a shear.

#### gpuix

- **Parsing and apply** (`css/transform.rs`, `Parts`): the five CSS properties and Motion's keys are parsed once per distinct value
  (`Cache`), resolve em / rem / vw / vh with the element's `LengthContext`, and become one `ElementTransform`, which `apply_styles`
  puts on the GPUI style. `Parts::is_active` says whether the element has a transform in CSS's sense; `layout/stacking.rs`
  (`creates_context`, `lift_z` at 0, like an effect group) and `layout/containing_block.rs` (`contain_layout`,
  `containing_block::contains_fixed` for the retained walks) read it, so `RetainedTree::elements_at` models the same contexts.
- **`animating_transform`.** While a CSS animation or transition, a `motion` track or a moving motion value of a property that moves,
  scales or turns the element is current, `StyleDesc::animating_transform` keeps the stacking context and the containing block at
  every value it samples, as `animating_opacity` keeps the effect group. The origins only place a transform and do not count. So a
  tick never flips the stacking context or the containing block, which would be a layout change. `OpacityRun` is now `LiftRun`
  (`Record::lift_run`, `Kept::LiftRun`, named `lift-run`): a record made on the other side of a run's start or end of
  either animation is not skipped.
- **Paint-only.** A change of a transform value is paint-only (`differs_only_in_paint`) as long as "has a transform" does not flip;
  a flip changes the containing block, so it is layout. Every transform `AnimProp` is in `paints_only`, so the ticks patch.
- **Tapes.** A patched tick of a moving transform changes the boxes the elements publish. The tape of a patch is compared with the
  recording apart from the transforms around each box (`same_writes_apart_from_transforms`, `describe_apart_from_transform`), and
  after a successful patch those boxes are published from the capture (`each_bounds`, `publish_captured_bounds`). Anything else that
  differs is still a mismatch.
- **Motion.** `AnimProp`s for every Motion key (numeric and length kinds) and `AnimValue::Transform` for `transform`, `translate`,
  `rotate` and `scale`; CSS transitions, `@keyframes`-style animations, `motion` tweens, springs and keyframes, motion values and
  Web Animations accept them. Motion's keys are positional for reduced motion (they jump, as Motion's transform animations are
  disabled), CSS transforms are not.

#### Geometry

Chrome's rules, with the layout box and the painted box kept apart.

- `getBoundingClientRect` (and `getElementBounds`, `probeElementBounds`, `getBuiltElementBounds`, the automation tree) is the smallest
  box that holds the four corners of the border box after the transforms around the element and its own. `offsetWidth`, `clientWidth`,
  `scrollWidth` and the other sizes, `ResizeObserver` and layout reads stay the layout box. `ElementBounds` keeps the layout box and
  `to_window`, the exact matrix, taken in prepaint from `Window::transform` and `Window::layout_own_transform`.
- `offsetX` and `offsetY` of a pointer event are in the target's own space (from its padding edge), through the inverse; inline spans
  map through their paragraph's container. `getElementsAt` tests each clip and box through its inverse, so it agrees with the pointer.
- `scrollIntoView` aligns the transformed box in the nearest scroller's space; key scrolling under the pointer, `IntersectionObserver`
  and `content-visibility` proximity use the window-space box. An `IntersectionObserver` target with a transform is intersected as
  its bounding box and its border and scrollbar room count as 0.
- A scroller scrolls as far as the transformed boxes inside it reach (forwards and down only), through `transformed_overflow` in the
  taffy engine, in both the snapped and the unsnapped extents. It reads the transform the node had when layout last ran, so a
  patched tick of an animated transform does not move the extent; the next layout pass does.

#### Animations

- **Pairing, as Chrome does it.** Lists that pair up function by function blend each pair (translate lengths blend their px and their
  fraction apart; `rotate`, `scale`, `skew` blend their numbers), the shorter list padded with identity functions. From the first pair
  that does not match (different kinds, `matrix()`, or a flip against a rotation), the rest of both lists is emitted as an
  `Interpolated { from, to, progress }` function, which GPUI resolves with the box size and blends through the CSS Transforms 1 2D
  matrix decomposition (translate, rotate, scale, skew); the matching head is blended on its own. `none` blends as the identity of
  the other list. The individual `translate`, `rotate` and `scale` blend as their own values, and `transform-origin` as two lengths.
  The pairing is `motion/transform_value.rs`; the keyframes of a run are positions among their distinct lists
  (`Layout::Transform`).
- **Internal formatter.** An animation hands its style to the renderer as text, so the tail of a blend between lists that do not pair
  is written as `-gpuix-interpolated(<progress>, <from list>, <to list>)` (`css/transform.rs`). It is not author CSS.
- **Patched ticks.** A tick of a transition, a CSS animation, a tween, a spring or a motion value of any of these is a patch with no
  decline of either kind (`getPatchCounters`), including a word that rolls up into an `overflow: hidden` slot (`y` from a percentage).
  Seeking, pausing and playing through `getAnimations()` draw whole frames (`Repaint::Layout`, as for any property).

#### Tests and oracles

- `fixtures/transform-geometry-oracle.{html,sh,json}`: 25 scenes against headless Chrome (translate, rotate, scale, skew, matrix,
  origin, percent, nested, individual, scrolled, flow, hit-*, offset-*, into-view, extent); `transform-geometry.test.tsx` compares
  boxes within 0.06 px and hits, offsets and scroll extents exactly. Regenerate with `sh transform-geometry-oracle.sh`.
- `fixtures/transform-animation-oracle.{html,sh,json}`: 72 animation cases (every pairing rule above, with flips and skews) read at
  several times; `transform-animation.test.tsx` holds every box within 0.06 px of Chrome's, with no allowed deviations.
- `transform-paint.test.tsx` (pixels: rotated rounded box, origin, composition order, an image, rotated and nested clips,
  translated text equal to offset text, inline-block against inline), `transform-stacking.test.tsx` (layer, `none`, negative
  z-index child, fixed and absolute children, margin collapse), `transform-states.test.tsx` (state styles and replay; run with
  `GPUIX_RETAIN_VERIFY=1` too), `transform-css-patch.test.tsx` (patch counters of CSS transitions and animations and a Web
  Animations seek), `motion-transform.test.tsx` in `packages/react` and `packages/solid` (the box of each key against the same CSS
  transform to the pixel, the wire names, tweens, springs, keyframes, motion values, exits, and patch counters).
- Rust: `window/transform_tests.rs` (matrix, origin, percentages, clips, culling, hit testing, scroll range, lifted elements) and
  `window/patch_tests.rs` in zed; the tape comparison in `tape.rs`.

#### Measured

FF's pages after the check (headless footprint, 10 s idle, 75 Hz; CPU is indicative, the machine's load differed between runs).
Patched and whole frames a second, before to after: changelog 0 / 33.8 to 32.9 / 18.3, fluid-hover 0 / 39.0 to 35.5 / 19.3,
carousel-dots 0 / 69.1 to 64.2 / 5.4, thinking-indicator 66.3 / 4.0 to 67.7 / 2.5. FF's own checks (changelog, plain-vs-fluid,
thinking-indicator, tsc) pass; the carousel-dots check read the fill's width and now reads its painted right edge. What is still a
whole frame there is not transform work: the hover commit (`PendingView`), the fluid highlight's `top` / `left` / `width` / `height`
(`Edited`, `LayoutAnimation`) and the carousel track's width spring. With the thinking-indicator's word slot `overflow: visible`
instead of `hidden`, `Operations` fell from 11 to 3 in 10 s: the declines were the word's text moving in and out of the clip. The
operation list that no longer depends on the clip ("Operations do not depend on the clip" above) closed them in the tests
(`motion-transform.test.tsx`, "a word that rolls up into an overflow hidden slot"); FF was not measured again.

#### Deviations

- **Text.** Rotated and scaled text is resampled and may look softer than Chrome's; subpixel text is off under a general matrix. A
  pure translation is exact.
- **Loose clips.** A rotated or skewed child that is itself clipped, inside a rotated clipping ancestor, clips to the ancestor's
  bounding box with its radii dropped (the clip model above). The pointer's path under such a nested clip is loose in the same way.
- **Bounding boxes.** Surfaces, `backdrop-filter` and `mask-image` under a general transform use the bounding box of the transformed
  content, not its shape. Clip paths and mask layers are mapped exactly.
- **Offscreen glyphs in a patch unit.** A patchable root paints every glyph, with no culling of glyphs inside the unit, so
  off-screen glyphs of a long text in a unit are rasterized there. Mask and background tile culling still varies with the clip, so a
  tiled mask or background that a transform moves across a clip edge can still change the operation count.
- **Motion's defaults.** A Motion key at its default (`x: 0`) still gives the element a stacking context and a containing block;
  Motion emits no transform for it. Motion values cannot carry angle strings (`"45deg"`), only numbers; a static `rotate` takes both.
- **Animation API.** `animate()` of a motion component takes Motion's keys, not a `transform` string. There is no `element.animate()`;
  `getAnimations()` seek, pause and play commands draw whole frames.
- **No 3D.** `rotateX()`, `rotateY()`, `perspective()` and `matrix3d()` are invalid in a string and the matching Motion keys are
  ignored; the z forms draw in 2D. A translation's `calc()` is a length plus a percentage of the box (`calc(10px + 50%)`), so FF's
  carousel fill computes its `x` in px in JS.
- **Layout animations.** `layout`, `layoutId` and `Reorder` still glide by an offset; `layout="size"` does not scale with a transform.
- **Not transform-aware yet.** Selection-drag autoscroll against a list's viewport and focus-into-view (`focus/skipped.rs`) use the
  untransformed box; `scrollIntoView` through several nested scrollers uses the nearest scroller's space for the whole chain.
- **Scroll extents of a non-scroller.** `scrollWidth` of an element that is not a scroller ignores overflow, and an absolute child at
  the end of a padded scroller adds the padding (scenes `e12` and `e17` of the geometry oracle, `EXTENT_DEVIATIONS`). Both are older than this round.
- **A recording is built, not moved, inside a transform.** A retained subtree replayed under another transform, and one scrolled or
  laid out in a transform that is more than a translation, is built again (`ReplayKey`), so a page that scrolls inside a rotated
  card does not get the relocation speedup.

**Open.** FF's workarounds that can go now (queued-stack and callout cards as insets, the chevron's JS tween, the dash-sliding
spinner, the arrow glyph, dialog scale, the changelog dot) can be rewritten with `y`, `scale` and `rotate` and have not been; the
sidebar's `marginLeft` for sub-rows stays, since the rects measured there deliberately include the offset.

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
  `bun run test:perf:menus` in `examples/`; profiling, no budgets). Before the perf pass a frame spent
  about 70% in build, `request_layout` and taffy and about 27% in prepaint and paint, and an idle frame
  (`renderer.flush()`, nothing changed) took, release build, p50 CPU: 10.6 ms with every menu laid out,
  4.1 ms with the far ones hidden, 2.8 ms with all hidden. `GlobalElementId` hashing its id path once, at
  construction (zed `1f55cd8cd9`), took that to 9.6, 3.7 and 2.7 ms. The perf pass took the idle frame
  to 0.99, 0.45 and 0.37 ms when retained subtrees first replayed (95% of the elements are in subtrees an
  idle frame leaves unbuilt); the page was not measured again after translated replay and lift holes.
  Build and layout, not paint, were most of the cost of a hidden menu, so culling alone takes a clipped
  box from 4.9 µs, the same as a visible one, to 2.1 against 3.1 for a visible one, and the retained
  replay is what makes the idle frame cheap. Skipping build and layout under a fully clipped node stays
  undone, because the descendants' bounds are readable from JS (`getBoundingClientRect`, scroll metrics,
  observers).
- **The perf pass (#36 to #42)**, below. Round 9 is counted in the test renderer and in the notes of its
  commits, not in a table: [what a hover frame costs](#46-57-58-what-a-hover-frame-costs) has the
  numbers, and the demo's pages were not measured again after it.
- **Round 12 (#62, #63)**, test renderer, release addon: a tick of one colour loop on 30000 nodes took 85.8
  ms drawn whole and takes 0.24 ms patched (500 nodes: 0.94 to 0.22 ms, 3000: 5.84 to 0.23, 12000: 30.5 to
  0.22; in a live window every tick after the first was patched and cost 0.6 to 1.0 ms at 3000, 12000 and 30000
  nodes). A hover over 5000 cards clipped out of view with colour loops took 37 to 43 ms and takes 3.0 ms (0.4
  ms with no cards), and an idle tick of them takes 0.00 ms. The [tables](#round-12-62-and-63) are in its
  section. The demo's pages were not measured again after it.
- **Round 14a (#81, #74, #75, #76)**, test renderer, release addon, draw CPU: a colour loop under a
  `relative` wrapper went from 3.45 to 0.67 ms at 500 nodes and from 8.89 to 0.43 ms at 8000 (r13); a text
  under `relative`, `absolute` or `opacity < 1`, or itself `absolute`, from 3.1-4.4 ms to 0.36-0.40 ms at
  3000 nodes (r18); a loop beside a focused `<input>` from 4.0 to 0.9 ms at 7.8k elements and from 7.4 to
  0.34 ms at 13k (r17); a loop on an `<svg>` with one running circle from 4.30 to 0.12 ms at 5000 nodes. A
  page of 190 entries replays at its second draw (14.3 to 1.8 ms), and a changed `color` on it builds 2
  elements and not 1902 (17 to 1.6 ms), at the price of a first draw about 15% dearer. FF's showcase went
  from 68 whole frames a second at 6.3 ms and 40 to 44% of a core to 63 patched of 68 at 3.3 to 4.2 ms and 21
  to 28%, and its patched tick from 4.11 to 1.82 ms (the tile alone: 1.92). See
  [Round 14a](#round-14a-81-74-75-76-and-the-patch-leftovers).
- **Round 14b (#77, #78, #79)**, live window, release addon, 75 Hz: an idle one-text window went from 75 JS
  wakes/s, 6.9 tick ms/s and 1.45% CPU to 0 wakes, 0 ticks and 0.29%; a visible window that is not key
  animates at 75 to 76 draws/s (about 30 before); the footprint of one window and one text went from 70 to
  65 MB. See [Round 14b](#round-14b-77-78-79).
- **Round 14c (#93)**, live window, release addon: a key down, mouse move, click and scroll posted to an idle
  window wake JS 0.1 to 1.4 ms after the post (before: no wake) and reach their handler 0.4 to 7.7 ms after
  it. See [Round 14c](#round-14c-93).
- **Round 16 (#83)**, test renderer: a forced read after a one-leaf change lays out 7 nodes in a list of 8000 elements, as
  in a list of 100 (a bounds read after a colour write lays out none); a page of 5500 elements that mounts its list and
  writes `scrollTop` 11 times with no frame builds 5502, 14, then 13 elements a round, where every pass built every
  element (about 28 ms a pass at 5000 elements; 5500, 5500, then 13 before Round 17). See [Round 16](#round-16-83-82-and-the-content-visibility-leftovers).
- **Round 17 (#88)**, test renderer: the first read after a change inside skipped `content-visibility` content is one
  layout pass (it was two). A read of a line in a hidden element of 300 cards (1203 elements) after a paint-only write and
  a draw builds 3 elements in 0.5 to 0.8 ms (1203 in 6 to 27 ms), and a hidden element of 8 cards 4 elements with 8 layout
  skips (36 from scratch). A line that grows in a card moves the cards after it and the read builds 8 elements with 7 skips
  (28); a page of cards with a scroller moved by a box above builds 3 (33). See [Round 17](#round-17-88-a-read-inside-skipped-content).
- **Round 19 (#92)**, FF's pages in a headless footprint run (10 s idle, 75 Hz), patched and whole frames a second before and after
  the transform keys replaced the layout-based fakes: changelog 0 / 33.8 to 32.9 / 18.3, fluid-hover 0 / 39.0 to 35.5 / 19.3,
  carousel-dots 0 / 69.1 to 64.2 / 5.4, thinking-indicator 66.3 / 4.0 to 67.7 / 2.5. See [Round 19](#round-19-92-css-transforms).
- **DOM parity**: areas A to E are behavior, not speed. Two costs are known: a same-turn bounds or
  scroll read is a layout pass (C, and the perf pass makes it cheap), and a `Quad` is larger (D, and by a matrix in Round 19).
- **Layout features** (tables, atomic inlines, container queries, `:has()`): tables and `:has()` cost only
  where used. A fitting atomic inline is two subtree layouts per frame. Container queries cost nothing in a
  steady frame, and a frame in which 500 containers cross a query takes 1.9 to 2.0 times a steady frame
  (`bun run test:perf:cq` in `examples/`).

### The perf pass (#36 to #42)

All numbers are CPU p50 per frame on the test renderer with a release addon, three runs for
poc and perf on the demo pages and one run for everything else. **They are not what a window shows**: the
measuring session had no display link, so GPU time, presentation and the compositor are not in them. The
Electron comparison (idle animated pages, a hover sweep, fast scroll) is rerun by the demo that uses this
fork, not here. "Poc" is the fork before the pass, "off" is the pass with `GPUIX_RETAIN_OFF=1` (the
persistent engine and the cheaper build, nothing skipped), "perf" is everything on.

**Pages of the fluid-functionalism demo** (the demo's own harness, not in this repo; the page, then
its element count; ms per frame, with the number of elements built per frame in parentheses for the last
two columns). *Idle* is nothing happening, *sweep* a pointer moving across the page, *scroll* a wheel.

| | page (elements) | poc | off | perf |
|---|---|---|---|---|
| idle | introduction (742) | 8.1 | 4.0 (359) | **1.2** (26) |
| | showcase (2348) | 15.2 | 8.2 (617) | **3.5** (60) |
| | changelog (2338) | 9.9 | 5.4 (451) | **2.2** (60) |
| | command menu (3478) | 13.8 | 7.4 (654) | **2.3** (42) |
| | fluid hover (2006) | 10.2 | 5.2 (456) | **1.8** (40) |
| sweep | introduction | 8.3 | 4.4 | **1.7** |
| | showcase | 15.5 | 9.0 | **4.2** |
| | changelog | 10.2 | 5.7 | **2.8** |
| | command menu | 14.2 | 8.2 | **2.9** |
| | fluid hover | 10.3 | 5.5 | **2.5** |
| scroll | introduction | 8.1 | 4.0 | **1.2** |
| | showcase | 15.3 (p95 138) | 8.7 (p95 28) | **3.5** (p95 18.9) |
| | changelog | 225 (a) | 8.5 | **5.7** |
| | command menu | 14.1 | 7.4 | **2.3** |
| | fluid hover | 18.5 (p95 93) | 9.6 | **3.4** (p95 13.0) |

(a) Poc drew 20.7 times per frame on that page in the test renderer, which draws a dirty window inline
whenever effects flush; a window with a display link draws once. The comparison is of the pass against
that artefact, and the other scroll rows are the fair ones.

The changelog and fluid hover pages have an animated svg that moves a pixel a frame, and the roots it
moves are rebuilt each frame, legitimately. A scroll frame builds more than an idle one (showcase 258
elements, changelog 319, fluid hover 143); an earlier profile put the p95 spikes on frames where a cull
region flips, which the demo does with `display: none` toggles.

**Benchmarks in `examples/`**:

| | poc | off | perf |
|---|---|---|---|
| #36 nested auto-width columns, depth 12, 8 instances (`depth-col`) | 54.9 ms (6861 µs each) | - | 0.07 ms (6 µs each) |
| #37 page of 2042 elements, idle (`one-animating-node`) | 18.0 ms | 9.2 ms | 1.29 ms |
| #37 the same page, one `opacity` node animating | 18.8 ms | - | 1.51 ms (2 elements built) |
| #38 2000 boxes, visible / clipped / in a scroller (`clipped`; "off" is the shape that measures culling, a11y on) | 10.8 / 10.2 / 10.1 ms | 6.6 / 4.6 / 4.6 ms | - |
| #38 a 16x13 table (639 elements), visible / clipped | 4.7 / 4.0 ms | 4.0 / 2.8 ms | 0.90 / 0.32 ms |
| #37 200 cards in a scroller, 3 px a frame | 10.9 ms a draw (32.7 ms for the 3 draws a flush makes here) | 4.9 to 5.4 ms | 0.86 to 0.98 ms (0.95 a draw in the poc-comparable run) |
| #39 pointer handler that writes then reads, 1 move a frame (`draws-vs-presents`) | 30.7 ms, 3 draws | - | 1.9 ms, 1 draw and 1 pass |
| #39 the same with 3 moves a frame | 65.4 ms, 7 draws | - | 3.6 ms, 1 draw and 3 passes |

**Pages of `retain.perf`** (per frame; hover is the pointer over the page, typing is a text edit):

| page (elements) | idle poc | idle perf | hover poc | hover perf | typing poc | typing perf | skipped idle |
|---|---|---|---|---|---|---|---|
| cards (2042) | 16.5 | 1.31 | 16.8 | 1.98 | - | - | 100% |
| table (593) | 4.33 | 0.58 | 4.80 | 1.34 | - | - | 100% |
| timeline (925) | 7.13 | 1.35 | 8.82 | 2.54 | - | - | 98% |
| mail (344) | 3.05 | 0.69 | 3.89 | 1.04 | 8.75 | 5.12 | 94% |
| chat (1053) | 2.95 | 1.16 | 3.34 | 1.32 | 8.40 | 4.72 | 59% |
| form (68) | 0.75 | 0.39 | 0.78 | 0.42 | 2.59 | 1.90 | 65% |

A chat page is capped by its virtual-list rows and a form by its inputs, which were never retained when
this was measured (Round 14a retains them; the table was not measured again).
`timeline.perf.test.tsx` (a 3259-clip pan): culling on, p95 44.7 ms against 34.1 ms here (the budget is
18 ms) and culling off 708 against 683 ms (400 ms), so it fails its budgets on poc as well as on the
pass; a drag of a clip, p50, 13.9 against 4.7 ms.

In the last measurement of `clipped.perf` with retention on, 2000 plain boxes in an unchanged scroller
cost 1.25 to 1.31 times what they cost with retention off, because every box was a root. The `MIN_ROOT`
rule that follows fixes it (1.00 for plain, rounded and shadowed boxes, 0.4 for texts, measured when the
rule was made); the table above does not have a measurement taken after it.

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
11. The perf pass (#36 to #42), on `perf` until it lands on `poc`: the perf benchmarks and their
    harness (`examples/*.perf.test.tsx`, `perf-harness.ts`, `perf-fixtures.tsx`), the work counters
    (`perf_counters.rs`), the borrowed-slot element build (`slot.rs`), the culling reach (`layout/cull.rs`),
    keyed layout requests and measure keys in the text, image and list elements, the retain planner
    (`retain.rs` and `retain/debug.rs`, `tape.rs`, the `Taped` and `retained_root` wiring in `renderer.rs`,
    the `CustomElement::retain_state` hook, roots moved by a scroller, holes and fragments), observers and
    `onLayout` read from the published boxes (`observers.rs`, `js/observers.ts`, `automation.rs`), fractional
    text units and the keyed insets of spans, the layout-only read (`draw_if_dirty`) and the test
    renderer's switches and getters, then the tests (`retained-replay`, `retained-fuzz`,
    `retained-round6`, `retained-scroll`, `retained-lifts`, `retained-observers`, `frame-event-order`,
    `text-idle-measures`, `clip-cull`, `forced-layout`, `offset-coordinates`, `atomic-inline-layout-read`
    and the rest of `retained-*`). Their README and `AGENTS.md` text is in the same commits, and the `FORK.md` text is
    `fixup!` commits of the fork commit.
    Round 9 follows, one commit per behavior: JS commits, motion writes and scrolls that notify and do
    not refresh (`invalidate_after_commit`, `live-invalidation`), a bounds read after a paint-only write
    (`read-after-paint-change`), an inherited animated value through `display: contents` (`splice_overlay`,
    `contents_styles`), the measure closure of the input taking the `MeasureRun`, the FxHash bounds map,
    fractional bounds (`capture_layout_box`, `fractional-bounds`) and `getFrameWorkCounters` with
    `hover-frame-work`.
    Round 12 follows: the baseline of a trimmed label in an atomic inline (`atomic-trim-baseline`),
    `TestRenderer.setWindowScaleFactor`, authored lengths in 1/64 device px (`fractional-lengths`, the
    collapsed border snap in `layout/collapse.rs`, reads in device layout units in `automation.rs`), and
    Motion's playback controls on a motion value's animation (`Playback` in `motion/values.rs`,
    `motion-controls-native`, `motion-controls`).
    The perf half of Round 12 (#62, #63), on `round12-perf`, one commit per behavior: every running animation
    says whether it only paints (`paints_only` of `css_animation.rs`, `css_transition.rs`, `MotionState` and
    the motion values) and an animation's clock advances without a style (`advance_clock`); dormant animations
    on culled content (`ElementBounds::culled`, `Dormancy`, `dormant_tick`, `offscreen-animation-work`);
    units, the preview and the capturing tape (`Retain::begin_unit`, `end_unit`, `preview`, `build_unit`);
    the patcher (`retain/patch.rs`, `request_tick`, `GPUIX_PATCH_OFF`, the test renderer's `getPatchCounters`,
    `setPatchEnabled` and `clockAdvance`, `animation-frame-work`); `<svg>` units (`shapes_running`,
    `SvgPaint::paints_in_place`); a tape that a replay publishes and the registries read when asked
    (`tape::publish`, `tape::Layered`, `tape::delta_of`); dormancy facts kept per element and clocks that are
    ticked when something is due (`PaintStatics`, `Run::quiet_until`); lifted units (`unit_root` with
    `lift_z`); `getPatchCounters` on the live renderer; and the persistent dormant set with the due queue of
    the clocks (`Retain::dormant`, `CssAnimations::advance_dormant`).
    Round 13b follows, one commit per behavior: the colour of a collapsed border is the one its owner is painted
    with in the frame, settled after the parts are built (#68, `collapse::Owners`, `settle_collapsed_borders`,
    `collapsed-border-transition`); a paint-only animation on a part of a collapsed table is a patched tick with
    the table as its unit (`Retain::collapsed_table`, `unit_facts`); `getLayoutCounters().shapeCacheHits`
    (#71, `TextCounters::shape_cache_hits`, `shape-cache`); the `contentVisibility` and `containIntrinsicSize`
    keys with their containment (`css/contain_intrinsic.rs`, `content_visibility::contained`); `hidden` that
    skips the content (`content_visibility::skips`, `contain_skipped`, `build_host_container`,
    `Retain::skip_content`, the Chrome oracle of `content-visibility`); `overflow: clip` on both axes laid
    out as a clip (`overflow-clip-flex`); `onContentVisibilityAutoStateChange` (`js/events/event-props.ts`);
    `auto` that follows what is near (`Relevance`, `content_visibility::watch`, `observers::frame_sweep`,
    `content-visibility-auto`) and invalidates as a commit does; a geometry read inside a skipped subtree that
    lays it out for real (`renderer::read_after_layout`, the test renderer's `getBuiltElementBounds`); and
    the per-element build that no longer moves a `StyleDesc` by value (`Animated` and `FocusFolded` hold boxes
    or borrows).
12. Window placement (#73): `WindowOptions.x` / `.y`, `to_gpui_window_options` choosing the display and the
    display-local origin, `screen.rs` and its JS entry (`js/screen.ts`, the `./screen` export and the adapter
    re-exports), the test renderer ignoring `x` / `y`, and the README text.
13. Round 14b (#77, #78): `setFrameWakeCallback` through zed's `set_run_loop_wake_hook`, `tick()` calling
    `run_loop_pumped()`, `startFrameLoop` without the fallback timer, `inactive_frame_interval: None` in
    `to_gpui_window_options`, the frame-loop tests and `window_options_tests`, and the README and `AGENTS.md`
    text.
14. Round 15a (fractional layout, baseline and scroll leftovers), one commit per behavior: percentages cut
    toward zero to 1/64 device px and a flex item's share rounded to it as Blink does (the rows added to
    `fractional-lengths-oracle`); a collapsed table's client size as its border box (`ElementBounds.collapsed_table`);
    an atomic inline that keeps the fraction of its place in every box it reports and is painted half up, and a
    span in a flex or grid item at a fractional top measured once (`atomic-trim-baseline`, the badge oracle of
    #80 exact at scale 1); a `<virtual-list>` write that is scroll-only, a list `scrollTop` that reads its offset
    and the maximum offset of a list that is a `div`'s; a scroll value that is not finite read as 0
    (`finiteOffset` in `js/scroll.ts`, `scroll-order-oracle`); `onLayout` and resize reports delivered only after
    a painted frame, after its scroll events (`frame-event-order`, `forced-layout`); a font's line metrics and a
    line's half-leading rounded in the device pixels of the window's scale (`fixtures/dsf-oracle.sh`, which
    regenerates the `-x2` oracles of `line-box`, `inline-strut` and `text-baseline`, and the tests that now run
    at both scales); and text measured in 1/64 device px with a number line height cut and a length or
    percentage rounded (`text-measure`, `fixtures/text-measure-oracle*.json`). Their README and `AGENTS.md`
    text is in the same commits, and the `FORK.md` text is `fixup!` commits of the fork commit.
15. Round 16 (#82, #83 and the content-visibility leftovers), one commit per behavior: layout containment of
    `contentVisibility` (`contain_layout` in `layout::style::apply`, `containing_block::contains_fixed`) and its
    Chrome cases; `AnimProp::paints_only` writes that notify paint-only (`RetainedTree::take_dirty`,
    `StyleDesc::differs_only_in_paint`), scroll writes that move nothing and `nodesLaidOut` / `nodesSized`
    (`forced-layout-incremental`); scroll anchoring (`overflowAnchor`, `css/overflow_anchor.rs`,
    `scroll-anchoring`); link resolution from `RelationRegistry` (`accessibility/relations.rs`, `relationVisits`);
    layout records (`tape::laid_out`, `layoutRecords`); reads that share a forced pass (`Relevance::ForcedRead`);
    skipped `auto` content in the accessibility tree (`accessibility/skipped.rs`), in the tab order
    (`focus/skipped.rs`) and in find (`text/search.rs`); the first frame's `contentvisibilityautostatechange`; and
    the follow-ups (a scroller in forced content, a paint-only write that keeps the share, an `<svg>` in a
    layout-only pass). Their README and `AGENTS.md` text is in the same commits, and the `FORK.md` text is
    `fixup!` commits of the fork commit.
16. Round 14a (patch fallbacks and the retention leftovers), one commit per behavior, on `round14a` until it
    lands on `poc`: an element with an animation or transition of `opacity` current is lifted for the whole
    run (`StyleDesc::animating_opacity`, `Record::lift_run` (`opacity_run` until Round 19), `Kept::LiftRun`, #81); a loop under a
    positioned, translucent or lifted ancestor, and on an absolute element, is a patched tick, and a tick drawn
    whole is counted under the reason its element is no unit (`Record::refusal`, `Unpatchable::Refused`, the
    string keys of `getPatchCounters().unpatchable`, #75); an `<input>` and a `<textarea>` retained, their
    keyed measure, the caret as `Dynamic::Blink` with `clock_due` and `keep_blinking`, and `<code>`,
    `<markdown>` and `<diff>` out of `varies_with_time` (#74); a binding touched only when due
    (`MotionValueStore::binding_due`), per-element `Demand` with `advance_dormant`, and a paint-only write as a
    tick (`Repaint::invalidate`, #76); an `<svg>` culled by its box, dormant with its shapes and rasterised in
    the paint while they run (`Subtree::of_element`, `SvgPaint::note_culled`); a root recorded by the frame
    that builds it (`Retain::exit` and `settle`, `Recording`, `recorded_root`); the inherited `color` read
    set (`color::reads`, `Record::reads_colors`, `Key::colors`, `ignoring_text_color`); the container of an
    anonymous collapsed table as its unit (`build_plan`); an `<svg>` that waits for its raster, and motion under
    `display: none`, no longer make a tick whole (`SvgPaint::unsettled`, `Unsettled`, the waiting words of
    `retain_state`, `Retain::preview` skipping an element with no box, `getRetainDebug().exclusionWhy` and
    `outside`); a patched tick that steps the motions that move (`GpuixView::sync_motions` and `step_motions`,
    `motionsSynced`), makes its scene from the draw orders it had (`boundsInserts`) and samples the motion values
    that move (`MotionValueStore::step`, `BindingIndex`, `step_values`, `motionValuesSampled`); the
    `animation-frame-work`, `offscreen-animation-work`,
    `retained-first-build`, `retained-inherited-color`, `retained-opacity-states` and
    `collapsed-border-transition` tests, `retained-scroll` counting relocations in pairs under the verifier,
    and the README and `AGENTS.md` text. Their `FORK.md` text is a `fixup!` commit of the fork commit.
17. Round 14c (#93): `GpuixRenderer.postNativeEvent` (`native_event.rs`, test-support), the frame-loop test of
    input waking an idle window in `events.test.tsx`, and the README and `AGENTS.md` text.
18. Round 19 (#92 CSS transforms), one commit per slice on `round19` until it lands on `poc`: the style keys, their parse and
    apply, the stacking context and the containing block (`css/transform.rs`, `layout/stacking.rs`,
    `layout/containing_block.rs`) and their painted-pixel tests; `ElementBounds::to_window`, `getBoundingClientRect`,
    `offsetX` / `offsetY`, `getElementsAt`, `scrollIntoView` and `IntersectionObserver` through transforms with the Chrome
    geometry oracle; Motion's transform keys as `AnimProp`s (`motionScale` / `motionRotate`, `animating_transform`,
    `OpacityRun` renamed `LiftRun`, tapes compared apart from transforms, `publish_captured_bounds`); CSS transitions and
    animations of the transform properties (`motion/transform_value.rs`) with the Chrome animation oracle; and the patch
    counter tests (`transform-css-patch`, `motion-transform` in React and Solid). Their README and `AGENTS.md` text is in
    one commit, and the `FORK.md` text is a `fixup!` commit of the fork commit.
19. The fork commit: `.gitmodules`, the submodule bump to zed `poc`, `FORK.md`, `scripts/sync-upstream.sh`,
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
     `resolve_calc_value`, a flex item's available cross space takes the item's own margins and not the
     container's, a block container's percentage padding and border resolve against its
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
     `position: relative` on table parts, `direction`, and collapsed borders (`collapsed_borders.rs`).
   - Text baselines: taffy `MeasuredLeaf` and `request_measured_layout`, `TextLayout`'s first baseline,
     the two grid baseline fixes.
7. The perf pass (zed `perf` until it lands on `poc`):
   - Vendored taffy (`third_party/taffy`): the fully associative measure cache (upstream #1010, the #36
     fix) with the fork's cache key (sizing mode, margin-collapsing flags and a `modes` byte, the axis
     out of the sign bits) and size entries that carry the margins collapsing through the node,
     `set_style` that is a no-op on an equal style, `remove` and `clear` that drop the node context,
     `set_children` tolerant of stale parents and adoption that unlinks, `mark_dirty` that walks to the
     root, the node `dirty_generation` and `layout_generation`, `mark_reached` and `unreached_nodes`,
     `RunMode` passed to measure functions, and the block container that adds a child's offset to the
     height its floats take only when the child holds floats (the scroll container height).
   - gpui layout engine (`taffy.rs`, `taffy/calc.rs`, `engine_fuzz_tests.rs`): the persistent engine
     with `NodeMeta`, keyed requests (`request_keyed_layout`, `request_keyed_measured_layout`),
     interned calcs, `end_pass` / `end_frame` collection, `LayoutCounters`, `escapes_subtree` (an
     absolute box whose containing block is inside the subtree is stable). Measured-leaf keys in
     `elements/text.rs`, `img.rs`, `list.rs` and `uniform_list.rs`, the atomic inlines' carried engine in
     `elements/atomic_inline.rs`, and text in 1/64 px (`elements/text.rs`, `util.rs`,
     `Window::layout_unsnapped_bounds`).
   - `elements/retained.rs` and `window/retained.rs`: `retained(id, key, build)`, `ReplayKey`,
     `NonReusable`, `NonRelocatable`, `RelocationMiss`, the recorded prepaint and paint (`window.rs`,
     `window/a11y.rs`, `scene.rs`), the verifier (per root and whole frame) and the layout-only phase;
     `relocatable`, `Scene::replay_translated`, `note_positioned_capture`, `note_relocated`,
     `Canvas::keeps_no_position` and the translation-covariant rounding in `window.rs` and `util.rs`;
     holes and fragments (`lift_z`, `LiftRecord`, `PlainPaint`, the `RetainedPhase::Lifted*` phases, the
     lift owners and `begin_lift` / `finish_lift` / `paint_lifted` in `window.rs`); the tests in
     `elements/retained/` (`hole_tests`, `fragment_tests`, `relocation_tests`, `regression_tests`).
   - Culling: `Element::cull_outset`, `prepaint_culled` / `paint_culled` (`element.rs`, `elements/div.rs`),
     `Window::lies_outside_the_mask`, `TaffyLayoutEngine::painted_extent`.
   - Layout-only reads: `Window::draw_if_dirty` as `layout_pass`, `publish_layout`, `paint_if_dirty`,
     `Window::replayed_root`,
     and publications and deferred draws carry the engine they were queued under (`SharedLayoutEngine`,
     `Window::with_layout_engine`).
   - Round 9, on `round9` until it lands on `poc`: `App::notify_paint_only` (#47); the baseline of a
     cap-trimmed label from the unsnapped origin (#44); `MeasureRun::Place` and `Probe`, so a size probe
     does not replace a text's placed layout (#48); `opacity: 0` painting with the scene hidden; element
     states kept under the id they were first stored under and `collect_garbage` skipped when nothing was
     freed; a text measure key that holds the layout part of its runs, `redecorate_lines`, and a line
     layout cache that idle frames do not age (#58); the painted extent and the scrollable overflow,
     the unsnapped one included, kept across passes (`Window::layout_unsnapped_scrollable_overflow`);
     `Window::frame_work_counters`.
   - Round 12, on `round12-parity` until it lands on `poc`: the baselines of a text-box-trimmed text
     counted from its trimmed box (#66); authored lengths cut to 1/64 device px in `to_taffy` and calcs
     cut where taffy resolves them, border widths floored to device pixels, and box edges snapped half up
     with a nonzero box at least a device pixel (#65, `taffy/fractional_lengths_tests.rs`).
   - Round 12 perf (#62, #63), on `round12-perf` until it lands on `poc`: `window/patch.rs`
     (`retained(..).patchable()`, `Window::request_tick`, `set_frame_patcher`, `repaint_in_place`,
     `PatchDecline`, `patch_counters`, `patch_tests.rs`), `Scene::rebuild_from_operations`, a lifted root as a
     patch unit (`Placement::Lifted`), recordings that name nested roots by the id their state is stored
     under, element states in one window-level map owned by the retained roots (`Window::element_states`,
     `StateOwner`), recordings placed by an `Origin` so a replay does not visit the roots inside,
     `DispatchTree::reuse_subtree` in one pass and dispatch nodes with `NodeExtras` behind an `Rc`.
   - Round 13b (#71, `content-visibility`), on `round13b` until it lands on `poc`, in order: a per-window
     shape store behind the line layout cache (`text_system/line_layout/shape_store.rs`,
     `TextCounters::shape_cache_hits`); `contain-intrinsic-size` for a box with size containment (taffy
     `Style::contain_intrinsic_size`, gpui `contain_intrinsic_width` / `contain_intrinsic_height`); and a face
     made at a size once in `gpui_macos` (the sized `CTFont` cache in `text_system.rs`).
8. Window placement (#73): `MacDisplay` exported from `gpui_macos`, with `MacScreen`, `MacDisplay::screen()`,
   `rotation()` and `is_builtin()`.
9. Round 14b (zed `round14b` until it lands on `poc`), in order: a draw outside a frame request calls
   `schedule_frame` (`Window::draw`); `gpui_macos::run_loop_wake` (`set_run_loop_wake_hook`,
   `run_loop_pumped`; `set_frame_wake_hook` is gone); the display link on frame demand (`schedule_frame` and
   `frame_waker` in `MacWindow`, `IDLE_STEPS_BEFORE_STOP`, `WindowFrameSource::is_running`); the profiler
   journal ring allocated by the first collector (#79); a registered font held once as a no-copy `CFData`
   (#79).
10. Round 15a (zed `round15a` until it lands on `poc`), in order: the taffy percentage cut
    (`resolve_percentage` in `util/resolve.rs`, at every site that resolves one) and the flex share rounded
    to 1/64 device px (`flexbox.rs`: items walked from the last, the running fraction); the atomic inline that
    keeps the fraction of its place (`Window::with_unsnapped_element_origin` and `origin_phase_stack`, which a
    patch unit carries); the trimmed baseline in the layout unit of its box; a list that scrolls as far as a
    `div` and says where it is (`max_scroll_offset` in `div.rs`, `ListState::dom_scroll`); the window's scale on
    `WindowTextSystem`, `PlatformTextSystem::layout_line` and `LineLayout`, with `FontMetrics::line_box(size,
    scale)` and `extent_in_line(.., scale)` rounding in device pixels; and text measured in 1/64 device px
    (`ceil_to_layout_unit` and `round_to_layout_unit` in `util.rs`, `LineHeight::to_laid_out_pixels`,
    `measured_size_in_device_pixels` in `taffy.rs`).
11. Round 16 (zed `round16` until it lands on `poc`), in order:
    - Vendored taffy: `Style::contain_layout` (a block with it is a formatting context of its own),
      `LayoutInput::height_from_containment` and `AlgoConstants::percentage_base` (a percentage height under a
      size-contained auto height is auto, in a block and in a flex container); `TaffyTree::layout_runs` and
      `size_runs` (the cache misses by run mode); relayout boundaries (`is_relayout_boundary`, pending boundaries
      that `compute_layout` lays out with the `SavedLayout` of their last placement, `boundary_relayouts`,
      `boundary_fallbacks`, `set_relayout_boundaries`, `relayout_boundary_tests`).
    - gpui layout engine: `LayoutCounters` with `nodes_laid_out`, `nodes_sized`, `boundaries_laid_out` and
      `boundary_fallbacks`; `NodeMeta::contains_layout` (a fixed node resolves to its nearest layout-contained
      ancestor, `Window::with_frame(contains_layout)`); the engine fuzz with boundaries.
    - `elements/scroll_anchoring.rs` (`Style::overflow_anchor`, the anchor picked in
      `ScrollAnchoring::before_layout` from `compute_layout_in`, the per-node flags set in `reuse_node`, state on
      `ScrollHandleState::anchoring`).
    - `Window::layout_stamp` (`LayoutStamp`: the invalidator's count of non-paint-only invalidations and the layout engine's `box_generation`).
    - Layout records: `LayoutRecord` on `RetainedState`, `RetainedLayout::SkippedLaidOut`, `SkipBasis::LaidOut`,
      `RetainedPhase::LayoutRecorded`, `Tape::laid_out`, `layout_painted_skips`, `refresh_epoch`, `reference_passes`
      (`elements/retained/layout_record_tests.rs`); a lifted box does not make a subtree unrecordable in a
      layout-only pass.
    - Synthetic accessibility nodes that nest and take relations (`A11ySubtreeBuilder::push_nested`,
      `push_relation`); `Window::next_tab_stop` and `prev_tab_stop`, which read where Tab would go.
    - `TextLayout::place` keeps the bounds, phase and `phase_y` of the layout it replaces.
12. Round 14a (zed `round14a` until it lands on `poc`), in order: `Style::has_active_opacity_animation`, which
    makes an element with a current opacity animation paint a group at 1 and at 0 (#81); a patch unit inside an
    open lift, `Placement::Deferred` (#75); a unit that holds the focused element is patched, and
    `TextSystem::font_generation` is public (#74); an absolute root recorded as a source for a patch and never
    skipped (`Recorded::root_escapes`, `TaffyLayoutEngine::escapes_subtree` not counting the root's own
    escape), and `Window::root_escape` with `Effects::without_root_escapes` so that its escapes do not count
    against its own recording (#75); `Recorded::is_loose`, a subtree kept within an overscan no region was
    made for compared as loose by the verifier; `Retained::ignoring_text_color` (a recording under a text
    colour of its own); and `Window::is_recording_any_relocation`, so a root nested in the reference build of a
    verified frame plans no relocation of its own; a patched frame that makes its scene again from the draw
    orders it had, while the bounds it inserts are the same (`Scene::order_log`, `insert_bounds`,
    `FrameWorkCounters::bounds_inserts`); and `PaintOperation::differs_from` formatting one side of a comparison.
13. Round 14c (a zed branch until it lands on `poc`): the NSEventThread observer in `gpui_macos::run_loop_wake`
    and `post_event_on_event_thread`, with `tests/run_loop_wake.rs` posting the events.
14. Round 19 (zed `round19` until it lands on `poc`), in order: `transform.rs` (the matrix helpers on `TransformationMatrix`,
    `TransformLength`, `TransformFunction`, `ElementTransform::resolve`), `Window::with_transform`, the clip model (`local_clip`
    beside the window `content_mask`) and the quad and shadow shaders; glyphs, images, underlines, paths, surfaces and effect
    groups through the ambient transform (`gpui_apple` `shaders.metal`, `metal_renderer.rs`); `Style::transform` drawn by
    `Div`, `List` and `Interactivity`, with culling and scrollable overflow following it (`taffy.rs`
    `transformed_overflow`); hit testing, listeners, patch and retained replay (`Hitbox::to_local`, `hit_chain_entries`,
    `MouseEvent::with_position_mapped`, `Surroundings` and `ReplayKey` carrying the transform, `patch_tests.rs`);
    `Style::contain_positioned`; `Window::layout_own_transform` and transforms resolved from the unsnapped box;
    `SkewX` / `SkewY` and the orthogonalized decomposition; and operations that do not depend on the content mask.

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
  upstream's `setFrameWakeCallback` return value. Round 14b changed the pump's other end: the hook is
  zed's `set_run_loop_wake_hook` (upstream's `set_frame_wake_hook` is gone from the fork), `tick()` calls
  `run_loop_pumped()` after it clears `pending`, and `startFrameLoop` has no fallback timer on the wake
  path. Re-apply those three on top of upstream's side, or the idle window wakes at the display rate again.
  Without zed's NSEventThread observer (Round 14c) it also never wakes on input.
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
    `css/length.rs` (`LengthContext` carries the container boxes); `css/custom_properties.rs`; `events/state_styles.rs` (`overlay_fields!`
    lists `container_type`, `container_name`, `container` and the `@container` blocks too, and layers `custom_properties`, and the `has*`
    fold order lives in `resolve_pointer_states`); `events/listeners.rs` and `dispatcher.rs` (restyle gating
    counts `has*`). Anything a build publishes must be idempotent, since a pass can be dropped.
  - gpuix selection order: `text/selection.rs`, `text/search.rs`, `text/paint.rs` (`DocPos`,
    `SelectableText::path`).
  - zed: `taffy` vendored in `third_party/taffy` is the biggest conflict surface (`compute/table/`,
    `compute/leaf.rs`, `style/mod.rs`, `tree/traits.rs`, `tree/taffy_tree.rs`, the grid alignment files); an
    upstream taffy bump means re-applying the fork's patches there. In gpui: `window.rs` (the draw pass
    loop), `elements/div.rs` (prepainted-layout listeners, the table painting hook), `elements/text.rs`,
    `text_system/` and the platform shapers, `style.rs`, `taffy.rs`, `view.rs`.
- **The perf pass.** Every file here is the fork's, so a conflict is upstream editing the code around
  it, and the resolution keeps the invariant named, not one side.
  - zed `third_party/taffy`: `tree/cache.rs` (measure entries stay fully associative, and the key keeps
    the sizing mode, the margin flags and the `modes` byte; if upstream lands #1010, keep the fork's key),
    `tree/taffy_tree.rs` (a style equal to the node's changes nothing, `mark_dirty` visits every ancestor,
    `set_children` tolerates stale parents, `remove` and `clear` drop the context, the dirty and layout
    generations, `mark_reached`), `tree/layout.rs` (`differs_inside`), `tree/traits.rs` (the measure
    function takes the `RunMode`), `compute/block.rs` (the float height rule). `engine_fuzz_tests.rs`
    must pass on seeds 1 to 10000 after any of these (`GPUI_ENGINE_FUZZ_SEEDS`).
  - zed `taffy.rs`: the engine must stay persistent. Requests that have a scope take a node by key and
    compare before they set; `NodeMeta::requested` is the style the element asked for, which every
    rewriter (`intrinsic.rs`, `natural.rs`, `containing_block.rs`, forced and stretched widths) must read
    and restore; `end_frame` is the only collector of a presented draw. A new request path must either
    be keyed or count as unstable. `escapes_subtree` decides whether an absolute box makes a retained
    subtree unstable.
  - zed `elements/retained.rs`, `window/retained.rs`: the conditions to skip, the `ReplayKey` and the
    `NonReusable` reasons. A new `Window` field that a prepaint or paint reads belongs in the key; a new
    thing that a replay cannot repeat belongs in `NonReusable`, and one that a moved replay cannot repeat
    in `NonRelocatable`.
  - zed relocation (`window/retained.rs` `Relocation` and `ReplayKey::relocation`, `scene.rs`
    `replay_translated` and `translate`, `window.rs` `snap_bounds`, `layout_bounds`, the underline,
    glyph, emoji and svg quantisation, `util.rs` `round_to_device_pixel_down`): the rounding is
    translation-covariant, and an upstream change to how painted geometry is snapped has to keep a shift
    by whole device pixels moving the result by the same shift. `relocation_tests.rs` and the fuzz
    (`RETAINED_FUZZ_OFF`) are the check.
  - zed holes and fragments (`window/retained.rs` `LiftRecord`, `PlainPaint`, `lifted_holes_since`,
    `replay_lifts`; `window.rs` `begin_lift`, `finish_lift`, `begin_lift_paint`, `paint_lifted`): a change
    to the stacking context's lift list must keep ordinals reserved for a replay, the owners of an entry and
    the `Lifted*` phases to the hooks. `hole_tests.rs` and `fragment_tests.rs`.
  - zed `window.rs`: `build_next_frame` passes (`draw_pass`, `refreshing`, `layout_pass`, `measuring`),
    `draw_if_dirty` / `paint_if_dirty`, `publish_layout`, the recording and replay of the frame's
    index ranges, and `lies_outside_the_mask`. A layout-only pass must not swap frames or record a frame (it leaves layout records only).
  - zed `elements/div.rs`, `element.rs`: `prepaint_unlifted_child` (culling) and the `*_culled` methods;
    the culled paths must keep publishing bounds, scroll metrics, tab stops and a11y nodes. Anything that
    keeps a position past the frame calls `note_positioned_capture`.
  - zed `elements/text.rs`, `text_system/`: text is laid out in 1/64 px and snapped when painted;
    `chrome_layout_units` has the Chrome numbers.
  - gpuix `retain.rs`: `inherited_fingerprint` lists every `Inherited` field (a size test fails
    otherwise); `compute_deltas` lists every source of volatile state, and `preview` declines a patch for
    any source it does not name as time-driven; `Exclusion` is the list of what a
    replay cannot repeat; `Scroller` and `Relocation` decide what a scroller moves; `worth` is the
    `MIN_ROOT` rule. If upstream adds a build input, it goes in one of those or in the tape.
  - gpuix `tape.rs` and its call sites (`tape::write` in `automation.rs`, `text/paint.rs`,
    `text/inline_hit.rs` and `motion/measure.rs`; `tape::note_build` in `motion/measure.rs` and
    `background_images.rs`): a registry write in a paint listener goes through `tape::write`, and an
    effect of building goes through `note_build`, or it is lost when a frame skips the subtree. The
    phases (`Prepaint`, `Paint`, `Layout`, `Lifted(rel)`) follow gpui's `RetainedPhase`; the hook map is
    in `renderer.rs`' `retained_root`, and an entry is the write and its offset. A replay does not write the
    registries: it publishes the lists it puts back (`tape::publish`) and a registry reads them when asked
    (`tape::Layered` for those that keep an order, `tape::owner` and `delta_of` for the painted boxes), so a
    new registry has to be read that way or be written on replay at a cost per element.
  - zed `window/patch.rs` and `Window::request_tick`: a patch must decline for anything a recording cannot
    repeat, and change nothing it declined (the element states go back, the layout pass ends). A new
    `Window` field that a prepaint or paint reads belongs in the `PatchUnit` surroundings as well as in the
    `ReplayKey`. `Window::element_states` (`StateOwner`, the pass stamps) and `Origin` (`range_now`) are
    what makes a replay a step per root: a path that copies states or visits nested roots per element undoes
    it. `patch_tests.rs` and the retained fuzz under the verifier are the check.
  - gpuix `retain/patch.rs`, and `begin_unit`, `end_unit` and `preview` in `retain.rs`: a unit builds alone
    only from what its kept `FrameEnv` holds, and a write that bypasses the tape (`report_scroll`) makes an
    element with a scroller inside no unit. A new source of volatile state is a reason in `Unpatchable`, and
    `Dormancy::holds` must read nothing a build of the element does not tell or `Sources::verdicts` does not
    move.
  - gpuix `observers.rs`, `automation.rs`, `retained_tree.rs` (`geometry_ids`), `js/observers.ts`:
    `onLayout` and the observers are read from the published boxes in `frame_sweep`, in the order scroll,
    `onLayout` and `ResizeObserver`, then `IntersectionObserver`. A paint listener that reports geometry
    for them would exclude the element from retention again.
  - gpuix `renderer.rs`: the `retain.*` notes between `enter` and `exit` in `build_element_unpumped`
    record what a build read, and `retained_root` / `build_subtree` / `FrameEnv` must hold everything a
    late build needs of the build around it. `set_retention_whole_frame(false)` stays. Keep `slot.rs`
    borrows: they are the 1.6 KB moves the build avoids.
  - gpuix `layout/cull.rs`: new elements that draw outside their box or are placed by something other
    than their parent call `block()`.
  - gpuix `text/inline_insets.rs`, `text/atomic.rs`: the measure key of a text holds the insets' key
    (`InsetSpec::text_key`) and the atomic inlines' digest.
  - The test renderer (`test_renderer.rs`, `js/testing.ts`) keeps the switches in [Switches and
    counters](#switches-and-counters), and `packages/native/index.d.ts` is regenerated, not merged.
- **Round 16 (layout records, boundaries, anchoring, relations).** Every file is the fork's; the invariants:
  - zed layout records (`elements/retained.rs` `LayoutRecord`, `window/retained.rs`, `window.rs`): a layout record is
    skipped with only under the conditions of a painted recording (same token, no notified entity, node alive, same
    engine) plus the stamps that say the box its tape holds is still the last one written: `rendered_serial` (a draw
    ends every record), `refresh_epoch` (`Window::refresh` ends the ones before it) and `reference_passes` (a verify pass
    builds inline). A pass that is abandoned, or whose prepaint a `List` rolls back, must never run the last
    publication, or its records would be skipped with. A layout tape never holds a painted tape
    (`layout_painted_skips`: `tape::PARENTS` would be corrupted), and the painted `Recorded` is never touched by it.
    gpuix `tape.rs` `Tape::laid_out` / `TapeStore::laid_out` and `tape::LAID_OUT` in the serial keep the two sets of
    boxes apart in `automation::Stored`. `layout_record_tests.rs` and `forced-layout-incremental.test.tsx`, plain and
    under the verifier, are the check.
  - zed taffy relayout boundaries (`tree/taffy_tree.rs` `mark_dirty`, `is_relayout_boundary`, `SavedLayout` in
    `tree/taffy_tree.rs`, `compute_layout`): the boundary list is Blink's and `compute_layout` lays pending boundaries out
    with the input they were last placed with, so any rewrite of `PerformLayout` has to keep storing it (and drop it on a
    hidden layout). The generations above a boundary still move; if an upstream change makes `mark_dirty` stop
    without bumping them, retained subtrees and request signatures go stale. A boundary whose answer changed must fall
    back to the chain to the root. `relayout_boundary_tests.rs` (a differential fuzz) and the gpui engine fuzz (seeds 1
    to 10000, which draws boundaries) are the check; floats are out of the fuzz.
  - zed `elements/scroll_anchoring.rs` and its state on `ScrollHandleState`: the anchor is picked in
    `before_layout`, called by `compute_layout_in` before the layout is computed and never at the end of a prepaint;
    `step` only compensates, after the offset is clamped; an offset other than the one the step left is an explicit
    scroll and drops the anchor; the suppression flags set in `reuse_node` are cleared at `end_pass`. Gpuix's
    `scroll_reporter` reports nothing in a layout-only pass. `scroll-anchoring.test.tsx` (the Chrome scenes) is the check.
  - gpuix `RelationRegistry` (`accessibility/relations.rs`, on `RetainedTree`): `links::compute` reads only what it lists.
    A prop, id, role or text mutation that `compute` starts to read has to reach `reads`, `prop_changed`,
    `text_changed` or `restructured`, or the links stay stale, and `compute` never walks `tree.elements`
    (`relationVisits`).
  - gpuix `StyleDesc::differs_only_in_paint` (`style.rs`) and `AnimProp::paints_only`: the paint-only key list. A style
    field that is not copied over the clone is a layout key, which is the safe side; a field added to the list must
    not move or size a box, change a stacking context or a clip, or start a motion, or a bounds read after the write
    answers from a stale layout. `batch_tests` in `renderer.rs` pins the list.
  - gpuix `Relevance` (`layout/content_visibility.rs`): `ForcedRead` holds the `gpui::LayoutStamp` read after the
    forced pass's trailing invalidation; a new invalidation that can move a box has to go through gpui's invalidator
    (not `notify_paint_only`), or a read shares a pass that is stale, and so does a box that a frame moves without a
    taffy box (`TaffyTree::box_generation` is bumped in `set_unrounded_layout`; a layout written anywhere else would
    not count). The skipped-content nodes (`accessibility/skipped.rs`)
    come from `apply_accessibility` on a throwaway `div`; do not write a role or a name there that the built path
    does not also write. `focus/skipped.rs` and `text/search.rs` (`announce_skipped`, `settle_frame`) follow the same
    rule: the retained tree, not the built one, is the source.
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
with `bun run test:perf` (chat, timeline and rerender) or one at a time, through the scripts that set
`NODE_ENV=production`. Use a release addon: a leftover debug build times like a regression.

The perf pass has a guard per issue, each failing on the bug it was written for and not on a clock:

```bash
cd examples
bun run test:perf:depth      # #36: depth 12 costs at most 3x the linear extrapolation of depth 1
bun run test:perf:animating  # #37: one animating node over 2042 elements costs under 0.4x of building
                             #   the page, and builds a fraction of its elements; clone and node counters
bun run test:perf:clipped    # #38: hidden boxes are skipped and cheaper; retention on costs at most
                             #   1.05x off for plain boxes in a scroller, rows that a scroll moves
                             #   cost at most half, a scroller of texts at most 0.6x
bun run test:perf:draws      # #39: at most 1.05 draws per frame; one layout pass per invalidation
bun run test:perf:retain     # #37: judged and skipping frames per page (cards, form, table, chat,
                             #   mail, timeline); prints, no budget
```

`test:perf:menus`, `:cq`, `:svg` and `:overflow` are older profiles with no budgets or loose ones.
`timeline.perf.test.tsx` fails its pan budgets on `poc` as well (p95 44.7 ms against 18 ms with culling
on, 708 against 400 with it off; 34.1 and 683 with the pass), so its failure is not a regression.

**Run the suites under `GPUIX_RETAIN_VERIFY=1`** after any change to what a build reads or what a paint
listener writes (see [Switches and counters](#switches-and-counters)): `packages/react`,
`packages/solid` and `examples`, with `GPUIX_RETAIN_VERIFY_REPORT=1` to see how many frames were
compared. A `[retained]` line on stderr or a failing `dispose()` is a hole in the key or the tape. The
read-after-write test of `retained-replay.test.tsx` takes about 1.5 s alone under it and has a timeout of
15 s. The gpui side has its own: `cargo test -p gpui --lib --features test-support` runs the engine fuzz (a
kept engine against a fresh one), the fuzz of random scrolled pages with retained, relocatable, lifted and
rounded wrappers against the same pages built, and the unit tests of holes, fragments and relocation
(see [Switches and counters](#switches-and-counters) for the long runs), and `retained-fuzz.test.tsx`
compares retention with a build of the same state under random interaction: cards with a motion (a div,
an svg group, an inline span) below `display: contents` wrappers that is started again, ended early and
hidden while it runs, and rows that lift a box in a rounded or square scroller under a root that ticks.
Motions are compared mid-way and settled, in the frame the clock draws after they moved as well as after
a flush, since a motion a replay left frozen shows without the verifier.

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
