import path from "path"
import type { ReactNode } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR, type Pixels } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const WIDTH = 300
const HEIGHT = 100
const GLYPHS = { fontSize: 80, lineHeight: "90px", fontWeight: 900 } as const

function shoot(name: string, tree: ReactNode) {
  const { render, renderer, unmount } = createTestRoot()
  try {
    render(tree)
    renderer.dispatchNativeEvents()
    const file = path.join(SHOTS_DIR, `background-clip-text-${name}.png`)
    renderer.captureScreenshot(file)
    return readLogicalPixels(file, renderer)
  } finally {
    unmount()
  }
}

/** A white stage with the clipped element in its top-left corner. */
function stage(style: Record<string, unknown>, children: ReactNode) {
  return (
    <div style={{ width: WIDTH, height: HEIGHT, backgroundColor: "#ffffff" }}>
      <div style={{ width: WIDTH, height: HEIGHT, ...style }}>{children}</div>
    </div>
  )
}

function inked(pixels: Pixels) {
  const found: Array<{ x: number; y: number; rgb: [number, number, number] }> = []
  for (let y = 0; y < HEIGHT; y++) {
    for (let x = 0; x < WIDTH; x++) {
      const [r, g, b] = pixels.at(x, y)
      if (r < 200 || g < 200 || b < 200) found.push({ x, y, rgb: [r, g, b] })
    }
  }
  return found
}

/** Pixels that are fully inside a glyph stroke, not an anti-aliased edge. */
function solidInk(pixels: Pixels) {
  return inked(pixels).filter(({ x, y }) => {
    const around = [pixels.at(x - 1, y), pixels.at(x + 1, y), pixels.at(x, y - 1), pixels.at(x, y + 1)]
    return around.every(([r, g, b]) => r < 200 || g < 200 || b < 200)
  })
}

/** The ink in the first and last fifth of its horizontal extent. The gradient
 *  spans the whole block, which is wider than the text, so the ends of the
 *  gradient are not reached. */
function ends(found: ReturnType<typeof inked>) {
  const xs = found.map(({ x }) => x)
  const min = Math.min(...xs)
  const span = Math.max(...xs) - min
  return {
    left: found.filter(({ x }) => x < min + span * 0.2),
    right: found.filter(({ x }) => x > min + span * 0.8),
  }
}

function expectConfinedToGlyphs(found: ReturnType<typeof inked>) {
  expect(found.length).toBeGreaterThan(200)
  // Glyphs of "MMM" cover well under half of the 300x100 box that the
  // background would fill unclipped.
  expect(found.length).toBeLessThan((WIDTH * HEIGHT) / 2)
}

const GRADIENT = "linear-gradient(to right, #ff0000, #0000ff)"

describeNative("backgroundClip: text", () => {
  it("shows a gradient inside the glyphs and nothing outside them", () => {
    const pixels = shoot(
      "gradient",
      stage(
        { backgroundImage: GRADIENT, backgroundClip: "text", color: "transparent" },
        <text style={GLYPHS}>MMM</text>
      )
    )
    const found = solidInk(pixels)
    expectConfinedToGlyphs(found)

    const { left, right } = ends(found)
    expect(left.length).toBeGreaterThan(20)
    expect(right.length).toBeGreaterThan(20)
    for (const { rgb } of left) {
      expect(rgb[0]).toBeGreaterThan(rgb[2])
    }
    for (const { rgb } of right) {
      expect(rgb[2]).toBeGreaterThan(rgb[0])
    }
    // The corners of the box stay the backdrop.
    expect(pixels.at(1, 1)).toEqual([255, 255, 255, 255])
    expect(pixels.at(WIDTH - 2, HEIGHT - 2)).toEqual([255, 255, 255, 255])
  })

  it("works through the background shorthand", () => {
    const longhand = shoot(
      "longhand",
      stage(
        { backgroundImage: GRADIENT, backgroundClip: "text", color: "transparent" },
        <text style={GLYPHS}>MMM</text>
      )
    )
    const shorthand = shoot(
      "shorthand",
      stage({ background: `${GRADIENT} text`, color: "transparent" }, <text style={GLYPHS}>MMM</text>)
    )
    const expected = inked(longhand)
    expect(expected.length).toBeGreaterThan(200)
    expect(inked(shorthand)).toEqual(expected)
  })

  it("paints through text in a nested inline span", () => {
    const pixels = shoot(
      "nested-span",
      stage(
        { backgroundImage: GRADIENT, backgroundClip: "text", color: "transparent" },
        <text style={{ ...GLYPHS, fontWeight: 400 }}>
          <span style={{ fontWeight: 900 }}>MM</span>M
        </text>
      )
    )
    const found = solidInk(pixels)
    expectConfinedToGlyphs(found)
    const { left, right } = ends(found)
    expect(left.length).toBeGreaterThan(20)
    expect(left.every(({ rgb }) => rgb[0] > rgb[2])).toBe(true)
    expect(right.length).toBeGreaterThan(20)
    expect(right.every(({ rgb }) => rgb[2] > rgb[0])).toBe(true)
    expect(pixels.at(1, 1)).toEqual([255, 255, 255, 255])
  })

  it("confines a solid backgroundColor to the glyphs", () => {
    const pixels = shoot(
      "solid-color",
      stage(
        { backgroundColor: "#00aa00", backgroundClip: "text", color: "transparent" },
        <text style={GLYPHS}>MMM</text>
      )
    )
    const found = solidInk(pixels)
    expectConfinedToGlyphs(found)
    // Every ink pixel is the green or an anti-aliased blend of it with white.
    for (const { rgb } of found) {
      expect(Math.abs(rgb[0] - rgb[2])).toBeLessThanOrEqual(4)
      expect(rgb[1]).toBeGreaterThanOrEqual(rgb[0])
    }
    expect(found.filter(({ rgb }) => rgb[0] < 8 && Math.abs(rgb[1] - 170) < 8 && rgb[2] < 8).length).toBeGreaterThan(200)
    expect(pixels.at(1, 1)).toEqual([255, 255, 255, 255])
  })

  it("without the clip the same background fills the whole box", () => {
    const pixels = shoot(
      "unclipped",
      stage({ backgroundImage: GRADIENT, color: "transparent" }, <text style={GLYPHS}>MMM</text>)
    )
    expect(pixels.at(1, 1)[0]).toBeGreaterThan(200)
    expect(pixels.at(1, 1)[2]).toBeLessThan(60)
    expect(inked(pixels).length).toBeGreaterThan((WIDTH * HEIGHT) * 0.9)
  })
})
