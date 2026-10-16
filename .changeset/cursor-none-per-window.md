---
'@gpuix/native': patch
'@gpuix/react': patch
'@gpuix/solid': patch
---

On macOS, `cursor: 'none'` hides the cursor for its own window only. Another window closing or losing focus no longer brings the cursor back, and the cursor is shown again when the pointer leaves the window, the window stops being key or it closes.
