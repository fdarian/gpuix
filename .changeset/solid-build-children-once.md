---
'@gpuix/solid': patch
---

`DialogPortal`, `Tooltip`, `Select`, `Combobox` and their parts no longer build their children twice. The first build was thrown away, so a component or effect inside a dialog ran twice on open.
