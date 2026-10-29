/// The Solid half of `motion-gestures.test.tsx` of the React package: `whileHover`, `whileTap`, `whileFocus` and
/// `whileInView` on a `motion.div`, their priority, the variant names a controlling component hands down, and the
/// gesture callbacks. The midpoints are headless Chrome's (`packages/react/src/__tests__/fixtures/gesture-oracle.json`).

import fs from "fs"
import os from "os"
import path from "path"
import { fileURLToPath } from "url"
import { createSignal, Show, type JSX } from "solid-js"
import { describe, expect, it } from "bun:test"
import { AnimatePresence, motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels } from "./pixels.js"

const ORACLE: { widthAtHalf: string; srgbRedBlueAtHalf: [number, number, number] } = JSON.parse(
  fs.readFileSync(
    path.resolve(
      path.dirname(fileURLToPath(import.meta.url)),
      "../../../react/src/__tests__/fixtures/gesture-oracle.json"
    ),
    "utf8"
  )
)

const shots = fs.mkdtempSync(path.join(os.tmpdir(), "gpuix-solid-motion-gestures-"))
const LINEAR = { duration: 1, ease: "linear" } as const
const INSTANT = { duration: 0 } as const
const RED = "#ff0000"
const BLUE = "#0000ff"
const GREEN = "#00ff00"
const HALF_WIDTH = Number.parseFloat(ORACLE.widthAtHalf)
const box = { width: 100, height: 40 } as const

function setup(body: () => JSX.Element) {
  const app = createTestRoot({ width: 400, height: 200 })
  const renderer = app.renderer
  renderer.clockPause()
  app.render(() => <div style={{ width: 400, height: 200, backgroundColor: "#ffffff" }}>{body()}</div>)
  let taken = 0
  const bounds = (testId: string) => {
    renderer.flush()
    return renderer.getElementBounds(renderer.findByTestId(testId)!.id)!
  }
  const page = {
    app,
    renderer,
    bounds,
    width: (testId: string) => bounds(testId).width,
    pixel: (testId: string, dx: number, dy: number) => {
      const file = path.join(shots, `shot-${taken++}.png`)
      renderer.captureScreenshot(file)
      const at = bounds(testId)
      return Array.from(readLogicalPixels(file, renderer).at(Math.floor(at.x + dx), Math.floor(at.y + dy)).slice(0, 3))
    },
    advance: (ms: number) => {
      renderer.clockFastForward(ms)
      renderer.flush()
      renderer.dispatchNativeEvents()
    },
    inside: (testId: string) => {
      const at = bounds(testId)
      return [at.x + 4, at.y + 4] as const
    },
    hover: (testId: string) => renderer.nativeSimulateMouseMove(...page.inside(testId)),
    leave: () => renderer.nativeSimulateMouseMove(390, 190),
    press: (testId: string) => renderer.nativeSimulateMouseDown(...page.inside(testId)),
    release: (testId: string) => renderer.nativeSimulateMouseUp(...page.inside(testId)),
  }
  return page
}

function expectNear(actual: number[], expected: number[], tolerance = 3) {
  for (let i = 0; i < expected.length; i++) {
    expect(Math.abs(actual[i]! - expected[i]!), `${actual} vs ${expected}`).toBeLessThanOrEqual(tolerance)
  }
}

const settle = (renderer: ReturnType<typeof setup>["renderer"]) => {
  for (let step = 0; step < 8; step++) {
    renderer.clockFastForward(20)
    renderer.flush()
    renderer.dispatchNativeEvents()
  }
}

describe.skipIf(!hasNativeTestRenderer)("Solid whileHover on a motion.div", () => {
  it("moves a width and a colour to the target while the pointer is over it, and back", () => {
    const page = setup(() => (
      <motion.div
        testId="d"
        style={{ ...box, backgroundColor: RED }}
        whileHover={{ width: 200, backgroundColor: BLUE }}
        transition={LINEAR}
      />
    ))
    expect(page.width("d")).toBe(100)
    page.hover("d")
    page.advance(1)
    page.advance(499)
    expect(page.width("d")).toBeCloseTo(HALF_WIDTH, 0)
    expectNear(page.pixel("d", 20, 20), ORACLE.srgbRedBlueAtHalf)
    page.advance(500)
    expect(page.width("d")).toBeCloseTo(200, 0)
    page.leave()
    page.advance(1)
    page.advance(1000)
    expect(page.width("d")).toBeCloseTo(100, 0)
    expectNear(page.pixel("d", 20, 20), [255, 0, 0])
    page.app.unmount()
  })

  it("returns a property to the animate value when animate sets it", () => {
    const page = setup(() => (
      <motion.div testId="d" style={box} animate={{ width: 140 }} whileHover={{ width: 200 }} transition={INSTANT} />
    ))
    page.advance(10)
    page.hover("d")
    page.advance(10)
    expect(page.width("d")).toBeCloseTo(200, 0)
    page.leave()
    page.advance(10)
    expect(page.width("d")).toBeCloseTo(140, 0)
    page.app.unmount()
  })

  it("lets go of the target on a node that is leaving AnimatePresence", () => {
    const [shown, setShown] = createSignal(true)
    const page = setup(() => (
      <AnimatePresence>
        <Show when={shown()}>
          <motion.div
            testId="d"
            style={box}
            whileHover={{ width: 200 }}
            exit={{ opacity: 0.5 }}
            transition={{ default: { duration: 10, ease: "linear" }, width: INSTANT }}
          />
        </Show>
      </AnimatePresence>
    ))
    page.hover("d")
    page.advance(10)
    expect(page.width("d")).toBeCloseTo(200, 0)
    page.app.flushSync(() => setShown(false))
    page.advance(10)
    expect(page.width("d")).toBeCloseTo(100, 0)
    page.app.unmount()
  })
})

describe.skipIf(!hasNativeTestRenderer)("Solid whileTap and whileFocus on a motion.div", () => {
  it("whileTap is on from the press to the release and wins over whileHover", () => {
    const page = setup(() => (
      <motion.div
        testId="d"
        style={box}
        whileHover={{ width: 150, height: 60 }}
        whileTap={{ width: 200 }}
        transition={INSTANT}
      />
    ))
    page.hover("d")
    page.advance(10)
    expect(page.bounds("d")).toMatchObject({ width: 150, height: 60 })
    page.press("d")
    page.advance(10)
    expect(page.bounds("d")).toMatchObject({ width: 200, height: 60 })
    page.release("d")
    page.advance(10)
    expect(page.bounds("d")).toMatchObject({ width: 150, height: 60 })
    page.app.unmount()
  })

  it("whileFocus is on for :focus-visible focus, under whileHover and whileTap", () => {
    const page = setup(() => (
      <>
        <motion.div
          testId="d"
          tabIndex={0}
          style={box}
          whileHover={{ width: 150 }}
          whileTap={{ width: 160 }}
          whileFocus={{ width: 200, height: 70 }}
          transition={INSTANT}
        />
        <div tabIndex={0} style={box} />
      </>
    ))
    page.renderer.simulateKeystrokes("Tab")
    page.advance(10)
    expect(page.bounds("d")).toMatchObject({ width: 200, height: 70 })
    page.hover("d")
    page.advance(10)
    expect(page.bounds("d")).toMatchObject({ width: 150, height: 70 })
    page.press("d")
    page.advance(10)
    expect(page.bounds("d")).toMatchObject({ width: 160, height: 70 })
    page.release("d")
    page.leave()
    page.renderer.simulateKeystrokes("Tab")
    page.advance(10)
    expect(page.bounds("d")).toMatchObject({ width: 100, height: 40 })
    page.app.unmount()
  })

  it("whileFocus is not on for a focus a mouse press gave it", () => {
    const page = setup(() => (
      <motion.div testId="d" tabIndex={0} style={box} whileFocus={{ width: 200 }} transition={INSTANT} />
    ))
    page.press("d")
    page.release("d")
    page.advance(10)
    expect(page.width("d")).toBeCloseTo(100, 0)
    page.app.unmount()
  })
})

describe.skipIf(!hasNativeTestRenderer)("Solid whileInView on a motion.div", () => {
  function observe(extra: Record<string, unknown> = {}) {
    const [spacer, setSpacer] = createSignal(600)
    const app = createTestRoot({ width: 400, height: 200 })
    const renderer = app.renderer
    renderer.clockPause()
    app.render(() => (
      <div style={{ width: 400, height: 200 }}>
        <div style={{ height: spacer() }} />
        <motion.div
          testId="d"
          style={{ width: 100, height: 100 }}
          whileInView={{ width: 200 }}
          transition={INSTANT}
          {...extra}
        />
      </div>
    ))
    const width = () => {
      renderer.flush()
      return renderer.getElementBounds(renderer.findByTestId("d")!.id)!.width
    }
    return {
      app,
      width,
      settle: () => settle(renderer),
      show: (at: number) => app.flushSync(() => setSpacer(at)),
    }
  }

  it("is on while any of it shows, and off when it does not", () => {
    const page = observe()
    page.settle()
    expect(page.width()).toBe(100)
    page.show(0)
    page.settle()
    expect(page.width()).toBeCloseTo(200, 0)
    page.show(600)
    page.settle()
    expect(page.width()).toBeCloseTo(100, 0)
    page.app.unmount()
  })

  it("stays on with viewport.once", () => {
    const page = observe({ viewport: { once: true } })
    page.show(0)
    page.settle()
    page.show(600)
    page.settle()
    expect(page.width()).toBeCloseTo(200, 0)
    page.app.unmount()
  })

  it("waits for viewport.amount of it to show", () => {
    const page = observe({ viewport: { amount: 0.5 } })
    page.show(160)
    page.settle()
    expect(page.width()).toBe(100)
    page.show(140)
    page.settle()
    expect(page.width()).toBeCloseTo(200, 0)
    page.app.unmount()
  })
})

describe.skipIf(!hasNativeTestRenderer)("Solid variants by name", () => {
  const childVariants = { rest: { height: 40 }, hover: { height: 80 }, press: { height: 60 } }

  it("takes a gesture target by variant name", () => {
    const page = setup(() => (
      <motion.div
        testId="d"
        style={{ height: 40 }}
        variants={{ rest: { width: 100 }, hover: { width: 200 }, press: { width: 160 } }}
        initial="rest"
        animate="rest"
        whileHover="hover"
        whileTap="press"
        transition={INSTANT}
      />
    ))
    expect(page.width("d")).toBeCloseTo(100, 0)
    page.hover("d")
    page.advance(10)
    expect(page.width("d")).toBeCloseTo(200, 0)
    page.press("d")
    page.advance(10)
    expect(page.width("d")).toBeCloseTo(160, 0)
    page.release("d")
    page.leave()
    page.advance(10)
    expect(page.width("d")).toBeCloseTo(100, 0)
    page.app.unmount()
  })

  it("hands the names of whileHover and whileTap down to the components with variants in it", () => {
    const page = setup(() => (
      <motion.div
        testId="parent"
        style={{ width: 200, height: 100 }}
        initial="rest"
        animate="rest"
        whileHover="hover"
        whileTap="press"
        transition={INSTANT}
      >
        <div style={{ height: 10 }} />
        <motion.div testId="child" style={{ width: 20 }} variants={childVariants} transition={INSTANT}>
          <motion.div testId="grandchild" style={{ width: 10 }} variants={{ hover: { width: 70 } }} transition={INSTANT} />
        </motion.div>
      </motion.div>
    ))
    expect(page.bounds("child").height).toBeCloseTo(40, 0)
    page.renderer.nativeSimulateMouseMove(...page.inside("parent"))
    page.advance(10)
    expect(page.bounds("child").height).toBeCloseTo(80, 0)
    expect(page.width("grandchild")).toBeCloseTo(70, 0)
    page.renderer.nativeSimulateMouseDown(...page.inside("parent"))
    page.advance(10)
    expect(page.bounds("child").height).toBeCloseTo(60, 0)
    page.renderer.nativeSimulateMouseUp(...page.inside("parent"))
    page.advance(10)
    expect(page.bounds("child").height).toBeCloseTo(80, 0)
    page.leave()
    page.advance(10)
    expect(page.bounds("child").height).toBeCloseTo(40, 0)
    expect(page.width("grandchild")).toBeCloseTo(10, 0)
    page.app.unmount()
  })

  it("hands the name of animate down, and follows it as it changes", () => {
    const [open, setOpen] = createSignal(false)
    const page = setup(() => (
      <motion.div
        testId="parent"
        style={{ width: 200, height: 100 }}
        initial="closed"
        animate={open() ? "open" : "closed"}
        transition={INSTANT}
      >
        <motion.div
          testId="child"
          style={{ width: 20 }}
          variants={{ closed: { height: 10 }, open: { height: 70 } }}
          transition={INSTANT}
        />
      </motion.div>
    ))
    expect(page.bounds("child").height).toBeCloseTo(10, 0)
    page.app.flushSync(() => setOpen(true))
    page.advance(10)
    expect(page.bounds("child").height).toBeCloseTo(70, 0)
    page.app.flushSync(() => setOpen(false))
    page.advance(10)
    expect(page.bounds("child").height).toBeCloseTo(10, 0)
    page.app.unmount()
  })

  it("lets a component that names variants itself break the chain, and inherit={false} too", () => {
    const page = setup(() => (
      <motion.div
        testId="parent"
        style={{ width: 200, height: 100 }}
        animate="rest"
        whileHover="hover"
        variants={{ rest: {}, hover: {} }}
        transition={INSTANT}
      >
        <div style={{ height: 10 }} />
        <motion.div testId="own" animate="rest" variants={childVariants} style={{ width: 20 }} transition={INSTANT} />
        <motion.div testId="cut" inherit={false} variants={childVariants} style={{ width: 20 }} transition={INSTANT} />
        <motion.div testId="object" animate={{ height: 30 }} variants={childVariants} style={{ width: 20 }} transition={INSTANT} />
      </motion.div>
    ))
    page.renderer.nativeSimulateMouseMove(...page.inside("parent"))
    page.advance(10)
    expect(page.bounds("own").height).toBeCloseTo(40, 0)
    expect(page.bounds("cut").height).toBeCloseTo(0, 0)
    expect(page.bounds("object").height).toBeCloseTo(80, 0)
    page.app.unmount()
  })

  it("passes the name of whileInView down", () => {
    const [spacer, setSpacer] = createSignal(600)
    const app = createTestRoot({ width: 400, height: 200 })
    const renderer = app.renderer
    renderer.clockPause()
    app.render(() => (
      <div style={{ width: 400, height: 200 }}>
        <div style={{ height: spacer() }} />
        <motion.div style={{ width: 100, height: 100 }} whileInView="seen" variants={{ seen: {} }} transition={INSTANT}>
          <motion.div testId="child" style={{ width: 10 }} variants={{ seen: { width: 90 } }} transition={INSTANT} />
        </motion.div>
      </div>
    ))
    const width = () => {
      renderer.flush()
      return renderer.getElementBounds(renderer.findByTestId("child")!.id)!.width
    }
    settle(renderer)
    expect(width()).toBe(10)
    app.flushSync(() => setSpacer(0))
    settle(renderer)
    expect(width()).toBeCloseTo(90, 0)
    app.unmount()
  })

  it("moves an SVG shape through the name of a component above it", () => {
    const page = setup(() => (
      <motion.div
        testId="d"
        style={{ width: 100, height: 100 }}
        whileHover="on"
        variants={{ off: {}, on: {} }}
        transition={INSTANT}
      >
        <svg width={100} height={100} viewBox="0 0 100 100">
          <motion.path d="M0 50 L100 50" stroke={RED} strokeWidth={20} variants={{ on: { stroke: GREEN } }} transition={INSTANT} />
        </svg>
      </motion.div>
    ))
    expectNear(page.pixel("d", 50, 50), [255, 0, 0])
    page.hover("d")
    page.advance(10)
    expectNear(page.pixel("d", 50, 50), [0, 255, 0])
    page.leave()
    page.advance(10)
    expectNear(page.pixel("d", 50, 50), [255, 0, 0])
    page.app.unmount()
  })
})

describe.skipIf(!hasNativeTestRenderer)("Solid gesture callbacks", () => {
  it("calls onHoverStart and onHoverEnd, with the position", () => {
    const calls: string[] = []
    const page = setup(() => (
      <motion.div
        testId="d"
        style={box}
        onHoverStart={(_event, info) => calls.push(`start ${info.point.x},${info.point.y}`)}
        onHoverEnd={() => calls.push("end")}
      />
    ))
    const [x, y] = page.inside("d")
    page.renderer.nativeSimulateMouseMove(x, y)
    page.advance(10)
    page.leave()
    page.advance(10)
    expect(calls).toEqual([`start ${x},${y}`, "end"])
    page.app.unmount()
  })

  it("calls onTapStart, onTap and onTapCancel", () => {
    const calls: string[] = []
    const page = setup(() => (
      <motion.div
        testId="d"
        style={box}
        onTapStart={() => calls.push("start")}
        onTap={() => calls.push("tap")}
        onTapCancel={() => calls.push("cancel")}
      />
    ))
    page.press("d")
    page.release("d")
    page.advance(10)
    page.press("d")
    page.renderer.nativeSimulateMouseMove(390, 190)
    page.renderer.nativeSimulateMouseUp(390, 190)
    page.advance(10)
    expect(calls).toEqual(["start", "tap", "start", "cancel"])
    page.app.unmount()
  })

  it("leaves the pointer handlers of the app alone", () => {
    const calls: string[] = []
    const page = setup(() => (
      <motion.div
        testId="d"
        style={box}
        onPointerEnter={() => calls.push("enter")}
        onPointerDown={() => calls.push("down")}
        onHoverStart={() => calls.push("hover")}
        onTapStart={() => calls.push("tap")}
      />
    ))
    page.hover("d")
    page.press("d")
    page.advance(10)
    expect(calls).toEqual(["hover", "enter", "down", "tap"])
    page.app.unmount()
  })

  it("calls onViewportEnter and onViewportLeave", () => {
    const calls: string[] = []
    const [spacer, setSpacer] = createSignal(600)
    const app = createTestRoot({ width: 400, height: 200 })
    const renderer = app.renderer
    renderer.clockPause()
    app.render(() => (
      <div style={{ width: 400, height: 200 }}>
        <div style={{ height: spacer() }} />
        <motion.div
          style={box}
          onViewportEnter={() => calls.push("enter")}
          onViewportLeave={() => calls.push("leave")}
        />
      </div>
    ))
    settle(renderer)
    expect(calls).toEqual([])
    app.flushSync(() => setSpacer(0))
    settle(renderer)
    app.flushSync(() => setSpacer(600))
    settle(renderer)
    expect(calls).toEqual(["enter", "leave"])
    app.unmount()
  })
})
