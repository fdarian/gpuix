/// `contentVisibility` and `containIntrinsicSize`: the boxes of an element that skips its content (`hidden`), and the
/// containment of every element that is not `visible`, and the read of a box inside a skipped one (`hidden-read-*`), against
/// headless Chrome.
///
/// `fixtures/content-visibility-oracle.html` is the page, `fixtures/content-visibility-cases.json` the cases both pages
/// are made from, and `fixtures/content-visibility-oracle.json` the boxes Chrome reports (`[x, y, width, height]` from
/// the top left of the case; regenerate with `sh fixtures/content-visibility-oracle.sh`). A case is a parent (`outer`)
/// holding `before`, the `target` and `after`; the target holds a child 150x300 that a hidden target does not lay out.
/// Boxes are compared to 1/64 px.

import fs from "fs"
import path from "path"
import type { CSSProperties } from "react"
import { fileURLToPath } from "url"
import { afterAll, beforeAll, describe, expect, it } from "vitest"
import { defineKeyframes, motion, motionValue } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { verifying } from "./retained-harness.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rect = [number, number, number, number]
type Style = Record<string, string | number>

interface Spec {
  name: string
  outer: Style
  target: Style
  child: Style
  measureChild: boolean
  before: Style
  after: Style
}

const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")
const CASES: Spec[] = JSON.parse(fs.readFileSync(path.join(FIXTURES, "content-visibility-cases.json"), "utf8")).cases
const ORACLE: Record<string, Record<string, Rect>> = JSON.parse(
  fs.readFileSync(path.join(FIXTURES, "content-visibility-oracle.json"), "utf8")
)

/** The same base the page gives every element (it has `boxSizing: border-box` too, which is all gpuix has), so a case only says what differs from a plain block. */
const BASE = { display: "block", margin: "0", padding: "0", flexGrow: 0, flexShrink: 1 }
const CASE = { position: "relative", width: "400px", fontFamily: "Menlo", fontSize: "16px", lineHeight: "20px", color: "#000" }
const UNIT = 1 / 64

const style = (...styles: Style[]): CSSProperties => Object.assign({}, BASE, ...styles) as CSSProperties

function CaseView({ spec }: { spec: Spec }) {
  return (
    <div testId="case" style={{ ...BASE, ...CASE } as CSSProperties}>
      <div testId="outer" style={style(spec.outer)}>
        <div testId="before" style={style({ width: "20px", height: "11px" }, spec.before)} />
        <div testId="target" style={style(spec.target)}>
          <div testId="child" style={style({ width: "150px", height: "300px" }, spec.child)} />
        </div>
        <div testId="after" style={style({ width: "30px", height: "13px" }, spec.after)} />
      </div>
    </div>
  )
}

describeNative("content-visibility against Chrome", () => {
  const root = createTestRoot({ width: 700, height: 900 })
  afterAll(() => root.unmount())

  const bounds = (testId: string) => {
    const element = root.renderer.findByTestId(testId)
    expect(element, `no element with testId ${testId}`).toBeDefined()
    const found = root.renderer.getElementBounds(element!.id)
    expect(found, `no painted bounds for ${testId}`).toBeDefined()
    return found!
  }

  it("has an oracle for every case", () => {
    expect(Object.keys(ORACLE)).toEqual(CASES.map((spec) => spec.name))
    expect(CASES.length).toBeGreaterThan(80)
  })

  for (const spec of CASES) {
    it(`lays out ${spec.name} as Chrome does`, () => {
      root.render(<CaseView spec={spec} />)
      root.renderer.dispatchNativeEvents()
      root.renderer.drawFrame()
      const origin = bounds("case")
      const expected = ORACLE[spec.name]!
      const mismatches: string[] = []
      for (const [key, chrome] of Object.entries(expected)) {
        const box = bounds(key)
        const actual = [box.x - origin.x, box.y - origin.y, box.width, box.height]
        if (actual.some((value, index) => Math.abs(value - chrome[index]!) > UNIT)) {
          mismatches.push(`${key} [${actual.join(", ")}] != Chrome's [${chrome.join(", ")}]`)
        }
      }
      expect(mismatches, spec.name).toEqual([])
    })
  }
})

// ── What the containment does that a box does not show ──

const PAGE = { width: 400, height: 300 }
const pixelsOf = (renderer: ReturnType<typeof createTestRoot>["renderer"], name: string) => {
  const file = `${SHOTS_DIR}/content-visibility-${process.pid}-${name}.png`
  fs.rmSync(file, { force: true })
  renderer.captureScreenshot(file)
  const pixels = readLogicalPixels(file, renderer)
  fs.rmSync(file, { force: true })
  return pixels
}

const isRed = ([r, g, b]: number[]) => r > 200 && g < 60 && b < 60
const isBlue = ([r, g, b]: number[]) => b > 200 && r < 60 && g < 60

describeNative("the containment of content-visibility", () => {
  const draw = (node: React.ReactNode) => {
    const root = createTestRoot({ width: PAGE.width, height: PAGE.height })
    root.render(node)
    root.renderer.dispatchNativeEvents()
    root.renderer.drawFrame()
    return root
  }

  const overflowing = (contentVisibility: string | undefined, extra: Style = {}) => (
    <div style={{ width: PAGE.width, height: PAGE.height, backgroundColor: "#ffffff", display: "block" }}>
      <div
        testId="box"
        style={{ width: 100, height: 100, backgroundColor: "#00ff00", contentVisibility, ...extra } as CSSProperties}
      >
        <div style={{ width: 100, height: 250, backgroundColor: "#ff0000" }} />
      </div>
    </div>
  )

  it("clips the content at the padding box, as overflow: clip does, and only when content-visibility is set", () => {
    for (const [value, clipped] of [
      ["auto", true],
      ["hidden", true],
      ["visible", false],
      [undefined, false],
    ] as const) {
      const root = draw(overflowing(value))
      try {
        const below = pixelsOf(root.renderer, `clip-${value}`).at(50, 180)
        expect(isRed(below), `${value}: ${below}`).toBe(!clipped)
      } finally {
        root.unmount()
      }
    }
  })

  it("keeps the clip an author's overflow already makes", () => {
    const root = draw(overflowing("auto", { overflow: "hidden" }))
    try {
      expect(isRed(pixelsOf(root.renderer, "clip-hidden").at(50, 180))).toBe(false)
    } finally {
      root.unmount()
    }
  })

  it("paints the background and the border of a skipped element and not its children", () => {
    const root = draw(
      <div style={{ width: PAGE.width, height: PAGE.height, backgroundColor: "#ffffff", display: "block" }}>
        <div
          testId="box"
          style={{
            width: 100,
            height: 100,
            backgroundColor: "#00ff00",
            border: "4px solid #0000ff",
            contentVisibility: "hidden",
          }}
        >
          <div style={{ width: 90, height: 90, backgroundColor: "#ff0000" }} />
        </div>
      </div>
    )
    try {
      const pixels = pixelsOf(root.renderer, "hidden")
      expect(pixels.at(50, 50)).toEqual([0, 255, 0, 255])
      expect(isBlue(pixels.at(2, 50))).toBe(true)
      expect(root.renderer.getElementsAt(50, 50).includes(root.renderer.findByTestId("box")!.id)).toBe(true)
    } finally {
      root.unmount()
    }
  })

  it("is a stacking context: a z-index inside it stays inside", () => {
    // A (the container) holds a red child at z-index 10, B is a later positioned sibling at z-index 5 over it. In a
    // stacking context A is one layer of z-index auto, so B is over all of it. Without, the red child is over B.
    for (const [value, blueOnTop] of [
      ["hidden", true],
      ["auto", true],
      [undefined, false],
    ] as const) {
      const root = draw(
        <div style={{ width: PAGE.width, height: PAGE.height, backgroundColor: "#ffffff", display: "block" }}>
          <div style={{ width: 100, height: 100, contentVisibility: value } as CSSProperties}>
            {value === "hidden" ? null : (
              <div style={{ width: 100, height: 100, position: "relative", zIndex: 10, backgroundColor: "#ff0000" }} />
            )}
          </div>
          <div
            testId="b"
            style={{ position: "relative", zIndex: 5, top: -100, width: 100, height: 100, backgroundColor: "#0000ff" }}
          />
        </div>
      )
      try {
        const pixel = pixelsOf(root.renderer, `stack-${value}`).at(50, 50)
        // A skipped element holds nothing to be over B.
        expect(isBlue(pixel), `${value}: ${pixel}`).toBe(value === "hidden" ? true : blueOnTop)
      } finally {
        root.unmount()
      }
    }
  })
})

// ── What a skipped subtree costs ──

const ROWS = 200
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

type Counts = Record<string, number>

function counts(renderer: ReturnType<typeof createTestRoot>["renderer"]): Counts {
  return { ...renderer.getLayoutCounters(), elementsBuilt: renderer.getBuildCounters().elementsBuilt }
}

/** What `change` and the frame it is drawn in did, as the difference of the counters. */
function work(renderer: ReturnType<typeof createTestRoot>["renderer"], change: () => void): Counts {
  const before = counts(renderer)
  change()
  renderer.drawFrame()
  const after = counts(renderer)
  const done: Counts = {}
  for (const key of Object.keys(after)) done[key] = after[key]! - before[key]!
  return done
}

const Rows = ({ count }: { count: number }) => (
  <>
    {Array.from({ length: count }, (_, index) => (
      <div key={index} style={{ height: 18, display: "block" }}>
        <text style={{ fontSize: 13, color: "#222222" }}>{`Row ${index} of the page that is not shown`}</text>
      </div>
    ))}
  </>
)

describeNative("a skipped subtree", { timeout: 120_000 }, () => {
  const page = (value: "hidden" | "visible", rows: number) => (
    <div style={{ width: 500, height: 600, display: "block" }}>
      <div testId="skipper" style={{ contentVisibility: value, containIntrinsicSize: "100px 40px" } as CSSProperties}>
        <Rows count={rows} />
      </div>
    </div>
  )

  it("lays out, shapes and creates nothing for 200 rows", async () => {
    const root = createTestRoot({ width: 500, height: 600 })
    const renderer = root.renderer
    const skipperBox = () => renderer.getElementBounds(renderer.findByTestId("skipper")!.id)!
    try {
      const first = work(renderer, () => root.render(page("hidden", ROWS)))
      expect(first.shapes, "shapes").toBe(0)
      expect(first.measures, "measures").toBe(0)
      expect(first.nodesCreated, "layout nodes: the page and the element itself").toBe(2)
      expect(first.elementsBuilt, "elements built: the same two").toBe(2)
      // Its height is the intrinsic one, whatever the rows would have been (a block is as wide as its parent).
      expect([skipperBox().width, skipperBox().height]).toEqual([500, 40])

      const more = work(renderer, () => root.render(page("hidden", ROWS + 50)))
      expect(more.shapes).toBe(0)
      expect(more.nodesCreated, "rows added below a skipped element get no node").toBe(0)

      const shown = work(renderer, () => root.render(page("visible", ROWS + 50)))
      expect(shown.shapes, "the rows are shaped when the value goes back to visible").toBeGreaterThanOrEqual(ROWS + 50)
      expect(shown.nodesCreated).toBeGreaterThanOrEqual(ROWS + 50)
      expect(skipperBox().height).toBe((ROWS + 50) * 18)

      const again = work(renderer, () => root.render(page("hidden", ROWS + 50)))
      expect(again.shapes).toBe(0)
      expect(skipperBox().height).toBe(40)
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })

  it("does not rebuild anything on an idle frame, and keeps building nothing for a change below it", async () => {
    const root = createTestRoot({ width: 500, height: 600 })
    try {
      root.render(page("hidden", ROWS))
      for (let frame = 0; frame < 6; frame++) root.renderer.drawFrame()
      expect(work(root.renderer, () => {}).elementsBuilt).toBe(0)
      // The DOM below changes: its rows are not built.
      const changed = work(root.renderer, () => root.render(page("hidden", ROWS + 20)))
      expect(changed.shapes).toBe(0)
      expect(changed.measures).toBe(0)
      root.renderer.drawFrame()
      expect(work(root.renderer, () => {}).elementsBuilt).toBe(0)
    } finally {
      root.unmount()
    }
  })
})

defineKeyframes("cv-pulse", {
  "0%": { color: "#ff0000", backgroundColor: "#ff0000" },
  "100%": { color: "#0000ff", backgroundColor: "#0000ff" },
})

describeNative("animations in a skipped subtree", { timeout: 120_000 }, () => {
  /** `below` puts the element past the window, where its paint-only loop is dormant (carried on without a build). */
  const scene = (value: "hidden" | "visible", below: boolean, onEvent: (kind: string) => void) => (
    <div style={{ width: 400, height: 300, display: "block" }}>
      {below ? <div style={{ width: 10, height: 1000 }} /> : null}
      <div testId="skipper" style={{ contentVisibility: value, containIntrinsicSize: "50px 20px" } as CSSProperties}>
        <div
          testId="loop"
          onAnimationStart={() => onEvent("start")}
          onAnimationIteration={() => onEvent("iteration")}
          onAnimationEnd={() => onEvent("end")}
          style={{ height: 20, width: 60, animation: "cv-pulse 1s linear infinite" }}
        />
      </div>
      <div testId="sibling" style={{ width: 30, height: 30, backgroundColor: "#cccccc" }} />
    </div>
  )

  for (const below of [false, true]) {
    it(`do not tick or fire events while skipped, and go on from the timeline when shown${below ? ", out of the window" : ""}`, () => {
      const events: string[] = []
      const root = createTestRoot({ width: 400, height: 300 })
      const renderer = root.renderer
      const show = (value: "hidden" | "visible") => root.render(scene(value, below, (kind) => events.push(kind)))
      try {
        renderer.clockPause()
        show("visible")
        // Enough frames for a loop out of the window to be dormant.
        for (let step = 0; step < 12; step++) {
          renderer.clockFastForward(100)
          renderer.drawFrame()
          renderer.dispatchNativeEvents()
        }
        const loop = renderer.findByTestId("loop")!.id
        expect(renderer.getAnimations(loop).length).toBe(1)
        const before = renderer.getAnimations(loop)[0]!.currentTime
        expect(events.filter((kind) => kind === "iteration").length, "it loops while shown").toBeGreaterThanOrEqual(1)

        show("hidden")
        renderer.drawFrame()
        events.length = 0
        // 3.5 s pass: three iterations that nobody sees.
        let built = 0
        for (let step = 0; step < 35; step++) {
          const done = work(renderer, () => renderer.clockFastForward(100))
          // The first frame after the change settles what the element was running.
          if (step > 0) built += done.elementsBuilt
          renderer.dispatchNativeEvents()
        }
        expect(events, "no event while skipped").toEqual([])
        // A verified run builds every frame twice over, so it only shows that the loop below is not built.
        expect(built, "nothing builds for a loop nobody sees").toBeLessThanOrEqual(verifying ? 5 * 35 : 0)

        show("visible")
        renderer.drawFrame()
        renderer.dispatchNativeEvents()
        const after = renderer.getAnimations(loop)
        expect(after.length, "the animation is the one that was running").toBe(1)
        expect(after[0]!.currentTime, "it is where the timeline is").toBeGreaterThan(before + 3400)
        // It samples the time it finds: the iterations missed while skipped are not replayed.
        expect(events.filter((kind) => kind === "iteration").length).toBeLessThanOrEqual(1)
        expect(events).not.toContain("start")
        if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
      } finally {
        root.unmount()
      }
    })
  }
})

describeNative("a tick whose only change is in skipped content", { timeout: 120_000 }, () => {
  const CARDS = 8
  const WRITES = 20

  /** Time passes as it does for a window: both clocks move, the frame callbacks and a pending change are drawn, events are delivered. */
  const elapse = (renderer: ReturnType<typeof createTestRoot>["renderer"]) => {
    renderer.clockAdvance(16)
    renderer.advanceTime(16)
    renderer.drawFrame()
    renderer.dispatchNativeEvents()
  }

  it("draws nothing and is counted as an empty tick: a motion value bound inside hidden content is written", () => {
    const tints = Array.from({ length: CARDS }, () => motionValue("#ff0000"))
    const root = createTestRoot({ width: 400, height: 600 })
    const renderer = root.renderer
    try {
      renderer.clockPause()
      root.render(
        <div style={{ width: 400, height: 600, flexDirection: "column" }}>
          {tints.map((tint, index) => (
            <div key={index} style={{ contentVisibility: "hidden", containIntrinsicSize: "80px 60px", flexShrink: 0 } as CSSProperties}>
              <motion.div testId={`bound-${index}`} style={{ width: 40, height: 40, backgroundColor: tint }} />
            </div>
          ))}
        </div>
      )
      renderer.flush()
      renderer.setInlineDraws(false)
      renderer.setA11yActive(false)
      for (let tick = 0; tick < 12; tick++) elapse(renderer)
      const before = renderer.getPatchCounters()
      const painted = renderer.getPaintedFrameCount()
      for (let write = 0; write < WRITES; write++) {
        for (const tint of tints) tint.set(write % 2 === 0 ? "#0000ff" : "#ff0000")
        elapse(renderer)
      }
      const after = renderer.getPatchCounters()
      expect(renderer.getPaintedFrameCount() - painted, "no frame is built").toBe(0)
      expect(after.patchedFrames - before.patchedFrames).toBe(0)
      expect(after.declines).toEqual(before.declines)
      expect(after.unpatchable).toEqual(before.unpatchable)
      expect(after.emptyTicks - before.emptyTicks, "a tick for each write").toBe(WRITES)
    } finally {
      root.unmount()
    }
  })
})
