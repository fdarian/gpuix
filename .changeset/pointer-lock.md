---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Pointer lock, shaped like the DOM's. `element.requestPointerLock()` on a host ref (and `renderer.requestPointerLock(id)`) hides the cursor and freezes it; `renderer.exitPointerLock()` releases it and `renderer.pointerLockElement()` returns the locked element. While locked, pointer and mouse events target the lock element and keep a frozen `clientX`/`clientY`. The request applies at the next frame, and the root's window-level `onPointerLockChange` and `onPointerLockError` report the outcome: a granted, moved or ended lock (including a window that lost focus or a locked element that left the tree), and a failed request (element not in the tree, or the platform refused). macOS only; other platforms always fail the request. `TestRenderer` gains `requestPointerLock`, `exitPointerLock`, `pointerLockElement`, `simulatePointerLockGranting` and `simulatePointerLockLost`.

`cursor: "none"` hides the cursor over an element (macOS; Windows hides it too but is untested, Linux keeps the default cursor).

`pointermove` and `mousemove` events carry `movementX` and `movementY`, the pointer's motion since the previous move in logical pixels, taken from the platform's relative motion (zero off macOS). They are required in the `onMouseMove` and `onPointerMove` event types (`HostMouseMoveEvent`, `HostPointerMoveEvent`). `TestRenderer.nativeSimulateMouseMove` takes the motion to report as a fifth argument.
