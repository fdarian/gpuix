import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { defineKeyframes } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

defineKeyframes("solid-grow", { from: { width: 100 }, to: { width: 200 } })
defineKeyframes("solid-shrink", { from: { width: 200 }, to: { width: 100 } })

describe.skipIf(!hasNativeTestRenderer)("Solid CSS animations", () => {
  const widthOf = (app: ReturnType<typeof createTestRoot>): number => {
    app.renderer.dispatchNativeEvents()
    return app.renderer.getElementBounds(app.renderer.findByTestId("box")!.id)!.width
  }

  it("repeats a spinner forever", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <div
        testId="box"
        style={{ width: 50, height: 20, animation: "solid-grow 1s linear infinite" }}
      />
    ))
    expect(widthOf(app)).toBe(100)

    app.renderer.clockFastForward(500)
    expect(widthOf(app)).toBeCloseTo(150, 0)
    app.renderer.clockFastForward(750)
    expect(widthOf(app)).toBeCloseTo(125, 0)
    app.renderer.clockFastForward(10_000)
    expect(widthOf(app)).toBeCloseTo(125, 0)
  })

  it("holds the last keyframe with fill-mode forwards and returns to the declared width without it", () => {
    const plain = createTestRoot()
    plain.renderer.clockPause()
    plain.render(() => (
      <div testId="box" style={{ width: 50, height: 20, animation: "solid-grow 1s linear" }} />
    ))
    plain.renderer.clockFastForward(500)
    expect(widthOf(plain)).toBeCloseTo(150, 0)
    plain.renderer.clockFastForward(1000)
    expect(widthOf(plain)).toBe(50)

    const held = createTestRoot()
    held.renderer.clockPause()
    held.render(() => (
      <div
        testId="box"
        style={{ width: 50, height: 20, animation: "solid-grow 1s linear forwards" }}
      />
    ))
    held.renderer.clockFastForward(2000)
    expect(widthOf(held)).toBe(200)
  })

  it("starts, loops and ends with the DOM event fields, and cancels when a signal removes it", () => {
    const [animation, setAnimation] = createSignal("solid-grow 1s linear 500ms 2")
    const seen: Array<[string, string | undefined, number | undefined]> = []
    const record = (type: string) => (event: { animationName?: string; elapsedTime?: number }) =>
      seen.push([type, event.animationName, event.elapsedTime])
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <div
        testId="box"
        onAnimationStart={record("start")}
        onAnimationIteration={record("iteration")}
        onAnimationEnd={record("end")}
        onAnimationCancel={record("cancel")}
        style={{ width: 50, height: 20, animation: animation() }}
      />
    ))
    app.renderer.dispatchNativeEvents()
    expect(seen).toEqual([])

    app.renderer.clockFastForward(600)
    app.renderer.dispatchNativeEvents()
    app.renderer.clockFastForward(1000)
    app.renderer.dispatchNativeEvents()
    app.renderer.clockFastForward(1000)
    app.renderer.dispatchNativeEvents()
    expect(seen).toEqual([
      ["start", "solid-grow", 0],
      ["iteration", "solid-grow", 1],
      ["end", "solid-grow", 2],
    ])

    seen.length = 0
    app.flushSync(() => setAnimation("solid-shrink 1s linear"))
    app.renderer.clockFastForward(500)
    app.flushSync(() => setAnimation("none"))
    app.renderer.clockFastForward(16)
    app.renderer.dispatchNativeEvents()
    expect(widthOf(app)).toBe(50)
    expect(seen).toEqual([
      ["start", "solid-shrink", 0],
      ["cancel", "solid-shrink", 0.5],
    ])
  })

  it("starts an animation that a hover style declares", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <div
        testId="box"
        style={{
          width: 50,
          height: 40,
          hover: { animation: "solid-grow 1s linear forwards" },
        }}
      />
    ))
    const bounds = app.renderer.getElementBounds(app.renderer.findByTestId("box")!.id)!
    expect(widthOf(app)).toBe(50)

    app.renderer.nativeSimulateMouseMove(bounds.x + 10, bounds.y + 10)
    app.renderer.clockFastForward(500)
    expect(widthOf(app)).toBeCloseTo(150, 0)
    app.renderer.clockFastForward(1000)
    expect(widthOf(app)).toBe(200)

    app.renderer.nativeSimulateMouseMove(bounds.x + 500, bounds.y + 300)
    app.renderer.clockFastForward(16)
    expect(widthOf(app)).toBe(50)
  })

  it("bubbles a child's animation events to its parent with the child as target", () => {
    const seen: string[] = []
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <div
        testId="parent"
        onAnimationEnd={(event) =>
          seen.push(`parent ${event.animationName} ${event.target === app.renderer.findByTestId("child")!.id}`)
        }
      >
        <div testId="child" style={{ width: 50, height: 20, animation: "solid-grow 1s linear" }} />
      </div>
    ))
    app.renderer.dispatchNativeEvents()
    app.renderer.clockFastForward(1500)
    app.renderer.dispatchNativeEvents()
    expect(seen).toEqual(["parent solid-grow true"])
  })
})
