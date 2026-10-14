import { describe, expect, it } from "bun:test"
import type { ScrollEvent, WheelEvent } from "@gpuix/native/host"
import type { HostElement } from "../host.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("Solid scrolling", () => {
  it("exposes the DOM scroll members on instances", () => {
    let scroller!: HostElement
    let target!: HostElement
    const app = createTestRoot()
    app.render(() => (
      <div
        ref={scroller}
        style={{ width: 200, height: 100, padding: 10, overflow: "auto" }}
      >
        <div style={{ height: 400 }} />
        <div ref={target} style={{ height: 20 }} />
      </div>
    ))

    expect(scroller.clientHeight).toBe(100)
    expect(scroller.scrollHeight).toBe(440)
    scroller.scrollTop = 120
    expect(scroller.scrollTop).toBe(120)
    scroller.scrollBy(0, 30)
    expect(scroller.scrollTop).toBe(150)
    scroller.scrollTo({ top: 10 })
    expect(scroller.scrollTop).toBe(10)
    target.scrollIntoView({ block: "end" })
    // The target ends 430 down, and the view is 100 tall.
    expect(scroller.scrollTop).toBe(330)
  })

  it("splits onWheel from onScroll", () => {
    const wheels: WheelEvent[] = []
    const scrolls: ScrollEvent[] = []
    const app = createTestRoot()
    app.render(() => (
      <div
        testId="scroller"
        style={{ width: 200, height: 100, overflow: "scroll" }}
        onWheel={(event) => wheels.push(event)}
        onScroll={(event) => scrolls.push(event)}
      >
        <div style={{ height: 400 }} />
      </div>
    ))

    app.renderer.nativeSimulateScrollWheel(100, 50, 0, -40)

    expect(wheels).toHaveLength(1)
    expect(wheels[0]).toMatchObject({ deltaY: 40, deltaMode: 0, clientX: 100, clientY: 50 })
    expect(scrolls).toHaveLength(1)
    expect(scrolls[0]).toMatchObject({ scrollTop: 40, scrollLeft: 0 })
  })
})
