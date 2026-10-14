import fs from "fs"
import path from "path"
import { describe, expect, it } from "vitest"
import { motion } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const FRAME = {
  width: "100%",
  height: "100%",
  backgroundColor: "#000000",
} as const

const SVG_STYLE = { width: 100, height: 100, color: "#ffffff" } as const

/** Brightness of the logical pixel: the svg paints a white mask over black. */
function sampler(file: string, renderer: ReturnType<typeof createTestRoot>["renderer"]) {
  fs.mkdirSync(SHOTS_DIR, { recursive: true })
  return (x: number, y: number): number => {
    renderer.dispatchNativeEvents()
    renderer.captureScreenshot(file)
    return readLogicalPixels(file, renderer).at(x, y)[0]
  }
}

const DRAWN = 200
const EMPTY = 40

describeNative("svg children", () => {
  it("draws DOM-style children from camelCase props", () => {
    const { render, renderer } = createTestRoot()
    const brightness = sampler(path.join(SHOTS_DIR, "svg-children-static.png"), renderer)
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" width={100} height={100} style={{ color: "#ffffff" }}>
          <g strokeWidth={10}>
            <path d="M0 25 L100 25" stroke="black" />
            <rect x={0} y={60} width={50} height={20} fill="black" />
            <circle cx={75} cy={75} r={10} fill="black" />
          </g>
        </svg>
      </div>
    )

    expect(brightness(50, 25)).toBeGreaterThan(DRAWN)
    expect(brightness(25, 70)).toBeGreaterThan(DRAWN)
    expect(brightness(75, 75)).toBeGreaterThan(DRAWN)
    expect(brightness(75, 50)).toBeLessThan(EMPTY)
  })

  it("sizes the box from width and height unless the style does", () => {
    const { render, renderer } = createTestRoot()
    render(
      <div style={FRAME}>
        <svg testId="sized" viewBox="0 0 10 10" width={40} height={30}>
          <circle cx={5} cy={5} r={4} />
        </svg>
        <svg testId="styled" viewBox="0 0 10 10" width={40} height={30} style={{ width: 70 }}>
          <circle cx={5} cy={5} r={4} />
        </svg>
      </div>
    )
    renderer.dispatchNativeEvents()
    const sized = renderer.getElementBounds(renderer.findByTestId("sized").id)
    const styled = renderer.getElementBounds(renderer.findByTestId("styled").id)
    expect([sized.width, sized.height]).toEqual([40, 30])
    expect([styled.width, styled.height]).toEqual([70, 30])
  })

  it("prefers children over source", () => {
    const { render, renderer } = createTestRoot()
    const brightness = sampler(path.join(SHOTS_DIR, "svg-children-source.png"), renderer)
    const everything =
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" width="100" height="100"><rect width="100" height="100"/></svg>'
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE} source={everything}>
          <rect x={0} y={0} width={30} height={100} />
        </svg>
      </div>
    )
    expect(brightness(15, 50)).toBeGreaterThan(DRAWN)
    expect(brightness(80, 50)).toBeLessThan(EMPTY)
  })

  it("applies dash props in user units", () => {
    const { render, renderer } = createTestRoot()
    const brightness = sampler(path.join(SHOTS_DIR, "svg-children-dash.png"), renderer)
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <path d="M0 50 L100 50" stroke="black" strokeWidth={20} strokeDasharray={[20, 20]} />
        </svg>
      </div>
    )
    expect(brightness(10, 50)).toBeGreaterThan(DRAWN)
    expect(brightness(30, 50)).toBeLessThan(EMPTY)
    expect(brightness(50, 50)).toBeGreaterThan(DRAWN)
  })

  it("measures dashes in pathLength units, as the DOM attribute does", () => {
    const { render, renderer } = createTestRoot()
    const brightness = sampler(path.join(SHOTS_DIR, "svg-children-path-length.png"), renderer)
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <path d="M0 50 L100 50" stroke="black" strokeWidth={20} pathLength={1} strokeDasharray="0.5 1" />
        </svg>
      </div>
    )
    expect(brightness(25, 50)).toBeGreaterThan(DRAWN)
    expect(brightness(75, 50)).toBeLessThan(EMPTY)
  })

  it("redraws when a child prop changes", () => {
    const { render, renderer } = createTestRoot()
    const brightness = sampler(path.join(SHOTS_DIR, "svg-children-update.png"), renderer)
    const App = ({ x }: { x: number }) => (
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <rect x={x} y={0} width={20} height={100} />
        </svg>
      </div>
    )
    render(<App x={0} />)
    expect(brightness(10, 50)).toBeGreaterThan(DRAWN)
    expect(brightness(70, 50)).toBeLessThan(EMPTY)
    render(<App x={60} />)
    expect(brightness(10, 50)).toBeLessThan(EMPTY)
    expect(brightness(70, 50)).toBeGreaterThan(DRAWN)
  })

  it("rejects a shape outside an svg", () => {
    const { render } = createTestRoot()
    expect(() => render(<div><path d="M0 0" /></div>)).toThrow(/only valid inside <svg>/)
    expect(() => render(<g />)).toThrow(/only valid inside <svg>/)
  })
})

describeNative("svg motion", () => {
  it("draws a path with pathLength", () => {
    const { render, renderer } = createTestRoot()
    const brightness = sampler(path.join(SHOTS_DIR, "svg-motion-draw.png"), renderer)
    renderer.clockPause()
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <motion.path
            d="M0 50 L100 50"
            stroke="black"
            strokeWidth={20}
            initial={{ pathLength: 0 }}
            animate={{ pathLength: 1 }}
            transition={{ duration: 1, ease: "linear" }}
          />
        </svg>
      </div>
    )

    expect(brightness(10, 50)).toBeLessThan(EMPTY)

    renderer.clockFastForward(500)
    expect(brightness(25, 50)).toBeGreaterThan(DRAWN)
    expect(brightness(75, 50)).toBeLessThan(EMPTY)

    renderer.clockFastForward(1000)
    expect(brightness(75, 50)).toBeGreaterThan(DRAWN)
    expect(brightness(99, 50)).toBeGreaterThan(DRAWN)
  })

  it("moves a dash along the path with pathOffset", () => {
    const { render, renderer } = createTestRoot()
    const brightness = sampler(path.join(SHOTS_DIR, "svg-motion-offset.png"), renderer)
    renderer.clockPause()
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <motion.path
            d="M0 50 L100 50"
            stroke="black"
            strokeWidth={20}
            initial={{ pathLength: 0.2, pathOffset: 0 }}
            animate={{ pathLength: 0.2, pathOffset: -0.5 }}
            transition={{ duration: 1, ease: "linear" }}
          />
        </svg>
      </div>
    )
    expect(brightness(10, 50)).toBeGreaterThan(DRAWN)
    expect(brightness(60, 50)).toBeLessThan(EMPTY)

    renderer.clockFastForward(1000)
    expect(brightness(10, 50)).toBeLessThan(EMPTY)
    expect(brightness(60, 50)).toBeGreaterThan(DRAWN)
  })

  it("morphs d between paths with the same commands", () => {
    const { render, renderer } = createTestRoot()
    const brightness = sampler(path.join(SHOTS_DIR, "svg-motion-morph.png"), renderer)
    renderer.clockPause()
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <motion.path
            stroke="black"
            strokeWidth={10}
            initial={{ d: "M0 75 L100 75" }}
            animate={{ d: "M0 25 L100 25" }}
            transition={{ duration: 1, ease: "linear" }}
          />
        </svg>
      </div>
    )
    expect(brightness(50, 75)).toBeGreaterThan(DRAWN)
    expect(brightness(50, 25)).toBeLessThan(EMPTY)

    renderer.clockFastForward(500)
    expect(brightness(50, 50)).toBeGreaterThan(DRAWN)
    expect(brightness(50, 75)).toBeLessThan(EMPTY)

    renderer.clockFastForward(1000)
    expect(brightness(50, 25)).toBeGreaterThan(DRAWN)
    expect(brightness(50, 50)).toBeLessThan(EMPTY)
  })

  it("switches d halfway when the commands differ", () => {
    const { render, renderer } = createTestRoot()
    const brightness = sampler(path.join(SHOTS_DIR, "svg-motion-discrete.png"), renderer)
    renderer.clockPause()
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <motion.path
            stroke="black"
            strokeWidth={10}
            initial={{ d: "M0 75 L100 75" }}
            animate={{ d: "M0 25 H100" }}
            transition={{ duration: 1, ease: "linear" }}
          />
        </svg>
      </div>
    )
    renderer.clockFastForward(400)
    expect(brightness(50, 75)).toBeGreaterThan(DRAWN)
    expect(brightness(50, 50)).toBeLessThan(EMPTY)

    renderer.clockFastForward(200)
    expect(brightness(50, 25)).toBeGreaterThan(DRAWN)
    expect(brightness(50, 75)).toBeLessThan(EMPTY)
  })

  it("animates fill through the colour engine, here as coverage", () => {
    const { render, renderer } = createTestRoot()
    const brightness = sampler(path.join(SHOTS_DIR, "svg-motion-fill.png"), renderer)
    renderer.clockPause()
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <motion.rect
            x={0}
            y={0}
            width={100}
            height={100}
            initial={{ fill: "#000000" }}
            animate={{ fill: "rgba(0, 0, 0, 0)" }}
            transition={{ duration: 1, ease: "linear" }}
          />
        </svg>
      </div>
    )
    expect(brightness(50, 50)).toBeGreaterThan(DRAWN)

    renderer.clockFastForward(500)
    const middle = brightness(50, 50)
    expect(middle).toBeGreaterThan(100)
    expect(middle).toBeLessThan(160)

    renderer.clockFastForward(1000)
    expect(brightness(50, 50)).toBeLessThan(EMPTY)
  })

  it("animates geometry from the elements own attribute", () => {
    const { render, renderer } = createTestRoot()
    const brightness = sampler(path.join(SHOTS_DIR, "svg-motion-radius.png"), renderer)
    renderer.clockPause()
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <motion.circle
            cx={50}
            cy={50}
            r={10}
            animate={{ r: 40 }}
            transition={{ duration: 1, ease: "linear" }}
          />
        </svg>
      </div>
    )
    expect(brightness(50, 75)).toBeLessThan(EMPTY)

    renderer.clockFastForward(1000)
    expect(brightness(50, 75)).toBeGreaterThan(DRAWN)
  })
})
