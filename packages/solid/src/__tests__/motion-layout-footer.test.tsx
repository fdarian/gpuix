/// The Solid half of `motion-layout-footer.test.tsx` of the React package (#118): FF's ask-user-questions card, with
/// `layout="position"` buttons in a footer under a region whose height springs to the content `onLayout` measured. The
/// commit that starts the spring is a commit in a frame where the spring has already ticked; what the ticks moved is
/// not what the commit moved, and Motion does not move the footer against the same card without `layout`
/// (`packages/react/src/__tests__/fixtures/motion-layout-footer-oracle.json`).

import { createSignal, Index, Show } from "solid-js"
import { describe, expect, it } from "bun:test"
import { motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

const spring = { type: "spring", duration: 0.24, bounce: 0.12 } as const
const tween = { duration: 0.2, ease: "easeOut" } as const

function Card(props: { name: string; layout: boolean; question: () => number; tick: () => number }) {
  const [height, setHeight] = createSignal<number | undefined>(undefined)
  const rows = () => Array.from({ length: props.question() === 0 ? 2 : 5 }, (_, index) => index)
  const layout = props.layout ? ("position" as const) : undefined
  return (
    <div
      testId={`${props.name}-scroller`}
      style={{ display: "flex", flexDirection: "column", overflow: "scroll", width: 300, height: 360, flexShrink: 0 }}
    >
      <div style={{ height: 30, flexShrink: 0 }} />
      <motion.div
        initial={false}
        animate={{ height: height() ?? "auto" }}
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
          <Index each={rows()}>{() => <div style={{ height: 40, backgroundColor: "#dddddd" }} />}</Index>
        </div>
      </motion.div>
      <div style={{ display: "flex", flexDirection: "row", gap: 8, padding: 8, flexShrink: 0 }}>
        <Show when={props.question() > 0}>
          <motion.div testId={`${props.name}-back`} layout={layout} transition={tween} style={{ width: 60, height: 28, backgroundColor: "#0000ff" }} />
        </Show>
        <motion.div testId={`${props.name}-skip`} layout={layout} transition={tween} style={{ width: 60, height: 28, backgroundColor: "#ff0000" }} />
      </div>
      <div style={{ height: 4, flexShrink: 0 }}>
        <text>{String(props.tick())}</text>
      </div>
      <div style={{ height: 900, flexShrink: 0 }} />
    </div>
  )
}

function boundsOf(renderer: TestRenderer, testId: string) {
  return renderer.getElementBounds(renderer.findByTestId(testId)!.id)!
}

describe.skipIf(!hasNativeTestRenderer)("Solid: a layout footer under a region that springs", () => {
  it("is where the footer of a card without layout is, in every frame, and glides only the move Back makes", () => {
    const [question, setQuestion] = createSignal(0)
    const [tick, setTick] = createSignal(0)
    const app = createTestRoot({ width: 700, height: 400 })
    const renderer = app.renderer
    try {
      renderer.clockPause()
      // As under the display link: a commit lands between frames, and the next frame draws it with its ticks.
      renderer.setInlineDraws(false)
      app.render(() => (
        <div style={{ display: "flex", flexDirection: "row" }}>
          <Card name="layout" layout question={question} tick={tick} />
          <Card name="plain" layout={false} question={question} tick={tick} />
        </div>
      ))
      renderer.drawFrame()
      renderer.clockFastForward(100)
      renderer.drawFrame()
      renderer.dispatchNativeEvents()
      renderer.drawFrame()
      renderer.clockFastForward(300)
      renderer.drawFrame()
      app.flushSync(() => setQuestion(1))
      const worst: string[] = []
      let xGlided = false
      for (let elapsed = 0; elapsed < 800; elapsed += 8) {
        // Two commits land while the spring runs, as FF's page has them.
        if (elapsed === 24 || elapsed === 48) app.flushSync(() => setTick(elapsed))
        renderer.clockFastForward(8)
        renderer.drawFrame()
        renderer.dispatchNativeEvents()
        const ours = boundsOf(renderer, "layout-skip")
        const plain = boundsOf(renderer, "plain-skip")
        if (Math.abs(ours.y - plain.y) > 0.5) worst.push(`t=${elapsed + 8}: layout ${ours.y.toFixed(1)}, plain ${plain.y.toFixed(1)}`)
        if (Math.abs(ours.x - (plain.x - 300)) > 0.5 && elapsed < 200) xGlided = true
      }
      expect(worst).toEqual([])
      expect(xGlided).toBe(true)
    } finally {
      app.unmount()
    }
  })
})
