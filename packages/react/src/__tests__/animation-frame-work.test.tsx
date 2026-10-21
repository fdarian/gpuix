// @ts-nocheck
/**
 * What a tick of an animation that only changes what it paints costs a page. One looping text above
 * N/5 rows of 4 bordered cells (the shape of the FF demo page) must make a tick of the same work for
 * N = 500 and N = 5000: the tick builds the text alone and gpui paints it again in the frame it
 * rendered last (`patchedFrames` counts them), which is how Chrome repaints the display items of an
 * animation and keeps the rest. What is patched is the same picture a frame built whole gives.
 *
 * What cannot be patched is drawn whole, as before: an animation that moves a box, an `<svg>` shape,
 * a tick in which the pointer moved or the tree was committed.
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
defineKeyframes("afw-grow", { from: { width: 40 }, to: { width: 80 } })
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

/** Below 1 an element is a stacking context: gpui lifts it out of the recording around it, which a unit cannot be. */
const opacityHead = (
  <motion.div testId="animated" animate={{ opacity: [1, 0.25, 1] }} transition={{ duration: 1.5, ease: "linear", repeat: Infinity }}>
    <text style={{ fontSize: 20, color: "#222222" }}>animated</text>
  </motion.div>
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

  counted("does not patch a native motion opacity loop, which is drawn whole as before", async () => {
    const done = await ticksOf(opacityHead, 500)
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

  counted("builds the spine and replays the roots of an svg spinner each tick, which a unit cannot hold", async () => {
    // An `<svg>` is painted by a raster of its own tree, so it is not an element a tick can build alone.
    const small = await ticksOf(spinnerHead, 500)
    const large = await ticksOf(spinnerHead, 5000)
    for (let index = 0; index < TICKS; index++) {
      expect(small[index].patchedFrames, `tick ${index}`).toBe(0)
      expect(large[index].patchedFrames, `tick ${index}`).toBe(0)
    }
    console.info(
      `[animation-frame-work] svg spinner per tick: 500 nodes built ${small[TICKS - 1].elementsBuilt}, replays ${small[TICKS - 1].replays}; ` +
        `5000 nodes built ${large[TICKS - 1].elementsBuilt}, replays ${large[TICKS - 1].replays}`,
    )
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
