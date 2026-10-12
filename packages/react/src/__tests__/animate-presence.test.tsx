import { useLayoutEffect } from "react"
import { describe, expect, it, vi } from "vitest"
import { AnimatePresence, motion, usePresence } from "../index"
import { flushSync } from "../reconciler/reconciler"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

/**
 * Waits for AnimatePresence to drop a child that has nothing to animate. The
 * removal is a state update from a passive effect, which React's scheduler runs
 * on its own clock, so a fixed sleep races it. Poll for the outcome instead.
 */
async function expectRemoved(renderer: ReturnType<typeof createTestRoot>["renderer"]) {
  await vi.waitFor(() => {
    renderer.flush()
    expect(renderer.getAllText()).toEqual([])
  })
}

describeNative("AnimatePresence", () => {
  it("keeps an exiting motion.div until the exit target finishes", () => {
    const { render, renderer } = createTestRoot()

    function App({ show }: { show: boolean }) {
      return (
        <AnimatePresence>
          {show ? (
            <motion.div
              key="card"
              initial={false}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.2, ease: "linear" }}
            >
              <text>Leaving</text>
            </motion.div>
          ) : null}
        </AnimatePresence>
      )
    }

    render(<App show />)
    expect(renderer.getAllText()).toEqual(["Leaving"])

    renderer.clockPause()
    render(<App show={false} />)
    expect(renderer.getAllText()).toEqual(["Leaving"])

    renderer.clockFastForward(100)
    renderer.dispatchNativeEvents()
    expect(renderer.getAllText()).toEqual(["Leaving"])

    renderer.clockFastForward(100)
    renderer.dispatchNativeEvents()
    expect(renderer.getAllText()).toEqual([])
  })

  it("keeps an exiting motion.div until its spring settles", () => {
    const { render, renderer } = createTestRoot()

    function App({ show }: { show: boolean }) {
      return (
        <AnimatePresence>
          {show ? (
            <motion.div
              key="card"
              initial={false}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ type: "spring", duration: 0.2, bounce: 0 }}
            >
              <text>Leaving</text>
            </motion.div>
          ) : null}
        </AnimatePresence>
      )
    }

    render(<App show />)
    renderer.clockPause()
    render(<App show={false} />)

    renderer.clockFastForward(50)
    renderer.dispatchNativeEvents()
    expect(renderer.getAllText()).toEqual(["Leaving"])

    renderer.clockFastForward(2000)
    renderer.dispatchNativeEvents()
    expect(renderer.getAllText()).toEqual([])
  })

  it("still removes an exiting child whose spring is invalid", () => {
    const { render, renderer } = createTestRoot()

    function App({ show }: { show: boolean }) {
      return (
        <AnimatePresence>
          {show ? (
            <motion.div
              key="card"
              initial={false}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ type: "spring", stiffness: -1 }}
            >
              <text>Leaving</text>
            </motion.div>
          ) : null}
        </AnimatePresence>
      )
    }

    render(<App show />)
    renderer.clockPause()
    render(<App show={false} />)
    renderer.clockFastForward(16)
    renderer.dispatchNativeEvents()
    expect(renderer.getAllText()).toEqual([])
  })

  it("starts a new motion generation when only the spring changes", () => {
    const { render, renderer } = createTestRoot()

    function App({ stiffness }: { stiffness: number }) {
      return (
        <motion.div
          initial={false}
          animate={{ opacity: 1 }}
          transition={{ type: "spring", stiffness }}
        >
          <text>Card</text>
        </motion.div>
      )
    }

    render(<App stiffness={100} />)
    const first = renderer.findByType("div")[0]?.customProps?.motion?.generation
    render(<App stiffness={300} />)
    const second = renderer.findByType("div")[0]?.customProps?.motion?.generation
    expect(second).toBeGreaterThan(first as number)
  })

  it("carries a spring's speed into the next target", () => {
    const { render, renderer } = createTestRoot()

    function Bar({ open }: { open: boolean }) {
      return (
        <motion.div
          testId="bar"
          initial={false}
          animate={{ width: open ? 300 : 0 }}
          transition={{ type: "spring", stiffness: 170, damping: 12 }}
          style={{ height: 20 }}
        >
          <text>bar</text>
        </motion.div>
      )
    }

    const widthAt = () => {
      renderer.dispatchNativeEvents()
      const bar = renderer.findByTestId("bar")
      return renderer.getElementBounds(bar.id).width
    }

    render(<Bar open={false} />)
    renderer.clockPause()
    render(<Bar open />)
    renderer.clockFastForward(60)
    const atRetarget = widthAt()
    expect(atRetarget).toBeGreaterThan(0)

    render(<Bar open={false} />)
    renderer.clockFastForward(16)
    const afterRetarget = widthAt()
    // Aimed back at 0, but still moving outward: a restart from rest would
    // have started shrinking on the first frame.
    expect(afterRetarget).toBeGreaterThan(atRetarget)
  })

  it("skips enter when AnimatePresence initial is false", () => {
    const { render, renderer } = createTestRoot()

    renderer.clockPause()
    render(
      <AnimatePresence initial={false}>
        <motion.div
          key="card"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.2, ease: "linear" }}
        >
          <text>Ready</text>
        </motion.div>
      </AnimatePresence>
    )

    expect(renderer.findByType("div")[0]?.customProps?.motion).toMatchObject({
      initial: false,
      animate: { opacity: 1 },
    })
  })

  it("removes a child without an exit target", async () => {
    const { render, renderer } = createTestRoot()

    function App({ show }: { show: boolean }) {
      return (
        <AnimatePresence>
          {show ? (
            <motion.div key="card" initial={false} animate={{ opacity: 1 }}>
              <text>Immediate</text>
            </motion.div>
          ) : null}
        </AnimatePresence>
      )
    }

    render(<App show />)
    render(<App show={false} />)
    await expectRemoved(renderer)
  })

  it("exits with the latest props for a stable key", () => {
    const { render, renderer } = createTestRoot()

    function App({ show, version }: { show: boolean; version: number }) {
      return (
        <AnimatePresence>
          {show ? (
            <motion.div
              key="card"
              initial={false}
              animate={{ opacity: 1 }}
              exit={{ opacity: version / 10 }}
              transition={{ duration: 0.2, ease: "linear" }}
            >
              <text>{`Version ${version}`}</text>
            </motion.div>
          ) : null}
        </AnimatePresence>
      )
    }

    render(<App show version={1} />)
    render(<App show version={2} />)
    render(<App show={false} version={2} />)

    expect(renderer.getAllText()).toEqual(["Version 2"])
    expect(renderer.findByType("div")[0]?.customProps?.motion).toMatchObject({
      animate: { opacity: 0.2 },
    })
  })

  it("removes a child when the exit target already matches", () => {
    const { render, renderer } = createTestRoot()

    function App({ show }: { show: boolean }) {
      return (
        <AnimatePresence>
          {show ? (
            <motion.div
              key="card"
              initial={false}
              animate={{ opacity: 1 }}
              exit={{ opacity: 1 }}
            >
              <text>No movement</text>
            </motion.div>
          ) : null}
        </AnimatePresence>
      )
    }

    render(<App show />)
    render(<App show={false} />)
    renderer.dispatchNativeEvents()

    expect(renderer.getAllText()).toEqual([])
  })

  it("does not retain a child after an invalid exit target", () => {
    const { render, renderer } = createTestRoot()

    function App({ show }: { show: boolean }) {
      return (
        <AnimatePresence>
          {show ? (
            <motion.div
              key="card"
              initial={false}
              animate={{ opacity: 1 }}
              exit={{ opacity: 2 }}
            >
              <text>Invalid target</text>
            </motion.div>
          ) : null}
        </AnimatePresence>
      )
    }

    render(<App show />)
    render(<App show={false} />)
    renderer.dispatchNativeEvents()

    expect(renderer.getAllText()).toEqual([])
  })

  it("ignores a queued completion from the previous target", () => {
    const { render, renderer } = createTestRoot()
    const completions: string[] = []

    function App({ show }: { show: boolean }) {
      return (
        <AnimatePresence>
          {show ? (
            <motion.div
              key="card"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.2, ease: "linear" }}
              onMotionComplete={() => completions.push("complete")}
            >
              <text>Current target</text>
            </motion.div>
          ) : null}
        </AnimatePresence>
      )
    }

    renderer.clockPause()
    render(<App show />)
    renderer.clockFastForward(200)
    render(<App show={false} />)
    renderer.dispatchNativeEvents()

    expect(renderer.getAllText()).toEqual(["Current target"])
    expect(completions).toEqual([])

    renderer.clockFastForward(200)
    renderer.dispatchNativeEvents()

    expect(renderer.getAllText()).toEqual([])
    expect(completions).toEqual(["complete"])
  })

  it("finishes an exit in an offscreen virtual-list row", () => {
    const { render, renderer } = createTestRoot({ height: 160 })

    function App({ show }: { show: boolean }) {
      return (
        <virtual-list
          estimatedItemHeight={40}
          overdraw={0}
          style={{ width: 400, height: 160 }}
        >
          {Array.from({ length: 100 }, (_, index) => (
            <div key={index} style={{ height: 40, flexShrink: 0 }}>
              {index === 99 ? (
                <AnimatePresence>
                  {show ? (
                    <motion.div
                      key="card"
                      initial={false}
                      animate={{ opacity: 1 }}
                      exit={{ opacity: 0 }}
                      transition={{ duration: 0.2, ease: "linear" }}
                    >
                      <text>Offscreen exit</text>
                    </motion.div>
                  ) : null}
                </AnimatePresence>
              ) : (
                <text>{`Row ${index}`}</text>
              )}
            </div>
          ))}
        </virtual-list>
      )
    }

    render(<App show />)
    expect(renderer.getAllText()).toContain("Offscreen exit")
    expect(renderer.getPaintedText()).not.toContain("Offscreen exit")

    renderer.clockPause()
    render(<App show={false} />)
    renderer.clockFastForward(200)
    renderer.dispatchNativeEvents()

    expect(renderer.getAllText()).not.toContain("Offscreen exit")
  })

  it("keeps keyed siblings while one child exits", () => {
    const { render, renderer } = createTestRoot()

    function App({ items }: { items: string[] }) {
      return (
        <div>
          <AnimatePresence>
            {items.map((item) => (
              <motion.div
                key={item}
                initial={false}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.2, ease: "linear" }}
              >
                <text>{item}</text>
              </motion.div>
            ))}
          </AnimatePresence>
        </div>
      )
    }

    render(<App items={["a", "b"]} />)
    renderer.clockPause()
    render(<App items={["b"]} />)
    expect(renderer.getAllText()).toEqual(["a", "b"])

    renderer.clockFastForward(200)
    renderer.dispatchNativeEvents()
    expect(renderer.getAllText()).toEqual(["b"])
  })

  it("completes when exit is committed before the first native frame", () => {
    const { root, renderer } = createTestRoot()

    function App({ show }: { show: boolean }) {
      return (
        <AnimatePresence>
          {show ? (
            <motion.div
              key="card"
              initial={false}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.2, ease: "linear" }}
            >
              <text>Coalesced frame</text>
            </motion.div>
          ) : null}
        </AnimatePresence>
      )
    }

    renderer.clockPause()
    flushSync(() => root.render(<App show />))
    flushSync(() => root.render(<App show={false} />))
    expect(renderer.findByType("div")[0]?.customProps?.motion).toMatchObject({
      isExit: true,
    })
    renderer.flush()
    renderer.clockFastForward(200)
    renderer.dispatchNativeEvents()

    expect(renderer.getAllText()).toEqual([])
  })

  it("keeps completed participants across an unrelated exit render", () => {
    const { render, renderer } = createTestRoot()
    const removers = new Map<string, () => void>()

    function Participant({ name }: { name: string }) {
      const [isPresent, safeToRemove] = usePresence()
      useLayoutEffect(() => {
        if (!isPresent && safeToRemove) removers.set(name, safeToRemove)
      }, [isPresent, name, safeToRemove])
      return <text>{name}</text>
    }

    function App({ show, revision }: { show: boolean; revision: number }) {
      return (
        <div>
          <AnimatePresence>
            {show ? (
              <div key="group" testId={`revision-${revision}`}>
                <Participant name="a" />
                <Participant name="b" />
              </div>
            ) : null}
          </AnimatePresence>
        </div>
      )
    }

    render(<App show revision={0} />)
    render(<App show={false} revision={0} />)
    removers.get("a")?.()
    render(<App show={false} revision={1} />)
    flushSync(() => removers.get("b")?.())
    renderer.flush()

    expect(renderer.getAllText()).toEqual([])
  })

  it("keeps a participant completed from the first exit layout", () => {
    const { render, renderer } = createTestRoot()
    let removeLater: (() => void) | undefined

    function Participant({ immediate }: { immediate?: boolean }) {
      const [isPresent, safeToRemove] = usePresence()
      useLayoutEffect(() => {
        if (isPresent || !safeToRemove) return
        if (immediate) safeToRemove()
        else removeLater = safeToRemove
      }, [immediate, isPresent, safeToRemove])
      return <text>{immediate ? "first" : "second"}</text>
    }

    function App({ show }: { show: boolean }) {
      return (
        <div>
          <AnimatePresence>
            {show ? (
              <div key="group">
                <Participant immediate />
                <Participant />
              </div>
            ) : null}
          </AnimatePresence>
        </div>
      )
    }

    render(<App show />)
    render(<App show={false} />)
    flushSync(() => removeLater?.())
    renderer.flush()

    expect(renderer.getAllText()).toEqual([])
  })

  it("does not complete a cancelled exit when a participant is replaced", async () => {
    const { render, renderer } = createTestRoot()
    let exits = 0

    function Participant() {
      usePresence()
      return <text>participant</text>
    }

    function App({ show, replace }: { show: boolean; replace: boolean }) {
      return (
        <div>
          <AnimatePresence onExitComplete={() => exits++}>
            {show ? (
              <div key="group">
                {replace ? <text>replacement</text> : <Participant />}
              </div>
            ) : null}
          </AnimatePresence>
        </div>
      )
    }

    render(<App show replace={false} />)
    render(<App show={false} replace={false} />)
    render(<App show replace />)
    await Promise.resolve()
    renderer.flush()

    expect(exits).toBe(0)
    expect(renderer.getAllText()).toEqual(["replacement"])
  })

  it("ignores safeToRemove after its participant unregisters", async () => {
    const { render, renderer } = createTestRoot()
    let staleRemove: (() => void) | undefined

    function Participant() {
      const [isPresent, safeToRemove] = usePresence()
      useLayoutEffect(() => {
        if (!isPresent && safeToRemove) staleRemove = safeToRemove
      }, [isPresent, safeToRemove])
      return <text>participant</text>
    }

    function App({ show, replace }: { show: boolean; replace: boolean }) {
      return (
        <div>
          <AnimatePresence>
            {show ? (
              <div key="group">
                {replace ? <text>replacement</text> : <Participant />}
              </div>
            ) : null}
          </AnimatePresence>
        </div>
      )
    }

    render(<App show replace={false} />)
    render(<App show={false} replace={false} />)
    render(<App show replace />)
    staleRemove?.()
    render(<App show={false} replace />)
    await expectRemoved(renderer)
  })
})

describe("AnimatePresence without native", () => {
  it("exports AnimatePresence next to motion", () => {
    expect(typeof AnimatePresence).toBe("function")
  })
})
