---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Scrollbars follow the macOS "Show scroll bars" setting. With Always (which macOS also picks while a mouse is attached) the bar is always drawn, a solid track and a thumb that neither fades nor thickens, and it takes a 15 px gutter (11 with `scrollbarWidth: "thin"`) out of the container, so `clientWidth`, `clientHeight` and the content box shrink by it; `overflow: "auto"` reserves it once the content scrolls and `scrollbarWidth: "none"` takes none. Overlay bars are unchanged except that the thumb now thickens under the pointer over 150 ms instead of at once. `TestRenderer.setScrollbarStyle("classic" | "overlay")` forces the style under test, which starts as overlay. Needs `ScrollHandle::scrollbar_size` and `Styled::without_scrollbar` from the zed fork.
