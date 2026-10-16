import { describe, expect, it } from "bun:test"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("Solid input event", () => {
  it("onChange carries inputType, data and isComposing", () => {
    const edits: unknown[] = []
    const app = createTestRoot()
    app.render(() => (
      <textarea
        testId="field"
        style={{ width: 200, height: 60 }}
        onChange={(event) => edits.push([event.inputType, event.data, event.value, event.isComposing])}
      />
    ))
    const id = app.renderer.findByTestId("field")!.id
    app.renderer.nativeSimulateKeystrokes(id, "a Enter Backspace Meta+z")
    app.renderer.simulateComposition(id, [{ key: "k", action: "mark", text: "か" }])
    expect(edits).toEqual([
      ["insertText", "a", "a", false],
      ["insertLineBreak", null, "a\n", false],
      ["deleteContentBackward", null, "a", false],
      ["historyUndo", null, "a\n", false],
      ["insertCompositionText", "か", "aか", true],
    ])
  })
})
