import React from "react"
import { describe, expect, it } from "vitest"
import {
  MediaQueryList,
  matchMedia,
  useMatchMedia,
  useMediaQuery,
  useReducedMotion,
} from "../index.js"
import { createTestRoot } from "../testing.js"

function Reduced() {
  return <text>{useReducedMotion() ? "reduced" : "full"}</text>
}

describe("matchMedia", () => {
  it("useReducedMotion follows the OS setting", () => {
    const { render, renderer, unmount } = createTestRoot()
    render(<Reduced />)
    expect(renderer.getAllText()).toEqual(["full"])

    renderer.setMediaFeatures({ reducedMotion: true })
    renderer.flush()
    expect(renderer.getAllText()).toEqual(["reduced"])

    renderer.setMediaFeatures({ reducedMotion: false })
    renderer.flush()
    expect(renderer.getAllText()).toEqual(["full"])
    unmount()
  })

  it("starts reduced when the OS already asks for it", () => {
    const { render, renderer, unmount } = createTestRoot()
    renderer.setMediaFeatures({ reducedMotion: true })
    render(<Reduced />)
    expect(renderer.getAllText()).toEqual(["reduced"])
    unmount()
  })

  it("stops listening to the window on unmount", () => {
    const { render, renderer, unmount } = createTestRoot()
    render(<Reduced />)
    unmount()
    renderer.setMediaFeatures({ reducedMotion: true })
    expect(renderer.drainEvents()).toEqual([])
  })

  it("useMediaQuery evaluates a query against the window", () => {
    function Probe() {
      const wide = useMediaQuery("(min-width: 1px)")
      const huge = useMediaQuery("(min-width: 100000px)")
      const contrast = useMediaQuery("(prefers-contrast: more)")
      return <text>{`${wide} ${huge} ${contrast}`}</text>
    }
    const { render, renderer, unmount } = createTestRoot()
    render(<Probe />)
    expect(renderer.getAllText()).toEqual(["true false false"])
    renderer.setMediaFeatures({ increasedContrast: true })
    renderer.flush()
    expect(renderer.getAllText()).toEqual(["true false true"])
    unmount()
  })

  it("useMatchMedia returns a DOM MediaQueryList for the window", () => {
    let list: MediaQueryList | undefined
    function Probe() {
      list = useMatchMedia()("(prefers-reduced-motion: reduce)")
      return <text>probe</text>
    }
    const { render, renderer, unmount } = createTestRoot()
    render(<Probe />)
    expect(list).toBeInstanceOf(MediaQueryList)
    expect(list?.media).toBe("(prefers-reduced-motion: reduce)")

    const seen: boolean[] = []
    list?.addEventListener("change", (event) => seen.push((event as { matches: boolean }).matches))
    renderer.setMediaFeatures({ reducedMotion: true })
    expect(seen).toEqual([true])
    unmount()
  })

  it("the global matchMedia uses the mounted root's window", () => {
    const { render, renderer, unmount } = createTestRoot()
    render(<text>x</text>)
    expect(matchMedia("(prefers-reduced-motion: reduce)").matches).toBe(false)
    renderer.setMediaFeatures({ reducedMotion: true })
    expect(matchMedia("(prefers-reduced-motion: reduce)").matches).toBe(true)
    expect(matchMedia("(prefers-color-scheme: dark)").matches).toBe(
      renderer.getAppearance() === "dark"
    )
    unmount()
    expect(() => matchMedia("(hover)")).toThrow(/mounted/)
  })

  it("the test window is a hover-capable fine-pointer screen", () => {
    const { render, unmount } = createTestRoot()
    render(<text>x</text>)
    expect(matchMedia("screen and (hover: hover) and (pointer: fine)").matches).toBe(true)
    unmount()
  })
})
