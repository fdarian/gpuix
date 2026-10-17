// @ts-nocheck
/**
 * A frame leaves unbuilt what nothing it was built from changed, and replays it (#37 step 5). What
 * the page shows and answers has to stay what a build of the same state would give: each test
 * changes one thing about a page of cards, expects that thing to show, and then expects the frame,
 * its accessibility tree and its boxes to be those of the same page with skipping turned off.
 *
 * `getRetainedCounters` says that something was replayed, so a test of what stays true of a
 * replayed card cannot pass by nothing having been skipped.
 */

import fs from "fs"
import React from "react"
import { describe, expect, it } from "vitest"
import { IntersectionObserver, motion, ResizeObserver } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rgba = [number, number, number, number]
const RED: Rgba = [255, 0, 0, 255]
const GREEN: Rgba = [0, 255, 0, 255]
const CARD_BG: Rgba = [51, 85, 119, 255]
const CARDS = 12
const WINDOW = { width: 640, height: 480 }

type CardProps = {
  index: number
  label?: string
  onClick?: () => void
  extra?: React.ReactNode
  style?: object
}

function Card({ index, label = `card ${index}`, onClick, extra, style }: CardProps) {
  return (
    <div
      testId={`card-${index}`}
      style={{
        width: 140,
        height: 90,
        backgroundColor: "#335577",
        hover: { backgroundColor: "#ff0000" },
        ...style,
      }}
    >
      <div testId={`title-${index}`} style={{ height: 20, backgroundColor: "#223344" }}>
        <text>{label}</text>
      </div>
      <div style={{ height: 20 }}>
        <span>one</span>
        <span> two</span>
      </div>
      <div
        testId={`button-${index}`}
        tabIndex={0}
        role="button"
        aria-label={`Button ${index}`}
        onClick={onClick}
        style={{ width: 40, height: 20, backgroundColor: "#555555", focusVisible: { backgroundColor: "#00ff00" } }}
      />
      {extra}
    </div>
  )
}

function Page({ card, style, children }: { card?: (index: number) => React.ReactNode; style?: object; children?: React.ReactNode }) {
  return (
    <div testId="page" style={{ display: "flex", flexWrap: "wrap", gap: 8, width: WINDOW.width, ...style }}>
      {Array.from({ length: CARDS }, (_, index) => card?.(index) ?? <Card key={index} index={index} />)}
      {children}
    </div>
  )
}

/** A run under GPUIX_RETAIN_VERIFY builds every frame that replays a second time, which the counts of
 *  what was built and replayed cannot be read from. */
const verifying = process.env.GPUIX_RETAIN_VERIFY !== undefined

function setup(element: React.ReactElement, options = {}) {
  const root = createTestRoot({ ...WINDOW, ...options })
  const { renderer } = root
  root.render(element)
  const idOf = (testId: string) => renderer.findByTestId(testId).id
  const bounds = (testId: string) => renderer.getElementBounds(idOf(testId))
  const center = (testId: string) => {
    const box = bounds(testId)
    return [box.x + box.width / 2, box.y + box.height / 2]
  }
  const counters = () => renderer.getRetainedCounters()
  const built = () => renderer.getBuildCounters().elementsBuilt
  /** Frames until the cards are recorded, so the next ones replay them. */
  const settle = () => {
    for (let frame = 0; frame < 5; frame++) renderer.flush()
  }
  const sample = (name: string, x: number, y: number): Rgba => {
    const file = `${SHOTS_DIR}/retained-replay-${name}.png`
    fs.rmSync(file, { force: true })
    renderer.captureScreenshot(file)
    return readLogicalPixels(file, renderer).at(x, y)
  }
  /** A point inside a card that none of its children cover. */
  const inside = (index: number): [number, number] => {
    const box = bounds(`card-${index}`)
    return [box.x + box.width - 10, box.y + box.height - 10]
  }
  /** What the frame shows and answers, as bytes and text. */
  const snapshot = (name: string) => {
    const file = `${SHOTS_DIR}/retained-replay-${name}.png`
    fs.rmSync(file, { force: true })
    renderer.captureScreenshot(file)
    const boxes = Array.from({ length: CARDS }, (_, index) => {
      const card = bounds(`card-${index}`)
      const button = bounds(`button-${index}`)
      return [card.x, card.y, card.width, card.height, button.x, button.y]
    })
    return {
      picture: fs.readFileSync(file).toString("base64"),
      a11y: JSON.stringify(renderer.getA11yTree(), (key, value) => (key === "frame" ? undefined : value)),
      boxes,
      text: renderer.getAllText(),
    }
  }
  /** The frame as it is, then the same state built whole: they have to be the same. */
  const expectWhatABuildShows = (name: string) => {
    const replayed = snapshot(`${name}-replayed`)
    renderer.setRetainPlanning(false)
    renderer.flush()
    const whole = snapshot(`${name}-built`)
    renderer.setRetainPlanning(true)
    renderer.flush()
    expect(replayed.boxes).toEqual(whole.boxes)
    expect(replayed.text).toEqual(whole.text)
    expect(replayed.a11y).toEqual(whole.a11y)
    expect(replayed.picture === whole.picture).toBe(true)
  }
  const finish = (replayed = true) => {
    expect(counters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    if (replayed) expect(counters().replays).toBeGreaterThan(0)
  }
  return { ...root, idOf, bounds, center, counters, built, settle, sample, inside, expectWhatABuildShows, finish }
}

describeNative("a page of retained cards", () => {
  it("replays the cards of a page that does not change, and builds none", () => {
    const { renderer, counters, built, settle, expectWhatABuildShows, finish, unmount } = setup(<Page />)
    try {
      settle()
      const before = counters()
      const builtBefore = built()
      renderer.flush()
      renderer.flush()
      expect(counters().replays).toBeGreaterThan(before.replays)
      if (!verifying) {
        expect(counters().builds).toBe(before.builds)
        expect(built() - builtBefore).toBe(0)
      }
      expectWhatABuildShows("idle")
      finish()
    } finally {
      unmount()
    }
  })

  it("paints the hover style of one card and leaves the others replayed", () => {
    const { renderer, counters, built, settle, center, inside, sample, expectWhatABuildShows, finish, unmount } = setup(<Page />)
    try {
      settle()
      expect(sample("hover-before", ...inside(3))).toEqual(CARD_BG)
      settle()
      const before = counters()
      const builtBefore = built()
      renderer.nativeSimulateMouseMove(...center("title-3"))
      renderer.flush()
      const builtByTheMove = built() - builtBefore
      expect(sample("hover-on", ...inside(3))).toEqual(RED)
      expect(sample("hover-other", ...inside(4))).toEqual(CARD_BG)
      expect(counters().replays).toBeGreaterThan(before.replays)
      // A card, its title and what is under the pointer are built; nowhere near the page.
      if (!verifying) expect(builtByTheMove).toBeLessThan(CARDS * 4)
      expectWhatABuildShows("hover-on")

      // Moving to the next card changes two.
      renderer.nativeSimulateMouseMove(...center("title-4"))
      renderer.flush()
      expect(sample("hover-moved-from", ...inside(3))).toEqual(CARD_BG)
      expect(sample("hover-moved-to", ...inside(4))).toEqual(RED)
      renderer.nativeSimulateMouseMove(639, 479)
      renderer.flush()
      settle()
      expect(sample("hover-off", ...inside(4))).toEqual(CARD_BG)
      expectWhatABuildShows("hover-off")
      finish()
    } finally {
      unmount()
    }
  })

  it("paints the focus ring of the button that Tab reaches, inside a replayed card", () => {
    const { renderer, counters, settle, center, sample, bounds, expectWhatABuildShows, finish, unmount } = setup(<Page />)
    try {
      settle()
      const before = counters().replays
      renderer.simulateKeystrokes("Tab")
      renderer.flush()
      const [x, y] = center("button-0")
      expect(sample("focus-first", x, y)).toEqual(GREEN)
      expect(renderer.getElement(renderer.getFocusedElementId())?.testId).toBe("button-0")
      renderer.simulateKeystrokes("Tab")
      renderer.flush()
      expect(sample("focus-second", ...center("button-1"))).toEqual(GREEN)
      expect(counters().replays).toBeGreaterThan(before)
      expect(bounds("button-1").x).toBeGreaterThan(bounds("button-0").x)
      expectWhatABuildShows("focus")
      finish()
    } finally {
      unmount()
    }
  })

  it("walks the tab order of replayed cards in order, forward and back", () => {
    const { renderer, settle, finish, unmount } = setup(<Page />)
    try {
      settle()
      const focused = () => renderer.getElement(renderer.getFocusedElementId())?.testId
      const order: string[] = []
      for (let press = 0; press < 5; press++) {
        renderer.simulateKeystrokes("Tab")
        order.push(focused())
      }
      expect(order).toEqual(["button-0", "button-1", "button-2", "button-3", "button-4"])
      renderer.simulateKeystrokes("Shift+Tab")
      expect(focused()).toBe("button-3")
      finish()
    } finally {
      unmount()
    }
  })

  it("keeps an animation running inside a card while the others are replayed", () => {
    const FADE = { duration: 1, ease: "linear" }
    const { renderer, counters, built, settle, bounds, expectWhatABuildShows, finish, unmount } = setup(
      <Page
        card={(index) =>
          index === 2 ? (
            <Card
              key={index}
              index={index}
              extra={
                <motion.div
                  testId="fader"
                  style={{ position: "absolute", left: 0, top: 0, width: 20, height: 10, backgroundColor: "#ff0000" }}
                  initial={{ left: 0 }}
                  animate={{ left: 100 }}
                  transition={FADE}
                />
              }
              style={{ position: "relative" }}
            />
          ) : null
        }
      />
    )
    try {
      renderer.clockPause()
      settle()
      const before = counters()
      const builtBefore = built()
      const leftAt = () => {
        renderer.clockFastForward(250)
        return bounds("fader").x - bounds("card-2").x
      }
      const first = leftAt()
      const second = leftAt()
      expect(first).toBeGreaterThan(0)
      expect(second).toBeGreaterThan(first)
      expect(counters().replays).toBeGreaterThan(before.replays)
      // Per frame the animated card is built, and the eleven others are not.
      if (!verifying) expect((built() - builtBefore) / 2).toBeLessThan(CARDS * 8 * 0.5)
      renderer.clockFastForward(2000)
      settle()
      expectWhatABuildShows("fade-done")
      finish()
    } finally {
      unmount()
    }
  })

  it("shows an edited text in one card and replays the rest", () => {
    const render = (label: string) => <Page card={(index) => (index === 5 ? <Card key={index} index={5} label={label} /> : null)} />
    const { renderer, counters, settle, render: rerender, expectWhatABuildShows, finish, unmount } = setup(render("first"))
    try {
      settle()
      expect(renderer.getAllText()).toContain("first")
      const before = counters()
      rerender(render("second, and longer"))
      renderer.flush()
      expect(renderer.getAllText()).toContain("second, and longer")
      expect(renderer.getAllText()).not.toContain("first")
      settle()
      expect(counters().replays).toBeGreaterThan(before.replays)
      expectWhatABuildShows("text")
      finish()
    } finally {
      unmount()
    }
  })

  it("hands a changed inherited color to the cards below it", () => {
    const render = (color: string) => (
      <div style={{ color }}>
        <Page
          card={(index) => (
            <Card key={index} index={index} extra={<div testId={`swatch-${index}`} style={{ width: 20, height: 10, backgroundColor: "currentColor" }} />} />
          )}
        />
      </div>
    )
    const { renderer, render: rerender, settle, bounds, sample, expectWhatABuildShows, finish, unmount } = setup(render("#ff0000"))
    try {
      settle()
      const swatch = bounds("swatch-7")
      const at = [swatch.x + 5, swatch.y + 5] as const
      expect(sample("color-red", ...at)).toEqual(RED)
      rerender(render("#00ff00"))
      settle()
      expect(sample("color-green", ...at)).toEqual(GREEN)
      expectWhatABuildShows("color")
      finish()
    } finally {
      unmount()
    }
  })

  it("lays the cards out again when the window is resized", () => {
    const { renderer, counters, settle, bounds, expectWhatABuildShows, finish, unmount } = setup(<Page style={{ width: "100%" }} />)
    try {
      settle()
      const wide = bounds("card-5")
      renderer.setWindowSize(320, 480)
      settle()
      const narrow = bounds("card-5")
      expect(narrow.y).toBeGreaterThan(wide.y)
      const before = counters().replays
      renderer.flush()
      renderer.flush()
      expect(counters().replays).toBeGreaterThan(before)
      expectWhatABuildShows("resize")
      renderer.setWindowSize(WINDOW.width, WINDOW.height)
      settle()
      expect(bounds("card-5").y).toBe(wide.y)
      finish()
    } finally {
      unmount()
    }
  })

  it("flips what a container query selects when the container resizes", () => {
    const render = (width: number) => (
      <div testId="container" style={{ containerType: "inline-size", width }}>
        <Page
          style={{ width: "100%" }}
          card={(index) =>
            index === 0 ? (
              <Card
                key={index}
                index={index}
                style={{ "@container": { "(min-width: 400px)": { backgroundColor: "#00ff00" } } }}
              />
            ) : null
          }
        />
      </div>
    )
    const { renderer, render: rerender, settle, inside, sample, expectWhatABuildShows, finish, unmount } = setup(render(300))
    try {
      settle()
      expect(sample("cq-narrow", ...inside(0))).toEqual(CARD_BG)
      rerender(render(600))
      settle()
      expect(sample("cq-wide", ...inside(0))).toEqual(GREEN)
      rerender(render(300))
      settle()
      expect(sample("cq-narrow-again", ...inside(0))).toEqual(CARD_BG)
      expectWhatABuildShows("cq")
      finish()
    } finally {
      unmount()
    }
  })

  it("reports to observers of an element while the cards around it are replayed", () => {
    const sizes: number[] = []
    const intersections: boolean[] = []
    function Observed({ width }: { width: number }) {
      const ref = React.useRef(null)
      React.useEffect(() => {
        const resize = new ResizeObserver((entries) => sizes.push(entries.at(-1).contentRect.width))
        const intersect = new IntersectionObserver((entries) => intersections.push(entries.at(-1).isIntersecting))
        resize.observe(ref.current)
        intersect.observe(ref.current)
        return () => {
          resize.disconnect()
          intersect.disconnect()
        }
      }, [])
      return <div ref={ref} testId="observed" style={{ width, height: 30, backgroundColor: "#999999" }} />
    }
    const render = (width: number) => <Page>{<Observed key="observed" width={width} />}</Page>
    const { renderer, counters, render: rerender, settle, finish, unmount } = setup(render(100))
    try {
      settle()
      renderer.dispatchNativeEvents()
      expect(sizes.at(-1)).toBe(100)
      expect(intersections.at(-1)).toBe(true)
      const before = counters().replays
      rerender(render(180))
      settle()
      renderer.dispatchNativeEvents()
      expect(sizes.at(-1)).toBe(180)
      expect(counters().replays).toBeGreaterThan(before)
      finish()
    } finally {
      unmount()
    }
  })

  it("answers getBoundingClientRect, hit tests and clicks inside replayed cards", () => {
    const clicks: number[] = []
    const { renderer, counters, settle, idOf, bounds, center, expectWhatABuildShows, finish, unmount } = setup(
      <Page card={(index) => <Card key={index} index={index} onClick={() => clicks.push(index)} />} />
    )
    try {
      settle()
      const before = counters().replays
      renderer.flush()
      expect(counters().replays).toBeGreaterThan(before)
      // 140 wide cards and an 8 gap: four to a row of 640.
      expect(bounds("card-5")).toMatchObject({ x: 148, y: 98, width: 140, height: 90 })
      expect(bounds("button-5")).toMatchObject({ x: 148, y: 98 + 40, width: 40, height: 20 })
      const element = renderer.getElement(idOf("button-5"))
      expect(element).toBeTruthy()
      expect(renderer.getElementsAt(...center("button-5"))[0]).toBe(idOf("button-5"))
      expect(renderer.getElementsAt(...center("title-5"))).toContain(idOf("title-5"))
      renderer.nativeSimulateClick(...center("button-5"))
      renderer.nativeSimulateClick(...center("button-0"))
      renderer.nativeSimulateClick(...center("button-11"))
      expect(clicks).toEqual([5, 0, 11])
      expectWhatABuildShows("answers")
      finish()
    } finally {
      unmount()
    }
  })

  it("keeps a card out of the replays while its style is pressed, and replays it after", () => {
    const { renderer, settle, center, inside, sample, finish, unmount } = setup(
      <Page card={(index) => <Card key={index} index={index} style={{ active: { backgroundColor: "#0000ff" } }} />} />
    )
    try {
      settle()
      renderer.nativeSimulateMouseMove(...center("title-6"))
      renderer.nativeSimulateMouseDown(...center("title-6"))
      renderer.flush()
      expect(sample("active-on", ...inside(6))).toEqual([0, 0, 255, 255])
      renderer.nativeSimulateMouseUp(...center("title-6"))
      renderer.nativeSimulateMouseMove(639, 479)
      settle()
      expect(sample("active-off", ...inside(6))).toEqual(CARD_BG)
      finish()
    } finally {
      unmount()
    }
  })
})
