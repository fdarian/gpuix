import { createSignal, Show } from "solid-js"
import { describe, expect, it } from "bun:test"
import type { HostElement } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("getBoundingClientRect", () => {
  it("answers for an element mounted in the same render", () => {
    const app = createTestRoot()
    let rect: ReturnType<HostElement["getBoundingClientRect"]> | undefined
    app.render(() => (
      <div style={{ padding: 10 }}>
        <div
          ref={(element) => queueMicrotask(() => (rect = element.getBoundingClientRect()))}
          style={{ width: 120, height: 40 }}
        />
      </div>
    ))
    return Promise.resolve().then(() => {
      expect(rect).toEqual({
        x: 10,
        y: 10,
        width: 120,
        height: 40,
        left: 10,
        top: 10,
        right: 130,
        bottom: 50,
      })
    })
  })

  it("reads the new size in the turn that changed it, before Solid flushes", () => {
    const [width, setWidth] = createSignal(50)
    let element!: HostElement
    const app = createTestRoot()
    app.render(() => (
      <div
        ref={(host) => {
          element = host
        }}
        style={{ width: width(), height: 20 }}
      />
    ))
    expect(element.getBoundingClientRect().width).toBe(50)

    setWidth(130)
    // Solid queues the style change and sends it in a microtask; the read must
    // not wait for that.
    expect(element.getBoundingClientRect().width).toBe(130)
  })

  it("flushes queued changes before a raw renderer read too", () => {
    const [shown, setShown] = createSignal(false)
    let late: HostElement | undefined
    const app = createTestRoot()
    app.render(() => (
      <div>
        <Show when={shown()}>
          <div ref={(host) => (late = host)} style={{ width: 70, height: 10 }} />
        </Show>
      </div>
    ))
    expect(late).toBeUndefined()

    setShown(true)
    expect(app.renderer.getElementBounds(late!.id)).toMatchObject({ width: 70, height: 10 })
  })

  it("is the border box, for an input as for a div", () => {
    const app = createTestRoot()
    let div!: HostElement
    let input!: HostElement
    const style = { width: 100, height: 30, borderWidth: 3, borderColor: "#ffffff" }
    app.render(() => (
      <div style={{ flexDirection: "column" }}>
        <div ref={(host) => (div = host)} style={style} />
        <input ref={(host) => (input = host)} style={style} />
      </div>
    ))
    expect([div.getBoundingClientRect().width, div.getBoundingClientRect().height]).toEqual([100, 30])
    expect([input.getBoundingClientRect().width, input.getBoundingClientRect().height]).toEqual([100, 30])
  })

  it("reports a zero rect for an element without a box", () => {
    const app = createTestRoot()
    let hidden!: HostElement
    app.render(() => (
      <div ref={(host) => (hidden = host)} style={{ visibility: "hidden", width: 50, height: 50 }} />
    ))
    expect(hidden.getBoundingClientRect()).toEqual({
      x: 0,
      y: 0,
      width: 0,
      height: 0,
      top: 0,
      left: 0,
      right: 0,
      bottom: 0,
    })
    expect(app.renderer.getElementBounds(hidden.id)).toBeNull()
  })
})
