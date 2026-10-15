import { For, Show, createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const box = { width: 100, height: 40, backgroundColor: "#333333" } as const

function tick(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0))
}

describe.skipIf(!hasNativeTestRenderer)("Solid removal-only updates", () => {
  it("takes a Show that a timer turns off out of paint and hit testing", async () => {
    const [shown, setShown] = createSignal(true)
    let clicks = 0
    const app = createTestRoot()
    app.render(() => (
      <div>
        <Show when={shown()}>
          <div testId="panel" onClick={() => clicks++} style={box}>
            <text>panel</text>
          </div>
        </Show>
      </div>
    ))
    const panel = app.renderer.findByTestId("panel")!
    const bounds = app.renderer.getElementBounds(panel.id)!
    const x = bounds.x + bounds.width / 2
    const y = bounds.y + bounds.height / 2
    expect(app.renderer.getElementsAt(x, y)).toContain(panel.id)

    setTimeout(() => setShown(false), 0)
    await tick()
    await tick()

    expect(app.renderer.getAllText()).toEqual([])
    expect(app.renderer.findByTestId("panel")).toBeUndefined()
    expect(app.renderer.getElementsAt(x, y)).not.toContain(panel.id)
    app.renderer.nativeSimulateClick(x, y)
    expect(clicks).toBe(0)
  })

  it("drops the items a For loses outside an event boundary", async () => {
    const [names, setNames] = createSignal(["a", "b", "c"])
    const app = createTestRoot()
    app.render(() => (
      <div>
        <For each={names()}>{(name) => <text>{name}</text>}</For>
      </div>
    ))
    expect(app.renderer.getAllText()).toEqual(["a", "b", "c"])

    setTimeout(() => setNames(["b"]), 0)
    await tick()
    await tick()

    expect(app.renderer.getAllText()).toEqual(["b"])
  })
})
