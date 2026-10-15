import { For, Show, createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { LayoutGroup, MotionConfig, motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

const linear = { duration: 1, ease: "linear" } as const
const column = { display: "flex", flexDirection: "column" } as const
const row = { display: "flex", flexDirection: "row" } as const

function frame(renderer: TestRenderer, ms: number): void {
  renderer.clockFastForward(ms)
  renderer.flush()
  renderer.dispatchNativeEvents()
}

function boundsOf(renderer: TestRenderer, testId: string) {
  return renderer.getElementBounds(renderer.findByTestId(testId)!.id)!
}

function List(props: { order: string[]; transition?: object; onEvent?: (event: string) => void }) {
  return (
    <div style={column}>
      <For each={props.order}>
        {(name) => (
          <motion.div
            testId={name}
            layout
            transition={props.transition}
            style={{ height: 20, width: 50 }}
            onLayoutAnimationStart={() => props.onEvent?.(`${name} start`)}
            onLayoutAnimationComplete={() => props.onEvent?.(`${name} complete`)}
          />
        )}
      </For>
    </div>
  )
}

function Tabs(props: { active: number; transition?: object }) {
  return (
    <div style={row}>
      <For each={[0, 1, 2]}>
        {(index) => (
          <div style={{ width: 100, height: 20 }}>
            <Show when={props.active === index}>
              <motion.div testId="highlight" layoutId="highlight" transition={props.transition} style={{ width: 100, height: 20 }} />
            </Show>
          </div>
        )}
      </For>
    </div>
  )
}

function Mover() {
  return <motion.div testId="moved" layoutId="h" transition={linear} style={{ width: 50, height: 20 }} />
}

describe.skipIf(!hasNativeTestRenderer)("Solid layout", () => {
  it("glides a sibling to its new position when the list reorders", () => {
    const [order, setOrder] = createSignal(["a", "b"])
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => <List order={order()} transition={linear} />)
    expect(boundsOf(app.renderer, "b").y).toBe(20)

    app.flushSync(() => setOrder(["b", "a"]))
    expect(boundsOf(app.renderer, "b").y).toBeCloseTo(20, 1)
    expect(boundsOf(app.renderer, "a").y).toBeCloseTo(0, 1)
    frame(app.renderer, 500)
    expect(boundsOf(app.renderer, "b").y).toBeCloseTo(10, 1)
    expect(boundsOf(app.renderer, "a").y).toBeCloseTo(10, 1)
    frame(app.renderer, 600)
    expect(boundsOf(app.renderer, "b").y).toBe(0)
    expect(boundsOf(app.renderer, "a").y).toBe(20)
  })

  it("does not glide without the prop", () => {
    const [order, setOrder] = createSignal(["a", "b"])
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <div style={column}>
        <For each={order()}>
          {(name) => <motion.div testId={name} layout={false} transition={linear} style={{ height: 20, width: 50 }} />}
        </For>
      </div>
    ))
    app.flushSync(() => setOrder(["b", "a"]))
    expect(boundsOf(app.renderer, "b").y).toBe(0)
  })

  it("uses transition.layout for the glide and keeps the rest for animate", () => {
    const [order, setOrder] = createSignal(["a", "b"])
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <div style={column}>
        <For each={order()}>
          {(name) => (
            <motion.div
              testId={name}
              layout
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ duration: 10, layout: linear }}
              style={{ height: 20, width: 50 }}
            />
          )}
        </For>
      </div>
    ))
    app.flushSync(() => setOrder(["b", "a"]))
    frame(app.renderer, 500)
    expect(boundsOf(app.renderer, "b").y).toBeCloseTo(10, 1)
    frame(app.renderer, 600)
    expect(boundsOf(app.renderer, "b").y).toBe(0)
  })

  it("takes the transition from MotionConfig when the component has none", () => {
    const [order, setOrder] = createSignal(["a", "b"])
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <MotionConfig transition={linear}>
        <List order={order()} />
      </MotionConfig>
    ))
    app.flushSync(() => setOrder(["b", "a"]))
    frame(app.renderer, 500)
    expect(boundsOf(app.renderer, "b").y).toBeCloseTo(10, 1)
  })

  it("jumps under reduced motion", () => {
    const [order, setOrder] = createSignal(["a", "b"])
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <MotionConfig reducedMotion="always">
        <List order={order()} transition={linear} />
      </MotionConfig>
    ))
    app.flushSync(() => setOrder(["b", "a"]))
    expect(boundsOf(app.renderer, "b").y).toBe(0)
  })

  it("reports when a glide starts and completes", () => {
    const [order, setOrder] = createSignal(["a", "b"])
    const events: string[] = []
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => <List order={order()} transition={linear} onEvent={(event) => events.push(event)} />)
    app.flushSync(() => setOrder(["b", "a"]))
    frame(app.renderer, 16)
    expect([...events].sort()).toEqual(["a start", "b start"])
    frame(app.renderer, 1100)
    frame(app.renderer, 16)
    expect([...events].sort()).toEqual(["a complete", "a start", "b complete", "b start"])
  })
})

describe.skipIf(!hasNativeTestRenderer)("Solid layoutId", () => {
  it("moves a shared highlight from one tab to the next", () => {
    const [active, setActive] = createSignal(0)
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => <Tabs active={active()} transition={linear} />)
    expect(boundsOf(app.renderer, "highlight").x).toBe(0)

    app.flushSync(() => setActive(2))
    expect(boundsOf(app.renderer, "highlight").x).toBeCloseTo(0, 1)
    frame(app.renderer, 500)
    expect(boundsOf(app.renderer, "highlight").x).toBeCloseTo(100, 1)
    frame(app.renderer, 600)
    expect(boundsOf(app.renderer, "highlight").x).toBe(200)
  })

  it("jumps under reduced motion", () => {
    const [active, setActive] = createSignal(0)
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <MotionConfig reducedMotion="always">
        <Tabs active={active()} transition={linear} />
      </MotionConfig>
    ))
    app.flushSync(() => setActive(2))
    expect(boundsOf(app.renderer, "highlight").x).toBe(200)
  })

  it("lets a LayoutGroup keep two sets of tabs apart", () => {
    const [inFirst, setInFirst] = createSignal(true)
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <div style={row}>
        <LayoutGroup id="first">
          <div style={{ width: 100, height: 20 }}>
            <Show when={inFirst()}>
              <Mover />
            </Show>
          </div>
        </LayoutGroup>
        <LayoutGroup id="second">
          <div style={{ width: 100, height: 20 }}>
            <Show when={!inFirst()}>
              <Mover />
            </Show>
          </div>
        </LayoutGroup>
      </div>
    ))
    app.flushSync(() => setInFirst(false))
    expect(boundsOf(app.renderer, "moved").x).toBe(100)
  })

  it("shares the glide across the same ids with no group", () => {
    const [inFirst, setInFirst] = createSignal(true)
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <div style={row}>
        <div style={{ width: 100, height: 20 }}>
          <Show when={inFirst()}>
            <Mover />
          </Show>
        </div>
        <div style={{ width: 100, height: 20 }}>
          <Show when={!inFirst()}>
            <Mover />
          </Show>
        </div>
      </div>
    ))
    app.flushSync(() => setInFirst(false))
    expect(boundsOf(app.renderer, "moved").x).toBeCloseTo(0, 1)
    frame(app.renderer, 500)
    expect(boundsOf(app.renderer, "moved").x).toBeCloseTo(50, 1)
  })

  it("joins nested group ids", () => {
    const [nested, setNested] = createSignal(true)
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <div style={row}>
        <div style={{ width: 100, height: 20 }}>
          <Show when={nested()}>
            <LayoutGroup id="a">
              <LayoutGroup id="b">
                <Mover />
              </LayoutGroup>
            </LayoutGroup>
          </Show>
        </div>
        <div style={{ width: 100, height: 20 }}>
          <Show when={!nested()}>
            <LayoutGroup id="a-b">
              <Mover />
            </LayoutGroup>
          </Show>
        </div>
      </div>
    ))
    app.flushSync(() => setNested(false))
    expect(boundsOf(app.renderer, "moved").x).toBeCloseTo(0, 1)
    frame(app.renderer, 500)
    expect(boundsOf(app.renderer, "moved").x).toBeCloseTo(50, 1)
  })

  it("keeps a grouped id from sharing with the ungrouped one", () => {
    const [grouped, setGrouped] = createSignal(false)
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <div style={row}>
        <div style={{ width: 100, height: 20 }}>
          <Show when={!grouped()}>
            <Mover />
          </Show>
        </div>
        <div style={{ width: 100, height: 20 }}>
          <Show when={grouped()}>
            <LayoutGroup id="g">
              <Mover />
            </LayoutGroup>
          </Show>
        </div>
      </div>
    ))
    app.flushSync(() => setGrouped(true))
    expect(boundsOf(app.renderer, "moved").x).toBe(100)
  })
})
