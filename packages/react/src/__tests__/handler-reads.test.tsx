/**
 * What a sync press, release, wheel or key handler can call. The root view is not leased while
 * native dispatches input (the window's input callback never leases it), so a handler reads
 * current values and its writes apply at once, as in the DOM.
 */
// @ts-nocheck

import { useRef, useState } from "react"
import { describe, expect, it } from "vitest"
import { document } from "../index"
import * as DialogPrimitive from "../components/dialog"
import { flushSync } from "../reconciler/reconciler"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

function Scene(props: { reads: (refs) => void; on: "mousedown" | "mouseup" }) {
  const [height, setHeight] = useState(20)
  const other = useRef(null)
  const field = useRef(null)
  const box = useRef(null)
  const read = () => {
    flushSync(() => setHeight(60))
    props.reads({ other: other.current, field: field.current, box: box.current })
  }
  return (
    <div
      style={{ width: 400, height: 300 }}
      onMouseDown={props.on === "mousedown" ? read : undefined}
      onMouseUp={props.on === "mouseup" ? read : undefined}
    >
      <input ref={other} testId="other" defaultValue="o" style={{ width: 100, height: 20 }} />
      <input ref={field} testId="field" defaultValue="f" style={{ width: 100, height: 20 }} />
      <div ref={box} testId="box" style={{ width: 50, height }} />
    </div>
  )
}

describeNative("reads inside a press or release handler", () => {
  for (const on of ["mousedown", "mouseup"] as const) {
    it(`answers focus, hover, activeElement and fresh bounds in a ${on} handler`, () => {
      const { render, renderer, unmount } = createTestRoot()
      try {
        const seen: Record<string, unknown>[] = []
        const errors: unknown[] = []
        render(
          <Scene
            on={on}
            reads={(refs) => {
              try {
                seen.push({
                  focusedId: renderer.getFocusedElementId(),
                  otherFocus: refs.other.matches(":focus"),
                  otherWithin: refs.other.matches(":focus-within"),
                  otherVisible: refs.other.matches(":focus-visible"),
                  fieldFocus: refs.field.matches(":focus"),
                  fieldHover: refs.field.matches(":hover"),
                  boxHeight: refs.box.getBoundingClientRect().height,
                  active: document.activeElement,
                })
              } catch (error) {
                errors.push(error)
              }
            }}
          />
        )
        const other = renderer.findByTestId("other")
        const field = renderer.findByTestId("field")
        renderer.focusElement(other.id)
        const bounds = renderer.getElementBounds(field.id)

        renderer.nativeSimulateClick(bounds.x + 10, bounds.y + 5)

        expect(errors).toEqual([])
        expect(seen).toHaveLength(1)
        const read = seen[0]
        expect(read.boxHeight).toBe(60)
        expect(read.fieldHover).toBe(true)
        if (on === "mousedown") {
          // The press moves focus after its handlers have run.
          expect(read.focusedId).toBe(other.id)
          expect(read.otherFocus).toBe(true)
          expect(read.fieldFocus).toBe(false)
          expect(read.active?.id).toBe(other.id)
        } else {
          expect(read.focusedId).toBe(field.id)
          expect(read.otherFocus).toBe(false)
          expect(read.fieldFocus).toBe(true)
          expect(read.active?.id).toBe(field.id)
        }
      } finally {
        unmount()
      }
    })
  }
})

describeNative("document.activeElement", () => {
  it("is the object a ref gives, follows focus() and blur(), and is null with nothing focused", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const refs = { a: null, b: null }
      render(
        <div style={{ width: 200, height: 100 }}>
          <div ref={(value) => (refs.a = value)} tabIndex={0} style={{ width: 50, height: 20 }} />
          <div ref={(value) => (refs.b = value)} tabIndex={0} style={{ width: 50, height: 20 }} />
        </div>
      )
      expect(document.activeElement).toBeNull()
      refs.a.focus()
      expect(document.activeElement).toBe(refs.a)
      refs.b.focus()
      expect(document.activeElement).toBe(refs.b)
      refs.b.blur()
      expect(document.activeElement).toBeNull()
    } finally {
      unmount()
    }
  })

  it("is null once the root is unmounted", () => {
    const { render, renderer, unmount } = createTestRoot()
    let ref = null
    render(<div ref={(value) => (ref = value)} tabIndex={0} style={{ width: 50, height: 20 }} />)
    ref.focus()
    expect(document.activeElement).toBe(ref)
    unmount()
    expect(document.activeElement).toBeNull()
  })
})

describeNative("a Dialog opened from a mousedown handler", () => {
  it("records the element focused before it opened and gives focus back to it on Escape", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const refs = { before: null }
      function App() {
        const [open, setOpen] = useState(false)
        return (
          <div style={{ width: 600, height: 400 }}>
            <input ref={(value) => (refs.before = value)} testId="before" style={{ width: 100, height: 20 }} />
            <div testId="opener" style={{ width: 80, height: 24 }} onMouseDown={() => setOpen(true)} />
            <DialogPrimitive.Root open={open} onOpenChange={setOpen}>
              <DialogPrimitive.Portal>
                <DialogPrimitive.Popup testId="popup" style={{ width: 200, height: 100 }} />
              </DialogPrimitive.Portal>
            </DialogPrimitive.Root>
          </div>
        )
      }
      render(<App />)
      refs.before.focus()
      const opener = renderer.getElementBounds(renderer.findByTestId("opener").id)

      renderer.nativeSimulateClick(opener.x + 5, opener.y + 5)
      expect(renderer.findByTestId("popup")).toBeDefined()

      renderer.simulateKeystrokes("escape")
      expect(renderer.findByTestId("popup")).toBeUndefined()
      expect(document.activeElement).toBe(refs.before)
    } finally {
      unmount()
    }
  })
})
