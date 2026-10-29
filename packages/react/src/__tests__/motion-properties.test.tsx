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

  it("animates a hex backgroundColor through sRGB, as the browser Motion hands it to does", () => {
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
    // Motion hands backgroundColor to the browser, which blends two hex colours in sRGB: (127.5, 0, 127.5).
    expect(red).toBeGreaterThan(125)
    expect(red).toBeLessThan(130)
    expect(green).toBeLessThan(3)
    expect(blue).toBeGreaterThan(125)
    expect(blue).toBeLessThan(130)

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

    it("resolves a margin percentage against the parent width on every side", () => {
      const { render: mount, renderer } = createTestRoot({ width: 400, height: 300 })
      const render = (node: ReactNode) => settled(mount, renderer, node)

      renderer.clockPause()
      render(
        <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", width: 400 }}>
          <motion.div
            testId="box"
            initial={{ marginLeft: 0, marginTop: 0 }}
            animate={{ marginLeft: "50%", marginTop: "25%" }}
            transition={linear}
            style={{ width: 20, height: 20 }}
          />
        </div>
      )
      renderer.clockFastForward(500)
      const halfway = boundsOf(renderer, "box")
      expect(halfway.x).toBeCloseTo(100, 0)
      // 25% of the 400 px width, not of the height.
      expect(halfway.y).toBeCloseTo(50, 0)

      renderer.clockFastForward(1000)
      const done = boundsOf(renderer, "box")
      expect(done.x).toBe(200)
      expect(done.y).toBe(100)
    })

    it("resolves an inset percentage against the parent height for top and width for left", () => {
      const { render: mount, renderer } = createTestRoot({ width: 400, height: 300 })
      const render = (node: ReactNode) => settled(mount, renderer, node)

      renderer.clockPause()
      render(
        <div style={{ position: "relative", width: 400, height: 200 }}>
          <motion.div
            testId="box"
            initial={{ top: 0, left: 0 }}
            animate={{ top: "50%", left: "50%" }}
            transition={linear}
            style={{ position: "absolute", width: 20, height: 20 }}
          />
        </div>
      )
      renderer.clockFastForward(500)
      const halfway = boundsOf(renderer, "box")
      expect(halfway.x).toBeCloseTo(100, 0)
      expect(halfway.y).toBeCloseTo(50, 0)

      renderer.clockFastForward(1000)
      const done = boundsOf(renderer, "box")
      expect(done.x).toBe(200)
      expect(done.y).toBe(100)
    })

    describe("a percentage height", () => {
      const Panel = (props: { parent: { height?: number; flexGrow?: number }; content?: number }) => (
        <div style={{ display: "flex", flexDirection: "column", width: 400, ...props.parent }}>
          <motion.div
            testId="panel"
            initial={{ height: 0 }}
            animate={{ height: "50%" }}
            transition={linear}
          >
            <div style={{ width: 40, height: props.content ?? 80 }} />
          </motion.div>
        </div>
      )

      it("is the content height against a parent whose height is auto, as in CSS", () => {
        const { render: mount, renderer } = createTestRoot({ width: 400, height: 300 })
        const render = (node: ReactNode) => settled(mount, renderer, node)

        renderer.clockPause()
        render(<Panel parent={{}} />)
        expect(boundsOf(renderer, "panel").height).toBe(0)

        // The blend from 0 has a percentage in it, which an auto parent cannot
        // resolve, so the height is auto from the first frame on, as in Chrome.
        renderer.clockFastForward(500)
        expect(boundsOf(renderer, "panel").height).toBe(80)

        renderer.clockFastForward(1000)
        expect(boundsOf(renderer, "panel").height).toBe(80)
      })

      it("is a share of the parent when the parent has a height", () => {
        const { render: mount, renderer } = createTestRoot({ width: 400, height: 300 })
        const render = (node: ReactNode) => settled(mount, renderer, node)

        renderer.clockPause()
        render(<Panel parent={{ height: 200 }} />)
        renderer.clockFastForward(500)
        expect(boundsOf(renderer, "panel").height).toBeCloseTo(50, 0)

        renderer.clockFastForward(1000)
        expect(boundsOf(renderer, "panel").height).toBe(100)
      })

      it("is a share of a flex item that its container sizes", () => {
        const { render: mount, renderer } = createTestRoot({ width: 400, height: 300 })
        const render = (node: ReactNode) => settled(mount, renderer, node)

        renderer.clockPause()
        render(
          <div style={{ display: "flex", flexDirection: "column", width: 400, height: 200 }}>
            <Panel parent={{ flexGrow: 1 }} />
          </div>
        )
        renderer.clockFastForward(500)
        expect(boundsOf(renderer, "panel").height).toBeCloseTo(50, 0)

        renderer.clockFastForward(1000)
        expect(boundsOf(renderer, "panel").height).toBe(100)
      })
    })

    describe("lengths that mix units", () => {
      const Bar = (props: {
        parent?: number
        from: number | string
        to: number | string
        fontSize?: number
      }) => (
        <div style={{ display: "flex", flexDirection: "column", width: props.parent ?? 400 }}>
          <motion.div
            testId="bar"
            initial={{ width: props.from }}
            animate={{ width: props.to }}
            transition={linear}
            style={{ height: 10, fontSize: props.fontSize }}
          />
        </div>
      )

      it("blends calc() to calc() part by part, and a parent that resizes mid-animation stays exact", () => {
        const { render: mount, renderer } = createTestRoot({ width: 800, height: 300 })
        const render = (node: ReactNode) => settled(mount, renderer, node)
        const bar = <Bar from="calc(100% - 20px)" to="calc(50% + 10px)" />

        renderer.clockPause()
        render(bar)
        expect(boundsOf(renderer, "bar").width).toBe(380)

        // calc(75% - 5px) of 400px.
        renderer.clockFastForward(500)
        expect(boundsOf(renderer, "bar").width).toBeCloseTo(295, 0)

        // The same blend of the new basis: calc(75% - 5px) of 600px.
        render(<Bar parent={600} from="calc(100% - 20px)" to="calc(50% + 10px)" />)
        expect(boundsOf(renderer, "bar").width).toBeCloseTo(445, 0)

        renderer.clockFastForward(1000)
        expect(boundsOf(renderer, "bar").width).toBe(310)
      })

      it("blends px to a percentage as calc(20px * (1 - t) + 50% * t)", () => {
        const { render: mount, renderer } = createTestRoot({ width: 800, height: 300 })
        const render = (node: ReactNode) => settled(mount, renderer, node)

        renderer.clockPause()
        render(<Bar from={20} to="50%" />)
        expect(boundsOf(renderer, "bar").width).toBe(20)
        renderer.clockFastForward(500)
        expect(boundsOf(renderer, "bar").width).toBeCloseTo(110, 0)
        renderer.clockFastForward(1000)
        expect(boundsOf(renderer, "bar").width).toBe(200)
      })

      it("resolves em against the element's font size", () => {
        const { render: mount, renderer } = createTestRoot({ width: 800, height: 300 })
        const render = (node: ReactNode) => settled(mount, renderer, node)

        renderer.clockPause()
        render(<Bar from="1em" to="calc(100% - 2em)" fontSize={20} />)
        expect(boundsOf(renderer, "bar").width).toBe(20)
        renderer.clockFastForward(500)
        // Halfway between 20px and calc(100% - 40px): calc(10px + 50% - 20px).
        expect(boundsOf(renderer, "bar").width).toBeCloseTo(190, 0)
        renderer.clockFastForward(1000)
        expect(boundsOf(renderer, "bar").width).toBe(360)
      })

      it("animates padding to em and a gap to calc()", () => {
        const { render: mount, renderer } = createTestRoot({ width: 800, height: 300 })
        const render = (node: ReactNode) => settled(mount, renderer, node)

        renderer.clockPause()
        render(
          <motion.div
            testId="box"
            style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", fontSize: 20 }}
            initial={{ paddingLeft: 0, rowGap: 0 }}
            animate={{ paddingLeft: "2em", rowGap: "calc(1em + 4px)" }}
            transition={linear}
          >
            <div testId="first" style={{ width: 20, height: 20 }} />
            <div testId="second" style={{ width: 20, height: 20 }} />
          </motion.div>
        )
        renderer.clockFastForward(500)
        const middle = boundsOf(renderer, "second")
        expect(boundsOf(renderer, "first").x).toBeCloseTo(20, 0)
        expect(middle.y - boundsOf(renderer, "first").y).toBeCloseTo(32, 0)

        renderer.clockFastForward(1000)
        expect(boundsOf(renderer, "first").x).toBe(40)
        expect(boundsOf(renderer, "second").y - boundsOf(renderer, "first").y).toBe(44)
      })

      it("transitions from and to a calc() as a CSS transition", () => {
        const { render: mount, renderer } = createTestRoot({ width: 800, height: 300 })
        const render = (node: ReactNode) => settled(mount, renderer, node)
        const box = (width: string) => (
          <div style={{ width: 400 }}>
            <div testId="box" style={{ width, height: 10, transition: "width 1s linear" }} />
          </div>
        )

        renderer.clockPause()
        render(box("calc(100% - 20px)"))
        expect(boundsOf(renderer, "box").width).toBe(380)

        render(box("calc(50% + 10px)"))
        renderer.clockFastForward(500)
        expect(boundsOf(renderer, "box").width).toBeCloseTo(295, 0)
        renderer.clockFastForward(1000)
        expect(boundsOf(renderer, "box").width).toBe(210)
      })
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
