import { describe, expect, it } from "bun:test"
import { document, window } from "@ghia/native/host"
import type { HostElement } from "../host.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("Solid pointer lock", () => {
  it("locks an element from its ref and reports the outcome at document", () => {
    const log: string[] = []
    let box!: HostElement
    const app = createTestRoot()
    app.render(() => (
      <div
        ref={box}
        style={{ width: 100, height: 100 }}
        onMouseMove={(event) => log.push(`${event.type}@${event.clientX},${event.clientY}`)}
      />
    ))
    const holder = () => (document.pointerLockElement === box ? "box" : "none")
    const onChange = () => log.push(`change:${holder()}`)
    const onError = () => log.push(`error:${holder()}`)
    document.addEventListener("pointerlockchange", onChange as never)
    document.addEventListener("pointerlockerror", onError as never)
    app.renderer.nativeSimulateMouseMove(20, 30)
    log.length = 0

    box.requestPointerLock()
    app.renderer.nativeSimulateMouseMove(60, 70)
    app.renderer.simulatePointerLockLost()
    app.renderer.simulatePointerLockGranting(false)
    box.requestPointerLock()

    document.removeEventListener("pointerlockchange", onChange as never)
    document.removeEventListener("pointerlockerror", onError as never)
    expect(log).toEqual(["change:box", "mousemove@20,30", "change:none", "error:none"])
  })

  it("reads movementX and movementY as 0 on every mouse event but a move", () => {
    const seen: string[] = []
    const app = createTestRoot()
    app.render(() => (
      <div
        style={{ width: 100, height: 100 }}
        onMouseMove={(event) => seen.push(`${event.type}:${event.movementX},${event.movementY}`)}
        onMouseDown={(event) => seen.push(`${event.type}:${event.movementX},${event.movementY}`)}
        onClick={(event) => seen.push(`${event.type}:${event.movementX},${event.movementY}`)}
      />
    ))

    app.renderer.nativeSimulateMouseMove(20, 30, undefined, { x: 6, y: -2 })
    app.renderer.nativeSimulateClick(20, 30)

    expect(seen).toEqual(["mousemove:6,-2", "mousedown:0,0", "click:0,0"])
  })

  it("targets document with both events, and window hears them after", () => {
    const log: string[] = []
    let box!: HostElement
    const app = createTestRoot()
    app.render(() => <div ref={box} style={{ width: 100, height: 100 }} />)
    const record = (name: string) => (event: { type: string; target: object | null }) =>
      log.push(`${name}:${event.type}:${event.target === document}`)
    const atDocument = record("document")
    const atWindow = record("window")
    for (const type of ["pointerlockchange", "pointerlockerror"]) {
      document.addEventListener(type, atDocument as never)
      window.addEventListener(type, atWindow as never)
    }

    app.renderer.simulatePointerLockGranting(false)
    box.requestPointerLock()
    app.renderer.simulatePointerLockGranting(true)
    box.requestPointerLock()

    for (const type of ["pointerlockchange", "pointerlockerror"]) {
      document.removeEventListener(type, atDocument as never)
      window.removeEventListener(type, atWindow as never)
    }
    expect(log).toEqual([
      "document:pointerlockerror:true",
      "window:pointerlockerror:true",
      "document:pointerlockchange:true",
      "window:pointerlockchange:true",
    ])
  })
})
