---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Accessibility follows the DOM. Any `aria-*` prop is accepted and mapped to AccessKit, `id` is the node's author id (the gpuix-only `aria-id` is removed), and the id-reference props (`aria-labelledby`, `aria-describedby`, `aria-controls`, `aria-owns`, `aria-details`, `aria-flowto`, `aria-errormessage`, `aria-activedescendant`) resolve by DOM `id`. The new `<label htmlFor>` element names an `<input>` or `<textarea>` and forwards clicks to it.
