import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const LINE = 20
const TEXT = { fontSize: 16, lineHeight: `${LINE}px`, fontFamily: "Menlo" } as const

type App = ReturnType<typeof createTestRoot>

function lines(app: App, testId: string) {
  const element = app.renderer.findByTestId(testId)
  expect(element).toBeDefined()
  const bounds = app.renderer.getElementBounds(element!.id)
  expect(bounds).not.toBeNull()
  return Math.round(bounds!.height / LINE)
}

function paint(view: () => any) {
  const app = createTestRoot()
  app.render(view)
  return app
}

describe.skipIf(!hasNativeTestRenderer)("Solid white-space", () => {
  it("collapses spaces, tabs and line feeds, and a line feed is a space", () => {
    const app = paint(() => (
      <div testId="p" style={{ width: 400, ...TEXT }}>
        {"a   b\t\tc\n\n  d\ne"}
      </div>
    ))
    expect(app.renderer.getPaintedText()).toEqual(["a b c d e"])
    expect(lines(app, "p")).toBe(1)
  })

  it("collapses across span edges and strips the paragraph's ends, as a signal changes", () => {
    const [word, setWord] = createSignal("  b  ")
    const app = paint(() => (
      <text testId="p" style={{ width: 400, ...TEXT }}>
        {"  \n a  "}
        <span style={{ fontWeight: 700 }}>{word()}</span>
        {"  c \n"}
      </text>
    ))
    expect(app.renderer.getPaintedText()).toEqual(["a b c"])

    app.flushSync(() => setWord("   "))
    expect(app.renderer.getPaintedText()).toEqual(["a c"])

    app.flushSync(() => setWord(" \n b\t"))
    expect(app.renderer.getPaintedText()).toEqual(["a b c"])
    expect(lines(app, "p")).toBe(1)
  })

  it("nowrap collapses and does not wrap", () => {
    const app = paint(() => (
      <div testId="p" style={{ width: 100, whiteSpace: "nowrap", ...TEXT }}>
        {"alpha   beta \n gamma   delta"}
      </div>
    ))
    expect(app.renderer.getPaintedText()).toEqual(["alpha beta gamma delta"])
    expect(lines(app, "p")).toBe(1)
  })

  it("pre keeps spaces, breaks at line feeds and does not wrap", () => {
    const app = paint(() => (
      <div testId="p" style={{ width: 80, whiteSpace: "pre", ...TEXT }}>
        {"  a  b\n    a very long second line\n"}
      </div>
    ))
    expect(app.renderer.getPaintedText()).toEqual(["  a  b\n    a very long second line"])
    expect(lines(app, "p")).toBe(2)
  })

  it("pre-wrap keeps spaces and wraps", () => {
    const app = paint(() => (
      <div testId="p" style={{ width: 100, whiteSpace: "pre-wrap", ...TEXT }}>
        {"alpha  beta  gamma\ndelta"}
      </div>
    ))
    expect(app.renderer.getPaintedText()).toEqual(["alpha  beta  gamma\ndelta"])
    expect(lines(app, "p")).toBeGreaterThan(2)
  })

  it("pre-line collapses spaces and keeps line feeds", () => {
    const app = paint(() => (
      <div testId="p" style={{ width: 400, whiteSpace: "pre-line", ...TEXT }}>
        {"  a   b  \n   c\n\nd  "}
      </div>
    ))
    expect(app.renderer.getPaintedText()).toEqual(["a b\nc\n\nd"])
    expect(lines(app, "p")).toBe(4)
  })

  it("inherits white-space, and a descendant or a span can override it", () => {
    const app = paint(() => (
      <div style={{ width: 400, whiteSpace: "pre", ...TEXT }}>
        <div>{"a  b"}</div>
        <div style={{ whiteSpace: "normal" }}>{"a  b"}</div>
        <text>
          {"c  "}
          <span style={{ whiteSpace: "normal" }}>{"d   e"}</span>
        </text>
      </div>
    ))
    expect(app.renderer.getPaintedText()).toEqual(["a  b", "a b", "c  d e"])
  })

  it("follows a change of the container", () => {
    const [mode, setMode] = createSignal<"normal" | "pre">("normal")
    const app = paint(() => (
      <div style={{ width: 400, whiteSpace: mode(), ...TEXT }}>{"a   b"}</div>
    ))
    expect(app.renderer.getPaintedText()).toEqual(["a b"])
    app.flushSync(() => setMode("pre"))
    expect(app.renderer.getPaintedText()).toEqual(["a   b"])
  })

  describe("search, selection and lookup after collapsing", () => {
    const view = (query: string) => () => (
      <div style={{ width: 600 }} highlight={{ query }}>
        <text style={{ ...TEXT, fontSize: 20 }}>
          {"alpha   \n  "}
          <span style={{ fontWeight: 700 }}>{"  beta  "}</span>
          {"   gamma"}
          <span style={{ backgroundColor: "#ffff00" }}>{"   \n  delta  "}</span>
        </text>
      </div>
    )

    it("matches the collapsed text across a span edge, and not the raw one", () => {
      const hits = paint(view("a beta g")).renderer.getPaintedHighlights()
      expect(hits).toHaveLength(1)
      expect(hits[0].text).toBe("alpha beta gamma delta")
      expect(hits[0].text.slice(hits[0].start, hits[0].end)).toBe("a beta g")
      expect(paint(view("alpha   ")).renderer.getPaintedHighlights()).toEqual([])
    })

    it("selects a later span from where it is painted", () => {
      const app = paint(view("delta"))
      const rect = app.renderer.getPaintedHighlights()[0].rects[0]
      const y = rect.y + rect.height / 2
      expect(app.renderer.dragSelect(rect.x + 1, y, rect.x + rect.width + 4, y)).toBe("delta")
    })

    it("copies the collapsed paragraph", () => {
      const app = paint(view("alpha"))
      expect(app.renderer.dragSelect(1, 10, 590, 10)).toBe("alpha beta gamma delta")
    })

    it("finds an element by its collapsed text", () => {
      const app = paint(() => <text style={TEXT}>{"hello  \n   world"}</text>)
      expect(app.renderer.findByText("hello world")).toBeDefined()
    })
  })
})
