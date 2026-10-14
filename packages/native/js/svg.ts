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
])

/**
 * Throws when `child` is an SVG shape placed anywhere but directly in an
 * `<svg>` or a `<g>`. A `<g>` is itself checked when it is placed, so the chain
 * always ends at an `<svg>`. `parent` is `undefined` for a window root.
 */
export function assertSvgPlacement(parent: string | undefined, child: string): void {
  if (!SVG_CHILD_TYPES.has(child)) return
  if (parent === "svg" || parent === "g") return
  throw new Error(
    `<${child}> is only valid inside <svg>, but its parent is ${parent ? `<${parent}>` : "the window root"}`
  )
}
