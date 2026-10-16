---
'@gpuix/native': minor
---

`textDecorationThickness` (and the thickness of the `textDecoration` shorthand) takes `em`, `rem`, the viewport units, `calc()` and a percentage of `1em`, not only absolute units. It resolves at the element that declares it, with that element's font size, and a descendant inherits the resolved thickness.
