import fs from "fs"
import { afterEach, describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"
import { readPixels, SHOTS_DIR, type Pixels } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const WIDTH = 300
const HEIGHT = 200
/** The marker's logical position, a 1x1 red box on a blue fill. */
const MARKER = { x: 100, y: 50 }

let testRoot: TestRoot | undefined
let counter = 0

afterEach(() => {
  testRoot?.unmount()
  testRoot = undefined
})

function open(scaleFactor?: number): TestRoot {
  testRoot = createTestRoot({ width: WIDTH, height: HEIGHT, scaleFactor })
  testRoot.render(
    // Larger than the window, so a resize keeps the fill under all of it.
    <div style={{ position: "relative", width: 600, height: 400, backgroundColor: "#0000ff" }}>
      <div
        style={{
          position: "absolute",
          left: MARKER.x,
          top: MARKER.y,
          width: 1,
          height: 1,
          backgroundColor: "#ff0000",
        }}
      />
    </div>
  )
  return testRoot
}

function capture(root: TestRoot): Pixels {
  const file = `${SHOTS_DIR}/screenshot-scale-${counter++}.png`
  fs.rmSync(file, { force: true })
  root.renderer.captureScreenshot(file)
  return readPixels(file)
}

/** The device rectangle `[x0, x1) x [y0, y1)` the red pixels cover, and how many they are. */
function redPixels(pixels: Pixels) {
  let x0 = Infinity
  let y0 = Infinity
  let x1 = -1
  let y1 = -1
  let count = 0
  for (let y = 0; y < pixels.height; y++) {
    for (let x = 0; x < pixels.width; x++) {
      const [r, g, b] = pixels.at(x, y)
      if (r > 200 && g < 60 && b < 60) {
        count++
        x0 = Math.min(x0, x)
        y0 = Math.min(y0, y)
        x1 = Math.max(x1, x + 1)
        y1 = Math.max(y1, y + 1)
      }
    }
  }
  return { x0, y0, x1, y1, count }
}

function expectCapture(root: TestRoot, scale: number, size = { width: WIDTH, height: HEIGHT }) {
  expect(root.renderer.getWindowSize()).toEqual(size)
  const pixels = capture(root)
  expect([pixels.width, pixels.height]).toEqual([size.width * scale, size.height * scale])
  expect(redPixels(pixels)).toEqual({
    x0: MARKER.x * scale,
    y0: MARKER.y * scale,
    x1: (MARKER.x + 1) * scale,
    y1: (MARKER.y + 1) * scale,
    count: scale * scale,
  })
  // The fill covers the whole capture, not a quadrant of a larger drawable.
  const [r, g, b] = pixels.at(pixels.width - 1, pixels.height - 1)
  expect([r, g, b]).toEqual([0, 0, 255])
}

describeNative("the screenshot follows the window's scale factor", () => {
  it("is 1x unless asked, whatever the display", () => {
    expectCapture(open(), 1)
  })

  it("is the window size times a set factor", () => {
    const root = open()
    root.renderer.setWindowScaleFactor(2)
    expectCapture(root, 2)
    root.renderer.setWindowScaleFactor(1)
    expectCapture(root, 1)
  })

  it("keeps a set factor across a resize, in either order", () => {
    const resized = { width: 320, height: 220 }
    const root = open()
    root.renderer.setWindowScaleFactor(2)
    root.renderer.setWindowSize(resized.width, resized.height)
    expectCapture(root, 2, resized)

    root.renderer.setWindowSize(WIDTH, HEIGHT)
    expectCapture(root, 2)
    root.renderer.setWindowScaleFactor(1)
    root.renderer.setWindowSize(resized.width, resized.height)
    expectCapture(root, 1, resized)
  })

  it("takes the factor as a creation option", () => {
    const root = open(2)
    expectCapture(root, 2)
    root.renderer.setWindowSize(320, 220)
    expectCapture(root, 2, { width: 320, height: 220 })
  })

  it("rejects a factor that is not positive", () => {
    expect(() => open(0)).toThrow(/scale factor must be positive/)
  })
})
