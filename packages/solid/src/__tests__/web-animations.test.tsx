import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { CSSAnimation, CSSTransition, defineKeyframes, document } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { HostElement } from "../host.js"

defineKeyframes("solid-wa-grow", { from: { width: 100 }, to: { width: 200 } })
defineKeyframes("solid-wa-tall", { from: { height: 10 }, to: { height: 30 } })
defineKeyframes("solid-wa-move", {
  from: { transform: "translateX(10px)", boxShadow: "none" },
  to: { transform: "translateX(1em) rotate(0.5turn)", boxShadow: "0 0 4px oklch(0.7 0.1 200)" },
})
defineKeyframes("solid-wa-circle", {
  from: { r: 5, fill: "oklch(0.7 0.1 200)", strokeDasharray: "4 2" },
  to: { r: 25, fill: "#0000ff", strokeDasharray: "8 4" },
})

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

  describe("the playback interface", () => {
    /** A window that draws only where a browser would have a frame: `frame()`. */
    const stage = (animation: string) => {
      const app = createTestRoot()
      app.renderer.clockPause()
      app.renderer.setInlineDraws(false)
      let box!: HostElement
      app.render(() => (
        <div ref={(element) => (box = element)} testId="box" style={{ width: 50, height: 20, animation }} />
      ))
      app.renderer.getElementBounds(app.renderer.findByTestId("box")!.id)
      const frame = () => {
        app.renderer.clockFastForward(16)
        app.renderer.drawFrame()
        app.renderer.dispatchNativeEvents()
      }
      return { app, frame, animations: () => box.getAnimations() }
    }

    it("is pending until the first frame, which resolves ready and sets the start time", async () => {
      const { app, frame, animations } = stage("solid-wa-grow 1s linear")
      const [animation] = animations()
      expect(animation.pending).toBe(true)
      expect(animation.startTime).toBeNull()
      expect(animation.timeline.currentTime).toBeGreaterThanOrEqual(0)
      const ready = animation.ready
      let resolved = false
      void ready.then(() => (resolved = true))
      await Promise.resolve()
      expect(resolved).toBe(false)
      frame()
      await expect(ready).resolves.toBe(animation)
      expect(animation.pending).toBe(false)
      expect(animation.startTime).toBe(animation.timeline.currentTime)
      expect(animation.ready).toBe(ready)
      app.unmount()
    })

    it("cancels: idle, unlisted, finished rejected at once, the event with the next frame, and not started again by the style", async () => {
      const { app, frame, animations } = stage("solid-wa-grow 1s linear")
      const [animation] = animations()
      frame()
      const heard: string[] = []
      animation.oncancel = (event) => heard.push(`oncancel ${event.currentTime} ${event.timelineTime! < animation.timeline.currentTime}`)
      animation.addEventListener("cancel", (event) => heard.push(`cancel ${event.type}`))
      const finished = animation.finished
      animation.currentTime = 300
      animation.cancel()
      expect(animation.playState).toBe("idle")
      expect(animation.currentTime).toBeNull()
      expect(animation.startTime).toBeNull()
      expect(animations()).toEqual([])
      await expect(finished).rejects.toMatchObject({ name: "AbortError" })
      expect(heard).toEqual([])
      frame()
      expect(heard).toEqual(["oncancel null true", "cancel cancel"])
      frame()
      expect(animation.playState).toBe("idle")
      expect(animations()).toEqual([])

      animation.play()
      expect(animation.playState).toBe("running")
      expect(animation.currentTime).toBe(0)
      expect(animation.pending).toBe(true)
      expect(animations()).toEqual([animation])
      frame()
      expect(animation.pending).toBe(false)
      app.unmount()
    })

    it("finishes at the end and fires finish once with the time it was at; at a rate of 0 it throws", async () => {
      const { app, frame, animations } = stage("solid-wa-grow 1s linear forwards")
      const [animation] = animations()
      frame()
      const heard: string[] = []
      animation.onfinish = (event) => heard.push(`onfinish ${event.currentTime}`)
      const finished = animation.finished
      animation.finish()
      expect(animation.playState).toBe("finished")
      expect(animation.currentTime).toBe(1000)
      await expect(finished).resolves.toBe(animation)
      expect(heard).toEqual([])
      frame()
      expect(heard).toEqual(["onfinish 1000"])
      frame()
      expect(heard).toEqual(["onfinish 1000"])

      animation.play()
      animation.playbackRate = 0
      expect(() => animation.finish()).toThrow(DOMException)
      try {
        animation.finish()
      } catch (error) {
        expect((error as DOMException).name).toBe("InvalidStateError")
      }
      app.unmount()
    })

    it("reverses at the next frame, keeping the time, with a new ready", async () => {
      const { app, frame, animations } = stage("solid-wa-grow 1s linear both")
      const [animation] = animations()
      frame()
      animation.currentTime = 300
      const ready = animation.ready
      animation.reverse()
      expect(animation.playbackRate).toBe(1)
      expect(animation.pending).toBe(true)
      expect(animation.ready).not.toBe(ready)
      const reversed = animation.ready
      frame()
      await expect(reversed).resolves.toBe(animation)
      expect(animation.playbackRate).toBe(-1)
      // The time ran on at the old rate for the 16 ms until the frame that flipped it.
      expect(animation.currentTime).toBeCloseTo(316, 3)
      app.unmount()
    })

    it("sets the start time to seek, and null to hold the time", () => {
      const { app, frame, animations } = stage("solid-wa-grow 1s linear both")
      const [animation] = animations()
      frame()
      const now = animation.timeline.currentTime
      animation.startTime = now - 300
      expect(animation.currentTime).toBe(300)
      expect(animation.startTime).toBe(now - 300)
      animation.startTime = null
      expect(animation.playState).toBe("paused")
      expect(animation.currentTime).toBe(300)
      expect(animation.startTime).toBeNull()
      expect(() => (animation.startTime = Number.NaN)).toThrow(TypeError)
      app.unmount()
    })

    it("updates the playback rate at the next frame when running and at once when paused", () => {
      const { app, frame, animations } = stage("solid-wa-grow 1s linear both")
      const [animation] = animations()
      frame()
      animation.updatePlaybackRate(2)
      expect(animation.playbackRate).toBe(1)
      expect(animation.pending).toBe(true)
      frame()
      expect(animation.playbackRate).toBe(2)
      animation.pause()
      frame()
      animation.updatePlaybackRate(3)
      expect(animation.playbackRate).toBe(3)
      expect(animation.pending).toBe(false)
      app.unmount()
    })
  })

  describe("effect", () => {
    const stage = (animation: string) => {
      const app = createTestRoot()
      app.renderer.clockPause()
      let box!: HostElement
      app.render(() => <div ref={(element) => (box = element)} testId="box" style={{ width: 50, height: 20, animation }} />)
      app.renderer.clockFastForward(0)
      return { app, box }
    }

    it("names the element and the keyframes of an animation, and keeps one effect for it", () => {
      const { app, box } = stage("solid-wa-grow 1s ease-in")
      const [animation] = box.getAnimations()
      expect(animation.effect.target).toBe(box)
      expect(animation.effect).toBe(animation.effect)
      expect(animation.effect.getKeyframes()).toEqual([
        { offset: 0, computedOffset: 0, easing: "ease-in", composite: "auto", width: "100px" },
        { offset: 1, computedOffset: 1, easing: "ease-in", composite: "auto", width: "200px" },
      ])
      app.unmount()
    })

    it("reports the timing in milliseconds, with a linear effect easing for an animation", () => {
      const { app, box } = stage("solid-wa-grow 2s ease-in 500ms 3 alternate both")
      const [animation] = box.getAnimations()
      expect(animation.effect.getTiming()).toEqual({
        delay: 500,
        endDelay: 0,
        fill: "both",
        iterationStart: 0,
        iterations: 3,
        duration: 2000,
        direction: "alternate",
        easing: "linear",
      })
      expect(animation.effect.getComputedTiming()).toMatchObject({ endTime: 6500, activeDuration: 6000, localTime: 0, progress: 0 })
      app.unmount()
    })

    it("keeps what updateTiming sets against a later style and refuses a bad member", () => {
      const app = createTestRoot()
      app.renderer.clockPause()
      const [animation, setAnimation] = createSignal("solid-wa-grow 1s linear")
      let box!: HostElement
      app.render(() => <div ref={(element) => (box = element)} testId="box" style={{ width: 50, height: 20, animation: animation() }} />)
      app.renderer.clockFastForward(0)
      const [running] = box.getAnimations()
      running.effect.updateTiming({ duration: 2000, iterations: Infinity })
      setAnimation("solid-wa-grow 5s linear 100ms")
      app.flushSync(() => {})
      app.renderer.clockFastForward(0)
      expect(running.effect.getTiming()).toMatchObject({ duration: 2000, iterations: Infinity, delay: 100 })
      expect(() => running.effect.updateTiming({ duration: -1 })).toThrow(TypeError)
      expect(() => running.effect.updateTiming({ easing: "nope" })).toThrow(TypeError)
      expect(running.effect.getTiming().duration).toBe(2000)
      app.unmount()
    })

    it("writes a transform as the functions it was written with, and a colour in the space it was written in", () => {
      const { app, box } = stage("solid-wa-move 1s cubic-bezier(0.25, 0.1, 0.25, 1)")
      const [animation] = box.getAnimations()
      expect(animation.effect.getKeyframes()).toEqual([
        {
          offset: 0,
          computedOffset: 0,
          easing: "cubic-bezier(0.25, 0.1, 0.25, 1)",
          composite: "auto",
          transform: "translateX(10px)",
          boxShadow: "none",
        },
        {
          offset: 1,
          computedOffset: 1,
          easing: "cubic-bezier(0.25, 0.1, 0.25, 1)",
          composite: "auto",
          transform: "translateX(16px) rotate(180deg)",
          boxShadow: "oklch(0.7 0.1 200) 0px 0px 4px 0px",
        },
      ])
      app.unmount()
    })

    it("writes the keyframes of a shape: lengths in px, a paint as a colour, a dash list with commas", () => {
      const app = createTestRoot()
      app.renderer.clockPause()
      let circle!: HostElement
      app.render(() => (
        <svg width={100} height={100} viewBox="0 0 100 100">
          <circle ref={(element) => (circle = element)} cx={50} cy={50} r={10} style={{ animation: "solid-wa-circle 1s step-start" }} />
        </svg>
      ))
      app.renderer.clockFastForward(0)
      const [animation] = circle.getAnimations()
      expect(animation.effect.getKeyframes()).toEqual([
        {
          offset: 0,
          computedOffset: 0,
          easing: "steps(1, start)",
          composite: "auto",
          r: "5px",
          fill: "oklch(0.7 0.1 200)",
          strokeDasharray: "4px, 2px",
        },
        {
          offset: 1,
          computedOffset: 1,
          easing: "steps(1, start)",
          composite: "auto",
          r: "25px",
          fill: "rgb(0, 0, 255)",
          strokeDasharray: "8px, 4px",
        },
      ])
      app.unmount()
    })

    it("gives a transition its computed start and end, and its easing to the effect", () => {
      const app = createTestRoot()
      app.renderer.clockPause()
      const [width, setWidth] = createSignal(50)
      let box!: HostElement
      app.render(() => (
        <div ref={(element) => (box = element)} testId="box" style={{ width: width(), height: 20, transition: "width 1s ease-in 200ms" }} />
      ))
      setWidth(150)
      app.flushSync(() => {})
      app.renderer.clockFastForward(0)
      const [transition] = box.getAnimations()
      expect(transition.effect.getKeyframes()).toEqual([
        { offset: 0, computedOffset: 0, easing: "linear", composite: "auto", width: "50px" },
        { offset: 1, computedOffset: 1, easing: "linear", composite: "auto", width: "150px" },
      ])
      expect(transition.effect.getTiming()).toMatchObject({ delay: 200, duration: 1000, fill: "backwards", easing: "ease-in" })
      transition.effect.updateTiming({ delay: 0 })
      expect(transition.effect.getComputedTiming().endTime).toBe(1000)
      app.unmount()
    })
  })

  it("lists the animations of the subtree, transitions first, in document order", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    const [margin, setMargin] = createSignal(0)
    let parent!: HostElement
    let child!: HostElement
    let sibling!: HostElement
    app.render(() => (
      <div ref={(element) => (parent = element)} testId="parent" style={{ width: 50, height: 20, animation: "solid-wa-tall 1s linear" }}>
        <div ref={(element) => (child = element)} style={{ width: 50, height: 20, animation: "solid-wa-grow 1s linear" }} />
        <div
          ref={(element) => (sibling = element)}
          style={{ width: 50, height: 20, marginTop: margin(), transition: "margin-top 1s linear" }}
        />
      </div>
    ))
    setMargin(10)
    app.flushSync(() => {})
    app.renderer.clockFastForward(0)
    const names = (animations: ReturnType<HostElement["getAnimations"]>) =>
      animations.map((animation) => (animation instanceof CSSTransition ? animation.transitionProperty : (animation as CSSAnimation).animationName))
    expect(names(parent.getAnimations())).toEqual(["solid-wa-tall"])
    const subtree = parent.getAnimations({ subtree: true })
    expect(names(subtree)).toEqual(["margin-top", "solid-wa-tall", "solid-wa-grow"])
    expect(subtree.map((animation) => animation.effect.target)).toEqual([sibling, parent, child])
    expect(subtree[1]).toBe(parent.getAnimations()[0])
    expect(subtree[2]).toBe(child.getAnimations()[0])
    expect(names(child.getAnimations({ subtree: true }))).toEqual(["solid-wa-grow"])
    expect(sibling.getAnimations({ subtree: false })).toHaveLength(1)
    expect(sibling.getAnimations({ subtree: false })[0]).toBe(subtree[0])
    app.unmount()
  })

  it("lists the animations of the window in document.getAnimations(), the objects the elements give", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    let first!: HostElement
    let second!: HostElement
    app.render(() => (
      <div>
        <div ref={(element) => (first = element)} style={{ width: 50, height: 20, animation: "solid-wa-grow 1s linear" }} />
        <div ref={(element) => (second = element)} style={{ width: 50, height: 20, animation: "solid-wa-tall 1s linear" }} />
      </div>
    ))
    app.renderer.clockFastForward(0)
    const listed = document.getAnimations()
    expect(listed).toHaveLength(2)
    expect(listed[0]).toBe(first.getAnimations()[0])
    expect(listed[1]).toBe(second.getAnimations()[0])
    expect(listed[1].effect.target).toBe(second)
    app.unmount()
    expect(document.getAnimations()).toHaveLength(0)
  })

  it("lists the animations of the latest window only when an earlier root was never unmounted", () => {
    // The test window is one per thread: `left` is never unmounted, and its renderer would answer
    // for the window of `app`.
    const left = createTestRoot()
    left.render(() => <div testId="left" style={{ width: 50, height: 20 }} />)
    const app = createTestRoot()
    app.renderer.clockPause()
    let box!: HostElement
    app.render(() => <div ref={(element) => (box = element)} style={{ width: 50, height: 20, animation: "solid-wa-grow 1s linear" }} />)
    app.renderer.clockFastForward(0)
    const listed = document.getAnimations()
    expect(listed).toHaveLength(1)
    expect(listed[0]).toBe(box.getAnimations()[0])
    app.unmount()
  })

  it("does not tell a disposed renderer's removed element of its animation's cancel", async () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    const [shown, setShown] = createSignal(true)
    let box: HostElement | undefined
    app.render(() => (
      <div>
        {shown() && <div ref={(element) => (box = element)} style={{ width: 50, height: 20, animation: "solid-wa-grow 1s linear" }} />}
      </div>
    ))
    app.renderer.clockFastForward(0)
    const heard: string[] = []
    const animation = box!.getAnimations()[0]
    animation.oncancel = () => {
      heard.push("cancel")
      animation.finish()
    }
    setShown(false)
    app.unmount()
    const errors: unknown[] = []
    const record = (error: unknown) => errors.push(error)
    process.on("uncaughtException", record)
    await new Promise((resolve) => setTimeout(resolve, 20))
    process.off("uncaughtException", record)
    expect(heard).toEqual([])
    expect(errors).toEqual([])
  })

  it("has no animations for an element that is not mounted", () => {
    const app = createTestRoot()
    let box!: HostElement
    app.render(() => <div ref={(element) => (box = element)} testId="box" style={{ width: 50, height: 20 }} />)
    app.unmount()
    expect(box.getAnimations()).toEqual([])
  })
})
