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
        <textarea testId="c" name="third" value="x" onChange={() => {}} style={field} />
      </>,
    )
    type("a", "j o")
    expect(submits).toEqual([])
    type("a", "Enter")
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
    formRef.current.requestSubmit()
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
    type("a", "Enter")
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
    type("a", "Enter")
    expect(submits).toEqual([])
    keyDownPrevented = false
    type("a", "Enter")
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
    type("area", "End Enter")
    expect(submits).toEqual([])
    expect(testRoot.renderer.getAllText()).toContain(JSON.stringify("ab\n"))
  })

  describe("implicit submission without a submit button", () => {
    // gpuix has no submit button, so a form has no default button: HTML submits
    // on Enter only when at most one field blocks implicit submission.
    it("does not submit on Enter when the form has two inputs", () => {
      mountForm(
        <>
          <input testId="a" name="a" style={field} />
          <input testId="b" name="b" style={field} />
        </>,
      )
      type("a", "Enter")
      type("b", "Enter")
      expect(submits).toEqual([])
    })

    it("counts a disabled input, as HTML does", () => {
      mountForm(
        <>
          <input testId="a" name="a" style={field} />
          <input testId="b" name="b" disabled style={field} />
        </>,
      )
      type("a", "Enter")
      expect(submits).toEqual([])
    })

    it("still submits through requestSubmit()", () => {
      mountForm(
        <>
          <input testId="a" name="a" value="1" onChange={() => {}} style={field} />
          <input testId="b" name="b" value="2" onChange={() => {}} style={field} />
        </>,
      )
      formRef.current.requestSubmit()
      expect(submits.map((submit) => submit.entries)).toEqual([
        [
          ["a", "1"],
          ["b", "2"],
        ],
      ])
    })

    it("does not count textareas, or the inputs of a nested form", () => {
      mountForm(
        <>
          <input testId="a" name="a" style={field} />
          <textarea testId="area" name="notes" style={field} />
          <form testId="inner">
            <input testId="nested" name="nested" style={field} />
          </form>
        </>,
      )
      type("a", "Enter")
      expect(submits.map((submit) => submit.target)).toEqual([idOf("form")])
    })
  })

  it("does nothing for an input outside a form", () => {
    testRoot.render(
      <div style={{ width: 400, height: 200 }} onSubmit={() => submits.push({})}>
        <form onSubmit={() => submits.push({})} />
        <input testId="a" style={field} />
      </div>,
    )
    type("a", "Enter")
    expect(submits).toEqual([])
  })

  it("runs the input's Enter keydown handler, then submits the form", () => {
    const own: string[] = []
    mountForm(
      <input
        testId="a"
        name="n"
        onKeyDown={(event) => {
          if (event.key === "Enter") own.push("input")
        }}
        style={field}
      />,
    )
    type("a", "Enter")
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
      formRef.current.requestSubmit()
      expect(submits).toEqual([])
      // invalid does not bubble, so the form never hears it.
      expect(invalids).toEqual([`b:${idOf("b")}`, `c:${idOf("c")}`])
      expect(focusedTestId()).toBe("b")
    })

    it("validates on Enter in the input: invalid at it, focus on it, no submit", () => {
      mountForm(
        <input
          testId="a"
          name="a"
          required
          style={field}
          onInvalid={(event) => invalids.push(`a:${event.target}`)}
        />,
      )
      type("a", "Enter")
      expect(submits).toEqual([])
      expect(invalids).toEqual([`a:${idOf("a")}`])
      expect(focusedTestId()).toBe("a")
    })

    it("moves focus on Enter to the first invalid control, which may be a textarea", () => {
      mountForm(
        <>
          <textarea
            testId="area"
            name="notes"
            required
            style={field}
            onInvalid={(event) => invalids.push(`area:${event.target}`)}
          />
          <input
            testId="a"
            name="a"
            style={field}
            onInvalid={(event) => invalids.push(`a:${event.target}`)}
          />
        </>,
      )
      type("a", "Enter")
      expect(submits).toEqual([])
      expect(invalids).toEqual([`area:${idOf("area")}`])
      expect(focusedTestId()).toBe("area")
    })

    it("submits once the required controls have a value", () => {
      mountForm(<input testId="a" name="a" required style={field} />)
      type("a", "Enter")
      expect(submits).toEqual([])
      type("a", "x Enter")
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
      formRef.current.requestSubmit()
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
      formRef.current.requestSubmit()
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
      type("button", "Space")
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
    type("inner", "Enter")
    expect(inner).toEqual([[["inner", "2"]]])
    expect(outer).toEqual([])
    type("outer", "Enter")
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
