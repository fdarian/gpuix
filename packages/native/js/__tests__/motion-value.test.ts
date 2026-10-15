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
  }

  animateMotionValue(id: number, target: unknown, transition?: unknown) {
    this.calls.push(`animate ${id}`)
    const node = this.node(id)
    node.target = target
    node.transition = transition
    node.animating = target !== node.value
  }

  stopMotionValue(id: number) {
    this.calls.push(`stop ${id}`)
    this.node(id).animating = false
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

  it("resolves the replaced animation when a new one starts", async () => {
    const renderer = fake()
    const x = motionValue(0)
    x.bind(host(renderer))
    const first = x.animate(10)
    const second = x.animate(20)
    await first.finished
    expect(x.isAnimating()).toBe(true)
    renderer.finish(x.id)
    await second.finished
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
    await stopped.finished
    expect(x.get()).toBe(4)

    const cancelled = x.animate(9)
    renderer.setMotionValue(x.id, 6)
    cancelled.cancel()
    await cancelled.finished
    expect(x.get()).toBe(4)

    const completed = x.animate(9)
    completed.complete()
    await completed.finished
    expect(x.get()).toBe(9)
    // A finished animation's controls do nothing.
    completed.cancel()
    expect(x.get()).toBe(9)
  })

  it("lets a jump or a stop end the animation", async () => {
    const renderer = fake()
    const x = motionValue(0)
    x.bind(host(renderer))
    const jumped = x.animate(5)
    x.jump(1)
    await jumped.finished
    const stopped = x.animate(5)
    x.stop()
    await stopped.finished
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
    await controls.finished
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
    // A spring only sees its source on the frames that sample it, and a read
    // is not a frame: an element binding or a listener makes it tick.
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
})
