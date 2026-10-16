---
'@gpuix/native': patch
'@gpuix/react': patch
'@gpuix/solid': patch
---

`zIndex` orders a positioned element in its stacking context, as in CSS, not only among its siblings. A `zIndex` on a descendant of a card now reaches over the card's later siblings. The window root, a positioned element with a `zIndex` (not `auto`), `position: fixed` and `sticky`, and anything with `opacity` below 1, a `filter`, `backdropFilter`, `maskImage` or `clipPath` form a context, and a `zIndex` inside one cannot rise above what is outside it. An `<anchored>` layer and each `<virtual-list>` row are contexts of their own, and nothing in a row rises above the next row. Negative `zIndex` paints above the context's background and below its in-flow content. Tab order stays in document order. `getElementsAt` lists elements in the order they are painted and hit. A `zIndex` on a static box still does nothing, including on a flex or grid item.
