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
  throw new Error(
    `<${child}> is only valid inside <svg>, but its parent is ${parent ? `<${parent}>` : "the window root"}`
  )
}
