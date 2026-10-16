---
'@gpuix/native': patch
'@gpuix/react': patch
'@gpuix/solid': patch
---

`display: contents` as the root or as a `<virtual-list>` row generates no box: its children lay out in an anonymous block in its place, and `getElementBounds` returns `null` for it. Its padding, border, background and size no longer apply there.
