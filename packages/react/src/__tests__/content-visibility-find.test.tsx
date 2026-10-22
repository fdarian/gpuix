/// Find in a page with skipped `content-visibility`. Chrome's find buffer (`FindBuffer::ShouldIgnoreContents`) leaves out
/// the content of a locked element that find-in-page cannot activate, which is a `hidden` one, and searches the content of
/// an `auto` element whether it is skipped or not; the match that becomes the active one makes the `auto` element around
/// it render (`ActivateFindInPageMatchRangeIfNeeded`). The `highlight` prop is the search here: the total it reports, the
/// match numbering that `activeIndex` follows, and the match that paints as the active one.

import type { CSSProperties } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer, type TestRenderer } from "../testing.js"
import { verifying } from "./retained-harness.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SIZE = { width: 900, height: 780 }
const ENTRIES = 40
const ENTRY = 100
const HIDDEN = 31

type Highlight = { query: string; activeIndex?: number }

type PageProps = { highlight: Highlight; counts: number[]; hidden?: number[] }

function Page({ highlight, counts, hidden = [HIDDEN] }: PageProps) {
  return (
    <div
      highlight={{ query: highlight.query, activeIndex: highlight.activeIndex }}
      onHighlight={(event) => counts.push(event.matchCount ?? -1)}
      style={{ width: SIZE.width, height: SIZE.height, display: "flex", flexDirection: "column", overflow: "hidden" }}
    >
      <div testId="scroller" style={{ overflowY: "scroll", flexGrow: 1, display: "flex", flexDirection: "column" }}>
        {Array.from({ length: ENTRIES }, (_, index) => (
          <div
            key={index}
            testId={`entry-${index}`}
            style={
              {
                height: ENTRY,
                flexShrink: 0,
                contentVisibility: hidden.includes(index) ? "hidden" : "auto",
                containIntrinsicSize: `auto ${ENTRY}px`,
              } as CSSProperties
            }
          >
            <div testId={`mark-${index}`} style={{ height: 1 }} />
            <text>{`Entry ${index} alpha`}</text>
            <text>{index === HIDDEN ? "only the hidden one has a zebra" : `row ${index}`}</text>
          </div>
        ))}
      </div>
    </div>
  )
}

function mount(highlight: Highlight, counts: number[], hidden?: number[]) {
  const root = createTestRoot(SIZE)
  const renderer = root.renderer
  renderer.setInlineDraws(false)
  const show = (next: Highlight, hiding = hidden) => root.render(<Page highlight={next} counts={counts} hidden={hiding} />)
  show(highlight)
  renderer.flush()
  const ids = new Map<string, number>()
  for (const element of renderer.findByType("div")) if (element.testId) ids.set(element.testId, element.id)
  settle(renderer)
  return { root, renderer, ids, show }
}

/** Draws until the frames stop changing what is built: a change takes effect in the frame after the one that found it. */
function settle(renderer: TestRenderer) {
  for (let frames = 0; frames < 8; frames++) {
    renderer.drawFrame()
    renderer.dispatchNativeEvents()
  }
}

const isBuilt = (renderer: TestRenderer, ids: Map<string, number>, index: number) =>
  Boolean(renderer.getBuiltElementBounds(ids.get(`mark-${index}`)!))

const activeTexts = (renderer: TestRenderer) =>
  renderer
    .getPaintedHighlights()
    .filter((hit) => hit.active)
    .map((hit) => hit.text)

function finish(root: ReturnType<typeof createTestRoot>, renderer: TestRenderer) {
  try {
    if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
  } finally {
    root.unmount()
  }
}

describeNative("find in a page with skipped content", { timeout: 120_000 }, () => {
  it("counts the matches in skipped auto entries and none in a hidden one", () => {
    const counts: number[] = []
    const { root, renderer, ids, show } = mount({ query: "alpha", activeIndex: 0 }, counts)
    try {
      expect(isBuilt(renderer, ids, 3), "an entry near the viewport is built").toBe(true)
      expect(isBuilt(renderer, ids, 25), "a far entry is skipped").toBe(false)
      // Chrome finds the text of a skipped `auto` element, not the text of a `hidden` one.
      expect(counts.at(-1)).toBe(ENTRIES - 1)
      show({ query: "zebra", activeIndex: 0 })
      settle(renderer)
      expect(counts.at(-1)).toBe(0)
      show({ query: "row", activeIndex: 0 })
      settle(renderer)
      expect(counts.at(-1)).toBe(ENTRIES - 1)
    } finally {
      finish(root, renderer)
    }
  })

  it("follows a change of content-visibility", () => {
    const counts: number[] = []
    const { root, renderer, show } = mount({ query: "zebra", activeIndex: 0 }, counts)
    try {
      expect(counts.at(-1)).toBe(0)
      show({ query: "zebra", activeIndex: 0 }, [])
      settle(renderer)
      expect(counts.at(-1), "shown, the entry is searched").toBe(1)
      show({ query: "zebra", activeIndex: 0 }, [HIDDEN])
      settle(renderer)
      expect(counts.at(-1), "and left out again").toBe(0)
    } finally {
      finish(root, renderer)
    }
  })

  it("builds the entry of the active match and paints the match as the active one", () => {
    const counts: number[] = []
    const { root, renderer, ids, show } = mount({ query: "alpha", activeIndex: 0 }, counts)
    try {
      expect(activeTexts(renderer)).toEqual(["Entry 0 alpha"])
      expect(isBuilt(renderer, ids, 25)).toBe(false)
      show({ query: "alpha", activeIndex: 25 })
      settle(renderer)
      expect(isBuilt(renderer, ids, 25), "the entry of the active match is built").toBe(true)
      expect(activeTexts(renderer)).toEqual(["Entry 25 alpha"])
      expect(isBuilt(renderer, ids, 24), "its neighbours are not").toBe(false)
      // It stays built for as long as the match is the active one, whatever the frames do.
      for (let frame = 0; frame < 6; frame++) {
        renderer.drawFrame()
        renderer.dispatchNativeEvents()
        expect(isBuilt(renderer, ids, 25), `frame ${frame}`).toBe(true)
      }
      // Past the hidden entry the matches are numbered without it: entry 32 is the match 31.
      show({ query: "alpha", activeIndex: 31 })
      settle(renderer)
      expect(activeTexts(renderer)).toEqual(["Entry 32 alpha"])
      expect(isBuilt(renderer, ids, 32)).toBe(true)
      expect(isBuilt(renderer, ids, 25), "the entry of the match that is not active is skipped again").toBe(false)
    } finally {
      finish(root, renderer)
    }
  })

  it("numbers the built matches by document order when the matches before them are skipped", () => {
    const counts: number[] = []
    const { root, renderer, ids, show } = mount({ query: "alpha" }, counts)
    try {
      renderer.scrollTo(ids.get("scroller")!, 0, 3000)
      settle(renderer)
      expect(isBuilt(renderer, ids, 0), "the first entries are skipped").toBe(false)
      expect(isBuilt(renderer, ids, 38)).toBe(true)
      // The match of entry 38 is the 38th, the hidden entry not counted, and 37 matches come before it.
      show({ query: "alpha", activeIndex: 37 })
      settle(renderer)
      expect(activeTexts(renderer)).toEqual(["Entry 38 alpha"])
      show({ query: "alpha", activeIndex: 33 })
      settle(renderer)
      expect(activeTexts(renderer)).toEqual(["Entry 34 alpha"])
      // A match that is skipped is not painted, so no built match paints as the active one.
      show({ query: "alpha", activeIndex: 2 })
      expect(activeTexts(renderer)).not.toContain("Entry 38 alpha")
    } finally {
      finish(root, renderer)
    }
  })

  it("leaves a hidden entry out of the numbering and never builds it", () => {
    const counts: number[] = []
    const { root, renderer, ids } = mount({ query: "zebra", activeIndex: 0 }, counts)
    try {
      expect(counts.at(-1)).toBe(0)
      expect(isBuilt(renderer, ids, HIDDEN)).toBe(false)
      expect(renderer.getPaintedHighlights()).toEqual([])
    } finally {
      finish(root, renderer)
    }
  })
})
