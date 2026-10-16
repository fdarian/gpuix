---
'@gpuix/native': patch
'@gpuix/react': patch
'@gpuix/solid': patch
---

`motion.span` animates the colours and the `opacity` of an inline span (`initial`, `animate`, `exit`, `transition`, `variants`), with `exit` working under `AnimatePresence`. A span repaints only what needs no layout, so a key of any other property throws and names it, as do `layout*`, `drag*`, `while*` and a motion value in the style. `opacity` now fades the text, lines, background, border and outline of any inline box, and takes part in a span's CSS transitions and animations.
