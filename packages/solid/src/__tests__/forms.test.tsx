import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { HostElement } from "../host.js"

const field = { width: 200, height: 30 }

function setup(view: () => any) {
  const app = createTestRoot({ width: 400, height: 400 })
  app.render(view)
  const renderer = app.renderer
  const idOf = (testId: string) => renderer.findByTestId(testId)!.id
  const focused = () => renderer.getElement(renderer.getFocusedElementId()!)?.testId ?? null
  const type = (testId: string, keys: string) =>
    renderer.nativeSimulateKeystrokes(idOf(testId), keys)
  return { renderer, idOf, focused, type }
}

describe.skipIf(!hasNativeTestRenderer)("Solid form", () => {
  it("submits on Enter in an input, with its FormData", () => {
    const submits: Array<{ entries: [string, FormDataEntryValue][]; target: number; submitter: unknown }> = []
    const { type, idOf } = setup(() => (
      <form
        testId="form"
        onSubmit={(event) =>
          submits.push({
            entries: Array.from(event.formData.entries()),
            target: event.target,
            submitter: event.submitter,
          })
        }
      >
        <input testId="a" name="first" style={field} />
        <div>
          <textarea testId="b" name="notes" value="hi" style={field} />
        </div>
        <textarea testId="c" value="unnamed" style={field} />
      </form>
    ))
    type("a", "j o")
    expect(submits).toEqual([])
    type("a", "Enter")
    expect(submits).toEqual([
      {
        entries: [
          ["first", "jo"],
          ["notes", "hi"],
        ],
        target: idOf("form"),
        submitter: null,
      },
    ])
  })

  it("blocks on a required empty control, fires invalid and focuses it", () => {
    const submits: number[] = []
    const invalids: string[] = []
    let form!: HostElement
    const { type, focused, idOf } = setup(() => (
      <form ref={form} onSubmit={() => submits.push(1)} onInvalid={() => invalids.push("form")}>
        <input testId="a" name="a" style={field} />
        <input
          testId="b"
          name="b"
          required
          style={field}
          onInvalid={(event) => invalids.push(`b:${event.target === idOf("b")}`)}
        />
      </form>
    ))
    form.requestSubmit()
    expect(submits).toEqual([])
    expect(invalids).toEqual(["b:true"])
    expect(focused()).toBe("b")
    type("b", "x")
    form.requestSubmit()
    expect(submits).toEqual([1])
  })

  it("validates on Enter in the input: invalid at it, focus on it, no submit", () => {
    const submits: number[] = []
    const invalids: string[] = []
    const { type, focused, idOf } = setup(() => (
      <form onSubmit={() => submits.push(1)} onInvalid={() => invalids.push("form")}>
        <input
          testId="a"
          name="a"
          required
          style={field}
          onInvalid={(event) => invalids.push(`a:${event.target === idOf("a")}`)}
        />
      </form>
    ))
    type("a", "Enter")
    expect(submits).toEqual([])
    expect(invalids).toEqual(["a:true"])
    expect(focused()).toBe("a")
  })

  it("moves focus on Enter to the first invalid control, which may be a textarea", () => {
    const submits: number[] = []
    const invalids: string[] = []
    const { type, focused, idOf } = setup(() => (
      <form onSubmit={() => submits.push(1)}>
        <textarea
          testId="area"
          name="notes"
          required
          style={field}
          onInvalid={(event) => invalids.push(`area:${event.target === idOf("area")}`)}
        />
        <input testId="a" name="a" style={field} onInvalid={() => invalids.push("a")} />
      </form>
    ))
    type("a", "Enter")
    expect(submits).toEqual([])
    expect(invalids).toEqual(["area:true"])
    expect(focused()).toBe("area")
  })

  it("requestSubmit() validates, then submits", () => {
    let form!: HostElement
    const submits: string[][][] = []
    const [required, setRequired] = createSignal(true)
    const { type, focused } = setup(() => (
      <form
        ref={form}
        onSubmit={(event) => submits.push(Array.from(event.formData.entries()) as string[][])}
      >
        <input testId="a" name="a" required={required()} style={field} />
      </form>
    ))
    form.requestSubmit()
    expect(submits).toEqual([])
    expect(focused()).toBe("a")
    setRequired(false)
    form.requestSubmit()
    expect(submits).toEqual([[["a", ""]]])
    type("a", "z")
    form.requestSubmit()
    expect(submits.at(-1)).toEqual([["a", "z"]])
  })

  it("requestSubmit() is only valid on a form", () => {
    let box!: HostElement
    setup(() => <div ref={box} style={field} />)
    expect(() => box.requestSubmit()).toThrow("only valid on <form>")
  })

  it("inserts a newline for Enter in a textarea and does not submit", () => {
    const submits: number[] = []
    const [text, setText] = createSignal("ab")
    const { type } = setup(() => (
      <form onSubmit={() => submits.push(1)}>
        <textarea
          testId="area"
          value={text()}
          style={{ width: 200, height: 80 }}
          onChange={(event) => setText(event.value ?? "")}
        />
      </form>
    ))
    type("area", "End Enter")
    expect(submits).toEqual([])
    expect(text()).toBe("ab\n")
  })

  it("does not submit on Enter when the form has two inputs and no submit button", () => {
    const submits: number[] = []
    const { type } = setup(() => (
      <form onSubmit={() => submits.push(1)}>
        <input testId="a" name="a" style={field} />
        <input testId="b" name="b" style={field} />
      </form>
    ))
    type("a", "Enter")
    type("b", "Enter")
    expect(submits).toEqual([])
  })

  it("does not submit when keydown is prevented", () => {
    const submits: number[] = []
    const { type } = setup(() => (
      <form onSubmit={() => submits.push(1)} onKeyDown={(event) => event.preventDefault()}>
        <input testId="a" name="a" style={field} />
      </form>
    ))
    type("a", "Enter")
    expect(submits).toEqual([])
  })
})
