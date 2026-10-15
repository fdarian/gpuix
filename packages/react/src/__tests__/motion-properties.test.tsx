import fs from "fs"
import path from "path"
import type { ReactNode } from "react"
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
  describe("lengths layout resolves", () => {
    const boundsOf = (
      renderer: ReturnType<typeof createTestRoot>["renderer"],
      testId: string
    ) => {
      renderer.dispatchNativeEvents()
      return renderer.getElementBounds(renderer.findByTestId(testId).id)
    }
    const linear = { duration: 1, ease: "linear" } as const
    // Layout is read back a frame after it is painted, and the clock of an
    // animation that needs it starts on the first frame that has it, so one
    // more frame runs before time moves.
    const settled = (
      render: (node: ReactNode) => void,
      renderer: ReturnType<typeof createTestRoot>["renderer"],
      node: ReactNode
    ) => {
      render(node)
      renderer.flush()
    }

    function Accordion(props: { open: boolean; content: number }) {
      return (
        <div style={{ display: "flex", flexDirection: "column" }}>
          <motion.div
            testId="panel"
            initial={{ height: 0 }}
            animate={{ height: props.open ? "auto" : 0 }}
            transition={linear}
            style={{ overflow: "hidden" }}
          >
            <div style={{ width: 40, height: props.content }} />
          </motion.div>
        </div>
      )
    }

    it("opens an accordion to the content height and closes it again", () => {
      const { render: mount, renderer } = createTestRoot({ width: 400, height: 300 })
      const render = (node: ReactNode) => settled(mount, renderer, node)

      renderer.clockPause()
      render(<Accordion open content={80} />)
      expect(boundsOf(renderer, "panel").height).toBe(0)

      renderer.clockFastForward(500)
      expect(boundsOf(renderer, "panel").height).toBeCloseTo(40, 0)

      renderer.clockFastForward(1000)
      expect(boundsOf(renderer, "panel").height).toBe(80)

      render(<Accordion open={false} content={80} />)
      renderer.clockFastForward(500)
      expect(boundsOf(renderer, "panel").height).toBeCloseTo(40, 0)

      renderer.clockFastForward(1000)
      expect(boundsOf(renderer, "panel").height).toBe(0)
    })

    it("follows the content while it grows during the animation", () => {
      const { render: mount, renderer } = createTestRoot({ width: 400, height: 300 })
      const render = (node: ReactNode) => settled(mount, renderer, node)

      renderer.clockPause()
      render(<Accordion open content={80} />)
      renderer.clockFastForward(500)
      expect(boundsOf(renderer, "panel").height).toBeCloseTo(40, 0)

      render(<Accordion open content={160} />)
      renderer.clockFastForward(250)
      const grown = boundsOf(renderer, "panel").height
      expect(grown).toBeGreaterThan(40)
      expect(grown).toBeLessThan(160)

      renderer.clockFastForward(1000)
      expect(boundsOf(renderer, "panel").height).toBe(160)
    })

    it("moves between px and a percentage of the parent and settles on the target unit", () => {
      const { render: mount, renderer } = createTestRoot({ width: 400, height: 300 })
      const render = (node: ReactNode) => settled(mount, renderer, node)
      const App = (props: { width: number | `${number}%` }) => (
        <div style={{ display: "flex", flexDirection: "column", width: 400 }}>
          <motion.div
            testId="bar"
            initial={{ width: 0 }}
            animate={{ width: props.width }}
            transition={linear}
            style={{ height: 10 }}
          />
        </div>
      )

      renderer.clockPause()
      render(<App width="50%" />)
      renderer.clockFastForward(500)
      expect(boundsOf(renderer, "bar").width).toBeCloseTo(100, 0)
      renderer.clockFastForward(1000)
      expect(boundsOf(renderer, "bar").width).toBe(200)

      render(<App width={100} />)
      renderer.clockFastForward(500)
      expect(boundsOf(renderer, "bar").width).toBeCloseTo(150, 0)
      renderer.clockFastForward(1000)
      expect(boundsOf(renderer, "bar").width).toBe(100)
    })

    it("resolves a percentage on a root against the window", () => {
      const { render: mount, renderer } = createTestRoot({ width: 400, height: 300 })
      const render = (node: ReactNode) => settled(mount, renderer, node)

      renderer.clockPause()
      render(
        <motion.div
          testId="root"
          initial={{ width: 0 }}
          animate={{ width: "50%" }}
          transition={linear}
          style={{ height: 10 }}
        />
      )
      renderer.clockFastForward(500)
      expect(boundsOf(renderer, "root").width).toBeCloseTo(100, 0)
      renderer.clockFastForward(1000)
      expect(boundsOf(renderer, "root").width).toBe(200)
    })

    it("starts from the laid-out size when the style leaves it unset", () => {
      const { render: mount, renderer } = createTestRoot({ width: 400, height: 300 })
      const render = (node: ReactNode) => settled(mount, renderer, node)

      renderer.clockPause()
      render(
        <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start" }}>
          <motion.div
            testId="box"
            initial={{ opacity: 1 }}
            animate={{ opacity: 1, width: 200 }}
            transition={linear}
            style={{ height: 10 }}
          >
            <div style={{ width: 40, height: 10 }} />
          </motion.div>
        </div>
      )
      expect(boundsOf(renderer, "box").width).toBeCloseTo(40, 0)

      renderer.clockFastForward(500)
      expect(boundsOf(renderer, "box").width).toBeCloseTo(120, 0)

      renderer.clockFastForward(1000)
      expect(boundsOf(renderer, "box").width).toBe(200)
    })
  })
})
