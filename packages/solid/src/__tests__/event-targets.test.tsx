/**
 * `target`, `currentTarget` and `relatedTarget` are element objects, the same
 * ones a `ref` gives, for every event. `elementId` stays the numeric id.
 */
import { afterEach, describe, expect, it } from "bun:test"
import { document, window, type DomEvent } from "@gpuix/native/host"
import type { HostElement } from "../host.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("Solid event targets are elements", () => {
  const cleanups: Array<() => void> = []

  afterEach(() => {
    for (const cleanup of cleanups.splice(0)) cleanup()
  })

  function listen(target: typeof window, type: string, listener: (event: DomEvent) => void) {
    target.addEventListener(type, listener)
    cleanups.push(() => target.removeEventListener(type, listener))
  }

  const box = { width: 40, height: 20 }

  it("gives focus, blur, focusin and focusout the focused elements as target and relatedTarget", () => {
    let a!: HostElement
    let b!: HostElement
    const seen: Array<{ type: string; target: unknown; relatedTarget: unknown; currentTarget: unknown }> = []
    const record = (event: DomEvent) =>
      seen.push({
        type: event.type,
        target: event.target,
        relatedTarget: event.relatedTarget,
        currentTarget: event.currentTarget,
      })
    const app = createTestRoot()
    app.render(() => (
      <div style={{ width: 200, height: 100 }}>
        <div ref={a} testId="a" tabIndex={0} style={box} onFocus={record} onBlur={record} onFocusIn={record} onFocusOut={record} />
        <div ref={b} testId="b" tabIndex={0} style={box} onFocus={record} onBlur={record} onFocusIn={record} onFocusOut={record} />
      </div>
    ))

    app.renderer.focusElement(app.renderer.findByTestId("a")!.id)
    app.renderer.focusElement(app.renderer.findByTestId("b")!.id)

    expect(seen.map((entry) => entry.type)).toEqual(["focus", "focusin", "blur", "focusout", "focus", "focusin"])
    const expected = [
      [a, null],
      [a, null],
      [a, b],
      [a, b],
      [b, a],
      [b, a],
    ]
    seen.forEach((entry, index) => {
      expect(entry.target).toBe(expected[index]![0]!)
      expect(entry.currentTarget).toBe(expected[index]![0]!)
      expect(entry.relatedTarget).toBe(expected[index]![1]!)
    })
    app.unmount()
  })

  it("gives pointerover and pointerout the element on the other side as relatedTarget", () => {
    let first!: HostElement
    let second!: HostElement
    const seen: Array<[string, unknown, unknown]> = []
    const record = (event: DomEvent) => seen.push([event.type, event.target, event.relatedTarget])
    const app = createTestRoot()
    app.render(() => (
      <div style={{ width: 600, height: 400 }}>
        <div ref={first} style={{ width: 200, height: 100 }} onPointerOver={record} onPointerOut={record} />
        <div ref={second} style={{ width: 300, height: 100 }} onPointerOver={record} onPointerOut={record} />
      </div>
    ))

    app.renderer.nativeSimulateMouseMove(50, 50)
    app.renderer.nativeSimulateMouseMove(50, 150)

    expect(seen.map((entry) => entry[0])).toEqual(["pointerover", "pointerout", "pointerover"])
    expect(seen[0]![1]).toBe(first)
    expect(seen[0]![2]).toBeNull()
    expect(seen[1]![1]).toBe(first)
    expect(seen[1]![2]).toBe(second)
    expect(seen[2]![1]).toBe(second)
    expect(seen[2]![2]).toBe(first)
    app.unmount()
  })

  it("keeps target fixed and moves currentTarget up the path while a click bubbles", () => {
    let inner!: HostElement
    let outer!: HostElement
    const seen: Array<{ at: string; target: unknown; currentTarget: unknown; phase: number }> = []
    const record = (at: string) => (event: DomEvent) =>
      seen.push({ at, target: event.target, currentTarget: event.currentTarget, phase: event.eventPhase })
    const app = createTestRoot()
    app.render(() => (
      <div ref={outer} style={{ width: 400, height: 300 }} onClick={record("outer")}>
        <div ref={inner} style={{ width: 100, height: 100 }} onClick={record("inner")} />
      </div>
    ))

    app.renderer.nativeSimulateClick(50, 50)

    expect(seen.map((entry) => [entry.at, entry.phase])).toEqual([
      ["inner", 2],
      ["outer", 3],
    ])
    expect(seen[0]!.target).toBe(inner)
    expect(seen[0]!.currentTarget).toBe(inner)
    expect(seen[1]!.target).toBe(inner)
    expect(seen[1]!.currentTarget).toBe(outer)
    app.unmount()
  })

  it("lets a window listener see the target element while currentTarget is the window or document", () => {
    let button!: HostElement
    const seen: Array<[string, unknown, unknown]> = []
    const app = createTestRoot()
    app.render(() => <div ref={button} style={{ width: 100, height: 100 }} onClick={() => {}} />)
    listen(window, "click", (event) => seen.push(["window", event.target, event.currentTarget]))
    listen(document, "click", (event) => seen.push(["document", event.target, event.currentTarget]))

    app.renderer.nativeSimulateClick(50, 50)

    expect(seen.map((entry) => entry[0])).toEqual(["document", "window"])
    expect(seen[0]![1]).toBe(button)
    expect(seen[0]![2]).toBe(document)
    expect(seen[1]![1]).toBe(button)
    expect(seen[1]![2]).toBe(window)
    app.unmount()
  })

  it("keeps elementId numeric next to the element target", () => {
    let element!: HostElement
    let seen: DomEvent | undefined
    const app = createTestRoot()
    app.render(() => (
      <div ref={element} style={{ width: 100, height: 100 }} onClick={(event) => (seen = event)} />
    ))

    app.renderer.nativeSimulateClick(50, 50)

    expect(seen?.target).toBe(element)
    expect(seen?.elementId).toBe(element.id)
    app.unmount()
  })

  it("has a null target for a key with nothing focused", () => {
    const app = createTestRoot()
    app.render(() => <div style={{ width: 100, height: 100 }} />)
    const seen: Array<[unknown, unknown]> = []
    listen(window, "keydown", (event) => seen.push([event.target, event.currentTarget]))

    app.renderer.simulateKeystrokes("x")

    expect(seen).toHaveLength(1)
    expect(seen[0]![0]).toBeNull()
    expect(seen[0]![1]).toBe(window)
    app.unmount()
  })
})
