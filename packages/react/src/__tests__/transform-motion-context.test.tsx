/// Whether an element has a transform in CSS's sense, as a stacking context and as the containing block of its fixed
/// descendants, when its transform comes from Motion's keys or from a CSS animation or transition.
///
/// - Motion writes `transform: none` when every key is at its default (`buildTransform`), and in the DOM `none` makes
///   neither. So `x: 0`, `scale: 1` and `rotate: 0` leave the box as a plain one, `x: 10` does not, and a tween of `x`
///   forms the context only on the frames whose `x` is not 0.
/// - A CSS animation or transition of a transform property makes the element a stacking context and a containing block
///   for as long as it runs, at the values of the run that are `none` or the identity: `fixtures/transform-run-context-
///   oracle.html` is the page and `.json` what headless Chrome shows (regenerate with `transform-run-context-oracle.sh`).

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { motion } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rgba = [number, number, number, number]
const RED: Rgba = [255, 0, 0, 255]
const BLUE: Rgba = [0, 0, 255, 255]

interface Observation {
  redOnTop: boolean
  fixed: [number, number]
}

const CHROME: Record<string, Observation & { running?: number }> = JSON.parse(
  fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/transform-run-context-oracle.json"), "utf8")
)

/** What Chrome shows of `name`, without whether an animation was current. */
function chrome(name: string): Observation {
  const seen = CHROME[name]
  expect(seen, `${name} is in the oracle`).toBeDefined()
  return { redOnTop: seen!.redOnTop, fixed: seen!.fixed }
}

const TIMEOUT = 120_000

/**
 * A red box at (100, 100) holding a fixed child 5px inside it, and a later blue box over its lower half. The red box is
 * over the blue one where it is a stacking context, and the child is at (105, 105) where it is a containing block.
 */
function scene(red: (props: { testId: string; style: object; children: React.ReactNode }) => React.ReactElement) {
  return (
    <div style={{ width: 400, height: 400, paddingLeft: 100, paddingTop: 100, backgroundColor: "#ffffff" }}>
      {red({
        testId: "red",
        style: { width: 100, height: 100, backgroundColor: "#ff0000" },
        children: <div testId="fixed" style={{ position: "fixed", left: 5, top: 5, width: 20, height: 20, backgroundColor: "#00ff00" }} />,
      })}
      <div style={{ width: 100, height: 100, marginTop: -50, backgroundColor: "#0000ff" }} />
    </div>
  )
}

const motionRed = (motionProps: object, style: object = {}) =>
  scene((props) => (
    <motion.div testId={props.testId} style={{ ...props.style, ...style }} {...motionProps}>
      {props.children}
    </motion.div>
  ))

const cssRed = (style: object) =>
  scene((props) => (
    <div testId={props.testId} style={{ ...props.style, ...style }}>
      {props.children}
    </div>
  ))

type Mounted = ReturnType<typeof createTestRoot>

function observe(mounted: Mounted, name: string): Observation {
  const renderer = mounted.renderer
  renderer.flush()
  renderer.dispatchNativeEvents()
  const file = `${SHOTS_DIR}/transform-motion-context-${process.pid}-${name.replace(/\W+/g, "-")}.png`
  fs.rmSync(file, { force: true })
  renderer.captureScreenshot(file)
  const pixel = readLogicalPixels(file, renderer).at(150, 175)
  const box = renderer.getElementBounds(renderer.findByTestId("fixed").id)
  expect(box, "the fixed child has a box").not.toBeNull()
  expect([RED, BLUE], `the pixel at (150, 175) is ${pixel}`).toContainEqual(pixel)
  return { redOnTop: pixel[0] === RED[0], fixed: [box!.x, box!.y] }
}

function mount(tree: React.ReactNode) {
  const mounted = createTestRoot({ width: 400, height: 400 })
  mounted.renderer.clockPause()
  mounted.render(tree)
  return mounted
}

function seen(tree: React.ReactNode, name: string) {
  const mounted = mount(tree)
  try {
    return observe(mounted, name)
  } finally {
    mounted.unmount()
  }
}

const PLAIN: Observation = { redOnTop: false, fixed: [5, 5] }
const TRANSFORMED: Observation = { redOnTop: true, fixed: [105, 105] }

describeNative("a motion element whose keys are all at their defaults has no transform", { timeout: TIMEOUT }, () => {
  it("is neither a stacking context nor a containing block, as with transform: none", () => {
    expect(seen(cssRed({ transform: "none" }), "css-none")).toEqual(PLAIN)
    for (const [name, style] of Object.entries({
      x: { x: 0 },
      scale: { scale: 1 },
      rotate: { rotate: 0 },
      "all keys": { x: 0, y: "0px", translateX: "0%", scale: 1, scaleX: 1, scaleY: 1, rotate: 0, skew: 0, skewX: 0, skewY: 0 },
    })) {
      expect(seen(motionRed({}, style), name), name).toEqual(PLAIN)
    }
    expect(seen(motionRed({ initial: { x: 0 }, animate: { x: 0 } }), "animate-x-0")).toEqual(PLAIN)
  })

  it("replaces a static transform, as Motion's `none` overwrites the one in the style", () => {
    expect(seen(motionRed({}, { transform: "translate(40px, 40px)", x: 0 }), "replaces")).toEqual(PLAIN)
  })

  it("is both with a key off its default, and with the CSS transform of a plain element", () => {
    for (const [name, style] of Object.entries({ x: { x: 10 }, scale: { scale: 1.5 }, rotate: { rotate: 5 }, "skewX beside x at 0": { x: 0, skewX: 5 } })) {
      const got = seen(motionRed({}, style), name)
      expect(got.redOnTop, name).toBe(true)
      expect(got.fixed[1], name).toBeGreaterThan(5)
    }
    expect(seen(motionRed({}, { x: 10 }), "x-10").fixed).toEqual([115, 105])
    expect(seen(cssRed({ transform: "translateX(0px)" }), "css-identity")).toEqual(TRANSFORMED)
  })
})

describeNative("a tween of a motion key is a transform on the frames whose transform is not none", { timeout: TIMEOUT }, () => {
  it("forms the context during a run of x from 0 and drops it after returning to 0", () => {
    const mounted = mount(motionRed({ initial: { x: 0 }, animate: { x: [0, 40, 0] }, transition: { duration: 1, ease: "linear" } }))
    try {
      const renderer = mounted.renderer
      expect(observe(mounted, "tween-start")).toEqual(PLAIN)
      renderer.clockFastForward(500)
      expect(observe(mounted, "tween-middle")).toEqual({ redOnTop: true, fixed: [145, 105] })
      renderer.clockFastForward(250)
      expect(observe(mounted, "tween-late").redOnTop).toBe(true)
      renderer.clockFastForward(1000)
      expect(observe(mounted, "tween-end")).toEqual(PLAIN)
    } finally {
      mounted.unmount()
    }
  })

  it("forms it on the frames of a scale run from 2, and drops it where the run ends at 1", () => {
    const mounted = mount(motionRed({ initial: { scale: 2 }, animate: { scale: 1 }, transition: { duration: 1, ease: "linear" } }))
    try {
      const renderer = mounted.renderer
      expect(observe(mounted, "scale-start").redOnTop).toBe(true)
      renderer.clockFastForward(500)
      expect(observe(mounted, "scale-middle").redOnTop).toBe(true)
      renderer.clockFastForward(1000)
      expect(observe(mounted, "scale-end")).toEqual(PLAIN)
    } finally {
      mounted.unmount()
    }
  })
})

describeNative("a CSS animation or transition of the transform keeps the context through the run, as Chrome does", { timeout: TIMEOUT }, () => {
  const transitions: [string, string | undefined, string | undefined][] = [
    ["transition none to translateX(100px)", "none", "translateX(100px)"],
    ["transition translateX(0px) to none", "translateX(0px)", "none"],
    ["transition none to translateX(0px)", "none", "translateX(0px)"],
    ["transition translateX(100px) to none, running", "translateX(100px)", "none"],
  ]
  for (const [name, from, to] of transitions) {
    it(`${name}`, () => {
      const style = (transform: string) => ({ transition: "transform 1s linear", transform })
      const mounted = mount(cssRed(style(from!)))
      try {
        mounted.renderer.flush()
        mounted.render(cssRed(style(to!)))
        mounted.renderer.flush()
        mounted.renderer.clockFastForward(500)
        expect(observe(mounted, name)).toEqual(chrome(name))
      } finally {
        mounted.unmount()
      }
    })
  }

  it("drops it when the transition has ended on none", () => {
    const style = (transform: string) => ({ transition: "transform 1s linear", transform })
    const mounted = mount(cssRed(style("translateX(100px)")))
    try {
      mounted.renderer.flush()
      mounted.render(cssRed(style("none")))
      mounted.renderer.flush()
      mounted.renderer.clockFastForward(2000)
      expect(observe(mounted, "transition-finished")).toEqual(chrome("transition translateX(100px) to none, finished"))
    } finally {
      mounted.unmount()
    }
  })

  it("transition of rotate from none to 0deg", () => {
    const style = (rotate: string) => ({ transition: "rotate 1s linear", rotate })
    const mounted = mount(cssRed(style("none")))
    try {
      mounted.renderer.flush()
      mounted.render(cssRed(style("0deg")))
      mounted.renderer.flush()
      mounted.renderer.clockFastForward(500)
      expect(observe(mounted, "rotate-run")).toEqual(chrome("transition rotate 0deg to 0deg via none"))
    } finally {
      mounted.unmount()
    }
  })
})
