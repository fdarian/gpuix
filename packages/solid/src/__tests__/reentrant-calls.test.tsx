import { afterEach, beforeEach, describe, expect, it, spyOn } from "bun:test"
import { clipboard } from "@gpuix/native/host"
import { window } from "../index.js"
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

  describe("an error a handler throws", () => {
    let reported: ReturnType<typeof spyOn>
    beforeEach(() => {
      reported = spyOn(globalThis, "reportError").mockImplementation(() => {})
    })
    afterEach(() => {
      reported.mockRestore()
    })

    it("goes to onError and the dispatch keeps going", () => {
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
      expect(reported).not.toHaveBeenCalled()
    })

    it("is reported as uncaught when there is no onError, without throwing from the simulate call", () => {
      const { press } = mount(() => {
        throw new Error("boom")
      })
      expect(press).not.toThrow()
      expect(reported.mock.calls.map(([error]) => (error as Error).message)).toEqual(["boom"])
    })

    it("does not stop the remaining handlers and window listeners", () => {
      const errors: unknown[] = []
      const seen: string[] = []
      const capture = () => {
        seen.push("window capture")
        throw new Error("window capture")
      }
      const bubble = () => seen.push("window bubble")
      window.addEventListener("keydown", capture, { capture: true })
      window.addEventListener("keydown", bubble)
      try {
        const { press } = mount(
          () => {
            seen.push("element")
            throw new Error("element")
          },
          { onError: (error) => errors.push(error) }
        )
        press()
      } finally {
        window.removeEventListener("keydown", capture, { capture: true })
        window.removeEventListener("keydown", bubble)
      }
      expect(seen).toEqual(["window capture", "element", "window bubble"])
      expect(errors.map((error) => (error as Error).message)).toEqual(["window capture", "element"])
    })
  })
})
