/// Text selection under a transformed ancestor: a press, a drag and a double or triple click hit the text where it is
/// drawn. The pointer is in the window, the text is laid out in its own space, and each text maps the pointer through
/// its transform before it compares it with its box (Chrome hit-tests text through the transform, as
/// `caretPositionFromPoint` does).
///
/// Every case drags across the same text twice: untransformed, at points in the text's own space, and transformed, at
/// the points the transform draws them at. The two select the same characters.

import React from "react"
import { describe, expect, it } from "vitest"
import { defineKeyframes } from "../index"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SLOW = 120_000

const SENTENCE = "alpha beta gamma delta"

type Point = [number, number]

interface Case {
  name: string
  /** The wrapper's transform, from its top left corner. */
  style: React.CSSProperties
  /** Where the transform draws the point `(x, y)` of the text's space. */
  draw: (x: number, y: number) => Point
}

const rotated = (degrees: number, left: number, top: number): Case => {
  const radians = (degrees * Math.PI) / 180
  return {
    name: `rotate(${degrees}deg)`,
    style: { left, top, transform: `rotate(${degrees}deg)` },
    draw: (x, y) => [left + x * Math.cos(radians) - y * Math.sin(radians), top + x * Math.sin(radians) + y * Math.cos(radians)],
  }
}

const CASES: Case[] = [
  { name: "scale(1.5)", style: { left: 0, top: 0, transform: "scale(1.5)" }, draw: (x, y) => [x * 1.5, y * 1.5] },
  { name: "scale(0.5)", style: { left: 0, top: 0, transform: "scale(0.5)" }, draw: (x, y) => [x * 0.5, y * 0.5] },
  { name: "translate(260px, 90px)", style: { left: 0, top: 0, transform: "translate(260px, 90px)" }, draw: (x, y) => [x + 260, y + 90] },
  rotated(90, 400, 40),
  rotated(-90, 40, 500),
  rotated(30, 200, 20),
]

function Sentence({ style, textRef }: { style: React.CSSProperties; textRef?: (instance: PublicInstance | null) => void }) {
  return (
    <div ref={textRef} style={{ position: "absolute", transformOrigin: "0 0", ...style }}>
      <text style={{ fontSize: 20, lineHeight: "24px", color: "#000000" }}>{SENTENCE}</text>
    </div>
  )
}

/** The box of the text in its own space: a wrapper at the origin that nothing transforms. */
function ownBox() {
  let wrapper: PublicInstance | null = null
  const root = createTestRoot()
  root.render(<Sentence style={{ left: 0, top: 0 }} textRef={(instance) => (wrapper = instance)} />)
  try {
    const box = wrapper!.getBoundingClientRect()
    return { x: box.x, y: box.y, width: box.width, height: box.height }
  } finally {
    root.unmount()
  }
}

type Renderer = ReturnType<typeof createTestRoot>["renderer"]

function selectedBy(style: React.CSSProperties, act: (renderer: Renderer) => void) {
  const root = createTestRoot()
  root.render(<Sentence style={style} />)
  try {
    act(root.renderer)
    return root.renderer.getSelectedText()
  } finally {
    root.unmount()
  }
}

describeNative("text selection in a transformed page", () => {
  const box = ownBox()
  const y = box.y + box.height / 2
  const from: Point = [box.x + box.width * 0.2, y]
  const to: Point = [box.x + box.width * 0.7, y]
  const reference = selectedBy({ left: 0, top: 0 }, (renderer) => renderer.dragSelect(...from, ...to))

  it("selects a part of the text when it is not transformed", () => {
    expect(reference).not.toBeNull()
    expect(reference!.length).toBeGreaterThan(3)
    expect(reference!.length).toBeLessThan(SENTENCE.length)
  }, SLOW)

  for (const transformed of CASES) {
    describe(transformed.name, () => {
      const [downX, downY] = transformed.draw(...from)
      const [upX, upY] = transformed.draw(...to)

      it("selects what the same drag selects in the text's own space", () => {
        expect(selectedBy(transformed.style, (renderer) => renderer.dragSelect(downX, downY, upX, upY))).toBe(reference)
      }, SLOW)

      it("selects the same backwards", () => {
        const backwards = selectedBy({ left: 0, top: 0 }, (renderer) => renderer.dragSelect(...to, ...from))
        expect(selectedBy(transformed.style, (renderer) => renderer.dragSelect(upX, upY, downX, downY))).toBe(backwards)
      }, SLOW)

      it("selects the word under a double click and the text of a triple click", () => {
        const middle: Point = [box.x + box.width * 0.5, y]
        const click = (at: Point, count: number) => (renderer: Renderer) => {
          renderer.nativeSimulateMouseDown(...at, 0, undefined, count)
          renderer.nativeSimulateMouseUp(...at, 0, undefined, count)
        }
        const word = selectedBy({ left: 0, top: 0 }, click(middle, 2))
        expect(word).toMatch(/^\w+$/)
        expect(selectedBy(transformed.style, click(transformed.draw(...middle), 2))).toBe(word)
        expect(selectedBy(transformed.style, click(transformed.draw(...middle), 3))).toBe(SENTENCE)
      }, SLOW)
    })
  }

  it("starts a selection from a press on the text outside the box it is laid out in", () => {
    // At a scale of 3 the text is drawn three times as wide as it is laid out: x = 2 * width is outside its box in its
    // own space, and on the text where it is drawn.
    const style = { left: 0, top: 0, transform: "scale(3)" }
    const x = box.x + box.width * 2
    expect(x).toBeGreaterThan(box.x + box.width)
    const selected = selectedBy(style, (renderer) => renderer.dragSelect(x, y * 3, x + 40, y * 3))
    expect(selected).not.toBeNull()
    expect(selected!.length).toBeGreaterThan(0)
    expect(SENTENCE).toContain(selected!.trim())
  }, SLOW)

  it("does not start a selection from a press where the text is laid out and not drawn", () => {
    // Translated 260px, the text is drawn at x >= 260, and the press is at x = 40, where it is laid out.
    const selected = selectedBy({ left: 0, top: 0, transform: "translate(260px, 90px)" }, (renderer) =>
      renderer.dragSelect(box.x + 40, y, box.x + 90, y)
    )
    expect(selected).toBeNull()
  }, SLOW)

  it("hits nothing of text a transform flattens", () => {
    const selected = selectedBy({ left: 0, top: 0, transform: "scale(0)" }, (renderer) =>
      renderer.dragSelect(box.x + 5, y, box.x + 90, y)
    )
    expect(selected).toBeNull()
  }, SLOW)
})

describeNative("a selection dragged to the edge of a transformed list", () => {
  const scaled = (factor: number) =>
    ({ position: "absolute", left: 0, top: 0, transform: `scale(${factor})`, transformOrigin: "0 0" }) as const

  function List({ factor }: { factor: number }) {
    return (
      <div style={{ ...scaled(factor), width: 200, height: 160 }}>
        <virtual-list testId="list" overdraw={0} estimatedItemHeight={40} style={{ width: 200, height: 160 }}>
          {Array.from({ length: 30 }, (_, index) => (
            <div key={index} style={{ display: "flex", height: 40, flexShrink: 0, alignItems: "center" }}>
              <text>{`row-${index}`}</text>
            </div>
          ))}
        </virtual-list>
      </div>
    )
  }

  /** Presses on the first row, drags to `to` and lets the edge scroll run. */
  function selectedByDragging(factor: number, to: Point) {
    const root = createTestRoot({ width: 800, height: 600 })
    root.render(<List factor={factor} />)
    try {
      const renderer = root.renderer
      renderer.nativeSimulateMouseDown(8 * factor, 20 * factor)
      renderer.nativeSimulateMouseMove(...to)
      for (let tick = 0; tick < 12; tick += 1) renderer.advanceTime(24)
      renderer.nativeSimulateMouseUp(...to)
      return renderer.getSelectedText()
    } finally {
      root.unmount()
    }
  }

  const rowsOf = (selected: string | null) => (selected ?? "").split("\n").filter((line) => line.startsWith("row-")).length

  it("selects the rows it scrolls in, at a scale of 1.5", () => {
    // The list is drawn 300 x 240, and y = 235 is in its bottom edge zone.
    expect(rowsOf(selectedByDragging(1.5, [60, 235]))).toBeGreaterThan(5)
  }, SLOW)

  it("selects the rows it scrolls in, at a scale of 0.5", () => {
    // Drawn 100 x 80; y = 75 is 150 in the list, in its edge zone.
    expect(rowsOf(selectedByDragging(0.5, [30, 75]))).toBeGreaterThan(5)
  }, SLOW)

  it("selects only what the pointer is over where it is not at an edge", () => {
    // y = 150 is 100 in the list at a scale of 1.5: inside, away from the edge zone.
    expect(rowsOf(selectedByDragging(1.5, [60, 150]))).toBeLessThanOrEqual(4)
  }, SLOW)
})

defineKeyframes("tselect-slide", { from: { translate: "0px 0px" }, to: { translate: "200px 150px" } })

describeNative("text selection after patched ticks of a moving transform", () => {
  const TICKS = 30
  const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

  /** The page the other patch counters run on: rows of text beside the sentence that slides. */
  function Slide({ animation }: { animation?: string }) {
    return (
      <div style={{ position: "relative", width: 700, height: 560, overflow: "hidden" }}>
        <div style={{ display: "flex", flexDirection: "column", width: 200 }}>
          {Array.from({ length: 40 }, (_, row) => (
            <div key={row} style={{ borderWidth: 1, borderColor: "#cccccc", padding: 2 }}>
              <text style={{ fontSize: 11, color: "#333333" }}>{`row ${row}`}</text>
            </div>
          ))}
        </div>
        <div testId="slide" style={{ position: "absolute", left: 260, top: 20, animation }}>
          <text style={{ fontSize: 20, lineHeight: "24px", color: "#000000" }}>{SENTENCE}</text>
        </div>
      </div>
    )
  }

  async function settle(renderer: Renderer) {
    for (let frame = 0; frame < 12; frame++) {
      renderer.flush()
      renderer.dispatchNativeEvents()
      await sleep(8)
    }
  }

  const SLIDE_AT = 260

  async function dragAfter(animation: string | undefined, ticks: number) {
    const root = createTestRoot({ width: 760, height: 620 })
    const renderer = root.renderer
    try {
      renderer.clockPause()
      root.render(<Slide animation={animation} />)
      await settle(renderer)
      renderer.setInlineDraws(false)
      renderer.setA11yActive(false)
      for (let warm = 0; warm < 12; warm++) {
        renderer.clockAdvance(16)
        renderer.drawFrame()
      }
      const before = renderer.getPatchCounters().patchedFrames
      for (let tick = 0; tick < ticks; tick++) {
        renderer.clockAdvance(16)
        renderer.drawFrame()
      }
      const patched = renderer.getPatchCounters().patchedFrames - before
      // Reading the box is after the ticks: the next draw would be whole, and the press comes before it.
      const rect = renderer.getElementBounds(renderer.findByTestId("slide").id)
      return { rect, patched, renderer, root }
    } catch (error) {
      root.unmount()
      throw error
    }
  }

  it("presses where the patched ticks drew the text and selects what the same drag selects before it moved", async () => {
    const still = await dragAfter(undefined, 0)
    let reference: string | null
    try {
      const y = still.rect.y + still.rect.height / 2
      still.renderer.dragSelect(still.rect.x + still.rect.width * 0.2, y, still.rect.x + still.rect.width * 0.7, y)
      reference = still.renderer.getSelectedText()
    } finally {
      still.root.unmount()
    }
    expect(reference).not.toBeNull()
    expect(reference!.length).toBeGreaterThan(3)

    const moved = await dragAfter("tselect-slide 1s linear infinite", TICKS)
    try {
      if (process.env.GPUIX_RETAIN_OFF === undefined && process.env.GPUIX_PATCH_OFF === undefined) {
        expect(moved.patched, "the ticks were patched").toBeGreaterThanOrEqual(TICKS - 2)
      }
      expect(moved.rect.x, "the sentence slid").toBeGreaterThan(SLIDE_AT + 40)
      const y = moved.rect.y + moved.rect.height / 2
      moved.renderer.dragSelect(moved.rect.x + moved.rect.width * 0.2, y, moved.rect.x + moved.rect.width * 0.7, y)
      expect(moved.renderer.getSelectedText()).toBe(reference)
    } finally {
      moved.root.unmount()
    }
  }, SLOW)
})
