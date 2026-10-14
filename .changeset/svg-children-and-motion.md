---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Add DOM-style SVG children and animate them. `<svg viewBox width height>` takes `path`, `circle`, `ellipse`, `rect`, `line`, `polyline`, `polygon` and `g` children with camelCase props (`strokeWidth`, `strokeDasharray`, `transform`, ...), serialised natively to SVG XML; the children win over `source` and `src`, and the shapes are an error outside `<svg>`. `motion.path`, `motion.circle` and the other shapes animate `pathLength` / `pathOffset` / `pathSpacing` (Framer's normalised draw), `strokeDasharray`, `strokeDashoffset`, `strokeWidth`, `fill`, `stroke`, the opacities, the geometry attributes and `d` morphs through the motion engine. The DOM `pathLength` attribute scales dash values; because the rasteriser ignores it, the shape's length is measured natively and dashes are written in absolute units.
