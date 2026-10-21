/// A page that is mounted again with the text it had is not shaped again, however many frames shaped other text in
/// between: the window keeps what it shaped past the two frames its line cache keeps a line for (a browser's font keeps
/// shape results across frames the same way). A revisit of a page used to shape every line of it again after one frame
/// that looked up other text.

import fs from "fs"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer, type TestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SQUARE_FONT = fs.readFileSync(new URL("./fixtures/gpuix-test-square.ttf", import.meta.url))
const ENTRIES = 190
const SIZE = { width: 900, height: 780 }

const texts = Array.from({ length: ENTRIES }, (_, index) => `Entry ${index}: the sidebar row keeps its fluid hover while the page behind it swaps`)

function Entry({ text }: { text: string }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", padding: 8, gap: 4 }}>
      <div style={{ display: "flex", flexDirection: "row", alignItems: "center", gap: 8 }}>
        <div style={{ width: 16, height: 16, borderRadius: 8, backgroundColor: "#999999" }} />
        <div style={{ display: "flex", flexDirection: "column" }}>
          <text style={{ fontSize: 14, color: "#222222" }}>{text}</text>
        </div>
      </div>
      <div style={{ height: 1, backgroundColor: "#dddddd" }} />
    </div>
  )
}

function Page() {
  return (
    <div style={{ width: SIZE.width, height: SIZE.height, display: "flex", flexDirection: "column", fontFamily: "GpuixTestSquare" }}>
      <div style={{ overflowY: "scroll", flexGrow: 1, display: "flex", flexDirection: "column" }}>
        {texts.map((text) => (
          <Entry key={text} text={text} />
        ))}
      </div>
    </div>
  )
}

/** The tree between two visits: one text that changes every frame, so every frame looks up a line the frame before did not. */
function Elsewhere({ frame }: { frame: number }) {
  return <text style={{ fontSize: 14, fontFamily: "GpuixTestSquare" }}>{`frame ${frame}`}</text>
}

type Step = { ms: number; shapes: number; wrapped: number; hits: number; shapeCacheHits: number }

function draw(renderer: TestRenderer): Step {
  const before = renderer.getLayoutCounters()
  const started = performance.now()
  renderer.flush()
  renderer.drawFrame()
  const ms = performance.now() - started
  const after = renderer.getLayoutCounters()
  return {
    ms,
    shapes: after.shapes - before.shapes,
    wrapped: after.wrappedLayouts - before.wrappedLayouts,
    hits: after.lineLayoutHits - before.lineLayoutHits,
    shapeCacheHits: after.shapeCacheHits - before.shapeCacheHits,
  }
}

describeNative("a page mounted again with the text it had", () => {
  for (const retention of [true, false]) {
    for (const gap of [0, 1, 30]) {
      it(`is not shaped again after ${gap} frames of other text, retention ${retention ? "on" : "off"}`, () => {
        const test = createTestRoot(SIZE)
        const renderer = test.renderer
        renderer.registerFont(SQUARE_FONT)
        try {
          renderer.setInlineDraws(false)
          renderer.setRetainEnabled(retention)
          test.render(<Page />)
          const first = draw(renderer)
          expect(first.shapes, "the first draw shapes every line").toBe(ENTRIES)
          expect(first.shapeCacheHits, "nothing was shaped before").toBe(0)

          test.render(<Elsewhere frame={0} />)
          for (let frame = 1; frame <= gap; frame++) {
            test.render(<Elsewhere frame={frame} />)
            draw(renderer)
          }
          test.render(<Page />)
          const again = draw(renderer)
          console.log(
            `[shape-cache] retention=${retention} gap=${gap}: first ${first.ms.toFixed(1)} ms, remount ${again.ms.toFixed(1)} ms; ` +
              `shapes ${first.shapes} -> ${again.shapes}, wrapped ${first.wrapped} -> ${again.wrapped}, shapeCacheHits ${again.shapeCacheHits}`
          )
          expect(again.shapes).toBe(0)
          expect(again.wrapped).toBe(0)
          if (gap > 0) expect(again.shapeCacheHits, "the layouts the first draw computed are all found again").toBe(first.wrapped)
        } finally {
          test.unmount()
        }
      })
    }
  }
})
