// @ts-nocheck
/**
 * What a tick of an animation that only changes what it paints costs a page. One looping text above
 * N/5 rows of 4 bordered cells (the shape of the FF demo page) must make a tick of the same work for
 * N = 500 and N = 5000: the tick builds the text alone and gpui paints it again in the frame it
 * rendered last (`patchedFrames` counts them), which is how Chrome repaints the display items of an
 * animation and keeps the rest. What is patched is the same picture a frame built whole gives.
 *
 * An `<svg>` whose shapes animate (the button spinner of the FF demo) is such a unit too: the `<svg>` is
 * built alone and its raster made again in the patched frame, when the document is small enough to be
 * rasterised in paint. A large one is rasterised by a job and is drawn whole, as before.
 *
 * An element that is lifted into the stacking context around it is a unit too: one with an opacity
 * below 1 (a skeleton pulse, a motion fade), a positioned one, one with a `z-index`, as long as nothing
 * in it is lifted out of it. An element whose opacity is animated is lifted, and paints as a group, for
 * as long as the animation or transition is current, whatever opacity it samples (Chrome's
 * `will-change: opacity` of an animation's side effects): a loop through exactly 1 and exactly 0 is
 * patched at every tick, those included. A loop under a positioned, translucent or lifted ancestor, however
 * many deep, is a unit as one under none is: gpui paints its operations inside the ancestor's. An element
 * that is itself `position: absolute` is placed against a containing block that gpui cannot tell is its
 * parent, so it is drawn whole.
 *
 * What cannot be patched is drawn whole, as before: an animation that moves a box, a large `<svg>`,
 * a tick in which the pointer moved or the tree was committed. Every tick drawn whole while something runs
 * is counted under the reason (`getPatchCounters().unpatchable`), also on a page with no unit.
 *
 * The counters are `getBuildCounters`, `getLayoutCounters`, `getFrameWorkCounters`,
 * `getRetainedCounters` and `getPatchCounters`.
 */

import fs from "fs"
import React, { useState } from "react"
import { describe, expect, it } from "vitest"
import { defineKeyframes, motion } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { verifying } from "./retained-harness.js"
import { SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const retained = process.env.GPUIX_RETAIN_OFF === undefined
const patching = retained && process.env.GPUIX_PATCH_OFF === undefined
const TIMEOUT = 180_000

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

defineKeyframes("afw-pulse", {
  "0%": { color: "#000000" },
  "50%": { color: "#ff0000" },
  "100%": { color: "#000000" },
})
defineKeyframes("afw-spin", {
  "0%": { strokeDasharray: "1 100", strokeDashoffset: 0 },
  "50%": { strokeDasharray: "60 100", strokeDashoffset: -20 },
  "100%": { strokeDasharray: "60 100", strokeDashoffset: -100 },
})
defineKeyframes("afw-breathe", {
  "0%": { opacity: 0.8 },
  "50%": { opacity: 0.3 },
  "100%": { opacity: 0.8 },
})
defineKeyframes("afw-skeleton", {
  "0%": { opacity: 1 },
  "50%": { opacity: 0.4 },
  "100%": { opacity: 1 },
})
defineKeyframes("afw-grow", { from: { width: 40 }, to: { width: 80 } })
defineKeyframes("afw-slide", { from: { top: 0 }, to: { top: 20 } })
defineKeyframes("afw-fade-in", {
  from: { backgroundColor: "rgba(255, 0, 0, 0)" },
  to: { backgroundColor: "rgba(255, 0, 0, 1)" },
})

type Work = Record<string, number>

/** Everything the counters count, as numbers. */
function read(renderer): Work {
  const retainedCounters = renderer.getRetainedCounters()
  const patches = renderer.getPatchCounters()
  const out: Work = {
    elementsBuilt: renderer.getBuildCounters().elementsBuilt,
    unitsBuilt: renderer.getBuildCounters().unitsBuilt,
    patchedFrames: patches.patchedFrames,
    patchedUnits: patches.patchedUnits,
    paintedFrames: renderer.getPaintedFrameCount(),
    hits: retainedCounters.hits,
    replays: retainedCounters.replays,
    builds: retainedCounters.builds,
    layoutFallbacks: retainedCounters.layoutFallbacks,
    layoutSkips: retainedCounters.layoutSkips,
    layoutReplays: retainedCounters.layoutReplays,
    layoutBuilds: retainedCounters.layoutBuilds,
  }
  for (const group of [renderer.getLayoutCounters(), renderer.getFrameWorkCounters()])
    for (const [key, value] of Object.entries(group)) if (typeof value === "number") out[key] = value
  return out
}

/** Runs `change`, draws the frame as the display link does, and returns what it did. */
function work(renderer, change: () => void): Work {
  renderer.resetBuildCounters()
  const before = read(renderer)
  change()
  renderer.drawFrame()
  const after = read(renderer)
  const done: Work = { elementsBuilt: after.elementsBuilt, unitsBuilt: after.unitsBuilt }
  for (const key of Object.keys(after)) if (key !== "elementsBuilt" && key !== "unitsBuilt") done[key] = after[key] - before[key]
  return done
}

/** A tick: the clock moves, without the window being told, and the frame is drawn. */
const tick = (renderer, ms = 16) => work(renderer, () => renderer.clockAdvance(ms))

async function settle(renderer, frames = 12) {
  for (let frame = 0; frame < frames; frame++) {
    renderer.flush()
    renderer.dispatchNativeEvents()
    await sleep(8)
  }
}

const SIZE = { width: 1240, height: 780 }

/** The page: `head` above N/5 rows of 4 bordered cells with a text each. */
function Page({ nodes, head }: { nodes: number; head: React.ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", width: 1200, height: 760, overflow: "hidden" }}>
      {head}
      {Array.from({ length: nodes / 5 }, (_, row) => (
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

const colourHead = (onIteration?: () => void) => (
  <text
    testId="animated"
    onAnimationIteration={onIteration}
    style={{ fontSize: 20, color: "#000000", animation: "afw-pulse 1.5s linear infinite" }}
  >
    animated
  </text>
)

const spinnerHead = (
  <svg testId="spinner" viewBox="0 0 24 24" width={24} height={24}>
    <circle
      cx={12}
      cy={12}
      r={10}
      pathLength={100}
      fill="none"
      stroke="#333333"
      strokeWidth={3}
      style={{ animation: "afw-spin 1.4s linear infinite" }}
    />
  </svg>
)

/** 200 x 200 is past what is rasterised in paint, so a job makes each image and the tick is a frame built whole. */
const largeSpinnerHead = (
  <svg testId="spinner" viewBox="0 0 24 24" width={200} height={200}>
    <circle
      cx={12}
      cy={12}
      r={10}
      pathLength={100}
      fill="none"
      stroke="#333333"
      strokeWidth={3}
      style={{ animation: "afw-spin 1.4s linear infinite" }}
    />
  </svg>
)

const motionHead = (
  <motion.div
    testId="animated"
    style={{ height: 24 }}
    animate={{ backgroundColor: ["#ff0000", "#0000ff", "#ff0000"] }}
    transition={{ duration: 1.5, ease: "linear", repeat: Infinity }}
  >
    <text style={{ fontSize: 20, color: "#222222" }}>animated</text>
  </motion.div>
)

const motionOpacity = (
  opacity: number[],
  duration = 1.5,
  children: React.ReactNode = <text style={{ fontSize: 20, color: "#222222" }}>animated</text>,
) => (
  <motion.div
    testId="animated"
    style={{ height: 24, backgroundColor: "#99ccff" }}
    animate={{ opacity }}
    transition={{ duration, ease: "linear", repeat: Infinity }}
  >
    {children}
  </motion.div>
)

/** Below 1 an element is a stacking context, lifted at 0 into the one around it. Between 0.3 and 0.8 it never leaves that. */
const opacityHead = motionOpacity([0.8, 0.3, 0.8])

/**
 * The loop that passes through 1, where the element is no stacking context: the ticks at its ends are drawn whole.
 * 1.6 s is 100 ticks of 16 ms, so that a tick lands on the instant the opacity is 1.
 */
const opacityDipHead = motionOpacity([1, 0.4, 1], 1.6)

/** What a stacking context takes in is its own: a positioned child with a z-index is painted in it. */
const opacityHoldingHead = motionOpacity(
  [0.8, 0.3, 0.8],
  1.5,
  <div style={{ position: "relative", zIndex: 2, width: 60, height: 12, backgroundColor: "#cc3333" }}>
    <text style={{ fontSize: 10, color: "#222222" }}>inside</text>
  </div>,
)

const breatheHead = (
  <div testId="animated" style={{ height: 24, backgroundColor: "#99ccff", animation: "afw-breathe 1.5s linear infinite" }}>
    <text style={{ fontSize: 20, color: "#222222" }}>animated</text>
  </div>
)

/** The loop through 1 and 0 of native motion. */
const opacityBlinkHead = motionOpacity([1, 0, 1], 1.6)

/** A skeleton placeholder: Tailwind's `animate-pulse` is this, opacity 1 at its ends. */
const skeletonHead = (
  <div style={{ display: "flex", flexDirection: "column", gap: 4, padding: 4 }}>
    <div testId="animated" style={{ height: 16, width: 240, borderRadius: 4, backgroundColor: "#cccccc", animation: "afw-skeleton 1.6s linear infinite" }} />
    <div style={{ height: 16, width: 180, borderRadius: 4, backgroundColor: "#cccccc" }} />
  </div>
)

defineKeyframes("afw-blink", {
  "0%": { opacity: 1 },
  "50%": { opacity: 0 },
  "100%": { opacity: 1 },
})

/** A loop through exactly 1 and exactly 0 (800 ms, the 50th tick of a 1.6 s loop of 16 ms ticks). */
const blinkHead = (
  <div testId="animated" style={{ height: 24, backgroundColor: "#99ccff", animation: "afw-blink 1.6s linear infinite" }}>
    <text style={{ fontSize: 20, color: "#222222" }}>animated</text>
  </div>
)

/** A box that is positioned without an index is lifted at 0 into the context around it, with nothing of its own to keep what is in it. */
const relativeHead = (
  <div testId="animated" style={{ position: "relative", height: 24, animation: "afw-pulse 1.5s linear infinite", color: "#000000" }}>
    <text style={{ fontSize: 20 }}>animated</text>
  </div>
)

const relativeIndexedHead = (
  <div
    testId="animated"
    style={{ position: "relative", zIndex: 3, height: 24, animation: "afw-pulse 1.5s linear infinite", color: "#000000" }}
  >
    <text style={{ fontSize: 20 }}>animated</text>
  </div>
)

/** An absolute badge in a relative card: the card is no unit, the badge is placed outside what it records. */
const badgeHead = (
  <div style={{ position: "relative", height: 40, width: 200, backgroundColor: "#eeeeee" }}>
    <text style={{ fontSize: 16, color: "#222222" }}>card</text>
    <div
      testId="animated"
      style={{
        position: "absolute",
        top: 4,
        right: 4,
        width: 20,
        height: 12,
        borderRadius: 6,
        backgroundColor: "#cc3333",
        animation: "afw-breathe 1.5s linear infinite",
      }}
    />
  </div>
)

const wrapped = (style: React.CSSProperties, child: React.ReactNode) => (
  <div style={{ display: "flex", flexDirection: "row", alignItems: "center", ...style }}>{child}</div>
)

/** The loop of r18 under an ancestor that gpuix lifts: the ancestor is positioned or translucent, the loop is not. */
const underRelative = (child: React.ReactNode) => wrapped({ position: "relative" }, child)
const underAbsolute = (child: React.ReactNode) => wrapped({ position: "absolute", left: 0, top: 0 }, child)
const underOpacity = (child: React.ReactNode) => wrapped({ opacity: 0.5 }, child)
const underNested = (child: React.ReactNode) => underRelative(underOpacity(underRelative(child)))
const selfAbsoluteHead = (
  <text
    testId="animated"
    style={{ fontSize: 14, color: "#000000", position: "absolute", left: 0, top: 0, animation: "afw-pulse 1.5s linear infinite" }}
  >
    animated
  </text>
)

/** The `top-abs` loop of r13: the animation moves an absolute box. */
const topAbsoluteHead = (
  <div style={{ position: "relative", width: 200, height: 40 }}>
    <div
      testId="animated"
      style={{ position: "absolute", left: 0, width: 20, height: 12, backgroundColor: "#336699", animation: "afw-slide 1.5s linear infinite" }}
    />
  </div>
)

const widthHead = (
  <div testId="animated" style={{ height: 20, backgroundColor: "#336699", animation: "afw-grow 1.5s linear infinite" }} />
)

function TransitionHead() {
  const [on, setOn] = useState(false)
  return (
    <div testId="toggle" onClick={() => setOn(true)} style={{ height: 24 }}>
      <text testId="animated" style={{ fontSize: 20, color: on ? "#ff0000" : "#0000ff", transition: "color 1.5s linear" }}>
        transition
      </text>
    </div>
  )
}

/** Fades out when clicked: opacity 1 to 0, at 0 from the first tick on (`steps(1, jump-start)`) for 1.6 s. */
function FadeOutHead() {
  const [on, setOn] = useState(false)
  return (
    <div testId="toggle" onClick={() => setOn(true)} style={{ height: 24 }}>
      <div
        testId="animated"
        style={{ height: 24, backgroundColor: "#99ccff", opacity: on ? 0 : 1, transition: "opacity 1.6s steps(1, jump-start)" }}
      >
        <text style={{ fontSize: 20, color: "#222222" }}>fading</text>
      </div>
    </div>
  )
}

async function mount(head: React.ReactNode, nodes: number, a11y = false) {
  const root = createTestRoot(SIZE)
  const renderer = root.renderer
  renderer.clockPause()
  root.render(<Page nodes={nodes} head={head} />)
  await settle(renderer)
  renderer.setInlineDraws(false)
  renderer.setA11yActive(a11y)
  return { root, renderer }
}

const WARM = 12
const TICKS = 8

/** `TICKS` ticks of a mounted page, after the frames that hand it over to ticks. */
async function ticksOf(head: React.ReactNode, nodes: number, a11y = false) {
  const page = await mount(head, nodes, a11y)
  try {
    for (let warm = 0; warm < WARM; warm++) tick(page.renderer)
    const done: Work[] = []
    for (let frame = 0; frame < TICKS; frame++) done.push(tick(page.renderer))
    if (verifying) expect(page.renderer.getRetainedCounters().mismatches, page.renderer.getRetentionMismatches().join("\n")).toBe(0)
    return done
  } finally {
    page.root.unmount()
  }
}

const shotFile = (name: string) => `${SHOTS_DIR}/animation-frame-work-${process.pid}-${name}.png`

function shot(renderer, name: string): Buffer {
  const file = shotFile(name)
  fs.rmSync(file, { force: true })
  renderer.captureScreenshot(file)
  return fs.readFileSync(file)
}

// A verified frame builds the page a second time, whole: only its mismatches are looked at there.
const counted = it.skipIf(!patching || verifying)

describeNative("a tick of a paint-only animation", { timeout: TIMEOUT }, () => {
  const heads = {
    "a keyframes colour": colourHead(),
    "a native motion colour loop": motionHead,
    "an svg stroke dash loop": spinnerHead,
    "a native motion opacity loop": opacityHead,
    "a native motion opacity loop holding a positioned child": opacityHoldingHead,
    "a keyframes opacity loop": breatheHead,
    "a keyframes colour loop of a relative box": relativeHead,
    "a keyframes colour loop of a relative box with a z-index": relativeIndexedHead,
    "a keyframes colour under a relative box": underRelative(colourHead()),
    "a keyframes colour under an absolute box": underAbsolute(colourHead()),
    "a keyframes colour under a box with opacity below 1": underOpacity(colourHead()),
    "a keyframes colour under a relative box in a group in a relative box": underNested(colourHead()),
    "an svg stroke dash loop under a relative box": underRelative(spinnerHead),
    "an svg stroke dash loop under an absolute box": underAbsolute(spinnerHead),
    "an svg stroke dash loop under a box with opacity below 1": underOpacity(spinnerHead),
    "a native motion colour loop under a relative box": underRelative(motionHead),
    "a native motion opacity loop under a relative box": underRelative(opacityHead),
  }
  for (const [name, head] of Object.entries(heads)) {
    counted(`does the work of one text on any page: ${name}`, async () => {
      const small = await ticksOf(head, 500)
      const large = await ticksOf(head, 5000)
      for (let index = 0; index < TICKS; index++) {
        expect(large[index], `tick ${index}`).toEqual(small[index])
        expect(small[index].patchedFrames, `tick ${index} is patched`).toBe(1)
        expect(small[index].patchedUnits).toBeGreaterThanOrEqual(1)
        expect(small[index].elementsBuilt, `tick ${index}`).toBeLessThanOrEqual(3)
        expect(small[index].prepainted, `tick ${index}`).toBeLessThanOrEqual(10)
        expect(small[index].hits + small[index].replays + small[index].builds, `tick ${index}`).toBe(0)
        expect(small[index].nodesCreated + small[index].measures + small[index].shapes, `tick ${index}`).toBe(0)
      }
      console.info(`[animation-frame-work] ${name}: ${JSON.stringify(small[TICKS - 1])}`)
    })
  }

  counted("does the same with a screen reader connected", async () => {
    const small = await ticksOf(colourHead(), 500, true)
    const large = await ticksOf(colourHead(), 5000, true)
    for (let index = 0; index < TICKS; index++) {
      expect(large[index], `tick ${index}`).toEqual(small[index])
      expect(small[index].patchedFrames, `tick ${index} is patched`).toBe(1)
    }
  })

  const pictures = {
    "a keyframes colour": colourHead(),
    "a native motion colour loop": motionHead,
    "an svg stroke dash loop": spinnerHead,
    "a native motion opacity loop": opacityHead,
    "a native motion opacity loop holding a positioned child": opacityHoldingHead,
    "a keyframes opacity loop": breatheHead,
    "a keyframes colour loop of a relative box": relativeHead,
    "a keyframes colour loop of a relative box with a z-index": relativeIndexedHead,
    "a keyframes colour under a relative box": underRelative(colourHead()),
    "a keyframes colour under an absolute box": underAbsolute(colourHead()),
    "a keyframes colour under a box with opacity below 1": underOpacity(colourHead()),
    "a keyframes colour under a relative box in a group in a relative box": underNested(colourHead()),
    "an svg stroke dash loop under a relative box": underRelative(spinnerHead),
    "an svg stroke dash loop under an absolute box": underAbsolute(spinnerHead),
    "an svg stroke dash loop under a box with opacity below 1": underOpacity(spinnerHead),
  }
  for (const [name, head] of Object.entries(pictures)) {
    it.skipIf(!patching)(`shows the picture of a frame built whole: ${name}`, async () => {
      const page = await mount(head, 500)
      try {
        const renderer = page.renderer
        for (let warm = 0; warm < WARM; warm++) tick(renderer)
        const before = renderer.getPatchCounters().patchedFrames
        for (let index = 0; index < 21; index++) tick(renderer)
        // Under verification a frame that replayed is built twice, and the ticks are patched all the same.
        expect(renderer.getPatchCounters().patchedFrames - before).toBe(21)
        const patched = shot(renderer, `${name.replace(/\W+/g, "-")}-patched`)
        // The same instant, drawn whole.
        renderer.setPatchEnabled(false)
        tick(renderer, 0)
        const whole = shot(renderer, `${name.replace(/\W+/g, "-")}-whole`)
        expect(patched.equals(whole)).toBe(true)
        // And a later instant differs: the picture moved.
        for (let index = 0; index < 10; index++) tick(renderer)
        expect(shot(renderer, `${name.replace(/\W+/g, "-")}-later`).equals(whole)).toBe(false)
        if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
      } finally {
        page.root.unmount()
      }
    })
  }

  counted("does the work of one text on any page: a colour transition", async () => {
    const run = async (nodes: number) => {
      const root = createTestRoot(SIZE)
      const renderer = root.renderer
      try {
        renderer.clockPause()
        root.render(<Page nodes={nodes} head={<TransitionHead />} />)
        await settle(renderer)
        renderer.setInlineDraws(false)
        renderer.setA11yActive(false)
        for (let warm = 0; warm < 4; warm++) tick(renderer)
        const rect = renderer.getElementBounds(renderer.findByTestId("toggle").id)
        renderer.nativeSimulateClick(rect.x + 4, rect.y + 4)
        renderer.dispatchNativeEvents()
        await settle(renderer, 6)
        renderer.setInlineDraws(false)
        for (let warm = 0; warm < 6; warm++) tick(renderer)
        const done: Work[] = []
        for (let frame = 0; frame < TICKS; frame++) done.push(tick(renderer))
        return done
      } finally {
        root.unmount()
      }
    }
    const small = await run(500)
    const large = await run(5000)
    for (let index = 0; index < TICKS; index++) {
      expect(large[index], `tick ${index}`).toEqual(small[index])
      expect(small[index].patchedFrames, `tick ${index} is patched`).toBe(1)
    }
  })

  // An element with a current opacity animation is an effect group at every opacity it samples, and a unit is lifted
  // in the frame that recorded it or it is not: the ticks at opacity 1 and at opacity 0 are patched like the others.
  const loops = {
    "a native motion opacity loop through 1": opacityDipHead,
    "a keyframes skeleton pulse through 1": skeletonHead,
    "a native motion opacity loop through 1 and 0": opacityBlinkHead,
    "a keyframes opacity loop through 1 and 0": blinkHead,
  }
  const LOOP_TICKS = 200
  // 1.6 s loops of 16 ms ticks, 12 of them before the first counted: opacity is 1 at the 88th and the 188th tick, and 0
  // at the 38th and the 138th of the loops through 0.
  const EXACT = [38, 88, 138, 188]
  for (const [name, head] of Object.entries(loops)) {
    counted(`patches every tick of a loop through opacity 1 and 0, those ticks included: ${name}`, async () => {
      const run = async (nodes: number) => {
        const page = await mount(head, nodes)
        try {
          for (let warm = 0; warm < WARM; warm++) tick(page.renderer)
          const before = page.renderer.getPatchCounters()
          const done: Work[] = []
          for (let frame = 0; frame < LOOP_TICKS; frame++) done.push(tick(page.renderer))
          const after = page.renderer.getPatchCounters()
          expect(after.patchedFrames - before.patchedFrames, "a patched frame for every tick").toBe(LOOP_TICKS)
          expect(after.declines, "no tick is declined by gpui").toBe(before.declines)
          expect(after.unpatchable, "no tick is left unpatched by gpuix").toBe(before.unpatchable)
          return done
        } finally {
          page.root.unmount()
        }
      }
      const small = await run(500)
      const large = await run(5000)
      for (let index = 0; index < LOOP_TICKS; index++) {
        expect(small[index].patchedFrames, `tick ${index} is patched`).toBe(1)
        expect(small[index].elementsBuilt, `tick ${index}`).toBeLessThanOrEqual(3)
        expect(large[index], `tick ${index}`).toEqual(small[index])
      }
      expect(EXACT.every((index) => small[index].patchedFrames === 1)).toBe(true)
    })

    it.skipIf(!patching)(`shows the picture of a frame built whole at every instant around opacity 1 and 0: ${name}`, async () => {
      const page = await mount(head, 500)
      try {
        const renderer = page.renderer
        const file = name.replace(/\W+/g, "-")
        for (let warm = 0; warm < WARM; warm++) tick(renderer)
        for (let step = 0; step < 200; step++) {
          tick(renderer)
          if (step % 25 !== 0 && !EXACT.some((exact) => Math.abs(step - exact) <= 2)) continue
          const patched = shot(renderer, `${file}-patched`)
          renderer.setPatchEnabled(false)
          tick(renderer, 0)
          const whole = shot(renderer, `${file}-whole`)
          renderer.setPatchEnabled(true)
          expect(patched.equals(whole), `tick ${step}`).toBe(true)
        }
        if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
      } finally {
        page.root.unmount()
      }
    })
  }

  counted("patches the ticks of an opacity transition that sits at exactly 0 while it runs, and shows what a whole frame does", async () => {
    const run = async (nodes: number) => {
      const root = createTestRoot(SIZE)
      const renderer = root.renderer
      try {
        renderer.clockPause()
        root.render(<Page nodes={nodes} head={<FadeOutHead />} />)
        await settle(renderer)
        renderer.setInlineDraws(false)
        renderer.setA11yActive(false)
        for (let warm = 0; warm < 4; warm++) tick(renderer)
        const rect = renderer.getElementBounds(renderer.findByTestId("toggle").id)
        renderer.nativeSimulateClick(rect.x + 4, rect.y + 4)
        renderer.dispatchNativeEvents()
        await settle(renderer, 6)
        renderer.setInlineDraws(false)
        // The commit and the frame that starts the transition are drawn whole. The 1.6 s run is at 0 from its first tick.
        for (let warm = 0; warm < 4; warm++) tick(renderer)
        const done: Work[] = []
        for (let frame = 0; frame < 80; frame++) done.push(tick(renderer))
        const patched = shot(renderer, `fade-out-${nodes}-patched`)
        renderer.setPatchEnabled(false)
        tick(renderer, 0)
        expect(patched.equals(shot(renderer, `fade-out-${nodes}-whole`)), "the picture of a frame built whole").toBe(true)
        renderer.setPatchEnabled(true)
        return done
      } finally {
        root.unmount()
      }
    }
    const small = await run(500)
    const large = await run(5000)
    for (let index = 0; index < small.length; index++) {
      expect(small[index].patchedFrames, `tick ${index} is patched`).toBe(1)
      expect(large[index], `tick ${index}`).toEqual(small[index])
    }
  })

  // gpuix puts `position: absolute` on gpui as absolute in an ancestor, whose layout gpui rewrites against a box the
  // unit does not hold: an element that is itself absolute is no unit.
  counted("draws the opacity loop of an absolute badge in a relative card whole, as before", async () => {
    const done = await ticksOf(badgeHead, 500)
    for (let index = 0; index < TICKS; index++) {
      expect(done[index].patchedFrames, `tick ${index}`).toBe(0)
      expect(done[index].elementsBuilt, `tick ${index}`).toBeGreaterThan(0)
    }
  })

  counted("does not patch an animation that moves a box", async () => {
    const done = await ticksOf(widthHead, 500)
    for (let index = 0; index < TICKS; index++) {
      expect(done[index].patchedFrames, `tick ${index}`).toBe(0)
      expect(done[index].elementsBuilt, `tick ${index}`).toBeGreaterThan(0)
    }
  })

  counted("draws a large svg whole, which is rasterised by a job and shows the image of the tick before", async () => {
    const small = await ticksOf(largeSpinnerHead, 500)
    for (let index = 0; index < TICKS; index++) {
      expect(small[index].patchedFrames, `tick ${index}`).toBe(0)
      expect(small[index].elementsBuilt, `tick ${index}`).toBeGreaterThan(0)
    }
  })

  it.skipIf(!patching)("shows a large svg as a frame built whole does, once its rasters have landed", async () => {
    const page = await mount(largeSpinnerHead, 500)
    try {
      const renderer = page.renderer
      for (let step = 0; step < 24; step++) {
        tick(renderer)
        await settle(renderer, 2)
        renderer.setInlineDraws(false)
      }
      expect(renderer.getPatchCounters().patchedFrames).toBe(0)
      await settle(renderer, 6)
      renderer.setInlineDraws(false)
      tick(renderer, 0)
      await settle(renderer, 6)
      renderer.setInlineDraws(false)
      const drawn = shot(renderer, "large-svg-drawn")
      renderer.setPatchEnabled(false)
      tick(renderer, 0)
      await settle(renderer, 6)
      renderer.setInlineDraws(false)
      expect(drawn.equals(shot(renderer, "large-svg-whole"))).toBe(true)
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      page.root.unmount()
    }
  })
})

describeNative("a tick that cannot be patched", { timeout: TIMEOUT }, () => {
  it.skipIf(!patching || verifying)("is a frame built whole, and patching resumes after a hover", async () => {
    const page = await mount(colourHead(), 500)
    try {
      const renderer = page.renderer
      for (let warm = 0; warm < WARM; warm++) tick(renderer)
      expect(tick(renderer).patchedFrames).toBe(1)
      const hovered = work(renderer, () => {
        renderer.clockAdvance(16)
        renderer.dispatchMouseMove(400, 300)
        renderer.dispatchNativeEvents()
        renderer.flush()
      })
      expect(hovered.patchedFrames, "the hover frame is built whole").toBe(0)
      expect(hovered.elementsBuilt).toBeGreaterThan(0)
      for (let index = 0; index < 3; index++) tick(renderer)
      for (let index = 0; index < 4; index++) expect(tick(renderer).patchedFrames, `after the hover, tick ${index}`).toBe(1)
    } finally {
      page.root.unmount()
    }
  })

  it.skipIf(!patching || verifying)("is a frame built whole when React commits, and patching resumes after it", async () => {
    let commit: (count: number) => void = () => {}
    function Counter() {
      const [count, setCount] = useState(0)
      commit = setCount
      return <Page nodes={500} head={<>{colourHead()}<text testId="count">{`count ${count}`}</text></>} />
    }
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.clockPause()
      root.render(<Counter />)
      await settle(renderer)
      renderer.setInlineDraws(false)
      renderer.setA11yActive(false)
      for (let warm = 0; warm < WARM; warm++) tick(renderer)
      expect(tick(renderer).patchedFrames).toBe(1)
      commit(1)
      await sleep(20)
      const committed = work(renderer, () => {
        renderer.clockAdvance(16)
        renderer.dispatchNativeEvents()
        renderer.flush()
      })
      expect(committed.patchedFrames).toBe(0)
      expect(committed.elementsBuilt).toBeGreaterThan(0)
      for (let index = 0; index < 3; index++) tick(renderer)
      for (let index = 0; index < 4; index++) expect(tick(renderer).patchedFrames, `after the commit, tick ${index}`).toBe(1)
      const text = renderer.findByTestId("count")
      expect(renderer.getElementBounds(text.id).width).toBeGreaterThan(0)
    } finally {
      root.unmount()
    }
  })

  it.skipIf(!patching || verifying)("is drawn whole when patching is off, and the same work as the build of any tick", async () => {
    const page = await mount(colourHead(), 500)
    try {
      const renderer = page.renderer
      for (let warm = 0; warm < WARM; warm++) tick(renderer)
      renderer.setPatchEnabled(false)
      for (let index = 0; index < 3; index++) tick(renderer)
      const off = tick(renderer)
      expect(off.patchedFrames).toBe(0)
      expect(off.elementsBuilt).toBeGreaterThan(0)
      renderer.setPatchEnabled(true)
      for (let index = 0; index < 3; index++) tick(renderer)
      expect(tick(renderer).patchedFrames).toBe(1)
    } finally {
      page.root.unmount()
    }
  })
})

/** The counts of `after` over `before`, of the reasons that gained. */
function gained(after: string, before: string): Record<string, number> {
  const was = JSON.parse(before) as Record<string, number>
  const out: Record<string, number> = {}
  for (const [reason, count] of Object.entries(JSON.parse(after) as Record<string, number>)) {
    if (count > (was[reason] ?? 0)) out[reason] = count - (was[reason] ?? 0)
  }
  return out
}

/** What `TICKS` ticks of a page after its warm-up did to the patch counters. */
async function patchesOf(head: React.ReactNode, nodes = 500) {
  const page = await mount(head, nodes)
  try {
    const renderer = page.renderer
    for (let warm = 0; warm < WARM; warm++) tick(renderer)
    const before = renderer.getPatchCounters()
    for (let frame = 0; frame < TICKS; frame++) tick(renderer)
    const after = renderer.getPatchCounters()
    return {
      patched: after.patchedFrames - before.patchedFrames,
      declines: gained(after.declines, before.declines),
      unpatchable: gained(after.unpatchable, before.unpatchable),
    }
  } finally {
    page.root.unmount()
  }
}

describeNative("a loop under a lifted ancestor", { timeout: TIMEOUT }, () => {
  const under = {
    "relative": underRelative,
    "absolute": underAbsolute,
    "opacity below 1": underOpacity,
    "a relative box in a group in a relative box": underNested,
  }
  for (const [name, around] of Object.entries(under)) {
    for (const nodes of [500, 5000]) {
      counted(`patches every tick, with no decline of either kind: a text, an svg, ${name}, ${nodes} nodes`, async () => {
        for (const head of [colourHead(), spinnerHead]) {
          const done = await patchesOf(around(head), nodes)
          expect(done).toEqual({ patched: TICKS, declines: {}, unpatchable: {} })
        }
      })
    }
  }
})

describeNative("a tick drawn whole says why", { timeout: TIMEOUT }, () => {
  const pulse = "afw-pulse 1.5s linear infinite"
  const children = (count: number) => Array.from({ length: count }, (_, index) => <div key={index} style={{ width: 2, height: 2 }} />)
  const reasons: Record<string, [React.ReactNode, string]> = {
    "an animation that moves a box": [widthHead, "LayoutAnimation"],
    "an animation that moves an absolute box": [topAbsoluteHead, "LayoutAnimation"],
    "an element that is itself absolute": [selfAbsoluteHead, "Escapes"],
    "an absolute badge in a relative card": [badgeHead, "Escapes"],
    "a scroller that runs a loop": [
      <div testId="animated" style={{ overflow: "scroll", width: 100, height: 24, color: "#000000", animation: pulse }}>
        <text style={{ fontSize: 12 }}>scroller</text>
      </div>,
      "Excluded:scroller",
    ],
    "a loop on an element that holds a scroller": [
      <div testId="animated" style={{ height: 24, color: "#000000", animation: pulse }}>
        <div style={{ overflow: "scroll", width: 100, height: 20 }}>
          <text style={{ fontSize: 12 }}>scroller</text>
        </div>
      </div>,
      "ExcludedBelow:scroller",
    ],
    "a loop on an element that lifts a box out of it": [
      <div testId="animated" style={{ height: 24, color: "#000000", animation: pulse }}>
        <div style={{ position: "relative", zIndex: 2, width: 20, height: 12, backgroundColor: "#cc3333" }} />
      </div>,
      "Lifts",
    ],
    "a loop on an element with more elements than a unit holds": [
      <div testId="animated" style={{ display: "flex", flexDirection: "row", width: 600, height: 24, color: "#000000", animation: pulse }}>
        {children(300)}
      </div>,
      "TooLarge",
    ],
  }
  for (const [name, [head, reason]] of Object.entries(reasons)) {
    counted(`counts every tick under ${reason}, on a page with no unit: ${name}`, async () => {
      const done = await patchesOf(head)
      expect(done.patched).toBe(0)
      expect(done.declines).toEqual({})
      expect(done.unpatchable).toEqual({ [reason]: TICKS })
    })
  }

  counted("counts nothing for a page that patches", async () => {
    const done = await patchesOf(colourHead())
    expect(done).toEqual({ patched: TICKS, declines: {}, unpatchable: {} })
  })
})

describeNative("a loop that is patched", { timeout: TIMEOUT }, () => {
  it("fires animationiteration once per iteration and reads currentTime of the instant of each tick", async () => {
    const seen: number[] = []
    let now = 0
    const page = await mount(colourHead(() => seen.push(now)), 500)
    try {
      const renderer = page.renderer
      const label = renderer.findByTestId("animated").id
      for (let warm = 0; warm < WARM; warm++) tick(renderer)
      const start = renderer.getAnimations(label)[0].currentTime
      seen.length = 0
      const patched = renderer.getPatchCounters().patchedFrames
      // 1.5 s loops, 16 ms ticks: iterations at 1500, 3000, 4500 ms of the animation's time.
      for (let step = 1; step <= 300; step++) {
        now = start + step * 16
        tick(renderer)
        renderer.dispatchNativeEvents()
        expect(renderer.getAnimations(label)[0].currentTime, `tick ${step}`).toBeCloseTo(now, 3)
      }
      expect(seen.length).toBe(Math.floor(now / 1500) - Math.floor(start / 1500))
      for (const at of seen) expect(at % 1500, "an iteration lands in the tick that passes its boundary").toBeLessThan(16)
      if (patching) expect(renderer.getPatchCounters().patchedFrames - patched).toBe(300)
    } finally {
      page.root.unmount()
    }
  })
})

describeNative("a patched tick on a live clock", { timeout: TIMEOUT }, () => {
  it.skipIf(!patching || verifying)("keeps drawing the loop in frames of its own", async () => {
    const page = await mount(colourHead(), 5000)
    try {
      const renderer = page.renderer
      renderer.clockResume()
      for (let warm = 0; warm < 10; warm++) {
        await sleep(16)
        renderer.drawFrame()
      }
      renderer.resetBuildCounters()
      const before = read(renderer)
      for (let frame = 0; frame < 30; frame++) {
        await sleep(16)
        renderer.drawFrame()
      }
      const after = read(renderer)
      expect(after.patchedFrames - before.patchedFrames, "frames are drawn by patching").toBeGreaterThan(10)
      expect(after.paintedFrames - before.paintedFrames, "and by nothing else").toBe(0)
      expect(renderer.getBuildCounters().elementsBuilt).toBeLessThanOrEqual(3 * 30)
    } finally {
      page.root.unmount()
    }
  })
})
