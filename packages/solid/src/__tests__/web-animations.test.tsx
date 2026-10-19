import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { CSSAnimation, CSSTransition, defineKeyframes } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { HostElement } from "../host.js"

defineKeyframes("solid-wa-grow", { from: { width: 100 }, to: { width: 200 } })
defineKeyframes("solid-wa-tall", { from: { height: 10 }, to: { height: 30 } })

describe.skipIf(!hasNativeTestRenderer)("Solid Web animations", () => {
  const sizeOf = (app: ReturnType<typeof createTestRoot>) => {
    app.renderer.dispatchNativeEvents()
    return app.renderer.getElementBounds(app.renderer.findByTestId("box")!.id)!
  }

  it("lists a CSSAnimation per name in order and gives the same object every call", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    let box!: HostElement
    app.render(() => (
      <div
        ref={(element) => (box = element)}
        testId="box"
        style={{ width: 50, height: 20, animation: "solid-wa-tall 1s linear, solid-wa-grow 1s linear" }}
      />
    ))
    app.renderer.clockFastForward(0)
    const animations = box.getAnimations()
    expect(animations.map((animation) => animation.constructor.name)).toEqual(["CSSAnimation", "CSSAnimation"])
    expect(animations.map((animation) => (animation as CSSAnimation).animationName)).toEqual([
      "solid-wa-tall",
      "solid-wa-grow",
    ])
    const again = box.getAnimations()
    expect(again[0]).toBe(animations[0])
    expect(again[1]).toBe(animations[1])
    app.unmount()
  })

  it("pauses, seeks and reports a transition with its play state and current time", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    const [width, setWidth] = createSignal(50)
    let box!: HostElement
    app.render(() => (
      <div
        ref={(element) => (box = element)}
        testId="box"
        style={{ width: width(), height: 20, transition: "width 1s linear" }}
      />
    ))
    setWidth(150)
    app.flushSync(() => {})
    app.renderer.clockFastForward(0)
    const [transition] = box.getAnimations()
    expect(transition).toBeInstanceOf(CSSTransition)
    expect((transition as CSSTransition).transitionProperty).toBe("width")
    expect(transition.playState).toBe("running")
    expect(transition.currentTime).toBe(0)

    transition.pause()
    transition.currentTime = 250
    app.renderer.clockFastForward(5000)
    expect(transition.playState).toBe("paused")
    expect(transition.currentTime).toBe(250)
    expect(sizeOf(app).width).toBeCloseTo(75, 0)
    transition.currentTime = 1000
    app.renderer.clockFastForward(0)
    expect(sizeOf(app).width).toBeCloseTo(150, 0)
    app.unmount()
  })

  it("resolves finished with the animation, and rejects it with an AbortError when the name goes", async () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    const [animation, setAnimation] = createSignal("solid-wa-grow 1s linear")
    let box!: HostElement
    app.render(() => (
      <div ref={(element) => (box = element)} testId="box" style={{ width: 50, height: 20, animation: animation() }} />
    ))
    app.renderer.clockFastForward(0)
    const [first] = box.getAnimations()
    const finished = first.finished
    first.currentTime = 1000
    app.renderer.clockFastForward(16)
    app.renderer.dispatchNativeEvents()
    await expect(finished).resolves.toBe(first)
    expect(first.playState).toBe("finished")
    expect(first.finished).toBe(finished)

    first.play()
    expect(first.currentTime).toBe(0)
    expect(first.finished).not.toBe(finished)
    const second = first.finished
    setAnimation("none")
    app.flushSync(() => {})
    app.renderer.clockFastForward(16)
    app.renderer.dispatchNativeEvents()
    expect(first.playState).toBe("idle")
    expect(first.currentTime).toBeNull()
    const error = await second.then(
      () => undefined,
      (reason: unknown) => reason
    )
    expect(error).toBeInstanceOf(DOMException)
    expect((error as DOMException).name).toBe("AbortError")
    app.unmount()
  })

  it("has no animations for an element that is not mounted", () => {
    const app = createTestRoot()
    let box!: HostElement
    app.render(() => <div ref={(element) => (box = element)} testId="box" style={{ width: 50, height: 20 }} />)
    app.unmount()
    expect(box.getAnimations()).toEqual([])
  })
})
