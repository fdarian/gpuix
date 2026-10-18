/// `flex` as a motion target, `auto` margins and insets, unset border widths and the `em` of a motion
/// value, against what Chrome animates.
///
/// `fixtures/motion-values-oracle.html` is the page and `fixtures/motion-values-oracle.json` what headless
/// Chrome reports for it (regenerate with the command in the HTML): each case is a Web Animation or a CSS
/// transition on a child, sampled at fractions of a one second linear animation as `[x, y, width, height]`
/// relative to the scene.

import fs from "fs"
import path from "path"
import type { CSSProperties, ReactNode } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { animate, defineKeyframes, motion, motionValue } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Style = Record<string, unknown>
type Rect = [number, number, number, number]

interface Case {
  kind: "waapi" | "transition"
  parent: Style
  child: Style
  second: Style | null
  content?: boolean
  keyframes?: Record<string, unknown>
  after?: Style
  transition?: string
  animations: number
  times: number[]
  samples: Record<string, { child: Rect; second: Rect | null }>
}

const ORACLE: { cases: Record<string, Case> } = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/motion-values-oracle.json"),
    "utf8"
  )
)

/** Chrome holds a margin's CSS transition from a length to `auto` until its end (see FORK.md); gpuix flips at half. */
const KNOWN_DEVIATIONS: Record<string, number[]> = {
  "margin transition length to auto allow-discrete": [0.5, 0.6],
}

/** Chrome floors a fractional border width to a whole device pixel, so its animated widths step; gpuix blends them. */
const WHOLE_PIXEL_BORDERS = (name: string) => name.startsWith("border width")

/** Only layout can be read back, and an outline does not take any. */
const NOT_LAID_OUT = ["outline width unset under solid", "outline width unset under none"]

type Renderer = ReturnType<typeof createTestRoot>["renderer"]

const boundsOf = (renderer: Renderer, testId: string) => {
  renderer.dispatchNativeEvents()
  return renderer.getElementBounds(renderer.findByTestId(testId).id)
}

const linear = { duration: 1, ease: "linear" } as const

function scene(data: Case, child: (style: Style) => ReactNode, childStyle: Style): ReactNode {
  return (
    <div testId="parent" style={data.parent as CSSProperties}>
      {child(childStyle)}
      {data.second ? <div testId="second" style={data.second as CSSProperties} /> : null}
    </div>
  )
}

function inner(data: Case): ReactNode {
  return data.content ? <div style={{ width: 20, height: 20 }} /> : null
}

function sampleCase(name: string): { name: string; at: (time: number) => { child: Rect; second: Rect | null } } {
  const data = ORACLE.cases[name]
  const { render, renderer } = createTestRoot({ width: 300, height: 200 })
  renderer.clockPause()
  if (data.kind === "waapi") {
    const initial: Style = {}
    const animate: Style = {}
    for (const [prop, value] of Object.entries(data.keyframes ?? {})) {
      if (Array.isArray(value)) {
        initial[prop] = value[0]
        animate[prop] = value[1]
      } else {
        animate[prop] = value
      }
    }
    render(
      scene(
        data,
        (style) => (
          <motion.div
            testId="child"
            style={style as CSSProperties}
            initial={Object.keys(initial).length > 0 ? initial : { opacity: 1 }}
            animate={animate}
            transition={linear}
          >
            {inner(data)}
          </motion.div>
        ),
        data.child
      )
    )
  } else {
    const plain = (style: Style) => (
      <div testId="child" style={{ ...style, transition: data.transition } as CSSProperties}>
        {inner(data)}
      </div>
    )
    render(scene(data, plain, data.child))
    renderer.flush()
    render(scene(data, plain, { ...data.child, ...data.after }))
  }
  renderer.flush()

  const clock = { elapsed: 0 }
  return {
    name,
    at(time) {
      renderer.clockFastForward(time * 1000 - clock.elapsed)
      clock.elapsed = time * 1000
      renderer.flush()
      const origin = boundsOf(renderer, "parent")
      const rectOf = (testId: string): Rect => {
        const bounds = boundsOf(renderer, testId)
        return [bounds.x - origin.x, bounds.y - origin.y, bounds.width, bounds.height]
      }
      return { child: rectOf("child"), second: data.second ? rectOf("second") : null }
    },
  }
}

describeNative("motion and CSS transitions against Chrome's values", () => {
  for (const [name, data] of Object.entries(ORACLE.cases)) {
    if (NOT_LAID_OUT.includes(name)) continue
    it(name, () => {
      const run = sampleCase(name)
      for (const time of data.times) {
        if (KNOWN_DEVIATIONS[name]?.includes(time)) continue
        const expected = data.samples[String(time)]
        const actual = run.at(time)
        const label = `${name} at ${time}`
        for (const [index, value] of expected.child.entries()) {
          const tolerance = WHOLE_PIXEL_BORDERS(name) ? 1 : 0.5
          expect(Math.abs(actual.child[index] - value), `${label} child[${index}] ${actual.child[index]} vs ${value}`).toBeLessThanOrEqual(tolerance)
        }
        for (const [index, value] of (expected.second ?? []).entries()) {
          expect(actual.second?.[index], `${label} second[${index}]`).toBeCloseTo(value, 0)
        }
      }
    })
  }
})

describeNative("a motion value's em and rem", () => {
  const sized = (fontSize: number, x: ReturnType<typeof motionValue<string>>) => (
    <div style={{ fontSize, display: "flex", width: 300, height: 40, alignItems: "flex-start" }}>
      <div testId={`box-${fontSize}`} style={{ marginLeft: x, width: 10, height: 10 }} />
    </div>
  )

  it("resolves against each element it is bound to: 2em is 40px on a 20px element and 20px on a 10px one", () => {
    const { render, renderer } = createTestRoot({ width: 300, height: 200 })
    renderer.clockPause()
    const x = motionValue("2em")
    render(
      <div>
        {sized(20, x)}
        {sized(10, x)}
      </div>
    )
    renderer.flush()
    const left = (testId: string) => boundsOf(renderer, testId).x
    expect(left("box-20")).toBe(40)
    expect(left("box-10")).toBe(20)
    expect(x.get()).toBe("2em")

    x.set("1rem")
    renderer.flush()
    expect(left("box-20")).toBe(16)
    expect(left("box-10")).toBe(16)
  })

  it("animates em as a part: 1em to 3em is 2em halfway on each element", () => {
    const { render, renderer } = createTestRoot({ width: 300, height: 200 })
    renderer.clockPause()
    const x = motionValue("1em")
    render(
      <div>
        {sized(20, x)}
        {sized(10, x)}
      </div>
    )
    renderer.flush()
    animate(x, "3em", { duration: 1, ease: "linear" })
    renderer.clockFastForward(500)
    renderer.flush()
    expect(boundsOf(renderer, "box-20").x).toBeCloseTo(40, 0)
    expect(boundsOf(renderer, "box-10").x).toBeCloseTo(20, 0)
  })
})

defineKeyframes("s2-auto-margin", { from: { marginLeft: "auto" }, to: { marginLeft: 100 } })
defineKeyframes("s2-flex", { from: { flex: "1 1 0px" }, to: { flex: "3 1 100px" } })

describeNative("CSS @keyframes with auto and flex", () => {
  it("flips an auto margin to a length at half, as a Web Animation does", () => {
    const { render, renderer } = createTestRoot({ width: 300, height: 200 })
    renderer.clockPause()
    render(
      <div style={{ display: "flex", width: 300, height: 40, alignItems: "flex-start" }}>
        <div testId="box" style={{ width: 50, height: 20, animation: "s2-auto-margin 1s linear forwards" }} />
      </div>
    )
    renderer.flush()
    renderer.clockFastForward(400)
    renderer.flush()
    expect(boundsOf(renderer, "box").x).toBe(250)
    renderer.clockFastForward(200)
    renderer.flush()
    expect(boundsOf(renderer, "box").x).toBe(100)
  })

  it("animates flex through its longhands", () => {
    const { render, renderer } = createTestRoot({ width: 300, height: 200 })
    renderer.clockPause()
    render(
      <div style={{ display: "flex", width: 300, height: 40 }}>
        <div testId="a" style={{ height: 20, animation: "s2-flex 1s linear forwards" }} />
        <div testId="b" style={{ height: 20, flex: "1 1 0px" }} />
      </div>
    )
    renderer.flush()
    renderer.clockFastForward(500)
    renderer.flush()
    expect(boundsOf(renderer, "a").width).toBeCloseTo(216.67, 0)
    expect(boundsOf(renderer, "b").width).toBeCloseTo(83.33, 0)
  })
})
