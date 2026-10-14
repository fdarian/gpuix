---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

`<input>` and `<textarea>` gain the DOM selection API on their refs: `select()`, `setSelectionRange(start, end, direction?)`, and the synchronous getters `selectionStart`, `selectionEnd` and `selectionDirection`, in UTF-16 code units. Native adds `selectText`, `setSelectionRange` and `getTextSelection`.

`<input disabled>` is no longer focusable, takes no editing and no click or key events, and paints in the muted colour. `<input type>` accepts `password` (bullets, no copy or cut), `number` (ArrowUp and ArrowDown step by `step` within `min` and `max`), `email`, `search` (search field role), `tel` and `url`. `maxLength` limits typed and pasted text.
