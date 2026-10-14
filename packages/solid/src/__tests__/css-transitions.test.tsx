import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("Solid CSS transitions", () => {
  const widthOf = (app: ReturnType<typeof createTestRoot>): number => {
    app.renderer.dispatchNativeEvents()
    return app.renderer.getElementBounds(app.renderer.findByTestId("box")!.id)!.width
  }

  it("animates a signal-driven style change and reports the events in order", () => {
    const [width, setWidth] = createSignal(100)
    const seen: string[] = []
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <div
        testId="box"
        onTransitionRun={(event) => seen.push(`run ${event.propertyName}`)}
        onTransitionStart={() => seen.push("start")}
        onTransitionEnd={(event) => seen.push(`end ${event.elapsedTime}`)}
        style={{ width: width(), height: 20, transition: "width 1s linear" }}
      />
    ))
    expect(widthOf(app)).toBe(100)

    app.flushSync(() => setWidth(200))
    app.renderer.clockFastForward(500)
    expect(widthOf(app)).toBeGreaterThan(149)
    expect(widthOf(app)).toBeLessThan(151)

    app.renderer.clockFastForward(1000)
    expect(widthOf(app)).toBe(200)
    expect(seen).toEqual(["run width", "start", "end 1"])
  })

  it("animates the hover style with the simulated pointer", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <div
        testId="box"
        style={{
          width: 100,
          height: 40,
          transition: "width 1s linear",
          hover: { width: 200 },
        }}
      />
    ))
    const bounds = app.renderer.getElementBounds(app.renderer.findByTestId("box")!.id)!

    app.renderer.nativeSimulateMouseMove(bounds.x + 10, bounds.y + 10)
    app.renderer.clockFastForward(500)
    expect(widthOf(app)).toBeGreaterThan(149)
    expect(widthOf(app)).toBeLessThan(151)

    app.renderer.clockFastForward(1000)
    expect(widthOf(app)).toBe(200)
  })

  it("does not transition without a duration", () => {
    const [width, setWidth] = createSignal(100)
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <div testId="box" style={{ width: width(), height: 20, transition: "width 0s" }} />
    ))
    app.flushSync(() => setWidth(200))
    expect(widthOf(app)).toBe(200)
  })
})
