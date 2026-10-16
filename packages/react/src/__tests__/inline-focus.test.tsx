/// Focus on inline boxes. A `<span>` with a `tabIndex` is focusable as in a
/// browser, although it has no element of its own: its paragraph anchors the focus
/// handle, and a press on its glyphs focuses it.

import React from "react"
import { describe, expect, it } from "vitest"
import { document, window, type DomEvent } from "@gpuix/native/host"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const LINE = 20
const TEXT = { fontSize: 16, lineHeight: `${LINE}px`, fontFamily: "Menlo", color: "#000000" } as const
const STOP = { width: 40, height: 20 } as const

let measuredColumn: number | undefined

/** Menlo has one advance for every character, so a column is a fixed width. Measured
 *  once, in a window of its own: a second test renderer stops the first. */
function columnWidth() {
  measuredColumn ??= measureColumn()
  return measuredColumn
}

function measureColumn() {
  const testRoot = createTestRoot()
  testRoot.render(
    <div highlight={{ query: "aaaaaaaaaa" }}>
      <text style={TEXT}>{"aaaaaaaaaa"}</text>
    </div>
  )
  const width = testRoot.renderer.getPaintedHighlights()[0]!.rects[0]!.width / 10
  testRoot.unmount()
  return width
}

/** The middle of a character cell of the element `testId`. */
function cell(testRoot: TestRoot, testId: string, column: number, line = 0) {
  const element = testRoot.renderer.findByTestId(testId)
  expect(element, `missing testId ${testId}`).toBeDefined()
  const box = testRoot.renderer.getElementBounds(element!.id)
  expect(box, `no painted bounds for ${testId}`).not.toBeNull()
  const width = columnWidth()
  return { x: box!.x + (column + 0.5) * width, y: box!.y + (line + 0.5) * LINE }
}

describeNative("focus on inline boxes", () => {
  function setup(node: React.ReactNode) {
    columnWidth()
    const testRoot = createTestRoot({ width: 400, height: 300 })
    testRoot.render(node)
    const focused = () =>
      testRoot.renderer.getElement(testRoot.renderer.getFocusedElementId())?.testId ?? null
    const tab = (keys = "Tab") => {
      testRoot.renderer.simulateKeystrokes(keys)
      return focused()
    }
    const click = (testId: string, column: number, line = 0) => {
      const point = cell(testRoot, testId, column, line)
      testRoot.renderer.nativeSimulateClick(point.x, point.y)
    }
    return { testRoot, focused, tab, click }
  }

  /** `[first] Go to one or two. [last]`, with a `tabIndex` on `one` and `two`. */
  function sentence(tabIndexes: { one?: number; two?: number } = { one: 0, two: 0 }) {
    return (
      <div style={{ width: 300 }}>
        <div testId="first" tabIndex={0} style={STOP} />
        <text testId="p" style={TEXT}>
          Go to{" "}
          <span testId="one" tabIndex={tabIndexes.one}>
            one
          </span>{" "}
          or{" "}
          <span testId="two" tabIndex={tabIndexes.two}>
            two
          </span>
          .
        </text>
        <div testId="last" tabIndex={0} style={STOP} />
      </div>
    )
  }

  describe("Tab order", () => {
    it("visits the spans of a paragraph between the focusables around it, in tree order", () => {
      const { focused, tab } = setup(sentence())
      expect(focused()).toBeNull()
      expect([tab(), tab(), tab(), tab()]).toEqual(["first", "one", "two", "last"])
      expect(tab()).toBe("first")
      expect([tab("Shift+Tab"), tab("Shift+Tab"), tab("Shift+Tab"), tab("Shift+Tab")]).toEqual([
        "last",
        "two",
        "one",
        "first",
      ])
    })

    it("orders a nested span after the span around it", () => {
      const { tab } = setup(
        <text style={TEXT}>
          <span testId="outer" tabIndex={0}>
            ab
            <span testId="inner" tabIndex={0}>
              cd
            </span>
          </span>{" "}
          <span testId="after" tabIndex={0}>
            ef
          </span>
        </text>
      )
      expect([tab(), tab(), tab()]).toEqual(["outer", "inner", "after"])
    })

    it("gives a positive tabIndex the DOM's place before the zeros", () => {
      const { tab } = setup(sentence({ one: 0, two: 1 }))
      expect([tab(), tab(), tab(), tab()]).toEqual(["two", "first", "one", "last"])
    })

    it("skips a span with a negative tabIndex", () => {
      const { tab } = setup(sentence({ one: -1, two: 0 }))
      expect([tab(), tab(), tab()]).toEqual(["first", "two", "last"])
    })

    it("skips a span without a tabIndex", () => {
      const { tab } = setup(sentence({ two: 0 }))
      expect([tab(), tab(), tab()]).toEqual(["first", "two", "last"])
    })

    it("leaves the tab order of two paragraphs in document order", () => {
      const { tab } = setup(
        <div style={{ width: 300 }}>
          <text style={TEXT}>
            <span testId="a" tabIndex={0}>
              a
            </span>
          </text>
          <div testId="between" tabIndex={0} style={STOP} />
          <text style={TEXT}>
            <span testId="b" tabIndex={0}>
              b
            </span>
          </text>
        </div>
      )
      expect([tab(), tab(), tab()]).toEqual(["a", "between", "b"])
    })
  })

  describe("focus() and blur()", () => {
    it("focuses and blurs a span, which is then document.activeElement", () => {
      const refs: Record<string, any> = {}
      const { testRoot, focused } = setup(
        <text style={TEXT}>
          <span ref={(element) => (refs.one = element)} testId="one" tabIndex={0}>
            one
          </span>{" "}
          <span ref={(element) => (refs.two = element)} testId="two" tabIndex={0}>
            two
          </span>
        </text>
      )
      expect(document.activeElement).toBeNull()
      refs.one.focus()
      expect(focused()).toBe("one")
      expect(document.activeElement).toBe(refs.one)
      refs.two.focus()
      expect(document.activeElement).toBe(refs.two)
      // blur() of an element that is not the focused one does nothing.
      refs.one.blur()
      expect(document.activeElement).toBe(refs.two)
      refs.two.blur()
      expect(document.activeElement).toBeNull()
      testRoot.unmount()
    })

    it("answers :focus, :focus-visible and the :focus-within of the ancestors", () => {
      const refs: Record<string, any> = {}
      const { testRoot } = setup(
        <div ref={(element) => (refs.root = element)} style={{ width: 300 }}>
          <text ref={(element) => (refs.text = element)} style={TEXT}>
            Go to{" "}
            <span ref={(element) => (refs.span = element)} testId="one" tabIndex={0}>
              one
            </span>
          </text>
        </div>
      )
      for (const element of [refs.root, refs.text, refs.span]) {
        expect(element.matches(":focus-within")).toBe(false)
      }
      refs.span.focus()
      expect(refs.span.matches(":focus")).toBe(true)
      expect(refs.span.matches(":focus-visible")).toBe(true)
      expect(refs.text.matches(":focus")).toBe(false)
      expect(refs.text.matches(":focus-within")).toBe(true)
      expect(refs.root.matches(":focus-within")).toBe(true)
      refs.span.blur()
      expect(refs.span.matches(":focus")).toBe(false)
      expect(refs.root.matches(":focus-within")).toBe(false)
      testRoot.unmount()
    })

    it("ignores focus() on a span without a tabIndex", () => {
      const refs: Record<string, any> = {}
      const { focused } = setup(
        <text style={TEXT}>
          <span ref={(element) => (refs.span = element)} testId="plain">
            plain
          </span>
        </text>
      )
      refs.span.focus()
      expect(focused()).toBeNull()
      expect(document.activeElement).toBeNull()
    })

    it("scrolls a focused span into view, with the span's own box", () => {
      const refs: Record<string, any> = {}
      const { testRoot } = setup(
        <div testId="scroller" style={{ width: 200, height: 60, overflow: "scroll" }}>
          <div style={{ width: 100, height: 200, flexShrink: 0 }} />
          <text style={TEXT}>
            <span ref={(element) => (refs.span = element)} testId="low" tabIndex={0}>
              low
            </span>
          </text>
        </div>
      )
      const scroller = testRoot.renderer.findByTestId("scroller")!.id
      expect(testRoot.renderer.getScrollOffset(scroller)?.[1] ?? 0).toBe(0)
      refs.span.focus()
      expect(testRoot.renderer.getScrollOffset(scroller)![1]).toBeGreaterThan(0)
      const port = testRoot.renderer.getElementBounds(scroller)!
      const box = testRoot.renderer.getElementBounds(testRoot.renderer.findByTestId("low")!.id)!
      expect(box.y).toBeGreaterThanOrEqual(port.y)
      expect(box.y + box.height).toBeLessThanOrEqual(port.y + port.height + 0.5)
      testRoot.unmount()
    })

    it("scrolls to a span that Tab lands on", () => {
      const { testRoot, tab } = setup(
        <div testId="scroller" style={{ width: 200, height: 60, overflow: "scroll" }}>
          <div style={{ width: 100, height: 200, flexShrink: 0 }} />
          <text style={TEXT}>
            <span testId="low" tabIndex={0}>
              low
            </span>
          </text>
        </div>
      )
      const scroller = testRoot.renderer.findByTestId("scroller")!.id
      expect(tab()).toBe("low")
      expect(testRoot.renderer.getScrollOffset(scroller)![1]).toBeGreaterThan(0)
      testRoot.unmount()
    })
  })

  describe("focus events", () => {
    type Seen = { type: string; target: unknown; relatedTarget: unknown; focusVisible: unknown }

    function recording() {
      const refs: Record<string, any> = {}
      const seen: Seen[] = []
      const record = (event: DomEvent) =>
        seen.push({
          type: event.type,
          target: event.target,
          relatedTarget: event.relatedTarget,
          focusVisible: event.focusVisible,
        })
      const scene = (
        <div style={{ width: 300 }}>
          <text style={TEXT}>
            <span
              ref={(element) => (refs.one = element)}
              testId="one"
              tabIndex={0}
              onFocus={record}
              onBlur={record}
            >
              one
            </span>{" "}
            <span
              ref={(element) => (refs.two = element)}
              testId="two"
              tabIndex={-1}
              onFocus={record}
              onBlur={record}
            >
              two
            </span>
          </text>
        </div>
      )
      const cleanups = [
        ["focus", record],
        ["blur", record],
      ].map(([type, listener]: any) => {
        window.addEventListener(type, listener, { capture: true })
        return () => window.removeEventListener(type, listener, { capture: true })
      })
      return { refs, seen, scene, stop: () => cleanups.forEach((cleanup) => cleanup()) }
    }

    it("fires focus, focusin, blur and focusout with the spans as target and relatedTarget", () => {
      const { refs, seen, scene, stop } = recording()
      const { testRoot } = setup(scene)
      refs.one.focus()
      refs.two.focus()
      stop()

      // React's onFocus and onBlur are focusin and focusout; the window hears focus
      // and blur in the capture phase, since they do not bubble.
      expect(seen.map((entry) => entry.type)).toEqual([
        "focus",
        "focusin",
        "blur",
        "focusout",
        "focus",
        "focusin",
      ])
      const of = (type: string) => seen.filter((entry) => entry.type === type)
      expect(of("focus").map((entry) => entry.target)).toEqual([refs.one, refs.two])
      expect(of("focus").map((entry) => entry.relatedTarget)).toEqual([null, refs.one])
      expect(of("focusin").map((entry) => entry.target)).toEqual([refs.one, refs.two])
      expect(of("blur").map((entry) => entry.target)).toEqual([refs.one])
      expect(of("blur").map((entry) => entry.relatedTarget)).toEqual([refs.two])
      expect(of("focusout").map((entry) => entry.target)).toEqual([refs.one])
      expect(of("focusout").map((entry) => entry.relatedTarget)).toEqual([refs.two])
      testRoot.unmount()
    })

    it("reports focusVisible by the heuristic of elements", () => {
      const { refs, seen, scene, stop } = recording()
      const { testRoot, tab } = setup(scene)
      // Keyboard focus is visible, and so is a script's before any input.
      tab()
      refs.two.focus()
      expect(seen.filter((entry) => entry.type === "focusin").map((entry) => entry.focusVisible)).toEqual([
        true,
        true,
      ])
      // A press makes a script's focus not visible.
      const point = cell(testRoot, "one", 1)
      testRoot.renderer.nativeSimulateClick(point.x, point.y)
      seen.length = 0
      refs.two.focus()
      expect(seen.find((entry) => entry.type === "focusin")!.focusVisible).toBe(false)
      stop()
      testRoot.unmount()
    })
  })

  describe("press", () => {
    it("focuses the span under a press and nothing for a press beside it", () => {
      const { focused, click } = setup(sentence())
      // "Go to one or two.": one is columns 6-8, two is 13-15.
      click("p", 7)
      expect(focused()).toBe("one")
      click("p", 14)
      expect(focused()).toBe("two")
      click("p", 1)
      expect(focused()).toBeNull()
    })

    it("focuses a span on the line it wraps onto", () => {
      const columns = 10
      const { focused, click } = setup(
        <div style={{ width: 300 }}>
          <text testId="p" style={{ ...TEXT, width: Math.ceil((columns + 0.5) * columnWidth()) }}>
            aaaa{" "}
            <span testId="wrap" tabIndex={0}>
              bbbb cccc
            </span>
          </text>
        </div>
      )
      click("p", 1, 1)
      expect(focused()).toBe("wrap")
      click("p", 1, 0)
      expect(focused()).toBeNull()
      click("p", 6, 0)
      expect(focused()).toBe("wrap")
    })

    it("focuses a span with tabIndex -1, which Tab does not reach", () => {
      const { focused, click, tab } = setup(sentence({ one: -1, two: 0 }))
      click("p", 7)
      expect(focused()).toBe("one")
      expect(tab()).toBe("two")
      expect(tab("Shift+Tab")).toBe("first")
    })

    it("moves from the span that has focus with Tab and Shift+Tab", () => {
      const { focused, click, tab } = setup(sentence())
      click("p", 7)
      expect(focused()).toBe("one")
      expect(tab()).toBe("two")
      expect(tab()).toBe("last")
      click("p", 14)
      expect(tab("Shift+Tab")).toBe("one")
    })

    it("focuses the innermost focusable span, and a focusable one around a plain one", () => {
      const { focused, click } = setup(
        <text testId="p" style={TEXT}>
          <span testId="outer" tabIndex={0}>
            ab
            <span testId="inner" tabIndex={0}>
              cd
            </span>
            <span>ef</span>
          </span>
        </text>
      )
      click("p", 3)
      expect(focused()).toBe("inner")
      click("p", 5)
      expect(focused()).toBe("outer")
      click("p", 0)
      expect(focused()).toBe("outer")
    })

    it("goes to the span, not to a focusable element around the paragraph", () => {
      const refs: Record<string, any> = {}
      const heard: string[] = []
      const { focused, click } = setup(
        <div
          ref={(element) => (refs.card = element)}
          testId="card"
          tabIndex={0}
          style={{ width: 300 }}
          onFocus={(event) => heard.push(event.target === refs.card ? "card" : "span")}
        >
          <text testId="p" style={TEXT}>
            Go to{" "}
            <span testId="one" tabIndex={0}>
              one
            </span>
          </text>
        </div>
      )
      click("p", 7)
      expect(focused()).toBe("one")
      expect(heard).toEqual(["span"])
      click("p", 1)
      expect(focused()).toBe("card")
      expect(heard).toEqual(["span", "card"])
    })

    it("focuses on the press and still starts a selection with a drag from the span", () => {
      const { testRoot, focused } = setup(
        <text testId="p" style={TEXT}>
          Hello{" "}
          <span testId="one" tabIndex={0}>
            world
          </span>
        </text>
      )
      const from = cell(testRoot, "p", 6)
      const to = cell(testRoot, "p", 14)
      testRoot.renderer.nativeSimulateMouseDown(from.x, from.y)
      expect(focused()).toBe("one")
      // A drag that becomes a selection gives up focus, whatever it started on.
      testRoot.renderer.nativeSimulateMouseMove(to.x, to.y)
      testRoot.renderer.nativeSimulateMouseUp(to.x, to.y)
      expect(testRoot.renderer.getSelectedText()).toBe("world")
    })

    it("leaves focus to a prevented mousedown", () => {
      const { focused, click } = setup(
        <text testId="p" style={TEXT}>
          <span testId="one" tabIndex={0} onMouseDown={(event) => event.preventDefault()}>
            one
          </span>
        </text>
      )
      click("p", 1)
      expect(focused()).toBeNull()
    })

    describe("mousedown default", () => {
      /** An input that has focus, then a press on a focusable span, which `prevent` makes on its `mousedown`
       *  or `pointerdown` as the DOM lets a handler do. */
      function pressed(prevent?: "mousedown" | "pointerdown", around = false) {
        const log: string[] = []
        const refs: Record<string, any> = {}
        const span = (
          <span
            testId="one"
            tabIndex={0}
            onFocus={() => log.push("focus one")}
            onBlur={() => log.push("blur one")}
            onMouseDown={prevent === "mousedown" ? (event) => event.preventDefault() : undefined}
            onPointerDown={prevent === "pointerdown" ? (event) => event.preventDefault() : undefined}
          >
            one
          </span>
        )
        const paragraph = (
          <text testId="p" style={TEXT}>
            Go to {span}
          </text>
        )
        const setupped = setup(
          <div style={{ width: 300 }}>
            <div
              ref={(element) => (refs.before = element)}
              testId="before"
              tabIndex={0}
              style={STOP}
              onFocus={() => log.push("focus before")}
              onBlur={() => log.push("blur before")}
            />
            {around ? (
              <div testId="card" tabIndex={0} style={{ width: 300 }}>
                {paragraph}
              </div>
            ) : (
              paragraph
            )}
          </div>
        )
        refs.before.focus()
        log.length = 0
        setupped.click("p", 7)
        return { ...setupped, log }
      }

      for (const around of [false, true]) {
        const where = around ? " inside a focusable element" : ""

        it(`keeps focus where it was when mousedown is prevented${where}`, () => {
          const { focused, log } = pressed("mousedown", around)
          expect(focused()).toBe("before")
          expect(log).toEqual([])
        })

        it(`focuses the span when nothing prevents the press${where}`, () => {
          const { focused, log } = pressed(undefined, around)
          expect(focused()).toBe("one")
          expect(log).toEqual(["blur before", "focus one"])
        })

        it(`focuses the span when only pointerdown is prevented${where}`, () => {
          const { focused, log } = pressed("pointerdown", around)
          expect(focused()).toBe("one")
          expect(log).toEqual(["blur before", "focus one"])
        })
      }

      it("does not carry a prevented press's focus over to the next press", () => {
        const { focused, click, testRoot } = setup(
          <div style={{ width: 300 }}>
            <text testId="p" style={TEXT}>
              <span testId="one" tabIndex={0} onMouseDown={(event) => event.preventDefault()}>
                one
              </span>
            </text>
            <div testId="plain" tabIndex={0} style={STOP} />
          </div>
        )
        click("p", 1)
        expect(focused()).toBeNull()
        const point = cell(testRoot, "plain", 0)
        testRoot.renderer.nativeSimulateClick(point.x, point.y)
        expect(focused()).toBe("plain")
      })
    })

    it("lets what covers the paragraph take the press", () => {
      const { focused, testRoot } = setup(
        <div style={{ width: 300, height: 100 }}>
          <text testId="p" style={TEXT}>
            <span testId="one" tabIndex={0}>
              one
            </span>
          </text>
          <div testId="cover" style={{ position: "absolute", left: 0, top: 0, width: 300, height: 100 }} />
        </div>
      )
      const point = cell(testRoot, "p", 1)
      testRoot.renderer.nativeSimulateClick(point.x, point.y)
      expect(focused()).toBeNull()
    })
  })

  describe("when the span stops being focusable", () => {
    it("blurs a focused span whose tabIndex is removed, silently like any element", () => {
      const refs: Record<string, any> = {}
      const log: string[] = []
      const scene = (tabIndex?: number) => (
        <text style={TEXT}>
          <span
            ref={(element) => (refs.span = element)}
            testId="one"
            tabIndex={tabIndex}
            onBlur={() => log.push("blur")}
          >
            one
          </span>
        </text>
      )
      const { testRoot, focused } = setup(scene(0))
      refs.span.focus()
      expect(focused()).toBe("one")
      testRoot.render(scene(undefined))
      testRoot.renderer.flush()
      expect(focused()).toBeNull()
      expect(document.activeElement).toBeNull()
      testRoot.unmount()
    })

    it("is not reached by Tab after the tabIndex is removed, and again once it is back", () => {
      const scene = (tabIndex?: number) => (
        <div style={{ width: 300 }}>
          <div testId="first" tabIndex={0} style={STOP} />
          <text style={TEXT}>
            <span testId="one" tabIndex={tabIndex}>
              one
            </span>
          </text>
          <div testId="last" tabIndex={0} style={STOP} />
        </div>
      )
      const { testRoot, tab } = setup(scene(0))
      expect([tab(), tab(), tab()]).toEqual(["first", "one", "last"])
      testRoot.render(scene(undefined))
      expect([tab(), tab()]).toEqual(["first", "last"])
      testRoot.render(scene(0))
      expect([tab(), tab(), tab()]).toEqual(["first", "one", "last"])
      testRoot.unmount()
    })

    it("takes an inert paragraph's spans out of the tab order", () => {
      const { tab } = setup(
        <div style={{ width: 300 }}>
          <div testId="first" tabIndex={0} style={STOP} />
          <div inert>
            <text style={TEXT}>
              <span testId="one" tabIndex={0}>
                one
              </span>
            </text>
          </div>
          <div testId="last" tabIndex={0} style={STOP} />
        </div>
      )
      expect([tab(), tab()]).toEqual(["first", "last"])
    })
  })
})
