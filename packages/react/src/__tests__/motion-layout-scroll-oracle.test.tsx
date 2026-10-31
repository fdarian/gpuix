/// A `layout` or `layoutId` glide whose commit also writes the `scrollTop` of a scroller goes through the frames Motion
/// 14.1.0 does in headless Chrome.
///
/// `fixtures/motion-layout-scroll-oracle.html` holds the scenes and `fixtures/motion-layout-scroll-oracle.json` what Chrome
/// reports for each (regenerate it with `motion-layout-oracle.py motion-layout-scroll-oracle`, see
/// `motion-layout-oracle.test.tsx`). Motion measures the box a render left (the snapshot) and the box the render's effects left
/// (the layout) around one commit, in the viewport: a scroll that an effect of the commit writes is in the second
/// measure, so the glide starts where the box was seen. A scroll that comes after the measure (a passive effect that React
/// runs after Motion's microtask) moves the glide with the content. gpuix measures at the first drawn frame of a commit, so
/// the scenes below write the scroll in an effect and draw once, or draw once and then write it.
///
/// Not replayed: `scrollDownLayoutScroll`, which sets Motion's `layoutScroll` on the scroller (gpuix has no such prop).

import fs from "fs"
import path from "path"
import { useEffect, useLayoutEffect } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"
import type { PublicInstance } from "../types/host.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const TOLERANCE = 1.5
const linear = { duration: 1, ease: "linear" } as const

interface Rect {
  x: number
  y: number
  width: number
  height: number
}

interface OracleScene {
  start: number
  before: Record<string, Rect | null>
  after: Record<string, Rect | null>
  frames: { t: number; boxes: Record<string, Rect | null> }[]
  scrolledAt: number[]
}

const ORACLE: { scenes: Record<string, OracleScene> } = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/motion-layout-scroll-oracle.json"),
    "utf8"
  )
)

type Scrollers = Record<string, PublicInstance>

const space = (height: number) => <div style={{ height, flexShrink: 0 }} />
const filler = <div style={{ height: 900, flexShrink: 0 }} />
const column = { display: "flex", flexDirection: "column" } as const

function scroller(scrollers: Scrollers, id: string, height: number, children: React.ReactNode) {
  return (
    <div
      testId={id}
      ref={(instance) => {
        if (instance) scrollers[id] = instance
      }}
      style={{ ...column, overflow: "scroll", height, width: 300, flexShrink: 0 }}
    >
      {children}
    </div>
  )
}

function box(testId: string, props: { layoutId?: string; height?: number } = {}) {
  return (
    <motion.div
      key={testId}
      testId={testId}
      layout={props.layoutId === undefined ? true : undefined}
      layoutId={props.layoutId}
      transition={linear}
      style={{ width: 100, height: props.height ?? 40, flexShrink: 0, backgroundColor: "#ff0000" }}
    />
  )
}

interface Scene {
  /** Where a scroller stands before, and where the commit's effect writes it. */
  before: Record<string, number>
  writes: Record<string, number>
  /** The effect that writes the scroll, or `none` for a scroll written after the first drawn frame of the commit. */
  when: "layout" | "passive" | "after-draw"
  render(state: "before" | "after", scrollers: Scrollers): React.ReactElement
}

const cardAndBubble = (state: "before" | "after", scrollers: Scrollers) => (
  <div style={column}>
    <div style={{ height: 60, flexShrink: 0 }}>{state === "after" ? null : box("card", { layoutId: "card" })}</div>
    {space(20)}
    {scroller(scrollers, "s", 200, [
      <div key="space" style={{ height: 100, flexShrink: 0 }} />,
      state === "after" ? box("card", { layoutId: "card" }) : null,
      <div key="filler" style={{ height: 900, flexShrink: 0 }} />,
    ])}
  </div>
)

const single = (before: number, after: number, scrollerHeight = 200) => (state: "before" | "after", scrollers: Scrollers) => (
  <div style={column}>
    {space(20)}
    {scroller(scrollers, "s", scrollerHeight, [
      <div key="space" style={{ height: state === "after" ? after : before, flexShrink: 0 }} />,
      box("box"),
      <div key="filler" style={{ height: 900, flexShrink: 0 }} />,
    ])}
  </div>
)

const nested = (state: "before" | "after", scrollers: Scrollers) => (
  <div style={column}>
    {space(10)}
    {scroller(scrollers, "outer", 300, [
      <div key="space" style={{ height: 15, flexShrink: 0 }} />,
      scroller(scrollers, "inner", 200, [
        <div key="space" style={{ height: state === "after" ? 60 : 0, flexShrink: 0 }} />,
        box("box"),
        <div key="filler" style={{ height: 900, flexShrink: 0 }} />,
      ]),
      <div key="tail" style={{ height: 900, flexShrink: 0 }} />,
    ])}
  </div>
)

const otherScroller = (state: "before" | "after", scrollers: Scrollers) => (
  <div style={{ display: "flex", flexDirection: "row" }}>
    {scroller(scrollers, "a", 200, [
      <div key="space" style={{ height: 60, flexShrink: 0 }} />,
      state === "after" ? null : box("card", { layoutId: "card" }),
      <div key="filler" style={{ height: 900, flexShrink: 0 }} />,
    ])}
    {scroller(scrollers, "b", 200, [
      <div key="space" style={{ height: 100, flexShrink: 0 }} />,
      state === "after" ? box("card", { layoutId: "card" }) : null,
      <div key="filler" style={{ height: 900, flexShrink: 0 }} />,
    ])}
  </div>
)

/** Markup of the oracle page's scenes, state for state. */
const SCENES: Record<string, Scene> = {
  scrollDownLayoutEffect: { before: { s: 0 }, writes: { s: 30 }, when: "layout", render: single(0, 60) },
  scrollDownPassiveSync: { before: { s: 0 }, writes: { s: 30 }, when: "passive", render: single(0, 60) },
  scrollDownPassiveDefault: { before: { s: 0 }, writes: { s: 30 }, when: "after-draw", render: single(0, 60) },
  scrollUpLayoutEffect: { before: { s: 50 }, writes: { s: 10 }, when: "layout", render: single(80, 140) },
  scrollUpPassiveDefault: { before: { s: 50 }, writes: { s: 10 }, when: "after-draw", render: single(80, 140) },
  nestedLayoutEffect: { before: { outer: 0, inner: 0 }, writes: { outer: 20, inner: 30 }, when: "layout", render: nested },
  nestedPassiveDefault: { before: { outer: 0, inner: 0 }, writes: { outer: 20, inner: 30 }, when: "after-draw", render: nested },
  layoutIdLayoutEffect: { before: { s: 0 }, writes: { s: 30 }, when: "layout", render: cardAndBubble },
  layoutIdPassiveSync: { before: { s: 0 }, writes: { s: 30 }, when: "passive", render: cardAndBubble },
  layoutIdPassiveDefault: { before: { s: 0 }, writes: { s: 30 }, when: "after-draw", render: cardAndBubble },
  layoutIdOtherScroller: { before: { a: 40, b: 0 }, writes: {}, when: "layout", render: otherScroller },
}

function write(scrollers: Scrollers, positions: Record<string, number>) {
  for (const id of Object.keys(positions)) scrollers[id]!.scrollTop = positions[id]!
}

function boundsOf(renderer: TestRenderer, testId: string): Rect | null {
  const element = renderer.findByTestId(testId)
  if (!element) return null
  return renderer.getElementBounds(element.id)
}

/** Renders the scene, calls `visit` at each recorded frame from the start of the animation on, with the clock at that
 *  frame's time and a frame drawn. */
function replay(name: string, visit: (renderer: TestRenderer, frame: OracleScene["frames"][number]) => void): void {
  const scene = SCENES[name]!
  const recorded = ORACLE.scenes[name]
  expect(recorded, `no oracle scene ${name}`).toBeDefined()
  const scrollers: Scrollers = {}
  function App(props: { state: "before" | "after" }) {
    useLayoutEffect(() => {
      if (props.state === "after" && scene.when === "layout") write(scrollers, scene.writes)
    }, [props.state])
    useEffect(() => {
      if (props.state === "after" && scene.when === "passive") write(scrollers, scene.writes)
    }, [props.state])
    return scene.render(props.state, scrollers)
  }
  const root = createTestRoot({ width: 700, height: 500 })
  const renderer = root.renderer
  renderer.clockPause()
  renderer.setInlineDraws(false)
  root.render(<App state="before" />)
  renderer.drawFrame()
  write(scrollers, scene.before)
  renderer.drawFrame()
  root.render(<App state="after" />)
  renderer.drawFrame()
  if (scene.when === "after-draw") {
    write(scrollers, scene.writes)
    renderer.drawFrame()
  }
  let now = 0
  // The first sample of the page is taken before Motion's projection is applied: the final layout.
  for (const frame of recorded!.frames.slice(1)) {
    if (frame.t < 0) continue
    renderer.clockFastForward(frame.t - now)
    now = frame.t
    renderer.drawFrame()
    visit(renderer, frame)
  }
  root.unmount()
}

function differences(renderer: TestRenderer, frame: OracleScene["frames"][number]): string[] {
  const found: string[] = []
  for (const id of Object.keys(frame.boxes)) {
    const chrome = frame.boxes[id]
    if (!chrome) continue
    const ours = boundsOf(renderer, id)
    if (!ours) {
      found.push(`t=${frame.t.toFixed(1)} ${id}: Chrome has a box, gpuix has none`)
      continue
    }
    for (const key of ["x", "y", "width", "height"] as const) {
      if (Math.abs(ours[key] - chrome[key]) > TOLERANCE) {
        found.push(`t=${frame.t.toFixed(1)} ${id}.${key}: gpuix ${ours[key].toFixed(2)}, Chrome ${chrome[key].toFixed(2)}`)
      }
    }
  }
  return found
}

describeNative("a scroll written in the commit of a glide, against Motion 14.1.0 in Chrome", () => {
  for (const name of Object.keys(SCENES)) {
    it(`${name}: every box is where Chrome has it, frame by frame`, () => {
      const found: string[] = []
      let compared = 0
      replay(name, (renderer, frame) => {
        compared += 1
        found.push(...differences(renderer, frame))
      })
      expect(compared, "frames replayed").toBeGreaterThan(30)
      expect(found).toEqual([])
    })
  }
})
