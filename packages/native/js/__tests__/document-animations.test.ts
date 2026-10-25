import { describe, expect, it } from "vitest"
import type { ElementWebAnimationInfo, WebAnimationInfo } from "../../index.js"
import { document, installElementSource } from "../events/window-target.js"
import type { NativeRenderer } from "../host.js"
import { CSSAnimation, getElementAnimations } from "../web-animations.js"

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
