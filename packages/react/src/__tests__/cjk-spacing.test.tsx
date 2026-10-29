/// CJK text is as wide as Chrome sets it, cluster by cluster, where it has punctuation: Chrome sets the punctuation of
/// a script run in the font of that run (a full stop after kana is Hiragino's, after Han PingFang's, where CoreText
/// cascades to the font of the characters around it), kerns kana with the GPOS table of Hiragino, and trims the empty
/// half of a bracket or a full stop that meets another (`text-spacing-trim: normal`, the default): `。「` is 6.5 px
/// narrower at 13 px, and the opening bracket is drawn in the half of its box that was left, the caret and the hit
/// test staying at the start of the box.
///
/// `fixtures/cjk-spacing-oracle.html` is the page and `cjk-spacing-oracle.json` / `-x2.json` the width of the Range
/// from the start of each text to the end of each of its grapheme clusters, and the offsets
/// `caretPositionFromPoint` gives for points a quarter and three quarters in the box of the trimmed opening brackets, at
/// device scale 1 and 2
/// (`sh fixtures/cjk-spacing-oracle.sh`). Chrome ceils a position to 1/64 of a device pixel, so the distances are
/// compared within that.

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")

const TEXTS = [
  "「こんにちは」、と彼は言った。",
  "（注）これはテストです。",
  "『吾輩は猫である。』夏目漱石",
  "これは、テストです。「いいえ」と答えた。",
  "東京都（とうきょうと）は、日本の首都です。",
  "価格：1,000円（税込）",
  "ＡＢＣ１２３、ｘｙｚ。",
  "中文、English，混合。「引用」（括号）！",
  "他说：“你好！”然后离开了。",
  "‘single’ and “double” 日本語",
  "한국어 문장입니다. 「인용」 “따옴표”",
  "ーカタカナ・ミドル・ドット",
  "ＡＢ、「ＣＤ」",
  "ｱｲｳｴｵ｡｢ﾃｽﾄ｣",
  "。。。「「（（",
  "漢字、。かな「」",
  "「始まり」",
  "。終わり",
  "日本語、中国語（簡体字）、韓国語。",
  "Hello、世界。「テスト」です。OK。",
  "テスト）（テスト",
  "これは「テスト」（test）です。",
  "春は、あけぼの。やうやう白くなりゆく山ぎは、少しあかりて、紫だちたる雲の細くたなびきたる。",
  "你好，世界。这是一个测试：「引用」；结束。",
  "你好，世界！（测试）——结束……",
  "日本語のテキスト、これはテスト。「かぎ」、（丸）。",
  "东京（Tokyo）、大阪（Osaka）。",
  "あいう」えお",
  "あいう。」えお",
  "あいうえお。「かきくけこ",
  "「あ」「い」「う」「え」",
  "テストのテキストです",
  "キスキスキスキス",
  "日本語のテキストです。これはテスト。",
]

const SCALES = [
  { scaleFactor: 1, file: "cjk-spacing-oracle.json" },
  { scaleFactor: 2, file: "cjk-spacing-oracle-x2.json" },
] as const

/** Chrome ceils a position to 1/64 of a device pixel, which is 1/128 of a pixel at scale 2. */
const tolerance = (scaleFactor: number) => 1 / (64 * scaleFactor) + 1e-4

type Case = { widths: number[]; hits: [number, number][] }
type Wrap = { starts: number[]; firstWidths: number[] }

const segmenter = new Intl.Segmenter("en", { granularity: "grapheme" })

function mismatchesOf(scaleFactor: number, file: string, only?: (id: string) => boolean) {
  const oracle: Record<string, Case> = JSON.parse(fs.readFileSync(path.join(FIXTURES, file), "utf8"))
  const root = createTestRoot({ width: 800, height: 120, scaleFactor })
  const mismatches: string[] = []
  try {
    for (const [id, expected] of Object.entries(oracle)) {
      if (id.startsWith("wrap|")) continue
      if (only && !only(id)) continue
      const [family, size, textIx] = id.split("|") as [string, string, string]
      const text = TEXTS[Number(textIx)]!
      const clusters = [...segmenter.segment(text)].map((g) => [g.index, g.index + g.segment.length] as [number, number])
      root.render(
        <div style={{ display: "flex", alignItems: "flex-start" }} highlight={{ ranges: clusters }}>
          <text style={{ fontFamily: family, fontSize: parseFloat(size), whiteSpace: "pre", color: "#000000" }}>{text}</text>
        </div>
      )
      const matches = root.renderer.getPaintedHighlights().sort((a, b) => a.start - b.start)
      if (matches.length !== expected.widths.length) {
        mismatches.push(`${id}: ${matches.length} clusters, Chrome ${expected.widths.length}`)
        continue
      }
      const origin = matches[0]!.rects[0]!.x
      matches.forEach((match, i) => {
        const rect = match.rects[match.rects.length - 1]!
        const distance = rect.x + rect.width - origin
        const prefix = `${id} ${text.slice(0, clusters[i]![1])}`
        if (Math.abs(distance - expected.widths[i]!) > tolerance(scaleFactor) && !LEGACY_KERN_SPLIT.has(prefix)) {
          mismatches.push(`${prefix}: ${distance.toFixed(4)}, Chrome ${expected.widths[i]}`)
        }
      })
    }
  } finally {
    root.unmount()
  }
  return mismatches
}

/**
 * Prefixes that end before the second letter of a Latin pair that Helvetica Neue kerns, with the kern split between the
 * two glyphs as HarfBuzz does for a font with a `kern` table: Chrome's Range ends at the advance of the first glyph, with
 * half of the kern, and gpuix's at the position of the second, which CoreText moved by all of it (line-fit-oracle's "ART").
 */
const LEGACY_KERN_SPLIT = new Set([
  "Helvetica Neue|12|26 东京（T",
  "Helvetica Neue|14|26 东京（T",
  "Helvetica Neue|17|26 东京（T",
])

/** The offset the pointer at `x` is at, by selecting from there to the end of the text. */
function hitOffsetsOf(scaleFactor: number, file: string) {
  const oracle: Record<string, Case> = JSON.parse(fs.readFileSync(path.join(FIXTURES, file), "utf8"))
  const root = createTestRoot({ width: 800, height: 120, scaleFactor })
  const mismatches: string[] = []
  try {
    for (const [id, expected] of Object.entries(oracle)) {
      if (id.startsWith("wrap|") || expected.hits.length === 0) continue
      const [family, size, textIx] = id.split("|") as [string, string, string]
      const text = TEXTS[Number(textIx)]!
      root.render(
        <div style={{ display: "flex", alignItems: "flex-start" }}>
          <text style={{ fontFamily: family, fontSize: parseFloat(size), whiteSpace: "pre", color: "#000000" }}>{text}</text>
        </div>
      )
      // Chrome snaps the caret to the nearer edge of the box, gpuix selects from the start of the glyph the pointer is in:
      // the points a quarter in and three quarters in are in the box of the bracket, so at its start, and the one past
      // the box is in the glyph after it, at the end of the bracket.
      for (const [hit, [x, offset]] of expected.hits.entries()) {
        const start = expected.hits[hit - (hit % 3)]![1]
        const end = expected.hits[hit - (hit % 3) + 2]![1]
        const wanted = hit % 3 === 2 ? end : start
        const selected = root.renderer.dragSelect(x, parseFloat(size) / 2, 790, parseFloat(size) / 2)
        const found = selected === null ? null : text.length - selected.length
        if (found !== wanted) mismatches.push(`${id} x ${x.toFixed(2)}: offset ${found}, Chrome ${offset}, the box is ${start} to ${end}`)
      }
    }
  } finally {
    root.unmount()
  }
  return mismatches
}

/** Where the text breaks in boxes of 50 to 300 px, and the width of the first cluster of each row. */
function wrapMismatchesOf(scaleFactor: number, file: string) {
  const oracle: Record<string, Wrap> = JSON.parse(fs.readFileSync(path.join(FIXTURES, file), "utf8"))
  const root = createTestRoot({ width: 800, height: 400, scaleFactor })
  const mismatches: string[] = []
  try {
    for (const [id, expected] of Object.entries(oracle)) {
      if (!id.startsWith("wrap|")) continue
      const [, family, textIx, boxWidth] = id.split("|") as [string, string, string, string]
      const text = TEXTS[Number(textIx)]!
      const clusters = [...segmenter.segment(text)].map((g) => [g.index, g.index + g.segment.length] as [number, number])
      root.render(
        <div style={{ width: Number(boxWidth) }} highlight={{ ranges: clusters }}>
          <text style={{ fontFamily: family, fontSize: 14, color: "#000000" }}>{text}</text>
        </div>
      )
      const matches = root.renderer.getPaintedHighlights().sort((a, b) => a.start - b.start)
      const starts: number[] = []
      const firstWidths: number[] = []
      let top: number | null = null
      for (const match of matches) {
        const rect = match.rects[0]!
        if (top === null || rect.y > top + 1) {
          starts.push(match.start)
          firstWidths.push(rect.width)
        }
        top = rect.y
      }
      if (JSON.stringify(starts) !== JSON.stringify(expected.starts)) {
        mismatches.push(`${id}: rows start at ${starts}, Chrome ${expected.starts}`)
        continue
      }
      firstWidths.forEach((width, i) => {
        if (Math.abs(width - expected.firstWidths[i]!) > tolerance(scaleFactor)) {
          mismatches.push(`${id}: the row at ${starts[i]} starts with ${width.toFixed(3)}, Chrome ${expected.firstWidths[i]}`)
        }
      })
    }
  } finally {
    root.unmount()
  }
  return mismatches
}

describeNative("CJK punctuation is set and trimmed as Chrome sets it", () => {
  for (const scale of SCALES) {
    it(`cluster by cluster in 27 texts, Helvetica Neue and Menlo, at scale ${scale.scaleFactor}`, { timeout: 300_000 }, () => {
      expect(mismatchesOf(scale.scaleFactor, scale.file)).toEqual([])
    })

    it(`puts the caret and the hit test of a trimmed opening bracket at the start of its box at scale ${scale.scaleFactor}`, { timeout: 300_000 }, () => {
      expect(hitOffsetsOf(scale.scaleFactor, scale.file)).toEqual([])
    })

    it(`breaks the lines where Chrome does, a bracket whole at the start of a row, at scale ${scale.scaleFactor}`, { timeout: 300_000 }, () => {
      expect(wrapMismatchesOf(scale.scaleFactor, scale.file)).toEqual([])
    })
  }
})
