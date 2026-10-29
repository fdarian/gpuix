import { describe, expect, it, vi } from "vitest"
import type { ElementWebAnimationInfo, WebAnimationInfo } from "../../index.js"
import { document, forgetWindowSource, installElementSource } from "../events/window-target.js"
import type { NativeRenderer } from "../host.js"
import { GpuixRenderer } from "../../index.js"
import { createNativeRenderer } from "../runtime.js"
import { CSSAnimation, cancelAnimationsOfElements, closeWebAnimations, getElementAnimations } from "../web-animations.js"

const info = (id: number, name: string): WebAnimationInfo => ({
  id,
  kind: "animation",
  animationName: name,
  currentTime: 0,
  endTime: 1000,
  playbackRate: 1,
  playState: "running",
  pending: false,
  startTime: 0,
})

/** A renderer that answers the reads `getAnimations` makes, with the animations it is given. */
function fakeRenderer(listed: ElementWebAnimationInfo[]): NativeRenderer {
  return {
    getAnimations: (elementId: number) => listed.filter((entry) => entry.elementId === elementId).map((entry) => entry.info),
    getSubtreeAnimations: () => listed,
    getDocumentAnimations: () => listed,
    getAnimation: () => {
      throw new Error("not read")
    },
    getAnimationEffect: () => ({}),
    getAnimationKeyframes: () => [],
    getAnimationTimelineTime: () => 0,
    pauseAnimation() {},
    playAnimation() {},
    cancelAnimation() {},
    finishAnimation() {},
    reverseAnimation() {},
    setAnimationCurrentTime() {},
    setAnimationPlaybackRate() {},
    updateAnimationPlaybackRate() {},
    setAnimationStartTime() {},
    releaseAnimation() {},
    forgetAnimation() {},
    updateAnimationTiming() {},
  } as unknown as NativeRenderer
}

describe("document.getAnimations()", () => {
  it("lists the animations of every installed renderer in the order the roots were installed", () => {
    const one = { id: 1 }
    const two = { id: 2 }
    const first = fakeRenderer([{ elementId: 1, info: info(10, "grow") }])
    const second = fakeRenderer([{ elementId: 1, info: info(20, "tall") }])
    const removeSecond = installElementSource(second, () => two)
    const removeFirst = installElementSource(first, () => one)
    try {
      const listed = document.getAnimations()
      expect(listed.map((animation) => (animation as CSSAnimation).animationName)).toEqual(["tall", "grow"])
      expect(listed[0].effect.target).toBe(two)
      expect(listed[1].effect.target).toBe(one)
      expect(document.getAnimations()[0]).toBe(listed[0])
      expect(getElementAnimations(first, 1, one)[0]).toBe(listed[1])
    } finally {
      removeSecond()
      removeFirst()
    }
    expect(document.getAnimations()).toEqual([])
  })

  it("throws for an animation whose element the root no longer has", () => {
    const renderer = fakeRenderer([{ elementId: 5, info: info(30, "grow") }])
    const remove = installElementSource(renderer, () => undefined)
    try {
      expect(() => document.getAnimations()).toThrow("Element 5 has an animation")
    } finally {
      remove()
    }
  })
})

describe("document.getAnimations() and a window that is gone", () => {
  it("lists nothing for a renderer whose window another took over, while its root keeps naming elements", () => {
    // Answers for another window, as a replaced test renderer does.
    const stale = fakeRenderer([{ elementId: 3, info: info(40, "grow") }])
    const live = fakeRenderer([{ elementId: 1, info: info(41, "tall") }])
    const element = {}
    const removeStale = installElementSource(stale, () => undefined)
    const removeLive = installElementSource(live, () => element)
    try {
      expect(() => document.getAnimations()).toThrow("Element 3 has an animation")
      forgetWindowSource(stale)
      const listed = document.getAnimations()
      expect(listed.map((animation) => (animation as CSSAnimation).animationName)).toEqual(["tall"])
      expect(listed[0].effect.target).toBe(element)
    } finally {
      removeStale()
      removeLive()
    }
  })

  it("stops listing a native renderer once its frame loop ended", () => {
    const running = vi.spyOn(GpuixRenderer.prototype, "tick")
    const listed = vi
      .spyOn(GpuixRenderer.prototype, "getDocumentAnimations")
      .mockReturnValue([{ elementId: 9, info: info(50, "grow") }])
    try {
      const renderer = createNativeRenderer()
      const remove = installElementSource(renderer, () => ({}))
      try {
        expect(document.getAnimations()).toHaveLength(1)
        running.mockReturnValueOnce(false)
        expect(renderer.tick()).toBe(false)
        expect(document.getAnimations()).toEqual([])
        expect(listed).toHaveBeenCalledTimes(1)
      } finally {
        remove()
      }
    } finally {
      vi.restoreAllMocks()
    }
  })
})

describe("the animations of a renderer whose window closed", () => {
  it("never tell a removed element's animation of its cancel after the renderer closed", async () => {
    const renderer = fakeRenderer([{ elementId: 1, info: info(60, "grow") }])
    const [animation] = getElementAnimations(renderer, 1, {})
    const heard: string[] = []
    animation.oncancel = () => heard.push("cancel")
    cancelAnimationsOfElements(renderer, [1])
    closeWebAnimations(renderer)
    await new Promise((resolve) => setTimeout(resolve, 20))
    expect(heard).toEqual([])
  })

  it("tells it while the renderer is open", async () => {
    const renderer = fakeRenderer([{ elementId: 1, info: info(61, "grow") }])
    const [animation] = getElementAnimations(renderer, 1, {})
    const heard: string[] = []
    animation.oncancel = () => heard.push("cancel")
    cancelAnimationsOfElements(renderer, [1])
    expect(heard).toEqual([])
    await new Promise((resolve) => setTimeout(resolve, 20))
    expect(heard).toEqual(["cancel"])
  })

  it("tells nothing of an element removed after it closed", async () => {
    const renderer = fakeRenderer([{ elementId: 1, info: info(62, "grow") }])
    const [animation] = getElementAnimations(renderer, 1, {})
    const heard: string[] = []
    animation.oncancel = () => heard.push("cancel")
    closeWebAnimations(renderer)
    cancelAnimationsOfElements(renderer, [1])
    await new Promise((resolve) => setTimeout(resolve, 20))
    expect(heard).toEqual([])
  })

  it("does not start a finished animation of a closed renderer from its microtask", async () => {
    const finished = { ...info(63, "grow"), playState: "finished", pending: true } as WebAnimationInfo
    const renderer = fakeRenderer([{ elementId: 1, info: finished }])
    renderer.getAnimation = () => finished
    const setTime = vi.spyOn(renderer, "setAnimationCurrentTime")
    getElementAnimations(renderer, 1, {})
    closeWebAnimations(renderer)
    await Promise.resolve()
    expect(setTime).not.toHaveBeenCalled()
  })
})
