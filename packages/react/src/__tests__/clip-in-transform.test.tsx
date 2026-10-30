/// A clip inside a transformed box keeps the box's text (#110). The text of a box is cut away when its glyphs lie outside the
/// content mask, and the mask is in window space: a glyph tested where layout put it, and not where the transform moved it,
/// was outside a mask that had moved with the box, so a card with `overflow: hidden` moved up by 80 px (or `overflow-x`,
/// `clip-path`, a clipping child) drew its background and no text.
///
/// `fixtures/clip-in-transform-oracle.html` is the page and `fixtures/clip-in-transform-oracle.json` the ink (red pixels) of
/// each cell in headless Chrome (`clip-in-transform-oracle.sh`): a 150 x 44 card whose text is longer than the card, with no
/// clip or each of the four, translated up and down and sideways through and past the old threshold, scaled, rotated, and a
/// translation with each. Chrome's ink of a clipped card is the same wherever it is moved, which is what is asserted of
/// gpuix: its ink in a cell, against its own ink of the same card unmoved, is Chrome's.
///
/// The cull is still there: a glyph that is really outside the clip is not painted, wherever the card is.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { motion } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { verifying } from "./retained-harness.js"
import { readPixels, SHOTS_DIR, type Pixels } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")
const TIMEOUT = 180_000

type Trigger = "none" | "overflow" | "overflowX" | "clipPath" | "child"
type Scale = 1 | 2
type Case = { id: string; trigger: Trigger; axis: string; transform: string; x: number; y: number; ink: number }

const ORACLE: Record<"x1" | "x2", { scale: number; cases: Case[] }> = JSON.parse(
  fs.readFileSync(path.join(FIXTURES, "clip-in-transform-oracle.json"), "utf8")
)
const PAGE = { width: 1260, height: 7020 }
const CELL = { width: 420, height: 260 }
const TEXT = "hello world hello world hello"

const textStyle = {
  fontFamily: "Helvetica",
  fontSize: 14,
  lineHeight: "18px",
  color: "#ff0000",
  whiteSpace: "nowrap",
} as const

function Card(props: { trigger: Trigger; transform: string; text?: string; width?: number }) {
  const style: React.CSSProperties = {
    position: "absolute",
    left: 135,
    top: 108,
    width: props.width ?? 150,
    height: 44,
    boxSizing: "border-box",
    backgroundColor: "#333333",
    borderRadius: 8,
    display: "flex",
    alignItems: "center",
    paddingLeft: 14,
    transform: props.transform,
  }
  if (props.trigger === "overflow") style.overflow = "hidden"
  if (props.trigger === "overflowX") style.overflowX = "hidden"
  if (props.trigger === "clipPath") style.clipPath = "inset(0 round 8px)"
  const text = <text style={textStyle}>{props.text ?? TEXT}</text>
  return (
    <div style={style}>
      {props.trigger === "child" ? (
        <div style={{ overflow: "hidden", flex: 1, minWidth: 0, height: "100%", display: "flex", alignItems: "center" }}>{text}</div>
      ) : (
        text
      )}
    </div>
  )
}

/** The red of the text over the grey of the card or the page: `R - max(G, B)` over 60, as `clip-in-transform-oracle.py`. */
function inkIn(pixels: Pixels, scale: Scale, c: Case) {
  let count = 0
  for (let y = c.y * scale; y < (c.y + CELL.height) * scale; y++) {
    for (let x = c.x * scale; x < (c.x + CELL.width) * scale; x++) {
      const rgb = pixels.at(x, y)
      if (rgb[0] - Math.max(rgb[1], rgb[2]) > 60) count++
    }
  }
  return count
}

const PAINTED = {} as Record<number, Pixels>
function page(scale: Scale) {
  const cached = PAINTED[scale]
  if (cached) return cached
  const root = createTestRoot({ width: PAGE.width, height: PAGE.height, scaleFactor: scale })
  try {
    root.render(
      <div style={{ position: "relative", width: PAGE.width, height: PAGE.height, backgroundColor: "#ffffff" }}>
        {ORACLE[`x${scale}`].cases.map((c) => (
          <div key={c.id} style={{ position: "absolute", left: c.x, top: c.y, width: CELL.width, height: CELL.height }}>
            <Card trigger={c.trigger} transform={c.transform} />
          </div>
        ))}
      </div>
    )
    root.renderer.dispatchNativeEvents()
    const file = path.join(SHOTS_DIR, `clip-in-transform-x${scale}.png`)
    root.renderer.captureScreenshot(file)
    return (PAINTED[scale] = readPixels(file))
  } finally {
    root.unmount()
  }
}

/** How far the ink of a card moved is from the same card unmoved, which Chrome puts at 1.0 for a translation. */
function relativeInk(scale: Scale, c: Case, ink: Map<string, number>) {
  const unmoved = ORACLE[`x${scale}`].cases.find((other) => other.trigger === c.trigger && other.transform === "translate(0px, 0px)")!
  return ink.get(c.id)! / ink.get(unmoved.id)!
}

describeNative("the text of a clipped box under a transform against Chrome", () => {
  for (const scale of [1, 2] as const) {
    it(`draws the text of every case Chrome draws at scale ${scale}`, () => {
      const pixels = page(scale)
      const cases = ORACLE[`x${scale}`].cases
      const mine = new Map(cases.map((c) => [c.id, inkIn(pixels, scale, c)]))
      const chrome = new Map(cases.map((c) => [c.id, c.ink]))
      const failures: string[] = []
      for (const c of cases) {
        if (c.ink === 0) continue
        const ink = mine.get(c.id)!
        const expected = relativeInk(scale, c, chrome)
        const found = relativeInk(scale, c, mine)
        if (ink === 0) failures.push(`${c.trigger} ${c.transform}: no text, Chrome has ${c.ink}`)
        else if (Math.abs(found / expected - 1) > 0.08) {
          failures.push(`${c.trigger} ${c.transform}: ${found.toFixed(2)} of its unmoved ink, Chrome ${expected.toFixed(2)}`)
        }
      }
      expect(failures, failures.join("\n")).toEqual([])
    }, TIMEOUT)
  }
})

/** Primitives a frame of one card painted: the glyphs that were not cut away, the card and its background. */
function primitivesOf(trigger: Trigger, transform: string, text: string, width?: number) {
  const root = createTestRoot({ width: 2400, height: 260 })
  try {
    root.render(
      <div style={{ position: "relative", width: 2400, height: 260, backgroundColor: "#ffffff" }}>
        <Card trigger={trigger} transform={transform} text={text} width={width} />
      </div>
    )
    root.renderer.dispatchNativeEvents()
    root.renderer.captureScreenshot(path.join(SHOTS_DIR, "clip-in-transform-count.png"))
    return root.renderer.getFrameWorkCounters().paintedPrimitives
  } finally {
    root.unmount()
  }
}

describeNative("glyphs outside a clip in a transformed box", () => {
  const LONG = Array.from({ length: 120 }, (_, index) => String.fromCodePoint(0x4e00 + index)).join("")
  const MOVES = [
    "translate(0px, 0px)",
    "translate(0px, -80px)",
    "translate(0px, -104px)",
    "translate(130px, 0px)",
    "translate(-130px, 40px)",
    "scale(0.5)",
    "rotate(30deg)",
  ]

  for (const trigger of ["overflow", "overflowX", "child"] as const) {
    it(`still cuts what a ${trigger} clip hides, wherever the box is`, () => {
      const open = primitivesOf("none", "translate(0px, 0px)", LONG)
      const unmoved = primitivesOf(trigger, "translate(0px, 0px)", LONG)
      // The clip shows about 9 of the 120 glyphs: the rest are not painted.
      expect(unmoved, `${open} primitives unclipped`).toBeLessThan(open - 60)
      for (const transform of MOVES) {
        const moved = primitivesOf(trigger, transform, LONG)
        expect(moved, `${trigger} ${transform}: the glyphs it paints, against the box unmoved`).toBeGreaterThanOrEqual(unmoved - 3)
        expect(moved, `${trigger} ${transform}: the glyphs it paints, against the box unmoved`).toBeLessThanOrEqual(unmoved + 8)
      }
    }, TIMEOUT)
  }

  it("cuts all of the text of a box whose clip has none of it in view", () => {
    // A text that starts past the end of the clip: no glyph shows.
    const hidden = (transform: string) => primitivesOf("overflow", transform, LONG, 10)
    const reference = primitivesOf("overflow", "translate(0px, 0px)", "", 10)
    for (const transform of ["translate(0px, 0px)", "translate(0px, -104px)", "translate(130px, 0px)"]) {
      expect(hidden(transform), transform).toBeLessThanOrEqual(reference + 8)
    }
  })
})

describeNative("a clipped card moved by Motion's y", () => {
  const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))
  const CARD = { left: 135, top: 108, width: 150, height: 44 }
  const inkOfFrame = (renderer: ReturnType<typeof createTestRoot>["renderer"], name: string) => {
    renderer.dispatchNativeEvents()
    const file = path.join(SHOTS_DIR, `clip-in-transform-${name}.png`)
    renderer.captureScreenshot(file)
    const pixels = readPixels(file)
    let count = 0
    for (let y = 0; y < 260; y++) {
      for (let x = 0; x < 420; x++) {
        const rgb = pixels.at(x, y)
        if (rgb[0] - Math.max(rgb[1], rgb[2]) > 60) count++
      }
    }
    return count
  }
  const page = (y: number | "slide") => (
    <div style={{ position: "relative", width: 420, height: 260, backgroundColor: "#ffffff" }}>
      <motion.div
        initial={y === "slide" ? { y: 0 } : false}
        animate={{ y: y === "slide" ? -104 : y }}
        transition={{ duration: y === "slide" ? 1 : 0, ease: "linear" }}
        style={{
          position: "absolute",
          ...CARD,
          boxSizing: "border-box",
          backgroundColor: "#333333",
          overflow: "hidden",
          borderRadius: 8,
          display: "flex",
          alignItems: "center",
          paddingLeft: 14,
        }}
      >
        <text style={textStyle}>{TEXT}</text>
      </motion.div>
    </div>
  )

  it("draws the text at a y of -80 and -104, as at 0", () => {
    const inks = [0, -52, -80, -104, 104].map((y) => {
      const root = createTestRoot({ width: 420, height: 260 })
      try {
        root.render(page(y))
        return inkOfFrame(root.renderer, `y${-y}`)
      } finally {
        root.unmount()
      }
    })
    expect(inks[0]).toBeGreaterThan(100)
    for (const ink of inks) expect(Math.abs(ink / inks[0]! - 1), `inks ${inks}`).toBeLessThan(0.05)
  }, TIMEOUT)

  it("keeps it on every frame of a slide through the old threshold", async () => {
    const root = createTestRoot({ width: 420, height: 260 })
    const renderer = root.renderer
    try {
      renderer.clockPause()
      root.render(page("slide"))
      for (let frame = 0; frame < 8; frame++) {
        renderer.flush()
        renderer.dispatchNativeEvents()
        await sleep(8)
      }
      const inks: number[] = []
      for (let tick = 0; tick < 40; tick++) {
        renderer.clockAdvance(25)
        renderer.drawFrame()
        inks.push(inkOfFrame(renderer, "slide"))
      }
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
      expect(Math.min(...inks), `inks ${inks}`).toBeGreaterThan(Math.max(...inks) * 0.95)
    } finally {
      root.unmount()
    }
  }, TIMEOUT)
})
