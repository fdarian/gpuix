/// The strut of an inline box that holds nothing but atomic inlines: a `<span>` with its own
/// `line-height` or font size makes the line as tall as its strut says, the ascent and descent of
/// its font with half-leading, whether it holds text or only a box, and on every line its box is on.
///
/// `fixtures/inline-strut-oracle.html` is the page and `fixtures/inline-strut-oracle.json` the
/// boxes headless Chrome reports for it (`getBoundingClientRect` relative to the box of the case,
/// `[x, y, width, height]`; regenerate with `fixtures/dsf-oracle.sh inline-strut`), and
/// `fixtures/inline-strut-oracle-x2.json` the same on a display of device scale 2, where the half-leading of a line
/// that is not whole is floored to a device pixel and not a CSS pixel (the `odd-` cases). The text is GpuixTestSquare
/// at 20px with a line height of 20px, so every character is 20px wide, the ascent is the whole em
/// and there is no descent, and only "A" is in the font.

import fs from "fs"
import path from "path"
import type { ReactElement } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SQUARE_FONT = fs.readFileSync(new URL("./fixtures/gpuix-test-square.ttf", import.meta.url))

const TEXT = { fontFamily: "GpuixTestSquare", fontSize: 20, lineHeight: "20px", color: "#000000" } as const

type Rect = [number, number, number, number]

const SCALES = [1, 2] as const
type Scale = (typeof SCALES)[number]

const oracleOf = (file: string): Record<string, Rect> =>
  JSON.parse(fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures", file), "utf8"))

const ORACLE: Record<Scale, Record<string, Rect>> = {
  1: oracleOf("inline-strut-oracle.json"),
  2: oracleOf("inline-strut-oracle-x2.json"),
}

function chrome(scale: Scale, id: string): Rect {
  const box = ORACLE[scale][id]
  expect(box, `no oracle box ${id}`).toBeDefined()
  return box ?? [0, 0, 0, 0]
}

/** The names the elements of a render are found by; boxes are relative to the one called `root`. */
function setup(scale: Scale) {
  const root = createTestRoot()
  root.renderer.setWindowScaleFactor(scale)
  root.renderer.registerFont(SQUARE_FONT)
  const refs = new Map<string, PublicInstance>()
  const ref = (name: string) => (instance: PublicInstance | null) => {
    if (instance) refs.set(name, instance)
  }
  const rect = (name: string): Rect => {
    root.renderer.dispatchNativeEvents()
    const element = refs.get(name)
    expect(element, `no element ${name}`).toBeDefined()
    const box = element?.getBoundingClientRect() ?? { x: 0, y: 0, width: 0, height: 0 }
    const origin = refs.get("root")?.getBoundingClientRect() ?? { x: 0, y: 0 }
    return [box.x - origin.x, box.y - origin.y, box.width, box.height]
  }
  /** Each `name` laid out as the oracle's `${scope}.${name}`; `root` is the oracle's `scope`. */
  const expectChrome = (scope: string, ...names: string[]) => {
    for (const name of names) {
      const expected = chrome(scale, name === "root" ? scope : `${scope}.${name}`)
      const actual = rect(name)
      for (const [ix, key] of ["x", "y", "w", "h"].entries()) {
        expect(
          Math.abs((actual[ix] ?? 0) - (expected[ix] ?? 0)),
          `${scope}.${name} (scale ${scale}): ${key} ${actual[ix]} != ${expected[ix]}`
        ).toBeLessThanOrEqual(0.51 / scale)
      }
    }
  }
  return { ...root, ref, rect, expectChrome }
}

type Names = ReturnType<typeof setup>["ref"]

const box = (ref: Names, name: string, style: Record<string, number | string> = {}): ReactElement => (
  <span ref={ref(name)} style={{ display: "inline-block", width: 30, height: 10, ...style }} />
)

/** A line of the case `scope`, at both scales: `inner` is what the container holds. */
function check(
  scope: string,
  inner: (ref: Names) => ReactElement,
  names: string[],
  width = 300,
  style: Record<string, number | string> = {}
) {
  for (const scale of SCALES) {
    const { render, ref, expectChrome, unmount } = setup(scale)
    try {
      render(
        <div ref={ref("root")} style={{ width, ...TEXT, ...style }}>
          {inner(ref)}
        </div>
      )
      expectChrome(scope, "root", ...names)
    } finally {
      unmount()
    }
  }
}

const spanOf = (ref: Names, name: string, style: Record<string, number | string>, children: ReactElement) => (
  <span ref={ref(name)} style={style}>
    {children}
  </span>
)

describeNative("a span that holds only a box", () => {
  it("makes the line as tall as the strut of its line-height", () => {
    check("tall", (ref) => spanOf(ref, "span", { lineHeight: "40px" }, box(ref, "box")), ["box"])
  })

  it("makes the line as tall as the strut of its font size", () => {
    check("big", (ref) => spanOf(ref, "span", { fontSize: 30 }, box(ref, "box")), ["box"])
  })

  it("leaves the line the strut of the container when its own is smaller", () => {
    check("small", (ref) => spanOf(ref, "span", { lineHeight: "10px" }, box(ref, "box")), ["box"])
  })

  it("is the same with text around the span", () => {
    check(
      "text",
      (ref) => (
        <>
          <span ref={ref("t1")}>A</span>
          {spanOf(ref, "span", { lineHeight: "40px" }, box(ref, "box"))}
          <span ref={ref("t2")}>A</span>
        </>
      ),
      ["t1", "box", "t2"]
    )
  })

  it("counts the strut of every span around the box", () => {
    check(
      "nested",
      (ref) =>
        spanOf(ref, "outer", { lineHeight: "40px" }, spanOf(ref, "inner", { lineHeight: "10px" }, box(ref, "box"))),
      ["box"]
    )
  })

  it("moves its strut with its vertical-align", () => {
    check("raised", (ref) => spanOf(ref, "span", { lineHeight: "40px", verticalAlign: "5px" }, box(ref, "box")), ["box"])
    check("lowered", (ref) => spanOf(ref, "span", { lineHeight: "40px", verticalAlign: "-15px" }, box(ref, "box")), ["box"])
  })

  it("is aligned to the top or the bottom of the line with its strut", () => {
    check("top", (ref) => spanOf(ref, "span", { lineHeight: "40px", verticalAlign: "top" }, box(ref, "box")), ["box"])
    check("bottom", (ref) => spanOf(ref, "span", { lineHeight: "40px", verticalAlign: "bottom" }, box(ref, "box")), ["box"])
  })

  it("keeps its strut on the baseline when the box is aligned to the top by itself", () => {
    check("box-top", (ref) => spanOf(ref, "span", { lineHeight: "40px" }, box(ref, "box", { verticalAlign: "top" })), ["box"])
  })

  it("has its strut on the line the box wraps to", () => {
    check(
      "wrap",
      (ref) => (
        <>
          <span ref={ref("t1")}>AAA</span>
          <span ref={ref("span")} style={{ lineHeight: "40px" }}>
            <span ref={ref("t2")}>A</span>
            {box(ref, "box")}
          </span>
        </>
      ),
      ["t1", "box"],
      100
    )
    check(
      "wrap-only",
      (ref) => (
        <>
          <span ref={ref("t1")}>AAAA</span>
          {spanOf(ref, "span", { lineHeight: "40px" }, box(ref, "box"))}
        </>
      ),
      ["box"],
      100
    )
  })

  it("floors a half-leading that is not whole to a device pixel", () => {
    check("odd-tall", (ref) => spanOf(ref, "span", { lineHeight: "45px" }, box(ref, "box")), ["box"])
    check(
      "odd-text",
      (ref) => (
        <>
          <span ref={ref("t1")}>A</span>
          {spanOf(ref, "span", { lineHeight: "45px" }, box(ref, "box"))}
          <span ref={ref("t2")}>A</span>
        </>
      ),
      ["t1", "box", "t2"]
    )
    check(
      "odd-container",
      (ref) => (
        <>
          <span ref={ref("t1")}>A</span>
          {box(ref, "box")}
        </>
      ),
      ["t1", "box"],
      300,
      { lineHeight: "25px" }
    )
    check("odd-raised", (ref) => spanOf(ref, "span", { lineHeight: "45px", verticalAlign: "5px" }, box(ref, "box")), ["box"])
    check("odd-top", (ref) => spanOf(ref, "span", { lineHeight: "45px", verticalAlign: "top" }, box(ref, "box")), ["box"])
    check("odd-size", (ref) => spanOf(ref, "span", { fontSize: 24, lineHeight: "45px" }, box(ref, "box")), ["box"])
  })
})
