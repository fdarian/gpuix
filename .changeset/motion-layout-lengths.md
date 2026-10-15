---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Motion and CSS transitions can resolve lengths through layout. `width`, `height` and the min and max sizes move between px and `%`, resolved against the parent's content box (the window's content size for a root element) and settling on the unit asked for. `motion` animates `height` to and from `"auto"` (an accordion), following the content while it grows, and a size the style leaves unset starts from the laid-out value. CSS transitions do the same for px and `%`, and for `height` to or from `auto` or unset when `interpolateSize: "allow-keywords"` is set, which inherits and defaults to `"numeric-only"` as in CSS. `width: "auto"` is an error in `motion` and stays discrete in transitions. `MotionStyle` types `height: "auto"` and percentage strings.
