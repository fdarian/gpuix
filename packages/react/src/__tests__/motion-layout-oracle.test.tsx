/// `layout` and `layoutId` animate a box through the frames Motion 14.1.0 does in headless Chrome.
///
/// `fixtures/motion-layout-oracle.html` holds the scenes (a `motion.div` with `layout`, `layout="position"`,
/// `layout="size"` or `layoutId`, a one second linear transition, a `before` and an `after` state) and
/// `fixtures/motion-layout-oracle.json` what Chrome reports for each: the `getBoundingClientRect()` of the tracked boxes
/// on every animation frame, with the time rebased to the start of Motion's animation (`start` is where that was after
/// the state changed, `before` and `after` the rects either side). The `radius` scene also records the computed border
/// radius and shadow. Every test below renders the same markup with gpuix's `motion`, advances the clock to each
/// recorded time and compares the boxes within 1.5 px. Regenerate the JSON, from the fixtures directory, with
///
///     MOTION_ROOT=<node_modules holding motion, framer-motion, react and react-dom at motion 14.1.0> \
///       uv run --with playwright python motion-layout-oracle.py
///
/// (it builds framer-motion into a temporary directory with `bun build` and starts only the headless shell Playwright
/// installs, one page per scene).

import fs from "fs"
import path from "path"
import type { ReactElement } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const TOLERANCE = 1.5
const linear = { duration: 1, ease: "linear" } as const
const column = { display: "flex", flexDirection: "column", width: 300 } as const

interface Rect {
  x: number
  y: number
  width: number
  height: number
}

interface OracleFrame {
  t: number
  boxes: Record<string, Rect | null>
  style?: { borderTopLeftRadius: string; boxShadow: string; offsetWidth: number; offsetHeight: number }
}

interface OracleScene {
  start: number
  before: Record<string, Rect | null>
  after: Record<string, Rect | null>
  frames: OracleFrame[]
}

const ORACLE: { meta: { motion: string; chrome: string }; scenes: Record<string, OracleScene> } = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/motion-layout-oracle.json"),
    "utf8"
  )
)

type State = "before" | "after"

function red(width: number, height: number) {
  return { width, height, flexShrink: 0, backgroundColor: "#ff0000" } as const
}

function spacer(shown: boolean) {
  return shown ? <div style={{ height: 30, flexShrink: 0 }} /> : null
}

/** Markup of the oracle page's scenes, state for state. */
const SCENES: Record<string, (state: State) => ReactElement> = {
  height: (state) => (
    <div style={column}>
      <motion.div testId="box" layout transition={linear} style={red(100, state === "after" ? 80 : 40)} />
    </div>
  ),
  widthMove: (state) => (
    <div style={column}>
      {spacer(state === "after")}
      <motion.div testId="box" layout transition={linear} style={red(state === "after" ? 200 : 100, 40)} />
    </div>
  ),
  position: (state) => (
    <div style={column}>
      {spacer(state === "after")}
      <motion.div testId="box" layout="position" transition={linear} style={red(100, state === "after" ? 80 : 40)} />
    </div>
  ),
  size: (state) => (
    <div style={column}>
      {spacer(state === "after")}
      <motion.div testId="box" layout="size" transition={linear} style={red(100, state === "after" ? 80 : 40)} />
    </div>
  ),
  moveOnlySize: (state) => (
    <div style={column}>
      {spacer(state === "after")}
      <motion.div testId="box" layout="size" transition={linear} style={red(100, 40)} />
    </div>
  ),
  nested: (state) => (
    <div style={column}>
      <motion.div
        testId="parent"
        layout
        transition={linear}
        style={{ width: 100, height: state === "after" ? 80 : 40, flexShrink: 0 }}
      >
        <div testId="plain" style={{ width: 100, height: 20, flexShrink: 0, backgroundColor: "#ff0000" }} />
        <motion.div
          testId="projected"
          layout
          transition={linear}
          style={{ width: 100, height: 20, flexShrink: 0, backgroundColor: "#0000ff" }}
        />
      </motion.div>
    </div>
  ),
  radius: (state) => (
    <div style={{ width: 300, height: 300, backgroundColor: "#ffffff" }}>
      <motion.div
        testId="card"
        layout
        transition={linear}
        style={{
          position: "relative",
          left: 40,
          top: 40,
          width: state === "after" ? 100 : 50,
          height: state === "after" ? 100 : 50,
          borderRadius: 20,
          backgroundColor: "#0000ff",
          boxShadow: "0px 0px 0px 10px rgb(0, 255, 0)",
        }}
      />
    </div>
  ),
  layoutId: (state) => (
    <div style={{ display: "flex", flexDirection: "row", width: 400 }}>
      <div style={{ width: 150, height: 100 }}>
        {state === "before" ? (
          <motion.div testId="card" layoutId="card" transition={linear} style={red(50, 50)} />
        ) : null}
      </div>
      <div style={{ width: 250, height: 100 }}>
        {state === "after" ? (
          <motion.div testId="card" layoutId="card" transition={linear} style={red(100, 100)} />
        ) : null}
      </div>
    </div>
  ),
}

function advance(renderer: TestRenderer, ms: number): void {
  if (ms > 0) renderer.clockFastForward(ms)
  renderer.flush()
  renderer.dispatchNativeEvents()
}

function boundsOf(renderer: TestRenderer, testId: string): Rect | null {
  const element = renderer.findByTestId(testId)
  if (!element) return null
  return renderer.getElementBounds(element.id)
}

/** Renders the scene's `before` then `after`, calls `visit` at each recorded frame from the start of the animation on,
 *  with the clock at that frame's time. */
function replay(
  name: string,
  options: { width?: number; height?: number },
  visit: (renderer: TestRenderer, frame: OracleFrame) => void
): void {
  const scene = ORACLE.scenes[name]
  expect(scene, `no oracle scene ${name}`).toBeDefined()
  const root = createTestRoot(options)
  const renderer = root.renderer
  renderer.clockPause()
  root.render(SCENES[name]!("before"))
  root.render(SCENES[name]!("after"))
  let now = 0
  for (const frame of scene!.frames) {
    if (frame.t < 0) continue
    advance(renderer, frame.t - now)
    now = frame.t
    visit(renderer, frame)
  }
}

function differences(renderer: TestRenderer, frame: OracleFrame): string[] {
  const found: string[] = []
  for (const id of Object.keys(frame.boxes)) {
    const chrome = frame.boxes[id]
    // A box that is not in the page at that frame has nothing to compare.
    if (!chrome) continue
    const ours = boundsOf(renderer, id)
    if (!ours) {
      found.push(`t=${frame.t.toFixed(1)} ${id}: Chrome has a box, gpuix has none`)
      continue
    }
    for (const key of ["x", "y", "width", "height"] as const) {
      if (Math.abs(ours[key] - chrome[key]) > TOLERANCE) {
        found.push(
          `t=${frame.t.toFixed(1)} ${id}.${key}: gpuix ${ours[key].toFixed(2)}, Chrome ${chrome[key].toFixed(2)}`
        )
      }
    }
  }
  return found
}

describeNative("layout against Motion 14.1.0 in Chrome", () => {
  for (const name of Object.keys(SCENES)) {
    it(`${name}: every box is where Chrome has it, frame by frame`, () => {
      const found: string[] = []
      let compared = 0
      replay(name, {}, (renderer, frame) => {
        compared += 1
        found.push(...differences(renderer, frame))
      })
      expect(compared, "frames replayed").toBeGreaterThan(30)
      expect(found).toEqual([])
    })
  }
})

describeNative("layout corrections against Motion 14.1.0 in Chrome", () => {
  it("keeps the corner radius and the shadow ring the size they were asked for while the card is scaled", () => {
    const frames = ORACLE.scenes.radius!.frames.filter(
      (frame) => frame.t >= 0 && frame.boxes.card && frame.boxes.card.width >= 45
    )
    expect(frames.length, "frames with a card of 45 px or more").toBeGreaterThan(5)
    const picked = new Set([0, 1, 2, 3, 4].map((i) => frames[Math.round((i * (frames.length - 1)) / 4)]!.t))

    const radius = 20 * (1 - 1 / Math.sqrt(2))
    const inside = radius - 1.5
    const outside = inside + 3
    const found: string[] = []
    replay("radius", { width: 300, height: 300 }, (renderer, frame) => {
      if (!picked.has(frame.t)) return
      const file = `${SHOTS_DIR}/motion-layout-oracle-${process.pid}-radius-${Math.round(frame.t)}.png`
      renderer.captureScreenshot(file)
      const pixels = readLogicalPixels(file, renderer)
      const card = frame.boxes.card!
      // The recorded rect is where Chrome shows the card; the radius on screen is 20 px at every size.
      const checks = [
        { what: "outside the corner arc (the shadow ring shows there)", x: card.x + inside, y: card.y + inside, expected: [0, 255, 0, 255] },
        { what: "inside the corner arc", x: card.x + outside, y: card.y + outside, expected: [0, 0, 255, 255] },
        { what: "ring 8 px out", x: card.x - 8, y: card.y + 25, expected: [0, 255, 0, 255] },
        { what: "past the 10 px ring", x: card.x - 12, y: card.y + 25, expected: [255, 255, 255, 255] },
      ]
      for (const check of checks) {
        const pixel = pixels.at(Math.round(check.x), Math.round(check.y))
        if (pixel.join() !== check.expected.join()) {
          found.push(
            `t=${frame.t.toFixed(1)} ${check.what} (${check.x.toFixed(1)}, ${check.y.toFixed(1)}): got ${pixel.join()}, want ${check.expected.join()}`
          )
        }
      }
    })
    expect(found).toEqual([])
  })
})
