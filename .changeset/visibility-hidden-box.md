---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

`visibility: hidden` follows CSS. The element keeps its layout box, so `getBoundingClientRect`, `onLayout`, `ResizeObserver` and `IntersectionObserver` report it like a visible one (they reported nothing before; `display: none` is what has no box). It paints nothing of its own, `visibility` inherits, and a descendant with `visibility: visible` paints. A hidden element is not a pointer target, cannot be focused, has no accessibility node and is left out of `getElementsAt`. Needs `Window::with_visibility` from the zed fork.
