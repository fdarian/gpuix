/**
 * A forced read costs what changed (#83). A change to keys that only paint (a colour, an opacity, a
 * shadow) invalidates no layout, so a bounds read or a scroll write after it runs no pass and no
 * node is laid out; a change to one leaf of a large tree runs one pass that lays out the leaf and
 * the chain above it, not the tree; and a scroll write that leaves the scroller where it is does
 * nothing.
 *
 * Inline draws are off so that a dirty window stays dirty until `drawFrame`, which is what lets
 * `getLayoutPassCount` and `getLayoutCounters().nodesLaidOut` see the work of a read.
 */

import fs from "fs"
import { useState } from "react"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { flushSync } from "../reconciler/reconciler"
import type { TestRenderer } from "../testing"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rgba = [number, number, number, number]

type Tone = {
  background: string
  ink: string
  opacity: number
  edge: string
  shadow: string
}

const BEFORE: Tone = {
  background: "#ff0000",
  ink: "#00ff00",
  opacity: 1,
  edge: "#ff0000",
  shadow: "0 0 0 10px #ff0000",
}

const AFTER: Tone = {
  background: "#0000ff",
  ink: "#ffff00",
  opacity: 0.5,
  edge: "#0000ff",
  shadow: "0 0 0 10px #0000ff",
}

/** Draws until a frame has nothing left to draw: the first frames of a window end with an invalidation
 *  of their own (observers, the events they queue), which would otherwise be counted as the test's. */
function settle(renderer: TestRenderer) {
  renderer.flush()
  for (let frame = 0; frame < 6; frame++) {
    const painted = renderer.getPaintedFrameCount()
    renderer.getElementBounds(renderer.getRoot()!.id)
    renderer.dispatchNativeEvents()
    renderer.drawFrame()
    if (renderer.getPaintedFrameCount() === painted) return
  }
  throw new Error("the window keeps drawing")
}

const BOX = { width: 60, height: 30, flexShrink: 0 } as const

type Page = {
  setTone: (tone: Tone) => void
  setHeight: (height: number) => void
  scroller: PublicInstance | null
}

function Paints(props: { page: Page }) {
  const [tone, setTone] = useState(BEFORE)
  props.page.setTone = setTone
  return (
    <div testId="stage" style={{ width: 400, height: 400, flexDirection: "column", padding: 20 }}>
      <div testId="background" style={{ ...BOX, backgroundColor: tone.background }} />
      <div testId="ink" style={{ ...BOX, color: tone.ink, backgroundColor: "currentColor" }} />
      <div testId="fade" style={{ ...BOX, backgroundColor: "#0000ff", opacity: tone.opacity }} />
      <div testId="edge" style={{ ...BOX, borderWidth: 4, borderStyle: "solid", borderColor: tone.edge }} />
      <div testId="glow" style={{ ...BOX, margin: 12, boxShadow: tone.shadow }} />
      <div
        testId="scroller"
        ref={(instance) => {
          props.page.scroller = instance
        }}
        style={{ width: 200, height: 100, overflowY: "scroll", flexShrink: 0 }}
      >
        <div style={{ height: 1000 }} />
      </div>
    </div>
  )
}

function mountPaints() {
  const page: Page = { setTone: () => {}, setHeight: () => {}, scroller: null }
  const root = createTestRoot({ width: 480, height: 480 })
  const renderer = root.renderer
  renderer.clockPause()
  renderer.setInlineDraws(false)
  root.render(<Paints page={page} />)
  settle(renderer)
  const ids = Object.fromEntries(
    ["background", "ink", "fade", "edge", "glow", "scroller"].map((testId) => [testId, renderer.findByTestId(testId)!.id])
  )
  return { root, renderer, page, ids }
}

function work(renderer: TestRenderer) {
  return {
    passes: renderer.getLayoutPassCount(),
    laidOut: renderer.getLayoutCounters().nodesLaidOut,
    sized: renderer.getLayoutCounters().nodesSized,
    painted: renderer.getPaintedFrameCount(),
  }
}

/** The pixels of the last frame. */
function screenshot(renderer: TestRenderer, name: string) {
  const file = `${SHOTS_DIR}/forced-layout-incremental-${process.pid}-${name}.png`
  fs.rmSync(file, { force: true })
  renderer.captureScreenshot(file)
  return readLogicalPixels(file, renderer)
}

function probes(renderer: TestRenderer, ids: Record<string, number>) {
  const at = (testId: string) => renderer.getElementBounds(ids[testId]!)!
  const background = at("background")
  const ink = at("ink")
  const fade = at("fade")
  const edge = at("edge")
  const glow = at("glow")
  return {
    background: [background.x + 10, background.y + 10],
    ink: [ink.x + 10, ink.y + 10],
    fade: [fade.x + 10, fade.y + 10],
    edge: [edge.x + 1, edge.y + 15],
    glow: [glow.x - 5, glow.y + 15],
  } as const
}

type Reads = Record<keyof ReturnType<typeof probes>, Rgba>

function colours(renderer: TestRenderer, ids: Record<string, number>, name: string): Reads {
  const where = probes(renderer, ids)
  const pixels = screenshot(renderer, name)
  const read = (key: keyof typeof where) => pixels.at(where[key][0]!, where[key][1]!) as Rgba
  return {
    background: read("background"),
    ink: read("ink"),
    fade: read("fade"),
    edge: read("edge"),
    glow: read("glow"),
  }
}

const WRITES: Record<string, (renderer: TestRenderer, ids: Record<string, number>, page: Page) => void> = {
  "a scrollTop write": (_renderer, _ids, page) => {
    page.scroller!.scrollTop = 120
  },
  "scrollTo": (_renderer, _ids, page) => {
    page.scroller!.scrollTo({ top: 120, left: 0 })
  },
  "renderer.scrollTo": (renderer, ids) => {
    renderer.scrollTo(ids.scroller!, 0, 120, "instant")
  },
  "a getBoundingClientRect read": (_renderer, _ids, page) => {
    page.scroller!.getBoundingClientRect()
  },
}

describeNative("a forced read after a change that only paints", { timeout: 30_000 }, () => {
  for (const [name, write] of Object.entries(WRITES)) {
    it(`lays out nothing for ${name}, and the next frame paints the new colours`, () => {
      const { root, renderer, page, ids } = mountPaints()
      try {
        const before = colours(renderer, ids, "before")
        settle(renderer)
        expect(before.background).toEqual([255, 0, 0, 255])
        expect(before.ink).toEqual([0, 255, 0, 255])
        expect(before.fade).toEqual([0, 0, 255, 255])
        expect(before.edge).toEqual([255, 0, 0, 255])
        expect(before.glow).toEqual([255, 0, 0, 255])
        const start = work(renderer)

        flushSync(() => page.setTone(AFTER))
        write(renderer, ids, page)

        const read = work(renderer)
        expect(read.passes - start.passes, "no layout pass").toBe(0)
        expect(read.laidOut - start.laidOut, "no node laid out").toBe(0)
        expect(read.sized - start.sized, "no node sized").toBe(0)
        expect(read.painted - start.painted, "and nothing painted yet").toBe(0)

        renderer.drawFrame()
        expect(renderer.getPaintedFrameCount() - start.painted, "the frame draws once").toBe(1)
        const after = colours(renderer, ids, "after")
        expect(after.background).toEqual([0, 0, 255, 255])
        expect(after.ink).toEqual([255, 255, 0, 255])
        expect(after.edge).toEqual([0, 0, 255, 255])
        expect(after.glow).toEqual([0, 0, 255, 255])
        expect(after.fade, "half faded").not.toEqual(before.fade)
        expect(renderer.getLayoutCounters().nodesLaidOut - start.laidOut, "the frame lays out nothing either").toBe(0)
      } finally {
        renderer.setInlineDraws(true)
        root.unmount()
      }
    })
  }

  it("keeps the layout current across a run of paint-only commits", () => {
    const { root, renderer, page, ids } = mountPaints()
    try {
      const start = work(renderer)
      const tones: Tone[] = [AFTER, BEFORE, { ...AFTER, background: "#00ff00" }, { ...BEFORE, opacity: 0.25 }]
      for (const tone of tones) {
        flushSync(() => page.setTone(tone))
        renderer.getElementBounds(ids.scroller!)
        page.scroller!.scrollTop = 0
      }
      const read = work(renderer)
      expect(read.passes - start.passes).toBe(0)
      expect(read.laidOut - start.laidOut).toBe(0)
    } finally {
      renderer.setInlineDraws(true)
      root.unmount()
    }
  })
})

type Layout = {
  width: number
  padding: number
  display: "flex" | "none"
  fontSize: number
  borderWidth: number
}

const FLAT: Layout = { width: 80, padding: 0, display: "flex", fontSize: 14, borderWidth: 0 }

const LAYOUT_CHANGES: Record<string, Layout> = {
  width: { ...FLAT, width: 120 },
  padding: { ...FLAT, padding: 10 },
  display: { ...FLAT, display: "none" },
  fontSize: { ...FLAT, fontSize: 28 },
  borderWidth: { ...FLAT, borderWidth: 6 },
}

describeNative("a forced read after a change that moves boxes", { timeout: 30_000 }, () => {
  for (const [key, changed] of Object.entries(LAYOUT_CHANGES)) {
    it(`lays out once for ${key}`, () => {
      let setLayout: (layout: Layout) => void = () => {}
      function Moves() {
        const [layout, set] = useState(FLAT)
        setLayout = set
        return (
          <div testId="stage" style={{ width: 400, height: 300, flexDirection: "column" }}>
            <div
              testId="moves"
              style={{
                width: layout.width,
                padding: layout.padding,
                display: layout.display,
                fontSize: layout.fontSize,
                borderWidth: layout.borderWidth,
                borderStyle: "solid",
                borderColor: "#000000",
                flexShrink: 0,
              }}
            >
              <text style={{ fontFamily: "Menlo", lineHeight: 1.2 }}>moves</text>
            </div>
          </div>
        )
      }
      const root = createTestRoot({ width: 480, height: 320 })
      const renderer = root.renderer
      renderer.clockPause()
      renderer.setInlineDraws(false)
      try {
        root.render(<Moves />)
        settle(renderer)
        const moves = renderer.findByTestId("moves")!.id
        const before = renderer.getElementBounds(moves)!
        const start = work(renderer)

        flushSync(() => setLayout(changed))
        const after = renderer.getElementBounds(moves)!
        const read = work(renderer)

        expect(read.passes - start.passes, "one pass").toBe(1)
        expect(read.laidOut - start.laidOut, "that lays something out").toBeGreaterThan(0)
        expect(after, "and sees the change").not.toEqual(before)
        renderer.getElementBounds(moves)
        expect(work(renderer).passes - read.passes, "and the next read none").toBe(0)
      } finally {
        renderer.setInlineDraws(true)
        root.unmount()
      }
    })
  }
})

/** The probe box, its row, its entry, the list and the page: the elements from the changed box to the window's root. */
const CHAIN = 5

/** An entry of ten elements, nine `div` and one `text`: the shape of a row of a long list. The first
 *  entry's box is the only one that ever changes. */
function Entry(props: { probe: number | null }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", padding: 8, gap: 4, flexShrink: 0 }}>
      <div style={{ display: "flex", flexDirection: "row", alignItems: "center", gap: 8 }}>
        <div testId={props.probe === null ? undefined : "probe"} style={{ width: props.probe ?? 16, height: 16, backgroundColor: "#999999" }}>
          <div style={{ width: 8, height: 8, backgroundColor: "#ffffff" }} />
        </div>
        <div style={{ display: "flex", flexDirection: "column" }}>
          <div style={{ display: "flex", flexDirection: "row" }}>
            <div style={{ display: "flex", flexDirection: "column" }}>
              <div style={{ display: "flex" }}>
                <text style={{ fontSize: 14, fontFamily: "Menlo", color: "#222222" }}>Entry: a short line of text</text>
              </div>
            </div>
          </div>
        </div>
      </div>
      <div style={{ height: 1, backgroundColor: "#dddddd" }} />
    </div>
  )
}

function Long(props: { entries: number; probe: { set: (width: number) => void } }) {
  const [width, setWidth] = useState(16)
  props.probe.set = setWidth
  return (
    <div style={{ width: 900, height: 780, display: "flex", flexDirection: "column" }}>
      <div testId="list" style={{ overflowY: "scroll", flexGrow: 1, display: "flex", flexDirection: "column" }}>
        {Array.from({ length: props.entries }, (_, index) => (
          <Entry key={index} probe={index === 0 ? width : null} />
        ))}
      </div>
    </div>
  )
}

/** What one forced read costs after the probe box of the first entry changes width. */
function costOfOneLeaf(entries: number) {
  const probe = { set: (_width: number) => {} }
  const root = createTestRoot({ width: 900, height: 780 })
  const renderer = root.renderer
  renderer.clockPause()
  renderer.setInlineDraws(false)
  renderer.setA11yActive(false)
  try {
    root.render(<Long entries={entries} probe={probe} />)
    settle(renderer)
    const id = renderer.findByTestId("probe")!.id
    const start = work(renderer)

    flushSync(() => probe.set(40))
    expect(renderer.getElementBounds(id)!.width).toBe(40)
    const read = work(renderer)
    renderer.getElementBounds(id)
    expect(work(renderer).passes - read.passes, "a second read shares the pass").toBe(0)
    return {
      nodes: entries * 10,
      passes: read.passes - start.passes,
      laidOut: read.laidOut - start.laidOut,
      sized: read.sized - start.sized,
    }
  } finally {
    renderer.setInlineDraws(true)
    root.unmount()
  }
}

describeNative("a forced read after a change to one leaf of a long list", { timeout: 60_000 }, () => {
  it("lays out the chain above the leaf and what it moves, not the list", () => {
    const small = costOfOneLeaf(10)
    const large = costOfOneLeaf(800)
    expect(large.nodes).toBe(8000)
    expect(large.passes, "one pass").toBe(1)
    expect(large.laidOut, "as many nodes as for a list of 100 elements").toBe(small.laidOut)
    expect(large.sized, "and sized").toBe(small.sized)
    // The chain and the two nodes the window puts above the page (7 when this was written), never a
    // share of the list; each ancestor is sized a few times on the way (28).
    expect(large.laidOut).toBeGreaterThanOrEqual(CHAIN)
    expect(large.laidOut).toBeLessThanOrEqual(CHAIN + 4)
    expect(large.sized).toBeLessThanOrEqual(6 * CHAIN)
  })
})

describeNative("a scroll write that leaves the scroller where it is", { timeout: 30_000 }, () => {
  function Scrolls(props: { onScroll: () => void; page: Page }) {
    const [height, setHeight] = useState(1000)
    props.page.setHeight = setHeight
    return (
      <div testId="stage" style={{ width: 300, height: 300 }}>
        <div
          testId="scroller"
          ref={(instance) => {
            props.page.scroller = instance
          }}
          onScroll={props.onScroll}
          style={{ width: 200, height: 100, overflowY: "scroll", scrollbarWidth: "none" /* a shown scrollbar fades on the clock and keeps the window drawing */ }}
        >
          <div testId="content" style={{ height }} />
        </div>
      </div>
    )
  }

  function mount() {
    const page: Page = { setTone: () => {}, setHeight: () => {}, scroller: null }
    const events = { count: 0 }
    const root = createTestRoot({ width: 320, height: 320 })
    const renderer = root.renderer
    renderer.clockPause()
    renderer.setInlineDraws(false)
    root.render(
      <Scrolls
        page={page}
        onScroll={() => {
          events.count++
        }}
      />
    )
    settle(renderer)
    return { root, renderer, page, events }
  }

  function frame(renderer: TestRenderer, ms = 1000 / 60) {
    renderer.clockFastForward(ms)
    renderer.dispatchNativeEvents()
    renderer.drawFrame()
    renderer.dispatchNativeEvents()
  }

  it("draws nothing, reports nothing and lays out nothing on a clean window", () => {
    const { root, renderer, page, events } = mount()
    try {
      page.scroller!.scrollTop = 150
      renderer.drawFrame()
      renderer.dispatchNativeEvents()
      settle(renderer)
      expect(page.scroller!.scrollTop).toBe(150)
      expect(events.count, "the first write reports").toBe(1)
      const start = work(renderer)

      page.scroller!.scrollTop = 150
      page.scroller!.scrollTo({ top: 150, left: 0 })
      renderer.scrollTo(renderer.findByTestId("scroller")!.id, 0, 150, "instant")
      renderer.drawFrame()
      renderer.dispatchNativeEvents()

      const end = work(renderer)
      expect(end.painted - start.painted, "no frame drawn for it").toBe(0)
      expect(end.passes - start.passes).toBe(0)
      expect(end.laidOut - start.laidOut).toBe(0)
      expect(events.count, "no scroll event").toBe(1)
    } finally {
      renderer.setInlineDraws(true)
      root.unmount()
    }
  })

  it("draws nothing for a write past the end when the scroller is at the end", () => {
    const { root, renderer, page, events } = mount()
    try {
      page.scroller!.scrollTop = 1e6
      renderer.drawFrame()
      renderer.dispatchNativeEvents()
      settle(renderer)
      const max = page.scroller!.scrollTop
      expect(max).toBe(900)
      const start = work(renderer)
      const reported = events.count

      page.scroller!.scrollTop = 5000
      page.scroller!.scrollTop = max + 1
      renderer.drawFrame()
      renderer.dispatchNativeEvents()

      expect(work(renderer).painted - start.painted).toBe(0)
      expect(events.count).toBe(reported)
    } finally {
      renderer.setInlineDraws(true)
      root.unmount()
    }
  })

  it("lays out only what it needs when the tree is dirty, and invalidates nothing more", () => {
    const { root, renderer, page, events } = mount()
    try {
      page.scroller!.scrollTop = 150
      renderer.drawFrame()
      renderer.dispatchNativeEvents()
      settle(renderer)
      const reported = events.count
      const start = work(renderer)

      flushSync(() => page.setHeight(1200))
      page.scroller!.scrollTop = 150
      const read = work(renderer)
      expect(read.passes - start.passes, "the layout the write needs").toBe(1)

      renderer.drawFrame()
      renderer.dispatchNativeEvents()
      expect(work(renderer).painted - start.painted, "the change draws once").toBe(1)
      expect(events.count, "and the write reports nothing").toBe(reported)
      expect(page.scroller!.scrollTop).toBe(150)
    } finally {
      renderer.setInlineDraws(true)
      root.unmount()
    }
  })

  it("cancels a smooth scroll in flight, as a browser does", () => {
    const { root, renderer, page } = mount()
    try {
      page.scroller!.scrollTo({ top: 600, behavior: "smooth" })
      for (let index = 0; index < 8; index++) frame(renderer)
      const position = page.scroller!.scrollTop
      expect(position, "the animation is under way").toBeGreaterThan(0)
      expect(position).toBeLessThan(600)

      page.scroller!.scrollTop = position
      for (let index = 0; index < 60; index++) frame(renderer)
      expect(page.scroller!.scrollTop, "and the write stopped it").toBe(position)
    } finally {
      renderer.setInlineDraws(true)
      root.unmount()
    }
  })

  it("starts no animation for a smooth scroll to where the scroller is", () => {
    const { root, renderer, page, events } = mount()
    try {
      page.scroller!.scrollTo({ top: 0, behavior: "smooth" })
      const start = work(renderer)
      renderer.drawFrame()
      renderer.dispatchNativeEvents()
      expect(work(renderer).painted - start.painted).toBe(0)
      expect(events.count).toBe(0)
    } finally {
      renderer.setInlineDraws(true)
      root.unmount()
    }
  })
})
