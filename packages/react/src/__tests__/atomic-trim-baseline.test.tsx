/// A text-box-trimmed label in an atomic inline (`inline-block`, `inline-flex`, `inline-grid`) sits on the line of the
/// block that holds it where Chrome puts it: the atomic takes its baseline from the label, and the baseline of a
/// trimmed text is counted from the top of its trimmed box, so the label is seated on the baseline of the strut and
/// not 5 to 6 px too high (#66: the stable label of a tab, an `inline-grid` of two stacked cap-trimmed spans in a
/// 16px/24px block). The same baseline puts the label in a flex or grid row with `alignItems: "baseline"` (the `row-`
/// cases). The `chip`, `flex-badge` and `block-badge` cases are the other half of the same rule: a 20px badge (`inline-flex`
/// or `inline-block`, trimmed text or not) in a block line sits on the strut's baseline, and the strut is the wrapper's
/// own font and line height (#72: Chrome's 13px/20px wrapper is 21px with the badge at +1, a wrapper that inherits the
/// default 16px/24px is 24px with it at +4, in both browsers, the `-16-24` cases).
///
/// `fixtures/atomic-trim-baseline-oracle.html` is the page, `fixtures/atomic-trim-baseline-oracle.json` the boxes
/// headless Chrome reports for it at device scale 1 and 2 (`getBoundingClientRect` relative to the line,
/// `[x, y, width, height]`; regenerate with `atomic-trim-baseline-oracle.sh`), and the spec of each case. The font is
/// Menlo, whose metrics are exact: the label's box is the cap height (11.671875px at 16px, 1493/2048 em) or the
/// x-height tall. Sizes, the place of the atomic and the place of the glyphs in the label are compared to 1/64 px: an
/// atomic inline keeps the fraction of its place in the line in every box it reports, as Chrome does (the label's top is
/// 5.328125 in both), and is painted where that place rounds half up to a device pixel.
///
/// The `frac-` cases are boxes at a fractional place on the page, read in page coordinates: an atomic inline whose line
/// top is a device pixel tie (x.5 at scale 1, the odd quarters at scale 2: a badge in a block, in a table cell centred
/// vertically, an empty `inline-block`), a paragraph at a fractional x and y with an atomic in it, and a span (and a
/// badge) in a flex and a grid item at a fractional top (#80).

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
  /** A box at a fractional place on the page, its parts read in page coordinates, see `fractionOf`. */
  frac?: true
  kind?: "badge" | "bare" | "paragraph" | "item"
  place?: "block" | "cell"
  left?: number
  top?: number
  /** A badge in the item of a flex or grid container, and not a span. */
  chip?: boolean
  /** A badge case: a 20px `display` span holding the text "x.com" in a wrapper of `size` / `line`, see `badgeOf`. */
  badge?: true
  display?: "inline-flex" | "inline-block"
  /** The wrapper is a span in a flex-wrap row (`item`) or a block (`div`). */
  wrap?: "item" | "div"
  size?: string
  holder?: "block" | "flex" | "grid"
  /** The line is a flex or grid container with `alignItems: "baseline"` holding a plain span and the label. */
  row?: "flex" | "grid"
  stack?: boolean
  /** The `text-box-trim` of a label (`trim-both`), or whether a badge's text is trimmed. */
  trim?: string | boolean
  edge?: string
  line?: string
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

type FractionPart = "case" | "cell" | "item" | "span" | "holder" | "label"

interface Fraction {
  case: Rect
  cell?: Rect
  item?: Rect
  span?: Rect
  holder?: Rect
  label?: Rect
  spec: Spec
}

const ORACLE: Record<"x1" | "x2", Record<string, Case & Fraction>> = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/atomic-trim-baseline-oracle.json"),
    "utf8"
  )
)

const UNIT = 1 / 64

type Ref = (name: string) => (instance: PublicInstance | null) => void

function labelOf(spec: Spec, ref: Ref, hidden: boolean) {
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

/** The chip of a design system: `inline-flex` (or `inline-block`) and 20px tall, 8px of padding and an 11px/14px text in
 *  a span of its own, cap-trimmed (a block, as a flex item) or not, in a wrapper of the spec's font size and line height
 *  (#72: the ThinkingSteps source chip). The label and the ink are the span of the text. */
function badgeOf(spec: Spec, ref: Ref) {
  const wrapper = { fontFamily: "Menlo", fontSize: Number.parseFloat(spec.size!), lineHeight: spec.line!, width: 300, color: "#000000" }
  const label = (
    <span ref={ref("label")} style={spec.trim ? { display: "block", textBox: "trim-both cap alphabetic" } : undefined}>
      x.com
    </span>
  )
  const badge = (
    <span
      ref={ref("holder")}
      style={{
        display: spec.display,
        alignItems: "center",
        whiteSpace: "nowrap",
        height: 20,
        paddingLeft: 8,
        paddingRight: 8,
        fontSize: 11,
        lineHeight: "14px",
      }}
    >
      {label}
    </span>
  )
  return spec.wrap === "item" ? (
    <div style={{ ...wrapper, display: "flex", flexWrap: "wrap" }}>
      <span ref={ref("line")}>{badge}</span>
    </div>
  ) : (
    <div ref={ref("line")} style={wrapper}>
      {badge}
    </div>
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
    const line = { width: 300, fontFamily: "Menlo", fontSize: 16, lineHeight: spec.line!, color: "#000000" }
    root.render(
      spec.badge ? (
        badgeOf(spec, ref)
      ) : spec.row ? (
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
      actual: {
        line: rect("line"),
        holder: rect("holder"),
        label: rect("label"),
        ink: rect(spec.badge ? "label" : "ink"),
      },
    }
  } finally {
    root.unmount()
  }
}

/** The chip of a `frac-` case: the `inline-flex` badge with a cap-trimmed text, or an empty `inline-block` of 20px. */
function chipOf(bare: boolean, ref: Ref) {
  if (bare) {
    return <span ref={ref("holder")} style={{ display: "inline-block", width: 20, height: 20, backgroundColor: "#dddddd" }} />
  }
  return (
    <span
      ref={ref("holder")}
      style={{
        display: "inline-flex",
        alignItems: "center",
        whiteSpace: "nowrap",
        height: 20,
        paddingLeft: 8,
        paddingRight: 8,
        fontSize: 11,
        lineHeight: "14px",
      }}
    >
      <span ref={ref("label")} style={{ display: "block", textBox: "trim-both cap alphabetic" }}>
        x.com
      </span>
    </span>
  )
}

/** The boxes of the `frac-` case `name` in a window of `scale`, in page coordinates. */
function fractionOf(name: string, scale: 1 | 2) {
  const root = createTestRoot()
  try {
    root.renderer.setWindowScaleFactor(scale)
    const refs = new Map<string, PublicInstance>()
    const ref: Ref = (key) => (instance) => {
      if (instance) refs.set(key, instance)
    }
    const expected = ORACLE[`x${scale}`][name]
    expect(expected, `no oracle case ${name}`).toBeDefined()
    const spec = expected!.spec
    const box = { position: "absolute", left: spec.left, top: spec.top, width: 300, fontFamily: "Menlo", color: "#000000" } as const
    switch (spec.kind) {
      case "badge":
      case "bare": {
        const bare = spec.kind === "bare"
        const style = { ...box, fontSize: bare ? 16 : 13, lineHeight: bare ? "24px" : "20px" }
        const chip = chipOf(bare, ref)
        root.render(
          spec.place === "cell" ? (
            <div ref={ref("case")} style={{ ...style, display: "table" }}>
              <div style={{ display: "table-row" }}>
                <div
                  ref={ref("cell")}
                  style={{ display: "table-cell", verticalAlign: "middle", height: 41, paddingTop: 8, paddingBottom: 8, paddingLeft: 12, paddingRight: 12 }}
                >
                  {chip}
                </div>
              </div>
            </div>
          ) : (
            <div ref={ref("case")} style={style}>
              {chip}
            </div>
          )
        )
        break
      }
      case "paragraph":
        root.render(
          <div ref={ref("case")} style={{ ...box, fontSize: 16, lineHeight: "24px" }}>
            <span ref={ref("span")}>Hello</span> {chipOf(true, ref)} world
          </div>
        )
        break
      case "item":
        root.render(
          <div ref={ref("case")} style={{ ...box, display: spec.holder, fontSize: 16, lineHeight: "24px" }}>
            <div ref={ref("item")} style={{ height: 41, ...(spec.chip ? { fontSize: 13, lineHeight: "20px" } : {}) }}>
              {spec.chip ? chipOf(false, ref) : <span ref={ref("span")}>Plain</span>}
            </div>
          </div>
        )
        break
      default:
        throw new Error(`no kind for ${name}`)
    }
    root.renderer.dispatchNativeEvents()
    expect(root.renderer.getA11yTree().frame?.scale_factor, "the window has the scale asked for").toBe(scale)
    const actual: Partial<Record<FractionPart, Rect>> = {}
    for (const [part, instance] of refs) {
      const box = instance.getBoundingClientRect()
      actual[part as FractionPart] = [box.x, box.y, box.width, box.height]
    }
    return { scale, expected: expected as Fraction, actual }
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

/** The places of a `frac-` case are Chrome's to 1/64 px, the fraction kept. What follows the baseline of a line (the top
 *  of a badge or a span, which are inside a line of text) has the half-leading of the line at scale 2 as well. */
function expectFraction(name: string, scale: 1 | 2) {
  const { actual, expected } = fractionOf(name, scale)
  const leading = halfLeadingError(scale)
  const parts = (["case", "cell", "item", "span", "holder", "label"] as const).filter((part) => expected[part])
  expect(Object.keys(actual).sort(), `${name} (scale ${scale}) reports the parts of the oracle`).toEqual([...parts].sort())
  for (const part of parts) {
    const chrome = expected[part]!
    const rect = actual[part]!
    const onBaseline = part === "span" || part === "holder" || part === "label"
    const within = (value: number, wanted: number, tolerance: number, what: string) =>
      expect(Math.abs(value - wanted), `${name} (scale ${scale}) ${part} ${what}: ${value} != Chrome's ${wanted}`).toBeLessThanOrEqual(
        tolerance
      )
    within(rect[0], chrome[0], UNIT, "x")
    within(rect[2], chrome[2], UNIT, "width")
    // The baseline of a cap-trimmed label is in the layout unit its box is, so a badge sits exactly where Chrome puts
    // it at scale 1: a 64th above it is a device pixel when it is painted on a tie (#80).
    // The exception is a badge centred in a table cell: it is placed by the height of its Menlo 13px line, which is 20.02
    // here and 20 in Chrome (the text measure, a later item), so it is a 64th off.
    const exact = scale === 1 && expected.spec.place !== "cell"
    within(rect[1], chrome[1], exact ? 1e-4 : UNIT + (onBaseline ? leading : 0), "y")
    within(rect[3], chrome[3], UNIT + (part === "label" ? 0 : leading), "height")
  }
}

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
    // A badge has the device-pixel rounding of the font's metrics at scale 2 in its heights as well: the content area of
    // an 11px or 13px text is whole device pixels in Chrome (12.5 for the 13px, the wrapper 20.49 round a badge on its
    // strut) and whole CSS pixels here, the same limit as the half-leading, and not the trim's.
    const rounded = expected.spec.badge || !(key === "line" || key === "ink")
    within(actual[key][3], expected[key][3], UNIT + (rounded ? leading : 0), `${key} height`)
  }
  if (expected.spec.badge) {
    // The wrapper is as tall as the strut and the badge make it, the badge sits on the baseline of the strut, and the
    // text is in the badge where Chrome has it: the same tolerances as the labels below.
    const place = UNIT + leading
    within(actual.holder[1], expected.holder[1], place, "holder y")
    within(actual.label[1], expected.label[1], place, "label y")
    within(actual.label[1] - actual.holder[1], expected.label[1] - expected.holder[1], UNIT + leading, "label in the badge")
    return
  }
  if (expected.spec.row) {
    // A flex or grid item is placed where layout puts it, the fraction included, so the label and the span beside it are
    // where Chrome has them but for the half-leading of the line they are seated on.
    within(actual.label[1], expected.label[1], UNIT + leading, "label y")
    within(actual.holder[1], expected.holder[1], UNIT + leading, "span y")
    return
  }
  const place = UNIT + leading
  within(actual.holder[1], expected.holder[1], place, "holder y")
  within(actual.label[1], expected.label[1], place, "label y")
  within(actual.ink[1], expected.ink[1], place, "ink y")
  // The label is seated on the baseline by its trimmed box, and the glyphs are in the label as Chrome has them.
  within(actual.ink[1] - actual.label[1], expected.ink[1] - expected.label[1], UNIT + leading, "ink below the label")
  within(actual.holder[1], actual.label[1], UNIT, "holder y against the label")
}

const NAMES = Object.keys(ORACLE.x1).filter((name) => !name.startsWith("frac-"))
const FRACTIONS = Object.keys(ORACLE.x1).filter((name) => name.startsWith("frac-"))
const SCALES = [1, 2] as const

describeNative("a text-box-trimmed label in an atomic inline", () => {
  it("has an oracle for both scales", () => {
    expect(Object.keys(ORACLE.x2)).toEqual(Object.keys(ORACLE.x1))
    expect(NAMES.length).toBeGreaterThan(40)
    expect(FRACTIONS.length).toBeGreaterThan(30)
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

    for (const name of FRACTIONS) {
      it(`keeps the fraction of its place as Chrome does at scale ${scale}: ${name}`, () => {
        expectFraction(name, scale)
      })
    }
  }
})
