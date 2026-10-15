/// `wheel` through the DOM dispatcher, end to end on the GPU test renderer: one event
/// per input, aimed at what is under the pointer, with a path, capture and bubble.

import { beforeEach, describe, expect, it } from "vitest"
import type { HostWheelEvent, ScrollEvent } from "@gpuix/native/host"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("wheel events", () => {
  let testRoot: ReturnType<typeof createTestRoot>
  let log: string[]

  beforeEach(() => {
    testRoot = createTestRoot()
    log = []
  })

  const id = (testId: string) => testRoot.renderer.findByTestId(testId)!.id
  const offset = (testId: string) => testRoot.renderer.getScrollOffset(id(testId))!
  const record = (name: string) => (event: HostWheelEvent) => {
    log.push(`${name}:${event.type}`)
  }

  describe("target and path", () => {
    it("bubbles from the element under the pointer to an ancestor's onWheel", () => {
      const seen: { target: number; currentTarget: number | null; phase: number; path: number[] }[] = []
      testRoot.render(
        <div
          testId="outer"
          style={{ width: 300, height: 300 }}
          onWheel={(event: HostWheelEvent) =>
            seen.push({
              target: event.target,
              currentTarget: event.currentTarget,
              phase: event.eventPhase,
              path: event.path!,
            })
          }
        >
          <div testId="inner" style={{ width: 100, height: 100 }} />
        </div>
      )

      testRoot.renderer.nativeSimulateScrollWheel(50, 50, 0, -30)

      expect(seen).toHaveLength(1)
      expect(seen[0]).toMatchObject({
        target: id("inner"),
        currentTarget: id("outer"),
        phase: 3,
      })
      expect(seen[0]!.path.slice(0, 2)).toEqual([id("inner"), id("outer")])
    })

    it("does not reach an element the pointer is not over", () => {
      testRoot.render(
        <div style={{ width: 400, height: 300 }}>
          <div style={{ width: 100, height: 100 }} onWheel={record("left")} />
          <div style={{ width: 100, height: 100 }} onWheel={record("right")} />
        </div>
      )
      testRoot.renderer.nativeSimulateScrollWheel(250, 50, 0, -30)
      expect(log).toEqual([])
    })

    it("runs capture handlers outermost first, then the target, then bubble handlers", () => {
      testRoot.render(
        <div
          style={{ width: 400, height: 300 }}
          onWheelCapture={record("outer-capture")}
          onWheel={record("outer")}
        >
          <div style={{ width: 300, height: 200 }} onWheel={record("middle")}>
            <div
              style={{ width: 100, height: 100 }}
              onWheelCapture={record("inner-capture")}
              onWheel={record("inner")}
            />
          </div>
        </div>
      )

      testRoot.renderer.nativeSimulateScrollWheel(50, 50, 0, -30)

      expect(log).toEqual([
        "outer-capture:wheel",
        "inner-capture:wheel",
        "inner:wheel",
        "middle:wheel",
        "outer:wheel",
      ])
    })

    it("stops at the element whose handler calls stopPropagation", () => {
      testRoot.render(
        <div style={{ width: 400, height: 300 }} onWheel={record("outer")}>
          <div
            style={{ width: 100, height: 100 }}
            onWheel={(event: HostWheelEvent) => {
              log.push("inner")
              event.stopPropagation()
            }}
          />
        </div>
      )

      testRoot.renderer.nativeSimulateScrollWheel(50, 50, 0, -30)

      expect(log).toEqual(["inner"])
    })

    it("fires once per input on each element of the path", () => {
      testRoot.render(
        <div style={{ width: 400, height: 300 }} onWheel={record("outer")}>
          <div style={{ width: 100, height: 100 }} onWheel={record("inner")} />
        </div>
      )

      testRoot.renderer.nativeSimulateScrollWheel(50, 50, 0, -30)
      expect(log).toEqual(["inner:wheel", "outer:wheel"])

      testRoot.renderer.nativeSimulateScrollWheel(50, 50, 0, -30)
      testRoot.renderer.nativeSimulateScrollWheel(50, 50, 0, -30)
      expect(log).toHaveLength(6)
    })

    it("fires once on a scroller that also scrolls", () => {
      testRoot.render(
        <div
          testId="scroller"
          style={{ width: 200, height: 100, overflow: "scroll" }}
          onWheel={record("scroller")}
        >
          <div style={{ height: 500 }} />
        </div>
      )

      testRoot.renderer.nativeSimulateScrollWheel(50, 50, 0, -30)

      expect(log).toEqual(["scroller:wheel"])
      expect(offset("scroller")).toEqual([0, -30])
    })

    it("fires whether or not anything can scroll", () => {
      testRoot.render(
        <div
          testId="scroller"
          style={{ width: 200, height: 100, overflow: "scroll" }}
          onWheel={record("scroller")}
        >
          <div style={{ height: 50 }} />
        </div>
      )

      testRoot.renderer.nativeSimulateScrollWheel(50, 50, 0, -30)

      expect(log).toEqual(["scroller:wheel"])
    })
  })

  describe("payload", () => {
    it("carries the pointer fields, the DOM-signed delta and the gesture phase", () => {
      const seen: HostWheelEvent[] = []
      testRoot.render(
        <div style={{ position: "absolute", left: 20, top: 10, width: 200, height: 200 }}>
          <div
            testId="target"
            style={{ position: "absolute", left: 10, top: 5, width: 100, height: 100 }}
            onWheel={(event: HostWheelEvent) => seen.push(event)}
          />
        </div>
      )

      testRoot.renderer.nativeSimulateScrollWheel(50, 40, -7, -50, "shift", "moved")

      expect(seen).toHaveLength(1)
      const event = seen[0]!
      expect(event).toMatchObject({
        type: "wheel",
        target: id("target"),
        deltaX: 7,
        deltaY: 50,
        deltaMode: 0,
        clientX: 50,
        clientY: 40,
        offsetX: 20,
        offsetY: 25,
        buttons: 0,
        pointerType: "mouse",
        precise: true,
        touchPhase: "moved",
      })
      expect(event.modifiers?.shift).toBe(true)
    })

    it("reports a downward wheel as positive and an upward one as negative", () => {
      const deltas: number[] = []
      testRoot.render(
        <div
          style={{ width: 100, height: 100 }}
          onWheel={(event: HostWheelEvent) => deltas.push(event.deltaY)}
        />
      )
      testRoot.renderer.nativeSimulateScrollWheel(50, 50, 0, -10)
      testRoot.renderer.nativeSimulateScrollWheel(50, 50, 0, 10)
      expect(deltas).toEqual([10, -10])
    })
  })

  describe("scrolling", () => {
    it("is cancelled for that input by preventDefault, and the next input scrolls", () => {
      let prevent = true
      testRoot.render(
        <div
          testId="scroller"
          style={{ width: 200, height: 100, overflow: "scroll" }}
          onWheel={(event: HostWheelEvent) => {
            if (prevent) event.preventDefault()
          }}
        >
          <div style={{ height: 500 }} />
        </div>
      )

      testRoot.renderer.nativeSimulateScrollWheel(50, 50, 0, -30)
      expect(offset("scroller")).toEqual([0, 0])

      prevent = false
      testRoot.renderer.nativeSimulateScrollWheel(50, 50, 0, -30)
      expect(offset("scroller")).toEqual([0, -30])
    })

    it("is cancelled by an ancestor's preventDefault in either phase", () => {
      let phase: "capture" | "bubble" = "capture"
      const prevent = (event: HostWheelEvent) => event.preventDefault()
      testRoot.render(
        <div
          style={{ width: 300, height: 300 }}
          onWheelCapture={(event: HostWheelEvent) => phase === "capture" && prevent(event)}
          onWheel={(event: HostWheelEvent) => phase === "bubble" && prevent(event)}
        >
          <div
            testId="scroller"
            style={{ width: 200, height: 100, overflow: "scroll" }}
          >
            <div style={{ height: 500 }} />
          </div>
        </div>
      )

      testRoot.renderer.nativeSimulateScrollWheel(50, 50, 0, -30)
      expect(offset("scroller")).toEqual([0, 0])
      phase = "bubble"
      testRoot.renderer.nativeSimulateScrollWheel(50, 50, 0, -30)
      expect(offset("scroller")).toEqual([0, 0])
    })

    it("is not cancelled by stopPropagation", () => {
      testRoot.render(
        <div
          testId="scroller"
          style={{ width: 200, height: 100, overflow: "scroll" }}
          onWheel={(event: HostWheelEvent) => event.stopPropagation()}
        >
          <div style={{ height: 500 }} />
        </div>
      )
      testRoot.renderer.nativeSimulateScrollWheel(50, 50, 0, -30)
      expect(offset("scroller")).toEqual([0, -30])
    })

    it("keeps scroll on the scroller: it does not bubble to an ancestor's onScroll", () => {
      const scrolls: string[] = []
      testRoot.render(
        <div
          testId="outer"
          style={{ width: 300, height: 300 }}
          onScroll={() => scrolls.push("outer")}
        >
          <div
            testId="inner"
            style={{ width: 200, height: 100, overflow: "scroll" }}
            onScroll={(event: ScrollEvent) => scrolls.push(`inner:${event.scrollTop}`)}
          >
            <div style={{ height: 500 }} />
          </div>
        </div>
      )

      testRoot.renderer.nativeSimulateScrollWheel(50, 50, 0, -30)

      expect(scrolls).toEqual(["inner:30"])
    })
  })
})
