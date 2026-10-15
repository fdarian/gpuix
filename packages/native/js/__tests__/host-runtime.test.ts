import { describe, expect, it } from "vitest"
import {
  createElementMethods,
  createMutationQueue,
  createRendererState,
  findRanges,
  observeAppearance,
  observeWindowInsets,
  observeWindowSize,
  observeSelectedText,
  readWindowInsets,
  readWindowSize,
} from "../host.js"
import type { MutationHost, NativeRenderer, WindowInsetsHost, WindowSizeHost } from "../host.js"

class RecordingRenderer implements MutationHost, WindowSizeHost, WindowInsetsHost {
  batches: unknown[][] = []
  fail = false
  width = 320
  height = 200

  applyBatch(json: string): number[] {
    if (this.fail) throw new Error("native failure")
    this.batches.push(JSON.parse(json))
    return [9]
  }

  getWindowSize() {
    return { width: this.width, height: this.height }
  }

  getWindowInsets() {
    return {
      safeArea: { top: 1, right: 2, bottom: 3, left: 4 },
      ime: { top: 0, right: 0, bottom: 20, left: 0 },
      effective: { top: 1, right: 2, bottom: 20, left: 4 },
    }
  }
}

describe("shared host runtime", () => {
  it("clicks the element through the renderer, and throws where the renderer cannot", () => {
    const clicked: number[] = []
    const renderer = { clickElement: (id: number) => clicked.push(id) } as unknown as NativeRenderer
    createElementMethods(renderer, 5).click()
    expect(clicked).toEqual([5])
    expect(() => createElementMethods({} as NativeRenderer, 5).click()).toThrow("clickElement")
  })

  it("requests the pointer lock for its own element", () => {
    const locked: number[] = []
    const renderer = { requestPointerLock: (id: number) => locked.push(id) } as unknown as NativeRenderer
    createElementMethods(renderer, 5).requestPointerLock()
    expect(locked).toEqual([5])
    expect(() => createElementMethods({} as NativeRenderer, 5).requestPointerLock()).toThrow(
      "requestPointerLock"
    )
  })

  it("queues mutations from an applyBatch-only host", () => {
    const batches: unknown[][] = []
    const renderer: MutationHost = {
      applyBatch(json) {
        batches.push(JSON.parse(json))
        return []
      },
    }
    const mutations = createMutationQueue(renderer)
    mutations.createElement(1, "div")
    mutations.flushMutations()
    expect(batches).toEqual([[["createElement", 1, "div"]]])
  })

  it("reads window size from a size-only host", () => {
    const renderer: WindowSizeHost = {
      getWindowSize: () => ({ width: 12, height: 8 }),
    }
    expect(readWindowSize(renderer)).toEqual({ width: 12, height: 8 })
  })

  it("reads insets from a size-and-insets host", () => {
    const renderer: WindowSizeHost & WindowInsetsHost = {
      getWindowSize: () => ({ width: 100, height: 50 }),
      getWindowInsets: () => ({
        safeArea: { top: 1, right: 0, bottom: 0, left: 0 },
        ime: { top: 0, right: 0, bottom: 10, left: 0 },
        effective: { top: 1, right: 0, bottom: 10, left: 0 },
      }),
    }
    expect(readWindowInsets(renderer)).toMatchObject({
      keyboardTop: 40,
      keyboardVisible: true,
      visibleHeight: 39,
    })
  })

  it("retains failed mutations and cleans handlers after success", () => {
    const renderer = new RecordingRenderer()
    const removed: number[] = []
    const mutations = createMutationQueue(renderer, (ids) => removed.push(...ids))

    mutations.createElement(1, "div")
    mutations.destroyElement(9)
    renderer.fail = true
    expect(() => mutations.flushMutations()).toThrow("native failure")
    expect(mutations.pending).toBe(2)
    expect(removed).toEqual([])

    renderer.fail = false
    mutations.flushMutations()
    expect(renderer.batches).toMatchInlineSnapshot(`
      [
        [
          [
            "createElement",
            1,
            "div",
          ],
          [
            "destroyElement",
            9,
          ],
        ],
      ]
    `)
    expect(removed).toEqual([9])
  })

  it("applies the rest of a batch without the style native rejects, and still reports it", () => {
    const rejecting = {
      batches: [] as unknown[][],
      applyBatch(json: string): number[] {
        const batch: unknown[][] = JSON.parse(json)
        const index = batch.findIndex(
          (mutation) => mutation[0] === "setStyle" && JSON.stringify(mutation[2]).includes("banana")
        )
        if (index >= 0) throw new Error(`Batch op ${index} setStyle parse error: bad animation`)
        this.batches.push(batch)
        return []
      },
    }
    const mutations = createMutationQueue(rejecting)

    mutations.createElement(1, "div")
    mutations.setStyle(1, { animation: "spin banana", width: 10 })
    mutations.createElement(2, "div")
    mutations.setStyle(2, { width: 20 })
    expect(() => mutations.flushMutations()).toThrow("Batch op 1 setStyle parse error")

    expect(mutations.pending).toBe(0)
    expect(rejecting.batches).toEqual([
      [
        ["createElement", 1, "div"],
        ["createElement", 2, "div"],
        ["setStyle", 2, { width: 20 }],
      ],
    ])
  })

  it("drops every rejected style from a batch, whatever the shifted indices", () => {
    const rejecting = {
      batches: [] as unknown[][],
      applyBatch(json: string): number[] {
        const batch: unknown[][] = JSON.parse(json)
        const index = batch.findIndex(
          (mutation) => mutation[0] === "setStyle" && JSON.stringify(mutation[2]).includes("banana")
        )
        if (index >= 0) throw new Error(`Batch op ${index} setStyle parse error: bad animation`)
        this.batches.push(batch)
        return []
      },
    }
    const mutations = createMutationQueue(rejecting)

    mutations.setStyle(1, { animation: "spin banana" })
    mutations.setStyle(2, { width: 20 })
    mutations.setStyle(3, { animation: "banana" })
    mutations.setStyle(4, { animation: "banana split" })
    mutations.setStyle(5, { width: 50 })
    expect(() => mutations.flushMutations()).toThrow("Batch op 0 setStyle parse error")

    expect(mutations.pending).toBe(0)
    expect(rejecting.batches).toEqual([
      [
        ["setStyle", 2, { width: 20 }],
        ["setStyle", 5, { width: 50 }],
      ],
    ])
  })

  it("owns one live root and rejects stale window events", () => {
    const renderer = new RecordingRenderer()
    const state = createRendererState(renderer)
    const received: string[] = []
    const first = state.attach({
      onWindowKeyDown: (event) => received.push(event.key ?? ""),
    })

    expect(
      state.dispatch({
        elementId: first.windowKeyEventId,
        eventType: "keydown",
        key: "a",
      })
    ).toBe(true)
    first.detach()

    const second = state.attach({
      onWindowKeyDown: (event) => received.push(event.key ?? ""),
    })
    expect(
      state.dispatch({
        elementId: first.windowKeyEventId,
        eventType: "keydown",
        key: "stale",
      })
    ).toBe(false)
    expect(second.windowKeyEventId).not.toBe(first.windowKeyEventId)
    expect(received).toEqual(["a"])
  })

  it("observes window geometry and stops polling", async () => {
    const renderer = new RecordingRenderer()
    const sizes: unknown[] = []
    const insets: unknown[] = []
    const stopSize = observeWindowSize(renderer, (value) => sizes.push(value), {
      intervalMs: 16,
    })
    const stopInsets = observeWindowInsets(renderer, (value) => insets.push(value), {
      intervalMs: false,
    })

    renderer.width = 640
    await new Promise((resolve) => setTimeout(resolve, 25))
    stopSize()
    stopInsets()

    expect(sizes).toEqual([
      { width: 320, height: 200 },
      { width: 640, height: 200 },
    ])
    expect(insets).toMatchInlineSnapshot(`
      [
        {
          "effective": {
            "bottom": 20,
            "left": 4,
            "right": 2,
            "top": 1,
          },
          "ime": {
            "bottom": 20,
            "left": 0,
            "right": 0,
            "top": 0,
          },
          "keyboardTop": 180,
          "keyboardVisible": true,
          "safeArea": {
            "bottom": 3,
            "left": 4,
            "right": 2,
            "top": 1,
          },
          "visibleHeight": 179,
        },
      ]
    `)
  })

  it("shares the native-compatible text matcher", () => {
    expect(findRanges({ text: "One one stone", query: "one", wholeWord: true }))
      .toEqual([[0, 3], [4, 7]])
  })

  it("observes selected text and removes its callback", () => {
    const renderer = new RecordingRenderer() as RecordingRenderer & {
      getSelectedText(): string | null
    }
    renderer.getSelectedText = () => "initial"
    let listener: ((event: { value?: string }) => void) | undefined
    const values: Array<string | null> = []
    const stop = observeSelectedText(
      renderer,
      (next) => {
        listener = next
        return () => { listener = undefined }
      },
      (value) => values.push(value)
    )
    listener?.({ value: "changed" })
    stop()
    listener?.({ value: "ignored" })
    expect(values).toEqual(["initial", "changed"])
  })

  it("observes the appearance, seeded from the renderer", () => {
    const renderer = new RecordingRenderer() as RecordingRenderer & {
      getAppearance(): string
    }
    renderer.getAppearance = () => "light"
    let listener: ((event: { value?: string }) => void) | undefined
    const values: string[] = []
    const stop = observeAppearance(
      renderer,
      (next) => {
        listener = next as typeof listener
        return () => { listener = undefined }
      },
      (value) => values.push(value)
    )
    listener?.({ value: "dark" })
    stop()
    expect(listener).toBeUndefined()
    expect(values).toEqual(["light", "dark"])
  })

  it("throws on an appearance that is not light or dark", () => {
    const renderer = new RecordingRenderer() as RecordingRenderer & {
      getAppearance(): string
    }
    renderer.getAppearance = () => "light"
    let listener: ((event: { value?: string }) => void) | undefined
    observeAppearance(
      renderer,
      (next) => {
        listener = next as typeof listener
        return () => {}
      },
      () => {}
    )
    expect(() => listener?.({ value: "vibrantDark" })).toThrow(/light" or "dark"/)
    expect(() => listener?.({})).toThrow(/light" or "dark"/)
  })

  it("routes appearanceChange to the root handler by its own event id", () => {
    const state = createRendererState(new RecordingRenderer())
    const values: Array<string | undefined> = []
    const binding = state.attach({
      onAppearanceChange: (event) => values.push(event.value),
    })
    const event = { elementId: binding.windowAppearanceEventId, eventType: "appearanceChange", value: "dark" }
    expect(state.dispatch(event)).toBe(true)
    expect(state.dispatch({ ...event, elementId: binding.windowAppearanceEventId + 1 })).toBe(false)
    expect(values).toEqual(["dark"])
  })

  it("drops appearanceChange when no root handler is set", () => {
    const state = createRendererState(new RecordingRenderer())
    const binding = state.attach({})
    expect(
      state.dispatch({
        elementId: binding.windowAppearanceEventId,
        eventType: "appearanceChange",
        value: "dark",
      })
    ).toBe(false)
  })

  it("routes pointer lock events to their root handlers, with no element id", () => {
    const state = createRendererState(new RecordingRenderer())
    const seen: string[] = []
    state.attach({
      onPointerLockChange: (event) => seen.push(`change:${event.elementId}:${event.target}`),
      onPointerLockError: (event) => seen.push(`error:${event.elementId}:${event.target}`),
    })
    expect(state.dispatch({ elementId: 0, eventType: "pointerlockchange", target: 7 })).toBe(true)
    expect(state.dispatch({ elementId: 0, eventType: "pointerlockerror", target: 8 })).toBe(true)
    expect(seen).toEqual(["change:0:7", "error:0:8"])
  })

  it("drops a pointer lock event nobody handles", () => {
    const state = createRendererState(new RecordingRenderer())
    state.attach({ onPointerLockError: () => {} })
    expect(state.dispatch({ elementId: 0, eventType: "pointerlockchange" })).toBe(false)
  })
})
