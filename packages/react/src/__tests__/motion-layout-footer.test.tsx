/// A `layout="position"` footer under a region whose height springs does not dip when a commit lands in the
/// first frames of the spring (#118).
///
/// FF's ask-user-questions card: the next question swaps the content (a commit), `onLayout` reports the new
/// content height (a second commit) and the region springs to it. The buttons of the footer under the region are
/// `layout="position"` boxes in a scroller. Motion does not move them against the same card without `layout`
/// (`fixtures/motion-layout-footer-oracle.json`, recorded from the page of the same name: `layout-skip` and
/// `plain-skip` have the same y in every frame), and gpuix must not either. The commit that starts the spring
/// is a commit in a frame where the spring has already ticked, and so is any commit that lands while it runs (FF's
/// page has them every few frames): what the ticks moved is not what the commit moved.

import fs from "fs"
import path from "path"
import { useState } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const spring = { type: "spring", duration: 0.24, bounce: 0.12 } as const
const tween = { duration: 0.2, ease: "easeOut" } as const

const ORACLE: { scenes: { footer: { frames: { t: number; boxes: Record<string, { x: number; y: number }> }[] } } } = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/motion-layout-footer-oracle.json"),
    "utf8"
  )
)

function Card(props: { name: string; layout: boolean; question: number; tick: number }) {
  const [height, setHeight] = useState<number | undefined>(undefined)
  const rows = props.question === 0 ? 2 : 5
  const layout = props.layout ? "position" : undefined
  return (
    <div
      testId={`${props.name}-scroller`}
      style={{ display: "flex", flexDirection: "column", overflow: "scroll", width: 300, height: 360, flexShrink: 0 }}
    >
      <div style={{ height: 30, flexShrink: 0 }} />
      <motion.div
        initial={false}
        animate={{ height: height ?? "auto" }}
        transition={spring}
        style={{ overflow: "hidden", flexShrink: 0, display: "flex", flexDirection: "column" }}
      >
        <div
          style={{ flexShrink: 0 }}
          onLayout={(event) => {
            if (event.height === undefined) throw new Error("onLayout reported no height")
            setHeight(Math.round(event.height))
          }}
        >
          {Array.from({ length: rows }, (_, index) => (
            <div key={`${props.question}-${index}`} style={{ height: 40, backgroundColor: "#dddddd" }} />
          ))}
        </div>
      </motion.div>
      <div style={{ display: "flex", flexDirection: "row", gap: 8, padding: 8, flexShrink: 0 }}>
        {props.question > 0 ? (
          <motion.div testId={`${props.name}-back`} layout={layout} transition={tween} style={{ width: 60, height: 28, backgroundColor: "#0000ff" }} />
        ) : null}
        <motion.div testId={`${props.name}-skip`} layout={layout} transition={tween} style={{ width: 60, height: 28, backgroundColor: "#ff0000" }} />
      </div>
      <div style={{ height: 4, flexShrink: 0 }}>
        <text>{String(props.tick)}</text>
      </div>
      <div style={{ height: 900, flexShrink: 0 }} />
    </div>
  )
}

function App(props: { question: number; tick: number }) {
  return (
    <div style={{ display: "flex", flexDirection: "row" }}>
      <Card name="layout" layout question={props.question} tick={props.tick} />
      <Card name="plain" layout={false} question={props.question} tick={props.tick} />
    </div>
  )
}

function boundsOf(renderer: TestRenderer, testId: string) {
  return renderer.getElementBounds(renderer.findByTestId(testId).id)
}

describe("Motion 14.1.0 in Chrome", () => {
  it("has the footer button of a `layout` card where the one of a plain card is, in every frame", () => {
    const frames = ORACLE.scenes.footer.frames.filter((frame) => frame.t > 0)
    expect(frames.length).toBeGreaterThan(40)
    for (const frame of frames) {
      expect(frame.boxes["layout-skip"]!.y, `t=${frame.t.toFixed(0)}`).toBeCloseTo(frame.boxes["plain-skip"]!.y, 5)
    }
  })
})

describeNative("a layout footer under a region that springs", () => {
  it("is where the footer of a card without layout is, in every frame, and glides only the move Back makes", () => {
    const { render, renderer, unmount } = createTestRoot({ width: 700, height: 400 })
    try {
      renderer.clockPause()
      // As under the display link: a commit lands between frames, and the next frame draws it with its ticks.
      renderer.setInlineDraws(false)
      render(<App question={0} tick={0} />)
      renderer.drawFrame()
      renderer.clockFastForward(100)
      renderer.drawFrame()
      renderer.dispatchNativeEvents()
      renderer.drawFrame()
      renderer.clockFastForward(300)
      renderer.drawFrame()
      render(<App question={1} tick={0} />)
      const worst: string[] = []
      let xGlided = false
      for (let elapsed = 0; elapsed < 800; elapsed += 8) {
        // Two commits land while the spring runs, as FF's page has them: the page re-renders with another tick.
        if (elapsed === 24 || elapsed === 48) render(<App question={1} tick={elapsed} />)
        renderer.clockFastForward(8)
        renderer.drawFrame()
        renderer.dispatchNativeEvents()
        const ours = boundsOf(renderer, "layout-skip")
        const plain = boundsOf(renderer, "plain-skip")
        if (Math.abs(ours.y - plain.y) > 0.5) worst.push(`t=${elapsed + 8}: layout ${ours.y.toFixed(1)}, plain ${plain.y.toFixed(1)}`)
        // Back arrives at the left of Skip: the plain card's Skip is at its final x at once, the layout card's glides there.
        if (Math.abs(ours.x - (plain.x - 300)) > 0.5 && elapsed < 200) xGlided = true
      }
      expect(worst).toEqual([])
      expect(xGlided, "Skip still glides to the place Back made").toBe(true)
    } finally {
      unmount()
    }
  })
})
