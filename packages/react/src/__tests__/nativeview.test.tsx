/// Tests for the <nativeview> element on the test renderer, whose surface is recorded instead of
/// being a real AppKit view: what the element tells the library through `onAttach` and
/// `onDetach`, and in which order. The geometry of the surface is covered with the native side.

import { afterEach, beforeEach, describe, expect, it } from "vitest"
import React from "react"
import type { EventPayload } from "@gpuix/native"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("custom element: nativeview", () => {
  let testRoot: TestRoot
  let errors: unknown[]
  let log: string[]
  let views: bigint[]

  beforeEach(() => {
    errors = []
    log = []
    views = []
    testRoot = createTestRoot({ width: 800, height: 600, onError: (error) => errors.push(error) })
  })

  afterEach(() => {
    testRoot.unmount()
  })

  const renderer = () => testRoot.renderer
  const idOf = (testId: string) => {
    const element = renderer().findByTestId(testId)
    expect(element, `missing testId ${testId}`).toBeDefined()
    return element!.id
  }
  const events = (testId: string) => renderer().getElement(idOf(testId))!.events
  const onAttach = (view: bigint) => {
    views.push(view)
    log.push("attach")
  }
  const onDetach = () => {
    log.push("detach")
  }
  /** The queued `attach` reaches JS, as the frame loop delivers it. */
  const deliver = () => renderer().dispatchNativeEvents()
  /** What native sends when a window closes: a `detach` with no unmount. */
  const detachFromNative = (elementId: number) =>
    (
      renderer() as unknown as { dispatchSyncEvent(event: EventPayload): unknown }
    ).dispatchSyncEvent({ elementId, eventType: "detach" })

  const App = (props: {
    show?: boolean
    attach?: ((view: bigint) => void) | undefined
    detach?: (() => void) | undefined
    style?: Record<string, unknown>
  }) =>
    props.show === false ? null : (
      <nativeview
        testId="n"
        style={{ width: 200, height: 100, ...props.style }}
        onAttach={props.attach}
        onDetach={props.detach}
      />
    )

  describe("native element", () => {
    it("is a nativeview with nativeEvents as a custom prop, and its handlers as listeners", () => {
      testRoot.render(<nativeview testId="n" nativeEvents onAttach={onAttach} onDetach={onDetach} />)
      const element = renderer().getElement(idOf("n"))!
      expect(element.type).toBe("nativeview")
      expect(element.customProps).toMatchObject({ nativeEvents: true })
      expect(element.customProps).not.toHaveProperty("onAttach")
      expect(element.customProps).not.toHaveProperty("onDetach")
      expect([...element.events]).toEqual(expect.arrayContaining(["attach", "detach"]))
    })

    it("has no listeners and no custom props without them", () => {
      testRoot.render(<nativeview testId="n" />)
      const element = renderer().getElement(idOf("n"))!
      expect(element.customProps ?? {}).not.toHaveProperty("nativeEvents")
      expect(element.events.has("attach")).toBe(false)
      expect(element.events.has("detach")).toBe(false)
    })

    it("registers both listeners for either handler alone", () => {
      testRoot.render(<App attach={onAttach} />)
      expect([...events("n")]).toEqual(expect.arrayContaining(["attach", "detach"]))
      testRoot.render(<App detach={onDetach} />)
      expect([...events("n")]).toEqual(expect.arrayContaining(["attach", "detach"]))
    })

    it("takes pointer events into the library's view only with nativeEvents", () => {
      testRoot.render(
        <div>
          <nativeview testId="a" style={{ width: 50, height: 50 }} />
          <nativeview testId="b" nativeEvents style={{ width: 50, height: 50 }} />
        </div>
      )
      expect(renderer().getNativeviewSurface(idOf("a"))!.capturesPointer).toBe(false)
      expect(renderer().getNativeviewSurface(idOf("b"))!.capturesPointer).toBe(true)
    })
  })

  describe("attach", () => {
    it("calls onAttach once with a bigint after the first frame", () => {
      testRoot.render(<App attach={onAttach} detach={onDetach} />)
      expect(log).toEqual([])
      deliver()
      expect(log).toEqual(["attach"])
      expect(typeof views[0]).toBe("bigint")
      expect(String(views[0])).toBe(String(renderer().getNativeviewSurface(idOf("n"))!.handle))

      renderer().drawFrame()
      deliver()
      expect(log).toEqual(["attach"])
    })

    it("hands each nativeview its own view", () => {
      const seen: Record<string, bigint> = {}
      testRoot.render(
        <div>
          <nativeview testId="a" style={{ width: 50, height: 50 }} onAttach={(view) => (seen.a = view)} />
          <nativeview testId="b" style={{ width: 50, height: 50 }} onAttach={(view) => (seen.b = view)} />
        </div>
      )
      deliver()
      expect(seen.a).not.toBe(seen.b)
    })

    it("is not called for a surface the platform cannot make, and warns once", () => {
      renderer().setNativeSurfacesAvailable(false)
      testRoot.render(<App attach={onAttach} detach={onDetach} />)
      for (let frame = 0; frame < 3; frame++) {
        renderer().drawFrame()
        deliver()
      }
      testRoot.render(<App attach={onAttach} detach={onDetach} style={{ width: 300 }} />)
      renderer().drawFrame()
      expect(renderer().getNativeviewWarnings(idOf("n"))).toBe(1)
      testRoot.render(<App show={false} />)
      expect(log).toEqual([])
      expect(errors).toEqual([])
    })
  })

  describe("detach", () => {
    it("is called synchronously by the commit that unmounts, after onAttach", () => {
      testRoot.render(<App attach={onAttach} detach={onDetach} />)
      deliver()
      testRoot.render(<App show={false} attach={onAttach} detach={onDetach} />)
      expect(log).toEqual(["attach", "detach"])
      deliver()
      expect(log).toEqual(["attach", "detach"])
    })

    it("is called when the root unmounts", () => {
      testRoot.render(<App attach={onAttach} detach={onDetach} />)
      deliver()
      testRoot.root.unmount()
      expect(log).toEqual(["attach", "detach"])
    })

    it("is not called for an unmount that came before the attach was delivered, and the attach is dropped", () => {
      testRoot.render(<App attach={onAttach} detach={onDetach} />)
      testRoot.render(<App show={false} attach={onAttach} detach={onDetach} />)
      deliver()
      expect(log).toEqual([])
      expect(errors).toEqual([])
    })

    it("drops an attach a detach overtook while the element stays", () => {
      testRoot.render(<App attach={onAttach} detach={onDetach} />)
      detachFromNative(idOf("n"))
      deliver()
      expect(log).toEqual([])
      // Without handlers native sends nothing more, so the end of the test does not detach twice.
      testRoot.render(<App />)
      expect(errors).toEqual([])
    })

    it("is called for every detach when there is no onAttach", () => {
      testRoot.render(<App detach={onDetach} />)
      deliver()
      expect(log).toEqual([])
      testRoot.render(<App show={false} detach={onDetach} />)
      expect(log).toEqual(["detach"])
    })

    it("is called after a detach that overtook the attach when there is no onAttach", () => {
      testRoot.render(<App detach={onDetach} />)
      detachFromNative(idOf("n"))
      expect(log).toEqual(["detach"])
      deliver()
      testRoot.render(<App />)
      expect(log).toEqual(["detach"])
    })

    it("is called with onAttach alone left, when onDetach was removed after the attach", () => {
      testRoot.render(<App attach={onAttach} detach={onDetach} />)
      deliver()
      testRoot.render(<App attach={onAttach} />)
      testRoot.render(<App show={false} />)
      expect(log).toEqual(["attach"])
    })
  })

  describe("handlers", () => {
    it("a new function replaces the old without a second native listener", () => {
      const batches: string[] = []
      const native = renderer() as unknown as { applyBatch(batch: string): unknown }
      const applyBatch = native.applyBatch.bind(native)
      native.applyBatch = (batch) => {
        batches.push(batch)
        return applyBatch(batch)
      }
      const first: string[] = []
      const second: string[] = []
      testRoot.render(<App attach={() => first.push("attach")} detach={() => first.push("detach")} />)
      const sent = batches.length
      testRoot.render(<App attach={() => second.push("attach")} detach={() => second.push("detach")} />)
      expect(batches.slice(sent).filter((batch) => batch.includes("setEventListener"))).toEqual([])

      deliver()
      testRoot.render(<App show={false} attach={() => second.push("attach")} />)
      expect(first).toEqual([])
      expect(second).toEqual(["attach", "detach"])
    })

    it("removing both takes the listeners off native", () => {
      testRoot.render(<App attach={onAttach} detach={onDetach} />)
      expect([...events("n")]).toEqual(expect.arrayContaining(["attach", "detach"]))
      testRoot.render(<App />)
      expect(events("n").has("attach")).toBe(false)
      expect(events("n").has("detach")).toBe(false)
      deliver()
      testRoot.render(<App show={false} />)
      expect(log).toEqual([])
    })

    it("removing one keeps both listeners", () => {
      testRoot.render(<App attach={onAttach} detach={onDetach} />)
      testRoot.render(<App attach={onAttach} />)
      expect([...events("n")]).toEqual(expect.arrayContaining(["attach", "detach"]))
    })

    it("adding them to an element that has none registers the listeners", () => {
      testRoot.render(<App />)
      expect(events("n").has("attach")).toBe(false)
      testRoot.render(<App attach={onAttach} detach={onDetach} />)
      expect([...events("n")]).toEqual(expect.arrayContaining(["attach", "detach"]))
    })

    it("a throw in onAttach is reported and onDetach still follows", () => {
      testRoot.render(
        <App
          attach={() => {
            log.push("attach")
            throw new Error("attach failed")
          }}
          detach={onDetach}
        />
      )
      deliver()
      expect(errors).toHaveLength(1)
      expect((errors[0] as Error).message).toBe("attach failed")
      testRoot.render(<App show={false} />)
      expect(log).toEqual(["attach", "detach"])
    })

    it("a throw in onDetach is reported and the unmount goes on", () => {
      testRoot.render(
        <App
          attach={onAttach}
          detach={() => {
            log.push("detach")
            throw new Error("detach failed")
          }}
        />
      )
      deliver()
      testRoot.render(<App show={false} />)
      expect(log).toEqual(["attach", "detach"])
      expect(errors).toHaveLength(1)
      expect((errors[0] as Error).message).toBe("detach failed")
      expect(renderer().findByTestId("n")).toBeFalsy()
    })
  })

  describe("lifetime", () => {
    it("display none does not detach", () => {
      testRoot.render(<App attach={onAttach} detach={onDetach} />)
      deliver()
      testRoot.render(<App attach={onAttach} detach={onDetach} style={{ display: "none" }} />)
      deliver()
      testRoot.render(<App attach={onAttach} detach={onDetach} />)
      deliver()
      expect(log).toEqual(["attach"])
    })

    it("a keyed reorder does not detach", () => {
      const List = (props: { order: string[] }) => (
        <div>
          {props.order.map((key) => (
            <div key={key} style={{ width: 50, height: 50 }}>
              <nativeview
                testId={key}
                style={{ width: 50, height: 50 }}
                onAttach={() => log.push(`attach ${key}`)}
                onDetach={() => log.push(`detach ${key}`)}
              />
            </div>
          ))}
        </div>
      )
      testRoot.render(<List order={["a", "b", "c"]} />)
      deliver()
      testRoot.render(<List order={["c", "a", "b"]} />)
      deliver()
      testRoot.render(<List order={["b", "c", "a"]} />)
      deliver()
      expect(log).toEqual(["attach a", "attach b", "attach c"])
    })

    it("an element mounted again attaches again as a new element", () => {
      testRoot.render(<App attach={onAttach} detach={onDetach} />)
      const first = idOf("n")
      deliver()
      testRoot.render(<App show={false} attach={onAttach} detach={onDetach} />)
      testRoot.render(<App attach={onAttach} detach={onDetach} />)
      expect(idOf("n")).not.toBe(first)
      deliver()
      expect(log).toEqual(["attach", "detach", "attach"])
      testRoot.render(<App show={false} />)
      expect(log).toEqual(["attach", "detach", "attach", "detach"])
    })

    it("detaches the nativeview inside a subtree that unmounts", () => {
      const Tree = (props: { show: boolean }) =>
        props.show ? (
          <div>
            <div>
              <nativeview style={{ width: 50, height: 50 }} onAttach={onAttach} onDetach={onDetach} />
            </div>
          </div>
        ) : null
      testRoot.render(<Tree show />)
      deliver()
      testRoot.render(<Tree show={false} />)
      expect(log).toEqual(["attach", "detach"])
    })
  })
})
