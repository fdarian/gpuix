---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

`<svg>` children take `pattern`, `marker` (with `markerStart` / `markerMid` / `markerEnd` and the `marker` shorthand), `filter` with every `fe*` primitive (applied with `filter="url(#id)"`), `text` with `tspan` and `textPath` (string children are the text, with the font and text-anchor props), and `image` (`data:` and `file://` URLs and paths; `blob:` and `http(s):` are dropped with a warning). Inside an `<svg>`, `<text>` is the SVG element and forwards SVG attributes; outside it is still the host text element. `motion.text`, `motion.tspan`, `motion.image`, `motion.feGaussianBlur`, `motion.feOffset`, `motion.feFlood` and `motion.feDropShadow` animate `stdDeviation`, `dx`, `dy`, `floodColor`, `floodOpacity`, `refX`, `refY`, `fontSize` and `letterSpacing` besides the existing keys.
