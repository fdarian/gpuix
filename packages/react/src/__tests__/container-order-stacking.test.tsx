/// An `@container` block that turns an element into a flex or grid container (or back to a block)
/// is seen by what the container's children are built by: `order` sorts them only in a flex or grid
/// container, and the `zIndex` of a static child stacks only when it is a flex or grid item. Both
/// follow the display the block gives, in the frame that lays the query container out, in both
/// directions. Chrome's behaviour is the CSS one: `order` and a static item's `zIndex` do nothing
/// in a block container, and do in a flex or grid one.

import type { ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const WIDE = "(min-width: 250px)"
const DISPLAYS = ["flex", "inline-flex", "grid", "inline-grid"] as const

function setup() {
  const root = createTestRoot()
  const nameOf = (id: number) => root.renderer.getElement(id)?.testId
  const boxOf = (testId: string) => {
    const element = root.renderer.findByTestId(testId)
    expect(element, `missing testId ${testId}`).toBeDefined()
    const bounds = root.renderer.getElementBounds(element!.id)
    expect(bounds, `no painted bounds for ${testId}`).toBeDefined()
    return bounds!
  }
  /** The names of `testIds`, in the order the boxes lie in: top to bottom, then left to right. */
  const layoutOrder = (...testIds: string[]) =>
    testIds
      .map((testId) => ({ testId, box: boxOf(testId) }))
      .sort((a, b) => a.box.y - b.box.y || a.box.x - b.box.x)
      .map((entry) => entry.testId)
  /** The test ids under the point, topmost first. */
  const hitsAt = (x: number, y: number) =>
    root.renderer
      .getElementsAt(x, y)
      .map(nameOf)
      .filter((name): name is string => name !== undefined)
  /** The test ids the boxes drew their labels by, in paint order. */
  const painted = () => {
    root.renderer.flush()
    return root.renderer.getPaintedText()
  }
  const paintedFrames = (change: () => void): number => {
    const before = root.renderer.getPaintedFrameCount()
    change()
    return root.renderer.getPaintedFrameCount() - before
  }
  return { ...root, layoutOrder, hitsAt, painted, paintedFrames }
}

/** The query container is `width` wide; its child is a block that `display` turns into a container of items. */
const orderApp = (width: number, display: string): ReactElement => (
  <div style={{ width, containerType: "inline-size" }}>
    <div testId="parent" style={{ display: "block", "@container": { [WIDE]: { display } } }}>
      <div testId="a" style={{ order: 2, width: 10, height: 10 }} />
      <div testId="b" style={{ width: 10, height: 10 }} />
      <div testId="c" style={{ order: -1, width: 10, height: 10 }} />
    </div>
  </div>
)

const sorted = ["c", "b", "a"]
const document = ["a", "b", "c"]

/**
 * `b` sits exactly on `a`, one below the other and then pulled up over it, so only the stacking
 * decides which one is at the point. `a` has a `zIndex` and `b` is positioned with none: `a` is above
 * `b` when it is a flex or grid item, and its `zIndex` does nothing when it is not.
 */
const stackApp = (width: number, display: string): ReactElement => (
  <div style={{ width, containerType: "inline-size" }}>
    <div
      testId="parent"
      style={{ display: "block", flexDirection: "column", "@container": { [WIDE]: { display } } }}
    >
      <div testId="a" style={{ width: 100, height: 100, zIndex: 1 }}>
        a
      </div>
      <div testId="b" style={{ width: 100, height: 100, position: "relative", top: -100 }}>
        b
      </div>
    </div>
  </div>
)

describeNative("an @container block that makes an element a flex or grid container", () => {
  describe("sorts the children by order", () => {
    it.each(DISPLAYS)("%s: in the first frame, wide and narrow", (display) => {
      for (const [width, expected] of [
        [300, sorted],
        [200, document],
      ] as const) {
        const { render, layoutOrder, paintedFrames, unmount } = setup()
        try {
          expect(paintedFrames(() => render(orderApp(width, display)))).toBe(1)
          expect(layoutOrder("a", "b", "c"), `${display} at ${width}`).toEqual(expected)
        } finally {
          unmount()
        }
      }
    })

    it.each(DISPLAYS)("%s: both ways in the frame that resizes the container", (display) => {
      const { render, layoutOrder, paintedFrames, unmount } = setup()
      try {
        render(orderApp(300, display))
        expect(paintedFrames(() => render(orderApp(200, display)))).toBe(1)
        expect(layoutOrder("a", "b", "c")).toEqual(document)
        expect(paintedFrames(() => render(orderApp(300, display)))).toBe(1)
        expect(layoutOrder("a", "b", "c")).toEqual(sorted)
        expect(paintedFrames(() => render(orderApp(200, display)))).toBe(1)
        expect(layoutOrder("a", "b", "c")).toEqual(document)
      } finally {
        unmount()
      }
    })
  })

  describe("stacks the zIndex of a static item", () => {
    const stack = (display: string, hits: string[], painted: string[]) => ({
      display,
      hits,
      painted,
    })
    const wide = (display: string) => stack(display, ["a", "b"], ["b", "a"])
    const narrow = (display: string) => stack(display, ["b", "a"], ["a", "b"])

    it.each(DISPLAYS)("%s: in the first frame, wide and narrow", (display) => {
      for (const [width, expected] of [
        [300, wide(display)],
        [200, narrow(display)],
      ] as const) {
        const { render, hitsAt, painted, paintedFrames, unmount } = setup()
        try {
          expect(paintedFrames(() => render(stackApp(width, display)))).toBe(1)
          expect(hitsAt(50, 50).slice(0, 2), `${display} at ${width}`).toEqual(expected.hits)
          expect(painted().filter((label) => label === "a" || label === "b")).toEqual(expected.painted)
        } finally {
          unmount()
        }
      }
    })

    it.each(DISPLAYS)("%s: both ways in the frame that resizes the container", (display) => {
      const { render, hitsAt, painted, paintedFrames, unmount } = setup()
      try {
        const check = (width: number, expected: ReturnType<typeof wide>) => {
          expect(paintedFrames(() => render(stackApp(width, display)))).toBe(1)
          expect(hitsAt(50, 50).slice(0, 2), `${display} at ${width}`).toEqual(expected.hits)
          expect(painted().filter((label) => label === "a" || label === "b")).toEqual(expected.painted)
        }
        render(stackApp(300, display))
        check(200, narrow(display))
        check(300, wide(display))
        check(200, narrow(display))
      } finally {
        unmount()
      }
    })
  })
})

/**
 * What hit testing and stacking read of an element is the style it was painted with, the `@container`
 * blocks the container above selects included. `b` lies exactly over `a` (pulled up by its negative
 * margin), so which of them is at the point is decided by the property under test.
 */
const readerApp = (width: number, a: object, b: object): ReactElement => (
  <div style={{ width, containerType: "inline-size" }}>
    <div testId="a" style={{ width: 100, height: 100, "@container": { [WIDE]: a } }}>
      a
    </div>
    <div testId="b" style={{ width: 100, height: 100, marginTop: -100, "@container": { [WIDE]: b } }}>
      b
    </div>
  </div>
)

describeNative("an @container block that sets what hit testing and stacking read", () => {
  const cases: {
    name: string
    a: object
    b: object
    /** The test ids at the point, topmost first, in a container wide enough for the block and in a narrow one. */
    wide: string[]
    narrow: string[]
    paintedWide?: string[]
    paintedNarrow?: string[]
  }[] = [
    {
      name: "a positioned element with a zIndex goes over a later sibling",
      a: { position: "relative", zIndex: 1 },
      b: {},
      wide: ["a", "b"],
      narrow: ["b", "a"],
      paintedWide: ["b", "a"],
      paintedNarrow: ["a", "b"],
    },
    {
      name: "opacity under 1 makes a stacking context above a later static sibling",
      a: { opacity: 0.5 },
      b: {},
      wide: ["a", "b"],
      narrow: ["b", "a"],
      paintedWide: ["b", "a"],
      paintedNarrow: ["a", "b"],
    },
    {
      name: "visibility hidden is not hit",
      a: {},
      b: { visibility: "hidden" },
      wide: ["a"],
      narrow: ["b", "a"],
    },
    {
      name: "pointerEvents none is not hit",
      a: {},
      b: { pointerEvents: "none" },
      wide: ["a"],
      narrow: ["b", "a"],
    },
  ]
  for (const { name, a, b, wide, narrow, paintedWide, paintedNarrow } of cases) {
    it(name, () => {
      const { render, hitsAt, painted, paintedFrames, unmount } = setup()
      try {
        const check = (width: number, hits: string[], order?: string[]) => {
          expect(paintedFrames(() => render(readerApp(width, a, b)))).toBe(1)
          expect(hitsAt(50, 50).filter((id) => id === "a" || id === "b"), `${name} at ${width}`).toEqual(hits)
          if (order) expect(painted().filter((label) => label === "a" || label === "b"), `${name} at ${width}`).toEqual(order)
        }
        check(300, wide, paintedWide)
        check(200, narrow, paintedNarrow)
        check(300, wide, paintedWide)
      } finally {
        unmount()
      }
    })
  }
})
