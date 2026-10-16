---
'@gpuix/native': patch
'@gpuix/react': patch
'@gpuix/solid': patch
---

An `opacity`, `filter`, `maskImage` or `clipPath` group no longer cuts an escaped `absolute` or `fixed` descendant to the clip it started in. A dropdown inside a card with `opacity` below 1 that sits in an `overflow: hidden` scroller is now drawn past the scroller's edge, as in CSS, instead of being clipped by it. With a `filter`, what the filter spreads of the card itself also reaches past that edge.
