// @ts-nocheck

import React from "react"
import { describe, expect, it } from "vitest"
import * as DialogPrimitive from "../components/dialog"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("React unmount", () => {
  it("destroys every top-level node, a portal's anchored layer included", () => {
    const testRoot = createTestRoot()
    testRoot.render(
      <DialogPrimitive.Root defaultOpen>
        <DialogPrimitive.Trigger style={{ width: 80, height: 24 }}>Open</DialogPrimitive.Trigger>
        <DialogPrimitive.Portal>
          <DialogPrimitive.Backdrop style={{ backgroundColor: "#00000080" }} />
          <DialogPrimitive.Popup style={{ width: 300, padding: 16, backgroundColor: "#1e293b" }}>
            <DialogPrimitive.Title>Settings</DialogPrimitive.Title>
          </DialogPrimitive.Popup>
        </DialogPrimitive.Portal>
      </DialogPrimitive.Root>
    )
    expect(testRoot.renderer.getRetainedElementCount()).toBeGreaterThan(0)
    testRoot.unmount()
    expect(testRoot.renderer.getRetainedElementCount()).toBe(0)
  })

  it("destroys sibling roots", () => {
    const testRoot = createTestRoot()
    testRoot.render(
      <>
        <div style={{ width: 10, height: 10 }} />
        <div style={{ width: 10, height: 10 }} />
      </>
    )
    expect(testRoot.renderer.getRetainedElementCount()).toBe(2)
    testRoot.unmount()
    expect(testRoot.renderer.getRetainedElementCount()).toBe(0)
  })
})
