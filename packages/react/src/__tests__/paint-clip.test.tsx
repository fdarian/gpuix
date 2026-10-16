import path from "path"
import type { ReactNode } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR, type Pixels } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const FILL = "#ff0000"

function shoot(name: string, tree: ReactNode, scrollTo?: { testId: string; y: number }) {
  const { render, renderer, unmount } = createTestRoot()
  try {
    render(tree)
    renderer.dispatchNativeEvents()
    if (scrollTo) {
      renderer.scrollTo(renderer.findByTestId(scrollTo.testId)!.id, 0, scrollTo.y)
      renderer.flush()
    }
    const file = path.join(SHOTS_DIR, `paint-clip-${name}.png`)
    renderer.captureScreenshot(file)
    return readLogicalPixels(file, renderer)
  } finally {
    unmount()
  }
}

function isFill(pixels: Pixels, x: number, y: number) {
  const [r, g, b, a] = pixels.at(x, y)
  return a > 200 && r > 200 && g < 60 && b < 60
}

/** The window shows through wherever nothing was painted. */
function isUntouched(pixels: Pixels, x: number, y: number) {
  const reference = pixels.at(pixels.width - 3, pixels.height - 3)
  const sampled = pixels.at(x, y)
  return sampled.every((channel, index) => Math.abs(channel - reference[index]!) <= 2)
}

describeNative("rounded overflow clip", () => {
  it("clips a filled child flush to the corner of a rounded overflow:hidden parent", () => {
    const pixels = shoot(
      "rounded-parent",
      <div
        style={{
          width: 100,
          height: 100,
          borderRadius: 30,
          overflow: "hidden",
          backgroundColor: "#0000ff",
        }}
      >
        <div style={{ width: 100, height: 100, backgroundColor: FILL }} />
      </div>
    )
    for (const [x, y] of [
      [1, 1],
      [98, 1],
      [1, 98],
      [98, 98],
      [4, 4],
    ] as const) {
      expect(isFill(pixels, x, y), `corner (${x}, ${y}) is not the child's colour`).toBe(false)
      expect(isUntouched(pixels, x, y), `corner (${x}, ${y}) is outside the rounded box`).toBe(true)
    }
    expect(isFill(pixels, 50, 50)).toBe(true)
    expect(isFill(pixels, 50, 2), "the straight edges stay filled").toBe(true)
    expect(isFill(pixels, 2, 50)).toBe(true)
    expect(isFill(pixels, 12, 12), "inside the arc").toBe(true)
  })

  it("clips to the padding box inside the border, with the radius reduced by the border width", () => {
    const pixels = shoot(
      "bordered-parent",
      <div
        style={{
          width: 100,
          height: 100,
          borderRadius: 30,
          borderWidth: 10,
          borderColor: "#0000ff",
          overflow: "hidden",
        }}
      >
        <div style={{ width: 80, height: 80, backgroundColor: FILL }} />
      </div>
    )
    // The padding box starts at (10, 10) with a 20px inner radius: its corner is cut.
    expect(isFill(pixels, 11, 11)).toBe(false)
    expect(isFill(pixels, 13, 13)).toBe(false)
    expect(isFill(pixels, 20, 20), "inside the inner arc").toBe(true)
    expect(isFill(pixels, 50, 11), "straight edge of the padding box").toBe(true)
    expect(isFill(pixels, 5, 50), "the border is not painted over").toBe(false)
  })

  it("follows an outer rounded clip through a nested one", () => {
    const pixels = shoot(
      "nested",
      <div
        style={{
          width: 100,
          height: 100,
          borderRadius: 40,
          overflow: "hidden",
        }}
      >
        <div
          style={{
            width: 60,
            height: 60,
            borderRadius: 10,
            overflow: "hidden",
          }}
        >
          <div style={{ width: 100, height: 100, backgroundColor: FILL }} />
        </div>
      </div>
    )
    // The inner box shares the outer top-left corner, so that corner takes the larger radius.
    expect(isFill(pixels, 3, 3), "outer 40px arc").toBe(false)
    expect(isFill(pixels, 8, 8), "outside the 40px arc, though inside a 10px one").toBe(false)
    expect(isFill(pixels, 30, 30)).toBe(true)
    // The inner box's own bottom-right corner is rounded by its 10px radius.
    expect(isFill(pixels, 58, 58)).toBe(false)
    expect(isFill(pixels, 50, 50)).toBe(true)
    expect(isFill(pixels, 58, 30), "straight inner edge").toBe(true)
  })

  it("rounds a scroller at the top and after scrolling to the bottom", () => {
    const tree = (
      <div
        testId="scroller"
        style={{ width: 100, height: 100, borderRadius: 30, overflow: "scroll" }}
      >
        <div style={{ width: 100, height: 300, backgroundColor: FILL }} />
      </div>
    )
    const top = shoot("scroller-top", tree)
    expect(isFill(top, 1, 1)).toBe(false)
    expect(isFill(top, 98, 1)).toBe(false)
    expect(isFill(top, 50, 50)).toBe(true)

    const bottom = shoot("scroller-bottom", tree, { testId: "scroller", y: 200 })
    expect(isFill(bottom, 1, 98), "content scrolled under the bottom corner is clipped").toBe(false)
    expect(isFill(bottom, 98, 98)).toBe(false)
    expect(isFill(bottom, 1, 1)).toBe(false)
    expect(isFill(bottom, 50, 50)).toBe(true)
  })

  it("keeps a rectangular clip rectangular when there is no radius", () => {
    const pixels = shoot(
      "square-parent",
      <div style={{ width: 100, height: 100, overflow: "hidden" }}>
        <div style={{ width: 200, height: 200, backgroundColor: FILL }} />
      </div>
    )
    expect(isFill(pixels, 0, 0)).toBe(true)
    expect(isFill(pixels, 99, 99)).toBe(true)
    expect(isFill(pixels, 100, 100)).toBe(false)
  })
})
