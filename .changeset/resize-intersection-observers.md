---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

`ResizeObserver` and `IntersectionObserver` for host elements (not `<virtual-list>`), exported from `@gpuix/native/host`, `@gpuix/react` and `@gpuix/solid`. They take host elements (a React or Solid `ref`) where the DOM takes an `Element`, deliver entries once per frame, and are not installed as globals.

`ResizeObserver` reports the content box or border box (`box` option) after `observe()` and on each change; the content box is the border box minus the element's `padding` and `borderWidth`. `IntersectionObserver` takes `root`, `rootMargin` and `threshold`, reports an initial entry, and then fires on threshold crossings. The intersection uses the border box clipped by every ancestor's `overflow` up to the root, so `rootMargin` can reach past the window edge.

The event payload gains `observation`, and `PaintReporter` receives the window being painted.

An element that is not mounted yet is observed once it is, and a removed element reports once as if it stopped painting.
