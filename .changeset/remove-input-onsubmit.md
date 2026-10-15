---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Breaking: the gpuix-only `onSubmit` on `<input>` and `<textarea>` is removed. `<textarea>` Enter always inserts a newline, and Enter in an `<input>` only submits its enclosing `<form>`. To act on Enter, handle `onKeyDown` with `event.key === "Enter"` (and `event.modifiers?.shift` for a textarea) and call `event.preventDefault()` to keep the newline or the implicit form submission. `onSubmit` remains on `<form>`.
