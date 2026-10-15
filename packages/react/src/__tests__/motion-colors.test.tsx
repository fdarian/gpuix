import fs from "fs"
import path from "path"
import { describe, expect, it } from "vitest"
import { defineKeyframes, motion } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rgba = [number, number, number, number]

const LINEAR = { duration: 1, ease: "linear" } as const
const BOX = { width: 100, height: 100 } as const

/** A sampler of painted logical pixels that takes a fresh screenshot each call. */
function sampler(name: string, renderer: ReturnType<typeof createTestRoot>["renderer"]) {
  fs.mkdirSync(SHOTS_DIR, { recursive: true })
  const file = path.join(SHOTS_DIR, `motion-colors-${name}.png`)
  return (x = 50, y = 50): Rgba => {
    renderer.dispatchNativeEvents()
    renderer.captureScreenshot(file)
    return readLogicalPixels(file, renderer).at(x, y) as Rgba
  }
}

function expectNear(actual: Rgba, expected: Rgba, tolerance = 3) {
  for (let i = 0; i < 4; i++) {
    expect(Math.abs(actual[i]! - expected[i]!), `${actual} vs ${expected}`).toBeLessThanOrEqual(tolerance)
  }
}

const RED: Rgba = [255, 0, 0, 255]
const GREEN: Rgba = [0, 255, 0, 255]
const BLUE: Rgba = [0, 0, 255, 255]

describeNative("motion colours resolve in the colour context of the element", () => {
  it("animates from currentColor to light-dark() on the side the scheme picks", () => {
    const { render, renderer } = createTestRoot()
    const pixel = sampler("light-dark", renderer)
    renderer.clockPause()
    render(
      <motion.div
        style={{ ...BOX, color: "#ff0000", colorScheme: "dark" }}
        initial={{ backgroundColor: "currentColor" }}
        animate={{ backgroundColor: "light-dark(#0000ff, #00ff00)" }}
        transition={LINEAR}
      />
    )
    expectNear(pixel(), RED)

    renderer.clockFastForward(500)
    const middle = pixel()
    expect(middle[0]).toBeGreaterThan(60)
    expect(middle[0]).toBeLessThan(250)
    expect(middle[1]).toBeGreaterThan(60)
    expect(middle[2]).toBeLessThan(60)

    renderer.clockFastForward(1000)
    expectNear(pixel(), GREEN)
  })

  it("animates to a color-mix() and a color() target", () => {
    const { render, renderer } = createTestRoot()
    const pixel = sampler("color-mix", renderer)
    renderer.clockPause()
    render(
      <div style={{ ...BOX, display: "flex" }}>
        <motion.div
          style={{ width: 50, height: 100 }}
          initial={{ backgroundColor: "#000000" }}
          animate={{ backgroundColor: "color-mix(in srgb, red 50%, blue)" }}
          transition={LINEAR}
        />
        <motion.div
          style={{ width: 50, height: 100 }}
          initial={{ backgroundColor: "#000000" }}
          animate={{ backgroundColor: "color(srgb 0 1 0)" }}
          transition={LINEAR}
        />
      </div>
    )
    renderer.clockFastForward(1000)
    expectNear(pixel(25, 50), [128, 0, 128, 255])
    expectNear(pixel(75, 50), GREEN)
  })

  it("resolves a border's currentColor to the element's own color, and a color target to the parent's", () => {
    const { render, renderer } = createTestRoot()
    const pixel = sampler("border", renderer)
    renderer.clockPause()
    render(
      <div style={{ color: "#0000ff" }}>
        <motion.div
          style={{ ...BOX, color: "#00ff00", borderWidth: 10, borderStyle: "solid" }}
          initial={{ borderColor: "#000000" }}
          animate={{ borderColor: "currentColor" }}
          transition={LINEAR}
        />
      </div>
    )
    renderer.clockFastForward(1000)
    expectNear(pixel(3, 50), GREEN)
  })

  it("animates color itself from the parent's color: currentColor reaches a child's background", () => {
    const { render, renderer } = createTestRoot()
    const pixel = sampler("color", renderer)
    renderer.clockPause()
    render(
      <div style={{ color: "#0000ff" }}>
        <motion.div
          style={BOX}
          initial={{ color: "#ff0000" }}
          animate={{ color: "currentColor" }}
          transition={LINEAR}
        >
          <div style={{ ...BOX, backgroundColor: "currentColor" }} />
        </motion.div>
      </div>
    )
    expectNear(pixel(), RED)
    renderer.clockFastForward(1000)
    expectNear(pixel(), BLUE)
  })

  it("follows the window appearance under light dark", () => {
    // The test window cannot switch appearance, so this takes the one it has.
    // Resolving again on a change is covered natively.
    const { render, renderer } = createTestRoot()
    const pixel = sampler("appearance", renderer)
    renderer.clockPause()
    render(
      <motion.div
        style={{ ...BOX, colorScheme: "light dark" }}
        initial={{ backgroundColor: "#000000" }}
        animate={{ backgroundColor: "light-dark(#ff0000, #0000ff)" }}
        transition={LINEAR}
      />
    )
    renderer.clockFastForward(1000)
    expectNear(pixel(), renderer.getAppearance() === "dark" ? BLUE : RED)
  })

  it("resolves the colours of CSS animation keyframes and transitions at each element", () => {
    defineKeyframes("motion-colors-tint", {
      from: { backgroundColor: "currentColor" },
      to: { backgroundColor: "light-dark(#0000ff, #00ff00)" },
    })
    const { render, renderer } = createTestRoot()
    const pixel = sampler("css", renderer)
    renderer.clockPause()
    render(
      <div style={{ display: "flex", ...BOX }}>
        <div
          style={{
            width: 50,
            height: 100,
            color: "#ff0000",
            colorScheme: "dark",
            animation: "motion-colors-tint 1s linear forwards",
          }}
        />
        <div
          style={{
            width: 50,
            height: 100,
            color: "#ff0000",
            backgroundColor: "currentColor",
            transition: "background-color 1s linear",
          }}
        />
      </div>
    )
    expectNear(pixel(25, 50), RED)
    renderer.clockFastForward(1000)
    expectNear(pixel(25, 50), GREEN)
  })

  it("moves a currentColor background with the element's color, not with a transition of its own", () => {
    const { render, renderer } = createTestRoot()
    const pixel = sampler("transition", renderer)
    renderer.clockPause()
    const App = ({ color, transition }: { color: string; transition: string }) => (
      <div style={{ ...BOX, color, backgroundColor: "currentColor", transition }} />
    )
    // `currentColor` computes to itself: only the `color` transition runs, and the
    // background follows the colour showing each frame.
    render(<App color="#ff0000" transition="color 1s linear" />)
    expectNear(pixel(), RED)
    render(<App color="#0000ff" transition="color 1s linear" />)
    renderer.clockFastForward(500)
    const middle = pixel()
    expect(middle[0]).toBeGreaterThan(40)
    expect(middle[2]).toBeGreaterThan(40)
    renderer.clockFastForward(1000)
    expectNear(pixel(), BLUE)

    // With no transition on `color`, a background-color transition does not start.
    render(<App color="#00ff00" transition="background-color 1s linear" />)
    expectNear(pixel(), GREEN)
  })
})

describeNative("an animated svg changes hue", () => {
  const SVG = { width: 100, height: 100 } as const

  it("blends the fill of a shape between two hues", () => {
    const { render, renderer } = createTestRoot()
    const pixel = sampler("svg-hue", renderer)
    renderer.clockPause()
    render(
      <svg viewBox="0 0 100 100" style={SVG}>
        <motion.rect
          x={0}
          y={0}
          width={100}
          height={100}
          initial={{ fill: "#ff0000" }}
          animate={{ fill: "#0000ff" }}
          transition={LINEAR}
        />
      </svg>
    )
    expectNear(pixel(), RED)

    renderer.clockFastForward(500)
    const middle = pixel()
    // The Oklab midpoint of red and blue keeps both and is not grey.
    expect(middle[0]).toBeGreaterThan(60)
    expect(middle[2]).toBeGreaterThan(60)
    expect(middle[1]).toBeLessThan(middle[0])
    expect(middle[1]).toBeLessThan(middle[2])

    renderer.clockFastForward(1000)
    expectNear(pixel(), BLUE)
  })

  it("resolves light-dark() and currentColor against the svg's context", () => {
    const { render, renderer } = createTestRoot()
    const pixel = sampler("svg-context", renderer)
    renderer.clockPause()
    render(
      <svg viewBox="0 0 100 100" style={{ ...SVG, color: "#00ff00", colorScheme: "dark" }}>
        <motion.rect
          x={0}
          y={0}
          width={50}
          height={100}
          initial={{ fill: "#000000" }}
          animate={{ fill: "light-dark(#ff0000, #0000ff)" }}
          transition={LINEAR}
        />
        <motion.rect
          x={50}
          y={0}
          width={50}
          height={100}
          initial={{ fill: "#000000" }}
          animate={{ fill: "currentColor" }}
          transition={LINEAR}
        />
      </svg>
    )
    renderer.clockFastForward(1000)
    expectNear(pixel(25, 50), BLUE)
    expectNear(pixel(75, 50), GREEN)
  })

  it("repaints the same svg every frame of a long animation", () => {
    const { render, renderer } = createTestRoot()
    const pixel = sampler("svg-frames", renderer)
    renderer.clockPause()
    render(
      <svg viewBox="0 0 100 100" style={SVG}>
        <motion.rect
          x={0}
          y={0}
          width={100}
          height={100}
          initial={{ fill: "#ff0000" }}
          animate={{ fill: "#00ff00" }}
          transition={{ duration: 2, ease: "linear" }}
        />
      </svg>
    )
    let previous = pixel()
    let changes = 0
    for (let frame = 0; frame < 20; frame++) {
      renderer.clockFastForward(100)
      const next = pixel()
      if (next[0] !== previous[0]) changes++
      previous = next
    }
    expect(changes).toBeGreaterThan(15)
    expectNear(previous, GREEN)
  })
})
