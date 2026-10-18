import { For, Show, createEffect, createSignal, onCleanup } from "solid-js"
import { describe, expect, it } from "bun:test"
import { AnimatePresence, motion, useIsPresent, usePresence } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

const linear = { duration: 0.2, ease: "linear" } as const

function frame(renderer: TestRenderer, ms: number): void {
  renderer.clockFastForward(ms)
  renderer.dispatchNativeEvents()
  renderer.flush()
}

function Status(props: { log: string[] }) {
  const isPresent = useIsPresent()
  createEffect(() => {
    props.log.push(isPresent() ? "present" : "leaving")
  })
  onCleanup(() => props.log.push("disposed"))
  return <text>{isPresent() ? "here" : "leaving"}</text>
}

function Card(props: { log: string[] }) {
  return (
    <motion.div initial={false} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={linear} style={{ height: 20 }}>
      <div>
        <Status log={props.log} />
      </div>
    </motion.div>
  )
}

describe.skipIf(!hasNativeTestRenderer)("Solid useIsPresent in a leaving child", () => {
  it("flips to false in a grandchild while the exit runs, and its effects stay alive until the child is gone", () => {
    const [show, setShow] = createSignal(true)
    const log: string[] = []
    const app = createTestRoot()
    app.render(() => (
      <div>
        <AnimatePresence>
          <Show when={show()}>
            <Card log={log} />
          </Show>
        </AnimatePresence>
      </div>
    ))
    expect(app.renderer.getAllText()).toEqual(["here"])
    app.renderer.clockPause()
    app.flushSync(() => setShow(false))
    expect(app.renderer.getAllText()).toEqual(["leaving"])
    expect(log).toEqual(["present", "leaving"])

    frame(app.renderer, 100)
    expect(app.renderer.getAllText()).toEqual(["leaving"])
    expect(log).not.toContain("disposed")

    frame(app.renderer, 100)
    expect(app.renderer.getAllText()).toEqual([])
    expect(log).toEqual(["present", "leaving", "disposed"])
  })

  it("is present outside an AnimatePresence", () => {
    const log: string[] = []
    const app = createTestRoot()
    app.render(() => <Status log={log} />)
    expect(app.renderer.getAllText()).toEqual(["here"])
  })

  it("reads present for a child that stays", () => {
    const [names, setNames] = createSignal(["a", "b"])
    const app = createTestRoot()
    app.render(() => (
      <div>
        <AnimatePresence>
          <For each={names()}>{(name) => <Card log={[name]} />}</For>
        </AnimatePresence>
      </div>
    ))
    app.renderer.clockPause()
    app.flushSync(() => setNames(["b"]))
    expect(app.renderer.getAllText()).toEqual(["leaving", "here"])
  })
})

describe.skipIf(!hasNativeTestRenderer)("Solid usePresence safeToRemove", () => {
  function Participant(props: { name: string; removers: Map<string, () => void> }) {
    const [isPresent, safeToRemove] = usePresence()
    createEffect(() => {
      if (!isPresent()) props.removers.set(props.name, safeToRemove)
    })
    return <text>{props.name}</text>
  }

  it("keeps a child with no exit until safeToRemove is called", () => {
    const [show, setShow] = createSignal(true)
    const removers = new Map<string, () => void>()
    const exits: string[] = []
    const app = createTestRoot()
    app.render(() => (
      <div>
        <AnimatePresence onExitComplete={() => exits.push("exited")}>
          <Show when={show()}>
            <div>
              <Participant name="a" removers={removers} />
            </div>
          </Show>
        </AnimatePresence>
      </div>
    ))
    app.flushSync(() => setShow(false))
    expect(app.renderer.getAllText()).toEqual(["a"])
    expect(exits).toEqual([])

    app.flushSync(() => removers.get("a")?.())
    expect(app.renderer.getAllText()).toEqual([])
    expect(exits).toEqual(["exited"])
  })

  it("waits for every participant", () => {
    const [show, setShow] = createSignal(true)
    const removers = new Map<string, () => void>()
    const app = createTestRoot()
    app.render(() => (
      <div>
        <AnimatePresence>
          <Show when={show()}>
            <div>
              <Participant name="a" removers={removers} />
              <Participant name="b" removers={removers} />
            </div>
          </Show>
        </AnimatePresence>
      </div>
    ))
    app.flushSync(() => setShow(false))
    app.flushSync(() => removers.get("a")?.())
    expect(app.renderer.getAllText()).toEqual(["a", "b"])
    app.flushSync(() => removers.get("b")?.())
    expect(app.renderer.getAllText()).toEqual([])
  })

  it("waits for the exit animation as well", () => {
    const [show, setShow] = createSignal(true)
    const removers = new Map<string, () => void>()
    const app = createTestRoot()
    app.render(() => (
      <div>
        <AnimatePresence>
          <Show when={show()}>
            <motion.div initial={false} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={linear}>
              <Participant name="a" removers={removers} />
            </motion.div>
          </Show>
        </AnimatePresence>
      </div>
    ))
    app.renderer.clockPause()
    app.flushSync(() => setShow(false))
    app.flushSync(() => removers.get("a")?.())
    expect(app.renderer.getAllText()).toEqual(["a"])

    frame(app.renderer, 200)
    expect(app.renderer.getAllText()).toEqual([])
  })

  it("removes the child once the animation is over and the participant is done, in either order", () => {
    const [show, setShow] = createSignal(true)
    const removers = new Map<string, () => void>()
    const app = createTestRoot()
    app.render(() => (
      <div>
        <AnimatePresence>
          <Show when={show()}>
            <motion.div initial={false} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={linear}>
              <Participant name="a" removers={removers} />
            </motion.div>
          </Show>
        </AnimatePresence>
      </div>
    ))
    app.renderer.clockPause()
    app.flushSync(() => setShow(false))
    frame(app.renderer, 200)
    expect(app.renderer.getAllText()).toEqual(["a"])

    app.flushSync(() => removers.get("a")?.())
    expect(app.renderer.getAllText()).toEqual([])
  })

  it("completes the exit when the only participant unmounts first", () => {
    const [show, setShow] = createSignal(true)
    const [inner, setInner] = createSignal(true)
    const removers = new Map<string, () => void>()
    const app = createTestRoot()
    app.render(() => (
      <div>
        <AnimatePresence>
          <Show when={show()}>
            <div>
              <Show when={inner()}>
                <Participant name="a" removers={removers} />
              </Show>
              <text>kept</text>
            </div>
          </Show>
        </AnimatePresence>
      </div>
    ))
    app.flushSync(() => setShow(false))
    expect(app.renderer.getAllText()).toEqual(["a", "kept"])
    app.flushSync(() => setInner(false))
    expect(app.renderer.getAllText()).toEqual([])
  })

  it("ignores a safeToRemove call while the child is present", () => {
    const [show] = createSignal(true)
    let remove: (() => void) | undefined
    function Early() {
      remove = usePresence()[1]
      return <text>early</text>
    }
    const app = createTestRoot()
    app.render(() => (
      <div>
        <AnimatePresence>
          <Show when={show()}>
            <div><Early /></div>
          </Show>
        </AnimatePresence>
      </div>
    ))
    app.flushSync(() => remove?.())
    expect(app.renderer.getAllText()).toEqual(["early"])
  })
})

describe.skipIf(!hasNativeTestRenderer)("Solid AnimatePresence re-entry while a child leaves", () => {
  it("keeps the leaving child's participants leaving while an entering child's are present", () => {
    const [page, setPage] = createSignal("a")
    const removers = new Map<string, () => void>()
    function Participant(props: { name: string }) {
      const [isPresent, safeToRemove] = usePresence()
      createEffect(() => {
        if (!isPresent()) removers.set(props.name, safeToRemove)
      })
      return <text>{isPresent() ? props.name : `${props.name} leaving`}</text>
    }
    const app = createTestRoot()
    app.render(() => (
      <div>
        <AnimatePresence>
          <Show when={page()} keyed>
            {(name) => (
              <div>
                <Participant name={name} />
              </div>
            )}
          </Show>
        </AnimatePresence>
      </div>
    ))
    app.flushSync(() => setPage("b"))
    expect(app.renderer.getAllText()).toEqual(["a leaving", "b"])

    app.flushSync(() => setPage("a"))
    expect(app.renderer.getAllText()).toEqual(["a leaving", "b leaving", "a"])
    expect(removers.has("b")).toBe(true)

    app.flushSync(() => removers.get("b")?.())
    expect(app.renderer.getAllText()).toEqual(["a leaving", "a"])
  })

  it("does not complete an exit that was waiting on a child that has returned", () => {
    const [show, setShow] = createSignal(true)
    const exits: string[] = []
    function Participant() {
      usePresence()
      return <text>p</text>
    }
    const app = createTestRoot()
    app.render(() => (
      <div>
        <AnimatePresence onExitComplete={() => exits.push("exited")}>
          <Show when={show()}>
            <div>
              <Participant />
            </div>
          </Show>
        </AnimatePresence>
      </div>
    ))
    app.flushSync(() => setShow(false))
    app.flushSync(() => setShow(true))
    expect(exits).toEqual([])
    expect(app.renderer.getAllText()).toEqual(["p", "p"])
  })
})
