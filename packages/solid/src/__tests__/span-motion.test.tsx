/// The Solid half of `span-motion.test.tsx` of the React package: what an inline box moves and how its paragraph
/// follows it, against headless Chrome (`packages/react/src/__tests__/fixtures/span-motion-oracle.json`), the
/// gestures of a span, motion values in its style, and the props it ignores.

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import { createSignal, type JSX } from "solid-js"
import { describe, expect, it } from "bun:test"
import { animate, motion, motionValue } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

interface Sample {
  next: [number, number, number, number] | null
  height: number
}
interface OracleCase {
  width: number
  samples: Record<string, Sample>
}

const ORACLE: Record<string, OracleCase> = JSON.parse(
  fs.readFileSync(
    path.resolve(
      path.dirname(fileURLToPath(import.meta.url)),
      "../../../react/src/__tests__/fixtures/span-motion-oracle.json"
    ),
    "utf8"
  )
).cases

const TEXT = { fontSize: 16, lineHeight: "20px", fontFamily: "Menlo", color: "#000000" } as const
const LINEAR = { duration: 1, ease: "linear" } as const
/** gpui measures a text to the next whole pixel, so a box is up to a pixel from Chrome's. */
const NEAR = 1.5

function setup(body: () => JSX.Element, width = 300) {
  const app = createTestRoot({ width: 400, height: 200 })
  const renderer = app.renderer
  renderer.clockPause()
  app.render(() => (
    <div style={{ width: 400, height: 200 }}>
      <text testId="p" style={{ ...TEXT, width }}>
        {body()}
      </text>
    </div>
  ))
  const bounds = (testId: string) => {
    renderer.flush()
    return renderer.getElementBounds(renderer.findByTestId(testId)!.id)!
  }
  return {
    app,
    renderer,
    bounds,
    left: (testId: string) => bounds(testId).x - bounds("p").x,
    height: () => bounds("p").height,
    advance: (ms: number) => {
      renderer.clockFastForward(ms)
      renderer.flush()
      renderer.dispatchNativeEvents()
    },
  }
}

describe.skipIf(!hasNativeTestRenderer)("Solid motion.span moves the space along its line, as Chrome lays it out", () => {
  const cases: Array<[string, Record<string, [unknown, unknown]>, Record<string, unknown>?]> = [
    ["padding of a span moves what follows it", { paddingLeft: [0, 40], paddingRight: [0, 20] }],
    ["margin of a span moves what follows it", { marginLeft: [0, 30], marginRight: [0, 10] }],
    [
      "border width of a span moves what follows it",
      { borderLeftWidth: [0, 8], borderRightWidth: [0, 4] },
      { borderStyle: "solid", borderColor: "#ff0000", borderWidth: 0 },
    ],
    ["font size of a span moves what follows it and grows the line", { fontSize: [16, 32] }],
  ]
  for (const [name, keys, style] of cases) {
    it(name, () => {
      const initial = Object.fromEntries(Object.entries(keys).map(([key, [from]]) => [key, from]))
      const target = Object.fromEntries(Object.entries(keys).map(([key, [, to]]) => [key, to]))
      const page = setup(() => (
        <>
          <motion.span testId="s" style={style} initial={initial as never} animate={target as never} transition={LINEAR}>
            aaaa
          </motion.span>
          <span testId="n">bbbb</span>
        </>
      ))
      let elapsed = 0
      for (const time of [0, 0.25, 0.5, 0.75, 1]) {
        page.advance(time * 1000 - elapsed)
        elapsed = time * 1000
        const chrome = ORACLE[name]!.samples[String(time)]!
        expect(Math.abs(page.left("n") - chrome.next![0]), `${name} at ${time}`).toBeLessThan(NEAR)
        expect(Math.abs(page.height() - chrome.height), `the height of the line at ${time}`).toBeLessThanOrEqual(1)
      }
      page.app.unmount()
    })
  }

  it("rewraps a paragraph while the padding of a span grows", () => {
    const name = "padding rewraps a paragraph"
    const page = setup(
      () => (
        <>
          aaaa{" "}
          <motion.span testId="s" initial={{ paddingLeft: 0 }} animate={{ paddingLeft: 60 }} transition={LINEAR}>
            bbbb
          </motion.span>{" "}
          cccc
        </>
      ),
      ORACLE[name]!.width
    )
    let elapsed = 0
    for (const time of [0, 0.25, 0.5, 0.75, 1]) {
      page.advance(time * 1000 - elapsed)
      elapsed = time * 1000
      expect(Math.abs(page.height() - ORACLE[name]!.samples[String(time)]!.height), `at ${time}`).toBeLessThanOrEqual(1)
    }
    page.app.unmount()
  })
})

describe.skipIf(!hasNativeTestRenderer)("Solid motion values in the style of a span", () => {
  it("move the padding of a span and the text after it", () => {
    const padding = motionValue(0)
    const page = setup(() => (
      <>
        <span testId="s" style={{ paddingLeft: padding }}>
          aaaa
        </span>
        <span testId="n">bbbb</span>
      </>
    ))
    const before = page.left("n")
    animate(padding, 40, LINEAR)
    page.advance(500)
    expect(page.left("n") - before).toBeCloseTo(20, 0)
    page.advance(1000)
    expect(page.left("n") - before).toBeCloseTo(40, 0)
    page.app.unmount()
  })

  it("take the transforms and keep them off the span", () => {
    const page = setup(() => (
      <>
        <motion.span testId="s" style={{ x: motionValue(50) } as never} animate={{ x: 100, scale: 2 }}>
          aaaa
        </motion.span>
        <span testId="n">bbbb</span>
      </>
    ))
    const before = page.bounds("s")
    page.advance(1000)
    expect(page.bounds("s")).toEqual(before)
    page.app.unmount()
  })
})

describe.skipIf(!hasNativeTestRenderer)("Solid whileHover, whileTap, whileFocus and whileInView on a motion.span", () => {
  it("goes to the hover target while the pointer is over the span, and back", () => {
    const page = setup(() => (
      <>
        <motion.span
          testId="s"
          whileHover={{ paddingLeft: 40, paddingRight: 20 }}
          whileTap={{ paddingLeft: 60 }}
          transition={LINEAR}
        >
          aaaa
        </motion.span>
        <span testId="n">bbbb</span>
      </>
    ))
    const rest = page.left("n")
    const box = page.bounds("s")
    page.renderer.nativeSimulateMouseMove(box.x + 4, box.y + 8)
    page.advance(1)
    page.advance(499)
    const chrome = ORACLE["padding of a span moves what follows it"]!.samples["0.5"]!.next![0]!
    expect(Math.abs(page.left("n") - chrome)).toBeLessThan(NEAR)
    page.advance(500)
    expect(page.left("n") - rest).toBeCloseTo(60, 0)
    page.renderer.nativeSimulateMouseMove(300, 150)
    page.advance(1)
    page.advance(1000)
    expect(page.left("n")).toBeCloseTo(rest, 0)
    page.app.unmount()
  })

  it("answers to a press with whileTap", () => {
    const page = setup(() => (
      <>
        <motion.span testId="s" whileTap={{ paddingLeft: 30 }} transition={{ duration: 0 }}>
          aaaa
        </motion.span>
        <span testId="n">bbbb</span>
      </>
    ))
    const rest = page.left("n")
    const box = page.bounds("s")
    page.renderer.nativeSimulateMouseDown(box.x + 4, box.y + 8)
    page.advance(10)
    expect(page.left("n") - rest).toBeCloseTo(30, 0)
    page.renderer.nativeSimulateMouseUp(box.x + 4, box.y + 8)
    page.advance(10)
    expect(page.left("n")).toBeCloseTo(rest, 0)
    page.app.unmount()
  })

  it("answers to focus that :focus-visible matches with whileFocus", () => {
    const page = setup(() => (
      <>
        <motion.span testId="s" tabIndex={0} whileFocus={{ paddingLeft: 20 }} transition={{ duration: 0 }}>
          aaaa
        </motion.span>
        <span testId="n" tabIndex={0}>
          bbbb
        </span>
      </>
    ))
    const rest = page.left("n")
    page.renderer.simulateKeystrokes("Tab")
    page.advance(10)
    expect(page.left("n") - rest).toBeCloseTo(20, 0)
    page.renderer.simulateKeystrokes("Tab")
    page.advance(10)
    expect(page.left("n")).toBeCloseTo(rest, 0)
    page.app.unmount()
  })

  it("animates to the whileInView target once the span is in view, and back when it leaves", () => {
    const [spacer, setSpacer] = createSignal(600)
    const app = createTestRoot({ width: 400, height: 200 })
    const renderer = app.renderer
    renderer.clockPause()
    app.render(() => (
      <div style={{ width: 400, height: 200 }}>
        <div style={{ height: spacer() }} />
        <text testId="p" style={TEXT}>
          <motion.span testId="s" whileInView={{ paddingLeft: 30 }} transition={{ duration: 0 }}>
            aaaa
          </motion.span>
          <span testId="n">bbbb</span>
        </text>
      </div>
    ))
    const left = () => {
      renderer.flush()
      return renderer.getElementBounds(renderer.findByTestId("n")!.id)!.x - renderer.getElementBounds(renderer.findByTestId("p")!.id)!.x
    }
    const settle = () => {
      for (let step = 0; step < 8; step++) {
        renderer.clockFastForward(20)
        renderer.flush()
        renderer.dispatchNativeEvents()
      }
    }
    settle()
    const rest = left()
    app.flushSync(() => setSpacer(0))
    settle()
    expect(left() - rest).toBeCloseTo(30, 0)
    app.flushSync(() => setSpacer(600))
    settle()
    expect(left()).toBeCloseTo(rest, 0)
    app.unmount()
  })

  it("takes layout and drag props and moves nothing with them", () => {
    const calls: string[] = []
    const page = setup(() => (
      <>
        <motion.span
          testId="s"
          layout
          layoutId="chip"
          drag
          dragConstraints={{ left: 0 }}
          onLayoutAnimationStart={() => calls.push("start")}
          animate={{ color: "#0000ff" }}
        >
          aaaa
        </motion.span>
        <span testId="n">bbbb</span>
      </>
    ))
    const before = page.left("n")
    page.advance(1000)
    expect(page.left("n")).toBe(before)
    expect(calls).toEqual([])
    page.app.unmount()
  })
})
