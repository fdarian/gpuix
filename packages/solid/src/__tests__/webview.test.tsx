// Tests for the <webview> element on the test renderer, whose pages are scripted: what the
// element asks of its page is recorded, and a test makes the page report. The geometry is what
// the window recorded of the native surface it composed.
import { createSignal } from "solid-js"
import { afterEach, describe, expect, it } from "bun:test"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"
import type { HostElement } from "../host.js"

type Event = { type: string; [key: string]: unknown }

function setup(view: () => any) {
  const app: TestRoot = createTestRoot({ width: 800, height: 600 })
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
    surface: (testId: string) => renderer.getWebviewSurface(idOf(testId)),
    commands: (testId: string) =>
      renderer
        .getWebviewCommands(idOf(testId))
        .map((command) => (command.argument == null ? command.name : `${command.name} ${command.argument}`)),
  }
}

let open: TestRoot | undefined
const mounted = (view: () => any) => {
  const result = setup(view)
  open = result.app
  return result
}

afterEach(() => {
  open?.unmount()
  open = undefined
})

describe.skipIf(!hasNativeTestRenderer)("Solid <webview>", () => {
  describe("size and surface", () => {
    it("is 300 by 150 unless sized", () => {
      const { renderer, idOf, surface } = mounted(() => <webview testId="w" />)
      const bounds = renderer.getElementBounds(idOf("w"))!
      expect([bounds.width, bounds.height]).toEqual([300, 150])
      expect(surface("w")).toMatchObject({ visible: true, width: 300, height: 150 })
    })

    it("takes the size of its style", () => {
      const { surface } = mounted(() => <webview testId="w" style={{ width: 120, height: 80 }} />)
      expect(surface("w")).toMatchObject({ width: 120, height: 80 })
    })

    it("puts the surface where the element is, in the frame it was painted", () => {
      const { renderer, idOf, surface } = mounted(() => (
        <div style={{ padding: 20, paddingTop: 50 }}>
          <webview testId="w" style={{ width: 200, height: 100 }} />
        </div>
      ))
      const bounds = renderer.getElementBounds(idOf("w"))!
      expect(surface("w")).toMatchObject({
        x: bounds.x,
        y: bounds.y,
        width: 200,
        height: 100,
        clipX: bounds.x,
        clipY: bounds.y,
        clipWidth: 200,
        clipHeight: 100,
      })
      expect([bounds.x, bounds.y]).toEqual([20, 50])
    })

    it("follows a scroll of an ancestor in the same frame, and is cut by its box", () => {
      const { renderer, idOf, surface } = mounted(() => (
        <div testId="scroller" style={{ height: 200, width: 400, overflow: "scroll" }}>
          <div style={{ height: 100 }} />
          <webview testId="w" style={{ width: 300, height: 150 }} />
          <div style={{ height: 600 }} />
        </div>
      ))
      const scroller = renderer.getElementBounds(idOf("scroller"))!
      expect(surface("w")).toMatchObject({ visible: true, y: scroller.y + 100, clipHeight: 100 })

      renderer.scrollTo(idOf("scroller"), 0, 60)
      const bounds = renderer.getElementBounds(idOf("w"))!
      const scrolled = surface("w")!
      expect(bounds.y).toBe(scroller.y + 100 - 60)
      expect(scrolled).toMatchObject({ x: bounds.x, y: bounds.y, width: 300, height: 150 })
      expect(scrolled.clipY).toBe(bounds.y)
      expect(scrolled.clipHeight).toBe(150)

      renderer.scrollTo(idOf("scroller"), 0, 120)
      const cut = surface("w")!
      const top = renderer.getElementBounds(idOf("w"))!
      expect(cut.y).toBe(top.y)
      expect(cut.clipY).toBe(scroller.y)
      expect(cut.clipHeight).toBe(top.y + 150 - scroller.y)
    })
  })

  describe("hidden", () => {
    function toggled(style: Record<string, unknown>) {
      const [on, setOn] = createSignal(false)
      const result = mounted(() => (
        <div>
          <div testId="holder" style={on() ? style : undefined}>
            <webview testId="w" />
          </div>
        </div>
      ))
      expect(result.surface("w")!.visible).toBe(true)
      result.app.flushSync(() => setOn(true))
      return { ...result, setOn: (value: boolean) => result.app.flushSync(() => setOn(value)) }
    }

    it("is hidden by display: none, and back when it shows", () => {
      const { surface, setOn } = toggled({ display: "none" })
      expect(surface("w")!.visible).toBe(false)
      setOn(false)
      expect(surface("w")!.visible).toBe(true)
    })

    it("is hidden by visibility: hidden", () => {
      const { surface } = toggled({ visibility: "hidden" })
      expect(surface("w")!.visible).toBe(false)
    })

    it("is hidden in content that content-visibility skips", () => {
      const { surface } = toggled({ contentVisibility: "hidden", width: 300, height: 150 })
      expect(surface("w")!.visible).toBe(false)
    })

    it("is gone when it unmounts", () => {
      const [shown, setShown] = createSignal(true)
      const { app, renderer, idOf } = mounted(() => <div>{shown() ? <webview testId="w" /> : null}</div>)
      const id = idOf("w")
      expect(renderer.getCompositionLayers().some((layer) => layer.webview === id)).toBe(true)
      app.flushSync(() => setShown(false))
      expect(renderer.getCompositionLayers().some((layer) => layer.webview === id && layer.visible)).toBe(false)
      expect(() => renderer.getWebviewCommands(id)).toThrow()
    })
  })

  describe("how it looks", () => {
    it("takes the opacity of the element and of an ancestor", () => {
      const { surface } = mounted(() => (
        <div style={{ opacity: 0.5 }}>
          <webview testId="w" style={{ opacity: 0.5 }} />
        </div>
      ))
      expect(surface("w")!.opacity).toBeCloseTo(0.25, 2)
    })

    it("is cut to a uniform border radius, and square when the corners differ", () => {
      const round = mounted(() => <webview testId="w" style={{ borderRadius: 12 }} />)
      expect(round.surface("w")!.cornerRadius).toBe(12)
      open?.unmount()
      const uneven = mounted(() => <webview testId="w" style={{ borderRadius: 12, borderBottomLeftRadius: 4 }} />)
      expect(uneven.surface("w")!.cornerRadius).toBe(0)
    })
  })

  describe("layering", () => {
    it("draws what is painted before it below it, and what is painted after it above its surface", () => {
      const before = mounted(() => (
        <div style={{ position: "relative", width: 400, height: 300 }}>
          <div style={{ position: "absolute", left: 0, top: 0, width: 100, height: 100, backgroundColor: "red" }} />
          <webview testId="w" style={{ position: "absolute", left: 50, top: 50 }} />
        </div>
      ))
      const below = before.renderer.getCompositionLayers()
      expect(below.map((layer) => layer.kind)).toEqual(["gpui", "native", "gpui"])
      expect(below[0]!.primitives).toBeGreaterThan(0)
      expect(below[2]!.primitives).toBe(0)
      open?.unmount()
      const after = mounted(() => (
        <div style={{ position: "relative", width: 400, height: 300 }}>
          <webview testId="w" style={{ position: "absolute", left: 50, top: 50 }} />
          <div style={{ position: "absolute", left: 100, top: 100, width: 80, height: 80, backgroundColor: "red" }} />
        </div>
      ))
      const layers = after.renderer.getCompositionLayers()
      expect(layers.map((layer) => layer.kind)).toEqual(["gpui", "native", "gpui"])
      expect(layers[2]!.primitives).toBeGreaterThan(0)
    })
  })

  describe("applied to AppKit", () => {
    it("puts the container and the page where the frame recorded them", () => {
      const { renderer, idOf, surface } = mounted(() => (
        <div testId="scroller" style={{ height: 120, width: 400, overflow: "scroll", opacity: 0.5 }}>
          <div style={{ height: 40 }} />
          <webview testId="w" style={{ width: 300, height: 150, borderRadius: 10 }} />
          <div style={{ height: 600 }} />
        </div>
      ))
      const recorded = surface("w")!
      const applied = renderer.getWebviewNativeView(idOf("w"))
      expect(applied.hidden).toBe(false)
      expect([applied.x, applied.y, applied.width, applied.height]).toEqual([
        recorded.clipX,
        recorded.clipY,
        recorded.clipWidth,
        recorded.clipHeight,
      ])
      expect([applied.contentWidth, applied.contentHeight]).toEqual([300, 150])
      expect(applied.opacity).toBeCloseTo(0.5, 2)
      expect(applied.masked).toBe(true)
    })
  })

  describe("loading", () => {
    it("loads src when it is first painted, and again when it changes", () => {
      const [src, setSrc] = createSignal("https://example.test/a")
      const { app, commands } = mounted(() => <webview testId="w" src={src()} />)
      expect(commands("w")).toEqual(["loadURL https://example.test/a"])
      app.flushSync(() => setSrc("https://example.test/b"))
      expect(commands("w")).toEqual(["loadURL https://example.test/a", "loadURL https://example.test/b"])
    })

    it("reports a src that is no URL as a failed load", () => {
      const events: Event[] = []
      const { renderer, commands } = mounted(() => (
        <webview testId="w" src="not a url" onDidFailLoad={(event) => events.push(event as never)} />
      ))
      renderer.dispatchNativeEvents()
      expect(events).toHaveLength(1)
      expect(events[0]).toMatchObject({ errorCode: -1002, validatedUrl: "not a url" })
      expect(commands("w")).toEqual([])
    })
  })

  describe("methods", () => {
    it("forward to the page and read what it says", () => {
      let view!: HostElement
      const { renderer, idOf, commands } = mounted(() => <webview testId="w" ref={view} />)
      view.loadURL("https://example.test/x")
      view.reload()
      view.stop()
      view.goBack()
      view.goForward()
      expect(commands("w")).toEqual(["loadURL https://example.test/x", "reload", "stop", "goBack", "goForward"])

      expect(view.getURL()).toBe("")
      expect(view.getTitle()).toBe("")
      expect(view.canGoBack()).toBe(false)
      renderer.simulateWebviewNavigate(idOf("w"), "https://example.test/page")
      renderer.simulateWebviewTitle(idOf("w"), "A page")
      renderer.setWebviewHistory(idOf("w"), true, true)
      expect(view.getURL()).toBe("https://example.test/page")
      expect(view.getTitle()).toBe("A page")
      expect(view.canGoBack()).toBe(true)
      expect(view.canGoForward()).toBe(true)
    })

    it("refuses a URL without a scheme", () => {
      let view!: HostElement
      mounted(() => <webview testId="w" ref={view} />)
      expect(() => view.loadURL("example.test")).toThrow(/Invalid URL/)
    })

    it("resolves executeJavaScript with the result, undefined and null, and rejects with the error", async () => {
      let view!: HostElement
      const { renderer, idOf, commands } = mounted(() => <webview testId="w" ref={view} />)
      const result = view.executeJavaScript("document.title")
      const nothing = view.executeJavaScript("void 0")
      const nothingElse = view.executeJavaScript("null")
      const failing = view.executeJavaScript("nope()")
      expect(commands("w")).toEqual([
        "executeJavaScript document.title",
        "executeJavaScript void 0",
        "executeJavaScript null",
        "executeJavaScript nope()",
      ])
      renderer.settleWebviewScript(idOf("w"), { result: { a: [1, "two", null, true] } })
      renderer.settleWebviewScript(idOf("w"), {})
      renderer.settleWebviewScript(idOf("w"), { result: null })
      renderer.settleWebviewScript(idOf("w"), { error: "ReferenceError: Can't find variable: nope" })
      expect(await result).toEqual({ a: [1, "two", null, true] })
      expect(await nothing).toBeUndefined()
      expect(await nothingElse).toBeNull()
      await expect(failing).rejects.toThrow("ReferenceError: Can't find variable: nope")
    })
  })

  describe("events", () => {
    function record() {
      const events: Event[] = []
      const on = (type: string) => (event: any) =>
        events.push({ type, ...event, bubbles: event.bubbles, eventPhase: event.eventPhase })
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
      const { renderer, idOf } = mounted(() => <webview testId="w" {...props} />)
      const id = idOf("w")
      renderer.simulateWebviewStartLoading(id)
      renderer.simulateWebviewNavigate(id, "https://example.test/")
      renderer.simulateWebviewTitle(id, "Example", true)
      renderer.simulateWebviewTitle(id, "", false)
      renderer.simulateWebviewStopLoading(id)
      renderer.simulateWebviewFail(id, -1009, "offline", "https://example.test/x")
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
        errorDescription: "offline",
        validatedUrl: "https://example.test/x",
      })
    })

    it("do not bubble", () => {
      const { events, props } = record()
      const heard: string[] = []
      const { renderer, idOf } = mounted(() => (
        <div {...({ onDidNavigate: () => heard.push("did-navigate") } as object)} style={{ width: 400, height: 300 }}>
          <webview testId="w" {...props} />
        </div>
      ))
      renderer.simulateWebviewNavigate(idOf("w"), "https://example.test/")
      expect(events).toHaveLength(1)
      expect(events[0]).toMatchObject({ bubbles: false, eventPhase: 2 })
      expect(heard).toEqual([])
    })
  })

  describe("focus", () => {
    it("is a Tab stop, and takes focus from a script", () => {
      let view!: HostElement
      const focused: string[] = []
      const { renderer, idOf } = mounted(() => (
        <div>
          <input testId="before" style={{ width: 100, height: 20 }} />
          <webview testId="w" ref={view} onFocus={() => focused.push("focus")} onBlur={() => focused.push("blur")} />
          <input testId="after" style={{ width: 100, height: 20 }} />
        </div>
      ))
      renderer.focusElement(idOf("before"))
      renderer.simulateKeystrokes("Tab")
      expect(renderer.getFocusedElementId()).toBe(idOf("w"))
      expect(focused).toEqual(["focus"])
      renderer.simulateKeystrokes("Tab")
      expect(renderer.getFocusedElementId()).toBe(idOf("after"))
      expect(focused).toEqual(["focus", "blur"])
      view.focus()
      expect(renderer.getFocusedElementId()).toBe(idOf("w"))
      view.blur()
      expect(renderer.getFocusedElementId()).toBeNull()
    })
  })
})
