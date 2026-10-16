// @ts-nocheck
/** The DOM focus model: focusable rules, tab order, event order, :focus-visible. */

import fs from "node:fs"
import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { isCI, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const box = { width: 40, height: 20 }

function setup(element: React.ReactElement, options = {}) {
  const root = createTestRoot({ width: 300, height: 200, ...options })
  root.render(element)
  const { renderer } = root
  const focused = () => renderer.getElement(renderer.getFocusedElementId())?.testId ?? null
  const idOf = (testId: string) => renderer.findByTestId(testId).id
  const center = (testId: string) => {
    const bounds = renderer.getElementBounds(idOf(testId))
    return [bounds.x + bounds.width / 2, bounds.y + bounds.height / 2]
  }
  return { root, renderer, focused, idOf, center }
}

describeNative("focus model", () => {
  describe("focusable rules", () => {
    it("is not made focusable by key or focus listeners", () => {
      const keys: string[] = []
      const { renderer, focused, idOf } = setup(
        <div style={{ width: 200, height: 100 }} onKeyDown={(event) => keys.push(`outer:${event.key}`)}>
          <div testId="listener" style={box} onKeyDown={() => keys.push("listener")} onFocus={() => {}} onBlur={() => {}} />
          <div testId="stop" tabIndex={0} style={box} />
        </div>
      )
      renderer.focusElement(idOf("listener"))
      renderer.flush()
      expect(focused()).toBe(null)

      renderer.nativeSimulateClick(...[20, 10])
      expect(focused()).toBe(null)

      // A tab stop is the only one: Tab lands there, not on the listener.
      renderer.simulateKeystrokes("tab")
      expect(focused()).toBe("stop")
    })

    it("delivers a focused element's keys to ancestor key listeners", () => {
      const keys: string[] = []
      const { renderer, idOf } = setup(
        <div style={{ width: 200, height: 100 }} onKeyDown={(event) => keys.push(event.key)}>
          <div testId="child" tabIndex={-1} autoFocus style={box} />
        </div>
      )
      renderer.focusElement(idOf("child"))
      renderer.simulateKeystrokes("x")
      expect(keys).toEqual(["x"])
    })

    it("makes an element focusable by tabIndex -1 without a tab stop", () => {
      const { renderer, focused, idOf } = setup(
        <div style={{ width: 200, height: 100 }}>
          <div testId="a" tabIndex={0} style={box} />
          <div testId="skipped" tabIndex={-1} style={box} />
          <div testId="b" tabIndex={0} style={box} />
        </div>
      )
      renderer.focusElement(idOf("skipped"))
      expect(focused()).toBe("skipped")
      renderer.simulateKeystrokes("tab")
      expect(focused()).toBe("b")
      renderer.simulateKeystrokes("shift-tab")
      expect(focused()).toBe("a")
      renderer.simulateKeystrokes("shift-tab")
      renderer.simulateKeystrokes("shift-tab")
      expect(["a", "b"]).toContain(focused())
    })

    it("does not focus a disabled input", () => {
      const { renderer, focused, idOf } = setup(
        <div style={{ width: 200, height: 100 }}>
          <input testId="field" disabled style={{ width: 100, height: 30 }} />
          <div testId="stop" tabIndex={0} style={box} />
        </div>
      )
      renderer.focusElement(idOf("field"))
      expect(focused()).toBe(null)
      renderer.simulateKeystrokes("tab")
      expect(focused()).toBe("stop")
    })

    it("does not focus a disabled input that has a tabIndex", () => {
      const { renderer, focused, idOf, center } = setup(
        <div style={{ width: 200, height: 100 }}>
          <input testId="field" disabled tabIndex={0} style={{ width: 100, height: 30 }} />
          <textarea testId="area" disabled tabIndex={-1} style={{ width: 100, height: 30 }} />
          <div testId="stop" tabIndex={0} style={box} />
        </div>
      )
      renderer.focusElement(idOf("field"))
      renderer.focusElement(idOf("area"))
      expect(focused()).toBe(null)
      renderer.nativeSimulateClick(...center("field"))
      expect(focused()).toBe(null)
      renderer.simulateKeystrokes("tab")
      expect(focused()).toBe("stop")
    })

    it("ignores focus() on an element that is not focusable, now and later", () => {
      const scene = (tabIndex?: number) => (
        <div style={{ width: 200, height: 100 }}>
          <div testId="plain" tabIndex={tabIndex} style={box} />
        </div>
      )
      const { root, renderer, focused, idOf } = setup(scene())
      renderer.focusElement(idOf("plain"))
      expect(focused()).toBe(null)
      // It becoming focusable afterwards is not a reason to honor the old call.
      root.render(scene(0))
      renderer.flush()
      expect(focused()).toBe(null)
    })

    it("keeps a focus() on an element that has just been created", () => {
      const { root, renderer, focused } = setup(<div style={{ width: 200, height: 100 }} />)
      let node
      root.render(
        <div style={{ width: 200, height: 100 }}>
          <div ref={(value) => (node = value)} testId="late" tabIndex={0} style={box} />
        </div>
      )
      node.focus()
      renderer.flush()
      expect(focused()).toBe("late")
    })

    it("ignores a Tab with ctrl, alt or cmd held", () => {
      const { renderer, focused, idOf } = setup(
        <div style={{ width: 200, height: 100 }}>
          <div testId="a" tabIndex={0} style={box} />
          <div testId="b" tabIndex={0} style={box} />
        </div>
      )
      renderer.focusElement(idOf("a"))
      for (const stroke of ["ctrl-tab", "alt-tab", "cmd-tab"]) {
        renderer.simulateKeystrokes(stroke)
        expect(focused()).toBe("a")
      }
      renderer.simulateKeystrokes("tab")
      expect(focused()).toBe("b")
    })

    it("hands a modified Tab to the page's keydown listeners", () => {
      const keys: string[] = []
      const { renderer, idOf } = setup(
        <div style={{ width: 200, height: 100 }} onKeyDown={(event) => keys.push(`${event.ctrlKey}:${event.key}`)}>
          <div testId="a" tabIndex={0} style={box} />
        </div>
      )
      renderer.focusElement(idOf("a"))
      renderer.simulateKeystrokes("ctrl-tab")
      expect(keys).toEqual(["true:Tab"])
    })
  })

  describe("mouse press", () => {
    it("targets the topmost element, so a backdrop does not focus the input under it", () => {
      const { renderer, focused, idOf, center } = setup(
        <div style={{ width: 300, height: 200 }}>
          <input testId="field" style={{ width: 120, height: 40 }} />
          <div
            testId="backdrop"
            style={{ position: "absolute", top: 0, left: 0, width: 300, height: 200, backgroundColor: "#00000080" }}
          />
        </div>
      )
      renderer.focusElement(idOf("field"))
      expect(focused()).toBe("field")
      renderer.nativeSimulateClick(...center("field"))
      expect(focused()).toBe(null)
    })

    it("still focuses the focusable ancestor of the topmost element", () => {
      const { renderer, focused, center } = setup(
        <div style={{ width: 300, height: 200 }}>
          <div testId="holder" tabIndex={0} style={{ width: 120, height: 60 }}>
            <div testId="inner" style={{ width: 120, height: 60, backgroundColor: "#334155" }} />
          </div>
        </div>
      )
      renderer.nativeSimulateClick(...center("inner"))
      expect(focused()).toBe("holder")
    })
  })

  describe("tab order", () => {
    it("visits positive tabIndex ascending, then 0s in document order, skipping -1", () => {
      const { renderer, focused } = setup(
        <div style={{ width: 280, height: 100 }}>
          <div testId="zero-1" tabIndex={0} style={box} />
          <div testId="three" tabIndex={3} style={box} />
          <div testId="minus" tabIndex={-1} style={box} />
          <div testId="one" tabIndex={1} style={box} />
          <div testId="zero-2" tabIndex={0} style={box} />
          <div testId="two" tabIndex={2} style={box} />
          <div testId="one-again" tabIndex={1} style={box} />
        </div>
      )
      const order = []
      for (let step = 0; step < 8; step++) {
        renderer.simulateKeystrokes("tab")
        order.push(focused())
      }
      expect(order).toEqual([
        "one",
        "one-again",
        "two",
        "three",
        "zero-1",
        "zero-2",
        "one",
        "one-again",
      ])
    })

    it("visits a custom element that has a tabIndex, like any other host", () => {
      const { renderer, focused, idOf } = setup(
        <div style={{ width: 280, height: 100 }}>
          <div testId="before" tabIndex={0} style={box} />
          <img testId="image" tabIndex={0} style={box} />
          <div testId="after" tabIndex={0} style={box} />
        </div>
      )
      renderer.focusElement(idOf("image"))
      expect(focused()).toBe("image")
      renderer.focusElement(idOf("before"))
      renderer.simulateKeystrokes("tab")
      expect(focused()).toBe("image")
      renderer.simulateKeystrokes("tab")
      expect(focused()).toBe("after")
      renderer.simulateKeystrokes("shift-tab")
      expect(focused()).toBe("image")
    })

    it("follows a changed tabIndex", () => {
      function App({ index }) {
        return (
          <div style={{ width: 200, height: 100 }}>
            <div testId="first" tabIndex={0} style={box} />
            <div testId="second" tabIndex={index} style={box} />
          </div>
        )
      }
      const root = createTestRoot({ width: 300, height: 200 })
      root.render(<App index={0} />)
      const { renderer } = root
      const focused = () => renderer.getElement(renderer.getFocusedElementId())?.testId
      renderer.simulateKeystrokes("tab")
      expect(focused()).toBe("first")
      root.render(<App index={1} />)
      renderer.simulateKeystrokes("tab")
      expect(focused()).toBe("second")
      renderer.simulateKeystrokes("tab")
      expect(focused()).toBe("first")
    })
  })

  describe("events", () => {
    it("fires blur(old), then focus(new), with onBlur/onFocus bubbling like React DOM", () => {
      const log: string[] = []
      const { renderer, idOf } = setup(
        <div
          style={{ width: 200, height: 100 }}
          onFocus={() => log.push("parent:focus")}
          onBlur={() => log.push("parent:blur")}
        >
          <div
            testId="a"
            tabIndex={0}
            style={box}
            onFocus={() => log.push("a:focus")}
            onBlur={() => log.push("a:blur")}
          />
          <div
            testId="b"
            tabIndex={0}
            style={box}
            onFocus={() => log.push("b:focus")}
            onBlur={() => log.push("b:blur")}
          />
        </div>
      )
      renderer.focusElement(idOf("a"))
      expect(log).toEqual(["a:focus", "parent:focus"])
      log.length = 0
      renderer.focusElement(idOf("b"))
      expect(log).toEqual(["a:blur", "parent:blur", "b:focus", "parent:focus"])
    })

    describe("synchronously", () => {
      // The test wrapper drains the queue after each call, which hides whether
      // native sent an event synchronously or queued it, so go below it. On a
      // live window the queue is only read on a later turn of the event loop.
      function watch(renderer) {
        const sync: string[] = []
        const handler = renderer.dispatchSyncEvent
        renderer.native.setSyncEventHandler((event) => {
          sync.push(`${event.eventType}:${renderer.getElement(event.target)?.testId}`)
          return handler(event)
        })
        const queued = () =>
          renderer.native
            .drainEvents()
            .filter((event) => ["focus", "blur", "focusin", "focusout"].includes(event.eventType))
            .map((event) => event.eventType)
        return { sync, queued }
      }
      const scene = (
        <div testId="scope" style={{ width: 200, height: 100 }}>
          <div testId="a" tabIndex={0} style={box} onFocus={() => {}} />
          <div testId="b" tabIndex={0} style={box} onFocus={() => {}} onBlur={() => {}} />
        </div>
      )

      it("sends the events of focus() and blur() through the sync handler, not the queue", () => {
        const { renderer, idOf } = setup(scene)
        renderer.native.flush()
        const { sync, queued } = watch(renderer)
        renderer.native.focusElement(idOf("a"))
        expect(sync).toEqual(["focus:a", "focusin:a"])
        renderer.native.focusElement(idOf("b"))
        expect(sync.slice(2)).toEqual(["blur:a", "focusout:a", "focus:b", "focusin:b"])
        renderer.native.blurElement(idOf("b"))
        expect(sync.slice(6)).toEqual(["blur:b", "focusout:b"])
        expect(queued()).toEqual([])
      })

      it("sends the events of focusNext(), focusPrevious() and focusNextWithin() through the sync handler", () => {
        const { renderer, idOf } = setup(scene)
        renderer.native.flush()
        const { sync, queued } = watch(renderer)
        renderer.native.focusNext()
        expect(sync).toEqual(["focus:a", "focusin:a"])
        renderer.native.focusNext()
        expect(sync.slice(2)).toEqual(["blur:a", "focusout:a", "focus:b", "focusin:b"])
        renderer.native.focusPrevious()
        expect(sync.slice(6)).toEqual(["blur:b", "focusout:b", "focus:a", "focusin:a"])
        renderer.native.focusNextWithin(idOf("scope"))
        expect(sync.slice(10)).toEqual(["blur:a", "focusout:a", "focus:b", "focusin:b"])
        expect(queued()).toEqual([])
      })
    })

    describe("from a handler", () => {
      // The events fire inside focus(), so the handler that called it hears
      // them before its next line, as in the DOM.
      it("delivers focus events before focus() returns", () => {
        const log: string[] = []
        let target
        const { renderer, idOf } = setup(
          <div style={{ width: 200, height: 100 }}>
            <div
              testId="a"
              tabIndex={0}
              style={box}
              onBlur={() => log.push("a:blur")}
              onKeyDown={() => {
                log.push("keydown")
                target.focus()
                log.push("focus() returned")
              }}
            />
            <div
              ref={(value) => (target = value)}
              testId="b"
              tabIndex={0}
              style={box}
              onFocus={() => log.push("b:focus")}
            />
          </div>
        )
        renderer.focusElement(idOf("a"))
        log.length = 0
        renderer.nativeSimulateKeystrokes(idOf("a"), "x")
        expect(log).toEqual(["keydown", "a:blur", "b:focus", "focus() returned"])
      })

      it("delivers focus events before focusNext() returns", () => {
        const log: string[] = []
        const { renderer, idOf } = setup(
          <div style={{ width: 200, height: 100 }}>
            <div
              testId="a"
              tabIndex={0}
              style={box}
              onBlur={() => log.push("a:blur")}
              onKeyDown={(event) => {
                if (event.key !== "x") return
                renderer.focusNext()
                log.push("focusNext() returned")
              }}
            />
            <div testId="b" tabIndex={0} style={box} onFocus={() => log.push("b:focus")} />
          </div>
        )
        renderer.focusElement(idOf("a"))
        log.length = 0
        renderer.nativeSimulateKeystrokes(idOf("a"), "x")
        expect(log).toEqual(["a:blur", "b:focus", "focusNext() returned"])
      })

      it("lets a focus handler move focus again", () => {
        const log: string[] = []
        let other
        const { renderer, focused, idOf } = setup(
          <div style={{ width: 200, height: 100 }}>
            <div testId="a" tabIndex={0} style={box} onKeyDown={() => renderer.focusElement(idOf("b"))} />
            <div
              testId="b"
              tabIndex={0}
              style={box}
              onFocus={() => {
                log.push("b:focus")
                other.focus()
              }}
              onBlur={() => log.push("b:blur")}
            />
            <div ref={(value) => (other = value)} testId="c" tabIndex={0} style={box} onFocus={() => log.push("c:focus")} />
          </div>
        )
        renderer.focusElement(idOf("a"))
        renderer.nativeSimulateKeystrokes(idOf("a"), "x")
        expect(focused()).toBe("c")
        expect(log).toEqual(["b:focus", "b:blur", "c:focus"])
      })
    })

    it("names onFocus and onBlur events by their lowercase DOM types, focusin and focusout", () => {
      const types: string[] = []
      const record = (event) => types.push(event.type)
      const { renderer, idOf } = setup(
        <div style={{ width: 200, height: 100 }}>
          <div testId="a" tabIndex={0} style={box} onFocus={record} onBlur={record} />
          <div testId="b" tabIndex={0} style={box} />
        </div>
      )
      renderer.focusElement(idOf("a"))
      renderer.focusElement(idOf("b"))
      expect(types).toEqual(["focusin", "focusout"])
    })

    it("orders the events by focus change, not by which element was created first", () => {
      // The listener on the later element is created first: a per-element
      // gpui subscription would fire focus(b) before blur(a).
      const order: string[] = []
      const { renderer, idOf } = setup(
        <div style={{ width: 200, height: 100 }}>
          <div testId="b" tabIndex={0} style={box} onFocus={() => order.push("b:focus")} onBlur={() => order.push("b:blur")} />
          <div testId="a" tabIndex={0} style={box} onFocus={() => order.push("a:focus")} onBlur={() => order.push("a:blur")} />
        </div>
      )
      renderer.focusElement(idOf("a"))
      renderer.focusElement(idOf("b"))
      renderer.focusElement(idOf("a"))
      expect(order).toEqual(["a:focus", "a:blur", "b:focus", "b:blur", "a:focus"])
    })

    it("reports focusVisible on the event", () => {
      const seen: boolean[] = []
      const { renderer, idOf } = setup(
        <div style={{ width: 200, height: 100 }}>
          <div testId="a" tabIndex={0} style={box} onFocus={(event) => seen.push(event.focusVisible)} />
          <div testId="b" tabIndex={0} style={box} onFocus={(event) => seen.push(event.focusVisible)} />
        </div>
      )
      // Script focus before any input matches, like Chrome.
      renderer.focusElement(idOf("a"))
      renderer.simulateKeystrokes("tab")
      expect(seen).toEqual([true, true])
    })

    it("does not report focusVisible for script focus once the mouse was used", () => {
      const seen: boolean[] = []
      const { renderer, idOf, center } = setup(
        <div style={{ width: 200, height: 100 }}>
          <div testId="a" tabIndex={0} style={box} onFocus={(event) => seen.push(event.focusVisible)} />
          <div testId="b" tabIndex={0} style={box} onFocus={(event) => seen.push(event.focusVisible)} />
        </div>
      )
      renderer.nativeSimulateClick(...center("a"))
      renderer.focusElement(idOf("b"))
      expect(seen).toEqual([false, false])
    })
  })

  describe("focus-visible", () => {
    const scene = (
      <div style={{ width: 280, height: 100 }}>
        <div testId="button" tabIndex={0} style={box} />
        <div testId="other" tabIndex={0} style={box} />
        <input testId="field" style={{ width: 80, height: 30 }} />
      </div>
    )

    it("is false after a mouse press and true after Tab", () => {
      const { renderer, idOf, center } = setup(scene)
      renderer.nativeSimulateClick(...center("button"))
      expect(renderer.matchesFocus(idOf("button"), ":focus")).toBe(true)
      expect(renderer.matchesFocus(idOf("button"), ":focus-visible")).toBe(false)

      renderer.simulateKeystrokes("tab")
      expect(renderer.matchesFocus(idOf("other"), ":focus-visible")).toBe(true)
      expect(renderer.matchesFocus(idOf("button"), ":focus")).toBe(false)
    })

    it("is always true for a text input, even after a click", () => {
      const { renderer, idOf, center } = setup(scene)
      renderer.nativeSimulateClick(...center("field"))
      expect(renderer.matchesFocus(idOf("field"), ":focus-visible")).toBe(true)
    })

    it("makes a mouse-focused element visible when a key goes down in it", () => {
      const { renderer, idOf, center } = setup(scene)
      renderer.nativeSimulateClick(...center("button"))
      expect(renderer.matchesFocus(idOf("button"), ":focus-visible")).toBe(false)
      renderer.simulateKeystrokes("x")
      expect(renderer.matchesFocus(idOf("button"), ":focus-visible")).toBe(true)
    })

    it("lets a script move focus-visible along, and not invent it", () => {
      const { renderer, idOf, center } = setup(scene)
      renderer.nativeSimulateClick(...center("button"))
      renderer.focusElement(idOf("other"))
      expect(renderer.matchesFocus(idOf("other"), ":focus-visible")).toBe(false)

      renderer.simulateKeystrokes("tab")
      renderer.focusElement(idOf("button"))
      expect(renderer.matchesFocus(idOf("button"), ":focus-visible")).toBe(true)
    })

    it("keeps focus-visible across the window going inactive and back", () => {
      const { renderer, idOf, focused, center } = setup(scene)
      renderer.simulateKeystrokes("tab")
      const target = focused()
      // A press on the focused element leaves its focus-visible, but the last
      // input is the mouse's, so only the remembered state can bring it back.
      renderer.nativeSimulateClick(...center(target))
      expect(renderer.matchesFocus(idOf(target), ":focus-visible")).toBe(true)
      renderer.setWindowActive(false)
      expect(renderer.matchesFocus(idOf(target), ":focus")).toBe(false)
      renderer.setWindowActive(true)
      expect(focused()).toBe(target)
      expect(renderer.matchesFocus(idOf(target), ":focus-visible")).toBe(true)
    })

    it("does not make a mouse focus visible by going inactive and back", () => {
      const { renderer, idOf, center } = setup(scene)
      renderer.nativeSimulateClick(...center("button"))
      renderer.setWindowActive(false)
      renderer.setWindowActive(true)
      expect(renderer.matchesFocus(idOf("button"), ":focus")).toBe(true)
      expect(renderer.matchesFocus(idOf("button"), ":focus-visible")).toBe(false)
    })

    it("keeps autoFocus programmatic when it was issued while the window was inactive", () => {
      const root = createTestRoot({ width: 300, height: 200 })
      const { renderer } = root
      renderer.setWindowActive(false)
      root.render(
        <div style={{ width: 280, height: 100 }}>
          <div testId="auto" tabIndex={0} autoFocus style={box} />
        </div>
      )
      renderer.flush()
      renderer.setWindowActive(true)
      const id = renderer.findByTestId("auto").id
      // No key or mouse yet, so a script focus is visible (Chrome).
      expect(renderer.matchesFocus(id, ":focus")).toBe(true)
      expect(renderer.matchesFocus(id, ":focus-visible")).toBe(true)
    })

    it("keeps a focus() made while inactive programmatic until the window returns", () => {
      const { renderer, idOf } = setup(scene)
      renderer.simulateKeystrokes("tab")
      renderer.setWindowActive(false)
      renderer.focusElement(idOf("other"))
      renderer.setWindowActive(true)
      expect(renderer.matchesFocus(idOf("other"), ":focus")).toBe(true)
      expect(renderer.matchesFocus(idOf("other"), ":focus-visible")).toBe(true)
    })

    it("styles focusVisible from that state, not from the last input", () => {
      const file = (name: string) => `${SHOTS_DIR}/focus-visible-${name}.png`
      const capture = (renderer, name: string) => {
        renderer.captureScreenshot(file(name))
        return fs.readFileSync(file(name))
      }
      const ring = { focusVisible: { backgroundColor: "#ff0000" } }
      const frame = (
        <div style={{ width: 280, height: 100, backgroundColor: "#101010" }}>
          <input testId="field" style={{ width: 120, height: 40, backgroundColor: "#303030", ...ring }} />
          <div testId="button" tabIndex={0} style={{ width: 60, height: 40, backgroundColor: "#303030", ...ring }} />
        </div>
      )
      const idle = setup(frame)
      const idlePng = capture(idle.renderer, "idle")

      const fieldClick = setup(frame)
      fieldClick.renderer.nativeSimulateClick(...fieldClick.center("field"))
      const fieldPng = capture(fieldClick.renderer, "field-click")

      const buttonClick = setup(frame)
      buttonClick.renderer.nativeSimulateClick(...buttonClick.center("button"))
      const clickPng = capture(buttonClick.renderer, "button-click")

      const buttonTab = setup(frame)
      buttonTab.renderer.simulateKeystrokes("tab tab")
      const tabPng = capture(buttonTab.renderer, "button-tab")

      expect(clickPng.equals(idlePng)).toBe(true)
      if (!isCI) {
        expect(fieldPng.equals(idlePng)).toBe(false)
        expect(tabPng.equals(idlePng)).toBe(false)
      }
    })
  })

  describe("custom element focus styles", () => {
    it("paints focusVisible on an img with a tabIndex", () => {
      const file = (name: string) => `${SHOTS_DIR}/focus-img-${name}.png`
      const capture = (renderer, name: string) => {
        renderer.captureScreenshot(file(name))
        return fs.readFileSync(file(name))
      }
      const frame = (
        <div style={{ width: 280, height: 100, backgroundColor: "#101010" }}>
          <img
            testId="image"
            tabIndex={0}
            style={{ width: 60, height: 40, backgroundColor: "#303030", focusVisible: { backgroundColor: "#ff0000" } }}
          />
        </div>
      )
      const { renderer } = setup(frame)
      const idle = capture(renderer, "idle")
      renderer.simulateKeystrokes("tab")
      const tabbed = capture(renderer, "tabbed")
      if (!isCI) expect(tabbed.equals(idle)).toBe(false)
      expect(renderer.matchesFocus(renderer.findByTestId("image").id, ":focus-visible")).toBe(true)
    })
  })

  describe("focus-within", () => {
    it("matches the focused element and every ancestor", () => {
      const { renderer, idOf } = setup(
        <div testId="outer" style={{ width: 200, height: 100 }}>
          <div testId="inner" style={{ width: 100, height: 50 }}>
            <div testId="leaf" tabIndex={0} style={box} />
          </div>
          <div testId="sibling" style={box} />
        </div>
      )
      renderer.focusElement(idOf("leaf"))
      const within = (testId: string) => renderer.matchesFocus(idOf(testId), ":focus-within")
      expect([within("leaf"), within("inner"), within("outer"), within("sibling")]).toEqual([
        true,
        true,
        true,
        false,
      ])
    })

    it("applies the focusWithin style to a non-focusable ancestor", () => {
      const file = (name: string) => `${SHOTS_DIR}/focus-within-${name}.png`
      const frame = (
        <div style={{ width: 280, height: 100, backgroundColor: "#101010" }}>
          <div
            testId="group"
            style={{ width: 120, height: 60, backgroundColor: "#303030", focusWithin: { backgroundColor: "#ff0000" } }}
          >
            <div testId="leaf" tabIndex={0} style={box} />
          </div>
        </div>
      )
      const { renderer, idOf } = setup(frame)
      renderer.captureScreenshot(file("before"))
      const before = fs.readFileSync(file("before"))
      renderer.focusElement(idOf("leaf"))
      renderer.captureScreenshot(file("during"))
      const during = fs.readFileSync(file("during"))
      renderer.blurElement(idOf("leaf"))
      renderer.captureScreenshot(file("after"))
      const after = fs.readFileSync(file("after"))
      if (!isCI) expect(during.equals(before)).toBe(false)
      expect(after.equals(before)).toBe(true)
    })
  })

  describe("element methods", () => {
    it("focuses, blurs and matches through a ref", () => {
      let node
      const log: string[] = []
      const { renderer, focused } = setup(
        <div style={{ width: 200, height: 100 }}>
          <div
            ref={(value) => (node = value)}
            testId="target"
            tabIndex={0}
            style={box}
            onFocus={() => log.push("focus")}
            onBlur={() => log.push("blur")}
          />
        </div>
      )
      expect(node.matches(":focus")).toBe(false)
      node.focus()
      expect(focused()).toBe("target")
      expect(node.matches(":focus")).toBe(true)
      expect(node.matches(":focus-within")).toBe(true)
      expect(node.matches(":focus-visible")).toBe(true)
      node.blur()
      expect(focused()).toBe(null)
      expect(node.matches(":focus")).toBe(false)
      expect(log).toEqual(["focus", "blur"])
      expect(() => node.matches(":visited")).toThrow(/:focus/)
      expect(() => node.matches("div")).toThrow()
    })

    it("blur() is a no-op on an element that is not focused", () => {
      let a
      let b
      const { renderer, focused, idOf } = setup(
        <div style={{ width: 200, height: 100 }}>
          <div ref={(value) => (a = value)} testId="a" tabIndex={0} style={box} />
          <div ref={(value) => (b = value)} testId="b" tabIndex={0} style={box} />
        </div>
      )
      renderer.focusElement(idOf("a"))
      b.blur()
      expect(focused()).toBe("a")
      expect(a.matches(":focus")).toBe(true)
    })

    it("scrolls into view unless preventScroll is set", () => {
      let near
      let far
      const { renderer, idOf } = setup(
        <div testId="scroller" style={{ width: 200, height: 60, overflow: "scroll" }}>
          <div ref={(value) => (near = value)} tabIndex={0} testId="near" style={{ width: 100, height: 40 }} />
          <div style={{ width: 100, height: 300 }} />
          <div ref={(value) => (far = value)} tabIndex={0} testId="far" style={{ width: 100, height: 40 }} />
        </div>
      )
      const scrollTop = () => renderer.getScrollOffset(idOf("scroller"))?.[1] ?? 0
      far.focus({ preventScroll: true })
      expect(scrollTop()).toBe(0)
      near.focus()
      far.focus()
      expect(scrollTop()).toBeLessThan(0)
    })
  })
})
