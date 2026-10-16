/**
 * `target`, `currentTarget` and `relatedTarget` are element objects, the same
 * ones a `ref` gives, for every event. `elementId` stays the numeric id.
 */
import React from "react"
import { afterEach, beforeEach, describe, expect, it } from "vitest"
import { document, window, type DomEvent } from "@gpuix/native/host"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("event targets are elements", () => {
  let testRoot: ReturnType<typeof createTestRoot>
  const cleanups: Array<() => void> = []

  beforeEach(() => {
    testRoot = createTestRoot()
  })

  afterEach(() => {
    for (const cleanup of cleanups.splice(0)) cleanup()
  })

  function listen(
    target: typeof window,
    type: string,
    listener: (event: DomEvent) => void,
    options?: { capture: boolean }
  ) {
    target.addEventListener(type, listener, options)
    cleanups.push(() => target.removeEventListener(type, listener, options))
  }

  const box = { width: 40, height: 20 }

  it("gives focus, blur, focusin and focusout the focused elements as target and relatedTarget", () => {
    let a: unknown
    let b: unknown
    const seen: Array<{ type: string; target: unknown; relatedTarget: unknown }> = []
    const record = (event: DomEvent) =>
      seen.push({ type: event.type, target: event.target, relatedTarget: event.relatedTarget })
    testRoot.render(
      <div style={{ width: 200, height: 100 }}>
        <div
          ref={(element) => (a = element)}
          testId="a"
          tabIndex={0}
          style={box}
          onFocus={record}
          onBlur={record}
        />
        <div
          ref={(element) => (b = element)}
          testId="b"
          tabIndex={0}
          style={box}
          onFocus={record}
          onBlur={record}
        />
      </div>
    )
    // React's onFocus and onBlur are focusin and focusout; focus and blur do
    // not bubble, so the window hears them in the capture phase.
    listen(window, "focus", record, { capture: true })
    listen(window, "blur", record, { capture: true })

    testRoot.renderer.focusElement(testRoot.renderer.findByTestId("a").id)
    testRoot.renderer.focusElement(testRoot.renderer.findByTestId("b").id)

    const byType = (type: string) => seen.filter((entry) => entry.type === type)
    expect(byType("focusin")).toHaveLength(2)
    expect(byType("focusout")).toHaveLength(1)
    expect(byType("focus")).toHaveLength(2)
    expect(byType("blur")).toHaveLength(1)
    expect(byType("focusin")[0].target).toBe(a)
    expect(byType("focusin")[0].relatedTarget).toBeNull()
    expect(byType("focusin")[1].target).toBe(b)
    expect(byType("focusin")[1].relatedTarget).toBe(a)
    expect(byType("focusout")[0].target).toBe(a)
    expect(byType("focusout")[0].relatedTarget).toBe(b)
    expect(byType("focus")[0].target).toBe(a)
    expect(byType("focus")[0].relatedTarget).toBeNull()
    expect(byType("focus")[1].target).toBe(b)
    expect(byType("focus")[1].relatedTarget).toBe(a)
    expect(byType("blur")[0].target).toBe(a)
    expect(byType("blur")[0].relatedTarget).toBe(b)
  })

  it("lets a window listener see the target element while currentTarget is the window", () => {
    let button: unknown
    const seen: Array<[string, unknown, unknown]> = []
    testRoot.render(
      <div ref={(element) => (button = element)} style={{ width: 100, height: 100 }} onClick={() => {}} />
    )
    listen(window, "click", (event) => seen.push(["window", event.target, event.currentTarget]))
    listen(document, "click", (event) => seen.push(["document", event.target, event.currentTarget]))

    testRoot.renderer.nativeSimulateClick(50, 50)

    expect(seen.map((entry) => entry[0])).toEqual(["document", "window"])
    expect(seen[0][1]).toBe(button)
    expect(seen[0][2]).toBe(document)
    expect(seen[1][1]).toBe(button)
    expect(seen[1][2]).toBe(window)
  })

  it("keeps elementId numeric next to the element target", () => {
    let element: { id: number } | undefined
    let seen: DomEvent | undefined
    testRoot.render(
      <div
        ref={(instance) => (element = instance as unknown as { id: number })}
        style={{ width: 100, height: 100 }}
        onClick={(event) => (seen = event)}
      />
    )

    testRoot.renderer.nativeSimulateClick(50, 50)

    expect(seen?.target).toBe(element)
    expect(seen?.elementId).toBe(element?.id)
    expect(typeof seen?.elementId).toBe("number")
  })

  it("has a null target for a key with nothing focused", () => {
    testRoot.render(<div style={{ width: 100, height: 100 }} />)
    const seen: Array<[unknown, unknown]> = []
    listen(window, "keydown", (event) => seen.push([event.target, event.currentTarget]))

    testRoot.renderer.simulateKeystrokes("x")

    expect(seen).toHaveLength(1)
    expect(seen[0][0]).toBeNull()
    expect(seen[0][1]).toBe(window)
  })
})
