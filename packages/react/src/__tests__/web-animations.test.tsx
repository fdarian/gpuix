/// `element.getAnimations()` against headless Chrome: `fixtures/web-animations-oracle.html` runs each case in Chrome
/// and `fixtures/web-animations-oracle.json` is what it reports after every step (see the page for the step
/// vocabulary). Each case is replayed here on the test clock, paused, so a step takes no time; where Chrome waits for
/// frames (`settle`) a frame without time passing is drawn and the events it queued are delivered. What a step shows
/// is compared as Chrome shows it: the width and height of the element, and for every animation the first
/// `getAnimations()` returned its class, name, play state, current time, rate and, for the cases that play again after
/// a finish, which `finished` promise it holds. The cases the oracle cannot say are below it.

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import {
  Animation,
  CSSAnimation,
  CSSTransition,
  defineKeyframes,
  type PublicInstance,
} from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Step = [string, ...unknown[]]
interface AnimationRecord {
  kind: string
  name: string
  playState: string
  currentTime: number | null
  playbackRate: number
  finishedGeneration: number
}
interface Snapshot {
  step: Step
  width: number | null
  height: number | null
  animations: AnimationRecord[]
  list?: [string, string][]
  finished?: string[][]
  same?: boolean
}
interface Case {
  name: string
  style: Record<string, unknown>
  change: Record<string, unknown> | null
  steps: Step[]
  records: Snapshot[]
}
const ORACLE: {
  keyframes: Record<string, Record<string, Record<string, number>>>
  base: Record<string, string>
  cases: Case[]
} = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/web-animations-oracle.json"),
    "utf8"
  )
)

for (const [name, frames] of Object.entries(ORACLE.keyframes)) defineKeyframes(name, frames)

/** `"50px"` as the number a style takes. */
const toStyle = (style: Record<string, unknown>): Record<string, unknown> =>
  Object.fromEntries(
    Object.entries(style).map(([key, value]) => [
      key,
      typeof value === "string" && /^-?[\d.]+px$/.test(value) ? Number.parseFloat(value) : value,
    ])
  )

const nextTask = () => new Promise<void>((resolve) => setTimeout(resolve, 0))

/** How a `finished` promise stands, as the oracle page reads it. */
const standing = (promise: Promise<Animation>, animation: Animation): Promise<string> =>
  Promise.race([
    promise.then(
      (value) => (value === animation ? "resolved" : "resolved-other"),
      (error) => `rejected:${error instanceof DOMException ? error.name : String(error)}`
    ),
    nextTask().then(() => "pending"),
  ])

function nameOf(animation: Animation): string {
  if (animation instanceof CSSAnimation) return animation.animationName
  if (animation instanceof CSSTransition) return animation.transitionProperty
  throw new Error("An animation that is neither a CSSAnimation nor a CSSTransition")
}

async function replay(scenario: Case): Promise<Snapshot[]> {
  const { render, renderer, unmount } = createTestRoot()
  renderer.clockPause()
  const element: { current: PublicInstance | null } = { current: null }
  let style: Record<string, unknown> = { ...ORACLE.base, ...scenario.style }
  const box = () => (
    <div
      testId="box"
      ref={(node: PublicInstance | null) => {
        element.current = node
      }}
      style={toStyle(style)}
    />
  )
  const frame = () => {
    renderer.clockFastForward(0)
    renderer.dispatchNativeEvents()
  }
  try {
    render(box())
    if (scenario.change) {
      style = { ...style, ...scenario.change }
      render(box())
    }
    frame()
    const tracked = element.current!.getAnimations()
    const promises: Promise<Animation>[][] = tracked.map(() => [])

    const read = (): Omit<Snapshot, "step"> => {
      frame()
      const mounted = element.current !== null
      const bounds = mounted ? renderer.getElementBounds(element.current!.id) : null
      return {
        width: bounds ? bounds.width : null,
        height: bounds ? bounds.height : null,
        animations: tracked.map((animation, index) => {
          const seen = promises[index]
          const finished = animation.finished
          if (!seen.includes(finished)) seen.push(finished)
          return {
            kind: animation.constructor.name,
            name: nameOf(animation),
            playState: animation.playState,
            currentTime: animation.currentTime,
            playbackRate: animation.playbackRate,
            finishedGeneration: seen.indexOf(finished),
          }
        }),
      }
    }

    const records: Snapshot[] = [{ step: ["start"], ...read() }]
    for (const step of scenario.steps) {
      const [op, first, second] = step
      const record: Partial<Snapshot> & { step: Step } = { step }
      if (op === "pause") tracked[first as number].pause()
      else if (op === "play") tracked[first as number].play()
      else if (op === "seek") tracked[first as number].currentTime = second as number
      else if (op === "rate") tracked[first as number].playbackRate = second as number
      else if (op === "style") {
        style = { ...style, ...(first as Record<string, unknown>) }
        render(box())
      } else if (op === "remove") render(<></>)
      else if (op === "settle") {
        frame()
        record.finished = await Promise.all(
          tracked.map((animation, index) => Promise.all(promises[index].map((promise) => standing(promise, animation))))
        )
      }
      if (op === "list") {
        record.list = element.current!.getAnimations().map((animation) => [animation.constructor.name, nameOf(animation)])
      }
      if (op === "same") {
        const again = element.current!.getAnimations()
        const once = element.current!.getAnimations()
        record.same =
          again.length === once.length && again.every((animation, index) => animation === once[index] && animation === tracked[index])
      }
      records.push({ ...record, ...read() } as Snapshot)
    }
    return records
  } finally {
    unmount()
  }
}

describeNative("Web animations against Chrome", () => {
  for (const scenario of ORACLE.cases) {
    // The first window of a loaded machine can take longer than the default 5s to open.
    it(scenario.name, { timeout: 30_000 }, async () => {
      const got = await replay(scenario)
      expect(got.length).toBe(scenario.records.length)
      scenario.records.forEach((want, index) => {
        const here = got[index]
        const where = `${scenario.name}, after ${JSON.stringify(want.step)}`
        expect(here.step, where).toEqual(want.step)
        if (want.width === null) expect(here.width, `${where}: width`).toBeNull()
        else expect(here.width, `${where}: width`).toBeCloseTo(want.width, 0)
        if (want.height === null) expect(here.height, `${where}: height`).toBeNull()
        else expect(here.height, `${where}: height`).toBeCloseTo(want.height, 0)
        want.animations.forEach((expected, animation) => {
          const actual = here.animations[animation]
          const label = `${where}: animation ${animation}`
          expect(actual.kind, label).toBe(expected.kind)
          expect(actual.name, label).toBe(expected.name)
          expect(actual.playState, `${label} playState`).toBe(expected.playState)
          if (expected.currentTime === null) expect(actual.currentTime, `${label} currentTime`).toBeNull()
          else expect(actual.currentTime, `${label} currentTime`).toBeCloseTo(expected.currentTime, 3)
          expect(actual.playbackRate, `${label} playbackRate`).toBe(expected.playbackRate)
          if (want.step[0] === "play") {
            expect(actual.finishedGeneration, `${label} finished promise`).toBe(expected.finishedGeneration)
          }
        })
        if (want.list) expect(here.list, `${where}: getAnimations()`).toEqual(want.list)
        if (want.same !== undefined) expect(here.same, `${where}: the same objects`).toBe(want.same)
        if (want.finished) {
          // The promise the animation held first: a cancel rejects it and leaves a pending one.
          expect(here.finished!.map((promises) => promises[0]), `${where}: finished`).toEqual(
            want.finished.map((promises) => promises[0])
          )
        }
      })
    })
  }
})

describeNative("Web animations beyond what Chrome can pin on a paused page", () => {
  const widthOf = (renderer: ReturnType<typeof createTestRoot>["renderer"]) => {
    renderer.dispatchNativeEvents()
    return renderer.getElementBounds(renderer.findByTestId("box").id).width
  }
  const grower = (animation: string, ref: (node: PublicInstance | null) => void) => (
    <div testId="box" ref={ref} style={{ width: 50, height: 20, animation }} />
  )

  it("runs time at the playbackRate on the test clock and finishes when it gets to the end", async () => {
    const { render, renderer, unmount } = createTestRoot()
    renderer.clockPause()
    const element: { current: PublicInstance | null } = { current: null }
    render(grower("grow 1s linear forwards", (node) => {
        element.current = node
      }))
    renderer.clockFastForward(0)
    const [animation] = element.current!.getAnimations()
    animation.playbackRate = 2
    renderer.clockFastForward(250)
    expect(animation.currentTime).toBeCloseTo(500, 3)
    expect(widthOf(renderer)).toBeCloseTo(150, 0)
    animation.playbackRate = -1
    renderer.clockFastForward(100)
    expect(animation.currentTime).toBeCloseTo(400, 3)
    expect(widthOf(renderer)).toBeCloseTo(140, 0)
    animation.playbackRate = 1
    renderer.clockFastForward(1000)
    expect(animation.playState).toBe("finished")
    expect(animation.currentTime).toBeCloseTo(1000, 3)
    renderer.dispatchNativeEvents()
    await expect(animation.finished).resolves.toBe(animation)
    unmount()
  })

  it("plays a paused animation from the time it holds", () => {
    const { render, renderer, unmount } = createTestRoot()
    renderer.clockPause()
    const element: { current: PublicInstance | null } = { current: null }
    render(grower("grow 1s linear", (node) => {
        element.current = node
      }))
    renderer.clockFastForward(0)
    const [animation] = element.current!.getAnimations()
    renderer.clockFastForward(300)
    animation.pause()
    renderer.clockFastForward(5000)
    expect(animation.currentTime).toBeCloseTo(300, 3)
    animation.play()
    expect(animation.playState).toBe("running")
    // The frame that play() asks for is drawn at once on the test clock, so time runs from the call.
    renderer.clockFastForward(100)
    renderer.clockFastForward(200)
    expect(animation.currentTime).toBeCloseTo(600, 0)
    expect(widthOf(renderer)).toBeCloseTo(160, 0)
    unmount()
  })

  it("keeps a finished animation listed through its fill and finishes a transition once", async () => {
    const { render, renderer, unmount } = createTestRoot()
    renderer.clockPause()
    const element: { current: PublicInstance | null } = { current: null }
    const target = (width: number) => (
      <div
        testId="box"
        ref={(node: PublicInstance | null) => {
          element.current = node
        }}
        style={{ width, height: 20, transition: "width 1s linear" }}
      />
    )
    render(target(50))
    render(target(150))
    renderer.clockFastForward(0)
    const [transition] = element.current!.getAnimations()
    expect(transition).toBeInstanceOf(CSSTransition)
    renderer.clockFastForward(1500)
    renderer.dispatchNativeEvents()
    expect(element.current!.getAnimations()).toEqual([])
    expect(transition.playState).toBe("finished")
    expect(transition.currentTime).toBe(1000)
    await expect(transition.finished).resolves.toBe(transition)
    transition.play()
    expect(transition.playState).toBe("running")
    expect(transition.currentTime).toBe(0)
    expect(element.current!.getAnimations()).toEqual([transition])
    unmount()
  })

  describe("reading an animation before JS has heard that it ended", () => {
    const setup = () => {
      const root = createTestRoot()
      root.renderer.clockPause()
      const element: { current: PublicInstance | null } = { current: null }
      const target = (width: number) => (
        <div
          testId="box"
          ref={(node: PublicInstance | null) => {
            element.current = node
          }}
          style={{ width, height: 20, transition: "width 1s linear" }}
        />
      )
      root.render(target(50))
      root.render(target(150))
      root.renderer.clockFastForward(0)
      root.renderer.dispatchNativeEvents()
      return { ...root, element, target, transition: () => element.current!.getAnimations()[0] }
    }

    it("answers for a transition in the task of the frame that finished it", async () => {
      const { renderer, transition: listed, unmount } = setup()
      const transition = listed()
      renderer.clockFastForward(1500)
      // The frame queued its finish event; nothing has delivered it.
      expect(transition.playState).toBe("finished")
      expect(transition.currentTime).toBe(1000)
      expect(transition.playbackRate).toBe(1)
      renderer.dispatchNativeEvents()
      expect(transition.playState).toBe("finished")
      expect(transition.currentTime).toBe(1000)
      await expect(transition.finished).resolves.toBe(transition)
      unmount()
    })

    it("answers idle for a run cancelled in the task of the frame that cancelled it, and stops asking native once it has heard", async () => {
      const { render, renderer, element, target, transition: listed, unmount } = setup()
      const transition = listed()
      const id = renderer.getAnimations(element.current!.id)[0].id
      // A cancel rejects the promise held when it comes, and leaves a new pending one.
      const held = transition.finished
      renderer.clockFastForward(300)
      render(target(80))
      renderer.clockFastForward(0)
      expect(transition.playState).toBe("idle")
      expect(transition.currentTime).toBeNull()
      expect(transition.playbackRate).toBe(1)
      expect(renderer.getAnimation(id).playState).toBe("idle")
      expect(() => transition.pause()).toThrow(DOMException)
      renderer.dispatchNativeEvents()
      expect(transition.playState).toBe("idle")
      await expect(held).rejects.toMatchObject({ name: "AbortError" })
      expect(() => renderer.getAnimation(id)).toThrow()
      unmount()
    })

    it("releases the run of an event no animation object was made for", () => {
      const { render, renderer, element, target, unmount } = setup()
      // Native only: `element.getAnimations()` is what makes the object.
      const id = renderer.getAnimations(element.current!.id)[0].id
      renderer.clockFastForward(300)
      render(target(80))
      renderer.clockFastForward(0)
      expect(renderer.getAnimation(id).playState).toBe("idle")
      renderer.dispatchNativeEvents()
      expect(() => renderer.getAnimation(id)).toThrow()
      unmount()
    })

    it("keeps answering for a transition that finished after its event was heard, until the style moves off its end", () => {
      const { render, renderer, element, target, transition: listed, unmount } = setup()
      const transition = listed()
      const id = renderer.getAnimations(element.current!.id)[0].id
      renderer.clockFastForward(1500)
      renderer.dispatchNativeEvents()
      expect(renderer.getAnimation(id).playState).toBe("finished")
      render(target(150))
      renderer.clockFastForward(0)
      renderer.dispatchNativeEvents()
      expect(transition.playState).toBe("finished")
      render(target(90))
      renderer.clockFastForward(0)
      renderer.dispatchNativeEvents()
      expect(transition.playState).toBe("idle")
      expect(() => renderer.getAnimation(id)).toThrow()
      unmount()
    })
  })

  it("keeps the internal events away from the listeners of the app", () => {
    const { render, renderer, unmount } = createTestRoot()
    renderer.clockPause()
    const seen: string[] = []
    const element: { current: PublicInstance | null } = { current: null }
    render(
      <div
        testId="box"
        ref={(node: PublicInstance | null) => {
          element.current = node
        }}
        onAnimationEnd={(event) => seen.push(event.type)}
        style={{ width: 50, height: 20, animation: "grow 1s linear" }}
      />
    )
    renderer.clockFastForward(0)
    const [animation] = element.current!.getAnimations()
    renderer.clockFastForward(2000)
    renderer.dispatchNativeEvents()
    expect(seen).toEqual(["animationend"])
    expect(animation.playState).toBe("finished")
    unmount()
  })
})
