// @ts-nocheck
/**
 * Cards that run three infinite loops each (a `motion.div`, a `motion.path` morphing `d`, a CSS colour loop), and
 * how to count what a page pays for them, for the tests of animations in `content-visibility` content.
 */

import fs from "fs"
import React from "react"
import { defineKeyframes, motion } from "../index"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

defineKeyframes("skipped-pulse", {
  "0%": { backgroundColor: "#ff0000" },
  "100%": { backgroundColor: "#0000ff" },
})

export const CARD = 60
const GLYPH_A = "M0 0 L24 0 L24 24 L0 24 Z"
const GLYPH_B = "M0 0 L6 0 L6 24 L0 24 Z"
const TINTS = ["#ff0000", "#0000ff", "#ff0000"]

/** A card of three loops, 1 s each, that start with the card. */
export function AnimatedCard({ index, kinds = ["motion", "path", "css"] }: { index: number; kinds?: string[] }) {
  return (
    <div testId={`card-${index}`} style={{ height: CARD, flexShrink: 0, display: "flex", flexDirection: "row", gap: 8 }}>
      {kinds.includes("motion") ? <motion.div
        testId={`motion-${index}`}
        style={{ width: 40, height: 40, backgroundColor: "#ff0000" }}
        animate={{ backgroundColor: TINTS }}
        transition={{ duration: 1, ease: "linear", repeat: Infinity }}
      /> : null}
      {kinds.includes("path") ? <svg testId={`svg-${index}`} viewBox="0 0 24 24" width={24} height={24} style={{ flexShrink: 0 }}>
        <motion.path
          testId={`path-${index}`}
          d={GLYPH_A}
          fill="#00aa00"
          animate={{ d: [GLYPH_A, GLYPH_B, GLYPH_A] }}
          transition={{ duration: 1, ease: "linear", repeat: Infinity }}
        />
      </svg> : null}
      {kinds.includes("css") ? <div testId={`css-${index}`} style={{ width: 40, height: 40, backgroundColor: "#ff0000", animation: "skipped-pulse 1s linear infinite" }} /> : null}
    </div>
  )
}

/** A loop that is drawn, whose ticks are the work a page with skipped loops must not add to. */
export function VisibleLoop() {
  return <div testId="visible-loop" style={{ width: 40, height: 20, flexShrink: 0, animation: "skipped-pulse 1s linear infinite" }} />
}

/** Time passes as it does for a window: both clocks move, the frame callbacks and a pending change are drawn, events are delivered. */
export function elapse(renderer, ms = 16) {
  renderer.clockAdvance(ms)
  renderer.advanceTime(ms)
  renderer.drawFrame()
  renderer.dispatchNativeEvents()
}

function gained(after: string, before: string): Record<string, number> {
  const was = JSON.parse(before) as Record<string, number>
  const out: Record<string, number> = {}
  for (const [reason, count] of Object.entries(JSON.parse(after) as Record<string, number>)) {
    if (count > (was[reason] ?? 0)) out[reason] = count - (was[reason] ?? 0)
  }
  return out
}

/** A sidebar of rows that change their background when the pointer is on them, beside the cards of a page. */
const ROWS = 60
const ROW = 10
export function Sidebar() {
  return (
    <div style={{ width: 100, flexShrink: 0, display: "flex", flexDirection: "column" }}>
      {Array.from({ length: ROWS }, (_, row) => (
        <div key={row} testId={`row-${row}`} style={{ height: ROW, flexShrink: 0, hover: { backgroundColor: "#e5e5e5" } }}>
          <text style={{ fontSize: 8, color: "#222222" }}>{`Item ${row}`}</text>
        </div>
      ))}
    </div>
  )
}

/** A block of static rows: a page that a frame replays when nothing in it changed. */
export function Filler() {
  return (
    <div style={{ height: 30, flexShrink: 0, overflow: "hidden" }}>
      {Array.from({ length: 150 }, (_, row) => (
        <div key={row} style={{ height: 10, flexShrink: 0 }}>
          <text style={{ fontSize: 8, color: "#222222" }}>{`Filler ${row}`}</text>
        </div>
      ))}
    </div>
  )
}

/** What `moves` hovers of the rows, each drawn as the window does, built: elements built and frames drawn whole. */
export function hovered(renderer, moves: number) {
  const over = (move: number) => renderer.dispatchMouseMove(10, ((move * 7 + 1) % ROWS) * ROW + ROW / 2)
  for (let warm = 0; warm < 4; warm++) {
    over(warm)
    elapse(renderer)
  }
  renderer.resetBuildCounters()
  const painted = renderer.getPaintedFrameCount()
  for (let move = 0; move < moves; move++) {
    over(move + 4)
    renderer.dispatchNativeEvents()
    renderer.flush()
    elapse(renderer)
  }
  return { elementsBuilt: renderer.getBuildCounters().elementsBuilt, paintedFrames: renderer.getPaintedFrameCount() - painted }
}

/** What `steps` ticks of 16 ms did to the window after `warm` that are not counted. */
export function spent(renderer, steps: number, warm = 12) {
  for (let step = 0; step < warm; step++) elapse(renderer)
  renderer.resetBuildCounters()
  const painted = renderer.getPaintedFrameCount()
  const dormant = renderer.getFrameWorkCounters().dormantTicks
  const patches = renderer.getPatchCounters()
  for (let step = 0; step < steps; step++) elapse(renderer)
  const after = renderer.getPatchCounters()
  return {
    paintedFrames: renderer.getPaintedFrameCount() - painted,
    patchedFrames: after.patchedFrames - patches.patchedFrames,
    emptyTicks: after.emptyTicks - patches.emptyTicks,
    declines: gained(after.declines, patches.declines),
    unpatchable: gained(after.unpatchable, patches.unpatchable),
    dormantTicks: renderer.getFrameWorkCounters().dormantTicks - dormant,
    motionsSynced: renderer.getBuildCounters().motionsSynced,
  }
}

/** The pixels of the window, in logical pixels. */
export function shotOf(renderer, name: string) {
  const file = `${SHOTS_DIR}/skipped-animations-${process.pid}-${name}.png`
  fs.rmSync(file, { force: true })
  renderer.captureScreenshot(file)
  const pixels = readLogicalPixels(file, renderer)
  fs.rmSync(file, { force: true })
  return pixels
}
