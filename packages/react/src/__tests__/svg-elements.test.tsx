import fs from "fs"
import path from "path"
import { describe, expect, it, vi } from "vitest"
import { motion } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const FRAME = { width: "100%", height: "100%", backgroundColor: "#000000" } as const
const SVG_STYLE = { width: 100, height: 100, color: "#ffffff" } as const

type Rgba = [number, number, number, number]

/** The colour of the logical pixel, over the black frame. */
function painter(file: string, renderer: ReturnType<typeof createTestRoot>["renderer"]) {
  fs.mkdirSync(SHOTS_DIR, { recursive: true })
  const read = () => {
    renderer.dispatchNativeEvents()
    renderer.captureScreenshot(file)
    return readLogicalPixels(file, renderer)
  }
  return {
    at: (x: number, y: number): Rgba => read().at(x, y) as Rgba,
    /** How many pixels of the box (x0..x1, y0..y1) are lit. */
    ink: (x0: number, y0: number, x1: number, y1: number): number => {
      const pixels = read()
      let lit = 0
      for (let y = y0; y < y1; y++) {
        for (let x = x0; x < x1; x++) {
          if (Math.max(...pixels.at(x, y).slice(0, 3)) > 150) lit++
        }
      }
      return lit
    },
    /** How many pixels of the box (x0..x1, y0..y1) satisfy `match`. */
    count: (x0: number, y0: number, x1: number, y1: number, match: (pixel: Rgba) => boolean): number => {
      const pixels = read()
      let found = 0
      for (let y = y0; y < y1; y++) {
        for (let x = x0; x < x1; x++) {
          if (match(pixels.at(x, y) as Rgba)) found++
        }
      }
      return found
    },
  }
}

const RED: Rgba = [255, 0, 0, 255]
const GREEN: Rgba = [0, 255, 0, 255]
const BLUE: Rgba = [0, 0, 255, 255]
const BLACK: Rgba = [0, 0, 0, 255]

/** 4x4: red in the left two columns, blue in the right two. */
const RED_BLUE_PNG =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAQAAAAECAIAAAAmkwkpAAAAE0lEQVR4nGP4z8AARGDiPwNxHACeow/xfxmViwAAAABJRU5ErkJggg=="

function near(actual: Rgba, expected: Rgba, tolerance = 12) {
  for (let channel = 0; channel < 4; channel++) {
    expect(Math.abs(actual[channel]! - expected[channel]!), `${actual} vs ${expected}`).toBeLessThanOrEqual(
      tolerance
    )
  }
}

describeNative("svg pattern", () => {
  it("tiles its children over a shape that fills with it", () => {
    const { render, renderer } = createTestRoot()
    const view = painter(path.join(SHOTS_DIR, "svg-pattern.png"), renderer)
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <defs>
            <pattern id="bars" width={20} height={20} patternUnits="userSpaceOnUse">
              <rect width={10} height={20} fill="#ff0000" />
            </pattern>
          </defs>
          <rect width={100} height={100} fill="url(#bars)" />
        </svg>
      </div>
    )
    for (const tile of [0, 20, 40, 60, 80]) {
      near(view.at(tile + 5, 50), RED)
      near(view.at(tile + 15, 50), BLACK)
    }
  })
})

describeNative("svg marker", () => {
  const dot = (
    <marker id="dot" markerWidth={12} markerHeight={12} refX={6} refY={6} markerUnits="userSpaceOnUse">
      <circle cx={6} cy={6} r={6} fill="#00ff00" />
    </marker>
  )

  it("draws at the end of a path", () => {
    const { render, renderer } = createTestRoot()
    const view = painter(path.join(SHOTS_DIR, "svg-marker-end.png"), renderer)
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <defs>{dot}</defs>
          <path d="M10 50 L60 50" stroke="#0000ff" strokeWidth={2} markerEnd="url(#dot)" />
        </svg>
      </div>
    )
    near(view.at(60, 50), GREEN)
    near(view.at(30, 50), BLUE)
    near(view.at(10, 50), BLUE)
    near(view.at(80, 50), BLACK)
  })

  it("takes the marker shorthand for all three positions", () => {
    const { render, renderer } = createTestRoot()
    const view = painter(path.join(SHOTS_DIR, "svg-marker-shorthand.png"), renderer)
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <defs>{dot}</defs>
          <path d="M10 50 L35 50 L60 50" stroke="#0000ff" strokeWidth={2} marker="url(#dot)" />
        </svg>
      </div>
    )
    near(view.at(10, 50), GREEN)
    near(view.at(35, 50), GREEN)
    near(view.at(60, 50), GREEN)
  })
})

describeNative("svg filter", () => {
  const SQUARE = { x: 30, y: 30, width: 40, height: 40, fill: "#ff0000", filter: "url(#f)" } as const
  const REGION = { x: 0, y: 0, width: 100, height: 100, filterUnits: "userSpaceOnUse" } as const

  it("blurs a shape past its edge", () => {
    const { render, renderer } = createTestRoot()
    const view = painter(path.join(SHOTS_DIR, "svg-filter-blur.png"), renderer)
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <defs>
            <filter id="f" {...REGION}>
              <feGaussianBlur stdDeviation={6} />
            </filter>
          </defs>
          <rect {...SQUARE} />
        </svg>
      </div>
    )
    const outside = view.at(26, 50)
    expect(outside[0]).toBeGreaterThan(20)
    expect(outside[0]).toBeLessThan(200)
    near(view.at(50, 50), RED)
    near(view.at(5, 50), BLACK)
  })

  it("offsets a shape", () => {
    const { render, renderer } = createTestRoot()
    const view = painter(path.join(SHOTS_DIR, "svg-filter-offset.png"), renderer)
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <defs>
            <filter id="f" {...REGION}>
              <feOffset dx={25} />
            </filter>
          </defs>
          <rect {...SQUARE} width={20} />
        </svg>
      </div>
    )
    near(view.at(40, 50), BLACK)
    near(view.at(65, 50), RED)
  })

  it("floods the region", () => {
    const { render, renderer } = createTestRoot()
    const view = painter(path.join(SHOTS_DIR, "svg-filter-flood.png"), renderer)
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <defs>
            <filter id="f" {...REGION}>
              <feFlood floodColor="#00ff00" />
            </filter>
          </defs>
          <rect {...SQUARE} />
        </svg>
      </div>
    )
    near(view.at(5, 5), GREEN)
    near(view.at(95, 95), GREEN)
  })

  it("chains primitives through result and in", () => {
    const { render, renderer } = createTestRoot()
    const view = painter(path.join(SHOTS_DIR, "svg-filter-shadow.png"), renderer)
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <defs>
            <filter id="f" {...REGION}>
              <feOffset in="SourceAlpha" dx={20} result="moved" />
              <feFlood floodColor="#0000ff" result="blue" />
              <feComposite in="blue" in2="moved" operator="in" result="shadow" />
              <feMerge>
                <feMergeNode in="shadow" />
                <feMergeNode in="SourceGraphic" />
              </feMerge>
            </filter>
          </defs>
          <rect {...SQUARE} width={20} />
        </svg>
      </div>
    )
    near(view.at(40, 50), RED)
    near(view.at(60, 50), BLUE)
    near(view.at(80, 50), BLACK)
  })

  it("animates stdDeviation", () => {
    const { render, renderer } = createTestRoot()
    const view = painter(path.join(SHOTS_DIR, "svg-filter-motion.png"), renderer)
    renderer.clockPause()
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <defs>
            <filter id="f" {...REGION}>
              <motion.feGaussianBlur
                stdDeviation={0}
                initial={{ stdDeviation: 0 }}
                animate={{ stdDeviation: 8 }}
                transition={{ duration: 1, ease: "linear" }}
              />
            </filter>
          </defs>
          <rect {...SQUARE} />
        </svg>
      </div>
    )
    near(view.at(24, 50), BLACK)
    renderer.clockFastForward(1000)
    expect(view.at(24, 50)[0]).toBeGreaterThan(20)
  })
})

describeNative("svg text", () => {
  const LETTERS = { fontFamily: "Helvetica", fontSize: 40, fill: "#ffffff" } as const

  it("paints its string children as glyphs", () => {
    const { render, renderer } = createTestRoot()
    const view = painter(path.join(SHOTS_DIR, "svg-text.png"), renderer)
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <text x={4} y={60} {...LETTERS}>
            HH
          </text>
        </svg>
      </div>
    )
    expect(view.ink(0, 10, 60, 70)).toBeGreaterThan(150)
    expect(view.ink(70, 0, 100, 100)).toBe(0)
  })

  it("redraws when the string changes", () => {
    const { render, renderer } = createTestRoot()
    const view = painter(path.join(SHOTS_DIR, "svg-text-update.png"), renderer)
    const App = ({ word }: { word: string }) => (
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <text x={2} y={60} {...LETTERS}>
            {word}
          </text>
        </svg>
      </div>
    )
    render(<App word="I" />)
    expect(view.ink(50, 0, 100, 100)).toBe(0)
    render(<App word="IIIIIIII" />)
    expect(view.ink(50, 0, 100, 100)).toBeGreaterThan(30)
  })

  it("takes currentColor and textAnchor", () => {
    const { render, renderer } = createTestRoot()
    const view = painter(path.join(SHOTS_DIR, "svg-text-anchor.png"), renderer)
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={{ ...SVG_STYLE, color: "#ff0000" }}>
          <text x={98} y={60} fontFamily="Helvetica" fontSize={40} fill="currentColor" textAnchor="end">
            HH
          </text>
        </svg>
      </div>
    )
    expect(view.ink(0, 0, 30, 100)).toBe(0)
    expect(view.ink(30, 0, 100, 100)).toBeGreaterThan(100)
    expect(view.count(30, 0, 100, 100, ([r, g, b]) => r > 200 && g < 40 && b < 40)).toBeGreaterThan(50)
  })

  it("lays a tspan in its own colour", () => {
    const { render, renderer } = createTestRoot()
    const view = painter(path.join(SHOTS_DIR, "svg-text-tspan.png"), renderer)
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <text x={2} y={60} {...LETTERS}>
            H
            <tspan dx={30} fill="#00ff00">
              H
            </tspan>
          </text>
        </svg>
      </div>
    )
    expect(view.count(40, 10, 100, 70, ([r, g, b]) => g > 200 && r < 60 && b < 60)).toBeGreaterThan(30)
    expect(view.ink(2, 10, 30, 70)).toBeGreaterThan(30)
  })

  it("animates the position", () => {
    const { render, renderer } = createTestRoot()
    const view = painter(path.join(SHOTS_DIR, "svg-text-motion.png"), renderer)
    renderer.clockPause()
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <motion.text
            y={60}
            {...LETTERS}
            initial={{ x: 0 }}
            animate={{ x: 60 }}
            transition={{ duration: 1, ease: "linear" }}
          >
            I
          </motion.text>
        </svg>
      </div>
    )
    expect(view.ink(0, 0, 30, 100)).toBeGreaterThan(20)
    expect(view.ink(60, 0, 100, 100)).toBe(0)
    renderer.clockFastForward(1000)
    expect(view.ink(0, 0, 30, 100)).toBe(0)
    expect(view.ink(60, 0, 100, 100)).toBeGreaterThan(20)
  })

  it("leaves the host text element alone outside an svg", () => {
    const { render, renderer } = createTestRoot()
    const view = painter(path.join(SHOTS_DIR, "svg-text-host.png"), renderer)
    render(
      <div style={FRAME}>
        <text style={{ color: "#ffffff", fontSize: 30 }}>Hi</text>
      </div>
    )
    expect(view.ink(0, 0, 60, 50)).toBeGreaterThan(30)
  })
})

describeNative("svg image", () => {
  it("paints the decoded pixels of a data URL", () => {
    const { render, renderer } = createTestRoot()
    const view = painter(path.join(SHOTS_DIR, "svg-image.png"), renderer)
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <image href={RED_BLUE_PNG} x={10} y={20} width={80} height={40} preserveAspectRatio="none" />
        </svg>
      </div>
    )
    near(view.at(25, 40), RED)
    near(view.at(75, 40), BLUE)
    near(view.at(50, 80), BLACK)
    near(view.at(5, 40), BLACK)
  })

  it("loads a file URL and drops a blob URL", () => {
    const file = path.join(SHOTS_DIR, "svg-image-source.png")
    fs.writeFileSync(file, Buffer.from(RED_BLUE_PNG.split(",")[1]!, "base64"))
    const { render, renderer } = createTestRoot()
    const view = painter(path.join(SHOTS_DIR, "svg-image-file.png"), renderer)
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={SVG_STYLE}>
          <image href={`file://${file}`} x={0} y={0} width={100} height={40} preserveAspectRatio="none" />
          <image href="blob:nothing" x={0} y={50} width={100} height={40} />
        </svg>
      </div>
    )
    near(view.at(25, 20), RED)
    near(view.at(75, 20), BLUE)
    near(view.at(50, 70), BLACK)
  })
})

describeNative("svg element placement", () => {
  it("rejects the new tags outside an svg", () => {
    const errors = vi.spyOn(console, "error").mockImplementation(() => {})
    try {
      for (const element of [<div><filter /></div>, <pattern />, <div><tspan /></div>, <feFlood />]) {
        errors.mockClear()
        createTestRoot().render(element)
        expect(String(errors.mock.calls[0]?.[0])).toMatch(/only valid inside <svg>/)
      }
    } finally {
      errors.mockRestore()
    }
  })
})
