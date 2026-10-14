import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import {
  MediaQueryList,
  createMatchMedia,
  createMediaQuery,
  createReducedMotion,
  matchMedia,
} from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("Solid matchMedia", () => {
  it("createReducedMotion follows the OS setting", () => {
    const app = createTestRoot()
    app.render(() => {
      const reduced = createReducedMotion()
      return <text>{reduced() ? "reduced" : "full"}</text>
    })
    expect(app.renderer.getAllText()).toEqual(["full"])

    app.renderer.setMediaFeatures({ reducedMotion: true })
    app.renderer.flush()
    expect(app.renderer.getAllText()).toEqual(["reduced"])

    app.renderer.setMediaFeatures({ reducedMotion: false })
    app.renderer.flush()
    expect(app.renderer.getAllText()).toEqual(["full"])
    app.unmount()
  })

  it("createMediaQuery accepts a reactive query", () => {
    const [query, setQuery] = createSignal("(min-width: 1px)")
    const app = createTestRoot()
    app.render(() => {
      const matches = createMediaQuery(query)
      return <text>{String(matches())}</text>
    })
    expect(app.renderer.getAllText()).toEqual(["true"])
    app.flushSync(() => setQuery("(min-width: 100000px)"))
    expect(app.renderer.getAllText()).toEqual(["false"])
    app.unmount()
  })

  it("createMatchMedia returns a DOM MediaQueryList for the window", () => {
    const app = createTestRoot()
    let list: MediaQueryList | undefined
    app.render(() => {
      list = createMatchMedia()("(prefers-contrast: more)")
      return <text>probe</text>
    })
    expect(list).toBeInstanceOf(MediaQueryList)
    expect(list?.matches).toBe(false)

    const seen: boolean[] = []
    list?.addEventListener("change", (event) => seen.push((event as { matches: boolean }).matches))
    app.renderer.setMediaFeatures({ increasedContrast: true })
    expect(seen).toEqual([true])
    app.unmount()
  })

  it("stops listening to the window on unmount", () => {
    const app = createTestRoot()
    app.render(() => {
      createReducedMotion()
      return <text>x</text>
    })
    app.unmount()
    app.renderer.setMediaFeatures({ reducedMotion: true })
    expect(app.renderer.drainEvents()).toEqual([])
  })

  it("the global matchMedia uses the mounted root's window", () => {
    const app = createTestRoot()
    app.render(() => <text>x</text>)
    expect(matchMedia("(prefers-reduced-motion: reduce)").matches).toBe(false)
    app.renderer.setMediaFeatures({ reducedMotion: true })
    expect(matchMedia("(prefers-reduced-motion: reduce)").matches).toBe(true)
    app.unmount()
    // Another test of this process may have left a root mounted, which then answers
    // for the global; the unmounted window must not, and with no root it throws.
    const answer = () => matchMedia("(prefers-reduced-motion: reduce)").matches
    try {
      expect(answer()).toBe(false)
    } catch (error) {
      expect(String(error)).toMatch(/mounted/)
    }
  })
})
