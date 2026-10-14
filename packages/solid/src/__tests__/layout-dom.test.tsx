import { createSignal, For, Show } from "solid-js"
import { describe, expect, it } from "bun:test"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"

function boundsOf(app: TestRoot, testId: string) {
  return app.renderer.getElementBounds(app.renderer.findByTestId(testId)!.id)!
}

describe.skipIf(!hasNativeTestRenderer)("DOM layout (solid)", () => {
  describe("text nodes", () => {
    it("empty placeholders from Show and For take no gap slot", () => {
      const [show, setShow] = createSignal(false)
      const [items, setItems] = createSignal<string[]>([])
      const app = createTestRoot()
      try {
        app.render(() => (
          <div style={{ display: "flex", gap: 10 }}>
            <div testId="a" style={{ width: 20, height: 20 }} />
            <Show when={show()}>
              <div testId="shown" style={{ width: 20, height: 20 }} />
            </Show>
            <For each={items()}>{() => <div style={{ width: 20, height: 20 }} />}</For>
            <div testId="b" style={{ width: 20, height: 20 }} />
          </div>
        ))
        expect(boundsOf(app, "b").x - boundsOf(app, "a").x).toBe(30)

        app.flushSync(() => setShow(true))
        expect(boundsOf(app, "b").x - boundsOf(app, "shown").x).toBe(30)
        expect(boundsOf(app, "shown").x - boundsOf(app, "a").x).toBe(30)

        app.flushSync(() => {
          setShow(false)
          setItems(["x"])
        })
        expect(boundsOf(app, "b").x - boundsOf(app, "a").x).toBe(60)
      } finally {
        app.unmount()
      }
    })

    it("white space between elements makes no flex item, inside <text> it is kept", () => {
      const app = createTestRoot()
      try {
        app.render(() => (
          <div style={{ display: "flex", gap: 10 }}>
            <div testId="a" style={{ width: 20, height: 20 }} />
            {" "}
            <div testId="b" style={{ width: 20, height: 20 }} />
            <text>
              {"x"}
              {" "}
              {"y"}
            </text>
          </div>
        ))
        expect(boundsOf(app, "b").x - boundsOf(app, "a").x).toBe(30)
        expect(app.renderer.getAllText()).toContain(" ")
      } finally {
        app.unmount()
      }
    })
  })
})
