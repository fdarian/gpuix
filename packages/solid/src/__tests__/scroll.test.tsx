import { createSignal, For } from "solid-js"
import { describe, expect, it } from "bun:test"
import type { ScrollEvent, WheelEvent } from "@ghia/native/host"
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

    app.renderer.nativeSimulateScrollWheel(100, 50, 0, 40)

    expect(wheels).toHaveLength(1)
    expect(wheels[0]).toMatchObject({ deltaY: 40, deltaMode: 0, clientX: 100, clientY: 50 })
    expect(scrolls).toHaveLength(1)
    expect(scrolls[0]).toMatchObject({ scrollTop: 40, scrollLeft: 0 })
  })
})

describe.skipIf(!hasNativeTestRenderer)("Solid wheel events", () => {
  it("bubbles and captures along the path, and preventDefault cancels the scroll", () => {
    const log: string[] = []
    let prevent = true
    const app = createTestRoot()
    app.render(() => (
      <div
        style={{ width: 300, height: 300 }}
        onWheelCapture={() => log.push("outer-capture")}
        onWheel={() => log.push("outer")}
      >
        <div
          testId="scroller"
          style={{ width: 200, height: 100, overflow: "scroll" }}
          onWheel={(event) => {
            log.push("scroller")
            if (prevent) event.preventDefault()
          }}
        >
          <div style={{ height: 400 }} />
        </div>
      </div>
    ))
    const scroller = app.renderer.findByTestId("scroller")!.id

    app.renderer.nativeSimulateScrollWheel(100, 50, 0, 40)
    expect(log).toEqual(["outer-capture", "scroller", "outer"])
    expect(app.renderer.getScrollOffset(scroller)).toEqual([0, 0])

    prevent = false
    app.renderer.nativeSimulateScrollWheel(100, 50, 0, 40)
    expect(app.renderer.getScrollOffset(scroller)).toEqual([0, 40])
  })
})

describe.skipIf(!hasNativeTestRenderer)("Solid scroll reads are never stale", () => {
  it("reads scrollHeight in the turn that changed the content, before Solid flushes", () => {
    const [rows, setRows] = createSignal(2)
    let scroller!: HostElement
    const app = createTestRoot()
    app.render(() => (
      <div ref={scroller} style={{ width: 200, height: 100, overflow: "scroll" }}>
        <For each={Array.from({ length: rows() }, (_, row) => row)}>
          {() => <div style={{ height: 50, flexShrink: 0 }} />}
        </For>
      </div>
    ))
    expect(scroller.scrollHeight).toBe(100)

    setRows(10)
    expect(scroller.scrollHeight).toBe(500)
  })
})

describe.skipIf(!hasNativeTestRenderer)("Solid simulated wheel sign", () => {
  it("scrolls down on a positive deltaY and right on a positive deltaX, like WheelEvent", () => {
    let scroller!: HostElement
    const app = createTestRoot()
    app.render(() => (
      <div ref={scroller} style={{ width: 200, height: 100, overflow: "scroll" }}>
        <div style={{ width: 600, height: 400 }} />
      </div>
    ))
    app.renderer.nativeSimulateScrollWheel(100, 50, 0, 60)
    expect(scroller.scrollTop).toBe(60)
    app.renderer.nativeSimulateScrollWheel(100, 50, 0, -20)
    expect(scroller.scrollTop).toBe(40)
    app.renderer.nativeSimulateScrollWheel(100, 50, 30, 0)
    expect(scroller.scrollLeft).toBe(30)
  })
})
