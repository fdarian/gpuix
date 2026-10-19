---
'@gpuix/native': patch
---

A `focusWithin`, `hasFocusVisible` or `focusVisible` style that sets `zIndex`, `position` or an effect (`opacity`, `filter`, a mask or a clip path) now lifts the element as the style it makes with the base says: a `zIndex` over a base `position: relative` applies, a `position: static` ends the lift of a base `zIndex`, and a flex or grid item takes a `zIndex` from the variant. Before, the variant was applied on its own, so a `zIndex` alone did nothing and the element was painted and hit tested in another order than `elementsFromPoint` and the retained frames modelled.
