---
'@gpuix/native': patch
'@gpuix/react': patch
'@gpuix/solid': patch
---

`target`, `currentTarget` and `relatedTarget` are element objects for every event, the same ones a `ref` gives, so `event.relatedTarget === ref.current` works. `currentTarget` is the `window` or `document` object during their phases, `target` is `null` for an event aimed at the window (a key with nothing focused), and `relatedTarget` is `null` when there is no other element. `elementId` stays the numeric id. `event.path` is replaced by `composedPath()`, the pointer-lock and selection callbacks get elements the same way, and handler types are `DomEvent` instead of `EventPayload` (which is the raw native payload `onEvent` still receives). `installActiveElementSource` is now `installElementSource`.
