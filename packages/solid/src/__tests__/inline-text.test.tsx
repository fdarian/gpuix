import { createSignal, Show } from "solid-js"
import { describe, expect, it } from "bun:test"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const LINE = 20
const TEXT = { fontSize: 16, lineHeight: LINE } as const

function boundsOf(app: ReturnType<typeof createTestRoot>, testId: string) {
  const element = app.renderer.findByTestId(testId)
  expect(element).toBeDefined()
  const bounds = app.renderer.getElementBounds(element!.id)
  expect(bounds).not.toBeNull()
  return bounds!
}

describe.skipIf(!hasNativeTestRenderer)("Solid inline text", () => {
  it("accepts a span host element", () => {
    const app = createTestRoot()
    app.render(() => (
      <text>
        a<span>b</span>
      </text>
    ))
    expect(app.renderer.findByType("span")).toHaveLength(1)
  })

  it("lays nested spans out as one line of one string", () => {
    const [name, setName] = createSignal("world")
    const app = createTestRoot()
    app.render(() => (
      <div style={{ width: 600 }}>
        <text testId="p" style={TEXT}>
          Hello <span style={{ color: "#ff0000", fontWeight: 700 }}>big <span style={{ backgroundColor: "#ffff00" }}>{name()}</span></span>!
        </text>
      </div>
    ))

    expect(app.renderer.getPaintedText()).toEqual(["Hello big world!"])
    expect(boundsOf(app, "p").height).toBeLessThanOrEqual(LINE + 2)

    app.flushSync(() => setName("there"))
    expect(app.renderer.getPaintedText()).toEqual(["Hello big there!"])
  })

  it("copies the paragraph without newlines between its runs", () => {
    const app = createTestRoot()
    app.render(() => (
      <div style={{ padding: 20 }}>
        <text style={{ fontSize: 20 }}>
          Hello <span style={{ fontWeight: 700 }}>big</span> world
        </text>
      </div>
    ))

    expect(app.renderer.dragSelect(21, 30, 900, 30)).toBe("Hello big world")
  })

  // Solid makes an empty text node for every dynamic hole. In a flex row with a
  // gap, it must not become an item of its own.
  it("adds no gap slot for a Show hole between gapped flex siblings", () => {
    const [shown, setShown] = createSignal(false)
    const app = createTestRoot()
    app.render(() => (
      <div style={{ display: "flex", flexDirection: "row", gap: 40, ...TEXT }}>
        <div testId="a" style={{ width: 50, height: 10 }} />
        <Show when={shown()}>
          <div testId="mid" style={{ width: 20, height: 10 }} />
        </Show>
        <div testId="b" style={{ width: 50, height: 10 }} />
      </div>
    ))

    expect(boundsOf(app, "b").x - boundsOf(app, "a").x).toBeCloseTo(50 + 40, 0)

    app.flushSync(() => setShown(true))
    expect(boundsOf(app, "b").x - boundsOf(app, "a").x).toBeCloseTo(50 + 40 + 20 + 40, 0)

    app.flushSync(() => setShown(false))
    expect(boundsOf(app, "b").x - boundsOf(app, "a").x).toBeCloseTo(50 + 40, 0)
  })

  it("adds no gap slot for a Show hole holding text between gapped flex siblings", () => {
    const [label, setLabel] = createSignal<string | undefined>(undefined)
    const app = createTestRoot()
    app.render(() => (
      <div style={{ display: "flex", flexDirection: "row", gap: 40, ...TEXT }}>
        <div testId="a" style={{ width: 50, height: 10 }} />
        {label()}
        <div testId="b" style={{ width: 50, height: 10 }} />
      </div>
    ))

    expect(boundsOf(app, "b").x - boundsOf(app, "a").x).toBeCloseTo(50 + 40, 0)

    app.flushSync(() => setLabel("mid"))
    expect(app.renderer.getPaintedText()).toEqual(["mid"])
    expect(boundsOf(app, "b").x - boundsOf(app, "a").x).toBeGreaterThan(50 + 40 + 40)
  })

  it("makes one flex item of a run of text nodes and one per span", () => {
    const app = createTestRoot()
    app.render(() => (
      <div testId="row" style={{ display: "flex", flexDirection: "row", gap: 40, ...TEXT }}>
        left {"text"}
        <span testId="span">span</span>
      </div>
    ))

    expect(app.renderer.getPaintedText()).toEqual(["left text", "span"])
    expect(boundsOf(app, "span").x - boundsOf(app, "row").x).toBeGreaterThan(40 + 20)
  })
})
