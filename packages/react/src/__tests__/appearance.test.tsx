import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot } from "../testing.js"

// The test window is a real offscreen platform window with no run loop, so a
// forced appearance never reaches it and no appearanceChange can be produced.
// The dispatch path is covered in packages/native/js/__tests__.
describe("appearance", () => {
  it("reports the effective appearance as light or dark", () => {
    const { renderer } = createTestRoot()
    expect(["light", "dark"]).toContain(renderer.getAppearance())
  })

  it("rejects an unknown appearance", () => {
    const { renderer } = createTestRoot()
    expect(() => renderer.setAppearance("sepia")).toThrow(/Invalid appearance/)
  })

  it("accepts system, light and dark", () => {
    const { renderer } = createTestRoot()
    for (const value of ["system", "light", "dark"]) {
      expect(() => renderer.setAppearance(value)).not.toThrow()
    }
  })

  it("mounts and unmounts with an onAppearanceChange handler", () => {
    const { render, unmount } = createTestRoot({ onAppearanceChange: () => {} })
    render(<div style={{ width: 10, height: 10 }} />)
    expect(() => unmount()).not.toThrow()
  })
})
