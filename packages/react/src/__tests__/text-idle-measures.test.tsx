/// A text that did not change costs no measure and no shaping in a frame, whether the frame is idle or one node
/// animates, whether the text is shown or hidden (`opacity: 0` ghost copies, `visibility: hidden`) and whether
/// retention skips its subtree or not: the layout node keeps the measure (step 3c) and the text keeps its shaped lines.
/// A browser lays a hidden copy out once and then pays nothing for it.

import fs from "fs"
import { describe, expect, it } from "vitest"
import { motion } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SQUARE_FONT = fs.readFileSync(new URL("./fixtures/gpuix-test-square.ttf", import.meta.url))
const FRAME_MS = 1000 / 75
const FRAMES = 20
const CARDS = 12

type Mover = "none" | "opacity" | "left" | "margin"

/** One card of labels the way a stable-label design system writes them: the visible text, an `opacity: 0` copy of
 *  the other weight in the same cell, a `visibility: hidden` one, and a wrapping paragraph. */
function Card({ index }: { index: number }) {
  return (
    <div style={{ width: 240, display: "flex", flexDirection: "column" }}>
      <div style={{ display: "grid" }}>
        <span style={{ gridColumn: 1, gridRow: 1, fontWeight: 400 }}>AAAA {index}</span>
        <span style={{ gridColumn: 1, gridRow: 1, fontWeight: 700, opacity: 0 }}>AAAA {index}</span>
      </div>
      <text style={{ visibility: "hidden" }}>AA AAA {index}</text>
      <text style={{ opacity: 0 }}>AAAA AAAA {index}</text>
      <div style={{ width: 120 }}>AAAA AAAA AAAA AAAA AAAA {index}</div>
    </div>
  )
}

/** The inline shapes that keep a text from a measure key: a padded span (insets that depend on the width) and an
 *  atomic inline (the stable label of a design system is an `inline-grid` span holding a hidden copy of the other weight). */
function InlineCard({ index }: { index: number }) {
  return (
    <div style={{ width: 240 }}>
      <div>
        AA <span style={{ padding: "0 4px", backgroundColor: "#cccccc" }}>AAA {index}</span> AA <span style={{ fontWeight: 700 }}>AAA</span>
      </div>
      <div>
        AA{" "}
        <span style={{ display: "inline-grid", whiteSpace: "nowrap" }}>
          <span style={{ gridColumn: 1, gridRow: 1, visibility: "hidden", fontWeight: 700 }}>AAAA {index}</span>
          <span style={{ gridColumn: 1, gridRow: 1 }}>AAA {index}</span>
        </span>{" "}
        AA
      </div>
      <div>
        AA <span style={{ display: "inline-block", width: 30, height: 10 }}>AA</span> AA
      </div>
    </div>
  )
}

function Page({ mover }: { mover: Mover }) {
  return (
    <div style={{ position: "relative", width: 1000, height: 700, fontFamily: "GpuixTestSquare", fontSize: 12, lineHeight: "14px", color: "#000000" }}>
      {Array.from({ length: CARDS }, (_, index) => (
        <Card key={index} index={index} />
      ))}
      {Array.from({ length: CARDS }, (_, index) => (
        <InlineCard key={index} index={index} />
      ))}
      {mover === "opacity" ? (
        <motion.div
          testId="mover"
          initial={{ opacity: 0.1 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 10, ease: "linear" }}
          style={{ position: "absolute", left: 300, top: 0, width: 200 }}
        >
          <text>AAA AAA</text>
          <text style={{ opacity: 0 }}>AAA AAAA</text>
        </motion.div>
      ) : null}
      {mover === "left" ? (
        <motion.div
          testId="mover"
          initial={{ left: 300 }}
          animate={{ left: 700 }}
          transition={{ duration: 10, ease: "linear" }}
          style={{ position: "absolute", top: 0, width: 200 }}
        >
          <text>AAA AAA</text>
          <text style={{ opacity: 0 }}>AAA AAAA</text>
        </motion.div>
      ) : null}
      {mover === "margin" ? (
        <div style={{ position: "absolute", left: 300, top: 0, width: 400, display: "flex" }}>
          <motion.div
            testId="mover"
            initial={{ marginLeft: 0 }}
            animate={{ marginLeft: 100 }}
            transition={{ duration: 10, ease: "linear" }}
            style={{ width: 200 }}
          >
            <text>AAA AAA</text>
            <text style={{ opacity: 0 }}>AAA AAAA</text>
          </motion.div>
        </div>
      ) : null}
    </div>
  )
}

describeNative("a text that did not change", () => {
  for (const retention of [true, false]) {
    for (const mover of ["none", "opacity", "left", "margin"] as const) {
      it(`costs no measure and no shaping in a frame, ${mover === "none" ? "idle" : `${mover} animating`}, retention ${retention ? "on" : "off"}`, () => {
        const test = createTestRoot({ width: 1000, height: 700 })
        const renderer = test.renderer
        renderer.registerFont(SQUARE_FONT)
        try {
          renderer.clockPause()
          test.render(<Page mover={mover} />)
          renderer.dispatchNativeEvents()
          renderer.setRetainEnabled(retention)
          for (let frame = 0; frame < 5; frame++) renderer.clockFastForward(FRAME_MS)
          const before = renderer.getLayoutCounters()
          const moverBefore = mover === "none" ? 0 : renderer.getElementBounds(renderer.findByTestId("mover").id)!.x
          for (let frame = 0; frame < FRAMES; frame++) renderer.clockFastForward(FRAME_MS)
          const after = renderer.getLayoutCounters()
          const delta = (key: keyof typeof after) => after[key] - before[key]
          console.log(
            `[text-idle] ${mover} retention=${retention} per frame: measures=${delta("measures") / FRAMES} shapes=${delta("shapes") / FRAMES} ` +
              `wrapped=${delta("wrappedLayouts") / FRAMES} line-layout-hits=${delta("lineLayoutHits") / FRAMES} passes=${delta("passes") / FRAMES}`
          )
          if (mover === "left" || mover === "margin") {
            const moved = renderer.getElementBounds(renderer.findByTestId("mover").id)!.x
            expect(moved, "the node moved").toBeGreaterThan(moverBefore)
          }
          expect(delta("passes")).toBeGreaterThanOrEqual(FRAMES)
          expect(delta("measures")).toBe(0)
          expect(delta("shapes")).toBe(0)
          expect(delta("wrappedLayouts")).toBe(0)
        } finally {
          test.unmount()
        }
      })
    }
  }

  it("is measured and shaped again when its text changes, and only that text", () => {
    const test = createTestRoot({ width: 1000, height: 700 })
    const renderer = test.renderer
    renderer.registerFont(SQUARE_FONT)
    function Labels({ second }: { second: string }) {
      return (
        <div style={{ width: 400, fontFamily: "GpuixTestSquare", fontSize: 12, lineHeight: "14px", color: "#000000" }}>
          <div>AAAA AAAA</div>
          <div style={{ opacity: 0 }}>AAA AAA</div>
          <div>{second}</div>
        </div>
      )
    }
    try {
      renderer.clockPause()
      test.render(<Labels second="AA" />)
      renderer.dispatchNativeEvents()
      renderer.clockFastForward(FRAME_MS)
      const before = renderer.getLayoutCounters()
      test.render(<Labels second="AAAAA" />)
      renderer.dispatchNativeEvents()
      renderer.clockFastForward(FRAME_MS)
      const after = renderer.getLayoutCounters()
      expect(after.measures - before.measures).toBeGreaterThan(0)
      expect(after.shapes - before.shapes).toBe(1)
    } finally {
      test.unmount()
    }
  })
})
