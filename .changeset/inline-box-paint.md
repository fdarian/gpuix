---
'@gpuix/native': patch
'@gpuix/react': patch
'@gpuix/solid': patch
---

A `<span>` paints its `backgroundColor`, border and `borderRadius` around each line of its text as a CSS inline box does (`box-decoration-break: slice`): behind the glyphs, with its padding inside, and cut at the line breaks. A span is as tall as the content area of its font plus its vertical padding and border, whatever the `lineHeight`, which also sets its `getBoundingClientRect()`, what the pointer is on, its cursor and its `outline` ring (which now follows `borderRadius`). A background no longer fills the margin box.
