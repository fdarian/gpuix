import { describe, expect, it } from "bun:test"
import { document } from "@gpuix/native/host"
import type { HostElement } from "../host.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("Solid pointer lock", () => {
  it("locks an element from its ref and reports the outcome on the root", () => {
    const log: string[] = []
    const targets: Array<object | null> = []
    let box!: HostElement
    const app = createTestRoot({
      onPointerLockChange: (event) => {
        targets.push(event.target)
        log.push(`change:${event.target === null ? "none" : "element"}`)
      },
      onPointerLockError: (event) => {
        targets.push(event.target)
        log.push(`error:${event.target === null ? "none" : "element"}`)
      },
    })
    app.render(() => (
      <div
        ref={box}
        style={{ width: 100, height: 100 }}
        onMouseMove={(event) => log.push(`${event.type}@${event.clientX},${event.clientY}`)}
      />
    ))
    app.renderer.nativeSimulateMouseMove(20, 30)
    log.length = 0

    box.requestPointerLock()
    app.renderer.nativeSimulateMouseMove(60, 70)
    app.renderer.simulatePointerLockLost()
    app.renderer.simulatePointerLockGranting(false)
    box.requestPointerLock()

    expect(log).toEqual(["change:element", "mousemove@20,30", "change:none", "error:element"])
    expect(targets).toHaveLength(3)
    expect(targets[0]).toBe(box)
    expect(targets[1]).toBeNull()
    expect(targets[2]).toBe(box)
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

  it("fires pointerlockchange and pointerlockerror at document listeners", () => {
    const log: string[] = []
    let box!: HostElement
    const app = createTestRoot()
    app.render(() => <div ref={box} style={{ width: 100, height: 100 }} />)
    const onChange = (event: { target: object | null }) =>
      log.push(`change:${event.target === box ? "box" : "none"}:${document.pointerLockElement === box}`)
    const onError = (event: { target: object | null }) =>
      log.push(`error:${event.target === box ? "box" : "none"}`)
    document.addEventListener("pointerlockchange", onChange as never)
    document.addEventListener("pointerlockerror", onError as never)

    box.requestPointerLock()
    app.renderer.exitPointerLock()
    app.renderer.simulatePointerLockGranting(false)
    box.requestPointerLock()

    document.removeEventListener("pointerlockchange", onChange as never)
    document.removeEventListener("pointerlockerror", onError as never)
    expect(log).toEqual(["change:box:true", "change:none:false", "error:box"])
  })
})
