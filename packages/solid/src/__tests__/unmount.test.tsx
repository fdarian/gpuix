import { describe, expect, it } from "bun:test"
import {
  Dialog,
  DialogBackdrop,
  DialogPopup,
  DialogPortal,
  DialogTitle,
  DialogTrigger,
} from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("Solid unmount", () => {
  it("destroys every top-level node, a portal's anchored layer included", () => {
    const app = createTestRoot()
    app.render(() => (
      <Dialog defaultOpen>
        <DialogTrigger style={{ width: 80, height: 24 }}><text>Open</text></DialogTrigger>
        <DialogPortal>
          <DialogBackdrop style={{ backgroundColor: "#00000080" }} />
          <DialogPopup style={{ width: 300, padding: 16, backgroundColor: "#1e293b" }}>
            <DialogTitle><text>Settings</text></DialogTitle>
          </DialogPopup>
        </DialogPortal>
      </Dialog>
    ))
    expect(app.renderer.getRetainedElementCount()).toBeGreaterThan(0)
    app.unmount()
    expect(app.renderer.getRetainedElementCount()).toBe(0)
  })

  it("destroys sibling roots", () => {
    const app = createTestRoot()
    app.render(() => (
      <>
        <div style={{ width: 10, height: 10 }} />
        <div style={{ width: 10, height: 10 }} />
      </>
    ))
    expect(app.renderer.getRetainedElementCount()).toBe(2)
    app.unmount()
    expect(app.renderer.getRetainedElementCount()).toBe(0)
  })
})
