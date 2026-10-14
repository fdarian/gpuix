---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Inline text follows the DOM. `<span>` is a new host element, and raw strings, `<span>` and a `<text>` inside a `<text>` or `<span>` are runs of one paragraph: one wrapped, selectable, searchable string whose runs take their own `color`, `backgroundColor`, `fontWeight`, `fontFamily`, `textDecoration` and font features from the inline box around them. In a flex or grid container each run of raw strings is one item and each `<span>` is an item of its own.

Breaking: sibling text nodes in a block `<div>` now flow on one line instead of stacking, and a child that is only whitespace, or has `display: none`, generates no box.
