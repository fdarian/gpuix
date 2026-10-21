// @ts-nocheck
/**
 * A `border-color` transition on a row of a collapsed table (issue #68). The colour of a collapsed
 * border takes no part in which border wins an edge, so the table does not need the colour of its
 * parts before it builds them: it resolves the widths and styles (the geometry) first, and paints
 * the colour its parts were built with in the same frame. A frame of the transition must therefore
 * not build the table's siblings again (it used to ask for a second pass, which rebuilt every root
 * of the window), and the fade must be what is painted, frame by frame.
 *
 * The counters are `getRetainedCounters`, `getLayoutCounters` and `getFrameWorkCounters`; the
 * per-frame deltas of the layout and frame work counters are reported, not asserted.
 */

import fs from "fs"
import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { verifying } from "./retained-harness.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const TIMEOUT = 120_000

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

const WIDTH = 400
const ROW = 36
const ROWS = 3
const SIBLINGS = 60
const FRAME_MS = 8
const FRAMES = 14
const REST = "#dd0000"
const LIT = "#0000dd"

const fade = { transitionProperty: "border-color", transitionDuration: "80ms", transitionTimingFunction: "linear" }

function Page({ lit, transition }: { lit: number; transition: boolean }) {
  return (
    <div style={{ width: WIDTH, flexDirection: "column", backgroundColor: "#ffffff" }}>
      <div style={{ display: "table", borderCollapse: "collapse", width: WIDTH }}>
        <div style={{ display: "table-row-group" }}>
          {Array.from({ length: ROWS }, (_, index) => (
            <div
              key={index}
              style={{
                display: "table-row",
                height: ROW,
                borderBottomStyle: "solid",
                borderBottomWidth: 1,
                borderBottomColor: index === lit || index === lit - 1 ? LIT : REST,
                ...(transition ? fade : {}),
              }}
            >
              {["Name", "Role", "Status"].map((label) => (
                <div key={label} style={{ display: "table-cell", verticalAlign: "middle", padding: 8 }}>
                  <text style={{ fontSize: 14, color: "#222222" }}>{label}</text>
                </div>
              ))}
            </div>
          ))}
        </div>
      </div>
      {Array.from({ length: SIBLINGS }, (_, index) => (
        <div key={index} style={{ height: 24, padding: 4 }}>
          <text style={{ fontSize: 14, color: "#555555" }}>{`Sibling row ${index}`}</text>
        </div>
      ))}
    </div>
  )
}

type Work = Record<string, number>

function read(renderer): Work {
  const out: Work = { builds: renderer.getRetainedCounters().builds }
  for (const group of [renderer.getLayoutCounters(), renderer.getFrameWorkCounters()])
    for (const [key, value] of Object.entries(group)) if (typeof value === "number") out[key] = value
  return out
}

/** A display-link frame: the clock moves as time passes, and one frame is drawn. */
function step(renderer, ms: number) {
  renderer.clockAdvance(ms)
  renderer.drawFrame()
  renderer.dispatchNativeEvents()
}

const shotFile = (name: string) => `${SHOTS_DIR}/collapsed-border-transition-${process.pid}-${name}.png`

function pixels(renderer, name: string) {
  const file = shotFile(name)
  fs.rmSync(file, { force: true })
  renderer.captureScreenshot(file)
  return readLogicalPixels(file, renderer)
}

/**
 * Lights the rows under the pointer's new place, steps the clock `FRAMES` frames and returns what each frame did, or
 * with `capture` the colour on the line under the middle row at the end of each: a screenshot takes the frame the
 * window retained away, so the next frame is built whole, and a run reads one or the other.
 */
async function glide(transition: boolean, capture: boolean) {
  const root = createTestRoot({ width: WIDTH, height: ROW * ROWS + SIBLINGS * 24 + 40 })
  const renderer = root.renderer
  try {
    renderer.clockPause()
    root.render(<Page lit={0} transition={transition} />)
    for (let frame = 0; frame < 20; frame++) {
      renderer.flush()
      renderer.dispatchNativeEvents()
      await sleep(8)
    }
    renderer.setInlineDraws(false)
    for (let frame = 0; frame < 10; frame++) step(renderer, FRAME_MS)

    // The line under the middle row, where the rule is REST before the change and LIT after it.
    const line = capture ? ruleUnderMiddleRow(pixels(renderer, "rest")) : undefined

    const frames: Work[] = []
    const colors: string[] = []
    root.render(<Page lit={2} transition={transition} />)
    for (let frame = 0; frame < FRAMES; frame++) {
      const before = read(renderer)
      step(renderer, frame === 0 ? 0 : FRAME_MS)
      const after = read(renderer)
      frames.push(Object.fromEntries(Object.keys(after).map((key) => [key, after[key] - before[key]])))
      if (capture) colors.push(pixels(renderer, `frame-${frame}`).at(5, line).join(","))
    }
    if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    return { frames, colors }
  } finally {
    root.unmount()
  }
}

/** The y of the rule under the middle row, found where it is REST before the change. */
function ruleUnderMiddleRow(pixels): number {
  const line = Array.from({ length: 7 }, (_, offset) => ROW * 2 - 3 + offset).find(
    (y) => pixels.at(5, y).join() === "221,0,0,255"
  )
  if (line === undefined) throw new Error("the rule under the middle row is not painted")
  return line
}

/** A verified frame builds the page a second time, whole: only its mismatches are looked at there. */
function expectNoBuilds(frames: Work[]) {
  if (!verifying) expect(frames.map((frame) => frame.builds)).toEqual(frames.map(() => 0))
}

function report(name: string, frames: Work[]) {
  const keys = Object.keys(frames[0]).filter((key) => frames.some((frame) => frame[key] !== 0))
  console.log(`${name}: per-frame deltas (${FRAMES} frames of ${FRAME_MS} ms)`)
  for (const key of keys) console.log(`  ${key.padEnd(24)} ${frames.map((frame) => frame[key]).join(" ")}`)
}

describeNative("a border-color transition on a row of a collapsed table", { timeout: TIMEOUT }, () => {
  it("builds none of the table's siblings in a frame of the transition", async () => {
    const { frames } = await glide(true, false)
    report("transition", frames)
    expectNoBuilds(frames)
  })

  it("paints the colour of the frame it is built in, from one end to the other", async () => {
    const { colors } = await glide(true, true)
    console.log(`painted: ${colors.join(" | ")}`)
    const between = new Set(colors.filter((color) => color !== "221,0,0,255" && color !== "0,0,221,255"))
    expect(between.size).toBeGreaterThanOrEqual(4)
    expect(colors[colors.length - 1]).toBe("0,0,221,255")
    // The first frame commits the change at 0 ms; the transition is linear, so frame k is k * 8 ms of 80 in.
    colors.forEach((color, frame) => {
      const done = Math.min(frame / 10, 1)
      const [red, green, blue] = color.split(",").map(Number)
      expect(Math.abs(red - 221 * (1 - done)), `frame ${frame}: ${color}`).toBeLessThanOrEqual(2)
      expect(green).toBe(0)
      expect(Math.abs(blue - 221 * done), `frame ${frame}: ${color}`).toBeLessThanOrEqual(2)
    })
  })

  it("changes in one frame, building none of the siblings, without a transition", async () => {
    const { frames } = await glide(false, false)
    report("no transition", frames)
    expectNoBuilds(frames)
    const { colors } = await glide(false, true)
    expect(colors).toEqual(colors.map(() => "0,0,221,255"))
  })
})
