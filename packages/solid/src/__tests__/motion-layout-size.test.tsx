import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

const linear = { duration: 1, ease: "linear" } as const

function frame(renderer: TestRenderer, ms: number): void {
  renderer.clockFastForward(ms)
  renderer.flush()
  renderer.dispatchNativeEvents()
}

function boundsOf(renderer: TestRenderer, testId: string) {
  return renderer.getElementBounds(renderer.findByTestId(testId)!.id)!
}

describe.skipIf(!hasNativeTestRenderer)("Solid layout size", () => {
  const mount = (layout: boolean | "position" | "size") => {
    const [height, setHeight] = createSignal(40)
    const [above, setAbove] = createSignal(0)
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <div style={{ display: "flex", flexDirection: "column", width: 300 }}>
        <div style={{ height: above(), flexShrink: 0 }} />
        <motion.div
          testId="box"
          layout={layout}
          transition={linear}
          style={{ width: 100, height: height(), flexShrink: 0 }}
        >
          <div testId="plain" style={{ width: 100, height: 20, flexShrink: 0 }} />
        </motion.div>
      </div>
    ))
    return { app, setHeight, setAbove }
  }

  it("glides a height change and a move together, about the centre", () => {
    const { app, setHeight, setAbove } = mount(true)
    app.flushSync(() => {
      setHeight(80)
      setAbove(30)
    })
    expect(boundsOf(app.renderer, "box")).toMatchObject({ y: 0, height: 40 })
    frame(app.renderer, 500)
    expect(boundsOf(app.renderer, "box").y).toBeCloseTo(15, 1)
    expect(boundsOf(app.renderer, "box").height).toBeCloseTo(60, 1)
    frame(app.renderer, 600)
    expect(boundsOf(app.renderer, "box")).toMatchObject({ y: 30, height: 80 })
  })

  it('"position" snaps the size and "size" snaps the corner', () => {
    const position = mount("position")
    position.app.flushSync(() => {
      position.setHeight(80)
      position.setAbove(30)
    })
    expect(boundsOf(position.app.renderer, "box")).toMatchObject({ y: 0, height: 80 })

    const size = mount("size")
    size.app.flushSync(() => {
      size.setHeight(80)
      size.setAbove(30)
    })
    expect(boundsOf(size.app.renderer, "box").y).toBeCloseTo(30, 1)
    expect(boundsOf(size.app.renderer, "box").height).toBeCloseTo(40, 1)
  })

  it("stretches what is inside the box along with it", () => {
    const { app, setHeight } = mount(true)
    app.flushSync(() => setHeight(80))
    expect(boundsOf(app.renderer, "plain").height).toBeCloseTo(10, 1)
    frame(app.renderer, 1100)
    expect(boundsOf(app.renderer, "plain").height).toBeCloseTo(20, 1)
  })
})
