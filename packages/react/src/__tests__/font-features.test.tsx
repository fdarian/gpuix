import fs from "fs"
import type { ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { expectScreenshotsEqual, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

/** Only `tnum` and `pnum` are asserted by width, because the default system font
 *  has them. Glyph substitutions work too, but only in a font that carries the
 *  feature: the system font has `smcp` and `frac` but no `zero` or `onum`, so
 *  those paint like the default there (Avenir Next and Baskerville have `smcp`
 *  and `onum`). */
function capture(name: string, tree: ReactElement) {
  const testRoot = createTestRoot()
  try {
    testRoot.render(tree)
    const file = `${SHOTS_DIR}/font-features-${name}.png`
    fs.rmSync(file, { force: true })
    testRoot.renderer.captureScreenshot(file)
    const widths: Record<string, number> = {}
    for (const testId of ["ones", "zeros"]) {
      const element = testRoot.renderer.findByTestId(testId)
      expect(element, `missing ${testId}`).toBeDefined()
      widths[testId] = testRoot.renderer.getElementBounds(element!.id)!.width
    }
    const width = (testId: "ones" | "zeros") => widths[testId]!
    return { file, width }
  } finally {
    testRoot.unmount()
  }
}

function digits(style: Record<string, unknown>, parent: Record<string, unknown> = {}) {
  return (
    <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", fontSize: 32, ...parent }}>
      <text testId="ones" style={style}>1111111111</text>
      <text testId="zeros" style={style}>0000000000</text>
    </div>
  )
}

describeNative("fontVariantNumeric", () => {
  it("tabular-nums gives every digit the same advance", () => {
    const tabular = capture("tabular", digits({ fontVariantNumeric: "tabular-nums" }))
    expect(tabular.width("ones")).toBeCloseTo(tabular.width("zeros"), 1)
    // The tests below only mean something if the default is not already tabular.
    const plain = capture("plain-digits", digits({}))
    expect(plain.width("ones")).toBeLessThan(plain.width("zeros") - 1)
  })

  it("proportional-nums gives a 1 less room than a 0", () => {
    const proportional = capture("proportional", digits({ fontVariantNumeric: "proportional-nums" }))
    expect(proportional.width("ones")).toBeLessThan(proportional.width("zeros") - 1)
  })

  it("inherits down the tree, and normal resets it", () => {
    const inherited = capture("inherit", digits({}, { fontVariantNumeric: "tabular-nums" }))
    expect(inherited.width("ones")).toBeCloseTo(inherited.width("zeros"), 1)
    const reset = capture("reset", digits({ fontVariantNumeric: "normal" }, { fontVariantNumeric: "tabular-nums" }))
    expectScreenshotsEqual(reset.file, capture("plain", digits({})).file)
  })

  it("ignores an invalid combination", () => {
    expectScreenshotsEqual(
      capture("invalid", digits({ fontVariantNumeric: "tabular-nums proportional-nums" })).file,
      capture("invalid-ref", digits({})).file
    )
  })
})

describeNative("fontFeatureSettings", () => {
  it("is the same as the matching font-variant-numeric keyword", () => {
    expectScreenshotsEqual(
      capture("settings-tnum", digits({ fontFeatureSettings: '"tnum" 1' })).file,
      capture("variant-tnum", digits({ fontVariantNumeric: "tabular-nums" })).file
    )
  })

  it("accepts single quotes, on, off and a list", () => {
    expectScreenshotsEqual(
      capture("list", digits({ fontFeatureSettings: "'tnum' on, 'liga' 0, \"calt\" off" })).file,
      capture("list-ref", digits({ fontFeatureSettings: '"tnum" 1, "liga" 0, "calt" 0' })).file
    )
  })

  it("wins over fontVariantNumeric on a tag both name", () => {
    expectScreenshotsEqual(
      capture("conflict", digits({ fontVariantNumeric: "tabular-nums", fontFeatureSettings: '"tnum" 0' })).file,
      capture("conflict-ref", digits({})).file
    )
  })

  it("combines with fontVariantNumeric when they name different features", () => {
    const both = capture("both", digits({ fontVariantNumeric: "tabular-nums", fontFeatureSettings: '"zero" 1' }))
    expect(both.width("ones")).toBeCloseTo(both.width("zeros"), 1)
  })

  it("keeps an inherited property when a descendant sets the other", () => {
    const tree = (
      <div style={{ fontSize: 32, fontFeatureSettings: '"tnum" 1', display: "flex", flexDirection: "column", alignItems: "flex-start" }}>
        <text testId="ones" style={{ fontVariantNumeric: "slashed-zero" }}>1111111111</text>
        <text testId="zeros" style={{ fontVariantNumeric: "slashed-zero" }}>0000000000</text>
      </div>
    )
    const shot = capture("inherit-other", tree)
    expect(shot.width("ones")).toBeCloseTo(shot.width("zeros"), 1)
  })

  it("ignores a malformed list", () => {
    expectScreenshotsEqual(
      capture("malformed", digits({ fontFeatureSettings: "tnum 1" })).file,
      capture("malformed-ref", digits({})).file
    )
  })
})
