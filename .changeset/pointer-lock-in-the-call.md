---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

`requestPointerLock()` and `exitPointerLock()` take effect in the call on macOS, from an input handler too: `renderer.pointerLockElement()` is the element when `requestPointerLock()` returns and the next pointer event is already aimed at it. (`setPointerCapture`, `releasePointerCapture` and `cancelDrag` already were in force when they returned; the README now says so.)

`pointerlockchange` and `pointerlockerror` are events at `document`: listen with `document.addEventListener('pointerlockchange', ...)`. They bubble to `window`, are not cancelable, and their `target` is `document`, as in the DOM; the element is `document.pointerLockElement`, like `document.activeElement`, and an error names none. The root's `onPointerLockChange` and `onPointerLockError` are removed: listen at `document` instead.

`movementX` and `movementY` are `0` on every mouse, pointer, wheel and drag event but `mousemove` and `pointermove`, as in the DOM; they were missing. They are now required fields of `HostMouseEvent`, and `HostMouseMoveEvent` and `HostPointerMoveEvent` are removed (use `HostMouseEvent` and `HostPointerEvent`).
