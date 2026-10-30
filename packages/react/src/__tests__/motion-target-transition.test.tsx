/// The `transition` a target of `animate`, `exit`, `initial` or `whileHover` carries: as in Motion it replaces the component's
/// `transition` as a whole for that animation, the way back out of a gesture plays with the one `animate` carries (else the
/// component's), an exit plays with the one `exit` carries (else the component's, never `animate`'s), and `initial` is never
/// animated, so its `transition` is ignored. `motion-default.test.tsx` replays the oracle through every route; these are the
/// cases Motion's `animateTarget` cannot say (which transition a later state of the component plays).

import { describe, expect, it } from "vitest"
import { AnimatePresence, motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const box = { width: 100, height: 40 } as const
const linear = (seconds: number) => ({ duration: seconds, ease: "linear" }) as const

function setup(node: React.ReactNode) {
  const testRoot = createTestRoot({ width: 400, height: 200 })
  const renderer = testRoot.renderer
  renderer.clockPause()
  const frame = (children: React.ReactNode) => <div style={{ width: 400, height: 200 }}>{children}</div>
  testRoot.render(frame(node))
  const page = {
    testRoot,
    renderer,
    rerender: (children: React.ReactNode) => testRoot.render(frame(children)),
    width: (testId: string) => {
      renderer.flush()
      return renderer.getElementBounds(renderer.findByTestId(testId).id)!.width
    },
    advance: (ms: number) => {
      renderer.clockFastForward(ms)
      renderer.flush()
      renderer.dispatchNativeEvents()
    },
    hover: (testId: string) => {
      renderer.flush()
      const bounds = renderer.getElementBounds(renderer.findByTestId(testId).id)!
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

describeNative("the transition a target carries", () => {
  it("is what the way back out of a hover plays when animate carries it, not the hover's", () => {
    const page = setup(
      <motion.div
        testId="d"
        style={box}
        animate={{ width: 100, transition: linear(1) }}
        whileHover={{ width: 200, transition: linear(0.2) }}
        transition={linear(0.1)}
      />
    )
    page.hover("d")
    page.advance(200)
    expect(page.width("d")).toBeCloseTo(200, 0)
    page.leave()
    page.advance(500)
    expect(page.width("d")).toBeCloseTo(150, 0)
    page.testRoot.unmount()
  })

  it("is the component's on the way back out of a hover when animate carries none", () => {
    const page = setup(
      <motion.div
        testId="d"
        style={box}
        animate={{ width: 100 }}
        whileHover={{ width: 200, transition: linear(0.1) }}
        transition={linear(1)}
      />
    )
    page.hover("d")
    page.advance(100)
    expect(page.width("d")).toBeCloseTo(200, 0)
    page.leave()
    page.advance(500)
    expect(page.width("d")).toBeCloseTo(150, 0)
    page.testRoot.unmount()
  })

  it("is ignored in initial, which nothing animates to", () => {
    const page = setup(
      <motion.div
        testId="d"
        style={box}
        initial={{ width: 50, transition: linear(10) }}
        animate={{ width: 150 }}
        transition={linear(1)}
      />
    )
    expect(page.width("d")).toBeCloseTo(50, 0)
    page.advance(500)
    expect(page.width("d")).toBeCloseTo(100, 0)
    page.testRoot.unmount()
  })

  it("starts the animation again when it changes, the style staying as it is", () => {
    const App = (props: { seconds: number }) => (
      <motion.div testId="d" style={box} animate={{ width: 200, transition: linear(props.seconds) }} />
    )
    const page = setup(<App seconds={1} />)
    page.advance(500)
    expect(page.width("d")).toBeCloseTo(150, 0)
    page.rerender(<App seconds={10} />)
    page.advance(1000)
    expect(page.width("d")).toBeCloseTo(155, 0)
    page.testRoot.unmount()
  })

  describe.each(["sync", "wait", "popLayout"] as const)('in AnimatePresence mode="%s"', (mode) => {
    it("is the exit's own when a child leaves, else the component's, never animate's", () => {
      const Child = (props: { exit: object }) => (
        <motion.div
          testId="d"
          style={box}
          initial={false}
          animate={{ width: 100, transition: linear(4) }}
          exit={props.exit}
          transition={linear(1)}
        />
      )
      const leave = (exit: object) => {
        const App = (props: { show: boolean }) => (
          <AnimatePresence mode={mode}>{props.show ? <Child key="d" exit={exit} /> : null}</AnimatePresence>
        )
        const page = setup(<App show />)
        page.rerender(<App show={false} />)
        page.advance(500)
        return page
      }
      const own = leave({ width: 0, transition: linear(2) })
      expect(own.width("d")).toBeCloseTo(75, 0)
      own.testRoot.unmount()
      const components = leave({ width: 0 })
      expect(components.width("d")).toBeCloseTo(50, 0)
      components.testRoot.unmount()
    })

    it("is the exit variant's own when a child leaves, else the component's", () => {
      const variants = {
        shown: { width: 100, transition: linear(4) },
        gone: { width: 0, transition: linear(2) },
        goneQuickly: { width: 0 },
      }
      const leave = (exit: string) => {
        const App = (props: { show: boolean }) => (
          <AnimatePresence mode={mode}>
            {props.show ? (
              <motion.div key="d" testId="d" style={box} initial={false} animate="shown" exit={exit} variants={variants} transition={linear(1)} />
            ) : null}
          </AnimatePresence>
        )
        const page = setup(<App show />)
        page.rerender(<App show={false} />)
        page.advance(500)
        return page
      }
      const own = leave("gone")
      expect(own.width("d")).toBeCloseTo(75, 0)
      own.testRoot.unmount()
      const components = leave("goneQuickly")
      expect(components.width("d")).toBeCloseTo(50, 0)
      components.testRoot.unmount()
    })
  })

  it("leaves the layout glide to the component's transition", () => {
    const column = { display: "flex", flexDirection: "column" } as const
    const app = (order: string[]) => (
      <div style={column}>
        {order.map((name) => (
          <motion.div
            key={name}
            testId={name}
            layout
            initial={{ opacity: 0 }}
            animate={{ opacity: 1, transition: { duration: 10 } }}
            transition={{ layout: linear(1) }}
            style={{ height: 20, width: 50 }}
          />
        ))}
      </div>
    )
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(app(["a", "b"]))
    render(app(["b", "a"]))
    renderer.clockFastForward(500)
    renderer.flush()
    renderer.dispatchNativeEvents()
    expect(renderer.getElementBounds(renderer.findByTestId("b").id)!.y).toBeCloseTo(10, 1)
  })
})
