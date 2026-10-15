import { describe, expect, it, vi } from "vitest"
import {
  defineKeyframes,
  registerKeyframesHost,
  unregisterKeyframesHost,
  type KeyframesHost,
} from "../keyframes.js"
import { GpuixRenderer } from "../../index.js"
import { createNativeRenderer } from "../runtime.js"
import { TestRenderer, hasNativeTestRenderer } from "../testing.js"

const fakeHost = () => {
  const calls: Array<[string, unknown]> = []
  const host: KeyframesHost = {
    defineKeyframes: vi.fn((name: string, framesJson: string) => {
      calls.push([name, JSON.parse(framesJson)])
    }),
  }
  return { host, calls }
}

describe("keyframes registry", () => {
  it("sends earlier definitions to a host that registers later, and later ones to every host", () => {
    defineKeyframes("kf-early", { from: { opacity: 0 }, to: { opacity: 1 } })
    const first = fakeHost()
    registerKeyframesHost(first.host)
    expect(first.calls).toContainEqual(["kf-early", { from: { opacity: 0 }, to: { opacity: 1 } }])

    const second = fakeHost()
    registerKeyframesHost(second.host)
    defineKeyframes("kf-late", { "0%, 100%": { width: 10 } })
    expect(first.calls).toContainEqual(["kf-late", { "0%, 100%": { width: 10 } }])
    expect(second.calls).toContainEqual(["kf-late", { "0%, 100%": { width: 10 } }])
  })

  it("registers a host once", () => {
    const { host, calls } = fakeHost()
    registerKeyframesHost(host)
    registerKeyframesHost(host)
    const before = calls.length
    defineKeyframes("kf-once", { to: { opacity: 0 } })
    expect(calls.length - before).toBe(1)
  })

  it("redefining a name replaces it for hosts that register afterwards", () => {
    defineKeyframes("kf-replaced", { to: { opacity: 0 } })
    defineKeyframes("kf-replaced", { to: { opacity: 0.5 } })
    const { host, calls } = fakeHost()
    registerKeyframesHost(host)
    expect(calls.filter(([name]) => name === "kf-replaced")).toEqual([
      ["kf-replaced", { to: { opacity: 0.5 } }],
    ])
  })

  it("keeps the registry and every other host in step when one host throws", () => {
    let closed = false
    const broken = {
      defineKeyframes: vi.fn(() => {
        if (closed) throw new Error("window closed")
      }),
    }
    const before = fakeHost()
    const after = fakeHost()
    registerKeyframesHost(before.host)
    registerKeyframesHost(broken)
    registerKeyframesHost(after.host)
    closed = true
    expect(() => defineKeyframes("kf-broken-host", { to: { opacity: 0.9 } })).toThrow("window closed")
    for (const { calls } of [before, after]) {
      expect(calls.filter(([name]) => name === "kf-broken-host")).toEqual([["kf-broken-host", { to: { opacity: 0.9 } }]])
    }
    const later = fakeHost()
    registerKeyframesHost(later.host)
    expect(later.calls.filter(([name]) => name === "kf-broken-host")).toEqual([["kf-broken-host", { to: { opacity: 0.9 } }]])
    unregisterKeyframesHost(broken)
  })

  it("rejects a missing name and non-object frames", () => {
    expect(() => defineKeyframes("", {})).toThrow(/needs a keyframes name/)
    expect(() => defineKeyframes("kf-bad", [] as never)).toThrow(/needs an object/)
    expect(() => defineKeyframes("kf-bad", null as never)).toThrow(/needs an object/)
  })
})

describe("validating at define time", () => {
  it("throws from defineKeyframes with only a host that accepts anything, and keeps nothing", () => {
    const { host, calls } = fakeHost()
    registerKeyframesHost(host)
    expect(() => defineKeyframes("kf-invalid", { to: { opacity: 5 } })).toThrow(/keyframes kf-invalid: to: opacity/)
    expect(() => defineKeyframes("kf-invalid", { middle: {} })).toThrow(/not a keyframe selector/)
    expect(() => defineKeyframes("none", { to: { opacity: 1 } })).toThrow(/animation name/)
    expect(calls.filter(([name]) => name === "kf-invalid")).toEqual([])
    const later = fakeHost()
    registerKeyframesHost(later.host)
    expect(later.calls.filter(([name]) => name === "kf-invalid")).toEqual([])
  })
})

describe("unregistering a host", () => {
  it("stops sending definitions to it and leaves the others", () => {
    const gone = fakeHost()
    const kept = fakeHost()
    registerKeyframesHost(gone.host)
    registerKeyframesHost(kept.host)
    unregisterKeyframesHost(gone.host)
    defineKeyframes("kf-after-unregister", { to: { opacity: 0 } })
    expect(gone.calls.filter(([name]) => name === "kf-after-unregister")).toEqual([])
    expect(kept.calls.filter(([name]) => name === "kf-after-unregister")).toHaveLength(1)
  })

  it("happens when the native renderer's window closes", () => {
    // No window can open here, so the native methods are stood in for.
    const running = vi.spyOn(GpuixRenderer.prototype, "tick")
    const define = vi.spyOn(GpuixRenderer.prototype, "defineKeyframes").mockReturnValue(undefined)
    const renderer = createNativeRenderer()
    registerKeyframesHost(renderer)
    running.mockReturnValueOnce(true)
    expect(renderer.tick()).toBe(true)
    defineKeyframes("kf-while-open", { to: { opacity: 0 } })
    expect(define).toHaveBeenCalledWith("kf-while-open", expect.any(String))
    running.mockReturnValueOnce(false)
    expect(renderer.tick()).toBe(false)
    define.mockClear()
    defineKeyframes("kf-after-close", { to: { opacity: 0 } })
    expect(define).not.toHaveBeenCalled()
    vi.restoreAllMocks()
  })
})

describe.skipIf(!hasNativeTestRenderer)("keyframes on the native test renderer", () => {
  it("validates a definition natively and keeps the earlier one when it is bad", () => {
    const renderer = new TestRenderer({ width: 100, height: 100 })
    defineKeyframes("kf-native", { from: { opacity: 0 }, to: { opacity: 1, animationTimingFunction: "ease-in" } })
    expect(() => defineKeyframes("kf-native", { to: { opacity: 5 } })).toThrow(/keyframes kf-native: to: opacity/)
    expect(() => defineKeyframes("kf-native", { middle: {} })).toThrow(/not a keyframe selector/)
    expect(() => defineKeyframes("kf-native", { to: { display: "none" } })).toThrow(/cannot be animated/)
    expect(() => defineKeyframes("none", { to: { opacity: 1 } })).toThrow(/animation name/)
    renderer.flush()
  })

  it("accepts the animation shorthand and longhands in a style and rejects a bad one", () => {
    const renderer = new TestRenderer({ width: 100, height: 100 })
    renderer.applyBatch(
      JSON.stringify([
        ["createElement", 1, "div"],
        ["setRoot", 1],
        ["setStyle", 1, { animation: "spin 1s linear infinite alternate", hover: { animationPlayState: "paused" } }],
      ])
    )
    expect(() =>
      renderer.applyBatch(JSON.stringify([["setStyle", 1, { animationDuration: "fast" }]]))
    ).toThrow(/animationDuration/)
    expect(() =>
      renderer.applyBatch(JSON.stringify([["setStyle", 1, { hover: { animation: "a b 1s" } }]]))
    ).toThrow(/hover\.animation/)
    renderer.flush()
  })
})
