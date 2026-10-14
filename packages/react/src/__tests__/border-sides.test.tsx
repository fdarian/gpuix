import fs from "fs"
import type { ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rgba = [number, number, number, number]
const RED: Rgba = [255, 0, 0, 255]
const GREEN: Rgba = [0, 255, 0, 255]
const BLUE: Rgba = [0, 0, 255, 255]
const WHITE: Rgba = [255, 255, 255, 255]

/** The colour on each side of a 100x100 box with 10px borders, and its middle. */
function sides(name: string, style: Record<string, unknown>) {
  const testRoot = createTestRoot()
  try {
    const tree: ReactElement = (
      <div
        style={{
          width: 100,
          height: 100,
          backgroundColor: "#ffffff",
          color: "#00ff00",
          ...style,
        }}
      />
    )
    testRoot.render(tree)
    const file = `${SHOTS_DIR}/border-sides-${name}.png`
    fs.rmSync(file, { force: true })
    testRoot.renderer.captureScreenshot(file)
    const pixels = readLogicalPixels(file, testRoot.renderer)
    return {
      top: pixels.at(50, 3),
      right: pixels.at(96, 50),
      bottom: pixels.at(50, 96),
      left: pixels.at(3, 50),
      middle: pixels.at(50, 50),
    }
  } finally {
    testRoot.unmount()
  }
}

describeNative("per-side border colours", () => {
  it("paints each side in its own colour", () => {
    const shot = sides("longhands", {
      borderWidth: 10,
      borderTopColor: "#ff0000",
      borderRightColor: "#00ff00",
      borderBottomColor: "#0000ff",
      borderLeftColor: "#000000",
    })
    expect(shot.top).toEqual(RED)
    expect(shot.right).toEqual(GREEN)
    expect(shot.bottom).toEqual(BLUE)
    expect(shot.left).toEqual([0, 0, 0, 255])
    expect(shot.middle).toEqual(WHITE)
  })

  it("lets a per-side colour override borderColor", () => {
    const shot = sides("override", { borderWidth: 10, borderColor: "#ff0000", borderBottomColor: "#0000ff" })
    expect(shot.top).toEqual(RED)
    expect(shot.left).toEqual(RED)
    expect(shot.bottom).toEqual(BLUE)
  })

  it("resolves currentColor and colour functions per side", () => {
    const shot = sides("functions", {
      borderWidth: 10,
      borderColor: "red",
      borderLeftColor: "currentColor",
      borderRightColor: "color-mix(in srgb, blue, blue)",
    })
    expect(shot.left).toEqual(GREEN)
    expect(shot.right).toEqual(BLUE)
  })
})

describeNative("borderColor with one to four values", () => {
  it("one value covers every side", () => {
    const shot = sides("one", { borderWidth: 10, borderColor: "#ff0000" })
    expect([shot.top, shot.right, shot.bottom, shot.left]).toEqual([RED, RED, RED, RED])
  })

  it("two values are vertical then horizontal", () => {
    const shot = sides("two", { borderWidth: 10, borderColor: "#ff0000 #0000ff" })
    expect([shot.top, shot.right, shot.bottom, shot.left]).toEqual([RED, BLUE, RED, BLUE])
  })

  it("three values are top, horizontal, bottom", () => {
    const shot = sides("three", { borderWidth: 10, borderColor: "#ff0000 #0000ff #00ff00" })
    expect([shot.top, shot.right, shot.bottom, shot.left]).toEqual([RED, BLUE, GREEN, BLUE])
  })

  it("four values are top, right, bottom, left, and may be functions", () => {
    const shot = sides("four", {
      borderWidth: 10,
      borderColor: "#ff0000 rgb(0 255 0) #0000ff color-mix(in srgb, red, red)",
    })
    expect([shot.top, shot.right, shot.bottom, shot.left]).toEqual([RED, GREEN, BLUE, RED])
  })

  it("one invalid colour ignores the whole value", () => {
    const shot = sides("invalid", { borderWidth: 10, borderColor: "#ff0000 nonsense" })
    expect(shot.top).not.toEqual(RED)
    expect(shot.right).not.toEqual(RED)
  })
})

describeNative("border shorthands", () => {
  it("border sets width, colour and style for every side", () => {
    const shot = sides("shorthand", { border: "10px solid #ff0000" })
    expect([shot.top, shot.right, shot.bottom, shot.left]).toEqual([RED, RED, RED, RED])
    expect(shot.middle).toEqual(WHITE)
  })

  it("takes the parts in any order and defaults the colour to currentColor", () => {
    expect(sides("order", { border: "#0000ff solid 10px" }).top).toEqual(BLUE)
    expect(sides("current", { border: "10px solid" }).top).toEqual(GREEN)
  })

  it("a side shorthand overrides one side of border", () => {
    const shot = sides("side", { border: "10px solid #ff0000", borderBottom: "10px solid #0000ff" })
    expect([shot.top, shot.bottom]).toEqual([RED, BLUE])
    const thin = sides("side-none", { border: "10px solid #ff0000", borderLeft: "none" })
    expect(thin.left).toEqual(WHITE)
    expect(thin.top).toEqual(RED)
  })

  it("longhands override the shorthand", () => {
    const shot = sides("longhand-wins", { border: "10px solid #ff0000", borderTopColor: "#0000ff" })
    expect(shot.top).toEqual(BLUE)
    expect(shot.right).toEqual(RED)
  })

  it("dashed draws gaps", () => {
    // A dashed edge is red in places and not red in others.
    const testRoot = createTestRoot()
    try {
      testRoot.render(<div style={{ width: 100, height: 100, border: "10px dashed #ff0000" }} />)
      const file = `${SHOTS_DIR}/border-sides-dashed-row.png`
      testRoot.renderer.captureScreenshot(file)
      const pixels = readLogicalPixels(file, testRoot.renderer)
      const row = Array.from({ length: 100 }, (_, x) => pixels.at(x, 4)[0] === 255 && pixels.at(x, 4)[1] === 0)
      expect(row.some((red) => red)).toBe(true)
      expect(row.some((red) => !red)).toBe(true)
    } finally {
      testRoot.unmount()
    }
  })

  it("ignores an unsupported line style or a missing style", () => {
    const none = sides("unstyled", {})
    expect(sides("dotted", { border: "10px dotted red" })).toEqual(none)
    expect(sides("no-style", { border: "10px red" })).toEqual(none)
  })

  it("repaints currentColor borders when a hover changes color", () => {
    const testRoot = createTestRoot()
    try {
      testRoot.render(
        <div style={{ width: 100, height: 100, color: "#0000ff", border: "10px solid", hover: { color: "#ff0000" } }} />
      )
      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      const file = `${SHOTS_DIR}/border-sides-hover.png`
      testRoot.renderer.captureScreenshot(file)
      expect(readLogicalPixels(file, testRoot.renderer).at(50, 3)).toEqual(RED)
    } finally {
      testRoot.unmount()
    }
  })
})
