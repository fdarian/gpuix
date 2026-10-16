---
'@gpuix/native': patch
'@gpuix/react': patch
'@gpuix/solid': patch
---

Renderer calls work from `mousedown`, `pointerdown`, `mouseup`, `click` and `wheel` handlers. `getFocusedElementId()`, `matches()`, `getBoundingClientRect()`, scroll metrics and motion-value reads and writes no longer throw `VIEW_BUSY` there, so a Dialog or Popover can open from an `onMouseDown`. The test renderer no longer emulates a root-view lease that a real window never takes, and `setRootViewLease` is removed. `document.activeElement` is the focused element as a `ref` gives it, or `null`. `matches()` also supports `:hover` and `:active`.
