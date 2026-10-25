// @ts-nocheck
/**
 * Motion's transform keys (`x`, `y`, `scale`, `rotate`, `skew`, `originX`, ...) on a `motion.*` component.
 *
 * - They compose in the order of Motion's `buildTransform` and mean what the same CSS transform means: the box
 *   they give is the one a plain element with that `transform` has, to the pixel.
 * - A tween, a spring and a motion value move an element through them and every tick of that is patched: the
 *   elements that run are built alone and gpui paints their operations again (`getPatchCounters`).
 * - The 3D keys are accepted and draw nothing. The CSS `scale` and `rotate` of a plain element are still CSS.
 */

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { animate, motion, useMotionValue } from "../index"
import type { MotionValue } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { verifying } from "./retained-harness.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const patching = process.env.GPUIX_RETAIN_OFF === undefined && process.env.GPUIX_PATCH_OFF === undefined
const TIMEOUT = 180_000
const TOLERANCE = 0.01

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

const SIZE = { width: 800, height: 600 }
const BOX = { width: 100, height: 50 }
const linear = { duration: 1, ease: "linear" } as const

/** The box of `testId` in the window: the one that holds the corners of its border box after every transform. */
function bounds(renderer, testId: string): [number, number, number, number] {
  const box = renderer.getElementBounds(renderer.findByTestId(testId).id)
  expect(box, `${testId} has a box`).not.toBeNull()
  return [box.x, box.y, box.width, box.height]
}

function expectBox(got: number[], want: number[], what: string) {
  for (const [index, value] of want.entries()) {
    expect(Math.abs(got[index]! - value), `${what}: [${got}] is not [${want}]`).toBeLessThanOrEqual(TOLERANCE)
  }
}

/** A box in a padded frame, so that a transform has room, drawn by `make`. */
function framed(make: (style: object) => React.ReactNode) {
  return (style: object) => (
    <div style={{ display: "flex", padding: 200 }}>
      {make({ width: BOX.width, height: BOX.height, backgroundColor: "#336699", ...style })}
    </div>
  )
}

const motionBox = framed((style) => <motion.div testId="box" style={style} />)
const cssBox = framed((style) => <div testId="box" style={style} />)

/** Mounts `tree`, and returns the box of `testId`. */
function boundsOf(tree: React.ReactNode) {
  const { render, renderer, unmount } = createTestRoot(SIZE)
  try {
    renderer.clockPause()
    render(tree)
    renderer.flush()
    return bounds(renderer, "box")
  } finally {
    unmount()
  }
}

describeNative("Motion's transform keys compose like the CSS transform they stand for", () => {
  const cases: [string, object, object][] = [
    ["x, y, scale and rotate", { x: 10, y: 5, scale: 2, rotate: 30 }, { transform: "translateX(10px) translateY(5px) scale(2) rotate(30deg)" }],
    [
      "every key, in Motion's order",
      { skewY: 4, skewX: 3, skew: 5, rotateZ: 10, rotate: 20, scaleY: 0.5, scaleX: 2, scale: 1.5, translateY: 7, translateX: 5, y: 20, x: 10 },
      {
        transform:
          "translateX(10px) translateY(20px) translateX(5px) translateY(7px) scale(1.5) scaleX(2) scaleY(0.5) rotate(20deg) rotate(10deg) skew(5deg) skewX(3deg) skewY(4deg)",
      },
    ],
    ["percentages of the border box", { x: "50%", y: "-25%" }, { transform: "translateX(50%) translateY(-25%)" }],
    ["length strings", { x: "2em", translateY: "10px" }, { transform: "translateX(2em) translateY(10px)" }],
    ["angle strings", { rotate: "0.25turn", skewX: "10deg" }, { transform: "rotate(0.25turn) skewX(10deg)" }],
    ["scaleX and scaleY alone", { scaleX: 2, scaleY: 0.5 }, { transform: "scaleX(2) scaleY(0.5)" }],
    ["fractions of the origin", { rotate: 45, originX: 0, originY: 0 }, { transform: "rotate(45deg)", transformOrigin: "0% 0%" }],
    ["lengths of the origin", { rotate: 45, originX: "10px", originY: "25%" }, { transform: "rotate(45deg)", transformOrigin: "10px 25%" }],
    ["an origin axis over transformOrigin", { rotate: 45, originX: 0, transformOrigin: "100% 100%" }, { transform: "rotate(45deg)", transformOrigin: "0% 100%" }],
    ["instead of transform", { x: 5, transform: "translateX(100px) rotate(90deg)" }, { transform: "translateX(5px)" }],
    ["before the individual CSS properties", { x: 10, translate: "7px 3px" }, { transform: "translateX(10px)", translate: "7px 3px" }],
  ]
  for (const [name, motionStyle, cssStyle] of cases) {
    it(`gives the box of the CSS transform: ${name}`, () => {
      const got = boundsOf(motionBox(motionStyle))
      const want = boundsOf(cssBox(cssStyle))
      expectBox(got, want, name)
      expect(want, "the transform did something").not.toEqual(boundsOf(cssBox({})))
    }, TIMEOUT)
  }

  it("leaves the box where it is when no key is set", () => {
    expect(boundsOf(motionBox({}))).toEqual(boundsOf(cssBox({})))
  }, TIMEOUT)

  it("accepts the 3D keys and draws nothing for them", () => {
    const plain = boundsOf(motionBox({ x: 10 }))
    const deep = boundsOf(motionBox({ x: 10, z: 40, translateZ: 20, rotateX: 45, rotateY: 30, transformPerspective: 500 }))
    expectBox(deep, plain, "3D keys")
    const onlyDeep = boundsOf(motionBox({ z: 40, rotateX: 45 }))
    expectBox(onlyDeep, boundsOf(motionBox({})), "only 3D keys")
  }, TIMEOUT)

  it("keeps the CSS scale and rotate of a plain element CSS", () => {
    const css = boundsOf(cssBox({ scale: 2, rotate: 30 }))
    expectBox(css, boundsOf(cssBox({ transform: "scale(2) rotate(30deg)" })), "plain element")
    // The CSS properties go translate, rotate, scale: rotate is applied to what scale leaves.
    expectBox(boundsOf(cssBox({ rotate: 30, scale: 2 })), css, "order of the keys")
  }, TIMEOUT)

  it("takes a motion component's own scale and rotate as Motion's", () => {
    const got = boundsOf(motionBox({ scale: 2, rotate: 30 }))
    expectBox(got, boundsOf(cssBox({ transform: "scale(2) rotate(30deg)" })), "static style")
    expectBox(got, boundsOf(cssBox({ scale: 2, rotate: 30 })), "CSS properties")
  }, TIMEOUT)
})

describeNative("an animation of the transform keys", () => {
  function run(animateTo: object, transition: object, ms: number, initial?: object) {
    const { render, renderer, unmount } = createTestRoot(SIZE)
    try {
      renderer.clockPause()
      render(
        <div style={{ display: "flex", padding: 200 }}>
          <motion.div testId="box" initial={initial} animate={animateTo} transition={transition} style={{ ...BOX, backgroundColor: "#336699" }} />
        </div>
      )
      renderer.flush()
      renderer.clockFastForward(ms)
      renderer.flush()
      renderer.dispatchNativeEvents()
      return bounds(renderer, "box")
    } finally {
      unmount()
    }
  }

  it("tweens x, rotate and scale from Motion's defaults to the target", () => {
    const half = run({ x: 100, rotate: 90, scale: 2 }, linear, 500)
    expectBox(half, boundsOf(cssBox({ transform: "translateX(50px) scale(1.5) rotate(45deg)" })), "half way")
    const end = run({ x: 100, rotate: 90, scale: 2 }, linear, 1000)
    expectBox(end, boundsOf(cssBox({ transform: "translateX(100px) scale(2) rotate(90deg)" })), "end")
  }, TIMEOUT)

  it("tweens a percentage, a skew and an origin", () => {
    const half = run({ x: "100%", skewX: 20, originX: 1 }, linear, 500)
    expectBox(half, boundsOf(cssBox({ transform: "translateX(50%) skewX(10deg)", transformOrigin: "75% 50%" })), "half way")
  }, TIMEOUT)

  it("goes through keyframes", () => {
    const quarter = run({ x: [0, 100, 0] }, linear, 250)
    expectBox(quarter, boundsOf(cssBox({ transform: "translateX(50px)" })), "a quarter of the way")
    const back = run({ x: [0, 100, 0] }, linear, 1000)
    expectBox(back, boundsOf(cssBox({})), "back at the start")
  }, TIMEOUT)

  it("starts from initial", () => {
    const start = run({ x: 100 }, linear, 0, { x: -50, scale: 0.5 })
    expectBox(start, boundsOf(cssBox({ transform: "translateX(-50px) scale(0.5)" })), "initial")
  }, TIMEOUT)

  it("settles a spring on its target", () => {
    const end = run({ x: 120, rotate: 45, scaleX: 1.5 }, { type: "spring", stiffness: 300, damping: 30 }, 10_000)
    expectBox(end, boundsOf(cssBox({ transform: "translateX(120px) scaleX(1.5) rotate(45deg)" })), "settled")
  }, TIMEOUT)

  it("takes a transition per key and the animate value of an exit", () => {
    const half = run({ x: 100, rotate: 90 }, { x: linear, rotate: { duration: 2, ease: "linear" } }, 500)
    expectBox(half, boundsOf(cssBox({ transform: "translateX(50px) rotate(22.5deg)" })), "two curves")
  }, TIMEOUT)

  it("ignores the 3D keys in animate and its transition", () => {
    const half = run({ x: 100, z: 50, rotateX: 90 }, { x: linear, z: linear, rotateX: linear }, 500)
    expectBox(half, boundsOf(cssBox({ transform: "translateX(50px)" })), "3D keys")
  }, TIMEOUT)
})

describeNative("a motion value in the style of a motion component", () => {
  it("moves the element as the value changes", () => {
    // Each reference mounts a root of its own, which the root of this test cannot be open beside.
    const wanted = {
      rest: boundsOf(cssBox({})),
      steps: [1, 2, 3, 4].map((step) =>
        boundsOf(cssBox({ transform: `translateX(${step * 10}px) scale(${1 + step / 4}) rotate(${step * 15}deg)` }))
      ),
    }
    const { render, renderer, root } = createTestRoot(SIZE)
    try {
      renderer.clockPause()
      let x!: MotionValue<number>
      let rotate!: MotionValue<number>
      let scale!: MotionValue<number>
      function App() {
        x = useMotionValue(0)
        rotate = useMotionValue(0)
        scale = useMotionValue(1)
        return (
          <div style={{ display: "flex", padding: 200 }}>
            <motion.div testId="box" style={{ ...BOX, backgroundColor: "#336699", x, rotate, scale }} />
          </div>
        )
      }
      render(<App />)
      renderer.flush()
      expectBox(bounds(renderer, "box"), wanted.rest, "at rest")
      for (const step of [1, 2, 3, 4]) {
        x.set(step * 10)
        rotate.set(step * 15)
        scale.set(1 + step / 4)
        renderer.flush()
        expectBox(bounds(renderer, "box"), wanted.steps[step - 1]!, `step ${step}`)
      }
      animate(x, 200, linear)
      renderer.clockFastForward(500)
      renderer.flush()
      renderer.dispatchNativeEvents()
      expect(bounds(renderer, "box")[0] - 200).toBeGreaterThan(40)
    } finally {
      // Not the test root's unmount: it disposes the window the bound values release through.
      root.unmount()
      renderer.dispose()
    }
  }, TIMEOUT)
})


/** Mounts a tree that has motion values, runs `drive` on its renderer, and returns the box of `testId`. */
function boundsDriven(
  make: () => React.ReactNode,
  drive: (renderer) => void = () => {}
): [number, number, number, number] {
  const { render, renderer, root } = createTestRoot(SIZE)
  try {
    renderer.clockPause()
    render(<div style={{ display: "flex", padding: 200 }}>{make()}</div>)
    renderer.flush()
    drive(renderer)
    renderer.flush()
    renderer.dispatchNativeEvents()
    return bounds(renderer, "box")
  } finally {
    // Not the test root's unmount: it disposes the window the bound values release through.
    root.unmount()
    renderer.dispose()
  }
}

describeNative("an angle string in Motion", () => {
  const cases: [string, number | string, string][] = [
    ["degrees", "45deg", "45deg"],
    ["turns", "0.5turn", "180deg"],
    ["radians", "1rad", "1rad"],
    ["gradians", "10grad", "10grad"],
  ]
  for (const [unit, written, css] of cases) {
    it(`drives rotate from a motion value in ${unit} like the CSS angle`, () => {
      const got = boundsDriven(() => <Rotated value={written} />)
      expectBox(got, boundsOf(cssBox({ transform: `rotate(${css})` })), unit)
      expect(got, "the angle turned the box").not.toEqual(boundsOf(cssBox({})))
    }, TIMEOUT)
  }

  function Rotated({ value, key_ = "rotate" }: { value: number | string; key_?: string }) {
    const angle = useMotionValue(value)
    return <motion.div testId="box" style={{ ...BOX, backgroundColor: "#336699", [key_]: angle }} />
  }

  it("is the same as the number of degrees", () => {
    expectBox(boundsDriven(() => <Rotated value="45deg" />), boundsOf(motionBox({ rotate: 45 })), "45deg")
    expectBox(boundsDriven(() => <Rotated value="0.5turn" />), boundsOf(motionBox({ rotate: 180 })), "0.5turn")
    for (const key of ["rotateZ", "skewX", "skewY", "skew"]) {
      expectBox(boundsDriven(() => <Rotated value="0.1turn" key_={key} />), boundsOf(motionBox({ [key]: 36 })), key)
    }
  }, TIMEOUT)

  it("mixes with a number in degrees as a motion value animates", () => {
    let target!: MotionValue<number | string>
    function Tracked({ from }: { from: number | string }) {
      target = useMotionValue(from)
      return <motion.div testId="box" style={{ ...BOX, backgroundColor: "#336699", rotate: target }} />
    }
    const run = (from: number | string, to: number | string) =>
      boundsDriven(
        () => <Tracked from={from} />,
        (renderer) => {
          animate(target, to, linear)
          renderer.clockFastForward(500)
        }
      )
    expectBox(run(0, "1turn"), boundsOf(motionBox({ rotate: 180 })), "number to turn")
    expectBox(run("0.5turn", 360), boundsOf(motionBox({ rotate: 270 })), "turn to number")
    expectBox(run("90deg", "0.75turn"), boundsOf(motionBox({ rotate: 180 })), "angle to angle")
  }, TIMEOUT)

  it("is skipped by a key that is no angle", () => {
    expectBox(
      boundsDriven(() => <Rotated value="45deg" key_="x" />),
      boundsOf(motionBox({})),
      "45deg bound to x"
    )
  }, TIMEOUT)

  it("is a target of animate, and a tween passes half of it halfway", () => {
    // The reference mounts a root of its own, which the root under test cannot be open beside.
    const half = boundsOf(motionBox({ rotate: 180 }))
    const { render, renderer, unmount } = createTestRoot(SIZE)
    try {
      renderer.clockPause()
      render(
        <div style={{ display: "flex", padding: 200 }}>
          <motion.div testId="box" initial={{ rotate: 0 }} animate={{ rotate: "1turn" }} transition={linear} style={{ ...BOX, backgroundColor: "#336699" }} />
        </div>
      )
      renderer.flush()
      renderer.clockFastForward(500)
      renderer.flush()
      renderer.dispatchNativeEvents()
      expectBox(bounds(renderer, "box"), half, "half of a turn")
    } finally {
      unmount()
    }
  }, TIMEOUT)
})

type Rect = [number, number, number, number]
interface OracleCase {
  property: string
  frames: [number, string][]
  extra: string
  boxes: Record<string, Rect>
}

const ORACLE: Record<string, OracleCase> = JSON.parse(
  fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/transform-animation-oracle.json"), "utf8")
)

/** The oracle's `transform` scenes that run over the whole animation, which a Motion tween of the lists can play. */
const TRANSFORM_SCENES = Object.entries(ORACLE).filter(
  ([, scene]) => scene.property === "transform" && scene.frames[0]![0] === 0 && scene.frames[scene.frames.length - 1]![0] === 1
)

describeNative("a Motion transform string animates as a CSS transform does", () => {
  for (const [name, scene] of TRANSFORM_SCENES) {
    it(`shows Chrome's box of the same lists at every time: ${name}`, () => {
      const { render, renderer, unmount } = createTestRoot(SIZE)
      try {
        renderer.clockPause()
        const lists = scene.frames.map(([, list]) => list)
        const origin = /transform-origin:\s*([^;]+)/.exec(scene.extra)?.[1]
        render(
          <div style={{ position: "absolute", left: 0, top: 0, width: 800, height: 600 }}>
            <motion.div
              testId="box"
              initial={{ transform: lists[0] }}
              animate={{ transform: lists }}
              transition={{ duration: 1, ease: "linear", times: scene.frames.map(([offset]) => offset) }}
              style={{ position: "absolute", left: 200, top: 200, width: 100, height: 50, ...(origin ? { transformOrigin: origin } : {}) }}
            />
          </div>
        )
        renderer.flush()
        const wrong: string[] = []
        let elapsed = 0
        for (const [time, want] of Object.entries(scene.boxes).sort(([a], [b]) => +a - +b)) {
          renderer.clockFastForward(Math.round((+time - elapsed) * 1000))
          elapsed = +time
          renderer.flush()
          renderer.dispatchNativeEvents()
          const got = bounds(renderer, "box")
          if (got.some((value, index) => Math.abs(value - want[index]!) > 0.06)) {
            wrong.push(`t=${time}: got [${got.map((value) => +value.toFixed(3))}] Chrome [${want.map((value) => +value.toFixed(3))}]`)
          }
        }
        expect(wrong).toEqual([])
      } finally {
        unmount()
      }
    }, TIMEOUT)
  }
})

describeNative("Motion's transform is the list and its keys beside it are ignored", () => {
  const to = "translateX(100px) rotate(45deg)"

  function finishes(props: object, style: object = {}, ms = 1000) {
    const { render, renderer, unmount } = createTestRoot(SIZE)
    try {
      renderer.clockPause()
      render(
        <div style={{ display: "flex", padding: 200 }}>
          <motion.div testId="box" transition={linear} style={{ ...BOX, backgroundColor: "#336699", ...style }} {...props} />
        </div>
      )
      renderer.flush()
      renderer.clockFastForward(ms)
      renderer.flush()
      renderer.dispatchNativeEvents()
      return bounds(renderer, "box")
    } finally {
      unmount()
    }
  }

  it("draws an animate target of transform", () => {
    expectBox(finishes({ initial: { transform: "none" }, animate: { transform: to } }), boundsOf(cssBox({ transform: to })), "end")
    expectBox(
      finishes({ initial: { transform: "none" }, animate: { transform: to } }, {}, 500),
      boundsOf(cssBox({ transform: "translateX(50px) rotate(22.5deg)" })),
      "half way"
    )
  }, TIMEOUT)

  it("takes an initial, variants, keyframes and a transition per key", () => {
    expectBox(finishes({ initial: { transform: to }, animate: { transform: to } }, {}, 0), boundsOf(cssBox({ transform: to })), "initial")
    expectBox(
      finishes({ variants: { on: { transform: to } }, initial: false, animate: "on" }, {}, 0),
      boundsOf(cssBox({ transform: to })),
      "a variant"
    )
    expectBox(
      finishes({ initial: { transform: "none" }, animate: { transform: ["translateX(0px)", "translateX(100px)", "translateX(0px)"] } }, {}, 500),
      boundsOf(cssBox({ transform: "translateX(100px)" })),
      "keyframes"
    )
    expectBox(
      finishes({ initial: { transform: "none" }, animate: { transform: to }, transition: { transform: { duration: 2, ease: "linear" } } }, {}, 1000),
      boundsOf(cssBox({ transform: "translateX(50px) rotate(22.5deg)" })),
      "its own transition"
    )
  }, TIMEOUT)

  it("is drawn alone: the keys beside it, animated or static, do nothing", () => {
    const want = boundsOf(cssBox({ transform: to }))
    expectBox(finishes({ initial: { transform: "none", x: 0 }, animate: { transform: to, x: 300, scale: 2, rotate: 90 } }), want, "animated keys")
    expectBox(finishes({ animate: { transform: to } }, {}, 1000), want, "no initial")
    expectBox(finishes({ initial: { transform: to, x: 30, scale: 3 }, animate: { transform: to } }, {}, 0), want, "initial keys")
    expectBox(finishes({ initial: { transform: to }, whileHover: { x: 50 } }, { x: 10, rotate: 20 }, 0), want, "static keys")
  }, TIMEOUT)

  it("starts from what the keys or the style built, and `none` is a transform that turns the keys off", () => {
    expectBox(
      finishes({ animate: { transform: "translateX(100px)" } }, { x: 50 }, 500),
      boundsOf(cssBox({ transform: "translateX(75px)" })),
      "from the keys"
    )
    expectBox(
      finishes({ animate: { transform: "translateX(100px)" } }, { transform: "translateX(50px)" }, 500),
      boundsOf(cssBox({ transform: "translateX(75px)" })),
      "from the style"
    )
    expectBox(finishes({ initial: { transform: "none" }, animate: { transform: "none" } }, { x: 50 }, 0), boundsOf(cssBox({})), "none stands for the keys")
  }, TIMEOUT)

  it("leaves a static style.transform to the keys, as before", () => {
    expectBox(boundsOf(motionBox({ transform: "translateX(100px)", x: 5 })), boundsOf(cssBox({ transform: "translateX(5px)" })), "keys")
    expectBox(boundsOf(motionBox({ transform: "translateX(100px)" })), boundsOf(cssBox({ transform: "translateX(100px)" })), "alone")
  }, TIMEOUT)

  it("is a stacking context and a containing block as a list, even one that moves nothing, and not as none", () => {
    const hosts = (transform: string) => {
      const { render, renderer, unmount } = createTestRoot(SIZE)
      try {
        renderer.clockPause()
        render(
          <div style={{ position: "relative", width: 800, height: 600 }}>
            <motion.div testId="host" initial={{ transform }} animate={{ transform }} style={{ position: "absolute", left: 100, top: 100, width: 300, height: 300 }}>
              <div testId="child" style={{ position: "fixed", left: 10, top: 10, width: 20, height: 20 }} />
            </motion.div>
          </div>
        )
        renderer.flush()
        return bounds(renderer, "child")
      } finally {
        unmount()
      }
    }
    expectBox(hosts("translateX(0px)"), [110, 110, 20, 20], "an identity list holds its fixed child")
    expectBox(hosts("none"), [10, 10, 20, 20], "none does not")
  }, TIMEOUT)
})

describeNative("a motion value that holds a transform string", () => {
  function Moving({ initial, extra = {} }: { initial: string; extra?: object }) {
    const transform = useMotionValue(initial)
    handle = transform
    return <motion.div testId="box" style={{ ...BOX, backgroundColor: "#336699", transform, ...extra }} />
  }
  let handle!: MotionValue<string>

  it("drives the transform of the element, and a change moves it", () => {
    const list = "translateX(10px) rotate(45deg)"
    expectBox(boundsDriven(() => <Moving initial={list} />), boundsOf(cssBox({ transform: list })), "bound")
    expectBox(
      boundsDriven(
        () => <Moving initial={list} />,
        () => handle.set("translateX(40px) scale(2)")
      ),
      boundsOf(cssBox({ transform: "translateX(40px) scale(2)" })),
      "set"
    )
  }, TIMEOUT)

  it("animates between two lists as a CSS transition does, and ignores the keys beside it", () => {
    expectBox(
      boundsDriven(
        () => <Moving initial="translateX(10px) rotate(0deg)" extra={{ x: 70, scale: 3 }} />,
        (renderer) => {
          animate(handle, "translateX(110px) rotate(90deg)", linear)
          renderer.clockFastForward(500)
        }
      ),
      boundsOf(cssBox({ transform: "translateX(60px) rotate(45deg)" })),
      "half way"
    )
  }, TIMEOUT)
})

const WARM = 12
const TICKS = 20

/** The page: the head above 100 rows of 4 bordered cells, the shape of the pages the other patch counters run on. */
function Page({ head }: { head: React.ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", width: 700, height: 560, overflow: "hidden" }}>
      {/* Room to move in: what a transform takes out of the window is not painted, and a patch keeps what was. */}
      <div style={{ padding: 100 }}>{head}</div>
      {Array.from({ length: 100 }, (_, row) => (
        <div key={row} style={{ display: "flex", flexDirection: "row", gap: 4, padding: 2 }}>
          {Array.from({ length: 4 }, (_, cell) => (
            <div key={cell} style={{ borderWidth: 1, borderColor: "#cccccc", padding: 2 }}>
              <text style={{ fontSize: 11, color: "#333333" }}>{`r${row} c${cell}`}</text>
            </div>
          ))}
        </div>
      ))}
    </div>
  )
}

function gained(after: string, before: string): Record<string, number> {
  const was = JSON.parse(before) as Record<string, number>
  const out: Record<string, number> = {}
  for (const [reason, count] of Object.entries(JSON.parse(after) as Record<string, number>)) {
    if (count > (was[reason] ?? 0)) out[reason] = count - (was[reason] ?? 0)
  }
  return out
}

async function settle(renderer, frames = 12) {
  for (let frame = 0; frame < frames; frame++) {
    renderer.flush()
    renderer.dispatchNativeEvents()
    await sleep(8)
  }
}

/** What `TICKS` ticks did to the counters, after the warm-up that hands the page over to ticks. */
async function ticksOf(head: React.ReactNode, drive?: (renderer, tick: number) => void) {
  const root = createTestRoot({ width: 760, height: 620 })
  const renderer = root.renderer
  try {
    renderer.clockPause()
    root.render(<Page head={head} />)
    await settle(renderer)
    renderer.setInlineDraws(false)
    renderer.setA11yActive(false)
    const tick = (index: number) => {
      drive?.(renderer, index)
      renderer.clockAdvance(16)
      renderer.drawFrame()
    }
    // Reading a box lays the page out and the tick after it is drawn whole: the reads are before and after the ticks.
    const start = bounds(renderer, "head")
    for (let warm = 0; warm < WARM; warm++) tick(warm)
    const patches = renderer.getPatchCounters()
    const painted = renderer.getPaintedFrameCount()
    for (let index = 0; index < TICKS; index++) tick(WARM + index)
    const after = renderer.getPatchCounters()
    const end = bounds(renderer, "head")
    if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    return {
      patched: after.patchedFrames - patches.patchedFrames,
      full: renderer.getPaintedFrameCount() - painted,
      declines: gained(after.declines, patches.declines),
      unpatchable: gained(after.unpatchable, patches.unpatchable),
      moved: start.join() !== end.join(),
    }
  } finally {
    root.root.unmount()
    renderer.dispose()
  }
}

/** What the ticks did to the counters, which a verified frame (built twice) does not leave to be counted. */
function expectPatched(done: Awaited<ReturnType<typeof ticksOf>>) {
  expect(done.moved, "the element moved").toBe(true)
  if (!patching || verifying) return
  expect(done.patched, JSON.stringify(done)).toBeGreaterThanOrEqual(TICKS - 1)
  expect(done.full).toBeLessThanOrEqual(1)
  expect(done.declines).toEqual({})
  expect(done.unpatchable).toEqual({})
}

describeNative("a tick of an animation of the transform keys", { timeout: TIMEOUT }, () => {
  const heads = {
    "a tween of x": (
      <motion.div testId="head" style={{ ...BOX, backgroundColor: "#336699" }} initial={{ x: 0 }} animate={{ x: 400 }} transition={{ duration: 1.5, ease: "linear" }} />
    ),
    "a looping tween of x, rotate and scale": (
      <motion.div
        testId="head"
        style={{ ...BOX, backgroundColor: "#336699" }}
        animate={{ x: [0, 200, 0], rotate: [0, 90, 0], scale: [1, 1.5, 1] }}
        transition={{ duration: 1.6, ease: "linear", repeat: Infinity }}
      />
    ),
    "a spring of x, y and rotate": (
      <motion.div
        testId="head"
        style={{ ...BOX, backgroundColor: "#336699" }}
        initial={{ x: 0, y: 0, rotate: 0 }}
        animate={{ x: 300, y: 30, rotate: 40 }}
        transition={{ type: "spring", stiffness: 20, damping: 2 }}
      />
    ),
    "a looping tween of a percentage, a skew and an origin": (
      <motion.div
        testId="head"
        style={{ ...BOX, backgroundColor: "#336699" }}
        animate={{ x: ["0%", "100%", "0%"], skewX: [0, 20, 0], originX: [0, 1, 0] }}
        transition={{ duration: 1.6, ease: "linear", repeat: Infinity }}
      />
    ),
    "a tween of a transform string": (
      <motion.div
        testId="head"
        style={{ ...BOX, backgroundColor: "#336699" }}
        initial={{ transform: "translateX(0px) rotate(0deg)" }}
        animate={{ transform: "translateX(400px) rotate(90deg)" }}
        transition={{ duration: 1.5, ease: "linear" }}
      />
    ),
    "a looping tween of transform lists that do not pair up": (
      <motion.div
        testId="head"
        style={{ ...BOX, backgroundColor: "#336699" }}
        animate={{ transform: ["translateX(0px) rotate(30deg)", "scale(1.5) translateX(120px)", "translateX(0px) rotate(30deg)"] }}
        transition={{ duration: 1.6, ease: "linear", repeat: Infinity }}
      />
    ),
    "a spring of a transform string": (
      <motion.div
        testId="head"
        style={{ ...BOX, backgroundColor: "#336699" }}
        initial={{ transform: "translateX(0px)" }}
        animate={{ transform: "translateX(300px) rotate(40deg)" }}
        transition={{ type: "spring", stiffness: 20, damping: 2 }}
      />
    ),
    "a tween of a transformed element holding text": (
      <motion.div
        testId="head"
        style={{ width: 160, height: 24, backgroundColor: "#99ccff" }}
        animate={{ rotate: [0, 30, 0], x: [0, 100, 0] }}
        transition={{ duration: 1.6, ease: "linear", repeat: Infinity }}
      >
        <text style={{ fontSize: 14, color: "#222222" }}>moving</text>
      </motion.div>
    ),
  }
  for (const [name, head] of Object.entries(heads)) {
    it(`patches every tick, with no decline of either kind: ${name}`, async () => {
      expectPatched(await ticksOf(head))
    })
  }

  // The slot clips the word: what it drops from the scene is still an operation, so a word that rolls
  // into the slot, or out of it, is repainted with the operations it was recorded with.
  for (const from of ["80%", "130%"]) {
    it(`patches every tick of a word that rolls up into an overflow hidden slot from y: ${from}`, async () => {
      const word = (
        <div style={{ overflow: "hidden", width: 200, height: 24 }}>
          <motion.div
            testId="head"
            style={{ height: 24, backgroundColor: "#99ccff" }}
            initial={{ y: from }}
            animate={{ y: "0%" }}
            transition={{ duration: 1.5, ease: "linear" }}
          >
            <text style={{ fontSize: 14, color: "#222222" }}>rolling</text>
          </motion.div>
        </div>
      )
      expectPatched(await ticksOf(word))
    })
  }

  it("patches every tick of a motion value that is set each frame", async () => {
    let x!: MotionValue<number>
    let rotate!: MotionValue<number>
    function Head() {
      x = useMotionValue(0)
      rotate = useMotionValue(0)
      return <motion.div testId="head" style={{ ...BOX, backgroundColor: "#336699", x, rotate }} />
    }
    const done = await ticksOf(<Head />, (_renderer, tick) => {
      x.set(tick * 7)
      rotate.set(tick * 3)
    })
    expectPatched(done)
  })

  it("patches every tick of a motion value that animates a transform string", async () => {
    let transform!: MotionValue<string>
    function Head() {
      transform = useMotionValue("translateX(0px) rotate(0deg)")
      return <motion.div testId="head" style={{ ...BOX, backgroundColor: "#336699", transform }} />
    }
    const done = await ticksOf(<Head />, (_renderer, tick) => {
      if (tick === 0) animate(transform, "translateX(500px) rotate(90deg)", { duration: 1.8, ease: "linear" })
    })
    expectPatched(done)
  })

  it("patches every tick of a motion value that animates an angle string", async () => {
    let rotate!: MotionValue<number | string>
    function Head() {
      rotate = useMotionValue(0)
      return <motion.div testId="head" style={{ ...BOX, backgroundColor: "#336699", rotate }} />
    }
    const done = await ticksOf(<Head />, (_renderer, tick) => {
      if (tick === 0) animate(rotate, "1turn", { duration: 1.8, ease: "linear" })
    })
    expectPatched(done)
  })

  it("patches every tick of a motion value that animates", async () => {
    let x!: MotionValue<number>
    function Head() {
      x = useMotionValue(0)
      return <motion.div testId="head" style={{ ...BOX, backgroundColor: "#336699", x }} />
    }
    const done = await ticksOf(<Head />, (_renderer, tick) => {
      if (tick === 0) animate(x, 500, { duration: 1.8, ease: "linear" })
    })
    expectPatched(done)
  })
})
