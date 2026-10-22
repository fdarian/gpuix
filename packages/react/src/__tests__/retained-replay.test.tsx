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

import React from "react"
import { expect, it } from "vitest"
import { flushSync, IntersectionObserver, motion, ResizeObserver, type PublicInstance } from "../index"
import { createTestRoot } from "../testing"
import { Card, CARDS, CARD_BG, describeNative, GREEN, Page, RED, setup, verifying, WINDOW } from "./retained-harness.js"

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

  it("scrolls smoothly an element with hidden overflow inside a replayed card, by the style it was built with", () => {
    const { renderer, counters, settle, idOf, expectWhatABuildShows, finish, unmount } = setup(
      <Page
        card={(index) =>
          index === 4 ? (
            <Card
              key={index}
              index={index}
              extra={
                <div testId="clipped" style={{ width: 50, height: 20, overflow: "hidden", scrollBehavior: "smooth" }}>
                  <div style={{ height: 400 }} />
                </div>
              }
            />
          ) : null
        }
      />
    )
    try {
      renderer.clockPause()
      settle()
      const before = counters().replays
      const id = idOf("clipped")
      const top = () => renderer.getScrollOffset(id)[1]
      renderer.scrollTo(id, 0, 100)
      expect(top(), "the first frame starts the animation").toBe(0)
      renderer.clockFastForward(100)
      renderer.flush()
      expect(top()).toBeGreaterThan(0)
      expect(top()).toBeLessThan(100)
      renderer.clockFastForward(1000)
      renderer.flush()
      expect(top()).toBe(100)
      // Replayed frames did not forget it was smooth.
      settle()
      expect(counters().replays).toBeGreaterThan(before)
      renderer.scrollTo(id, 0, 0)
      renderer.clockFastForward(100)
      renderer.flush()
      expect(top()).toBeGreaterThan(0)
      expect(top()).toBeLessThan(100)
      renderer.clockFastForward(1000)
      renderer.flush()
      settle()
      expectWhatABuildShows("smooth")
      finish()
    } finally {
      unmount()
    }
  })

  it("selects text across replayed cards, and replays them again once it is let go", () => {
    const { renderer, counters, settle, bounds, expectWhatABuildShows, finish, unmount } = setup(<Page />)
    try {
      settle()
      const from = bounds("title-0")
      const to = bounds("title-1")
      const selected = renderer.dragSelect(from.x + 2, from.y + 5, to.x + 60, to.y + 5)
      expect(selected).toContain("card 0")
      expect(selected).toContain("card 1")
      expect(renderer.getSelectedText()).toBe(selected)
      settle()
      expectWhatABuildShows("selection")
      renderer.clearSelection()
      const before = counters().replays
      settle()
      expect(renderer.getSelectedText()).toBeNull()
      expect(counters().replays).toBeGreaterThan(before)
      expectWhatABuildShows("selection-cleared")
      finish()
    } finally {
      unmount()
    }
  })

  it("moves what an element with hidden overflow shows inside a replayed card when script scrolls it", () => {
    const { renderer, settle, idOf, bounds, expectWhatABuildShows, finish, unmount } = setup(
      <Page
        card={(index) =>
          index === 7 ? (
            <Card
              key={index}
              index={index}
              extra={
                <div testId="clipped" style={{ width: 50, height: 20, overflow: "hidden" }}>
                  <div testId="content" style={{ height: 400 }} />
                </div>
              }
            />
          ) : null
        }
      />
    )
    try {
      settle()
      const rest = bounds("content").y
      renderer.scrollTo(idOf("clipped"), 0, 60, "instant")
      renderer.flush()
      expect(bounds("content").y).toBe(rest - 60)
      settle()
      expectWhatABuildShows("hidden-scroll")
      finish()
    } finally {
      unmount()
    }
  })

  it("repaints replayed cards when the window's appearance changes what light-dark() resolves to", () => {
    const render = () => (
      <Page card={(index) => <Card key={index} index={index} style={{ colorScheme: "light dark", backgroundColor: "light-dark(#ff0000, #00ff00)" }} />} />
    )
    const { renderer, settle, inside, sample, expectWhatABuildShows, finish, unmount } = setup(render())
    try {
      renderer.setAppearance("light")
      settle()
      expect(sample("scheme-light", ...inside(2))).toEqual(RED)
      renderer.setAppearance("dark")
      settle()
      expect(sample("scheme-dark", ...inside(2))).toEqual(GREEN)
      expectWhatABuildShows("scheme")
      finish()
    } finally {
      unmount()
    }
  })

  it("answers a read after a write inside one card from a pass that builds the spine and publishes the rest", () => {
    // About 2000 elements, in cards of a fixed size, so that a change inside one moves no other.
    const COUNT = 220
    const PROBE = 100
    let write!: (tall: boolean) => void
    const probe = React.createRef<PublicInstance>()
    function Big() {
      const [tall, setTall] = React.useState(false)
      write = setTall
      return (
        <div testId="page" style={{ display: "flex", flexWrap: "wrap", gap: 8, width: 1240 }}>
          {Array.from({ length: COUNT }, (_, index) => (
            <Card key={index} index={index} extra={index === PROBE ? <div ref={probe} style={{ width: 20, height: tall ? 30 : 10 }} /> : null} />
          ))}
        </div>
      )
    }
    const root = createTestRoot({ width: 1240, height: 780 })
    const { renderer } = root
    try {
      root.render(<Big />)
      for (let frame = 0; frame < 5; frame++) renderer.flush()
      // A live window is drawn by the display link, once per frame; a read in between is a pass.
      renderer.setInlineDraws(false)
      renderer.drawFrame()
      renderer.drawFrame()
      const sampled = Array.from({ length: COUNT / 20 }, (_, group) => [`card-${group * 20}`, `title-${group * 20 + 3}`, `button-${group * 20 + 7}`]).flat()
      // findByTestId walks the whole tree of 2000 elements, so the sampled ones are looked up once.
      const ids = new Map(sampled.map((testId) => [testId, renderer.findByTestId(testId).id]))
      const boxOf = (testId: string) => renderer.getElementBounds(ids.get(testId)!)
      const boxes = () => Object.fromEntries(sampled.map((testId) => [testId, boxOf(testId)]))
      const before = boxes()
      const probeBefore = probe.current!.getBoundingClientRect()
      expect(probeBefore.height).toBe(10)
      renderer.drawFrame()

      const counters = renderer.getRetainedCounters()
      const builtBefore = renderer.getBuildCounters().elementsBuilt
      const passes = renderer.getLayoutPassCount()
      flushSync(() => write(true))
      const probeAfter = probe.current!.getBoundingClientRect()
      expect(renderer.getLayoutPassCount()).toBe(passes + 1)
      expect(probeAfter.height).toBe(30)
      expect(probeAfter.y).toBe(probeBefore.y)
      const built = renderer.getBuildCounters().elementsBuilt - builtBefore
      const during = renderer.getRetainedCounters()
      // The cards were not touched: every one but the one written is skipped and publishes its boxes.
      expect(during.layoutReplays - counters.layoutReplays).toBeGreaterThanOrEqual(COUNT - 1)
      // What is built is the page and the card written, each in a root of its own that leaves a layout record.
      if (!verifying) expect(during.layoutBuilds - counters.layoutBuilds).toBe(2)
      expect(boxes()).toEqual(before)
      // The page, the card and what changed in it: nowhere near the 2000 elements a build makes.
      if (!verifying) expect(built).toBeLessThan(30)

      // The frame after replays what the pass left alone, and shows the same as a build.
      const replays = renderer.getRetainedCounters().replays
      renderer.drawFrame()
      expect(renderer.getRetainedCounters().replays - replays).toBeGreaterThanOrEqual(COUNT - 1)
      const shown = boxes()
      expect(probe.current!.getBoundingClientRect()).toEqual(probeAfter)
      renderer.setRetainPlanning(false)
      renderer.setInlineDraws(true)
      renderer.flush()
      expect(boxes()).toEqual(shown)
      expect(probe.current!.getBoundingClientRect()).toEqual(probeAfter)
      expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      renderer.setInlineDraws(true)
      root.unmount()
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
