---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Pointer, mouse and drag handlers use DOM event types and propagate like the DOM. `onClick`, `onAuxClick`, `onDoubleClick`, `onContextMenu`, the `onMouse*`, `onPointer*` and `onDrag*` handlers and `onDrop` listen for the lowercase DOM types (`click`, `dblclick`, `pointerdown`, `drop`, ...), and each has an `on…Capture` variant that runs from the root down to the target before the bubble handlers. Handlers receive a DOM-shaped event with `target`, `currentTarget`, `eventPhase`, `stopPropagation()`, `stopImmediatePropagation()` and `preventDefault()`. Native is asked to emit a type while either its bubble or its capture handler is present.

`onFileDrop` is removed: a file drop is a `drop` event, so use `onDrop` and read `event.dataTransfer.files`. `onMouseDownOutside` keeps its gpuix type and has no capture variant. `event.eventType` of these events is now lowercase.
