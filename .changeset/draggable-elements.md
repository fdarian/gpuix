---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Elements can be dragged. `draggable` makes an element a drag source: a primary press on it, or inside it, that moves more than 4px fires `onDragStart` at the element (after `pointercancel` at the press target), `onDrag` on each move, and `onDragEnter`/`onDragOver`/`onDragLeave`/`onDrop` on what is under the pointer, the same events a file drag fires. The release sends `drop` to the target, then `onDragEnd` to the source with `dataTransfer.dropEffect` set to the effect of the drop, or `"none"` if nothing was dropped. Escape cancels the drag, and so does `event.preventDefault()` in `onDragStart`. No pointer or mouse event fires during the drag, and the release is not a click.

`event.dataTransfer` now works for these drags with `setData`, `getData`, `clearData`, `types`, `effectAllowed` and `dropEffect`, in DOM protected mode: `getData` returns `""` except in `dragstart` and `drop`. `onPointerCancel` fires at drag start. There is no drag image, so `setDragImage` does nothing.

The renderer gains `cancelDrag()`, which the host calls when `dragstart` was cancelled.
