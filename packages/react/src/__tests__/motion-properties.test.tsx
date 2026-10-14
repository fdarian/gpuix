import fs from "fs"
import path from "path"
import { describe, expect, it } from "vitest"
import { motion } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("motion properties", () => {
  it("animates padding and margin, each on its own transition", () => {
    const { render, renderer } = createTestRoot()

    function App() {
      return (
        <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start" }}>
          <motion.div
            testId="box"
            initial={{ marginLeft: 0, paddingLeft: 0 }}
            animate={{ marginLeft: 40, paddingLeft: 30 }}
            transition={{
              duration: 1,
              ease: "linear",
              marginLeft: { duration: 0.2, ease: "linear" },
            }}
          >
            <div testId="inner" style={{ width: 20, height: 20 }} />
          </motion.div>
        </div>
      )
    }

    const offsets = () => {
      renderer.dispatchNativeEvents()
      const box = renderer.getElementBounds(renderer.findByTestId("box").id)
      const inner = renderer.getElementBounds(renderer.findByTestId("inner").id)
      return { margin: box.x, padding: inner.x - box.x }
    }

    renderer.clockPause()
    render(<App />)
    expect(offsets()).toEqual({ margin: 0, padding: 0 })

    renderer.clockFastForward(200)
    const early = offsets()
    // Margin has finished its 0.2 s tween; padding is a fifth of the way into 1 s.
    expect(early.margin).toBeCloseTo(40, 0)
    expect(early.padding).toBeGreaterThan(4)
    expect(early.padding).toBeLessThan(8)

    renderer.clockFastForward(1000)
    const done = offsets()
    expect(done.margin).toBeCloseTo(40, 0)
    expect(done.padding).toBeCloseTo(30, 0)
  })

  it("animates a colour through Oklab", () => {
    const { render, renderer } = createTestRoot()
    const file = path.join(SHOTS_DIR, "motion-background-color.png")
    fs.mkdirSync(SHOTS_DIR, { recursive: true })

    renderer.clockPause()
    render(
      <motion.div
        initial={{ backgroundColor: "#ff0000" }}
        animate={{ backgroundColor: "#0000ff" }}
        transition={{ duration: 1, ease: "linear" }}
        style={{ width: "100%", height: "100%" }}
      />
    )

    const sample = () => {
      renderer.dispatchNativeEvents()
      renderer.captureScreenshot(file)
      return readLogicalPixels(file, renderer).at(10, 10)
    }

    expect(sample().slice(0, 3)).toEqual([255, 0, 0])

    renderer.clockFastForward(500)
    const [red, green, blue] = sample()
    // oklab(0.539985 0.0962 -0.09284), the midpoint of red and blue in Oklab,
    // is rgb(140, 83, 162). The sRGB midpoint would be (128, 0, 128).
    expect(red).toBeGreaterThan(135)
    expect(red).toBeLessThan(146)
    expect(green).toBeGreaterThan(78)
    expect(green).toBeLessThan(88)
    expect(blue).toBeGreaterThan(157)
    expect(blue).toBeLessThan(168)

    renderer.clockFastForward(1000)
    expect(sample().slice(0, 3)).toEqual([0, 0, 255])
  })
})
