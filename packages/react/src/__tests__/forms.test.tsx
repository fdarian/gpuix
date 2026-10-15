/**
 * `<form>`: implicit submission from an `<input>`, validation of `required`
 * controls, `FormData`, and `requestSubmit()`.
 */
// @ts-nocheck

import React, { useRef, useState } from "react"
import { beforeEach, describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const field = { width: 200, height: 30 }

describeNative("form", () => {
  let testRoot: ReturnType<typeof createTestRoot>
  let submits: Array<{ entries: string[][]; target: number; submitter: unknown }>
  let invalids: string[]
  let keyDownPrevented: boolean
  let formRef: { current: { requestSubmit(): void } }

  beforeEach(() => {
    testRoot = createTestRoot()
    submits = []
    invalids = []
    keyDownPrevented = false
    formRef = { current: null }
  })

  const focusedTestId = () =>
    testRoot.renderer.getElement(testRoot.renderer.getFocusedElementId())?.testId ?? null
  const idOf = (testId: string) => testRoot.renderer.findByTestId(testId).id
  const type = (testId: string, keys: string) =>
    testRoot.renderer.nativeSimulateKeystrokes(idOf(testId), keys)

  function mountForm(children: React.ReactNode, extra: { onWrapperSubmit?: Function } = {}) {
    testRoot.render(
      <div style={{ width: 400, height: 400 }} onSubmit={extra.onWrapperSubmit}>
        <form
          ref={formRef}
          testId="form"
          onSubmit={(event) => {
            submits.push({
              entries: Array.from(event.formData.entries()),
              target: event.target,
              submitter: event.submitter,
            })
          }}
          onInvalid={() => invalids.push("form")}
        >
          {children}
        </form>
      </div>,
    )
  }

  it("submits the form on Enter in an input, with its FormData", () => {
    mountForm(
      <>
        <input testId="a" name="first" style={field} />
        <div>
          <textarea testId="b" name="notes" value="hi" onChange={() => {}} style={field} />
        </div>
        <input testId="c" name="third" value="x" onChange={() => {}} style={field} />
      </>,
    )
    type("a", "j o")
    expect(submits).toEqual([])
    type("a", "enter")
    expect(submits).toEqual([
      {
        entries: [
          ["first", "jo"],
          ["notes", "hi"],
          ["third", "x"],
        ],
        target: idOf("form"),
        submitter: null,
      },
    ])
  })

  it("leaves out controls with no name and disabled controls", () => {
    mountForm(
      <>
        <input testId="a" name="kept" value="1" onChange={() => {}} style={field} />
        <input testId="b" value="2" onChange={() => {}} style={field} />
        <input testId="c" name="off" disabled value="3" onChange={() => {}} style={field} />
        <input testId="d" name="" value="4" onChange={() => {}} style={field} />
        <input testId="e" name="ro" readOnly value="5" onChange={() => {}} style={field} />
      </>,
    )
    type("a", "enter")
    expect(submits[0].entries).toEqual([
      ["kept", "1"],
      ["ro", "5"],
    ])
  })

  it("bubbles submit to the ancestors of the form", () => {
    const heard: number[] = []
    mountForm(<input testId="a" name="n" style={field} />, {
      onWrapperSubmit: (event) => heard.push(event.target),
    })
    type("a", "enter")
    expect(heard).toEqual([idOf("form")])
  })

  it("does not submit when keydown is prevented", () => {
    testRoot.render(
      <form
        onSubmit={() => submits.push({})}
        onKeyDown={(event) => {
          if (keyDownPrevented) event.preventDefault()
        }}
      >
        <input testId="a" name="n" style={field} />
      </form>,
    )
    keyDownPrevented = true
    type("a", "enter")
    expect(submits).toEqual([])
    keyDownPrevented = false
    type("a", "enter")
    expect(submits).toHaveLength(1)
  })

  it("inserts a newline for Enter in a textarea and does not submit", () => {
    function App() {
      const [text, setText] = useState("ab")
      return (
        <form onSubmit={() => submits.push({})}>
          <textarea
            testId="area"
            name="notes"
            value={text}
            style={{ width: 200, height: 80 }}
            onChange={(event) => setText(event.value)}
          />
          <text testId="out">{JSON.stringify(text)}</text>
        </form>
      )
    }
    testRoot.render(<App />)
    type("area", "end enter")
    expect(submits).toEqual([])
    expect(testRoot.renderer.getAllText()).toContain(JSON.stringify("ab\n"))
  })

  it("does nothing for an input outside a form, and keeps the input's own onSubmit", () => {
    const own: string[] = []
    testRoot.render(
      <div style={{ width: 400, height: 200 }} onSubmit={() => submits.push({})}>
        <form onSubmit={() => submits.push({})} />
        <input testId="a" onSubmit={() => own.push("input")} style={field} />
      </div>,
    )
    type("a", "enter")
    expect(own).toEqual(["input"])
    expect(submits).toEqual([])
  })

  it("submits both the input's onSubmit and the form's", () => {
    const own: string[] = []
    mountForm(<input testId="a" name="n" onSubmit={() => own.push("input")} style={field} />)
    type("a", "enter")
    expect(own).toEqual(["input"])
    expect(submits).toHaveLength(1)
  })

  describe("required", () => {
    it("blocks submission, fires invalid on each empty control, and focuses the first", () => {
      mountForm(
        <>
          <input testId="a" name="a" style={field} onChange={() => {}} value="ok" />
          <input
            testId="b"
            name="b"
            required
            style={field}
            onInvalid={(event) => invalids.push(`b:${event.target}`)}
          />
          <input
            testId="c"
            name="c"
            required
            style={field}
            onInvalid={(event) => invalids.push(`c:${event.target}`)}
          />
        </>,
      )
      type("a", "enter")
      expect(submits).toEqual([])
      // invalid does not bubble, so the form never hears it.
      expect(invalids).toEqual([`b:${idOf("b")}`, `c:${idOf("c")}`])
      expect(focusedTestId()).toBe("b")
    })

    it("submits once the required controls have a value", () => {
      mountForm(<input testId="a" name="a" required style={field} />)
      type("a", "enter")
      expect(submits).toEqual([])
      type("a", "x enter")
      expect(submits.map((submit) => submit.entries)).toEqual([[["a", "x"]]])
    })

    it("does not focus a control whose invalid event was cancelled", () => {
      mountForm(
        <>
          <input
            testId="a"
            name="a"
            required
            style={field}
            onInvalid={(event) => event.preventDefault()}
          />
          <input testId="b" name="b" required style={field} />
        </>,
      )
      testRoot.renderer.focusElement(idOf("a"))
      testRoot.renderer.simulateKeystrokes("enter")
      expect(submits).toEqual([])
      expect(focusedTestId()).toBe("b")
    })

    it("does not validate a disabled or read-only control", () => {
      mountForm(
        <>
          <input testId="a" name="a" required disabled style={field} />
          <input testId="b" name="b" required readOnly style={field} />
          <input testId="c" name="c" style={field} />
        </>,
      )
      type("c", "enter")
      expect(submits).toHaveLength(1)
    })
  })

  describe("requestSubmit()", () => {
    it("validates, then submits", () => {
      mountForm(<input testId="a" name="a" required style={field} />)
      formRef.current.requestSubmit()
      expect(submits).toEqual([])
      expect(focusedTestId()).toBe("a")
      type("a", "y")
      formRef.current.requestSubmit()
      expect(submits.map((submit) => submit.entries)).toEqual([[["a", "y"]]])
    })

    it("can be called from an event handler", () => {
      function App() {
        const ref = useRef()
        return (
          <form ref={ref} onSubmit={() => submits.push({})}>
            <input testId="a" name="a" style={field} />
            <div
              testId="button"
              tabIndex={0}
              style={{ width: 40, height: 20 }}
              onKeyDown={() => ref.current.requestSubmit()}
            />
          </form>
        )
      }
      testRoot.render(<App />)
      type("button", "space")
      expect(submits).toHaveLength(1)
    })
  })

  it("belongs a control to its nearest form", () => {
    const outer: string[][][] = []
    const inner: string[][][] = []
    testRoot.render(
      <form
        testId="outerForm"
        onSubmit={(event) => {
          if (event.target === idOf("outerForm")) outer.push(Array.from(event.formData.entries()))
        }}
      >
        <input testId="outer" name="outer" value="1" onChange={() => {}} style={field} />
        <form onSubmit={(event) => inner.push(Array.from(event.formData.entries()))}>
          <input testId="inner" name="inner" value="2" onChange={() => {}} style={field} />
        </form>
      </form>,
    )
    type("inner", "enter")
    expect(inner).toEqual([[["inner", "2"]]])
    expect(outer).toEqual([])
    type("outer", "enter")
    expect(outer).toEqual([[["outer", "1"]]])
  })

  it("is a Form in the accessibility tree", () => {
    mountForm(<input testId="a" name="a" style={field} />)
    const roles = Object.values(testRoot.renderer.getA11yTree().nodes ?? {}).map(
      (node) => node.aria?.role,
    )
    expect(roles).toContain("Form")
  })
})
