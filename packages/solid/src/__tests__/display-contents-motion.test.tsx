/// The Solid half of "what moves on it reaches its children" in `display-contents.test.tsx` of the React package: a
/// `display: contents` element has no box, so what it runs is shown by the text it hands its font to.
import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("Solid display: contents running something", () => {
  const heightOf = (app: ReturnType<typeof createTestRoot>): number => {
    app.renderer.dispatchNativeEvents()
    return app.renderer.getElementBounds(app.renderer.findByTestId("word")!.id)!.height
  }

  it("lays its children out at the fontSize a CSS transition of a signal has reached", () => {
    const [fontSize, setFontSize] = createSignal(16)
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <div style={{ display: "contents", fontSize: fontSize(), transition: "font-size 1s linear" }}>
        <div testId="word" style={{ width: 300 }}>
          WWWW
        </div>
      </div>
    ))
    const start = heightOf(app)

    app.flushSync(() => setFontSize(48))
    app.renderer.clockFastForward(500)
    const half = heightOf(app)
    app.renderer.clockFastForward(600)
    const end = heightOf(app)

    expect(half).toBeGreaterThan(start)
    expect(end).toBeGreaterThan(half)
  })

  it("lays its children out at the fontSize a motion has reached", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <motion.div
        style={{ display: "contents" }}
        initial={{ fontSize: 16 }}
        animate={{ fontSize: 48 }}
        transition={{ duration: 1, ease: "linear" }}
      >
        <div testId="word" style={{ width: 300 }}>
          WWWW
        </div>
      </motion.div>
    ))
    const start = heightOf(app)
    app.renderer.clockFastForward(500)
    const half = heightOf(app)
    app.renderer.clockFastForward(600)
    const end = heightOf(app)

    expect(half).toBeGreaterThan(start)
    expect(end).toBeGreaterThan(half)
  })
})
