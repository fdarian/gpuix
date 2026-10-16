import { describe, expect, it } from "bun:test"
import { document } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { HostElement } from "../host.js"

const box = { width: 40, height: 20 }

function setup(view: () => any) {
  const app = createTestRoot({ width: 300, height: 200 })
  app.render(view)
  const renderer = app.renderer
  const focused = () => renderer.getElement(renderer.getFocusedElementId()!)?.testId ?? null
  const idOf = (testId: string) => renderer.findByTestId(testId)!.id
  const center = (testId: string): [number, number] => {
    const bounds = renderer.getElementBounds(idOf(testId))!
    return [bounds.x + bounds.width / 2, bounds.y + bounds.height / 2]
  }
  return { app, renderer, focused, idOf, center }
}

describe.skipIf(!hasNativeTestRenderer)("Solid focus model", () => {
  it("fires blur, focusout, focus, focusin in that order", () => {
    const log: string[] = []
    const listen = (name: string) => ({
      onFocus: () => log.push(`${name}:focus`),
      onBlur: () => log.push(`${name}:blur`),
      onFocusIn: () => log.push(`${name}:focusin`),
      onFocusOut: () => log.push(`${name}:focusout`),
    })
    // `b` is created first: a gpui subscription per element would fire its
    // focus listeners ahead of a's blur.
    const { renderer, idOf } = setup(() => (
      <div style={{ width: 200, height: 100 }}>
        <div testId="b" tabIndex={0} style={box} {...listen("b")} />
        <div testId="a" tabIndex={0} style={box} {...listen("a")} />
      </div>
    ))
    renderer.focusElement(idOf("a"))
    expect(log).toEqual(["a:focus", "a:focusin"])
    log.length = 0
    renderer.focusElement(idOf("b"))
    expect(log).toEqual(["a:blur", "a:focusout", "b:focus", "b:focusin"])
  })

  it("names focus and key events by their lowercase DOM types", () => {
    const types: string[] = []
    const record = (event: { type: string }) => types.push(event.type)
    const { renderer, idOf } = setup(() => (
      <div style={{ width: 200, height: 100 }}>
        <div
          testId="a"
          tabIndex={0}
          style={box}
          onFocus={record}
          onBlur={record}
          onFocusIn={record}
          onFocusOut={record}
          onKeyDown={record}
          onKeyUp={record}
        />
        <div testId="b" tabIndex={0} style={box} />
      </div>
    ))
    renderer.focusElement(idOf("a"))
    renderer.nativeSimulateKeystrokes(idOf("a"), "x")
    renderer.focusElement(idOf("b"))
    expect(types).toEqual(["focus", "focusin", "keydown", "keyup", "blur", "focusout"])
  })

  it("bubbles focusin and focusout but not focus and blur", () => {
    const log: string[] = []
    const { renderer, idOf } = setup(() => (
      <div
        style={{ width: 200, height: 100 }}
        onFocus={() => log.push("parent:focus")}
        onBlur={() => log.push("parent:blur")}
        onFocusIn={() => log.push("parent:focusin")}
        onFocusOut={() => log.push("parent:focusout")}
      >
        <div testId="a" tabIndex={0} style={box} />
        <div testId="b" tabIndex={0} style={box} onFocusIn={() => log.push("b:focusin")} />
      </div>
    ))
    renderer.focusElement(idOf("a"))
    expect(log).toEqual(["parent:focusin"])
    log.length = 0
    renderer.focusElement(idOf("b"))
    expect(log).toEqual(["parent:focusout", "b:focusin", "parent:focusin"])
  })

  it("is not made focusable by listeners, but ancestors still hear the keys", () => {
    const keys: string[] = []
    const { renderer, focused, idOf } = setup(() => (
      <div style={{ width: 200, height: 100 }} onKeyDown={(event) => keys.push(event.key)}>
        <div testId="listener" style={box} onKeyDown={() => {}} onFocus={() => {}} onBlur={() => {}} />
        <div testId="stop" tabIndex={-1} style={box} />
      </div>
    ))
    renderer.focusElement(idOf("listener"))
    expect(focused()).toBe(null)
    renderer.focusElement(idOf("stop"))
    expect(focused()).toBe("stop")
    renderer.simulateKeystrokes("x")
    expect(keys).toEqual(["x"])
  })

  it("orders tab stops: positive ascending, then 0s, skipping -1", () => {
    const { renderer, focused } = setup(() => (
      <div style={{ width: 280, height: 100 }}>
        <div testId="zero-1" tabIndex={0} style={box} />
        <div testId="three" tabIndex={3} style={box} />
        <div testId="minus" tabIndex={-1} style={box} />
        <div testId="one" tabIndex={1} style={box} />
        <div testId="zero-2" tabIndex={0} style={box} />
        <div testId="two" tabIndex={2} style={box} />
      </div>
    ))
    const order: (string | null)[] = []
    for (let step = 0; step < 6; step++) {
      renderer.simulateKeystrokes("Tab")
      order.push(focused())
    }
    expect(order).toEqual(["one", "two", "three", "zero-1", "zero-2", "one"])
  })

  it("follows the focus-visible heuristic", () => {
    const { renderer, idOf, center } = setup(() => (
      <div style={{ width: 280, height: 100 }}>
        <div testId="button" tabIndex={0} style={box} />
        <div testId="other" tabIndex={0} style={box} />
        <input testId="field" style={{ width: 80, height: 30 }} />
      </div>
    ))
    const visible = (testId: string) => renderer.matchesFocus(idOf(testId), ":focus-visible")

    renderer.nativeSimulateClick(...center("button"))
    expect(visible("button")).toBe(false)
    renderer.simulateKeystrokes("x")
    expect(visible("button")).toBe(true)

    renderer.nativeSimulateClick(...center("other"))
    expect(visible("other")).toBe(false)
    renderer.simulateKeystrokes("Tab")
    expect(visible("field")).toBe(true)

    renderer.nativeSimulateClick(...center("button"))
    renderer.nativeSimulateClick(...center("field"))
    expect(visible("field")).toBe(true)
  })

  it("reports focusVisible on the event", () => {
    const seen: (boolean | undefined)[] = []
    const { renderer, idOf } = setup(() => (
      <div style={{ width: 200, height: 100 }}>
        <div testId="a" tabIndex={0} style={box} onFocus={(event) => seen.push(event.focusVisible)} />
        <div testId="b" tabIndex={0} style={box} onFocus={(event) => seen.push(event.focusVisible)} />
      </div>
    ))
    // Script focus before any user input matches, like Chrome.
    renderer.focusElement(idOf("a"))
    renderer.simulateKeystrokes("Tab")
    expect(seen).toEqual([true, true])
  })

  it("matches :focus-within on every ancestor of the focused element", () => {
    let leaf!: HostElement
    let outer!: HostElement
    let sibling!: HostElement
    setup(() => (
      <div ref={outer} style={{ width: 200, height: 100 }}>
        <div style={{ width: 100, height: 50 }}>
          <div ref={leaf} tabIndex={0} style={box} />
        </div>
        <div ref={sibling} style={box} />
      </div>
    ))
    leaf.focus()
    expect([leaf.matches(":focus-within"), outer.matches(":focus-within"), sibling.matches(":focus-within")])
      .toEqual([true, true, false])
    expect([leaf.matches(":focus"), outer.matches(":focus")]).toEqual([true, false])
  })

  it("focuses, blurs and matches through the element", () => {
    let target!: HostElement
    const log: string[] = []
    const { focused } = setup(() => (
      <div style={{ width: 200, height: 100 }}>
        <div
          ref={target}
          testId="target"
          tabIndex={0}
          style={box}
          onFocus={() => log.push("focus")}
          onBlur={() => log.push("blur")}
        />
      </div>
    ))
    target.focus()
    expect(focused()).toBe("target")
    expect(target.matches(":focus")).toBe(true)
    target.blur()
    expect(focused()).toBe(null)
    expect(log).toEqual(["focus", "blur"])
    expect(() => target.matches(":visited")).toThrow(":focus")
  })

  it("scrolls into view unless preventScroll is set", () => {
    let far!: HostElement
    const { renderer, idOf } = setup(() => (
      <div testId="scroller" style={{ width: 200, height: 60, overflow: "scroll" }}>
        <div style={{ width: 100, height: 300 }} />
        <div ref={far} tabIndex={0} style={{ width: 100, height: 40 }} />
      </div>
    ))
    const scrollTop = () => renderer.getScrollOffset(idOf("scroller"))?.[1] ?? 0
    far.focus({ preventScroll: true })
    expect(scrollTop()).toBe(0)
    far.blur()
    far.focus()
    expect(scrollTop()).toBeGreaterThan(0)
  })

  it("scrolls Tab and focus() to the nearest edge, and only if needed", () => {
    let visible!: HostElement
    let far!: HostElement
    const item = { width: 100, height: 40, flexShrink: 0 }
    const { renderer, focused, idOf } = setup(() => (
      <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }}>
        <div testId="i0" tabIndex={0} style={item} />
        <div ref={visible} testId="i1" tabIndex={0} style={item} />
        <div testId="i2" tabIndex={0} style={item} />
        <div ref={far} testId="i3" tabIndex={0} style={item} />
        <div testId="i4" tabIndex={0} style={item} />
        <div testId="i5" tabIndex={0} style={item} />
      </div>
    ))
    const scrollTop = () => renderer.getScrollOffset(idOf("scroller"))?.[1] ?? 0
    renderer.simulateKeystrokes("Tab Tab")
    expect(focused()).toBe("i1")
    expect(scrollTop()).toBe(0)
    renderer.simulateKeystrokes("Tab")
    expect(focused()).toBe("i2")
    expect(scrollTop()).toBe(20)
    visible.focus()
    expect(scrollTop()).toBe(20)
    far.focus({ preventScroll: true })
    expect(scrollTop()).toBe(20)
    far.blur()
    far.focus()
    expect(scrollTop()).toBe(60)
  })

  it("does not focus a disabled input that has a tabIndex", () => {
    const { renderer, focused, idOf, center } = setup(() => (
      <div style={{ width: 200, height: 100 }}>
        <input testId="field" disabled tabIndex={0} style={{ width: 100, height: 30 }} />
        <div testId="stop" tabIndex={0} style={box} />
      </div>
    ))
    renderer.focusElement(idOf("field"))
    expect(focused()).toBe(null)
    renderer.nativeSimulateClick(...center("field"))
    expect(focused()).toBe(null)
    renderer.simulateKeystrokes("Tab")
    expect(focused()).toBe("stop")
  })

  it("ignores focus() on an element that is not focusable", () => {
    const { renderer, focused, idOf } = setup(() => (
      <div style={{ width: 200, height: 100 }}>
        <div testId="plain" style={box} />
      </div>
    ))
    renderer.focusElement(idOf("plain"))
    expect(focused()).toBe(null)
  })

  it("targets the topmost element of a press, so a backdrop does not focus the input under it", () => {
    const { renderer, focused, idOf, center } = setup(() => (
      <div style={{ width: 300, height: 200 }}>
        <input testId="field" style={{ width: 120, height: 40 }} />
        <div
          testId="backdrop"
          style={{ position: "absolute", top: 0, left: 0, width: 300, height: 200, backgroundColor: "#00000080" }}
        />
      </div>
    ))
    renderer.focusElement(idOf("field"))
    expect(focused()).toBe("field")
    renderer.nativeSimulateClick(...center("field"))
    expect(focused()).toBe(null)
  })

  it("keeps focus-visible across the window going inactive and back", () => {
    const { renderer, focused, idOf } = setup(() => (
      <div style={{ width: 280, height: 100 }}>
        <div testId="a" tabIndex={0} style={box} />
      </div>
    ))
    renderer.simulateKeystrokes("Tab")
    expect(focused()).toBe("a")
    renderer.setWindowActive(false)
    renderer.setWindowActive(true)
    expect(renderer.matchesFocus(idOf("a"), ":focus-visible")).toBe(true)
  })

  describe("when nobody called focus()", () => {
    // A press, an `autoFocus` mount and a frame move focus without a call of JS's,
    // and their events are still delivered before the input, the commit or the
    // draw returns. The wrapper drains the queue after each call, which would hide
    // a queued event, so watch below it.
    function watch(renderer: ReturnType<typeof setup>["renderer"]) {
      const sync: string[] = []
      const handler = (renderer as any).dispatchSyncEvent
      ;(renderer as any).native.setSyncEventHandler((event: any) => {
        sync.push(`${event.eventType}:${renderer.getElement(event.target)?.testId}`)
        return handler(event)
      })
      const queued = () =>
        (renderer as any).native
          .drainEvents()
          .filter((event: any) => ["focus", "blur", "focusin", "focusout"].includes(event.eventType))
          .map((event: any) => event.eventType)
      return { sync, queued }
    }

    const pressScene = (onMouseDownB: (event: any) => void = () => {}) => {
      const refs: Record<string, HostElement> = {}
      const view = () => (
        <div testId="scope" style={{ width: 200, height: 100 }}>
          <div ref={(el) => (refs.a = el)} testId="a" tabIndex={0} style={box} onMouseDown={() => {}} onFocus={() => {}} />
          <div ref={(el) => (refs.b = el)} testId="b" tabIndex={0} style={box} onMouseDown={onMouseDownB} onFocus={() => {}} onBlur={() => {}} />
          <div testId="plain" style={box} onMouseDown={() => {}} />
        </div>
      )
      return { refs, view }
    }

    it("delivers the focus of a press after its mousedown and before the press returns", () => {
      const scene = pressScene()
      const { renderer, center } = setup(scene.view)
      renderer.native.flush()
      const { sync, queued } = watch(renderer)

      renderer.native.simulateMouseDown(...center("a"), 0)
      expect(sync).toEqual(["mousedown:a", "focus:a", "focusin:a"])
      expect(document.activeElement).toBe(scene.refs.a)

      renderer.native.simulateMouseDown(...center("b"), 0)
      expect(sync.slice(3)).toEqual(["mousedown:b", "blur:a", "focusout:a", "focus:b", "focusin:b"])
      expect(document.activeElement).toBe(scene.refs.b)

      renderer.native.simulateMouseDown(...center("plain"), 0)
      expect(sync.slice(8)).toEqual(["mousedown:plain", "blur:b", "focusout:b"])
      expect(document.activeElement).toBeNull()
      expect(queued()).toEqual([])
    })

    it("delivers no focus events for a press whose mousedown was cancelled", () => {
      const scene = pressScene((event) => event.preventDefault())
      const { renderer, center } = setup(scene.view)
      renderer.native.flush()
      const { sync, queued } = watch(renderer)

      renderer.native.simulateMouseDown(...center("a"), 0)
      renderer.native.simulateMouseUp(...center("a"), 0)
      renderer.native.simulateMouseDown(...center("b"), 0)
      expect(sync.filter((event) => /^(focus|blur)/.test(event))).toEqual(["focus:a", "focusin:a"])
      expect(document.activeElement).toBe(scene.refs.a)
      expect(queued()).toEqual([])
    })

  })
})
