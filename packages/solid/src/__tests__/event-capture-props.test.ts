import { describe, expect, it } from "bun:test"
import type { MutationRenderer, NativeRenderer } from "@gpuix/native/host"
import { createHostElement, HostRoot, insertHostNode, setHostProperty } from "../host.js"

function setup() {
  const listeners: Array<[number, string, boolean]> = []
  const mutations: MutationRenderer = {
    createElement() {},
    destroyElement: () => [],
    appendChild() {},
    insertBefore() {},
    setStyle() {},
    setText() {},
    setEventListener(id, eventType, hasHandler) {
      listeners.push([id, eventType, hasHandler])
    },
    setRoot() {},
    setKeyboardFocusDim() {},
    setCustomProp() {},
    flushMutations() {},
  }
  let nextId = 0
  const root = new HostRoot({
    mutations,
    nativeRenderer: {} as NativeRenderer,
    eventHandlers: new Map(),
    allocateId: () => ++nextId,
  })
  const element = createHostElement("div")
  return { root, element, listeners }
}

const handler = () => {}

describe("Solid host event listeners", () => {
  it("registers DOM lowercase types and keys capture handlers apart", () => {
    const { root, element, listeners } = setup()
    setHostProperty(element, "onPointerDown", handler)
    setHostProperty(element, "onClickCapture", handler)
    insertHostNode(root, element)
    expect(listeners).toEqual([
      [element.id, "pointerdown", true],
      [element.id, "click", true],
    ])
    expect([...(root.eventHandlers.get(element.id)?.keys() ?? [])]).toEqual([
      "pointerdown",
      "clickCapture",
    ])
  })

  it("tells native once while both phases are present, and stops when both are gone", () => {
    const { root, element, listeners } = setup()
    insertHostNode(root, element)
    setHostProperty(element, "onDrop", handler)
    setHostProperty(element, "onDropCapture", handler)
    expect(listeners).toEqual([[element.id, "drop", true]])
    setHostProperty(element, "onDrop", undefined, handler)
    expect(listeners).toHaveLength(1)
    setHostProperty(element, "onDropCapture", undefined, handler)
    expect(listeners).toEqual([[element.id, "drop", true], [element.id, "drop", false]])
  })

  it("swaps a handler without telling native", () => {
    const { root, element, listeners } = setup()
    insertHostNode(root, element)
    const next = () => {}
    setHostProperty(element, "onContextMenu", handler)
    setHostProperty(element, "onContextMenu", next, handler)
    expect(listeners).toHaveLength(1)
    expect(root.eventHandlers.get(element.id)?.get("contextmenu")).toBe(next)
  })

  it("keeps onMouseDownOutside as the gpuix type", () => {
    const { root, element, listeners } = setup()
    insertHostNode(root, element)
    setHostProperty(element, "onMouseDownOutside", handler)
    expect(listeners).toEqual([[element.id, "mousedownoutside", true]])
  })
})
