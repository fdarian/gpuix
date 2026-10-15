import { describe, expect, it } from "vitest"
import { TestRenderer, hasNativeTestRenderer } from "../testing.js"

const linear = { duration: 1, ease: "linear" }

const mount = () => {
  const renderer = new TestRenderer({ width: 400, height: 300 })
  renderer.clockPause()
  renderer.applyBatch(
    JSON.stringify([
      ["createElement", 1, "div"],
      ["setRoot", 1],
      ["setStyle", 1, { display: "flex" }],
      ["createElement", 2, "div"],
      ["appendChild", 1, 2],
      ["setStyle", 2, { height: 20, width: 5 }],
    ])
  )
  renderer.flush()
  return renderer
}

const bind = (renderer: TestRenderer, binding: Record<string, number>) => {
  renderer.applyBatch(JSON.stringify([["setCustomProp", 2, "motionValues", binding]]))
  renderer.flush()
}

const widthOf = (renderer: TestRenderer) => renderer.getElementBounds(2)?.width

describe.skipIf(!hasNativeTestRenderer)("native motion values", () => {
  it("reads and writes at the window clock", () => {
    const renderer = mount()
    renderer.createMotionValue(1, 0)
    renderer.animateMotionValue(1, 100, linear)
    expect(renderer.isMotionValueAnimating(1)).toBe(true)
    renderer.clockFastForward(250)
    expect(renderer.getMotionValue(1)).toBeCloseTo(25, 5)
    expect(renderer.getMotionValueVelocity(1)).toBeCloseTo(100, 5)
    renderer.stopMotionValue(1)
    renderer.clockFastForward(1000)
    expect(renderer.getMotionValue(1)).toBeCloseTo(25, 5)
    renderer.jumpMotionValue(1, 3)
    expect(renderer.getMotionValue(1)).toBe(3)
  })

  it("rejects an id twice, an unknown id and a velocity of a colour", () => {
    const renderer = mount()
    renderer.createMotionValue(2, "#000000")
    expect(() => renderer.createMotionValue(2, 1)).toThrow(/already exists/)
    expect(() => renderer.getMotionValue(404)).toThrow(/unknown motion value/)
    expect(() => renderer.getMotionValueVelocity(2)).toThrow(/number/)
    expect(renderer.disposeMotionValue(2)).toBe(true)
    expect(renderer.disposeMotionValue(2)).toBe(false)
  })

  it("lays a bound value over the element's style on every frame", () => {
    const renderer = mount()
    renderer.createMotionValue(3, 50)
    bind(renderer, { width: 3 })
    expect(widthOf(renderer)).toBe(50)

    renderer.animateMotionValue(3, 150, linear)
    renderer.clockFastForward(500)
    renderer.flush()
    expect(widthOf(renderer)).toBeCloseTo(100, 0)
    renderer.clockFastForward(1000)
    renderer.flush()
    expect(widthOf(renderer)).toBe(150)
  })

  it("drives a bound property through a transform and a spring follower", () => {
    const renderer = mount()
    renderer.createMotionValue(4, 0)
    renderer.createMotionTransform(5, 4, [0, 1], [40, 240])
    bind(renderer, { width: 5 })
    expect(widthOf(renderer)).toBe(40)
    renderer.setMotionValue(4, 0.5)
    renderer.flush()
    expect(widthOf(renderer)).toBe(140)

    renderer.createMotionFollower(6, 4, { stiffness: 400, damping: 40 })
    bind(renderer, { width: 6 })
    renderer.setMotionValue(4, 100)
    for (let frame = 0; frame < 10; frame++) {
      renderer.clockFastForward(16)
      renderer.flush()
    }
    const mid = widthOf(renderer) ?? 0
    expect(mid).toBeGreaterThan(0.5)
    expect(mid).toBeLessThan(100)
    renderer.clockFastForward(5000)
    renderer.flush()
    renderer.clockFastForward(16)
    renderer.flush()
    expect(widthOf(renderer)).toBeCloseTo(100, 0)
  })

  it("delivers change and animation events to a listener, once each", () => {
    const renderer = mount()
    renderer.drainEvents()
    renderer.createMotionValue(7, 0)
    renderer.setMotionValueListener(7, true, true)
    renderer.animateMotionValue(7, 10, linear)
    renderer.clockFastForward(500)
    renderer.flush()
    renderer.clockFastForward(2000)
    renderer.flush()
    renderer.flush()

    const events = renderer
      .drainEvents()
      .filter((event) => event.eventType.startsWith("motionValue"))
    expect(events.map((event) => event.eventType)).toEqual([
      "motionValueAnimationStart",
      "motionValueChange",
      "motionValueChange",
      "motionValueAnimationComplete",
    ])
    expect(events.every((event) => event.elementId === 7)).toBe(true)
    expect(JSON.parse(events[1].value ?? "")).toBeCloseTo(5, 5)
    expect(JSON.parse(events[2].value ?? "")).toBe(10)

    renderer.setMotionValueListener(7, false, false)
    renderer.setMotionValue(7, 1)
    renderer.flush()
    expect(renderer.drainEvents().filter((event) => event.eventType.startsWith("motionValue"))).toEqual([])
  })
})
