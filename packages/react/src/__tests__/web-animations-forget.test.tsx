/// A CSS animation the style cancelled is idle and out of every list, and native keeps the run for a command to bring back
/// as long as JS holds its `Animation`: an app that lets go of it lets native drop the run.

import v8 from "node:v8"
import vm from "node:vm"
import { describe, expect, it } from "vitest"
import { defineKeyframes, type Animation, type PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

v8.setFlagsFromString("--expose-gc")
const collect = vm.runInNewContext("gc") as () => void

defineKeyframes("forget-grow", { from: { width: 100 }, to: { width: 200 } })

const BOX = { width: 50, height: 20 }

function setup() {
  const { render, renderer, unmount } = createTestRoot()
  renderer.clockPause()
  renderer.setInlineDraws(false)
  const element: { current: PublicInstance | null } = { current: null }
  const show = (animation: string) =>
    render(
      <div
        testId="box"
        ref={(node: PublicInstance | null) => {
          element.current = node
        }}
        style={{ ...BOX, animation }}
      />
    )
  const frames = (count: number) => {
    for (let frame = 0; frame < count; frame++) {
      renderer.clockFastForward(16)
      renderer.drawFrame()
      renderer.dispatchNativeEvents()
    }
  }
  show("forget-grow 1s linear")
  renderer.getElementBounds(element.current!.id)
  frames(2)
  return { renderer, element, show, frames, unmount }
}

describeNative("An animation the style cancelled and nobody holds", () => {
  it("is kept while the app holds it, so play() runs it again", async () => {
    const { renderer, element, show, frames, unmount } = setup()
    try {
      const animation: Animation = element.current!.getAnimations()[0]
      const id = renderer.getAnimations(element.current!.id)[0].id
      show("none")
      renderer.getElementBounds(element.current!.id)
      frames(3)
      expect(animation.playState).toBe("idle")
      for (let round = 0; round < 3; round++) {
        collect()
        await new Promise((resolve) => setTimeout(resolve, 10))
      }
      expect(renderer.getAnimation(id).playState).toBe("idle")
      animation.play()
      frames(2)
      expect(animation.playState).toBe("running")
      expect(element.current!.getAnimations()).toEqual([animation])
    } finally {
      unmount()
    }
  })

  it("is dropped by native once the app lets go of it", async () => {
    const { renderer, element, show, frames, unmount } = setup()
    try {
      let animation: Animation | null = element.current!.getAnimations()[0]
      const id = renderer.getAnimations(element.current!.id)[0].id
      show("none")
      renderer.getElementBounds(element.current!.id)
      frames(3)
      expect(animation.playState).toBe("idle")
      expect(renderer.getAnimation(id).playState).toBe("idle")
      animation = null
      for (let round = 0; round < 20; round++) {
        collect()
        await new Promise((resolve) => setTimeout(resolve, 10))
        try {
          renderer.getAnimation(id)
        } catch {
          break
        }
      }
      expect(() => renderer.getAnimation(id)).toThrow(/No running CSS animation or transition has the id/)
    } finally {
      unmount()
    }
  })
})
