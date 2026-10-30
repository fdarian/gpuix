import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

const linear = { duration: 1, ease: "linear" } as const
const column = { display: "flex", flexDirection: "column" } as const

function frame(renderer: TestRenderer, ms: number): void {
  renderer.clockFastForward(ms)
  renderer.flush()
  renderer.dispatchNativeEvents()
}

function boundsOf(renderer: TestRenderer, testId: string) {
  return renderer.getElementBounds(renderer.findByTestId(testId)!.id)!
}

// Two identical stacks: a region whose height an animation drives, and a row under it. Only the
// row of the first stack has `layout`.
function Stacks(props: { open: boolean }) {
  const stack = (name: string, layout: boolean) => (
    <div style={{ ...column, width: 100 }}>
      <motion.div
        animate={{ height: props.open ? 120 : 20 }}
        initial={false}
        transition={linear}
        style={{ width: 100, overflow: "hidden" }}
      />
      <motion.div testId={`${name}-row`} layout={layout} transition={linear} style={{ height: 20, width: 50 }} />
    </div>
  )
  return (
    <div style={{ display: "flex", flexDirection: "row" }}>
      {stack("glide", true)}
      {stack("plain", false)}
    </div>
  )
}

describe.skipIf(!hasNativeTestRenderer)("Solid layout under an animating ancestor", () => {
  it("follows a height animation above it at once, as a box without layout does", () => {
    const [open, setOpen] = createSignal(false)
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => <Stacks open={open()} />)
    app.flushSync(() => setOpen(true))
    for (let step = 0; step < 12; step += 1) {
      frame(app.renderer, 50)
      expect(boundsOf(app.renderer, "glide-row").y).toBeCloseTo(boundsOf(app.renderer, "plain-row").y, 1)
    }
    expect(boundsOf(app.renderer, "plain-row").y).toBeGreaterThan(70)
  })

  it("keeps a box still under scroll anchoring while the region above it animates", () => {
    const [open, setOpen] = createSignal(false)
    const app = createTestRoot()
    app.renderer.clockPause()
    const scroller = (name: string, layout: boolean) => (
      <div testId={`${name}-scroller`} style={{ ...column, width: 100, height: 100, overflow: "scroll" }}>
        <motion.div
          animate={{ height: open() ? 120 : 20 }}
          initial={false}
          transition={linear}
          style={{ width: 100, overflow: "hidden", flexShrink: 0 }}
        />
        <motion.div testId={`${name}-row`} layout={layout} transition={linear} style={{ height: 20, width: 50, flexShrink: 0 }} />
        <div style={{ height: 400, flexShrink: 0 }} />
      </div>
    )
    app.render(() => (
      <div style={{ display: "flex", flexDirection: "row" }}>
        {scroller("glide", true)}
        {scroller("plain", false)}
      </div>
    ))
    for (const name of ["glide", "plain"]) app.renderer.scrollTo(app.renderer.findByTestId(`${name}-scroller`)!.id, 0, 10)
    frame(app.renderer, 16)
    expect(boundsOf(app.renderer, "glide-row").y).toBeCloseTo(10, 1)

    app.flushSync(() => setOpen(true))
    for (let step = 0; step < 12; step += 1) {
      frame(app.renderer, 50)
      expect(boundsOf(app.renderer, "glide-row").y).toBeCloseTo(boundsOf(app.renderer, "plain-row").y, 1)
    }
  })

  it("glides what a commit moves while an animation above it keeps running", () => {
    const [open, setOpen] = createSignal(false)
    const [inserted, setInserted] = createSignal(false)
    const app = createTestRoot()
    app.renderer.clockPause()
    const stack = (name: string, layout: boolean) => (
      <div style={{ ...column, width: 100 }}>
        <motion.div
          animate={{ height: open() ? 120 : 20 }}
          initial={false}
          transition={linear}
          style={{ width: 100, overflow: "hidden", flexShrink: 0 }}
        />
        {inserted() ? <div style={{ height: 30, flexShrink: 0 }} /> : null}
        <motion.div testId={`${name}-row`} layout={layout} transition={linear} style={{ height: 20, width: 50 }} />
      </div>
    )
    app.render(() => (
      <div style={{ display: "flex", flexDirection: "row" }}>
        {stack("glide", true)}
        {stack("plain", false)}
      </div>
    ))
    app.flushSync(() => setOpen(true))
    frame(app.renderer, 300)
    const before = boundsOf(app.renderer, "glide-row").y
    expect(before).toBeCloseTo(boundsOf(app.renderer, "plain-row").y, 1)

    app.flushSync(() => setInserted(true))
    expect(boundsOf(app.renderer, "glide-row").y).toBeCloseTo(before, 1)
    frame(app.renderer, 500)
    expect(boundsOf(app.renderer, "plain-row").y - boundsOf(app.renderer, "glide-row").y).toBeCloseTo(15, 0)
    frame(app.renderer, 600)
    expect(boundsOf(app.renderer, "glide-row").y).toBeCloseTo(boundsOf(app.renderer, "plain-row").y, 1)
  })
})
