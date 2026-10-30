/// The Solid half of `motion-target-transition.test.tsx` of the React package: the `transition` a target of `animate`, `exit`,
/// `initial` or `whileHover` carries replaces the component's as a whole for that animation, the way back out of a gesture plays
/// with the one `animate` carries (else the component's), an exit plays with the one `exit` or the exit variant carries (else the
/// component's, never `animate`'s), and `initial`'s is ignored.

import { createSignal, Show, type JSX } from "solid-js"
import { describe, expect, it } from "bun:test"
import { AnimatePresence, motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const box = { width: 100, height: 40 } as const
const linear = (seconds: number) => ({ duration: seconds, ease: "linear" }) as const

function setup(body: () => JSX.Element) {
  const app = createTestRoot({ width: 400, height: 200 })
  const renderer = app.renderer
  renderer.clockPause()
  app.render(() => <div style={{ width: 400, height: 200 }}>{body()}</div>)
  const page = {
    app,
    renderer,
    width: (testId: string) => {
      renderer.flush()
      return renderer.getElementBounds(renderer.findByTestId(testId)!.id)!.width
    },
    advance: (ms: number) => {
      renderer.clockFastForward(ms)
      renderer.flush()
      renderer.dispatchNativeEvents()
    },
    hover: (testId: string) => {
      renderer.flush()
      const bounds = renderer.getElementBounds(renderer.findByTestId(testId)!.id)!
      renderer.nativeSimulateMouseMove(bounds.x + 4, bounds.y + 4)
      page.advance(0)
    },
    leave: () => {
      renderer.nativeSimulateMouseMove(390, 190)
      page.advance(0)
    },
  }
  return page
}

describe.skipIf(!hasNativeTestRenderer)("Solid: the transition a target carries", () => {
  it("is what the way back out of a hover plays when animate carries it, not the hover's", () => {
    const page = setup(() => (
      <motion.div
        testId="d"
        style={box}
        animate={{ width: 100, transition: linear(1) }}
        whileHover={{ width: 200, transition: linear(0.2) }}
        transition={linear(0.1)}
      />
    ))
    page.hover("d")
    page.advance(200)
    expect(page.width("d")).toBeCloseTo(200, 0)
    page.leave()
    page.advance(500)
    expect(page.width("d")).toBeCloseTo(150, 0)
    page.app.unmount()
  })

  it("is the component's on the way back out of a hover when animate carries none", () => {
    const page = setup(() => (
      <motion.div
        testId="d"
        style={box}
        animate={{ width: 100 }}
        whileHover={{ width: 200, transition: linear(0.1) }}
        transition={linear(1)}
      />
    ))
    page.hover("d")
    page.advance(100)
    expect(page.width("d")).toBeCloseTo(200, 0)
    page.leave()
    page.advance(500)
    expect(page.width("d")).toBeCloseTo(150, 0)
    page.app.unmount()
  })

  it("is ignored in initial, which nothing animates to", () => {
    const page = setup(() => (
      <motion.div
        testId="d"
        style={box}
        initial={{ width: 50, transition: linear(10) }}
        animate={{ width: 150 }}
        transition={linear(1)}
      />
    ))
    expect(page.width("d")).toBeCloseTo(50, 0)
    page.advance(500)
    expect(page.width("d")).toBeCloseTo(100, 0)
    page.app.unmount()
  })

  it("starts the animation again when it changes, the style staying as it is", () => {
    const [seconds, setSeconds] = createSignal(1)
    const page = setup(() => (
      <motion.div testId="d" style={box} animate={{ width: 200, transition: linear(seconds()) }} />
    ))
    page.advance(500)
    expect(page.width("d")).toBeCloseTo(150, 0)
    page.app.flushSync(() => setSeconds(10))
    page.advance(1000)
    expect(page.width("d")).toBeCloseTo(155, 0)
    page.app.unmount()
  })

  describe.each(["sync", "wait", "popLayout"] as const)('in AnimatePresence mode="%s"', (mode) => {
    it("is the exit's own when a child leaves, else the component's, never animate's", () => {
      const leave = (exit: object) => {
        const [show, setShow] = createSignal(true)
        const page = setup(() => (
          <AnimatePresence mode={mode}>
            <Show when={show()}>
              <motion.div
                testId="d"
                style={box}
                initial={false}
                animate={{ width: 100, transition: linear(4) }}
                exit={exit}
                transition={linear(1)}
              />
            </Show>
          </AnimatePresence>
        ))
        page.app.flushSync(() => setShow(false))
        page.advance(500)
        return page
      }
      const own = leave({ width: 0, transition: linear(2) })
      expect(own.width("d")).toBeCloseTo(75, 0)
      own.app.unmount()
      const components = leave({ width: 0 })
      expect(components.width("d")).toBeCloseTo(50, 0)
      components.app.unmount()
    })

    it("is the exit variant's own when a child leaves, else the component's", () => {
      const variants = {
        shown: { width: 100, transition: linear(4) },
        gone: { width: 0, transition: linear(2) },
        goneQuickly: { width: 0 },
      }
      const leave = (exit: string) => {
        const [show, setShow] = createSignal(true)
        const page = setup(() => (
          <AnimatePresence mode={mode}>
            <Show when={show()}>
              <motion.div testId="d" style={box} initial={false} animate="shown" exit={exit} variants={variants} transition={linear(1)} />
            </Show>
          </AnimatePresence>
        ))
        page.app.flushSync(() => setShow(false))
        page.advance(500)
        return page
      }
      const own = leave("gone")
      expect(own.width("d")).toBeCloseTo(75, 0)
      own.app.unmount()
      const components = leave("goneQuickly")
      expect(components.width("d")).toBeCloseTo(50, 0)
      components.app.unmount()
    })
  })
})
