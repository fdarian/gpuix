// @ts-nocheck
/**
 * What a tick of an animation that only changes what it paints costs a page. One looping text above
 * N/5 rows of 4 bordered cells (the shape of the FF demo page) must make a tick of the same work for
 * N = 500 and N = 5000: the tick builds the text alone and gpui paints it again in the frame it
 * rendered last (`patchedFrames` counts them), which is how Chrome repaints the display items of an
 * animation and keeps the rest. What is patched is the same picture a frame built whole gives.
 *
 * An `<svg>` whose shapes animate (the button spinner of the FF demo) is such a unit too: the `<svg>` is
 * built alone and its raster made again in the patched frame. While its shapes run its document is
 * rasterised in the paint that needs it whatever its size, since a job would show every tick the image of the
 * tick before; a large one at rest is still rasterised by a job.
 *
 * An element that is lifted into the stacking context around it is a unit too: one with an opacity
 * below 1 (a skeleton pulse, a motion fade), a positioned one, one with a `z-index`, as long as nothing
 * in it is lifted out of it. An element whose opacity is animated is lifted, and paints as a group, for
 * as long as the animation or transition is current, whatever opacity it samples (Chrome's
 * `will-change: opacity` of an animation's side effects): a loop through exactly 1 and exactly 0 is
 * patched at every tick, those included. A loop under a positioned, translucent or lifted ancestor, however
 * many deep, is a unit as one under none is: gpui paints its operations inside the ancestor's. An element
 * that is itself `position: absolute` is a unit too: the box that places it is outside it, so a frame builds
 * it whole every time and a tick paints it again where that frame laid it out (`position: fixed` is not one).
 *
 * What cannot be patched is drawn whole, as before: an animation that moves a box,
 * a tick in which the pointer moved or the tree was committed. Every tick drawn whole while something runs
 * is counted under the reason (`getPatchCounters().unpatchable`), also on a page with no unit.
 *
 * The counters are `getBuildCounters`, `getLayoutCounters`, `getFrameWorkCounters`,
 * `getRetainedCounters` and `getPatchCounters`.
 */

import fs from "fs"
import React, { useEffect, useState } from "react"
import { describe, expect, it } from "vitest"
import { animate, defineKeyframes, motion, useMotionValue, useSpring, useTransform } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { verifying } from "./retained-harness.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

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
    motionsSynced: renderer.getBuildCounters().motionsSynced,
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
  const done: Work = { elementsBuilt: after.elementsBuilt, unitsBuilt: after.unitsBuilt, motionsSynced: after.motionsSynced }
  for (const key of Object.keys(after)) if (!(key in done)) done[key] = after[key] - before[key]
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

/** The page: `head` above N/5 rows of 4 bordered cells with a text each, and `tail` after them (outside the clip). */
function Page({ nodes, head, tail }: { nodes: number; head: React.ReactNode; tail?: React.ReactNode }) {
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
      {tail}
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

/** 200 x 200 is past what a document at rest has rasterised in paint, but a running one is. */
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

/** A document of more than 4096 bytes in a box of 24 x 24: big by its bytes and not by its box. */
const HEAVY_PATH = Array.from({ length: 400 }, (_, dot) => `M${(dot % 20) + 2} ${Math.floor(dot / 20) + 2}h0.5v0.5h-0.5z`).join(" ")
const heavySpinnerHead = (
  <svg testId="spinner" viewBox="0 0 24 24" width={24} height={24}>
    <path d={HEAVY_PATH} fill="#aabbcc" />
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

const selfAbsoluteSpinnerHead = (
  <svg testId="spinner" viewBox="0 0 24 24" width={24} height={24} style={{ position: "absolute", left: 0, top: 0 }}>
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

/** An absolute element against a containing block that moves: the card is moved by a click. */
function MovingBlockHead() {
  const [moved, setMoved] = useState(false)
  return (
    <div
      testId="toggle"
      onClick={() => setMoved(true)}
      style={{ position: "relative", flexShrink: 0, height: 40, width: 300, marginLeft: moved ? 120 : 0, backgroundColor: "#eeeeee" }}
    >
      <text
        testId="animated"
        style={{ fontSize: 16, color: "#000000", position: "absolute", left: 6, top: 4, animation: "afw-pulse 1.5s linear infinite" }}
      >
        animated
      </text>
    </div>
  )
}

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

async function mount(head: React.ReactNode, nodes: number, a11y = false, tail?: React.ReactNode) {
  const root = createTestRoot(SIZE)
  const renderer = root.renderer
  renderer.clockPause()
  root.render(<Page nodes={nodes} head={head} tail={tail} />)
  await settle(renderer)
  renderer.setInlineDraws(false)
  renderer.setA11yActive(a11y)
  return { root, renderer }
}

const WARM = 12
const TICKS = 8

/** `TICKS` ticks of a mounted page, after the frames that hand it over to ticks. */
async function ticksOf(head: React.ReactNode, nodes: number, a11y = false, tail?: React.ReactNode) {
  const page = await mount(head, nodes, a11y, tail)
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
    "an svg stroke dash loop on a large svg": largeSpinnerHead,
    "an svg stroke dash loop on a document of more than 4 KB": heavySpinnerHead,
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
    "a keyframes colour on an element that is itself absolute": selfAbsoluteHead,
    "an svg stroke dash loop on an svg that is itself absolute": selfAbsoluteSpinnerHead,
    "a keyframes opacity loop on an absolute badge in a relative card": badgeHead,
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
    "an svg stroke dash loop on a large svg": largeSpinnerHead,
    "an svg stroke dash loop on a document of more than 4 KB": heavySpinnerHead,
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
    "a keyframes colour on an element that is itself absolute": selfAbsoluteHead,
    "an svg stroke dash loop on an svg that is itself absolute": selfAbsoluteSpinnerHead,
    "a keyframes opacity loop on an absolute badge in a relative card": badgeHead,
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

  counted("does not patch an animation that moves a box", async () => {
    const done = await ticksOf(widthHead, 500)
    for (let index = 0; index < TICKS; index++) {
      expect(done[index].patchedFrames, `tick ${index}`).toBe(0)
      expect(done[index].elementsBuilt, `tick ${index}`).toBeGreaterThan(0)
    }
  })

  it.skipIf(!patching)("patches every tick of a large svg whose shapes run, which is rasterised in paint", async () => {
    const page = await mount(largeSpinnerHead, 500)
    try {
      const renderer = page.renderer
      for (let warm = 0; warm < WARM; warm++) tick(renderer)
      const before = renderer.getPatchCounters().patchedFrames
      for (let index = 0; index < 20; index++) tick(renderer)
      expect(renderer.getPatchCounters().patchedFrames - before).toBe(20)
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

/** What `TICKS` ticks of a page after its warm-up did to the patch counters. `ready` waits for whatever arrives on its own. */
async function patchesOf(head: React.ReactNode, nodes = 500, ready?: (renderer) => Promise<void>) {
  const page = await mount(head, nodes)
  try {
    const renderer = page.renderer
    await ready?.(renderer)
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

describeNative("a loop on an element that is itself absolute", { timeout: TIMEOUT }, () => {
  const heads = {
    "a text": selfAbsoluteHead,
    "an svg": selfAbsoluteSpinnerHead,
    "a badge in a relative card": badgeHead,
    "a text in a relative box": underRelative(selfAbsoluteHead),
  }
  for (const [name, head] of Object.entries(heads)) {
    for (const nodes of [500, 5000]) {
      counted(`patches every tick, with no decline of either kind: ${name}, ${nodes} nodes`, async () => {
        expect(await patchesOf(head, nodes)).toEqual({ patched: TICKS, declines: {}, unpatchable: {} })
      })
    }
  }

  counted("is placed by its containing block when that moves, and patched there", async () => {
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.clockPause()
      root.render(<Page nodes={500} head={<MovingBlockHead />} />)
      await settle(renderer)
      renderer.setInlineDraws(false)
      renderer.setA11yActive(false)
      for (let warm = 0; warm < WARM; warm++) tick(renderer)
      const label = renderer.findByTestId("animated").id
      const x = renderer.getElementBounds(label).x
      // Reading bounds lays the page out, and the tick after it is drawn whole.
      for (let warm = 0; warm < 3; warm++) tick(renderer)
      expect(tick(renderer).patchedFrames).toBe(1)

      const rect = renderer.getElementBounds(renderer.findByTestId("toggle").id)
      renderer.nativeSimulateClick(rect.x + 150, rect.y + 30)
      renderer.dispatchNativeEvents()
      await settle(renderer, 6)
      renderer.setInlineDraws(false)
      for (let warm = 0; warm < 4; warm++) tick(renderer)
      expect(renderer.getElementBounds(label).x, "the text follows its block").toBeCloseTo(x + 120, 3)
      const before = renderer.getPatchCounters()
      for (let frame = 0; frame < TICKS; frame++) expect(tick(renderer).patchedFrames, `tick ${frame}`).toBe(1)
      const after = renderer.getPatchCounters()
      expect(after.declines).toBe(before.declines)
      expect(after.unpatchable).toBe(before.unpatchable)
      expect(renderer.getElementBounds(label).x).toBeCloseTo(x + 120, 3)
      const patched = shot(renderer, "moved-block-patched")
      renderer.setPatchEnabled(false)
      tick(renderer, 0)
      expect(patched.equals(shot(renderer, "moved-block-whole")), "the picture of a frame built whole").toBe(true)
      renderer.setPatchEnabled(true)
    } finally {
      root.unmount()
    }
  })
})

describeNative("a tick drawn whole says why", { timeout: TIMEOUT }, () => {
  const pulse = "afw-pulse 1.5s linear infinite"
  const children = (count: number) => Array.from({ length: count }, (_, index) => <div key={index} style={{ width: 2, height: 2 }} />)
  const reasons: Record<string, [React.ReactNode, string]> = {
    "an animation that moves a box": [widthHead, "LayoutAnimation"],
    "an animation that moves an absolute box": [topAbsoluteHead, "LayoutAnimation"],
    "an element that is fixed to the viewport": [
      <text testId="animated" style={{ fontSize: 14, color: "#000000", position: "fixed", left: 0, top: 0, animation: pulse }}>
        animated
      </text>,
      "Excluded:fixed",
    ],
    "a loop on an element that holds an absolute box against a block outside it": [
      <div testId="animated" style={{ height: 24, color: "#000000", animation: pulse }}>
        <div style={{ position: "absolute", left: 0, top: 0, width: 20, height: 12, backgroundColor: "#cc3333" }} />
      </div>,
      "Escapes",
    ],
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

const probeStyle = { width: 160, height: 24 }
/** The frames a window drew: whole ones and patched ones. */
const frames = (done: Work) => done.paintedFrames + done.patchedFrames
const withProbe = (probe: React.ReactNode) => (
  <div style={{ display: "flex", flexDirection: "row", gap: 8 }}>
    {colourHead()}
    {probe}
  </div>
)

/** Focuses the probe, and draws the frame that notes the blink and the one after it that makes the input a unit. */
async function focusProbe(renderer) {
  renderer.focusElement(renderer.findByTestId("probe").id)
  renderer.dispatchNativeEvents()
  renderer.drawFrame()
  wholeFrame(renderer)
}

/** A frame built whole: the view is told, as a commit does. */
function wholeFrame(renderer) {
  renderer.flush()
  renderer.drawFrame()
}

// r17: an `<input>` or `<textarea>` anywhere on the page made every tick of a loop elsewhere a frame built whole. An
// idle one paints the same in every tick, a focused one changes the phase of its caret twice a second: neither is
// a reason to draw the page.
describeNative("an input on a page with a loop", { timeout: TIMEOUT }, () => {
  const pages: Record<string, { head: React.ReactNode; tail?: React.ReactNode; focus?: boolean }> = {
    "an input": { head: withProbe(<input testId="probe" style={probeStyle} />) },
    "a textarea": { head: withProbe(<textarea testId="probe" style={probeStyle} />) },
    "a focused input": { head: withProbe(<input testId="probe" style={probeStyle} />), focus: true },
    "an input with a value, selected": { head: withProbe(<input testId="probe" defaultValue="some text" style={probeStyle} />), focus: true },
    "an input outside the clip": { head: colourHead(), tail: <input testId="probe" style={probeStyle} /> },
  }
  const run = async (name: string, nodes: number) => {
    const shape = pages[name]
    const page = await mount(shape.head, nodes, false, shape.tail)
    try {
      const renderer = page.renderer
      if (shape.focus) await focusProbe(renderer)
      for (let warm = 0; warm < WARM; warm++) tick(renderer)
      const before = renderer.getPatchCounters()
      const done: Work[] = []
      for (let frame = 0; frame < TICKS; frame++) done.push(tick(renderer))
      const after = renderer.getPatchCounters()
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
      return {
        done,
        patched: after.patchedFrames - before.patchedFrames,
        declines: gained(after.declines, before.declines),
        unpatchable: gained(after.unpatchable, before.unpatchable),
      }
    } finally {
      page.root.unmount()
    }
  }
  for (const name of Object.keys(pages)) {
    counted(`patches every tick of the loop and does the work of one text on any page: ${name}`, async () => {
      const small = await run(name, 500)
      const large = await run(name, 5000)
      for (const result of [small, large]) {
        expect(result.patched).toBe(TICKS)
        expect(result.declines).toEqual({})
        expect(result.unpatchable).toEqual({})
      }
      for (let index = 0; index < TICKS; index++) {
        expect(large.done[index], `tick ${index}`).toEqual(small.done[index])
        expect(small.done[index].elementsBuilt, `tick ${index}`).toBeLessThanOrEqual(6)
        expect(small.done[index].hits + small.done[index].replays + small.done[index].builds, `tick ${index}`).toBe(0)
      }
    })

    it.skipIf(!patching)(`shows the picture of a frame built whole: ${name}`, async () => {
      const shape = pages[name]
      const page = await mount(shape.head, 500, false, shape.tail)
      try {
        const renderer = page.renderer
        if (shape.focus) await focusProbe(renderer)
        for (let warm = 0; warm < WARM; warm++) tick(renderer)
        for (let step = 0; step < 40; step++) tick(renderer)
        const file = name.replace(/\W+/g, "-")
        const patched = shot(renderer, `${file}-patched`)
        renderer.setPatchEnabled(false)
        tick(renderer, 0)
        const whole = shot(renderer, `${file}-whole`)
        renderer.setPatchEnabled(true)
        expect(patched.equals(whole)).toBe(true)
        if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
      } finally {
        page.root.unmount()
      }
    })
  }
})

const PATCH = ["diff --git a/a.ts b/a.ts", "--- a/a.ts", "+++ b/a.ts", "@@ -1,2 +1,2 @@", "-const a = 1", "+const a = 2", " const b = 3", ""].join("\n")

// A `<code>`, `<markdown>` or `<diff>` is rebuilt by every frame that builds its page and paints the same in a tick: it is no
// reason for a tick of a loop elsewhere to be drawn whole.
describeNative("a code block on a page with a loop", { timeout: TIMEOUT }, () => {
  const probes: Record<string, React.ReactNode> = {
    "a code block": <code testId="probe" code={"const a = 1\nconst b = 2"} language="ts" style={{ width: 240 }} />,
    "a markdown block": <markdown testId="probe" source={"# Title\n\nsome *text* and `code`"} style={{ width: 240 }} />,
    "a diff": <diff testId="probe" patch={PATCH} style={{ width: 300 }} />,
  }
  const run = async (name: string, nodes: number) => {
    const page = await mount(withProbe(probes[name]), nodes)
    try {
      const renderer = page.renderer
      for (let warm = 0; warm < WARM; warm++) tick(renderer)
      const before = renderer.getPatchCounters()
      const done: Work[] = []
      for (let frame = 0; frame < TICKS; frame++) done.push(tick(renderer))
      const after = renderer.getPatchCounters()
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
      return {
        done,
        patched: after.patchedFrames - before.patchedFrames,
        declines: gained(after.declines, before.declines),
        unpatchable: gained(after.unpatchable, before.unpatchable),
      }
    } finally {
      page.root.unmount()
    }
  }
  for (const name of Object.keys(probes)) {
    counted(`patches every tick of the loop and does the work of one text on any page: ${name}`, async () => {
      const small = await run(name, 500)
      const large = await run(name, 5000)
      for (const result of [small, large]) {
        expect(result.patched).toBe(TICKS)
        expect(result.declines).toEqual({})
        expect(result.unpatchable).toEqual({})
      }
      for (let index = 0; index < TICKS; index++) {
        expect(large.done[index], `tick ${index}`).toEqual(small.done[index])
        expect(small.done[index].elementsBuilt, `tick ${index}`).toBeLessThanOrEqual(6)
        expect(small.done[index].hits + small.done[index].replays + small.done[index].builds, `tick ${index}`).toBe(0)
      }
    })

    it.skipIf(!patching)(`shows the picture of a frame built whole: ${name}`, async () => {
      const page = await mount(withProbe(probes[name]), 500)
      try {
        const renderer = page.renderer
        for (let warm = 0; warm < WARM; warm++) tick(renderer)
        for (let step = 0; step < 40; step++) tick(renderer)
        const file = name.replace(/\W+/g, "-")
        const patched = shot(renderer, `${file}-patched`)
        renderer.setPatchEnabled(false)
        tick(renderer, 0)
        const whole = shot(renderer, `${file}-whole`)
        renderer.setPatchEnabled(true)
        expect(patched.equals(whole)).toBe(true)
        if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
      } finally {
        page.root.unmount()
      }
    })
  }
})

describeNative("the caret of a focused input", { timeout: TIMEOUT }, () => {
  const blinking = (value = "") => <input testId="probe" defaultValue={value} style={{ width: 200, height: 24 }} />

  counted("blinks in patched frames that build the input alone, at any size of the page", async () => {
    const run = async (nodes: number) => {
      const page = await mount(blinking("some text"), nodes)
      try {
        const renderer = page.renderer
        await focusProbe(renderer)
        wholeFrame(renderer)
        const before = renderer.getPatchCounters()
        const done: Work[] = []
        for (let phase = 0; phase < 6; phase++) done.push(work(renderer, () => renderer.advanceTime(500)))
        const after = renderer.getPatchCounters()
        expect(after.patchedFrames - before.patchedFrames, "a patched frame for every change of phase").toBe(6)
        expect(after.declines, "no change of phase is declined by gpui").toBe(before.declines)
        expect(after.unpatchable, "no change of phase is left unpatched by gpuix").toBe(before.unpatchable)
        return done
      } finally {
        page.root.unmount()
      }
    }
    const small = await run(500)
    const large = await run(5000)
    for (let phase = 0; phase < 6; phase++) {
      expect(small[phase].patchedFrames, `phase ${phase}`).toBe(1)
      expect(small[phase].elementsBuilt, `phase ${phase} builds the input alone`).toBeLessThanOrEqual(2)
      expect(small[phase].hits + small[phase].replays + small[phase].builds, `phase ${phase}`).toBe(0)
      expect(large[phase], `phase ${phase}`).toEqual(small[phase])
    }
  })

  // Chrome paints a caret again when it blinks, and a loop elsewhere does not blink it: the ticks of the loop leave the
  // input as it was painted, and the tick of the phase builds it once.
  counted("is built by the tick of its phase and by no tick of a loop elsewhere", async () => {
    const run = async (focused: boolean) => {
      const page = await mount(withProbe(blinking("some text")), 500)
      try {
        const renderer = page.renderer
        if (focused) await focusProbe(renderer)
        wholeFrame(renderer)
        wholeFrame(renderer)
        const before = renderer.getPatchCounters()
        const within: Work[] = []
        for (let frame = 0; frame < 20; frame++) within.push(tick(renderer))
        const phases = [work(renderer, () => renderer.advanceTime(500)), work(renderer, () => renderer.advanceTime(500))]
        const after = renderer.getPatchCounters()
        expect(after.patchedFrames - before.patchedFrames, "every tick is patched").toBe(22)
        expect(gained(after.declines, before.declines)).toEqual({})
        expect(gained(after.unpatchable, before.unpatchable)).toEqual({})
        return { within, phases }
      } finally {
        page.root.unmount()
      }
    }
    const idle = await run(false)
    const focused = await run(true)
    for (const [index, done] of focused.within.entries())
      expect(done.elementsBuilt, `tick ${index} builds what an idle input leaves alone`).toBe(idle.within[index].elementsBuilt)
    for (const [index, done] of focused.phases.entries()) {
      const input = done.elementsBuilt - idle.phases[index].elementsBuilt
      expect(input, `the change of phase ${index} builds the input once`).toBeGreaterThan(0)
      expect(input).toBeLessThanOrEqual(6)
    }
  })

  // Nobody sees a caret that is scrolled out of its scroller, or outside the window or a clip: Chrome paints no frame for it. The input
  // still takes keys (it is not culled, which would drop its handlers), and blinks again when it is back.
  counted("asks for no frame while it cannot be seen, takes text there, and blinks again when it is seen", async () => {
    const values: string[] = []
    const field = (
      <div testId="scroller" style={{ width: 300, height: 60, flexShrink: 0, overflow: "auto" }}>
        <input testId="probe" style={{ width: 200, height: 24 }} onChange={(event) => values.push(event.value)} />
        <div style={{ width: 10, height: 600 }} />
      </div>
    )
    const page = await mount(field, 500)
    try {
      const renderer = page.renderer
      const scroller = renderer.findByTestId("scroller").id
      const id = renderer.findByTestId("probe").id
      // The scrollbar that a scroll shows fades out by itself, in frames of its own.
      const scrollTo = (top: number) => {
        renderer.scrollTo(scroller, 0, top)
        wholeFrame(renderer)
        tick(renderer, 2000)
        tick(renderer)
        wholeFrame(renderer)
        wholeFrame(renderer)
      }
      const idle = (what: string) => {
        for (let index = 0; index < 4; index++) expect(frames(work(renderer, () => renderer.advanceTime(500))), `${what}, 500 ms ${index}`).toBe(0)
      }
      await focusProbe(renderer)
      wholeFrame(renderer)
      wholeFrame(renderer)
      expect(frames(work(renderer, () => renderer.advanceTime(500))), "in view").toBe(1)
      scrollTo(300)
      idle("out of view")
      renderer.nativeSimulateKeystrokes(id, "a b")
      renderer.dispatchNativeEvents()
      wholeFrame(renderer)
      wholeFrame(renderer)
      expect(values.at(-1), "text typed while it is out of view").toBe("ab")
      idle("typed in")
      scrollTo(0)
      for (let index = 0; index < 4; index++) expect(frames(work(renderer, () => renderer.advanceTime(500))), `back in view, 500 ms ${index}`).toBe(1)
    } finally {
      page.root.unmount()
    }
  })

  counted("asks for no frame while it is outside the clip of its ancestor", async () => {
    const page = await mount(<div style={{ width: 10, height: 10 }} />, 500, false, <input testId="probe" style={probeStyle} />)
    try {
      const renderer = page.renderer
      await focusProbe(renderer)
      wholeFrame(renderer)
      wholeFrame(renderer)
      for (let index = 0; index < 4; index++) expect(frames(work(renderer, () => renderer.advanceTime(500))), `500 ms ${index}`).toBe(0)
    } finally {
      page.root.unmount()
    }
  })

  it.skipIf(!patching)("shows the picture of a frame built whole across the ticks of a loop and a change of phase", async () => {
    const page = await mount(withProbe(blinking("some text")), 500)
    try {
      const renderer = page.renderer
      await focusProbe(renderer)
      wholeFrame(renderer)
      wholeFrame(renderer)
      for (let phase = 0; phase < 3; phase++) {
        for (let frame = 0; frame < 20; frame++) tick(renderer)
        const patched = shot(renderer, `loop-phase-${phase}-patched`)
        renderer.setPatchEnabled(false)
        tick(renderer, 0)
        const whole = shot(renderer, `loop-phase-${phase}-whole`)
        renderer.setPatchEnabled(true)
        expect(patched.equals(whole), `phase ${phase}`).toBe(true)
        work(renderer, () => renderer.advanceTime(500))
      }
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      page.root.unmount()
    }
  })

  counted("asks for no frame while nothing is focused, and none once it is blurred or the window is not key", async () => {
    const page = await mount(blinking("some text"), 500)
    try {
      const renderer = page.renderer
      const id = renderer.findByTestId("probe").id
      wholeFrame(renderer)
      for (let index = 0; index < 4; index++) expect(work(renderer, () => renderer.advanceTime(500)).paintedFrames, "idle").toBe(0)
      await focusProbe(renderer)
      wholeFrame(renderer)
      wholeFrame(renderer)
      expect(work(renderer, () => renderer.advanceTime(500)).patchedFrames, "focused").toBe(1)
      renderer.setWindowActive(false)
      renderer.dispatchNativeEvents()
      renderer.drawFrame()
      for (let index = 0; index < 4; index++) expect(work(renderer, () => renderer.advanceTime(500)).paintedFrames, "window not key").toBe(0)
      renderer.setWindowActive(true)
      renderer.dispatchNativeEvents()
      renderer.drawFrame()
      wholeFrame(renderer)
      wholeFrame(renderer)
      expect(work(renderer, () => renderer.advanceTime(500)).patchedFrames, "key again").toBe(1)
      renderer.blurElement(id)
      renderer.dispatchNativeEvents()
      renderer.drawFrame()
      for (let index = 0; index < 4; index++) expect(work(renderer, () => renderer.advanceTime(500)).paintedFrames, "blurred").toBe(0)
    } finally {
      page.root.unmount()
    }
  })

  // The caret is painted in both phases (in no colour while it is off), so the patched frame has the operations of the one
  // before and the picture of the one a build of the page makes.
  it.skipIf(!patching)("shows the picture of a frame built whole in both phases, after focus, blur and the window going key", async () => {
    const page = await mount(blinking("some text"), 500)
    try {
      const renderer = page.renderer
      const id = renderer.findByTestId("probe").id
      const same = (name: string) => {
        const patched = shot(renderer, `caret-${name}-patched`)
        renderer.setPatchEnabled(false)
        tick(renderer, 0)
        const whole = shot(renderer, `caret-${name}-whole`)
        renderer.setPatchEnabled(true)
        expect(patched.equals(whole), name).toBe(true)
        return patched
      }
      const idle = same("idle")
      await focusProbe(renderer)
      wholeFrame(renderer)
      wholeFrame(renderer)
      const lit = same("lit")
      expect(lit.equals(idle), "a caret is drawn").toBe(false)
      work(renderer, () => renderer.advanceTime(500))
      const dark = same("dark")
      expect(dark.equals(lit), "the caret went out").toBe(false)
      expect(dark.equals(idle), "and left the picture of an input that has none").toBe(true)
      work(renderer, () => renderer.advanceTime(500))
      expect(same("lit again").equals(lit)).toBe(true)
      renderer.setWindowActive(false)
      renderer.dispatchNativeEvents()
      renderer.drawFrame()
      expect(same("inactive").equals(idle), "no caret in a window that is not key").toBe(true)
      renderer.setWindowActive(true)
      renderer.dispatchNativeEvents()
      renderer.drawFrame()
      wholeFrame(renderer)
      wholeFrame(renderer)
      expect(same("key again").equals(lit)).toBe(true)
      renderer.blurElement(id)
      renderer.dispatchNativeEvents()
      renderer.drawFrame()
      expect(same("blurred").equals(idle)).toBe(true)
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      page.root.unmount()
    }
  })

  // What is typed, selected or scrolled is not retained from the frame before: a frame that replays the page around the input
  // draws it as one that builds everything does.
  it.skipIf(!patching)("draws typing, selection and the caret moving as a frame built whole does", async () => {
    const page = await mount(withProbe(<input testId="probe" style={probeStyle} />), 500)
    try {
      const renderer = page.renderer
      const id = renderer.findByTestId("probe").id
      await focusProbe(renderer)
      const steps = ["h e l l o", "Shift+ArrowLeft Shift+ArrowLeft", "ArrowLeft", "Meta+a", "Backspace", "w o r l d Meta+z", "Meta+Shift+z"]
      let previous: Buffer | undefined
      for (const [index, keys] of steps.entries()) {
        renderer.nativeSimulateKeystrokes(id, keys)
        renderer.dispatchNativeEvents()
        for (let frame = 0; frame < 3; frame++) tick(renderer)
        const retained = shot(renderer, `typing-${index}-retained`)
        renderer.setRetainEnabled(false)
        tick(renderer, 0)
        const whole = shot(renderer, `typing-${index}-whole`)
        renderer.setRetainEnabled(true)
        for (let frame = 0; frame < 3; frame++) tick(renderer)
        expect(retained.equals(whole), keys).toBe(true)
        expect(previous === undefined || !previous.equals(retained), `${keys} changed the picture`).toBe(true)
        previous = retained
      }
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      page.root.unmount()
    }
  })
})

/** A motion value that `start` runs from an effect, and the element `render` binds it to. */
function BoundHead({ initial, start, render }) {
  const value = useMotionValue(initial)
  useEffect(() => {
    const controls = start(value)
    return () => controls.stop()
  }, [value])
  return render(value)
}

const loop = { duration: 1.5, ease: "linear", repeat: Infinity }
// Under-damped, so it is still moving after the 20 ticks a test draws.
const spring = { type: "spring", stiffness: 12, damping: 0.4, mass: 1 }
const swing = ["#000000", "#ff0000", "#000000"]

const boundTo = (name: string, initial, start, render) => ({ [name]: <BoundHead initial={initial} start={start} render={render} /> })

/** What binds a motion value to a property that only paints, and the animation that drives it. */
const boundHeads = {
  ...boundTo("a colour keyframes loop bound to color", "#000000", (value) => animate(value, swing, loop), (value) => (
    <text testId="animated" style={{ fontSize: 20, color: value }}>
      animated
    </text>
  )),
  ...boundTo("a colour keyframes loop bound to backgroundColor", "#000000", (value) => animate(value, swing, loop), (value) => (
    <div testId="animated" style={{ height: 24, backgroundColor: value }}>
      <text style={{ fontSize: 20, color: "#ffffff" }}>animated</text>
    </div>
  )),
  ...boundTo("a number keyframes loop bound to opacity", 0.8, (value) => animate(value, [0.8, 0.3, 0.8], loop), (value) => (
    <div testId="animated" style={{ height: 24, backgroundColor: "#99ccff", opacity: value }}>
      <text style={{ fontSize: 20, color: "#222222" }}>animated</text>
    </div>
  )),
  ...boundTo("a spring bound to opacity", 0.9, (value) => animate(value, 0.5, spring), (value) => (
    <div testId="animated" style={{ height: 24, backgroundColor: "#99ccff", opacity: value }}>
      <text style={{ fontSize: 20, color: "#222222" }}>animated</text>
    </div>
  )),
  ...boundTo("a spring bound to color", "#000000", (value) => animate(value, "#606060", spring), (value) => (
    <text testId="animated" style={{ fontSize: 20, color: value }}>
      animated
    </text>
  )),
}

function RangeHead() {
  const x = useMotionValue(0)
  const color = useTransform(x, [0, 1], ["#000000", "#ff0000"])
  useEffect(() => {
    const controls = animate(x, [0, 1, 0], loop)
    return () => controls.stop()
  }, [x])
  return (
    <text testId="animated" style={{ fontSize: 20, color }}>
      animated
    </text>
  )
}

/**
 * The route to an svg stroke: a motion value binds to a style property, not to an attribute of a shape, so it
 * drives the `color` of the `<svg>` that a shape strokes with `currentColor`.
 */
const strokeHead = boundTo("a colour loop bound to the color of an svg stroked with currentColor", "#000000", (value) => animate(value, swing, loop), (value) => (
  <svg testId="spinner" viewBox="0 0 24 24" width={24} height={24} style={{ color: value }}>
    <circle cx={12} cy={12} r={10} fill="none" stroke="currentColor" strokeWidth={3} />
  </svg>
))

const paintHeads = {
  ...boundHeads,
  "a colour range transform of a number loop bound to color": <RangeHead />,
  ...strokeHead,
}

describeNative("a tick of a motion value that only paints", { timeout: TIMEOUT }, () => {
  for (const [name, head] of Object.entries(paintHeads)) {
    counted(`patches every tick, with no decline of either kind, at any size of the page: ${name}`, async () => {
      for (const nodes of [500, 5000]) expect(await patchesOf(head, nodes), `${nodes} nodes`).toEqual({ patched: TICKS, declines: {}, unpatchable: {} })
    })

    counted(`does the work of one text on any page: ${name}`, async () => {
      const small = await ticksOf(head, 500)
      const large = await ticksOf(head, 5000)
      for (let index = 0; index < TICKS; index++) {
        expect(large[index], `tick ${index}`).toEqual(small[index])
        expect(small[index].patchedFrames, `tick ${index} is patched`).toBe(1)
        expect(small[index].elementsBuilt, `tick ${index}`).toBeLessThanOrEqual(3)
        expect(small[index].prepainted, `tick ${index}`).toBeLessThanOrEqual(10)
        expect(small[index].hits + small[index].replays + small[index].builds, `tick ${index}`).toBe(0)
        expect(small[index].nodesCreated + small[index].measures + small[index].shapes, `tick ${index}`).toBe(0)
      }
    })

    it.skipIf(!patching)(`shows the picture of a frame built whole: ${name}`, async () => {
      const page = await mount(head, 500)
      try {
        const renderer = page.renderer
        const file = name.replace(/\W+/g, "-")
        for (let warm = 0; warm < WARM; warm++) tick(renderer)
        const before = renderer.getPatchCounters().patchedFrames
        for (let index = 0; index < 21; index++) tick(renderer)
        expect(renderer.getPatchCounters().patchedFrames - before).toBe(21)
        const patched = shot(renderer, `${file}-patched`)
        renderer.setPatchEnabled(false)
        tick(renderer, 0)
        const whole = shot(renderer, `${file}-whole`)
        expect(patched.equals(whole)).toBe(true)
        for (let index = 0; index < 10; index++) tick(renderer)
        expect(shot(renderer, `${file}-later`).equals(whole), "the picture moved").toBe(false)
        if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
      } finally {
        page.root.unmount()
      }
    })
  }
})

describeNative("a tick of a motion value that moves a box", { timeout: TIMEOUT }, () => {
  const sized = {
    width: boundTo("width", 40, (value) => animate(value, [40, 80, 40], loop), (value) => (
      <div testId="animated" style={{ height: 20, width: value, backgroundColor: "#336699" }} />
    )),
    left: boundTo("left", 0, (value) => animate(value, [0, 20, 0], loop), (value) => (
      <div style={{ position: "relative", width: 200, height: 40 }}>
        <div testId="animated" style={{ position: "absolute", top: 0, width: 20, height: 12, left: value, backgroundColor: "#336699" }} />
      </div>
    )),
    top: boundTo("top", 0, (value) => animate(value, [0, 20, 0], loop), (value) => (
      <div style={{ position: "relative", width: 200, height: 40 }}>
        <div testId="animated" style={{ position: "absolute", left: 0, width: 20, height: 12, top: value, backgroundColor: "#336699" }} />
      </div>
    )),
  }
  for (const [name, heads] of Object.entries(sized)) {
    counted(`is a layout animation, drawn whole and counted under LayoutAnimation: ${name}`, async () => {
      const done = await patchesOf(heads[name])
      expect(done.patched).toBe(0)
      expect(done.declines).toEqual({})
      expect(done.unpatchable).toEqual({ LayoutAnimation: TICKS })
    })
  }
})

/** A box bound to a value that never moves: it paints what it is bound to and asks for nothing. */
function Resting({ prop, value }: { prop: string; value: string | number }) {
  const bound = useMotionValue(value)
  return <div style={{ height: 4, backgroundColor: "#999999", [prop]: bound }} />
}

const resting = (count: number, prop = "backgroundColor", value: string | number = "#999999") =>
  Array.from({ length: count }, (_, index) => <Resting key={index} prop={prop} value={value} />)

/**
 * What decides which bound elements a tick builds is whether their value can have changed, not that they are bound:
 * a page of boxes bound to values that rest costs a tick of a loop beside them what the loop costs alone.
 */
describeNative("a tick beside motion values that rest", { timeout: TIMEOUT }, () => {
  const loopHead = boundHeads["a colour keyframes loop bound to color"]
  const beside = (rest: React.ReactNode) => (
    <>
      {rest}
      {loopHead}
    </>
  )

  counted("builds the element that moves and none of the 50 that are bound and rest", async () => {
    const alone = await ticksOf(loopHead, 500)
    const crowded = await ticksOf(beside(resting(50)), 500)
    for (let index = 0; index < TICKS; index++) {
      expect(crowded[index].patchedFrames, `tick ${index} is patched`).toBe(1)
      expect(crowded[index], `tick ${index}`).toEqual(alone[index])
    }
  })

  counted("builds the same when the resting values are opacities", async () => {
    const alone = await ticksOf(loopHead, 500)
    const crowded = await ticksOf(beside(resting(50, "opacity", 0.5)), 500)
    for (let index = 0; index < TICKS; index++) expect(crowded[index], `tick ${index}`).toEqual(alone[index])
  })

  counted("patches the ticks of a loop beside a width bound to a value that rests", async () => {
    for (const nodes of [500, 5000]) {
      expect(await patchesOf(beside(resting(1, "width", 40)), nodes), `${nodes} nodes`).toEqual({ patched: TICKS, declines: {}, unpatchable: {} })
    }
  })

  counted("patches the ticks of a loop beside a left and a top bound to values that rest", async () => {
    const head = beside(
      <div style={{ position: "relative", width: 100, height: 20 }}>
        <Resting prop="left" value={4} />
        <Resting prop="top" value={4} />
      </div>,
    )
    expect(await patchesOf(head, 500)).toEqual({ patched: TICKS, declines: {}, unpatchable: {} })
  })
})

/** What a tick reads of the motions of the page: the ones that move, however many rest beside them. */
describeNative("a tick beside native motions that rest", { timeout: TIMEOUT }, () => {
  const resting = (count: number) =>
    Array.from({ length: count }, (_, index) => (
      <motion.div key={index} style={{ height: 4, backgroundColor: "#99ccff" }} initial={false} animate={{ opacity: 0.5 }} />
    ))
  const beside = (count: number) => (
    <>
      {motionHead}
      {resting(count)}
    </>
  )

  counted("syncs the motion that moves and none of the 400 that rest", async () => {
    const alone = await ticksOf(motionHead, 500)
    const crowded = await ticksOf(beside(400), 500)
    for (let index = 0; index < TICKS; index++) {
      expect(alone[index].motionsSynced, `tick ${index}`).toBe(1)
      expect(crowded[index].patchedFrames, `tick ${index} is patched`).toBe(1)
      expect(crowded[index], `tick ${index}`).toEqual(alone[index])
    }
  })

  counted("syncs them all again in the frame built whole after the tree changed", async () => {
    const page = await mount(beside(400), 500)
    try {
      for (let warm = 0; warm < WARM; warm++) tick(page.renderer)
      const before = tick(page.renderer)
      expect(before.motionsSynced).toBe(1)
      const whole = work(page.renderer, () => page.renderer.setWindowSize(SIZE.width - 1, SIZE.height))
      expect(whole.patchedFrames).toBe(0)
      expect(whole.motionsSynced).toBeGreaterThanOrEqual(401)
      for (let warm = 0; warm < WARM; warm++) tick(page.renderer)
      expect(tick(page.renderer).motionsSynced).toBe(1)
    } finally {
      page.root.unmount()
    }
  })
})

/** The ticks around the end of an animation are the ones where a value stops being a source: each shows what a frame built whole shows. */
describeNative("an animation of a motion value that ends beside values that rest", { timeout: TIMEOUT }, () => {
  const ending = (animation: (value) => any) => (
    <>
      {resting(2)}
      <BoundHead
        initial="#000000"
        start={animation}
        render={(value) => (
          <text testId="animated" style={{ fontSize: 20, color: value }}>
            animated
          </text>
        )}
      />
    </>
  )
  const heads = {
    "a tween": ending((value) => animate(value, "#ff0000", { duration: 0.3, ease: "linear" })),
    "a spring": ending((value) => animate(value, "#606060", { type: "spring", stiffness: 300, damping: 30, mass: 1 })),
    "a tween that is paused and played again": ending((value) => {
      const controls = animate(value, "#ff0000", { duration: 0.3, ease: "linear" })
      controls.pause()
      setTimeout(() => controls.play(), 0)
      return controls
    }),
  }
  for (const [name, head] of Object.entries(heads)) {
    it.skipIf(!patching)(`shows the picture of a frame built whole at every tick, the last included: ${name}`, async () => {
      // A screenshot makes the frame after it whole, so three passes look at every third tick and leave the others patched.
      for (let pass = 0; pass < 3; pass++) {
        const page = await mount(head, 500)
        try {
          const renderer = page.renderer
          const file = name.replace(/\W+/g, "-")
          let patchedTicks = 0
          for (let step = 0; step < 45; step++) {
            patchedTicks += tick(renderer).patchedFrames
            if (step % 3 !== pass) continue
            const patched = shot(renderer, `${file}-patched`)
            renderer.flush()
            expect(patched.equals(shot(renderer, `${file}-whole`)), `pass ${pass}, tick ${step}`).toBe(true)
          }
          expect(patchedTicks, "the ticks of the animation are patched").toBeGreaterThanOrEqual(10)
          if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
        } finally {
          page.root.unmount()
        }
      }
    })
  }
})

const CULLED_TICKS = 24

/** A scroller 60 px high whose content is 400 px of nothing and then `inner`, which is out of its view. */
const culledHead = (inner: React.ReactNode) => (
  <div testId="scroller" style={{ width: 300, height: 60, flexShrink: 0, overflow: "auto" }}>
    <div style={{ width: 10, height: 400, flexShrink: 0 }} />
    {inner}
    <div style={{ width: 10, height: 400, flexShrink: 0 }} />
  </div>
)

const colourLoop = (value) => animate(value, swing, loop)

/**
 * A value bound to an element nobody sees is dormant like a CSS loop on one: Chrome runs the animation and paints no
 * frame for it. Its value and its events go on (Motion's `change` follows the animation's frame, not the element's
 * visibility), and the element is built with the value of the frame that shows it.
 */
describeNative("a motion value bound to an element that is culled", { timeout: TIMEOUT }, () => {
  /** A text bound to a colour loop, that reports each change of the value. */
  const colourWithEvents = (events: string[]) => (
    <BoundHead
      initial="#000000"
      start={(value) => {
        const off = value.on("change", (latest) => events.push(latest))
        const controls = colourLoop(value)
        return {
          stop: () => {
            off()
            controls.stop()
          },
        }
      }}
      render={(value) => (
        <text testId="animated" style={{ fontSize: 20, color: value }}>
          animated
        </text>
      )}
    />
  )
  const culledColour = (events: string[]) => culledHead(colourWithEvents(events))

  counted("asks for no frame while it is out of view, as a CSS loop does", async () => {
    const page = await mount(culledColour([]), 500)
    try {
      const renderer = page.renderer
      for (let warm = 0; warm < WARM; warm++) tick(renderer)
      for (let index = 0; index < CULLED_TICKS; index++) expect(frames(tick(renderer)), `tick ${index}`).toBe(0)
    } finally {
      page.root.unmount()
    }
  })

  // An element below 1 in opacity is not culled, with a CSS loop either, so it is not dormant.
  counted("asks for none for a value bound to backgroundColor, a spring and a transform either", async () => {
    const heads = {
      backgroundColor: boundHeads["a colour keyframes loop bound to backgroundColor"],
      spring: boundHeads["a spring bound to color"],
      range: <RangeHead />,
    }
    for (const [name, head] of Object.entries(heads)) {
      const page = await mount(culledHead(head), 500)
      try {
        for (let warm = 0; warm < WARM; warm++) tick(page.renderer)
        for (let index = 0; index < CULLED_TICKS; index++) expect(frames(tick(page.renderer)), `${name}, tick ${index}`).toBe(0)
      } finally {
        page.root.unmount()
      }
    }
  })

  counted("keeps the value and sends one change for each tick, as it does in view", async () => {
    const run = async (head: (events: string[]) => React.ReactNode) => {
      const events: string[] = []
      const page = await mount(head(events), 500)
      try {
        const renderer = page.renderer
        for (let warm = 0; warm < WARM; warm++) tick(renderer)
        renderer.dispatchNativeEvents()
        events.length = 0
        for (let index = 0; index < CULLED_TICKS; index++) {
          tick(renderer)
          renderer.dispatchNativeEvents()
        }
        return events
      } finally {
        page.root.unmount()
      }
    }
    const inView = await run(colourWithEvents)
    const culled = await run(culledColour)
    expect(inView.length, "the loop changes its value at every tick").toBe(CULLED_TICKS)
    expect(culled).toEqual(inView)
  })

  counted("draws no frame for a value that only a listener reads, and sends its changes", async () => {
    const events: string[] = []
    const head = (
      <BoundHead
        initial="#000000"
        start={(value) => {
          const off = value.on("change", (latest) => events.push(latest))
          const controls = colourLoop(value)
          return {
            stop: () => {
              off()
              controls.stop()
            },
          }
        }}
        render={() => <div style={{ width: 1, height: 1 }} />}
      />
    )
    const page = await mount(head, 500)
    try {
      const renderer = page.renderer
      for (let warm = 0; warm < WARM; warm++) tick(renderer)
      renderer.dispatchNativeEvents()
      events.length = 0
      for (let index = 0; index < CULLED_TICKS; index++) {
        expect(frames(tick(renderer)), `tick ${index}`).toBe(0)
        renderer.dispatchNativeEvents()
      }
      expect(events.length).toBe(CULLED_TICKS)
    } finally {
      page.root.unmount()
    }
  })

  counted("resolves the promise of an animation that ends out of view", async () => {
    let finished = false
    const head = culledHead(
      <BoundHead
        initial="#000000"
        start={(value) => {
          const controls = animate(value, "#ff0000", { duration: 0.3, ease: "linear" })
          controls.then(() => (finished = true))
          return controls
        }}
        render={(value) => <text style={{ fontSize: 20, color: value }}>animated</text>}
      />,
    )
    const page = await mount(head, 500)
    try {
      for (let index = 0; index < 40; index++) {
        tick(page.renderer)
        page.renderer.dispatchNativeEvents()
      }
      await sleep(0)
      expect(finished).toBe(true)
    } finally {
      page.root.unmount()
    }
  })

  counted("is drawn with the value of the frame that shows it, and ticks in patches again", async () => {
    const page = await mount(culledColour([]), 500)
    try {
      const renderer = page.renderer
      const scroller = renderer.findByTestId("scroller").id
      for (let warm = 0; warm < WARM; warm++) tick(renderer)
      for (let index = 0; index < CULLED_TICKS; index++) tick(renderer)
      renderer.scrollTo(scroller, 0, 400)
      // The scrollbar that a scroll shows fades out by itself, in frames of its own.
      tick(renderer)
      tick(renderer, 2000)
      for (let index = 0; index < 4; index++) tick(renderer)
      const before = renderer.getPatchCounters()
      for (let index = 0; index < TICKS; index++) expect(tick(renderer).patchedFrames, `tick ${index}`).toBe(1)
      const after = renderer.getPatchCounters()
      expect(gained(after.declines, before.declines)).toEqual({})
      expect(gained(after.unpatchable, before.unpatchable)).toEqual({})
      const patched = shot(renderer, "culled-woken-patched")
      renderer.setPatchEnabled(false)
      tick(renderer, 0)
      expect(patched.equals(shot(renderer, "culled-woken-whole")), "the picture of a frame built whole").toBe(true)
      renderer.setPatchEnabled(true)
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      page.root.unmount()
    }
  })

  counted("asks for none while it is below the window, either", async () => {
    const page = await mount(<div style={{ width: 10, height: 10 }} />, 5000, false, boundHeads["a colour keyframes loop bound to color"])
    try {
      for (let warm = 0; warm < WARM; warm++) tick(page.renderer)
      for (let index = 0; index < CULLED_TICKS; index++) expect(frames(tick(page.renderer)), `tick ${index}`).toBe(0)
    } finally {
      page.root.unmount()
    }
  })
})

/**
 * A script that writes a value that only paints, every frame (a `requestAnimationFrame` loop, or the `change` of
 * another value), changes what the element paints and nothing it lays out: the element is a source for a patch, as it
 * is for a tick of a loop, and not a reason to draw the page again.
 */
describeNative("a motion value written by a script", { timeout: TIMEOUT }, () => {
  const COLOURS = ["#ff0000", "#0000ff", "#00aa00", "#aa00aa"]
  const writing = (props: Record<string, any>) => {
    const handle: { value?: any; other?: any } = {}
    const head = (
      <BoundHead
        initial="#000000"
        start={(value) => {
          handle.value = value
          return { stop() {} }
        }}
        render={(value) => (
          <text testId="animated" style={{ fontSize: 20, color: value }}>
            animated
          </text>
        )}
        {...props}
      />
    )
    return { head, handle }
  }

  /** A tick of the page of a script: the script writes, then the frame is drawn. */
  const step = (renderer, handle, index: number) => work(renderer, () => handle.value.set(COLOURS[index % COLOURS.length]))

  const run = async (nodes: number) => {
    const { head, handle } = writing({})
    const page = await mount(head, nodes)
    try {
      const renderer = page.renderer
      for (let warm = 0; warm < WARM; warm++) step(renderer, handle, warm)
      const before = renderer.getPatchCounters()
      const done: Work[] = []
      for (let index = 0; index < TICKS; index++) done.push(step(renderer, handle, WARM + index))
      const after = renderer.getPatchCounters()
      return { done, declines: gained(after.declines, before.declines), unpatchable: gained(after.unpatchable, before.unpatchable) }
    } finally {
      page.root.unmount()
    }
  }

  counted("is patched at every write, at any size of the page", async () => {
    const small = await run(500)
    const large = await run(5000)
    expect(small.declines).toEqual({})
    expect(small.unpatchable).toEqual({})
    for (let index = 0; index < TICKS; index++) {
      expect(small.done[index].patchedFrames, `write ${index} is patched`).toBe(1)
      expect(small.done[index].paintedFrames, `write ${index} is no frame built whole`).toBe(0)
      expect(small.done[index].elementsBuilt, `write ${index}`).toBeLessThanOrEqual(3)
      expect(large.done[index], `write ${index}`).toEqual(small.done[index])
    }
  })

  it.skipIf(!patching)("shows the picture of a frame built whole", async () => {
    // A screenshot makes the frame after it whole, so three passes look at every third write and leave the others patched.
    for (let pass = 0; pass < 3; pass++) {
      const { head, handle } = writing({})
      const page = await mount(head, 500)
      try {
        const renderer = page.renderer
        let patchedWrites = 0
        for (let index = 0; index < 12; index++) {
          patchedWrites += step(renderer, handle, index).patchedFrames
          if (index % 3 !== pass) continue
          const patched = shot(renderer, `written-${index}-patched`)
          renderer.flush()
          expect(patched.equals(shot(renderer, `written-${index}-whole`)), `pass ${pass}, write ${index}`).toBe(true)
        }
        expect(patchedWrites, "the writes are patched").toBeGreaterThanOrEqual(6)
        if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
      } finally {
        page.root.unmount()
      }
    }
  })

  // The writes that start and end a run are the ones where an opacity becomes a group, or stops being one.
  const runs = {
    opacity: { initial: 1, loop: [0.9, 0.3, 0.9], rest: 1, to: 0.5 },
    color: { initial: "#000000", loop: swing, rest: "#0000ff", to: "#606060" },
  }
  for (const [prop, run] of Object.entries(runs)) {
    it.skipIf(!patching)(`shows the picture of a frame built whole at every tick of an animation started, stopped and replaced, bound to ${prop}`, async () => {
      const handle: { value?: any } = {}
      const head = (
        <BoundHead
          initial={run.initial}
          start={(value) => {
            handle.value = value
            return { stop() {} }
          }}
          render={(value) => (
            <div testId="animated" style={{ height: 24, backgroundColor: "#99ccff", [prop]: value }}>
              <text style={{ fontSize: 20, color: "#222222" }}>animated</text>
            </div>
          )}
        />
      )
      // A screenshot makes the frame after it whole, so three passes look at every third tick and leave the others patched.
      for (let pass = 0; pass < 3; pass++) {
        const page = await mount(head, 500)
        try {
          const renderer = page.renderer
          const value = handle.value
          const steps: (() => void)[] = [
            () => animate(value, run.loop, loop),
            ...Array.from({ length: 3 }, () => () => {}),
            () => value.stop(),
            () => {},
            () => value.set(run.rest),
            () => {},
            () => animate(value, run.to, { duration: 0.2, ease: "linear" }),
            ...Array.from({ length: 14 }, () => () => {}),
          ]
          let patchedTicks = 0
          for (const [index, step] of steps.entries()) {
            step()
            patchedTicks += tick(renderer).patchedFrames
            if (index % 3 !== pass) continue
            const patched = shot(renderer, `written-run-${prop}-patched`)
            renderer.flush()
            expect(patched.equals(shot(renderer, `written-run-${prop}-whole`)), `pass ${pass}, step ${index}`).toBe(true)
          }
          expect(patchedTicks, "the writes and ticks are patched").toBeGreaterThanOrEqual(10)
          if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
        } finally {
          page.root.unmount()
        }
      }
    })
  }

  // The case of FF: a loop on a value nothing is bound to, and a script that copies it into the one the element paints.
  counted("is patched when the change of a value that nothing paints writes the one that does", async () => {
    const source: { value?: any; target?: any } = {}
    const head = (
      <BoundHead
        initial="#000000"
        start={(target) => {
          source.target = target
          return { stop() {} }
        }}
        render={(target) => (
          <>
            <BoundHead
              initial={0}
              start={(value) => {
                source.value = value
                const off = value.on("change", (latest) => target.set(latest > 0.5 ? "#ff0000" : "#0000ff"))
                const controls = animate(value, [0, 1, 0], loop)
                return { stop: () => (off(), controls.stop()) }
              }}
              render={() => <div style={{ width: 1, height: 1 }} />}
            />
            <text testId="animated" style={{ fontSize: 20, color: target }}>
              animated
            </text>
          </>
        )}
      />
    )
    const page = await mount(head, 500)
    try {
      const renderer = page.renderer
      const both = () => {
        const done = work(renderer, () => renderer.clockAdvance(16))
        renderer.dispatchNativeEvents()
        const written = work(renderer, () => {})
        return { done, written }
      }
      for (let warm = 0; warm < WARM; warm++) both()
      const before = renderer.getPatchCounters()
      const wholeBefore = renderer.getPaintedFrameCount()
      for (let index = 0; index < 20; index++) both()
      const after = renderer.getPatchCounters()
      expect(renderer.getPaintedFrameCount() - wholeBefore, "no frame is built whole").toBe(0)
      expect(after.patchedFrames - before.patchedFrames, "the writes are patched").toBeGreaterThanOrEqual(20)
      expect(gained(after.declines, before.declines)).toEqual({})
      expect(gained(after.unpatchable, before.unpatchable)).toEqual({})
    } finally {
      page.root.unmount()
    }
  })

  // A write to an element that is no unit (a scroller, a span) is drawn whole in the draw the tick asked for, with the new value.
  it.skipIf(!patching)("draws a write to the value of an element that is no unit whole, with the value written", async () => {
    const handle: { value?: any } = {}
    const head = (
      <BoundHead
        initial="#ff0000"
        start={(value) => {
          handle.value = value
          return { stop() {} }
        }}
        render={(value) => (
          <div testId="animated" style={{ width: 100, height: 40, flexShrink: 0, overflow: "scroll", backgroundColor: value }}>
            <div style={{ width: 300, height: 10 }} />
          </div>
        )}
      />
    )
    const page = await mount(head, 500)
    try {
      const renderer = page.renderer
      for (let warm = 0; warm < WARM; warm++) tick(renderer)
      const red = shot(renderer, "no-unit-red")
      const done = work(renderer, () => handle.value.set("#0000ff"))
      expect(done.paintedFrames, "a frame built whole").toBe(1)
      const blue = shot(renderer, "no-unit-blue")
      expect(blue.equals(red), "the picture moved").toBe(false)
      renderer.flush()
      expect(blue.equals(shot(renderer, "no-unit-flushed")), "the picture of a frame built whole").toBe(true)
    } finally {
      page.root.unmount()
    }
  })

  counted("draws a write to a width whole, as before", async () => {
    const handle: { value?: any } = {}
    const head = (
      <BoundHead
        initial={40}
        start={(value) => {
          handle.value = value
          return { stop() {} }
        }}
        render={(value) => <div testId="animated" style={{ height: 20, width: value, backgroundColor: "#336699" }} />}
      />
    )
    const page = await mount(head, 500)
    try {
      const renderer = page.renderer
      for (let warm = 0; warm < WARM; warm++) work(renderer, () => handle.value.set(40 + warm))
      for (let index = 0; index < TICKS; index++) {
        const done = work(renderer, () => handle.value.set(60 + index))
        expect(done.patchedFrames, `write ${index}`).toBe(0)
        expect(done.paintedFrames, `write ${index}`).toBe(1)
      }
      expect(renderer.getElementBounds(renderer.findByTestId("animated").id).width).toBe(60 + TICKS - 1)
    } finally {
      page.root.unmount()
    }
  })
})


/** A document past what is rasterised in the paint that needs it (200 x 200 is 40000 pixels at any scale), so a job makes its image. */
const BIG_DOCUMENT =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20"><rect width="20" height="20" fill="#cc2222"/><circle cx="10" cy="10" r="6" fill="#2222cc"/></svg>'
const ICON_DOCUMENT =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M4 12h16M12 4v16" stroke="currentColor" stroke-width="2" fill="none"/></svg>'

/** The same size as the big document and another picture, so that swapping them starts a job. */
const GREEN_DOCUMENT =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20"><rect width="20" height="20" fill="#cc2222"/><circle cx="10" cy="10" r="6" fill="#22cc22"/></svg>'

const bigSvg = <svg testId="big" source={BIG_DOCUMENT} style={{ width: 200, height: 200, flexShrink: 0 }} />

/** The centre of the big document is blue (the slow one red) once its raster has been painted, and the page's grey or white before. */
function bigIsDrawn(renderer): boolean {
  const bounds = renderer.getElementBounds(renderer.findByTestId("big").id)
  const file = shotFile("big-pixel")
  fs.rmSync(file, { force: true })
  renderer.captureScreenshot(file)
  const at = readLogicalPixels(file, renderer).at(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2)
  return Math.abs(at[0] - at[2]) > 100
}

/** Frames the display link would draw, while the job of the big document runs on its own thread. */
async function untilBigIsDrawn(renderer) {
  for (let frame = 0; frame < 400 && !bigIsDrawn(renderer); frame++) {
    renderer.flush()
    renderer.dispatchNativeEvents()
    await sleep(10)
  }
  expect(bigIsDrawn(renderer), "the raster of the big document landed").toBe(true)
  await settle(renderer, 6)
}

const svgsHead = (
  <div style={{ display: "flex", flexDirection: "row", alignItems: "center", gap: 8 }}>
    {bigSvg}
    <svg testId="icon" source={ICON_DOCUMENT} style={{ width: 24, height: 24, color: "#333333", flexShrink: 0 }} />
    <svg testId="drawn" viewBox="0 0 24 24" width={24} height={24} style={{ color: "#333333" }}>
      <path d="M4 12h16M12 4v16" stroke="currentColor" strokeWidth={2} fill="none" />
    </svg>
    {colourHead()}
  </div>
)

const GLYPH_A = "M3 12 L12 4 L21 12 L12 20 Z"
const GLYPH_B = "M3 12 L12 9 L21 12 L12 15 Z"

/** FF's thinking indicator: a glyph that morphs `d` on a path of a 24 x 24 svg, beside a text with a colour loop. */
const glyphHead = (
  <div style={{ display: "flex", flexDirection: "row", alignItems: "center", gap: 8 }}>
    <svg testId="glyph" viewBox="0 0 24 24" width={24} height={24} style={{ color: "#333333" }}>
      <motion.path
        d={GLYPH_A}
        fill="none"
        stroke="currentColor"
        strokeWidth={1.5}
        animate={{ d: [GLYPH_A, GLYPH_B, GLYPH_A] }}
        transition={{ duration: 1.5, ease: "linear", repeat: Infinity }}
      />
    </svg>
    {colourHead()}
  </div>
)

describeNative("an svg that is not changing", { timeout: TIMEOUT }, () => {
  for (const nodes of [500, 5000]) {
    it.skipIf(!patching)(`does not stop a colour loop elsewhere from being patched at every tick: a job-rasterised svg, an icon, an svg of shapes, ${nodes} nodes`, async () => {
      const done = await patchesOf(svgsHead, nodes, untilBigIsDrawn)
      if (verifying) return
      expect(done).toEqual({ patched: TICKS, declines: {}, unpatchable: {} })
    })

    it.skipIf(!patching)(`patches every tick of a path that morphs d in a 24 x 24 svg beside a colour loop, ${nodes} nodes`, async () => {
      const done = await patchesOf(glyphHead, nodes)
      if (verifying) return
      expect(done).toEqual({ patched: TICKS, declines: {}, unpatchable: {} })
    })
  }

  /** Svgs no paint reaches: a paint that makes no image leaves nothing to wait for. */
  const unpainted = {
    "has no area": <svg source={ICON_DOCUMENT} style={{ width: 0, height: 0, color: "#333333" }} />,
    "is a big document with no height": <svg source={BIG_DOCUMENT} style={{ width: 200, height: 0 }} />,
    "is in a box with opacity 0": (
      <div style={{ opacity: 0 }}>
        <svg source={ICON_DOCUMENT} style={{ width: 24, height: 24 }} />
        <svg source={BIG_DOCUMENT} style={{ width: 200, height: 200 }} />
      </div>
    ),
    "is in a box with visibility hidden": (
      <div style={{ visibility: "hidden" }}>
        <svg source={ICON_DOCUMENT} style={{ width: 24, height: 24 }} />
        <svg source={BIG_DOCUMENT} style={{ width: 200, height: 200 }} />
      </div>
    ),
    "is in a box that clips it to nothing": (
      <div style={{ height: 0, overflow: "hidden" }}>
        <svg source={ICON_DOCUMENT} style={{ width: 24, height: 24 }} />
        <svg source={BIG_DOCUMENT} style={{ width: 200, height: 200 }} />
      </div>
    ),
  }
  for (const [name, carrier] of Object.entries(unpainted)) {
    it.skipIf(!patching)(`does not stop a colour loop from being patched while the page holds an svg that ${name}`, async () => {
      const head = (
        <div style={{ display: "flex", flexDirection: "row", alignItems: "center", gap: 8 }}>
          {carrier}
          {colourHead()}
        </div>
      )
      for (const nodes of [500, 5000]) {
        const done = await patchesOf(head, nodes)
        if (verifying) continue
        expect(done, `${nodes} nodes`).toEqual({ patched: TICKS, declines: {}, unpatchable: {} })
      }
    })
  }

  it.skipIf(!patching)("draws the raster of a changed document as it arrives, and patches the ticks around it", async () => {
    let swap: () => void = () => {}
    function Swapping() {
      const [swapped, setSwapped] = useState(false)
      swap = () => setSwapped(true)
      return (
        <Page
          nodes={500}
          head={
            <>
              {colourHead()}
              <svg testId="big" source={swapped ? GREEN_DOCUMENT : BIG_DOCUMENT} style={{ width: 200, height: 200, flexShrink: 0 }} />
            </>
          }
        />
      )
    }
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.clockPause()
      root.render(<Swapping />)
      await settle(renderer)
      renderer.setInlineDraws(false)
      renderer.setA11yActive(false)
      await untilBigIsDrawn(renderer)
      for (let warm = 0; warm < WARM; warm++) tick(renderer)
      const centre = () => {
        const bounds = renderer.getElementBounds(renderer.findByTestId("big").id)
        const file = shotFile("centre")
        fs.rmSync(file, { force: true })
        renderer.captureScreenshot(file)
        return readLogicalPixels(file, renderer).at(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2)
      }
      expect(centre()[2], "the first document is drawn").toBeGreaterThan(150)
      for (let warm = 0; warm < WARM; warm++) tick(renderer)
      swap()
      await sleep(20)
      // The commit is drawn whole and its paint starts the job, which lands when the executor runs.
      const started = tick(renderer)
      const ticks: number[] = []
      for (let index = 0; index < 12; index++) ticks.push(tick(renderer).patchedFrames)
      const whole = ticks.filter((patched) => patched === 0).length
      if (!verifying) {
        expect(started.patchedFrames, "the commit is drawn whole").toBe(0)
        // The tick that finds the job running is the first to see it, and is drawn whole.
        expect(whole, `ticks after the commit: ${ticks}`).toBeLessThanOrEqual(2)
      }
      renderer.flush()
      const arrival: number[] = []
      for (let index = 0; index < 12; index++) arrival.push(tick(renderer).patchedFrames)
      if (!verifying) {
        expect(arrival.slice(0, 4).filter((patched) => patched === 0).length, `the arrival: ${arrival}`).toBeLessThanOrEqual(3)
        expect(arrival.slice(4), `patching resumed: ${arrival}`).toEqual(new Array(8).fill(1))
      }
      expect(centre()[1], "the second document is drawn").toBeGreaterThan(150)
      const patched = shot(renderer, "swap-patched")
      renderer.setPatchEnabled(false)
      tick(renderer, 0)
      expect(patched.equals(shot(renderer, "swap-whole")), "the picture of a frame built whole").toBe(true)
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })

  it.skipIf(!patching)("draws the picture of the page built whole at every tick", async () => {
    const page = await mount(svgsHead, 500)
    try {
      const renderer = page.renderer
      await untilBigIsDrawn(renderer)
      for (let warm = 0; warm < WARM; warm++) tick(renderer)
      for (let index = 0; index < 3; index++) tick(renderer)
      const patched = shot(renderer, "svgs-patched")
      renderer.setPatchEnabled(false)
      tick(renderer, 0)
      expect(patched.equals(shot(renderer, "svgs-whole")), "the picture of a frame built whole").toBe(true)
    } finally {
      page.root.unmount()
    }
  })

  it.skipIf(!patching)("draws the arrival of a raster as a frame of its own, and patches the ticks after it", async () => {
    let show: () => void = () => {}
    function Late() {
      const [shown, setShown] = useState(false)
      show = () => setShown(true)
      return <Page nodes={500} head={<>{colourHead()}{shown ? bigSvg : null}</>} />
    }
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.clockPause()
      root.render(<Late />)
      await settle(renderer)
      renderer.setInlineDraws(false)
      renderer.setA11yActive(false)
      for (let warm = 0; warm < WARM; warm++) tick(renderer)
      if (!verifying) expect(tick(renderer).patchedFrames).toBe(1)
      show()
      await sleep(20)
      const whole: number[] = []
      let patchedInARow = 0
      for (let frame = 0; frame < 400 && patchedInARow < 6; frame++) {
        await sleep(10)
        const done = tick(renderer)
        if (done.patchedFrames === 1) patchedInARow++
        else {
          patchedInARow = 0
          whole.push(frame)
        }
      }
      expect(bigIsDrawn(renderer), "the raster landed").toBe(true)
      if (!verifying) {
        expect(patchedInARow, "patching resumed").toBe(6)
        // The commit that adds the svg, the frame that paints and starts the job, the frame that paints what landed.
        expect(whole.length, `whole ticks: ${whole}`).toBeLessThanOrEqual(4)
      }
      const patched = shot(renderer, "arrival-patched")
      renderer.setPatchEnabled(false)
      tick(renderer, 0)
      expect(patched.equals(shot(renderer, "arrival-whole")), "the picture of a frame built whole").toBe(true)
      renderer.setPatchEnabled(true)
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })
})

describeNative("an animation that nothing draws because its box is not generated", { timeout: TIMEOUT }, () => {
  const hidden = {
    "a native motion colour loop": (
      <motion.div
        style={{ height: 24, backgroundColor: "#99ccff" }}
        animate={{ backgroundColor: ["#ff0000", "#0000ff", "#ff0000"] }}
        transition={{ duration: 1.5, ease: "linear", repeat: Infinity }}
      />
    ),
    "a keyframes colour loop": <text style={{ fontSize: 20, color: "#000000", animation: "afw-pulse 1.5s linear infinite" }}>hidden</text>,
    "a native motion colour loop under a box of its own": (
      <div style={{ position: "relative" }}>
        <motion.div
          style={{ height: 24, backgroundColor: "#99ccff" }}
          animate={{ backgroundColor: ["#ff0000", "#0000ff", "#ff0000"] }}
          transition={{ duration: 1.5, ease: "linear", repeat: Infinity }}
        />
      </div>
    ),
  }
  for (const [name, loop] of Object.entries(hidden)) {
    it.skipIf(!patching)(`does not stop a colour loop elsewhere from being patched: ${name} in a display: none box`, async () => {
      const head = (
        <div style={{ display: "flex", flexDirection: "column" }}>
          <div style={{ display: "none" }}>{loop}</div>
          {colourHead()}
        </div>
      )
      for (const nodes of [500, 5000]) {
        const done = await patchesOf(head, nodes)
        if (verifying) continue
        expect(done, `${nodes} nodes`).toEqual({ patched: TICKS, declines: {}, unpatchable: {} })
      }
    })
  }
})
