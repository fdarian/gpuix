---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

`<UniversalStyle style={...} />` puts a style under the own style of every element, like a CSS `* { ... }` rule, with the lowest precedence: an element's own style and state styles win per property, and a shorthand of its own replaces a universal longhand. `<UniversalStyle style={{ transition: 'background-color 300ms, color 300ms' }} />` is a theme cross-fade: every element fades when `colorScheme` or the window appearance flips its `light-dark()` colours. Text nodes get nothing, several components stack in mount order, and a change restyles every element once. The layer is also the `setUniversalStyle(style | null)` mutation, validated like `setStyle`.
