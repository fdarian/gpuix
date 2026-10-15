import { describe, expect, it } from "vitest"
import { TestRenderer, hasNativeTestRenderer } from "../testing.js"

const motion = {
  initial: { opacity: 0.5 },
  animate: { width: 200, opacity: 1 },
  transition: { duration: 1, ease: "linear" },
}

const mount = (type: string) => {
  const renderer = new TestRenderer({ width: 400, height: 300 })
  renderer.clockPause()
  renderer.applyBatch(
    JSON.stringify([
      ["createElement", 1, "div"],
      ["setRoot", 1],
      ["setStyle", 1, { display: "flex", flexDirection: "row" }],
      ["createElement", 2, type],
      ["appendChild", 1, 2],
      ["setStyle", 2, { height: 20, flexShrink: 0 }],
      ["setCustomProp", 2, "motion", motion],
    ])
  )
  // The first frame only paints the element, which is when it is measured.
  renderer.flush()
  renderer.flush()
  return renderer
}

const widthOf = (renderer: TestRenderer) => renderer.getElementBounds(2)?.width ?? Number.NaN

// A width the style leaves unset starts from the width layout gave, which only
// a painted frame of the element can tell. An element that is never measured
// has no start, so its animation jumps to the target.
describe.skipIf(!hasNativeTestRenderer)("native motion on custom elements", () => {
  for (const type of ["div", "img", "svg"]) {
    it(`glides an unset width on <${type}> instead of jumping`, () => {
      const renderer = mount(type)
      const start = widthOf(renderer)
      expect(start).toBeLessThan(200)

      renderer.clockFastForward(500)
      renderer.flush()
      const middle = widthOf(renderer)
      expect(middle).toBeGreaterThan(start)
      expect(middle).toBeLessThan(200)

      renderer.clockFastForward(1000)
      renderer.flush()
      expect(widthOf(renderer)).toBe(200)
    })
  }
})
