---
'@gpuix/native': patch
---

`motion` and CSS transitions animate `padding` and `flexBasis` between a length and a percentage. A padding percentage is of the parent's width on every side, a `flexBasis` percentage of the flex container's main size. An unset `fontSize` or `fontWeight` starts from the value the element inherits (or the initial `16px` and `400` at the root) instead of jumping to the target or starting from `400`.
