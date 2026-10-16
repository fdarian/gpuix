import { describe, expect, it } from "bun:test"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("Solid DOM-shaped key events", () => {
  it("carries DOM key, code and modifier booleans", () => {
    const events: any[] = []
    const app = createTestRoot()
    app.render(() => (
      <div style={{ width: 200, height: 100 }}>
        <div
          testId="target"
          tabIndex={0}
          style={{ width: 40, height: 20 }}
          onKeyDown={(event) => events.push(event)}
        />
      </div>
    ))
    const id = app.renderer.findByTestId("target")!.id
    app.renderer.nativeSimulateKeystrokes(id, "Enter Shift+a Meta+ArrowDown")
    expect(events.map((event) => [event.key, event.code])).toEqual([
      ["Enter", "Enter"],
      ["A", "KeyA"],
      ["ArrowDown", "ArrowDown"],
    ])
    expect(events[1].shiftKey).toBe(true)
    expect(events[2].metaKey).toBe(true)
    expect(events[0].repeat).toBe(false)
  })
})
