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
import React from "react"
import { afterEach, describe, expect, it } from "vitest"
import { defineKeyframes, motion } from "../index"
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
            onAnimationIteration={() => onIteration?.(index)}
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
        <text testId={`label-${index}`} onAnimationIteration={() => onIteration?.(index)} style={{ ...shared, animation }}>
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
      renderer.clockResume()
      const label = renderer.findByTestId(kind === "colour" ? "label-7" : "circle-7").id
      for (let warm = 0; warm < 6; warm++) {
        renderer.flush()
        await sleep(8)
      }
      // The frame callbacks of the last full frame hand the page over to the ticks.
      for (let handover = 0; handover < 4; handover++) {
        await sleep(16)
        renderer.drawFrame()
      }
      renderer.resetBuildCounters()
      const before = read(renderer)
      const time = renderer.getAnimations(label)[0].currentTime
      const started = Date.now()
      while (Date.now() - started < 2300) {
        await sleep(16)
        renderer.drawFrame()
        renderer.dispatchNativeEvents()
      }
      const after = read(renderer)
      expect(after.elementsBuilt, "nothing is built").toBe(0)
      expect(after.paintedFrames - before.paintedFrames, "nothing is painted").toBe(0)
      expect(after.prepainted - before.prepainted).toBe(0)
      expect(renderer.getAnimations(label)[0].currentTime - time, "the clock of the animation moves").toBeGreaterThan(2000)
      expect(seen.length, "an iteration at every second").toBeGreaterThanOrEqual(2)
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
