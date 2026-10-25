/// Scrolls that follow the pointer or the focus, in a transformed page: the pointer and the targets are in the window,
/// and a scroller compares them with its viewport in the space it lays out in.
///
/// - Dragging a selection to a list's edge, or a text control's, scrolls where the edge is drawn.
/// - `scrollIntoView` of a virtual list's row compares the row's box with the viewport in the list's space.
/// - Tab into skipped content scrolls the nearest scroller by the box it lands in, in that scroller's space.

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer, type TestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SLOW = 120_000

const scaled = (factor: number) => ({ position: "absolute", left: 0, top: 0, transform: `scale(${factor})`, transformOrigin: "0 0" }) as const

/** A list of 160px that shows four rows of 40px, drawn `factor` times as big. */
function ScaledList({ factor }: { factor: number }) {
  return (
    <div style={{ ...scaled(factor), width: 200, height: 160 }}>
      <virtual-list testId="list" overdraw={0} estimatedItemHeight={40} style={{ width: 200, height: 160 }}>
        {Array.from({ length: 30 }, (_, index) => (
          <div key={index} testId={`row-${index}`} style={{ display: "flex", height: 40, flexShrink: 0, alignItems: "center" }}>
            <text>{`row-${index}`}</text>
          </div>
        ))}
      </virtual-list>
    </div>
  )
}

function mount(node: React.ReactNode, size = { width: 800, height: 600 }) {
  const root = createTestRoot(size)
  root.render(node)
  const renderer = root.renderer
  const id = (testId: string) => renderer.findByTestId(testId)!.id
  return { ...root, renderer, id, scrollTop: (testId: string) => renderer.getScrollOffset(id(testId))![1]! }
}

/** Presses at `from`, drags to `to`, and lets the edge scroll run for a while. */
function drag(renderer: TestRenderer, from: [number, number], to: [number, number], ticks = 8) {
  renderer.nativeSimulateMouseDown(...from)
  renderer.nativeSimulateMouseMove(...to)
  for (let tick = 0; tick < ticks; tick += 1) renderer.advanceTime(24)
}

describeNative("a selection dragged to the edge of a transformed list", () => {
  it("scrolls it where the bottom edge is drawn, at a scale of 1.5", () => {
    const { renderer, scrollTop, unmount } = mount(<ScaledList factor={1.5} />)
    try {
      // The list is drawn 300 x 240, and its bottom edge zone, 133..160 in it, is 200..240.
      drag(renderer, [5, 20], [50, 235])
      expect(scrollTop("list")).toBeGreaterThan(40)
      renderer.nativeSimulateMouseUp(50, 235)
    } finally {
      unmount()
    }
  }, SLOW)

  it("does not scroll it for a point the list's own size would call near the edge, at a scale of 1.5", () => {
    const { renderer, scrollTop, unmount } = mount(<ScaledList factor={1.5} />)
    try {
      // y = 150 is 100 in the list: a third of the way up its edge zone, which starts at 133.
      drag(renderer, [5, 20], [50, 150])
      expect(scrollTop("list")).toBe(0)
      renderer.nativeSimulateMouseUp(50, 150)
    } finally {
      unmount()
    }
  }, SLOW)

  it("scrolls it at the bottom edge it is drawn with at a scale of 0.5", () => {
    const { renderer, scrollTop, unmount } = mount(<ScaledList factor={0.5} />)
    try {
      // The list is drawn 100 x 80, and y = 75 is 150 in it, inside the edge zone that starts at 133.
      drag(renderer, [8, 20], [30, 75])
      expect(scrollTop("list")).toBeGreaterThan(40)
      renderer.nativeSimulateMouseUp(30, 75)
    } finally {
      unmount()
    }
  }, SLOW)

  it("does not scroll it above the edge it is drawn with at a scale of 0.5", () => {
    const { renderer, scrollTop, unmount } = mount(<ScaledList factor={0.5} />)
    try {
      // y = 50 is 100 in the list.
      drag(renderer, [8, 20], [30, 50])
      expect(scrollTop("list")).toBe(0)
      renderer.nativeSimulateMouseUp(30, 50)
    } finally {
      unmount()
    }
  }, SLOW)

  it("does not scroll it once the transform flattens it, and stops", () => {
    const { render, renderer, scrollTop, unmount } = mount(<ScaledList factor={1} />)
    try {
      renderer.nativeSimulateMouseDown(5, 20)
      renderer.nativeSimulateMouseMove(50, 150)
      render(<ScaledList factor={0} />)
      renderer.nativeSimulateMouseMove(50, 155)
      for (let tick = 0; tick < 8; tick += 1) renderer.advanceTime(24)
      const stoppedAt = scrollTop("list")
      for (let tick = 0; tick < 8; tick += 1) renderer.advanceTime(24)
      expect(scrollTop("list")).toBe(stoppedAt)
      renderer.nativeSimulateMouseUp(50, 155)
    } finally {
      unmount()
    }
  }, SLOW)
})

describeNative("a selection dragged to the edge of a text control in a transformed page", () => {
  const lines = Array.from({ length: 30 }, (_, index) => `line ${index}`).join("\n")

  /** The line of the offset `index` of the text. */
  const lineOf = (index: number) => lines.slice(0, index).split("\n").length - 1

  /** A textarea of 80px, four lines of 20px, drawn `factor` times as big, with the caret at the start. Drags from its
   *  first line to `y` (a window y) and returns the line the selection ends at. */
  function dragTo(factor: number, y: number) {
    let control: { selectionEnd: number; setSelectionRange(start: number, end: number): void } | undefined
    const { renderer, unmount } = mount(
      <div style={{ ...scaled(factor), width: 200, height: 80 }}>
        <textarea
          ref={(element: typeof control) => (control = element)}
          value={lines}
          readOnly
          style={{ width: 200, height: 80, lineHeight: "20px" }}
        />
      </div>
    )
    try {
      control!.setSelectionRange(0, 0)
      renderer.flush()
      renderer.nativeSimulateMouseDown(5 * factor, 10 * factor)
      renderer.nativeSimulateMouseMove(40 * factor, y)
      for (let tick = 0; tick < 10; tick += 1) renderer.advanceTime(24)
      renderer.nativeSimulateMouseUp(40 * factor, y)
      return lineOf(control!.selectionEnd)
    } finally {
      unmount()
    }
  }

  it("scrolls it where the bottom edge is drawn", () => {
    // Drawn 300 x 120, y = 150 is 100 in it, below its 80: the selection goes past the four lines it shows.
    expect(dragTo(1.5, 150)).toBeGreaterThanOrEqual(5)
    // Drawn 100 x 40, y = 50 is 100 in it.
    expect(dragTo(0.5, 50)).toBeGreaterThanOrEqual(5)
  }, SLOW)

  it("does not scroll it for a point inside the box it is drawn in", () => {
    // y = 110 is 73 in the control, whose height is 80: inside, though the window y is past 80.
    expect(dragTo(1.5, 110)).toBe(3)
    // y = 30 is 60 in it.
    expect(dragTo(0.5, 30)).toBe(3)
  }, SLOW)
})

describeNative("scrollIntoView of a virtual list's row in a transformed list", () => {
  it("compares the row's box with the viewport in the list's space", () => {
    const { renderer, id, scrollTop, unmount } = mount(<ScaledList factor={2} />)
    try {
      // Row 3 is the last the list shows: drawn at 240..320 in the window, where the list's viewport (0..160) ends.
      renderer.scrollIntoView(id("row-3"), "nearest", "nearest")
      expect(scrollTop("list")).toBe(0)
      renderer.scrollIntoView(id("row-3"), "start", "nearest")
      expect(scrollTop("list")).toBe(120)
    } finally {
      unmount()
    }
  }, SLOW)
})

describeNative("Tab into skipped content in a transformed scroller", () => {
  const ENTRIES = 40
  const ENTRY = 100
  const SIZE = { width: 900, height: 780 }

  /** A scroller of 400px with entries of 100px, `content-visibility: auto`, drawn half as big. */
  const Page = () => (
    <div style={{ ...scaled(0.5), width: 600, height: 400, display: "flex", flexDirection: "column" }}>
      <div testId="scroller" style={{ overflowY: "scroll", flexGrow: 1, display: "flex", flexDirection: "column" }}>
        {Array.from({ length: ENTRIES }, (_, index) => (
          <div
            key={index}
            testId={`entry-${index}`}
            style={{ height: ENTRY, flexShrink: 0, contentVisibility: "auto", containIntrinsicSize: `auto ${ENTRY}px` } as React.CSSProperties}
          >
            <div testId={`btn-${index}`} tabIndex={0} style={{ height: 24 }} />
          </div>
        ))}
      </div>
    </div>
  )

  const settle = (renderer: TestRenderer) => {
    for (let frames = 0; frames < 8; frames++) {
      renderer.drawFrame()
      renderer.flush()
    }
  }

  it("scrolls the button to the edge of the scroller it is drawn at", () => {
    const { renderer, id, scrollTop, unmount } = mount(<Page />, SIZE)
    try {
      renderer.setInlineDraws(false)
      settle(renderer)
      const built = (name: string) => Boolean(renderer.getBuiltElementBounds(id(name)))
      const skipped = Array.from({ length: ENTRIES }, (_, index) => index).find((index) => !built(`entry-${index}`) || !built(`btn-${index}`))!
      expect(skipped, "some entry is skipped").toBeGreaterThan(0)
      renderer.focusElement(id(`btn-${skipped - 1}`))
      settle(renderer)
      const before = scrollTop("scroller")
      renderer.simulateKeystrokes("Tab")
      settle(renderer)
      expect(renderer.getElement(renderer.getFocusedElementId())?.testId).toBe(`btn-${skipped}`)
      expect(scrollTop("scroller"), "the first Tab scrolled").toBeGreaterThan(before)
      const button = renderer.getElementBounds(id(`btn-${skipped}`))!
      const scroller = renderer.getElementBounds(id("scroller"))!
      expect(button.y + button.height, "the bottom of the button is the bottom of the scroller").toBeCloseTo(scroller.y + scroller.height, 0)
    } finally {
      unmount()
    }
  }, SLOW)
})
