/// Tests for the <webview> element on the test renderer, whose pages are scripted: what the
/// element asks of its page is recorded, and a test makes the page report. The geometry is what
/// the window recorded of the native surface it composed.

import { afterEach, beforeEach, describe, expect, it } from "vitest"
import React, { useRef } from "react"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing"
import type { WebviewInstance } from "../types/host.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Event = { type: string; [key: string]: unknown }

describeNative("custom element: webview", () => {
  let testRoot: TestRoot

  beforeEach(() => {
    testRoot = createTestRoot({ width: 800, height: 600 })
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
  const surface = (testId: string) => renderer().getWebviewSurface(idOf(testId))
  const commands = (testId: string) =>
    renderer()
      .getWebviewCommands(idOf(testId))
      .map((command) => (command.argument == null ? command.name : `${command.name} ${command.argument}`))

  describe("size and surface", () => {
    it("is 300 by 150 unless sized", () => {
      testRoot.render(<webview testId="w" />)
      const bounds = renderer().getElementBounds(idOf("w"))!
      expect([bounds.width, bounds.height]).toEqual([300, 150])
      expect(surface("w")).toMatchObject({ visible: true, width: 300, height: 150 })
    })

    it("takes the size of its style", () => {
      testRoot.render(<webview testId="w" style={{ width: 120, height: 80 }} />)
      expect(surface("w")).toMatchObject({ width: 120, height: 80 })
    })

    it("puts the surface where the element is, in the frame it was painted", () => {
      testRoot.render(
        <div style={{ padding: 20, paddingTop: 50 }}>
          <webview testId="w" style={{ width: 200, height: 100 }} />
        </div>
      )
      const bounds = renderer().getElementBounds(idOf("w"))!
      expect(surface("w")).toMatchObject({
        x: bounds.x,
        y: bounds.y,
        width: bounds.width,
        height: bounds.height,
        clipX: bounds.x,
        clipY: bounds.y,
        clipWidth: bounds.width,
        clipHeight: bounds.height,
      })
      expect([bounds.x, bounds.y]).toEqual([20, 50])
    })

    it("follows a scroll of an ancestor in the same frame, and is cut by its box", () => {
      testRoot.render(
        <div testId="scroller" style={{ height: 200, width: 400, overflow: "scroll" }}>
          <div style={{ height: 100 }} />
          <webview testId="w" style={{ width: 300, height: 150 }} />
          <div style={{ height: 600 }} />
        </div>
      )
      const scroller = renderer().getElementBounds(idOf("scroller"))!
      const before = surface("w")!
      expect(before).toMatchObject({ visible: true, y: scroller.y + 100, clipHeight: 100 })

      renderer().scrollTo(idOf("scroller"), 0, 60)
      const bounds = renderer().getElementBounds(idOf("w"))!
      const scrolled = surface("w")!
      expect(bounds.y).toBe(scroller.y + 100 - 60)
      expect(scrolled).toMatchObject({ x: bounds.x, y: bounds.y, width: 300, height: 150 })
      // Inside the box now: not cut.
      expect(scrolled.clipY).toBe(bounds.y)
      expect(scrolled.clipHeight).toBe(150)
      expect(scrolled.clipWidth).toBe(300)

      renderer().scrollTo(idOf("scroller"), 0, 120)
      const cut = surface("w")!
      const top = renderer().getElementBounds(idOf("w"))!
      expect(cut.y).toBe(top.y)
      expect(cut.clipY).toBe(scroller.y)
      expect(cut.clipHeight).toBe(top.y + 150 - scroller.y)
    })

    it("is hidden when it is scrolled out of the box that holds it", () => {
      testRoot.render(
        <div testId="scroller" style={{ height: 100, width: 400, overflow: "scroll" }}>
          <webview testId="w" style={{ width: 300, height: 150 }} />
          <div style={{ height: 800 }} />
        </div>
      )
      expect(surface("w")!.visible).toBe(true)
      renderer().scrollTo(idOf("scroller"), 0, 400)
      expect(surface("w")!.visible).toBe(false)
    })
  })

  describe("hidden", () => {
    function toggle(style: Record<string, unknown>) {
      const App = ({ on }: { on: boolean }) => (
        <div>
          <div testId="holder" style={on ? style : undefined}>
            <webview testId="w" />
          </div>
        </div>
      )
      testRoot.render(<App on={false} />)
      expect(surface("w")!.visible).toBe(true)
      testRoot.render(<App on={true} />)
    }

    it("is hidden by display: none, and back when it shows", () => {
      toggle({ display: "none" })
      expect(surface("w")!.visible).toBe(false)
      testRoot.render(
        <div>
          <div testId="holder">
            <webview testId="w" />
          </div>
        </div>
      )
      expect(surface("w")!.visible).toBe(true)
    })

    it("is hidden by visibility: hidden", () => {
      toggle({ visibility: "hidden" })
      expect(surface("w")!.visible).toBe(false)
    })

    it("is hidden in content that content-visibility skips", () => {
      testRoot.render(
        <div>
          <div testId="holder" style={{ contentVisibility: "hidden", width: 300, height: 150 }}>
            <webview testId="w" />
          </div>
        </div>
      )
      // Never painted: it has no surface to show.
      expect(surface("w")).toBeNull()
      testRoot.render(
        <div>
          <div testId="holder" style={{ width: 300, height: 150 }}>
            <webview testId="w" />
          </div>
        </div>
      )
      expect(surface("w")!.visible).toBe(true)
      testRoot.render(
        <div>
          <div testId="holder" style={{ contentVisibility: "hidden", width: 300, height: 150 }}>
            <webview testId="w" />
          </div>
        </div>
      )
      expect(surface("w")!.visible).toBe(false)
    })

    it("is gone when it unmounts", () => {
      testRoot.render(
        <div>
          <webview testId="w" />
        </div>
      )
      const id = idOf("w")
      expect(renderer().getCompositionLayers().some((layer) => layer.webview === id)).toBe(true)
      testRoot.render(<div />)
      expect(renderer().getCompositionLayers().some((layer) => layer.webview === id && layer.visible)).toBe(false)
      expect(() => renderer().getWebviewCommands(id)).toThrow()
    })
  })

  describe("how it looks", () => {
    it("takes the opacity of the element and of an ancestor", () => {
      testRoot.render(
        <div style={{ opacity: 0.5 }}>
          <webview testId="w" style={{ opacity: 0.5 }} />
        </div>
      )
      expect(surface("w")!.opacity).toBeCloseTo(0.25, 2)
    })

    it("is opaque by default", () => {
      testRoot.render(<webview testId="w" />)
      expect(surface("w")!.opacity).toBe(1)
    })

    it("is cut to a uniform border radius", () => {
      testRoot.render(<webview testId="w" style={{ borderRadius: 12 }} />)
      expect(surface("w")!.cornerRadius).toBe(12)
    })

    it("is square when the corners differ", () => {
      testRoot.render(<webview testId="w" style={{ borderRadius: 12, borderBottomLeftRadius: 4 }} />)
      expect(surface("w")!.cornerRadius).toBe(0)
      testRoot.render(<webview testId="w" style={{ borderRadius: "12px / 4px" }} />)
      expect(surface("w")!.cornerRadius).toBe(0)
    })
  })

  describe("layering", () => {
    const layers = () => renderer().getCompositionLayers()
    const shape = () => layers().map((layer) => (layer.kind === "gpui" ? (layer.isBase ? "base" : "over") : "native"))

    it("draws what is painted before it below it, in the window's own layer", () => {
      testRoot.render(
        <div style={{ position: "relative", width: 400, height: 300 }}>
          <div style={{ position: "absolute", left: 0, top: 0, width: 100, height: 100, backgroundColor: "red" }} />
          <webview testId="w" style={{ position: "absolute", left: 50, top: 50 }} />
        </div>
      )
      expect(shape()).toEqual(["base", "native", "over"])
      expect(layers()[0]!.primitives).toBeGreaterThan(0)
      expect(layers()[2]!.primitives).toBe(0)
    })

    it("draws what is painted after it, over it, in a layer above its surface", () => {
      testRoot.render(
        <div style={{ position: "relative", width: 400, height: 300 }}>
          <webview testId="w" style={{ position: "absolute", left: 50, top: 50 }} />
          <div
            testId="over"
            style={{ position: "absolute", left: 100, top: 100, width: 80, height: 80, backgroundColor: "red" }}
          />
        </div>
      )
      expect(shape()).toEqual(["base", "native", "over"])
      expect(layers()[2]!.primitives).toBeGreaterThan(0)
    })

    it("leaves what is painted after it but does not overlap it in the layer below", () => {
      testRoot.render(
        <div style={{ position: "relative", width: 600, height: 300 }}>
          <webview testId="w" style={{ position: "absolute", left: 0, top: 0, width: 100, height: 100 }} />
          <div style={{ position: "absolute", left: 300, top: 0, width: 80, height: 80, backgroundColor: "red" }} />
        </div>
      )
      expect(layers()[2]!.primitives).toBe(0)
    })
  })

  describe("applied to AppKit", () => {
    it("puts the container and the page where the frame recorded them", () => {
      testRoot.render(
        <div testId="scroller" style={{ height: 120, width: 400, overflow: "scroll", opacity: 0.5 }}>
          <div style={{ height: 40 }} />
          <webview testId="w" style={{ width: 300, height: 150, borderRadius: 10 }} />
          <div style={{ height: 600 }} />
        </div>
      )
      const recorded = surface("w")!
      const applied = renderer().getWebviewNativeView(idOf("w"))
      expect(applied.hidden).toBe(false)
      expect([applied.x, applied.y, applied.width, applied.height]).toEqual([
        recorded.clipX,
        recorded.clipY,
        recorded.clipWidth,
        recorded.clipHeight,
      ])
      expect([applied.contentX, applied.contentY, applied.contentWidth, applied.contentHeight]).toEqual([
        recorded.x - recorded.clipX,
        recorded.y - recorded.clipY,
        300,
        150,
      ])
      expect(applied.opacity).toBeCloseTo(0.5, 2)
      expect(applied.masked).toBe(true)

      renderer().scrollTo(idOf("scroller"), 0, 80)
      const scrolled = surface("w")!
      const moved = renderer().getWebviewNativeView(idOf("w"))
      expect([moved.x, moved.y, moved.width, moved.height]).toEqual([
        scrolled.clipX,
        scrolled.clipY,
        scrolled.clipWidth,
        scrolled.clipHeight,
      ])
      expect(moved.contentWidth).toBe(300)
      expect(moved.contentHeight).toBe(150)
    })

    it("hides the container when the element does not show", () => {
      const App = ({ shown }: { shown: boolean }) => (
        <div style={{ display: shown ? "block" : "none" }}>
          <webview testId="w" />
        </div>
      )
      testRoot.render(<App shown={true} />)
      expect(renderer().getWebviewNativeView(idOf("w")).hidden).toBe(false)
      testRoot.render(<App shown={false} />)
      expect(renderer().getWebviewNativeView(idOf("w")).hidden).toBe(true)
    })
  })

  describe("loading", () => {
    it("loads src when it is first painted, and again when it changes", () => {
      testRoot.render(<webview testId="w" src="https://example.test/a" />)
      expect(commands("w")).toEqual(["loadURL https://example.test/a"])
      testRoot.render(<webview testId="w" src="https://example.test/a" />)
      expect(commands("w")).toEqual(["loadURL https://example.test/a"])
      testRoot.render(<webview testId="w" src="https://example.test/b" />)
      expect(commands("w")).toEqual(["loadURL https://example.test/a", "loadURL https://example.test/b"])
    })

    it("does not load when src goes away", () => {
      testRoot.render(<webview testId="w" src="https://example.test/a" />)
      testRoot.render(<webview testId="w" />)
      expect(commands("w")).toEqual(["loadURL https://example.test/a"])
    })

    it("loads a file URL and others as given", () => {
      testRoot.render(<webview testId="w" src="file:///tmp/site/index.html" />)
      expect(commands("w")).toEqual(["loadURL file:///tmp/site/index.html"])
    })

    it("reports a src that is no URL as a failed load", () => {
      const events: Event[] = []
      testRoot.render(<webview testId="w" src="not a url" onDidFailLoad={(event) => events.push(event as never)} />)
      renderer().dispatchNativeEvents()
      expect(events).toHaveLength(1)
      expect(events[0]).toMatchObject({ errorCode: -1002, validatedUrl: "not a url" })
      expect(commands("w")).toEqual([])
    })
  })

  describe("methods", () => {
    it("forward to the page", () => {
      let view: WebviewInstance | null = null
      testRoot.render(<webview testId="w" ref={(instance) => (view = instance)} />)
      view!.loadURL("https://example.test/x")
      view!.reload()
      view!.stop()
      view!.goBack()
      view!.goForward()
      expect(commands("w")).toEqual(["loadURL https://example.test/x", "reload", "stop", "goBack", "goForward"])
    })

    it("refuses a URL without a scheme", () => {
      let view: WebviewInstance | null = null
      testRoot.render(<webview testId="w" ref={(instance) => (view = instance)} />)
      expect(() => view!.loadURL("example.test")).toThrow(/Invalid URL/)
      expect(commands("w")).toEqual([])
    })

    it("read what the page says", () => {
      let view: WebviewInstance | null = null
      testRoot.render(<webview testId="w" ref={(instance) => (view = instance)} />)
      expect(view!.getURL()).toBe("")
      expect(view!.getTitle()).toBe("")
      expect(view!.canGoBack()).toBe(false)
      expect(view!.canGoForward()).toBe(false)

      renderer().simulateWebviewNavigate(idOf("w"), "https://example.test/page")
      renderer().simulateWebviewTitle(idOf("w"), "A page")
      renderer().setWebviewHistory(idOf("w"), true, false)
      expect(view!.getURL()).toBe("https://example.test/page")
      expect(view!.getTitle()).toBe("A page")
      expect(view!.canGoBack()).toBe(true)
      expect(view!.canGoForward()).toBe(false)
      renderer().setWebviewHistory(idOf("w"), true, true)
      expect(view!.canGoForward()).toBe(true)
    })

    it("resolves executeJavaScript with the result", async () => {
      let view: WebviewInstance | null = null
      testRoot.render(<webview testId="w" ref={(instance) => (view = instance)} />)
      const result = view!.executeJavaScript("document.title")
      expect(commands("w")).toEqual(["executeJavaScript document.title"])
      renderer().settleWebviewScript(idOf("w"), { result: { a: [1, "two", null, true] } })
      await expect(result).resolves.toEqual({ a: [1, "two", null, true] })
    })

    it("resolves undefined for a script that returns undefined, and null for null", async () => {
      let view: WebviewInstance | null = null
      testRoot.render(<webview testId="w" ref={(instance) => (view = instance)} />)
      const nothing = view!.executeJavaScript("void 0")
      const null_ = view!.executeJavaScript("null")
      renderer().settleWebviewScript(idOf("w"), {})
      renderer().settleWebviewScript(idOf("w"), { result: null })
      await expect(nothing).resolves.toBeUndefined()
      await expect(null_).resolves.toBeNull()
    })

    it("rejects executeJavaScript with the error of the script", async () => {
      let view: WebviewInstance | null = null
      testRoot.render(<webview testId="w" ref={(instance) => (view = instance)} />)
      const result = view!.executeJavaScript("nope()")
      renderer().settleWebviewScript(idOf("w"), { error: "ReferenceError: Can't find variable: nope" })
      await expect(result).rejects.toThrow("ReferenceError: Can't find variable: nope")
    })

    it("settles scripts in the order they ran", async () => {
      let view: WebviewInstance | null = null
      testRoot.render(<webview testId="w" ref={(instance) => (view = instance)} />)
      const first = view!.executeJavaScript("1")
      const second = view!.executeJavaScript("2")
      renderer().settleWebviewScript(idOf("w"), { result: 1 })
      renderer().settleWebviewScript(idOf("w"), { result: 2 })
      await expect(Promise.all([first, second])).resolves.toEqual([1, 2])
    })

    it("throw for a ref to another element", () => {
      let view: WebviewInstance | null = null
      testRoot.render(<div testId="d" ref={(instance) => (view = instance as never)} />)
      expect((view as unknown as { reload?: unknown }).reload).toBeUndefined()
    })
  })

  describe("events", () => {
    function record() {
      const events: Event[] = []
      const on =
        (type: string) =>
        (event: unknown) => {
          const { bubbles, eventPhase, target } = event as { bubbles: boolean; eventPhase: number; target: unknown }
          events.push({ type, ...(event as object), bubbles, eventPhase, target })
        }
      return {
        events,
        props: {
          onDidStartLoading: on("did-start-loading"),
          onDidStopLoading: on("did-stop-loading"),
          onDidNavigate: on("did-navigate"),
          onPageTitleUpdated: on("page-title-updated"),
          onDidFailLoad: on("did-fail-load"),
        },
      }
    }

    it("arrive with their payloads", () => {
      const { events, props } = record()
      testRoot.render(<webview testId="w" {...props} />)
      const id = idOf("w")
      renderer().simulateWebviewStartLoading(id)
      renderer().simulateWebviewNavigate(id, "https://example.test/")
      renderer().simulateWebviewTitle(id, "Example", true)
      renderer().simulateWebviewTitle(id, "", false)
      renderer().simulateWebviewStopLoading(id)
      renderer().simulateWebviewFail(id, -1009, "The Internet connection appears to be offline.", "https://example.test/x")
      expect(events.map((event) => event.type)).toEqual([
        "did-start-loading",
        "did-navigate",
        "page-title-updated",
        "page-title-updated",
        "did-stop-loading",
        "did-fail-load",
      ])
      expect(events[1]).toMatchObject({ url: "https://example.test/" })
      expect(events[2]).toMatchObject({ title: "Example", explicitSet: true })
      expect(events[3]).toMatchObject({ title: "", explicitSet: false })
      expect(events[5]).toMatchObject({
        errorCode: -1009,
        errorDescription: "The Internet connection appears to be offline.",
        validatedUrl: "https://example.test/x",
      })
    })

    it("do not bubble and cannot be cancelled", () => {
      const { events, props } = record()
      const heard: string[] = []
      const parent = (type: string) => () => heard.push(type)
      testRoot.render(
        <div {...({ onDidNavigate: parent("did-navigate") } as object)} style={{ width: 400, height: 300 }}>
          <webview testId="w" {...props} />
        </div>
      )
      renderer().simulateWebviewNavigate(idOf("w"), "https://example.test/")
      expect(events).toHaveLength(1)
      expect(events[0]).toMatchObject({ bubbles: false, eventPhase: 2 })
      expect(heard).toEqual([])
    })

    it("report to the element that has the page", () => {
      const first = record()
      const second = record()
      testRoot.render(
        <div>
          <webview testId="a" {...first.props} />
          <webview testId="b" {...second.props} />
        </div>
      )
      renderer().simulateWebviewNavigate(idOf("b"), "https://example.test/b")
      expect(first.events).toHaveLength(0)
      expect(second.events).toHaveLength(1)
    })
  })

  describe("focus", () => {
    it("is a Tab stop, and takes focus from a script", () => {
      let view: WebviewInstance | null = null
      const focused: string[] = []
      testRoot.render(
        <div>
          <input testId="before" style={{ width: 100, height: 20 }} />
          <webview
            testId="w"
            ref={(instance) => (view = instance)}
            onFocus={() => focused.push("focus")}
            onBlur={() => focused.push("blur")}
          />
          <input testId="after" style={{ width: 100, height: 20 }} />
        </div>
      )
      renderer().focusElement(idOf("before"))
      renderer().simulateKeystrokes("Tab")
      expect(renderer().getFocusedElementId()).toBe(idOf("w"))
      expect(focused).toEqual(["focus"])
      renderer().simulateKeystrokes("Tab")
      expect(renderer().getFocusedElementId()).toBe(idOf("after"))
      expect(focused).toEqual(["focus", "blur"])

      view!.focus()
      expect(renderer().getFocusedElementId()).toBe(idOf("w"))
      view!.blur()
      expect(renderer().getFocusedElementId()).toBeNull()
    })

    it("is not a Tab stop with a negative tabIndex, but takes focus", () => {
      let view: WebviewInstance | null = null
      testRoot.render(
        <div>
          <input testId="before" style={{ width: 100, height: 20 }} />
          <webview testId="w" tabIndex={-1} ref={(instance) => (view = instance)} />
          <input testId="after" style={{ width: 100, height: 20 }} />
        </div>
      )
      renderer().focusElement(idOf("before"))
      renderer().simulateKeystrokes("Tab")
      expect(renderer().getFocusedElementId()).toBe(idOf("after"))
      view!.focus()
      expect(renderer().getFocusedElementId()).toBe(idOf("w"))
    })
  })

  describe("retention", () => {
    it("replays a page with a webview in it in a frame that changes nothing, and keeps its surface", () => {
      testRoot.render(
        <div>
          {Array.from({ length: 6 }, (_, index) => (
            <div key={index} style={{ height: 20 }}>
              <text>row {index}</text>
              <webview testId={`w${index}`} style={{ width: 40, height: 15 }} />
            </div>
          ))}
        </div>
      )
      for (let frame = 0; frame < 3; frame++) renderer().flush()
      const before = renderer().getRetainedCounters()
      const recorded = surface("w2")
      renderer().flush()
      renderer().flush()
      expect(renderer().getRetainedCounters().replays).toBeGreaterThan(before.replays)
      if (process.env.GPUIX_RETAIN_VERIFY === undefined) {
        expect(renderer().getRetainedCounters().builds).toBe(before.builds)
      }
      expect(surface("w2")).toEqual(recorded)
      expect(commands("w2")).toEqual([])
    })
  })
})
