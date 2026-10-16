---
'@gpuix/native': minor
---

CSS text decoration draws what it names. `textDecorationStyle` (and the `textDecoration` shorthand) now draws `double`, `dotted`, `dashed` and `wavy` on underline, line-through and overline alike, `overline` is painted, and `textDecorationThickness` (`auto`, `from-font` or a length, also as the shorthand's thickness component) sets the thickness of every line of the declaration. Before, `double`, `dotted` and `dashed` drew solid, a wavy line-through drew solid, `overline` drew nothing and the line was always 1px.
