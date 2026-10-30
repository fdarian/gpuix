/// `filter` animates: a CSS transition, a CSS animation and the keys of a `motion` component (`initial`, `animate`,
/// `exit`, `whileHover`, tweens and springs) blend the lists of filter functions as Chrome does
/// (`fixtures/filter-interp-oracle.json`, the numbers of which `motion/filter_value.rs` replays), and what is painted
/// is the filter at that moment. A moment is read from the picture: a black box on white has soft edge pixels in
/// proportion to its blur, a grey box gets brighter with `brightness()`.

import fs from "fs"
import path from "path"
import React from "react"
import { beforeAll, describe, expect, it } from "vitest"
import { AnimatePresence, defineKeyframes, motion } from "../index"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const LINEAR = { duration: 1, ease: "linear" } as const
const STAGE = { position: "relative", width: 200, height: 200, backgroundColor: "#ffffff" } as const
const BOX = { position: "absolute", left: 60, top: 60, width: 40, height: 40 } as const

defineKeyframes("fm-brighten", {
  from: { filter: "brightness(1)" },
  "50%": { filter: "brightness(2)" },
  to: { filter: "brightness(1)" },
})
defineKeyframes("fm-blur-in", { to: { filter: "blur(8px)" } })
defineKeyframes("fm-list", {
  from: { filter: "none" },
  to: { filter: "blur(8px) brightness(2)" },
})
defineKeyframes("fm-step", {
  from: { filter: "blur(0px)" },
  "50%": { filter: "blur(8px)" },
  to: { filter: "brightness(2)" },
})

function Stage(props: { children: React.ReactNode }) {
  return <div style={STAGE}>{props.children}</div>
}

function setup(node: React.ReactNode) {
  const root = createTestRoot({ width: 200, height: 200 })
  const renderer = root.renderer
  renderer.clockPause()
  let shots = 0
  const page = {
    root,
    renderer,
    show: (next: React.ReactNode) => {
      root.render(next)
      renderer.flush()
    },
    advance: (ms: number) => {
      renderer.clockFastForward(ms)
      renderer.flush()
      renderer.dispatchNativeEvents()
    },
    pixels: () => {
      renderer.dispatchNativeEvents()
      const file = path.join(SHOTS_DIR, `filter-motion-${process.pid}-${shots++}.png`)
      renderer.captureScreenshot(file)
      const pixels = readLogicalPixels(file, renderer)
      fs.rmSync(file)
      return pixels
    },
    /** The red channel at the logical point. */
    red: (x: number, y: number) => page.pixels().at(x, y)[0]!,
    /** How many pixels of the row through the middle of the box are neither black nor white: the softness of its edges. */
    soft: () => {
      const pixels = page.pixels()
      let count = 0
      for (let x = 30; x < 130; x++) {
        const red = pixels.at(x, 80)[0]!
        if (red > 8 && red < 247) count++
      }
      return count
    },
  }
  root.render(node)
  renderer.flush()
  return page
}

/** The softness of the row for a box that is statically `filter`. */
function softOf(filter: string) {
  const page = setup(
    <Stage>
      <div style={{ ...BOX, backgroundColor: "#000000", filter }} />
    </Stage>
  )
  const soft = page.soft()
  page.root.unmount()
  return soft
}

/** The softness of the still blurs the animated ones are compared with: measured first, as one test renderer lives at a time. */
const REF = { half: 0, full: 0 }
beforeAll(() => {
  if (!hasNativeTestRenderer) return
  REF.half = softOf("blur(4px)")
  REF.full = softOf("blur(8px)")
})

function expectNear(actual: number, expected: number, tolerance: number, what = "") {
  expect(Math.abs(actual - expected), `${what}: ${actual} vs ${expected}`).toBeLessThanOrEqual(tolerance)
}

const transitioned = (filter: string, extra: object = {}, transition: string | null = "filter 1s linear") => (
  <Stage>
    <div testId="box" style={{ ...BOX, backgroundColor: "#000000", filter, ...(transition ? { transition } : {}), ...extra }} />
  </Stage>
)

const grey = (filter: string, extra: object = {}, transition: string | null = "filter 1s linear") =>
  transitioned(filter, { backgroundColor: "#404040", ...extra }, transition)

describeNative("filter in a CSS transition", () => {
  it("softens an edge as the blur grows, as a still blur of that radius does", () => {
    const page = setup(transitioned("blur(0px)"))
    expect(page.soft()).toBeLessThanOrEqual(2)
    page.show(transitioned("blur(8px)"))
    page.advance(0)
    expect(page.soft()).toBeLessThanOrEqual(2)
    page.advance(500)
    expectNear(page.soft(), REF.half, 2, "halfway")
    page.advance(500)
    expectNear(page.soft(), REF.full, 2, "at the end")
    expect(REF.full).toBeGreaterThan(REF.half + 4)
    page.root.unmount()
  })

  it("blends brightness, and a list against none pads every function", () => {
    const page = setup(grey("brightness(1)"))
    expectNear(page.red(80, 80), 0x40, 1)
    page.show(grey("brightness(2)"))
    page.advance(500)
    expectNear(page.red(80, 80), 0x60, 3, "brightness(1.5)")
    page.advance(500)
    expectNear(page.red(80, 80), 0x80, 1, "brightness(2)")

    page.show(grey("none"))
    page.advance(500)
    expectNear(page.red(80, 80), 0x60, 3, "none padded with brightness(1)")
    page.advance(500)
    expectNear(page.red(80, 80), 0x40, 1, "none")
    page.root.unmount()
  })

  it("blends the functions of a list one by one", () => {
    const page = setup(grey("blur(0px) brightness(1)"))
    page.show(grey("blur(8px) brightness(2)"))
    page.advance(500)
    // The middle of the box is far from the blurred edge, so only the brightness shows there.
    expectNear(page.red(80, 80), 0x60, 3)
    page.root.unmount()
  })

  it("starts no transition between lists that cannot blend, as Chrome starts none", () => {
    const page = setup(grey("blur(4px)"))
    page.show(grey("brightness(2)"))
    page.advance(200)
    expectNear(page.red(80, 80), 0x80, 1, "at once")
    page.root.unmount()
  })

  it("flips lists that cannot blend at the middle under allow-discrete", () => {
    const discrete = (filter: string) => grey(filter, {}, "filter 1s linear allow-discrete")
    const page = setup(discrete("brightness(1)"))
    page.show(discrete("contrast(0)"))
    page.advance(400)
    expectNear(page.red(80, 80), 0x40, 1, "before the middle")
    page.advance(200)
    expectNear(page.red(70, 70), 0x80, 3, "contrast(0) is mid grey")
    page.root.unmount()
  })

  it("blends a drop-shadow offset and colour", () => {
    const page = setup(
      <Stage>
        <div
          testId="box"
          style={{ ...BOX, backgroundColor: "#00ff00", filter: "drop-shadow(0px 0px 0px rgb(255, 0, 0))", transition: "filter 1s linear" }}
        />
      </Stage>
    )
    const shadowed = (filter: string) => (
      <Stage>
        <div testId="box" style={{ ...BOX, backgroundColor: "#00ff00", filter, transition: "filter 1s linear" }} />
      </Stage>
    )
    page.show(shadowed("drop-shadow(20px 0px 0px rgb(0, 0, 255))"))
    page.advance(500)
    // 10px right of the box, the shadow is the mix of red and blue.
    const [r, g, b] = page.pixels().at(105, 80)
    expectNear(r!, 128, 4, "red")
    expectNear(b!, 128, 4, "blue")
    expect(g).toBeLessThan(4)
    page.root.unmount()
  })
})

describeNative("filter in a CSS animation", () => {
  it("runs a keyframed brightness up and back", () => {
    const page = setup(grey("none", { animation: "fm-brighten 1s linear" }, null))
    expectNear(page.red(80, 80), 0x40, 1, "start")
    page.advance(250)
    expectNear(page.red(80, 80), 0x60, 3, "a quarter")
    page.advance(250)
    expectNear(page.red(80, 80), 0x80, 3, "half")
    page.advance(500)
    expectNear(page.red(80, 80), 0x40, 1, "end")
    page.root.unmount()
  })

  it("runs from the filter the element has, here none, to a keyframe", () => {
    const page = setup(transitioned("none", { animation: "fm-blur-in 1s linear forwards" }, null))
    expect(page.soft()).toBeLessThanOrEqual(2)
    page.advance(500)
    expectNear(page.soft(), REF.half, 2, "halfway")
    page.advance(600)
    expectNear(page.soft(), REF.full, 2, "forwards")
    page.root.unmount()
  })

  it("pads a shorter list with the identity of the longer one", () => {
    const page = setup(grey("none", { animation: "fm-list 1s linear" }, null))
    page.advance(500)
    expectNear(page.red(80, 80), 0x60, 3)
    page.root.unmount()
  })

  it("decides each interval on its own: the first blurs, the second steps", () => {
    const page = setup(transitioned("none", { animation: "fm-step 1s linear" }, null))
    page.advance(250)
    expectNear(page.soft(), REF.half, 2, "first interval blends")
    page.advance(260)
    // 51% in: the second interval has passed its middle at 75%, not yet.
    expect(page.soft()).toBeGreaterThan(REF.half)
    page.advance(240)
    expect(page.soft()).toBeLessThanOrEqual(2)
    page.root.unmount()
  })

  it("lists its keyframes as Chrome writes the computed values", () => {
    let box: PublicInstance | null = null
    const page = setup(
      <Stage>
        <div
          ref={(element) => {
            box = element as PublicInstance
          }}
          testId="box"
          style={{ ...BOX, animation: "fm-list 1s linear" }}
        />
      </Stage>
    )
    page.advance(0)
    const animation = box!.getAnimations()[0]!
    const keyframes = (animation.effect as KeyframeEffect).getKeyframes()
    expect(keyframes.map((frame) => frame.filter)).toEqual(["none", "blur(8px) brightness(2)"])
    page.root.unmount()
  })
})

describeNative("filter in a motion component", () => {
  it("animates FF's blur-in with a scale on a spring", () => {
    const spring = { type: "spring", stiffness: 300, damping: 30 } as const
    const page = setup(
      <Stage>
        <motion.div
          testId="box"
          style={{ ...BOX, backgroundColor: "#000000" }}
          initial={{ scale: 0.85, filter: "blur(4px)" }}
          animate={{ scale: 1, filter: "blur(0px)" }}
          transition={spring}
        />
      </Stage>
    )
    const start = page.soft()
    // The box starts at 0.85 of its size, so it has less edge to blur.
    expectNear(start, REF.half, 6, "starts blurred")
    page.advance(100)
    const middle = page.soft()
    expect(middle).toBeLessThan(start)
    expect(middle).toBeGreaterThan(2)
    page.advance(2000)
    expect(page.soft()).toBeLessThanOrEqual(2)
    const box = page.renderer.getElementBounds(page.renderer.findByTestId("box")!.id)!
    expectNear(box.width, 40, 0.01, "rests at scale 1")
    page.root.unmount()
  })

  it("tweens a list against none and holds the range of the amounts while a spring overshoots", () => {
    const page = setup(
      <Stage>
        <motion.div
          testId="box"
          style={{ ...BOX, backgroundColor: "#ff0000" }}
          initial={{ filter: "grayscale(0.2) brightness(1)" }}
          animate={{ filter: "grayscale(0.8) brightness(1)" }}
          transition={{ type: "spring", stiffness: 500, damping: 5 }}
        />
      </Stage>
    )
    for (let frame = 0; frame < 60; frame++) {
      page.advance(16)
      const [r, g, b] = page.pixels().at(80, 80)
      // grayscale(1) of red has r = g = b; above 1 it must not invert the hue.
      expect(r! >= g! && r! >= b!, `${r},${g},${b}`).toBe(true)
    }
    page.root.unmount()
  })

  it("tweens between keyframes of lists", () => {
    const page = setup(
      <Stage>
        <motion.div
          testId="box"
          style={{ ...BOX, backgroundColor: "#404040" }}
          initial={{ filter: "brightness(1)" }}
          animate={{ filter: ["brightness(1)", "brightness(2)", "brightness(1)"] }}
          transition={LINEAR}
        />
      </Stage>
    )
    page.advance(250)
    expectNear(page.red(80, 80), 0x60, 3, "a quarter")
    page.advance(250)
    expectNear(page.red(80, 80), 0x80, 3, "half")
    page.root.unmount()
  })

  it("retargets in flight from the filter it shows, carrying the speed of a spring", () => {
    const spring = { type: "spring", stiffness: 100, damping: 20 } as const
    const view = (filter: string) => (
      <Stage>
        <motion.div
          testId="box"
          style={{ ...BOX, backgroundColor: "#000000" }}
          initial={{ filter: "blur(0px)" }}
          animate={{ filter }}
          transition={spring}
        />
      </Stage>
    )
    const page = setup(view("blur(8px)"))
    page.advance(150)
    const before = page.soft()
    expect(before).toBeGreaterThan(2)
    page.show(view("blur(0px)"))
    page.advance(16)
    const after = page.soft()
    // The blur keeps growing for a moment: the spring carried its speed, and did not restart from rest at the target.
    expect(after).toBeGreaterThanOrEqual(before - 1)
    page.advance(3000)
    expect(page.soft()).toBeLessThanOrEqual(2)
    page.root.unmount()
  })

  it("runs a whileHover filter and its way back", () => {
    const page = setup(
      <Stage>
        <motion.div
          testId="box"
          style={{ ...BOX, backgroundColor: "#000000" }}
          whileHover={{ filter: "blur(8px)" }}
          transition={LINEAR}
        />
      </Stage>
    )
    expect(page.soft()).toBeLessThanOrEqual(2)
    page.renderer.nativeSimulateMouseMove(80, 80)
    page.advance(1)
    page.advance(499)
    expectNear(page.soft(), REF.half, 3, "halfway in")
    page.advance(500)
    expectNear(page.soft(), REF.full, 3, "hovered")
    page.renderer.nativeSimulateMouseMove(190, 190)
    page.advance(1)
    page.advance(1000)
    expect(page.soft()).toBeLessThanOrEqual(2)
    page.root.unmount()
  })

  it("runs an exit filter while the element leaves", () => {
    function Presence({ shown }: { shown: boolean }) {
      return (
        <Stage>
          <AnimatePresence>
            {shown && (
              <motion.div
                key="a"
                testId="box"
                style={{ ...BOX, backgroundColor: "#000000" }}
                exit={{ filter: "blur(8px)" }}
                transition={LINEAR}
              />
            )}
          </AnimatePresence>
        </Stage>
      )
    }
    const page = setup(<Presence shown />)
    expect(page.soft()).toBeLessThanOrEqual(2)
    page.show(<Presence shown={false} />)
    page.advance(500)
    expectNear(page.soft(), REF.half, 3, "halfway out")
    page.advance(600)
    expect(page.renderer.findByTestId("box")).toBeFalsy()
    page.root.unmount()
  })
})
