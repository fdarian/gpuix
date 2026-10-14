---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

**Breaking:** an element that listens for both `onMouseDown` and `onMouseMove` no longer captures the pointer when it is pressed. Move and up events now stop at the element's edge, as in the DOM, and a child's `click` is no longer swallowed by a parent that listens for both. To keep a drag going outside the element, call `ref.current.setPointerCapture(event.pointerId)` in its `onPointerDown` or `onMouseDown` handler. The capture ends after `pointerup`.

Pointer capture follows the Pointer Events spec more closely. Releasing a capture fires `pointerup`, then `lostpointercapture`, then `mouseup`, then `click`. `releasePointerCapture` on an element that is not in the tree does nothing and `hasPointerCapture` returns `false`; only `setPointerCapture` throws there. An unknown `pointerId` still throws from all three.
