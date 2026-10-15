import { describe, expect, it } from "vitest"
import { TestRenderer, hasNativeTestRenderer } from "../testing.js"

const linear = { duration: 1, ease: "linear" }

// A column of two boxes: growing the first pushes the second down 100 px.
const mount = (layout: unknown) => {
  const renderer = new TestRenderer({ width: 400, height: 400 })
  renderer.clockPause()
  renderer.applyBatch(
    JSON.stringify([
      ["createElement", 1, "div"],
      ["setRoot", 1],
      ["setStyle", 1, { display: "flex", flexDirection: "column", width: 100 }],
      ["createElement", 2, "div"],
      ["appendChild", 1, 2],
      ["setStyle", 2, { height: 20, width: 50 }],
      ["createElement", 3, "div"],
      ["appendChild", 1, 3],
      ["setStyle", 3, { height: 20, width: 50 }],
      ["setCustomProp", 3, "layout", layout],
    ])
  )
  renderer.flush()
  return renderer
}

const pushDown = (renderer: TestRenderer) => {
  renderer.applyBatch(JSON.stringify([["setStyle", 2, { height: 120, width: 50 }]]))
  renderer.flush()
}

const topOf = (renderer: TestRenderer, id: number) => renderer.getElementBounds(id)?.y

describe.skipIf(!hasNativeTestRenderer)("native layout flip", () => {
  it("glides a box from where it was to where layout put it", () => {
    const renderer = mount({ mode: true, transition: linear })
    expect(topOf(renderer, 3)).toBe(20)

    pushDown(renderer)
    expect(topOf(renderer, 3)).toBeCloseTo(20, 1)
    renderer.clockFastForward(500)
    renderer.flush()
    expect(topOf(renderer, 3)).toBeCloseTo(70, 1)
    renderer.clockFastForward(600)
    renderer.flush()
    expect(topOf(renderer, 3)).toBe(120)
  })

  it("hit-tests where the box is drawn", () => {
    const renderer = mount({ mode: true, transition: linear })
    pushDown(renderer)
    renderer.clockFastForward(500)
    renderer.flush()
    expect(renderer.getElementsAt(10, 75)).toContain(3)
    expect(renderer.getElementsAt(10, 125)).not.toContain(3)
  })

  it("does not glide a box twice when an enclosing one already carries it", () => {
    const renderer = mount({ mode: true, transition: linear })
    renderer.applyBatch(
      JSON.stringify([
        ["createElement", 4, "div"],
        ["appendChild", 3, 4],
        ["setStyle", 4, { height: 10, width: 10 }],
        ["setCustomProp", 4, "layout", { mode: true, transition: linear }],
      ])
    )
    renderer.flush()
    pushDown(renderer)
    renderer.clockFastForward(500)
    renderer.flush()
    expect(topOf(renderer, 3)).toBeCloseTo(70, 1)
    expect(topOf(renderer, 4)).toBeCloseTo(70, 1)
  })

  it("jumps under reduced motion and when only size is asked for", () => {
    for (const layout of [{ mode: true, reducedMotion: true, transition: linear }, "size"]) {
      const renderer = mount(layout)
      pushDown(renderer)
      expect(topOf(renderer, 3)).toBe(120)
    }
  })

  it("leaves an element without the prop where layout put it", () => {
    const renderer = mount("size")
    renderer.applyBatch(JSON.stringify([["setCustomProp", 3, "layout", null]]))
    pushDown(renderer)
    expect(topOf(renderer, 3)).toBe(120)
  })
})
