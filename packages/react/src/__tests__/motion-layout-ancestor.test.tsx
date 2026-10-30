import { describe, expect, it } from "vitest"
import { motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const linear = { duration: 1, ease: "linear" } as const
const column = { display: "flex", flexDirection: "column" } as const

function frame(renderer: TestRenderer, ms: number): void {
  renderer.clockFastForward(ms)
  renderer.flush()
  renderer.dispatchNativeEvents()
}

function boundsOf(renderer: TestRenderer, testId: string) {
  return renderer.getElementBounds(renderer.findByTestId(testId).id)
}

// Two identical stacks: a region whose height an animation drives, and rows under it. Only the
// rows of the first stack have `layout`.
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

describeNative("layout under an animating ancestor", () => {
  it("follows a height animation above it at once, as a box without layout does", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(<Stacks open={false} />)
    render(<Stacks open />)
    for (let step = 0; step < 12; step += 1) {
      frame(renderer, 50)
      expect(boundsOf(renderer, "glide-row").y).toBeCloseTo(boundsOf(renderer, "plain-row").y, 1)
    }
    expect(boundsOf(renderer, "plain-row").y).toBeGreaterThan(70)
  })

  it("keeps a box still under scroll anchoring while the region above it animates", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const scroller = (name: string, layout: boolean, open: boolean) => (
      <div testId={`${name}-scroller`} style={{ ...column, width: 100, height: 100, overflow: "scroll" }}>
        <motion.div
          animate={{ height: open ? 120 : 20 }}
          initial={false}
          transition={linear}
          style={{ width: 100, overflow: "hidden", flexShrink: 0 }}
        />
        <motion.div testId={`${name}-row`} layout={layout} transition={linear} style={{ height: 20, width: 50, flexShrink: 0 }} />
        <div style={{ height: 400, flexShrink: 0 }} />
      </div>
    )
    const app = (open: boolean) => (
      <div style={{ display: "flex", flexDirection: "row" }}>
        {scroller("glide", true, open)}
        {scroller("plain", false, open)}
      </div>
    )
    render(app(false))
    for (const name of ["glide", "plain"]) renderer.scrollTo(renderer.findByTestId(`${name}-scroller`).id, 0, 10)
    frame(renderer, 16)
    expect(boundsOf(renderer, "glide-row").y).toBeCloseTo(10, 1)

    render(app(true))
    for (let step = 0; step < 12; step += 1) {
      frame(renderer, 50)
      expect(boundsOf(renderer, "glide-row").y).toBeCloseTo(boundsOf(renderer, "plain-row").y, 1)
    }
  })

  it("glides what a commit moves while an animation above it keeps running", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const stack = (name: string, layout: boolean, open: boolean, inserted: boolean) => (
      <div style={{ ...column, width: 100 }}>
        <motion.div
          animate={{ height: open ? 120 : 20 }}
          initial={false}
          transition={linear}
          style={{ width: 100, overflow: "hidden", flexShrink: 0 }}
        />
        {inserted ? <div style={{ height: 30, flexShrink: 0 }} /> : null}
        <motion.div testId={`${name}-row`} layout={layout} transition={linear} style={{ height: 20, width: 50 }} />
      </div>
    )
    const app = (open: boolean, inserted: boolean) => (
      <div style={{ display: "flex", flexDirection: "row" }}>
        {stack("glide", true, open, inserted)}
        {stack("plain", false, open, inserted)}
      </div>
    )
    render(app(false, false))
    render(app(true, false))
    frame(renderer, 300)
    const before = boundsOf(renderer, "glide-row").y
    expect(before).toBeCloseTo(boundsOf(renderer, "plain-row").y, 1)

    // The 30 px block comes in at this commit: the box shows where it was, then glides the 30 px
    // while the region above goes on.
    render(app(true, true))
    expect(boundsOf(renderer, "glide-row").y).toBeCloseTo(before, 1)
    frame(renderer, 500)
    expect(boundsOf(renderer, "plain-row").y - boundsOf(renderer, "glide-row").y).toBeCloseTo(15, 0)
    frame(renderer, 600)
    expect(boundsOf(renderer, "glide-row").y).toBeCloseTo(boundsOf(renderer, "plain-row").y, 1)
  })
})
