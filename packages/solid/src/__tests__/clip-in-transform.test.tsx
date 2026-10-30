/// The Solid half of the React test of the same name (#110): the card of FF's repro, a Motion `y` on a box that clips
/// (`overflow: hidden`, `overflowX`, `clip-path`, a clipping child), draws its text at a `y` of -80 and -104, as at 0.

import fs from "fs"
import os from "os"
import path from "path"
import { describe, expect, it } from "bun:test"
import { motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels } from "./pixels.js"

const shots = fs.mkdtempSync(path.join(os.tmpdir(), "gpuix-solid-clip-in-transform-"))
const TRIGGERS = ["overflow", "overflowX", "clipPath", "child"] as const
const TEXT_STYLE = { fontFamily: "Helvetica", fontSize: 14, color: "#ff0000", whiteSpace: "nowrap" } as const

function inkAt(trigger: (typeof TRIGGERS)[number], y: number) {
  const app = createTestRoot({ width: 420, height: 260 })
  try {
    const style: Record<string, unknown> = {
      position: "absolute",
      left: 135,
      top: 108,
      width: 150,
      height: 44,
      backgroundColor: "#333333",
      borderRadius: 8,
      display: "flex",
      alignItems: "center",
      paddingLeft: 14,
    }
    if (trigger === "overflow") style.overflow = "hidden"
    if (trigger === "overflowX") style.overflowX = "hidden"
    if (trigger === "clipPath") style.clipPath = "inset(0 round 8px)"
    app.render(() => (
      <div style={{ position: "relative", width: 420, height: 260, backgroundColor: "#ffffff" }}>
        <motion.div initial={false} animate={{ y }} transition={{ duration: 0 }} style={style as never}>
          {trigger === "child" ? (
            <div style={{ overflow: "hidden", flex: 1, minWidth: 0, height: "100%", display: "flex", alignItems: "center" }}>
              <text style={TEXT_STYLE}>hello world hello world hello</text>
            </div>
          ) : (
            <text style={TEXT_STYLE}>hello world hello world hello</text>
          )}
        </motion.div>
      </div>
    ))
    app.renderer.flush()
    const file = path.join(shots, `${trigger}-${-y}.png`)
    app.renderer.captureScreenshot(file)
    const pixels = readLogicalPixels(file, app.renderer)
    let count = 0
    for (let row = 0; row < 260; row++) {
      for (let column = 0; column < 420; column++) {
        const rgb = pixels.at(column, row)
        if (rgb[0]! - Math.max(rgb[1]!, rgb[2]!) > 60) count++
      }
    }
    return count
  } finally {
    app.unmount()
  }
}

describe.skipIf(!hasNativeTestRenderer)("Solid text of a clipped box moved by Motion's y", () => {
  for (const trigger of TRIGGERS) {
    it(`keeps the text of a ${trigger} clip at a y of -52, -80 and -104`, () => {
      const unmoved = inkAt(trigger, 0)
      expect(unmoved).toBeGreaterThan(100)
      for (const y of [-52, -80, -104, 104]) {
        expect(Math.abs(inkAt(trigger, y) / unmoved - 1), `${trigger} at y ${y}`).toBeLessThan(0.08)
      }
    })
  }
})
