---
'@gpuix/native': patch
'@gpuix/react': patch
'@gpuix/solid': patch
---

`preventDefault()` on a `pointerdown` keeps the compatibility mouse events of that press from firing, as in browsers: no `mousedown`, no `mousemove` while the button is held and no `mouseup`. `pointermove`, `pointerup`, `click`, `contextmenu`, the focus move and the `mouseover`/`mouseout`/`mouseenter`/`mouseleave` events are unaffected, and the next press fires its mouse events again.
