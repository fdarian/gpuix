---
'@gpuix/native': minor
---

`<virtual-list>` paints its own box style, which it ignored. `backgroundColor`, borders, `borderRadius`, `boxShadow`, `opacity`, `filter`, `mask` and `clipPath` now apply to the list and its rows, and a list's `borderRadius` and `clipPath` cut its hit area as they do a `div`'s, together with the list's scrollbar, so a pointer over the clipped-away part reaches what is underneath. Every other element already clipped hit testing, since its hitbox comes from a styled box.
