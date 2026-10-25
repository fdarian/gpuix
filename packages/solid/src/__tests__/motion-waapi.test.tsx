/// The Solid half of `motion-waapi.test.tsx` of the React package: Motion hands an `opacity` animation to the browser's
/// Web Animations, which `getAnimations()` then lists as a plain `Animation`, and runs an `x` animation in JavaScript, which
/// nothing lists. The expected shape is Chrome's (`packages/react/src/__tests__/fixtures/motion-waapi-oracle.json`), and so
/// is what the commands on the listed `Animation` do to the motion that made it.

import { describe, expect, it } from "bun:test"
import { motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { HostElement } from "../host.js"

describe.skipIf(!hasNativeTestRenderer)("Solid motion animations in getAnimations()", () => {
  const frame = (app: ReturnType<typeof createTestRoot>, ms: number) => {
    app.renderer.clockFastForward(ms)
    app.renderer.drawFrame()
    app.renderer.dispatchNativeEvents()
  }

  it("lists an opacity animation as an Animation from the frame after it is made, and drops it when it finishes", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    app.renderer.setInlineDraws(false)
    let box!: HostElement
    app.render(() => (
      <motion.div
        ref={(element: HostElement) => (box = element)}
        testId="box"
        style={{ width: 50, height: 20 }}
        animate={{ opacity: 0.5 }}
        transition={{ duration: 0.4, ease: "linear" }}
      />
    ))
    expect(box.getAnimations()).toEqual([])
    frame(app, 0)
    frame(app, 200)
    const [animation] = box.getAnimations()
    expect(box.getAnimations().length).toBe(1)
    expect(animation.constructor.name).toBe("Animation")
    expect(animation.id).toBe("")
    expect(animation.playState).toBe("running")
    expect(animation.currentTime).toBe(200)
    expect(animation.playbackRate).toBe(1)
    const timing = animation.effect.getTiming()
    expect([timing.delay, timing.endDelay, timing.duration, timing.iterations, timing.direction, timing.fill, timing.easing]).toEqual([
      0, 0, 400, 1, "normal", "both", "linear",
    ])
    const keyframes = animation.effect.getKeyframes()
    expect(keyframes.map((keyframe) => [keyframe.offset, keyframe.easing, keyframe.opacity])).toEqual([
      [null, "linear", "1"],
      [null, "linear", "0.5"],
    ])
    expect(box.getAnimations()[0]).toBe(animation)

    frame(app, 260)
    expect(box.getAnimations()).toEqual([])
    expect(animation.playState).toBe("idle")
    app.unmount()
  })

  it("lists nothing for an x animation, which Motion runs in JavaScript", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    app.renderer.setInlineDraws(false)
    let box!: HostElement
    app.render(() => (
      <motion.div
        ref={(element: HostElement) => (box = element)}
        testId="box"
        style={{ width: 50, height: 20 }}
        animate={{ x: 100 }}
        transition={{ duration: 0.4, ease: "linear" }}
      />
    ))
    frame(app, 0)
    frame(app, 200)
    expect(box.getAnimations()).toEqual([])
    app.unmount()
  })

  it("holds a listed opacity animation when it is paused, and a cancel takes the opacity back to where it started without a completion", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    app.renderer.setInlineDraws(false)
    let box!: HostElement
    let completions = 0
    app.render(() => (
      <motion.div
        ref={(element: HostElement) => (box = element)}
        testId="box"
        style={{ width: 50, height: 20 }}
        animate={{ opacity: 0.5 }}
        transition={{ duration: 0.4, ease: "linear" }}
        onMotionComplete={() => completions++}
      />
    ))
    frame(app, 0)
    frame(app, 100)
    const [animation] = box.getAnimations()
    expect(animation.currentTime).toBe(100)

    animation.pause()
    expect(animation.playState).toBe("paused")
    expect(animation.pending).toBe(true)
    frame(app, 0)
    expect(animation.pending).toBe(false)
    frame(app, 600)
    expect(animation.currentTime).toBe(100)
    expect(box.getAnimations()).toEqual([animation])
    expect(completions).toBe(0)

    animation.play()
    frame(app, 0)
    frame(app, 150)
    expect(animation.currentTime).toBe(250)

    animation.cancel()
    expect(box.getAnimations()).toEqual([])
    frame(app, 0)
    frame(app, 600)
    expect(animation.playState).toBe("idle")
    expect(completions).toBe(0)
    app.unmount()
  })

  it("completes the motion when the listed animation is finished", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    app.renderer.setInlineDraws(false)
    let box!: HostElement
    let completions = 0
    app.render(() => (
      <motion.div
        ref={(element: HostElement) => (box = element)}
        testId="box"
        style={{ width: 50, height: 20 }}
        animate={{ opacity: 0.5 }}
        transition={{ duration: 0.4, ease: "linear" }}
        onMotionComplete={() => completions++}
      />
    ))
    frame(app, 0)
    frame(app, 100)
    const [animation] = box.getAnimations()
    animation.finish()
    expect(animation.playState).toBe("finished")
    expect(completions).toBe(0)
    frame(app, 20)
    expect(box.getAnimations()).toEqual([])
    expect(completions).toBe(1)
    app.unmount()
  })
})
