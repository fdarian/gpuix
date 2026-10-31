# Frames and wakes

When a frame is drawn, what wakes JS to run it, and what an idle or quiet window costs. GPUI runs on the JavaScript
thread on macOS, so every rule here is about sharing that thread between frames, input and the app's own tasks. Two
fork patches carry it: no inline draws in real windows (patch 1) and a frame pump driven by the main run loop
(patch 2, with #77, #93, #78 and #79).

## No inline draws in real windows (patch 1)

**Problem.** The macOS addon is built with GPUI's `test-support` feature. It is the crate's default feature
(`packages/native/Cargo.toml:99`), and CI builds the release artifact with `bun run build`, not `build:release`
(`.github/workflows/ci.yml:33`). With that feature, `App::flush_effects` in `zed/crates/gpui/src/app.rs` draws every
dirty window synchronously at the end of every `App::update`. Test contexts need that because they never run a display
link. A real window does not, but the feature is a Cargo feature, so the check was compiled in for it too.

**Evidence.** Each wheel event cost a full draw, about 3 ms, inline in the input handler, and every `applyBatch` and
`scrollTo` drew synchronously as well. Timed in a real (non-offscreen) window with the demo's 2.7k-node diff, 300 calls
each:

| Call | Unguarded `test-support` | Guarded (this fork) | Addon built without `test-support` |
|---|---|---|---|
| `simulateScrollWheel` | 2.34 ms p50 (2.52 p95) | 0.001 ms p50 | 0.03 ms |
| `scrollTo` | 2.33 ms p50 (2.47 p95) | 0.000 ms p50 | 0.01 ms |

The last column was measured with a different method (the wheel path through the real event handler, 2.7 ms down to
0.03 ms), so compare it only loosely. It is the floor: the guarded build is as cheap as a build with no `test-support`
at all.

**Design.** `flush_effects` draws inline only when `App::mode` is `GpuiMode::Test`. `App::new_app` starts in
`GpuiMode::Production`, and only the test contexts (`TestAppContext`, `VisualTestAppContext`, the headless one) switch
it. `TestGpuixRenderer` is built on `VisualTestAppContext`, so it keeps drawing inline and `flush()` still returns a
drawn frame. The real `GpuixRenderer` runs on `gpui_platform::application()`, stays in `Production`, and is drawn by the
display link. The change is one commit in `fdarian/zed` (`gpui: draw inline in flush_effects only for test-mode apps`)
and gpuix needs no change.

A bounds or scroll read in the same turn as a mutation does lay out synchronously, through `Window::draw_if_dirty`. That
is an explicit call, not the `flush_effects` path this patch guards, and it does not draw. Don't make a read draw: inside
input dispatch a draw swaps the frame the dispatch is reading, so a key's dispatch path indexes a different dispatch
tree, and mouse listeners the dispatch holds out of the frame are written back over the new one (scrollers stop
scrolling, bubble handlers see `is_hovered` false). How a layout-only pass avoids that is in
[forced-reads.md](forced-reads.md).

**Alternatives rejected.**

- **Dropping `test-support` from the release build.** It fixes the draw cost, but the feature also gates
  `TestGpuixRenderer`, `createTestRoot` and `captureScreenshot` on macOS (`packages/native/src/renderer.rs`,
  `capture_screenshot`), so a plain release addon loses them.
- **Two addons: a release addon without `test-support`, and a separate test addon with it, loaded only by
  `@gpuix/native/testing`.** Built and tried; it cannot work in one process. Both addons embed GPUI, and a load-time
  constructor registers the Objective-C window class (`gpui_macos/src/window.rs:366`, `ClassDecl::new(name,
  superclass).unwrap()` in `build_window_class`, reached from `build_classes::__ctor_private`). The second `dlopen`
  finds the class already registered, `unwrap()` panics inside an `extern "C"` constructor, and the process aborts
  ("panic in a function that cannot unwind"). Minimal repro: `require` the two `.node` files in one `bun -e`. In the
  react suite, six test files abort their vitest worker: `styles`, `render`, `events`, `floating-controls`,
  `animate-presence` and `debug-frame-overlay`. Each imports the adapter's public entry (which loads the release addon
  through `@gpuix/native`) and `@gpuix/native/testing` (which loads the test addon). The other 21 files, 300 tests,
  passed. Making the two coexist needs either lazily loaded adapter addons or a zed change that tolerates an
  already-registered class, which would share Objective-C method implementations between two dylibs that each have
  their own statics. Neither was worth it, and the two-addon build also loses `captureScreenshot` in the release addon.

**Trade-offs.** The shipped addon still contains the `test-support` code, so it is larger than a build without it.
`TestGpuixRenderer` is also still part of the production binary. The guard trusts `GpuiMode`: an embedder that builds
its own `App` and sets the mode to `Test` gets the old behaviour, which is the point.

**Upstreaming.** Two routes. Land the guard in `remorses/zed` (one commit, no gpuix change), or build the release addon
without `test-support` and make the adapters load the addon lazily so a test process that also uses
`@gpuix/native/testing` never loads two. The first is smaller. The second also shrinks the binary, but it needs the lazy
load and a decision about `captureScreenshot`, and CI would build and publish a second artifact for the test addon.

## How JS is woken (patch 2)

**Problem.** On macOS, GPUI runs on the JavaScript thread and was only serviced by an 8 ms `setTimeout` around
`renderer.tick()`. AppKit input and GPUI's display-link dispatch source therefore waited for the next JS timer: frames
were not paced by the display, and input could wait most of a timer period.

**Why upstream used a timer.** It was deliberate. Bun owns the event loop, and an earlier blocking pump starved Bun's
I/O ([remorses/gpuix#39](https://github.com/remorses/gpuix/issues/39)). Latency was not what the timer was chosen for.

**Alternatives rejected.**

- **Keep the 8 ms timer.** It is the baseline column below: 2 skipped refreshes, a 16.7 ms worst gap between frames on
  screen, 77% of scroll steps drawn.
- **Block in the run loop** (`CFRunLoopRunInMode` for up to `waitMs` inside `tick`, the first version of this patch).
  Input delay dropped to about 0.2 ms, but Bun's I/O ran up to `waitMs` late on every tick: 100 sequential awaited file
  reads took 810 ms, against 3.8 ms with the timer. That is the regression #39 fixed, so the blocking wait was dropped.

**The comparison that settled it.** 120 Hz display, every column built so that scrolling does not draw inline (patch 1
or no `test-support`). The display-link column is a per-vsync wake, which the run-loop wake below replaced; it is the
design the other two were weighed against.

| | 8 ms timer | Blocking wait | Display link |
|---|---|---|---|
| Skipped refreshes while scrolling | 2 | 0 | 0 |
| Worst gap between frames on screen | 16.7 ms | 8.33 ms | 8.33 ms |
| Scroll steps that got drawn | 77% | 99% | 100% |
| Input delay, p50 | 4.1 ms | 0.24 ms | 3.9 ms |
| 100 file reads awaited one at a time | 3.8 ms | 810 ms | 8.7 ms |
| Idle CPU (20 s runs) | 4.42% | not measured | 4.46% |

Waking from the display is the only version that is good on every row, and it costs no extra idle CPU (4.42% against 4.46%). Its lead over the timer is small; patch 1 is the
large practical win. What it left is idle cost (a link that runs whenever a window is visible) and input latency up to a
vsync, which the next sections remove.

### The run-loop watch (#77)

The wake is "the main run loop has work". A per-vsync wake meant an idle one-text window woke JS 75 times a second (FF
measured 120 on a 120 Hz display, about 2.3% CPU), `tick()` ran each time, input waited for the next vsync because
AppKit events are only drained inside `tick()`, and a 100 ms fallback timer ticked an idle app besides.

- **The watch (zed, `gpui_macos/src/run_loop_wake.rs`).** `gpui_macos::set_run_loop_wake_hook` starts a helper thread
  with its own kqueue that holds two `EVFILT_MACHPORT` knotes (no message is received; `EV_DISPATCH`) on the ports the
  main run loop waits on: the main dispatch queue's (`_dispatch_get_main_queue_port_4CF`; CF puts it in the run loop's
  set only while inside `CFRunLoopRun`) and the port set of `kCFRunLoopDefaultMode`. That set is found once on the main
  thread by adding a sentinel `CFMachPort` source to that mode alone, taking the port set that holds it and removing the
  sentinel (a panic if there is not exactly one). When a knote fires the helper calls the hook, so a wake is input (see
  [below](#input-to-an-idle-window-93)), a main-queue block (the display link's frame step included) or a run-loop timer.
  `run_loop_pumped()` enables the knotes again, and a message still pending fires at once, so nothing is lost.
  `set_run_loop_wake_hook(None)` stops the thread through an `EVFILT_USER` knote and joins it.
- **gpuix.** `renderer.setFrameWakeCallback(fn)` registers a non-blocking, weak N-API threadsafe function behind the hook
  and returns whether the platform has one (`true` on macOS); dropping the renderer clears the hook, and clearing the
  callback passes `None`. A wake that arrives while one is still pending, or while `tick()` runs, is dropped. `tick()`
  stays non-blocking: it clears `pending` and calls `run_loop_pumped()` on every path. `startFrameLoop`
  (`packages/native/js/runtime.ts`) schedules `tick()` from the callback and arms no fallback timer on the wake path
  (an idle window arms no timer and gets no wakes). `frameMs` opts into the timer loop, and Windows and Linux use it,
  because only macOS has a display link in GPUI's path.
- **The display link runs on frame demand.** `MacWindow` implements `schedule_frame` and `frame_waker`, which start the
  link for a visible window; it stops after `IDLE_STEPS_BEFORE_STOP` = 2 steps in which nobody asked for a frame (about
  17 ms at 120 Hz). An occluded window never starts it. The occlusion, screen and key-status starts stay, and the idle
  count stops them again.
- **A draw outside a frame request asks for a frame** (`Window::draw` calls `schedule_frame`). It leaves `needs_present`
  for the next request to present, and a stopped link had nothing to wake it for that request. Not covered:
  `needs_present` set by `patch_frame` in a draw outside a frame request (`patch.rs`, left as it is).

**Measured** (75 Hz display, one-text window, live window with the release addon):

| | before (per-vsync wake) | after |
|---|---|---|
| Idle: JS wakes per second | 75 | 0 |
| Idle: `tick()` ms per second | 6.9 | 0 |
| Idle: CPU | 1.45% | 0.29% |
| A DOM write after idle | | first draw 20 to 25 ms after the write, 3 wakes |
| Focused input, caret blinking | | 2 draws/s, 8 wakes/s |
| Colour animation, draws and presents | 75/s | 75/s |

For comparison, Chrome with an idle one-text window has the renderer and the GPU process at 0% (its browser process was
about 9.5% on a fresh profile, which is not frame production).

**Known.**

- Input wakes JS once per event batch; batches are not coalesced to the vsync. A tick costs an estimated 0.2 to 0.3 ms of
  CPU, so a stream of input costs a tick per batch.
- The watch alone does not see input; the observer below is what makes an idle window answer it.
- Apple's app-launch measurement (`libapp_launch_measurement`) polls the main queue every 3 ms, for up to about 500 polls
  after the first present when the startup frames are slow. Each poll is main-queue work and wakes JS, so a few seconds
  after launch cost extra ticks (it still wakes JS about 2.5 s and 5.3 s after start).

### Input to an idle window (#93)

With only the watch, an idle window did not wake on input: a key down, a mouse move, a click or a scroll sat in AppKit's
queue until something else pumped, and FF's Command Menu looked frozen after about 4 s idle (15 of 15 trials without a
wake in 600 ms). AppKit receives every WindowServer event on `com.apple.NSEventThread` (a source1 into HIToolbox
`MessageHandler`, `PullEventsFromWindowServerOnConnection` and `PushToCGEventQueue`, or SkyLight
`CGSSnarfAndDispatchDatagrams`) and hands it to the main thread with `CFRunLoopPerformBlock(mainRL,
kCFRunLoopCommonModes)` and `CFRunLoopWakeUp(mainRL)`. Nothing runs the main run loop under gpuix (the main thread sits
in libuv), so its `ignoreWakeUps` is set and CoreFoundation sends no message. Neither port the watch holds gets one, and
the event enters AppKit's queue only when a pump runs the loop. The 100 ms fallback timer hid this until #77 removed it.

A `kCFRunLoopBeforeWaiting` observer in the common modes on NSEventThread's run loop (found by its name, with
`task_threads` and `_CFRunLoopGet0`) calls `dispatch_async_f` with a no-op on the main queue. That is a message on a port
the watch already holds, sent after the source callout that queued the event. It is level-triggered, so a wake that lands
while a tick has the knotes disabled is not lost. It is not a timer: NSEventThread ran 0 iterations over 5 to 15 s idle.
Don't remove the observer: input to an idle window stops waking JS, and nothing else hides it any more.

Post to the wake of JS, and post to its handler:

| | wake (ms) | handler (ms) |
|---|---|---|
| keyDown | 0.5 to 1.2 | 4.1 to 7.7 |
| mouseMove | 0.4 to 0.9 | 3.3 to 6.2 |
| click | 0.1 to 1.4 | 0.7 to 5.1 |
| scroll | 0.1 to 0.5 | 0.4 to 1.2 |

**Known.**

- An event posted with `-[NSApp postEvent:atStart:]` from a thread that is neither NSEventThread nor the main thread
  still does not wake. It queues a block on the main run loop with an ignored wake-up and never touches NSEventThread. No
  real input takes that path; FF's `poster.ts` Worker trials do.
- The observer uses the private `_CFRunLoopGet0` and the thread name `com.apple.NSEventThread`.

### One frame per JS task

A frame is one task of the JS event loop, as a rendering update is in the HTML event loop: every task that was queued
while it ran gets its turn before the next frame starts. The wake reaches JS as a threadsafe-function call whose callback
only schedules `tick()` with `setImmediate`. The tick runs in the immediates phase, and Bun then polls I/O and runs due
timers before it takes the next wake (in Node, the check phase is followed by timers and poll). The renderer's pending
flag stays set from the wake until `tick()` returns, so wakes that arrive during a long frame are dropped, not queued.

Work that arrived meanwhile is not dropped: the frame step the display link merged into the main queue during a long tick
is still there when `tick()` returns and `run_loop_pumped()` re-arms the watch, so it fires at once and the step runs in
the next tick, right after the JS tasks that were queued. Two steps can run in one pump when a step outlasts a vsync. A
frame that ends before the next vsync takes every wake, so fast frames keep the display rate.

Don't tick inline from the wake callback or clear the flag before the tick (#56). Bun's `ThreadSafeFunction::on_dispatch`
drains its queue without yielding, so once a frame outlasted a vsync each wake queued the next, and timers, promises
waiting on I/O and IPC stopped for as long as an animation ran.

What the frame of a running animation costs is not the pump's, but it decides how long the task is. A tick of a
paint-only animation is offered to gpuix's patcher before gpui builds anything and costs what the changed units cost; a
tick the patcher declines is drawn whole, and an animation whose content is culled draws and presents nothing. A scroll
animation is always asked for as `request_animation_frame` and drawn whole. See [patch-frames.md](patch-frames.md).

### Windows that are not key (#78)

gpui throttles a window that is visible but not key with `inactive_frame_interval`, 30 fps by default. A window of this
fork animates at the display rate, so `to_gpui_window_options` sets it to `None`: 75 to 76 draws/s were measured with
another app frontmost, where gpui capped the same window at about 30 (FF saw about 26).

A window AppKit reports as occluded draws nothing: its link stops on the occlusion notification (`start_display_link`
refuses a window that is not visible) and nothing else asks it for a frame. Under a cover that was 0 draws, presents and
wakes, and the display rate again within about 1 s after the cover left. Animations keep time with the clock and catch up
when the window shows, as the animations of Chrome's hidden pages do. AppKit counts a window whose shadow is uncovered as
visible, so a cover that matches the window's top-left edge does not occlude it; Chrome on macOS, which reads the same
`occlusionState`, behaves the same.

### Waking JS for a webview

WebKit answers the UI process with `CFRunLoopWakeUp`, which the run-loop watch above does not see (its ports are the main dispatch queue's, not the loop's wake). With nothing else going on, `did-start-loading`, `did-navigate`, `did-stop-loading`, `page-title-updated`, `did-fail-load` and the result of `executeJavaScript` of a `<webview>` waited for the next input or frame; nothing arrived for 1.5 s after a `reload()`, then everything at once when something else ticked.

`packages/native/src/platform/webview_pulse.rs` keeps one repeating `CFRunLoopTimer` on the main run loop (common modes) while any page exists, and a timer does wake the watch. The period is 16 ms for 1.5 s after anything asked a page for work (a load, a script, a key, a click, a wheel, a focus change, an event the page sent) and 250 ms otherwise, which catches what a page does by itself. The last page removed invalidates the timer.

**Cost:** an app with a `<webview>` is not idle: about 4 wakes a second at rest and about 60 in the 1.5 s after an action. Don't remove the timer; see [webview.md](webview.md) for the page and [deviations.md](deviations.md#webview-and-window-composition) for the listed cost.

## Memory footprint of a window (#79)

One window and one text, 1x display, footprint: 70 to 65 MB with fonts, 68 to 63 MB without.

**Cut.**

- **gpui's profiler journal ring** (4 MiB). Every `App` allocated it and wrote to it on each task poll, draw and present,
  though only Zed's hang detection and `bench_context` read it. Its slots are now allocated by the first collector, and
  nothing is recorded before one exists. gpuix never reads it. Zed asks at startup, so its behaviour is unchanged.
- **A registered font is held once.** font-kit's `Handle::from_memory` kept the bytes in an `Arc` and copied them into a
  `CFData` for the validation and again for every `load()`, so a font was held two to three times. `add_fonts` now builds
  the `CTFont` for an owned single TrueType or OpenType file from a no-copy `CFData::from_arc` over the bytes.
  Collections and resource-fork fonts keep font-kit's path, which unpacks them into a new buffer. On Arial Unicode.ttf
  (23.3 MB), registering it and laying out a line added 47.9 MB before and 1.1 MB now.

**Left, and why.**

- **`CAMetalLayer` drawables**: 2 at rest, 3 while animating. At 1280 x 780 BGRA each is 3.9 MB at 1x and 15.2 MiB at 2x,
  so about 30 MB on a Retina display. `maximumDrawableCount` stays 3, because with 2 `nextDrawable` blocks when the GPU
  and the compositor together take longer than a vsync.
- **The JavaScriptCore heap** (App-Specific Tag 1, 17 to 20 MB). It is there with no window: it is the JS that was loaded.
- **ColorSync transform tables**, about 11 MB in Malloc Small, from AppKit and CoreAnimation colour matching (the Dock
  icon draw, `CGColor` conversion on commit).
- **gpui's instance buffer** (2 MB) and **A8 glyph atlas** (1 MB).
- **The Objective-C and AppKit runtime**, under 4 MB.

Floors measured for comparison: a bare `bun` 4.4 MB, with the addon loaded 7.8 MB, a compiled app with its JS and no
window 26 MB.


### Where a session's memory goes after a tour (#112)

FF's demo in a real 1280 x 780 window on a 1x display, started on the introduction, then every page visited once through
the app's own `navigate` and back (`footprint.tsx --window --tour`, two rounds). `phys_footprint` is read through
`proc_pid_rusage` (`/usr/bin/footprint` hangs on this machine), and beside it the bytes the system malloc zones hold live
(`malloc_zone_statistics`), which is where the addon allocates. The footprint swings 40 to 50 MB between runs of one build
(it counts the allocator's fragmentation, which depends on timing); the live bytes move by 1 to 2.

| MB, mean of 4 runs (range) | start | showcase | end of tour | end of 2nd tour | at rest |
| --- | --- | --- | --- | --- | --- |
| poc (2a7ee180), footprint | 100 (95-104) | 184 (178-188) | 266 (257-282) | 300 (294-309) | 293 (270-309) |
| poc, malloc live | 40 | 76 | 100 | 103 | 103 |
| round 36, footprint | 102 (101-105) | 186 (183-189) | 241 (209-267) | 236 (212-248) | 236 (212-247) |
| round 36, malloc live | 39 | 68 | 68 | 69 | 69 |

A page that is left and entered again does not grow: the live bytes of poc go from 100 at the end of the first tour to
103 at the end of the second, those of this branch from 68 to 69.

The footprint gap at the end of the first tour (266 against 241) is inside the swing between runs; at the end of the
second tour and at rest the ranges do not overlap (300 against 236, 293 against 236).

**What each cut saves**, in live MB at the end of one tour, one cut turned off at a time in a probe build (the shrinks
gated by an environment variable, never committed), two runs each; all on is 81, the node store move is on top of that:

| cut | live MB |
| --- | --- |
| CSS transition and animation states boxed (an empty slot costs 8 bytes, not 15.5 KB) | 15 (poc 96 to 104, every shrink off 89) |
| the layout engine's node store, moved into a store of its own size | 14 (81 to 67) |
| element arena chunks past what a frame used (gpui) | 4 |
| the bounds registry, and the retained tree's element table and id sets | 1.5 each |
| the scene's primitive vectors, and the hitboxes, listeners and element states of a frame (gpui) | 1 each |

Three further tables (the retain records, the motion state table, the measurements and paint statics) gave back
nothing on the demo in two runs each, and their cuts are not in the tree. The CSS transition shrink itself reads 0 on the
demo too; the boxing in the same commit is where its 15 MB is.

**A page that swaps between sizes does not thrash.** The tables shrink only once they are over four times the use, to
twice the use, and only at the point where the entries left, so there is no timer and no wake. `examples/page-swap.perf.test.tsx`
(`bun run test:perf:swap`) swaps a 2500-row page with a 60-row one, which is over the ratio and reallocates on every
swap, and with a 900-row one, which is under it and never does. CPU of the frame that follows the swap, poc against this
branch, two runs each, p50: over the ratio, the frame that goes from the big page to the small one 24.3 and 24.3 ms against 23.7 and 24.0, the one back 189.5 and 186.7 against 188.7 and 191.7; under the ratio 72.2 and 71.9 against 71.7 and 72.6, and 182.9 and 181.9 against 185.0 and 185.7; a steady frame 0.78 against 0.76 ms on the big page and 0.13 against 0.14 on the small one. The cost of a swap is the build of the page that comes in; the shrink is not
seen in it. `test:perf` (chat, timeline, rerender) reads the same on both within run to run noise (chat idle flush
1.59 and 1.68 ms against 1.70 and 1.67, rerender frame 9.9 against 9.8 ms), and the two `timeline.perf` pan budgets fail on both.

**The node store and ids.** Moving the layout engine's nodes gives each a new id. `HostBox` kept the layout node a host
was painted with, and `Window::box_stamp` of an old id could read another node's stamp, so a host that had moved could
pass for unmoved. A `HostBox` now keeps `Window::layout_engine_id()` beside the node and is not unmoved once it differs
(`automation/host_box_tests.rs` fails without it: the first host of a page widens in the frame that moves the store).

**Left, and why.** Footprint at rest after the tour, 245 MB in the run `vmmap` was read on: malloc small 118 MB dirty
(the zones hold 69 MB live and 84 MB of fragmentation), the JavaScriptCore heap 55 MB dirty, large allocations 19 MB,
IOSurface 11 MB, graphics 11 MB, stacks 7.5 MB, JIT 6 MB. Ranked:

1. **Malloc fragmentation**, about 84 MB at rest. `malloc_zone_pressure_relief(NULL, 0)` called after the tour freed
   nothing the footprint shows (285 to 285 MB and 254 to 254 MB in two runs), so it is not a call to add. What would
   is fewer, larger or longer-lived allocations in the addon, which is a survey of the allocation sites, not a cut.
2. **The JavaScriptCore heap**, 55 MB dirty and 52 MB reclaimable after the tour, 16 MB after a GC: JS garbage that
   collects late. A `Bun.gc()` after a page unmounts would take most of it; it is the app's call, not gpuix's.
3. **Live native data that is not a table**: 39 MB at the start and 68 MB at the end of a tour. The glyph and shape
   caches, `StyleTable` (about 3000 distinct styles at 1.6 KB each at the showcase) and the interned strings hold most of
   it, and none of it shrinks. About 5 MB of the showcase's is the style table, whose `Arc<StyleDesc>` and raw JSON kept for collision
   checks could be one.
4. **A transition state is 15.5 KB** while it runs: a page with 5000 transitioning elements holds 77 MB of them. Making
   `previous: StyleDesc` an `Arc` takes most of that off; the demo never has more than a few hundred.
5. **`CAMetalLayer` drawables** (11 MB of IOSurface at 1x, about 30 MB on a Retina display) and ColorSync tables: the floor
   listed above.

## API

- `FrameLoopOptions` is exported from `@gpuix/native/runtime`.
- Solid's `render()` takes `frameLoop: { frameMs }` and passes it to `startFrameLoop`.
- `GPUIX_PRESENT_LOG` (a zed commit) logs when each `MTLDrawable` is presented; it is how the present gaps in the tables
  above were measured.

## Tests

- gpuix `packages/native/js/__tests__/frame-loop.test.ts` (10) runs the loop against a fake renderer: the tick at start
  and on every wake, teardown, termination, tick errors, and timer polling. An idle wake loop ticks once at start and then
  never, and ticks once per wake and not in between. `frame-loop-yields.test.ts` (2) runs the real loop against a hidden
  window with synthetic wakes from a Rust thread (`simulateFrameWakes`, `setTickDelay`, test-support builds only) and
  checks that timers and file reads keep running when every frame outlasts a vsync.
- zed `gpui_macos` `run_loop_wake` lib (1: the set is found twice alike, is not the main queue's port and has members)
  and `tests/run_loop_wake.rs` (`harness = false`, it pumps the main run loop on the main thread): a `dispatch_async` from
  another thread and a `CFRunLoopTimer` each wake the hook while nothing runs the loop, neither runs before the pump, the
  hook stays quiet for 300 ms after a pump, work that arrives while the knotes are disabled wakes at the re-arm, and
  `None` stops the wakes. With an NSApplication and a window that are never active: 2 s idle with no call of the hook; a
  key down, a mouse move, a click and a scroll posted on NSEventThread each wake the hook within 20 ms (0.1 to 3.5 ms) and
  are dequeued; a window moved with `setFrameOrigin:` comes back from WindowServer as an AppKitDefined event that wakes
  the hook. Without the observer it fails at `key down: no wake within 20ms`.
- zed gpui `window::tests`: a draw outside a frame request schedules a frame before `flush_effects` can, and
  `on_next_frame` schedules and wakes. The idle count of the link is not tested (a window needs a display); the
  measurements above cover it.
- gpuix `window_options_tests` has the unfocused case (an unfocused window is not throttled).
- The webview wake timer is checked by the e2e only (events of a real page arriving at an idle app, [testing.md](testing.md#the-webview-e2e)); the test renderer's pages report from the test.
- Input wakes need a way to post events without the OS: the test-support `GpuixRenderer.postNativeEvent(kind, x?, y?)`
  (`packages/native/src/native_event.rs`) builds the NSEvent for the window and posts it from NSEventThread with
  `gpui_macos::post_event_on_event_thread`; nothing reaches the OS and the app is not activated. A scroll has no NSEvent
  factory, so it is a CGEvent that needs the window number in field 51 and the private `CGEventSetWindowLocation` (test
  only). The react `events.test.tsx` frame-loop test opens a visible window with `focus: false` on the wake path: 0 wakes
  over 2 s idle once the launch measurement ends, then each kind wakes JS 0.1 to 1.4 ms after the post and reaches its
  handler 0.4 to 7.7 ms after it (keyDown through the window-level `onKeyDown`, nothing focused). It fails at `keyDown
  woke` without the observer. The test renderer's own wake support is in [testing.md](testing.md).
- zed `gpui_macos` lib with font-kit (45: an owned TTF gives a native handle and a collection a memory one, and the opsz
  test, Chrome's advances for the Inter subset registered as owned bytes, passes on the new path); gpui journal tests (3:
  no slots, no offered entries and no sequence before a collector; entries offered after one are collected; a writer
  without a collector records nothing, then records once one exists).
