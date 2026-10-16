---
'@gpuix/native': patch
'@gpuix/react': patch
'@gpuix/solid': patch
---

The `IntersectionObserver` root rectangle is the root element's padding box, as in the DOM, and `rootMargin` applies to that. A target that lies entirely under the root's border no longer intersects, and `rootBounds` in an entry is the padding box. An `overflow` ancestor between the target and the root clips at its padding box too, whether or not its border is painted.
