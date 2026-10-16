---
'@gpuix/native': patch
'@gpuix/react': patch
'@gpuix/solid': patch
---

A `position: absolute` or `fixed` that only a state style sets (`hover`, `active`, `focusVisible`, `focusWithin`, `groupHover` and the other group variants) escapes the `overflow` clip of the ancestors before its containing block while the state applies, and `getElementsAt` and `scrollIntoView` now see it: a dropdown that appears on hover is hit outside the card that would clip it. A state that sets `position` back to `static` puts the clip back. `IntersectionObserver` still reads the declared `position`.
