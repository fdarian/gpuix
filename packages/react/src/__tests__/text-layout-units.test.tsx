/// Text is laid out in fractions of a pixel, as Chrome lays it out in 1/64 of one, and snapped to device pixels when it
/// is painted: a cap-trimmed label is centred in a fixed-height box, and a chip is as wide as its text. gpui checks the
/// layout itself to 1/64 (`chrome_layout_units` in gpui's text.rs); this checks what an app reads back, which is the
/// layout and not the painted box, with the system fonts, against the same Chrome, to a unit of 1/64.
///
/// Numbers are from headless Chrome 155 on macOS (`getBoundingClientRect`), each case giving its HTML.
///   box    `<div style="height:40px;display:flex;align-items:center;font:13px/20px 'Helvetica Neue'">
///           <span id=label style="text-box:trim-both cap alphabetic">Label</span></div>`
///          -> label y 15.359375 in the box, 31.796875 x 9.28125. `font:12px/16px` in a 32px box, "Settings":
///          y 11.71875, 44.015625 x 8.5625. `font:14px/1.5 Menlo` in 40px, "Hello": y 14.890625, 42.15625 x 10.203125.
///   chip   `<span id=chip style="display:inline-flex;padding:0 6px">Label text</span>` in `font:13px/20px 'Helvetica Neue'`:
///          69.3125 x 20; `padding:0 8px` "Settings" at 13px/24px: 63.671875 x 24; Menlo 12px/16px "Hello world": 79.484375 x 16;
///          Helvetica Neue 15px/22px "Fractional widths" as a flex item: 114.4375 x 22.

import React from "react"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

function setup() {
  const root = createTestRoot({ width: 600, height: 300 })
  const refs = new Map<string, PublicInstance>()
  const ref = (name: string) => (instance: PublicInstance | null) => {
    if (instance) refs.set(name, instance)
  }
  const rect = (name: string) => refs.get(name)!.getBoundingClientRect()
  return { ...root, ref, rect }
}

/** What a box is read as: layout's own value, a multiple of 1/64, so it can differ from Chrome's by a unit of it. */
function within(actual: number, chrome: number, what: string, tolerance: number) {
  expect(Math.abs(actual - chrome), `${what}: ${actual} != Chrome's ${chrome}`).toBeLessThanOrEqual(tolerance)
}

const UNIT = 1 / 64 + 1e-6

const TRIM = { textBox: "trim-both cap alphabetic" } as const

describeNative("a cap-trimmed label in a fixed-height box", () => {
  const cases = [
    { family: "Helvetica Neue", size: 13, line: "20px", box: 40, text: "Label", y: 15.359375, w: 31.796875, h: 9.28125 },
    { family: "Helvetica Neue", size: 12, line: "16px", box: 32, text: "Settings", y: 11.71875, w: 44.015625, h: 8.5625 },
    { family: "Menlo", size: 14, line: 1.5, box: 40, text: "Hello", y: 14.890625, w: 42.15625, h: 10.203125 },
  ]
  for (const sample of cases) {
    it(`${sample.text} in ${sample.family} ${sample.size}px is as tall as the cap height and centred`, () => {
      const { render, ref, rect, unmount } = setup()
      try {
        render(
          <div
            ref={ref("box")}
            style={{ width: 600, height: sample.box, display: "flex", alignItems: "center", color: "#000000" }}
          >
            <text
              ref={ref("label")}
              style={{ fontFamily: sample.family, fontSize: sample.size, lineHeight: sample.line, ...TRIM }}
            >
              {sample.text}
            </text>
          </div>
        )
        const label = rect("label")
        const box = rect("box")
        within(label.top - box.top, sample.y, "label y", 1e-6)
        within(label.width, sample.w, "label width", UNIT)
        within(label.height, sample.h, "label height", UNIT)
        const above = label.top - box.top
        const below = box.bottom - label.bottom
        expect(Math.abs(above - below), `${above} above and ${below} below`).toBeLessThanOrEqual(1)
      } finally {
        unmount()
      }
    })
  }
})

describeNative("a chip as wide as its text", () => {
  const cases = [
    { family: "Helvetica Neue", size: 13, line: 20, pad: 6, text: "Label text", w: 69.3125, h: 20 },
    { family: "Helvetica Neue", size: 13, line: 24, pad: 8, text: "Settings", w: 63.671875, h: 24 },
    { family: "Menlo", size: 12, line: 16, pad: 0, text: "Hello world", w: 79.484375, h: 16 },
    { family: "Helvetica Neue", size: 15, line: 22, pad: 0, text: "Fractional widths", w: 114.4375, h: 22 },
  ]
  for (const sample of cases) {
    it(`${sample.text} in ${sample.family} ${sample.size}px`, () => {
      const { render, ref, rect, unmount } = setup()
      try {
        render(
          <div
            style={{
              width: 600,
              display: "flex",
              flexDirection: "column",
              alignItems: "flex-start",
              fontFamily: sample.family,
              fontSize: sample.size,
              lineHeight: `${sample.line}px`,
              color: "#000000",
            }}
          >
            <div ref={ref("chip")} style={{ display: "flex", padding: `0 ${sample.pad}px` }}>
              <text>{sample.text}</text>
            </div>
          </div>
        )
        const chip = rect("chip")
        within(chip.width, sample.w, "chip width", UNIT)
        within(chip.height, sample.h, "chip height", 0.01)
      } finally {
        unmount()
      }
    })
  }
})
