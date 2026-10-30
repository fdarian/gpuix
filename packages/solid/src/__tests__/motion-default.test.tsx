// @ts-nocheck
/// The Solid half of `motion-default.test.tsx` of the React package: the transition a `motion` component gives a key that
/// its `transition` does not describe, against Motion 14.1.0's own `animateTarget`
/// (`packages/react/src/__tests__/fixtures/motion-default-oracle.json`). A row with a `target` replays through every route in its
/// `routes` (all four when it lists none): a variant name, an `animate` object, an `exit` object of a child leaving
/// `AnimatePresence`, and a `whileHover` object entering.

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import { describe, expect, it } from "bun:test"
import { createSignal, Show } from "solid-js"
import { AnimatePresence, motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

type Route = "variant" | "animate" | "exit" | "hover"
const ROUTES: Route[] = ["variant", "animate", "exit", "hover"]

interface OracleCase {
  key: string
  from: number
  to: number | Array<number | null>
  transition?: Record<string, unknown>
  target?: Record<string, unknown>
  routes?: Route[]
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

function boxFor(case_: OracleCase, route: Route | "plain", present: () => boolean) {
  const style = { ...BOX, backgroundColor: "#336699" }
  const written = (value: unknown) => ({ [case_.key]: value })
  switch (route) {
    case "plain":
      return (
        <motion.div testId="box" style={style} initial={written(case_.from)} animate={written(case_.to)} transition={case_.transition} />
      )
    case "variant":
      return (
        <motion.div
          testId="box"
          style={style}
          initial="off"
          animate="on"
          variants={{ off: written(case_.from), on: { ...written(case_.to), transition: case_.target } }}
          transition={case_.transition}
        />
      )
    case "animate":
      return (
        <motion.div
          testId="box"
          style={style}
          initial={written(case_.from)}
          animate={{ ...written(case_.to), transition: case_.target }}
          transition={case_.transition}
        />
      )
    case "exit":
      return (
        <AnimatePresence initial={false}>
          <Show when={present()}>
            <motion.div
              testId="box"
              style={style}
              animate={written(case_.from)}
              exit={{ ...written(case_.to), transition: case_.target }}
              transition={case_.transition}
            />
          </Show>
        </AnimatePresence>
      )
    case "hover":
      return (
        <motion.div
          testId="box"
          style={style}
          initial={written(case_.from)}
          animate={written(case_.from)}
          whileHover={{ ...written(case_.to), transition: case_.target }}
          transition={case_.transition}
        />
      )
  }
}

function sample(case_: OracleCase, route: Route | "plain"): number[] {
  const app = createTestRoot({ width: 800, height: 600 })
  const renderer = app.renderer
  renderer.clockPause()
  const [present, setPresent] = createSignal(true)
  app.render(() => (
    <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", padding: ORIGIN }}>
      {boxFor(case_, route, present)}
    </div>
  ))
  const read = READ[case_.key]!
  const boundsOfBox = () => {
    const element = renderer.findByTestId("box")
    return !element ? null : renderer.getElementBounds(element.id)
  }
  if (route === "hover") {
    frame(renderer, 0)
    const rest = boundsOfBox()!
    renderer.nativeSimulateMouseMove(rest.x + rest.width / 2, rest.y + rest.height / 2)
    frame(renderer, 0)
  }
  if (route === "exit") {
    frame(renderer, 0)
    app.flushSync(() => setPresent(false))
    frame(renderer, 0)
  }
  let at = 0
  const follow = () => {
    const bounds = boundsOfBox()
    if (bounds) renderer.nativeSimulateMouseMove(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2)
  }
  return case_.times.map((time) => {
    const elapsed = Math.round((time - at) * 1000)
    if (route === "hover") {
      // The pointer follows the box in steps that it cannot outrun: a box that moves 50 px in 50 ms (as `x` does) and is
      // 100 px wide leaves a pointer that waits for the next sample, where it was by luck of the rounding of the offset.
      for (let done = 0; done < elapsed; done += 10) {
        frame(renderer, Math.min(10, elapsed - done))
        follow()
      }
    } else {
      frame(renderer, elapsed)
    }
    at = time
    const bounds = boundsOfBox()
    if (route === "exit" && bounds === null) {
      // The exit is over, and the box is gone: the oracle must be at rest by now.
      return case_.values.at(-1)!
    }
    expect(bounds).not.toBeNull()
    return read(bounds!)
  })
}

describe.skipIf(!hasNativeTestRenderer)("Solid: the default transition of a motion key", () => {
  const cases = Object.entries(ORACLE).filter(([, case_]) => case_.key in READ)

  const replays = cases.flatMap(([name, case_]) =>
    case_.target === undefined
      ? [{ name, case_, route: "plain" as const }]
      : (case_.routes ?? ROUTES).map((route) => ({ name: `${name} (${route})`, case_, route }))
  )

  it("replays every row with a target through every route it runs on", () => {
    const routes = new Set(replays.map((replay) => replay.route))
    expect(routes).toEqual(new Set(["plain", ...ROUTES]))
    expect(replays.length).toBeGreaterThan(130)
  })

  for (const replay of replays) {
    it(replay.name, () => {
      const got = sample(replay.case_, replay.route)
      const tolerance = replay.case_.key.startsWith("scale") ? TOLERANCE.factor : TOLERANCE.pixels
      replay.case_.values.forEach((want, index) => {
        const error = Math.abs(got[index]! - want)
        if (error > tolerance) throw new Error(`${replay.name} at ${replay.case_.times[index]} s: ${got[index]} is not ${want}`)
      })
    })
  }
})
