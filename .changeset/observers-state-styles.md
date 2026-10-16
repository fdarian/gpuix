---
'@gpuix/native': patch
'@gpuix/react': patch
'@gpuix/solid': patch
---

`ResizeObserver`, `IntersectionObserver` and `clientLeft` read the padding and border an element was laid out with, so a `focusVisible` or `focusWithin` style (and a percentage padding) that changes them is seen. A `ResizeObserver` reports a change of the content box when only the padding moved and the border box stayed put. Needs `Window::layout_insets` from the zed fork.
