/// The Solid half of `motion-layout-scroll-oracle.test.tsx` of the React package: a `layout` or `layoutId` glide whose
/// commit also writes the `scrollTop` of a scroller in a `createEffect` (FF's transcript pins to its end that way) goes
/// through the frames Motion 14.1.0 does in headless Chrome
/// (`packages/react/src/__tests__/fixtures/motion-layout-scroll-oracle.json`). A scroll the effect writes is in the
/// measure, so the glide starts where the box was seen; one written after the first drawn frame moves it with the content.

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import { createEffect, createSignal, type JSX } from "solid-js"
import { describe, expect, it } from "bun:test"
import type { HostElement } from "../index.js"
import { motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

interface Rect {
  x: number
  y: number
  width: number
  height: number
}

interface OracleScene {
  frames: { t: number; boxes: Record<string, Rect | null> }[]
}

const ORACLE: { scenes: Record<string, OracleScene> } = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../react/src/__tests__/fixtures/motion-layout-scroll-oracle.json"),
    "utf8"
  )
)

const TOLERANCE = 1.5
const linear = { duration: 1, ease: "linear" } as const
const column = { display: "flex", flexDirection: "column" } as const

type Scrollers = Record<string, HostElement>

function scroller(scrollers: Scrollers, id: string, height: number, children: JSX.Element) {
  return (
    <div
      testId={id}
      ref={(element) => {
        scrollers[id] = element
      }}
      style={{ ...column, overflow: "scroll", height, width: 300, flexShrink: 0 }}
    >
      {children}
    </div>
  )
}

const box = (testId: string, layoutId?: string) => (
  <motion.div
    testId={testId}
    layout={layoutId === undefined ? true : undefined}
    layoutId={layoutId}
    transition={linear}
    style={{ width: 100, height: 40, flexShrink: 0, backgroundColor: "#ff0000" }}
  />
)

interface Scene {
  before: Record<string, number>
  writes: Record<string, number>
  /** `effect`: `createEffect` of the commit writes the scroll. `after-draw`: it is written after the first drawn frame. */
  when: "effect" | "after-draw"
  render(after: () => boolean, scrollers: Scrollers): JSX.Element
}

const single = (before: number, after: number) => (isAfter: () => boolean, scrollers: Scrollers) => (
  <div style={column}>
    <div style={{ height: 20, flexShrink: 0 }} />
    {scroller(scrollers, "s", 200,
      <>
        <div style={{ height: isAfter() ? after : before, flexShrink: 0 }} />
        {box("box")}
        <div style={{ height: 900, flexShrink: 0 }} />
      </>
    )}
  </div>
)

const nested = (isAfter: () => boolean, scrollers: Scrollers) => (
  <div style={column}>
    <div style={{ height: 10, flexShrink: 0 }} />
    {scroller(scrollers, "outer", 300,
      <>
        <div style={{ height: 15, flexShrink: 0 }} />
        {scroller(scrollers, "inner", 200,
          <>
            <div style={{ height: isAfter() ? 60 : 0, flexShrink: 0 }} />
            {box("box")}
            <div style={{ height: 900, flexShrink: 0 }} />
          </>
        )}
        <div style={{ height: 900, flexShrink: 0 }} />
      </>
    )}
  </div>
)

const cardAndBubble = (isAfter: () => boolean, scrollers: Scrollers) => (
  <div style={column}>
    <div style={{ height: 60, flexShrink: 0 }}>{isAfter() ? null : box("card", "card")}</div>
    <div style={{ height: 20, flexShrink: 0 }} />
    {scroller(scrollers, "s", 200,
      <>
        <div style={{ height: 100, flexShrink: 0 }} />
        {isAfter() ? box("card", "card") : null}
        <div style={{ height: 900, flexShrink: 0 }} />
      </>
    )}
  </div>
)

const SCENES: Record<string, Scene> = {
  scrollDownLayoutEffect: { before: { s: 0 }, writes: { s: 30 }, when: "effect", render: single(0, 60) },
  scrollDownPassiveDefault: { before: { s: 0 }, writes: { s: 30 }, when: "after-draw", render: single(0, 60) },
  scrollUpLayoutEffect: { before: { s: 50 }, writes: { s: 10 }, when: "effect", render: single(80, 140) },
  scrollUpPassiveDefault: { before: { s: 50 }, writes: { s: 10 }, when: "after-draw", render: single(80, 140) },
  nestedLayoutEffect: { before: { outer: 0, inner: 0 }, writes: { outer: 20, inner: 30 }, when: "effect", render: nested },
  layoutIdLayoutEffect: { before: { s: 0 }, writes: { s: 30 }, when: "effect", render: cardAndBubble },
  layoutIdPassiveDefault: { before: { s: 0 }, writes: { s: 30 }, when: "after-draw", render: cardAndBubble },
}

function write(scrollers: Scrollers, positions: Record<string, number>) {
  for (const id of Object.keys(positions)) scrollers[id]!.scrollTo({ top: positions[id]! })
}

function boundsOf(renderer: TestRenderer, testId: string): Rect | null {
  const element = renderer.findByTestId(testId)
  return element ? renderer.getElementBounds(element.id) : null
}

describe.skipIf(!hasNativeTestRenderer)("Solid: a scroll written in the commit of a glide, against Motion 14.1.0 in Chrome", () => {
  for (const name of Object.keys(SCENES)) {
    it(`${name}: every box is where Chrome has it, frame by frame`, () => {
      const scene = SCENES[name]!
      const recorded = ORACLE.scenes[name]!
      const scrollers: Scrollers = {}
      const [after, setAfter] = createSignal(false)
      const app = createTestRoot({ width: 700, height: 500 })
      const renderer = app.renderer
      renderer.clockPause()
      renderer.setInlineDraws(false)
      app.render(() => {
        createEffect(() => {
          if (after() && scene.when === "effect") write(scrollers, scene.writes)
        })
        return scene.render(after, scrollers)
      })
      renderer.drawFrame()
      write(scrollers, scene.before)
      renderer.drawFrame()
      app.flushSync(() => setAfter(true))
      renderer.drawFrame()
      if (scene.when === "after-draw") {
        write(scrollers, scene.writes)
        renderer.drawFrame()
      }
      const found: string[] = []
      let compared = 0
      let now = 0
      for (const frame of recorded.frames.slice(1)) {
        if (frame.t < 0) continue
        renderer.clockFastForward(frame.t - now)
        now = frame.t
        renderer.drawFrame()
        compared += 1
        for (const id of Object.keys(frame.boxes)) {
          const chrome = frame.boxes[id]
          const ours = boundsOf(renderer, id)
          if (!chrome || !ours) continue
          for (const key of ["x", "y", "width", "height"] as const) {
            if (Math.abs(ours[key] - chrome[key]) > TOLERANCE) {
              found.push(`t=${frame.t.toFixed(1)} ${id}.${key}: gpuix ${ours[key].toFixed(2)}, Chrome ${chrome[key].toFixed(2)}`)
            }
          }
        }
      }
      expect(compared).toBeGreaterThan(30)
      expect(found).toEqual([])
      app.unmount()
    })
  }
})
