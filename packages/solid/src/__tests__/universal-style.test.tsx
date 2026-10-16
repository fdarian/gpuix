import { createSignal, Show } from "solid-js"
import { describe, expect, it } from "bun:test"
import { UniversalStyle } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

type App = ReturnType<typeof createTestRoot>

const sizeOf = (app: App, testId: string) => {
  app.renderer.dispatchNativeEvents()
  const bounds = app.renderer.getElementBounds(app.renderer.findByTestId(testId)!.id)!
  return [bounds.width, bounds.height]
}

/** A row that keeps its children at their own height, so a size comes from a style. */
const rowStyle = { display: "flex", alignItems: "flex-start", width: 300, height: 100 } as const

describe.skipIf(!hasNativeTestRenderer)("Solid UniversalStyle", () => {
  it("sets the layer on mount, follows its prop and clears it on unmount", () => {
    const [style, setStyle] = createSignal<object>({ width: 30, height: 10, flexShrink: 0 })
    const [mounted, setMounted] = createSignal(true)
    const app = createTestRoot({ width: 300, height: 100 })
    app.render(() => (
      <div style={rowStyle}>
        <Show when={mounted()}>
          <UniversalStyle style={style()} />
        </Show>
        <div testId="bare" />
        <div testId="own" style={{ width: 70 }} />
      </div>
    ))
    expect(sizeOf(app, "bare")).toEqual([30, 10])
    expect(sizeOf(app, "own")).toEqual([70, 10])

    app.flushSync(() => setStyle({ width: 40, height: 20, flexShrink: 0 }))
    expect(sizeOf(app, "bare")).toEqual([40, 20])
    expect(sizeOf(app, "own")).toEqual([70, 20])

    app.flushSync(() => setMounted(false))
    expect(sizeOf(app, "bare")).toEqual([0, 0])
    expect(sizeOf(app, "own")).toEqual([70, 0])
    app.unmount()
  })

  it("reaches an element mounted after the layer", () => {
    const [late, setLate] = createSignal(false)
    const app = createTestRoot({ width: 300, height: 100 })
    app.render(() => (
      <div style={rowStyle}>
        <UniversalStyle style={{ width: 30, height: 10, flexShrink: 0 }} />
        <Show when={late()}>
          <div testId="late" />
        </Show>
      </div>
    ))
    app.flushSync(() => setLate(true))
    expect(sizeOf(app, "late")).toEqual([30, 10])
    app.unmount()
  })

  it("stacks: a later component wins a shared property, and unmounting one leaves the other", () => {
    const [first, setFirst] = createSignal(true)
    const [second, setSecond] = createSignal(true)
    const app = createTestRoot({ width: 300, height: 100 })
    app.render(() => (
      <div style={rowStyle}>
        <Show when={first()}>
          <UniversalStyle style={{ width: 30, height: 10, flexShrink: 0 }} />
        </Show>
        <Show when={second()}>
          <UniversalStyle style={{ width: 50 }} />
        </Show>
        <div testId="bare" />
      </div>
    ))
    expect(sizeOf(app, "bare")).toEqual([50, 10])

    app.flushSync(() => setFirst(false))
    expect(sizeOf(app, "bare")).toEqual([50, 0])

    app.flushSync(() => setFirst(true))
    app.flushSync(() => setSecond(false))
    expect(sizeOf(app, "bare")).toEqual([30, 10])

    app.flushSync(() => setFirst(false))
    expect(sizeOf(app, "bare")).toEqual([0, 0])
    app.unmount()
  })

  it("gives elements that declared no transition one, mounted before the layer or after it", () => {
    const [width, setWidth] = createSignal(100)
    const [late, setLate] = createSignal(false)
    const app = createTestRoot({ width: 300, height: 100 })
    app.renderer.clockPause()
    app.render(() => (
      <div style={rowStyle}>
        <UniversalStyle style={{ transition: "width 1s linear" }} />
        <div testId="early" style={{ width: width(), height: 10, flexShrink: 0 }} />
        <Show when={late()}>
          <div testId="late" style={{ width: width(), height: 10, flexShrink: 0 }} />
        </Show>
        <div testId="own" style={{ width: width(), height: 10, flexShrink: 0, transition: "none" }} />
      </div>
    ))
    app.flushSync(() => setLate(true))
    expect(sizeOf(app, "late")).toEqual([100, 10])

    app.flushSync(() => setWidth(200))
    app.renderer.clockFastForward(500)
    for (const testId of ["early", "late"]) {
      expect(sizeOf(app, testId)[0]).toBeCloseTo(150, 0)
    }
    expect(sizeOf(app, "own")[0]).toBe(200)

    app.renderer.clockFastForward(1000)
    expect(sizeOf(app, "early")[0]).toBe(200)
    expect(sizeOf(app, "late")[0]).toBe(200)
    app.unmount()
  })

  it("throws outside a root", () => {
    expect(() => UniversalStyle({ style: {} })).toThrow()
  })
})
