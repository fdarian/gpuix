// @ts-nocheck
/**
 * `transform`, `translate`, `rotate`, `scale` and `transformOrigin` in state styles, and a transform
 * that changes while the page around it is replayed from its record: the frame must be the one a
 * whole build draws, and nothing may be rebuilt that did not change. Run with GPUIX_RETAIN_VERIFY=1 too.
 */
import fs from "fs"
import React from "react"
import { expect, it } from "vitest"
import { Card, describeNative, Page, RED, setup, verifying, WINDOW } from "./retained-harness.js"
import { createTestRoot } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const BLUE = [0, 0, 255, 255]
const WHITE = [255, 255, 255, 255]

function sample(root, name: string, x: number, y: number) {
  const file = `${SHOTS_DIR}/transform-states-${process.pid}-${name}.png`
  fs.rmSync(file, { force: true })
  root.renderer.flush()
  root.renderer.captureScreenshot(file)
  return readLogicalPixels(file, root.renderer).at(x, y)
}

const BOX = { width: 100, height: 100, backgroundColor: "#ff0000", transformOrigin: "0 0" }

describeNative("transforms in state styles", () => {
  const page = (style) => (
    <div style={{ width: 400, height: 400, backgroundColor: "#ffffff" }}>
      <div style={style} />
    </div>
  )

  it("applies the transform a hover gives, and takes it away again", () => {
    const root = createTestRoot({ width: 400, height: 400 })
    try {
      root.render(page({ ...BOX, transform: "scale(0.5)", hover: { transform: "none" } }))
      // The box is drawn at half the size: (0, 0) to (50, 50).
      root.renderer.nativeSimulateMouseMove(300, 300)
      expect(sample(root, "off", 75, 75)).toEqual(WHITE)
      root.renderer.nativeSimulateMouseMove(25, 25)
      expect(sample(root, "on", 75, 75)).toEqual(RED)
      root.renderer.nativeSimulateMouseMove(300, 300)
      expect(sample(root, "off-again", 75, 75)).toEqual(WHITE)
    } finally {
      root.unmount()
    }
  })

  it("gives hover an individual property, over the base's transform", () => {
    const root = createTestRoot({ width: 400, height: 400 })
    try {
      root.render(page({ ...BOX, transform: "scale(0.5)", hover: { scale: 2 } }))
      root.renderer.nativeSimulateMouseMove(25, 25)
      // scale: 2 is applied outside the transform's 0.5: the box is its own size.
      expect(sample(root, "individual", 75, 75)).toEqual(RED)
      expect(sample(root, "individual", 125, 125)).toEqual(WHITE)
    } finally {
      root.unmount()
    }
  })

  it("keeps the base transform under a hover value that is not valid", () => {
    const root = createTestRoot({ width: 400, height: 400 })
    try {
      root.render(
        page({
          ...BOX,
          transform: "scale(2)",
          rotate: "10deg",
          hover: {
            backgroundColor: "#0000ff",
            transform: "scale(",
            rotate: "ten",
            scale: "1px",
            translate: "2",
            transformOrigin: "left left",
          },
        })
      )
      // Scaled by 2 and turned by 10 degrees about the corner: (150, 100) is in the box, (190, 134) is beside it.
      expect(sample(root, "before", 150, 100)).toEqual(RED)
      expect(sample(root, "before", 190, 134)).toEqual(WHITE)
      root.renderer.nativeSimulateMouseMove(50, 50)
      expect(sample(root, "after", 150, 100)).toEqual(BLUE)
      expect(sample(root, "after", 190, 134)).toEqual(WHITE)
    } finally {
      root.unmount()
    }
  })
})

describeNative("a transform that changes under a retained page", () => {
  const CARD = 1

  it("draws the frame a whole build draws, and rebuilds only the card", () => {
    const withCard = (style) => (
      <Page card={(index) => (index === CARD ? <Card key={index} index={index} style={style} /> : null)} />
    )
    const view = setup(withCard({}))
    view.settle()
    const steps = [
      { transform: "translate(20px, 10px)" },
      { transform: "translate(20px, 10px) rotate(15deg)" },
      { transform: "rotate(15deg)", transformOrigin: "0 0" },
      { rotate: "30deg", scale: 1.2, translate: "5px 5px" },
      { transform: "none" },
      {},
    ]
    for (const [index, style] of steps.entries()) {
      view.render(withCard(style))
      view.settle()
      view.expectWhatABuildShows(`step-${index}`, [`card-${CARD}`, `button-${CARD}`])
    }
    view.finish(!verifying)
    view.unmount()
  })

  it("draws a card that a transform covers over the card beside it, and hits the one on top", () => {
    const view = setup(
      <Page
        card={(index) =>
          index === CARD ? <Card key={index} index={index} style={{ transform: "translateX(100px)", backgroundColor: "#0000ff" }} /> : null
        }
      />
    )
    view.settle()
    const beside = view.bounds(`card-${CARD + 1}`)
    const [x, y] = [beside.x + 5, beside.y + beside.height - 10]
    expect(view.sample("covers", x, y)).toEqual(BLUE)
    expect(view.renderer.getElementsAt(x, y)[0]).toBe(view.idOf(`card-${CARD}`))
    view.expectWhatABuildShows("covers", [`card-${CARD}`])
    view.finish(!verifying)
    view.unmount()
  })
})
