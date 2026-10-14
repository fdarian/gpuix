import fs from "fs"
import type { ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { expectScreenshotsEqual, readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

let counter = 0

/** Text on a mid-grey card, so black and white text both show. */
function shot(style: Record<string, unknown>, textStyle: Record<string, unknown> = {}, appearance?: string) {
  const testRoot = createTestRoot()
  try {
    if (appearance) testRoot.renderer.setAppearance(appearance)
    const tree: ReactElement = (
      <div style={{ width: 160, height: 100, backgroundColor: "#808080", ...style }}>
        <div>
          <text style={{ fontSize: 60, ...textStyle }}>MW</text>
        </div>
      </div>
    )
    testRoot.render(tree)
    const file = `${SHOTS_DIR}/default-text-color-${counter++}.png`
    fs.rmSync(file, { force: true })
    testRoot.renderer.captureScreenshot(file)
    return { file, appearance: testRoot.renderer.getAppearance() }
  } finally {
    testRoot.unmount()
  }
}

describeNative("default text colour", () => {
  it("is black by default and under a light scheme", () => {
    const black = shot({ color: "#000000" }).file
    expectScreenshotsEqual(shot({}).file, black)
    expectScreenshotsEqual(shot({ colorScheme: "light" }).file, black)
    expectScreenshotsEqual(shot({ colorScheme: "normal" }).file, black)
    expectScreenshotsEqual(shot({ colorScheme: "only light" }).file, black)
  })

  it("is white under a dark scheme", () => {
    const white = shot({ color: "#ffffff" }).file
    expectScreenshotsEqual(shot({ colorScheme: "dark" }).file, white)
    expectScreenshotsEqual(shot({ colorScheme: "only dark" }).file, white)
  })

  it("follows the window appearance under light dark", () => {
    const dark = shot({ colorScheme: "light dark" })
    const expected = shot({ color: dark.appearance === "dark" ? "#ffffff" : "#000000" })
    expectScreenshotsEqual(dark.file, expected.file)
  })

  it("is inherited by descendants without being re-declared", () => {
    const white = shot({ color: "#ffffff" }).file
    // The text sits two levels below the element that sets the scheme.
    expectScreenshotsEqual(shot({ colorScheme: "dark" }).file, white)
  })

  it("lets a light island inside a dark scheme go back to black", () => {
    const black = shot({ color: "#000000" }).file
    expectScreenshotsEqual(shot({ colorScheme: "dark" }, { colorScheme: "light" }).file, black)
  })

  it("is replaced by any declared color, whatever the scheme", () => {
    const red = shot({ color: "#ff0000" }).file
    expectScreenshotsEqual(shot({ colorScheme: "dark", color: "#ff0000" }).file, red)
    expectScreenshotsEqual(shot({ color: "#ff0000", colorScheme: "dark" }, { colorScheme: "light" }).file, red)
  })

  it("makes currentColor white too", () => {
    const testRoot = createTestRoot()
    try {
      testRoot.render(<div style={{ colorScheme: "dark", width: 40, height: 40, backgroundColor: "currentColor" }} />)
      const file = `${SHOTS_DIR}/default-text-color-current.png`
      testRoot.renderer.captureScreenshot(file)
      expect(readLogicalPixels(file, testRoot.renderer).at(20, 20)).toEqual([255, 255, 255, 255])
    } finally {
      testRoot.unmount()
    }
  })
})
