---
'@gpuix/native': patch
'@gpuix/react': patch
'@gpuix/solid': patch
---

`position: absolute` and `position: fixed` boxes escape the `overflow` clip and the scroll offset of the ancestors before their containing block, as in CSS. A dropdown inside an `overflow: hidden` card with no `position` is no longer cut by the card or moved by a scroller between it and its containing block; give the card a `position` to clip it again. `fixed` is clipped by the window alone and scrolls with nothing. `IntersectionObserver`, `getElementsAt`, `scrollIntoView` and key scrolling follow the same rule.
