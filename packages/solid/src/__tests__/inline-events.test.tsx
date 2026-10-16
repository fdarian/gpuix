/** Events on inline boxes: a span is the target where its glyphs are. */
import fs from "fs"
import os from "os"
import path from "path"
import { describe, expect, it } from "bun:test"
import type { HostElement } from "../host.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const shots = fs.mkdtempSync(path.join(os.tmpdir(), "gpuix-solid-inline-events-"))

const LINE = 20
const TEXT = { fontSize: 16, lineHeight: `${LINE}px`, fontFamily: "Menlo", color: "#000000" } as const

type App = ReturnType<typeof createTestRoot>

/** Menlo has one advance for every character. Measured in a window of its own,
 *  before the one under test is made: a second test renderer stops the first. */
function measureColumn() {
  const app = createTestRoot()
  app.render(() => (
    <div highlight={{ query: "aaaaaaaaaa" }}>
      <text style={TEXT}>{"aaaaaaaaaa"}</text>
    </div>
  ))
  const width = app.renderer.getPaintedHighlights()[0]!.rects[0]!.width / 10
  app.unmount()
  return width
}

const COLUMN = hasNativeTestRenderer ? measureColumn() : 0

function cell(app: App, testId: string, column: number) {
  const box = app.renderer.getElementBounds(app.renderer.findByTestId(testId)!.id)!
  return { x: box.x + (column + 0.5) * COLUMN, y: box.y + LINE / 2 }
}

describe.skipIf(!hasNativeTestRenderer)("Solid events on inline boxes", () => {
  it("targets the span for a click over its glyphs and bubbles to the text", () => {
    const seen: Array<[string, unknown, unknown]> = []
    let span!: HostElement
    let text!: HostElement
    const app = createTestRoot()
    app.render(() => (
      <div style={{ width: 400 }}>
        <text
          ref={text}
          testId="p"
          style={TEXT}
          onClick={(event) => seen.push(["text", event.target, event.currentTarget])}
        >
          Hello <span ref={span} onClick={(event) => seen.push(["span", event.target, event.currentTarget])}>world</span>!
        </text>
      </div>
    ))

    const over = cell(app, "p", 7)
    app.renderer.nativeSimulateClick(over.x, over.y)
    expect(seen).toEqual([
      ["span", span, span],
      ["text", span, text],
    ])

    seen.length = 0
    for (const column of [2, 11, 30]) {
      const away = cell(app, "p", column)
      app.renderer.nativeSimulateClick(away.x, away.y)
    }
    expect(seen).toEqual([
      ["text", text, text],
      ["text", text, text],
      ["text", text, text],
    ])
    app.unmount()
  })

  it("sends enter and leave between the spans of one paragraph", () => {
    const seen: string[] = []
    const record = (name: string) => () => seen.push(name)
    const app = createTestRoot()
    app.render(() => (
      <div style={{ width: 400 }}>
        <text testId="p" style={TEXT} onMouseEnter={record("text enter")} onMouseLeave={record("text leave")}>
          <span onMouseEnter={record("a enter")} onMouseLeave={record("a leave")}>one</span>{" "}
          <span onMouseEnter={record("b enter")} onMouseLeave={record("b leave")}>two</span>
        </text>
      </div>
    ))
    for (const column of [1, 5, 3]) {
      const point = cell(app, "p", column)
      app.renderer.nativeSimulateMouseMove(point.x, point.y)
    }
    expect(seen).toEqual(["text enter", "a enter", "a leave", "b enter", "b leave"])
    app.unmount()
  })

  it("paints the hover style of a span while the pointer is on it", () => {
    const app = createTestRoot()
    app.render(() => (
      <div style={{ width: 400, backgroundColor: "#ffffff" }}>
        <text testId="p" style={TEXT}>
          Hello <span style={{ hover: { color: "#0000ff" } }}>world</span>!
        </text>
      </div>
    ))
    const shot = (name: string) => {
      const file = path.join(shots, `${name}.png`)
      app.renderer.captureScreenshot(file)
      return fs.readFileSync(file)
    }
    const rest = shot("rest")
    const over = cell(app, "p", 8)
    app.renderer.nativeSimulateMouseMove(over.x, over.y)
    expect(shot("over").equals(rest)).toBe(false)

    const away = cell(app, "p", 1)
    app.renderer.nativeSimulateMouseMove(away.x, away.y)
    expect(shot("away").equals(rest)).toBe(true)
    app.unmount()
  })
})
