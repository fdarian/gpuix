/// The Solid half of `motion-mount.test.tsx` of the React package: what a `motion.div` does on mount without an
/// `initial`, with `initial={false}`, and while it repeats. The expected values of the repeat cases are Motion's
/// (`packages/react/src/__tests__/fixtures/motion-repeat-oracle.json`).

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import { createSignal, Show } from "solid-js"
import { describe, expect, it } from "bun:test"
import { AnimatePresence, motion } from "../index.js"
import type { MotionTransition } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

interface OracleCase {
  keyframes: number[]
  transition: Omit<MotionTransition, "repeat"> & { repeat?: number | "Infinity" }
  times: number[]
  values: number[]
}

const ORACLE: Record<string, OracleCase> = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../react/src/__tests__/fixtures/motion-repeat-oracle.json"),
    "utf8"
  )
)

const column = { display: "flex", flexDirection: "column", alignItems: "flex-start" } as const

function frame(renderer: TestRenderer, ms: number): void {
  renderer.clockFastForward(ms)
  renderer.flush()
  renderer.dispatchNativeEvents()
}

function widthOf(renderer: TestRenderer, testId: string): number {
  return renderer.getElementBounds(renderer.findByTestId(testId)!.id)!.width
}

describe.skipIf(!hasNativeTestRenderer)("Solid motion on mount without an initial", () => {
  it("animates a keyframe list from its first keyframe, repeating for ever", () => {
    const app = createTestRoot()
    const renderer = app.renderer
    renderer.clockPause()
    app.render(() => (
      <div style={column}>
        <motion.div
          testId="box"
          style={{ height: 10 }}
          animate={{ width: [0, 100, 0] }}
          transition={{ duration: 2, ease: "linear", repeat: Infinity }}
        />
      </div>
    ))
    expect(widthOf(renderer, "box")).toBeCloseTo(0, 0)
    const expected = [[500, 50], [500, 100], [500, 50], [500, 0], [500, 50], [10_000, 50]] as const
    for (const [wait, width] of expected) {
      frame(renderer, wait)
      expect(widthOf(renderer, "box")).toBeCloseTo(width, 0)
    }
  })

  it("starts a null first keyframe from the element's own width", () => {
    const app = createTestRoot()
    const renderer = app.renderer
    renderer.clockPause()
    app.render(() => (
      <div style={column}>
        <motion.div
          testId="box"
          style={{ width: 40, height: 10 }}
          animate={{ width: [null, 100] }}
          transition={{ duration: 1, ease: "linear", repeat: Infinity, repeatType: "reverse" }}
        />
      </div>
    ))
    expect(widthOf(renderer, "box")).toBeCloseTo(40, 0)
    frame(renderer, 500)
    expect(widthOf(renderer, "box")).toBeCloseTo(70, 0)
    frame(renderer, 1000)
    expect(widthOf(renderer, "box")).toBeCloseTo(70, 0)
  })

  it("animates a plain value from the element's own style", () => {
    const app = createTestRoot()
    const renderer = app.renderer
    renderer.clockPause()
    app.render(() => (
      <div style={column}>
        <motion.div
          testId="box"
          style={{ width: 40, height: 10 }}
          animate={{ width: 100 }}
          transition={{ duration: 1, ease: "linear" }}
        />
      </div>
    ))
    expect(widthOf(renderer, "box")).toBeCloseTo(40, 0)
    frame(renderer, 500)
    expect(widthOf(renderer, "box")).toBeCloseTo(70, 0)
    frame(renderer, 600)
    expect(widthOf(renderer, "box")).toBeCloseTo(100, 0)
  })

  it("animates keyframes beside an initial that names other properties", () => {
    const app = createTestRoot()
    const renderer = app.renderer
    renderer.clockPause()
    app.render(() => (
      <div style={column}>
        <motion.div
          testId="box"
          initial={{ height: 10 }}
          animate={{ width: [0, 100] }}
          transition={{ duration: 1, ease: "linear", repeat: Infinity }}
        />
      </div>
    ))
    frame(renderer, 250)
    expect(widthOf(renderer, "box")).toBeCloseTo(25, 0)
    frame(renderer, 1500)
    expect(widthOf(renderer, "box")).toBeCloseTo(75, 0)
  })

  it("rests on the last keyframe with initial={false}, repeat or not", () => {
    const app = createTestRoot()
    const renderer = app.renderer
    renderer.clockPause()
    app.render(() => (
      <div style={column}>
        <motion.div
          testId="box"
          style={{ height: 10 }}
          initial={false}
          animate={{ width: [0, 100, 30] }}
          transition={{ duration: 1, repeat: Infinity, repeatType: "mirror" }}
        />
      </div>
    ))
    for (const wait of [0, 250, 1000, 7300]) {
      frame(renderer, wait)
      expect(widthOf(renderer, "box")).toBeCloseTo(30, 0)
    }
  })

  it("rests on the last keyframe in an AnimatePresence with initial={false}, and animates the next one", () => {
    const [second, setSecond] = createSignal(false)
    const app = createTestRoot()
    const renderer = app.renderer
    renderer.clockPause()
    app.render(() => (
      <div style={column}>
        <AnimatePresence initial={false}>
          <motion.div
            testId="first"
            style={{ height: 10 }}
            animate={{ width: [0, 100] }}
            transition={{ duration: 1, ease: "linear" }}
          />
          <Show when={second()}>
            <motion.div
              testId="second"
              style={{ height: 10 }}
              animate={{ width: [0, 100] }}
              transition={{ duration: 1, ease: "linear" }}
            />
          </Show>
        </AnimatePresence>
      </div>
    ))
    expect(widthOf(renderer, "first")).toBeCloseTo(100, 0)
    app.flushSync(() => setSecond(true))
    frame(renderer, 250)
    expect(widthOf(renderer, "second")).toBeCloseTo(25, 0)
    expect(widthOf(renderer, "first")).toBeCloseTo(100, 0)
  })
})

describe.skipIf(!hasNativeTestRenderer)("Solid motion repeat, against Motion", () => {
  for (const [name, source] of Object.entries(ORACLE)) {
    it(name, () => {
      const app = createTestRoot()
      const renderer = app.renderer
      renderer.clockPause()
      const transition = { ...source.transition, repeat: source.transition.repeat === "Infinity" ? Infinity : source.transition.repeat }
      app.render(() => (
        <div style={column}>
          <motion.div testId="box" style={{ height: 10 }} animate={{ width: source.keyframes }} transition={transition} />
        </div>
      ))
      let elapsed = 0
      source.times.forEach((time, index) => {
        frame(renderer, Math.round((time - elapsed) * 1000))
        elapsed = time
        const got = widthOf(renderer, "box")
        expect(Math.abs(got - source.values[index]!), `${name} at ${time}s: got ${got}, Motion ${source.values[index]}`).toBeLessThan(0.6)
      })
    })
  }
})
