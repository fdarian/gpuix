/// Within one style object, of a shorthand and a longhand it covers the one written later wins, as in
/// CSS: `{ paddingLeft: 4, padding: 8 }` pads the left by 8 and `{ padding: 8, paddingLeft: 4 }` by 4.
/// It holds for a static style, for each state style, for the keys of a `UniversalStyle` layer over the
/// layers before it, and for the maps of `motion`.
///
/// `fixtures/shorthand-order-oracle.html` is the page and `fixtures/shorthand-order-oracle.json` the boxes
/// headless Chrome lays out for it (regenerate with the command in the HTML). Each case there holds the
/// style objects in written order and the `[x, y, width, height]` of the children and the host.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { motion, UniversalStyle } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Style = Record<string, unknown>
type Rect = [number, number, number, number]

interface Case {
  host: Style
  child: Style
  child2: Style
  rects: { child: Rect; child2: Rect; host: Rect }
}

interface UniversalCase {
  layers: Style[]
  childInBox: [number, number]
}

const ORACLE: { cases: Record<string, Case>; universal: Record<string, UniversalCase> } = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/shorthand-order-oracle.json"),
    "utf8"
  )
)

const NAMES = Object.keys(ORACLE.cases)

/** Keys that shape the box rather than take part in the order, so a state or motion style leaves them. */
const STRUCTURAL = new Set(["display", "position", "gridTemplateColumns", "width", "height", "flexWrap", "alignContent"])

/** What `motion` animates: a longhand per side, and the shorthands it splits over them. */
const ANIMATED = new Set([
  "padding",
  "paddingLeft",
  "margin",
  "marginLeft",
  "gap",
  "rowGap",
  "columnGap",
  "inset",
  "left",
  "borderWidth",
  "borderLeftWidth",
])

const HOST_BASE = { display: "flex", width: 120, height: 90 }

type Renderer = ReturnType<typeof createTestRoot>["renderer"]

function rectOf(renderer: Renderer, testId: string): Rect {
  renderer.dispatchNativeEvents()
  const bounds = renderer.getElementBounds(renderer.findByTestId(testId).id)
  const scene = renderer.getElementBounds(renderer.findByTestId("scene").id)
  return [bounds.x - scene.x, bounds.y - scene.y, bounds.width, bounds.height]
}

function split(style: Style, keep: (key: string) => boolean): [Style, Style] {
  const kept: Style = {}
  const rest: Style = {}
  for (const key of Object.keys(style)) (keep(key) ? kept : rest)[key] = style[key]
  return [kept, rest]
}

function scene(children: React.ReactNode) {
  return (
    <div testId="scene" style={{ width: 300, height: 200, fontSize: 16 }}>
      {children}
    </div>
  )
}

function expectRects(renderer: Renderer, name: string) {
  const oracle = ORACLE.cases[name]!.rects
  expect(
    { child: rectOf(renderer, "child"), child2: rectOf(renderer, "child2"), host: rectOf(renderer, "host") },
    name
  ).toEqual(oracle)
}

const KID = { width: 20, height: 10 }

function laidOut(ui: React.ReactElement, check: (renderer: Renderer) => void) {
  const { render, renderer, unmount } = createTestRoot({ width: 400, height: 300 })
  try {
    renderer.clockPause()
    render(ui)
    renderer.flush()
    check(renderer)
  } finally {
    unmount()
  }
}

describeNative("a shorthand and a longhand it covers in one style object", () => {
  it("has the cases the oracle page lists", () => {
    expect(NAMES.length).toBeGreaterThan(40)
  })

  it.each(NAMES)("lays out %s as Chrome does", (name) => {
    const found = ORACLE.cases[name]!
    laidOut(
      scene(
        <div testId="host" style={{ ...HOST_BASE, ...found.host }}>
          <div testId="child" style={{ ...KID, ...found.child }} />
          <div testId="child2" style={{ ...KID, ...found.child2 }} />
        </div>
      ),
      (renderer) => expectRects(renderer, name)
    )
  })

  it.each(NAMES)("lays out %s in a hover state as Chrome does", (name) => {
    const found = ORACLE.cases[name]!
    const [structural, hover] = split(found.host, (key) => STRUCTURAL.has(key))
    laidOut(
      scene(
        <div testId="host" style={{ ...HOST_BASE, ...structural, hover }}>
          <div testId="child" style={{ ...KID, ...found.child }} />
          <div testId="child2" style={{ ...KID, ...found.child2 }} />
        </div>
      ),
      (renderer) => {
        const host = found.rects.host
        const origin = renderer.getElementBounds(renderer.findByTestId("scene").id)
        renderer.nativeSimulateMouseMove(origin.x + host[0] + host[2] - 1, origin.y + host[1] + host[3] - 1)
        renderer.clockFastForward(16)
        expectRects(renderer, name)
      }
    )
  })

  const animated = NAMES.filter((name) => {
    const found = ORACLE.cases[name]!
    const keys = [...Object.keys(found.host), ...Object.keys(found.child), ...Object.keys(found.child2)]
    return keys.every((key) => STRUCTURAL.has(key) || ANIMATED.has(key) || key === "borderStyle")
  })

  it("has motion cases", () => {
    expect(animated.length).toBeGreaterThan(10)
  })

  it.each(animated)("lays out %s in a motion animate map as Chrome does", (name) => {
    const found = ORACLE.cases[name]!
    const keep = (key: string) => STRUCTURAL.has(key) || key === "borderStyle"
    const [hostStyle, hostAnimate] = split(found.host, keep)
    const [childStyle, childAnimate] = split(found.child, keep)
    const [child2Style, child2Animate] = split(found.child2, keep)
    laidOut(
      scene(
        <motion.div testId="host" style={{ ...HOST_BASE, ...hostStyle }} initial={false} animate={hostAnimate}>
          <motion.div testId="child" style={{ ...KID, ...childStyle }} initial={false} animate={childAnimate} />
          <motion.div testId="child2" style={{ ...KID, ...child2Style }} initial={false} animate={child2Animate} />
        </motion.div>
      ),
      (renderer) => expectRects(renderer, name)
    )
  })

  it.each(animated)("lays out %s in a motion initial map as Chrome does", (name) => {
    const found = ORACLE.cases[name]!
    const keep = (key: string) => STRUCTURAL.has(key) || key === "borderStyle"
    const [hostStyle, hostInitial] = split(found.host, keep)
    const [childStyle, childInitial] = split(found.child, keep)
    const [child2Style, child2Initial] = split(found.child2, keep)
    const to = { opacity: 1 }
    laidOut(
      scene(
        <motion.div testId="host" style={{ ...HOST_BASE, ...hostStyle }} initial={hostInitial} animate={to}>
          <motion.div testId="child" style={{ ...KID, ...childStyle }} initial={childInitial} animate={to} />
          <motion.div testId="child2" style={{ ...KID, ...child2Style }} initial={child2Initial} animate={to} />
        </motion.div>
      ),
      (renderer) => expectRects(renderer, name)
    )
  })

  it("applies a style rewritten with its keys in the other order", () => {
    const { render, renderer, unmount } = createTestRoot({ width: 400, height: 300 })
    const tree = (style: Style) =>
      scene(
        <div testId="host" style={{ ...HOST_BASE, ...style }}>
          <div testId="child" style={KID} />
        </div>
      )
    render(tree({ padding: 8, paddingLeft: 4 }))
    expect(rectOf(renderer, "child")[0]).toBe(4)
    render(tree({ paddingLeft: 4, padding: 8 }))
    expect(rectOf(renderer, "child")[0]).toBe(8)
    unmount()
  })

  it("lets the written order pick between the transitions of a shorthand and its longhand", () => {
    const run = (transition: Style) => {
      let left = 0
      laidOut(
        scene(
          <motion.div
            testId="host"
            style={{ ...HOST_BASE }}
            initial={{ padding: 0 }}
            animate={{ padding: 100 }}
            transition={transition}
          >
            <div testId="child" style={KID} />
          </motion.div>
        ),
        (renderer) => {
          renderer.clockFastForward(1000)
          left = rectOf(renderer, "child")[0]
        }
      )
      return left
    }
    const slow = { duration: 10, ease: "linear" }
    const fast = { duration: 1, ease: "linear" }
    expect(run({ padding: fast, paddingLeft: slow })).toBeCloseTo(10, 0)
    expect(run({ paddingLeft: slow, padding: fast })).toBeCloseTo(100, 0)
  })
})

describeNative("a UniversalStyle layer over the layers before it", () => {
  const UNIVERSAL = Object.keys(ORACLE.universal)

  it.each(UNIVERSAL)("pads %s as Chrome does", (name) => {
    const found = ORACLE.universal[name]!
    laidOut(
      scene(
        <>
          {found.layers.map((layer, index) => (
            <UniversalStyle key={index} style={layer} />
          ))}
          <div testId="box" style={{ width: 60, height: 40 }}>
            <div testId="child" style={KID} />
          </div>
        </>
      ),
      (renderer) => {
        const box = rectOf(renderer, "box")
        const child = rectOf(renderer, "child")
        expect([child[0] - box[0], child[1] - box[1]]).toEqual(found.childInBox)
      }
    )
  })

  it("keeps the order when a layer changes", () => {
    const { render, renderer, unmount } = createTestRoot({ width: 400, height: 300 })
    const tree = (first: Style, second: Style) =>
      scene(
        <>
          <UniversalStyle style={first} />
          <UniversalStyle style={second} />
          <div testId="box" style={{ width: 60, height: 40 }}>
            <div testId="child" style={KID} />
          </div>
        </>
      )
    const offset = () => {
      const box = rectOf(renderer, "box")
      const child = rectOf(renderer, "child")
      return [child[0] - box[0], child[1] - box[1]]
    }
    render(tree({ padding: 8 }, { paddingLeft: 4 }))
    expect(offset()).toEqual([4, 8])
    render(tree({ paddingLeft: 4 }, { padding: 8 }))
    expect(offset()).toEqual([8, 8])
    render(tree({ paddingLeft: 4 }, { padding: 8, paddingLeft: 2 }))
    expect(offset()).toEqual([2, 8])
    unmount()
  })
})
