// @ts-nocheck
/**
 * A JS commit and a motion write dirty the window the way a live window is dirtied, with
 * `Window::refresh` left out: a refreshing frame replays nothing, so on a page of 600 rows one
 * setState or one `set` built every element (5403 over 3000 nodes) instead of the few that changed.
 * The live renderer and the test renderer reach the same `invalidate_after_commit`, which is what
 * lets this catch a `refresh` put back in it.
 */
import React, { useState } from "react"
import { expect, it } from "vitest"
import { motion, motionValue } from "../index"
import { flushSync } from "../reconciler/reconciler.js"
import { createTestRoot } from "../testing"
import { describeNative, verifying } from "./retained-harness.js"

const ROWS = 600
const WARM_FRAMES = 4
/** A verified frame builds the whole page again, which on 3000 elements is the cost of the test. */
const FRAMES = verifying ? 2 : 6
/** The changed row, its two cells and their text, and the mover; the page holds ~3000 elements. */
const HANDFUL = 30

const retained = process.env.GPUIX_RETAIN_OFF === undefined
const setLabels: Array<(label: string) => void> = []

function Row({ index }: { index: number }) {
  const [label, setLabel] = useState(`row ${index}`)
  setLabels[index] = setLabel
  return (
    <div style={{ display: "flex", flexDirection: "row", gap: 4, padding: 2 }}>
      <div style={{ borderWidth: 1, borderColor: "#cccccc", padding: 2 }}>
        <text style={{ fontSize: 11, color: "#333333" }}>{label}</text>
      </div>
      <div style={{ borderWidth: 1, borderColor: "#cccccc", padding: 2 }}>
        <text style={{ fontSize: 11, color: "#333333" }}>{`cell ${index}`}</text>
      </div>
    </div>
  )
}

function Page({ left }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", width: 1200, height: 760, overflow: "hidden" }}>
      <motion.div
        testId="mover"
        style={{ position: "absolute", top: 0, left, width: 40, height: 20, backgroundColor: "#00aa00" }}
      />
      {Array.from({ length: ROWS }, (_, index) => (
        <Row key={index} index={index} />
      ))}
    </div>
  )
}

async function mount() {
  const left = motionValue(0)
  const root = createTestRoot({ width: 1240, height: 780 })
  const renderer = root.renderer
  renderer.clockPause()
  root.render(<Page left={left} />)
  for (let frame = 0; frame < 6; frame++) renderer.flush()
  // A live window is drawn by the display link, once a frame.
  renderer.setInlineDraws(false)
  renderer.setA11yActive(false)
  for (let frame = 0; frame < WARM_FRAMES; frame++) renderer.drawFrame()
  return { root, renderer, left }
}

/** Runs `change` then draws the frame, as the live window does after a write. */
function framesOf(renderer, change: (frame: number) => void) {
  const frames: Array<{ built: number; hits: number }> = []
  for (let frame = 0; frame < FRAMES; frame++) {
    const built = renderer.getBuildCounters().elementsBuilt
    const hits = renderer.getRetainedCounters().hits
    change(frame)
    renderer.drawFrame()
    frames.push({
      built: renderer.getBuildCounters().elementsBuilt - built,
      hits: renderer.getRetainedCounters().hits - hits,
    })
  }
  return frames
}

const expectOnlyWhatChanged = (frames) => {
  if (!retained) return
  for (const frame of frames) {
    expect(frame.hits).toBeGreaterThan(0)
    // A verified run builds each replayed frame a second time, whole.
    if (!verifying) expect(frame.built).toBeLessThan(HANDFUL)
  }
}

describeNative("a write from JS on a page of 600 rows", { timeout: 180_000 }, () => {
  it("builds a handful of elements for a setState on one row", async () => {
    const { root, renderer } = await mount()
    try {
      const frames = framesOf(renderer, (frame) => {
        flushSync(() => setLabels[300](`changed ${frame}`))
        renderer.flush()
      })
      expectOnlyWhatChanged(frames)
      expect(renderer.getAllText()).toContain(`changed ${FRAMES - 1}`)
      expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })

  it("builds a handful of elements for a motion value set", async () => {
    const { root, renderer, left } = await mount()
    try {
      const frames = framesOf(renderer, (frame) => left.set(10 + frame * 7))
      expectOnlyWhatChanged(frames)
      expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })
})
