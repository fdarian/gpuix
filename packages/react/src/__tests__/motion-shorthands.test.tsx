import fs from "fs"
import path from "path"
import type { ReactNode } from "react"
import { describe, expect, it } from "vitest"
import { motion } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { readLogicalPixels, SHOTS_DIR, type Pixels } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const linear = { duration: 1, ease: "linear" } as const

type Renderer = ReturnType<typeof createTestRoot>["renderer"]

const boundsOf = (renderer: Renderer, testId: string) => {
  renderer.dispatchNativeEvents()
  return renderer.getElementBounds(renderer.findByTestId(testId).id)
}

const mounted = (render: (node: ReactNode) => void, renderer: Renderer, node: ReactNode) => {
  renderer.clockPause()
  render(node)
  renderer.flush()
}

const column = (children: ReactNode) => (
  <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start" }}>{children}</div>
)

describeNative("a multi-value shorthand string in motion", () => {
  it("animates padding '0.5em 0.75em' to '1em 1.5em' with each axis on its own value", () => {
    const { render, renderer } = createTestRoot({ width: 400, height: 300 })
    const tree = (
      <motion.div
        testId="box"
        style={{ fontSize: 16 }}
        initial={{ padding: "0.5em 0.75em" }}
        animate={{ padding: "1em 1.5em" }}
        transition={linear}
      >
        <div testId="inner" style={{ width: 20, height: 20 }} />
      </motion.div>
    )
    mounted(render, renderer, column(tree))
    const offset = () => {
      const box = boundsOf(renderer, "box")
      const inner = boundsOf(renderer, "inner")
      return { x: inner.x - box.x, y: inner.y - box.y, width: box.width, height: box.height }
    }
    expect(offset()).toEqual({ x: 12, y: 8, width: 44, height: 36 })

    renderer.clockFastForward(500)
    const middle = offset()
    expect(middle.y).toBeCloseTo(12, 0)
    expect(middle.x).toBeCloseTo(18, 0)
    // The right and bottom sides took the same values as the top and the left.
    expect(middle.width).toBeCloseTo(20 + 2 * 18, 0)
    expect(middle.height).toBeCloseTo(20 + 2 * 12, 0)

    renderer.clockFastForward(1000)
    expect(offset()).toEqual({ x: 24, y: 16, width: 68, height: 52 })
  })

  it("animates a three-value padding with the right and left sides sharing the middle value", () => {
    const { render, renderer } = createTestRoot({ width: 400, height: 300 })
    mounted(
      render,
      renderer,
      column(
        <motion.div
          testId="box"
          initial={{ padding: 0 }}
          animate={{ padding: "10px 20px 30px" }}
          transition={linear}
        >
          <div style={{ width: 20, height: 20 }} />
        </motion.div>
      )
    )
    renderer.clockFastForward(1000)
    const box = boundsOf(renderer, "box")
    expect(box.width).toBe(20 + 40)
    expect(box.height).toBe(20 + 10 + 30)
  })

  it("animates a two-value inset to each side", () => {
    const { render, renderer } = createTestRoot({ width: 400, height: 300 })
    mounted(
      render,
      renderer,
      <div testId="parent" style={{ position: "relative", width: 400, height: 300 }}>
        <motion.div
          testId="box"
          style={{ position: "absolute", width: 20, height: 20 }}
          initial={{ inset: "0 0" }}
          animate={{ inset: "20px 40px" }}
          transition={linear}
        />
      </div>
    )
    const place = () => {
      const parent = boundsOf(renderer, "parent")
      const box = boundsOf(renderer, "box")
      return { x: box.x - parent.x, y: box.y - parent.y }
    }
    expect(place()).toEqual({ x: 0, y: 0 })
    renderer.clockFastForward(500)
    // top is the first value, left the second.
    expect(place().y).toBeCloseTo(10, 0)
    expect(place().x).toBeCloseTo(20, 0)
    renderer.clockFastForward(1000)
    expect(place()).toEqual({ x: 40, y: 20 })
  })

  it("animates a two-value gap as the row gap and then the column gap", () => {
    const { render, renderer } = createTestRoot({ width: 400, height: 300 })
    mounted(
      render,
      renderer,
      column(
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
      )
    )
    const gaps = () => ({
      column: boundsOf(renderer, "b").x - boundsOf(renderer, "a").x - 40,
      row: boundsOf(renderer, "c").y - boundsOf(renderer, "a").y - 10,
    })
    expect(gaps()).toEqual({ column: 0, row: 0 })
    renderer.clockFastForward(500)
    expect(gaps().row).toBeCloseTo(10, 0)
    expect(gaps().column).toBeCloseTo(4, 0)
    renderer.clockFastForward(1000)
    expect(gaps()).toEqual({ column: 8, row: 20 })
  })

  it("animates a four-value borderWidth side by side", () => {
    const { render, renderer } = createTestRoot({ width: 400, height: 300 })
    mounted(
      render,
      renderer,
      column(
        <motion.div
          testId="box"
          style={{ borderStyle: "solid" }}
          initial={{ borderWidth: 0 }}
          animate={{ borderWidth: "2px 4px 6px 8px" }}
          transition={linear}
        >
          <div testId="inner" style={{ width: 20, height: 20 }} />
        </motion.div>
      )
    )
    const widths = () => {
      const box = boundsOf(renderer, "box")
      const inner = boundsOf(renderer, "inner")
      return {
        top: inner.y - box.y,
        left: inner.x - box.x,
        right: box.x + box.width - (inner.x + inner.width),
        bottom: box.y + box.height - (inner.y + inner.height),
      }
    }
    renderer.clockFastForward(500)
    const middle = widths()
    expect(middle.top).toBeCloseTo(1, 0)
    expect(middle.right).toBeCloseTo(2, 0)
    expect(middle.bottom).toBeCloseTo(3, 0)
    expect(middle.left).toBeCloseTo(4, 0)
    renderer.clockFastForward(1000)
    expect(widths()).toEqual({ top: 2, right: 4, bottom: 6, left: 8 })
  })

  it("animates a keyframes list of padding strings, splitting every frame", () => {
    const { render, renderer } = createTestRoot({ width: 400, height: 300 })
    mounted(
      render,
      renderer,
      column(
        <motion.div
          testId="box"
          initial={{ padding: 0 }}
          animate={{ padding: ["0 0", "10px 20px", "30px 40px"] }}
          transition={linear}
        >
          <div testId="inner" style={{ width: 20, height: 20 }} />
        </motion.div>
      )
    )
    const offset = () => {
      const box = boundsOf(renderer, "box")
      const inner = boundsOf(renderer, "inner")
      return { x: inner.x - box.x, y: inner.y - box.y }
    }
    renderer.clockFastForward(500)
    expect(offset().y).toBeCloseTo(10, 0)
    expect(offset().x).toBeCloseTo(20, 0)
    renderer.clockFastForward(1000)
    expect(offset()).toEqual({ x: 40, y: 30 })
  })

  it("transitions a padding string as a CSS transition", () => {
    const { render, renderer } = createTestRoot({ width: 400, height: 300 })
    const box = (padding: string) =>
      column(
        <div testId="box" style={{ padding, fontSize: 16, transition: "padding 1s linear" }}>
          <div testId="inner" style={{ width: 20, height: 20 }} />
        </div>
      )
    const offset = () => {
      const outer = boundsOf(renderer, "box")
      const inner = boundsOf(renderer, "inner")
      return { x: inner.x - outer.x, y: inner.y - outer.y }
    }
    mounted(render, renderer, box("0.5em 0.75em"))
    expect(offset()).toEqual({ x: 12, y: 8 })
    render(box("1em 1.5em"))
    renderer.flush()
    renderer.clockFastForward(500)
    expect(offset().y).toBeCloseTo(12, 0)
    expect(offset().x).toBeCloseTo(18, 0)
    renderer.clockFastForward(1000)
    expect(offset()).toEqual({ x: 24, y: 16 })
  })

  it("animates borderColor 'red blue' with the top and bottom red and the sides blue", () => {
    const { render, renderer } = createTestRoot({ width: 200, height: 200 })
    const file = path.join(SHOTS_DIR, "motion-shorthand-border-color.png")
    fs.mkdirSync(SHOTS_DIR, { recursive: true })
    mounted(
      render,
      renderer,
      <motion.div
        style={{ width: 80, height: 80, borderStyle: "solid", borderWidth: 20, backgroundColor: "#ffffff" }}
        initial={{ borderColor: "#000000 #000000" }}
        animate={{ borderColor: "#ff0000 #0000ff" }}
        transition={linear}
      />
    )
    const sample = () => {
      renderer.dispatchNativeEvents()
      renderer.captureScreenshot(file)
      return readLogicalPixels(file, renderer)
    }
    // The middle of the top border, then of the left one.
    const top = (pixels: Pixels) => pixels.at(60, 10)
    const left = (pixels: Pixels) => pixels.at(10, 60)

    expect(top(sample()).slice(0, 3)).toEqual([0, 0, 0])
    renderer.clockFastForward(500)
    const middle = sample()
    expect(top(middle)[0]).toBeGreaterThan(30)
    expect(top(middle)[2]).toBeLessThan(30)
    expect(left(middle)[2]).toBeGreaterThan(30)
    expect(left(middle)[0]).toBeLessThan(30)
    renderer.clockFastForward(1000)
    const done = sample()
    expect(top(done).slice(0, 3)).toEqual([255, 0, 0])
    expect(left(done).slice(0, 3)).toEqual([0, 0, 255])
    expect(done.at(60, 70).slice(0, 3)).toEqual([255, 0, 0])
    expect(done.at(70, 60).slice(0, 3)).toEqual([0, 0, 255])
  })

  describe("borderRadius", () => {
    const FILL = "#ff0000"
    const MARGIN = 40

    const radiusShot = (name: string, renderer: Renderer) => {
      renderer.dispatchNativeEvents()
      const file = path.join(SHOTS_DIR, `motion-shorthand-radius-${name}.png`)
      fs.mkdirSync(SHOTS_DIR, { recursive: true })
      renderer.captureScreenshot(file)
      return readLogicalPixels(file, renderer)
    }

    /** Whether (x, y), in the box's own coordinates, is painted. */
    const painted = (pixels: Pixels, x: number, y: number) => {
      const [r, g, b] = pixels.at(MARGIN + x, MARGIN + y)
      if (r > 200 && g < 60 && b < 60) return true
      if (r > 240 && g > 240 && b > 240) return false
      throw new Error(`(${x}, ${y}) is neither the fill nor the page: ${[r, g, b]}`)
    }

    const page = (animate: Record<string, unknown>, initial: Record<string, unknown>) => (
      <div style={{ width: 280, height: 220, padding: MARGIN, backgroundColor: "#ffffff" }}>
        <motion.div
          style={{ width: 200, height: 140, backgroundColor: FILL }}
          initial={initial}
          animate={animate}
          transition={linear}
        />
      </div>
    )

    it("animates '10px / 20px' to '30px / 40px' through a 20px by 30px corner", () => {
      const { render, renderer } = createTestRoot({ width: 280, height: 220 })
      mounted(render, renderer, page({ borderRadius: "30px / 40px" }, { borderRadius: "10px / 20px" }))

      // Halfway the corner is an ellipse of 20px across and 30px down. (1, 13) and
      // (4, 8) are outside it but inside a 20px circle, so the vertical radius
      // has moved on from the start's 20px.
      renderer.clockFastForward(500)
      const middle = radiusShot("middle", renderer)
      expect(painted(middle, 8, 12)).toBe(true)
      expect(painted(middle, 1, 13)).toBe(false)
      expect(painted(middle, 4, 8)).toBe(false)
      expect(painted(middle, 20, 2)).toBe(true)
      // The same corner on the opposite side of the box.
      expect(painted(middle, 199 - 8, 139 - 12)).toBe(true)
      expect(painted(middle, 199 - 1, 139 - 13)).toBe(false)

      renderer.clockFastForward(1000)
      const done = radiusShot("done", renderer)
      // A 30px by 40px ellipse centred at (30, 40): (6, 12) is inside, (3, 8) outside.
      expect(painted(done, 12, 16)).toBe(true)
      expect(painted(done, 3, 8)).toBe(false)
    })

    it("animates a circular radius into an elliptical one", () => {
      const { render, renderer } = createTestRoot({ width: 280, height: 220 })
      mounted(render, renderer, page({ borderTopLeftRadius: "40px 80px" }, { borderTopLeftRadius: 0 }))
      renderer.clockFastForward(500)
      const middle = radiusShot("longhand-middle", renderer)
      // A 20px by 40px ellipse at the top left, and the other corners still square.
      expect(painted(middle, 1, 1)).toBe(false)
      expect(painted(middle, 3, 25)).toBe(true)
      expect(painted(middle, 25, 1)).toBe(true)
      expect(painted(middle, 199, 0)).toBe(true)
      expect(painted(middle, 0, 139)).toBe(true)
    })
  })
})
