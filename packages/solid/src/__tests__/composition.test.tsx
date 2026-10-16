import { describe, expect, it } from "bun:test"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("Solid composition events", () => {
  it("runs a composition in an input with DOM-shaped key and composition events", () => {
    const log: string[] = []
    const app = createTestRoot()
    app.render(() => (
      <div
        style={{ width: 300, height: 100 }}
        onCompositionStart={(event) => log.push(`parent:start ${JSON.stringify(event.data)}`)}
        onCompositionEndCapture={(event) => log.push(`parent-capture:end ${JSON.stringify(event.data)}`)}
      >
        <input
          testId="field"
          style={{ width: 200, height: 30 }}
          onKeyDown={(event) => log.push(`keydown ${event.key} ${event.isComposing}`)}
          onKeyUp={(event) => log.push(`keyup ${event.key} ${event.isComposing}`)}
          onCompositionStart={(event) => log.push(`start ${JSON.stringify(event.data)}`)}
          onCompositionUpdate={(event) => log.push(`update ${JSON.stringify(event.data)}`)}
          onCompositionEnd={(event) => log.push(`end ${JSON.stringify(event.data)}`)}
        />
      </div>
    ))
    const id = app.renderer.findByTestId("field")!.id
    app.renderer.simulateComposition(id, [
      { key: "k", action: "mark", text: "k" },
      { key: "Enter", action: "commit", text: "か" },
    ])
    expect(log).toEqual([
      "keydown Process false",
      'start ""',
      'parent:start ""',
      'update "k"',
      "keyup k true",
      "keydown Process true",
      'parent-capture:end "か"',
      'end "か"',
      "keyup Enter false",
    ])
  })
})
