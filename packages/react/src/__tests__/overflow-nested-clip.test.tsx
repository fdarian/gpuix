import fs from "fs"
import type { ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rgba = [number, number, number, number]
type Sampler = (x: number, y: number) => Rgba

const STAGE = 200

let counter = 0

function paint(tree: ReactElement): Sampler {
  const testRoot = createTestRoot()
  try {
    testRoot.render(tree)
    testRoot.renderer.dispatchNativeEvents()
    const file = `${SHOTS_DIR}/overflow-nested-clip-${counter++}.png`
    fs.rmSync(file, { force: true })
    testRoot.renderer.captureScreenshot(file)
    const pixels = readLogicalPixels(file, testRoot.renderer)
    return (x, y) => pixels.at(x, y)
  } finally {
    testRoot.unmount()
  }
}

/** A white stage with the box under test 20px in from its corner; `positioned` makes it the containing block of absolute boxes inside. */
function stage(children: ReactElement, positioned = false) {
  return (
    <div
      style={{
        width: STAGE,
        height: STAGE,
        boxSizing: "border-box",
        padding: 20,
        backgroundColor: "#ffffff",
        position: positioned ? "relative" : undefined,
      }}
    >
      {children}
    </div>
  )
}

const red = ([r, g, b]: Rgba) => r > 235 && g < 25 && b < 25
const white = ([r, g, b]: Rgba) => r > 235 && g > 235 && b > 235

interface Box {
  left: number
  top: number
  width: number
  height: number
  /** Horizontal and vertical radius of every corner. */
  radius: [number, number]
}

/** Whether the point is inside the box with its corners rounded by quarter ellipses. */
function inside(box: Box, x: number, y: number) {
  const [rx, ry] = box.radius
  if (x < box.left || x > box.left + box.width || y < box.top || y > box.top + box.height) return false
  if (rx <= 0 || ry <= 0) return true
  const dx = Math.max(box.left + rx - x, x - (box.left + box.width - rx), 0)
  const dy = Math.max(box.top + ry - y, y - (box.top + box.height - ry), 0)
  return (dx / rx) ** 2 + (dy / ry) ** 2 <= 1
}

/** The pixels of the stage whose centre is more than a pixel from every edge of both boxes, and
 *  that are not what a box clipped by both should paint (red inside both, white elsewhere). */
function wrongPixels(sample: Sampler, boxes: Box[]) {
  const both = (x: number, y: number) => boxes.every((box) => inside(box, x, y))
  const wrong: [number, number][] = []
  for (let y = 0; y < STAGE; y++) {
    for (let x = 0; x < STAGE; x++) {
      const here = both(x + 0.5, y + 0.5)
      const steady = [
        [-1, 0],
        [1, 0],
        [0, -1],
        [0, 1],
        [-1, -1],
        [1, 1],
        [1, -1],
        [-1, 1],
      ].every(([dx, dy]) => both(x + 0.5 + dx!, y + 0.5 + dy!) === here)
      if (!steady) continue
      const pixel = sample(x, y)
      if (here ? !red(pixel) : !white(pixel)) wrong.push([x, y])
    }
  }
  return wrong
}

const roundedBox = (
  left: number,
  top: number,
  width: number,
  height: number,
  radius: number | [number, number],
): Box => ({ left, top, width, height, radius: typeof radius === "number" ? [radius, radius] : radius })

const RED = "#ff0000"

describeNative("nested overflow clips", () => {
  /** A rounded clip with padding, around a clip that ends square inside its corner. */
  const cutCorner = (inner?: ReactElement) =>
    stage(
      <div
        style={{
          width: 120,
          height: 120,
          boxSizing: "border-box",
          padding: 4,
          overflow: "hidden",
          borderRadius: 24,
        }}
      >
        <div style={{ position: "relative", width: "100%", height: "100%", overflow: "hidden" }}>
          {inner}
          <div style={{ width: 400, height: 400, backgroundColor: RED }} />
        </div>
      </div>,
      true,
    )

  it("cuts a child clip at the rounded corner of its parent, not at its own square one", () => {
    const sample = paint(cutCorner())
    // The child's corner is at (24, 24), in the corner the parent rounds away: the old
    // approximation painted a square corner there.
    expect(white(sample(25, 25))).toBe(true)
    expect(white(sample(135, 135))).toBe(true)
    expect(red(sample(40, 40))).toBe(true)
    expect(red(sample(80, 80))).toBe(true)
    expect(
      wrongPixels(sample, [roundedBox(20, 20, 120, 120, 24), roundedBox(24, 24, 112, 112, 0)]),
    ).toEqual([])
  })

  it("cuts the content of the child at the same corner, text included", () => {
    const sample = paint(
      cutCorner(
        <>
          <div style={{ width: 30, height: 30, backgroundColor: "#00ff00" }} />
          <text style={{ color: "#0000ff", fontSize: 16 }}>{"Hello, clip"}</text>
        </>,
      ),
    )
    // The green square starts at the child's corner and is cut by the parent's arc.
    expect(white(sample(25, 25))).toBe(true)
    const [r, g, b] = sample(44, 44)
    expect([r < 30, g > 220, b < 30]).toEqual([true, true, true])
    // The blue text is below the green square, inside both clips.
    let text = 0
    for (let y = 58; y < 80; y++) {
      for (let x = 28; x < 120; x++) {
        const [pr, pg, pb] = sample(x, y)
        if (pb > pr + 60 && pb > pg + 60) text++
      }
    }
    expect(text).toBeGreaterThan(40)
  })

  it("cuts an elliptical parent corner and a child with the transposed ellipse at once", () => {
    const sample = paint(
      stage(
        <div
          style={{
              width: 120,
            height: 120,
            overflow: "hidden",
            borderRadius: "40px / 10px",
          }}
        >
          <div
            style={{
              width: "100%",
              height: "100%",
              overflow: "hidden",
              borderRadius: "10px / 40px",
            }}
          >
            <div style={{ width: 400, height: 400, backgroundColor: RED }} />
          </div>
        </div>,
      ),
    )
    expect(
      wrongPixels(sample, [roundedBox(20, 20, 120, 120, [40, 10]), roundedBox(20, 20, 120, 120, [10, 40])]),
    ).toEqual([])
    // Inside both ellipses at 7.5 by 7.5 from the corner, which the larger radius per axis
    // (40 by 40) would have cut away.
    expect(red(sample(27, 27))).toBe(true)
  })

  it("paints a clip that lines up with its parent's exactly like the parent alone", () => {
    const single = paint(
      stage(
        <div style={{ width: 120, height: 120, overflow: "hidden", borderRadius: 24 }}>
          <div style={{ width: 400, height: 400, backgroundColor: RED }} />
        </div>,
      ),
    )
    const nested = paint(
      stage(
        <div style={{ width: 120, height: 120, overflow: "hidden", borderRadius: 24 }}>
          <div style={{ width: "100%", height: "100%", overflow: "hidden", borderRadius: 10 }}>
            <div style={{ width: 400, height: 400, backgroundColor: RED }} />
          </div>
        </div>,
      ),
    )
    let worst = 0
    for (let y = 0; y < STAGE; y++) {
      for (let x = 0; x < STAGE; x++) {
        const a = single(x, y)
        const b = nested(x, y)
        for (let i = 0; i < 3; i++) worst = Math.max(worst, Math.abs(a[i]! - b[i]!))
      }
    }
    expect(worst).toBeLessThanOrEqual(6)
    expect(wrongPixels(nested, [roundedBox(20, 20, 120, 120, 24)])).toEqual([])
  })

  it("paints a box that escapes both clips outside them", () => {
    const sample = paint(
      stage(
        <div
          style={{
              width: 120,
            height: 120,
            boxSizing: "border-box",
            padding: 4,
            overflow: "hidden",
            borderRadius: 24,
          }}
        >
          <div style={{ width: "100%", height: "100%", overflow: "hidden" }}>
            <div style={{ width: 400, height: 400, backgroundColor: RED }} />
            <div
              style={{
                position: "absolute",
                left: 4,
                top: 4,
                width: 12,
                height: 12,
                backgroundColor: "#00ff00",
              }}
            />
          </div>
        </div>,
        true,
      ),
    )
    // The stage is the box's containing block, so it sits at its corner, outside the parent.
    const [r, g, b] = sample(10, 10)
    expect([r < 30, g > 220, b < 30]).toEqual([true, true, true])
    expect(red(sample(80, 80))).toBe(true)
    expect(white(sample(150, 150))).toBe(true)
  })
})
