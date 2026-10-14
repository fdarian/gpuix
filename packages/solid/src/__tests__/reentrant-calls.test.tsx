import { describe, expect, it } from "bun:test"
import { clipboard } from "@gpuix/native/host"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRendererOptions } from "../testing.js"

function mount(onKeyDown: (renderer: ReturnType<typeof createTestRoot>["renderer"]) => void, options: TestRendererOptions = {}) {
  const app = createTestRoot(options)
  app.render(() => (
    <div style={{ width: 400, height: 300 }}>
      <div testId="target" tabIndex={0} style={{ width: 40, height: 20 }} onKeyDown={() => onKeyDown(app.renderer)} />
      <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }}>
        <div style={{ height: 500 }} />
      </div>
    </div>
  ))
  const press = () => app.renderer.nativeSimulateKeystrokes(app.renderer.findByTestId("target")!.id, "x")
  return { app, press }
}

describe.skipIf(!hasNativeTestRenderer)("renderer calls from a keydown handler", () => {
  it("writes and reads the clipboard", async () => {
    const reads: Array<Promise<string>> = []
    const { press } = mount((renderer) => {
      renderer.writeClipboardText("direct")
      expect(renderer.readClipboardText()).toBe("direct")
      reads.push(clipboard.writeText("from handler").then(() => clipboard.readText()))
    })
    press()
    expect(await Promise.all(reads)).toEqual(["from handler"])
  })

  it("scrolls and reads the offset back", () => {
    const seen: unknown[] = []
    const { app, press } = mount((renderer) => {
      const id = renderer.findByTestId("scroller")!.id
      renderer.scrollTo(id, 0, -40)
      seen.push(renderer.getScrollOffset(id))
    })
    press()
    expect(seen).toEqual([[0, -40]])
    expect(app.renderer.getScrollOffset(app.renderer.findByTestId("scroller")!.id)).toEqual([0, -40])
  })

  it("sends an error the handler throws to onError and keeps dispatching", () => {
    const errors: unknown[] = []
    let calls = 0
    const { press } = mount(
      () => {
        calls += 1
        throw new Error(`boom ${calls}`)
      },
      { onError: (error) => errors.push(error) }
    )
    press()
    press()
    expect(errors.map((error) => (error as Error).message)).toEqual(["boom 1", "boom 2"])
  })

  it("throws from the simulate call when there is no onError", () => {
    const { press } = mount(() => {
      throw new Error("boom")
    })
    expect(press).toThrow("boom")
  })
})
