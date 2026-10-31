# Testing

How the fork is tested: the commands, the Chrome oracles that pin DOM/CSS behaviour, the shape of the offscreen test renderer, the switches and counters that tests and measurements read, the retention verifier and the fuzzers, and how performance is measured. Each topic file lists the tests for its own area under `## Tests`; this file holds what they share.

## Test commands

```bash
cd packages/native && cargo test --lib          # Rust unit tests
cd packages/native && bun run test              # vitest; needs react and the wasm build for one test
cd packages/react && bun run test               # vitest, needs the addon built
cd packages/solid && bun run test               # builds, then bun test
cd examples && bun run test                     # the examples suite; *.perf.test.tsx is excluded
cd zed && cargo test -p gpui --lib --features test-support   # GPUI's own tests, where the zed commits are tested
cd zed && cargo test -p gpui_macos --test native_surface       # window composition in a real window (needs an unoccluded display)
cd packages/native/examples/webview-e2e && bun run e2e         # <webview> on real WKWebViews in a real window
```

- `packages/native` has one test that bundles both adapters for the browser. It needs `packages/react` built and `bun run build:web` (nightly Rust and `wasm-bindgen`), so it fails on a machine without them. That is unrelated to the fork.
- The `*.perf.test.tsx` files in `examples/` run with `bun run test:perf` (chat, timeline and rerender) or one at a time, through the scripts that set `NODE_ENV=production`. Use a release addon: a leftover debug build times like a regression.
- Tests that need the native addon are `describe.skip` when it is missing (`hasNativeTestRenderer`), so a green run without the addon proves little.
- Almost none of the tests needs a real window: the test renderer (`createTestRoot`) draws offscreen. The macOS-only pieces (pointer lock, Metal effects, the OS reduced-motion read, the real window drag) were therefore checked against a simulated platform, not a live one. The exceptions open a real window: zed's `native_surface` test and the `<webview>` e2e below.

The perf pass has a guard per issue, each failing on the bug it was written for and not on a clock:

```bash
cd examples
bun run test:perf:depth      # #36: depth 12 costs at most 3x the linear extrapolation of depth 1
bun run test:perf:animating  # #37: one animating node over 2042 elements costs under 0.4x of building
                             #   the page, and builds a fraction of its elements; clone and node counters
bun run test:perf:clipped    # #38: hidden boxes are skipped and cheaper; retention on costs at most
                             #   1.05x off for plain boxes in a scroller, rows that a scroll moves
                             #   cost at most half, a scroller of texts at most 0.6x
                             #   (every frame of the scroll is checked to move the scroller and paint once, #101)
bun run test:perf:draws      # #39: at most 1.05 draws per frame; one layout pass per invalidation
bun run test:perf:layout-idle # #117: a loop beside an idle layout box (layout, "position", "size") patches
                             #   every tick, and a tick costs at most 2.5x (+0.2 ms) that beside no box
bun run test:perf:retain     # #37: judged and skipping frames per page (cards, form, table, chat,
                             #   mail, timeline); prints, no budget
bun run test:perf:swap       # #112: a page that swaps between a big and a small one, over and under the shrink ratio; prints,
                             #   no budget
```

`test:perf:menus`, `:cq`, `:svg` and `:overflow` are older profiles with no budgets or loose ones. The one-animating-node budget counts units: a unit root keeps the start of its build (`BuildStart`, one `Inherited` clone) for a patch to build from again, so the bound of `motion-opacity` is `elementsBuilt + retainableRoots + unitsBuilt`. `timeline.perf.test.tsx` fails its pan budgets; see [Known failing budgets](#known-failing-budgets).

Run the suites under `GPUIX_RETAIN_VERIFY=1` before landing anything that changes what a build reads, what a paint listener writes, a custom element, an exclusion, or anything gpui records: see [The retention verifier](#the-retention-verifier).

## The test renderer

The test renderer is shaped like a live window, not like a harness.

- **Window.** It opens from the live `WindowOptions` (appearance and frameless included). The window is really on no screen (origin -10000, -10000), is not made key and is not ordered on screen, so `x` / `y` are forced to `None`. `TestRenderer.dispose()` tears it down explicitly (`createTestRoot().unmount()` calls it). State nobody disposed is leaked at thread exit rather than dropped, because the drop ran after other thread-locals were gone and aborted the process.
- **Input.** Keys take Playwright's `keyboard.press` grammar and send a `keydown` then a `keyup`. The wheel, `scrollTo` and `getScrollOffset` take the DOM sign. A move carries the buttons held by earlier downs. Every `simulate*` helper draws once if the window is dirty, before and after the input, never otherwise.
- **Hover.** The hover resync that follows a paint (`events/listeners.rs`) applies at most once per frame in the test renderer. A state style that moves the element from under the pointer would otherwise flip hover once per flush, with no frame between, and never return.
- **Scrollbars.** `TestRenderer.setScrollbarStyle("classic" | "overlay")` forces the style; the test window starts as overlay, whatever the host's setting.
- **Accessibility.** The renderer pretends a screen reader is connected (`setA11yActive(false)` removes it), so tests run with an accessibility tree and still see subtrees culled and replayed.
- **Window actions.** The offscreen window has no current mouse event to start a move from, so the drag-region actions (`move`, `titlebarDoubleClick`, `zoom`, `showMenu`) are recorded and read with `takeSimulatedWindowActions()`; see [window.md](window.md#window-drag-regions-appregion). What was and was not exercised live is in [window.md](window.md#platform-status).
- **Placement.** Placement is tested with fake displays in Rust ([window.md](window.md#tests)). **Multiple displays are not exercised in CI**: the tests that read real displays run on whatever the machine has. A window opened on a second display, a rotated one and a mirrored one were not checked live.
- **Size.** `setWindowSize` (AppKit `setContentSize`) resizes the window for the resize tests. `setWindowScaleFactor` keeps its factor across it.

### Window scale and size (#86, #94)

Headless Chrome is 1x unless `--force-device-scale-factor`, which wins over the display. The test window follows that, so a screenshot or a box does not depend on the machine's display.

- **Scale.** A screenshot is the Metal layer's drawable, sized by the platform scale and not by `Window::scale_factor`. A window on no screen took `2.0` from `get_scale_factor`, so on a 1x primary display `setWindowScaleFactor(1)` left the content in the top-left quadrant of a PNG twice the window. zed's `PlatformWindow::force_scale_factor` (test-support) is the fix: `MacWindowState::forced_scale_factor` is what `scale_factor()` returns when set, and forcing sets the layer's `contentsScale` and drawable size, so the capture is the window size times the factor and `set_frame_size`, `update_window_scale_factor` and screen changes keep it. `TestWindow` sets its field; `Window::set_scale_factor` calls it before applying the factor. A window on no screen (or a factor of 0) takes `NSScreen::screens[0]`'s scale, 1.0 with no screens, as a window nobody placed is on the primary display in Chrome. The `TestRenderer` option `scaleFactor` is the native constructor's second argument, not a `WindowOptions` field, so a live window has none; the default is 1, whatever the display. It is applied in the open-window callback before the first frame.
- **Size.** AppKit's `constrainFrameRect:toScreen:` pulled the window onto a display when `orderFront` showed it (asked at (-10000, 10000), a probe found it at (-2372, 189) on the built-in display), and capped every later `setContentSize` to the visible frame: an 1022 px window was 921 logical px, 900x1400 was 949 high. Headless Chrome's window is the size asked whatever the displays. zed's `GPUIWindow` overrides `constrainFrameRect:toScreen:` (a lock-free `keepsFrame` ivar, set at creation) to return the frame unchanged for a window opened by a platform that called `MacPlatform::keep_window_frames`; every other window calls super. `gpui_platform::current_offscreen_platform` is that platform, used only by the `TestRenderer`.
- **Resizing a live window (#94).** A live window keeps AppKit's constraint on show and when the user drags it. gpuix has no resize API for a live window (only `TestRenderer.setWindowSize`), but gpui has `Window::resize`, and `setContentSize:` asks `constrainFrameRect:toScreen:` as well (900x1400 on a 949 pt visible frame gave 900x949). `MacWindow::resize` sets `keepsFrame` around its `setContentSize:` and puts back the value it found, so an explicit programmatic resize is the size asked on any window and nothing else changes. Without the `keepsFrame` window, FF's `check-home-grid` (its checks open `createTestRoot` windows through `scripts/test-window.ts`) failed on a 1512 pt display with a 1600 px viewport container and passed on 1920 pt.
- **Tests.** react `screenshot-scale`: 900x1400 and 2000x600 (`getWindowSize` and PNG size at scale 1 and 2), and a window 300 px wider and taller than the primary display read through `screen` (1812x1282 on the development machine; a `100vw` x `100vh` box, `getWindowSize` and PNG size at scale 1 and 2, at creation and after `setWindowSize`). gpui_macos `window_resize` (`harness = false`, a shown live window): AppKit's own `setContentSize:` to 3000x4000 is capped, `Window::resize` to it is 3000x4000, and once small again AppKit's is capped as before. 3000x4000 stays under Metal's 16384 px texture limit at 2x; 9000 high aborted. The fallback for a screenless window has no unit test: libtest's threads abort creating an NSWindow.

### Waking JS for input (#95)

`TestGpuixRenderer` has the live renderer's wake surface with the same names and semantics, so a downstream app can check that input to an idle window wakes JS and reaches its handler ([frames-and-wakes.md](frames-and-wakes.md#input-to-an-idle-window-93)) headlessly with `createTestRoot`. gpuix only; zed is unchanged.

- `setFrameWakeCallback(cb)` is the same `FrameWake` as `GpuixRenderer` (`gpui_macos::set_run_loop_wake_hook`, true on macOS).
- `postNativeEvent(kind, x?, y?)` takes the same kinds, through `native_event::post` on the test window's `NSView`. The NSEvent reaches the window's own input callback, so the JS events are the ones the `simulate*` helpers produce.
- `requiresTick()` and `tick()`. The test platform is `MacPlatform::new(false)`, whose `pump_events` panics, so `tick()` drains AppKit itself (`native_event::drain_app_events`): `-[NSApp nextEventMatchingMask:NSEventMaskAny untilDate:distantPast inMode:NSDefaultRunLoopMode dequeue:YES]` and `sendEvent:` until none are left, then `CFRunLoopRunInMode(default, 0)` once so the wake's no-op block on the main dispatch queue is drained, then `run_until_parked`. Then `pending` is cleared and `run_loop_pumped()` re-arms the watch as the live tick does.
- JS: `TestRenderer` exposes the four, and its `tick()` also hands the queued native events to JS as `dispatchNativeEvents` does, so `startFrameLoop(testRoot.renderer)` runs on the wake path. `tick()` draws nothing: frames stay `flush()` and `drawFrame()`.

Post to the wake of JS / post to its handler, in react `events.test.tsx` (ms): keyDown 2.5 / 14.0, mouseMove 0.8 / 7.9, click 1.3 / 7.8, scroll 0.3 / 0.6.

The window's startup and the process's launch measurements wake the loop in bursts for about 5 s after it starts (wakes at 0 to 330 ms, then 2.1 s and 4.8 s), as a live window does. A check waits for a quiet second before it posts; 0 wakes over 2 s idle holds after that. An event posted with `-[NSApp postEvent:atStart:]` from another thread still does not wake.

The guard is `events.test.tsx`, "wakes an idle test-renderer wake loop for input that AppKit receives, and runs its handler in that tick": `createTestRoot`, `startFrameLoop(testRoot.renderer)`, 0 wakes over 2 s idle, then keyDown (window-level `onKeyDown`, nothing focused), mouseMove, click and scroll each wake JS within 20 ms of the post and reach their handler within 50 ms. With `setFrameWakeCallback` not installed it fails at `keyDown woke`.

The frame-loop tests (a fake renderer, and the real loop with synthetic wakes: `simulateFrameWakes` and `setTickDelay`, test-support builds only) are listed in [frames-and-wakes.md](frames-and-wakes.md#tests). `GPUIX_PRESENT_LOG` (a zed commit) logs when each `MTLDrawable` is presented; it is how present gaps are measured.

### The `<webview>` e2e

The test renderer scripts its pages, so what a real `WKWebView` does is checked by an example app that opens a real window, in `packages/native/examples/webview-e2e/`:

```bash
cd packages/native && bun run build        # the release addon
cd packages/react && bun run build
cd packages/native/examples/webview-e2e && bun run e2e     # sh run.sh; exit status 0 passed, 1 a check failed, 2 could not run, 3 not checkable
E2E_ONLY=loading,pixels,pointer,focus,scripts,resize,visibility,lockstep bun run e2e   # some groups
E2E_STEPS=300 bun run e2e                                                              # scroll frames of the lockstep (default 120)
E2E_ONLY=lockstep E2E_SKIP_ACTIVATION=1 bun run e2e                                   # the lockstep group on a machine that will not make the app active
```

`run.sh` builds a helper dylib (`e2e_helper.m`, into `~/.cache/gpuix-webview-e2e/libe2e.dylib`), then `bun e2e.tsx` renders the React app (`app.tsx`) and runs the checks (`driver.ts`) **inside the same process**. Screenshots and the present log (`GPUIX_PRESENT_LOG`) go to `~/.cache/gpuix-webview-e2e/`.

Why in-process: macOS (TCC) denies a driver capturing another process's window and posting events to another process (`CGEventPostToPid`) without permissions it does not have, and a global post would land on whatever the user is doing, so a driver outside the app can neither see nor move it. The helper, loaded through `bun:ffi` so that the product has no test hook, captures the app's own window with `CGWindowListCreateImage` (no permission for one's own window), hands `NSEvent`s to the window's `sendEvent:` (mouse) or `-[NSApp sendEvent:]` (keys), and reads the first responder. It touches nothing outside the process. A wheel event is made with `CGEventCreateScrollWheelEvent2` and `+[NSEvent eventWithCGEvent:]`, which has no window and a screen location, so the helper overrides `locationInWindow`, `windowNumber` and `window` for that one event.

The app has a GPUI input, a focusable button and a hover element; a scroll container holding a page taller than the container, with a GPUI marker beside it; a rounded page; a half-opaque page over the rounded one; and a popover painted after them, partly over a page. The fixture page (`fixture.html`) has coloured blocks, an input, a button with a counter, a link and a script that sets its title; `fixture-bare.html` has nothing tabbable. Colours are compared to within 22 per channel (the capture is in the display's colour space), and a half-opaque page is checked to lie between the two it is over.

Groups: events (the five, with a failed load: `-1100` for a missing local file), pixels, scroll (cut at the container's edge, offsets 0 to 460), pointer (a click reaches the page, a click on the popover does not, a wheel reaches the page and not the container, hover leaves the GPUI element beside the page), focus, Tab in and out (with and without tabbable elements), `executeJavaScript` (a number, an object, `undefined` against `null`, a thrown error, a `ReferenceError`), resize, visibility (`display: none`, unmount and remount) and lockstep. The last run of the final build: 56 passed, 0 failed, 1 not checkable (the cursor, which WebKit sets only for the real pointer).

Before any check the e2e makes sure the machine lets it look at its window: it activates its own app and makes its own window key (up to six tries, 300 ms apart), then needs the app active, the window key and visible to AppKit (`occlusionState`), the login session on the console and unlocked, and a capture that comes back at the window's size and is not one colour. If it cannot, it prints `not checkable: <the condition>` and exits 3, with no failures listed. A check that fails while the window has lost any of that is reported as `NOT CHECKABLE` with the condition and the failure it saw, not as a failure; the e2e makes the window key again, continues if it can, and otherwise stops with exit 3. Every `FAIL` line ends with the window's state (`app active, window key, window visible`). Exit 1 is only for a check that failed on a machine that could be looked at.

`E2E_SKIP_ACTIVATION=1` (only with `E2E_ONLY=lockstep`, which reads frames and captures and sends no events) waives "the app is active" and "the window is key": it does not activate the app, moves its own window to the top left of the main screen with `setFrameOrigin` (clear of a dialog in the middle of the screen) and still needs the window visible, the session unlocked and a good capture. It exists for a machine where a dialog that cannot be dismissed holds the activation.

What the machine takes away, as measured: when another app holds the activation (the owner switching apps, or a system authentication dialog from `SecurityAgent`, which no other app can take the front from), the window stays visible and capturable but is not key, so no event reaches the page (the first responder stays `GPUIView`) and every pointer, focus and Tab check fails; a window that is hidden (`Cmd-H`), minimized, ordered out, or on a display that is asleep makes `CGWindowListCreateImage` return null (`capturing the window failed (2)`); a window that is covered gets no frames, so its pages never load.

The lockstep check scrolls the container (offsets 0 to 41) and, after each frame, captures twice and compares the top row of the page's marker with that of the GPUI marker beside it in the same capture. It allows 3% mismatches and prints, per run (idle, loaded), the frames and captures, the mismatches as a rate against the limit, which side was late (the page or the GPUI marker, against the previous and the requested offset), how many were in the capture right after the frame and how many a turn later, the largest gap, the window's state and the load average, then repeats those lines after the table ([deviations.md](deviations.md#webview-and-window-composition) has the numbers). A machine with load makes it worse: compare variants in one process, switching every few steps.

It runs on a display where its window is shown (an occluded window gets no display link, which stopped zed's `native_surface` test). It is not part of any suite or CI.

### The `<nativeview>` e2e

`packages/native/examples/nativeview-e2e/` has the same shape as the `<webview>` e2e (an example app that opens a real window and checks it from inside the same process by captured pixels and by events handed to its own window) and reuses its helper (`libe2e.dylib`, `environment.ts`: the "not checkable" pre-flight, exit 3). The library side is a separate crate, `addon/`: a napi addon with napi and the Objective-C runtime as its only dependencies, which makes a plain coloured `NSView` (a runtime-declared subclass that counts `mouseDown:` and `hitTest:` and answers `hitTest:` as asked) from the `bigint` it is handed, adds it to the content view and removes it in `detach`. No gpuix type crosses.

```bash
cd packages/native && bun run build        # the release addon
cd packages/react && bun run build
cd packages/native/examples/nativeview-e2e && bun run e2e     # sh run.sh; exit 0 passed, 1 a check failed, 2 could not run, 3 not checkable, 124 the hard limit of 600 s
E2E_ONLY=order,geometry,overflow,pointer,focus,lifecycle,close bun run e2e   # some groups; close is always last
sh check-no-gpui.sh                                                           # the proof below
```

`check-no-gpui.sh` prints `cargo tree` and `otool -L` of the addon and fails if `gpui`, `gpuix` or `zed` is in either (the path of the checkout is left out of the match): the dependencies are `napi`, `napi-derive`, `objc` (and `napi-build` at build time), and the addon links `libobjc`, `libiconv` and `libSystem` only.

Groups: order (background below the view, a child and a later sibling above it, the border around it), geometry (scroll offsets, window resize, `translate` / `scale`, `display: none` and back without a second attach, `visibility: hidden`), overflow (a view drawing outside its box with `overflow` visible and hidden, the inner radius, an ancestor clip), pointer (default, `nativeEvents` with each `hitTest:` answer, a child above the view, `stopPropagation` on `pointerdown`, `pointer-events: none`), focus (the keyboard returns to the window view on a GPUI press), lifecycle (attach and detach counts and order from the addon's log, `onDetach` while the view still has a superview and before the pixels change, also inside `flushSync`, remount, two views, keyed reorder, an unmount before the queued attach arrives) and close (the app's own window is closed from a main-queue block; the process ends with the last window, so the addon appends every attach and detach to a log as it is called and `verdict.ts` reads it after the process ended). The last full run: 52 passed, 0 failed, 0 not checkable. On this machine four earlier runs of the same code lost the app's activation while running and reported 2 to 9 rows as not checkable (exit 3), with no failures; each check now first gets the window back (`ensureCheckable`) and the run reports not checkable only if it cannot.

## Chrome oracles

An oracle is a DOM/CSS question put to headless Chrome once, with the answer committed. Tests compare gpuix against the committed answer and never start Chrome, so the suites stay offline and the expected value is Chrome's, not a number someone typed in.

Files, all in `packages/react/src/__tests__/fixtures/` (one native oracle in `packages/native/js/__tests__/fixtures/`):

- `<name>-oracle.html` is the page. It is built so that Chrome's answer is readable by script: boxes (`getBoundingClientRect` relative to a `data-case` box), computed values, pixel rows of a screenshot, or an event log. The header comment says what the page measures and, for a page with no script, the command that regenerates it.
- `<name>-oracle.json` is the answer. `<name>-oracle-x2.json` is the same page on a display of device scale 2.
- `<name>-oracle.{sh,ts,py}` regenerate the JSON. Never edit a JSON by hand: change the page, rerun the generator and commit both.

### How an oracle is generated

The shell scripts run the headless shell that Playwright caches, `~/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell` (nothing is downloaded). `CHROME=<path>` picks another binary. Every generator starts its own process from that binary and ends it; none attaches to a running browser or uses anyone's profile, and `CHROME` must not point at one. Only `content-visibility-animation-oracle.json` records the Chrome version (`156.0.8078.4`); the others do not.

| Shape | How | Oracles |
|---|---|---|
| Dump the DOM | `--headless --window-size=1280,800 --dump-dom file://<page>`, usually with `--virtual-time-budget=3000 --allow-file-access-from-files` and `--force-device-scale-factor=1`. The page script writes its answer into a `data-oracle` (or `data-rects`) attribute; `grep -o` takes it out and `python3` unescapes it. | most `.sh` scripts; the pages with no `.sh` give the same command in their header |
| Two scales | `dsf-oracle.sh <name>...` (default `line-box inline-strut text-baseline`) writes `<name>-oracle.json` and `-x2.json` with `--force-device-scale-factor=1` and `=2`. It is the display's own scale, which is what a Retina screen is: Blink zooms the page by it, so a font is measured at its device size and its ascent, descent and line gap are rounded in device pixels (Menlo 20px has a content area of 23 CSS px at scale 2). Playwright's `deviceScaleFactor: 2` only scales the raster and measures every font as at scale 1 (24), so it says nothing about a 2x display. | every `-x2` oracle |
| Pixels | `--screenshot` at a device scale, read by a stdlib Python script run with `python3 -I`: ink coverage, the rows a glyph is painted on, underline rows, colours at named points. | `font-smoothing`, `glyph-origin`, `mask-border`, `text-decoration`, `text-decoration-lines`, `inline-opacity` |
| DevTools protocol | `bun <name>-oracle.ts [scene...]` spawns the shell with `--remote-debugging-port=<random>` and `--no-sandbox`, drives it over CDP and kills it. Needed where the answer depends on a real compositor clock, Tab presses or wheel events. Scenes named on the command line are rerun alone and replace only their entries; each scene gets a page (or a Chrome) of its own. | `content-visibility-a11y`, `-auto`, `-observers`, `display-contents`, `scroll-anchoring`, `scroll-order`, `svg-transitions`, `wheel-fraction` |
| Playwright | `uv run --with playwright python`, for Tab presses, a devtools trace and animations that need real time. `content-visibility-animation-oracle.sh` runs each scene in a Chrome of its own and drops the trace. `motion-waapi-oracle.py` needs `MOTION_ROOT` (a `node_modules` that holds `motion`, `framer-motion`, `react` and `react-dom` at motion 14.1.0): it builds `vendor/motion.js` and a `bun build` of `motion-waapi-oracle-react-entry.js` into a temporary directory and runs every case in a page of its own (`WORKERS` pages at a time), since Motion's keyframe-resolver queue is global. `motion-layout-oracle.py` (same `MOTION_ROOT`) records framer-motion's `layout` and `layoutId` in real time, one page per scene (`height`, `widthMove`, `position`, `size`, `moveOnlySize`, `nested`, `radius`, `layoutId`): the boxes every frame after the flip, with the start of the glide fitted to a straight line of progress, and `motion-layout-oracle.test.tsx` replays each at the recorded times to 1.5 px, with the corner radius and shadow ring of `radius` read from pixels. | `tab-order`, `content-visibility-animation`, `motion-waapi`, `motion-layout` |
| Node and DevTools | `web-animations-oracle.sh` runs the page in two ways and joins the JSON: the first kind of case with `--dump-dom` and a virtual time budget, and `apiCases`, `treeCases` and `effectCases` with `web-animations-oracle.mjs` (node 22+) in real time over the DevTools protocol, since a virtual-time run draws one frame and no event, `ready` or start time ever arrives. | `web-animations` |
| Motion's engine | Not Chrome: Motion's own animation code run on a clock the script steps, with `bun`. `MOTION_ROOT` is a `node_modules` that holds motion (14.1.0), `MOTION_DOM` the `motion-dom/dist/es` directory (12.x). `motion-controls-oracle.mjs` records 109 rows of the controls `animate()` returns (the time, the delay, speed 0, `isAnimating()`, `startTime`, `attachTimeline`, the default curve of a value, springs of a value: when each ends and what it reads from then on, ...), and a test compares native to each with no list of known differences (the spring rows to 1e-6). `motion-default-oracle.mjs` runs motion-dom's `animateTarget` on a stub element (`MOTION_DOM`, 14.1.0) for 79 transitions that decide the default curve of a key (none, `delay` only, `default`, an entry beside it, `type` without a duration, a variant's), and `motion-default.test.tsx` of React and Solid replays the width, height, `x`, `y`, `translateX` and `scale*` ones (72 of them) to 0.6 px. | `motion-controls`, `motion-repeat`, `motion-default` |

### How a test reads one

A test loads the JSON from `fixtures/` with `fs.readFileSync`, renders the same markup through the test renderer and compares. `oracle-html.tsx` turns a page's spans and `style` attributes into elements, `oracle-boxes.tsx` does the same for boxes, images and table elements (a table element becomes the `div` with the `display` the user agent stylesheet gives it, and the `id` is the `testId`). Scenes that both sides play (`scroll-anchoring-scenes.ts`) are one description, so the page and the test cannot drift apart.

An `-x2` file is read with `setWindowScaleFactor(2)`. Tolerances follow Chrome's units: the boxes of an inline element are compared to 1/64 px (`TOLERANCE`, Chrome's layout unit), a block to the nearest device pixel (`0.51 / scale`). The Solid tests read the React fixtures by relative path.

### The oracles

Test files are in `packages/react/src/__tests__/` unless a path says otherwise. "+x2" marks a second file for scale 2.

| Topic | Oracle | Read by |
|---|---|---|
| Container queries | `container-query` | `container-query.test.tsx` |
| | `container-hosts` (+ `container-hosts-cases.json`) | `container-hosts.test.tsx` |
| | `container-order` | `container-order.test.tsx` |
| | `inline-container` | `inline-container-query.test.tsx` |
| | `inline-container-display` | `inline-container-display.test.tsx` |
| | `table-container-display` | `table-container-display.test.tsx` |
| Sizing and shorthands | `fit-content`, `fit-content-margin` | `fit-content.test.tsx` |
| | `flex-basis` | `flex-basis.test.tsx` |
| | `flex-column-size` (+x2) | `flex-column-size.test.tsx` |
| | `flex-row-fit` (+x2) | `flex-row-fit.test.tsx` |
| | `tooltip-chip` (+x2; a Fluid Functionalism tooltip chip in a deferred `<anchored>` in a shrink-wrapped trigger) | `tooltip-chip.test.tsx` (React and Solid) |
| | `flow-root` (+x2) | `flow-root.test.tsx` |
| | `fractional-lengths` | `fractional-lengths.test.tsx` |
| | `shorthand-order` | `shorthand-order.test.tsx` |
| | `inline-relative` | `inline-relative-percent.test.tsx` |
| | `sticky-padding` | `sticky.test.tsx` |
| | `direction` | `direction.test.tsx` |
| | `resolved-style` | `resolved-style.test.tsx` |
| | `display-contents` | `display-contents.test.tsx`, `virtual-list-contents.test.tsx` (React and Solid) |
| Tables | `table-percent-heights` | `table-percent-heights.test.tsx` |
| | `table-border-spacing` (+x2) | `table-border-spacing.test.tsx` |
| | `table-baseline-cell` (+x2) | `table-baseline-cell.test.tsx` |
| Line boxes and baselines | `line-box` (+x2) | `line-box.test.tsx` |
| | `inline-strut` (+x2) | `inline-strut.test.tsx` |
| | `empty-inline-box` (+x2) | `empty-inline-box.test.tsx` |
| | `text-baseline` (+x2) | `text-baselines.test.tsx` |
| | `x-height` (+x2) | `text-box-metrics.test.tsx` |
| | `line-fit` (+x2) | `line-fit.test.tsx` |
| | `line-height-normal` (+x2) | `line-height-normal.test.tsx` |
| | `atomic-trim-baseline` | `atomic-trim-baseline.test.tsx` |
| | `scroll-baseline` (+x2) | `scroll-baseline.test.tsx` |
| | `inherited-start` | `inherited-start.test.tsx` |
| Text measure and breaking | `text-measure` (+x2) | `text-measure.test.tsx` |
| | `line-break` | `line-break.test.tsx` |
| | `hyphen-break` | `hyphen-break.test.tsx` |
| | `soft-hyphen` (+x2) | `soft-hyphen.test.tsx` |
| | `fallback-advance` (+x2) | `fallback-advance.test.tsx` |
| | `cjk-spacing` (+x2) | `cjk-spacing.test.tsx` |
| | `text-wrap-spans` | `text-wrap-spans.test.tsx` |
| Glyphs and decorations | `glyph-origin` | `glyph-origin.test.tsx` |
| | `font-smoothing` | `font-smoothing.test.tsx`, `font-optical-size.test.tsx` |
| | `text-decoration` | `text-decoration-oracle.test.tsx` |
| | `text-decoration-lines` | `text-decoration-lines.test.tsx` (React and Solid) |
| | `text-decoration-motion` | `text-decoration-motion.test.tsx` |
| Paint | `mask-border` | `mask-border.test.tsx` |
| | `inline-opacity` | `inline-opacity.test.tsx` (React and Solid) |
| Transforms | `transform-geometry` | `transform-geometry.test.tsx` |
| | `transform-animation` | `transform-animation.test.tsx` |
| | `transform-skipped-read` | `transform-skipped-read.test.tsx` |
| | `transform-3d` | `transform-3d.test.tsx` |
| | `transform-text` | `transform-text.test.tsx` |
| | `transform-snap` (+x2; 528 boxes moved by a translation as `left`, `transform`, `will-change` and `translateZ(0)`, with a background, a border or both, in parents at fractional positions) | `transform-snap.test.tsx` (React; edges within a tenth of a device pixel, Motion's `x` too) |
| | `transform-backdrop` | `transform-backdrop.test.tsx` |
| | `transform-run-context` | `transform-motion-context.test.tsx` |
| Scrolling | `scroll-anchoring` (+ `scroll-anchoring-scenes.ts`) | `scroll-anchoring.test.tsx` |
| | `scroll-order` | `scroll-element-api.test.tsx`, `forced-layout.test.tsx`, `frame-event-order.test.tsx` |
| | `scroll-extent` | `scroll-extent.test.tsx`, `virtual-list-contents.test.tsx` |
| | `scroll-fraction` (+x2) | `scroll-fraction.test.tsx`, `fractional-bounds.test.tsx` |
| | `wheel-fraction` (+x2) | `scroll-wheel-fraction.test.tsx` |
| | `virtual-list` (+x2) | `virtual-list-rows.test.tsx` |
| | `scrollbar-gutter` | `scrollbar-gutter-frame.test.tsx`, `content-visibility-observers.test.tsx` (`io8b2`) |
| Focus | `tab-order` | `tab-order.test.tsx` |
| Motion | `animatable` | `animatable.test.tsx` |
| | `motion-values` | `motion-chrome-values.test.tsx` |
| | `gesture` | `motion-gestures.test.tsx` (React and Solid) |
| | `span-motion` | `span-motion.test.tsx` (React and Solid) |
| | `svg-keyframes` (`none` and `auto` keyframes, path pairs, arc flags) | `svg-keyframes.test.tsx` |
| | `svg-transitions` (`bun svg-transitions-oracle.ts`: timed rows sampled with every transition paused, and event rows, of attribute and `style` changes on shapes and the `<svg>`, including a transition declared in the same update) | `svg-transitions.test.tsx` (React and Solid) |
| | `web-animations` (`apiCases`: playback, replayed on a window that draws only at `settle`; `treeCases`: `getAnimations()`, `{ subtree }` and `document.getAnimations()`; `effectCases`: `getKeyframes()` and `getTiming()` of every kind of value) | `web-animations.test.tsx` (React and Solid), `web-animations-forget.test.tsx` |
| | `motion-waapi` (Motion 14.1.0 `animate()` and `motion.div` in Chrome: what `getAnimations()` lists, the element's pixels, Motion's completions, the events, and the commands on the listed `Animation`) | `motion-waapi.test.tsx` (React and Solid) |
| | `motion-repeat` (Motion's engine) | `motion-mount.test.tsx` (React and Solid) |
| | `motion-controls` (Motion's engine) | `packages/native/js/__tests__/motion-controls-native.test.ts` |
| Content visibility | `content-visibility` (+ `content-visibility-cases.json`) | `content-visibility.test.tsx` |
| | `content-visibility-a11y` | `content-visibility-a11y.test.tsx` |
| | `content-visibility-auto` | `content-visibility-auto.test.tsx` |
| | `content-visibility-observers` | `content-visibility-observers.test.tsx` |
| | `content-visibility-animation` (scenes `q1` to `q3` for runs and transitions, `q1_load_read`, `q2_class_read`, `q3_inside_read` and `q6` for reads of never-built content, `q7` for which elements a read styles) | `content-visibility.test.tsx` and `content-visibility-auto.test.tsx`, through `skipped-animation-events-fixture.tsx` and `skipped-animation-scope-fixture.tsx` |

Where an oracle disagrees with Chrome on purpose, the test says so (`scroll-anchoring.test.tsx` has `KNOWN_DEVIATIONS`) and the topic's section in [deviations.md](deviations.md) lists it.

## Switches and counters

Environment variables set a mode for a whole process; the per-window setters do the same on one window of the test renderer.

| Switch | Effect |
|---|---|
| `GPUIX_RETAIN_OFF=1` | Nothing is recorded or skipped: the baseline for timing a build. `setRetainEnabled(false)` does it at runtime. |
| `GPUIX_RETAIN_VERIFY=1` | Every frame that replayed is built once more, whole, and compared (a frame that recorded roots to be moved is not: the frame after it moves them and is compared). gpui's mismatches print `[retained]` on stderr, are counted in `getRetainedCounters().mismatches` and listed by `getRetentionMismatches()`. gpuix's own checks print `[retain]` on stderr and are listed by the window's `getRetainMismatches()`: an element judged unchanged that built differently (also counted in `retainMismatches`), and a spine that is not closed under parent. Both kinds fail `dispose()` (`createTestRoot().unmount()` calls it). The reference pass reads the instant of the pass it is compared with. It also skips every subtree it can (minimum root 1) and turns the back-off off, so small test pages exercise replay. `setRetainVerify(true)` does it per window. |
| `GPUIX_RETAIN_VERIFY_REPORT=1` | With verify, prints the verified frames, gpui's mismatches, `gpuix-mismatches=N` (gpuix's own findings), replays, hits and non-reusable reasons for each window at dispose. |
| `GPUIX_RETAIN_MIN_ROOT=n` | The smallest subtree that is skipped unless it is costly to build (a text, an image, an `<svg>` or a custom element). Default 2; 1 takes every box, to measure. |
| `GPUIX_RETAIN_DEBUG=1` | Keeps what the last recorded frame built and why; `getRetainDebug()` returns it as JSON: elements built by reason (`slots`, largest first, named by type, `testId` and ancestors), what named them (`sources`, `causes`), the ones a record said could be skipped and were not (`notSkippedWhy`), the subtrees gpui built late (`deferred`), why an element excluded itself where the exclusion has more than one cause (`exclusionWhy`, from `CustomElement::unretained_because`; for an `<svg>` the `Unsettled` name), and the elements ticks were declined for as outside every unit (`outside`: label, record, refusal, whether it is hidden or unbuilt; `[d:none]` in a label). Not wrapped by `TestRenderer`; call it on `renderer.native`. |
| `GPUIX_PATCH_OFF=1` | A tick of paint-only animations is drawn whole, like any frame, and not by patching the frame shown: the baseline for timing a tick. `setPatchEnabled(false)` does it per window on the test renderer (the live renderer has the variable only). Patching is also off while the retention is. |
| `setRetainPlanning(false)` | Elements are recorded and judged, and all are built: what `retain.perf` counts. |
| `setRetainLift(names)` | Counts as if the named exclusions were lifted, to see which to lift first. Nothing is reused. |

Counters on the test renderer, each since the window was made (subtract two reads for a span of frames) unless it says otherwise:

- `getRetainedCounters()`: `hits` (subtrees a frame did not build), `replays`, `lazyBuilds`, `builds`, `layoutFallbacks`, `layoutSkips`, `layoutReplays`, `layoutBuilds` and `layoutRecords` (the same for layout-only passes, and the builds that left a layout record: a layout-only pass wraps what it builds, so the first build of a root is one of them; a skip at a translated place is a `layoutSkips` too, zed counts it as `layout_translations` and JS does not see it), `verified` and `mismatches` (under verify), `relocations` (subtrees a scroller moved without building them), and three JSON objects keyed by reason: `nonReusable` (`Lift`, `Escape`, `DeferredDraw`, `NestedLayout`, `UnstableLayout`, `UnsettledLayout`, `LayoutMismatch`, `RedrawRequest`, `Autoscroll`, `FrameCallback`, `PendingAsset`, `WindowControl`, `CachedView`, `Publication`), `nonRelocatable` (`Scale`, `Hole`, `Sticky`, `Escape`, `PositionedCapture`, `InputHandler`, `NestedScroll`, `Publication`, `Clip`) and `relocationMisses` (`Place`, `Region`, `Clip`).
- `getBuildCounters()` (`resetBuildCounters()` starts over): `elementsBuilt`, `elementsSkipped`, `styleDescClones`, `inheritedClones`, the retainable share (`retainableRoots`, `retainableElements`, `retainableElementsLarge`, `retainReasons` as JSON), `retainMismatches` with `getRetainMismatches()` (elements judged unchanged that built differently, under `setRetainVerify`), `unitsBuilt` (subtrees built whole as units) and `unitsOversized` (units over 256 elements, built whole), `motionsSynced` (the motion elements a sync read: in a frame built whole the ones it builds and the ones a gesture reached, in a patched tick the ones that move) and `motionValuesSampled` (the values sampled for the elements that bind them: all of them in a frame built whole, the ones that move or were written in a patched tick).
- `getLayoutCounters()`: `nodesCreated`, `nodesReused`, `nodesRemoved`, `stylesSet`, `childrenSet`, `passes`, `measures`, `nodesLaidOut` and `nodesSized` (taffy's cache misses by run mode: a leaf change at depth d lays out d + 1 nodes and sizes d, whatever the fan-out; a relayout boundary stops the chain below it), `relationVisits` (the elements `accessibility::links::compute` read since the tree was made; 0 without a label or an `aria-*` reference), the text system's `shapes`, `wrappedLayouts` and `lineLayoutHits`, and the subtrees whose painted extent (`extentWalks`), scrollable overflow (`overflowWalks`) and unsnapped overflow (`unsnappedOverflowWalks`) were worked out from their children and not found as an earlier pass left them.
- `getFrameWorkCounters()`, from gpui's `Window::frame_work_counters`, says what a frame did to content that did not change: `prepainted` and `painted` (elements whose own prepaint and paint ran, outside a culled subtree, the ones a patch painted included), `culledPrepainted` and `culledPainted`, `paintedPrimitives` (what painting put in the scene, a patch's included, with what is hidden or clipped away left out) and `replayedPrimitives` (what a replay put in again), and `boundsInserts` (the draw orders worked out with the scene's bounds tree; a scene a patch makes again from the orders it had pays none). A replayed root counts none of the elements under it, and a root's wrapper is not counted. It sits beside `getRetainedCounters()`, which counts the replays.
- `getPatchCounters()`, on the test renderer and on the live `GpuixRenderer` (so a run in a real window with the display link can tell a patched tick from one drawn whole). It reads gpui's `Window::patch_counters`. Fields: `patchedFrames` and `patchedUnits`, `emptyTicks` (ticks that repainted no root and drew nothing, #97), and two JSON objects keyed by reason.
  - `declines` are the ticks gpui drew whole after the patch was tried: `PendingView`, `Refreshing`, `UnknownUnit`, `StaleRecording`, `Surroundings`, `LayoutMismatch`, `UnstableLayout`, `Effects`, `Hitboxes`, `Registrations`, `Operations`, `Lifts` (a unit whose root stopped being a stacking context while it holds lifted children, as a fade that ends at 1 on a box with an absolute child does) and `Rejected`.
  - `unpatchable` are the ticks gpuix did not try to patch: `NoFrame`, `LayoutPass`, `Unsteady`, `Gliding` (a `layout` glide is in flight, or its events are to be sent), `ScrollAnimation`, `Edited`, `Everything`, `Sourced`, `Outside` (an element with a box that the last build gives no reason for; the first blink of a caret is one), `Ended`, `Dormancy` (nothing to repaint, but elements are to enter the dormant set; a tick that has no unit and nothing to settle is no tick drawn at all, see `emptyTicks`) and `NotAUnit` for a unit that could not be built. The reason an element something runs on is no unit (`Record::refusal`, returned by `Retain::preview` as `Unpatchable::Refused`) is one of `Excluded:<kind>` (the kinds of `Exclusion`: `paint-reporter`, `highlight`, `custom-element`, `svg`, `virtual-list`, `virtual-row`, `collapsed-table`, `display-contents`, `anchored`, `sticky`, `fixed`, `scroller`, `layout-animation`, `atomic-inline`, `unstable-layout`), `ExcludedBelow:<kind>` (something in it is), `Escapes` (it holds an absolute box placed against a block outside it), `Lifts` (something in it is lifted out of it into a stacking context it does not form), `Scrolls`, `LayoutAnimation` (what runs on it moves what it lays out; looked at first, so an absolute element whose `top` runs is `LayoutAnimation`, and so is a tick made `Sourced` by the measurement of a layout animation) and `TooLarge` (over 256 elements). The keys are strings, and a page with no unit counts its ticks too (`patches_ticks` needs no unit). A run is patched when `patchedFrames` equals its ticks.
- `getLayoutPassCount()`, `getPaintedFrameCount()` and `getCulledSubtreeCount()`; `getRetainedElementCount()` and `getRetainRecordCounts()`, which stay bounded by the elements alive.
- `setInlineDraws(false)` with `drawFrame()` is a frame the way the display link ends one; `setA11yActive(false)` removes the pretend screen reader.

### The test clock

`clockFastForward(ms)` moves a paused clock and notifies the window, so the frame after it is built whole. `clockAdvance(ms)` moves it without telling the window, as time passes in a live one, so the frame after it is a tick, which patches when only paint-only animations ran. Use `clockAdvance` with `setInlineDraws(false)` and `drawFrame()` to measure or test a tick.

## The retention verifier

`Window::set_retention_verifier` builds every frame that replayed once more with every subtree built, and compares per replayed root the scene primitives, hitboxes, cursors, listener counts, tab stops and accessibility nodes, plus whatever text the embedder's `describe` returns (gpuix returns its tapes, and the handles a recording registered). Both sides of a root are put in one form first: draw orders, and groups and masks that hold nothing, are stripped (including the widenings in the text of a lifted element, which sits on a line of its own).

It also compares the frame as a whole (the order of the scene, the hitboxes in hit-test order, the tab order and cursors, the dispatch tree and the accessibility tree), because the per-root comparison with the draw order stripped cannot see an error in the order between roots, which is what a hole or a fragment can get wrong. The frame shown is the one built whole.

- **Whole-frame comparison is off in gpuix**, with `Window::set_retention_whole_frame(false)`, because its build is not identical twice within a frame: `anchor-size()` reads the size the trigger painted a pass ago, so outside the subtrees that replayed the two passes differ. The per-root comparison stays on. An embedder whose build is identical twice leaves both on.
- **The reference pass takes the instant of the pass it is compared with** (`Window::is_retention_reference`); gpuix reads the clock once per pass. On a running clock the reference would be a pass ahead, and a root that something animating places would differ from its build by the distance that moved.
- **Patches are verified the same way.** With a verifier set, a patched draw is followed by a frame built whole at the instant of the patch, and the operations of each patched root are compared with the patched ones (a difference is a `[retained]` mismatch). That frame is the one shown, so the next tick patches against it.
- **It is weaker where a recording was made in a region.** When either side of a root's comparison was recorded in one, both are compared without what a build culls and nothing can hit but the region holds: the key, action, modifier and mouse listeners of the boxes between the clip and the region, groups that hold nothing, and masks that only widen the groups around them. It cannot see a range that moves across frames, which a test with the verifier off covers.
- **Its reference build leaves recordings unmovable.** A frame that only replays is built a second time and shown as that build, whose recordings are direct and cannot be moved. A scroll after an idle frame under `GPUIX_RETAIN_VERIFY=1` therefore builds the rows where it puts them (`lazyBuilds` 40 per scroll, `relocations` 0). `retained-scroll` scrolls in pairs, the verifier off for the frame that records and on for the one that moves, to count relocations under it.

Run the suites under it before landing anything that changes what a build reads, what a paint listener writes, a custom element, an exclusion, or anything gpui records: `packages/react`, `packages/solid` and `examples`, with `GPUIX_RETAIN_VERIFY_REPORT=1` to see how many frames were compared. Zero mismatches is the bar. A `[retained]` line on stderr or a failing `dispose()` is a hole in the key or the tape. It builds each replayed frame twice, so it is not for timing. The read-after-write test of `retained-replay.test.tsx` takes about 1.5 s alone under it and has a timeout of 15 s.

## Fuzzers

The gpui fuzzers run inside `cargo test -p gpui --lib --features test-support`.

- **Retained fuzz** (`elements/retained/regression_tests.rs`): random pages with every feature on, played as scripts of frames in two windows and verified (100 seeds of each scrolled fuzz, 300 of each of the others). `RETAINED_FUZZ_OFF=lift,round,lifts,toggle,nested,scale,abandon,cull` takes features away to find which one a failure needs. A failure is shrunk to the Rust of a test that plays it unless `RETAINED_FUZZ_NOSHRINK` is set, which also lets a panic through. `ITERATIONS` and `SEED` run long (about 1.7 s a seed; `SEED=$((n * 190)) ITERATIONS=190 cargo test -p gpui --lib --features test-support scrolled_` is one eighth of 1500). `RETAINED_VERIFY_DUMP=dir` writes the texts of the first frame that differs from its build, and the registries of that frame.
- **Engine fuzz** (`taffy/engine_fuzz_tests.rs`): random edit sequences laid out in an engine kept across frames and in a fresh one, every node compared ([layout.md](layout.md) describes the persistent engine). Seeds 1 to 20000 pass; the suite runs 80. `GPUI_ENGINE_FUZZ_SEEDS=first..last cargo test --release -p gpui --lib --features test-support -- --ignored a_long_run_of_edits_lays_out_like_a_new_engine` runs any range.
- **Unit tests** of holes, fragments and relocation run in the same command.
- **`retained-fuzz.test.tsx`** (JS) compares retention with a build of the same state under random interaction: cards with a motion (a div, an svg group, an inline span) below `display: contents` wrappers that is started again, ended early and hidden while it runs, and rows that lift a box in a rounded or square scroller under a root that ticks. Motions are compared mid-way and settled, in the frame the clock draws after they moved as well as after a flush, since a motion a replay left frozen shows without the verifier.

## Measuring performance

- **Use a release addon.** A leftover debug build times like a regression, and scratch apps link this checkout (see [working-on-the-fork.md](working-on-the-fork.md#building)).
- **The numbers are CPU per frame on the test renderer.** There is no display link in a test, so GPU time, presentation and the compositor are not in them, and they are not what a window shows. The comparison with Electron (idle animated pages, a hover sweep, fast scroll) is rerun by the app that uses this fork.
- **The test renderer draws a dirty window inline whenever effects flush; a window with a display link draws once.** A baseline taken this way overstates the cost of anything that flushes effects during a frame (the changelog page of the demo drew 20.7 times per frame on `poc` in a scroll). Measure a frame the way the display link ends one with `setInlineDraws(false)` and `drawFrame()`, and a tick with `clockAdvance` ([The test clock](#the-test-clock)).
- **Baselines.** `poc` is the fork before the retention work, "off" is `GPUIX_RETAIN_OFF=1` (the persistent engine and the cheaper build, nothing skipped), "perf" is everything on. The tables are in [retention.md](retention.md#measured). Use p50 over several runs (three on the demo pages, medians of 7 rounds after one warm-up for the offscreen runs); the machine is shared, so compare numbers taken together.
- **Tick timings.** CPU per draw in the headless test renderer with a 75 Hz timer (10 s a run, 2 s of settling) for loops, and in the offscreen test renderer for one-off pages. `getPatchCounters().unpatchable` and `declines`, `getBuildCounters` and `getFrameWorkCounters` say why a tick is drawn whole; pin a fix with a test that counts it again.
- **A suite under the verifier is not a timing.** It builds every replayed frame twice.

### Known failing budgets

`timeline.perf.test.tsx` (a 3259-clip pan) fails its budgets on `poc` too, before the retention work, so its failure is not a regression. With culling on, p95 is 44.7 ms on `poc` and 34.1 ms with the retention (the budget is 18 ms); with culling off it is 708 ms against 683 ms (the budget is 400 ms). A drag of a clip, p50, is 13.9 ms against 4.7 ms.
