/// `contentVisibility: "auto"`: an element skips its content while it is not relevant to the user, which is while it is
/// not near the viewport (its border box, outset by 150% of the viewport on each side, cut by what clips it and by the
/// viewport, still shows something) and does not hold the focus. Against headless Chrome for which elements are skipped,
/// the tells (`contentvisibilityautostatechange`) and the size of the scroller, and then for what a skipped element costs.
///
/// `fixtures/content-visibility-auto-oracle.html` is the page, `fixtures/content-visibility-auto-oracle.ts` drives it
/// and writes `fixtures/content-visibility-auto-oracle.json` (regenerate with `bun fixtures/content-visibility-auto-oracle.ts`).

import fs from "fs"
import path from "path"
import type { CSSProperties, ReactNode } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { animate, defineKeyframes, motion, motionValue } from "../index"
import { createTestRoot, hasNativeTestRenderer, type TestRenderer } from "../testing.js"
import { verifying } from "./retained-harness.js"
import { defineAnimationEventTests } from "./skipped-animation-events-fixture.js"
import { AnimatedCard, CARD, elapse, Filler, hovered, shotOf, Sidebar, spent, VisibleLoop } from "./skipped-animations-fixture.js"

const SQUARE_FONT = fs.readFileSync(new URL("./fixtures/gpuix-test-square.ttf", import.meta.url))

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")
interface OracleStep {
  step: string
  skipped?: number[]
  /** `[x, y, width, height]` of the elements a scene reads the geometry of. */
  rects?: number[][]
  /** The elements a scene reads, by `testId`, for the scenes whose reads change from step to step. */
  names?: string[]
  /** For each section, how many of its rows are built. */
  rows?: number[]
  sizes?: number[]
  focused?: number
  /** `[scrollLeft, scrollTop, scrollWidth, scrollHeight, clientWidth, clientHeight]` of the scrollers a scene reads. */
  metrics?: number[][]
  height: number
  top: number
  events: [number, boolean][]
}
/** The first frame of a page: the events in the order Chrome tells them, with the animation frame callbacks between. */
interface OracleLoad {
  log: string[]
  events: [number, boolean][]
}
const { load: LOAD, ...SCENES_OF_ORACLE } = JSON.parse(
  fs.readFileSync(path.join(FIXTURES, "content-visibility-auto-oracle.json"), "utf8")
) as Record<string, OracleStep[]> & { load: OracleLoad }
/** The scenes `firstRead...` are read right after a change, not settled: the test of the first read has them. */
const isFirstRead = (name: string) => name.startsWith("firstRead")
const ORACLE = Object.fromEntries(Object.entries(SCENES_OF_ORACLE).filter(([name]) => !isFirstRead(name)))
const FIRST_READS = Object.fromEntries(Object.entries(SCENES_OF_ORACLE).filter(([name]) => isFirstRead(name)))

const ENTRIES = 190
const SIZE = { width: 900, height: 780 }
const UNIT = 1 / 64

type Events = [number, boolean][]

/** An entry of 37px: padding 8, a bar of 16, a gap of 4, a rule of 1. */
function Entry({
  index,
  intrinsic,
  events,
  focusable,
  barHeight = 16,
}: {
  index: number
  intrinsic: string
  events: Events
  focusable?: boolean
  barHeight?: number
}) {
  return (
    <div
      testId={`entry-${index}`}
      onContentVisibilityAutoStateChange={(event) => events.push([index, event.skipped])}
      style={
        {
          display: "flex",
          flexDirection: "column",
          padding: 8,
          gap: 4,
          flexShrink: 0,
          contentVisibility: "auto",
          containIntrinsicSize: intrinsic,
        } as CSSProperties
      }
    >
      <div testId={`bar-${index}`} tabIndex={focusable ? -1 : undefined} style={{ height: barHeight, flexShrink: 0 }} />
      <div style={{ height: 1, flexShrink: 0 }} />
    </div>
  )
}

const Page = ({ children }: { children: ReactNode }) => (
  <div style={{ width: SIZE.width, height: SIZE.height, display: "flex", flexDirection: "column", overflow: "hidden" }}>{children}</div>
)

function List({ intrinsic, events, scrollerHeight, focusable }: { intrinsic: string; events: Events; scrollerHeight?: number; focusable?: boolean }) {
  return (
    <Page>
      <div
        testId="scroller"
        style={{ overflowY: "scroll", flexGrow: scrollerHeight ? 0 : 1, height: scrollerHeight, display: "flex", flexDirection: "column" }}
      >
        {Array.from({ length: ENTRIES }, (_, index) => (
          <Entry key={index} index={index} intrinsic={intrinsic} events={events} focusable={focusable} />
        ))}
      </div>
    </Page>
  )
}

const ITEMS = 120

/** A row of 100px items, scrolled along the x axis. */
function Row({ events }: { events: Events }) {
  return (
    <Page>
      <div testId="scroller" style={{ overflowX: "scroll", overflowY: "hidden", display: "flex", flexDirection: "row", height: 100, flexShrink: 0 }}>
        {Array.from({ length: ITEMS }, (_, index) => (
          <div
            key={index}
            testId={`entry-${index}`}
            onContentVisibilityAutoStateChange={(event) => events.push([index, event.skipped])}
            style={{ width: 100, height: 100, flex: "none", contentVisibility: "auto", containIntrinsicSize: "auto 100px 100px" } as CSSProperties}
          >
            <div testId={`bar-${index}`} style={{ height: 16 }} />
          </div>
        ))}
      </div>
    </Page>
  )
}

const OUTERS = 24
const INNERS = 20

/** Sections of rows, both `auto`. */
function Sections({ events, rows }: { events: Events; rows: Events }) {
  return (
    <Page>
      <div testId="scroller" style={{ overflowY: "scroll", flexGrow: 1, display: "flex", flexDirection: "column" }}>
        {Array.from({ length: OUTERS }, (_, outer) => (
          <div
            key={outer}
            testId={`entry-${outer}`}
            onContentVisibilityAutoStateChange={(event) => events.push([outer, event.skipped])}
            style={{ flexShrink: 0, contentVisibility: "auto", containIntrinsicSize: "auto 800px" } as CSSProperties}
          >
            {Array.from({ length: INNERS }, (_, inner) => (
              <div
                key={inner}
                testId={`row-${outer}-${inner}`}
                onContentVisibilityAutoStateChange={(event) => rows.push([outer * 100 + inner, event.skipped])}
                style={{ height: 40, contentVisibility: "auto", containIntrinsicSize: "auto 40px" } as CSSProperties}
              >
                <div testId={`bar-${outer}-${inner}`} style={{ height: 16 }} />
              </div>
            ))}
          </div>
        ))}
      </div>
    </Page>
  )
}

interface Remembering {
  shown: "visible" | "hidden"
  grown: boolean
}

/** What `auto` in `contain-intrinsic-size` stands for: a toggled element, one that was never shown, one far away. */
function Remembers({ events, shown, grown }: { events: Events } & Remembering) {
  const box = (value: string, id: string, height: number) => (
    <div testId={id} style={{ flexShrink: 0, width: 200, contentVisibility: value, containIntrinsicSize: "auto 60px" } as CSSProperties}>
      <div style={{ height, width: 50, flexShrink: 0 }} />
    </div>
  )
  return (
    <Page>
      <div testId="scroller" style={{ overflowY: "scroll", flexGrow: 1, display: "flex", flexDirection: "column" }}>
        {box(shown, "shown", grown ? 200 : 100)}
        {box("hidden", "never", 100)}
        <div style={{ height: 3000, flexShrink: 0 }} />
        <div
          testId="far"
          onContentVisibilityAutoStateChange={(event) => events.push([0, event.skipped])}
          style={{ flexShrink: 0, width: 200, contentVisibility: "auto", containIntrinsicSize: "auto 60px" } as CSSProperties}
        >
          <div style={{ height: 100, width: 50, flexShrink: 0 }} />
        </div>
      </div>
    </Page>
  )
}

/** Ids by `testId`, read once: looking an element up by `testId` walks the whole tree. */
function idsOf(renderer: TestRenderer): Map<string, number> {
  const ids = new Map<string, number>()
  for (const element of renderer.findByType("div")) if (element.testId) ids.set(element.testId, element.id)
  return ids
}

interface Seen {
  skipped?: number[]
  rects?: number[][]
  rows?: number[]
  sizes?: number[]
  focused?: number
  metrics?: number[][]
  height: number
  top: number
}

/** Draws until a frame changes nothing the test can see: a change takes effect in the frame after the one that found it. */
function settle(renderer: TestRenderer, read: () => string): number {
  let last = ""
  for (let frames = 1; frames <= 24; frames++) {
    renderer.drawFrame()
    renderer.dispatchNativeEvents()
    const now = read()
    if (now === last) return frames
    last = now
  }
  throw new Error("the scene did not settle in 24 frames")
}

/** Draws the window's first frame, with something else in it, so that what is added after it is not in the first frame. */
function drawFirstFrame(root: ReturnType<typeof createTestRoot>, renderer: TestRenderer) {
  root.render(
    <Page>
      <div style={{ height: 1 }} />
    </Page>
  )
  renderer.flush()
  renderer.drawFrame()
  renderer.dispatchNativeEvents()
}

/** A scene of the oracle: how to draw it, what it looks like, and what each step after the first does to it. */
interface Scene {
  render: (events: Events) => ReactNode
  /** The test's view of the scene, the one the oracle page reads. */
  look: (renderer: TestRenderer, ids: Map<string, number>) => Seen
  /** Step `n + 1` of the oracle. */
  steps: ((renderer: TestRenderer, ids: Map<string, number>, rerender: (node: ReactNode) => void, events: Events) => void)[]
  /**
   * What of Chrome's events the scene is held to: `exact` tells the same events step by step, `last` leaves each element
   * told the same state in the end (a layout that moves as the elements are laid out tells more than once), `none` holds
   * it to nothing, as Chrome's tells and the boxes it has disagree in a section inside a section.
   */
  events: "exact" | "last" | "none"
}

const built = (renderer: TestRenderer, ids: Map<string, number>, name: string) => Boolean(renderer.getBuiltElementBounds(ids.get(name)!))
const skippedIn = (renderer: TestRenderer, ids: Map<string, number>, count: number) =>
  Array.from({ length: count }, (_, index) => index).filter((index) => !built(renderer, ids, `bar-${index}`))
const scrolled = (renderer: TestRenderer, ids: Map<string, number>, along: "x" | "y") => {
  const metrics = renderer.getScrollMetrics(ids.get("scroller")!)!
  return along === "y" ? { top: metrics[1]!, height: metrics[3]! } : { top: metrics[0]!, height: metrics[2]! }
}
const scrollTo = (along: "x" | "y", offset: number): Scene["steps"][number] => (renderer, ids) =>
  along === "y" ? renderer.scrollTo(ids.get("scroller")!, 0, offset) : renderer.scrollTo(ids.get("scroller")!, offset, 0)

const listScene = (intrinsic: string, tops: number[], scrollerHeight?: number): Scene => ({
  render: (events) => <List intrinsic={intrinsic} events={events} scrollerHeight={scrollerHeight} />,
  look: (renderer, ids) => ({ skipped: skippedIn(renderer, ids, ENTRIES), ...scrolled(renderer, ids, "y") }),
  steps: tops.map((top) => scrollTo("y", top)),
  events: intrinsic === "auto 21px" ? "exact" : "last",
})

const FORCED_BARS = [10, 100, 150, 189]
const rectOf = (renderer: TestRenderer, id: number) => {
  const box = renderer.getElementBounds(id)!
  return [box.x, box.y, box.width, box.height]
}

/** Two skipped elements, one in the other. */
function Nested() {
  return (
    <Page>
      <div testId="scroller" style={{ overflowY: "scroll", flexGrow: 1, display: "flex", flexDirection: "column" }}>
        <div testId="outer" style={{ flexShrink: 0, padding: 4, contentVisibility: "hidden", containIntrinsicSize: "60px" } as CSSProperties}>
          <div style={{ height: 30 }} />
          <div testId="inner" style={{ margin: 6, contentVisibility: "hidden", containIntrinsicSize: "50px" } as CSSProperties}>
            <div testId="bar" style={{ height: 16, margin: 3 }} />
          </div>
        </div>
        <div style={{ height: 20, flexShrink: 0 }} />
      </div>
    </Page>
  )
}

/** A scroller in a `hidden` element and one in an `auto` element far below. */
function ScrollersInSkipped() {
  const scrolling = (testId: string) => (
    <div testId={testId} style={{ height: 50, width: 40, overflowY: "scroll", flexShrink: 0 }}>
      <div style={{ height: 200, width: 40, flexShrink: 0 }} />
    </div>
  )
  return (
    <Page>
      <div testId="scroller" style={{ overflowY: "scroll", flexGrow: 1, display: "flex", flexDirection: "column" }}>
        <div style={{ flexShrink: 0, contentVisibility: "hidden", containIntrinsicSize: "30px" } as CSSProperties}>{scrolling("held-scroller")}</div>
        <div style={{ height: 6000, flexShrink: 0 }} />
        <div style={{ flexShrink: 0, contentVisibility: "auto", containIntrinsicSize: "auto 30px" } as CSSProperties}>{scrolling("far-scroller")}</div>
      </div>
    </Page>
  )
}

const SCENES: Record<string, Scene> = {
  // The intrinsic size is not the size the entries have, so what is near moves as they are laid out.
  list: listScene("auto 40px", [1000, 4000, 7000, 0]),
  exact: listScene("auto 21px", [1000, 4000, 7000, 0]),
  // What clips an entry cuts the margin short.
  clipped: listScene("auto 21px", [2000, 0], 400),
  // No `contain-intrinsic-size`: `auto` is in effect all the same, so a skipped entry keeps the size it had.
  bare: listScene("none", [3000, 0]),
  // A read of geometry below a skipped element lays it out and leaves it skipped.
  forced: {
    ...listScene("auto 40px", [4000, 0]),
    look: (renderer, ids) => {
      const rects = FORCED_BARS.map((index) => rectOf(renderer, ids.get(`bar-${index}`)!))
      return { rects, skipped: skippedIn(renderer, ids, ENTRIES), ...scrolled(renderer, ids, "y") }
    },
  },
  forcedNested: {
    render: () => <Nested />,
    look: (renderer, ids) => ({ rects: ["bar", "inner", "outer"].map((name) => rectOf(renderer, ids.get(name)!)), ...scrolled(renderer, ids, "y") }),
    steps: [],
    events: "none",
  },
  // The scroll size of a scroller in skipped content is its extent, read before the content is built.
  forcedScroller: {
    render: () => <ScrollersInSkipped />,
    look: (renderer, ids) => ({
      metrics: ["held-scroller", "far-scroller"].map((name) => renderer.getScrollMetrics(ids.get(name)!)!),
      ...scrolled(renderer, ids, "y"),
    }),
    steps: [scrollTo("y", 2000)],
    events: "none",
  },
  horizontal: {
    render: (events) => <Row events={events} />,
    look: (renderer, ids) => ({ skipped: skippedIn(renderer, ids, ITEMS), ...scrolled(renderer, ids, "x") }),
    steps: [scrollTo("x", 5000), scrollTo("x", 0)],
    events: "exact",
  },
  nested: {
    render: (events) => <Sections events={events} rows={[]} />,
    look: (renderer, ids) => {
      const skipped = Array.from({ length: OUTERS }, (_, outer) => outer).filter((outer) => !built(renderer, ids, `row-${outer}-0`))
      const rows = Array.from({ length: OUTERS }, (_, outer) =>
        skipped.includes(outer) ? 0 : Array.from({ length: INNERS }, (_, row) => row).filter((row) => built(renderer, ids, `bar-${outer}-${row}`)).length
      )
      return { skipped, rows, ...scrolled(renderer, ids, "y") }
    },
    steps: [6000, 12000, 0].map((top) => scrollTo("y", top)),
    events: "none",
  },
  focus: {
    render: (events) => <List intrinsic="auto 21px" events={events} focusable />,
    look: (renderer, ids) => {
      const focused = renderer.getFocusedElementId()
      const index = Array.from({ length: ENTRIES }, (_, entry) => entry).find((entry) => ids.get(`bar-${entry}`) === focused)
      return { skipped: skippedIn(renderer, ids, ENTRIES), focused: index ?? -1, ...scrolled(renderer, ids, "y") }
    },
    steps: [
      (renderer, ids) => renderer.focusElement(ids.get("bar-180")!),
      scrollTo("y", 4000),
      scrollTo("y", 0),
      (renderer, ids) => renderer.blurElement(ids.get("bar-180")!),
    ],
    events: "exact",
  },
  remember: {
    render: (events) => <Remembers events={events} shown="visible" grown={false} />,
    look: (renderer, ids) => ({
      sizes: ["shown", "never", "far"].map((name) => renderer.getElementBounds(ids.get(name)!)!.height),
      ...scrolled(renderer, ids, "y"),
    }),
    steps: [
      (_renderer, _ids, rerender, events) => rerender(<Remembers events={events} shown="hidden" grown={false} />),
      (_renderer, _ids, rerender, events) => rerender(<Remembers events={events} shown="hidden" grown />),
      (_renderer, _ids, rerender, events) => rerender(<Remembers events={events} shown="visible" grown />),
      (_renderer, _ids, rerender, events) => rerender(<Remembers events={events} shown="hidden" grown />),
      scrollTo("y", 3000),
      scrollTo("y", 0),
    ],
    events: "exact",
  },
}

describeNative("content-visibility: auto against Chrome", { timeout: 120_000 }, () => {
  it("has the scenes of the oracle", () => {
    expect(Object.keys(ORACLE).sort()).toEqual(Object.keys(SCENES).sort())
    for (const [name, scene] of Object.entries(SCENES)) expect(scene.steps.length + 1, name).toBe(ORACLE[name]!.length)
  })

  for (const [name, scene] of Object.entries(SCENES)) {
    it(`skips what Chrome skips, tells what Chrome tells, and sizes the box as it does: ${name}`, () => {
      const events: Events = []
      const root = createTestRoot(SIZE)
      const renderer = root.renderer
      try {
        renderer.setInlineDraws(false)
        // The oracle adds the scene to a page that has settled: the first frame of a window is told differently.
        drawFirstFrame(root, renderer)
        root.render(scene.render(events))
        renderer.flush()
        const ids = idsOf(renderer)
        const read = () => JSON.stringify(scene.look(renderer, ids))
        const told = new Map<number, boolean>()
        const toldByChrome = new Map<number, boolean>()
        ORACLE[name]!.forEach((expected, step) => {
          events.length = 0
          if (step > 0) scene.steps[step - 1]!(renderer, ids, (node) => root.render(node), events)
          settle(renderer, read)
          const seen = scene.look(renderer, ids)
          const label = `${name}, ${expected.step}`
          if (expected.skipped) expect(seen.skipped, `${label}: skipped`).toEqual(expected.skipped)
          expected.rects?.forEach((rect, index) =>
            rect.forEach((value, side) =>
              expect(Math.abs(seen.rects![index]![side]! - value), `${label}: rect ${index} is [${seen.rects![index]}], Chrome's [${rect}]`).toBeLessThanOrEqual(UNIT)
            )
          )
          if (expected.metrics) expect(seen.metrics, `${label}: scroll metrics of the scrollers in skipped content`).toEqual(expected.metrics)
          if (expected.rows) expect(seen.rows, `${label}: rows built in each section`).toEqual(expected.rows)
          if (expected.focused !== undefined) expect(seen.focused, `${label}: the focused entry`).toBe(expected.focused)
          if (expected.sizes) {
            expected.sizes.forEach((size, index) =>
              expect(Math.abs(seen.sizes![index]! - size), `${label}: size of box ${index}: ${seen.sizes![index]} / ${size}`).toBeLessThanOrEqual(UNIT)
            )
          }
          expect(Math.abs(seen.height - expected.height), `${label}: scroll extent ${seen.height} / ${expected.height}`).toBeLessThanOrEqual(UNIT)
          expect(seen.top, `${label}: scroll offset`).toBe(expected.top)
          for (const [index, skipped] of events) told.set(index, skipped)
          for (const [index, skipped] of expected.events) toldByChrome.set(index, skipped)
          if (scene.events !== "none") {
            expect([...told].sort((a, b) => a[0] - b[0]), `${label}: what each element was last told`).toEqual(
              [...toldByChrome].sort((a, b) => a[0] - b[0])
            )
          }
          if (scene.events === "exact") {
            const once = (list: Events) => list.map(([index, skipped]) => `${index}:${skipped}`).sort()
            expect(once(events), `${label}: events`).toEqual(once(expected.events))
          }
        })
        if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
      } finally {
        root.unmount()
      }
    })
  }
})

// ── The frame that mounts it ──

/** What `change` and the frame it is drawn in did, as the difference of the counters. */
function drawn(renderer: TestRenderer, change: () => void) {
  const layout = renderer.getLayoutCounters()
  const built = renderer.getBuildCounters().elementsBuilt
  change()
  const started = performance.now()
  renderer.drawFrame()
  const ms = performance.now() - started
  const after = renderer.getLayoutCounters()
  return {
    ms,
    shapes: after.shapes - layout.shapes,
    nodes: after.nodesCreated - layout.nodesCreated,
    passes: after.passes - layout.passes,
    elementsBuilt: renderer.getBuildCounters().elementsBuilt - built,
  }
}

describeNative("an auto element in the frame that mounts it", { timeout: 120_000 }, () => {
  it("is built or skipped in that frame, and told once, as Chrome does", () => {
    const events: Events = []
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      drawFirstFrame(root, renderer)
      root.render(<List intrinsic="auto 21px" events={events} />)
      renderer.flush()
      const ids = idsOf(renderer)
      // One frame, and no more, before the page is looked at.
      renderer.drawFrame()
      renderer.dispatchNativeEvents()
      const expected = ORACLE.exact![0]!
      expect(skippedIn(renderer, ids, ENTRIES)).toEqual(expected.skipped)
      const once = (list: Events) => list.map(([index, skipped]) => `${index}:${skipped}`).sort()
      expect(once(events), "each entry is told its state once").toEqual(once(expected.events))
      // A page of any kind is built whole again by the frame after its first, and then nothing is left to do.
      renderer.drawFrame()
      renderer.drawFrame()
      renderer.dispatchNativeEvents()
      const built = renderer.getBuildCounters().elementsBuilt
      renderer.drawFrame()
      renderer.dispatchNativeEvents()
      expect(renderer.getBuildCounters().elementsBuilt, "an idle frame builds nothing").toBe(built)
      expect(skippedIn(renderer, ids, ENTRIES)).toEqual(expected.skipped)
      expect(once(events), "and tells nothing more").toEqual(once(expected.events))
    } finally {
      root.unmount()
    }
  })

  it("builds the entries near the viewport and skips the others: the page of 190 entries with text", () => {
    const report: string[] = []
    const measured: Record<string, ReturnType<typeof drawn> & { entries: number }> = {}
    for (const value of ["visible", "auto"] as const) {
      const events: Events = []
      const root = createTestRoot(SIZE)
      const renderer = root.renderer
      try {
        renderer.registerFont(SQUARE_FONT)
        renderer.setInlineDraws(false)
        root.render(<TextPage value={value} events={events} />)
        renderer.flush()
        const ids = idsOf(renderer)
        const first = drawn(renderer, () => {})
        const entries = skippedIn(renderer, ids, ENTRIES).length
        measured[value] = { ...first, entries: ENTRIES - entries }
        report.push(
          `${value}: ${ENTRIES - entries} entries built, ${first.elementsBuilt} elements built, ${first.shapes} shapes, ${first.nodes} nodes created, ${first.passes} layout passes, draw ${first.ms.toFixed(1)} ms`
        )
        if (value === "auto") {
          renderer.dispatchNativeEvents()
          // Everything the window shows is built in the frame, whatever else is.
          for (let index = 0; index < 18; index++) expect(built(renderer, ids, `bar-${index}`), `entry ${index} is on screen`).toBe(true)
          expect(first.shapes, "a text line shaped for the entries that are built and no others").toBeLessThanOrEqual(ENTRIES - entries)
          expect(first.shapes).toBeGreaterThanOrEqual(18)
          expect(ENTRIES - entries, "only the entries near the viewport").toBeLessThan(ENTRIES / 2)
        }
      } finally {
        root.unmount()
      }
    }
    console.log(`[content-visibility auto] r11 page, first draw\n  ${report.join("\n  ")}`)
    expect(measured.auto!.shapes).toBeLessThan(measured.visible!.shapes / 2)
    expect(measured.auto!.nodes).toBeLessThan(measured.visible!.nodes)
  })
})

function TextEntry({ index, value, events, intrinsic = "auto 40px" }: { index: number; value: "visible" | "auto"; events: Events; intrinsic?: string }) {
  return (
    <div
      onContentVisibilityAutoStateChange={(event) => events.push([index, event.skipped])}
      style={{ display: "flex", flexDirection: "column", flexShrink: 0, padding: 8, gap: 4, contentVisibility: value, containIntrinsicSize: intrinsic } as CSSProperties}
    >
      <div testId={`row-${index}`} style={{ display: "flex", flexDirection: "row", alignItems: "center", gap: 8 }}>
        <div testId={`bar-${index}`} style={{ width: 16, height: 16, borderRadius: 8, backgroundColor: "#999999" }} />
        <div style={{ display: "flex", flexDirection: "column" }}>
          <text style={{ fontSize: 14, color: "#222222" }}>{`Entry ${index}: the sidebar row keeps its fluid hover while the page behind it swaps`}</text>
        </div>
        <div style={{ display: "flex", flexDirection: "row", gap: 2 }}>
          <div style={{ width: 6, height: 6, backgroundColor: "#dddddd" }} />
          <div style={{ width: 6, height: 6, backgroundColor: "#dddddd" }} />
        </div>
      </div>
      <div testId={`rule-${index}`} style={{ height: 1, backgroundColor: "#dddddd" }} />
    </div>
  )
}

function TextPage({ value, events, intrinsic }: { value: "visible" | "auto"; events: Events; intrinsic?: string }) {
  return (
    <div style={{ width: SIZE.width, height: SIZE.height, display: "flex", flexDirection: "column", fontFamily: "GpuixTestSquare" }}>
      <div testId="scroller" style={{ overflowY: "scroll", flexGrow: 1, display: "flex", flexDirection: "column" }}>
        {Array.from({ length: ENTRIES }, (_, index) => (
          <TextEntry key={index} index={index} value={value} events={events} intrinsic={intrinsic} />
        ))}
      </div>
    </div>
  )
}

/** A sidebar of text rows beside the list: roots a frame can replay, which a change in the list must leave alone. */
function BesideSidebar({ rows, events }: { rows: number; events: Events }) {
  return (
    <div style={{ width: SIZE.width, height: SIZE.height, display: "flex", flexDirection: "row", fontFamily: "GpuixTestSquare" }}>
      <div style={{ width: 160, flexShrink: 0, display: "flex", flexDirection: "column" }}>
        {Array.from({ length: rows }, (_, index) => (
          <div key={index} style={{ height: 24, padding: 4 }}>
            <text style={{ fontSize: 14, color: "#555555" }}>{`Sidebar row ${index}`}</text>
          </div>
        ))}
      </div>
      <div testId="scroller" style={{ overflowY: "scroll", flexGrow: 1, display: "flex", flexDirection: "column" }}>
        {Array.from({ length: ENTRIES }, (_, index) => (
          <TextEntry key={index} index={index} value="auto" events={events} intrinsic="auto 37px" />
        ))}
      </div>
    </div>
  )
}

describeNative("a change of what is near", { timeout: 120_000 }, () => {
  /** The subtrees each frame after a scroll builds, and how many entries the scroll showed. */
  const scrollRun = (rows: number, by: number) => {
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.registerFont(SQUARE_FONT)
      renderer.setInlineDraws(false)
      renderer.clockPause()
      root.render(<BesideSidebar rows={rows} events={[]} />)
      renderer.flush()
      const ids = idsOf(renderer)
      const frame = () => {
        const before = renderer.getRetainedCounters().builds
        renderer.drawFrame()
        renderer.dispatchNativeEvents()
        return renderer.getRetainedCounters().builds - before
      }
      for (let index = 0; index < 8; index++) frame()
      expect([frame(), frame()], "the page is at rest").toEqual([0, 0])
      const skipped = skippedIn(renderer, ids, ENTRIES).length
      renderer.scrollTo(ids.get("scroller")!, 0, by)
      const builds = [frame(), frame(), frame(), frame()]
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
      return { builds, shown: skipped - skippedIn(renderer, ids, ENTRIES).length }
    } finally {
      root.unmount()
    }
  }

  it("builds the roots above the entries that change and replays the rest, as a commit does", () => {
    // 37px an entry: 5px shows none, 120px shows three. The scroll itself has the entries in view built again, once.
    const still = scrollRun(60, 5)
    expect(still.shown).toBe(0)
    const moved = scrollRun(60, 120)
    expect(moved.shown).toBe(3)
    // Verification builds every frame twice: only that it found no mismatch is left to hold.
    if (verifying) return
    expect(scrollRun(0, 120), "the sidebar is replayed: its 60 rows add no build").toEqual(moved)
    expect(moved.builds[0], "the scroll costs what it costs without a change").toBe(still.builds[0])
    // The frame that shows the three entries builds them, and records their roots in that frame (a root built whole is
    // recorded by the build that makes it): their roots are the only builds it adds, and no frame after it builds them again.
    const added = moved.builds[1] - still.builds[1]
    expect(added, "the roots of the three entries are built once, in the frame that shows them").toBeGreaterThan(0)
    expect(added).toBeLessThanOrEqual(3 * 8)
    expect(moved.builds.slice(2), "and the frames after that build nothing").toEqual([0, 0])
  })
})

describeNative("a read of geometry below a skipped element", { timeout: 120_000 }, () => {
  const work = (renderer: TestRenderer, read: () => void) => {
    const layout = renderer.getLayoutCounters()
    const built = renderer.getBuildCounters().elementsBuilt
    const passes = renderer.getLayoutPassCount()
    const frame = renderer.getFrameWorkCounters()
    read()
    const after = renderer.getFrameWorkCounters()
    return {
      nodes: renderer.getLayoutCounters().nodesCreated - layout.nodesCreated,
      shapes: renderer.getLayoutCounters().shapes - layout.shapes,
      built: renderer.getBuildCounters().elementsBuilt - built,
      passes: renderer.getLayoutPassCount() - passes,
      painted: after.painted - frame.painted,
      primitives: after.paintedPrimitives - frame.paintedPrimitives,
    }
  }

  it("lays out the entry it is in, leaves it skipped and paints nothing of it", () => {
    const events: Events = []
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.registerFont(SQUARE_FONT)
      renderer.setInlineDraws(false)
      root.render(<TextPage value="auto" events={events} />)
      renderer.flush()
      const ids = idsOf(renderer)
      for (let index = 0; index < 6; index++) {
        renderer.drawFrame()
        renderer.dispatchNativeEvents()
      }
      const clean = () => {
        renderer.drawFrame()
        renderer.getBuiltElementBounds(ids.get("bar-3")!)
      }
      const skippedBefore = skippedIn(renderer, ids, ENTRIES)
      expect(skippedBefore).toContain(150)
      events.length = 0
      // What the frame after a change that moves nothing and a read of a box that is built paints, for the frame after the
      // read to be compared with.
      const dirty = () => {
        renderer.flush()
        renderer.getElementBounds(ids.get("bar-3")!)
        return work(renderer, () => renderer.drawFrame())
      }
      dirty()
      const control = dirty()
      expect(control.painted).toBeGreaterThan(0)

      clean()
      const near = work(renderer, () => expect(renderer.getElementBounds(ids.get("bar-3")!)).toBeTruthy())
      expect(near, "a read of an element that is built costs nothing on a clean window").toMatchObject({ passes: 0, built: 0, nodes: 0, shapes: 0 })

      clean()
      let bar: ReturnType<TestRenderer["getElementBounds"]>
      const far = work(renderer, () => (bar = renderer.getElementBounds(ids.get("bar-150")!)))
      // Above it are the 53 near entries of 37px and 97 skipped ones of 56px (40px and the padding): size containment stays.
      expect(bar!.y, "the bar of an entry that is skipped, where Chrome puts it").toBe(53 * 37 + 97 * 56 + 8)
      expect(far.passes, "one layout pass").toBe(1)
      expect(far.nodes, "nodes for the entry that holds it and nothing else").toBeGreaterThan(0)
      expect(far.nodes).toBeLessThanOrEqual(12)
      expect(far.shapes, "the one line of text of that entry").toBe(1)
      expect(far.painted + far.primitives, "nothing is painted by a read").toBe(0)

      expect(skippedIn(renderer, ids, ENTRIES), "it is skipped still").toEqual(skippedBefore)
      renderer.dispatchNativeEvents()
      expect(events, "and nothing was told").toEqual([])
      expect(renderer.getBuiltElementBounds(ids.get("bar-150")!), "its content is not built").toBeFalsy()

      const frame = work(renderer, () => renderer.drawFrame())
      // Verification builds each frame it replays a second time, and counts what that paints.
      if (!verifying) {
        expect(frame.painted, "the frame after the read paints what any frame does").toBe(control.painted)
        expect(frame.built, "and builds what it does").toBe(control.built)
      }
      const text = renderer.getPaintedText()
      expect(text.some((line) => line.startsWith("Entry 3:")), "the entries that are near are painted").toBe(true)
      expect(text.some((line) => line.startsWith("Entry 150:")), "and the one that was read is not").toBe(false)
      expect(skippedIn(renderer, ids, ENTRIES)).toEqual(skippedBefore)
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })
})

/** What reading the boxes of `names` costs: the layout passes (the window's, and the engine's) and the nodes. */
const reading = (renderer: TestRenderer, ids: Map<string, number>, names: string[]) => {
  const layout = renderer.getLayoutCounters()
  const passes = renderer.getLayoutPassCount()
  const read = names.map((name) => renderer.getElementBounds(ids.get(name)!))
  const after = renderer.getLayoutCounters()
  return {
    read,
    passes: renderer.getLayoutPassCount() - passes,
    enginePasses: after.passes - layout.passes,
    nodes: after.nodesCreated - layout.nodesCreated,
    laidOut: after.nodesLaidOut - layout.nodesLaidOut,
  }
}

describeNative("consecutive reads of geometry below skipped elements", { timeout: 120_000 }, () => {
  type Bounds = ReturnType<TestRenderer["getElementBounds"]>

  /** The page of `TextPage`, drawn to rest, and what a read of `names` costs in passes and nodes. */
  function open(intrinsic?: string) {
    const events: Events = []
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    renderer.registerFont(SQUARE_FONT)
    renderer.setInlineDraws(false)
    root.render(<TextPage value="auto" events={events} intrinsic={intrinsic} />)
    renderer.flush()
    const ids = idsOf(renderer)
    for (let index = 0; index < 6; index++) {
      renderer.drawFrame()
      renderer.dispatchNativeEvents()
    }
    return { root, renderer, ids, events }
  }

  /** A frame, and a read of a built element, which leaves a window that has nothing to lay out. */
  const clean = (renderer: TestRenderer, ids: Map<string, number>) => {
    renderer.drawFrame()
    renderer.getBuiltElementBounds(ids.get("bar-3")!)
  }

  // 53 entries of 37px are near and built, the rest skipped at 56px (40px and the padding).
  const BAR_150 = 53 * 37 + 97 * 56 + 8
  const BAR_160 = 53 * 37 + 107 * 56 + 8

  it("share the pass that forced the entry: ten reads inside one entry run one pass", () => {
    const { root, renderer, ids } = open()
    try {
      clean(renderer, ids)
      const names = Array.from({ length: 10 }, (_, read) => ["bar-150", "row-150", "rule-150"][read % 3]!)
      const first = reading(renderer, ids, names.slice(0, 1))
      expect(first.read[0]!.y, "where Chrome puts the bar of a skipped entry").toBe(BAR_150)
      expect(first.passes).toBe(1)
      const rest = reading(renderer, ids, names.slice(1))
      expect(rest, "the nine that follow cost nothing").toMatchObject({ passes: 0, enginePasses: 0, nodes: 0, laidOut: 0 })
      const [row, rule] = [rest.read[0]!, rest.read[1]!]
      expect(row.y, "the row holds the bar").toBeLessThanOrEqual(BAR_150)
      expect(rule.y, "and the rule is under it, a gap away").toBe(row.y + row.height + 4)
      for (const [index, bounds] of rest.read.entries()) expect(bounds, `read ${index + 1}`).toEqual(reading(renderer, ids, [names[index + 1]!]).read[0])

      // A read of an element that is built, and of the scroller's metrics, is answered by the same pass.
      clean(renderer, ids)
      reading(renderer, ids, ["bar-150"])
      const near = reading(renderer, ids, ["bar-3", "scroller"])
      expect(near, "the built elements are those of the forced pass").toMatchObject({ passes: 0, nodes: 0 })
      const before = renderer.getLayoutPassCount()
      expect(renderer.getScrollMetrics(ids.get("scroller")!)![3]).toBeGreaterThan(SIZE.height)
      expect(renderer.getLayoutPassCount() - before, "and so are the scroll metrics").toBe(0)
    } finally {
      root.unmount()
    }
  })

  it("take one more pass for each entry that is not forced yet, and that pass forces it with the entries read before", () => {
    const { root, renderer, ids } = open()
    try {
      clean(renderer, ids)
      const first = reading(renderer, ids, ["bar-150"])
      expect(first.passes).toBe(1)
      const second = reading(renderer, ids, ["bar-160"])
      expect(second.read[0]!.y).toBe(BAR_160)
      expect(second.passes, "the union of the two entries is forced in one pass").toBe(1)
      // Verification builds a pass twice, so the engine counts what a pass takes, not one.
      expect(second.enginePasses, "and the engine's passes are those of the layout passes").toBe(first.enginePasses)
      const again = reading(renderer, ids, ["bar-150", "bar-160", "bar-150", "bar-160", "row-160"])
      expect(again, "both are forced now").toMatchObject({ passes: 0, enginePasses: 0, nodes: 0, laidOut: 0 })
      expect(again.read[0], "the first entry kept its box through the second pass").toEqual(first.read[0])
      expect(again.read[1]).toEqual(second.read[0])
      // A third entry makes the union of three.
      const third = reading(renderer, ids, ["bar-170", "bar-150"])
      expect(third.passes).toBe(1)
      expect(third.read[0]!.y).toBe(BAR_160 + 10 * 56)
      expect(third.read[1]).toEqual(first.read[0])
    } finally {
      root.unmount()
    }
  })

  it("end with a change: the next read lays out the new content", () => {
    const { root, renderer, ids, events } = open()
    try {
      clean(renderer, ids)
      expect(reading(renderer, ids, ["bar-150"]).read[0]!.y).toBe(BAR_150)
      expect(reading(renderer, ids, ["bar-150", "row-150"]).passes).toBe(0)

      root.render(<TextPage value="auto" events={events} intrinsic="auto 60px" />)
      renderer.flush()
      const changed = reading(renderer, ids, ["bar-150"])
      expect(changed.read[0]!.y, "the 97 skipped entries above are 20px taller").toBe(BAR_150 + 97 * 20)
      // One pass lays out the change and the entry, as Chrome does: the ancestors are forced before the pass is built.
      expect(changed.passes).toBe(1)
      const shared = reading(renderer, ids, ["bar-150", "rule-150"])
      expect(shared.passes, "and the reads after it share it").toBe(0)
      expect(shared.read[0]).toEqual(changed.read[0])

      // A scroll write moves the boxes: the next read lays out again.
      renderer.scrollTo(ids.get("scroller")!, 0, 30)
      const scrolled = reading(renderer, ids, ["bar-150"])
      expect(scrolled.read[0]!.y, "where it is now").toBe(BAR_150 + 97 * 20 - 30)
      expect(scrolled.passes).toBeGreaterThan(0)
      expect(reading(renderer, ids, ["bar-150"]).passes).toBe(0)
    } finally {
      root.unmount()
    }
  })

  it("survive a painted frame that moved no box: the next read shares the pass and the frames skip the entry", () => {
    const { root, renderer, ids } = open()
    try {
      clean(renderer, ids)
      const skippedBefore = Array.from({ length: ENTRIES }, (_, index) => index).filter((index) => !renderer.getBuiltElementBounds(ids.get(`bar-${index}`)!))
      expect(skippedBefore).toContain(150)

      const first = reading(renderer, ids, ["bar-150"])
      expect(first.passes).toBe(1)
      const frames = renderer.getPaintedFrameCount()
      renderer.drawFrame()
      expect(renderer.getPaintedFrameCount() - frames, "the frame paints").toBe(1)

      clean(renderer, ids)
      const after = reading(renderer, ids, ["bar-150", "row-150"])
      expect(after.passes, "the frame skipped the entry again but moved no box, so the pass that forced it is still the window's").toBe(0)
      expect(after.read[0]).toEqual(first.read[0])
      expect(reading(renderer, ids, ["bar-150"]).passes).toBe(0)
      // The painted text is read through a flush, which is a layout invalidation and ends the share.
      expect(renderer.getPaintedText().some((line) => line.startsWith("Entry 150:")), "the frame does not paint the entry").toBe(false)
      expect(reading(renderer, ids, ["bar-150"]).passes, "and a layout invalidation ends the share").toBe(1)

      renderer.drawFrame()
      renderer.dispatchNativeEvents()
      expect(renderer.getPaintedText().some((line) => line.startsWith("Entry 150:"))).toBe(false)
      expect(renderer.getPaintedText().some((line) => line.startsWith("Entry 3:"))).toBe(true)
      const skippedAfter = Array.from({ length: ENTRIES }, (_, index) => index).filter((index) => !renderer.getBuiltElementBounds(ids.get(`bar-${index}`)!))
      expect(skippedAfter, "the entries are skipped still").toEqual(skippedBefore)
      expect(renderer.getBuiltElementBounds(ids.get("bar-150")!), "and not built").toBeFalsy()
    } finally {
      root.unmount()
    }
  })
})

// ── The first read after a change ──

const SCROLLER_STYLE = { overflowY: "scroll", flexGrow: 1, display: "flex", flexDirection: "column" } as CSSProperties
const Lead = () => <div style={{ height: 20, flexShrink: 0 }} />
const After = () => <div testId="after" style={{ height: 20, flexShrink: 0 }} />
const Scrolled = ({ children }: { children: ReactNode }) => (
  <Page>
    <div testId="scroller" style={SCROLLER_STYLE}>
      {children}
    </div>
  </Page>
)
const hidden = (intrinsic: string, padding: number) =>
  ({ flexShrink: 0, contentVisibility: "hidden", containIntrinsicSize: intrinsic, padding }) as CSSProperties

/** The page of the scene `firstReadKept`: a `hidden` element of 8 cards of 3 lines, between two boxes that change. */
function KeptPage({ step, visibility = "hidden" }: { step: number; visibility?: "hidden" | "visible" }) {
  // No scroll container around the page, unlike the oracle's (and `Page`, which clips). The boxes are the same.
  return (
    <div style={{ width: SIZE.width, height: SIZE.height, display: "flex", flexDirection: "column" }}>
      <div style={{ height: [20, 20, 30, 30][step], flexShrink: 0 }} />
      <div testId="held" style={{ ...hidden("30px", 4), contentVisibility: visibility } as CSSProperties}>
        {Array.from({ length: 8 }, (_, card) => (
          <div key={card} testId={`card-${card}`} style={{ margin: 3, padding: 2, flexShrink: 0 }}>
            {Array.from({ length: 3 }, (_, line) => (
              <div key={line} testId={`line-${card}-${line}`} style={{ height: 10 + line * 4 + card, width: 40, margin: 1, flexShrink: 0 }} />
            ))}
          </div>
        ))}
      </div>
      <div testId="after" style={{ height: [20, 36, 36, 20][step], flexShrink: 0 }} />
    </div>
  )
}

/** The page of the scenes `firstReadKept...`: a `hidden` or `visible` element between two boxes. */
function LockedShell({ visibility = "hidden", children }: { visibility?: "hidden" | "visible"; children: ReactNode }) {
  return (
    <div style={{ width: SIZE.width, height: SIZE.height, display: "flex", flexDirection: "column" }}>
      <div style={{ height: 20, flexShrink: 0 }} />
      <div testId="held" style={{ ...hidden("30px", 4), contentVisibility: visibility } as CSSProperties}>
        {children}
      </div>
      <div testId="after" style={{ height: 20, flexShrink: 0 }} />
    </div>
  )
}

/** A card of 3 lines, the middle one taller when `grown` and painted when `tinted`. */
function LockCard({ group, grown, tinted }: { group: number; grown?: boolean; tinted?: boolean }) {
  return (
    <div testId={`card-${group}`} style={{ margin: 3, padding: 2, flexShrink: 0 }}>
      {Array.from({ length: 3 }, (_, line) => (
        <div
          key={line}
          testId={`line-${group}-${line}`}
          style={{
            height: grown && line === 1 ? 30 : 10 + line * 4 + group,
            width: 40,
            margin: 1,
            flexShrink: 0,
            backgroundColor: tinted && line === 1 ? "#ff0000" : undefined,
          }}
        />
      ))}
    </div>
  )
}

/** The scene `firstReadKeptInside`: a line grows, a line is painted, a card is inserted and one removed. */
function InsidePage({ step }: { step: number }) {
  const groups = [...(step >= 3 ? [9] : []), ...[0, 1, 2, 3, 4, 5, 6, 7].filter((group) => step < 4 || group !== 0)]
  return (
    <LockedShell>
      {groups.map((group) => (
        <LockCard key={group} group={group} grown={step >= 1 && group === 2} tinted={step >= 2 && group === 2} />
      ))}
    </LockedShell>
  )
}

/** The scene `firstReadKeptNested`: a `hidden` element in a `hidden` element. */
function NestedPage({ step }: { step: number }) {
  return (
    <div style={{ width: SIZE.width, height: SIZE.height, display: "flex", flexDirection: "column" }}>
      <div style={{ height: step >= 4 ? 35 : 20, flexShrink: 0 }} />
      <div testId="outer" style={hidden("60px", 4)}>
        <div testId="pre" style={{ margin: 3, flexShrink: 0 }}>
          <div testId="pre-line" style={{ height: step >= 2 ? 30 : 12, width: 40, flexShrink: 0 }} />
        </div>
        <div
          testId="inner"
          style={{ margin: 3, padding: 2, flexShrink: 0, contentVisibility: "hidden", containIntrinsicSize: step >= 3 ? "70px" : "40px" } as CSSProperties}
        >
          {Array.from({ length: 4 }, (_, bar) => (
            <div key={bar} testId={`bar-${bar}`} style={{ height: bar === 1 && step >= 1 ? 25 : 8 + bar, width: 30, margin: 1, flexShrink: 0 }} />
          ))}
        </div>
        <div testId="post" style={{ margin: 3, flexShrink: 0 }}>
          <div testId="post-line" style={{ height: 14, width: 40, flexShrink: 0 }} />
        </div>
      </div>
      <div testId="after" style={{ height: 20, flexShrink: 0 }} />
    </div>
  )
}

const SHOWN_STEPS: ("hidden" | "visible")[] = ["hidden", "visible", "visible", "hidden", "hidden", "visible"]

/** The scene `firstReadKeptShown`: an element that shows, changes shown, hides, changes hidden and shows again. */
function ShownPage({ step }: { step: number }) {
  return (
    <LockedShell visibility={SHOWN_STEPS[step]}>
      {Array.from({ length: 8 }, (_, group) => (
        <div key={group} testId={`card-${group}`} style={{ margin: 3, padding: 2, flexShrink: 0 }}>
          {Array.from({ length: 3 }, (_, line) => (
            <div
              key={line}
              testId={`line-${group}-${line}`}
              style={{ height: group === 2 && line === 1 ? [16, 16, 30, 30, 12, 12][step] : 10 + line * 4 + group, width: 40, margin: 1, flexShrink: 0 }}
            />
          ))}
        </div>
      ))}
    </LockedShell>
  )
}

/** A scene of the oracle that is read right after each change, and what the page looks like at each step. */
interface FirstRead {
  render: (step: number) => ReactNode
  /** Whether the frame that follows the reads skips the content of the first element read. */
  skipped?: (step: number) => boolean
}

const FIRST_READ_SCENES: Record<string, FirstRead> = {
  // A layout write inside a `hidden` element.
  firstReadInside: {
    render: (step) => (
      <Scrolled>
        <Lead />
        <div testId="held" style={hidden(step >= 3 ? "50px" : "30px", step >= 4 ? 10 : 4)}>
          <div testId="child" style={{ height: step >= 1 ? 90 : 70, width: 40, margin: step >= 2 ? 12 : 5, flexShrink: 0 }} />
          <div testId="twin" style={{ height: 20, width: 30, flexShrink: 0 }} />
        </div>
        <After />
      </Scrolled>
    ),
  },
  // Changes outside a `hidden` element that holds many elements, with the boxes inside it read after each.
  firstReadKept: {
    render: (step) => <KeptPage step={step} />,
  },
  firstReadKeptInside: { render: (step) => <InsidePage step={step} /> },
  firstReadKeptNested: { render: (step) => <NestedPage step={step} /> },
  firstReadKeptShown: { render: (step) => <ShownPage step={step} />, skipped: (step) => SHOWN_STEPS[step] === "hidden" },
  // A write that turns the ancestor `hidden` or `visible` in the turn of a write inside it.
  firstReadTurns: {
    render: (step) => (
      <Scrolled>
        <Lead />
        <div testId="held" style={{ ...hidden("40px", 4), contentVisibility: step % 2 === 1 ? "hidden" : "visible" } as CSSProperties}>
          <div testId="child" style={{ height: [70, 70, 70, 120, 150][step], width: 40, margin: 5, flexShrink: 0 }} />
        </div>
        <After />
      </Scrolled>
    ),
    skipped: (step) => step % 2 === 1,
  },
  // A node inserted into `hidden` content, a `hidden` element inserted with the node read inside it, and a node inserted into that.
  firstReadInserted: {
    render: (step) => (
      <Scrolled>
        <Lead />
        <div testId="held" style={hidden("30px", 4)}>
          {step >= 1 ? <div key="ins" testId="ins" style={{ height: 35, width: 30, margin: 3, flexShrink: 0 }} /> : null}
          <div key="a" testId="a" style={{ height: 20, width: 30, flexShrink: 0 }} />
        </div>
        {step >= 2 ? (
          <div key="wrapper" testId="wrapper" style={hidden("40px", 2)}>
            {step >= 3 ? <div key="deep" testId="deep" style={{ height: 12, width: 10, flexShrink: 0 }} /> : null}
            <div key="target" testId="target" style={{ height: 25, width: 20, flexShrink: 0 }} />
          </div>
        ) : null}
        <After />
      </Scrolled>
    ),
  },
  // Two `hidden` elements, one in the other, where only the inner one changes.
  firstReadNested: {
    render: (step) => (
      <Scrolled>
        <Lead />
        <div testId="outer" style={hidden("120px", 4)}>
          <div style={{ height: 30 }} />
          <div
            testId="inner"
            style={
              {
                margin: step >= 4 ? 2 : 6,
                contentVisibility: step === 2 ? "visible" : "hidden",
                containIntrinsicSize: step >= 1 ? "80px" : "50px",
              } as CSSProperties
            }
          >
            <div testId="bar" style={{ height: step >= 3 ? 40 : 16, margin: 3 }} />
          </div>
        </div>
        <After />
      </Scrolled>
    ),
  },
  // `auto` entries far from the viewport: a change of all of them, then of a far one and of a near one.
  firstReadAuto: {
    render: (step) => (
      <Page>
        <div testId="scroller" style={SCROLLER_STYLE}>
          {Array.from({ length: ENTRIES }, (_, index) => (
            <Entry
              key={index}
              index={index}
              intrinsic={step >= 1 ? "auto 60px" : "auto 40px"}
              events={[]}
              barHeight={index === 150 && step >= 2 ? 30 : index === 10 && step >= 3 ? 40 : 16}
            />
          ))}
        </div>
      </Page>
    ),
  },
  // The scroll size of a scroller in a `hidden` element.
  firstReadScroller: {
    render: (step) => (
      <Scrolled>
        <div testId="held" style={{ flexShrink: 0, contentVisibility: "hidden", containIntrinsicSize: "30px" } as CSSProperties}>
          <div testId="scrolling" style={{ height: step >= 3 ? 80 : 50, width: 40, overflowY: "scroll", flexShrink: 0 }}>
            <div style={{ height: [200, 320, 30, 30][step], width: 40, flexShrink: 0 }} />
          </div>
        </div>
      </Scrolled>
    ),
  },
}

describeNative("the first read after a change", { timeout: 120_000 }, () => {
  it("has the scenes of the oracle", () => {
    expect(Object.keys(FIRST_READS).sort()).toEqual(Object.keys(FIRST_READ_SCENES).sort())
  })

  for (const [name, scene] of Object.entries(FIRST_READ_SCENES)) {
    it(`lays out once, as Chrome does, and returns Chrome's boxes: ${name}`, () => {
      const root = createTestRoot(SIZE)
      const renderer = root.renderer
      try {
        renderer.setInlineDraws(false)
        drawFirstFrame(root, renderer)
        root.render(scene.render(0))
        renderer.flush()
        for (let index = 0; index < 6; index++) {
          renderer.drawFrame()
          renderer.dispatchNativeEvents()
        }
        FIRST_READS[name]!.forEach((expected, step) => {
          const label = `${name}, ${expected.step}`
          if (step > 0) {
            root.render(scene.render(step))
            renderer.flush()
          }
          const ids = idsOf(renderer)
          if (expected.metrics) {
            const passes = renderer.getLayoutPassCount()
            const engine = renderer.getLayoutCounters().passes
            const metrics = renderer.getScrollMetrics(ids.get("scrolling")!)
            expect(renderer.getLayoutPassCount() - passes, `${label}: the first scroll metrics read costs one pass`).toBe(1)
            expect(renderer.getLayoutCounters().passes - engine, `${label}: and the engine's passes are those of one`).toBeGreaterThan(0)
            expect(metrics, `${label}: scroll metrics`).toEqual(expected.metrics[0])
            const again = renderer.getLayoutPassCount()
            expect(renderer.getScrollMetrics(ids.get("scrolling")!)).toEqual(metrics)
            expect(renderer.getElementBounds(ids.get("held")!)).toBeDefined()
            expect(renderer.getLayoutPassCount() - again, `${label}: the reads after it share the pass`).toBe(0)
          } else {
            const names = expected.names!
            const first = reading(renderer, ids, names.slice(0, 1))
            expect(first.passes, `${label}: the first read costs one pass`).toBe(1)
            expect(first.enginePasses, `${label}: and the engine's passes are those of one`).toBeGreaterThan(0)
            const rest = reading(renderer, ids, names.slice(1))
            expect(rest, `${label}: the reads after it cost nothing`).toMatchObject({ passes: 0, enginePasses: 0, nodes: 0, laidOut: 0 })
            const seen = [...first.read, ...rest.read].map((box) => [box!.x, box!.y, box!.width, box!.height])
            expected.rects!.forEach((rect, index) =>
              rect.forEach((value, side) =>
                expect(Math.abs(seen[index]![side]! - value), `${label}: ${names[index]} is [${seen[index]}], Chrome's [${rect}]`).toBeLessThanOrEqual(UNIT)
              )
            )
            renderer.drawFrame()
            renderer.dispatchNativeEvents()
            const kept = scene.skipped ? scene.skipped(step) : true
            expect(Boolean(renderer.getBuiltElementBounds(ids.get(names[0]!)!)), `${label}: the frame after the read ${kept ? "skips" : "builds"} it`).toBe(!kept)
          }
        })
        if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
      } finally {
        root.unmount()
      }
    })
  }
})

describeNative("forced reads inside hidden content that is laid out once and kept", { timeout: 120_000 }, () => {
  /** How the page was drawn before the reads: `never` has no frame at all, `visible` draws the content shown and makes it hidden
   *  with no frame after, `hidden` draws the frames that skip it. */
  type Drawn = "never" | "visible" | "hidden"

  function reads(drawn: Drawn) {
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      drawFirstFrame(root, renderer)
      const frames = () => {
        for (let index = 0; index < 6; index++) {
          renderer.drawFrame()
          renderer.dispatchNativeEvents()
        }
      }
      if (drawn !== "never") {
        root.render(<KeptPage step={0} visibility="visible" />)
        renderer.flush()
        frames()
      }
      const painted = () => renderer.getPaintedFrameCount()
      let drawnFrames = painted()
      const steps = FIRST_READS.firstReadKept!.map((expected, step) => {
        root.render(<KeptPage step={step} />)
        renderer.flush()
        if (step === 0 && drawn === "hidden") {
          frames()
          drawnFrames = painted()
        }
        const ids = idsOf(renderer)
        const names = expected.names!
        const before = renderer.getRetainedCounters()
        renderer.resetBuildCounters()
        const first = reading(renderer, ids, names.slice(0, 1))
        const built = renderer.getBuildCounters().elementsBuilt
        const rest = reading(renderer, ids, names.slice(1))
        const after = renderer.getRetainedCounters()
        const seen = [...first.read, ...rest.read].map((box) => [box!.x, box!.y, box!.width, box!.height])
        expected.rects!.forEach((rect, index) =>
          rect.forEach((value, side) =>
            expect(Math.abs(seen[index]![side]! - value), `${drawn}, ${expected.step}: ${names[index]} is [${seen[index]}], Chrome's [${rect}]`).toBeLessThanOrEqual(UNIT)
          )
        )
        expect(first.passes, `${drawn}, ${expected.step}: the first read is one pass`).toBe(1)
        expect(rest.passes, `${drawn}, ${expected.step}: and the reads after it share it`).toBe(0)
        expect(painted() - drawnFrames, "no frame is drawn between the reads").toBe(0)
        return {
          built,
          skips: after.layoutSkips - before.layoutSkips,
          records: after.layoutRecords - before.layoutRecords,
          builds: after.layoutBuilds - before.layoutBuilds,
        }
      })
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
      return steps
    } finally {
      root.unmount()
    }
  }

  for (const drawn of ["never", "visible", "hidden"] as const) {
    it(`builds the hidden subtree for the first read and not for the next, whatever changes outside it: ${drawn}`, () => {
      const steps = reads(drawn)
      expect(steps[0]!.built, "the first read builds the page, the subtree in it included").toBeGreaterThanOrEqual(36)
      expect(steps[0]!.records, "and leaves a layout record of each card and of the page").toBeGreaterThanOrEqual(9)
      // A box above it grows (step 2) and the hidden element is not where it was: its cards are skipped where they are now,
      // as Chrome keeps the layout of a locked subtree that only moves.
      // A verified pass builds the page a second time to compare it, which is not the cost being measured.
      for (const step of verifying ? [] : [1, 2, 3]) {
        expect(steps[step]!.built, `step ${step}: the box that changed and the page, not the subtree`).toBeLessThanOrEqual(8)
        expect(steps[step]!.skips, `step ${step}: the cards are skipped with their layout records`).toBeGreaterThanOrEqual(8)
      }
    })
  }
})

describeNative("consecutive reads inside a hidden element", { timeout: 120_000 }, () => {
  it("share the pass for the bounds and the scroll metrics of what it holds", () => {
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      root.render(
        <Page>
          <div testId="held" style={{ contentVisibility: "hidden", containIntrinsicSize: "30px", flexShrink: 0 } as CSSProperties}>
            <div testId="inner" style={{ height: 50, width: 40, overflowY: "scroll" }}>
              <div testId="inner-child" style={{ height: 200, width: 40, flexShrink: 0 }} />
            </div>
          </div>
        </Page>
      )
      renderer.flush()
      const ids = idsOf(renderer)
      renderer.drawFrame()
      renderer.getBuiltElementBounds(ids.get("held")!)
      const passes = () => renderer.getLayoutPassCount()
      const start = passes()
      const metrics = renderer.getScrollMetrics(ids.get("inner")!)!
      expect(passes() - start, "a scroll metrics read forces the held element").toBe(1)
      expect([metrics[4], metrics[5]], "the client size of the scroller").toEqual([40, 50])
      expect([metrics[2], metrics[3]], "and its extent, which Chrome's scrollHeight is").toEqual([40, 200])
      expect(renderer.getElementBounds(ids.get("inner-child")!)).toMatchObject({ width: 40, height: 200 })
      expect(renderer.getScrollMetrics(ids.get("inner")!)).toEqual(metrics)
      expect(renderer.getElementBounds(ids.get("inner")!)).toMatchObject({ width: 40, height: 50 })
      expect(passes() - start, "and the reads after it share the pass").toBe(1)
      expect(renderer.getBuiltElementBounds(ids.get("inner-child")!), "the element is held still").toBeFalsy()
    } finally {
      root.unmount()
    }
  })
})

describeNative("consecutive reads and a change that moves no box", { timeout: 120_000 }, () => {
  it("a paint-only write and a painted frame that moved no box leave the shared pass, a layout write ends it", () => {
    const tint = motionValue("#ff0000")
    const width = motionValue(60)
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      renderer.clockPause()
      root.render(
        <Page>
          <motion.div testId="tint" style={{ width: 20, height: 20, flexShrink: 0, backgroundColor: tint }} />
          <motion.div testId="sized" style={{ width, height: 20, flexShrink: 0 }} />
          <div testId="held" style={{ contentVisibility: "hidden", containIntrinsicSize: "30px", flexShrink: 0 } as CSSProperties}>
            <div testId="held-child" style={{ height: 70, width: 40, margin: 5 }} />
          </div>
        </Page>
      )
      renderer.flush()
      const ids = idsOf(renderer)
      renderer.drawFrame()
      renderer.getBuiltElementBounds(ids.get("tint")!)
      const passes = () => renderer.getLayoutPassCount()
      const start = passes()
      const child = renderer.getElementBounds(ids.get("held-child")!)
      expect(child).toMatchObject({ width: 40, height: 70 })
      expect(passes() - start, "the first read forces the held element").toBe(1)

      tint.set("#0000ff")
      expect(renderer.getElementBounds(ids.get("held-child")!), "after a colour write").toEqual(child)
      expect(renderer.getScrollMetrics(ids.get("held")!), "and a scroll metrics read").toBeDefined()
      expect(passes() - start, "a paint-only write moves no box: Chrome's layout stays clean, so the read costs no pass").toBe(1)

      width.set(90)
      expect(renderer.getElementBounds(ids.get("sized")!).width).toBe(90)
      expect(renderer.getElementBounds(ids.get("held-child")!)).toEqual(child)
      expect(passes() - start, "a write of a width ends the share: the pass that lays it out and one that forces").toBeGreaterThan(1)
      const forced = passes()
      expect(renderer.getElementBounds(ids.get("held-child")!)).toEqual(child)
      expect(passes() - forced, "and shares again").toBe(0)

      tint.set("#00ff00")
      renderer.drawFrame()
      const framed = passes()
      expect(renderer.getElementBounds(ids.get("held-child")!), "after a painted frame").toEqual(child)
      expect(passes() - framed, "the frame moved no box, so the pass that forced the held element is still the window's").toBe(0)
      renderer.drawFrame()
      expect(renderer.getElementBounds(ids.get("held-child")!), "and after another").toEqual(child)
      expect(passes() - framed).toBe(0)
      expect(renderer.getBuiltElementBounds(ids.get("held-child")!), "the frames skip the held element").toBeFalsy()
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })

  it("a painted frame in which a running animation moved a box ends the shared pass", () => {
    const above = motionValue(20)
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      renderer.clockPause()
      root.render(
        <Page>
          <motion.div testId="above" style={{ width: 20, height: above, flexShrink: 0 }} />
          <div testId="held" style={{ contentVisibility: "hidden", containIntrinsicSize: "30px", flexShrink: 0 } as CSSProperties}>
            <div testId="held-child" style={{ height: 70, width: 40, margin: 5 }} />
          </div>
        </Page>
      )
      renderer.flush()
      const ids = idsOf(renderer)
      for (let step = 0; step < 3; step++) renderer.drawFrame()
      const passes = () => renderer.getLayoutPassCount()
      const child = renderer.getElementBounds(ids.get("held-child")!)!
      renderer.drawFrame()
      const unmoved = passes()
      expect(renderer.getElementBounds(ids.get("held-child")!), "a frame that moved nothing").toEqual(child)
      expect(passes() - unmoved).toBe(0)

      const controls = animate(above, 120, { duration: 1, ease: "linear" })
      renderer.drawFrame()
      for (let step = 0; step < 3; step++) {
        renderer.clockFastForward(200)
        renderer.drawFrame()
        const before = passes()
        const moved = renderer.getElementBounds(ids.get("held-child")!)!
        const read = passes() - before
        const height = renderer.getBuiltElementBounds(ids.get("above")!)!.height
        expect(height, `step ${step}: the animation is running`).toBeGreaterThan(20)
        expect(moved.y - child.y, `step ${step}: the box under the animated one moved with it, as a fresh pass puts it`).toBeCloseTo(height - 20, 1)
        expect(read, `step ${step}: the frame moved a box, so the held element is laid out again`).toBe(1)
      }
      controls.stop()
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })
})

describeNative("consecutive reads and a layout animation that does not move the held element", { timeout: 120_000 }, () => {
  type Box = NonNullable<ReturnType<TestRenderer["getElementBounds"]>>
  const HELD = { contentVisibility: "hidden", containIntrinsicSize: "30px", flexShrink: 0 } as CSSProperties
  const TEXT = { fontSize: 16, lineHeight: "20px", fontFamily: "Menlo", color: "#000000", width: 360 } as const

  /** Runs `frames` frames of the animation, reads `target` after each and gives the passes that read cost. */
  function rounds(renderer: TestRenderer, frames: number, target: () => Box, check: (box: Box) => void) {
    const costs: number[] = []
    for (let frame = 0; frame < frames; frame++) {
      renderer.clockAdvance(200)
      renderer.drawFrame()
      const before = renderer.getLayoutPassCount()
      const box = target()
      costs.push(renderer.getLayoutPassCount() - before)
      check(box)
    }
    return costs
  }

  it("keeps the shared pass while a sibling subtree grows, and a read there stays one pass", () => {
    const grow = motionValue(20)
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      renderer.clockPause()
      root.render(
        <Page>
          <div testId="held" style={HELD}>
            <div testId="held-child" style={{ height: 70, width: 40, margin: 5 }} />
          </div>
          <div style={{ flexShrink: 0 }}>
            <motion.div testId="growing" style={{ width: 20, height: grow, flexShrink: 0 }} />
          </div>
        </Page>
      )
      renderer.flush()
      const ids = idsOf(renderer)
      for (let step = 0; step < 3; step++) renderer.drawFrame()
      const child = renderer.getElementBounds(ids.get("held-child")!)!
      const controls = animate(grow, 120, { duration: 2, ease: "linear" })
      renderer.drawFrame()
      renderer.clockAdvance(200)
      renderer.drawFrame()
      renderer.getElementBounds(ids.get("held-child")!)
      let height = 20
      const costs = rounds(renderer, 4, () => renderer.getElementBounds(ids.get("held-child")!)!, (box) => {
        expect(box, "the held element is where it was").toEqual(child)
        const now = renderer.getBuiltElementBounds(ids.get("growing")!)!.height
        expect(now, "the animation runs").toBeGreaterThan(height)
        height = now
      })
      expect(costs, "the box that grows is after the held element: it moves nothing the pass laid out").toEqual([0, 0, 0, 0])
      controls.stop()
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })

  it("ends the shared pass when the animation moves the held element, an ancestor of it, or resizes it", () => {
    const above = motionValue(20)
    const margin = motionValue(0)
    const width = motionValue(60)
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      renderer.clockPause()
      root.render(
        <Page>
          <motion.div testId="above" style={{ width: 20, height: above, flexShrink: 0 }} />
          <motion.div testId="wrapper" style={{ marginLeft: margin, flexShrink: 0 }}>
            <motion.div testId="held" style={{ ...HELD, width }}>
              <div testId="held-child" style={{ height: 70, width: 40, margin: 5 }} />
            </motion.div>
          </motion.div>
        </Page>
      )
      renderer.flush()
      const ids = idsOf(renderer)
      for (let step = 0; step < 3; step++) renderer.drawFrame()
      const child = renderer.getElementBounds(ids.get("held-child")!)!
      const held = renderer.getElementBounds(ids.get("held")!)!

      const animated = (value: ReturnType<typeof motionValue<number>>, to: number) => animate(value, to, { duration: 2, ease: "linear" })
      const aboveRun = animated(above, 120)
      renderer.drawFrame()
      let costs = rounds(renderer, 3, () => renderer.getElementBounds(ids.get("held-child")!)!, (moved) => {
        expect(moved.y - child.y, "the held element moved down with the box above it").toBeCloseTo(renderer.getBuiltElementBounds(ids.get("above")!)!.height - 20, 1)
      })
      expect(costs, "a box above it moved it: every read is fresh").toEqual([1, 1, 1])
      aboveRun.stop()
      above.set(above.get())
      renderer.drawFrame()
      const settled = renderer.getElementBounds(ids.get("held-child")!)!

      const wrapper = animated(margin, 80)
      renderer.drawFrame()
      costs = rounds(renderer, 3, () => renderer.getElementBounds(ids.get("held-child")!)!, (moved) => {
        expect(moved.x - settled.x, "an ancestor of the held element moved it").toBeCloseTo(margin.get(), 1)
        expect(margin.get()).toBeGreaterThan(0)
      })
      expect(costs, "an ancestor of it moved: every read is fresh").toEqual([1, 1, 1])
      wrapper.stop()
      margin.set(margin.get())
      renderer.drawFrame()

      const own = animated(width, 200)
      renderer.drawFrame()
      costs = rounds(renderer, 3, () => renderer.getElementBounds(ids.get("held-child")!)!, (now) => {
        expect(now.x - settled.x, "the child is as far from the left").toBeCloseTo(margin.get(), 1)
        expect(renderer.getElementBounds(ids.get("held")!)!.width, "the held element is as wide as the animation made it").toBeGreaterThan(held.width)
      })
      expect(costs, "the held element is resized: every read is fresh").toEqual([1, 1, 1])
      own.stop()
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })

  it("is fresh after a change inside an inline-block that moves the held element in it", () => {
    const grow = motionValue(10)
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      renderer.clockPause()
      root.render(
        <Page>
          <text style={TEXT}>
            {"Before the box "}
            <div testId="inline" style={{ display: "inline-block", width: 200, height: 160, flexDirection: "column" }}>
              <motion.div testId="growing" style={{ width: 20, height: grow, flexShrink: 0 }} />
              <div testId="held" style={HELD}>
                <div testId="held-child" style={{ height: 70, width: 40, margin: 5 }} />
              </div>
            </div>
            {" and after it"}
          </text>
        </Page>
      )
      renderer.flush()
      const ids = idsOf(renderer)
      for (let step = 0; step < 3; step++) renderer.drawFrame()
      const child = renderer.getElementBounds(ids.get("held-child")!)!
      renderer.drawFrame()
      const controls = animate(grow, 100, { duration: 2, ease: "linear" })
      renderer.drawFrame()
      const costs = rounds(renderer, 4, () => renderer.getElementBounds(ids.get("held-child")!)!, (moved) => {
        const height = renderer.getBuiltElementBounds(ids.get("growing")!)!.height
        expect(height, "the animation runs").toBeGreaterThan(10)
        expect(moved.y - child.y, "the held element sits under the box that grows in the inline-block").toBeCloseTo(height - 10, 1)
      })
      expect(costs, "the inline-block laid out another box in its own engine: every read is fresh").toEqual([1, 1, 1, 1])
      controls.stop()
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })

  it("is fresh while an ancestor animates the font size, which reflows the held content and moves no box", () => {
    const size = motionValue(16)
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      renderer.clockPause()
      root.render(
        <Page>
          <motion.div testId="wrapper" style={{ fontFamily: "Menlo", fontSize: size, width: 400, height: 200, flexShrink: 0, overflow: "hidden" }}>
            <div testId="held" style={HELD}>
              <div testId="label" style={{ alignSelf: "flex-start" }}>
                <text>Hello world</text>
              </div>
            </div>
            <div testId="control" style={{ alignSelf: "flex-start" }}>
              <text>Hello world</text>
            </div>
          </motion.div>
        </Page>
      )
      renderer.flush()
      const ids = idsOf(renderer)
      for (let step = 0; step < 3; step++) renderer.drawFrame()
      const first = renderer.getElementBounds(ids.get("label")!)!
      const controls = animate(size, 48, { duration: 2, ease: "linear" })
      renderer.drawFrame()
      let height = first.height
      const costs = rounds(renderer, 4, () => renderer.getElementBounds(ids.get("label")!)!, (label) => {
        const control = renderer.getBuiltElementBounds(ids.get("control")!)!
        expect(label.height, "the held text is as tall as a text of the same font size that is not held").toBeCloseTo(control.height, 1)
        expect(label.height, "and the font size grows").toBeGreaterThan(height)
        height = label.height
      })
      expect(costs, "no box moved, and every read is a fresh layout").toEqual([1, 1, 1, 1])
      controls.stop()
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })

  it("shares a pass forced for one held element while an animation elsewhere moves boxes that are not above it", () => {
    const grow = motionValue(20)
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      renderer.clockPause()
      root.render(
        <Page>
          <div testId="held-a" style={HELD}>
            <div testId="child-a" style={{ height: 70, width: 40, margin: 5 }} />
          </div>
          <div testId="held-b" style={HELD}>
            <div testId="child-b" style={{ height: 50, width: 30, margin: 5 }} />
          </div>
          <motion.div testId="elsewhere" style={{ width: 20, height: grow, flexShrink: 0 }} />
        </Page>
      )
      renderer.flush()
      const ids = idsOf(renderer)
      for (let step = 0; step < 3; step++) renderer.drawFrame()
      const passes = () => renderer.getLayoutPassCount()
      const start = passes()
      const a = renderer.getElementBounds(ids.get("child-a")!)!
      expect(passes() - start, "the first read forces the held element").toBe(1)
      const controls = animate(grow, 120, { duration: 2, ease: "linear" })
      renderer.drawFrame()
      renderer.clockAdvance(200)
      renderer.drawFrame()
      renderer.getElementBounds(ids.get("child-a")!)
      const costs = rounds(renderer, 3, () => renderer.getElementBounds(ids.get("child-a")!)!, (box) => expect(box).toEqual(a))
      expect(costs, "the animation is below both held elements").toEqual([0, 0, 0])
      const b = renderer.getElementBounds(ids.get("child-b")!)!
      expect(b.height).toBe(50)
      controls.stop()
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })
})

describeNative("a read of geometry below a skipped element, for what else it could change", { timeout: 120_000 }, () => {
  it("remembers no size, starts no animation and tells nothing, for an auto and a hidden element", () => {
    const seen: string[] = []
    const events: Events = []
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    const Forced = () => (
      <Page>
        <div testId="scroller" style={{ overflowY: "scroll", flexGrow: 1, display: "flex", flexDirection: "column" }}>
          <div testId="held" style={{ contentVisibility: "hidden", containIntrinsicSize: "auto 30px", flexShrink: 0 } as CSSProperties}>
            <div testId="held-child" style={{ height: 70, width: 40, margin: 5 }} />
          </div>
          <div style={{ height: 6000, flexShrink: 0 }} />
          <div
            testId="far"
            onContentVisibilityAutoStateChange={(event) => events.push([0, event.skipped])}
            style={{ contentVisibility: "auto", containIntrinsicSize: "auto 20px", flexShrink: 0 } as CSSProperties}
          >
            <div
              testId="loop"
              onAnimationStart={() => seen.push("start")}
              onAnimationIteration={() => seen.push("iteration")}
              style={{ height: 100, width: 60, animation: "cv-auto-pulse 1s linear infinite" }}
            />
          </div>
        </div>
      </Page>
    )
    try {
      renderer.setInlineDraws(false)
      renderer.clockPause()
      root.render(<Forced />)
      renderer.flush()
      const ids = idsOf(renderer)
      const frame = () => {
        renderer.clockFastForward(100)
        renderer.drawFrame()
        renderer.dispatchNativeEvents()
      }
      for (let step = 0; step < 6; step++) frame()
      const height = (name: string) => renderer.getBuiltElementBounds(ids.get(name)!)!.height
      expect([height("held"), height("far")], "skipped, at the length").toEqual([30, 20])
      events.length = 0

      expect(renderer.getElementBounds(ids.get("loop")!)!.height, "the content is laid out").toBe(100)
      expect(renderer.getElementBounds(ids.get("held-child")!)!.height).toBe(70)
      for (let step = 0; step < 36; step++) frame()
      expect(seen, "no animation started or looped below them").toEqual([])
      expect(events, "nothing is told").toEqual([])
      expect([height("held"), height("far")], "and they did not remember the size they were laid out at").toEqual([30, 20])
      expect(renderer.getBuiltElementBounds(ids.get("loop")!)).toBeFalsy()
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })
})

// ── What is told, and to whom ──

/** One entry of 37px, `auto`, that tells its state into `events`. */
function LoadEntry({ index, events }: { index: number; events: Events }) {
  return <Entry index={index} intrinsic="auto 40px" events={events} />
}

/** The page of the scene `load` of the oracle: an entry on screen, a spacer, an entry far below, and two more once it is `later`. */
function LoadPage({ events, later }: { events: Events; later: boolean }) {
  return (
    <Page>
      <div testId="scroller" style={{ overflowY: "scroll", flexGrow: 1, display: "flex", flexDirection: "column" }}>
        <LoadEntry key={0} index={0} events={events} />
        {later ? <LoadEntry key={2} index={2} events={events} /> : null}
        <div key="spacer" style={{ height: 3000, flexShrink: 0 }} />
        <LoadEntry key={1} index={1} events={events} />
        {later ? <LoadEntry key={3} index={3} events={events} /> : null}
      </div>
    </Page>
  )
}

describeNative("the first frame of a window", { timeout: 120_000 }, () => {
  /** What Chrome tells in the order it tells it, as `[index, skipped]` pairs. */
  const chrome = LOAD.events

  it("tells the elements it has `skipped: true`, and the near ones `false`, as Chrome does for a page at load", () => {
    const events: Events = []
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      root.render(<LoadPage events={events} later={false} />)
      renderer.flush()
      renderer.drawFrame()
      renderer.dispatchNativeEvents()
      // Chrome: the entry on screen and the far one are locked before the first frame, the near one is unlocked by it.
      // There is no animation frame callback here, so both come after the first frame, in the order Chrome has.
      expect(events).toEqual(chrome.slice(0, 3))
      expect(LOAD.log.slice(0, 4), "Chrome: the locks before the first callback, the unlock after it").toEqual([
        "event 0 true",
        "event 1 true",
        "raf 1",
        "event 0 false",
      ])
      settle(renderer, () => "")
      expect(events, "and nothing more").toEqual(chrome.slice(0, 3))
    } finally {
      root.unmount()
    }
  })

  it("tells an element that comes after the first frame once, with its state", () => {
    const events: Events = []
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      root.render(<LoadPage events={events} later={false} />)
      renderer.flush()
      settle(renderer, () => "")
      events.length = 0
      root.render(<LoadPage events={events} later />)
      renderer.flush()
      settle(renderer, () => "")
      expect(events).toEqual(chrome.slice(3))
    } finally {
      root.unmount()
    }
  })

  it("tells an element once when a frame with other content came before it", () => {
    const events: Events = []
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      drawFirstFrame(root, renderer)
      root.render(<LoadPage events={events} later={false} />)
      renderer.flush()
      settle(renderer, () => "")
      // Chrome tells an element that is added after the first frame once.
      expect(events).toEqual([[0, false], [1, true]])
    } finally {
      root.unmount()
    }
  })
})

describeNative("contentvisibilityautostatechange", { timeout: 120_000 }, () => {
  it("bubbles from the element, has a capture twin, and is cancelled to no effect", () => {
    const seen: string[] = []
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    const Tree = ({ far }: { far: boolean }) => (
      <div
        testId="outer"
        style={{ width: SIZE.width, height: SIZE.height, display: "block" }}
        onContentVisibilityAutoStateChangeCapture={(event) => seen.push(`capture ${event.target === event.currentTarget ? "self" : "below"} ${event.skipped}`)}
        onContentVisibilityAutoStateChange={(event) => {
          seen.push(`bubble ${event.target === event.currentTarget ? "self" : "below"} ${event.skipped} ${event.cancelable}`)
          event.preventDefault()
        }}
      >
        <div testId="near" style={{ height: 50, contentVisibility: "auto" } as CSSProperties}>
          <div style={{ height: 30 }} />
        </div>
        {far ? <div style={{ height: 6000 }} /> : null}
        <div testId="far" style={{ height: 50, contentVisibility: "auto" } as CSSProperties}>
          <div style={{ height: 30 }} />
        </div>
      </div>
    )
    try {
      renderer.setInlineDraws(false)
      root.render(<Tree far />)
      renderer.flush()
      settle(renderer, () => "")
      // The elements of the window's first frame are told twice, the near one `true` and then `false`.
      expect(seen.sort()).toEqual(
        [
          "bubble below false true",
          "bubble below true true",
          "bubble below true true",
          "capture below false",
          "capture below true",
          "capture below true",
        ].sort()
      )
    } finally {
      root.unmount()
    }
  })

  it("is not sent for a hidden element, a visible one, or one that changes from auto to visible", () => {
    const seen: string[] = []
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    const Toggled = ({ value }: { value: "visible" | "hidden" | "auto" }) => (
      <div style={{ width: SIZE.width, height: SIZE.height, display: "block" }}>
        <div
          testId="target"
          onContentVisibilityAutoStateChange={(event) => seen.push(`${event.skipped}`)}
          style={{ height: 50, contentVisibility: value } as CSSProperties}
        >
          <div testId="child" style={{ height: 30 }} />
        </div>
      </div>
    )
    try {
      renderer.setInlineDraws(false)
      for (const value of ["visible", "hidden"] as const) {
        root.render(<Toggled value={value} />)
        renderer.flush()
        settle(renderer, () => "")
      }
      expect(seen, "neither says anything").toEqual([])
      root.render(<Toggled value="auto" />)
      renderer.flush()
      settle(renderer, () => "")
      expect(seen, "auto on screen: built, told once").toEqual(["false"])
      root.render(<Toggled value="visible" />)
      renderer.flush()
      settle(renderer, () => "")
      expect(seen, "going back to visible is not told").toEqual(["false"])
    } finally {
      root.unmount()
    }
  })
})

// ── Changing the value ──

describeNative("changing contentVisibility", { timeout: 120_000 }, () => {
  it("builds every entry for visible, none for hidden, and the near ones for auto", () => {
    const events: Events = []
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    const Value = ({ value }: { value: "visible" | "hidden" | "auto" }) => (
      <Page>
        <div testId="scroller" style={{ overflowY: "scroll", flexGrow: 1, display: "flex", flexDirection: "column" }}>
          {Array.from({ length: ENTRIES }, (_, index) => (
            <div
              key={index}
              onContentVisibilityAutoStateChange={(event) => events.push([index, event.skipped])}
              style={{ flexShrink: 0, padding: 8, contentVisibility: value, containIntrinsicSize: "auto 21px" } as CSSProperties}
            >
              <div testId={`bar-${index}`} style={{ height: 21 }} />
            </div>
          ))}
        </div>
      </Page>
    )
    try {
      renderer.setInlineDraws(false)
      root.render(<Value value="visible" />)
      renderer.flush()
      const ids = idsOf(renderer)
      const read = () => JSON.stringify(skippedIn(renderer, ids, ENTRIES))
      const step = (value: "visible" | "hidden" | "auto") => {
        root.render(<Value value={value} />)
        settle(renderer, read)
        return skippedIn(renderer, ids, ENTRIES).length
      }
      expect(step("visible")).toBe(0)
      expect(step("hidden")).toBe(ENTRIES)
      expect(step("auto"), "the entries near the viewport, 37px each").toBe(ENTRIES - 53)
      expect(step("visible")).toBe(0)
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })
})

// ── Work ──

defineKeyframes("cv-auto-pulse", {
  "0%": { color: "#ff0000", backgroundColor: "#ff0000" },
  "100%": { color: "#0000ff", backgroundColor: "#0000ff" },
})

describeNative("a far auto element", { timeout: 120_000 }, () => {
  /** A frame of the page where the element holds nothing, once it has settled: what the loop must not add to. */
  const measureWithoutLoop = () => {
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      renderer.clockPause()
      root.render(
        <Page>
          <div testId="scroller" style={{ overflowY: "scroll", flexGrow: 1, display: "flex", flexDirection: "column" }}>
            <div style={{ height: 6000, flexShrink: 0 }} />
            <div style={{ contentVisibility: "auto", containIntrinsicSize: "auto 20px", flexShrink: 0 } as CSSProperties}>
              <div style={{ height: 20, width: 60 }} />
            </div>
          </div>
        </Page>
      )
      renderer.flush()
      for (let step = 0; step < 36; step++) {
        renderer.clockFastForward(100)
        renderer.drawFrame()
      }
      const before = renderer.getBuildCounters().elementsBuilt
      renderer.clockFastForward(100)
      renderer.drawFrame()
      return renderer.getBuildCounters().elementsBuilt - before
    } finally {
      root.unmount()
    }
  }

  it("builds nothing and runs nothing below it, and goes on from the timeline when it comes near", () => {
    const seen: string[] = []
    const withoutLoop = measureWithoutLoop()
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    const Far = () => (
      <Page>
        <div testId="scroller" style={{ overflowY: "scroll", flexGrow: 1, display: "flex", flexDirection: "column" }}>
          <div style={{ height: 6000, flexShrink: 0 }} />
          <div testId="far" style={{ contentVisibility: "auto", containIntrinsicSize: "auto 20px", flexShrink: 0 } as CSSProperties}>
            <div
              testId="loop"
              onAnimationStart={() => seen.push("start")}
              onAnimationIteration={() => seen.push("iteration")}
              style={{ height: 20, width: 60, animation: "cv-auto-pulse 1s linear infinite" }}
            />
          </div>
        </div>
      </Page>
    )
    try {
      renderer.setInlineDraws(false)
      renderer.clockPause()
      root.render(<Far />)
      renderer.flush()
      const ids = idsOf(renderer)
      const scroller = ids.get("scroller")!
      const frame = () => {
        renderer.clockFastForward(100)
        renderer.drawFrame()
        renderer.dispatchNativeEvents()
      }
      for (let step = 0; step < 36; step++) frame()
      expect(seen, "nothing runs below it while it is far").toEqual([])
      expect(renderer.getBuiltElementBounds(ids.get("loop")!), "the loop is not built").toBeFalsy()
      const perFrame = () => {
        const before = renderer.getBuildCounters().elementsBuilt
        frame()
        return renderer.getBuildCounters().elementsBuilt - before
      }
      expect(perFrame(), "a frame costs what it costs without the loop").toBe(withoutLoop)

      renderer.scrollTo(scroller, 0, 5500)
      for (let step = 0; step < 6; step++) frame()
      expect(seen, "it starts when it is near").toContain("start")
      seen.length = 0
      for (let step = 0; step < 12; step++) frame()
      expect(seen.filter((kind) => kind === "iteration").length, "and loops").toBeGreaterThanOrEqual(1)

      renderer.scrollTo(scroller, 0, 0)
      for (let step = 0; step < 4; step++) frame()
      seen.length = 0
      for (let step = 0; step < 36; step++) frame()
      expect(seen, "nothing runs once it is far again").toEqual([])
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })
})

describeNative("content a read forced keeps its layout across the draws that skip it", { timeout: 120_000 }, () => {
  const frames = (renderer: TestRenderer, count = 3) => {
    for (let index = 0; index < count; index++) {
      renderer.drawFrame()
      renderer.dispatchNativeEvents()
    }
  }

  /** What a read of `names` costs, and the boxes it returns as [x, y, width, height]. */
  function cost(renderer: TestRenderer, ids: Map<string, number>, names: string[]) {
    const before = renderer.getRetainedCounters()
    renderer.resetBuildCounters()
    const read = reading(renderer, ids, names)
    const built = renderer.getBuildCounters().elementsBuilt
    const after = renderer.getRetainedCounters()
    return {
      passes: read.passes,
      built,
      skips: after.layoutSkips - before.layoutSkips,
      translations: after.layoutTranslations - before.layoutTranslations,
      rects: read.read.map((box) => [box!.x, box!.y, box!.width, box!.height]),
    }
  }

  const sameRects = (label: string, seen: number[][], expected: number[][]) =>
    expected.forEach((rect, index) =>
      rect.forEach((value, side) => expect(Math.abs(seen[index]![side]! - value), `${label}: box ${index} is [${seen[index]}], Chrome's [${rect}]`).toBeLessThanOrEqual(UNIT))
    )

  const noMismatches = (renderer: TestRenderer) => {
    if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
  }

  /** What a read costs at most when only the host and the spine are built: the page, the boxes beside the host and the host. A
   *  verified pass builds the page a second time to compare it, which is not the cost being measured. */
  const BUILT_AT_MOST: Record<string, number> = { firstReadKeptInside: 4, firstReadKeptNested: 9, firstReadKeptShown: 4 }
  const SKIPS_AT_LEAST: Record<string, number> = { firstReadKeptInside: 8, firstReadKeptNested: 2, firstReadKeptShown: 8 }

  for (const name of ["firstReadKeptInside", "firstReadKeptNested", "firstReadKeptShown"]) {
    it(`a read after a draw builds the host and the spine, and returns Chrome's boxes: ${name}`, () => {
      const scene = FIRST_READ_SCENES[name]!
      const root = createTestRoot(SIZE)
      const renderer = root.renderer
      try {
        renderer.setInlineDraws(false)
        drawFirstFrame(root, renderer)
        root.render(scene.render(0))
        renderer.flush()
        frames(renderer)
        FIRST_READS[name]!.forEach((expected, step) => {
          if (step > 0) {
            root.render(scene.render(step))
            renderer.flush()
            frames(renderer)
          }
          const label = `${name}, ${expected.step}`
          const ids = idsOf(renderer)
          const shown = scene.skipped ? !scene.skipped(step) : false
          const first = cost(renderer, ids, expected.names!)
          sameRects(`${label}, first read`, first.rects, expected.rects!)
          expect(first.passes, `${label}: the first read is one pass`).toBeLessThanOrEqual(1)
          frames(renderer)
          const kept = cost(renderer, ids, expected.names!)
          sameRects(`${label}, read after a draw that moved no box`, kept.rects, expected.rects!)
          expect(kept.passes, `${label}: the draws moved no box, so the pass of the first read is still the window's`).toBe(0)
          // A layout invalidation that moves nothing ends the share: the read after it is a pass.
          renderer.flush()
          frames(renderer)
          const second = cost(renderer, ids, expected.names!)
          sameRects(`${label}, read after a draw`, second.rects, expected.rects!)
          if (shown) {
            expect(second.passes, `${label}: shown, the draw built the boxes and there is nothing to lay out`).toBe(0)
            expect(Boolean(renderer.getBuiltElementBounds(ids.get(expected.names![0]!)!)), `${label}: and they are built`).toBe(true)
            return
          }
          expect(second.passes, `${label}: the read after the draw is one pass`).toBeLessThanOrEqual(1)
          if (!verifying) {
            expect(second.built, `${label}: and builds the host and the spine`).toBeLessThanOrEqual(BUILT_AT_MOST[name]!)
            expect(second.skips, `${label}: the rest is skipped with its layout records`).toBeGreaterThanOrEqual(SKIPS_AT_LEAST[name]!)
          }
          frames(renderer)
          expect(Boolean(renderer.getBuiltElementBounds(ids.get(expected.names![0]!)!)), `${label}: the content is skipped again`).toBe(false)
        })
        noMismatches(renderer)
      } finally {
        root.unmount()
      }
    })
  }

  it("builds the card a paint-only change inside the content is in, and not the others", () => {
    const scene = FIRST_READ_SCENES.firstReadKeptInside!
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      drawFirstFrame(root, renderer)
      root.render(scene.render(0))
      renderer.flush()
      frames(renderer)
      const ids = idsOf(renderer)
      const names = FIRST_READS.firstReadKeptInside![0]!.names!
      cost(renderer, ids, names)
      frames(renderer)
      root.render(scene.render(1))
      renderer.flush()
      frames(renderer)
      cost(renderer, idsOf(renderer), names)
      frames(renderer)
      root.render(scene.render(2))
      renderer.flush()
      frames(renderer)
      const changed = cost(renderer, idsOf(renderer), names)
      sameRects("recoloured", changed.rects, FIRST_READS.firstReadKeptInside![2]!.rects!)
      if (!verifying) {
        expect(changed.built, "the card with the line, the host and the page").toBeLessThanOrEqual(8)
        expect(changed.skips, "the other cards").toBeGreaterThanOrEqual(7)
      }
      noMismatches(renderer)
    } finally {
      root.unmount()
    }
  })

  it("skips the cards a line that grew moved, where they are now", () => {
    const scene = FIRST_READ_SCENES.firstReadKeptInside!
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      drawFirstFrame(root, renderer)
      root.render(scene.render(0))
      renderer.flush()
      frames(renderer)
      const names = FIRST_READS.firstReadKeptInside![0]!.names!
      cost(renderer, idsOf(renderer), names)
      frames(renderer)
      root.render(scene.render(1))
      renderer.flush()
      frames(renderer)
      // A line of the third card grew by 20: it is built, and the five cards after it only moved.
      const moved = cost(renderer, idsOf(renderer), names)
      sameRects("a line grew", moved.rects, FIRST_READS.firstReadKeptInside![1]!.rects!)
      if (!verifying) {
        expect(moved.built, "the card with the line, the host and the page: the cards after it are not built").toBeLessThanOrEqual(8)
        expect(moved.skips, "and the other seven cards are skipped, the five that moved included").toBeGreaterThanOrEqual(7)
      }
      noMismatches(renderer)
    } finally {
      root.unmount()
    }
  })

  // A scroller the user can scroll (`overflow: scroll`) is excluded from retention, and so is the card around it; one that only
  // script scrolls (`hidden`) keeps a scroll handle and is retained.
  it("puts the text and the scrollers of cards the page moved where a build puts them", () => {
    const CARDS = 6
    const Moved = ({ above }: { above: number }) => (
      <div style={{ width: SIZE.width, height: SIZE.height, display: "flex", flexDirection: "column", fontFamily: "GpuixTestSquare" }}>
        <div style={{ height: above, flexShrink: 0 }} />
        <div testId="held" style={hidden("30px", 4)}>
          {Array.from({ length: CARDS }, (_, card) => (
            <div key={card} testId={`card-${card}`} style={{ margin: 3, padding: 2, flexShrink: 0, width: 160 }}>
              <div testId={`words-${card}`} style={{ flexShrink: 0 }}>
                <text style={{ fontSize: 14 }}>{`Card ${card} has enough words in it to wrap onto more than one line`}</text>
              </div>
              <div testId={`scroller-${card}`} style={{ height: 24 + card, width: 60, overflow: "hidden", flexShrink: 0 }}>
                <div style={{ height: 90, width: 40, flexShrink: 0 }} />
              </div>
            </div>
          ))}
        </div>
      </div>
    )
    const names = Array.from({ length: CARDS }, (_, card) => [`card-${card}`, `words-${card}`, `scroller-${card}`]).flat()
    const read = (renderer: TestRenderer) => {
      const ids = idsOf(renderer)
      const boxes = cost(renderer, ids, names)
      return { ...boxes, metrics: names.filter((name) => name.startsWith("scroller")).map((name) => renderer.getScrollMetrics(ids.get(name)!)) }
    }
    const open = (above: number) => {
      const root = createTestRoot(SIZE)
      root.renderer.registerFont(SQUARE_FONT)
      root.renderer.setInlineDraws(false)
      drawFirstFrame(root, root.renderer)
      root.render(<Moved above={above} />)
      root.renderer.flush()
      return root
    }
    const ABOVE = [52, 20, 52, 7]
    // A window of its own for each: a page that was not laid out before is built whole.
    const built = ABOVE.map((above) => {
      const root = open(above)
      try {
        return read(root.renderer)
      } finally {
        root.unmount()
      }
    })
    const root = open(20)
    try {
      read(root.renderer)
      frames(root.renderer)
      ABOVE.forEach((above, step) => {
        root.render(<Moved above={above} />)
        root.renderer.flush()
        const now = read(root.renderer)
        sameRects(`moved to ${above}`, now.rects, built[step]!.rects)
        expect(now.metrics, `scroll metrics at ${above}`).toEqual(built[step]!.metrics)
        // The draw before the first move found the scrollers new, at their origin: that is not a change, so the records of the
        // cards around them stay and every move, the first included, skips the cards where they are, scrollers and text and all
        // (the first built 21 elements and skipped 6 when a new handle was a change).
        if (!verifying) expect(now.built, `at ${above}: the page and the host, not the cards that only moved`).toBeLessThanOrEqual(4)
        if (!verifying) expect(now.skips, `at ${above}: the cards are skipped`).toBeGreaterThanOrEqual(6)
        frames(root.renderer)
      })
      noMismatches(root.renderer)
    } finally {
      root.unmount()
    }
  })

  // Cards in a scroller viewport that clips (the content mask the cards are under stays where it is) and a box above them that
  // grows: what is culled inside a card does not depend on where the card is while all of it is well inside the viewport.
  it("skips the cards a box above them moved in a viewport that did not, where they are now, unless they reach its edge", () => {
    const CARDS = 8
    const VIEWPORT = 300
    const Moved = ({ above }: { above: number }) => (
      <div style={{ width: SIZE.width, height: SIZE.height, display: "flex", flexDirection: "column", fontFamily: "GpuixTestSquare" }}>
        <div testId="viewport" style={{ width: 400, height: VIEWPORT, overflow: "hidden", flexShrink: 0, margin: 4 }}>
          <div style={{ height: above, flexShrink: 0 }} />
          {Array.from({ length: CARDS }, (_, card) => (
            <div key={card} testId={`card-${card}`} style={{ height: 50, width: 160, marginLeft: 30, flexShrink: 0 }}>
              <div testId={`part-${card}`} style={{ height: 20, width: 100, flexShrink: 0 }} />
              <text style={{ fontSize: 12 }}>{`Card ${card}`}</text>
            </div>
          ))}
        </div>
      </div>
    )
    const names = Array.from({ length: CARDS }, (_, card) => [`card-${card}`, `part-${card}`]).flat()
    const read = (renderer: TestRenderer) => cost(renderer, idsOf(renderer), names)
    const open = (above: number) => {
      const root = createTestRoot(SIZE)
      root.renderer.registerFont(SQUARE_FONT)
      root.renderer.setInlineDraws(false)
      drawFirstFrame(root, root.renderer)
      root.render(<Moved above={above} />)
      root.renderer.flush()
      return root
    }
    /** Culled subtrees one frame skips. */
    const culledByAFrame = (renderer: TestRenderer) => {
      const before = renderer.getCulledSubtreeCount()
      renderer.drawFrame()
      renderer.dispatchNativeEvents()
      return renderer.getCulledSubtreeCount() - before
    }
    // Cards are 50 apart from `above`, in a viewport of 300 from 4, and what culling compares of a card reaches 15 beyond its text:
    // the fifth card is inside the viewport for all of these, the sixth reaches its bottom edge, and the last two are outside it.
    // A draw ends the layout records, so the reads follow each other with none between.
    const ABOVE = [30, 50, 30, 12, 50]
    const built = ABOVE.map((above) => {
      const root = open(above)
      try {
        const boxes = read(root.renderer)
        frames(root.renderer)
        return { boxes, culled: culledByAFrame(root.renderer) }
      } finally {
        root.unmount()
      }
    })
    const root = open(30)
    try {
      read(root.renderer)
      const steps = ABOVE.map((above, step) => {
        root.render(<Moved above={above} />)
        root.renderer.flush()
        const now = read(root.renderer)
        sameRects(`moved to ${above}`, now.rects, built[step]!.boxes.rects)
        return now
      })
      if (!verifying) {
        // Every card but the sixth is put where it is: the five inside the viewport, and the two outside it that are culled both
        // times. The sixth reaches the edge of the viewport before or after the move, and is built. Moved back to where it was
        // recorded (step 2) the cards are where their records are, and none is moved.
        expect(steps.map((now) => now.translations), "cards put where they are now").toEqual([0, 7, 0, 7, 7])
        steps.slice(1).forEach((now, step) => expect(now.built, `step ${step + 1}: the sixth card, the page and the viewport`).toBeLessThanOrEqual(8))
      }
      noMismatches(root.renderer)
    } finally {
      root.unmount()
    }
    // What a frame after the move makes of the culled subtrees is a build's, whichever way the cards were put.
    ABOVE.forEach((above, step) => {
      const root = open(30)
      try {
        read(root.renderer)
        root.render(<Moved above={above} />)
        root.renderer.flush()
        read(root.renderer)
        frames(root.renderer)
        expect(culledByAFrame(root.renderer), `culled subtrees at ${above}`).toBe(built[step]!.culled)
      } finally {
        root.unmount()
      }
    })
  })

  it("keeps the layout of a host that was never drawn: the first draw skips it and the read after it builds the spine", () => {
    const scene = FIRST_READ_SCENES.firstReadKeptInside!
    const expected = FIRST_READS.firstReadKeptInside![0]!
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      drawFirstFrame(root, renderer)
      root.render(scene.render(0))
      renderer.flush()
      const ids = idsOf(renderer)
      const first = cost(renderer, ids, expected.names!)
      sameRects("never drawn", first.rects, expected.rects!)
      expect(first.built, "the first read builds the page and what is in the host").toBeGreaterThanOrEqual(36)
      frames(renderer)
      const kept = cost(renderer, ids, expected.names!)
      sameRects("after the first draw", kept.rects, expected.rects!)
      expect(kept.passes, "the first draw moved no box: the pass of the first read is still the window's").toBe(0)
      renderer.flush()
      frames(renderer)
      const second = cost(renderer, ids, expected.names!)
      sameRects("after a layout invalidation and a draw", second.rects, expected.rects!)
      if (!verifying) {
        expect(second.built).toBeLessThanOrEqual(4)
        expect(second.skips).toBeGreaterThanOrEqual(8)
      }
      noMismatches(renderer)
    } finally {
      root.unmount()
    }
  })

  it("builds the content whole when the host shows again", () => {
    const scene = FIRST_READ_SCENES.firstReadKeptShown!
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      drawFirstFrame(root, renderer)
      root.render(scene.render(0))
      renderer.flush()
      frames(renderer)
      const ids = idsOf(renderer)
      const names = FIRST_READS.firstReadKeptShown![0]!.names!
      cost(renderer, ids, names)
      frames(renderer)
      // The layout the read left is kept. The host shows: the frame builds all of it, and paints it where Chrome has it.
      root.render(scene.render(1))
      renderer.flush()
      renderer.resetBuildCounters()
      frames(renderer, 1)
      expect(renderer.getBuildCounters().elementsBuilt, "the host, its 8 cards and their 24 lines").toBeGreaterThanOrEqual(33)
      const shown = FIRST_READS.firstReadKeptShown![1]!
      sameRects("shown", names.map((name) => {
        const box = renderer.getBuiltElementBounds(idsOf(renderer).get(name)!)!
        return [box.x, box.y, box.width, box.height]
      }), shown.rects!)
      noMismatches(renderer)
    } finally {
      root.unmount()
    }
  })

  it("keeps the boxes of a page of more than 4096 across the draws", () => {
    const CARDS = 1100
    const Big = () => (
      <div style={{ width: SIZE.width, height: SIZE.height, display: "flex", flexDirection: "column" }}>
        <div testId="held" style={hidden("30px", 4)}>
          {Array.from({ length: CARDS }, (_, card) => (
            <div key={card} style={{ margin: 1, flexShrink: 0 }}>
              {Array.from({ length: 3 }, (_, line) => (
                <div key={line} testId={card === 700 ? `line-${line}` : undefined} style={{ height: 4 + line, width: 40, flexShrink: 0 }} />
              ))}
            </div>
          ))}
        </div>
      </div>
    )
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      drawFirstFrame(root, renderer)
      root.render(<Big />)
      renderer.flush()
      frames(renderer)
      const ids = idsOf(renderer)
      const first = cost(renderer, ids, ["line-1"])
      expect(first.rects[0]![3], "laid out").toBe(5)
      for (let draw = 0; draw < 3; draw++) {
        frames(renderer, 2)
        const again = cost(renderer, ids, ["line-1"])
        expect(again.rects, `after a draw (${draw})`).toEqual(first.rects)
      }
      noMismatches(renderer)
    } finally {
      root.unmount()
    }
  })

  it("replays nothing a change under a skipped host made stale: the pointer, a motion value", () => {
    const Page = ({ visibility, width }: { visibility: "hidden" | "visible"; width: ReturnType<typeof motionValue<number>> }) => (
      <LockedShell visibility={visibility}>
        {Array.from({ length: 8 }, (_, group) => (
          <div key={group} testId={`card-${group}`} style={{ margin: 3, padding: 2, flexShrink: 0 }}>
            <motion.div testId={`moved-${group}`} style={{ height: 12, width: group === 3 ? width : 40, margin: 1, flexShrink: 0 }} />
            <div testId={`hovered-${group}`} style={{ height: 12, width: 40, margin: 1, flexShrink: 0, hover: { height: 30 } } as CSSProperties} />
          </div>
        ))}
      </LockedShell>
    )
    const names = ["moved-3", "hovered-3", "moved-7", "hovered-7", "held", "after"]
    const boxes = (renderer: TestRenderer) =>
      names.map((name) => {
        const box = renderer.getBuiltElementBounds(idsOf(renderer).get(name)!)!
        return [box.x, box.y, box.width, box.height]
      })
    const width = motionValue(40)
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    let shown: number[][]
    try {
      renderer.setInlineDraws(false)
      drawFirstFrame(root, renderer)
      root.render(<Page visibility="visible" width={width} />)
      renderer.flush()
      frames(renderer)
      const ids = idsOf(renderer)
      const spot = renderer.getBuiltElementBounds(ids.get("hovered-3")!)!
      // The pointer is on an element of the host when it hides, and the read lays the content out with the element hovered.
      renderer.nativeSimulateMouseMove(spot.x + 5, spot.y + 5)
      frames(renderer)
      expect(renderer.getBuiltElementBounds(ids.get("hovered-3")!)!.height, "hovered").toBe(30)
      root.render(<Page visibility="hidden" width={width} />)
      renderer.flush()
      expect(renderer.getElementBounds(ids.get("hovered-3")!)!.height, "the pass that forces the content lays it out as it is").toBe(30)
      expect(renderer.getElementBounds(ids.get("moved-3")!)!.width).toBe(40)
      // The pointer leaves and a motion value moves while the draws skip the content.
      renderer.nativeSimulateMouseExit(spot.x + 5, 900)
      width.set(70)
      frames(renderer)
      expect(renderer.getElementBounds(ids.get("hovered-3")!)!.height, "the pointer left").toBe(12)
      expect(renderer.getElementBounds(ids.get("moved-3")!)!.width, "the motion value moved").toBe(70)
      noMismatches(renderer)

      root.render(<Page visibility="visible" width={width} />)
      renderer.flush()
      frames(renderer)
      shown = boxes(renderer)
      noMismatches(renderer)
    } finally {
      root.unmount()
    }
    // When the host shows, the content is the one a fresh build has.
    const freshWidth = motionValue(70)
    const fresh = createTestRoot(SIZE)
    try {
      fresh.renderer.setInlineDraws(false)
      drawFirstFrame(fresh, fresh.renderer)
      fresh.render(<Page visibility="visible" width={freshWidth} />)
      fresh.renderer.flush()
      frames(fresh.renderer)
      expect(shown).toEqual(boxes(fresh.renderer))
    } finally {
      fresh.unmount()
    }
  })
})

describeNative("animations in auto content that is skipped", { timeout: 120_000 }, () => {
  const CARDS = 6
  const TICKS = 60

  type Props = { cards: number; visibleLoop?: boolean; control?: boolean; kinds?: string[]; filler?: boolean }

  /** `cards` auto elements 6000px down a scroller, far past what is near, beside a sidebar and under what is always drawn. */
  const Far = ({ cards, visibleLoop, control, kinds, filler }: Props) => (
    <div style={{ width: SIZE.width, height: SIZE.height, display: "flex", flexDirection: "row" }}>
      <Sidebar />
      <div style={{ flex: 1, display: "flex", flexDirection: "column" }}>
        {filler ? <Filler /> : null}
        {visibleLoop ? <VisibleLoop /> : null}
        {control ? <AnimatedCard index={100} /> : null}
        <div testId="scroller" style={{ overflowY: "auto", flexGrow: 1, display: "flex", flexDirection: "column" }}>
          <div style={{ height: 6000, flexShrink: 0 }} />
          {Array.from({ length: cards }, (_, index) => (
            <div key={index} testId={`far-${index}`} style={{ contentVisibility: "auto", containIntrinsicSize: "auto 60px", flexShrink: 0 } as CSSProperties}>
              <AnimatedCard index={index} kinds={kinds} />
            </div>
          ))}
        </div>
      </div>
    </div>
  )

  const mount = (props: Props) => {
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    renderer.clockPause()
    root.render(<Far {...props} />)
    renderer.flush()
    renderer.setInlineDraws(false)
    renderer.setA11yActive(false)
    return { root, renderer }
  }

  /** The loops start in content that is far, or are running when it is scrolled away from. */
  const STARTS = ["far from the start", "running when scrolled away"] as const

  const awayFrom = (renderer: TestRenderer, start: (typeof STARTS)[number]) => {
    if (start === "far from the start") return
    const scroller = renderer.findByTestId("scroller")!.id
    renderer.scrollTo(scroller, 0, 6000, "instant")
    for (let tick = 0; tick < 10; tick++) elapse(renderer)
    renderer.scrollTo(scroller, 0, 0, "instant")
  }

  /** The scrollbar fades for a while after a scroll, and draws. */
  const warm = (start: (typeof STARTS)[number]) => (start === "far from the start" ? 12 : 200)

  const ticksOf = (props: Props, start: (typeof STARTS)[number]) => {
    const page = mount(props)
    try {
      awayFrom(page.renderer, start)
      return spent(page.renderer, TICKS, warm(start))
    } finally {
      page.root.unmount()
    }
  }

  for (const start of STARTS) {
    it(`costs a page what an empty one costs: motion, motion.path and CSS loops, ${start}`, () => {
      const empty = ticksOf({ cards: 0 }, start)
      const full = ticksOf({ cards: CARDS }, start)
      expect(full).toEqual(empty)
      expect(full).toMatchObject({ paintedFrames: 0, patchedFrames: 0, declines: {}, dormantTicks: 0, motionsSynced: 0 })
    })

    it(`adds nothing to the ticks of a loop that is drawn, ${start}`, () => {
      const alone = ticksOf({ cards: 0, visibleLoop: true }, start)
      const beside = ticksOf({ cards: CARDS, visibleLoop: true }, start)
      expect(alone.paintedFrames + alone.patchedFrames, "a frame for each tick of the loop").toBeGreaterThanOrEqual(TICKS - 1)
      expect(beside).toEqual(alone)
    })

    it(`builds for a hover what it builds for cards that run nothing: ${start}`, () => {
      const frames = (kinds: string[]) => {
        const page = mount({ cards: CARDS, kinds, filler: true })
        try {
          awayFrom(page.renderer, start)
          for (let tick = 0; tick < warm(start); tick++) elapse(page.renderer)
          return hovered(page.renderer, 20)
        } finally {
          page.root.unmount()
        }
      }
      const running = frames(["motion", "path", "css"])
      expect(running).toEqual(frames([]))
      // A frame built whole reads the motions it builds, and the motions of skipped content are not.
      expect(running.motionsSynced).toBe(0)
    })
  }

  /** The colour of each loop of a card, at the place where it shows the time of its clock. */
  const looks = (renderer: TestRenderer, card: number, name: string) => {
    const shot = shotOf(renderer, name)
    const box = renderer.getElementBounds(renderer.findByTestId(`card-${card}`)!.id)!
    expect(box.y, `card ${card} is in the window: ${JSON.stringify(box)}`).toBeLessThan(SIZE.height - CARD)
    return [shot.at(box.x + 20, box.y + 20), shot.at(box.x + 40 + 8 + 18, box.y + 12), shot.at(box.x + 40 + 8 + 24 + 8 + 20, box.y + 20)].map(
      (pixel) => pixel.slice(0, 3)
    )
  }

  it("shows each loop at the current instant when it is scrolled near again, and runs them", () => {
    const { root, renderer } = mount({ cards: CARDS, control: true })
    try {
      const scroller = renderer.findByTestId("scroller")!.id
      // The cards are built in the frame after the one that finds them near, and their loops start there.
      const settle = () => {
        for (let frame = 0; frame < 3; frame++) {
          renderer.flush()
          renderer.drawFrame()
          renderer.dispatchNativeEvents()
        }
      }
      renderer.scrollTo(scroller, 0, 6000, "instant")
      settle()
      for (let tick = 0; tick < 10; tick++) elapse(renderer)
      renderer.scrollTo(scroller, 0, 0, "instant")
      for (let tick = 0; tick < 25; tick++) elapse(renderer)
      renderer.scrollTo(scroller, 0, 6000, "instant")
      settle()
      const first = looks(renderer, 3, "first")
      expectSameColours(first, looks(renderer, 100, "first-control"), "when shown")
      for (let tick = 0; tick < 7; tick++) elapse(renderer)
      const later = looks(renderer, 3, "later")
      expect(later, "the loops run again").not.toEqual(first)
      expectSameColours(later, looks(renderer, 100, "later-control"), "later")
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })

  function expectSameColours(shown: number[][], control: number[][], when: string) {
    for (let at = 0; at < shown.length; at++) {
      for (let channel = 0; channel < 3; channel++) {
        expect(Math.abs(shown[at]![channel]! - control[at]![channel]!), `loop ${at}, channel ${channel}, ${when}: ${shown[at]} against ${control[at]}`).toBeLessThanOrEqual(2)
      }
    }
  }
})

describeNative("animations in auto content that is near but clipped away by an overflow hidden scroller", { timeout: 120_000 }, () => {
  const TICKS = 60
  const CARDS = 3
  const CLIP = 120

  /** The cards are `gap` px down a scroller that shows `CLIP`: inside the 150% margin of the window, and outside what the scroller shows once `gap` or the scroll takes them out. */
  const Page = ({ gap, cards, kinds }: { gap: number; cards: number; kinds?: string[] }) => (
    <div style={{ width: SIZE.width, height: SIZE.height, display: "flex", flexDirection: "column" }}>
      <div testId="scroller" style={{ height: CLIP, flexShrink: 0, overflow: "hidden", display: "flex", flexDirection: "column" }}>
        <div style={{ height: gap, flexShrink: 0 }} />
        {Array.from({ length: cards }, (_, index) => (
          <div key={index} testId={`near-${index}`} style={{ contentVisibility: "auto", containIntrinsicSize: "auto 60px", flexShrink: 0 } as CSSProperties}>
            <AnimatedCard index={index} kinds={kinds} />
          </div>
        ))}
        <div style={{ height: 600, flexShrink: 0 }} />
      </div>
    </div>
  )

  type Scene = { name: string; gap: number; then?: (root: ReturnType<typeof createTestRoot>, props: { cards: number; kinds?: string[] }) => void }

  /** The loops run for 10 ticks where the scene begins, drawn or not, and then the scene moves the cards out of the clip. */
  const ABOVE: Scene = {
    name: "visible when the loop began, scrolled out above the clip",
    gap: 20,
    then: (root) => root.renderer.scrollTo(root.renderer.findByTestId("scroller")!.id, 0, 400, "instant"),
  }
  const BELOW: Scene = { name: "below the clip from the start", gap: 400 }
  const PUSHED: Scene = {
    name: "visible when the loop began, pushed out below the clip",
    gap: 20,
    then: (root, props) => root.render(<Page gap={400} {...props} />),
  }

  const ticksOf = (scene: Scene, cards: number, kinds?: string[]) => {
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.clockPause()
      root.render(<Page gap={scene.gap} cards={cards} kinds={kinds} />)
      renderer.flush()
      renderer.setInlineDraws(false)
      renderer.setA11yActive(false)
      for (let tick = 0; tick < 10; tick++) elapse(renderer)
      scene.then?.(root, { cards, kinds })
      const ticks = spent(renderer, TICKS, 12)
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
      return ticks
    } finally {
      root.unmount()
    }
  }

  /** What a page with no cards costs: the cards cost nothing over it. */
  const costsNothing = (scene: Scene, kinds?: string[]) => {
    const empty = ticksOf(scene, 0, kinds)
    const full = ticksOf(scene, CARDS, kinds)
    expect(full).toEqual(empty)
    expect(full).toMatchObject({ paintedFrames: 0, patchedFrames: 0, declines: {}, dormantTicks: 0, motionsSynced: 0 })
  }

  const KINDS = [{ name: "a CSS loop", kinds: ["css"] }, { name: "motion, motion.path and CSS loops", kinds: undefined }]

  describe.each(KINDS)("$name", ({ kinds }) => {
    it(`costs a page what an empty one costs: ${ABOVE.name}`, () => costsNothing(ABOVE, kinds))

    // The mask inside a host the scroller clips away has no area, and a child tested against it is culled whether the host
    // is above the clip or below it. A host below the clip used to be drawn whole every tick: its mask is a line at its top
    // edge (`Bounds::intersect`), the child's box touched it and was not culled, so nothing went dormant and no patch unit
    // was registered (`UnknownUnit`).
    it(`costs a page what an empty one costs: ${BELOW.name}`, () => costsNothing(BELOW, kinds))
    it(`costs a page what an empty one costs: ${PUSHED.name}`, () => costsNothing(PUSHED, kinds))
  })
})

defineAnimationEventTests("auto")

describeNative("a read of a line in 300 hidden cards, after a paint-only write and a painted frame", { timeout: 120_000 }, () => {
  const CARDS = 300
  const Cards = ({ tint }: { tint: ReturnType<typeof motionValue<string>> }) => (
    <Page>
      <motion.div testId="tint" style={{ width: 20, height: 20, flexShrink: 0, backgroundColor: tint }} />
      <div testId="title" style={{ flexShrink: 0 }}>
        Three lines of 300 cards
      </div>
      <div style={{ overflowY: "scroll", flexGrow: 1, display: "flex", flexDirection: "column" }}>
        <div testId="held" style={{ contentVisibility: "hidden", containIntrinsicSize: "30px", flexShrink: 0 } as CSSProperties}>
          {Array.from({ length: CARDS }, (_, card) => (
            <div key={card} testId={`card-${card}`} style={{ margin: 3, padding: 2, flexShrink: 0 }}>
              {Array.from({ length: 3 }, (_, line) => (
                <div key={line} testId={`line-${card}-${line}`} style={{ height: 10 + line * 4, width: 40, margin: 1, flexShrink: 0 }} />
              ))}
            </div>
          ))}
        </div>
      </div>
    </Page>
  )

  it("runs no layout pass: no box moved, so the pass the first read forced is still the window's", () => {
    let expected: ReturnType<TestRenderer["getElementBounds"]> | undefined
    const tint = motionValue("#ff0000")
    const root = createTestRoot(SIZE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      renderer.clockPause()
      root.render(<Cards tint={tint} />)
      renderer.flush()
      const ids = idsOf(renderer)
      for (let index = 0; index < 3; index++) {
        renderer.drawFrame()
        renderer.dispatchNativeEvents()
      }
      const line = ids.get("line-250-1")!
      const first = reading(renderer, ids, ["line-250-1"])
      expect(first.passes, "the first read forces the held element").toBe(1)
      expected = first.read[0]

      const colours = ["#00ff00", "#0000ff", "#ffff00", "#ff00ff", "#00ffff"]
      for (let round = 0; round < 15; round++) {
        tint.set(colours[round % colours.length]!)
        renderer.drawFrame()
        const passes = renderer.getLayoutPassCount()
        expect(renderer.getElementBounds(line), `round ${round}: the box a fresh pass puts it at`).toEqual(first.read[0])
        expect(renderer.getLayoutPassCount() - passes, `round ${round}: no layout pass`).toBe(0)
      }
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
    // The boxes are the ones a window that never drew has.
    const fresh = createTestRoot(SIZE)
    try {
      fresh.renderer.setInlineDraws(false)
      fresh.render(<Cards tint={motionValue("#ff0000")} />)
      fresh.renderer.flush()
      const freshIds = idsOf(fresh.renderer)
      expect(fresh.renderer.getElementBounds(freshIds.get("line-250-1")!)).toEqual(expected)
    } finally {
      fresh.unmount()
    }
  })
})
