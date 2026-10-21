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
import { defineKeyframes } from "../index"
import { createTestRoot, hasNativeTestRenderer, type TestRenderer } from "../testing.js"
import { verifying } from "./retained-harness.js"

const SQUARE_FONT = fs.readFileSync(new URL("./fixtures/gpuix-test-square.ttf", import.meta.url))

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")
interface OracleStep {
  step: string
  skipped?: number[]
  /** For each section, how many of its rows are built. */
  rows?: number[]
  sizes?: number[]
  focused?: number
  height: number
  top: number
  events: [number, boolean][]
}
const ORACLE: Record<string, OracleStep[]> = JSON.parse(
  fs.readFileSync(path.join(FIXTURES, "content-visibility-auto-oracle.json"), "utf8")
)

const ENTRIES = 190
const SIZE = { width: 900, height: 780 }
const UNIT = 1 / 64

type Events = [number, boolean][]

/** An entry of 37px: padding 8, a bar of 16, a gap of 4, a rule of 1. */
function Entry({ index, intrinsic, events, focusable }: { index: number; intrinsic: string; events: Events; focusable?: boolean }) {
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
      <div testId={`bar-${index}`} tabIndex={focusable ? -1 : undefined} style={{ height: 16, flexShrink: 0 }} />
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
  rows?: number[]
  sizes?: number[]
  focused?: number
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

const built = (renderer: TestRenderer, ids: Map<string, number>, name: string) => Boolean(renderer.getElementBounds(ids.get(name)!))
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

const SCENES: Record<string, Scene> = {
  // The intrinsic size is not the size the entries have, so what is near moves as they are laid out.
  list: listScene("auto 40px", [1000, 4000, 7000, 0]),
  exact: listScene("auto 21px", [1000, 4000, 7000, 0]),
  // What clips an entry cuts the margin short.
  clipped: listScene("auto 21px", [2000, 0], 400),
  // No `contain-intrinsic-size`: `auto` is in effect all the same, so a skipped entry keeps the size it had.
  bare: listScene("none", [3000, 0]),
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

function TextEntry({ index, value, events }: { index: number; value: "visible" | "auto"; events: Events }) {
  return (
    <div
      onContentVisibilityAutoStateChange={(event) => events.push([index, event.skipped])}
      style={{ display: "flex", flexDirection: "column", flexShrink: 0, padding: 8, gap: 4, contentVisibility: value, containIntrinsicSize: "auto 40px" } as CSSProperties}
    >
      <div style={{ display: "flex", flexDirection: "row", alignItems: "center", gap: 8 }}>
        <div testId={`bar-${index}`} style={{ width: 16, height: 16, borderRadius: 8, backgroundColor: "#999999" }} />
        <div style={{ display: "flex", flexDirection: "column" }}>
          <text style={{ fontSize: 14, color: "#222222" }}>{`Entry ${index}: the sidebar row keeps its fluid hover while the page behind it swaps`}</text>
        </div>
        <div style={{ display: "flex", flexDirection: "row", gap: 2 }}>
          <div style={{ width: 6, height: 6, backgroundColor: "#dddddd" }} />
          <div style={{ width: 6, height: 6, backgroundColor: "#dddddd" }} />
        </div>
      </div>
      <div style={{ height: 1, backgroundColor: "#dddddd" }} />
    </div>
  )
}

function TextPage({ value, events }: { value: "visible" | "auto"; events: Events }) {
  return (
    <div style={{ width: SIZE.width, height: SIZE.height, display: "flex", flexDirection: "column", fontFamily: "GpuixTestSquare" }}>
      <div testId="scroller" style={{ overflowY: "scroll", flexGrow: 1, display: "flex", flexDirection: "column" }}>
        {Array.from({ length: ENTRIES }, (_, index) => (
          <TextEntry key={index} index={index} value={value} events={events} />
        ))}
      </div>
    </div>
  )
}

// ── What is told, and to whom ──

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
      expect(seen.sort()).toEqual(
        ["bubble below false true", "bubble below true true", "capture below false", "capture below true"].sort()
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
      expect(renderer.getElementBounds(ids.get("loop")!), "the loop is not built").toBeFalsy()
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
