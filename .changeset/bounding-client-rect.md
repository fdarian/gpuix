---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

`getElementBounds` is `getBoundingClientRect`: it is never stale. Queued mutations are sent and the window is laid out and painted synchronously when anything changed, so a read right after a mount or a style change answers for the tree as it is instead of returning `null` or the previous frame. On macOS this needs `Window::draw_if_dirty` from the zed fork; on Windows and Linux the UI thread draws before it answers.

React and Solid host elements gain `getBoundingClientRect()`, returning `{ x, y, width, height, top, left, right, bottom }` in window coordinates, and a zero rect for an element with no box. `getElementsAt` and the automation bounds do the same flush and draw.

`<input>` reports its border box like every other element. It reported the padding box, narrower by the border width.
