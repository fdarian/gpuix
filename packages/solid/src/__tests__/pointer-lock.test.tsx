import { describe, expect, it } from "bun:test"
import type { HostElement } from "../host.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("Solid pointer lock", () => {
  it("locks an element from its ref and reports the outcome on the root", () => {
    const log: string[] = []
    let box!: HostElement
    const app = createTestRoot({
      onPointerLockChange: (event) => log.push(`change:${event.target ?? "none"}`),
      onPointerLockError: (event) => log.push(`error:${event.target}`),
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

    expect(log).toEqual([
      `change:${box.id}`,
      "mousemove@20,30",
      "change:none",
      `error:${box.id}`,
    ])
  })
})
