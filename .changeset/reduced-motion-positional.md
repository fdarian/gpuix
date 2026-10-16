---
'@gpuix/native': patch
'@gpuix/react': patch
'@gpuix/solid': patch
---

`reducedMotion` jumps only `width`, `height`, `top`, `right`, `bottom` and `left` and layout animations, as Motion for React does. Padding, margin, `gap`, min and max sizes and `flexBasis` keep animating along with opacity, colours, shadows and radii. A motion value bound to `width`, `height` or an inset still jumps under `animate()`; one bound to padding or `flexBasis` animates.
