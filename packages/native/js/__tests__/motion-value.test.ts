import { describe, expect, it } from "vitest"
import {
  MotionValue,
  animate,
  computedValue,
  createRendererState,
  isMotionValue,
  motionValue,
  springValue,
  transformValue,
  velocityValue,
  wireMotionTransition,
} from "../host.js"
import type { MotionValueRenderer } from "../host.js"
import { TestRenderer, hasNativeTestRenderer } from "../testing.js"

type FakeNode = {
  kind: "source" | "spring" | "transform" | "follower"
  value: unknown
  input?: number
  velocity: number
  animating: boolean
  target?: unknown
  transition?: unknown
  /** The animation the controls act on, until it is stopped or jumped. */
  run?: { held: boolean; speed: number; time: number }
}

/** Just enough of the native graph to see what the core asks of it. */
class FakeRenderer {
  nodes = new Map<number, FakeNode>()
  calls: string[] = []
  listeners = new Map<number, { change: boolean; animation: boolean }>()
  failTransform = false

  applyBatch(): number[] {
    return []
  }

  private node(id: number): FakeNode {
    const node = this.nodes.get(id)
    if (!node) throw new Error(`unknown motion value ${id}`)
    return node
  }

  private add(id: number, node: FakeNode, call: string) {
    if (this.nodes.has(id)) throw new Error(`motion value ${id} already exists`)
    this.nodes.set(id, node)
    this.calls.push(call)
  }

  createMotionValue(id: number, initial: unknown) {
    this.add(id, { kind: "source", value: initial, velocity: 0, animating: false }, `create ${id}`)
  }

  createMotionTransform(id: number, input: number, inputRange: number[], outputRange: unknown[]) {
    if (this.failTransform) throw new Error("bad range")
    this.node(input)
    this.add(
      id,
      { kind: "transform", value: outputRange[0], input, velocity: 0, animating: false },
      `transform ${id}<-${input}`
    )
  }

  createMotionFollower(id: number, source: number) {
    this.add(
      id,
      { kind: "follower", value: this.node(source).value, input: source, velocity: 0, animating: false },
      `follower ${id}<-${source}`
    )
  }

  createMotionSpring(id: number, initial: unknown) {
    this.add(id, { kind: "spring", value: initial, velocity: 0, animating: false }, `spring ${id}`)
  }

  disposeMotionValue(id: number) {
    this.calls.push(`dispose ${id}`)
    this.listeners.delete(id)
    return this.nodes.delete(id)
  }

  getMotionValue(id: number) {
    return this.node(id).value
  }

  getMotionValueVelocity(id: number) {
    return this.node(id).velocity
  }

  isMotionValueAnimating(id: number) {
    return this.node(id).animating
  }

  setMotionValue(id: number, value: unknown) {
    this.calls.push(`set ${id}`)
    this.node(id).value = value
  }

  jumpMotionValue(id: number, value: unknown) {
    this.calls.push(`jump ${id}`)
    const node = this.node(id)
    node.value = value
    node.animating = false
    node.run = undefined
  }

  animateMotionValue(id: number, target: unknown, transition?: unknown) {
    this.calls.push(`animate ${id}`)
    const node = this.node(id)
    node.target = target
    node.transition = transition
    node.animating = target !== node.value
    node.run = { held: false, speed: 1, time: 0 }
  }

  stopMotionValue(id: number) {
    this.calls.push(`stop ${id}`)
    const node = this.node(id)
    node.animating = false
    node.run = undefined
  }

  pauseMotionValue(id: number) {
    this.calls.push(`pause ${id}`)
    const run = this.node(id).run
    if (run && this.node(id).animating) run.held = true
  }

  playMotionValue(id: number) {
    this.calls.push(`play ${id}`)
    const node = this.node(id)
    if (!node.run) return
    if (!node.animating) {
      node.animating = true
      node.run.time = 0
    }
    node.run.held = false
  }

  seekMotionValue(id: number, seconds: number) {
    this.calls.push(`seek ${id} ${seconds}`)
    const run = this.node(id).run
    if (run) run.time = seconds
  }

  setMotionValueSpeed(id: number, speed: number) {
    this.calls.push(`speed ${id} ${speed}`)
    const run = this.node(id).run
    if (run) run.speed = speed
  }

  completeMotionValue(id: number) {
    this.calls.push(`complete ${id}`)
    const node = this.node(id)
    if (!node.run) return
    const frames = node.target
    const lastFrame = Array.isArray(frames) ? frames.filter((frame) => frame !== null).at(-1) : frames
    const holdsList = Array.isArray(node.value) && Array.isArray(frames) && frames.every((frame) => typeof frame === "number")
    node.value = holdsList ? frames : lastFrame
    node.animating = false
  }

  detachMotionValueClock(id: number) {
    this.calls.push(`detach ${id}`)
  }

  getMotionValuePlayback(id: number) {
    const node = this.node(id)
    if (!node.run) return null
    const state = node.run.held ? "paused" : node.animating ? "running" : "finished"
    return { time: node.run.time, speed: node.run.speed, duration: 1, iterationDuration: 1, state, startTime: null }
  }

  setMotionValueListener(id: number, change: boolean, animation: boolean) {
    this.calls.push(`listen ${id} ${change} ${animation}`)
    this.listeners.set(id, { change, animation })
  }

  /** What the renderer does when a frame sampled a change. */
  change(id: number, value: unknown, velocity = 0) {
    const node = this.node(id)
    node.value = value
    node.velocity = velocity
    return this.emit("motionValueChange", id, JSON.stringify(value))
  }

  start(id: number) {
    return this.emit("motionValueAnimationStart", id)
  }

  /** The animation reaches its target and the renderer says so. */
  finish(id: number) {
    const node = this.node(id)
    node.value = node.target
    node.animating = false
    node.velocity = 0
    return this.emit("motionValueAnimationComplete", id)
  }

  emit(eventType: string, elementId: number, value?: string) {
    return createRendererState(this as unknown as MotionValueRenderer).dispatch({
      elementId,
      eventType,
      value,
    })
  }
}

/** Whether a promise has resolved by the time the microtasks and the next timer have run. */
const resolved = (promise: Promise<unknown>) =>
  Promise.race([promise.then(() => true), new Promise<boolean>((done) => setTimeout(() => done(false), 0))])

const fake = () => new FakeRenderer()
const host = (renderer: FakeRenderer) => renderer as unknown as MotionValueRenderer

describe("a motion value without a renderer", () => {
  it("keeps its state in JS and reports writes to listeners at once", () => {
    const x = motionValue(1)
    const seen: number[] = []
    x.on("change", (value) => seen.push(value))
    x.set(2)
    x.set(2)
    x.jump(3)
    expect(x.get()).toBe(3)
    expect(seen).toEqual([2, 3])
    expect(x.getVelocity()).toBe(0)
    expect(x.host).toBeUndefined()
  })

  it("has a velocity only for numbers", () => {
    expect(() => motionValue("#fff").getVelocity()).toThrow(/number/)
  })

  it("gives every value its own id and knows its own kind", () => {
    const a = motionValue(0)
    const b = motionValue(0)
    expect(a.id).not.toBe(b.id)
    expect(isMotionValue(a)).toBe(true)
    expect(isMotionValue(3)).toBe(false)
    expect(a).toBeInstanceOf(MotionValue)
  })

  it("cannot read a transform or write a computed value", () => {
    const x = motionValue(0)
    expect(() => transformValue(x, [0, 1], [0, 10]).get()).toThrow(/bound/)
    expect(() => computedValue(() => x.get() + 1).set(3)).toThrow(/cannot be written/)
    expect(computedValue(() => x.get() + 1).get()).toBe(1)
  })
})

describe("binding", () => {
  it("creates the node on the first bind with the JS state, once", () => {
    const renderer = fake()
    const x = motionValue(5)
    x.set(7)
    const release = x.bind(host(renderer))
    const releaseAgain = x.bind(host(renderer))
    expect(renderer.calls).toEqual([`create ${x.id}`])
    expect(renderer.getMotionValue(x.id)).toBe(7)
    expect(x.host).toBe(renderer)
    release()
    expect(renderer.nodes.has(x.id)).toBe(true)
    releaseAgain()
    expect(renderer.nodes.has(x.id)).toBe(false)
    expect(x.host).toBeUndefined()
  })

  it("disposes on the last release and takes the value back into JS", () => {
    const renderer = fake()
    const x = motionValue(1)
    const release = x.bind(host(renderer))
    renderer.setMotionValue(x.id, 42)
    release()
    expect(renderer.nodes.size).toBe(0)
    expect(x.get()).toBe(42)
    const again = x.bind(host(renderer))
    expect(renderer.getMotionValue(x.id)).toBe(42)
    again()
  })

  it("ignores a second release of the same binding", () => {
    const renderer = fake()
    const x = motionValue(1)
    const first = x.bind(host(renderer))
    const second = x.bind(host(renderer))
    first()
    first()
    expect(renderer.nodes.has(x.id)).toBe(true)
    second()
    expect(renderer.nodes.has(x.id)).toBe(false)
  })

  it("refuses a second renderer and a renderer without a graph", () => {
    const x = motionValue(1)
    const one = fake()
    x.bind(host(one))
    expect(() => x.bind(host(fake()))).toThrow(/another renderer/)
    expect(() => motionValue(1).bind({ applyBatch: () => [] })).toThrow(/no motion value graph/)
  })

  it("keeps the node for a listener after the last binding goes", () => {
    const renderer = fake()
    const x = motionValue(0)
    const release = x.bind(host(renderer))
    const off = x.on("change", () => {})
    release()
    expect(renderer.nodes.has(x.id)).toBe(true)
    off()
    expect(renderer.nodes.has(x.id)).toBe(false)
  })

  it("rolls back a bind that fails, including the inputs it already bound", () => {
    const renderer = fake()
    renderer.failTransform = true
    const x = motionValue(0)
    const mapped = transformValue(x, [0, 1], [0, 10])
    expect(() => mapped.bind(host(renderer))).toThrow(/bad range/)
    expect(renderer.nodes.size).toBe(0)
    expect(mapped.host).toBeUndefined()
    expect(x.host).toBeUndefined()
  })
})

describe("listeners", () => {
  it("asks for the events somebody listens to, and no others", () => {
    const renderer = fake()
    const x = motionValue(0)
    x.bind(host(renderer))
    expect(renderer.calls.some((call) => call.startsWith("listen"))).toBe(false)

    const offChange = x.on("change", () => {})
    expect(renderer.listeners.get(x.id)).toEqual({ change: true, animation: false })
    const offStart = x.on("animationStart", () => {})
    expect(renderer.listeners.get(x.id)).toEqual({ change: true, animation: true })
    offChange()
    expect(renderer.listeners.get(x.id)).toEqual({ change: false, animation: true })
    offStart()
    expect(renderer.listeners.get(x.id)).toEqual({ change: false, animation: false })
  })

  it("delivers the renderer's events to the value's listeners", () => {
    const renderer = fake()
    const x = motionValue(0)
    x.bind(host(renderer))
    const log: string[] = []
    x.on("change", (value) => log.push(`change ${value}`))
    x.on("animationStart", () => log.push("start"))
    x.on("animationComplete", () => log.push("complete"))
    renderer.start(x.id)
    renderer.change(x.id, 4)
    renderer.emit("motionValueAnimationComplete", x.id)
    expect(log).toEqual(["start", "change 4", "complete"])
  })

  it("does not claim events for a value nobody holds", () => {
    expect(fake().emit("motionValueChange", 9999, "1")).toBe(false)
  })

  it("stops delivering to a listener that was removed", () => {
    const renderer = fake()
    const x = motionValue(0)
    x.bind(host(renderer))
    const seen: number[] = []
    const off = x.on("change", (value) => seen.push(value))
    renderer.change(x.id, 1)
    off()
    off()
    renderer.change(x.id, 2)
    expect(seen).toEqual([1])
  })
})

describe("animate", () => {
  it("waits for a renderer, then starts from the value it has", async () => {
    const renderer = fake()
    const x = motionValue(0)
    const controls = x.animate(10, { duration: 1 })
    expect(x.isAnimating()).toBe(true)
    x.set(2)
    const release = x.bind(host(renderer))
    expect(renderer.calls).toContain(`animate ${x.id}`)
    expect(renderer.nodes.get(x.id)?.transition).toEqual({ duration: 1 })
    expect(renderer.getMotionValue(x.id)).toBe(2)
    expect(renderer.listeners.get(x.id)).toEqual({ change: false, animation: true })

    let done = false
    void controls.then(() => {
      done = true
    })
    renderer.finish(x.id)
    await controls.finished
    expect(done).toBe(true)
    expect(x.get()).toBe(10)
    // The animation was the only thing holding a node nobody binds.
    release()
    expect(renderer.nodes.size).toBe(0)
  })

  it("keeps the node alive until an animation nobody binds is over", () => {
    const renderer = fake()
    const x = motionValue(0)
    const release = x.bind(host(renderer))
    x.animate(5)
    release()
    expect(renderer.nodes.has(x.id)).toBe(true)
    renderer.finish(x.id)
    expect(renderer.nodes.size).toBe(0)
    expect(x.get()).toBe(5)
  })

  it("resolves at once when nothing moves", async () => {
    const renderer = fake()
    const x = motionValue(3)
    x.bind(host(renderer))
    await x.animate(3).finished
  })

  it("leaves the replaced animation pending and idle when a new one starts", async () => {
    const renderer = fake()
    const x = motionValue(0)
    x.bind(host(renderer))
    const first = x.animate(10)
    const second = x.animate(20)
    expect(first.state).toBe("idle")
    expect(second.state).toBe("running")
    expect(x.isAnimating()).toBe(true)
    renderer.finish(x.id)
    await second.finished
    expect(second.state).toBe("finished")
    expect(await resolved(first.finished)).toBe(false)
  })

  it("does not end the new animation on the completion of the one it stopped", () => {
    const renderer = fake()
    const x = motionValue(0)
    x.bind(host(renderer))
    const first = x.animate(10)
    first.stop()
    const second = x.animate(20)
    let settled = false
    void second.finished.then(() => {
      settled = true
    })
    // The stop's completion arrives after the new animation began.
    renderer.emit("motionValueAnimationComplete", x.id)
    return Promise.resolve().then(() => {
      expect(settled).toBe(false)
      expect(x.isAnimating()).toBe(true)
    })
  })

  it("stops, cancels and completes through its controls", async () => {
    const renderer = fake()
    const x = motionValue(1)
    x.bind(host(renderer))

    const stopped = x.animate(9)
    renderer.setMotionValue(x.id, 4)
    stopped.stop()
    expect(stopped.state).toBe("idle")
    expect(await resolved(stopped.finished)).toBe(false)
    expect(x.get()).toBe(4)

    const cancelled = x.animate(9)
    renderer.setMotionValue(x.id, 6)
    cancelled.cancel()
    expect(cancelled.state).toBe("idle")
    expect(await resolved(cancelled.finished)).toBe(false)
    expect(x.get()).toBe(4)

    const completed = x.animate(9)
    completed.complete()
    expect(completed.state).toBe("finished")
    await completed.finished
    expect(x.get()).toBe(9)
    // A finished animation cannot be cancelled: the value stays where it ended.
    completed.cancel()
    expect(x.get()).toBe(9)
    expect(completed.state).toBe("idle")
  })

  it("cancels a finished animation without taking the value from its end", async () => {
    const renderer = fake()
    const x = motionValue(1)
    x.bind(host(renderer))
    const controls = x.animate(9)
    renderer.finish(x.id)
    await controls.finished
    expect(controls.state).toBe("finished")
    controls.cancel()
    expect(x.get()).toBe(9)
    expect(controls.state).toBe("idle")
  })

  it("lets a jump or a stop end the animation", async () => {
    const renderer = fake()
    const x = motionValue(0)
    x.bind(host(renderer))
    const jumped = x.animate(5)
    x.jump(1)
    expect(jumped.state).toBe("idle")
    expect(await resolved(jumped.finished)).toBe(false)
    const stopped = x.animate(5)
    x.stop()
    expect(stopped.state).toBe("idle")
    expect(await resolved(stopped.finished)).toBe(false)
    expect(x.isAnimating()).toBe(false)
  })

  it("only animates a plain value", () => {
    const x = motionValue(0)
    expect(() => springValue(0).animate(1)).toThrow(/set it/)
    expect(() => transformValue(x, [0, 1], [0, 1]).animate(1)).toThrow(/computed/)
  })

  it("animates a bare number on a host and reports each update", async () => {
    const renderer = fake()
    const updates: number[] = []
    const controls = animate(0, 10, { host: host(renderer), duration: 1, onUpdate: (v) => updates.push(v) })
    const id = [...renderer.nodes.keys()][0]
    expect(renderer.nodes.get(id)?.transition).toEqual({ duration: 1 })
    renderer.change(id, 5)
    renderer.finish(id)
    await controls.finished
    expect(updates).toEqual([5])
    expect(renderer.nodes.size).toBe(0)
    expect(() => animate(0, 1, { duration: 1 } as never)).toThrow(/host/)
  })
})

describe("animate with keyframes", () => {
  it("sends keyframes through as they are and Infinity as the string the renderer accepts", () => {
    const renderer = fake()
    const x = motionValue(0)
    x.bind(host(renderer))
    const transition = { duration: 2, repeat: Infinity, repeatType: "mirror", times: [0, 0.5, 1] } as const
    x.animate([null, 10, 4], transition)
    const node = renderer.nodes.get(x.id)
    expect(node?.target).toEqual([null, 10, 4])
    expect(node?.transition).toEqual({ duration: 2, repeat: "Infinity", repeatType: "mirror", times: [0, 0.5, 1] })
    // The caller's object is not rewritten.
    expect(transition.repeat).toBe(Infinity)
    expect(JSON.parse(JSON.stringify(node?.transition)).repeat).toBe("Infinity")

    x.animate(5, { duration: 1, repeat: 2 })
    expect(renderer.nodes.get(x.id)?.transition).toEqual({ duration: 1, repeat: 2 })
  })

  it("sends Infinity from a value that waits for a renderer, and from the bare number form", () => {
    const renderer = fake()
    const x = motionValue(0)
    x.animate([0, 1], { repeat: Infinity })
    x.bind(host(renderer))
    expect(renderer.nodes.get(x.id)?.transition).toEqual({ repeat: "Infinity" })

    animate(0, [null, 10], { host: host(renderer), repeat: Infinity })
    const id = [...renderer.nodes.keys()].at(-1) ?? 0
    expect(renderer.nodes.get(id)?.target).toEqual([null, 10])
    expect(renderer.nodes.get(id)?.transition).toEqual({ repeat: "Infinity" })
  })

  it("completes at the last keyframe, and cancels back to where it started", async () => {
    const renderer = fake()
    const x = motionValue(3)
    x.bind(host(renderer))

    const completed = x.animate([null, 10, 7, null])
    completed.complete()
    await completed.finished
    expect(x.get()).toBe(7)

    const cancelled = x.animate([20, 30])
    cancelled.cancel()
    expect(await resolved(cancelled.finished)).toBe(false)
    expect(x.get()).toBe(20)
  })

  it("completes a list-valued animation at its target, whichever way the array is read", () => {
    const renderer = fake()
    const list = motionValue([0, 0])
    list.bind(host(renderer))
    list.animate([5, 6]).complete()
    expect(list.get()).toEqual([5, 6])
    list.animate([[1, 1], null, [2, 2]]).complete()
    expect(list.get()).toEqual([2, 2])
  })

  it("jumps straight to the end under reduced motion, keyframes or not", () => {
    const renderer = fake()
    const x = motionValue(0)
    x.bind(host(renderer))
    const release = x.reduceMotion()
    x.animate([0, 50, 100], { duration: 1, repeat: Infinity })
    expect(renderer.nodes.get(x.id)?.transition).toEqual({ duration: 0 })
    release()
  })
})

describe("derived values", () => {
  it("binds a transform's input first and releases it last", () => {
    const renderer = fake()
    const x = motionValue(0)
    const mapped = transformValue(x, [0, 1], [0, 100], { clamp: false })
    const release = mapped.bind(host(renderer))
    expect(renderer.calls).toEqual([`create ${x.id}`, `transform ${mapped.id}<-${x.id}`])
    expect(() => mapped.set(1)).toThrow(/cannot be written/)
    release()
    expect(renderer.calls.slice(2)).toEqual([`dispose ${mapped.id}`, `dispose ${x.id}`])
  })

  it("keeps a shared input while another value still reads it", () => {
    const renderer = fake()
    const x = motionValue(0)
    const a = transformValue(x, [0, 1], [0, 1])
    const b = transformValue(x, [0, 1], [0, 2])
    const releaseA = a.bind(host(renderer))
    const releaseB = b.bind(host(renderer))
    releaseA()
    expect(renderer.nodes.has(x.id)).toBe(true)
    releaseB()
    expect(renderer.nodes.size).toBe(0)
  })

  it("makes a follower of a value, and a spring with its own target of a plain one", () => {
    const renderer = fake()
    const x = motionValue(3)
    const follower = springValue(x, { stiffness: 400, damping: 40 })
    const own = springValue(8, { stiffness: 100 })
    const releaseFollower = follower.bind(host(renderer))
    const releaseOwn = own.bind(host(renderer))
    expect(renderer.calls).toEqual([`create ${x.id}`, `follower ${follower.id}<-${x.id}`, `spring ${own.id}`])
    expect(own.get()).toBe(8)
    own.set(20)
    expect(renderer.calls.at(-1)).toBe(`set ${own.id}`)
    releaseFollower()
    releaseOwn()
    expect(own.get()).toBe(20)
    expect(renderer.nodes.size).toBe(0)
  })

  it("keeps a computed value current from the change events of what it reads", () => {
    const renderer = fake()
    const x = motionValue(1)
    const y = motionValue(10)
    const useY = motionValue(0)
    const sum = computedValue(() => (useY.get() === 0 ? x.get() : x.get() + y.get()))
    const release = sum.bind(host(renderer))
    expect(renderer.getMotionValue(sum.id)).toBe(1)
    expect(y.host).toBeUndefined()

    renderer.change(x.id, 2)
    expect(renderer.getMotionValue(sum.id)).toBe(2)

    renderer.change(useY.id, 1)
    expect(renderer.getMotionValue(sum.id)).toBe(12)
    expect(y.host).toBe(renderer)

    renderer.change(useY.id, 0)
    expect(y.host).toBeUndefined()

    release()
    expect(renderer.nodes.size).toBe(0)
    expect(x.host).toBeUndefined()
  })

  it("reads a transform inside a computed value", () => {
    const renderer = fake()
    const x = motionValue(0)
    const mapped = transformValue(x, [0, 1], [5, 6])
    const label = computedValue(() => mapped.get() * 2)
    const release = label.bind(host(renderer))
    expect(renderer.getMotionValue(label.id)).toBe(10)
    release()
    expect(renderer.nodes.size).toBe(0)
  })

  it("lets a computed value have listeners of its own", () => {
    const renderer = fake()
    const x = motionValue(1)
    const double = computedValue(() => x.get() * 2)
    double.bind(host(renderer))
    const seen: number[] = []
    double.on("change", (value) => seen.push(value))
    renderer.change(x.id, 4)
    renderer.change(double.id, renderer.getMotionValue(double.id))
    expect(seen).toEqual([8])
  })

  it("derives a velocity from the source on each change and on completion", () => {
    const renderer = fake()
    const x = motionValue(0)
    const speed = velocityValue(x)
    const release = speed.bind(host(renderer))
    expect(renderer.getMotionValue(speed.id)).toBe(0)
    renderer.change(x.id, 5, 120)
    expect(renderer.getMotionValue(speed.id)).toBe(120)
    renderer.change(x.id, 5, 0)
    renderer.emit("motionValueAnimationComplete", x.id)
    expect(renderer.getMotionValue(speed.id)).toBe(0)
    release()
    expect(renderer.nodes.size).toBe(0)
  })
})

describe("destroy", () => {
  it("disposes the node, ends the animation and refuses later use", async () => {
    const renderer = fake()
    const x = motionValue(0)
    const release = x.bind(host(renderer))
    x.on("change", () => {})
    const controls = x.animate(5)
    x.destroy()
    expect(controls.state).toBe("idle")
    expect(await resolved(controls.finished)).toBe(false)
    expect(renderer.nodes.size).toBe(0)
    expect(() => x.get()).toThrow(/destroyed/)
    expect(() => x.set(1)).toThrow(/destroyed/)
    expect(() => x.bind(host(renderer))).toThrow(/destroyed/)
    release()
    x.destroy()
  })

  it("disposes a value that was never bound", () => {
    const x = motionValue(0)
    x.destroy()
    expect(() => x.on("change", () => {})).toThrow(/destroyed/)
  })
})

describe.skipIf(!hasNativeTestRenderer)("on the native test renderer", () => {
  const linear = { duration: 1, ease: "linear" } as const

  const mount = () => {
    const renderer = new TestRenderer({ width: 400, height: 300 })
    renderer.clockPause()
    renderer.applyBatch(
      JSON.stringify([
        ["createElement", 1, "div"],
        ["setRoot", 1],
        ["setStyle", 1, { display: "flex" }],
        ["createElement", 2, "div"],
        ["appendChild", 1, 2],
        ["setStyle", 2, { height: 20, width: 5 }],
      ])
    )
    renderer.flush()
    return renderer
  }

  /** One frame of the window, with the events it produced delivered. */
  const frame = (renderer: TestRenderer, ms: number) => {
    renderer.clockFastForward(ms)
    renderer.flush()
    renderer.dispatchNativeEvents()
  }

  const widthOf = (renderer: TestRenderer) => renderer.getElementBounds(2)?.width

  it("drives an element, reports to listeners and frees the node with the last binding", async () => {
    const renderer = mount()
    const x = motionValue(0)
    const width = transformValue(x, [0, 1], [10, 110])
    const speed = velocityValue(x)
    const doubled = computedValue(() => x.get() * 2)

    const releases = [width, speed, doubled].map((value) => value.bind(renderer))
    renderer.applyBatch(JSON.stringify([["setCustomProp", 2, "motionValues", { width: width.id }]]))
    renderer.flush()
    expect(widthOf(renderer)).toBe(10)

    const changes: number[] = []
    const log: string[] = []
    const listeners = [
      x.on("change", (value) => changes.push(value)),
      x.on("animationStart", () => log.push("start")),
      x.on("animationComplete", () => log.push("complete")),
    ]

    const controls = x.animate(1, linear)
    let finished = false
    void controls.finished.then(() => {
      finished = true
    })
    frame(renderer, 500)
    expect(widthOf(renderer)).toBeCloseTo(60, 0)
    expect(changes.at(-1)).toBeCloseTo(0.5, 5)
    expect(x.getVelocity()).toBeCloseTo(1, 5)
    expect(finished).toBe(false)

    frame(renderer, 1000)
    frame(renderer, 16)
    expect(widthOf(renderer)).toBe(110)
    expect(log).toEqual(["start", "complete"])
    await controls.finished
    expect(finished).toBe(true)

    // The computed values trail by a frame but settle on the final value.
    frame(renderer, 16)
    frame(renderer, 16)
    expect(doubled.get()).toBe(2)
    expect(speed.get()).toBe(0)

    for (const release of releases) release()
    expect(renderer.getMotionValue(x.id)).toBe(1)
    for (const off of listeners) off()
    const id = x.id
    expect(() => renderer.getMotionValue(id)).toThrow(/unknown motion value/)
    expect(x.host).toBeUndefined()
    expect(x.get()).toBe(1)
  })

  it("follows with a spring and can be bound again after it was freed", () => {
    const renderer = mount()
    const x = motionValue(0)
    const follower = springValue(x, { stiffness: 400, damping: 40 })
    const release = follower.bind(renderer)
    const off = follower.on("change", () => {})
    x.set(100)
    for (let i = 0; i < 10; i++) frame(renderer, 16)
    const mid = follower.get()
    expect(mid).toBeGreaterThan(0.5)
    expect(mid).toBeLessThan(100)
    release()
    off()
    expect(() => follower.get()).toThrow(/bound/)

    const again = x.bind(renderer)
    expect(x.get()).toBe(100)
    again()
  })

  it("reads a follower that is neither bound to an element nor listened to at the clock", () => {
    const renderer = mount()
    const x = motionValue(0)
    const follower = springValue(x, { stiffness: 400, damping: 40 })
    const release = follower.bind(renderer)
    x.set(100)
    expect(follower.get()).toBe(0)

    renderer.clockFastForward(160)
    const mid = follower.get()
    expect(mid).toBeGreaterThan(1)
    expect(mid).toBeLessThan(100)
    expect(follower.getVelocity()).toBeGreaterThan(0)
    expect(follower.isAnimating()).toBe(true)
    expect(follower.get()).toBe(mid)

    renderer.clockFastForward(20_000)
    expect(follower.get()).toBeCloseTo(100, 5)
    expect(follower.isAnimating()).toBe(false)
    release()
  })

  describe("keyframes", () => {
    const run = (renderer: TestRenderer, ms: number) => renderer.clockFastForward(ms)

    it("plays keyframes spaced by times and ends on the last one", async () => {
      const renderer = mount()
      const x = motionValue(0)
      const release = x.bind(renderer)
      const controls = x.animate([null, 100, 40], { duration: 1, ease: "linear", times: [0, 0.25, 1] })
      run(renderer, 125)
      expect(x.get()).toBeCloseTo(50, 5)
      run(renderer, 125)
      expect(x.get()).toBeCloseTo(100, 5)
      run(renderer, 375)
      expect(x.get()).toBeCloseTo(70, 5)
      expect(x.isAnimating()).toBe(true)
      frame(renderer, 1000)
      frame(renderer, 16)
      expect(x.get()).toBe(40)
      expect(x.isAnimating()).toBe(false)
      await controls.finished
      release()
    })

    it("takes a leading null from where the value is, and a first keyframe as its own start", () => {
      const renderer = mount()
      const x = motionValue(10)
      x.bind(renderer)
      x.animate([null, 50], linear)
      run(renderer, 500)
      expect(x.get()).toBeCloseTo(30, 5)
      x.stop()
      x.animate([200, 100], linear)
      run(renderer, 500)
      expect(x.get()).toBeCloseTo(150, 5)
    })

    it("loops with repeat Infinity, never completes, and a stop mid-loop ends it where it is", async () => {
      const renderer = mount()
      const x = motionValue(0)
      x.bind(renderer)
      const log: string[] = []
      x.on("animationStart", () => log.push("start"))
      x.on("animationComplete", () => log.push("complete"))

      const controls = x.animate([0, 100], { ...linear, repeat: Infinity })
      let finished = false
      void controls.finished.then(() => {
        finished = true
      })
      for (let second = 0; second < 30; second++) frame(renderer, 1000)
      expect(x.isAnimating()).toBe(true)
      // An iteration owns its last instant: a loop at an exact boundary shows the end, as Motion does.
      expect(x.get()).toBeCloseTo(100, 5)
      frame(renderer, 250)
      expect(x.get()).toBeCloseTo(25, 5)
      expect(log).toEqual(["start"])
      expect(finished).toBe(false)

      x.stop()
      frame(renderer, 16)
      const held = x.get()
      expect(held).toBeCloseTo(25, 0)
      frame(renderer, 10_000)
      expect(x.get()).toBe(held)
      expect(x.isAnimating()).toBe(false)
      expect(await resolved(controls.finished)).toBe(false)
      expect(finished).toBe(false)
      expect(controls.state).toBe("idle")
      expect(log).toEqual(["start", "complete"])
    })

    it("mirrors and reverses a repeating animation", () => {
      const renderer = mount()
      const x = motionValue(0)
      x.bind(renderer)
      const timed = { ...linear, times: [0, 0.25, 1], repeat: 1 }

      x.animate([0, 100, 40], { ...timed, repeatType: "reverse" })
      run(renderer, 1250)
      expect(x.get()).toBeCloseTo(60, 5)
      x.jump(0)

      x.animate([0, 100, 40], { ...timed, repeatType: "mirror" })
      run(renderer, 1250)
      expect(x.get()).toBeCloseTo(100, 5)
      // Keyframes 40, 100, 0 at the same times: three quarters through, back down.
      run(renderer, 500)
      expect(x.get()).toBeCloseTo(100 / 3, 5)
    })

    it("holds the end for repeatDelay before the next iteration", () => {
      const renderer = mount()
      const x = motionValue(0)
      x.bind(renderer)
      x.animate([0, 100], { ...linear, repeat: 1, repeatDelay: 1 })
      run(renderer, 1500)
      expect(x.get()).toBeCloseTo(100, 5)
      run(renderer, 1000)
      expect(x.get()).toBeCloseTo(50, 5)
    })

    it("drives a bound property while it loops", () => {
      const renderer = mount()
      const x = motionValue(10)
      x.bind(renderer)
      renderer.applyBatch(JSON.stringify([["setCustomProp", 2, "motionValues", { width: x.id }]]))
      renderer.flush()
      x.animate([10, 110], { ...linear, repeat: Infinity, repeatType: "mirror" })
      frame(renderer, 500)
      expect(widthOf(renderer)).toBeCloseTo(60, 0)
      frame(renderer, 1000)
      expect(widthOf(renderer)).toBeCloseTo(60, 0)
      frame(renderer, 250)
      expect(widthOf(renderer)).toBeCloseTo(35, 0)
    })

    it("keyframes colours, and a list of numbers is one target unless it holds lists", () => {
      const renderer = mount()
      const colour = motionValue("#000000")
      colour.bind(renderer)
      colour.animate(["#000000", "#ff0000", "#000000"], linear)
      run(renderer, 500)
      expect(colour.get()).toContain("rgba(255")

      const point = motionValue([0, 0])
      point.bind(renderer)
      point.animate([10, 20], linear)
      run(renderer, 500)
      expect(point.get()).toEqual([5, 10])
      point.animate([[0, 0], [10, 20], null], linear)
      run(renderer, 1000)
      expect(point.get()).toEqual([10, 20])
    })

    it("rejects keyframes the renderer cannot play", () => {
      const renderer = mount()
      const x = motionValue(0)
      x.bind(renderer)
      expect(() => x.animate([0, 1, 2], { type: "spring" })).toThrow(/spring/)
      expect(() => x.animate([0, 1, 2], { times: [0, 1] })).toThrow(/times/)
      expect(() => x.animate([null])).toThrow(/null/)
      expect(() => x.animate([0, 1], { repeat: 1.5 })).toThrow(/repeat/)
      expect(x.isAnimating()).toBe(false)
    })
  })
})

describe("wireMotionTransition", () => {
  it("sends Infinity as a string at the top, in default and in each property, without rewriting the input", () => {
    const transition = {
      duration: 1,
      repeat: Infinity,
      default: { repeat: Infinity, ease: [0.4, 0, 0.2, 1] },
      opacity: { repeat: 3 },
      width: { repeat: Infinity, times: [0, 1] },
    } as const
    expect(wireMotionTransition(transition)).toEqual({
      duration: 1,
      repeat: "Infinity",
      default: { repeat: "Infinity", ease: [0.4, 0, 0.2, 1] },
      opacity: { repeat: 3 },
      width: { repeat: "Infinity", times: [0, 1] },
    })
    expect(transition.repeat).toBe(Infinity)
    expect(wireMotionTransition(undefined)).toBeUndefined()
  })
})
