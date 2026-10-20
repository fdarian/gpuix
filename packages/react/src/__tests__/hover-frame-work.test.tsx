// @ts-nocheck
/**
 * What a hover or sweep frame does to content that did not change and cannot be seen: a page that
 * is laid out and hit-tested as in Chrome, and costs nothing else. A sidebar of hover rows beside
 * a main area of N cards must make a frame of the same work for N = 200 as for N = 0 when the
 * cards are below the window, inside a clipped scroller, or hold `opacity: 0` copies of their
 * label. The recordings that a frame replays are reported (they scale with N), not asserted.
 *
 * The counters are `getBuildCounters`, `getLayoutCounters` and `getFrameWorkCounters`.
 */

import React, { useState } from "react"
import { afterEach, describe, expect, it } from "vitest"
import { defineKeyframes } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { verifying } from "./retained-harness.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const retained = process.env.GPUIX_RETAIN_OFF === undefined
const TIMEOUT = 120_000

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

type Work = Record<string, number>

/** What a frame did, from the difference of two reads. */
const WORK_KEYS = [
  "elementsBuilt",
  "measures",
  "shapes",
  "stylesSet",
  "childrenSet",
  "nodesCreated",
  "extentWalks",
  "overflowWalks",
  "unsnappedOverflowWalks",
  "prepainted",
  "culledPrepainted",
  "painted",
  "culledPainted",
  "paintedPrimitives",
] as const

function read(renderer): Work {
  const layout = renderer.getLayoutCounters()
  const frame = renderer.getFrameWorkCounters()
  return {
    elementsBuilt: renderer.getBuildCounters().elementsBuilt,
    ...layout,
    ...frame,
    replays: renderer.getRetainedCounters().replays,
    builds: renderer.getRetainedCounters().builds,
  }
}

/** Runs `change`, draws the frame as the display link does, and returns what it did. */
function work(renderer, change: () => void): Work {
  renderer.resetBuildCounters()
  const before = read(renderer)
  change()
  renderer.drawFrame()
  const after = read(renderer)
  const done: Work = { elementsBuilt: after.elementsBuilt }
  for (const key of Object.keys(after)) if (key !== "elementsBuilt") done[key] = after[key] - before[key]
  return done
}

const pick = (done: Work) => Object.fromEntries(WORK_KEYS.map((key) => [key, done[key]]))

async function settle(renderer, frames = 20) {
  for (let frame = 0; frame < frames; frame++) {
    renderer.flush()
    renderer.dispatchNativeEvents()
    await sleep(8)
  }
}

// ── Sidebar and cards ───────────────────────────────────────────────

const ROW = 16
const ROWS = 60
const SIZE = { width: 900, height: 1022 }

type Place = "below" | "scroller"

/** The label of a design system that does not move when its weight does: the text over an `opacity: 0` copy of itself in the cell of an `inline-grid`. */
function Label({ text, ghost }: { text: string; ghost: boolean }) {
  const cell = { gridColumnStart: 1, gridRowStart: 1, fontSize: 14 }
  return (
    <div style={{ display: "inline-grid" }}>
      {ghost ? (
        <text style={{ ...cell, fontWeight: 600, opacity: 0 }}>{text}</text>
      ) : null}
      <text style={{ ...cell, color: "#222222" }}>{text}</text>
    </div>
  )
}

function Card({ index, ghost }: { index: number; ghost: boolean }) {
  return (
    <div style={{ height: 120, flexShrink: 0, padding: 8, marginBottom: 8, backgroundColor: "#f4f4f4", borderRadius: 8, flexDirection: "column", gap: 4 }}>
      <div style={{ display: "flex" }}>
        <Label text={`Card title ${index}`} ghost={ghost} />
      </div>
      <text style={{ fontSize: 12, color: "#666666" }}>{`Description line one of card ${index}`}</text>
      <text style={{ fontSize: 12, color: "#666666" }}>{`Description line two of card ${index}`}</text>
      <div style={{ width: 60, height: 18, borderRadius: 9, backgroundColor: "#dddddd" }} />
    </div>
  )
}

function SidebarPage({ cards, place, ghost }: { cards: number; place: Place; ghost: boolean }) {
  const content = Array.from({ length: cards }, (_, index) => <Card key={index} index={index} ghost={ghost} />)
  return (
    <div style={{ width: SIZE.width, height: SIZE.height, flexDirection: "row" }}>
      <div style={{ width: 200, flexDirection: "column", flexShrink: 0, padding: 4 }}>
        {Array.from({ length: ROWS }, (_, index) => (
          <div key={index} style={{ height: ROW, flexShrink: 0, padding: 1, display: "flex", borderRadius: 4, hover: { backgroundColor: "#e5e5e5" } }}>
            <text style={{ fontSize: 12, color: "#222222" }}>{`Sidebar item ${index}`}</text>
          </div>
        ))}
      </div>
      {place === "below" ? (
        <div style={{ flex: 1, flexDirection: "column" }}>
          <div style={{ height: SIZE.height, flexShrink: 0 }} />
          {content}
        </div>
      ) : (
        <div style={{ flex: 1, height: 400, overflow: "auto", flexDirection: "column" }}>{content}</div>
      )}
    </div>
  )
}

async function mountSidebar(props: { cards: number; place: Place; ghost: boolean }, a11y: boolean) {
  const root = createTestRoot({ width: SIZE.width, height: SIZE.height })
  const renderer = root.renderer
  root.render(<SidebarPage {...props} />)
  await settle(renderer)
  renderer.setInlineDraws(false)
  renderer.setA11yActive(a11y)
  let moves = 0
  /** A hover frame: the pointer enters another row of the sidebar. */
  const hover = async () =>
    work(renderer, () => {
      const row = (moves * 7 + 1) % 58
      moves++
      renderer.dispatchMouseMove(20, 4 + row * ROW + ROW / 2)
      renderer.dispatchNativeEvents()
      renderer.flush()
    })
  for (let warm = 0; warm < 15; warm++) await hover()
  return { root, renderer, hover }
}

const MOVES = 6

async function hoverFrames(props, a11y: boolean) {
  const page = await mountSidebar(props, a11y)
  try {
    const frames: Work[] = []
    for (let move = 0; move < MOVES; move++) frames.push(await page.hover())
    if (verifying) expect(page.renderer.getRetainedCounters().mismatches, page.renderer.getRetentionMismatches().join("\n")).toBe(0)
    return frames
  } finally {
    page.root.unmount()
  }
}

const mean = (frames: Work[], key: string) => frames.reduce((sum, frame) => sum + frame[key], 0) / frames.length

describeNative("a hover frame on a page with unchanged content that is not shown", { timeout: TIMEOUT }, () => {
  const cases = []
  for (const place of ["below", "scroller"] as const)
    for (const ghost of [false, true])
      for (const a11y of [false, true]) cases.push({ place, ghost, a11y })

  for (const { place, ghost, a11y } of cases) {
    const name = `${place}${ghost ? ", each card label holding an opacity 0 copy" : ""}, a11y ${a11y ? "on" : "off"}`
    it.skipIf(!retained)(`does the work of an empty main area for 200 cards: ${name}`, async () => {
      const empty = await hoverFrames({ cards: 0, place, ghost }, a11y)
      const full = await hoverFrames({ cards: 200, place, ghost }, a11y)
      // A verified frame builds the page a second time, whole: only its mismatches are looked at there.
      if (verifying) return
      for (let move = 0; move < MOVES; move++) expect(pick(full[move]), `move ${move}`).toEqual(pick(empty[move]))
      // The recordings are put into the frame again: what the cards cost, and not asserted.
      console.info(
        `[hover-frame-work] ${name}: replays ${mean(empty, "replays")} -> ${mean(full, "replays")}, ` +
          `replayed primitives ${mean(empty, "replayedPrimitives")} -> ${mean(full, "replayedPrimitives")}, ` +
          `built ${mean(empty, "elementsBuilt")} -> ${mean(full, "elementsBuilt")}, ` +
          `prepainted ${mean(empty, "prepainted")} -> ${mean(full, "prepainted")}, ` +
          `painted primitives ${mean(empty, "paintedPrimitives")} -> ${mean(full, "paintedPrimitives")}`,
      )
      // Cards below the window are not reached at all, in a scroller each is a root that is replayed.
      if (place === "scroller") expect(mean(full, "replays")).toBeGreaterThan(mean(empty, "replays"))
    })
  }

  it.skipIf(verifying)("builds and paints the cards each frame when retention is off, which is what the equality above rules out", async () => {
    /** One window at a time: the test renderers share the thread's window. */
    const retentionOffFrame = async (cards: number) => {
      const page = await mountSidebar({ cards, place: "scroller", ghost: true }, false)
      try {
        page.renderer.setRetainEnabled(false)
        return await page.hover()
      } finally {
        page.root.unmount()
      }
    }
    const none = await retentionOffFrame(0)
    const full = await retentionOffFrame(200)
    expect(full.elementsBuilt).toBeGreaterThan(none.elementsBuilt + 1000)
    expect(full.prepainted + full.culledPrepainted).toBeGreaterThan(none.prepainted + 1000)
    expect(full.measures + full.shapes + full.stylesSet).toBeGreaterThanOrEqual(none.measures + none.shapes + none.stylesSet)
    expect(full.culledPrepainted).toBeGreaterThan(none.culledPrepainted)
  })
})

// ── opacity: 0 ──────────────────────────────────────────────────────

function GhostPage({ ghosts, opacity, onGhost, tick, refs }: { ghosts: boolean; opacity: number; onGhost?: () => void; tick: number; refs?: object }) {
  return (
    <div style={{ width: 400, height: 300, flexDirection: "column", gap: 4 }}>
      <div style={{ width: 20 + tick, height: 10, backgroundColor: "#0000ff" }} />
      <div style={{ display: "flex" }}>
        <div style={{ display: "inline-grid" }}>
          {ghosts ? (
            <text style={{ gridColumnStart: 1, gridRowStart: 1, fontWeight: 600, opacity }}>Ghost label text</text>
          ) : null}
          <text style={{ gridColumnStart: 1, gridRowStart: 1 }}>Ghost label text</text>
        </div>
      </div>
      {ghosts ? (
        <div
          testId="ghost-box"
          ref={(instance) => {
            if (refs) refs.box = instance
          }}
          onMouseDown={onGhost}
          style={{ opacity, width: 80, height: 30, backgroundColor: "#ff0000", padding: 2 }}
        >
          <text>GHOSTBOX</text>
        </div>
      ) : null}
    </div>
  )
}

/** The primitives of the frame that builds the page (the first one, or any with retention off). */
async function ghostFrames(ghosts: boolean, opacity: number, onGhost?: () => void, refs?: object) {
  const root = createTestRoot({ width: 400, height: 300 })
  const renderer = root.renderer
  renderer.setInlineDraws(false)
  root.render(<GhostPage ghosts={ghosts} opacity={opacity} onGhost={onGhost} tick={0} refs={refs} />)
  await settle(renderer)
  const first = work(renderer, () => {})
  const painted = renderer.getPaintedText()
  // Retention off builds everything on every frame, so a changed width builds the ghosts again.
  work(renderer, () => {})
  renderer.setRetainEnabled(false)
  root.render(<GhostPage ghosts={ghosts} opacity={opacity} onGhost={onGhost} tick={1} refs={refs} />)
  await settle(renderer, 4)
  const rebuilt = work(renderer, () => {})
  return { root, renderer, first, rebuilt, painted }
}

describeNative("an opacity 0 element", { timeout: TIMEOUT }, () => {
  it("paints no primitive on the frame that builds it, and on a frame with retention off", async () => {
    const control = await ghostFrames(false, 0)
    const ghosts = await ghostFrames(true, 0)
    try {
      expect(control.first.paintedPrimitives).toBeGreaterThan(0)
      expect(ghosts.first.paintedPrimitives).toBe(control.first.paintedPrimitives)
      expect(control.rebuilt.paintedPrimitives).toBeGreaterThan(0)
      expect(ghosts.rebuilt.paintedPrimitives).toBe(control.rebuilt.paintedPrimitives)
      expect(ghosts.rebuilt.elementsBuilt).toBeGreaterThan(control.rebuilt.elementsBuilt)
    } finally {
      control.root.unmount()
      ghosts.root.unmount()
    }
  })

  it("would paint them at opacity 0.01, which is what the count above is for", async () => {
    const control = await ghostFrames(false, 0.01)
    const faint = await ghostFrames(true, 0.01)
    try {
      expect(faint.first.paintedPrimitives).toBeGreaterThan(control.first.paintedPrimitives)
      expect(faint.rebuilt.paintedPrimitives).toBeGreaterThan(control.rebuilt.paintedPrimitives)
    } finally {
      control.root.unmount()
      faint.root.unmount()
    }
  })

  it("still has its layout box and is hit", async () => {
    let hits = 0
    const refs: any = {}
    const page = await ghostFrames(true, 0, () => hits++, refs)
    try {
      const rect = refs.box.getBoundingClientRect()
      expect(rect.width).toBe(80)
      expect(rect.height).toBe(30)
      page.renderer.nativeSimulateMouseDown(rect.x + 10, rect.y + 10)
      expect(hits).toBe(1)
    } finally {
      page.root.unmount()
    }
  })
})

// ── #57: one animating text on a page of ~3000 nodes ────────────────

defineKeyframes("hover-frame-work-pulse", {
  "0%": { color: "#000000" },
  "50%": { color: "#ff0000" },
  "100%": { color: "#000000" },
})

const ANIM_ROWS = 600

function AnimatedPage() {
  return (
    <div style={{ display: "flex", flexDirection: "column", width: 1200, height: 760, overflow: "hidden" }}>
      <text testId="animated" style={{ fontSize: 20, color: "#000000", animation: "hover-frame-work-pulse 1.5s linear infinite" }}>
        animated
      </text>
      {Array.from({ length: ANIM_ROWS }, (_, row) => (
        <div key={row} style={{ display: "flex", flexDirection: "row", gap: 4, padding: 2 }}>
          {Array.from({ length: 4 }, (_, cell) => (
            <div key={cell} style={{ borderWidth: 1, borderColor: "#cccccc", padding: 2 }}>
              <text style={{ fontSize: 11, color: "#333333" }}>{`r${row} c${cell}`}</text>
            </div>
          ))}
        </div>
      ))}
    </div>
  )
}

describeNative("a keyframes animation of one text on a page of 3000 nodes", { timeout: TIMEOUT }, () => {
  it.skipIf(!retained || verifying)("builds the text and its parent spine each frame", async () => {
    const root = createTestRoot({ width: 1240, height: 780 })
    const renderer = root.renderer
    try {
      renderer.clockPause()
      root.render(<AnimatedPage />)
      await settle(renderer, 6)
      renderer.setInlineDraws(false)
      renderer.setA11yActive(false)
      for (let warm = 0; warm < 5; warm++) {
        renderer.clockFastForward(16)
        renderer.drawFrame()
      }
      for (let frame = 0; frame < 10; frame++) {
        const done = work(renderer, () => renderer.clockFastForward(16))
        expect(done.elementsBuilt, `frame ${frame}`).toBeLessThanOrEqual(3)
        expect(done.builds, `frame ${frame}`).toBeLessThanOrEqual(3)
        expect(done.prepainted, `frame ${frame}`).toBeLessThanOrEqual(20)
        expect(done.measures, `frame ${frame}`).toBe(0)
        expect(done.shapes, `frame ${frame}`).toBe(0)
      }
    } finally {
      root.unmount()
    }
  })
})

// ── #58: an inherited color above labels ────────────────────────────

const LABELS = 30
const LABEL_HEIGHT = 28

function RecolorPage({ color, width, tick = 0 }: { color: string; width: number; tick?: number }) {
  return (
    <div style={{ width, flexDirection: "column", color }}>
      <div style={{ width: 20 + tick, height: 4, backgroundColor: "#0000ff" }} />
      {Array.from({ length: LABELS }, (_, index) => (
        <div key={index} style={{ height: LABEL_HEIGHT, padding: 6, display: "flex" }}>
          <text style={{ fontSize: 14, width: "50%" }}>{`Item number ${index} label`}</text>
          <div style={{ display: "inline-grid" }}>
            {[600, 400].map((weight) => (
              <text key={weight} style={{ gridColumnStart: 1, gridRowStart: 1, fontSize: 14, fontWeight: weight, opacity: 0 }}>{`Item number ${index} label`}</text>
            ))}
            <text style={{ gridColumnStart: 1, gridRowStart: 1, fontSize: 14 }}>{`Item number ${index} label`}</text>
          </div>
        </div>
      ))}
    </div>
  )
}

describeNative("an inherited color changed above 30 labels", { timeout: TIMEOUT }, () => {
  it("measures and shapes nothing, and a re-measure of the labels after idle frames shapes nothing", async () => {
    const root = createTestRoot({ width: 300, height: LABELS * LABEL_HEIGHT })
    const renderer = root.renderer
    try {
      root.render(<RecolorPage color="#111111" width={260} />)
      await settle(renderer)
      renderer.setInlineDraws(false)
      renderer.setA11yActive(false)
      for (let warm = 0; warm < 5; warm++) renderer.drawFrame()

      for (let frame = 0; frame < 6; frame++) {
        const done = work(renderer, () => {
          root.render(<RecolorPage color={frame % 2 === 0 ? "#222222" : "#111111"} width={260} />)
          renderer.flush()
        })
        expect(done.measures, `recolor ${frame}`).toBe(0)
        expect(done.shapes, `recolor ${frame}`).toBe(0)
      }

      // The layout cache of text lines holds this frame's and the frame before's: let it forget.
      for (let idle = 0; idle < 10; idle++) {
        work(renderer, () => {
          root.render(<RecolorPage color="#111111" width={260} tick={idle + 1} />)
          renderer.flush()
        })
      }

      const narrower = work(renderer, () => {
        root.render(<RecolorPage color="#111111" width={250} />)
        renderer.flush()
      })
      expect(narrower.measures, "the labels are measured against another width").toBeGreaterThan(0)
      expect(narrower.shapes, "and their lines are not shaped again").toBe(0)
    } finally {
      root.unmount()
    }
  })
})
