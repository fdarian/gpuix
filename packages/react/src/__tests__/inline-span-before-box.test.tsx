/// A text `<span>` right before an `inline-block` (no space between), in the layouts where the box
/// wraps to the next line, where it does not, and where the span wraps itself. The expected numbers are
/// from headless Chrome (chrome-headless-shell, never the Chrome app) for
///
///     <div class=r style="width:Wpx;font:20px/20px GpuixTestSquare"><span id=t>TEXT</span><span style="display:inline-block;width:Bpx;height:Hpx"></span></div>
///
/// with GpuixTestSquare 20px (each character 20px wide, ascent the whole em, no descent). Chrome's
/// `getBoundingClientRect` of the span, relative to the container, for (W, TEXT, B x H):
///
///     100, "AAA",     50x20  (the box wraps)         span 0,0 60x20, box 0,20 50x20, root 100x40
///     200, "AAA",     50x20  (it fits)               span 0,0 60x20, box 60,0 50x20, root 200x20
///     120, "AA AAAA", 50x20  (span wraps, box wraps) span 0,0 80x40, box 0,40 50x20, root 120x60
///     120, "AA AAAA", 30x20  (span wraps, box fits)  span 0,0 80x40, box 80,20 30x20, root 120x40
///     100, "AAA",     50x40  (a taller box wraps)    span 0,0 60x20, box 0,20 50x40, root 100x60
///
/// The space that ends the first line of a wrapped span hangs in Chrome and has no width, while gpui
/// measures it (see inline-bounds.test.tsx), so the wrapped cases are made with a first line that is
/// narrower than the second: the union of the fragments is then the same.

import fs from "fs"
import React from "react"
import { describe, expect, it } from "vitest"
import { ResizeObserver } from "../index"
import type { PublicInstance, ResizeObserverEntry } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SQUARE_FONT = fs.readFileSync(new URL("./fixtures/gpuix-test-square.ttf", import.meta.url))

const TEXT = { fontFamily: "GpuixTestSquare", fontSize: 20, lineHeight: "20px", color: "#000000" } as const

type Rect = [x: number, y: number, w: number, h: number]

type Layout = {
  width: number
  text: string
  box: [width: number, height: number]
  span: Rect
  boxRect: Rect
  root: [width: number, height: number]
  /** The text a find matches in the span, and the rect of each line of the match. */
  find: string
  wash: Rect[]
}

const LAYOUTS: Record<string, Layout> = {
  "the box wraps": {
    width: 100,
    text: "AAA",
    box: [50, 20],
    span: [0, 0, 60, 20],
    boxRect: [0, 20, 50, 20],
    root: [100, 40],
    find: "AAA",
    wash: [[0, 0, 60, 20]],
  },
  "the box fits": {
    width: 200,
    text: "AAA",
    box: [50, 20],
    span: [0, 0, 60, 20],
    boxRect: [60, 0, 50, 20],
    root: [200, 20],
    find: "AAA",
    wash: [[0, 0, 60, 20]],
  },
  "the span wraps and the box wraps": {
    width: 120,
    text: "AA AAAA",
    box: [50, 20],
    span: [0, 0, 80, 40],
    boxRect: [0, 40, 50, 20],
    root: [120, 60],
    find: "AAAA",
    wash: [[0, 20, 80, 20]],
  },
  "the span wraps and the box fits": {
    width: 120,
    text: "AA AAAA",
    box: [30, 20],
    span: [0, 0, 80, 40],
    boxRect: [80, 20, 30, 20],
    root: [120, 40],
    find: "AAAA",
    wash: [[0, 20, 80, 20]],
  },
  "a taller box wraps": {
    width: 100,
    text: "AAA",
    box: [50, 40],
    span: [0, 0, 60, 20],
    boxRect: [0, 20, 50, 40],
    root: [100, 60],
    find: "AAA",
    wash: [[0, 0, 60, 20]],
  },
}

function expectRect(actual: Rect, expected: Rect, context: string) {
  expect(actual.map((value) => Math.round(value)), context).toEqual(expected)
}

describeNative("a text span right before an inline-block", () => {
  for (const [name, layout] of Object.entries(LAYOUTS)) {
    const page = (ref: (name: string) => (instance: PublicInstance | null) => void, find?: string) => (
      <div
        ref={ref("root")}
        style={{ width: layout.width, ...TEXT }}
        highlight={find === undefined ? undefined : { query: find, activeIndex: 0 }}
      >
        <span ref={ref("span")}>{layout.text}</span>
        <span ref={ref("box")} style={{ display: "inline-block", width: layout.box[0], height: layout.box[1] }} />
      </div>
    )

    const setup = () => {
      const root = createTestRoot()
      root.renderer.registerFont(SQUARE_FONT)
      const refs = new Map<string, PublicInstance>()
      const ref = (key: string) => (instance: PublicInstance | null) => {
        if (instance) refs.set(key, instance)
      }
      const rect = (key: string): Rect => {
        const element = refs.get(key)
        expect(element, `no element ${key}`).toBeDefined()
        const box = element!.getBoundingClientRect()
        const origin = refs.get("root")!.getBoundingClientRect()
        return [box.x - origin.x, box.y - origin.y, box.width, box.height]
      }
      return { ...root, ref, rect, refs }
    }

    it(`has its rect when ${name}`, () => {
      const { render, ref, rect, unmount } = setup()
      try {
        render(page(ref))
        expect(rect("root").slice(2), "root").toEqual(layout.root)
        expectRect(rect("span"), layout.span, "span")
        expectRect(rect("box"), layout.boxRect, "box")
      } finally {
        unmount()
      }
    })

    it(`reports the size of the span to a ResizeObserver when ${name}`, () => {
      const { render, renderer, ref, refs, unmount } = setup()
      try {
        const entries: ResizeObserverEntry[] = []
        render(page(ref))
        new ResizeObserver((batch) => entries.push(...batch)).observe(refs.get("span")!)
        renderer.dispatchNativeEvents()
        expect(entries, "the span was never reported").toHaveLength(1)
        expect(entries[0]!.borderBoxSize[0]!.inlineSize).toBeCloseTo(layout.span[2], 0)
        expect(entries[0]!.borderBoxSize[0]!.blockSize).toBeCloseTo(layout.span[3], 0)
      } finally {
        unmount()
      }
    })

    it(`washes the text of the span, and only that, when ${name}`, () => {
      const { render, renderer, ref, refs, unmount } = setup()
      try {
        render(page(ref, layout.find))
        const origin = refs.get("root")!.getBoundingClientRect()
        const hits = renderer.getPaintedHighlights()
        expect(hits, "one match").toHaveLength(1)
        const rects = hits[0]!.rects.map((r): Rect => [r.x - origin.x, r.y - origin.y, r.width, r.height])
        expect(rects, "one rect per line of the match").toHaveLength(layout.wash.length)
        rects.forEach((rect, line) => expectRect(rect, layout.wash[line]!, `wash ${line}`))
      } finally {
        unmount()
      }
    })

    it.runIf(layout.text.includes(" "))(`washes a match across the break of the span when ${name}`, () => {
      const { render, renderer, ref, refs, unmount } = setup()
      try {
        render(page(ref, "AA AAAA"))
        const origin = refs.get("root")!.getBoundingClientRect()
        const rects = renderer
          .getPaintedHighlights()
          .flatMap((hit) => hit.rects)
          .map((r): Rect => [r.x - origin.x, r.y - origin.y, r.width, r.height])
        expect(rects).toHaveLength(2)
        expect(rects[0]!.slice(0, 2)).toEqual([0, 0])
        expect(rects[0]![2]).toBeGreaterThanOrEqual(40)
        expect(rects[0]![3]).toBe(20)
        expectRect(rects[1]!, [0, 20, 80, 20], "second line")
      } finally {
        unmount()
      }
    })

    it(`selects the text of the span when ${name}`, () => {
      const { render, renderer, ref, refs, unmount } = setup()
      try {
        render(page(ref))
        const origin = refs.get("root")!.getBoundingClientRect()
        const lastLine = layout.span[3] - 10
        const selected = renderer.dragSelect(origin.x + 1, origin.y + 10, origin.x + layout.span[2] + 2, origin.y + lastLine)
        expect(selected).toBe(layout.text)
      } finally {
        unmount()
      }
    })
  }

  // <div style="width:100px;font:20px/20px GpuixTestSquare"><span id=t>AAA<span style="display:inline-block;width:50px;height:20px"></span></span></div>
  // Chrome: span 0,0 60x40, box 0,20 50x20; with the box first, box 0,0 and span 0,0 60x40, the text on the second line.
  it.each([
    ["text, then the box", true, [0, 0, 60, 40], [0, 20, 50, 20]],
    ["the box, then text", false, [0, 0, 60, 40], [0, 0, 50, 20]],
  ] as const)("has the rect of a span that holds %s and wraps", (_name, textFirst, spanRect, boxRect) => {
    const root = createTestRoot()
    root.renderer.registerFont(SQUARE_FONT)
    const refs = new Map<string, PublicInstance>()
    const ref = (key: string) => (instance: PublicInstance | null) => {
      if (instance) refs.set(key, instance)
    }
    try {
      const inlineBlock = <span ref={ref("box")} style={{ display: "inline-block", width: 50, height: 20 }} />
      root.render(
        <div ref={ref("root")} style={{ width: 100, ...TEXT }}>
          <span ref={ref("span")}>
            {textFirst ? "AAA" : inlineBlock}
            {textFirst ? inlineBlock : "AAA"}
          </span>
        </div>
      )
      const origin = refs.get("root")!.getBoundingClientRect()
      const relative = (key: string): Rect => {
        const box = refs.get(key)!.getBoundingClientRect()
        return [box.x - origin.x, box.y - origin.y, box.width, box.height]
      }
      expectRect(relative("span"), [...spanRect], "span")
      expectRect(relative("box"), [...boxRect], "box")
    } finally {
      root.unmount()
    }
  })
})
