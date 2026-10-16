/**
 * Renderer calls a keydown handler makes while native is still dispatching the
 * key, and where an error it throws goes: to `onError`, else reported as
 * uncaught, never thrown from the simulate call.
 */
// @ts-nocheck

import React from "react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { clipboard, window } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

function Scene(props: { onKeyDown: (event) => void }) {
  return (
    <div style={{ width: 400, height: 300 }}>
      <div testId="target" tabIndex={0} style={{ width: 40, height: 20 }} onKeyDown={props.onKeyDown} />
      <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }}>
        <div style={{ height: 500 }} />
      </div>
    </div>
  )
}

describeNative("renderer calls from a keydown handler", () => {
  function mount(onKeyDown: (event, testRoot) => void, options = {}) {
    const testRoot = createTestRoot(options)
    testRoot.render(<Scene onKeyDown={(event) => onKeyDown(event, testRoot)} />)
    const press = () =>
      testRoot.renderer.nativeSimulateKeystrokes(testRoot.renderer.findByTestId("target").id, "x")
    return { testRoot, press }
  }

  it("writes and reads the clipboard", async () => {
    const reads: Array<Promise<string>> = []
    const { press } = mount((_event, testRoot) => {
      testRoot.renderer.writeClipboardText("direct")
      expect(testRoot.renderer.readClipboardText()).toBe("direct")
      reads.push(clipboard.writeText("from handler").then(() => clipboard.readText()))
    })
    press()
    expect(await Promise.all(reads)).toEqual(["from handler"])
  })

  it("scrolls and reads the offset back", () => {
    const seen: unknown[] = []
    const { testRoot, press } = mount((_event, { renderer }) => {
      const id = renderer.findByTestId("scroller").id
      renderer.scrollTo(id, 0, 40)
      seen.push(renderer.getScrollOffset(id))
    })
    press()
    expect(seen).toEqual([[0, 40]])
    expect(testRoot.renderer.getScrollOffset(testRoot.renderer.findByTestId("scroller").id)).toEqual([0, 40])
  })

  it("scrolls to an item and changes window key events", () => {
    const seen: unknown[] = []
    const { testRoot, press } = mount((_event, { renderer }) => {
      renderer.scrollToItem(renderer.findByTestId("scroller").id, 0)
      renderer.setWindowKeyEvents(true, true, 99)
      seen.push(renderer.getAppearance())
    })
    press()
    expect(seen).toEqual([testRoot.renderer.getAppearance()])
  })

  describe("an error a handler throws", () => {
    const reported = vi.fn()
    beforeEach(() => {
      reported.mockClear()
      vi.stubGlobal("reportError", reported)
    })
    afterEach(() => {
      vi.unstubAllGlobals()
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
      expect(errors.map((error) => error.message)).toEqual(["boom 1", "boom 2"])
      expect(reported).not.toHaveBeenCalled()
    })

    it("is reported as uncaught when there is no onError, without throwing from the simulate call", () => {
      const { press } = mount(() => {
        throw new Error("boom")
      })
      expect(press).not.toThrow()
      expect(reported.mock.calls.map(([error]) => error.message)).toEqual(["boom"])
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
      expect(errors.map((error) => error.message)).toEqual(["window capture", "element"])
    })
  })
})
