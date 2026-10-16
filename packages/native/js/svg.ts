/** Element types that exist only as children of an `<svg>`. */
export const SVG_CHILD_TYPES: ReadonlySet<string> = new Set([
  "path",
  "circle",
  "ellipse",
  "rect",
  "line",
  "polyline",
  "polygon",
  "g",
  "defs",
  "linearGradient",
  "radialGradient",
  "stop",
  "clipPath",
  "mask",
  "use",
  "symbol",
  "pattern",
  "marker",
  "image",
  "tspan",
  "textPath",
  "filter",
  "feBlend",
  "feColorMatrix",
  "feComponentTransfer",
  "feFuncR",
  "feFuncG",
  "feFuncB",
  "feFuncA",
  "feComposite",
  "feConvolveMatrix",
  "feDiffuseLighting",
  "feDisplacementMap",
  "feDistantLight",
  "feDropShadow",
  "feFlood",
  "feGaussianBlur",
  "feImage",
  "feMerge",
  "feMergeNode",
  "feMorphology",
  "feOffset",
  "fePointLight",
  "feSpecularLighting",
  "feSpotLight",
  "feTile",
  "feTurbulence",
])

/**
 * The props of an SVG `<text>`. `text` is also the host text element, which
 * forwards only its universal props, so these are what lets `<svg><text x={4}>`
 * reach the rasteriser. The host element never has them.
 */
export const SVG_TEXT_PROPS: ReadonlySet<string> = new Set([
  "x",
  "y",
  "dx",
  "dy",
  "rotate",
  "textLength",
  "lengthAdjust",
  "transform",
  "fill",
  "fillOpacity",
  "fillRule",
  "stroke",
  "strokeWidth",
  "strokeOpacity",
  "strokeLinecap",
  "strokeLinejoin",
  "strokeMiterlimit",
  "strokeDasharray",
  "strokeDashoffset",
  "opacity",
  "display",
  "visibility",
  "clipPath",
  "mask",
  "filter",
  "paintOrder",
  "fontFamily",
  "fontSize",
  "fontWeight",
  "fontStyle",
  "fontVariant",
  "fontStretch",
  "textAnchor",
  "dominantBaseline",
  "alignmentBaseline",
  "baselineShift",
  "letterSpacing",
  "wordSpacing",
  "textDecoration",
  "writingMode",
  "direction",
  "unicodeBidi",
  "textRendering",
  "xmlSpace",
])

/**
 * Throws when `child` is an SVG element placed anywhere but directly in an
 * `<svg>` or in another SVG element (`<g>`, `<defs>`, `<linearGradient>`, ...).
 * That one is itself checked when it is placed, so the chain always ends at an
 * `<svg>`. `parent` is `undefined` for a window root.
 */
export function assertSvgPlacement(parent: string | undefined, child: string): void {
  if (!SVG_CHILD_TYPES.has(child)) return
  if (parent === "svg" || (parent !== undefined && SVG_CHILD_TYPES.has(parent))) return
  // `text` is a host element too, so a `<text>` parent proves nothing; the
  // `<tspan>` and `<textPath>` it holds are checked on the Rust side.
  if (parent === "text" && (child === "tspan" || child === "textPath")) return
  throw new Error(
    `<${child}> is only valid inside <svg>, but its parent is ${parent ? `<${parent}>` : "the window root"}`
  )
}
