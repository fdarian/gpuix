---
'@gpuix/native': patch
---

`boxShadow` is clipped as in CSS. A shadow that is not `inset` is painted only outside the border box (with the box's own rounded corners), so a translucent `backgroundColor` no longer looks darker for the shadow under it, and an `inset` shadow is painted only inside the padding box, so it no longer shows under a translucent border.
