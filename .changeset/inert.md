---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

New `inert` prop, like the DOM attribute. The element and its subtree are not focusable (no Tab stop, `focus()` does nothing, a focused descendant is blurred when an ancestor becomes inert), are left out of the accessibility tree, cannot be selected, and take no pointer input: clicks reach whatever is behind them. `<input>` and `<textarea>` inside behave as `disabled`.
