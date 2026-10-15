---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Files dragged in from the OS are DOM drag events. `onDragEnter`, `onDragOver` and `onDragLeave` fire along the path under the dragged files, and `onDrop` delivers `event.dataTransfer.files`: entries with `path`, `name`, `size`, `type` (guessed from the extension) and `lastModified`. As in a browser, a drag fires no pointer or mouse events, and `drop` is delivered only if the last `dragover` was cancelled with `preventDefault()`; otherwise the target gets `dragleave`. The `paths` field of `onDrop` is removed: read `event.dataTransfer.files` and map to `path`.

The test renderer can send the steps of a file drag separately: `nativeSimulateFileDragEnter`, `nativeSimulateFileDragMove`, `nativeSimulateFileDragSubmit`, `nativeSimulateFileDragExit` and `nativeSimulateFileDragEnd`. `nativeSimulateFileDrop` still does a whole drop.
