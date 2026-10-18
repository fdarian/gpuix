---
'@gpuix/native': patch
'@gpuix/react': patch
'@gpuix/solid': patch
---

A `display: contents` element is no longer focusable (it has no box, as in Chrome), and as the root or a `<virtual-list>` row it keeps its `role` and `aria-*` in the accessibility tree around its children. The README documents how a contents row is measured, and tests check its listeners, reads, focus and observers against Chrome.
