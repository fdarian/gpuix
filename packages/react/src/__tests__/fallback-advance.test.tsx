/// A text set in a font that is not the one asked for (CJK, emoji) is as wide as in Chrome, cluster by cluster. CoreText
/// picks the same fallback font as Chrome but advances it differently: Chrome takes the tracked advances of a font with a
/// `trak` table and no `sbix` (PingFang) at the CSS size, and Skia's advances with the tracking off at the device size for
/// the rest (Apple Color Emoji, Hiragino, Apple SD Gothic Neo).
///
/// `fixtures/fallback-advance-oracle.html` is the page and `fallback-advance-oracle.json` / `-x2.json` the width of the Range
/// from the start of each text to the end of each of its grapheme clusters, at device scale 1 and 2
/// (`sh fixtures/fallback-advance-oracle.sh`). Chrome ceils a position to 1/64 of a device pixel, so the distances are
/// compared within that. (The Range around one cluster is a floor and a ceil, up to 1/32 off its advance.)

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")

const TEXTS = [
  "中文测试",
  "世界！你好。",
  "한국어",
  "こんにちは",
  "😀😁😂 👨‍👩‍👧 🇺🇸🇯🇵 ❤️ 👍🏽",
  "Mixed 中文 and 😀 한국어 end",
  "😀😁😂👨‍👩‍👧🇺🇸🇯🇵❤️👍🏽",
  "Hello, World! Typography 123",
]

const SCALES = [
  { scaleFactor: 1, file: "fallback-advance-oracle.json" },
  { scaleFactor: 2, file: "fallback-advance-oracle-x2.json" },
] as const

/** Chrome ceils a position to 1/64 of a device pixel, which is 1/128 of a pixel at scale 2. */
const tolerance = (scaleFactor: number) => 1 / (64 * scaleFactor) + 1e-4

const segmenter = new Intl.Segmenter("en", { granularity: "grapheme" })

/** The families whose text falls back to other fonts, and the families asked for by name with the texts they have glyphs for. */
const FALLBACK_FAMILIES: Record<string, number[]> = {
  "Helvetica Neue": [0, 1, 2, 3, 4, 5],
  Menlo: [0, 1, 2, 3, 4, 5],
}
/** Fonts that need no change: their text is in the font that was asked for and CoreText advances it as Chrome does. */
const CONTROL_FAMILIES: Record<string, number[]> = {
  Menlo: [7],
  "system-ui": [7],
}

/** CoreText's `system-ui` is up to 0.05 px wide of Chrome's over a line of 28 letters from 18px up, which is not the fallback. */
const SYSTEM_UI_MAX_SIZE = 16

/** What a style asks for to get the font Chrome's `system-ui` is. */
const GPUIX_FAMILY: Record<string, string> = { "system-ui": ".SystemUIFont" }
const NAMED_FAMILIES: Record<string, number[]> = {
  "PingFang SC": [0, 1],
  "Hiragino Sans": [3],
  "Apple SD Gothic Neo": [2],
  "Apple Color Emoji": [6],
}

function mismatchesOf(families: Record<string, number[]>, scaleFactor: number, file: string) {
  const oracle: Record<string, number[]> = JSON.parse(fs.readFileSync(path.join(FIXTURES, file), "utf8"))
  const root = createTestRoot({ width: 800, height: 120, scaleFactor })
  const mismatches: string[] = []
  try {
    for (const [id, widths] of Object.entries(oracle)) {
      const [family, size, textIx] = id.split("|") as [string, string, string]
      if (!families[family]?.includes(Number(textIx))) continue
      if (family === "system-ui" && parseFloat(size) > SYSTEM_UI_MAX_SIZE) continue
      const text = TEXTS[Number(textIx)]!
      const clusters = [...segmenter.segment(text)].map((g) => [g.index, g.index + g.segment.length] as [number, number])
      root.render(
        <div style={{ display: "flex", alignItems: "flex-start" }} highlight={{ ranges: clusters }}>
          <text style={{ fontFamily: GPUIX_FAMILY[family] ?? family, fontSize: parseFloat(size), whiteSpace: "pre", color: "#000000" }}>{text}</text>
        </div>
      )
      const matches = root.renderer.getPaintedHighlights().sort((a, b) => a.start - b.start)
      if (matches.length !== widths.length) {
        mismatches.push(`${id}: ${matches.length} clusters, Chrome ${widths.length}`)
        continue
      }
      const origin = matches[0]!.rects[0]!.x
      matches.forEach((match, i) => {
        const rect = match.rects[match.rects.length - 1]!
        const distance = rect.x + rect.width - origin
        if (Math.abs(distance - widths[i]!) > tolerance(scaleFactor)) {
          mismatches.push(`${id} ${text.slice(0, clusters[i]![1])}: ${distance.toFixed(4)}, Chrome ${widths[i]}`)
        }
      })
    }
  } finally {
    root.unmount()
  }
  return mismatches
}

describeNative("a text in a fallback font is as wide as Chrome sets it", () => {
  for (const scale of SCALES) {
    it(`leaves the text of the fonts CoreText advances as Chrome does at scale ${scale.scaleFactor}`, () => {
      expect(mismatchesOf(CONTROL_FAMILIES, scale.scaleFactor, scale.file)).toEqual([])
    })

    it(`cluster by cluster in the fallback fonts of Helvetica Neue and Menlo at scale ${scale.scaleFactor}`, { timeout: 300_000 }, () => {
      expect(mismatchesOf(FALLBACK_FAMILIES, scale.scaleFactor, scale.file)).toEqual([])
    })

    it(`cluster by cluster in the fonts asked for by name at scale ${scale.scaleFactor}`, { timeout: 300_000 }, () => {
      expect(mismatchesOf(NAMED_FAMILIES, scale.scaleFactor, scale.file)).toEqual([])
    })
  }
})
