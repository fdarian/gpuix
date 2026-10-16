---
'@gpuix/native': patch
'@gpuix/react': patch
'@gpuix/solid': patch
---

`zIndex` applies to a flex or grid item without a `position`, as in CSS: the item is lifted into the nearest stacking context and, with a `zIndex` other than `auto`, forms a context of its own. A static box that is not an item still ignores it. An element with `opacity` below 1, a `filter`, `backdropFilter`, `maskImage` or `clipPath` that is not positioned now paints, and is hit, in the 0 layer with the positioned `auto` and 0 elements, in tree order, instead of in its place in the tree, so it is over a later in-flow sibling. Layout does not change. `getElementsAt` follows the new order.
