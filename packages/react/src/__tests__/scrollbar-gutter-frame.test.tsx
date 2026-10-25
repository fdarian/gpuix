/// An `overflow: auto` scroller on a window with classic scrollbars takes its gutter in the frame its content starts to
/// scroll, as a browser does: layout works out whether the bar is there, and builds again when it is (Blink's
/// relayout loop in `BlockNode::Layout`, which keeps a bar that appeared on for the rest of that layout). What one
/// `drawFrame` leaves is therefore compared with what headless Chrome gives after the same change: `clientWidth`, the
/// size of a child, and what a ResizeObserver and an IntersectionObserver with the scroller as root report.
///
/// `fixtures/scrollbar-gutter-oracle.html` is the page, `sh fixtures/scrollbar-gutter-oracle.sh` writes
/// `fixtures/scrollbar-gutter-oracle.json`.

import fs from "fs"
import path from "path"
import { memo, type ReactNode } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { IntersectionObserver, ResizeObserver } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

interface Box {
  rect: number[]
  client: number[]
  scroll: number[]
}
const ORACLE = JSON.parse(
  fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/scrollbar-gutter-oracle.json"), "utf8")
) as Record<string, Record<string, Box>>

const SCROLLER = { overflow: "auto", width: 200, height: 100 } as const

/** A minimum height for the target, which lays the scroller out again with nothing else changed. */
type Nudge = { minHeight: number } | {}

type Refs = (name: string) => { ref: (node: PublicInstance | null) => void }

interface Case {
  /** The page before the change and after it. */
  page: (after: boolean, ref: Refs, nudge: Nudge) => ReactNode
  /** The scroller the observers watch, and the child they observe. */
  scroller: string
  target: string
  /** The scrollers whose `clientWidth`, `clientHeight` and scroll size the oracle gives. */
  scrollers: string[]
}

const CASES: Record<string, Case> = {
  grow: {
    scroller: "s",
    target: "c",
    scrollers: ["s"],
    page: (after, ref, nudge) => (
      <div {...ref("s")} style={SCROLLER}>
        <div {...ref("c")} style={{ width: "100%", height: after ? 300 : 50, ...nudge }} />
      </div>
    ),
  },
  grow_wide: {
    scroller: "s",
    target: "c",
    scrollers: ["s"],
    page: (after, ref, nudge) => (
      <div {...ref("s")} style={SCROLLER}>
        <div {...ref("c")} style={{ width: after ? 300 : 100, height: 50, ...nudge }} />
      </div>
    ),
  },
  grow_both: {
    scroller: "s",
    target: "c",
    scrollers: ["s"],
    page: (after, ref, nudge) => (
      <div {...ref("s")} style={SCROLLER}>
        <div {...ref("c")} style={{ width: after ? 300 : 100, height: after ? 300 : 50, ...nudge }} />
      </div>
    ),
  },
  // Content that scrolls only without the bar: Chrome keeps the bar that appeared, and so must the passes of one frame.
  fits_only_with_bar: {
    scroller: "s",
    target: "c",
    scrollers: ["s"],
    page: (after, ref, nudge) => (
      <div {...ref("s")} style={SCROLLER}>
        <div {...ref("c")} style={{ ...(after ? { width: "100%", aspectRatio: "200 / 101" } : { width: "100%", height: 50 }), ...nudge }} />
      </div>
    ),
  },
  // The scroller is below the fold of a clip: it is culled, and has to work out its bar all the same.
  below_clip: {
    scroller: "s",
    target: "c",
    scrollers: ["s"],
    page: (after, ref, nudge) => (
      <div style={{ overflow: "hidden", width: 300, height: 100 }}>
        <div style={{ height: 140 }} />
        <div {...ref("s")} style={SCROLLER}>
          <div {...ref("c")} style={{ width: "100%", height: after ? 300 : 50, ...nudge }} />
        </div>
      </div>
    ),
  },
  nested: {
    scroller: "o",
    target: "c",
    scrollers: ["o", "s"],
    page: (after, ref, nudge) => (
      <div {...ref("o")} style={SCROLLER}>
        <div {...ref("s")} style={{ overflow: "auto", width: "100%", height: after ? 300 : 50 }}>
          <div {...ref("c")} style={{ width: "100%", height: after ? 500 : 50, ...nudge }} />
        </div>
      </div>
    ),
  },
}

interface Seen {
  boxes: Record<string, { rect: number[]; client: number[]; scroll: number[] }>
  ro: number[] | undefined
  io: { root: number[]; ratio: number } | undefined
}

/** Plays `change` once on a classic-scrollbar window and reads what the single `drawFrame` after it leaves. */
function play(name: string) {
  const spec = CASES[name]!
  const root = createTestRoot({ width: 600, height: 400 })
  const renderer = root.renderer
  try {
    renderer.setInlineDraws(false)
    renderer.setScrollbarStyle("classic")
    const refs = new Map<string, PublicInstance>()
    const ref: Refs = (key) => ({ ref: (node) => void (node ? refs.set(key, node) : refs.delete(key)) })
    let ro: number[] | undefined
    let io: Seen["io"]
    let target: PublicInstance | undefined
    const show = (after: boolean, nudge: Nudge = {}) => root.render(spec.page(after, ref, nudge))
    show(false)
    for (let frame = 0; frame < 3; frame++) renderer.drawFrame()
    target = refs.get(spec.target)!
    new ResizeObserver((entries) => {
      const entry = entries[entries.length - 1]!
      ro = [entry.contentBoxSize[0]!.inlineSize, entry.contentBoxSize[0]!.blockSize]
    }).observe(target)
    new IntersectionObserver(
      (entries) => {
        const entry = entries[entries.length - 1]!
        const bounds = entry.rootBounds!
        io = { root: [bounds.x, bounds.y, bounds.width, bounds.height], ratio: entry.intersectionRatio }
      },
      { root: refs.get(spec.scroller)!, threshold: [0, 0.25, 0.5, 0.75, 1] }
    ).observe(target)
    for (let frame = 0; frame < 3; frame++) {
      renderer.drawFrame()
      renderer.dispatchNativeEvents()
    }
    ro = undefined
    io = undefined
    const before = renderer.getBuildCounters().elementsBuilt
    show(true)
    renderer.drawFrame()
    renderer.dispatchNativeEvents()
    const built = renderer.getBuildCounters().elementsBuilt - before
    const read = (): Seen => {
      const boxes: Seen["boxes"] = {}
      for (const [key, node] of refs) {
        const bounds = renderer.getElementBounds(node.id)!
        const metrics = renderer.getScrollMetrics(node.id)
        boxes[key] = {
          rect: [bounds.x, bounds.y, bounds.width, bounds.height],
          client: metrics ? [metrics[4]!, metrics[5]!] : [],
          scroll: metrics ? [metrics[2]!, metrics[3]!] : [],
        }
      }
      return { boxes, ro, io }
    }
    const frame = read()
    const passes = renderer.getLayoutPassCount()
    // Whatever the first frame left has to be what the frames after it leave.
    for (let settle = 0; settle < 6; settle++) {
      renderer.drawFrame()
      renderer.dispatchNativeEvents()
    }
    const later = read()
    show(true, { minHeight: 1 })
    renderer.drawFrame()
    renderer.dispatchNativeEvents()
    return { frame, later, again: read(), built, passes: renderer.getLayoutPassCount() - passes, spec }
  } finally {
    root.unmount()
  }
}

function expectChrome(seen: Seen, name: string, spec: Case, key = name) {
  const oracle = ORACLE[key]!
  for (const key of Object.keys(oracle)) {
    const box = oracle[key]!
    const got = seen.boxes[key]!
    expect(got.rect[2], `${name} ${key} width`).toBeCloseTo(box.rect[2]!, 1)
    expect(got.rect[3], `${name} ${key} height`).toBeCloseTo(box.rect[3]!, 1)
    if (spec.scrollers.includes(key)) {
      expect(got.client, `${name} ${key} client`).toEqual(box.client)
      expect(got.scroll, `${name} ${key} scroll`).toEqual(box.scroll)
    }
  }
  const target = oracle[spec.target]!
  expect(seen.ro, `${name} resize observer`).toBeDefined()
  expect(seen.ro![0]!, `${name} resize observer width`).toBeCloseTo(target.client[0]!, 0)
  const scroller = oracle[spec.scroller]!
  if (seen.io) {
    expect(seen.io.root, `${name} intersection observer root`).toEqual([scroller.rect[0], scroller.rect[1], scroller.client[0], scroller.client[1]])
  }
}

describeNative("the gutter of an overflow auto scroller on a classic scrollbar window", () => {
  it("has a case for each of the oracle's", () => {
    expect(Object.keys(CASES).sort()).toEqual(Object.keys(ORACLE).filter((key) => !key.endsWith(" again")).sort())
  })

  for (const name of Object.keys(CASES)) {
    it(`is there in the frame the content starts to scroll, as in Chrome: ${name}`, () => {
      const played = play(name)
      expectChrome(played.frame, name, played.spec)
      expect(played.later.boxes, "the frames after it leave it as it is").toEqual(played.frame.boxes)
      expectChrome(played.again, name, played.spec, `${name} again`)
    })
  }

  it("delivers the root of an intersection observer with the gutter in the first frame", () => {
    const played = play("grow")
    expect(played.frame.io).toBeDefined()
    expect(played.frame.io!.root).toEqual([0, 0, 185, 100])
  })

  it("builds the frame twice for the gutter, and a frame that needs no gutter once", () => {
    /** Elements one `drawFrame` builds when the content under a scroller grows from 50 to `to`. */
    const built = (overflow: "auto" | "scroll", to: number) => {
      const root = createTestRoot({ width: 600, height: 400 })
      try {
        root.renderer.setInlineDraws(false)
        root.renderer.setScrollbarStyle("classic")
        const page = (height: number) => (
          <div style={{ overflow, width: 200, height: 100 }}>
            <div style={{ width: "100%", height }} />
          </div>
        )
        root.render(page(50))
        for (let frame = 0; frame < 3; frame++) root.renderer.drawFrame()
        const before = root.renderer.getBuildCounters().elementsBuilt
        root.render(page(to))
        root.renderer.drawFrame()
        return root.renderer.getBuildCounters().elementsBuilt - before
      } finally {
        root.unmount()
      }
    }
    const once = built("scroll", 300)
    expect(once).toBeGreaterThan(0)
    expect(built("auto", 300), "the flip frame").toBe(2 * once)
    expect(built("auto", 60), "content that still fits").toBe(once)
  })

  it("is there in the frame an ancestor's change makes an unchanged component's scroller scroll", () => {
    const Scroller = memo(() => (
      <div testId="s" style={{ overflow: "auto", width: 200, height: "100%" }}>
        <div style={{ width: "100%", height: 150 }} />
      </div>
    ))
    const page = (height: number) => (
      <div style={{ width: 200, height }}>
        <Scroller />
      </div>
    )
    const root = createTestRoot({ width: 600, height: 400 })
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      renderer.setScrollbarStyle("classic")
      root.render(page(200))
      for (let frame = 0; frame < 3; frame++) renderer.drawFrame()
      const id = renderer.findByTestId("s")!.id
      expect(renderer.getScrollMetrics(id)![4]).toBe(200)
      root.render(page(100))
      renderer.drawFrame()
      expect(renderer.getScrollMetrics(id)!.slice(2)).toEqual([185, 150, 185, 100])
    } finally {
      root.unmount()
    }
  })
})
