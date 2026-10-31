// Tests for the <nativeview> element on the test renderer, whose surface is recorded instead of
// being a real AppKit view: what the element tells the library through `onAttach` and
// `onDetach`, and in which order. The geometry of the surface is covered with the native side.
import { For, Show, createSignal } from "solid-js"
import { afterEach, beforeEach, describe, expect, it } from "bun:test"
import type { EventPayload } from "@ghia/native"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"

let open: TestRoot | undefined
let errors: unknown[]
let log: string[]
let views: bigint[]

beforeEach(() => {
  errors = []
  log = []
  views = []
})

afterEach(() => {
  open?.unmount()
  open = undefined
})

const onAttach = (view: bigint) => {
  views.push(view)
  log.push("attach")
}
const onDetach = () => {
  log.push("detach")
}

function mounted(view: () => any) {
  const app = createTestRoot({ width: 800, height: 600, onError: (error) => errors.push(error) })
  open = app
  app.render(view)
  const renderer = app.renderer
  const idOf = (testId: string) => {
    const element = renderer.findByTestId(testId)
    expect(element, `missing testId ${testId}`).toBeDefined()
    return element!.id
  }
  return {
    app,
    renderer,
    idOf,
    events: (testId: string) => renderer.getElement(idOf(testId))!.events,
    /** The queued `attach` reaches JS, as the frame loop delivers it. */
    deliver: () => renderer.dispatchNativeEvents(),
    /** What native sends when a window closes: a `detach` with no unmount. */
    detachFromNative: (elementId: number) =>
      (renderer as unknown as { dispatchSyncEvent(event: EventPayload): unknown }).dispatchSyncEvent({
        elementId,
        eventType: "detach",
      }),
  }
}

const BOTH = ["attach", "detach"]

describe.skipIf(!hasNativeTestRenderer)("Solid <nativeview>", () => {
  describe("native element", () => {
    it("is a nativeview with nativeEvents as a custom prop, and its handlers as listeners", () => {
      const { renderer, idOf } = mounted(() => (
        <nativeview testId="n" nativeEvents onAttach={onAttach} onDetach={onDetach} />
      ))
      const element = renderer.getElement(idOf("n"))!
      expect(element.type).toBe("nativeview")
      expect(element.customProps).toMatchObject({ nativeEvents: true })
      expect(element.customProps).not.toHaveProperty("onAttach")
      expect(element.customProps).not.toHaveProperty("onDetach")
      expect([...element.events]).toEqual(expect.arrayContaining(BOTH))
    })

    it("has no listeners and no custom props without them", () => {
      const { renderer, idOf } = mounted(() => <nativeview testId="n" />)
      const element = renderer.getElement(idOf("n"))!
      expect(element.customProps ?? {}).not.toHaveProperty("nativeEvents")
      expect(element.events.has("attach")).toBe(false)
      expect(element.events.has("detach")).toBe(false)
    })

    it("registers both listeners for either handler alone", () => {
      const first = mounted(() => <nativeview testId="n" onAttach={onAttach} />)
      expect([...first.events("n")]).toEqual(expect.arrayContaining(BOTH))
      open?.unmount()
      const second = mounted(() => <nativeview testId="n" onDetach={onDetach} />)
      expect([...second.events("n")]).toEqual(expect.arrayContaining(BOTH))
    })

    it("takes pointer events into the library's view only with nativeEvents", () => {
      const { renderer, idOf } = mounted(() => (
        <div>
          <nativeview testId="a" style={{ width: 50, height: 50 }} />
          <nativeview testId="b" nativeEvents style={{ width: 50, height: 50 }} />
        </div>
      ))
      expect(renderer.getNativeviewSurface(idOf("a"))!.capturesPointer).toBe(false)
      expect(renderer.getNativeviewSurface(idOf("b"))!.capturesPointer).toBe(true)
    })
  })

  describe("attach", () => {
    it("calls onAttach once with a bigint after the first frame", () => {
      const { renderer, idOf, deliver } = mounted(() => (
        <nativeview testId="n" style={{ width: 200, height: 100 }} onAttach={onAttach} onDetach={onDetach} />
      ))
      expect(log).toEqual([])
      deliver()
      expect(log).toEqual(["attach"])
      expect(typeof views[0]).toBe("bigint")
      expect(String(views[0])).toBe(String(renderer.getNativeviewSurface(idOf("n"))!.handle))

      renderer.drawFrame()
      deliver()
      expect(log).toEqual(["attach"])
    })

    it("hands each nativeview its own view", () => {
      const seen: Record<string, bigint> = {}
      const { deliver } = mounted(() => (
        <div>
          <nativeview style={{ width: 50, height: 50 }} onAttach={(view) => (seen.a = view)} />
          <nativeview style={{ width: 50, height: 50 }} onAttach={(view) => (seen.b = view)} />
        </div>
      ))
      deliver()
      expect(seen.a).not.toBe(seen.b)
    })

    it("is not called for a surface the platform cannot make, and warns once", () => {
      const app = createTestRoot({ width: 800, height: 600, onError: (error) => errors.push(error) })
      open = app
      const renderer = app.renderer
      const [width, setWidth] = createSignal(200)
      const [show, setShow] = createSignal(true)
      renderer.setNativeSurfacesAvailable(false)
      app.render(() => (
        <Show when={show()}>
          <nativeview testId="n" style={{ width: width(), height: 100 }} onAttach={onAttach} onDetach={onDetach} />
        </Show>
      ))
      for (let frame = 0; frame < 3; frame++) {
        renderer.drawFrame()
        renderer.dispatchNativeEvents()
      }
      app.flushSync(() => setWidth(300))
      renderer.drawFrame()
      expect(renderer.getNativeviewWarnings(renderer.findByTestId("n")!.id)).toBe(1)
      app.flushSync(() => setShow(false))
      expect(log).toEqual([])
      expect(errors).toEqual([])
    })
  })

  describe("detach", () => {
    it("is called synchronously by the flush that unmounts, after onAttach", () => {
      const [show, setShow] = createSignal(true)
      const { app, deliver } = mounted(() => (
        <Show when={show()}>
          <nativeview style={{ width: 200, height: 100 }} onAttach={onAttach} onDetach={onDetach} />
        </Show>
      ))
      deliver()
      app.flushSync(() => setShow(false))
      expect(log).toEqual(BOTH)
      deliver()
      expect(log).toEqual(BOTH)
    })

    it("is called when the root unmounts", () => {
      const { app, deliver } = mounted(() => (
        <nativeview style={{ width: 200, height: 100 }} onAttach={onAttach} onDetach={onDetach} />
      ))
      deliver()
      app.root.unmount()
      expect(log).toEqual(BOTH)
    })

    it("is called for a nativeview inside a subtree that unmounts", () => {
      const [show, setShow] = createSignal(true)
      const { app, deliver } = mounted(() => (
        <Show when={show()}>
          <div>
            <div>
              <nativeview style={{ width: 50, height: 50 }} onAttach={onAttach} onDetach={onDetach} />
            </div>
          </div>
        </Show>
      ))
      deliver()
      app.flushSync(() => setShow(false))
      expect(log).toEqual(BOTH)
    })

    it("is not called for an unmount that came before the attach was delivered, and the attach is dropped", () => {
      const [show, setShow] = createSignal(true)
      const { app, deliver } = mounted(() => (
        <Show when={show()}>
          <nativeview style={{ width: 200, height: 100 }} onAttach={onAttach} onDetach={onDetach} />
        </Show>
      ))
      app.flushSync(() => setShow(false))
      deliver()
      expect(log).toEqual([])
      expect(errors).toEqual([])
    })

    it("drops an attach a detach overtook while the element stays", () => {
      const [attach, setAttach] = createSignal<typeof onAttach | undefined>(onAttach)
      const [detach, setDetach] = createSignal<typeof onDetach | undefined>(onDetach)
      const { app, idOf, deliver, detachFromNative } = mounted(() => (
        <nativeview testId="n" style={{ width: 200, height: 100 }} onAttach={attach()} onDetach={detach()} />
      ))
      detachFromNative(idOf("n"))
      deliver()
      expect(log).toEqual([])
      // Without handlers native sends nothing more, so the end of the test does not detach twice.
      app.flushSync(() => {
        setAttach(undefined)
        setDetach(undefined)
      })
      expect(errors).toEqual([])
    })

    it("is called for every detach when there is no onAttach", () => {
      const [show, setShow] = createSignal(true)
      const { app, deliver } = mounted(() => (
        <Show when={show()}>
          <nativeview style={{ width: 200, height: 100 }} onDetach={onDetach} />
        </Show>
      ))
      deliver()
      expect(log).toEqual([])
      app.flushSync(() => setShow(false))
      expect(log).toEqual(["detach"])
    })

    it("is called after a detach that overtook the attach when there is no onAttach", () => {
      const [detach, setDetach] = createSignal<typeof onDetach | undefined>(onDetach)
      const { app, idOf, deliver, detachFromNative } = mounted(() => (
        <nativeview testId="n" style={{ width: 200, height: 100 }} onDetach={detach()} />
      ))
      detachFromNative(idOf("n"))
      expect(log).toEqual(["detach"])
      deliver()
      app.flushSync(() => setDetach(undefined))
      expect(log).toEqual(["detach"])
    })

    it("is not called after onDetach was removed following the attach", () => {
      const [detach, setDetach] = createSignal<typeof onDetach | undefined>(onDetach)
      const [show, setShow] = createSignal(true)
      const { app, deliver } = mounted(() => (
        <Show when={show()}>
          <nativeview style={{ width: 200, height: 100 }} onAttach={onAttach} onDetach={detach()} />
        </Show>
      ))
      deliver()
      app.flushSync(() => setDetach(undefined))
      app.flushSync(() => setShow(false))
      expect(log).toEqual(["attach"])
    })
  })

  describe("handlers", () => {
    it("a new function replaces the old without a second native listener", () => {
      const first: string[] = []
      const second: string[] = []
      const [attach, setAttach] = createSignal<(view: bigint) => void>(() => first.push("attach"))
      const [detach, setDetach] = createSignal<() => void>(() => first.push("detach"))
      const [show, setShow] = createSignal(true)
      const { app, renderer, deliver } = mounted(() => (
        <Show when={show()}>
          <nativeview style={{ width: 200, height: 100 }} onAttach={attach()} onDetach={detach()} />
        </Show>
      ))
      const batches: string[] = []
      const native = renderer as unknown as { applyBatch(batch: string): unknown }
      const applyBatch = native.applyBatch.bind(native)
      native.applyBatch = (batch) => {
        batches.push(batch)
        return applyBatch(batch)
      }
      app.flushSync(() => {
        setAttach(() => () => second.push("attach"))
        setDetach(() => () => second.push("detach"))
      })
      expect(batches.filter((batch) => batch.includes("setEventListener"))).toEqual([])

      deliver()
      app.flushSync(() => setShow(false))
      expect(first).toEqual([])
      expect(second).toEqual(BOTH)
    })

    it("removing both takes the listeners off native", () => {
      const [attach, setAttach] = createSignal<typeof onAttach | undefined>(onAttach)
      const [detach, setDetach] = createSignal<typeof onDetach | undefined>(onDetach)
      const [show, setShow] = createSignal(true)
      const { app, events, deliver } = mounted(() => (
        <Show when={show()}>
          <nativeview testId="n" style={{ width: 200, height: 100 }} onAttach={attach()} onDetach={detach()} />
        </Show>
      ))
      expect([...events("n")]).toEqual(expect.arrayContaining(BOTH))
      app.flushSync(() => {
        setAttach(undefined)
        setDetach(undefined)
      })
      expect(events("n").has("attach")).toBe(false)
      expect(events("n").has("detach")).toBe(false)
      deliver()
      app.flushSync(() => setShow(false))
      expect(log).toEqual([])
    })

    it("removing one keeps both listeners", () => {
      const [detach, setDetach] = createSignal<typeof onDetach | undefined>(onDetach)
      const { app, events } = mounted(() => (
        <nativeview testId="n" style={{ width: 200, height: 100 }} onAttach={onAttach} onDetach={detach()} />
      ))
      app.flushSync(() => setDetach(undefined))
      expect([...events("n")]).toEqual(expect.arrayContaining(BOTH))
    })

    it("adding them to an element that has none registers the listeners", () => {
      const [attach, setAttach] = createSignal<typeof onAttach | undefined>(undefined)
      const [detach, setDetach] = createSignal<typeof onDetach | undefined>(undefined)
      const { app, events } = mounted(() => (
        <nativeview testId="n" style={{ width: 200, height: 100 }} onAttach={attach()} onDetach={detach()} />
      ))
      expect(events("n").has("attach")).toBe(false)
      app.flushSync(() => {
        setAttach(() => onAttach)
        setDetach(() => onDetach)
      })
      expect([...events("n")]).toEqual(expect.arrayContaining(BOTH))
    })

    it("a throw in onAttach is reported and onDetach still follows", () => {
      const [show, setShow] = createSignal(true)
      const { app, deliver } = mounted(() => (
        <Show when={show()}>
          <nativeview
            style={{ width: 200, height: 100 }}
            onAttach={() => {
              log.push("attach")
              throw new Error("attach failed")
            }}
            onDetach={onDetach}
          />
        </Show>
      ))
      deliver()
      expect(errors).toHaveLength(1)
      expect((errors[0] as Error).message).toBe("attach failed")
      app.flushSync(() => setShow(false))
      expect(log).toEqual(BOTH)
    })

    it("a throw in onDetach is reported and the unmount goes on", () => {
      const [show, setShow] = createSignal(true)
      const { app, renderer, deliver } = mounted(() => (
        <Show when={show()}>
          <nativeview
            testId="n"
            style={{ width: 200, height: 100 }}
            onAttach={onAttach}
            onDetach={() => {
              log.push("detach")
              throw new Error("detach failed")
            }}
          />
        </Show>
      ))
      deliver()
      app.flushSync(() => setShow(false))
      expect(log).toEqual(BOTH)
      expect(errors).toHaveLength(1)
      expect((errors[0] as Error).message).toBe("detach failed")
      expect(renderer.findByTestId("n")).toBeFalsy()
    })
  })

  describe("lifetime", () => {
    it("display none does not detach", () => {
      const [hidden, setHidden] = createSignal(false)
      const { app, deliver } = mounted(() => (
        <nativeview
          style={{ width: 200, height: 100, display: hidden() ? "none" : "flex" }}
          onAttach={onAttach}
          onDetach={onDetach}
        />
      ))
      deliver()
      app.flushSync(() => setHidden(true))
      deliver()
      app.flushSync(() => setHidden(false))
      deliver()
      expect(log).toEqual(["attach"])
    })

    it("a keyed reorder does not detach", () => {
      const [order, setOrder] = createSignal(["a", "b", "c"])
      const { app, deliver } = mounted(() => (
        <div>
          <For each={order()}>
            {(key) => (
              <div style={{ width: 50, height: 50 }}>
                <nativeview
                  testId={key}
                  style={{ width: 50, height: 50 }}
                  onAttach={() => log.push(`attach ${key}`)}
                  onDetach={() => log.push(`detach ${key}`)}
                />
              </div>
            )}
          </For>
        </div>
      ))
      deliver()
      app.flushSync(() => setOrder(["c", "a", "b"]))
      deliver()
      app.flushSync(() => setOrder(["b", "c", "a"]))
      deliver()
      expect(log).toEqual(["attach a", "attach b", "attach c"])
    })

    it("an element mounted again attaches again as a new element", () => {
      const [show, setShow] = createSignal(true)
      const { app, renderer, deliver } = mounted(() => (
        <Show when={show()}>
          <nativeview testId="n" style={{ width: 200, height: 100 }} onAttach={onAttach} onDetach={onDetach} />
        </Show>
      ))
      const first = renderer.findByTestId("n")!.id
      deliver()
      app.flushSync(() => setShow(false))
      app.flushSync(() => setShow(true))
      expect(renderer.findByTestId("n")!.id).not.toBe(first)
      deliver()
      expect(log).toEqual(["attach", "detach", "attach"])
      app.flushSync(() => setShow(false))
      expect(log).toEqual(["attach", "detach", "attach", "detach"])
    })
  })
})
