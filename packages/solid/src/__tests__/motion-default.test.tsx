// @ts-nocheck
/// The Solid half of `motion-default.test.tsx` of the React package: the transition a `motion` component gives a key that
/// its `transition` does not describe, against Motion 14.1.0's own `animateTarget`
/// (`packages/react/src/__tests__/fixtures/motion-default-oracle.json`).

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import { describe, expect, it } from "bun:test"
import { motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

interface OracleCase {
  key: string
  from: number
  to: number | Array<number | null>
  transition?: Record<string, unknown>
  target?: Record<string, unknown>
  times: number[]
  values: number[]
}

const ORACLE: Record<string, OracleCase> = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../react/src/__tests__/fixtures/motion-default-oracle.json"),
    "utf8"
  )
)

const BOX = { width: 100, height: 50 }
const ORIGIN = 200
const TOLERANCE = { pixels: 0.6, factor: 0.006 }

const READ: Record<string, (box: { x: number; y: number; width: number; height: number }) => number> = {
  x: (box) => box.x - ORIGIN,
  y: (box) => box.y - ORIGIN,
  translateX: (box) => box.x - ORIGIN,
  width: (box) => box.width,
  height: (box) => box.height,
  scale: (box) => box.width / BOX.width,
  scaleX: (box) => box.width / BOX.width,
  scaleY: (box) => box.height / BOX.height,
}

function frame(renderer: TestRenderer, ms: number): void {
  renderer.clockFastForward(ms)
  renderer.flush()
  renderer.dispatchNativeEvents()
}

function sample(case_: OracleCase): number[] {
  const app = createTestRoot({ width: 800, height: 600 })
  const renderer = app.renderer
  renderer.clockPause()
  const style = { ...BOX, backgroundColor: "#336699" }
  const written = (value: unknown) => ({ [case_.key]: value })
  app.render(() => (
    <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", padding: ORIGIN }}>
      {case_.target === undefined ? (
        <motion.div
          testId="box"
          style={style}
          initial={written(case_.from)}
          animate={written(case_.to)}
          transition={case_.transition}
        />
      ) : (
        <motion.div
          testId="box"
          style={style}
          initial="off"
          animate="on"
          variants={{ off: written(case_.from), on: { ...written(case_.to), transition: case_.target } }}
          transition={case_.transition}
        />
      )}
    </div>
  ))
  const read = READ[case_.key]!
  let at = 0
  return case_.times.map((time) => {
    frame(renderer, Math.round((time - at) * 1000))
    at = time
    const bounds = renderer.getElementBounds(renderer.findByTestId("box")!.id)
    expect(bounds).not.toBeNull()
    return read(bounds!)
  })
}

describe.skipIf(!hasNativeTestRenderer)("Solid: the default transition of a motion key", () => {
  const cases = Object.entries(ORACLE).filter(([, case_]) => case_.key in READ)

  it("covers every kind of key and transition", () => {
    expect(cases.length).toBeGreaterThan(70)
  })

  for (const [name, case_] of cases) {
    it(name, () => {
      const got = sample(case_)
      const tolerance = case_.key.startsWith("scale") ? TOLERANCE.factor : TOLERANCE.pixels
      case_.values.forEach((want, index) => {
        const error = Math.abs(got[index]! - want)
        if (error > tolerance) throw new Error(`${name} at ${case_.times[index]} s: ${got[index]} is not ${want}`)
      })
    })
  }
})
