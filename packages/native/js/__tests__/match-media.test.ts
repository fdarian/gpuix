import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import {
  MediaQueryList,
  MediaQueryListEvent,
  createRendererState,
  matchMedia,
  matchMediaFor,
  observeReducedMotion,
  readReducedMotion,
} from "../host.js"
import type { MediaFeatures, NativeRenderer } from "../host.js"

class FakeWindow {
  width = 800
  height = 600
  appearance: "light" | "dark" = "light"
  features: MediaFeatures = { reducedMotion: false, increasedContrast: false, forcedColors: false }
  mediaChangeCalls: boolean[] = []

  applyBatch(): number[] {
    return []
  }

  getWindowSize() {
    return { width: this.width, height: this.height }
  }

  getAppearance() {
    return this.appearance
  }

  getMediaFeatures() {
    return this.features
  }

  setWindowMediaChange(enabled: boolean) {
    this.mediaChangeCalls.push(enabled)
  }

  /** What the renderer does when the OS notifies. */
  emitMediaChange() {
    createRendererState(this as NativeRenderer).dispatch({
      elementId: 0,
      eventType: "mediaChange",
      value: "system",
    })
  }
}

function fakeWindow(): FakeWindow & NativeRenderer {
  return new FakeWindow() as FakeWindow & NativeRenderer
}

describe("matchMedia", () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it("returns a MediaQueryList with the DOM shape", () => {
    const window = fakeWindow()
    const list = matchMediaFor(window)("(MIN-WIDTH:400px)")
    expect(list).toBeInstanceOf(MediaQueryList)
    expect(list.media).toBe("(min-width: 400px)")
    expect(list.matches).toBe(true)
    expect(list.onchange).toBeNull()
  })

  it("returns the same function for the same renderer", () => {
    const window = fakeWindow()
    expect(matchMediaFor(window)).toBe(matchMediaFor(window))
    expect(matchMediaFor(fakeWindow())).not.toBe(matchMediaFor(window))
  })

  it("evaluates against the live window, not a snapshot", () => {
    const window = fakeWindow()
    const list = matchMediaFor(window)("(max-width: 500px)")
    expect(list.matches).toBe(false)
    window.width = 400
    expect(list.matches).toBe(true)
  })

  it("reads the appearance and the OS preferences", () => {
    const window = fakeWindow()
    const matcher = matchMediaFor(window)
    const dark = matcher("(prefers-color-scheme: dark)")
    const reduced = matcher("(prefers-reduced-motion: reduce)")
    const contrast = matcher("(prefers-contrast: more)")
    expect([dark.matches, reduced.matches, contrast.matches]).toEqual([false, false, false])
    window.appearance = "dark"
    window.features = { reducedMotion: true, increasedContrast: true, forcedColors: false }
    expect([dark.matches, reduced.matches, contrast.matches]).toEqual([true, true, true])
  })

  it("treats a renderer without getMediaFeatures as no preference", () => {
    const window = fakeWindow()
    Reflect.deleteProperty(window, "getMediaFeatures")
    ;(window as { getMediaFeatures?: unknown }).getMediaFeatures = undefined
    expect(matchMediaFor(window)("(prefers-reduced-motion: reduce)").matches).toBe(false)
    expect(readReducedMotion(window)).toBe(false)
  })

  it("throws, instead of guessing, when the renderer cannot report what a query reads", () => {
    const window = fakeWindow()
    ;(window as { getWindowSize?: unknown }).getWindowSize = undefined
    const list = matchMediaFor(window)("(min-width: 1px)")
    expect(() => list.matches).toThrow(/window size/)
    // A query that does not read the size is unaffected.
    expect(matchMediaFor(window)("(hover: hover)").matches).toBe(true)
  })

  it("fires change only when matches flips", () => {
    const window = fakeWindow()
    const list = matchMediaFor(window)("(prefers-reduced-motion: reduce)")
    const events: MediaQueryListEvent[] = []
    list.addEventListener("change", (event) => events.push(event as MediaQueryListEvent))

    window.emitMediaChange()
    expect(events).toHaveLength(0)

    window.features = { reducedMotion: true, increasedContrast: false, forcedColors: false }
    window.emitMediaChange()
    expect(events).toHaveLength(1)
    expect(events[0]).toBeInstanceOf(MediaQueryListEvent)
    expect(events[0].type).toBe("change")
    expect(events[0].matches).toBe(true)
    expect(events[0].media).toBe("(prefers-reduced-motion: reduce)")
    expect(events[0].target).toBe(list)
    expect(events[0].currentTarget).toBe(list)

    window.emitMediaChange()
    expect(events).toHaveLength(1)

    window.features = { reducedMotion: false, increasedContrast: false, forcedColors: false }
    window.emitMediaChange()
    expect(events.map((event) => event.matches)).toEqual([true, false])
  })

  it("fires onchange and legacy listeners, in registration order", () => {
    const window = fakeWindow()
    const list = matchMediaFor(window)("(prefers-color-scheme: dark)")
    const order: string[] = []
    list.addListener(() => order.push("legacy"))
    list.onchange = function (event) {
      expect(this).toBe(list)
      order.push(`onchange:${event.matches}`)
    }
    list.addEventListener("change", { handleEvent: () => order.push("object") })

    window.appearance = "dark"
    window.emitMediaChange()
    expect(order).toEqual(["legacy", "onchange:true", "object"])
  })

  it("dedupes listeners and honors removal, once and signal", () => {
    const window = fakeWindow()
    const list = matchMediaFor(window)("(prefers-color-scheme: dark)")
    const calls: string[] = []
    const same = () => calls.push("same")
    list.addEventListener("change", same)
    list.addEventListener("change", same)
    list.addEventListener("change", () => calls.push("once"), { once: true })
    const controller = new AbortController()
    list.addEventListener("change", () => calls.push("signal"), { signal: controller.signal })
    const removed = () => calls.push("removed")
    list.addEventListener("change", removed)
    list.removeEventListener("change", removed)

    window.appearance = "dark"
    window.emitMediaChange()
    expect(calls).toEqual(["same", "once", "signal"])

    controller.abort()
    window.appearance = "light"
    window.emitMediaChange()
    expect(calls).toEqual(["same", "once", "signal", "same"])
  })

  it("keeps running listeners after one throws, and reports the error", async () => {
    const window = fakeWindow()
    const list = matchMediaFor(window)("(prefers-color-scheme: dark)")
    const reported: unknown[] = []
    const onError = (error: unknown) => reported.push(error)
    process.once("uncaughtException", onError)
    const ran: string[] = []
    list.addEventListener("change", () => {
      throw new Error("boom")
    })
    list.addEventListener("change", () => ran.push("second"))
    window.appearance = "dark"
    window.emitMediaChange()
    expect(ran).toEqual(["second"])
    await vi.advanceTimersByTimeAsync(0)
    process.off("uncaughtException", onError)
  })

  it("subscribes to the renderer only while something listens", () => {
    const window = fakeWindow()
    const matcher = matchMediaFor(window)
    const first = matcher("(prefers-reduced-motion: reduce)")
    const second = matcher("(prefers-color-scheme: dark)")
    expect(window.mediaChangeCalls).toEqual([])

    const listener = () => {}
    first.addEventListener("change", listener)
    second.onchange = () => {}
    expect(window.mediaChangeCalls).toEqual([true])

    first.removeEventListener("change", listener)
    expect(window.mediaChangeCalls).toEqual([true])
    second.onchange = null
    expect(window.mediaChangeCalls).toEqual([true, false])
  })

  it("unsubscribes after a once listener fires", () => {
    const window = fakeWindow()
    const list = matchMediaFor(window)("(prefers-color-scheme: dark)")
    list.addEventListener("change", () => {}, { once: true })
    window.appearance = "dark"
    window.emitMediaChange()
    expect(window.mediaChangeCalls).toEqual([true, false])
  })

  it("does not report a flip that happened before the first listener", () => {
    const window = fakeWindow()
    const list = matchMediaFor(window)("(prefers-color-scheme: dark)")
    window.appearance = "dark"
    const events: boolean[] = []
    list.addEventListener("change", (event) => events.push((event as MediaQueryListEvent).matches))
    window.emitMediaChange()
    expect(events).toEqual([])
  })

  it("re-evaluates size queries on mediaChange, with no timer", () => {
    const window = fakeWindow()
    window.height = 300
    const matcher = matchMediaFor(window)
    const events: boolean[] = []
    const narrow = matcher("(max-width: 500px)")
    narrow.addEventListener("change", (event) => events.push((event as MediaQueryListEvent).matches))
    const landscape = matcher("(orientation: landscape)")
    const orientation: boolean[] = []
    landscape.addEventListener("change", (event) =>
      orientation.push((event as MediaQueryListEvent).matches)
    )
    expect(vi.getTimerCount()).toBe(0)

    window.emitMediaChange()
    expect(events).toEqual([])

    window.width = 400
    window.emitMediaChange()
    expect(events).toEqual([true])
    expect(orientation).toEqual([])

    window.width = 250
    window.emitMediaChange()
    expect(events).toEqual([true])
    expect(orientation).toEqual([false])
  })

  it("reads the window size only when the window says something changed", () => {
    const window = fakeWindow()
    const spy = vi.spyOn(window, "getWindowSize")
    matchMediaFor(window)("(max-width: 500px)").onchange = () => {}
    spy.mockClear()
    vi.advanceTimersByTime(10_000)
    expect(spy).not.toHaveBeenCalled()
  })

  it("keeps windows apart", () => {
    const left = fakeWindow()
    const right = fakeWindow()
    right.features = { reducedMotion: true, increasedContrast: false, forcedColors: false }
    expect(matchMediaFor(left)("(prefers-reduced-motion: reduce)").matches).toBe(false)
    expect(matchMediaFor(right)("(prefers-reduced-motion: reduce)").matches).toBe(true)
  })

  it("observes reduced motion now and on change", () => {
    const window = fakeWindow()
    const seen: boolean[] = []
    const stop = observeReducedMotion(window, (reduced) => seen.push(reduced))
    expect(seen).toEqual([false])
    window.features = { reducedMotion: true, increasedContrast: false, forcedColors: false }
    window.emitMediaChange()
    expect(seen).toEqual([false, true])
    stop()
    window.features = { reducedMotion: false, increasedContrast: false, forcedColors: false }
    window.emitMediaChange()
    expect(seen).toEqual([false, true])
    expect(window.mediaChangeCalls).toEqual([true, false])
  })

  describe("the global matchMedia", () => {
    it("throws before a root is mounted", () => {
      expect(() => matchMedia("(hover)")).toThrow(/mounted/)
    })

    it("uses the window of the mounted root", () => {
      const window = fakeWindow()
      window.features = { reducedMotion: true, increasedContrast: false, forcedColors: false }
      const binding = createRendererState(window).attach()
      expect(matchMedia("(prefers-reduced-motion: reduce)").matches).toBe(true)
      binding.detach()
      expect(() => matchMedia("(hover)")).toThrow(/mounted/)
    })

    it("falls back to the previous root when the newest one unmounts", () => {
      const first = fakeWindow()
      first.features = { reducedMotion: true, increasedContrast: false, forcedColors: false }
      const second = fakeWindow()
      const firstBinding = createRendererState(first).attach()
      const secondBinding = createRendererState(second).attach()
      expect(matchMedia("(prefers-reduced-motion: reduce)").matches).toBe(false)
      secondBinding.detach()
      expect(matchMedia("(prefers-reduced-motion: reduce)").matches).toBe(true)
      firstBinding.detach()
      expect(() => matchMedia("(hover)")).toThrow(/mounted/)
    })

    it("keeps the newest root when an older one unmounts", () => {
      const first = fakeWindow()
      const second = fakeWindow()
      second.features = { reducedMotion: true, increasedContrast: false, forcedColors: false }
      const firstBinding = createRendererState(first).attach()
      const secondBinding = createRendererState(second).attach()
      firstBinding.detach()
      expect(matchMedia("(prefers-reduced-motion: reduce)").matches).toBe(true)
      secondBinding.detach()
    })
  })

  describe("teardown when the renderer throws", () => {
    it("surfaces a renderer error from unsubscribing", () => {
      const window = fakeWindow()
      const list = matchMediaFor(window)("(min-width: 1px)")
      const listener = () => {}
      list.addEventListener("change", listener)
      window.setWindowMediaChange = (enabled: boolean) => {
        if (!enabled) throw new Error("window closed")
      }
      expect(() => list.removeEventListener("change", listener)).toThrow(/window closed/)
    })

    it("leaves no entry behind when subscribing throws", () => {
      const window = fakeWindow()
      const setWindowMediaChange = window.setWindowMediaChange.bind(window)
      window.setWindowMediaChange = () => {
        throw new Error("window closed")
      }
      const list = matchMediaFor(window)("(min-width: 1px)")
      expect(() => list.addEventListener("change", () => {})).toThrow(/window closed/)
      window.setWindowMediaChange = setWindowMediaChange
      const other = matchMediaFor(window)("(min-width: 1px)")
      const listener = () => {}
      other.addEventListener("change", listener)
      other.removeEventListener("change", listener)
      expect(window.mediaChangeCalls).toEqual([true, false])
    })
  })
})
