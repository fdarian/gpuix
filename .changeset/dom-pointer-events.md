---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Pointer events follow the DOM. Every window hit-tests like a browser: the pointer's target is the topmost element under it, whatever it paints, and the event runs along the target's ancestors in the tree you wrote. An `<anchored>` layer bubbles to the element it is declared under. `pointerEvents: "none"` makes an element never the target, inherits, and a descendant's `"auto"` restores it.

Native emits `mouseover`/`mouseout` (bubbling) and `mouseenter`/`mouseleave` (per element, in DOM order, with `relatedTarget`), `mousedown`/`mouseup`/`mousemove` with the matching `pointer*` event first, `click` on the nearest common ancestor of the press and release targets, `auxclick`, `dblclick`, `contextmenu` on the secondary press, and `detail`. Events carry `target`, `path`, `clientX`/`clientY`, `offsetX`/`offsetY` (from the target's padding edge), `buttons`, `pointerId` and `pointerType`. A type is only produced when the target or an ancestor listens for it. `hover` and `active` styles apply to the target and its ancestors.

Breaking: a filled or absolutely positioned element no longer hides what is behind it from hover, and a click on a filled child bubbles to its parent's `onClick`. `pointerEvents: "auto"` no longer stops the wheel. `<anchored>` loses its `occlude` prop: it is hit before what it covers because it paints on top, and `pointerEvents: "none"` lets the pointer through. `onFileDrop` on custom elements becomes `onDrop`.
