---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

`onScrollChange` reports the scroll offset of an `overflow: "scroll"` element after it moves, whatever moved it: the wheel, a scrollbar drag, `scrollTo` or `scrollIntoView`. `onScroll` still reports only wheel input. The event payload gains `scrollX` and `scrollY`.
