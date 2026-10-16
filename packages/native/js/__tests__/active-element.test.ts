import { describe, expect, it } from "vitest"
import {
  document,
  elementOf,
  forgetFocusSource,
  installElementSource,
  noteFocusEvent,
} from "../events/window-target.js"

function root(elements: Record<number, object>) {
  const state = { focused: null as number | null }
  const renderer = { getFocusedElementId: () => state.focused }
  const remove = installElementSource(renderer, (id) => elements[id])
  return { renderer, state, remove }
}

describe("document.activeElement across roots", () => {
  it("is null with no root, and with a root that focuses nothing", () => {
    expect(document.activeElement).toBeNull()
    const only = root({ 1: {} })
    expect(document.activeElement).toBeNull()
    only.remove()
  })

  it("is the focused element's object", () => {
    const element = {}
    const only = root({ 1: element })
    only.state.focused = 1
    expect(document.activeElement).toBe(element)
    only.remove()
    expect(document.activeElement).toBeNull()
  })

  it("is the element of the root that heard a focus event last, among roots that have one", () => {
    const a = {}
    const b = {}
    const first = root({ 1: a })
    const second = root({ 1: b })
    first.state.focused = 1
    noteFocusEvent(first.renderer)
    second.state.focused = 1
    noteFocusEvent(second.renderer)
    expect(document.activeElement).toBe(b)
    noteFocusEvent(first.renderer)
    expect(document.activeElement).toBe(a)
    first.state.focused = null
    expect(document.activeElement).toBe(b)
    first.remove()
    second.remove()
  })

  it("throws when the root has no element for the focused id", () => {
    const only = root({})
    only.state.focused = 7
    expect(() => document.activeElement).toThrow("Element 7")
    only.remove()
  })

  it("lists no renderer that cannot report focus", () => {
    const remove = installElementSource({}, () => ({}))
    expect(document.activeElement).toBeNull()
    remove()
  })

  it("keeps naming elements for events after its window was taken over, but stops reporting focus", () => {
    const element = {}
    const only = root({ 1: element })
    only.state.focused = 1
    forgetFocusSource(only.renderer)
    expect(document.activeElement).toBeNull()
    expect(elementOf(only.renderer, 1)).toBe(element)
    only.remove()
    expect(() => elementOf(only.renderer, 1)).toThrow("No element source")
  })
})

describe("document.pointerLockElement", () => {
  it("is the object of the element the renderer reports as locked, and null otherwise", () => {
    const element = {}
    const state = { locked: null as number | null }
    const remove = installElementSource({ pointerLockElement: () => state.locked }, (id) =>
      id === 4 ? element : undefined
    )
    expect(document.pointerLockElement).toBeNull()
    state.locked = 4
    expect(document.pointerLockElement).toBe(element)
    state.locked = 9
    expect(() => document.pointerLockElement).toThrow(/Element 9 holds the pointer lock/)
    remove()
    expect(document.pointerLockElement).toBeNull()
  })

  it("is null for a renderer that cannot lock", () => {
    const remove = installElementSource({}, () => ({}))
    expect(document.pointerLockElement).toBeNull()
    remove()
  })
})
