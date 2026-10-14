---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

`motion.div` animates every numeric, length, colour and `boxShadow` style property, not just eight. `animate`, `initial` and `exit` accept padding, margin, gap, border widths and radii, outline, font size, `backgroundColor`, `color`, `borderColor` and the rest, and `padding`, `margin`, `borderWidth`, `borderRadius` and `gap` expand to their longhands. Colours interpolate in Oklab with premultiplied alpha and shadows per layer. `transition` takes a curve per property (`transition: { duration: 0.3, opacity: { duration: 0.1 } }`) with a `default`, and a property `initial` does not name starts from the element's own style, as Motion for React reads the DOM. A retarget restarts only the properties whose target or transition changed. A discrete or unknown key in `animate` is now an error.
