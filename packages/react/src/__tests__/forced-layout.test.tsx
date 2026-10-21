/**
 * A bounds or scroll read on a dirty window forces a layout, as a browser does, and never a
 * paint: it is a layout-only pass, the window stays dirty, and the next frame draws once.
 *
 * The test window draws a dirty window whenever an update flushes, which would hide every pass,
 * so these tests turn that off (`setInlineDraws(false)`) and draw frames with `drawFrame`.
 */

import { useRef, useState } from "react"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { ResizeObserver } from "../index"
import { flushSync } from "../reconciler/reconciler"
import type { TestRenderer } from "../testing"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const TEXT = { fontSize: 16, lineHeight: "20px", fontFamily: "Menlo", color: "#000000" } as const

type Spans = Record<string, PublicInstance | null>

/** Bordered and padded boxes, a scroller, a wrapped paragraph with inline spans, an absolute
 *  child and a grid: every kind of box a read can ask about. */
function Fixture(props: { grow: boolean; spans: Spans }) {
  const span = (name: string) => (instance: PublicInstance | null) => {
    props.spans[name] = instance
  }
  return (
    <div testId="root" style={{ width: 420, height: 600, flexDirection: "column" }}>
      <div
        testId="bordered"
        style={{
          width: 200,
          height: props.grow ? 90 : 50,
          padding: 6,
          borderWidth: 4,
          borderStyle: "solid",
          borderColor: "#f00",
        }}
      >
        <div testId="inner" style={{ width: 40, height: 20 }} />
      </div>
      <div testId="scroller" style={{ width: 150, height: 80, overflow: "scroll" }}>
        <div testId="content" style={{ height: props.grow ? 600 : 300 }} />
      </div>
      <div testId="positioned" style={{ position: "relative", width: 100, height: 40 }}>
        <div testId="absolute" style={{ position: "absolute", left: 10, top: 5, width: 20, height: 20 }} />
      </div>
      <text testId="paragraph" style={{ ...TEXT, width: props.grow ? 120 : 300 }}>
        {"Lead words "}
        <span ref={span("first")} style={{ paddingLeft: 4, paddingRight: 4 }}>
          a span with enough words to wrap across lines
        </span>
        {" and a "}
        <span ref={span("second")}>tail</span>
        {" to finish"}
      </text>
      <div testId="grid" style={{ display: "grid", gridTemplateColumns: "50px 70px", gap: 3 }}>
        <div testId="cell-a" style={{ height: 12 }} />
        <div testId="cell-b" style={{ height: props.grow ? 30 : 12 }} />
      </div>
    </div>
  )
}

const HOSTS = [
  "root",
  "bordered",
  "inner",
  "scroller",
  "content",
  "positioned",
  "absolute",
  "paragraph",
  "grid",
  "cell-a",
  "cell-b",
]

function snapshot(renderer: TestRenderer, spans: Spans) {
  const hosts = Object.fromEntries(
    HOSTS.map((testId) => {
      const id = renderer.findByTestId(testId)!.id
      return [testId, { bounds: renderer.getElementBounds(id), scroll: renderer.getScrollMetrics(id) }]
    })
  )
  const inline = Object.fromEntries(
    Object.entries(spans).map(([name, instance]) => [name, instance!.getBoundingClientRect()])
  )
  return { hosts, inline }
}

describeNative("a read on a dirty window", () => {
  it("publishes the same boxes from a layout-only pass as from a draw", () => {
    const drawnSpans: Spans = {}
    const drawn = createTestRoot()
    drawn.render(<Fixture grow spans={drawnSpans} />)
    const reference = snapshot(drawn.renderer, drawnSpans)
    drawn.unmount()
    expect(reference.hosts.bordered!.bounds).not.toBeNull()
    expect(Object.keys(reference.inline)).toEqual(["first", "second"])
    expect(reference.inline.first!.height, "the span wraps").toBeGreaterThan(20)
    expect(reference.hosts.scroller!.scroll![3], "scrollHeight").toBe(600)

    const laidOutSpans: Spans = {}
    const laidOut = createTestRoot()
    try {
      const renderer = laidOut.renderer
      renderer.setInlineDraws(false)
      laidOut.render(<Fixture grow spans={laidOutSpans} />)
      const painted = renderer.getPaintedFrameCount()
      const passes = renderer.getLayoutPassCount()

      const candidate = snapshot(renderer, laidOutSpans)

      expect(renderer.getPaintedFrameCount(), "the reads painted nothing").toBe(painted)
      expect(renderer.getLayoutPassCount() - passes, "the reads shared one pass").toBe(1)
      expect(candidate).toEqual(reference)
    } finally {
      laidOut.renderer.setInlineDraws(true)
      laidOut.unmount()
    }
  })

  it("sees a style write without a paint, and the next frame is the only draw", () => {
    function Box() {
      const [width, setWidth] = useState(100)
      const ref = useRef<PublicInstance>(null)
      return (
        <div
          testId="trigger"
          style={{ width: 300, height: 100 }}
          onPointerMove={() => {
            flushSync(() => setWidth(160))
            seen.push(ref.current!.getBoundingClientRect().width)
          }}
        >
          <div ref={ref} testId="box" style={{ width, height: 40 }} />
        </div>
      )
    }
    const seen: number[] = []
    const { render, renderer, unmount } = createTestRoot()
    try {
      renderer.setInlineDraws(false)
      render(<Box />)
      const id = renderer.findByTestId("box")!.id
      expect(renderer.getElementBounds(id)!.width).toBe(100)
      renderer.drawFrame()
      const painted = renderer.getPaintedFrameCount()
      const passes = renderer.getLayoutPassCount()

      renderer.dispatchMouseMove(5, 5)

      expect(seen, "the handler's read, inside the input being dispatched").toEqual([160])
      expect(renderer.getPaintedFrameCount(), "no paint for the read").toBe(painted)
      expect(renderer.getLayoutPassCount() - passes).toBe(1)
      expect(renderer.getElementBounds(id)!.width, "outside a handler too").toBe(160)

      renderer.drawFrame()
      expect(renderer.getPaintedFrameCount() - painted, "the frame draws once").toBe(1)
    } finally {
      renderer.setInlineDraws(true)
      unmount()
    }
  })

  it("does a pass for a write and none on a clean window", () => {
    function Box(props: { width: number }) {
      return <div testId="box" style={{ width: props.width, height: 40 }} />
    }
    const { render, renderer, unmount } = createTestRoot()
    try {
      renderer.setInlineDraws(false)
      render(<Box width={100} />)
      const id = renderer.findByTestId("box")!.id
      // The first read after a commit notifies the view, whatever drew before it.
      renderer.getElementBounds(id)
      renderer.drawFrame()
      const passes = () => renderer.getLayoutPassCount()
      const start = passes()

      expect(renderer.getElementBounds(id)!.width).toBe(100)
      expect(passes() - start, "a clean window serves the last frame").toBe(0)

      render(<Box width={160} />)
      expect(renderer.getElementBounds(id)!.width).toBe(160)
      expect(passes() - start).toBe(1)
      expect(renderer.getElementBounds(id)!.width).toBe(160)
      expect(passes() - start, "nothing invalidated since: no second pass").toBe(1)

      render(<Box width={200} />)
      expect(renderer.getElementBounds(id)!.width).toBe(200)
      expect(passes() - start, "a new invalidation, a new pass").toBe(2)

      renderer.drawFrame()
      const afterFrame = passes()
      expect(renderer.getElementBounds(id)!.width).toBe(200)
      expect(passes(), "the frame drew it: clean again").toBe(afterFrame)
    } finally {
      renderer.setInlineDraws(true)
      unmount()
    }
  })

  it("reports to a ResizeObserver from the frame, not from a pass that only laid out", () => {
    // The DOM delivers observations in the rendering update, never from a forced layout
    // (`fixtures/scroll-order-oracle.json`, `forcedLayout`).
    const { render, renderer, unmount } = createTestRoot()
    try {
      renderer.setInlineDraws(false)
      let element!: PublicInstance
      const sizes: number[] = []
      render(
        <div
          ref={(instance) => {
            if (instance) element = instance
          }}
          style={{ width: 120, height: 30 }}
        />
      )
      const painted = renderer.getPaintedFrameCount()
      new ResizeObserver((entries) => sizes.push(entries[0]!.borderBoxSize[0]!.inlineSize)).observe(element)
      renderer.getElementBounds(element.id)
      renderer.dispatchNativeEvents()

      expect(sizes, "the read laid out and reported nothing").toEqual([])
      expect(renderer.getPaintedFrameCount()).toBe(painted)

      renderer.drawFrame()
      renderer.dispatchNativeEvents()
      expect(sizes).toEqual([120])
    } finally {
      renderer.setInlineDraws(true)
      unmount()
    }
  })
})
