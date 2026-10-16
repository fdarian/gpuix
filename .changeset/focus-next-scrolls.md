---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

`renderer.focusNext()` and `renderer.focusPrevious()` now scroll the newly focused element into view, like Tab: every scrolling ancestor moves to the nearest edge, and only if the element is not already in view. The scroll happens in the renderer after the move, so it does not matter how the method is called. Tab no longer scrolls from JS a second time.
