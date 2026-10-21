// @ts-nocheck
/**
 * A `border-color` transition on a row of a collapsed table (issue #68). The colour of a collapsed
 * border takes no part in which border wins an edge, so the table does not need the colour of its
 * parts before it builds them: it resolves the widths and styles (the geometry) first, and paints
 * the colour its parts were built with in the same frame. A frame of the transition must therefore
 * not build the table's siblings again (it used to ask for a second pass, which rebuilt every root
 * of the window), and the fade must be what is painted, frame by frame.
 *
 * A tick of the transition is a patched tick (#62): the table is the unit, since it is the table that
 * paints the borders of its parts, so the tick builds the table alone and has the frame shown painted
 * again there. The counters are `getRetainedCounters`, `getLayoutCounters`, `getFrameWorkCounters` and
 * `getPatchCounters`. A patched tick lays out the nodes of the unit against the nodes the frame has (one
 * pass, `nodesReused` of the table's, none created), as every unit does, and nothing of the page around it.
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
const STEPS = 10
const FRAME_MS = 8
const FRAMES = 14
const REST = "#dd0000"
const LIT = "#0000dd"

const fade = { transitionProperty: "border-color", transitionDuration: "80ms", transitionTimingFunction: "linear" }

function Page({
  lit,
  transition,
  rule = 1,
  siblings = SIBLINGS,
  anonymous = false,
}: {
  lit: number
  transition: boolean
  rule?: number
  siblings?: number
  /** The rows are the parts of an anonymous table: no element of the page is the table. */
  anonymous?: boolean
}) {
  return (
    <div style={{ width: WIDTH, flexDirection: "column", backgroundColor: "#ffffff" }}>
      <div style={{ display: anonymous ? "block" : "table", borderCollapse: "collapse", width: WIDTH }}>
        <div style={{ display: "table-row-group" }}>
          {Array.from({ length: ROWS }, (_, index) => (
            <div
              key={index}
              style={{
                display: "table-row",
                height: ROW,
                borderBottomStyle: "solid",
                borderBottomWidth: rule,
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
      {Array.from({ length: siblings }, (_, index) => (
        <div key={index} style={{ height: 24, padding: 4 }}>
          <text style={{ fontSize: 14, color: "#555555" }}>{`Sibling row ${index}`}</text>
        </div>
      ))}
    </div>
  )
}

type Work = Record<string, number>

function read(renderer): Work {
  const patches = renderer.getPatchCounters()
  const out: Work = {
    builds: renderer.getRetainedCounters().builds,
    patchedFrames: patches.patchedFrames,
    patchedUnits: patches.patchedUnits,
  }
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

type Glide = {
  transition: boolean
  siblings?: number
  anonymous?: boolean
  /** A screenshot takes the frame the window retained away, so the next frame is built whole: this says which to take. */
  capture?: "each" | number
  /** The frame at which the rule under the lit rows gets this wide, by an edit of the tree. */
  widenAt?: { frame: number; rule: number }
}

/**
 * Lights the rows under the pointer's new place, steps the clock `FRAMES` frames and returns what each frame did, and
 * with `capture` the colour on the line under the middle row at the end of each frame, or of one: a run reads one or
 * the other.
 */
async function glide(options: Glide) {
  const siblings = options.siblings ?? SIBLINGS
  const root = createTestRoot({ width: WIDTH, height: ROW * ROWS + siblings * 24 + 40 })
  const renderer = root.renderer
  try {
    renderer.clockPause()
    root.render(<Page lit={0} transition={options.transition} siblings={siblings} anonymous={options.anonymous} />)
    for (let frame = 0; frame < 20; frame++) {
      renderer.flush()
      renderer.dispatchNativeEvents()
      await sleep(8)
    }
    renderer.setInlineDraws(false)
    for (let frame = 0; frame < 10; frame++) step(renderer, FRAME_MS)

    // The line under the middle row, where the rule is REST before the change and LIT after it.
    const line = options.capture === undefined ? undefined : ruleUnderMiddleRow(pixels(renderer, "rest"))

    const frames: Work[] = []
    const colors: string[] = []
    const rules: number[] = []
    root.render(<Page lit={2} transition={options.transition} siblings={siblings} anonymous={options.anonymous} />)
    for (let frame = 0; frame < FRAMES; frame++) {
      if (options.widenAt?.frame === frame) {
        root.render(<Page lit={2} transition={options.transition} siblings={siblings} rule={options.widenAt.rule} anonymous={options.anonymous} />)
      }
      const before = read(renderer)
      step(renderer, frame === 0 ? 0 : FRAME_MS)
      const after = read(renderer)
      frames.push(Object.fromEntries(Object.keys(after).map((key) => [key, after[key] - before[key]])))
      if (line !== undefined && (options.capture === "each" || options.capture === frame)) {
        const shot = pixels(renderer, `frame-${frame}`)
        colors.push(shot.at(5, line).join(","))
        rules.push(ruleThickness(shot))
      }
    }
    if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    return { frames, colors, rules }
  } finally {
    root.unmount()
  }
}

/** How many rows of pixels at the left edge, around the middle row's bottom, are not the white of the page. */
function ruleThickness(pixels): number {
  let thick = 0
  for (let y = ROW * 2 - 8; y < ROW * 2 + 8; y++) if (pixels.at(5, y).join() !== "255,255,255,255") thick++
  return thick
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

/** The frames of the run that the clock alone moved: the change commits in the first, the second makes the unit. */
const TICKS = Array.from({ length: STEPS - 1 }, (_, index) => index + 2)

/** What a patched tick does: the unit alone, and none of the page around it. */
const TICK_WORK = ["nodesCreated", "nodesReused", "nodesRemoved", "passes", "prepainted", "painted", "paintedPrimitives", "replayedPrimitives"]

describeNative("a border-color transition on a row of a collapsed table", { timeout: TIMEOUT }, () => {
  it("draws a tick of the transition by building the table alone", async () => {
    const { frames } = await glide({ transition: true })
    report("transition", frames)
    expectNoBuilds(frames.slice(TICKS[0]))
    for (const tick of TICKS) {
      const frame = frames[tick]
      expect(frame.patchedFrames, `tick ${tick} is patched`).toBe(1)
      expect(frame.patchedUnits, `tick ${tick} repaints one unit, the table`).toBe(1)
      expect(frame.nodesCreated, `tick ${tick} makes no layout node`).toBe(0)
      expect(frame.nodesRemoved).toBe(0)
      expect(frame.replayedPrimitives, `tick ${tick} replays no root of the page`).toBe(0)
    }
    // The ticks after the end of the transition have nothing to draw.
    for (const tick of Array.from({ length: FRAMES - STEPS - 1 }, (_, index) => STEPS + 1 + index)) {
      expect(frames[tick].patchedFrames).toBe(0)
      expect(frames[tick].painted).toBe(0)
    }
  })

  // A verified frame builds the page whole after each patch, which is the work it counts.
  it.skipIf(verifying)("does the same work in a tick whatever the page around the table", async () => {
    const small = await glide({ transition: true })
    const large = await glide({ transition: true, siblings: SIBLINGS * 4 })
    for (const tick of TICKS) {
      for (const key of TICK_WORK) expect(large.frames[tick][key], `tick ${tick}: ${key}`).toBe(small.frames[tick][key])
    }
  })

  it("paints the colour of the frame it is built in, from one end to the other", async () => {
    const { colors } = await glide({ transition: true, capture: "each" })
    console.log(`painted: ${colors.join(" | ")}`)
    const between = new Set(colors.filter((color) => color !== "221,0,0,255" && color !== "0,0,221,255"))
    expect(between.size).toBeGreaterThanOrEqual(4)
    expect(colors[colors.length - 1]).toBe("0,0,221,255")
    // The first frame commits the change at 0 ms; the transition is linear, so frame k is k * 8 ms of 80 in.
    colors.forEach((color, frame) => expectFade(color, frame))
  })

  it("paints the same colour in the ticks that are patched", async () => {
    // A screenshot takes the frame the window retained away, so each tick is read in a run where the ones before are not.
    for (const tick of [2, 5, STEPS, STEPS + 2]) {
      const { frames, colors } = await glide({ transition: true, capture: tick })
      expect(frames[tick].patchedFrames, `tick ${tick} is patched`).toBe(tick <= STEPS ? 1 : 0)
      expectFade(colors[0], tick)
    }
  })

  it("lays out again when the width of the border changes in the middle of the transition", async () => {
    const wide = 3
    const { frames, colors, rules } = await glide({ transition: true, capture: "each", widenAt: { frame: 5, rule: wide } })
    expect(rules[4], "the rule is one line wide before").toBe(1)
    rules.slice(5).forEach((rule, index) => expect(rule, `frame ${5 + index}`).toBe(wide))
    colors.slice(5).forEach((color, index) => expectFade(color, 5 + index))
    expect(frames[5].passes, "the frame that finds the change lays out").toBeGreaterThanOrEqual(1)
    expect(frames[5].patchedFrames).toBe(0)

    // Without the screenshots in between: the edit is one whole frame, and the ticks go on patched after it.
    const run = await glide({ transition: true, widenAt: { frame: 5, rule: wide } })
    report("width changed at frame 5", run.frames)
    expect(run.frames[5].patchedFrames).toBe(0)
    expect(run.frames[5].passes).toBeGreaterThanOrEqual(1)
    expect(run.frames[STEPS].patchedFrames).toBe(1)
  })

  it("draws the ticks of a table that no element is whole, and paints the fade", async () => {
    // The anonymous table around loose parts has no element to be a unit.
    const { frames } = await glide({ transition: true, anonymous: true })
    for (const tick of TICKS) expect(frames[tick].patchedFrames, `tick ${tick}`).toBe(0)
    const { colors } = await glide({ transition: true, anonymous: true, capture: "each" })
    colors.forEach((color, frame) => expectFade(color, frame))
  })

  it("changes in one frame, building none of the siblings, without a transition", async () => {
    const { frames } = await glide({ transition: false })
    report("no transition", frames)
    expectNoBuilds(frames)
    const { colors } = await glide({ transition: false, capture: "each" })
    expect(colors).toEqual(colors.map(() => "0,0,221,255"))
  })
})

/** The colour of the rule at frame `frame` of the fade: frame k is k * 8 ms of the 80, from REST to LIT. */
function expectFade(color: string, frame: number) {
  const done = Math.min(frame / STEPS, 1)
  const [red, green, blue] = color.split(",").map(Number)
  expect(Math.abs(red - 221 * (1 - done)), `frame ${frame}: ${color}`).toBeLessThanOrEqual(2)
  expect(green).toBe(0)
  expect(Math.abs(blue - 221 * done), `frame ${frame}: ${color}`).toBeLessThanOrEqual(2)
}
