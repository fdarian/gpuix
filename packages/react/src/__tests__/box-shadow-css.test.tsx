import fs from "fs"
import type { ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { expectScreenshotsEqual, readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

function capture(name: string, tree: ReactElement) {
  const testRoot = createTestRoot()
  try {
    testRoot.render(tree)
    const file = `${SHOTS_DIR}/box-shadow-css-${name}.png`
    fs.rmSync(file, { force: true })
    testRoot.renderer.captureScreenshot(file)
    return { file, pixels: readLogicalPixels(file, testRoot.renderer) }
  } finally {
    testRoot.unmount()
  }
}

/** A 40x40 box with room around it for a shadow. */
function boxed(style: Record<string, unknown>) {
  return (
    <div style={{ padding: 40, color: "#0000ff" }}>
      <div style={{ width: 40, height: 40, backgroundColor: "#ffffff", ...style }} />
    </div>
  )
}

describeNative("boxShadow as a CSS string", () => {
  it("paints exactly like the object form", () => {
    const css = capture("css", boxed({ boxShadow: "0 6px 4px 2px rgba(255, 0, 0, 0.5)" }))
    const object = capture(
      "object",
      boxed({
        boxShadow: { offsetX: 0, offsetY: 6, blurRadius: 4, spreadRadius: 2, color: "rgba(255, 0, 0, 0.5)" },
      })
    )
    expectScreenshotsEqual(css.file, object.file)
  })

  it("resolves em against the element's font size", () => {
    const em = capture("em", boxed({ fontSize: 10, boxShadow: "0 0.6em 0 0.2em #ff0000" }))
    const px = capture("em-px", boxed({ fontSize: 10, boxShadow: "0 6px 0 2px #ff0000" }))
    expectScreenshotsEqual(em.file, px.file)
  })

  it("takes several layers, one of them inset, in the order given", () => {
    const css = capture(
      "layers-css",
      boxed({ boxShadow: "0 4px 8px #00000080, inset 0 0 0 4px #ff0000, 6px 6px 0 #00ff00" })
    )
    const array = capture(
      "layers-array",
      boxed({
        boxShadow: [
          { offsetX: 0, offsetY: 4, blurRadius: 8, spreadRadius: 0, color: "#00000080" },
          { offsetX: 0, offsetY: 0, blurRadius: 0, spreadRadius: 4, color: "#ff0000", inset: true },
          { offsetX: 6, offsetY: 6, blurRadius: 0, spreadRadius: 0, color: "#00ff00" },
        ],
      })
    )
    expectScreenshotsEqual(css.file, array.file)
    // The inset ring is inside the box edge.
    const [r, g, b] = css.pixels.at(41, 60)
    expect([r, g, b]).toEqual([255, 0, 0])
  })

  it("accepts the colour first, last, or in the middle of the layer", () => {
    const reference = capture("colour-last", boxed({ boxShadow: "8px 8px 0 0 #ff0000" }))
    expectScreenshotsEqual(
      reference.file,
      capture("colour-first", boxed({ boxShadow: "#ff0000 8px 8px 0 0" })).file
    )
    expectScreenshotsEqual(
      capture("inset-first", boxed({ boxShadow: "inset 0 0 0 4px #ff0000" })).file,
      capture("inset-last", boxed({ boxShadow: "0 0 0 4px #ff0000 inset" })).file
    )
  })

  it("defaults the colour to currentColor, as in CSS", () => {
    const shot = capture("current", boxed({ boxShadow: "0 0 0 10px" }))
    expect(shot.pixels.at(35, 60)).toEqual([0, 0, 255, 255])
    const same = capture("current-explicit", boxed({ boxShadow: "0 0 0 10px currentColor" }))
    expectScreenshotsEqual(shot.file, same.file)
  })

  it("resolves colour functions inside a layer", () => {
    const shot = capture("mix", boxed({ boxShadow: "0 0 0 10px color-mix(in srgb, red, red)" }))
    expect(shot.pixels.at(35, 60)).toEqual([255, 0, 0, 255])
  })

  it("accepts px lengths and a bare 0", () => {
    expectScreenshotsEqual(
      capture("px", boxed({ boxShadow: "0px 0px 0px 6px #ff0000" })).file,
      capture("bare", boxed({ boxShadow: "0 0 0 6px #ff0000" })).file
    )
  })

  it("ignores an invalid string, like any invalid declaration", () => {
    const none = capture("none", boxed({}))
    for (const bad of ["1px", "4 4 red", "1px 2px 3px 4px 5px red", "red", "1foo 2foo red", "1% 2% red"]) {
      expectScreenshotsEqual(capture("invalid", boxed({ boxShadow: bad })).file, none.file)
    }
    expectScreenshotsEqual(capture("keyword-none", boxed({ boxShadow: "none" })).file, none.file)
  })

  it("keeps the object and array forms working", () => {
    const shot = capture(
      "object-still",
      boxed({ boxShadow: { offsetX: 0, offsetY: 0, blurRadius: 0, spreadRadius: 10, color: "#ff0000" } })
    )
    expect(shot.pixels.at(35, 60)).toEqual([255, 0, 0, 255])
  })

  it("applies in a hover state", () => {
    const testRoot = createTestRoot()
    try {
      testRoot.render(boxed({ hover: { boxShadow: "0 0 0 10px #ff0000" } }))
      testRoot.renderer.nativeSimulateMouseMove(60, 60)
      const file = `${SHOTS_DIR}/box-shadow-css-hover.png`
      testRoot.renderer.captureScreenshot(file)
      expect(readLogicalPixels(file, testRoot.renderer).at(35, 60)).toEqual([255, 0, 0, 255])
    } finally {
      testRoot.unmount()
    }
  })
})
