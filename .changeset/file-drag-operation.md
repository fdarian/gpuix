---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

On macOS, the result of a `dragover` of files dragged in from another app reaches the OS. The drag cursor shows the `dropEffect` the handler chose (`"copy"`, `"move"` or `"link"`), and a drop that the page would not take (the `dragover` was not cancelled with `preventDefault()`, or it chose `"none"`) is refused, so the files slide back instead of being dropped on nothing. A window with no `dragover` handler now refuses file drops. The drag events of files are delivered synchronously for this, so their handlers run inside the OS drag callback.

`nativeSimulateFileDragEnter` and `nativeSimulateFileDragMove` return the operation the window answered with.
