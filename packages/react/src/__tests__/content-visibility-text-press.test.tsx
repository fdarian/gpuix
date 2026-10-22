/// A press on the text of a page of `content-visibility: auto` sections. The registry a press is resolved against holds the
/// texts the last frame painted, and the window lays out and prepaints again between frames (a read of a box, the
/// dispatch of the press itself) without painting: a text that registry holds must keep the box it was prepainted in
/// through a pass that places it again and does not prepaint it, as a skipped subtree is.

import fs from "fs"
import { memo, type CSSProperties } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer, type TestRenderer } from "../testing.js"
import { verifying } from "./retained-harness.js"

const SQUARE_FONT = fs.readFileSync(new URL("./fixtures/gpuix-test-square.ttf", import.meta.url))

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SIZE = { width: 900, height: 780 }
const OUTERS = 24
const INNERS = 20

const Section = memo(function Section({ outer }: { outer: number }) {
  return (
    <div
      style={{ display: "flex", flexDirection: "column", flexShrink: 0, paddingTop: 12, contentVisibility: "auto", containIntrinsicSize: "auto 800px" } as CSSProperties}
    >
      <text style={{ fontSize: 20 }}>{`Section ${outer}`}</text>
      {Array.from({ length: INNERS }, (_, inner) => (
        <div
          key={inner}
          style={{ display: "flex", flexDirection: "row", gap: 8, padding: 6, contentVisibility: "auto", containIntrinsicSize: "auto 40px" } as CSSProperties}
        >
          <div testId={`bar-${outer}-${inner}`} style={{ width: 16, height: 16, backgroundColor: "#999999" }} />
          <text style={{ fontSize: 14 }}>{`Card ${outer}.${inner}: a row of the demo page that is nested in a section`}</text>
        </div>
      ))}
    </div>
  )
})

/** The sections are memoized: a change of `side` or `height` changes the layout around them and not the sections. */
function Page({ side = 0, height = SIZE.height }: { side?: number; height?: number }) {
  return (
    <div style={{ width: SIZE.width, height, display: "flex", flexDirection: "row", fontFamily: "GpuixTestSquare" }}>
      <div style={{ width: side, flexShrink: 0 }} />
      <div testId="scroller" style={{ overflowY: "scroll", flexGrow: 1, display: "flex", flexDirection: "column" }}>
        {Array.from({ length: OUTERS }, (_, outer) => (
          <Section key={outer} outer={outer} />
        ))}
      </div>
    </div>
  )
}

function open() {
  const root = createTestRoot(SIZE)
  const renderer = root.renderer
  renderer.registerFont(SQUARE_FONT)
  renderer.setInlineDraws(false)
  root.render(<Page />)
  renderer.flush()
  const ids = new Map<string, number>()
  for (const element of renderer.findByType("div")) if (element.testId) ids.set(element.testId, element.id)
  return { root, renderer, ids }
}

const frame = (renderer: TestRenderer) => {
  renderer.drawFrame()
  renderer.dispatchNativeEvents()
}

/** Presses down the page, drags a little, and lets go. */
function press(renderer: TestRenderer) {
  for (const y of [12, 100, 400, 700, 770]) {
    renderer.nativeSimulateMouseDown(60, y)
    renderer.nativeSimulateMouseMove(300, y + 30)
    renderer.nativeSimulateMouseUp(300, y + 30)
  }
}

describeNative("a press on the text of a page of auto sections", { timeout: 120_000 }, () => {
  it("resolves against the texts the last frame painted, as scrolls and reads skip and force the sections", () => {
    const { root, renderer, ids } = open()
    try {
      for (let index = 0; index < 6; index++) frame(renderer)
      press(renderer)
      for (const top of [500, 3000, 8000, 3300, 0, 20, 5000]) {
        renderer.scrollTo(ids.get("scroller")!, 0, top)
        press(renderer)
        frame(renderer)
        press(renderer)
      }
      for (const [outer, inner] of [[9, 12], [4, 3], [20, 0]]) {
        renderer.getElementBounds(ids.get(`bar-${outer}-${inner}`)!)
        press(renderer)
        frame(renderer)
        press(renderer)
      }
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })

  it("keeps the box of a text that a pass places again and does not prepaint: the layout around the sections changes", () => {
    const { root, renderer, ids } = open()
    try {
      for (let index = 0; index < 6; index++) frame(renderer)
      // Reads inside two skipped sections, a pointer over the page and a change of the sidebar and of the height of the
      // window between them: the passes the reads and the press run place the texts of the sections near the top again
      // with the answer they had, from the probes of the layout around them, and skip the sections.
      renderer.getElementBounds(ids.get("bar-9-12")!)
      renderer.nativeSimulateMouseMove(100, 392)
      root.render(<Page side={140} height={551} />)
      renderer.flush()
      renderer.getElementBounds(ids.get("bar-6-1")!)
      root.render(<Page side={277} height={695} />)
      renderer.flush()
      press(renderer)
      frame(renderer)
      press(renderer)
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })
})
