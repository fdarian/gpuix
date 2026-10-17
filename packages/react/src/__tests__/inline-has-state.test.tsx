/// `hasHover` and an `inline-block`: a box on a line of text is a descendant of the element whose
/// paragraph it is in, like any other child, although it is laid out by the text. The pointer finds
/// it through the text's hitbox, and the relational state reads the same hover chain.

import fs from "fs"
import type { ReactElement } from "react"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rgba = [number, number, number, number]
const RED: Rgba = [255, 0, 0, 255]
const BLUE: Rgba = [0, 0, 255, 255]
const GREEN: Rgba = [0, 255, 0, 255]

type TestRoot = ReturnType<typeof createTestRoot>

function sample(testRoot: TestRoot, name: string, x: number, y: number): Rgba {
  const file = `${SHOTS_DIR}/inline-has-state-${name}.png`
  fs.rmSync(file, { force: true })
  testRoot.renderer.captureScreenshot(file)
  return readLogicalPixels(file, testRoot.renderer).at(x, y)
}

/** The centre of the box a ref named, once it is laid out. */
function centre(testRoot: TestRoot, element: PublicInstance | null): [number, number] {
  testRoot.renderer.dispatchNativeEvents()
  const box = element?.getBoundingClientRect()
  expect(box, "the box is laid out").toBeDefined()
  return [(box?.x ?? 0) + (box?.width ?? 0) / 2, (box?.y ?? 0) + (box?.height ?? 0) / 2]
}

function withRoot(tree: ReactElement, run: (testRoot: TestRoot) => void) {
  const testRoot = createTestRoot()
  try {
    testRoot.render(tree)
    run(testRoot)
  } finally {
    testRoot.unmount()
  }
}

const inlineBlock = { display: "inline-block" } as const

describeNative("hasHover and an inline-block", () => {
  it("restyles the element whose paragraph the box is in while the box is hovered", () => {
    let box: PublicInstance | null = null
    const tree = (
      <div
        style={{ width: 300, height: 80, backgroundColor: "#ff0000", fontSize: 20, hasHover: { backgroundColor: "#0000ff" } }}
      >
        AA
        <span
          ref={(instance) => {
            box = instance ?? box
          }}
          style={{ ...inlineBlock, width: 40, height: 40 }}
        />
      </div>
    )
    withRoot(tree, (testRoot) => {
      const [x, y] = centre(testRoot, box)
      testRoot.renderer.nativeSimulateMouseMove(280, 70)
      expect(sample(testRoot, "box-off", 250, 70)).toEqual(RED)
      testRoot.renderer.nativeSimulateMouseMove(x, y)
      expect(sample(testRoot, "box-on", 250, 70)).toEqual(BLUE)
      testRoot.renderer.nativeSimulateMouseMove(280, 70)
      expect(sample(testRoot, "box-off-again", 250, 70)).toEqual(RED)
    })
  })

  it("restyles it while a box inside the box is hovered", () => {
    let inner: PublicInstance | null = null
    const tree = (
      <div
        style={{ width: 300, height: 80, backgroundColor: "#ff0000", fontSize: 20, hasHover: { backgroundColor: "#0000ff" } }}
      >
        AA
        <span style={{ ...inlineBlock, padding: 5 }}>
          <div
            ref={(instance) => {
              inner = instance ?? inner
            }}
            style={{ width: 20, height: 20 }}
          />
        </span>
      </div>
    )
    withRoot(tree, (testRoot) => {
      const [x, y] = centre(testRoot, inner)
      testRoot.renderer.nativeSimulateMouseMove(x, y)
      expect(sample(testRoot, "inner-on", 250, 70)).toEqual(BLUE)
      testRoot.renderer.nativeSimulateMouseMove(280, 70)
      expect(sample(testRoot, "inner-off", 250, 70)).toEqual(RED)
    })
  })

  it("does not match while only the element's own text is hovered, and does for a span beside the box", () => {
    let span: PublicInstance | null = null
    const tree = (
      <div
        style={{ width: 300, height: 80, backgroundColor: "#ff0000", fontSize: 20, hasHover: { backgroundColor: "#0000ff" } }}
      >
        AAAA{" "}
        <span
          ref={(instance) => {
            span = instance ?? span
          }}
        >
          BBBB
        </span>
        <span style={{ ...inlineBlock, width: 40, height: 40 }} />
      </div>
    )
    withRoot(tree, (testRoot) => {
      // The raw text is the element itself.
      testRoot.renderer.nativeSimulateMouseMove(3, 10)
      expect(sample(testRoot, "own-text", 250, 70)).toEqual(RED)
      const [x, y] = centre(testRoot, span)
      testRoot.renderer.nativeSimulateMouseMove(x, y)
      expect(sample(testRoot, "span", 250, 70)).toEqual(BLUE)
      testRoot.renderer.nativeSimulateMouseMove(280, 70)
      expect(sample(testRoot, "span-off", 250, 70)).toEqual(RED)
    })
  })

  it("a box with hasHover of its own restyles when something inside it is hovered", () => {
    let inner: PublicInstance | null = null
    const tree = (
      <div style={{ width: 300, height: 80, fontSize: 20 }}>
        AA
        <span
          style={{
            ...inlineBlock,
            padding: 10,
            backgroundColor: "#ff0000",
            hasHover: { backgroundColor: "#00ff00" },
          }}
        >
          <div
            ref={(instance) => {
              inner = instance ?? inner
            }}
            style={{ width: 20, height: 20 }}
          />
        </span>
      </div>
    )
    withRoot(tree, (testRoot) => {
      const [x, y] = centre(testRoot, inner)
      // The padding of the box is outside its child: (x - 15, y) is inside the box only.
      expect(sample(testRoot, "own-off", x - 15, y)).toEqual(RED)
      testRoot.renderer.nativeSimulateMouseMove(x, y)
      expect(sample(testRoot, "own-on", x - 15, y)).toEqual(GREEN)
      testRoot.renderer.nativeSimulateMouseMove(280, 70)
      expect(sample(testRoot, "own-off-again", x - 15, y)).toEqual(RED)
    })
  })
})
