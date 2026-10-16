---
'@gpuix/native': minor
---

`clipPath` supports `path()`, `rect()` and `xywh()`. `path(evenodd, "M0 0 H100 ...")` clips to SVG path data (curves and arcs included, in px from the top left of the reference box) with an optional fill rule, `rect(top right bottom left)` measures every edge from the top left of the box and takes `auto`, and `xywh(x y width height)` is a position and a size. Both take `round` radii. A path needing more than 4096 vertices is flattened more coarsely and rejected if it still does not fit.
