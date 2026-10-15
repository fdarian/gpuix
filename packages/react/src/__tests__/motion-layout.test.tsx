import { describe, expect, it } from "vitest"
import { LayoutGroup, MotionConfig, motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const linear = { duration: 1, ease: "linear" } as const
const column = { display: "flex", flexDirection: "column" } as const
const row = { display: "flex", flexDirection: "row" } as const

function frame(renderer: TestRenderer, ms: number): void {
  renderer.clockFastForward(ms)
  renderer.flush()
  renderer.dispatchNativeEvents()
}

function boundsOf(renderer: TestRenderer, testId: string) {
  return renderer.getElementBounds(renderer.findByTestId(testId).id)
}

function List(props: { order: string[]; transition?: object }) {
  return (
    <div style={column}>
      {props.order.map((name) => (
        <motion.div
          key={name}
          testId={name}
          layout
          transition={props.transition}
          style={{ height: 20, width: 50 }}
        />
      ))}
    </div>
  )
}

function Tabs(props: { active: number; transition?: object; scope?: string }) {
  return (
    <div style={row}>
      {[0, 1, 2].map((index) => (
        <div key={index} style={{ width: 100, height: 20 }}>
          {props.active === index ? (
            <motion.div
              testId={`highlight-${props.scope ?? ""}`}
              layoutId="highlight"
              transition={props.transition}
              style={{ width: 100, height: 20 }}
            />
          ) : null}
        </div>
      ))}
    </div>
  )
}

describeNative("layout", () => {
  it("glides a sibling to its new position when the list reorders", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(<List order={["a", "b"]} transition={linear} />)
    expect(boundsOf(renderer, "b").y).toBe(20)

    render(<List order={["b", "a"]} transition={linear} />)
    expect(boundsOf(renderer, "b").y).toBeCloseTo(20, 1)
    expect(boundsOf(renderer, "a").y).toBeCloseTo(0, 1)
    frame(renderer, 500)
    expect(boundsOf(renderer, "b").y).toBeCloseTo(10, 1)
    expect(boundsOf(renderer, "a").y).toBeCloseTo(10, 1)
    frame(renderer, 600)
    expect(boundsOf(renderer, "b").y).toBe(0)
    expect(boundsOf(renderer, "a").y).toBe(20)
  })

  it("does not glide without the prop, and layout={false} turns it off", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const app = (order: string[], layout?: boolean) => (
      <div style={column}>
        {order.map((name) => (
          <motion.div key={name} testId={name} layout={layout} transition={linear} style={{ height: 20, width: 50 }} />
        ))}
      </div>
    )
    render(app(["a", "b"], false))
    render(app(["b", "a"], false))
    expect(boundsOf(renderer, "b").y).toBe(0)
  })

  it("uses transition.layout for the glide and keeps the rest for animate", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const app = (order: string[]) => (
      <div style={column}>
        {order.map((name) => (
          <motion.div
            key={name}
            testId={name}
            layout
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ duration: 10, layout: linear }}
            style={{ height: 20, width: 50 }}
          />
        ))}
      </div>
    )
    render(app(["a", "b"]))
    render(app(["b", "a"]))
    frame(renderer, 500)
    expect(boundsOf(renderer, "b").y).toBeCloseTo(10, 1)
    frame(renderer, 600)
    expect(boundsOf(renderer, "b").y).toBe(0)
  })

  it("takes the transition from MotionConfig when the component has none", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const app = (order: string[]) => (
      <MotionConfig transition={linear}>
        <List order={order} />
      </MotionConfig>
    )
    render(app(["a", "b"]))
    render(app(["b", "a"]))
    frame(renderer, 500)
    expect(boundsOf(renderer, "b").y).toBeCloseTo(10, 1)
  })

  it("jumps under reduced motion", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const app = (order: string[]) => (
      <MotionConfig reducedMotion="always">
        <List order={order} transition={linear} />
      </MotionConfig>
    )
    render(app(["a", "b"]))
    render(app(["b", "a"]))
    expect(boundsOf(renderer, "b").y).toBe(0)
  })

  it("reports when a glide starts and completes", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const events: string[] = []
    const app = (order: string[]) => (
      <div style={column}>
        {order.map((name) => (
          <motion.div
            key={name}
            testId={name}
            layout
            transition={linear}
            style={{ height: 20, width: 50 }}
            onLayoutAnimationStart={() => events.push(`${name} start`)}
            onLayoutAnimationComplete={() => events.push(`${name} complete`)}
          />
        ))}
      </div>
    )
    render(app(["a", "b"]))
    render(app(["b", "a"]))
    frame(renderer, 16)
    expect([...events].sort()).toEqual(["a start", "b start"])
    frame(renderer, 1100)
    frame(renderer, 16)
    expect([...events].sort()).toEqual(["a complete", "a start", "b complete", "b start"])
  })
})

describeNative("layoutId", () => {
  it("moves a shared highlight from one tab to the next", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(<Tabs active={0} transition={linear} />)
    expect(boundsOf(renderer, "highlight-").x).toBe(0)

    render(<Tabs active={2} transition={linear} />)
    expect(boundsOf(renderer, "highlight-").x).toBeCloseTo(0, 1)
    frame(renderer, 500)
    expect(boundsOf(renderer, "highlight-").x).toBeCloseTo(100, 1)
    frame(renderer, 600)
    expect(boundsOf(renderer, "highlight-").x).toBe(200)
  })

  it("jumps under reduced motion", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const app = (active: number) => (
      <MotionConfig reducedMotion="always">
        <Tabs active={active} transition={linear} />
      </MotionConfig>
    )
    render(app(0))
    render(app(2))
    expect(boundsOf(renderer, "highlight-").x).toBe(200)
  })

  it("lets a LayoutGroup keep two sets of tabs apart", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    // The highlight leaves the first group and appears in the second, with the same layoutId.
    const app = (inFirst: boolean) => (
      <div style={row}>
        <LayoutGroup id="first">
          <div style={{ width: 100, height: 20 }}>
            {inFirst ? <motion.div testId="moved" layoutId="h" transition={linear} style={{ width: 50, height: 20 }} /> : null}
          </div>
        </LayoutGroup>
        <LayoutGroup id="second">
          <div style={{ width: 100, height: 20 }}>
            {inFirst ? null : <motion.div testId="moved" layoutId="h" transition={linear} style={{ width: 50, height: 20 }} />}
          </div>
        </LayoutGroup>
      </div>
    )
    render(app(true))
    render(app(false))
    expect(boundsOf(renderer, "moved").x).toBe(100)
  })

  it("shares the glide across the same ids with no group", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const app = (inFirst: boolean) => (
      <div style={row}>
        <div style={{ width: 100, height: 20 }}>
          {inFirst ? <motion.div testId="moved" layoutId="h" transition={linear} style={{ width: 50, height: 20 }} /> : null}
        </div>
        <div style={{ width: 100, height: 20 }}>
          {inFirst ? null : <motion.div testId="moved" layoutId="h" transition={linear} style={{ width: 50, height: 20 }} />}
        </div>
      </div>
    )
    render(app(true))
    render(app(false))
    expect(boundsOf(renderer, "moved").x).toBeCloseTo(0, 1)
    frame(renderer, 500)
    expect(boundsOf(renderer, "moved").x).toBeCloseTo(50, 1)
  })

  it("joins nested group ids", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    // "a" > "b" scopes h as "a-b-h", and so does the single group "a-b".
    const app = (nested: boolean) => (
      <div style={row}>
        <div style={{ width: 100, height: 20 }}>
          {nested ? (
            <LayoutGroup id="a">
              <LayoutGroup id="b">
                <motion.div testId="moved" layoutId="h" transition={linear} style={{ width: 50, height: 20 }} />
              </LayoutGroup>
            </LayoutGroup>
          ) : null}
        </div>
        <div style={{ width: 100, height: 20 }}>
          {nested ? null : (
            <LayoutGroup id="a-b">
              <motion.div testId="moved" layoutId="h" transition={linear} style={{ width: 50, height: 20 }} />
            </LayoutGroup>
          )}
        </div>
      </div>
    )
    render(app(true))
    render(app(false))
    expect(boundsOf(renderer, "moved").x).toBeCloseTo(0, 1)
    frame(renderer, 500)
    expect(boundsOf(renderer, "moved").x).toBeCloseTo(50, 1)
  })

  it("keeps a group inside another from sharing with the ungrouped id", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const app = (grouped: boolean) => {
      const box = <motion.div testId="moved" layoutId="h" transition={linear} style={{ width: 50, height: 20 }} />
      return (
        <div style={row}>
          <div style={{ width: 100, height: 20 }}>{grouped ? null : box}</div>
          <div style={{ width: 100, height: 20 }}>{grouped ? <LayoutGroup id="g">{box}</LayoutGroup> : null}</div>
        </div>
      )
    }
    render(app(false))
    render(app(true))
    expect(boundsOf(renderer, "moved").x).toBe(100)
  })
})
