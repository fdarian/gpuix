---
'@gpuix/native': minor
---

`borderStyle` takes the whole CSS `<line-style>` grammar and is per side. `dotted`, `double`, `groove`, `ridge`, `inset` and `outset` are drawn (Chrome's colour shading and proportions), `none` and `hidden` take the side's width, `borderStyle` takes one to four values, and `borderTopStyle`, `borderRightStyle`, `borderBottomStyle` and `borderLeftStyle` set one side. The `border` and `borderTop` ... shorthands accept every style too, and a state style's `border` shorthand resets the style longhands it covers. Before, only `solid` and `dashed` were accepted, a `dashed` anywhere made every side dashed, and other styles made a shorthand invalid.
