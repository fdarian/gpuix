import { describe, expect, it, vi } from "vitest"
import { AnimatePresence, motion, useIsPresent } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import type { TestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

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
    <motion.div
      initial={false}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      transition={linear}
      style={{ height: 20 }}
    >
      <text>{props.name}</text>
    </motion.div>
  )
}

describeNative('AnimatePresence mode="wait"', () => {
  it("renders the entering child only after the exit finishes", () => {
    const { render, renderer } = createTestRoot()
    const events: string[] = []
    const app = (name: string) => (
      <div>
        <AnimatePresence mode="wait" onExitComplete={() => events.push("exited")}>
          <Page key={name} name={name} />
        </AnimatePresence>
      </div>
    )

    render(app("a"))
    renderer.clockPause()
    render(app("b"))
    expect(renderer.getAllText()).toEqual(["a"])

    frame(renderer, 100)
    expect(renderer.getAllText()).toEqual(["a"])
    expect(events).toEqual([])

    frame(renderer, 100)
    expect(renderer.getAllText()).toEqual(["b"])
    expect(events).toEqual(["exited"])
  })

  it("waits for every exit, and keeps children that stay", () => {
    const { render, renderer } = createTestRoot()
    const app = (names: string[]) => (
      <div>
        <AnimatePresence mode="wait">
          {names.map((name) => (
            <Page key={name} name={name} />
          ))}
        </AnimatePresence>
      </div>
    )

    render(app(["a", "b"]))
    renderer.clockPause()
    render(app(["b", "c"]))
    expect(renderer.getAllText()).toEqual(["a", "b"])

    frame(renderer, 200)
    expect(renderer.getAllText()).toEqual(["b", "c"])
  })

  it("lets a child without an exit target hand over at once", async () => {
    const { render, renderer } = createTestRoot()
    const app = (name: string) => (
      <div>
        <AnimatePresence mode="wait">
          <div key={name}>
            <text>{name}</text>
          </div>
        </AnimatePresence>
      </div>
    )

    render(app("a"))
    render(app("b"))
    await vi.waitFor(() => {
      renderer.flush()
      expect(renderer.getAllText()).toEqual(["b"])
    })
  })

  it("renders entering children immediately in sync mode", () => {
    const { render, renderer } = createTestRoot()
    const app = (name: string) => (
      <div>
        <AnimatePresence>
          <Page key={name} name={name} />
        </AnimatePresence>
      </div>
    )

    render(app("a"))
    renderer.clockPause()
    render(app("b"))
    expect(renderer.getAllText()).toEqual(["a", "b"])
  })
})

describeNative('AnimatePresence mode="popLayout"', () => {
  function List(props: { items: string[]; mode?: "popLayout" | "sync" }) {
    return (
      <div style={{ ...column, position: "relative" }}>
        <AnimatePresence mode={props.mode ?? "popLayout"}>
          {props.items.map((item) => (
            <motion.div
              key={item}
              testId={item}
              initial={false}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 1, ease: "linear" }}
              style={{ height: 20 }}
            />
          ))}
        </AnimatePresence>
      </div>
    )
  }

  it("takes the exiting child out of flow so siblings reflow at once", () => {
    const { render, renderer } = createTestRoot()
    render(<List items={["a", "b", "c"]} />)
    expect(boundsOf(renderer, "b").y).toBe(20)
    expect(boundsOf(renderer, "c").y).toBe(40)

    renderer.clockPause()
    render(<List items={["b", "c"]} />)
    expect(renderer.findByTestId("a")).toBeDefined()
    expect(boundsOf(renderer, "b").y).toBe(0)
    expect(boundsOf(renderer, "c").y).toBe(20)
    expect(boundsOf(renderer, "a")).toMatchObject({ x: 0, y: 0, width: 100, height: 20 })

    frame(renderer, 1000)
    expect(renderer.findByTestId("a")).toBeUndefined()
    expect(boundsOf(renderer, "c").y).toBe(20)
  })

  it("keeps the box a middle child last had, relative to its parent", () => {
    const { render, renderer } = createTestRoot()
    const app = (items: string[]) => (
      <div style={{ paddingLeft: 30, paddingTop: 10 }}>
        <List items={items} />
      </div>
    )
    render(app(["a", "b", "c"]))
    const before = boundsOf(renderer, "b")
    expect(before.x).toBe(30)

    renderer.clockPause()
    render(app(["a", "c"]))
    expect(boundsOf(renderer, "b")).toMatchObject({
      x: before.x,
      y: before.y,
      width: before.width,
      height: before.height,
    })
    expect(boundsOf(renderer, "c").y).toBe(before.y)
  })

  it("keeps the box inside a parent that has padding", () => {
    const { render, renderer } = createTestRoot()
    const app = (items: string[]) => (
      <div style={{ ...column, position: "relative", paddingLeft: 30, paddingTop: 10 }}>
        <AnimatePresence mode="popLayout">
          {items.map((item) => (
            <motion.div
              key={item}
              testId={item}
              initial={false}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 1, ease: "linear" }}
              style={{ height: 20 }}
            />
          ))}
        </AnimatePresence>
      </div>
    )
    render(app(["a", "b"]))
    const before = boundsOf(renderer, "b")
    renderer.clockPause()
    render(app(["a"]))
    expect(boundsOf(renderer, "b")).toMatchObject({ x: before.x, y: before.y, width: before.width, height: before.height })
  })

  it("paints a pinned child where it was inside a bordered, padded, scrolled parent with a margin", () => {
    const { render, renderer } = createTestRoot()
    const app = (items: string[]) => (
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
            {items.map((item) => (
              <motion.div
                key={item}
                testId={item}
                initial={false}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 1, ease: "linear" }}
                style={{ height: 20, margin: 8, flexShrink: 0 }}
              />
            ))}
          </AnimatePresence>
        </div>
      </div>
    )
    render(app(["a", "b", "c", "d"]))
    renderer.scrollTo(renderer.findByTestId("scroller").id, 0, -30)
    const before = boundsOf(renderer, "b")

    renderer.clockPause()
    render(app(["a", "c", "d"]))
    expect(boundsOf(renderer, "b")).toEqual(before)
  })

  it("leaves siblings in the way of the exit in sync mode", () => {
    const { render, renderer } = createTestRoot()
    render(<List items={["a", "b"]} mode="sync" />)
    renderer.clockPause()
    render(<List items={["b"]} mode="sync" />)
    expect(boundsOf(renderer, "b").y).toBe(20)
  })

  it("puts the child back in flow when it returns before it leaves", () => {
    const { render, renderer } = createTestRoot()
    render(<List items={["a", "b"]} />)
    renderer.clockPause()
    render(<List items={["b"]} />)
    expect(boundsOf(renderer, "b").y).toBe(0)
    render(<List items={["a", "b"]} />)
    expect(boundsOf(renderer, "b").y).toBe(20)
  })
})

describeNative("AnimatePresence custom", () => {
  function Card(props: { name: string }) {
    return (
      <motion.div
        key={props.name}
        testId={props.name}
        initial={false}
        animate={{ opacity: 1 }}
        exit={(direction) => ({ opacity: direction === "left" ? 0.25 : 0.75 })}
        transition={{ duration: 1, ease: "linear" }}
        style={{ height: 20 }}
      />
    )
  }

  it("hands the current custom to the exit function of the leaving child", () => {
    const { render, renderer } = createTestRoot()
    const app = (names: string[], direction: string) => (
      <div>
        <AnimatePresence custom={direction}>
          {names.map((name) => (
            <Card key={name} name={name} />
          ))}
        </AnimatePresence>
      </div>
    )

    render(app(["a"], "right"))
    render(app([], "left"))
    expect(renderer.findByTestId("a")?.customProps?.motion).toMatchObject({
      isExit: true,
      animate: { opacity: 0.25 },
    })
  })

  it("falls back to the custom prop of the motion component", () => {
    const { render, renderer } = createTestRoot()
    const app = (show: boolean) => (
      <div>
        <AnimatePresence>
          {show ? (
            <motion.div
              key="a"
              testId="a"
              custom={0.5}
              initial={false}
              animate={{ opacity: 1 }}
              exit={(opacity) => ({ opacity: opacity as number })}
              transition={{ duration: 1, ease: "linear" }}
            />
          ) : null}
        </AnimatePresence>
      </div>
    )
    render(app(true))
    render(app(false))
    expect(renderer.findByTestId("a")?.customProps?.motion).toMatchObject({
      animate: { opacity: 0.5 },
    })
  })

  it("does not call the exit function outside AnimatePresence", () => {
    const { render, renderer } = createTestRoot()
    render(
      <motion.div
        testId="a"
        animate={{ opacity: 1 }}
        exit={() => {
          throw new Error("called")
        }}
      />
    )
    expect(renderer.findByTestId("a")).toBeDefined()
  })
})

describeNative("useIsPresent", () => {
  it("reports false while the child leaves", () => {
    const { render, renderer } = createTestRoot()

    function Label() {
      return <text>{useIsPresent() ? "present" : "leaving"}</text>
    }

    const app = (show: boolean) => (
      <AnimatePresence>
        {show ? (
          <motion.div
            key="a"
            initial={false}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={linear}
          >
            <Label />
          </motion.div>
        ) : null}
      </AnimatePresence>
    )

    render(app(true))
    expect(renderer.getAllText()).toEqual(["present"])
    renderer.clockPause()
    render(app(false))
    expect(renderer.getAllText()).toEqual(["leaving"])
  })
})
