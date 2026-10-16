---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

The focus events of a mouse press, of `autoFocus` and of a focus change a frame notices are delivered synchronously on macOS and in the test renderer, like those of `focus()`: `blur`, `focusout`, `focus`, `focusin` reach handlers, and `document.activeElement` names the new element, before the `mousedown` default returns, inside the commit that mounts an `autoFocus` element, and before the frame returns. `autoFocus` now focuses at commit, as in React DOM, instead of on the next frame; a `mousedown` handler that mounts an `autoFocus` popup must call `preventDefault()` to keep focus there, as on the web, because the press takes focus after the handlers. Windows and Linux still queue them.
