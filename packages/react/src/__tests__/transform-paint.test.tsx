import fs from "fs"
import type { ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { expectScreenshotsEqual, readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rgba = [number, number, number, number]
const RED: Rgba = [255, 0, 0, 255]
const GREEN: Rgba = [0, 255, 0, 255]
const BLUE: Rgba = [0, 0, 255, 255]
const WHITE: Rgba = [255, 255, 255, 255]

type TestRoot = ReturnType<typeof createTestRoot>

const shot = (name: string) => `${SHOTS_DIR}/transform-paint-${process.pid}-${name}.png`

function capture(testRoot: TestRoot, name: string) {
  const file = shot(name)
  fs.rmSync(file, { force: true })
  testRoot.renderer.flush()
  testRoot.renderer.flush()
  testRoot.renderer.captureScreenshot(file)
  return file
}

/** The frame of `tree` on a white page, as pixels addressed in logical px. */
function paint(name: string, tree: ReactElement, run: (at: (x: number, y: number) => Rgba, testRoot: TestRoot) => void) {
  const testRoot = createTestRoot({ width: 400, height: 400 })
  try {
    testRoot.render(<div style={{ width: 400, height: 400, backgroundColor: "#ffffff" }}>{tree}</div>)
    run(readLogicalPixels(capture(testRoot, name), testRoot.renderer).at, testRoot)
  } finally {
    testRoot.unmount()
  }
}

const box = (style: Record<string, unknown>) => (
  <div style={{ position: "absolute", left: 100, top: 100, width: 100, height: 100, backgroundColor: "#ff0000", ...style }} />
)

describeNative("transform: painted pixels", () => {
  it("rotates a rounded box, corners and all", () => {
    paint("rotate", box({ borderRadius: 20, transform: "rotate(45deg)" }), (at) => {
      expect(at(150, 150)).toEqual(RED)
      // The corner of the box it had, now outside the diamond.
      expect(at(101, 101)).toEqual(WHITE)
      // Outside the box it had and inside the diamond, 60px above the centre.
      expect(at(150, 90)).toEqual(RED)
      // The tip of the diamond is cut by the radius of the corner.
      expect(at(150, 81)).toEqual(WHITE)
    })
  })

  it("leaves a box where it is for `transform: none`", () => {
    paint("none", box({ transform: "none" }), (at) => {
      expect(at(101, 101)).toEqual(RED)
      expect(at(199, 199)).toEqual(RED)
      expect(at(99, 99)).toEqual(WHITE)
    })
  })

  it("ignores a transform that is not valid", () => {
    paint("invalid", box({ transform: "rotate(45)", rotate: "banana", scale: "2px", translate: "10" }), (at) => {
      expect(at(101, 101)).toEqual(RED)
      expect(at(199, 199)).toEqual(RED)
      expect(at(99, 99)).toEqual(WHITE)
    })
  })

  it("turns around the transform-origin, the middle by default", () => {
    const small = { width: 50, height: 50 }
    paint("origin-corner", box({ ...small, transform: "rotate(90deg)", transformOrigin: "0 0" }), (at) => {
      // A quarter turn around the top left corner swings the box to the left of it.
      expect(at(75, 125)).toEqual(RED)
      expect(at(125, 125)).toEqual(WHITE)
    })
    paint("origin-middle", box({ ...small, transform: "rotate(90deg)" }), (at) => {
      expect(at(125, 125)).toEqual(RED)
      expect(at(101, 101)).toEqual(RED)
      expect(at(151, 101)).toEqual(WHITE)
    })
    paint("origin-keywords", box({ ...small, transform: "rotate(90deg)", transformOrigin: "right bottom" }), (at) => {
      // Around (150, 150): the box at (100..150, 100..150) goes to (150..200, 100..150).
      expect(at(175, 125)).toEqual(RED)
      expect(at(125, 125)).toEqual(WHITE)
    })
  })

  it("composes translate, rotate, scale and transform in the order of CSS Transforms 2", () => {
    const small = { width: 10, height: 10, transformOrigin: "0 0" }
    // The scale is applied outside the transform: the 10px translation becomes 20.
    paint("order-scale", box({ ...small, scale: 2, transform: "translateX(10px)" }), (at) => {
      expect(at(121, 101)).toEqual(RED)
      expect(at(139, 119)).toEqual(RED)
      expect(at(119, 101)).toEqual(WHITE)
      expect(at(141, 101)).toEqual(WHITE)
    })
    // And the translation outside the scale: 30px, unscaled.
    paint("order-translate", box({ ...small, scale: 2, translate: "30px" }), (at) => {
      expect(at(131, 101)).toEqual(RED)
      expect(at(149, 119)).toEqual(RED)
      expect(at(129, 101)).toEqual(WHITE)
      expect(at(151, 101)).toEqual(WHITE)
    })
    paint("order-rotate", box({ ...small, rotate: 90, scale: "2 1" }), (at) => {
      // Scaled to 20x10, then a quarter turn about the top left corner: 10 wide and 20 tall, to its left.
      expect(at(95, 119)).toEqual(RED)
      expect(at(105, 101)).toEqual(WHITE)
      expect(at(95, 121)).toEqual(WHITE)
    })
  })

  it("scales an image to cover twice the area", () => {
    const file = shot("red.svg")
    fs.writeFileSync(
      file,
      '<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect width="20" height="20" fill="#ff0000"/></svg>',
      "utf8"
    )
    const testRoot = createTestRoot({ width: 400, height: 400 })
    try {
      testRoot.render(
        <div style={{ width: 400, height: 400, backgroundColor: "#ffffff" }}>
          <img
            src={file}
            style={{ position: "absolute", left: 10, top: 10, width: 20, height: 20, transform: "scale(2)", transformOrigin: "0 0" }}
          />
        </div>
      )
      for (let frame = 0; frame < 4; frame++) testRoot.renderer.flush()
      const at = readLogicalPixels(capture(testRoot, "scaled-image"), testRoot.renderer).at
      expect(at(45, 45)).toEqual(RED)
      expect(at(48, 12)).toEqual(RED)
      expect(at(55, 55)).toEqual(WHITE)
      expect(at(5, 5)).toEqual(WHITE)
    } finally {
      testRoot.unmount()
      fs.rmSync(file, { force: true })
    }
  })

  it("clips what a rotated overflow:hidden box holds to the rotated box", () => {
    const tree = (
      <div
        style={{
          position: "absolute",
          left: 100,
          top: 100,
          width: 100,
          height: 100,
          overflow: "hidden",
          backgroundColor: "#ff0000",
          transform: "rotate(45deg)",
        }}
      >
        <div style={{ position: "absolute", left: -50, top: 40, width: 200, height: 20, backgroundColor: "#0000ff" }} />
      </div>
    )
    paint("rotated-clip", tree, (at) => {
      // The bar crosses the box through its middle, along the diagonal.
      expect(at(150, 150)).toEqual(BLUE)
      expect(at(182, 182)).toEqual(BLUE)
      // 55px from the middle along the bar: past the end of the box it lies in, though inside the
      // box before the turn.
      expect(at(189, 189)).toEqual(WHITE)
      expect(at(111, 111)).toEqual(WHITE)
      // Beside the bar, the box itself.
      expect(at(150, 120)).toEqual(RED)
    })
  })

  it("composes a rotated child with the rotation of the box that clips it", () => {
    const tree = (
      <div
        style={{
          position: "absolute",
          left: 100,
          top: 100,
          width: 100,
          height: 100,
          overflow: "hidden",
          transform: "rotate(45deg)",
        }}
      >
        <div
          style={{
            position: "absolute",
            left: 30,
            top: 30,
            width: 40,
            height: 40,
            backgroundColor: "#00ff00",
            transform: "rotate(-45deg)",
          }}
        />
      </div>
    )
    paint("rotated-child", tree, (at) => {
      // The two turns cancel: a square 40px wide around the middle of the box, with its sides straight.
      expect(at(131, 131)).toEqual(GREEN)
      expect(at(169, 169)).toEqual(GREEN)
      expect(at(131, 169)).toEqual(GREEN)
      expect(at(128, 150)).toEqual(WHITE)
      expect(at(150, 172)).toEqual(WHITE)
    })
  })

  it("draws a translated line of text exactly as the same text placed by an offset", () => {
    const text = "Hamburgefonstiv 0123"
    const testRoot = createTestRoot({ width: 400, height: 100 })
    try {
      const page = (inner: ReactElement) => (
        <div style={{ width: 400, height: 100, backgroundColor: "#ffffff", color: "#000000" }}>{inner}</div>
      )
      testRoot.render(page(<div style={{ transform: "translate(13px, 7px)" }}>{text}</div>))
      const translated = capture(testRoot, "text-translated")
      testRoot.render(page(<div style={{ paddingLeft: 13, paddingTop: 7 }}>{text}</div>))
      const placed = capture(testRoot, "text-placed")
      expectScreenshotsEqual(translated, placed)
      testRoot.render(page(<div style={{ translate: "13px 7px" }}>{text}</div>))
      expectScreenshotsEqual(capture(testRoot, "text-translate-property"), placed)
      testRoot.render(page(<div>{text}</div>))
      expect(fs.readFileSync(capture(testRoot, "text-in-place")).equals(fs.readFileSync(placed))).toBe(false)
    } finally {
      testRoot.unmount()
    }
  })

  it("gives an inline-block a transform and a plain inline element none", () => {
    const line = (inline: ReactElement) => (
      <div style={{ width: 300, height: 50, color: "#000000" }}>
        before {inline} after
      </div>
    )
    const testRoot = createTestRoot({ width: 300, height: 50 })
    try {
      const page = (inner: ReactElement) => <div style={{ width: 300, height: 50, backgroundColor: "#ffffff" }}>{inner}</div>
      testRoot.render(page(line(<span style={{ transform: "translateY(20px) rotate(10deg)" }}>span</span>)))
      const transformedSpan = capture(testRoot, "inline-span-transformed")
      testRoot.render(page(line(<span>span</span>)))
      expectScreenshotsEqual(transformedSpan, capture(testRoot, "inline-span-plain"))

      const block = (style: Record<string, unknown>) =>
        page(
          line(
            <span style={{ display: "inline-block", width: 20, height: 10, backgroundColor: "#ff0000", ...style }} />
          )
        )
      /** Whether a red pixel lies in the rows `from` to `to`, anywhere along the line. */
      const hasRed = (file: string, from: number, to: number) => {
        const at = readLogicalPixels(file, testRoot.renderer).at
        return Array.from({ length: 300 * (to - from) }, (_, index) => at(index % 300, from + Math.floor(index / 300))).some(
          (pixel) => pixel[0] === 255 && pixel[1] === 0 && pixel[2] === 0
        )
      }
      testRoot.render(block({}))
      const plain = capture(testRoot, "inline-block-plain")
      expect(hasRed(plain, 0, 22)).toBe(true)
      expect(hasRed(plain, 28, 50)).toBe(false)
      testRoot.render(block({ transform: "translateY(25px)" }))
      const moved = capture(testRoot, "inline-block-moved")
      expect(hasRed(moved, 0, 22)).toBe(false)
      expect(hasRed(moved, 28, 50)).toBe(true)
    } finally {
      testRoot.unmount()
    }
  })
})
