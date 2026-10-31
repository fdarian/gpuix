# Events and input

How pointer, focus, keyboard and form input reach JS as DOM events, and how hover, press and focus state turn into styles. It also covers the inline-span side of the same machinery (listeners, cursor, focus on text spans).

## Pointer events

GPUI decides hover per element and hands gpuix one listener per node. That gave gpuix-only rules: a filled or absolute box blocked hover and clicks to its ancestors, an element with both `onMouseDown` and `onMouseMove` silently captured the pointer, `click` was synthesised from mouse-up with no press-origin check, `pointerEvents: "auto"` meant "block the wheel", and a popup could not tell whether a press was inside its trigger. There was no capture phase, no `target`, no pointer events and no drag and drop. The design replaces those rules with the DOM's.

### One hit test per pointer event

zed gets an opt-in DOM hit-testing mode per window (`Window::set_dom_hit_testing`, default off, so other GPUI users are unaffected). Each hitbox records its parent. The hit is the topmost hitbox that is not `HitboxBehavior::PassThrough`, plus its parent chain. `PassThrough` is `pointer-events: none`: never a target, but still in the chain when a descendant is hit. A deferred (anchored) layer is its own hitbox root, so hover, focus and wheel over a popup follow layout, as a portal does in `body`.

Rules that feed the hit test:

- **`pointerEvents: "none"` is inherited** (a descendant sets `auto` to restore it), and an element with `none` is never a target even if it has listeners. `auto` no longer blocks the wheel.
- **`inert` behaves like inherited `pointer-events: none`**, and a descendant's `auto` does not undo it. A pass-through or inert scroller never claims the wheel, including a latched gesture.
- **`clipPath` clips hit testing** (zed `Hitbox.clip`): a point outside the clip falls through to what is below, and the clip cuts the element's subtree with it. See [paint-and-transforms.md](paint-and-transforms.md) for the clip itself.
- **`borderRadius` cuts the hit area of the box itself** (zed `Hitbox.corner_radii`), whatever its `overflow`: a pointer over a rounded-away corner reaches what is below, as in a browser. A child covering the corner is still hit through its own box.
- **Text with no element of its own has no hitbox**; the element that holds it is the target. Spans with listeners are the exception, see [inline spans](#inline-span-listeners-cursor-and-focus).

`clipPath` and `borderRadius` apply to every host element whose hitbox comes from a styled box. `<virtual-list>` inserts its own shaped hitbox, and its scrollbar sits inside the clip. A hitbox a custom element inserts itself with `Window::insert_hitbox` has no clip and square corners; use `insert_hitbox_shaped` to get both.

### The dispatcher

A native dispatcher in `packages/native/src/events/` owns the events. One set of window-level GPUI listeners per view turns each input into a path (target first, then logical ancestors; an anchored layer continues to its tree parent) and emits DOM events. Per-view `PointerState` (`events/state.rs`) holds the hovered and active chains, press targets, click count and pointer capture.

A type is emitted only if some element on the path, or `window` or `document`, listens to it. JS keeps the window and document listeners, so it tells native which types they hear (`syncWindowEventListeners`). A pointer move over bare background, or over no element at all inside the window, therefore reaches them (the second as an event with no target). A `window` or `document` listener hears every event of its type, not only those an element on the path listens for. A new event type therefore needs its prop in `packages/native/js/events/event-props.ts`, which is what sets the native listener; without it the type is never emitted.

Every element needs a host-derived GPUI id (`events::ids::host_element_id`, `host_id_of`; the parts inside a native element take a `__gpuix_<kind>` id built from it). gpui keys `InteractiveElementState` by `GlobalElementId`, so an element without an `.id(..)` silently loses its hover state, its scroll offset, its accessibility node and every state gpui keeps for it. `<img>` once had none, which is why an animated GIF never left frame zero: `ImgState` holds the frame index.

JS propagation is `dispatchDomEvent` in `packages/native/js/events/dispatch.ts`: capture, target, bubble along `payload.path`, with `stopPropagation`, `stopImmediatePropagation`, `preventDefault` and `on<Event>Capture` props. Native sends element ids; JS resolves them through the adapter's element source, so `target`, `currentTarget` and `relatedTarget` are the objects a `ref` gives (`null` for an event aimed at the window). `path` is `composedPath()`, `elementId` keeps the numeric id, and window-level callbacks get the same resolution. The raw `onEvent` tap keeps ids.

Handler errors go to `onError` if the renderer has one, else `reportError`, for element, window and document listeners alike. Dispatch continues after a throw, and simulate calls no longer rethrow.

### Events and payload

- Pointer and mouse events (pointer first). `over`/`out` bubble; `enter`/`leave` fire per element in DOM order.
- `click` fires on the nearest common ancestor of the press and release targets, with `auxclick`, `dblclick`, `detail` and `contextmenu`.
- `wheel`. `onWheel` and `onWheelCapture` are pointer-event props (`HostWheelEvent`: the payload fields plus `deltaX/Y` and `deltaMode`). The wheel goes only to scrollers that are ancestors of the target, so a modal or popup no longer scrolls the container it was declared in. Routing and latching are in [scroll.md](scroll.md).
- `mousedownoutside` fires for every listener whose logical subtree does not contain the press target. Popups and submenus in anchored layers count as inside their trigger.
- Payload: `target`, `currentTarget`, `relatedTarget` (elements), `composedPath()`, `elementId`, `clientX/Y`, `offsetX/Y` (from the target's padding edge, set by the dispatcher in `dispatcher.rs`), `button`, `buttons`, modifiers, `detail`, `pointerId`, `pointerType`, and `movementX/Y` (the motion on a move, 0 on every other event).
- A `pointerdown` a handler cancelled suppresses the press's compatibility events: its `mousedown`, the `mousemove`s while it is held and its `mouseup`. `click` still fires, as in the DOM.
- Disabled controls get no `click`, `auxclick` or `dblclick` (HTML), and `element.click()` and accessibility clicks skip them too. Pointer and mouse move events still reach them. See [state styles](#state-styles-from-pointer-and-focus) for how a control counts as disabled.
- Clicks from the keyboard or VoiceOver are DOM clicks with `detail` 0, `pointerId` -1 and `pointerType` `""`.
- Transition and animation events go through this dispatcher with `target` and `path`, and bubble (they are not cancelable). Native emits the lowercase DOM names (`transitionrun`, `animationend`, ...) and each has an `on…Capture` twin. See [motion.md](motion.md).

### Synchronous dispatch

A press, a wheel and the file-drag events reach JS synchronously. On macOS and in the test renderer, `mousedown`/`pointerdown`, `mouseup`/`pointerup`/`click` and `wheel` run in the capture phase through `events/sync_dispatch.rs`, as key handlers do. This is how an event handler gets to veto GPUI's native defaults:

- `preventDefault` on a `mousedown` cancels the focus default and the input editor's focus (it calls `window.prevent_default()`). `pointerdown` does not, as in Chrome.
- `preventDefault` on a wheel cancels that input's scroll.

The input does not lease the root view (GPUI's input callback is `AnyWindowHandle::update`, and the test renderer dispatches the same way), so every renderer call works from a handler and reads current values, and `document.activeElement` is the focused element. Only a call that starts another input or drives the test app (`simulate*`, `advanceTime`, `captureScreenshot`) cannot run from a handler. A new napi method goes through `sync_dispatch::route` and the `update_*` helpers. See [frames-and-wakes.md](frames-and-wakes.md) for how JS is woken on the other path.

### Pointer capture

Capture is explicit: `setPointerCapture` / `releasePointerCapture` / `hasPointerCapture` with `gotpointercapture` and `lostpointercapture`. Implicit capture is gone. A call is in force when it returns (`PointerRequests`): the next pointer input reads it and fires `gotpointercapture` first. `cancelDrag` works the same way.

`Reorder` (React and Solid) calls `setPointerCapture` on `pointerdown` and ends the drag on `pointerup`, `pointercancel` or `lostpointercapture`. On macOS the press is dispatched synchronously, so no move is lost.

### Drag and drop

File drags fire `dragenter`/`dragover`/`dragleave`/`drop` with `dataTransfer.files` (`path`, `name`, `size`, `type`, `lastModified`). `draggable` elements start a session after 4 px of movement and fire `dragstart`, `drag`, `dragend` and in-app drops through the same code. `dataTransfer` is a JS object per session. During a drag no pointer or mouse events fire, as in the DOM. Escape during an element drag cancels it and reaches no `keydown` listener (`events/keyboard.rs` does not send it), as in Chrome.

On macOS the OS drop operation of dragged-in files follows `dragover`. A `dragover` that calls `preventDefault` with a `dropEffect` of `copy`, `move` or `link` sets the drag cursor through `Window::set_drag_operation` (a zed commit; `gpui_macos` answers `draggingEntered` and `draggingUpdated` with it, within what the source allows). Anything else refuses the drop at the OS, and a window with no `dragover` listener refuses file drops. The file-drag events are delivered synchronously for this, from inside the OS drag callback.

### `cursor: "none"` and pointer lock

`cursor: "none"` and `requestPointerLock` / `exitPointerLock` (macOS) are backed by zed commits for `CursorStyle::None` and pointer lock. The lock is taken in the call, from an input handler too (`GpuixRenderer::settle_pointer_lock`), so `pointerLockElement` is current and the next pointer event is aimed at the lock element. `pointerlockchange` and `pointerlockerror` are events at `document` (`event.target` is `document`, they bubble to `window`, the element is `document.pointerLockElement`).

`cursor: none` hides the cursor per window (`CursorHide` in `gpui_macos`). It ends when the pointer leaves the window, the window stops being key or it closes.

## Focus

- **Order and events.** DOM order: `blur`, `focusout`, `focus`, `focusin`. `focusin`/`focusout` bubble and carry `relatedTarget`.
- **`:focus-visible`** uses the browser's keyboard-modality heuristic and is readable with `matches(':focus-visible')`. A `focusWithin` state style exists too.
- **`tabIndex` as in the DOM.** `-1` is focusable by script only, positive values follow DOM order. A listener for a key or focus event no longer makes an element focusable; give it a `tabIndex`. A disabled input is never focusable.
- **`focus({ preventScroll })` and `blur()`.** `focusNext()` and `focusPrevious()` scroll the new focus into view with `nearest`, as Tab does.
- **Timing.** The focus events of a press, of `autoFocus` and of a frame reach JS before the call that caused them returns (`settle_focus`, macOS and the test renderer), so `document.activeElement` is current in the next handler. `autoFocus` focuses in the commit that mounts the element.

### Focus on mousedown

The focus default (`focus.rs`, `defer_mouse_down_default`) uses the dispatcher's target path, not the topmost hit, and blurs when nothing on the path is focusable. A `preventDefault` from any `mousedown` handler on the path cancels both the focus move and the blur, including an input editor's own focus; `pointerdown` does not cancel it, as in Chrome. The order is `pointerdown`, `mousedown`, `blur`, `focusout`, `focus`, `focusin`, pinned in `focus.test.tsx`. There is no second mousedown emission.

Because `autoFocus` focuses in the commit, a `mousedown` handler that mounts an `autoFocus` popup must call `preventDefault()` to keep focus there; the press takes focus after the handlers, as on the web.

### The keyboard of a `<webview>`

A `<webview>` is focusable like a text control, and its page holds the keyboard exactly while the element is focused: `observe_focus_in` queues a `PageMove`, `settle_focus` moves the first responder before the focus events reach JS, and Tab and Shift+Tab enter the page at its first or last tabbable element and leave it when WebKit runs out. A press on the page reaches GPUI first (through the window's `sendEvent:`), so it takes focus like a press on any element. While the page has the keyboard, GPUI's key dispatch does not run. How each piece works, with the deviations (a page with nothing tabbable takes one stop; keys do not reach the parent) is in [webview.md](webview.md#focus).

### Label activation

`<label htmlFor>` names and activates its control. After a `click` whose path contains a label (and whose target is not inside the labelled control or other interactive content in the label), if it was not default-prevented, gpuix focuses the control and dispatches a `click` on it. A pointer release, `element.click()` and the VoiceOver Press all run it (`events/activation.rs`). The release goes through the synchronous dispatch from a deferred step, so queued focus handlers run first. A disabled control gets no activation.

## Keyboard

- `event.key` and `event.code` are DOM values (`"Enter"`, `" "`, `"ArrowLeft"`), with `repeat` and modifier fields (zed reports key auto-repeat).
- On macOS `code` comes from the virtual key code (`NativeKey::key_code` through Chromium's table in `events/key_codes.rs`) and `key` is the layout's character with shift and option applied (`NativeKey::layout_char`), as in Chrome: `"ß"` for Option+s, `code` `"KeyC"` for the C position on a Cyrillic layout, `"NumpadEnter"`.
- `keydown` and `keyup` also carry the legacy `keyCode` and `which` with Chromium's macOS values (NumLock is 12, an input method's `Process` 229). There is no `keypress` and no `charCode`.
- An input method composing in an `<input>` or `<textarea>` gives `isComposing`, a `Process` `keydown` for a key it takes first (observe-only: an `ImeKeyDown` that `preventDefault` cannot stop) and `compositionstart`/`compositionupdate`/`compositionend`.
- Each keystroke is one `keydown` that goes through capture and bubble. Adapters export `window` and `document` objects for window-level listeners; no globals are installed.
- In text editors, `keydown` runs before the built-in action, and `preventDefault` cancels both the action and the text insertion.

## Inputs and forms

- `<input>` has `select()`, `setSelectionRange`, `selectionStart/End/Direction`, `disabled`, `type`, `maxLength` and number `min/max/step`.
- `<form onSubmit>`, `name`, `required`, Enter to submit, `requestSubmit()` and `invalid` events.
- The per-edit event is `onChange` with `inputType`, `data` and `isComposing`, once per edit like the DOM `input`. A composition's last `change` comes before `compositionend`.
- In a textarea, Enter always inserts a newline.

## Accessibility

Every `aria-*` attribute is accepted and mapped to AccessKit where a mapping exists. `id` replaces `aria-id`. Relations resolve by author id (a zed commit). `<label htmlFor>` names its control (activation is above), and `inert` is supported.

The test renderer's tree dump reports a descendant of an `aria-hidden` or `inert` node as hidden (a zed commit: AccessKit's flag covers descendants, the dump read only the node that carries it). A referenced element with no role becomes a `group` node so `aria-labelledby` can name it from its text.

## Platform APIs

- `navigator.clipboard.writeText`/`readText`, attached to an existing `navigator` and never creating one. `@ghia/native` declares the global `Clipboard` type once for both adapters.
- File picker `accept` filters (a zed commit).
- `EyeDropper` on macOS.

The Solid `DialogPortal` used to build its children twice; that is fixed, along with the same pattern in the floating components.

## State styles from pointer and focus

State styles read the trackers above; they do not probe the tree themselves.

- **`:hover` and `:active`.** `:hover` applies to the target and every ancestor, `:active` to the primary press chain. `resolve_state_styles` (`events/state_styles.rs`) folds hover and active into the base style before motion runs, reading `PointerState` once. A hovered `color` reaches descendants' `currentColor`. There was a `StateSource` probe; it sat above an element's hitbox and hid it from hit testing, so it is gone. `overlay_fields!` in `events/state_styles.rs` lists every `StyleDesc` field that takes part in state styles.
- **Group state.** `group` with `groupHover`, `groupActive` and `groupFocusWithin`. `groupHover`/`groupActive` apply when the nearest ancestor with that `group` name is hovered or active. A group applies to descendants only, and the nearest same name wins. The group's focused-within state is read from the focus-within path (`FocusStyles::within`), the source `focusWithin` uses. GPUI's `.group()` and `.group_hover()` are no longer called on host elements. There is no group state inside an inert subtree.
- **`:has()` is relational state, not a selector.** `hasHover`, `hasActive`, `hasFocus` and `hasFocusVisible` are nested style objects like `hover`, true while a strict descendant (logical tree, as the hover chain and `focusWithin` use) is hovered, pressed, focused or `:focus-visible`. `hasDisabled` and `hasEnabled` are the other two (below). Each is read from the chain gpuix already keeps (the hover chain and the active chain minus their target, the focus-within set minus the focused element, the focus-visible element's strict ancestors), so it costs O(1) per element and no tree walk. The restyle gating that decides which elements a hover or focus change restyles counts the `has*` keys. Transitions, animations, `<UniversalStyle>`, `motion`, inert gating and inline spans treat them like their plain counterparts.
- **`:disabled` / `:enabled` and `hasDisabled` / `hasEnabled`.** The form controls are `<input>`, `<textarea>` and the `div`s that stand in for a `<button>` or `<select>` (`Button`, `buttonProps` users such as the dialog triggers, `SelectTrigger`, the `Select` root, `ComboboxTrigger`). Those `div`s say so with the internal `formControl` prop (`"enabled"` | `"disabled"`, forwarded for every host type), because `disabled` itself is not forwarded for a `div`, so a plain `<div disabled>` matches neither, as in React DOM. `RetainedElement::control_disabled` is the one reader, and `in_disabled_form_control` (click suppression) uses it too, so a disabled `Button` receives no `click()` or VoiceOver press. The strict ancestors of the controls are worked out the first time a style that declares `hasDisabled` or `hasEnabled` asks in a frame (`ControlStyles`, from the tree's `control_ids`, walking `parent`), not every frame. Unlike `hover` they are not pointer state: an `inert` subtree keeps them.

### Layer order

State layers fold in the order base, `groupHover`, `hasHover`, `hover`, `groupActive`, `hasActive`, `active`, `hasEnabled`, `hasDisabled`, `enabled`, `disabled`, `groupFocusWithin`, `hasFocus`. `focusWithin`, `hasFocusVisible` and `focusVisible` are applied to the element after that fold, so focus layers beat the pointer ones, `groupFocusWithin` included. An element's own state beats what its descendants cause, and that beats a group. The four disabled layers sit after `active` and before the focus ones, ordered as Tailwind orders its variants, so `disabled` wins a field over `hover`/`active` while the control still matches them, as in CSS.

A state style's shorthand resets the longhands it covers; that rule is in [paint-and-transforms.md](paint-and-transforms.md).

### Retention

The body of every state style is in the revision and `StyleDesc` equality, so editing one rebuilds. A hover on the element is a pointer delta. `:disabled`, `:enabled`, `:has(:disabled)` and `:has(:enabled)` read the tree (`RetainedElement::control_disabled`, `ControlStyles`, worked out once per frame). A flip is a prop mutation, which moves the revision of the control and of every ancestor, which are exactly the elements `:has()` can name, so there is no delta. Their `PointerStates` flags have bits in `pointer_bits`, which only attributes a rebuild in the counters. See [retention.md](retention.md).

## Inline span listeners, cursor and focus

A run with listeners or `hover`/`active`/`groupHover`/`groupActive` styles stays inline. The pointer is hit-tested by glyph (`text/inline_hit.rs`, which keeps each painted paragraph's `TextLayout`), and `events/inline_target.rs` puts the inline boxes under the glyph in front of the container's hit chain, so the span is the event target and the dispatcher needs no change. `run_styles` folds the pointer state into the run.

The hit registry and the painted text, start regions, highlights and match ordinals are keyed by window (`WindowId`), so a second window's frame reset keeps the first one's entries.

- `cursor` is a hitbox per fragment (`inline_cursor.rs`).
- `tabIndex` is a zero-size anchor div under the span's host id that tracks its focus handle (`inline_focus.rs`).
- `focusVisible` and `focusWithin` fold into the run and ring the fragments with `outline*` (`inline_decoration.rs`).

Span layout, bounds and decorations are in [text.md](text.md).

## Breaking changes

Against upstream gpuix.

Pointer events:

- A filled or positioned element no longer blocks hover or clicks to its ancestors. `click`, `mousedown`, `mousemove` and `mouseup` bubble, so code that relied on occlusion needs `stopPropagation`. The anchored `occlude` prop is removed; use `pointerEvents`.
- `pointerEvents: "none"` is inherited and `auto` no longer blocks the wheel.
- The wheel goes only to scrollers that are ancestors of the target, so a modal or popup no longer scrolls the container it was declared in. `onWheel` and `onWheelCapture` are pointer-event props.
- Implicit capture (a node with `onMouseDown` and `onMouseMove`) is removed. Call `setPointerCapture`. `createPointerCaptureMethods` is renamed `createElementMethods`.
- `onFileDrop` and the `paths` field are removed. Use `onDrop` with `dataTransfer.files`.
- Event type strings are lowercase DOM names (`mousemove`, not `mouseMove`).
- `pointerlockchange` and `pointerlockerror` are `document` events; the root `onPointerLockChange` and `onPointerLockError` are removed. `movementX/Y` are required fields of `HostMouseEvent` (0 on every event but a move), and `HostMouseMoveEvent` and `HostPointerMoveEvent` are removed.
- Escape during a `draggable` element drag no longer reaches `keydown` listeners or closes a dialog.
- On macOS a window with no `dragover` listener refuses files dragged in, and a `dragover` that does not call `preventDefault` refuses the drop.
- Clicks from the keyboard or VoiceOver are DOM clicks with `detail` 0, `pointerId` -1 and `pointerType` `""`. `buttonProps(behavior, renderer)` takes the renderer.
- A handler that throws no longer ends the dispatch, and nothing rethrows into the caller of a simulate method. Errors reach `onError`.
- `target`, `currentTarget` and `relatedTarget` are the elements a `ref` gives, not numeric ids; `path` is `composedPath()`, `elementId` keeps the id, and `target` is `null` for an event aimed at the window. A handler type is `DomEvent` / `HostMouseEvent`.
- A `window` or `document` listener hears every event of its type, not only those an element on the path listens for.
- A `mousedown`/`pointerdown`, `mouseup`/`pointerup`/`click` or `wheel` handler runs inside the synchronous dispatch on macOS (and in the test renderer).
- Disabled controls get no `click`/`auxclick`/`dblclick`.
- A press whose `pointerdown` was cancelled sends no `mousedown`, `mousemove` or `mouseup`; code that cancels `pointerdown` and still listens for `mousedown` stops hearing it.

Focus, keyboard and forms:

- Key, `onFocus` and `onBlur` listeners no longer make an element focusable; give it a `tabIndex`. A disabled input is never focusable.
- `event.key` uses DOM names (`"Enter"`, `" "`, `"ArrowLeft"`).
- `windowKeyDown`, `windowKeyUp` and `keystrokeId` are removed; one `keydown` per keystroke propagates.
- React `onFocus`/`onBlur` bubble, as in React DOM. Solid's do not, as on the web.
- `onSubmit` is removed from `<input>` and `<textarea>`; use `onKeyDown` or wrap the input in a `<form>`. In a textarea, Enter always inserts a newline. `TextareaProps` no longer extends `InputProps`.
- `aria-id` is replaced by `id`.
- Positive `tabIndex` follows DOM order instead of GPUI's raw ordering.
- Mousedown on content with no focusable ancestor now blurs the focused element.
- `autoFocus` focuses in the commit, not on the next frame, so a `mousedown` handler that mounts an `autoFocus` popup must call `preventDefault()` to keep focus there.
- On macOS `event.code` is the physical key and `event.key` the layout's character. `keydown` can carry key `"Process"`, and `isComposing` is true while text is marked.

## Upstreaming

Pointer events. The zed side is opt-in and independent: the DOM hit-testing mode, `CursorStyle::None`, pointer lock, the per-window cursor hide, the drag operation of dragged-in files (`Window::set_drag_operation`), `active_drag_value` and the pass-through hitbox can go to `remorses/zed` as separate commits. The gpuix side removes behaviour upstream apps depend on (occlusion, implicit capture, `onFileDrop`), so it is a major-version discussion, and `remorses/gpuix` wants an approved issue first.

Focus, keyboard and forms. The zed commits are small and independent (a11y relations by author id, `PathPromptOptions` accept filters, key auto-repeat on `KeystrokeEvent`, `Entity::is_updating`, the `NativeKey` (virtual key code and layout character) and `is_composing` of key events, the observe-only `ImeKeyDown` and `Window`'s input-method driving calls) and can go to `remorses/zed` one by one. The gpuix side depends on `dispatchDomEvent` and `sync_dispatch`, so it cannot land before the pointer-event work.

## Tests

- `focus.test.tsx` pins the focus order on a press.
- The keyboard handoff of a `<webview>` is pinned in the `webview.test.tsx` files of the adapters (scripted pages) and on real pages by the e2e ([testing.md](testing.md#the-webview-e2e)).
- `offsetX`/`offsetY` are tested through the React and Solid handlers, on a bordered target and a nested one (#40).

The Chrome oracles and test-renderer commands are in [testing.md](testing.md).

Deviations from Chrome and the DOM (asynchronous dispatch off macOS, input methods, drag images, pointer lock platforms, `:has()` limits) are in [deviations.md](deviations.md#events-and-input).
