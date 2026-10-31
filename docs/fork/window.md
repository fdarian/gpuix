# Window

What an app sets on the window itself: the system appearance, bundled fonts, `appRegion` drag regions for a frameless
window, and where a window opens (`x` / `y` and the `screen` displays). All four are small, independent of the rest of
the fork, and exist because the apps this fork ports already use Electron's equivalents.

## Appearance

The `appearance` window option follows the macOS system appearance by default, with `getAppearance()`,
`setAppearance()` and an `appearanceChange` event. `light-dark()` ([paint-and-transforms.md](paint-and-transforms.md))
and `prefers-color-scheme` ([motion.md](motion.md)) read it.

The test renderer applies it too: it opens its window from the same `WindowOptions` through `to_gpui_window_options`, and
a zed commit makes `VisualTestPlatform` forward `set_window_appearance` to the real platform (the trait default was a
no-op), so a headless window follows the override and delivers the change. Conflicts live in
`packages/native/js/host.ts`, `src/renderer.rs` and `src/test_renderer.rs`.

## The `fonts` option

The `fonts` window option takes a list of font files, reads them and passes them to upstream's `registerFont` right after
the renderer is created, so `fontFamily` resolves to them. It has no Rust (`js/fonts.ts`, `js/runtime.ts`,
`js/testing.ts`). If upstream grows its own font option, drop the fonts commit. How a registered font is held in memory is
in [frames-and-wakes.md](frames-and-wakes.md#memory-footprint-of-a-window-79).

## Window drag regions (`appRegion`)

A `titlebarTransparent` window has no titlebar, and nothing in gpuix said what moves it. AppKit dragged such a window
from a fixed strip at the top of the view and held clicks in that strip back to tell a double click apart, so an app's own
bar had to sit exactly under the strip and its buttons paid the delay. GPUI had the parts (`Window::start_window_move`,
`window_control_area(Drag)`, the `app_owns_titlebar_drag` window option) and nothing connected them to a page.
`appRegion: "drag" | "no-drag"` is Electron's `-webkit-app-region`.

`appRegion` is not a DOM or CSS property, and the standards-track `window-drag` has a different value set. It is the
exception to the DOM/CSS principle ([index.md](index.md#the-principle-dom-and-css-semantics)): a web port needs it and nothing in the DOM does the job.

### Why the press is handled natively

macOS `performWindowDragWithEvent:` needs the `NSEvent` of the mouse-down that is being dispatched, so the move has to
start inside that dispatch, and Windows only moves a window from `WM_NCHITTEST` answering `HTCAPTION`, which no JS round
trip can answer in time. A JS handler cannot do either.

### Regions are layout rectangles, not hit testing

This is Electron's model. Every box with a value contributes its border box in tree order and the last box that holds the
point wins: `drag` adds, `no-drag` cuts. Z-order, overlap, `pointerEvents`, `opacity`, `overflow` and `borderRadius` do
not take part, so an overlay with no value over a bar does not stop it dragging. The value inherits (Chromium has since
about 150, as `window-drag`, CSSWG #13101), so `drag`, a `no-drag` inside it and a `drag` inside that all work, and a
child that overflows a drag box drags.

`appRegion` is a `StyleDesc` field that inherits through `Inherited::descend`, and it is read only when the window is
frameless (`titlebarTransparent`, in a test window too). Each host element with an effective value registers its host
hitbox as a Drag area or a client area (`events/app_region.rs`, `mark`, called wherever `host_hitbox` is). An inline span
in a paragraph is covered by `text/inline_region.rs`. `app_owns_titlebar_drag` is set from the same option, so AppKit
stops dragging the top strip itself.

### The zed side

It is opt-in through DOM hit-testing mode, so other GPUI users are unchanged.

- A hitbox can be registered as a client area (`.window_client_area()`, `Window::insert_window_client_hitbox`). In DOM
  mode `Window::window_control_area_at(position)` takes the last registered entry, in paint order, whose unclipped
  hitbox bounds contain the point, and a client entry answers `None`; a fullscreen window has no areas. Outside DOM mode
  it keeps the old rule (the first area whose hitbox is under the mouse).
- The platform's own hook (what Windows asks on `WM_NCHITTEST`) calls it with the point:
  `PlatformWindow::on_hit_test_window_control`'s callback takes the position in window logical pixels (Windows converts
  the message's screen `lparam` with `ScreenToClient`), because `mouse_position` lags the hit test.
- The cursor is the arrow in a Drag area with no button held, and stays the arrow for a press that began in one. `Window`
  tracks the held buttons for that (`pressed_mouse_buttons`, private).
- `gpui_macos` `start_window_move` copies the native window and drops the state lock before calling AppKit. The call
  returns right away, but a `windowDidMove` could reach `window_did_move`, which locks the same mutex. It then clears the
  left-button bookkeeping, since Apple documents that the mouse-up of a window drag may not be sent.

### The press

The capture-phase mouse-down listener in `events/listeners.rs` asks `window_control_area_at` before the dispatcher sees
the press, and `press_outcome` (pure, per platform) decides.

- **macOS and Linux.** A left press (not ctrl on macOS, no other button down) is swallowed: no DOM event, no focus
  default, GPUI's propagation stopped. It starts the move, or on `click_count` 2 does `titlebar_double_click()` (macOS,
  which follows the user's "double-click a window's title bar" setting) or `zoom_window()` (Linux). A Linux right press
  opens the window menu. Right click and ctrl-click reach the page on macOS.
- **Windows.** gpuix asks for no action. GPUI receives the caption's `WM_NC*BUTTONDOWN` as a mouse down, and a stopped one
  makes `DefWindowProc` skip the drag, the maximise and the system menu, so the press is taken for the window
  (`PointerState::window_presses`) and never stopped. It only calls `Window::prevent_default()`, which keeps GPUI's focus,
  the active state, the input editor's caret and the text selection (`text/paint.rs` returns on a prevented mouse-down, as
  the DOM does for `preventDefault`) from running. A scrollbar skips a press inside a Drag area (Electron: the region
  covers the scrollbar).
- **The release.** `window_presses` also makes the release the window's, and until it comes a move is not the page's. A
  release that never arrives (the OS may not send one after a window move) cannot leave it stuck: a press of that button
  that reaches the page, a pointer exit or a move that reports no button held ends it.
- **The resync after a paint** (a layout that moved under a still pointer) is deferred until the draw has swapped frames,
  because `window_control_area_at` reads the rendered frame. A bar that appears under the pointer is a region from its
  first frame, and hover leaves once.

### What the page sees

No `pointerdown`, `mousedown`, `mouseup`, `click`, `dblclick`, focus change or `mousedownoutside` for a press in a region,
as in Electron. With no button held, a pointer over a Drag region is outside the window as far as the dispatcher is
concerned (`hides_pointer`): hover leaves with the usual leave events, no moves fire, and leaving the region brings hover
back. A button that went down outside the region keeps the pointer through it. The wheel is untouched.

The test renderer's offscreen window has no current mouse event to start a move from, so the actions (`move`,
`titlebarDoubleClick`, `zoom`, `showMenu`) are recorded and read with `takeSimulatedWindowActions()`. Nothing is recorded
for Windows, where the OS owns the region.

### Platform status

macOS is built and tested offscreen only: the test renderer's recorded actions, Rust unit tests for `press_outcome` and
`hides_pointer`, and zed's `gpui` tests for `window_control_area_at`. The real drag and the real double click were not
exercised in a live window. `performWindowDragWithEvent:` hands the drag to the WindowServer and returns right away
(Apple's `NSWindow.performDrag(with:)` docs: a mouse-up may not be sent; a probe here returned in about 0.1 ms), so
nothing runs nested inside GPUI's mouse dispatch, and starting the move there is what Chromium, winit, tao, Qt and Zed's
own title bar do. Whether the mouse-up arrives is unverified, and the bookkeeping reset is right either way. Windows and
Linux were written, not compiled.

## The macOS menu bar

[docs/reference.md](../reference.md#the-macos-menu-bar) describes the menus an app gets. The rules behind them are in `packages/native/src/app_menu.rs`; two are not visible from the API.

- **No Edit menu with `⌘C` / `⌘V` / `⌘X` / `⌘A`.** AppKit consumes a key equivalent before the window sees the key event, so those items would take the keystroke away from the selection listener in `text::paint` and from the per-focus clipboard handling in `custom_elements::input`. An Edit menu needs those handlers moved into GPUI actions first.
- **The system menu bar is outside the window,** so the automation API cannot see it. Verify it through the accessibility tree of the app you launched, by its pid, and not with a screenshot:
  ```bash
  osascript -e 'tell application "System Events" to tell (first process whose unix id is PID) to get name of every menu bar item of menu bar 1'
  ```

## Window placement and displays

An app can put a window on a second monitor and learn where the monitors are. Electron's answer is `x` / `y` in the window
options and `screen.getAllDisplays()` / `getPrimaryDisplay()`.

### API

- `WindowOptions.x` and `.y` (CSS px) put the window's top-left there when both are set; with one missing the window is
  centred as before. The space is global: origin at the primary display's top-left, y down, so a display to the left of or
  above it is negative.
- `screen` (`packages/native/js/screen.ts`, `./screen` in `@ghia/native`, re-exported by `@ghia/react` and
  `@ghia/solid`) lists the displays with Electron's `Display` fields (`id`, `label`, `bounds`, `workArea`, `size`,
  `workAreaSize`, `scaleFactor`, `rotation`, `internal`).
- `fullscreen` with a point goes fullscreen on that display.
- The functions are read off the native namespace, as `validateKeyframes` is: the browser entry has none, and a named
  import would fail the adapters' browser bundles.

### What it maps to in gpui

- **GPUI reads a window's origin relative to its display, not globally.** `MacWindow::open` takes
  `WindowParams.bounds.origin` against the top-left of the display named by `WindowOptions.display_id` (points, y down).
  `to_gpui_window_options` is the only place that builds `window_bounds` and `display_id`: it picks the display for (x, y)
  and passes `DisplayId` and `(x - display.x, y - display.y)`. `WindowBounds::Fullscreen` wraps the same bounds, so a
  fullscreen window opens on that display.
- **The display comes first by containment, else by distance.** The point is in a display's bounds half-open, so a right
  or bottom edge belongs to the neighbour, as in a DOM rect. A point on no display takes the nearest, measured to the rect
  and not the origin, the first of equals. The window's size is the one asked for (or the centred size); it is not
  clamped to the display.
- **zed makes `MacDisplay` public** (`gpui_macos`, one commit). `PlatformDisplay` gives only local bounds (origin 0, 0)
  and no name, scale, rotation or built-in flag, and `MacDisplay` was crate-private. `MacDisplay::screen() ->
  Option<MacScreen>` returns the display's `NSScreen` data (`localized_name`, `frame`, `visible_frame`,
  `backing_scale_factor`) with frames in global points: Cocoa's y is flipped with the height of the first `NSScreen`,
  which is what `CGDisplayBounds` is. `rotation()` is `CGDisplayRotation`, `is_builtin()` is `CGDisplayIsBuiltin`. The
  trait is unchanged, and `bounds()`, `visible_bounds()` and the window's local placement are as they were (Zed relies on
  them). `screen()` is `None` for a display AppKit has no screen for (a mirrored one); `getAllDisplays()` leaves it out
  and `getPrimaryDisplay()` throws if the primary has none.
- **It works before the window opens.** On macOS no GPUI `App` exists until `init` opens the first window, so the screen
  API cannot go through `App::displays()`. `screen.rs` reads `MacDisplay` directly, and `init_macos` reads the displays
  before it builds the window options. `init_threaded` passes none: with no displays the window is placed at (x, y) with
  no `display_id`.

Multiple displays are not exercised in CI (the tests run on whatever displays the machine has); a window opened on a
second display, a rotated one and a mirrored one were not checked live.

## Breaking changes

- A `titlebarTransparent` window no longer drags from AppKit's top strip, and clicks there are no longer held back for
  double-click detection. An app puts `appRegion: "drag"` on its bar, and `"no-drag"` on the controls inside it.
- zed: `Window::start_window_move` takes `cx` (`start_window_move(cx)`), and in DOM mode `window_control_area_at` follows
  bounds and paint order, not hit testing.

## Upstreaming

- **Appearance and fonts.** Both are small and independent of everything else.
- **Drag regions.** The zed side is opt-in and small: client areas, `window_control_area_at`, the cursor rule and the
  macOS lock fix go to `remorses/zed` as separate commits (the lock fix is a deadlock fix that stands alone;
  `start_window_move(cx)` changes a public signature). The gpuix side needs the pointer dispatcher and `PointerState`
  ([events-and-input.md](events-and-input.md)), so it goes with them.
- **Placement.** The zed side is one commit that adds exports and accessors to `gpui_macos`, with no behaviour change, and
  can go alone. The gpuix side (`screen.rs`, the two options, `to_gpui_window_options`) is independent.

## Tests

- Placement is Rust unit tests with fake displays, so any layout runs on one machine: `window_options_tests` (18) and
  `screen.rs` (10) cover a point on a second display at (1512, -200), a point on the primary, a point on no display, one
  of `x` / `y` only, fullscreen with a point, no displays, and the edges of `display_for_point`. The real displays are
  read in `screen.test` (7) and the Rust `screen` tests on macOS, which hold what must be true for any setup: the primary
  is in the list and starts at (0, 0), ids are unique, each work area lies inside its bounds, `size` is the rectangle's,
  the scale factor is at least 1, the rotation is a quarter turn, the label is not empty. zed has `gpui_macos` `display`
  (2): `screen().frame` equals `CGDisplayBounds`.
- Drag regions: see Platform status above; the window tests are listed in [testing.md](testing.md).

Deviations: [deviations.md#window](deviations.md#window).
