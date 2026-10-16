import React from "react"
import { describe, expect, it } from "vitest"
import { matchMedia } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const RED = [255, 0, 0, 255]
const BLUE = [0, 0, 255, 255]

/** The appearance a window opened with no override reports: the platform's. */
function systemAppearance(): "light" | "dark" {
  const { renderer, unmount } = createTestRoot()
  try {
    return renderer.getAppearance() as "light" | "dark"
  } finally {
    unmount()
  }
}

const opposite = (appearance: "light" | "dark") => (appearance === "dark" ? "light" : "dark")

describeNative("appearance", () => {
  it("rejects an unknown appearance", () => {
    const { renderer, unmount } = createTestRoot()
    expect(() => renderer.setAppearance("sepia")).toThrow(/Invalid appearance/)
    unmount()
  })

  it("is forced by setAppearance and restored by system", () => {
    const system = systemAppearance()
    const { renderer, unmount } = createTestRoot()
    renderer.setAppearance(opposite(system))
    expect(renderer.getAppearance()).toBe(opposite(system))
    renderer.setAppearance(system)
    expect(renderer.getAppearance()).toBe(system)
    renderer.setAppearance(opposite(system))
    renderer.setAppearance("system")
    expect(renderer.getAppearance()).toBe(system)
    unmount()
  })

  it("starts in the appearance option, and a window without one starts in the system's", () => {
    const system = systemAppearance()
    const forced = createTestRoot({ appearance: opposite(system) })
    expect(forced.renderer.getAppearance()).toBe(opposite(system))
    forced.unmount()

    const plain = createTestRoot()
    expect(plain.renderer.getAppearance()).toBe(system)
    plain.unmount()
  })

  it("rejects an unknown appearance option before opening the window", () => {
    expect(() => createTestRoot({ appearance: "sepia" })).toThrow(/Invalid appearance/)
  })

  it("delivers appearanceChange once per change", () => {
    const system = systemAppearance()
    const seen: unknown[] = []
    const { render, renderer, unmount } = createTestRoot({
      onAppearanceChange: (event) => seen.push(event.value),
    })
    render(<div style={{ width: 10, height: 10 }} />)

    renderer.setAppearance(opposite(system))
    renderer.setAppearance(opposite(system))
    expect(seen).toEqual([opposite(system)])

    renderer.setAppearance("system")
    expect(seen).toEqual([opposite(system), system])
    unmount()
  })

  it("moves matchMedia(prefers-color-scheme) and tells its listeners", () => {
    const system = systemAppearance()
    const { render, renderer, unmount } = createTestRoot()
    render(<text>x</text>)
    const dark = matchMedia("(prefers-color-scheme: dark)")
    const changes: boolean[] = []
    dark.addEventListener("change", (event) => changes.push((event as { matches: boolean }).matches))
    expect(dark.matches).toBe(system === "dark")

    // Forcing the appearance the window already has is not a change, so start from the other one.
    const other = opposite(system)
    renderer.setAppearance(other)
    expect(dark.matches).toBe(other === "dark")
    expect(changes).toEqual([other === "dark"])
    renderer.setAppearance(system)
    expect(dark.matches).toBe(system === "dark")
    expect(changes).toEqual([other === "dark", system === "dark"])
    unmount()
  })

  it("resolves light-dark() against the forced appearance", () => {
    const { render, renderer, unmount } = createTestRoot({ width: 120, height: 120 })
    render(
      <div
        style={{
          width: 100,
          height: 100,
          colorScheme: "light dark",
          backgroundColor: "light-dark(#ff0000, #0000ff)",
        }}
      />,
    )
    const pixel = (name: string) => {
      const file = `${SHOTS_DIR}/appearance-${name}.png`
      renderer.captureScreenshot(file)
      return readLogicalPixels(file, renderer).at(50, 50)
    }

    renderer.setAppearance("dark")
    expect(pixel("dark")).toEqual(BLUE)
    renderer.setAppearance("light")
    expect(pixel("light")).toEqual(RED)
    unmount()
  })
})
