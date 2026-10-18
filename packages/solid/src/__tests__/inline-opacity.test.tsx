/// The Solid half of the React test of the same name: an inline box with an `opacity` below one is composited as a
/// group, against the colours headless Chrome paints (`packages/react/src/__tests__/fixtures/inline-opacity-oracle.json`).

import fs from "fs"
import os from "os"
import path from "path"
import { fileURLToPath } from "url"
import { describe, expect, it } from "bun:test"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels } from "./pixels.js"

type Rgb = [number, number, number]

const ORACLE: Record<string, { colors: Record<string, Rgb> }> = JSON.parse(
  fs.readFileSync(
    path.resolve(
      path.dirname(fileURLToPath(import.meta.url)),
      "../../../react/src/__tests__/fixtures/inline-opacity-oracle.json"
    ),
    "utf8"
  )
).cases

const TEXT = { color: "#000000", fontFamily: "Helvetica", fontSize: 48, fontWeight: 700, lineHeight: "60px", width: 300 } as const
const PADDING = { paddingLeft: 12 }
const shots = fs.mkdtempSync(path.join(os.tmpdir(), "gpuix-solid-inline-opacity-"))

const CASES: Record<string, () => unknown> = {
  "text over its own background": () => (
    <span testId="a" style={{ opacity: 0.5, backgroundColor: "#f00", color: "#00f", ...PADDING }}>
      MM
    </span>
  ),
  "nested groups": () => (
    <span testId="outer" style={{ opacity: 0.5, backgroundColor: "#f00", ...PADDING }}>
      <span testId="inner" style={{ opacity: 0.5, backgroundColor: "#0f0", color: "#00f", ...PADDING }}>
        MM
      </span>
    </span>
  ),
}

describe.skipIf(!hasNativeTestRenderer)("Solid opacity of an inline box against Chrome", () => {
  for (const [name, body] of Object.entries(CASES)) {
    it(name, () => {
      const app = createTestRoot({ width: 300, height: 80 })
      try {
        app.render(() => (
          <div style={{ width: 300, height: 80, backgroundColor: "#ffffff" }}>
            <text testId="p" style={TEXT as never}>
              {body() as never}
            </text>
          </div>
        ))
        const renderer = app.renderer
        const file = path.join(shots, `${name.replaceAll(" ", "-")}.png`)
        renderer.captureScreenshot(file)
        const pixels = readLogicalPixels(file, renderer)
        const paragraph = renderer.getElementBounds(renderer.findByTestId("p")!.id)!
        for (const [point, expected] of Object.entries(ORACLE[name]!.colors)) {
          const [span, kind] = point.split(" ") as [string, "padding" | "stem"]
          const box = renderer.getElementBounds(renderer.findByTestId(span)!.id)!
          const x = Math.round(kind === "padding" ? box.x + 4 : box.x + PADDING.paddingLeft + 7)
          const color = pixels.at(x, Math.round(paragraph.y + 30)).slice(0, 3)
          color.forEach((channel, index) => {
            expect(Math.abs(channel - expected[index]!), `${point}: ${color} against ${expected}`).toBeLessThanOrEqual(3)
          })
        }
      } finally {
        app.unmount()
      }
    })
  }
})
