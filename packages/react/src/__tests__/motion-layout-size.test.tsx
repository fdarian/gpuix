/// `layout` animates the size of a box as well as its position, through a scale about its centre
/// with the corrections Motion's projection makes (`layout`, `"position"`, `"size"`, `layoutId`).
import { describe, expect, it } from "vitest"
import { motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const linear = { duration: 1, ease: "linear" } as const

function frame(renderer: TestRenderer, ms: number): void {
  renderer.clockFastForward(ms)
  renderer.flush()
  renderer.dispatchNativeEvents()
}

function boundsOf(renderer: TestRenderer, testId: string) {
  return renderer.getElementBounds(renderer.findByTestId(testId).id)
}

function Box(props: { height: number; width?: number; layout?: boolean | "position" | "size"; above?: number }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", width: 300 }}>
      {props.above ? <div style={{ height: props.above, flexShrink: 0 }} /> : null}
      <motion.div
        testId="box"
        layout={props.layout ?? true}
        transition={linear}
        style={{ width: props.width ?? 100, height: props.height, flexShrink: 0, backgroundColor: "#ff0000" }}
      />
    </div>
  )
}

describeNative("layout size", () => {
  it("glides a height change from the shown height, about the centre", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(<Box height={40} />)
    expect(boundsOf(renderer, "box")).toMatchObject({ y: 0, height: 40 })

    render(<Box height={80} />)
    expect(boundsOf(renderer, "box").y).toBeCloseTo(0, 1)
    expect(boundsOf(renderer, "box").height).toBeCloseTo(40, 1)
    frame(renderer, 500)
    expect(boundsOf(renderer, "box").y).toBeCloseTo(0, 1)
    expect(boundsOf(renderer, "box").height).toBeCloseTo(60, 1)
    frame(renderer, 600)
    expect(boundsOf(renderer, "box")).toMatchObject({ y: 0, height: 80, width: 100 })
  })

  it("glides a width and a move together", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(<Box height={40} width={100} />)
    render(<Box height={40} width={200} above={30} />)
    // Shown where it was: 100 wide at y 0.
    expect(boundsOf(renderer, "box")).toMatchObject({ x: 0, y: 0, width: 100, height: 40 })
    frame(renderer, 500)
    expect(boundsOf(renderer, "box").width).toBeCloseTo(150, 1)
    expect(boundsOf(renderer, "box").y).toBeCloseTo(15, 1)
    expect(boundsOf(renderer, "box").x).toBeCloseTo(0, 1)
    frame(renderer, 600)
    expect(boundsOf(renderer, "box")).toMatchObject({ x: 0, y: 30, width: 200, height: 40 })
  })

  it('"position" snaps the size and glides the corner', () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(<Box height={40} layout="position" />)
    render(<Box height={80} layout="position" above={30} />)
    expect(boundsOf(renderer, "box")).toMatchObject({ y: 0, height: 80 })
    frame(renderer, 500)
    expect(boundsOf(renderer, "box").y).toBeCloseTo(15, 1)
    expect(boundsOf(renderer, "box").height).toBeCloseTo(80, 1)
  })

  it('"size" glides the size and snaps the corner', () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(<Box height={40} layout="size" />)
    render(<Box height={80} layout="size" above={30} />)
    // The old size at the new corner.
    expect(boundsOf(renderer, "box").y).toBeCloseTo(30, 1)
    expect(boundsOf(renderer, "box").height).toBeCloseTo(40, 1)
    frame(renderer, 500)
    expect(boundsOf(renderer, "box").y).toBeCloseTo(30, 1)
    expect(boundsOf(renderer, "box").height).toBeCloseTo(60, 1)
    frame(renderer, 600)
    expect(boundsOf(renderer, "box")).toMatchObject({ y: 30, height: 80 })
  })

  it('"size" does not glide a move', () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(<Box height={40} layout="size" />)
    render(<Box height={40} layout="size" above={30} />)
    expect(boundsOf(renderer, "box").y).toBe(30)
  })

  it("lays nothing out while the size glides", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(<Box height={40} />)
    render(<Box height={80} />)
    frame(renderer, 16)
    const before = renderer.getLayoutCounters()
    for (let step = 0; step < 10; step += 1) frame(renderer, 50)
    const after = renderer.getLayoutCounters()
    expect(after.nodesLaidOut).toBe(before.nodesLaidOut)
    expect(after.nodesSized).toBe(before.nodesSized)
  })

  it("stretches what is inside a box that is not a layout box, and leaves a layout box in it at its own place", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const app = (height: number) => (
      <div style={{ display: "flex", flexDirection: "column", width: 300 }}>
        <motion.div layout transition={linear} testId="parent" style={{ width: 100, height, flexShrink: 0 }}>
          <div testId="plain" style={{ width: 100, height: 20, flexShrink: 0 }} />
          <motion.div layout transition={linear} testId="projected" style={{ width: 100, height: 20, flexShrink: 0 }} />
        </motion.div>
      </div>
    )
    render(app(40))
    expect(boundsOf(renderer, "plain")).toMatchObject({ y: 0, height: 20 })
    render(app(80))
    // The parent is shown at half its height. What is inside it is half as tall, as in Motion.
    expect(boundsOf(renderer, "parent").height).toBeCloseTo(40, 1)
    expect(boundsOf(renderer, "plain").height).toBeCloseTo(10, 1)
    // The box that is itself laid out is not stretched.
    expect(boundsOf(renderer, "projected").height).toBeCloseTo(20, 1)
    frame(renderer, 1100)
    expect(boundsOf(renderer, "plain").height).toBeCloseTo(20, 1)
    expect(boundsOf(renderer, "projected").height).toBeCloseTo(20, 1)
  })

  it("shares the size of the box a layoutId leaves", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const app = (first: boolean) => (
      <div style={{ display: "flex", flexDirection: "row", width: 400 }}>
        <div style={{ width: 150, height: 100 }}>
          {first ? <motion.div testId="shared" layoutId="card" transition={linear} style={{ width: 50, height: 50, backgroundColor: "#f00" }} /> : null}
        </div>
        <div style={{ width: 250, height: 100 }}>
          {first ? null : <motion.div testId="shared" layoutId="card" transition={linear} style={{ width: 100, height: 100, backgroundColor: "#f00" }} />}
        </div>
      </div>
    )
    render(app(true))
    render(app(false))
    expect(boundsOf(renderer, "shared")).toMatchObject({ x: 0, y: 0, width: 50, height: 50 })
    frame(renderer, 500)
    expect(boundsOf(renderer, "shared").width).toBeCloseTo(75, 1)
    expect(boundsOf(renderer, "shared").height).toBeCloseTo(75, 1)
    frame(renderer, 600)
    expect(boundsOf(renderer, "shared")).toMatchObject({ x: 150, y: 0, width: 100, height: 100 })
  })
})

describeNative("layout size corrections", () => {
  function shot(renderer: TestRenderer, name: string) {
    const file = `${SHOTS_DIR}/motion-layout-size-${process.pid}-${name}.png`
    renderer.captureScreenshot(file)
    return readLogicalPixels(file, renderer)
  }

  it("keeps a border radius the size it asked for while the box is scaled", () => {
    const { render, renderer } = createTestRoot({ width: 300, height: 300 })
    renderer.clockPause()
    const app = (side: number) => (
      <div style={{ width: 300, height: 300, backgroundColor: "#ffffff" }}>
        <motion.div
          testId="card"
          layout
          transition={linear}
          style={{ width: side, height: side, borderRadius: 20, backgroundColor: "#ff0000" }}
        />
      </div>
    )
    render(app(50))
    render(app(100))
    // Shown 50 by 50 at the corner. A radius of 20 px leaves the point 4 px in from the corner
    // outside the box; the radius scaled with the box (10 px) would leave it in.
    renderer.flush()
    const pixels = shot(renderer, "radius")
    expect(pixels.at(4, 4)).toEqual([255, 255, 255, 255])
    expect(pixels.at(25, 25)).toEqual([255, 0, 0, 255])
    frame(renderer, 1100)
    const settled = shot(renderer, "radius-settled")
    expect(settled.at(4, 4)).toEqual([255, 255, 255, 255])
    expect(settled.at(60, 60)).toEqual([255, 0, 0, 255])
  })

  it("keeps a box shadow the size it asked for while the box is scaled", () => {
    const { render, renderer } = createTestRoot({ width: 300, height: 300 })
    renderer.clockPause()
    const app = (side: number) => (
      <div style={{ width: 300, height: 300, backgroundColor: "#ffffff" }}>
        <motion.div
          testId="card"
          layout
          transition={linear}
          style={{ position: "relative", left: 40, top: 40, width: side, height: side, backgroundColor: "#0000ff", boxShadow: "0px 0px 0px 10px #00ff00" }}
        />
      </div>
    )
    render(app(50))
    render(app(100))
    renderer.flush()
    const pixels = shot(renderer, "shadow")
    // The box is shown 50 by 50 at (40, 40); its ring is 10 px wide, not the 5 px a stretch would leave.
    expect(pixels.at(40 - 8, 60)).toEqual([0, 255, 0, 255])
    expect(pixels.at(40 - 12, 60)).toEqual([255, 255, 255, 255])
    expect(pixels.at(60, 60)).toEqual([0, 0, 255, 255])
  })
})
