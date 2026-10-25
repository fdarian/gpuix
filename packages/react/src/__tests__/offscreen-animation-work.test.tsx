// @ts-nocheck
/**
 * What a page pays for animations nobody sees. A sidebar of hover rows beside a main area of N cards
 * whose label and swatch loop a keyframes animation of colours (or a native `motion` of opacity) must
 * make a frame of the same work for N = 200 as for N = 0 when the cards are below the window or clipped
 * out of an `overflow: hidden` scroller: a paint-only animation of an element that is culled is
 * dormant, so the roots above it are replayed, and its clock is carried on without a build. Chrome
 * paints no frame for such content, and a card that shows again is built with the clock of that frame.
 * An animation that moves a box (`width`) is not dormant and is built as it always was. An `<svg>` is a
 * box like another for it: it clips to its viewport, so a stroke loop on its shapes is dormant when the
 * `<svg>` is culled, whatever its size.
 *
 * The counters are `getBuildCounters`, `getLayoutCounters`, `getFrameWorkCounters` and
 * `getPaintedFrameCount`. The recordings that a frame replays are reported, not asserted: they scale
 * with the number of cards.
 */

import fs from "fs"
import React, { useEffect, useState } from "react"
import { afterEach, describe, expect, it } from "vitest"
import { animate, defineKeyframes, motion, useMotionValue, useSpring } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { verifying } from "./retained-harness.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const retained = process.env.GPUIX_RETAIN_OFF === undefined
const TIMEOUT = 120_000

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

const PULSE = {
  "0%": { color: "#ff0000", backgroundColor: "#ff0000" },
  "100%": { color: "#0000ff", backgroundColor: "#0000ff" },
}
defineKeyframes("offscreen-pulse", PULSE)
defineKeyframes("offscreen-grow", { from: { width: 40 }, to: { width: 80 } })
defineKeyframes("offscreen-dash", {
  "0%": { strokeDasharray: "1 100", strokeDashoffset: 0 },
  "50%": { strokeDasharray: "60 100", strokeDashoffset: -20 },
  "100%": { strokeDasharray: "60 100", strokeDashoffset: -100 },
})

/** A path of about 8 KB of markup, so that its document is past what is rasterised in paint at rest. */
const FILLER = Array.from({ length: 400 }, (_, dot) => `M${(dot % 20) + 2} ${Math.floor(dot / 20) + 2}h0.5v0.5h-0.5z`).join(" ")

type Work = Record<string, number>

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
  "builds",
  "paintedFrames",
] as const

function read(renderer): Work {
  return {
    elementsBuilt: renderer.getBuildCounters().elementsBuilt,
    ...renderer.getLayoutCounters(),
    ...renderer.getFrameWorkCounters(),
    replays: renderer.getRetainedCounters().replays,
    builds: renderer.getRetainedCounters().builds,
    paintedFrames: renderer.getPaintedFrameCount(),
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

const SCROLLER = 400
/** The space before the cards: past the clip, with room for the pixel and the outset that culling leaves around it. */
const BEFORE = SCROLLER + 40
const CARD = 128

type Kind = "colour" | "motion" | "width" | "svg" | "bigsvg"
type Place = "below" | "hidden" | "auto"

/** `onIteration` is given to the animated label, which the page reports by card. */
function Card({ index, kind, onIteration }: { index: number; kind: Kind; onIteration?: (index: number) => void }) {
  const shared = { fontSize: 14, color: "#ff0000" }
  const animation = kind === "width" ? "offscreen-grow 1s linear infinite" : "offscreen-pulse 1s linear infinite"
  if (kind === "svg" || kind === "bigsvg") {
    // More than 4096 bytes of document is past what a document at rest has rasterised in paint.
    const filler = kind === "svg" ? "" : FILLER
    return (
      <div
        testId={`card-${index}`}
        style={{ height: 120, flexShrink: 0, padding: 8, marginBottom: 8, backgroundColor: "#f4f4f4", borderRadius: 8, flexDirection: "column", gap: 4 }}
      >
        <text style={{ fontSize: 14, color: "#222222" }}>{`Card title ${index}`}</text>
        <svg testId={`svg-${index}`} viewBox="0 0 24 24" width={24} height={24} style={{ flexShrink: 0 }}>
          {filler ? <path d={filler} fill="#aabbcc" /> : null}
          <circle
            testId={`circle-${index}`}
            cx={12}
            cy={12}
            r={10}
            pathLength={100}
            fill="none"
            stroke="#333333"
            strokeWidth={3}
            onAnimationIteration={onIteration ? () => onIteration(index) : undefined}
            style={{ animation: "offscreen-dash 1s linear infinite" }}
          />
        </svg>
      </div>
    )
  }
  return (
    <div
      testId={`card-${index}`}
      style={{ height: 120, flexShrink: 0, padding: 8, marginBottom: 8, backgroundColor: "#f4f4f4", borderRadius: 8, flexDirection: "column", gap: 4 }}
    >
      {kind === "motion" ? (
        <motion.div
          testId={`label-${index}`}
          animate={{ opacity: [1, 0.25, 1] }}
          transition={{ duration: 1, ease: "linear", repeat: Infinity }}
        >
          <text style={{ fontSize: 14, color: "#222222" }}>{`Card title ${index}`}</text>
        </motion.div>
      ) : (
        <text testId={`label-${index}`} onAnimationIteration={onIteration ? () => onIteration(index) : undefined} style={{ ...shared, animation }}>
          {`Card title ${index}`}
        </text>
      )}
      <text style={{ fontSize: 12, color: "#666666" }}>{`Description line of card ${index}`}</text>
      <div
        testId={`swatch-${index}`}
        style={{ width: kind === "width" ? undefined : 60, height: 18, borderRadius: 9, backgroundColor: "#ff0000", animation }}
      />
    </div>
  )
}

function SidebarPage({ cards, kind, place, onIteration }: { cards: number; kind: Kind; place: Place; onIteration?: (index: number) => void }) {
  const content = Array.from({ length: cards }, (_, index) => <Card key={index} index={index} kind={kind} onIteration={onIteration} />)
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
        <div testId="scroller" style={{ flex: 1, height: SCROLLER, overflow: place, flexDirection: "column" }}>
          <div style={{ height: BEFORE, flexShrink: 0 }} />
          {content}
        </div>
      )}
    </div>
  )
}

async function mount(props: { cards: number; kind: Kind; place: Place; onIteration?: (index: number) => void }) {
  const root = createTestRoot({ width: SIZE.width, height: SIZE.height })
  const renderer = root.renderer
  renderer.clockPause()
  root.render(<SidebarPage {...props} />)
  await settle(renderer)
  renderer.setInlineDraws(false)
  renderer.setA11yActive(false)
  return { root, renderer }
}

/** A tick of an idle page: the clock moves and the frame is drawn. */
const tick = (renderer) => work(renderer, () => renderer.clockFastForward(16))

/** A hover frame: the pointer enters another row of the sidebar. */
function hoverer(renderer) {
  let moves = 0
  return () =>
    work(renderer, () => {
      const row = (moves * 7 + 1) % 58
      moves++
      renderer.dispatchMouseMove(20, 4 + row * ROW + ROW / 2)
      renderer.dispatchNativeEvents()
      renderer.flush()
    })
}

const TICKS = 6

async function frames(props: { cards: number; kind: Kind; place: Place }, run: (renderer) => Work) {
  const page = await mount(props)
  try {
    for (let warm = 0; warm < 12; warm++) run(page.renderer)
    const done: Work[] = []
    for (let frame = 0; frame < TICKS; frame++) done.push(run(page.renderer))
    if (verifying) expect(page.renderer.getRetainedCounters().mismatches, page.renderer.getRetentionMismatches().join("\n")).toBe(0)
    return done
  } finally {
    page.root.unmount()
  }
}

const mean = (done: Work[], key: string) => done.reduce((sum, frame) => sum + frame[key], 0) / done.length

describeNative("a frame on a page with animations nobody sees", { timeout: TIMEOUT }, () => {
  const cases = []
  for (const kind of ["colour", "motion", "svg", "bigsvg"] as const) for (const place of ["below", "hidden"] as const) cases.push({ kind, place })

  for (const { kind, place } of cases) {
    const name = `${kind} loops, ${place === "below" ? "below the window" : "clipped out of an overflow hidden scroller"}`

    it.skipIf(!retained)(`an idle tick does the work of an empty main area for 200 cards: ${name}`, async () => {
      const empty = await frames({ cards: 0, kind, place }, tick)
      const full = await frames({ cards: 200, kind, place }, tick)
      if (verifying) return
      for (let frame = 0; frame < TICKS; frame++) expect(pick(full[frame]), `tick ${frame}`).toEqual(pick(empty[frame]))
      console.info(
        `[offscreen-animation-work] tick, ${name}: replays ${mean(empty, "replays")} -> ${mean(full, "replays")}, ` +
          `built ${mean(empty, "elementsBuilt")} -> ${mean(full, "elementsBuilt")}, ` +
          `prepainted ${mean(empty, "prepainted")} -> ${mean(full, "prepainted")}, ` +
          `painted primitives ${mean(empty, "paintedPrimitives")} -> ${mean(full, "paintedPrimitives")}, ` +
          `painted frames ${mean(empty, "paintedFrames")} -> ${mean(full, "paintedFrames")}`,
      )
    })

    it.skipIf(!retained)(`a hover frame over the sidebar does the work of an empty main area for 200 cards: ${name}`, async () => {
      const hover = async (cards: number) => {
        const page = await mount({ cards, kind, place })
        try {
          // One hoverer per page, so the pointer keeps moving between rows.
          const move = hoverer(page.renderer)
          for (let warm = 0; warm < 15; warm++) move()
          const done: Work[] = []
          for (let frame = 0; frame < TICKS; frame++) {
            page.renderer.clockFastForward(16)
            done.push(move())
          }
          if (verifying) expect(page.renderer.getRetainedCounters().mismatches, page.renderer.getRetentionMismatches().join("\n")).toBe(0)
          return done
        } finally {
          page.root.unmount()
        }
      }
      const empty = await hover(0)
      const full = await hover(200)
      if (verifying) return
      for (let frame = 0; frame < TICKS; frame++) expect(pick(full[frame]), `hover ${frame}`).toEqual(pick(empty[frame]))
    })
  }

  it.skipIf(!retained || verifying)("builds and paints the cards each tick when retention is off, which is what the equality above rules out", async () => {
    const off = async (cards: number) => {
      const page = await mount({ cards, kind: "colour", place: "hidden" })
      try {
        page.renderer.setRetainEnabled(false)
        for (let warm = 0; warm < 3; warm++) tick(page.renderer)
        return tick(page.renderer)
      } finally {
        page.root.unmount()
      }
    }
    const none = await off(0)
    const full = await off(200)
    expect(full.elementsBuilt).toBeGreaterThan(none.elementsBuilt + 600)
  })

  it.skipIf(!retained || verifying)("builds the cards each tick when their loop moves a box: a width animation is not dormant", async () => {
    const full = await frames({ cards: 200, kind: "width", place: "hidden" }, tick)
    expect(mean(full, "elementsBuilt")).toBeGreaterThan(400)
  })
})

describeNative("a loop that is culled", { timeout: TIMEOUT }, () => {
  it("fires animationiteration at the right times and reads currentTime of the instant of each tick", async () => {
    const seen: Array<[number, number]> = []
    let now = 0
    const page = await mount({ cards: 40, kind: "colour", place: "hidden", onIteration: (index) => seen.push([index, now]) })
    try {
      const renderer = page.renderer
      const label = renderer.findByTestId("label-30").id
      for (let warm = 0; warm < 5; warm++) tick(renderer)
      const first = renderer.getAnimations(label)
      expect(first.length).toBe(1)
      const start = first[0].currentTime
      seen.length = 0
      // 1 s loops: iterations come at 1000, 2000 and 3000 ms of the animation's time, 16 ms ticks.
      for (let step = 1; step <= 200; step++) {
        now = start + step * 16
        tick(renderer)
        renderer.dispatchNativeEvents()
        expect(renderer.getAnimations(label)[0].currentTime, `tick ${step}`).toBeCloseTo(now, 3)
      }
      const mine = seen.filter(([index]) => index === 30)
      expect(mine.length).toBe(Math.floor(now / 1000) - Math.floor(start / 1000))
      // Every iteration lands in the tick that passes its boundary.
      for (const [, at] of mine) expect(at % 1000).toBeLessThan(16)
      if (!verifying) expect(seen.length).toBe(40 * mine.length)
    } finally {
      page.root.unmount()
    }
  })

  /** Loops of different lengths, one finite, one with a delay: the boundaries of each fall on other frames. */
  const LOOPS = ["700ms linear infinite", "1300ms linear 2", "2s linear 500ms infinite", "600ms linear 3", "5s linear infinite"]

  function TimedPage({ shown, onEvent }: { shown: boolean; onEvent: (event: string) => void }) {
    return (
      <div style={{ width: SIZE.width, height: SIZE.height, flexDirection: "column" }}>
        {shown ? null : <div style={{ height: SIZE.height + BEFORE, flexShrink: 0 }} />}
        {LOOPS.map((loop, index) => (
          <text
            key={index}
            testId={`timed-${index}`}
            onAnimationIteration={(event) => onEvent(`iteration:${index}:${event.elapsedTime}`)}
            onAnimationEnd={(event) => onEvent(`end:${index}:${event.elapsedTime}`)}
            style={{ fontSize: 14, color: "#ff0000", animation: `offscreen-pulse ${loop}` }}
          >{`timed ${index}`}</text>
        ))}
      </div>
    )
  }

  it("reports the iterations and ends of loops that are not due for a while at the ticks that pass their boundaries, as when they are shown", async () => {
    await compareTimedEvents(TimedPage)
  })

  /** The same loops on the shapes of an `<svg>`, which is a box that clips like another. */
  function TimedSvgPage({ shown, onEvent }: { shown: boolean; onEvent: (event: string) => void }) {
    return (
      <div style={{ width: SIZE.width, height: SIZE.height, flexDirection: "column" }}>
        {shown ? null : <div style={{ height: SIZE.height + BEFORE, flexShrink: 0 }} />}
        <svg testId="timed-svg" viewBox="0 0 24 24" width={24} height={24} style={{ flexShrink: 0 }}>
          {LOOPS.map((loop, index) => (
            <circle
              key={index}
              testId={`timed-${index}`}
              cx={12}
              cy={12}
              r={2 + index * 2}
              fill="none"
              stroke="#ff0000"
              pathLength={100}
              onAnimationIteration={(event) => onEvent(`iteration:${index}:${event.elapsedTime}`)}
              onAnimationEnd={(event) => onEvent(`end:${index}:${event.elapsedTime}`)}
              style={{ animation: `offscreen-dash ${loop}` }}
            />
          ))}
        </svg>
      </div>
    )
  }

  it("reports the iterations and ends of loops on the shapes of a culled svg at the ticks that pass their boundaries, as when it is shown", async () => {
    await compareTimedEvents(TimedSvgPage)
  })

  async function compareTimedEvents(Page: typeof TimedPage) {
    const run = async (shown: boolean) => {
      const reported: Array<[number, string]> = []
      let step = 0
      const root = createTestRoot({ width: SIZE.width, height: SIZE.height })
      const renderer = root.renderer
      try {
        renderer.clockPause()
        root.render(<Page shown={shown} onEvent={(event) => reported.push([step, event])} />)
        await settle(renderer)
        renderer.setInlineDraws(false)
        renderer.setA11yActive(false)
        reported.length = 0
        const offset = renderer.getAnimations(renderer.findByTestId("timed-0").id)[0].currentTime
        for (step = 1; step <= 400; step++) {
          tick(renderer)
          renderer.dispatchNativeEvents()
        }
        if (retained && !verifying) {
          const built = work(renderer, () => renderer.clockFastForward(16)).elementsBuilt
          expect(shown ? built > 0 : built, shown ? "the shown loops are built" : "the culled loops are not").toBe(shown ? true : 0)
        }
        return { reported, offset }
      } finally {
        root.unmount()
      }
    }
    const culled = await run(false)
    const shown = await run(true)
    // 6.4 s: five boundaries of the 700 ms loop, a finish of the 1300 ms and the 600 ms ones, ...
    expect(shown.reported.filter(([, event]) => event.startsWith("end")).length).toBe(2)
    expect(shown.reported.length).toBeGreaterThan(12)
    expect(culled.reported).toEqual(shown.reported)
  }

  it.skipIf(!retained || verifying)("builds the loops again when their keyframes are redefined to move a box, and leaves them be when they paint again", async () => {
    const page = await mount({ cards: 40, kind: "colour", place: "hidden" })
    try {
      const renderer = page.renderer
      for (let warm = 0; warm < 8; warm++) tick(renderer)
      expect(tick(renderer).elementsBuilt, "dormant").toBe(0)
      defineKeyframes("offscreen-pulse", { from: { width: 40 }, to: { width: 80 } })
      renderer.flush()
      for (let step = 0; step < 3; step++) tick(renderer)
      expect(tick(renderer).elementsBuilt, "a loop that moves a box is built in every tick").toBeGreaterThan(40)
      defineKeyframes("offscreen-pulse", PULSE)
      renderer.flush()
      for (let step = 0; step < 4; step++) tick(renderer)
      expect(tick(renderer).elementsBuilt, "dormant again").toBe(0)
    } finally {
      defineKeyframes("offscreen-pulse", PULSE)
      page.root.unmount()
    }
  })

  for (const kind of ["colour", "svg", "bigsvg"] as const)
  it.skipIf(!retained || verifying)(`is carried on by a tick that builds and draws nothing: ${kind}`, async () => {
    const seen: number[] = []
    const page = await mount({ cards: 40, kind, place: "hidden", onIteration: (index) => index === 7 && seen.push(index) })
    try {
      const renderer = page.renderer
      const label = renderer.findByTestId(kind === "colour" ? "label-7" : "circle-7").id
      for (let warm = 0; warm < 12; warm++) elapse(renderer)
      renderer.resetBuildCounters()
      const before = read(renderer)
      const time = renderer.getAnimations(label)[0].currentTime
      for (let step = 0; step < 144; step++) elapse(renderer)
      const after = read(renderer)
      expect(after.elementsBuilt, "nothing is built").toBe(0)
      expect(after.paintedFrames - before.paintedFrames, "nothing is painted").toBe(0)
      expect(after.prepainted - before.prepainted).toBe(0)
      expect(renderer.getAnimations(label)[0].currentTime - time, "the clock of the animation moves").toBeGreaterThan(2000)
      expect(seen.length, "an iteration at every second").toBeGreaterThanOrEqual(2)
      expect(after.dormantTicks - before.dormantTicks, "a tick for each iteration").toBeLessThanOrEqual(seen.length + 2)
    } finally {
      page.root.unmount()
    }
  })

  it.skipIf(!retained)("shows the stroke of the current instant when a card with an svg is scrolled into view", async () => {
    const page = await mount({ cards: 40, kind: "svg", place: "auto" })
    const shot = (name: string) => {
      const file = `${SHOTS_DIR}/offscreen-animation-${process.pid}-${name}.png`
      fs.rmSync(file, { force: true })
      page.renderer.captureScreenshot(file)
      return readLogicalPixels(file, page.renderer)
    }
    try {
      const renderer = page.renderer
      for (let step = 0; step < 44; step++) tick(renderer)
      const scroller = renderer.findByTestId("scroller").id
      renderer.scrollTo(scroller, 0, BEFORE + 20 * CARD, "instant")
      renderer.flush()
      renderer.drawFrame()
      const box = renderer.getElementBounds(renderer.findByTestId("svg-20").id)
      expect(box.y).toBeLessThan(SIZE.height)
      const grid = (pixels) => {
        const out = []
        for (let y = 0; y < 24; y += 2) for (let x = 0; x < 24; x += 2) out.push(pixels.at(Math.round(box.x) + x, Math.round(box.y) + y))
        return out
      }
      const shown = grid(shot("svg-shown"))
      // The ring is drawn: some of the grid is not the card's grey.
      expect(shown.filter(([r]) => r < 120).length).toBeGreaterThan(3)
      if (!verifying) {
        renderer.setRetainEnabled(false)
        renderer.flush()
        expect(grid(shot("svg-built"))).toEqual(shown)
      }
    } finally {
      page.root.unmount()
    }
  })

  it.skipIf(!retained)("shows the colour of the current instant when a card is scrolled into view", async () => {
    const page = await mount({ cards: 40, kind: "colour", place: "auto" })
    const shot = (name: string) => {
      const file = `${SHOTS_DIR}/offscreen-animation-${process.pid}-${name}.png`
      fs.rmSync(file, { force: true })
      page.renderer.captureScreenshot(file)
      return readLogicalPixels(file, page.renderer)
    }
    try {
      const renderer = page.renderer
      for (let step = 0; step < 44; step++) tick(renderer)
      const scroller = renderer.findByTestId("scroller").id
      // Bring card 20 to the top of the scroller.
      renderer.scrollTo(scroller, 0, BEFORE + 20 * CARD, "instant")
      renderer.flush()
      renderer.drawFrame()
      const swatch = renderer.getElementBounds(renderer.findByTestId("swatch-20").id)
      const at = [swatch.x + 30, swatch.y + 9]
      expect(swatch.y).toBeLessThan(SIZE.height)
      const [r, , b] = shot("shown").at(...at)
      // 44 ticks of 16 ms: 704 ms of a 1 s red to blue loop, not the red of a card that was never built.
      expect(r).toBeLessThan(150)
      expect(b).toBeGreaterThan(100)
      if (!verifying) {
        renderer.setRetainEnabled(false)
        renderer.flush()
        expect(shot("built").at(...at)).toEqual(shot("shown").at(...at))
      }
    } finally {
      page.root.unmount()
    }
  })
})

// ── Native motion that nobody sees ──────────────────────────────────

const LOOP_SPACE = 8
const TINTS = ["#ff0000", "#0000ff", "#ff0000"]

/** An opacity loop and a background-colour loop, 1 s each. */
function LoopCard({ index, onComplete }: { index: number; onComplete?: (index: number) => void }) {
  return (
    <div
      testId={`loop-card-${index}`}
      style={{ height: 120, flexShrink: 0, padding: LOOP_SPACE, marginBottom: 8, backgroundColor: "#f4f4f4", borderRadius: 8, flexDirection: "column", gap: 4 }}
    >
      <motion.div
        testId={`fade-${index}`}
        style={{ height: 30 }}
        animate={{ opacity: [1, 0.25, 1] }}
        transition={{ duration: 1, ease: "linear", repeat: Infinity }}
        onMotionComplete={() => onComplete?.(index)}
      >
        <text style={{ fontSize: 14, color: "#222222" }}>{`Loop card ${index}`}</text>
      </motion.div>
      <motion.div
        testId={`tint-${index}`}
        style={{ width: 100, height: 30, backgroundColor: "#ff0000" }}
        animate={{ backgroundColor: TINTS }}
        transition={{ duration: 1, ease: "linear", repeat: Infinity }}
      />
    </div>
  )
}

/** A tween that ends 500 ms in, listened to for its end. */
function FiniteCard({ index, onComplete }: { index: number; onComplete: (index: number) => void }) {
  return (
    <div
      testId={`finite-card-${index}`}
      style={{ height: 120, flexShrink: 0, padding: LOOP_SPACE, marginBottom: 8, backgroundColor: "#f4f4f4", borderRadius: 8, flexDirection: "column" }}
    >
      <motion.div
        testId={`finite-${index}`}
        style={{ height: 30 }}
        animate={{ opacity: [1, 0.25] }}
        transition={{ duration: 0.5, ease: "linear" }}
        onMotionComplete={() => onComplete(index)}
      >
        <text style={{ fontSize: 14, color: "#222222" }}>{`Finite card ${index}`}</text>
      </motion.div>
    </div>
  )
}

type MotionPlace = "hidden" | "auto" | "none"

function MotionPage({ cards, place, card }: { cards: number; place: MotionPlace; card: (index: number) => React.ReactNode }) {
  const content = Array.from({ length: cards }, (_, index) => <React.Fragment key={index}>{card(index)}</React.Fragment>)
  return (
    <div style={{ width: SIZE.width, height: SIZE.height, flexDirection: "column" }}>
      {place === "none" ? (
        <div style={{ display: "none", flexDirection: "column" }}>{content}</div>
      ) : (
        <div testId="scroller" style={{ width: 400, height: SCROLLER, overflow: place, flexDirection: "column" }}>
          <div style={{ height: BEFORE, flexShrink: 0 }} />
          {content}
        </div>
      )}
    </div>
  )
}

async function mountMotion(props: { cards: number; place: MotionPlace; card: (index: number) => React.ReactNode }) {
  const root = createTestRoot({ width: SIZE.width, height: SIZE.height })
  const renderer = root.renderer
  renderer.clockPause()
  root.render(<MotionPage {...props} />)
  await settle(renderer)
  renderer.setInlineDraws(false)
  renderer.setA11yActive(false)
  return { root, renderer }
}

/** Time passes as it does for a window: both clocks move, the frame callbacks and a pending change are drawn, events are delivered. */
function elapse(renderer, ms = 16) {
  renderer.clockAdvance(ms)
  renderer.advanceTime(ms)
  renderer.drawFrame()
  renderer.dispatchNativeEvents()
}

/** What `steps` ticks of 16 ms did, after `warm` that are not counted. */
function spent(renderer, steps: number, warm = 12) {
  for (let step = 0; step < warm; step++) elapse(renderer)
  renderer.resetBuildCounters()
  const before = read(renderer)
  const patchedBefore = renderer.getPatchCounters().patchedFrames
  for (let step = 0; step < steps; step++) elapse(renderer)
  const after = read(renderer)
  return {
    dormantTicks: after.dormantTicks - before.dormantTicks,
    paintedFrames: after.paintedFrames - before.paintedFrames,
    patchedFrames: renderer.getPatchCounters().patchedFrames - patchedBefore,
    elementsBuilt: after.elementsBuilt,
    motionsSynced: renderer.getBuildCounters().motionsSynced,
  }
}

describeNative("a native motion loop that nobody sees", { timeout: TIMEOUT }, () => {
  const loop = (index: number) => <LoopCard index={index} />

  for (const place of ["hidden", "none"] as const) {
    const where = place === "hidden" ? "clipped out of an overflow hidden scroller" : "inside a display none subtree"

    it.skipIf(!retained || verifying)(`costs a page what an empty one costs over 60 ticks: ${where}`, async () => {
      const run = async (cards: number) => {
        const page = await mountMotion({ cards, place, card: loop })
        try {
          return spent(page.renderer, 60)
        } finally {
          page.root.unmount()
        }
      }
      const empty = await run(0)
      const full = await run(40)
      console.info(`[offscreen-animation-work] 60 ticks of 40 culled motion loops, ${where}: ${JSON.stringify(empty)} -> ${JSON.stringify(full)}`)
      expect(full.dormantTicks, "dormant ticks").toBe(0)
      expect(full).toEqual(empty)
    })
  }

  it.skipIf(!retained || verifying)("reports the end of a finite animation at its end time, with no dormant tick for each frame before it", async () => {
    const done: Array<[number, number]> = []
    let step = 0
    const page = await mountMotion({
      cards: 20,
      place: "hidden",
      card: (index) => <FiniteCard index={index} onComplete={(which) => done.push([which, step])} />,
    })
    try {
      const renderer = page.renderer
      done.length = 0
      renderer.resetBuildCounters()
      const before = read(renderer)
      // 500 ms tween: it ends in the tick that passes 500 ms.
      for (step = 1; step <= 48; step++) elapse(renderer)
      const ticks = read(renderer).dormantTicks - before.dormantTicks
      expect(done.length, "one end for each card").toBe(20)
      expect(new Set(done.map(([, at]) => at)), "all in the tick that passes 500 ms").toEqual(new Set([Math.ceil(500 / 16)]))
      expect(new Set(done.map(([which]) => which)).size).toBe(20)
      expect(ticks, "dormant ticks").toBeLessThanOrEqual(2)
    } finally {
      page.root.unmount()
    }
  })

  it.skipIf(!retained || verifying)("pauses, plays and finishes the animation of a motion nobody sees", async () => {
    const done: number[] = []
    const page = await mountMotion({
      cards: 12,
      place: "hidden",
      card: (index) => <FiniteCard index={index} onComplete={(which) => done.push(which)} />,
    })
    try {
      const renderer = page.renderer
      const element = renderer.findByTestId("finite-11").id
      for (let step = 0; step < 5; step++) elapse(renderer)
      const [listed] = renderer.getAnimations(element)
      expect(listed.kind).toBe("script")
      renderer.pauseAnimation(listed.id)
      for (let step = 0; step < 4; step++) elapse(renderer)
      const held = renderer.getAnimation(listed.id)
      expect(held.playState).toBe("paused")
      expect(held.pending).toBe(false)
      for (let step = 0; step < 60; step++) elapse(renderer)
      expect(renderer.getAnimation(listed.id).currentTime, "a pause holds the time").toBe(held.currentTime)
      expect(done.includes(11), "a paused animation completes nothing").toBe(false)

      renderer.playAnimation(listed.id)
      for (let step = 0; step < 40; step++) elapse(renderer)
      expect(done.includes(11), "the rest of the animation plays out").toBe(true)
      expect(renderer.getAnimations(element)).toEqual([])
    } finally {
      page.root.unmount()
    }
  })

  it.skipIf(!retained)("resumes when scrolled into view, and shows the colour of the current instant", async () => {
    const page = await mountMotion({ cards: 40, place: "auto", card: loop })
    const shot = (name: string) => {
      const file = `${SHOTS_DIR}/offscreen-animation-${process.pid}-${name}.png`
      fs.rmSync(file, { force: true })
      page.renderer.captureScreenshot(file)
      return readLogicalPixels(file, page.renderer)
    }
    try {
      const renderer = page.renderer
      for (let step = 0; step < 44; step++) elapse(renderer)
      const scroller = renderer.findByTestId("scroller").id
      renderer.scrollTo(scroller, 0, BEFORE + 20 * CARD, "instant")
      renderer.flush()
      renderer.drawFrame()
      const tint = renderer.getElementBounds(renderer.findByTestId("tint-20").id)
      const at = [tint.x + 50, tint.y + 15]
      expect(tint.y).toBeLessThan(SIZE.height)
      // 704 ms of a 1 s red, blue, red loop: 0.41 of the way back to red.
      const [r, , b] = shot("loop-shown").at(...at)
      expect(r).toBeLessThan(150)
      expect(b).toBeGreaterThan(100)
      if (!verifying) {
        renderer.setRetainEnabled(false)
        renderer.flush()
        expect(shot("loop-built").at(...at)).toEqual(shot("loop-shown").at(...at))
        renderer.setRetainEnabled(true)
        renderer.flush()
      }
      const painted = renderer.getPaintedFrameCount()
      const patched = renderer.getPatchCounters().patchedFrames
      for (let step = 0; step < 4; step++) elapse(renderer)
      expect(renderer.getPaintedFrameCount() + renderer.getPatchCounters().patchedFrames - painted - patched, "a frame for each tick of the loop in view").toBeGreaterThanOrEqual(4)
      // 768 ms: redder than 704.
      const [later] = shot("loop-later").at(...at)
      expect(later).toBeGreaterThan(r + 10)
    } finally {
      page.root.unmount()
    }
  })
})

// ── CSS animations, transitions and motion values that nobody sees ──

/** What `steps` ticks of 16 ms did to a page with 40 culled cards, after the warm-up, as `spent` counts. */
async function spentOn(props: { cards: number; kind: Kind; place: Place; onIteration?: (index: number) => void }, steps: number) {
  const page = await mount(props)
  try {
    return spent(page.renderer, steps)
  } finally {
    page.root.unmount()
  }
}

describeNative("a CSS loop that nobody sees", { timeout: TIMEOUT }, () => {
  for (const kind of ["colour", "svg", "bigsvg"] as const)
    it.skipIf(!retained || verifying)(`costs a page what an empty one costs over 60 ticks when nothing listens to it: ${kind}`, async () => {
      const empty = await spentOn({ cards: 0, kind, place: "hidden" }, 60)
      const full = await spentOn({ cards: 40, kind, place: "hidden" }, 60)
      console.info(`[offscreen-animation-work] 60 ticks of 40 culled CSS loops, ${kind}: ${JSON.stringify(empty)} -> ${JSON.stringify(full)}`)
      expect(full.dormantTicks, "dormant ticks").toBe(0)
      expect(full).toEqual(empty)
    })

  for (const kind of ["colour", "svg"] as const)
    it.skipIf(!retained || verifying)(`delivers every iteration at its time, in about one dormant tick for each: ${kind}`, async () => {
      const seen: Array<[number, number]> = []
      let step = 0
      const page = await mount({ cards: 40, kind, place: "hidden", onIteration: (index) => seen.push([index, step]) })
      try {
        const renderer = page.renderer
        for (step = -12; step < 0; step++) elapse(renderer)
        seen.length = 0
        const before = read(renderer)
        // 1 s loops that began 192 ms before the count: iterations come at 1000, 2000 and 3000 ms of their time.
        for (step = 1; step <= 200; step++) elapse(renderer)
        const ticks = read(renderer).dormantTicks - before.dormantTicks
        const boundaries = [1000, 2000, 3000].map((ms) => Math.ceil((ms - 12 * 16) / 16))
        expect(new Set(seen.map(([, at]) => at)), "iterations land in the tick that passes each boundary").toEqual(new Set(boundaries))
        expect(seen.length, "three for each card").toBe(40 * 3)
        expect(new Set(seen.map(([index]) => index)).size).toBe(40)
        expect(ticks, "dormant ticks").toBeLessThanOrEqual(3 + 2)
        console.info(`[offscreen-animation-work] 200 ticks of 40 culled CSS loops with an iteration listener, ${kind}: ${ticks} dormant ticks`)
      } finally {
        page.root.unmount()
      }
    })

  /** A colour animation of 500 ms that ends once, listened to for its end. */
  function EndingCard({ index, onEnd }: { index: number; onEnd: (index: number) => void }) {
    return (
      <div testId={`ending-card-${index}`} style={{ height: 120, flexShrink: 0, padding: LOOP_SPACE, marginBottom: 8, backgroundColor: "#f4f4f4", borderRadius: 8, flexDirection: "column" }}>
        <text testId={`ending-${index}`} onAnimationEnd={() => onEnd(index)} style={{ fontSize: 14, color: "#ff0000", animation: "offscreen-pulse 500ms linear forwards" }}>
          {`Ending card ${index}`}
        </text>
      </div>
    )
  }

  it.skipIf(!retained || verifying)("fires animationend at its end time, in at most two dormant ticks", async () => {
    const done: Array<[number, number]> = []
    let step = 0
    const page = await mountMotion({ cards: 20, place: "hidden", card: (index) => <EndingCard index={index} onEnd={(which) => done.push([which, step])} /> })
    try {
      const renderer = page.renderer
      renderer.resetBuildCounters()
      const before = read(renderer)
      for (step = 1; step <= 48; step++) elapse(renderer)
      const ticks = read(renderer).dormantTicks - before.dormantTicks
      expect(done.length, "one end for each card").toBe(20)
      expect(new Set(done.map(([, at]) => at)), "all in the tick that passes 500 ms").toEqual(new Set([Math.ceil(500 / 16)]))
      expect(ticks, "dormant ticks").toBeLessThanOrEqual(2)
      console.info(`[offscreen-animation-work] 48 ticks of 20 culled CSS animations of 500 ms: ${ticks} dormant ticks`)
    } finally {
      page.root.unmount()
    }
  })

  /** A colour that turns blue when `flip` is called, over 500 ms. */
  function TransitionCards({ cards, expose, onEnd }: { cards: number; expose: (flip: () => void) => void; onEnd: (index: number) => void }) {
    const [on, setOn] = useState(false)
    useEffect(() => {
      expose(() => setOn(true))
    }, [])
    return (
      <MotionPage
        cards={cards}
        place="hidden"
        card={(index) => (
          <div testId={`turning-card-${index}`} style={{ height: 120, flexShrink: 0, padding: LOOP_SPACE, marginBottom: 8, backgroundColor: "#f4f4f4", borderRadius: 8, flexDirection: "column" }}>
            <text
              testId={`turning-${index}`}
              onTransitionEnd={() => onEnd(index)}
              style={{ fontSize: 14, color: on ? "#0000ff" : "#ff0000", transition: "color 500ms linear" }}
            >{`Turning card ${index}`}</text>
          </div>
        )}
      />
    )
  }

  it.skipIf(!retained || verifying)("fires transitionend at its end time, in at most two dormant ticks", async () => {
    const done: Array<[number, number]> = []
    let step = 0
    let flip = () => {}
    const root = createTestRoot({ width: SIZE.width, height: SIZE.height })
    const renderer = root.renderer
    try {
      renderer.clockPause()
      root.render(<TransitionCards cards={20} expose={(next) => (flip = next)} onEnd={(which) => done.push([which, step])} />)
      await settle(renderer)
      renderer.setInlineDraws(false)
      renderer.setA11yActive(false)
      flip()
      await settle(renderer)
      renderer.drawFrame()
      expect(renderer.getAnimations(renderer.findByTestId("turning-3").id).length, "the transition runs").toBe(1)
      done.length = 0
      const before = read(renderer)
      for (step = 1; step <= 48; step++) elapse(renderer)
      const ticks = read(renderer).dormantTicks - before.dormantTicks
      expect(done.length, "one end for each card").toBe(20)
      expect(new Set(done.map(([, at]) => at)), "all in the tick that passes 500 ms").toEqual(new Set([Math.ceil(500 / 16)]))
      expect(ticks, "dormant ticks").toBeLessThanOrEqual(2)
      console.info(`[offscreen-animation-work] 48 ticks of 20 culled CSS transitions of 500 ms: ${ticks} dormant ticks`)
    } finally {
      root.unmount()
    }
  })
})

describeNative("a motion value bound to an element that nobody sees", { timeout: TIMEOUT }, () => {
  const changes: string[] = []

  /** A text whose colour is a value that loops, and that reports its changes when `listen`. */
  function BoundCard({ listen }: { listen: boolean }) {
    const value = useMotionValue("#000000")
    useEffect(() => {
      const off = listen ? value.on("change", (latest) => changes.push(latest)) : undefined
      const controls = animate(value, ["#000000", "#ff0000", "#000000"], { duration: 1, ease: "linear", repeat: Infinity })
      return () => {
        off?.()
        controls.stop()
      }
    }, [value])
    return (
      <div style={{ height: 120, flexShrink: 0 }}>
        <text style={{ fontSize: 14, color: value }}>bound</text>
      </div>
    )
  }

  const run = async (cards: number, listen: boolean) => {
    const page = await mountMotion({ cards, place: "hidden", card: () => <BoundCard listen={listen} /> })
    try {
      changes.length = 0
      const done = spent(page.renderer, 60)
      return { ...done, changes: changes.length }
    } finally {
      page.root.unmount()
    }
  }

  it.skipIf(!retained || verifying)("costs a page what an empty one costs over 60 ticks when nothing listens to it", async () => {
    const empty = await run(0, false)
    const full = await run(20, false)
    console.info(`[offscreen-animation-work] 60 ticks of 20 culled bound values: ${JSON.stringify(empty)} -> ${JSON.stringify(full)}`)
    expect(full.dormantTicks, "dormant ticks").toBe(0)
    expect(full).toEqual(empty)
  })

  it.skipIf(!retained || verifying)("sends a change for each tick, in a dormant tick of its own, when it is listened to", async () => {
    const full = await run(20, true)
    expect(full.dormantTicks, "a dormant tick for each tick").toBeGreaterThanOrEqual(59)
    expect(full.changes, "a change for each card at each tick").toBeGreaterThanOrEqual(59 * 20)
    expect(full.paintedFrames, "no frame is drawn").toBe(0)
  })
})

// ── Springs that nobody sees ────────────────────────────────────────

type SpringPlace = "culled" | "skipped" | "visible"

const SPRING = { type: "spring", stiffness: 300, damping: 20 } as const
const SPRING_TICKS = 150

/** Cards below the clip of a scroller (`culled`), in content that skips (`skipped`), or in plain view (`visible`). */
function SpringPage({ cards, place, card }: { cards: number; place: SpringPlace; card: (index: number) => React.ReactNode }) {
  const content = Array.from({ length: cards }, (_, index) => <React.Fragment key={index}>{card(index)}</React.Fragment>)
  if (place === "culled") return <MotionPage cards={cards} place="hidden" card={card} />
  return (
    <div style={{ width: SIZE.width, height: SIZE.height, flexDirection: "column" }}>
      {place === "skipped" ? (
        <div style={{ width: 400, height: SCROLLER, flexDirection: "column", contentVisibility: "hidden" } as React.CSSProperties}>{content}</div>
      ) : (
        content
      )}
    </div>
  )
}

/** What a page of springs does over `ticks` ticks of 16 ms: the tick each completion was delivered in, and the dormant ticks. */
async function springRun(place: SpringPlace, card: (index: number, report: (step: number | undefined) => void) => React.ReactNode, cards: number, ticks = SPRING_TICKS, probe?: (renderer) => number) {
  const root = createTestRoot({ width: SIZE.width, height: SIZE.height })
  const renderer = root.renderer
  const done: Array<[number, number]> = []
  let step = 0
  try {
    renderer.clockPause()
    root.render(<SpringPage cards={cards} place={place} card={(index) => card(index, () => done.push([index, step]))} />)
    await settle(renderer)
    renderer.setInlineDraws(false)
    renderer.setA11yActive(false)
    done.length = 0
    renderer.resetBuildCounters()
    const before = read(renderer)
    for (step = 1; step <= ticks; step++) elapse(renderer)
    return { done, dormantTicks: read(renderer).dormantTicks - before.dormantTicks, paintedFrames: read(renderer).paintedFrames - before.paintedFrames, probed: probe?.(renderer) }
  } finally {
    root.unmount()
  }
}

const stepsOf = (done: Array<[number, number]>) => new Set(done.map(([, at]) => at))

describeNative("a native spring that nobody sees", { timeout: TIMEOUT }, () => {
  const card = (index: number, report: () => void) => (
    <div testId={`spring-card-${index}`} style={{ height: 120, flexShrink: 0 }}>
      <motion.div
        testId={`spring-${index}`}
        style={{ height: 30 }}
        initial={{ opacity: 1 }}
        animate={{ opacity: 0.25 }}
        transition={SPRING}
        onMotionComplete={report}
      />
    </div>
  )

  it.skipIf(!retained || verifying)("is visible: completes once for each card", async () => {
    const visible = await springRun("visible", card, 3)
    expect(visible.done.length).toBe(3)
    expect(stepsOf(visible.done).size, "all in one tick").toBe(1)
    console.info(`[offscreen-animation-work] native spring in view completes in tick ${[...stepsOf(visible.done)]}`)
  })

  for (const place of ["culled", "skipped"] as const)
    it.skipIf(!retained || verifying)(`completes in the tick it does in view, in at most two dormant ticks: ${place}`, async () => {
      const visible = await springRun("visible", card, 3)
      const hidden = await springRun(place, card, 20)
      console.info(`[offscreen-animation-work] ${SPRING_TICKS} ticks of 20 ${place} native springs: ${hidden.dormantTicks} dormant ticks, completed in ${[...stepsOf(hidden.done)]} (in view: ${[...stepsOf(visible.done)]})`)
      expect(hidden.done.length, "one completion for each card").toBe(20)
      expect(stepsOf(hidden.done), "the tick of the spring in view").toEqual(stepsOf(visible.done))
      expect(hidden.dormantTicks, "dormant ticks").toBeLessThanOrEqual(2)
      expect(hidden.paintedFrames, "only the frame that applies its end is drawn").toBeLessThanOrEqual(1)
    })
})

describeNative("a spring motion value that nobody sees", { timeout: TIMEOUT }, () => {
  type Listen = "none" | "animation" | "change"
  const values: Array<{ get: () => number }> = []

  /** A box whose opacity is a value that springs to 1 (or follows a source that was set to 1). */
  const card = (listen: Listen, following: boolean) =>
    function SpringValueCard({ index, report }: { index: number; report: () => void }) {
      const source = useMotionValue(0)
      const own = useSpring(following ? source : 0, SPRING)
      useEffect(() => {
        values[index] = own
        const offs = [] as Array<() => void>
        if (listen === "animation") offs.push(own.on("animationComplete", report))
        if (listen === "change") offs.push(own.on("change", () => {}))
        if (following) source.set(1)
        else own.set(1)
        return () => offs.forEach((off) => off())
      }, [])
      return (
        <div style={{ height: 120, flexShrink: 0 }}>
          <div testId={`bound-${index}`} style={{ width: 50, height: 20, backgroundColor: "#ff0000", opacity: own }} />
        </div>
      )
    }

  const scenes = [
    { name: "a spring with its own target", following: false },
    { name: "a follower of a value at rest", following: true },
  ]
  for (const scene of scenes)
    for (const place of ["culled", "skipped"] as const)
      it.skipIf(!retained || verifying)(`completes in the tick it does in view, in at most two dormant ticks: ${scene.name}, ${place}`, async () => {
        const Card = card("animation", scene.following)
        const render = (index: number, report: () => void) => <Card index={index} report={report} />
        const probe = () => values[0].get()
        const visible = await springRun("visible", render, 3, SPRING_TICKS, probe)
        const hidden = await springRun(place, render, 20, SPRING_TICKS, probe)
        console.info(`[offscreen-animation-work] ${SPRING_TICKS} ticks of 20 ${place} springs, ${scene.name}: ${hidden.dormantTicks} dormant ticks, completed in ${[...stepsOf(hidden.done)]} (in view: ${[...stepsOf(visible.done)]})`)
        expect(visible.done.length, "in view").toBe(3)
        expect(hidden.done.length, "one completion for each card").toBe(20)
        expect(stepsOf(hidden.done), "the tick of the spring in view").toEqual(stepsOf(visible.done))
        expect(hidden.dormantTicks, "dormant ticks").toBeLessThanOrEqual(2)
        expect(hidden.probed, "the value after the run").toBe(visible.probed)
      })

  it.skipIf(!retained || verifying)("is not sampled at all when nothing listens to it", async () => {
    const Card = card("none", false)
    const hidden = await springRun("culled", (index, report) => <Card index={index} report={report} />, 20)
    expect(hidden.dormantTicks).toBe(0)
  })

  it.skipIf(!retained || verifying)("sends a change for each tick it moves in when it is listened to", async () => {
    const Card = card("change", false)
    const hidden = await springRun("culled", (index, report) => <Card index={index} report={report} />, 20, 30)
    expect(hidden.dormantTicks, "a dormant tick for each tick").toBeGreaterThanOrEqual(28)
  })

  it.skipIf(!retained || verifying)("shows the value of the current instant when it is revealed mid-run", async () => {
    const Card = card("animation", false)
    const render = (index: number, report: () => void) => <Card index={index} report={report} />
    const probe = () => values[0].get()
    const visible = await springRun("visible", render, 1, 8, probe)
    const hidden = await springRun("culled", render, 1, 8, probe)
    expect(hidden.probed).toBe(visible.probed)
    expect(visible.probed).toBeGreaterThan(0)
    expect(visible.probed).toBeLessThan(1)
  })

  /** A follower of a value that loops: it retargets in every frame it is sampled in, and never completes. */
  function LoopFollowerCard({ index }: { index: number }) {
    const source = useMotionValue(0)
    const own = useSpring(source, SPRING)
    useEffect(() => {
      values[index] = own
      const off = own.on("animationComplete", () => {})
      const controls = animate(source, 1, { duration: 1, ease: "linear", repeat: Infinity, repeatType: "reverse" })
      return () => {
        off()
        controls.stop()
      }
    }, [])
    return (
      <div style={{ height: 120, flexShrink: 0 }}>
        <div testId={`bound-${index}`} style={{ width: 50, height: 20, backgroundColor: "#ff0000", opacity: own }} />
      </div>
    )
  }

  for (const place of ["culled", "skipped"] as const)
    it.skipIf(!retained || verifying)(`reads what it reads in view after the same ticks, and when it is revealed: a follower of a loop, ${place}`, async () => {
      const render = (index: number) => <LoopFollowerCard index={index} />
      const probe = () => values[0].get()
      for (const ticks of [30, 90]) {
        const visible = await springRun("visible", render, 3, ticks, probe)
        const hidden = await springRun(place, render, 20, ticks, probe)
        expect(hidden.probed, `the value after ${ticks} ticks`).toBe(visible.probed)
        expect(hidden.dormantTicks, "a dormant tick for each tick").toBeGreaterThanOrEqual(ticks - 2)
      }
      if (place === "culled") {
        const reveal = (renderer) => {
          renderer.scrollTo(renderer.findByTestId("scroller").id, 0, BEFORE, "instant")
          renderer.flush()
          elapse(renderer)
          return values[0].get()
        }
        const visible = await springRun("visible", render, 3, 31, probe)
        const hidden = await springRun(place, render, 20, 30, reveal)
        expect(hidden.probed, "the value of the tick after it is revealed").toBe(visible.probed)
      }
    })
})
