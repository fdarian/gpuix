import { For, Show, createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { AnimatePresence, motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

const linear = { duration: 0.2, ease: "linear" } as const
const column = { display: "flex", flexDirection: "column", width: 100 } as const

function frame(renderer: TestRenderer, ms: number): void {
  renderer.clockFastForward(ms)
  renderer.dispatchNativeEvents()
  renderer.flush()
}

function boundsOf(renderer: TestRenderer, testId: string) {
  const element = renderer.findByTestId(testId)
  if (!element) throw new Error(`no element ${testId}`)
  const bounds = renderer.getElementBounds(element.id)
  if (!bounds) throw new Error(`${testId} did not paint`)
  return bounds
}

function Page(props: { name: string }) {
  return (
    <motion.div initial={false} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={linear} style={{ height: 20 }}>
      <text>{props.name}</text>
    </motion.div>
  )
}

describe.skipIf(!hasNativeTestRenderer)('Solid AnimatePresence mode="wait"', () => {
  it("renders the entering child only after the exit finishes", () => {
    const [name, setName] = createSignal("a")
    const events: string[] = []
    const app = createTestRoot()
    app.render(() => (
      <div>
        <AnimatePresence mode="wait" onExitComplete={() => events.push("exited")}>
          <Show when={name()} keyed>
            {(page) => <Page name={page} />}
          </Show>
        </AnimatePresence>
      </div>
    ))
    app.renderer.clockPause()
    app.flushSync(() => setName("b"))
    expect(app.renderer.getAllText()).toEqual(["a"])

    frame(app.renderer, 100)
    expect(app.renderer.getAllText()).toEqual(["a"])
    expect(events).toEqual([])

    frame(app.renderer, 100)
    expect(app.renderer.getAllText()).toEqual(["b"])
    expect(events).toEqual(["exited"])
  })

  it("renders the entering child at once when the leaving one has no exit", () => {
    const [name, setName] = createSignal("a")
    const events: string[] = []
    const app = createTestRoot()
    app.render(() => (
      <div>
        <AnimatePresence mode="wait" onExitComplete={() => events.push("exited")}>
          <Show when={name()} keyed>
            {(page) => <div><text>{page}</text></div>}
          </Show>
        </AnimatePresence>
      </div>
    ))
    app.flushSync(() => setName("b"))
    expect(app.renderer.getAllText()).toEqual(["b"])
    expect(events).toEqual(["exited"])
  })

  it("waits for every exit, and keeps children that stay", () => {
    const [names, setNames] = createSignal(["a", "b"])
    const app = createTestRoot()
    app.render(() => (
      <div>
        <AnimatePresence mode="wait">
          <For each={names()}>{(name) => <Page name={name} />}</For>
        </AnimatePresence>
      </div>
    ))
    app.renderer.clockPause()
    app.flushSync(() => setNames(["b", "c"]))
    expect(app.renderer.getAllText()).toEqual(["a", "b"])

    frame(app.renderer, 200)
    expect(app.renderer.getAllText()).toEqual(["b", "c"])
  })

  it("renders entering children immediately in sync mode", () => {
    const [name, setName] = createSignal("a")
    const app = createTestRoot()
    app.render(() => (
      <div>
        <AnimatePresence>
          <Show when={name()} keyed>
            {(page) => <Page name={page} />}
          </Show>
        </AnimatePresence>
      </div>
    ))
    app.renderer.clockPause()
    app.flushSync(() => setName("b"))
    expect(app.renderer.getAllText()).toEqual(["a", "b"])
  })
})

describe.skipIf(!hasNativeTestRenderer)('Solid AnimatePresence mode="popLayout"', () => {
  function List(props: { items: string[] }) {
    return (
      <div style={{ ...column, position: "relative" }}>
        <AnimatePresence mode="popLayout">
          <For each={props.items}>
            {(item) => (
              <motion.div
                testId={item}
                initial={false}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 1, ease: "linear" }}
                style={{ height: 20 }}
              />
            )}
          </For>
        </AnimatePresence>
      </div>
    )
  }

  it("takes the exiting child out of flow so siblings reflow at once", () => {
    const [items, setItems] = createSignal(["a", "b", "c"])
    const app = createTestRoot()
    app.render(() => <List items={items()} />)
    expect(boundsOf(app.renderer, "b").y).toBe(20)
    expect(boundsOf(app.renderer, "c").y).toBe(40)

    app.renderer.clockPause()
    app.flushSync(() => setItems(["b", "c"]))
    expect(boundsOf(app.renderer, "b").y).toBe(0)
    expect(boundsOf(app.renderer, "c").y).toBe(20)
    expect(boundsOf(app.renderer, "a")).toMatchObject({ x: 0, y: 0, width: 100, height: 20 })

    frame(app.renderer, 600)
    frame(app.renderer, 600)
    expect(app.renderer.findByTestId("a")).toBeUndefined()
    expect(boundsOf(app.renderer, "c").y).toBe(20)
  })

  it("keeps the box a middle child last had, relative to its parent", () => {
    const [items, setItems] = createSignal(["a", "b", "c"])
    const app = createTestRoot()
    app.render(() => (
      <div style={{ paddingLeft: 30, paddingTop: 10 }}>
        <List items={items()} />
      </div>
    ))
    const before = boundsOf(app.renderer, "b")
    expect(before.x).toBe(30)

    app.renderer.clockPause()
    app.flushSync(() => setItems(["a", "c"]))
    expect(boundsOf(app.renderer, "b")).toMatchObject({
      x: before.x,
      y: before.y,
      width: before.width,
      height: before.height,
    })
    expect(boundsOf(app.renderer, "c").y).toBe(before.y)
  })

  it("paints a pinned child where it was inside a bordered, padded, scrolled parent with a margin", () => {
    const [items, setItems] = createSignal(["a", "b", "c", "d"])
    const app = createTestRoot()
    app.render(() => (
      <div style={{ paddingLeft: 30, paddingTop: 10 }}>
        <div
          testId="scroller"
          style={{
            ...column,
            height: 60,
            position: "relative",
            overflow: "scroll",
            borderWidth: 2,
            paddingLeft: 6,
            paddingTop: 4,
          }}
        >
          <AnimatePresence mode="popLayout">
            <For each={items()}>
              {(item) => (
                <motion.div
                  testId={item}
                  initial={false}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  transition={{ duration: 1, ease: "linear" }}
                  style={{ height: 20, margin: 8, flexShrink: 0 }}
                />
              )}
            </For>
          </AnimatePresence>
        </div>
      </div>
    ))
    app.renderer.scrollTo(app.renderer.findByTestId("scroller")!.id, 0, 30)
    const before = boundsOf(app.renderer, "b")

    app.renderer.clockPause()
    app.flushSync(() => setItems(["a", "c", "d"]))
    expect(boundsOf(app.renderer, "b")).toEqual(before)
  })
  it("puts the child back in flow when it returns before it leaves", () => {
    const [items, setItems] = createSignal(["a", "b"])
    const app = createTestRoot()
    app.render(() => <List items={items()} />)
    app.renderer.clockPause()
    app.flushSync(() => setItems(["b"]))
    expect(boundsOf(app.renderer, "b").y).toBe(0)
    app.flushSync(() => setItems(["a", "b"]))
    expect(boundsOf(app.renderer, "b").y).toBe(20)
  })
})

describe.skipIf(!hasNativeTestRenderer)("Solid AnimatePresence", () => {
  it("keeps an exiting child where it was while its siblings stay, until its exit completes", () => {
    const [names, setNames] = createSignal(["a", "b", "c"])
    const app = createTestRoot()
    app.render(() => (
      <div>
        <AnimatePresence>
          <For each={names()}>{(name) => <Page name={name} />}</For>
        </AnimatePresence>
      </div>
    ))
    app.renderer.clockPause()
    app.flushSync(() => setNames(["a", "c"]))
    expect(app.renderer.getAllText()).toEqual(["a", "b", "c"])

    frame(app.renderer, 200)
    expect(app.renderer.getAllText()).toEqual(["a", "c"])
  })
})

describe.skipIf(!hasNativeTestRenderer)("Solid AnimatePresence custom", () => {
  it("hands the current custom to the exit function of the leaving child", () => {
    const [show, setShow] = createSignal(true)
    const [direction, setDirection] = createSignal("right")
    const app = createTestRoot()
    app.render(() => (
      <div>
        <AnimatePresence custom={direction()}>
          <Show when={show()}>
            <motion.div
              testId="a"
              initial={false}
              animate={{ opacity: 1 }}
              exit={(custom) => ({ opacity: custom === "left" ? 0.25 : 0.75 })}
              transition={{ duration: 1, ease: "linear" }}
              style={{ height: 20 }}
            />
          </Show>
        </AnimatePresence>
      </div>
    ))
    app.flushSync(() => {
      setDirection("left")
      setShow(false)
    })
    expect(app.renderer.findByTestId("a")?.customProps?.motion).toMatchObject({
      isExit: true,
      animate: { opacity: 0.25 },
    })
  })

  it("falls back to the custom prop of the motion component", () => {
    const [show, setShow] = createSignal(true)
    const app = createTestRoot()
    app.render(() => (
      <div>
        <AnimatePresence>
          <Show when={show()}>
            <motion.div
              testId="a"
              custom={0.5}
              initial={false}
              animate={{ opacity: 1 }}
              exit={(opacity) => ({ opacity: opacity as number })}
              transition={{ duration: 1, ease: "linear" }}
            />
          </Show>
        </AnimatePresence>
      </div>
    ))
    app.flushSync(() => setShow(false))
    expect(app.renderer.findByTestId("a")?.customProps?.motion).toMatchObject({ animate: { opacity: 0.5 } })
  })

  it("does not call the exit function outside AnimatePresence", () => {
    const app = createTestRoot()
    app.render(() => (
      <motion.div
        testId="a"
        animate={{ opacity: 1 }}
        exit={() => {
          throw new Error("called")
        }}
      />
    ))
    expect(app.renderer.findByTestId("a")).toBeDefined()
  })
})

describe.skipIf(!hasNativeTestRenderer)("Solid AnimatePresence swaps", () => {
  it("leaves only the entering child when the leaving one has no exit", () => {
    const [name, setName] = createSignal("a")
    const app = createTestRoot()
    app.render(() => (
      <div>
        <AnimatePresence>
          <Show when={name()} keyed>
            {(page) => <motion.div animate={{ opacity: 1 }}><text>{page}</text></motion.div>}
          </Show>
        </AnimatePresence>
      </div>
    ))
    app.renderer.clockPause()
    app.flushSync(() => setName("b"))
    frame(app.renderer, 500)
    expect(app.renderer.getAllText()).toEqual(["b"])
    app.flushSync(() => setName("c"))
    frame(app.renderer, 500)
    expect(app.renderer.getAllText()).toEqual(["c"])
  })

  it("exits the leaving child and then removes it while another enters", () => {
    const [name, setName] = createSignal("a")
    const app = createTestRoot()
    app.render(() => (
      <div>
        <AnimatePresence>
          <Show when={name()} keyed>
            {(page) => <Page name={page} />}
          </Show>
        </AnimatePresence>
      </div>
    ))
    app.renderer.clockPause()
    app.flushSync(() => setName("b"))
    expect(app.renderer.getAllText()).toEqual(["a", "b"])

    frame(app.renderer, 100)
    expect(app.renderer.getAllText()).toEqual(["a", "b"])

    frame(app.renderer, 200)
    expect(app.renderer.getAllText()).toEqual(["b"])
  })
})
