---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Scrolling follows the DOM.

- A scroller scrolls by its descendants' scrollable overflow plus the padding and border after it, once: a padded scroller no longer scrolls twice its padding too far, content that fits never scrolls, and the painted scrollbar no longer counts as content. `overflow` never changes `display` or wraps children, so a wide child of an `overflowX` scroller overflows it as in CSS. `"auto"` scrolls like `"scroll"`; `"hidden"` is a scroll container that only code scrolls, and `"clip"` only clips.
- One wheel event goes to the innermost scroller under the pointer that can use any of it, which takes it whole; a scroller at its limit passes the event up, and the next gesture chains to the scroller around it. A trackpad gesture and its momentum stay on the scroller that took its first delta. `overscrollBehavior` (`"contain"`, `"none"`) keeps the delta in a scroller at its limit. `<virtual-list>` takes part too.
- **Breaking:** `onScroll` is now the DOM `scroll`, fired whenever the offset changes with `{ scrollTop, scrollLeft }`, and replaces `onScrollChange`. The wheel event is `onWheel`, with DOM-signed deltas (`deltaY` is positive for down; it was negative), `deltaMode`, `clientX` and `clientY`.
- Host elements get `scrollTop`, `scrollLeft`, `scrollWidth`, `scrollHeight`, `clientWidth`, `clientHeight`, `scrollTo` and `scrollBy`.
- `scrollIntoView({ block, inline, behavior })` works for any descendant and scrolls every scrollable ancestor. It defaults to `block: "start"` where it used to scroll the minimum.
- Scroll reads are never stale, like `getBoundingClientRect`: `scrollTop`, `scrollHeight`, `clientHeight` and the rest, `getScrollMetrics`, and the alignment of `scrollIntoView` send queued mutations and draw any pending change first, so a read right after content changed in the same turn answers for the tree as it is.
- `scrollBehavior: "smooth"` animates `scrollTo` and `scrollIntoView` calls that name no `behavior`.
- Arrow keys, PageUp, PageDown, Home, End and Space scroll the focused element's scrollable ancestor unless a handler called `preventDefault()`. The built-in Select and Combobox now do.
