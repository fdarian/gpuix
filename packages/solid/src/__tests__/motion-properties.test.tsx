import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const linear = { duration: 1, ease: "linear" } as const

describe.skipIf(!hasNativeTestRenderer)("Solid motion lengths layout resolves", () => {
  const boundsOf = (app: ReturnType<typeof createTestRoot>, testId: string) => {
    app.renderer.dispatchNativeEvents()
    return app.renderer.getElementBounds(app.renderer.findByTestId(testId)!.id)!
  }
  // Layout is read back a frame after it is painted, and the clock of an
  // animation that needs it starts on the first frame that has it, so one more
  // frame runs before time moves.
  const mounted = (app: ReturnType<typeof createTestRoot>, code: Parameters<typeof app.render>[0]) => {
    app.renderer.clockPause()
    app.render(code)
    app.renderer.flush()
  }
  const update = (app: ReturnType<typeof createTestRoot>, fn: () => void) => {
    app.flushSync(fn)
    app.renderer.flush()
  }

  it("opens an accordion to the content height and closes it again", () => {
    const [open, setOpen] = createSignal(true)
    const app = createTestRoot({ width: 400, height: 300 })
    mounted(app, () => (
      <div style={{ display: "flex", flexDirection: "column" }}>
        <motion.div
          testId="panel"
          initial={{ height: 0 }}
          animate={{ height: open() ? "auto" : 0 }}
          transition={linear}
          style={{ overflow: "hidden" }}
        >
          <div style={{ width: 40, height: 80 }} />
        </motion.div>
      </div>
    ))
    expect(boundsOf(app, "panel").height).toBe(0)

    app.renderer.clockFastForward(500)
    expect(boundsOf(app, "panel").height).toBeCloseTo(40, 0)
    app.renderer.clockFastForward(1000)
    expect(boundsOf(app, "panel").height).toBe(80)

    update(app, () => setOpen(false))
    app.renderer.clockFastForward(500)
    expect(boundsOf(app, "panel").height).toBeCloseTo(40, 0)
    app.renderer.clockFastForward(1000)
    expect(boundsOf(app, "panel").height).toBe(0)
  })

  it("follows the content while it grows during the animation", () => {
    const [content, setContent] = createSignal(80)
    const app = createTestRoot({ width: 400, height: 300 })
    mounted(app, () => (
      <div style={{ display: "flex", flexDirection: "column" }}>
        <motion.div
          testId="panel"
          initial={{ height: 0 }}
          animate={{ height: "auto" }}
          transition={linear}
          style={{ overflow: "hidden" }}
        >
          <div style={{ width: 40, height: content() }} />
        </motion.div>
      </div>
    ))
    app.renderer.clockFastForward(500)
    expect(boundsOf(app, "panel").height).toBeCloseTo(40, 0)

    update(app, () => setContent(160))
    app.renderer.clockFastForward(250)
    const grown = boundsOf(app, "panel").height
    expect(grown).toBeGreaterThan(40)
    expect(grown).toBeLessThan(160)

    app.renderer.clockFastForward(1000)
    expect(boundsOf(app, "panel").height).toBe(160)
  })

  it("moves between px and a percentage of the parent and settles on the target unit", () => {
    const [width, setWidth] = createSignal<number | `${number}%`>("50%")
    const app = createTestRoot({ width: 400, height: 300 })
    mounted(app, () => (
      <div style={{ display: "flex", flexDirection: "column", width: 400 }}>
        <motion.div
          testId="bar"
          initial={{ width: 0 }}
          animate={{ width: width() }}
          transition={linear}
          style={{ height: 10 }}
        />
      </div>
    ))
    app.renderer.clockFastForward(500)
    expect(boundsOf(app, "bar").width).toBeCloseTo(100, 0)
    app.renderer.clockFastForward(1000)
    expect(boundsOf(app, "bar").width).toBe(200)

    update(app, () => setWidth(100))
    app.renderer.clockFastForward(500)
    expect(boundsOf(app, "bar").width).toBeCloseTo(150, 0)
    app.renderer.clockFastForward(1000)
    expect(boundsOf(app, "bar").width).toBe(100)
  })

  it("resolves a percentage on a root against the window", () => {
    const app = createTestRoot({ width: 400, height: 300 })
    mounted(app, () => (
      <motion.div
        testId="root"
        initial={{ width: 0 }}
        animate={{ width: "50%" }}
        transition={linear}
        style={{ height: 10 }}
      />
    ))
    app.renderer.clockFastForward(500)
    expect(boundsOf(app, "root").width).toBeCloseTo(100, 0)
    app.renderer.clockFastForward(1000)
    expect(boundsOf(app, "root").width).toBe(200)
  })

  it("starts from the laid-out size when the style leaves it unset", () => {
    const app = createTestRoot({ width: 400, height: 300 })
    mounted(app, () => (
      <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start" }}>
        <motion.div
          testId="box"
          initial={{ opacity: 1 }}
          animate={{ opacity: 1, width: 200 }}
          transition={linear}
          style={{ height: 10 }}
        >
          <div style={{ width: 40, height: 10 }} />
        </motion.div>
      </div>
    ))
    expect(boundsOf(app, "box").width).toBeCloseTo(40, 0)
    app.renderer.clockFastForward(500)
    expect(boundsOf(app, "box").width).toBeCloseTo(120, 0)
    app.renderer.clockFastForward(1000)
    expect(boundsOf(app, "box").width).toBe(200)
  })
})
