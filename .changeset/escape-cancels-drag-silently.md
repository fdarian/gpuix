---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Escape during a `draggable` element drag cancels the drag and nothing else: the `keydown` is not sent to any element, `window` or `document` listener, so a dialog under the drag does not close on it. Chrome swallows the key the same way. Escape with no drag in progress is unchanged.
