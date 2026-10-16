---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

`requestPointerLock()` and `exitPointerLock()` take effect in the call on macOS, from an input handler too: `renderer.pointerLockElement()` is the element when `requestPointerLock()` returns and the next pointer event is already aimed at it. (`setPointerCapture`, `releasePointerCapture` and `cancelDrag` already were in force when they returned; the README now says so.)

`pointerlockchange` and `pointerlockerror` are events at `document`: listen with `document.addEventListener('pointerlockchange', ...)`. They bubble to `window`, are not cancelable, and their `target` is the locked element (the element that was asked, for an error). The root's `onPointerLockChange` and `onPointerLockError` still get them. `document.pointerLockElement` is the locked element, like `document.activeElement`.

`movementX` and `movementY` are `0` on every mouse, pointer, wheel and drag event but `mousemove` and `pointermove`, as in the DOM; they were missing. They are now required fields of `HostMouseEvent`, and `HostMouseMoveEvent` and `HostPointerMoveEvent` are removed (use `HostMouseEvent` and `HostPointerEvent`).
