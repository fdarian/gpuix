/// The Solid half of the React test of the same name: `filter` animates in a CSS transition, a CSS animation and the
/// keys of a `motion` component, and what is painted is the filter at that moment. A moment is read from the picture: a
/// black box on white has soft edge pixels in proportion to its blur, a grey box gets brighter with `brightness()`.

import fs from "fs"
import os from "os"
import path from "path"
import { createSignal } from "solid-js"
import { beforeAll, describe, expect, it } from "bun:test"
import { defineKeyframes, motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels } from "./pixels.js"

const shots = fs.mkdtempSync(path.join(os.tmpdir(), "gpuix-solid-filter-motion-"))
const STAGE = { position: "relative", width: 200, height: 200, backgroundColor: "#ffffff" } as const
const BOX = { position: "absolute", left: 60, top: 60, width: 40, height: 40 } as const
const LINEAR = { duration: 1, ease: "linear" } as const

defineKeyframes("solid-fm-brighten", {
  from: { filter: "brightness(1)" },
  "50%": { filter: "brightness(2)" },
  to: { filter: "brightness(1)" },
})
defineKeyframes("solid-fm-list", { from: { filter: "none" }, to: { filter: "blur(8px) brightness(2)" } })

function pixelsOf(app: ReturnType<typeof createTestRoot>, name: string) {
  app.renderer.dispatchNativeEvents()
  const file = path.join(shots, `${name}-${Math.random().toString(36).slice(2)}.png`)
  app.renderer.captureScreenshot(file)
  return readLogicalPixels(file, app.renderer)
}

/** How many pixels of the row through the middle of the box are neither black nor white. */
function soft(app: ReturnType<typeof createTestRoot>) {
  const pixels = pixelsOf(app, "soft")
  let count = 0
  for (let x = 30; x < 130; x++) {
    const red = pixels.at(x, 80)[0]
    if (red > 8 && red < 247) count++
  }
  return count
}

function softOf(filter: string) {
  const app = createTestRoot({ width: 200, height: 200 })
  app.renderer.clockPause()
  app.render(() => (
    <div style={STAGE}>
      <div style={{ ...BOX, backgroundColor: "#000000", filter }} />
    </div>
  ))
  const count = soft(app)
  app.unmount()
  return count
}

/** The softness of the still blurs the animated ones are compared with: measured first, as one test renderer lives at a time. */
const REF = { half: 0, full: 0 }
beforeAll(() => {
  if (!hasNativeTestRenderer) return
  REF.half = softOf("blur(4px)")
  REF.full = softOf("blur(8px)")
})

const near = (actual: number, expected: number, tolerance: number) =>
  expect(Math.abs(actual - expected)).toBeLessThanOrEqual(tolerance)

describe.skipIf(!hasNativeTestRenderer)("Solid filter animations", () => {
  it("transitions blur as a still blur of that radius", () => {
    const [filter, setFilter] = createSignal("blur(0px)")
    const app = createTestRoot({ width: 200, height: 200 })
    app.renderer.clockPause()
    app.render(() => (
      <div style={STAGE}>
        <div style={{ ...BOX, backgroundColor: "#000000", filter: filter(), transition: "filter 1s linear" }} />
      </div>
    ))
    app.renderer.flush()
    expect(soft(app)).toBeLessThanOrEqual(2)
    app.flushSync(() => setFilter("blur(8px)"))
    app.renderer.flush()
    app.renderer.clockFastForward(500)
    near(soft(app), REF.half, 2)
    app.renderer.clockFastForward(500)
    near(soft(app), REF.full, 2)
    app.unmount()
  })

  it("pads a list against none in a transition and blends brightness", () => {
    const [filter, setFilter] = createSignal("none")
    const app = createTestRoot({ width: 200, height: 200 })
    app.renderer.clockPause()
    app.render(() => (
      <div style={STAGE}>
        <div style={{ ...BOX, backgroundColor: "#404040", filter: filter(), transition: "filter 1s linear" }} />
      </div>
    ))
    app.renderer.flush()
    app.flushSync(() => setFilter("brightness(2)"))
    app.renderer.flush()
    app.renderer.clockFastForward(500)
    near(pixelsOf(app, "bright").at(80, 80)[0], 0x60, 3)
    app.renderer.clockFastForward(500)
    near(pixelsOf(app, "bright").at(80, 80)[0], 0x80, 1)
    app.unmount()
  })

  it("runs keyframes of brightness and a list from none", () => {
    const app = createTestRoot({ width: 200, height: 200 })
    app.renderer.clockPause()
    app.render(() => (
      <div style={STAGE}>
        <div style={{ ...BOX, backgroundColor: "#404040", animation: "solid-fm-brighten 1s linear" }} />
      </div>
    ))
    app.renderer.flush()
    app.renderer.clockFastForward(250)
    near(pixelsOf(app, "kf").at(80, 80)[0], 0x60, 3)
    app.renderer.clockFastForward(250)
    near(pixelsOf(app, "kf").at(80, 80)[0], 0x80, 3)
    app.unmount()

    const second = createTestRoot({ width: 200, height: 200 })
    second.renderer.clockPause()
    second.render(() => (
      <div style={STAGE}>
        <div style={{ ...BOX, backgroundColor: "#404040", animation: "solid-fm-list 1s linear" }} />
      </div>
    ))
    second.renderer.flush()
    second.renderer.clockFastForward(500)
    near(pixelsOf(second, "kf2").at(80, 80)[0], 0x60, 3)
    second.unmount()
  })

  it("springs FF's blur-in with a scale and rests sharp", () => {
    const app = createTestRoot({ width: 200, height: 200 })
    app.renderer.clockPause()
    app.render(() => (
      <div style={STAGE}>
        <motion.div
          testId="box"
          style={{ ...BOX, backgroundColor: "#000000" }}
          initial={{ scale: 0.85, filter: "blur(4px)" }}
          animate={{ scale: 1, filter: "blur(0px)" }}
          transition={{ type: "spring", stiffness: 300, damping: 30 }}
        />
      </div>
    ))
    app.renderer.flush()
    // The box starts at 0.85 of its size, so it has less edge to blur.
    near(soft(app), REF.half, 6)
    app.renderer.clockFastForward(100)
    app.renderer.flush()
    const middle = soft(app)
    expect(middle).toBeGreaterThan(2)
    expect(middle).toBeLessThan(REF.half)
    app.renderer.clockFastForward(2000)
    app.renderer.flush()
    expect(soft(app)).toBeLessThanOrEqual(2)
    app.unmount()
  })

  it("tweens an animate target", () => {
    const [filter, setFilter] = createSignal("blur(0px)")
    const app = createTestRoot({ width: 200, height: 200 })
    app.renderer.clockPause()
    app.render(() => (
      <div style={STAGE}>
        <motion.div
          testId="box"
          style={{ ...BOX, backgroundColor: "#000000" }}
          animate={{ filter: filter() }}
          transition={LINEAR}
        />
      </div>
    ))
    app.renderer.flush()
    app.flushSync(() => setFilter("blur(8px)"))
    app.renderer.flush()
    app.renderer.clockFastForward(500)
    app.renderer.flush()
    near(soft(app), REF.half, 3)
    app.renderer.clockFastForward(500)
    app.renderer.flush()
    near(soft(app), REF.full, 3)
    app.unmount()
  })
})
