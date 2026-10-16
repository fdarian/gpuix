---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

`anchor-size(width)` and `anchor-size(height)` also resolve on `margin`, the four `margin*` sides and `top` / `right` / `bottom` / `left` of a direct child of `<anchored>`, not only on the six size properties. A layer mounted under a trigger that has already painted builds at the trigger's size in its first frame instead of one frame later.
