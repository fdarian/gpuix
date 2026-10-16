---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Breaking: the renderer's scroll helpers take and return DOM scroll positions. `scrollTo(id, left, top)` and `getScrollOffset(id)` on the test renderer, the live renderer, the browser renderer and the automation protocol use `scrollLeft` / `scrollTop` (zero or more, a larger `top` is further down), the numbers `element.scrollTop` and the `onScroll` event report. They used GPUI's offset, where scrolling down was a more negative `y`, so a call that passed `0, -100` now passes `0, 100`, and a read that negated the result stops negating it. The sign flips in one place in native (`scroll::gpui_offset` and `scroll::dom_position`).
