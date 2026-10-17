import { describe, expect, it } from "bun:test"
import { motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const linear = { duration: 1, ease: "linear" } as const

describe.skipIf(!hasNativeTestRenderer)("Solid motion multi-value shorthand strings", () => {
  const boundsOf = (app: ReturnType<typeof createTestRoot>, testId: string) => {
    app.renderer.dispatchNativeEvents()
    return app.renderer.getElementBounds(app.renderer.findByTestId(testId)!.id)!
  }
  const mounted = (app: ReturnType<typeof createTestRoot>, code: Parameters<typeof app.render>[0]) => {
    app.renderer.clockPause()
    app.render(code)
    app.renderer.flush()
  }

  it("animates padding '0.5em 0.75em' to '1em 1.5em' with each axis on its own value", () => {
    const app = createTestRoot({ width: 400, height: 300 })
    mounted(app, () => (
      <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start" }}>
        <motion.div
          testId="box"
          style={{ fontSize: 16 }}
          initial={{ padding: "0.5em 0.75em" }}
          animate={{ padding: "1em 1.5em" }}
          transition={linear}
        >
          <div testId="inner" style={{ width: 20, height: 20 }} />
        </motion.div>
      </div>
    ))
    const offset = () => {
      const box = boundsOf(app, "box")
      const inner = boundsOf(app, "inner")
      return { x: inner.x - box.x, y: inner.y - box.y }
    }
    expect(offset()).toEqual({ x: 12, y: 8 })
    app.renderer.clockFastForward(500)
    expect(offset().y).toBeCloseTo(12, 0)
    expect(offset().x).toBeCloseTo(18, 0)
    app.renderer.clockFastForward(1000)
    expect(offset()).toEqual({ x: 24, y: 16 })
  })

  it("animates a two-value gap as the row gap and then the column gap", () => {
    const app = createTestRoot({ width: 400, height: 300 })
    mounted(app, () => (
      <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start" }}>
        <motion.div
          testId="grid"
          style={{ display: "flex", flexWrap: "wrap", width: 90 }}
          initial={{ gap: "0 0" }}
          animate={{ gap: "20px 8px" }}
          transition={linear}
        >
          <div testId="a" style={{ width: 40, height: 10 }} />
          <div testId="b" style={{ width: 40, height: 10 }} />
          <div testId="c" style={{ width: 40, height: 10 }} />
        </motion.div>
      </div>
    ))
    app.renderer.clockFastForward(500)
    const a = boundsOf(app, "a")
    expect(boundsOf(app, "c").y - a.y - 10).toBeCloseTo(10, 0)
    expect(boundsOf(app, "b").x - a.x - 40).toBeCloseTo(4, 0)
    app.renderer.clockFastForward(1000)
    expect(boundsOf(app, "c").y - boundsOf(app, "a").y - 10).toBe(20)
    expect(boundsOf(app, "b").x - boundsOf(app, "a").x - 40).toBe(8)
  })

  it("animates a keyframes list of padding strings, splitting every frame", () => {
    const app = createTestRoot({ width: 400, height: 300 })
    mounted(app, () => (
      <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start" }}>
        <motion.div
          testId="box"
          initial={{ padding: 0 }}
          animate={{ padding: ["0 0", "10px 20px", "30px 40px"] }}
          transition={linear}
        >
          <div testId="inner" style={{ width: 20, height: 20 }} />
        </motion.div>
      </div>
    ))
    app.renderer.clockFastForward(500)
    expect(boundsOf(app, "inner").y - boundsOf(app, "box").y).toBeCloseTo(10, 0)
    expect(boundsOf(app, "inner").x - boundsOf(app, "box").x).toBeCloseTo(20, 0)
    app.renderer.clockFastForward(1000)
    expect(boundsOf(app, "inner").y - boundsOf(app, "box").y).toBe(30)
    expect(boundsOf(app, "inner").x - boundsOf(app, "box").x).toBe(40)
  })
})
