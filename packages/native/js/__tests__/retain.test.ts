import { describe, expect, it } from "vitest"
import { TestRenderer, hasNativeTestRenderer } from "../testing.js"

// A column of three cards, each with two lines of text.
const mount = () => {
  const renderer = new TestRenderer({ width: 400, height: 400 })
  renderer.clockPause()
  const batch: unknown[] = [
    ["createElement", 1, "div"],
    ["setRoot", 1],
    ["setStyle", 1, { display: "flex", flexDirection: "column", width: 300, color: "#223344" }],
  ]
  for (const card of [10, 20, 30]) {
    batch.push(["createElement", card, "div"], ["appendChild", 1, card], ["setStyle", card, { height: 40, hover: { backgroundColor: "#aa0000" } }])
    for (const line of [1, 2]) {
      const id = card + line
      batch.push(["createElement", id, "div"], ["appendChild", card, id], ["createElement", id * 10, "text"], ["setText", id * 10, "some words"], ["appendChild", id, id * 10])
    }
  }
  renderer.applyBatch(JSON.stringify(batch))
  renderer.setRetainVerify(true)
  renderer.flush()
  return renderer
}

const reasons = (renderer: TestRenderer) => JSON.parse(renderer.getBuildCounters().retainReasons) as Record<string, number>

describe.skipIf(!hasNativeTestRenderer)("native retain counters", () => {
  it("counts a page that does not change as one retainable root, and every element once", () => {
    const renderer = mount()
    renderer.flush()
    renderer.resetBuildCounters()
    renderer.flush()
    const counters = renderer.getBuildCounters()
    expect(counters.retainableRoots).toBe(1)
    expect(counters.retainableElements).toBe(counters.elementsBuilt)
    expect(Object.values(reasons(renderer)).reduce((sum, count) => sum + count, 0)).toBe(counters.elementsBuilt)
    expect(counters.retainMismatches).toBe(0)
  })

  it("counts the element the pointer is over as changed, and its siblings as retainable", () => {
    const renderer = mount()
    renderer.flush()
    renderer.resetBuildCounters()
    const card = renderer.getElementBounds(10)!
    renderer.dispatchMouseMove(card.x + 2, card.y + 2)
    renderer.flush()
    const counters = renderer.getBuildCounters()
    expect(reasons(renderer)["changed:pointer"]).toBeGreaterThan(0)
    expect(counters.retainableElements).toBeGreaterThan(0)
    expect(counters.retainableElements).toBeLessThan(counters.elementsBuilt)
    expect(counters.retainMismatches, renderer.getRetainMismatches().join("\n")).toBe(0)
  })

  it("counts an element that cannot be reused under the reason, and counts it as reusable once the reason is lifted", () => {
    const renderer = mount()
    renderer.applyBatch(JSON.stringify([["setStyle", 20, { height: 40, overflow: "scroll" }]]))
    renderer.flush()
    renderer.flush()
    renderer.resetBuildCounters()
    renderer.flush()
    expect(reasons(renderer)["excluded:scroller"]).toBeGreaterThan(0)
    renderer.setRetainLift(["scroller"])
    renderer.flush()
    renderer.resetBuildCounters()
    renderer.flush()
    const counters = renderer.getBuildCounters()
    expect(reasons(renderer)["excluded:scroller"]).toBeUndefined()
    expect(counters.retainableElements).toBe(counters.elementsBuilt)
  })

  it("does not count an overflow: hidden element as an obstacle", () => {
    const renderer = mount()
    renderer.applyBatch(JSON.stringify([["setStyle", 20, { height: 40, overflow: "hidden" }]]))
    renderer.flush()
    renderer.flush()
    renderer.resetBuildCounters()
    renderer.flush()
    const counters = renderer.getBuildCounters()
    expect(counters.retainableElements).toBe(counters.elementsBuilt)
    expect(counters.retainMismatches).toBe(0)
  })

  it("rejects an exclusion it does not know", () => {
    const renderer = mount()
    expect(() => renderer.setRetainLift(["nonsense"])).toThrow(/no exclusion called nonsense/)
  })
})
