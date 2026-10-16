import fs from "fs"
import os from "os"
import path from "path"
import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const FRAME = { width: "100%", height: "100%", backgroundColor: "#000000" } as const
const SVG_STYLE = { width: 100, height: 100, color: "#ffffff" } as const

const shots = fs.mkdtempSync(path.join(os.tmpdir(), "gpuix-solid-svg-"))

type App = ReturnType<typeof createTestRoot>

/** The screenshot bytes: two frames paint alike exactly when the bytes match. */
function shot(app: App, name: string): Buffer {
  app.renderer.dispatchNativeEvents()
  const file = path.join(shots, `${name}.png`)
  app.renderer.captureScreenshot(file)
  return fs.readFileSync(file)
}

describe.skipIf(!hasNativeTestRenderer)("Solid svg children", () => {
  it("draws DOM-style children and redraws when a signal changes", () => {
    const [x, setX] = createSignal(0)
    const app = createTestRoot()
    app.render(() => (
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" fill="currentColor" style={SVG_STYLE}>
          <g strokeWidth={4}>
            <rect x={x()} y={0} width={20} height={100} />
          </g>
        </svg>
      </div>
    ))
    const first = shot(app, "first")
    app.flushSync(() => setX(60))
    const second = shot(app, "second")
    expect(first.equals(second)).toBe(false)

    app.flushSync(() => setX(0))
    expect(shot(app, "back").equals(first)).toBe(true)
  })

  it("draws what shapes refer to by id", () => {
    const [shown, setShown] = createSignal(false)
    const app = createTestRoot()
    app.render(() => (
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" width={100} height={100}>
          <defs>
            <linearGradient id="fade">
              <stop offset={0} stopColor="#ffffff" />
              <stop offset={1} stopColor="#ff0000" />
            </linearGradient>
            <clipPath id="top">
              <rect width={100} height={50} />
            </clipPath>
            <rect id="bar" width={100} height={100} fill="url(#fade)" />
          </defs>
          {shown() ? <use href="#bar" clipPath="url(#top)" /> : null}
        </svg>
      </div>
    ))
    const hidden = shot(app, "defs-hidden")
    app.flushSync(() => setShown(true))
    expect(shot(app, "defs-shown").equals(hidden)).toBe(false)
  })

  it("sizes the box from width and height", () => {
    const app = createTestRoot()
    app.render(() => (
      <div style={FRAME}>
        <svg testId="sized" viewBox="0 0 10 10" width={40} height={30}>
          <circle cx={5} cy={5} r={4} />
        </svg>
      </div>
    ))
    app.renderer.dispatchNativeEvents()
    const bounds = app.renderer.getElementBounds(app.renderer.findByTestId("sized")!.id)!
    expect([bounds.width, bounds.height]).toEqual([40, 30])
  })

  it("draws a path with an animated pathLength", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <motion.path
            d="M0 50 L100 50"
            stroke="currentColor"
            strokeWidth={20}
            initial={{ pathLength: 0 }}
            animate={{ pathLength: 1 }}
            transition={{ duration: 1, ease: "linear" }}
          />
        </svg>
      </div>
    ))
    const start = shot(app, "draw-0")
    app.renderer.clockFastForward(500)
    const middle = shot(app, "draw-500")
    app.renderer.clockFastForward(1000)
    const end = shot(app, "draw-1500")

    expect(start.equals(middle)).toBe(false)
    expect(middle.equals(end)).toBe(false)

    // The finished draw paints exactly what the same path paints with no motion.
    const still = createTestRoot()
    still.renderer.clockPause()
    still.render(() => (
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <path d="M0 50 L100 50" stroke="currentColor" strokeWidth={20} />
        </svg>
      </div>
    ))
    expect(shot(still, "draw-still").equals(end)).toBe(true)
  })

  it("morphs d between paths with the same commands", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <motion.path
            stroke="currentColor"
            strokeWidth={10}
            initial={{ d: "M0 75 L100 75" }}
            animate={{ d: "M0 25 L100 25" }}
            transition={{ duration: 1, ease: "linear" }}
          />
        </svg>
      </div>
    ))
    const start = shot(app, "morph-0")
    app.renderer.clockFastForward(500)
    const middle = shot(app, "morph-500")
    app.renderer.clockFastForward(1000)
    const end = shot(app, "morph-1500")
    expect(new Set([start.toString("base64"), middle.toString("base64"), end.toString("base64")]).size).toBe(3)
  })

  it("draws a text whose position and string are signals", () => {
    const [x, setX] = createSignal(0)
    const [word, setWord] = createSignal("I")
    const app = createTestRoot()
    app.render(() => (
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <text x={x()} y={60} fontFamily="Helvetica" fontSize={40} fill="currentColor">
            {word()}
          </text>
        </svg>
      </div>
    ))
    const first = shot(app, "text-0")
    app.flushSync(() => setX(50))
    const moved = shot(app, "text-50")
    expect(moved.equals(first)).toBe(false)
    app.flushSync(() => setWord("W"))
    expect(shot(app, "text-w").equals(moved)).toBe(false)
    app.flushSync(() => setX(0))
    app.flushSync(() => setWord("I"))
    expect(shot(app, "text-back").equals(first)).toBe(true)

    // An svg with no text paints differently, so the first shot did draw it.
    const empty = createTestRoot()
    empty.render(() => (
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE} />
      </div>
    ))
    expect(shot(empty, "text-none").equals(first)).toBe(false)
  })

  it("applies a filter by id", () => {
    const [on, setOn] = createSignal(false)
    const app = createTestRoot()
    app.render(() => (
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <defs>
            <filter id="f" x={0} y={0} width={100} height={100} filterUnits="userSpaceOnUse">
              <feGaussianBlur stdDeviation={6} />
            </filter>
          </defs>
          <rect x={30} y={30} width={40} height={40} fill="#ff0000" filter={on() ? "url(#f)" : undefined} />
        </svg>
      </div>
    ))
    const sharp = shot(app, "filter-off")
    app.flushSync(() => setOn(true))
    expect(shot(app, "filter-on").equals(sharp)).toBe(false)
  })

  it("rejects a shape outside an svg", () => {
    const app = createTestRoot()
    expect(() => app.render(() => <div><circle r={1} /></div>)).toThrow(/only valid inside <svg>/)
    expect(() => app.render(() => <div><filter /></div>)).toThrow(/only valid inside <svg>/)
  })
})
