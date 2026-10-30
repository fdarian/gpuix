/// The tooltip chip of Fluid Functionalism, in the deferred `<anchored>` that carries it, is as wide as its content against
/// headless Chrome, at device scale 1 and 2. The layer is a portal into `body`, so Chrome's box is `position: fixed`: a flex
/// column `min(max-content, max(min-content, available))` wide with the viewport as its available width, and "Keyboard
/// shortcut" is one line and not wrapped at the space into its longest word. The trigger's wrapper, which the layer is a
/// layout child of, is 60px wide and shrink-wrapped by its row; the layer was sized in that box from Round 26 (#105).
/// `fixtures/tooltip-chip-oracle.html` is the page and `tooltip-chip-oracle.json` (scale 1) and `-x2.json` (scale 2) the
/// boxes it lays out (`sh fixtures/dsf-oracle.sh tooltip-chip`).

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, it } from "vitest"
import { motion } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { expectBoxes } from "./oracle-boxes.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Oracle = Record<string, { html: string; boxes: Record<string, [number, number, number, number]> }>

const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")
const FONT = path.join(FIXTURES, "inter-text-subset.ttf")
const oracleOf = (file: string): Oracle => JSON.parse(fs.readFileSync(path.join(FIXTURES, file), "utf8"))

const CAPTION = { fontSize: 12, lineHeight: "16px" }
const CHIP = {
  position: "relative",
  backgroundColor: "#171717",
  borderRadius: 8,
  padding: 8,
  textBox: "trim-both cap alphabetic",
}

const CHIPS: Record<string, React.ReactNode> = {
  "rich chip": (
    <div testId="body" style={{ display: "flex", flexDirection: "column", gap: 4 }}>
      <text testId="t1" style={{ ...CAPTION, color: "#fafafa", fontVariationSettings: "'wght' 550, 'opsz' 18" }}>
        Keyboard shortcut
      </text>
      <text testId="t2" style={{ ...CAPTION, color: "#a3a3a3" }}>
        Ctrl + S
      </text>
    </div>
  ),
  "plain chip nowrap": (
    <text style={{ ...CAPTION, color: "#fafafa", whiteSpace: "nowrap" }}>
      Save your changes
    </text>
  ),
}

function tooltip(chip: React.ReactNode) {
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
                {chip}
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
        const { render, renderer, unmount } = createTestRoot({
          width: 1280,
          height: 800,
          scaleFactor: oracle.scale,
          fonts: [FONT],
        })
        try {
          render(tooltip(CHIPS[name]))
          renderer.flush()
          renderer.dispatchNativeEvents()
          const boxes = Object.fromEntries(Object.entries(source.boxes).filter(([id]) => id !== "anchor"))
          expectBoxes(renderer, boxes, 0.51 / oracle.scale)
        } finally {
          unmount()
        }
      })
    }
  }
})
