import fs from "fs"
import type { ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { expectScreenshotsEqual, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

/** The default font is the system UI font, a variable font on macOS with a `wght`
 *  axis from 1 to 1000 whose named instances are 100, 200, ... 900 apart from the
 *  few in between (590 is its semibold). Width is a stand-in for the weight: a
 *  heavier `wght` is wider. */
const HEAVY = '"wght" 900'
const LIGHT = '"wght" 100'

function capture(name: string, tree: ReactElement, testIds: string[] = ["text"]) {
  const testRoot = createTestRoot()
  try {
    testRoot.render(tree)
    const file = `${SHOTS_DIR}/font-variation-settings-${name}.png`
    fs.rmSync(file, { force: true })
    testRoot.renderer.captureScreenshot(file)
    const widths: Record<string, number> = {}
    for (const testId of testIds) {
      const element = testRoot.renderer.findByTestId(testId)
      expect(element, `missing ${testId}`).toBeDefined()
      widths[testId] = testRoot.renderer.getElementBounds(element!.id)!.width
    }
    return { file, width: (testId = "text") => widths[testId]! }
  } finally {
    testRoot.unmount()
  }
}

function line(style: Record<string, unknown>, parent: Record<string, unknown> = {}) {
  return (
    <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", fontSize: 32, ...parent }}>
      <text testId="text" style={style}>WWWWWWWWWW</text>
    </div>
  )
}

describeNative("fontVariationSettings", () => {
  it("changes the width of the text against normal", () => {
    const plain = capture("plain", line({})).width()
    const light = capture("light", line({ fontVariationSettings: LIGHT })).width()
    const heavy = capture("heavy", line({ fontVariationSettings: HEAVY })).width()
    expect(light).toBeLessThan(plain - 1)
    expect(heavy).toBeGreaterThan(plain + 1)
  })

  it("takes any number on the axis", () => {
    const at = (value: number) => capture(`wght-${value}`, line({ fontVariationSettings: `"wght" ${value}` })).width()
    expect(at(500)).toBeLessThan(at(650))
    expect(at(650)).toBeLessThan(at(800))
  })

  it("accepts single quotes, normal, and several axes", () => {
    expectScreenshotsEqual(
      capture("single", line({ fontVariationSettings: "'wght' 900" })).file,
      capture("double", line({ fontVariationSettings: HEAVY })).file
    )
    expectScreenshotsEqual(
      capture("normal", line({ fontVariationSettings: "normal" })).file,
      capture("normal-ref", line({})).file
    )
    // wdth is the width axis of the system font, 30 to 150.
    const narrow = capture("axes", line({ fontVariationSettings: '"wght" 900, "wdth" 60' })).width()
    expect(narrow).toBeLessThan(capture("axes-ref", line({ fontVariationSettings: HEAVY })).width())
  })

  it("takes the last value of a tag named twice", () => {
    expectScreenshotsEqual(
      capture("twice", line({ fontVariationSettings: `${LIGHT}, ${HEAVY}` })).file,
      capture("twice-ref", line({ fontVariationSettings: HEAVY })).file
    )
  })

  it("ignores an axis the font does not have", () => {
    expectScreenshotsEqual(
      capture("unknown", line({ fontVariationSettings: '"ZZZZ" 500' })).file,
      capture("unknown-ref", line({})).file
    )
  })

  it("clamps to the range of the axis", () => {
    expectScreenshotsEqual(
      capture("clamp", line({ fontVariationSettings: '"wght" 5000' })).file,
      capture("clamp-ref", line({ fontVariationSettings: '"wght" 1000' })).file
    )
  })

  it("inherits down the tree, and normal resets it", () => {
    const tree = (child: Record<string, unknown>) => (
      <div style={{ fontSize: 32, fontVariationSettings: HEAVY, display: "flex", flexDirection: "column", alignItems: "flex-start" }}>
        <div style={{ display: "flex" }}>
          <text testId="text" style={child}>WWWWWWWWWW</text>
        </div>
      </div>
    )
    const inherited = capture("inherit", tree({}))
    expect(inherited.width()).toBeCloseTo(capture("inherit-ref", line({ fontVariationSettings: HEAVY })).width(), 1)
    const reset = capture("reset", tree({ fontVariationSettings: "normal" }))
    expect(reset.width()).toBeCloseTo(capture("reset-ref", line({})).width(), 1)
    const lighter = capture("override", tree({ fontVariationSettings: LIGHT }))
    expect(lighter.width()).toBeLessThan(reset.width())
  })

  it("combines with fontFeatureSettings", () => {
    const digits = (style: Record<string, unknown>) => (
      <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", fontSize: 32 }}>
        <text testId="ones" style={style}>1111111111</text>
        <text testId="zeros" style={style}>0000000000</text>
      </div>
    )
    const tabular = capture("combined", digits({ fontFeatureSettings: '"tnum" 1', fontVariationSettings: HEAVY }), ["ones", "zeros"])
    expect(tabular.width("ones")).toBeCloseTo(tabular.width("zeros"), 1)
    const lighter = capture("combined-ref", digits({ fontFeatureSettings: '"tnum" 1' }), ["ones", "zeros"])
    expect(tabular.width("ones")).toBeGreaterThan(lighter.width("ones") + 1)
  })
})

describeNative("fontVariationSettings on a span", () => {
  const spans = (first: Record<string, unknown>, second: Record<string, unknown>) => (
    <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", fontSize: 32 }}>
      <text testId="text">
        <span style={first}>WWWWW</span>
        <span style={second}>WWWWW</span>
      </text>
    </div>
  )

  it("applies to its own run, on the same line as the others", () => {
    const both = capture("span-plain", spans({}, {})).width()
    const second = capture("span-second", spans({}, { fontVariationSettings: HEAVY })).width()
    const first = capture("span-first", spans({ fontVariationSettings: HEAVY }, {})).width()
    const heavy = capture("span-both", spans({ fontVariationSettings: HEAVY }, { fontVariationSettings: HEAVY })).width()
    expect(second).toBeGreaterThan(both + 1)
    expect(second).toBeLessThan(heavy - 1)
    expect(first).toBeCloseTo(second, 0)
    // Two runs of the same family keep their own face.
    const mixed = capture("span-mixed", spans({ fontVariationSettings: LIGHT }, { fontVariationSettings: HEAVY })).width()
    const lightBoth = capture("span-light", spans({ fontVariationSettings: LIGHT }, { fontVariationSettings: LIGHT })).width()
    expect(mixed).toBeGreaterThan(lightBoth + 1)
    expect(mixed).toBeLessThan(heavy - 1)
  })

  it("is inherited by a nested span, and normal resets it", () => {
    const tree = (inner: Record<string, unknown>) => (
      <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", fontSize: 32 }}>
        <text testId="text">
          <span style={{ fontVariationSettings: HEAVY }}>
            WWWWW<span style={inner}>WWWWW</span>
          </span>
        </text>
      </div>
    )
    const inherited = capture("nested", tree({})).width()
    expect(inherited).toBeCloseTo(capture("nested-ref", spans({ fontVariationSettings: HEAVY }, { fontVariationSettings: HEAVY })).width(), 0)
    expect(capture("nested-reset", tree({ fontVariationSettings: "normal" })).width()).toBeLessThan(inherited - 1)
  })

  it("is inherited from the paragraph's container by an unstyled span", () => {
    const tree = (
      <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", fontSize: 32, fontVariationSettings: HEAVY }}>
        <text testId="text">
          <span style={{ color: "red" }}>WWWWW</span>WWWWW
        </text>
      </div>
    )
    expect(capture("container", tree).width()).toBeCloseTo(capture("container-ref", line({ fontVariationSettings: HEAVY })).width(), 0)
  })
})

describeNative("fontWeight on a variable font", () => {
  const weight = (value: number | string) => capture(`weight-${value}`, line({ fontWeight: value })).width()

  it("sets wght to any weight, not only the named instances", () => {
    const [w600, w650, w700] = [weight(600), weight(650), weight(700)]
    expect(w600).toBeLessThan(w650 - 0.5)
    expect(w650).toBeLessThan(w700 - 0.5)
  })

  it("is the same as the matching wght", () => {
    expectScreenshotsEqual(
      capture("weight-650", line({ fontWeight: 650 })).file,
      capture("wght-650", line({ fontVariationSettings: '"wght" 650' })).file
    )
  })

  it("gives way to a wght in fontVariationSettings", () => {
    expectScreenshotsEqual(
      capture("weight-over", line({ fontWeight: 900, fontVariationSettings: '"wght" 400' })).file,
      capture("weight-over-ref", line({})).file
    )
  })

  it("keeps the weights the named instances already give", () => {
    expectScreenshotsEqual(capture("weight-400", line({ fontWeight: 400 })).file, capture("weight-normal", line({})).file)
    expectScreenshotsEqual(capture("weight-bold", line({ fontWeight: "bold" })).file, capture("weight-700", line({ fontWeight: 700 })).file)
  })
})

describeNative("fontVariationSettings on form fields", () => {
  function field(name: string, tag: "input" | "textarea", style: Record<string, unknown>, parent: Record<string, unknown> = {}, text: "value" | "placeholder" = "value") {
    const props = text === "value" ? { value: "WWWWWWWWWW", readOnly: true } : { placeholder: "WWWWWWWWWW" }
    const Field = tag
    const testRoot = createTestRoot()
    try {
      testRoot.render(
        <div style={{ fontSize: 32, width: 600, ...parent }}>
          <Field {...props} style={{ width: 600, border: "none", padding: 0, color: "white", ...style }} />
        </div>
      )
      const file = `${SHOTS_DIR}/font-variation-settings-field-${name}.png`
      fs.rmSync(file, { force: true })
      testRoot.renderer.captureScreenshot(file)
      return file
    } finally {
      testRoot.unmount()
    }
  }

  function expectScreenshotsNotIdentical(left: string, right: string) {
    expect(fs.readFileSync(left).equals(fs.readFileSync(right))).toBe(false)
  }

  for (const tag of ["input", "textarea"] as const) {
    for (const text of ["value", "placeholder"] as const) {
      describe(`${tag} ${text}`, () => {
        const shot = (name: string, style: Record<string, unknown>, parent?: Record<string, unknown>) =>
          field(`${tag}-${text}-${name}`, tag, style, parent, text)

        it("takes fontVariationSettings from its own style, as it takes it from an ancestor", () => {
          const own = shot("own", { fontVariationSettings: HEAVY })
          expectScreenshotsEqual(own, shot("inherited", {}, { fontVariationSettings: HEAVY }))
          expectScreenshotsNotIdentical(own, shot("plain", {}))
        })

        it("lets its own value override an ancestor's", () => {
          expectScreenshotsEqual(
            shot("override", { fontVariationSettings: "normal" }, { fontVariationSettings: HEAVY }),
            shot("override-ref", {})
          )
        })

        it("takes the weight of a variable font on wght", () => {
          expectScreenshotsEqual(shot("weight", { fontWeight: 650 }), shot("wght", { fontVariationSettings: '"wght" 650' }))
          expectScreenshotsNotIdentical(shot("weight-a", { fontWeight: 650 }), shot("weight-b", { fontWeight: 600 }))
        })
      })
    }
  }
})

describeNative("an invalid fontVariationSettings is an uncaught error", () => {
  function errorsOf(style: Record<string, unknown>) {
    const errors: Error[] = []
    const { render, unmount } = createTestRoot({ onUncaughtError: (error) => errors.push(error) })
    try {
      render(<div style={style} />)
      return errors.map((error) => error.message).join("\n")
    } finally {
      unmount()
    }
  }

  it("names the property and the value", () => {
    for (const value of ["wght 900", '"wgh" 900', '"wght"', '"wght" 900,', '"wght" 900px', '"wght" on', "bold"]) {
      expect(errorsOf({ fontVariationSettings: value }), value).toContain(`invalid fontVariationSettings: ${JSON.stringify(value)}`)
    }
  })

  it("accepts what CSS has", () => {
    for (const value of ["normal", '"wght" 900', "'wght' 900, \"wdth\" 90.5", '"slnt" -10', '"wght" 1e3', '"wght" .5']) {
      expect(errorsOf({ fontVariationSettings: value }), value).toBe("")
    }
  })
})
