---
'@gpuix/native': minor
---

`borderRadius` and the four corner longhands take the CSS grammar, with elliptical corners. `borderRadius` is a number or a string of one to four lengths or percentages and optionally `/` and one to four vertical radii (`"50%"`, `"10px 20px"`, `"10px 20px / 5px"`), and `borderTopLeftRadius` and the other three take `"20px 10px"` (horizontal, then vertical) or `"50% 20px"`. A percentage is of the border box, the width across and the height down, so `"50%"` on a wide box is an ellipse, and radii that overflow a side are scaled down together. The corner cuts the background, border, shadow, `overflow` clip and hit area. `clipPath: "inset(... round a / b)"` (and `rect()` and `xywh()`) now uses the vertical radii after the `/` instead of dropping them. A negative or invalid radius throws, in state styles too, and so does a negative number. Motion still animates a circular number of px.
