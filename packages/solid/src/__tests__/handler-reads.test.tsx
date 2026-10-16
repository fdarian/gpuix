/**
 * What a sync press or release handler can call. The root view is not leased while native
 * dispatches input, so a handler reads current values, as in the DOM.
 */
import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { Dialog, DialogPopup, DialogPortal, document } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { HostElement } from "../host.js"

type Read = {
  focusedId: number | null
  otherFocus: boolean
  otherWithin: boolean
  fieldFocus: boolean
  fieldHover: boolean
  boxHeight: number
  active: HostElement | null
}

describe.skipIf(!hasNativeTestRenderer)("reads inside a press or release handler", () => {
  for (const on of ["mousedown", "mouseup"] as const) {
    it(`answers focus, hover, activeElement and fresh bounds in a ${on} handler`, () => {
      const app = createTestRoot()
      try {
        const seen: Read[] = []
        const [height, setHeight] = createSignal(20)
        let other!: HostElement
        let field!: HostElement
        let box!: HostElement
        const read = () => {
          setHeight(60)
          seen.push({
            focusedId: app.renderer.getFocusedElementId(),
            otherFocus: other.matches(":focus"),
            otherWithin: other.matches(":focus-within"),
            fieldFocus: field.matches(":focus"),
            fieldHover: field.matches(":hover"),
            boxHeight: box.getBoundingClientRect().height,
            active: document.activeElement,
          })
        }
        app.render(() => (
          <div
            style={{ width: 400, height: 300 }}
            onMouseDown={on === "mousedown" ? read : undefined}
            onMouseUp={on === "mouseup" ? read : undefined}
          >
            <input ref={other} testId="other" defaultValue="o" style={{ width: 100, height: 20 }} />
            <input ref={field} testId="field" defaultValue="f" style={{ width: 100, height: 20 }} />
            <div ref={box} testId="box" style={{ width: 50, height: height() }} />
          </div>
        ))
        app.renderer.focusElement(other.id)
        const bounds = app.renderer.getElementBounds(field.id)!

        app.renderer.nativeSimulateClick(bounds.x + 10, bounds.y + 5)

        expect(seen).toHaveLength(1)
        const first = seen[0]!
        expect(first.boxHeight).toBe(60)
        expect(first.fieldHover).toBe(true)
        if (on === "mousedown") {
          // The press moves focus after its handlers have run.
          expect(first.focusedId).toBe(other.id)
          expect(first.otherFocus).toBe(true)
          expect(first.otherWithin).toBe(true)
          expect(first.fieldFocus).toBe(false)
          expect(first.active).toBe(other)
        } else {
          expect(first.focusedId).toBe(field.id)
          expect(first.otherFocus).toBe(false)
          expect(first.fieldFocus).toBe(true)
          expect(first.active).toBe(field)
        }
      } finally {
        app.unmount()
      }
    })
  }
})

describe.skipIf(!hasNativeTestRenderer)("document.activeElement", () => {
  it("is the object a ref gives, follows focus() and blur(), and is null with nothing focused", () => {
    const app = createTestRoot()
    try {
      let a!: HostElement
      let b!: HostElement
      app.render(() => (
        <div style={{ width: 200, height: 100 }}>
          <div ref={a} tabIndex={0} style={{ width: 50, height: 20 }} />
          <div ref={b} tabIndex={0} style={{ width: 50, height: 20 }} />
        </div>
      ))
      expect(document.activeElement).toBeNull()
      a.focus()
      expect(document.activeElement).toBe(a)
      b.focus()
      expect(document.activeElement).toBe(b)
      b.blur()
      expect(document.activeElement).toBeNull()
    } finally {
      app.unmount()
    }
  })

  it("is null once the root is unmounted", () => {
    const app = createTestRoot()
    let ref!: HostElement
    app.render(() => <div ref={ref} tabIndex={0} style={{ width: 50, height: 20 }} />)
    ref.focus()
    expect(document.activeElement).toBe(ref)
    app.unmount()
    expect(document.activeElement).toBeNull()
  })
})

describe.skipIf(!hasNativeTestRenderer)("a Dialog opened from a mousedown handler", () => {
  it("records the element focused before it opened and gives focus back to it on Escape", () => {
    const app = createTestRoot()
    try {
      const [open, setOpen] = createSignal(false)
      let before!: HostElement
      app.render(() => (
        <div style={{ width: 600, height: 400 }}>
          <input ref={before} testId="before" style={{ width: 100, height: 20 }} />
          <div testId="opener" style={{ width: 80, height: 24 }} onMouseDown={() => setOpen(true)} />
          <Dialog open={open()} onOpenChange={setOpen}>
            <DialogPortal>
              <DialogPopup testId="popup" style={{ width: 200, height: 100 }} />
            </DialogPortal>
          </Dialog>
        </div>
      ))
      before.focus()
      const opener = app.renderer.getElementBounds(app.renderer.findByTestId("opener")!.id)!

      app.renderer.nativeSimulateClick(opener.x + 5, opener.y + 5)
      expect(app.renderer.findByTestId("popup")).toBeDefined()

      app.renderer.simulateKeystrokes("Escape")
      expect(app.renderer.findByTestId("popup")).toBeUndefined()
      expect(document.activeElement).toBe(before)
    } finally {
      app.unmount()
    }
  })
})
