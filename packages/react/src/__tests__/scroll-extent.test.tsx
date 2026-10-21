/// How far a scroller scrolls, against Chrome (#69). `scrollHeight` and `clientHeight` are the extent and the client
/// size rounded half up in CSS px, wherever the box sits, and the farthest the scroller goes is the extent (with the
/// padding after the content) less the client size, each rounded to a device pixel first: `scrollTo(0, scrollHeight -
/// clientHeight)` goes all the way at scale 1 (that difference is the maximum there) and stops up to a CSS pixel short
/// at scale 2, and an offset of 1e6 stops at the maximum. gpuix measured the end of the content from boxes snapped to
/// device pixels, so a scroller at a fractional top (700.5 of content in 300 at a top of 0.59375) had a maximum 1px
/// short of Chrome's at scale 1 and `scrollTo(0, scrollHeight - clientHeight)` moved nothing.
///
/// `fixtures/scroll-extent-oracle.html` is the page, `fixtures/scroll-extent-oracle.json` what headless Chrome reports
/// for each case at device scale 1 and 2 (regenerate with `scroll-extent-oracle.sh`): scrollers 300 and 300.5 long
/// along the scrolled axis, put at a top or left of 0, 0.25 and 0.6 by the padding of a wrapper, holding 700, 700.25,
/// 700.5 and 700.75 of content or two children of 350.3 and 350.45, with 0 or 0.5 of padding after them, vertical and
/// horizontal. Every value is compared exactly.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

interface Case {
  id: string
  axis: "x" | "y"
  size: number
  pad: number
  padEnd: number
  split: "single" | "two"
  f: number | null
  extent: number
  client: number
  st1: number
  st2: number
}

const ORACLE: Record<"x1" | "x2", Case[]> = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/scroll-extent-oracle.json"),
    "utf8"
  )
)

function measured(spec: Case, scale: 1 | 2) {
  const vertical = spec.axis === "y"
  const root = createTestRoot()
  try {
    root.renderer.setWindowScaleFactor(scale)
    let scroller: PublicInstance | undefined
    const lengths = spec.split === "single" ? [700 + spec.f!] : [350.3, 350.45]
    const child = (length: number, key: number) => (
      <div key={key} style={vertical ? { height: length } : { width: length, height: 20, flexShrink: 0 }} />
    )
    root.render(
      <div style={vertical ? { paddingTop: spec.pad } : { paddingLeft: spec.pad }}>
        <div
          ref={(instance) => {
            if (instance) scroller = instance
          }}
          style={{
            overflow: "auto",
            width: vertical ? 300 : spec.size,
            height: vertical ? spec.size : 40,
            ...(vertical ? { paddingBottom: spec.padEnd } : { paddingRight: spec.padEnd }),
          }}
        >
          {vertical ? (
            lengths.map(child)
          ) : (
            <div style={{ display: "flex", width: "max-content" }}>{lengths.map(child)}</div>
          )}
        </div>
      </div>
    )
    root.renderer.dispatchNativeEvents()
    expect(root.renderer.getA11yTree().frame?.scale_factor, "the window has the scale asked for").toBe(scale)
    const element = scroller!
    const extent = vertical ? element.scrollHeight : element.scrollWidth
    const client = vertical ? element.clientHeight : element.clientWidth
    const offset = () => (vertical ? element.scrollTop : element.scrollLeft)
    const scrollTo = (value: number) => (vertical ? element.scrollTo(0, value) : element.scrollTo(value, 0))
    scrollTo(extent - client)
    const st1 = offset()
    scrollTo(0)
    if (vertical) element.scrollTop = 1e6
    else element.scrollLeft = 1e6
    return { extent, client, st1, st2: offset() }
  } finally {
    root.unmount()
  }
}

// 60 windows a test: the default 5s is too close on a loaded machine.
const SLOW = 60_000

describeNative("how far a scroller scrolls follows Chrome", () => {
  for (const scale of [1, 2] as const) {
    for (const axis of ["y", "x"] as const) {
      it(`scrolls as Chrome does along ${axis === "y" ? "the vertical" : "the horizontal"} axis at scale ${scale}`, () => {
        const mismatches: string[] = []
        for (const spec of ORACLE[`x${scale}`].filter((entry) => entry.axis === axis)) {
          const actual = measured(spec, scale)
          const expected = { extent: spec.extent, client: spec.client, st1: spec.st1, st2: spec.st2 }
          if (JSON.stringify(actual) !== JSON.stringify(expected)) {
            mismatches.push(`${spec.id}: ${JSON.stringify(actual)}, Chrome ${JSON.stringify(expected)}`)
          }
        }
        expect(mismatches).toEqual([])
      }, SLOW)
    }
  }
})
