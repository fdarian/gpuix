---
'@gpuix/native': patch
'@gpuix/react': patch
'@gpuix/solid': patch
---

Focus follows the DOM more closely. A disabled `<input>` or `<textarea>` is never focusable, even with a `tabIndex`. `focus()` on an existing element that is not focusable does nothing instead of focusing it when it later becomes focusable. A mouse press targets only the topmost element under the pointer, so clicking a modal backdrop no longer focuses an input beneath it. An `autoFocus` or `focus()` issued while the window is inactive stays a script focus until the window comes back, and a keyboard-focused element keeps `:focus-visible` across the window deactivating and reactivating. Tab is only kept from GPUI when the Tab default will run: with ctrl, alt or cmd held, or no root attached, it passes through. The test renderer gains `setWindowActive(active)`.
