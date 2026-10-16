import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { HostElement } from "../host.js"

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
  describe("lengths layout resolves", () => {
    const boundsOf = (app: ReturnType<typeof createTestRoot>, testId: string) => {
      app.renderer.dispatchNativeEvents()
      return app.renderer.getElementBounds(app.renderer.findByTestId(testId)!.id)!
    }
    // Layout is read back a frame after it is painted, so frames run between
    // mounting and changing the style, as they do in a real window.
    const mounted = (app: ReturnType<typeof createTestRoot>, code: Parameters<typeof app.render>[0]) => {
      app.renderer.clockPause()
      app.render(code)
      app.renderer.flush()
    }
    const update = (app: ReturnType<typeof createTestRoot>, fn: () => void) => {
      app.flushSync(fn)
      app.renderer.flush()
    }

    it("opens and closes an accordion when interpolateSize allows keywords", () => {
      const [open, setOpen] = createSignal(false)
      const app = createTestRoot({ width: 400, height: 300 })
      mounted(app, () => (
        <div style={{ display: "flex", flexDirection: "column" }}>
          <div
            testId="panel"
            style={{
              height: open() ? "auto" : 0,
              overflow: "hidden",
              interpolateSize: "allow-keywords",
              transition: "height 1s linear",
            }}
          >
            <div style={{ width: 40, height: 80 }} />
          </div>
        </div>
      ))
      expect(boundsOf(app, "panel").height).toBe(0)

      update(app, () => setOpen(true))
      app.renderer.clockFastForward(500)
      expect(boundsOf(app, "panel").height).toBeCloseTo(40, 0)
      app.renderer.clockFastForward(1000)
      expect(boundsOf(app, "panel").height).toBe(80)

      update(app, () => setOpen(false))
      app.renderer.clockFastForward(500)
      expect(boundsOf(app, "panel").height).toBeCloseTo(40, 0)
      app.renderer.clockFastForward(1000)
      expect(boundsOf(app, "panel").height).toBe(0)
    })

    it("follows the content while it grows during the transition", () => {
      const [open, setOpen] = createSignal(false)
      const [content, setContent] = createSignal(80)
      const app = createTestRoot({ width: 400, height: 300 })
      mounted(app, () => (
        <div style={{ display: "flex", flexDirection: "column" }}>
          <div
            testId="panel"
            style={{
              height: open() ? "auto" : 0,
              overflow: "hidden",
              interpolateSize: "allow-keywords",
              transition: "height 1s linear",
            }}
          >
            <div style={{ width: 40, height: content() }} />
          </div>
        </div>
      ))
      update(app, () => setOpen(true))
      app.renderer.clockFastForward(500)
      expect(boundsOf(app, "panel").height).toBeCloseTo(40, 0)

      update(app, () => setContent(160))
      app.renderer.clockFastForward(250)
      const grown = boundsOf(app, "panel").height
      expect(grown).toBeGreaterThan(40)
      expect(grown).toBeLessThan(160)

      app.renderer.clockFastForward(1000)
      expect(boundsOf(app, "panel").height).toBe(160)
    })

    it("transitions between px and a percentage of the parent", () => {
      const [width, setWidth] = createSignal<number | `${number}%`>(0)
      const app = createTestRoot({ width: 400, height: 300 })
      mounted(app, () => (
        <div style={{ display: "flex", flexDirection: "column", width: 400 }}>
          <div
            testId="bar"
            style={{ width: width(), height: 10, transition: "width 1s linear" }}
          />
        </div>
      ))
      update(app, () => setWidth("50%"))
      app.renderer.clockFastForward(500)
      expect(boundsOf(app, "bar").width).toBeCloseTo(100, 0)
      app.renderer.clockFastForward(1000)
      expect(boundsOf(app, "bar").width).toBe(200)

      update(app, () => setWidth(100))
      app.renderer.clockFastForward(500)
      expect(boundsOf(app, "bar").width).toBeCloseTo(150, 0)
      app.renderer.clockFastForward(1000)
      expect(boundsOf(app, "bar").width).toBe(100)
    })

    it("resolves a percentage on a root against the window", () => {
      const [width, setWidth] = createSignal<number | `${number}%`>(0)
      const app = createTestRoot({ width: 400, height: 300 })
      mounted(app, () => (
        <div testId="bar" style={{ width: width(), height: 10, transition: "width 1s linear" }} />
      ))
      update(app, () => setWidth("50%"))
      app.renderer.clockFastForward(500)
      expect(boundsOf(app, "bar").width).toBeCloseTo(100, 0)
      app.renderer.clockFastForward(1000)
      expect(boundsOf(app, "bar").width).toBe(200)
    })

    it("starts from the laid-out height when the style leaves it unset", () => {
      const [height, setHeight] = createSignal<number | undefined>(undefined)
      const app = createTestRoot({ width: 400, height: 300 })
      mounted(app, () => (
        <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start" }}>
          <div
            testId="box"
            style={{
              height: height(),
              interpolateSize: "allow-keywords",
              transition: "height 1s linear",
            }}
          >
            <div style={{ width: 40, height: 40 }} />
          </div>
        </div>
      ))
      expect(boundsOf(app, "box").height).toBe(40)

      update(app, () => setHeight(120))
      app.renderer.clockFastForward(500)
      expect(boundsOf(app, "box").height).toBeCloseTo(80, 0)
      app.renderer.clockFastForward(1000)
      expect(boundsOf(app, "box").height).toBe(120)
    })
  })

  it("bubbles a child's transition to its parent with the child as target, and stopPropagation ends it", () => {
    const [width, setWidth] = createSignal(100)
    const [stop, setStop] = createSignal(false)
    const seen: string[] = []
    let child!: HostElement
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <div
        testId="parent"
        onTransitionEndCapture={() => seen.push("parent capture")}
        onTransitionEnd={(event) =>
          seen.push(`parent ${event.propertyName} ${event.target === child}`)
        }
      >
        <div
          ref={child}
          testId="child"
          onTransitionEnd={(event) => {
            seen.push("child")
            if (stop()) event.stopPropagation()
          }}
          style={{ width: width(), height: 20, transition: "width 1s linear" }}
        />
      </div>
    ))
    app.flushSync(() => setWidth(200))
    app.renderer.clockFastForward(1500)
    app.renderer.dispatchNativeEvents()
    expect(seen).toEqual(["parent capture", "child", "parent width true"])

    seen.length = 0
    app.flushSync(() => {
      setStop(true)
      setWidth(300)
    })
    app.renderer.clockFastForward(1500)
    app.renderer.dispatchNativeEvents()
    expect(seen).toEqual(["parent capture", "child"])
  })
})
