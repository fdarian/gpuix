import { describe, expect, it } from "bun:test"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("Solid simulated mouse buttons", () => {
  function mount() {
    const log: string[] = []
    let box: { setPointerCapture(pointerId: number): void } | undefined
    const app = createTestRoot()
    app.render(() => (
      <div style={{ width: 600, height: 400 }}>
        <div
          ref={(element) => (box = element as unknown as typeof box)}
          style={{ width: 100, height: 100 }}
          onPointerDown={(event) => box!.setPointerCapture(event.pointerId)}
          onPointerMove={(event) => log.push(`move:${event.pressedButton}`)}
          onPointerCancel={(event) => log.push(event.type)}
          onLostPointerCapture={(event) => log.push(event.type)}
        />
      </div>
    ))
    return { app, log }
  }

  it("a move names the held button, so the capturer hears moves outside it", () => {
    const { app, log } = mount()

    app.renderer.nativeSimulateMouseDown(50, 50)
    app.renderer.nativeSimulateMouseMove(300, 300)
    app.renderer.nativeSimulateMouseMove(500, 380)

    expect(log).toEqual(["move:0", "move:0"])
  })

  it("a lost release ends the press and the capture", () => {
    const { app, log } = mount()
    app.renderer.nativeSimulateMouseDown(50, 50)
    app.renderer.nativeSimulateMouseMove(300, 300)
    log.length = 0

    app.renderer.simulateLostMouseUp()
    expect(log).toEqual([])
    app.renderer.nativeSimulateMouseMove(310, 300)

    expect(log).toEqual(["pointercancel", "lostpointercapture"])
  })

  it("a move after the release names no button", () => {
    const { app, log } = mount()
    app.renderer.nativeSimulateMouseDown(50, 50)
    app.renderer.nativeSimulateMouseUp(50, 50)
    log.length = 0

    app.renderer.nativeSimulateMouseMove(60, 50)

    expect(log).toEqual(["move:undefined"])
  })
})
