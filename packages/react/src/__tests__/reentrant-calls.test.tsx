/**
 * Renderer calls a keydown handler makes while native is still dispatching the
 * key, and what happens to an error it throws.
 */
// @ts-nocheck

import React from "react"
import { describe, expect, it } from "vitest"
import { clipboard } from "../index"
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
      renderer.scrollTo(id, 0, -40)
      seen.push(renderer.getScrollOffset(id))
    })
    press()
    expect(seen).toEqual([[0, -40]])
    expect(testRoot.renderer.getScrollOffset(testRoot.renderer.findByTestId("scroller").id)).toEqual([0, -40])
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
    expect(errors.map((error) => error.message)).toEqual(["boom 1", "boom 2"])
  })

  it("throws from the simulate call when there is no onError", () => {
    const { press } = mount(() => {
      throw new Error("boom")
    })
    expect(press).toThrow("boom")
  })
})
