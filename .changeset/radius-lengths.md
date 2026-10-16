---
'@gpuix/native': minor
---

`borderRadius` and the corner longhands take every CSS length: `em`, `rem`, `vw`, `vh`, `vmin`, `vmax` and `calc()` that mixes a length and a percentage (`"calc(1em + 10%)"`). The length part resolves against the element's font size, the root's and the window when the style is applied, and a percentage stays per axis, the width across and the height down. A radius that is negative whatever the units are is an error as before; one that only comes out negative (`calc(1em - 20px)`) is zero.
