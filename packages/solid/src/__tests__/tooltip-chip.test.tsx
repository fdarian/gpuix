/// The Solid half of the React test of the same name: the tooltip chip of Fluid Functionalism, in the deferred `<anchored>`
/// that carries it, is as wide as its content against headless Chrome
/// (`packages/react/src/__tests__/fixtures/tooltip-chip-oracle.json` and `-x2.json`).

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import { describe, expect, it } from "bun:test"
import type { JSX } from "solid-js"
import { motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rect = [number, number, number, number]
type Oracle = Record<string, { boxes: Record<string, Rect> }>

const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../react/src/__tests__/fixtures")
const FONT = path.join(FIXTURES, "inter-text-subset.ttf")
const oracleOf = (file: string): Oracle => JSON.parse(fs.readFileSync(path.join(FIXTURES, file), "utf8"))

const CAPTION = { fontSize: 12, lineHeight: "16px" } as const
const CHIP = {
  position: "relative",
  backgroundColor: "#171717",
  borderRadius: 8,
  padding: 8,
  textBox: "trim-both cap alphabetic",
} as const

const CHIPS: Record<string, () => JSX.Element> = {
  "rich chip": () => (
    <div testId="body" style={{ display: "flex", flexDirection: "column", gap: 4 }}>
      <text testId="t1" style={{ ...CAPTION, color: "#fafafa", fontVariationSettings: "'wght' 550, 'opsz' 18" }}>
        Keyboard shortcut
      </text>
      <text testId="t2" style={{ ...CAPTION, color: "#a3a3a3" }}>
        Ctrl + S
      </text>
    </div>
  ),
  "plain chip nowrap": () => (
    <text style={{ ...CAPTION, color: "#fafafa", whiteSpace: "nowrap" }}>Save your changes</text>
  ),
}

function tooltip(chip: () => JSX.Element) {
  return (
    <div style={{ position: "relative", width: 700, fontFamily: "Inter Variable", ...CAPTION }}>
      <div testId="card" style={{ display: "flex", flexDirection: "column", padding: 20 }}>
        <div testId="row" style={{ display: "flex", flexDirection: "row", alignItems: "center", gap: 16, minHeight: 36 }}>
          <div style={{ width: 96, flexShrink: 0 }}>
            <text style={CAPTION}>Rich content</text>
          </div>
          <div testId="group" style={{ display: "flex", flexDirection: "row", alignItems: "center", gap: 10 }}>
            <div testId="wrapper" style={{ position: "relative", display: "flex", flexShrink: 0 }}>
              <div
                testId="trigger"
                style={{ height: 36, paddingLeft: 16, paddingRight: 16, display: "flex", alignItems: "center" }}
              >
                <text style={CAPTION}>Save</text>
              </div>
              <anchored side="bottom" align="start" gap={8} fit="flip" deferred priority={2} style={{ backgroundColor: "transparent" }}>
                <motion.div testId="chip" style={CHIP}>
                  {chip()}
                </motion.div>
              </anchored>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}

const ORACLES = [
  { scale: 1, cases: oracleOf("tooltip-chip-oracle.json") },
  { scale: 2, cases: oracleOf("tooltip-chip-oracle-x2.json") },
]

describeNative("the tooltip chip in its anchored layer against Chrome", () => {
  for (const oracle of ORACLES) {
    for (const [name, source] of Object.entries(oracle.cases)) {
      it(`${name} at scale ${oracle.scale}`, () => {
        const app = createTestRoot({ width: 1280, height: 800, scaleFactor: oracle.scale, fonts: [FONT] })
        try {
          app.render(() => tooltip(CHIPS[name]!))
          app.renderer.flush()
          app.renderer.dispatchNativeEvents()
          for (const [id, want] of Object.entries(source.boxes)) {
            if (id === "anchor") continue
            const element = app.renderer.findByTestId(id)
            expect(element, `${id} is mounted`).toBeDefined()
            const bounds = app.renderer.getElementBounds(element!.id)!
            const got = [bounds.x, bounds.y, bounds.width, bounds.height]
            want.forEach((value, index) => {
              expect(Math.abs(got[index]! - value), `${id} ${["x", "y", "width", "height"][index]}: got [${got}] want [${want}]`).toBeLessThanOrEqual(
                0.51 / oracle.scale
              )
            })
          }
        } finally {
          app.unmount()
        }
      })
    }
  }
})
