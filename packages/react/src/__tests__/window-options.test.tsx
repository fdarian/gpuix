/// The test window takes the options a live window does, through the same translation, so what
/// the offscreen window can represent behaves as it does live. See docs/reference.md's Testing section
/// for what it cannot.

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const describeRegions =
  hasNativeTestRenderer && process.platform !== "win32" ? describe : describe.skip

describeNative("test window options", () => {
  it("keeps the 1280x800 default size, and takes width and height", () => {
    const plain = createTestRoot()
    expect(plain.renderer.getWindowSize()).toEqual({ width: 1280, height: 800 })
    plain.unmount()

    const sized = createTestRoot({ width: 320, height: 240 })
    expect(sized.renderer.getWindowSize()).toEqual({ width: 320, height: 240 })
    sized.unmount()
  })

  it("rejects a size that is not a positive finite number", () => {
    expect(() => createTestRoot({ width: 0 })).toThrow(/width/)
    expect(() => createTestRoot({ height: Number.NaN })).toThrow(/height/)
  })

  it("titles the platform window", () => {
    const plain = createTestRoot()
    expect(plain.renderer.getWindowTitle()).toBe("GPUIX")
    plain.unmount()

    const titled = createTestRoot({ title: "Inspector" })
    expect(titled.renderer.getWindowTitle()).toBe("Inspector")
    titled.unmount()
  })

  it("opens a window with the other options, and ignores fullscreen", () => {
    const { renderer, render, unmount } = createTestRoot({
      width: 320,
      height: 240,
      minWidth: 200,
      minHeight: 150,
      resizable: false,
      transparent: true,
      windowBackground: "blurred",
      titlebarTransparent: true,
      trafficLightX: 20,
      trafficLightY: 30,
      show: false,
      appId: "gpuix.test",
      appName: "Test",
      fullscreen: true,
    })
    render(<div style={{ width: 10, height: 10 }} />)
    renderer.flush()
    expect(renderer.getWindowSize()).toEqual({ width: 320, height: 240 })
    unmount()
  })
})

describeRegions("titlebarTransparent", () => {
  function Bar() {
    const [pressed, setPressed] = React.useState(0)
    return (
      <div
        testId="bar"
        style={{ appRegion: "drag", width: 200, height: 40 }}
        onMouseDown={() => setPressed((count) => count + 1)}
      >
        <text testId="count">{`pressed ${pressed}`}</text>
      </div>
    )
  }

  it("makes appRegion drag the window, and the page never hears the press", () => {
    const { renderer, render, unmount } = createTestRoot({ titlebarTransparent: true })
    render(<Bar />)

    renderer.nativeSimulateMouseDown(100, 20)
    renderer.nativeSimulateMouseUp(100, 20)

    expect(renderer.takeSimulatedWindowActions()).toEqual(["move"])
    expect(renderer.getAllText()).toEqual(["pressed 0"])
    unmount()
  })

  it("leaves appRegion alone in a window that has its own titlebar", () => {
    const { renderer, render, unmount } = createTestRoot()
    render(<Bar />)

    renderer.nativeSimulateMouseDown(100, 20)
    renderer.nativeSimulateMouseUp(100, 20)

    expect(renderer.takeSimulatedWindowActions()).toEqual([])
    expect(renderer.getAllText()).toEqual(["pressed 1"])
    unmount()
  })
})
