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

/** The boxes of the case `name` in a window of `scale`, relative to its line. */
function laidOut(name: string, scale: 1 | 2) {
  const root = createTestRoot()
  try {
    root.renderer.setWindowScaleFactor(scale)
    const refs = new Map<string, PublicInstance>()
    const ref = (key: string) => (instance: PublicInstance | null) => {
      if (instance) refs.set(key, instance)
    }
    const expected = ORACLE[`x${scale}`][name]
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
    expect(root.renderer.getA11yTree().frame?.scale_factor, "the window has the scale asked for").toBe(scale)
    return {
      scale,
      expected: expected!,
      actual: { line: rect("line"), holder: rect("holder"), label: rect("label"), ink: rect("ink") },
    }
  } finally {
    root.unmount()
  }
}

/** Chrome shares the half-leading of a line out in device pixels (a 24px line round a 19px content area has 5 device
 *  pixels above it, 2.5px at scale 2), where gpuix floors it in CSS pixels (2px): at scale 2 the baseline of a line, and
 *  every box seated on it, is up to a device pixel higher than Chrome's. That is `extent_in_line` and the ascent and
 *  descent of `line_box` being whole CSS pixels (zed `text_system.rs`), and has nothing to do with the trim. The
 *  largest difference seen is exactly this device pixel, in the heights and offsets that follow the baseline. */
const halfLeadingError = (scale: 1 | 2) => (scale === 1 ? 0 : 1 / scale)

function expectCase(name: string, scale: 1 | 2) {
  const { actual, expected } = laidOut(name, scale)
  const within = (value: number, chrome: number, tolerance: number, what: string) =>
    expect(Math.abs(value - chrome), `${name} (scale ${scale}) ${what}: ${value} != Chrome's ${chrome}`).toBeLessThanOrEqual(
      tolerance
    )
  const leading = halfLeadingError(scale)
  for (const key of ["line", "holder", "label", "ink"] as const) {
    // A width is the advance of "Hello", which Chrome's shaping can make a unit wider or narrower (seen: 2/128 at scale
    // 2, 1/64 at scale 1); x follows a width in the row cases. A height is the metric of the font rounded to a unit, and
    // a trim-start or trim-end label has the height of the line less the trim, so it has the half-leading too.
    within(actual[key][2], expected[key][2], UNIT, `${key} width`)
    within(actual[key][0], expected[key][0], UNIT, `${key} x`)
    within(actual[key][3], expected[key][3], UNIT + (key === "line" || key === "ink" ? 0 : leading), `${key} height`)
  }
  if (expected.spec.row) {
    // A flex or grid item is placed where layout puts it, the fraction included, so the label is where Chrome has it
    // but for the half-leading of the line it is seated on. The box of the span inside it is not read that way yet: it
    // is as far below the label's top as the fraction of that top (an untrimmed label at 8.5 in a 41px flex box has
    // its span at 11 and Chrome at 10.5), so it is left out.
    within(actual.label[1], expected.label[1], UNIT + leading, "label y")
    return
  }
  // The atomic is placed at a whole device pixel (the offset of an element is snapped), where Chrome keeps the
  // fraction of the label's top: the same to half a device pixel (seen: 0.48 at scale 1, 0.25 more at scale 2 on top
  // of the half-leading).
  const place = UNIT + leading + 0.5 / scale
  within(actual.holder[1], expected.holder[1], place, "holder y")
  within(actual.label[1], expected.label[1], place, "label y")
  within(actual.ink[1], expected.ink[1], place, "ink y")
  // The label is seated on the baseline by its trimmed box, and the glyphs are in the label as Chrome has them.
  within(actual.ink[1] - actual.label[1], expected.ink[1] - expected.label[1], UNIT + leading, "ink below the label")
  within(actual.holder[1], actual.label[1], UNIT, "holder y against the label")
}

const NAMES = Object.keys(ORACLE.x1)
const SCALES = [1, 2] as const

describeNative("a text-box-trimmed label in an atomic inline", () => {
  it("has an oracle for both scales", () => {
    expect(Object.keys(ORACLE.x2)).toEqual(NAMES)
    expect(NAMES.length).toBeGreaterThan(40)
  })

  for (const scale of SCALES) {
    it(`sits on the baseline of the strut, as the tab label of a design system does, at scale ${scale}`, () => {
      expectCase("ff", scale)
    })

    for (const name of NAMES) {
      it(`is where Chrome puts it at scale ${scale}: ${name}`, () => {
        expectCase(name, scale)
      })
    }
  }
})
