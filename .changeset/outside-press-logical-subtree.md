---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

**Breaking:** `onMouseDownOutside` fires for a press on any element outside the listener's logical subtree, for every button, after `pointerdown` and `mousedown`. A press inside an `<anchored>` popup declared under the element is inside it, and a press on an overlay that merely paints above it is outside. The handler runs at the listening element only, and `event.target` is the pressed element. The Solid `Select` no longer reopens when its trigger is pressed while open.
