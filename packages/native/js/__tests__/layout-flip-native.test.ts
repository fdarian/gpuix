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

describe.skipIf(!hasNativeTestRenderer)("native layout drag", () => {
  const drag = { x: 11, y: 12, active: 13 }
  const held = (renderer: TestRenderer, offset: number) => {
    renderer.jumpMotionValue(12, offset)
    renderer.flush()
  }

  const mountDraggable = () => {
    const renderer = mount({ mode: true, transition: linear, dragOffset: drag })
    renderer.createMotionValue(11, 0)
    renderer.createMotionValue(12, 0)
    renderer.createMotionValue(13, 0)
    renderer.flush()
    return renderer
  }

  it("shows a held box where the drag values put it, and hit-tests there", () => {
    const renderer = mountDraggable()
    renderer.jumpMotionValue(13, 1)
    held(renderer, 30)
    expect(topOf(renderer, 3)).toBe(50)
    expect(renderer.getElementsAt(10, 55)).toContain(3)
    held(renderer, 41)
    expect(topOf(renderer, 3)).toBe(61)
  })

  it("ignores the offset until the drag is active", () => {
    const renderer = mountDraggable()
    held(renderer, 30)
    expect(topOf(renderer, 3)).toBe(20)
  })

  it("keeps a held box under the pointer when layout moves its slot, then glides home on release", () => {
    const renderer = mountDraggable()
    renderer.jumpMotionValue(13, 1)
    held(renderer, 30)
    expect(topOf(renderer, 3)).toBe(50)
    pushDown(renderer)
    expect(topOf(renderer, 3)).toBe(50)
    renderer.jumpMotionValue(13, 0)
    renderer.jumpMotionValue(12, 0)
    renderer.flush()
    expect(topOf(renderer, 3)).toBeCloseTo(50, 1)
    renderer.clockFastForward(500)
    renderer.flush()
    expect(topOf(renderer, 3)).toBeCloseTo(85, 1)
    renderer.clockFastForward(600)
    renderer.flush()
    expect(topOf(renderer, 3)).toBe(120)
  })
})

describe.skipIf(!hasNativeTestRenderer)("native layoutId", () => {
  const shared = { mode: true, layoutId: "highlight", transition: linear }

  // A row holding one box at x = 0.
  const mountRow = (layout: unknown = shared) => {
    const renderer = new TestRenderer({ width: 400, height: 100 })
    renderer.clockPause()
    renderer.applyBatch(
      JSON.stringify([
        ["createElement", 1, "div"],
        ["setRoot", 1],
        ["setStyle", 1, { display: "flex", flexDirection: "row", width: 400 }],
        ["createElement", 2, "div"],
        ["appendChild", 1, 2],
        ["setStyle", 2, { height: 20, width: 50 }],
        ["setCustomProp", 2, "layout", layout],
      ])
    )
    renderer.flush()
    return renderer
  }

  // A spacer pushes the new box to x = 200.
  const addBox = (layout: unknown = shared) => [
    ["createElement", 3, "div"],
    ["appendChild", 1, 3],
    ["setStyle", 3, { height: 20, width: 200 }],
    ["createElement", 4, "div"],
    ["appendChild", 1, 4],
    ["setStyle", 4, { height: 20, width: 50 }],
    ["setCustomProp", 4, "layout", layout],
  ]

  const leftOf = (renderer: TestRenderer, id: number) => renderer.getElementBounds(id)?.x

  it("starts a box that replaces another from where the old one was shown", () => {
    const renderer = mountRow()
    renderer.applyBatch(JSON.stringify([["destroyElement", 2], ...addBox()]))
    renderer.flush()
    expect(leftOf(renderer, 4)).toBeCloseTo(0, 1)
    renderer.clockFastForward(500)
    renderer.flush()
    expect(leftOf(renderer, 4)).toBeCloseTo(100, 1)
    renderer.clockFastForward(600)
    renderer.flush()
    expect(leftOf(renderer, 4)).toBe(200)
  })

  it("starts from the old box one frame later too, but not after that", () => {
    const renderer = mountRow()
    renderer.applyBatch(JSON.stringify([["destroyElement", 2]]))
    renderer.flush()
    renderer.applyBatch(JSON.stringify(addBox()))
    renderer.flush()
    expect(leftOf(renderer, 4)).toBeCloseTo(0, 1)

    const late = mountRow()
    late.applyBatch(JSON.stringify([["destroyElement", 2]]))
    late.flush()
    late.flush()
    late.flush()
    late.applyBatch(JSON.stringify(addBox()))
    late.flush()
    expect(leftOf(late, 4)).toBe(200)
  })

  it("lets a box take over from one that is still mounted", () => {
    const renderer = mountRow()
    renderer.applyBatch(JSON.stringify(addBox()))
    renderer.flush()
    expect(leftOf(renderer, 4)).toBeCloseTo(0, 1)
    expect(leftOf(renderer, 2)).toBe(0)
    renderer.clockFastForward(500)
    renderer.flush()
    // The box that stayed pushes the spacer and the newcomer 50 px further.
    expect(leftOf(renderer, 4)).toBeCloseTo(125, 1)
    expect(leftOf(renderer, 2)).toBe(0)
  })

  it("does not share between different ids or when the id is left out", () => {
    for (const other of [{ ...shared, layoutId: "other" }, { mode: true, transition: linear }]) {
      const renderer = mountRow()
      renderer.applyBatch(JSON.stringify([["destroyElement", 2], ...addBox(other)]))
      renderer.flush()
      expect(leftOf(renderer, 4)).toBe(200)
    }
  })

  it("jumps under reduced motion", () => {
    const reduced = { ...shared, reducedMotion: true }
    const renderer = mountRow(reduced)
    renderer.applyBatch(JSON.stringify([["destroyElement", 2], ...addBox(reduced)]))
    renderer.flush()
    expect(leftOf(renderer, 4)).toBe(200)
  })

  it("reports when a glide starts and when it completes, to those who listen", () => {
    const renderer = mountRow()
    renderer.applyBatch(
      JSON.stringify([
        ["destroyElement", 2],
        ...addBox(),
        ["setEventListener", 4, "layoutAnimationStart", true],
        ["setEventListener", 4, "layoutAnimationComplete", true],
      ])
    )
    renderer.flush()
    renderer.flush()
    const types = () => renderer.drainEvents().map((event) => `${event.elementId} ${event.eventType}`)
    expect(types()).toEqual(["4 layoutAnimationStart"])

    renderer.clockFastForward(500)
    renderer.flush()
    expect(types()).toEqual([])
    renderer.clockFastForward(600)
    renderer.flush()
    renderer.flush()
    expect(types()).toEqual(["4 layoutAnimationComplete"])
  })

  it("stays silent without a listener", () => {
    const renderer = mountRow()
    renderer.applyBatch(JSON.stringify([["destroyElement", 2], ...addBox()]))
    renderer.flush()
    renderer.clockFastForward(1100)
    renderer.flush()
    renderer.flush()
    expect(renderer.drainEvents()).toEqual([])
  })
})
