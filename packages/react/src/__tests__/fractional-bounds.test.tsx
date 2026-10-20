// @ts-nocheck
/**
 * Boxes are read back as layout computed them, in fractions of a pixel (1/64, as Chrome's `LayoutUnit`),
 * and not as they are snapped to device pixels when painted (#45). The integer APIs round as Chrome does.
 *
 * Numbers are from headless Chrome 155 on macOS, with the markup beside each case.
 *   row     `<div style="display:flex;width:300px;align-items:flex-start;font:13px/20px 'Helvetica Neue'">
 *            <div id=chip style="display:flex;padding:0 6px;flex-shrink:0">Label text</div>
 *            <div id=after style="width:10px;height:10px;flex-shrink:0"></div></div>`
 *           -> chip 0 x 0, 69.3125 x 20; after 69.3125 x 0, 10 x 10.
 *   thirds  `<div style="display:flex;width:100px;align-items:flex-start">` of three
 *           `<div style="width:33.3333%;height:10px;flex-shrink:0">`, the second with `border:1px solid;box-sizing:border-box`
 *           -> x 0, 33.328125, 66.65625, each 33.328125 x 10; clientWidth 33, 31, 33; clientHeight 10, 8, 10.
 *   scroller `<div style="display:flex;width:100px;align-items:flex-start">` of a `<div style="width:33.3333%;height:10px">` and
 *           `<div id=scroller style="width:33.3333%;height:15px;display:flex;overflow:auto;border:1px solid;box-sizing:border-box">` holding
 *           `<div style="display:flex;padding:0 6px;flex-shrink:0;height:30px">Label text Label text</div>`
 *           -> the scroller at 33.328125 x 33.328125 x 15; clientWidth 31, clientHeight 13, scrollWidth 130, scrollHeight 30
 *           (the content is 130.234375 wide).
 *   inline  `<div style="width:200px">Hello <span id=sp>wide world</span> end</div>` after the chip, in the same row
 *           -> the span at 102.53125, 63.296875 wide (112.53125 with the 10px box between).
 */

import React from "react"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { IntersectionObserver, ResizeObserver } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { elementsAtPoints, idsByTestId } from "./retained-harness.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const verifying = process.env.GPUIX_RETAIN_VERIFY !== undefined
const TIMEOUT = 60_000

const FONT = { fontFamily: "Helvetica Neue", fontSize: 13, lineHeight: "20px", color: "#000000" }

function Page({ ref }: { ref: (name: string) => (instance: PublicInstance | null) => void }) {
  return (
    <div style={{ ...FONT, flexDirection: "column", width: 400 }}>
      <div testId="row" ref={ref("row")} style={{ display: "flex", width: 300, alignItems: "flex-start" }}>
        <div testId="chip" ref={ref("chip")} style={{ display: "flex", padding: "0 6px", flexShrink: 0 }}>
          <text>Label text</text>
        </div>
        <div testId="after" ref={ref("after")} style={{ width: 10, height: 10, flexShrink: 0 }} />
        <div testId="para" ref={ref("para")} style={{ width: 200 }}>
          <text>
            Hello <span testId="span" ref={ref("span")}>wide world</span> end
          </text>
        </div>
      </div>
      <div testId="thirds" ref={ref("thirds")} style={{ display: "flex", width: 100, alignItems: "flex-start" }}>
        <div testId="t1" ref={ref("t1")} style={{ width: "33.3333%", height: 10, flexShrink: 0 }} />
        <div
          testId="t2"
          ref={ref("t2")}
          style={{
            width: "33.3333%",
            height: 10,
            flexShrink: 0,
            borderWidth: 1,
            borderStyle: "solid",
            borderColor: "#ff0000",
          }}
        />
        <div testId="t3" ref={ref("t3")} style={{ width: "33.3333%", height: 10, flexShrink: 0 }} />
      </div>
      <div testId="scrolls" style={{ display: "flex", width: 100, alignItems: "flex-start" }}>
        <div style={{ width: "33.3333%", height: 10, flexShrink: 0 }} />
        <div
          testId="scroller"
          ref={ref("scroller")}
          style={{
            width: "33.3333%",
            height: 15,
            flexShrink: 0,
            display: "flex",
            overflow: "auto",
            borderWidth: 1,
            borderStyle: "solid",
            borderColor: "#ff0000",
          }}
        >
          <div style={{ display: "flex", padding: "0 6px", height: 30, flexShrink: 0, alignItems: "flex-start" }}>
            <text style={{ whiteSpace: "nowrap" }}>Label text Label text</text>
          </div>
        </div>
      </div>
    </div>
  )
}

function setup(window = { width: 480, height: 300 }) {
  const root = createTestRoot(window)
  const refs = new Map<string, PublicInstance>()
  const ref = (name: string) => (instance: PublicInstance | null) => {
    if (instance) refs.set(name, instance)
  }
  root.render(<Page ref={ref} />)
  root.renderer.flush()
  const ids = idsByTestId(root.renderer)
  const box = (testId: string) => {
    const bounds = root.renderer.getElementBounds(ids.get(testId))
    return [bounds.x, bounds.y, bounds.width, bounds.height]
  }
  const element = (testId: string) => refs.get(testId)!
  return { ...root, ids, box, element }
}

/** Chrome's values to 1/64: the text it shapes can be a unit wider or narrower than gpui's. */
const near = (actual: number[], chrome: number[]) => {
  expect(actual.length).toBe(chrome.length)
  actual.forEach((value, index) =>
    expect(Math.abs(value - chrome[index]!), `${actual} != ${chrome}`).toBeLessThanOrEqual(1 / 64 + 1e-6)
  )
}

describeNative("fractional bounds", () => {
  it("reads a box after a text where the text ends, and a percentage as layout cut it", () => {
    const { box, unmount } = setup()
    try {
      near(box("chip"), [0, 0, 69.3125, 20])
      near(box("after"), [69.3125, 0, 10, 10])
      expect(box("after")[0]).not.toBe(Math.round(box("after")[0]))
      near(box("t1"), [0, 20, 33.328125, 10])
      near(box("t2"), [33.328125, 20, 33.328125, 10])
      near(box("t3"), [66.65625, 20, 33.328125, 10])
      near(box("scroller"), [33.328125, 30, 33.328125, 15])
    } finally {
      unmount()
    }
  })

  it("reads an inline box from the origin of its paragraph, which a text before it moves by a fraction", () => {
    const { box, unmount } = setup()
    try {
      const span = box("span")
      near([span[0], span[2]], [112.53125, 63.296875])
      // The paragraph starts at 69.3125: a span is not placed from where the paragraph is painted.
      expect(span[0]).not.toBe(Math.round(span[0]))
    } finally {
      unmount()
    }
  })

  it("gives getBoundingClientRect and getElementBounds the same fractions, with the derived edges", () => {
    const { element, box, unmount } = setup()
    try {
      const rect = element("t2").getBoundingClientRect()
      near([rect.x, rect.y, rect.width, rect.height], box("t2"))
      expect(rect.left).toBe(rect.x)
      expect(rect.right).toBe(rect.x + rect.width)
      near([rect.right], [66.65625])
      near([rect.bottom], [30])
    } finally {
      unmount()
    }
  })

  it("rounds clientWidth, clientHeight, scrollWidth and scrollHeight to integers as Chrome does", () => {
    const { element, unmount } = setup()
    try {
      const sizes = (testId: string) => {
        const e = element(testId)
        return [e.clientWidth, e.clientHeight, e.scrollWidth, e.scrollHeight]
      }
      expect(sizes("t1")).toEqual([33, 10, 33, 10])
      // The padding box: the 1px border is out of it, 31.328125 wide.
      expect(sizes("t2")).toEqual([31, 8, 31, 8])
      expect(sizes("t3")).toEqual([33, 10, 33, 10])
      expect(sizes("scroller")).toEqual([31, 13, 130, 30])
    } finally {
      unmount()
    }
  })

  it("leaves the scroll offsets as they are", () => {
    const { element, renderer, unmount } = setup()
    try {
      const scroller = element("scroller")
      renderer.scrollTo(scroller.id, 12.5, 7.25, "instant")
      renderer.flush()
      expect([scroller.scrollLeft, scroller.scrollTop]).toEqual([12.5, 7.25])
    } finally {
      unmount()
    }
  })

  it("reports a ResizeObserver the fractions of the border box and the content box", () => {
    const { element, renderer, unmount } = setup()
    try {
      const entries = []
      new ResizeObserver((batch) => entries.push(...batch)).observe(element("t2"))
      renderer.dispatchNativeEvents()
      expect(entries).toHaveLength(1)
      const entry = entries[0]
      expect(entry.borderBoxSize).toEqual([{ inlineSize: 33.328125, blockSize: 10 }])
      expect(entry.contentBoxSize).toEqual([{ inlineSize: 31.328125, blockSize: 8 }])
      expect(entry.contentRect).toMatchObject({ x: 0, y: 0, width: 31.328125, height: 8 })
    } finally {
      unmount()
    }
  })

  it("reports an IntersectionObserver the rect getBoundingClientRect gives, and a root element's own", () => {
    const { element, renderer, unmount } = setup()
    try {
      const entries = []
      const observer = new IntersectionObserver((batch) => entries.push(...batch))
      observer.observe(element("t2"))
      observer.observe(element("span"))
      renderer.dispatchNativeEvents()
      expect(entries).toHaveLength(2)
      for (const entry of entries) {
        const rect = entry.target.getBoundingClientRect()
        expect(entry.boundingClientRect).toEqual(rect)
        expect(entry.intersectionRect).toEqual(rect)
      }

      const inside = []
      const rooted = new IntersectionObserver((batch) => inside.push(...batch), { root: element("t2") })
      rooted.observe(element("t2"))
      renderer.dispatchNativeEvents()
      expect(inside).toHaveLength(1)
      // The root's padding box: the 1px border is out of it.
      const root = inside[0].rootBounds
      near([root.x, root.y, root.width, root.height], [34.328125, 21, 31.328125, 8])
    } finally {
      unmount()
    }
  })

  it("reports onLayout the box getBoundingClientRect gives", () => {
    const root = createTestRoot({ width: 480, height: 300 })
    try {
      const events = []
      root.render(
        <div style={{ ...FONT, display: "flex", alignItems: "flex-start" }}>
          <div style={{ display: "flex", padding: "0 6px", flexShrink: 0 }}>
            <text>Label text</text>
          </div>
          <div
            testId="laid"
            style={{ width: "33.3333%", height: 10 }}
            onLayout={(event) => events.push([event.x, event.y, event.width, event.height])}
          />
        </div>
      )
      root.renderer.dispatchNativeEvents()
      expect(events).toHaveLength(1)
      const bounds = root.renderer.getElementBounds(root.renderer.findByTestId("laid").id)
      expect(events[0]).toEqual([bounds.x, bounds.y, bounds.width, bounds.height])
      near([events[0][0], events[0][2]], [69.3125, 160])
      expect(events[0][3]).toBe(10)
    } finally {
      root.unmount()
    }
  })

  it("finds an element by its fractional box, where painting has snapped it", () => {
    const { ids, renderer, unmount } = setup()
    try {
      const names = new Map([...ids].map(([testId, id]) => [id, testId]))
      // t1 ends at 33.328125 and t2 starts there: 33.2 is in t1, 33.4 in t2, whatever the painted edge is.
      const found = elementsAtPoints(renderer, [
        [33.2, 25],
        [33.4, 25],
      ]).map((hit) => hit.map((id) => names.get(id)).filter((name) => name?.startsWith("t")))
      expect(found[0]).toContain("t1")
      expect(found[0]).not.toContain("t2")
      expect(found[1]).toContain("t2")
      expect(found[1]).not.toContain("t1")
    } finally {
      unmount()
    }
  })
})

const ROWS = 40

function Rows() {
  return (
    <div style={{ ...FONT, width: 400, height: 320, padding: 10 }}>
      <div testId="list" style={{ width: 380, height: 300, overflowY: "auto" }}>
        {Array.from({ length: ROWS }, (_, index) => (
          <div key={index} testId={`row-${index}`} style={{ display: "flex", height: "26.6666%", flexShrink: 0, alignItems: "flex-start" }}>
            <div style={{ display: "flex", padding: "0 6px", flexShrink: 0 }}>
              <text>{`Label text ${index}`}</text>
            </div>
            <div testId={`after-${index}`} style={{ width: "33.3333%", height: "33.3333%", flexShrink: 0, backgroundColor: "#335577" }} />
            <text testId={`label-${index}`}>
              Hello <span testId={`span-${index}`}>wide world</span>
            </text>
          </div>
        ))}
      </div>
    </div>
  )
}

/** Every box of the page after each scroll step, with retention on or off. */
function scrolled(retention: boolean, steps: number[]) {
  const root = createTestRoot({ width: 420, height: 320 })
  const { renderer } = root
  try {
    if (!retention) renderer.setRetainEnabled(false)
    root.render(<Rows />)
    for (let frame = 0; frame < 5; frame++) renderer.flush()
    const ids = idsByTestId(renderer)
    const read = () => {
      const boxes = {}
      for (const [testId, id] of ids) {
        const bounds = renderer.getElementBounds(id)
        if (bounds) boxes[testId] = [bounds.x, bounds.y, bounds.width, bounds.height]
      }
      return boxes
    }
    const frames = [read()]
    for (const step of steps) {
      renderer.nativeSimulateScrollWheel(200, 150, 0, step)
      renderer.flush()
      renderer.nativeSimulateScrollWheel(200, 150, 0, 4.25)
      renderer.flush()
      frames.push(read())
    }
    return { frames, counters: renderer.getRetainedCounters(), mismatches: renderer.getRetentionMismatches() }
  } finally {
    root.unmount()
  }
}

describeNative("fractional bounds of rows a frame replays or moves", () => {
  it(
    "are the ones a build reads, in place and after a scroll moves them by whole device pixels",
    () => {
      const steps = [3, 9.5, 20, 60, 10, 140]
      const built = scrolled(false, steps)
      const replayed = scrolled(true, steps)
      expect(replayed.counters.mismatches, replayed.mismatches.join("\n")).toBe(0)
      if (!verifying) expect(replayed.counters.relocations).toBeGreaterThan(0)
      expect(replayed.frames.length).toBe(built.frames.length)
      for (let frame = 0; frame < built.frames.length; frame++) {
        expect(replayed.frames[frame], `frame ${frame}`).toEqual(built.frames[frame])
      }
      const some = built.frames.at(-1)
      const fractional = Object.values(some).filter((box) => box.some((value) => !Number.isInteger(value)))
      expect(fractional.length, "the page has fractional boxes to compare").toBeGreaterThan(20)
    },
    TIMEOUT
  )
})
