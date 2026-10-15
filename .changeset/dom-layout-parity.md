---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Layout follows CSS more closely:

- `position` defaults to `static`. An `absolute` box resolves against its nearest positioned ancestor (`relative`, `absolute`, `fixed` or `sticky`) and `fixed` against the window. Parents of absolutely placed boxes need `position: "relative"`.
- Empty and white-space-only text nodes, and `display: "none"` elements, build no box, so they no longer add a flex or grid item or a `gap`.
- `aspectRatio` takes CSS ratio strings, and `<img>` sizes like a replaced element.
- `flexDirection` accepts `row-reverse` and `column-reverse`; new `order`.
- `margin*` and `top`/`right`/`bottom`/`left` accept percentages and `"auto"`; `display` accepts `block` and `none`.
- `gridTemplateColumns` / `gridTemplateRows` take full CSS track lists; new `gridColumn`, `gridRow`, `gridColumnStart/End`, `gridRowStart/End`, `gridArea`, `gridTemplateAreas`, `gridAutoColumns`, `gridAutoRows`, `gridAutoFlow`, `justifyItems`, `justifySelf`.
- `<anchored>` flips to the other side of the trigger when it does not fit (the default `fit` is now `"flip"`), no longer offsets `align="center"` twice, and has `onPlacement` and `constrainToAvailable`.
- Positioned children paint (and take hits) above in-flow siblings, and `zIndex` orders them. Tab order is unchanged.
- `display: "contents"` splices an element's children into its parent. `<anchored>` `onPlacement` also reports `anchorWidth`/`anchorHeight`, and its direct children accept `"anchor-size(width)"` / `"anchor-size(height)"` for the six size properties.
