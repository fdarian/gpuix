import { describe, expect, it } from "vitest"
import { hostConfig } from "../reconciler/host-config.js"
import type { Container, HostContext, MutationRenderer, Props } from "../types/host.js"

function setup() {
  const listeners: Array<[number, string, boolean]> = []
  const renderer: MutationRenderer = {
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
    requestFlush() {},
    flushMutations() {},
  }
  const container: Container = {
    nativeRenderer: {},
    renderer,
    ids: { nextElementId: 0 },
    eventHandlers: new Map(),
    windowKeyEventHandlers: {},
    windowKeyEventId: 0,
    windowSelectionEventId: 0,
    motionBindings: new Map(),
    motionReleases: [],
    instances: new Map(),
  }
  const create = (props: Props) => {
    const instance = hostConfig.createInstance(
      "div",
      props,
      container,
      null as unknown as HostContext
    )
    hostConfig.appendChildToContainer(container, instance)
    return instance
  }
  const update = (instance: ReturnType<typeof create>, before: Props, after: Props) =>
    hostConfig.commitUpdate(instance, "div", before, after, null)
  return { container, listeners, create, update }
}

const handler = () => {}

describe("React host event listeners", () => {
  it("registers DOM lowercase types and keys capture handlers apart", () => {
    const { container, listeners, create } = setup()
    const instance = create({ onPointerDown: handler, onClickCapture: handler })
    expect(listeners).toEqual([
      [instance.id, "pointerdown", true],
      [instance.id, "click", true],
    ])
    const handlers = container.eventHandlers.get(instance.id)
    expect([...(handlers?.keys() ?? [])]).toEqual(["pointerdown", "clickCapture"])
  })

  it("asks native for a type once while both phases are present", () => {
    const { listeners, create } = setup()
    const instance = create({ onClick: handler, onClickCapture: handler })
    expect(listeners).toEqual([[instance.id, "click", true]])
  })

  it("keeps the native listener until both phases are gone", () => {
    const { listeners, create, update } = setup()
    const both: Props = { onDrop: handler, onDropCapture: handler }
    const instance = create(both)
    const captureOnly: Props = { onDropCapture: handler }
    update(instance, both, captureOnly)
    expect(listeners).toEqual([[instance.id, "drop", true]])
    update(instance, captureOnly, {})
    expect(listeners).toEqual([[instance.id, "drop", true], [instance.id, "drop", false]])
  })

  it("adds a capture handler to an element that already bubbles", () => {
    const { listeners, create, update } = setup()
    const before: Props = { onMouseMove: handler }
    const instance = create(before)
    update(instance, before, { onMouseMove: handler, onMouseMoveCapture: handler })
    expect(listeners).toEqual([[instance.id, "mousemove", true]])
  })

  it("swaps a handler without telling native", () => {
    const { listeners, container, create, update } = setup()
    const before: Props = { onContextMenu: handler }
    const instance = create(before)
    const next = () => {}
    update(instance, before, { onContextMenu: next })
    expect(listeners).toHaveLength(1)
    expect(container.eventHandlers.get(instance.id)?.get("contextmenu")).toBe(next)
  })

  it("keeps onMouseDownOutside as the gpuix type", () => {
    const { listeners, create } = setup()
    const instance = create({ onMouseDownOutside: handler })
    expect(listeners).toEqual([[instance.id, "mousedownoutside", true]])
  })
})
