---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

The new `<form>` element submits its `<input>` and `<textarea>` controls together. `name` and `required` are new props on both controls. Enter in an `<input>` submits the form when it is the form's only `<input>` (there is no submit button) and `keydown` is not prevented, empty `required` controls get `invalid` and the first takes focus instead of submitting, and `submit` carries `event.formData`. `requestSubmit()` on the form ref validates and submits. The form has the `Form` accessibility role.
