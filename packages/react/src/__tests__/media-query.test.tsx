import React from "react"
import { describe, expect, it, vi } from "vitest"
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

  describe("resizing the window", () => {
    function listen(query: string, seen: boolean[]): MediaQueryList {
      const list = matchMedia(query)
      list.addEventListener("change", (event) => seen.push((event as { matches: boolean }).matches))
      return list
    }

    it("fires change when a size query flips, and not when the size changes but the result does not", () => {
      const interval = vi.spyOn(globalThis, "setInterval")
      const { render, renderer, unmount } = createTestRoot()
      render(<text>x</text>)
      renderer.setWindowSize(1280, 800)
      const seen: boolean[] = []
      const list = listen("(max-width: 1000px)", seen)
      expect(list.matches).toBe(false)

      renderer.setWindowSize(900, 800)
      expect(seen).toEqual([true])
      expect(list.matches).toBe(true)

      renderer.setWindowSize(800, 700)
      expect(seen).toEqual([true])

      renderer.setWindowSize(1100, 700)
      expect(seen).toEqual([true, false])
      expect(interval).not.toHaveBeenCalled()
      interval.mockRestore()
      unmount()
    })

    it("re-evaluates orientation and aspect-ratio from the new size", () => {
      const { render, renderer, unmount } = createTestRoot()
      render(<text>x</text>)
      renderer.setWindowSize(1280, 800)
      const portrait: boolean[] = []
      const square: boolean[] = []
      listen("(orientation: portrait)", portrait)
      listen("(aspect-ratio: 1/1)", square)

      renderer.setWindowSize(600, 900)
      expect(portrait).toEqual([true])
      expect(square).toEqual([])

      renderer.setWindowSize(700, 700)
      expect(square).toEqual([true])
      unmount()
    })

    it("a window opened at a size reports it in matches before any resize", () => {
      const { render, unmount } = createTestRoot()
      render(<text>x</text>)
      expect(matchMedia("(width: 1280px) and (height: 800px)").matches).toBe(true)
      unmount()
    })
  })
})
