---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Scroll containers and `<virtual-list>` get macOS-style overlay scrollbars by default: hidden at rest, shown while the offset changes, faded out after about a second, thicker with a faint track under the pointer. The thumb drags and the track pages toward the pointer. Painted by `paint_overlay`, so a bar never changes what a container can scroll.

**Breaking:** `scrollbarWidth` is now CSS (`"auto"`, `"thin"`, `"none"`) instead of a number of pixels, and `scrollbarColor` is `"auto"` or `"<thumb> <track>"` instead of one colour that switched the bar on. Use `scrollbarWidth: "none"` for a container with no bar.
