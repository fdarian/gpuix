/// A text-box-trimmed label in an atomic inline (`inline-block`, `inline-flex`, `inline-grid`) sits on the line of the
/// block that holds it where Chrome puts it: the atomic takes its baseline from the label, and the baseline of a
/// trimmed text is counted from the top of its trimmed box, so the label is seated on the baseline of the strut and
/// not 5 to 6 px too high (#66: the stable label of a tab, an `inline-grid` of two stacked cap-trimmed spans in a
/// 16px/24px block). The same baseline puts the label in a flex or grid row with `alignItems: "baseline"` (the `row-`
/// cases).
///
/// `fixtures/atomic-trim-baseline-oracle.html` is the page, `fixtures/atomic-trim-baseline-oracle.json` the boxes
/// headless Chrome reports for it at device scale 1 and 2 (`getBoundingClientRect` relative to the line,
/// `[x, y, width, height]`; regenerate with `atomic-trim-baseline-oracle.sh`), and the spec of each case. The font is
/// Menlo, whose metrics are exact: the label's box is the cap height (11.671875px at 16px, 1493/2048 em) or the
/// x-height tall. Sizes and the place of the glyphs in the label are compared to 1/64 px; the top of the atomic is
/// compared to half a device pixel, because gpuix snaps the origin an atomic inline is placed at to a device pixel
/// where Chrome keeps the fraction (the label's top is 5.328125 in Chrome and 5 here).

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rect = [number, number, number, number]

interface Spec {
  holder: "block" | "flex" | "grid"
  /** The line is a flex or grid container with `alignItems: "baseline"` holding a plain span and the label. */
  row?: "flex" | "grid"
  stack: boolean
  trim: string
  edge: string
  line: string
  labelLine?: string
  labelSize?: string
  align?: string
}

interface Case {
  line: Rect
  holder: Rect
  label: Rect
  ink: Rect
  spec: Spec
}

const ORACLE: Record<"x1" | "x2", Record<string, Case>> = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/atomic-trim-baseline-oracle.json"),
    "utf8"
  )
)

const UNIT = 1 / 64

function labelOf(spec: Spec, ref: (name: string) => (instance: PublicInstance | null) => void, hidden: boolean) {
  const style: Record<string, number | string> = { textBox: `${spec.trim} ${spec.edge}` }
  if (spec.stack) {
    style.gridArea = "1 / 1"
  }
  if (hidden) {
    style.visibility = "hidden"
  }
  if (spec.labelLine) {
    style.lineHeight = spec.labelLine
  }
  if (spec.labelSize) {
    style.fontSize = Number.parseFloat(spec.labelSize)
  }
  const name = hidden ? "hidden" : "label"
  const ink = <span ref={hidden ? undefined : ref("ink")}>Hello</span>
  return spec.holder === "block" ? (
    <div ref={hidden ? undefined : ref(name)} style={style}>
      {ink}
    </div>
  ) : (
    <span ref={hidden ? undefined : ref(name)} style={style}>
      {ink}
    </span>
  )
}

/** The boxes of the case `name`, relative to its line, and the device scale the window has. */
function laidOut(name: string) {
  const root = createTestRoot()
  try {
    const refs = new Map<string, PublicInstance>()
    const ref = (key: string) => (instance: PublicInstance | null) => {
      if (instance) refs.set(key, instance)
    }
    const scale = root.renderer.getA11yTree().frame?.scale_factor
    expect(scale === 1 || scale === 2, `a window of scale ${scale}`).toBe(true)
    const expected = ORACLE[`x${scale as 1 | 2}`][name]
    expect(expected, `no oracle case ${name}`).toBeDefined()
    const spec = expected!.spec
    const line = { width: 300, fontFamily: "Menlo", fontSize: 16, lineHeight: spec.line, color: "#000000" }
    root.render(
      spec.row ? (
        <div
          ref={ref("line")}
          style={{ ...line, display: spec.row, alignItems: "baseline", gridTemplateColumns: "auto auto", justifyContent: "start" }}
        >
          <span ref={ref("holder")} style={{ whiteSpace: "nowrap" }}>
            Plain
          </span>
          {labelOf(spec, ref, false)}
        </div>
      ) : (
        <div ref={ref("line")} style={line}>
          <span
            ref={ref("holder")}
            style={{
              display: spec.holder === "block" ? "inline-block" : spec.holder === "flex" ? "inline-flex" : "inline-grid",
              whiteSpace: "nowrap",
              verticalAlign: spec.align,
            }}
          >
            {spec.stack ? labelOf(spec, ref, true) : null}
            {labelOf(spec, ref, false)}
          </span>
        </div>
      )
    )
    root.renderer.dispatchNativeEvents()
    const origin = refs.get("line")!.getBoundingClientRect()
    const rect = (key: string): Rect => {
      const box = refs.get(key)!.getBoundingClientRect()
      return [box.x - origin.x, box.y - origin.y, box.width, box.height]
    }
    return {
      scale: scale as number,
      expected: expected!,
      actual: { line: rect("line"), holder: rect("holder"), label: rect("label"), ink: rect("ink") },
    }
  } finally {
    root.unmount()
  }
}

function expectCase(name: string) {
  const found = laidOut(name)
  const { actual, expected, scale } = found
  const within = (value: number, chrome: number, tolerance: number, what: string) =>
    expect(Math.abs(value - chrome), `${name} (scale ${scale}) ${what}: ${value} != Chrome's ${chrome}`).toBeLessThanOrEqual(
      tolerance
    )
  for (const key of ["line", "holder", "label", "ink"] as const) {
    // A width is the advance of "Hello", which Chrome's shaping can make a unit wider or narrower; a height is the
    // metric of the font rounded to a unit, and a line a unit taller than Chrome's where the label's rounds up.
    within(actual[key][2], expected[key][2], 2 * UNIT, `${key} width`)
    within(actual[key][3], expected[key][3], UNIT, `${key} height`)
    within(actual[key][0], expected[key][0], 2 * UNIT, `${key} x`)
  }
  // The atomic is placed at a whole device pixel (the offset of an element is snapped), where Chrome keeps the
  // fraction of the label's top: the same to half a device pixel. At scale 2 the strut's baseline is also up to half
  // a pixel higher than Chrome's, which shares the half-leading out in device pixels and gpuix in CSS pixels, so a
  // line of 24px with a content area of 19 has its baseline at 17 and not 17.5. This is not about the trim: the
  // label's offset from that baseline is exact.
  for (const key of ["holder", "label"] as const) {
    within(actual[key][1], expected[key][1], (scale === 1 ? 0.5 : 0.75) + UNIT, `${key} y`)
  }
  if (expected.spec.row) {
    // A flex or grid item is placed where layout puts it, the fraction included: the label is where Chrome has it
    // to 1/64 (and at scale 2 the half pixel of the strut's baseline above). The box of the span inside it is not read that way yet: it is as far below the label's top as the
    // fraction of that top (an untrimmed label at 8.5 in a 41px flex box has its span at 11 and Chrome at 10.5), so
    // it is left out.
    within(actual.label[1], expected.label[1], (scale === 1 ? 0 : 0.5) + UNIT, "label y")
    return
  }
  within(actual.ink[1], expected.ink[1], (scale === 1 ? 0.5 : 0.75) + UNIT, "ink y")
  // The label is seated on the baseline by its trimmed box, and the glyphs are in the label as Chrome has them.
  within(actual.ink[1] - actual.label[1], expected.ink[1] - expected.label[1], UNIT, "ink below the label")
  within(actual.holder[1], actual.label[1], UNIT, "holder y against the label")
}

const NAMES = Object.keys(ORACLE.x1)

describeNative("a text-box-trimmed label in an atomic inline", () => {
  it("has an oracle for both scales", () => {
    expect(Object.keys(ORACLE.x2)).toEqual(NAMES)
    expect(NAMES.length).toBeGreaterThan(40)
  })

  it("sits on the baseline of the strut, as the tab label of a design system does", () => {
    expectCase("ff")
  })

  for (const name of NAMES) {
    it(`is where Chrome puts it: ${name}`, () => {
      expectCase(name)
    })
  }
})
