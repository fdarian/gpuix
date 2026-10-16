/// Window drag regions end to end: `appRegion` registers boxes with the window, a press in one
/// goes to the window and not the page, and the pointer over one is outside the window as far as
/// the page can tell. The test window records the window's moves instead of starting one.
///
/// Windows hands the whole region to the OS (HTCAPTION), so the press rules do not run there.

import { describe, it, expect, beforeEach } from "vitest"
import React from "react"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeRegions =
  hasNativeTestRenderer && process.platform !== "win32" ? describe : describe.skip

const onMac = process.platform === "darwin"
const doubleClickAction = onMac ? "titlebarDoubleClick" : "zoom"

describeRegions("appRegion", () => {
  let testRoot: ReturnType<typeof createTestRoot>
  let log: string[]

  beforeEach(() => {
    testRoot = createTestRoot({ titlebarTransparent: true })
    log = []
  })

  function listeners(name: string) {
    const record = (event: { type: string }) => {
      log.push(`${name}:${event.type}`)
    }
    return {
      onPointerDown: record,
      onMouseDown: record,
      onMouseUp: record,
      onClick: record,
      onContextMenu: record,
      onPointerMove: record,
      onMouseMove: record,
      onMouseEnter: record,
      onMouseLeave: record,
      onWheel: record,
    }
  }

  function windowActions(): string[] {
    return testRoot.renderer.takeSimulatedWindowActions()
  }

  /**
   * A 400x300 page. The bar (y 0..40) is a drag region with a no-drag button at its left
   * (x 0..50, y 0..30) that holds a small drag handle (x 0..10, y 0..10). The page body is
   * y 40..140. An overlay with no appRegion covers x 200..300 of the bar.
   */
  function Page() {
    return (
      <div style={{ position: "relative", width: 400, height: 300 }}>
        <div style={{ appRegion: "drag", width: 400, height: 40 }} {...listeners("bar")}>
          <div style={{ appRegion: "no-drag", width: 50, height: 30 }} {...listeners("button")}>
            <div style={{ appRegion: "drag", width: 10, height: 10 }} {...listeners("handle")} />
          </div>
        </div>
        <div style={{ width: 400, height: 100 }} {...listeners("body")} />
        <div
          style={{ position: "absolute", left: 200, top: 0, width: 100, height: 40 }}
          {...listeners("overlay")}
        />
      </div>
    )
  }

  describe("a press", () => {
    it("starts the window move and reaches the page not at all", () => {
      testRoot.render(<Page />)

      testRoot.renderer.nativeSimulateMouseDown(350, 20)
      testRoot.renderer.nativeSimulateMouseUp(350, 20)

      expect(log).toEqual([])
      expect(windowActions()).toEqual(["move"])
    })

    it("is not a click, a focus change or a mousedownoutside for anything", () => {
      const outside: string[] = []
      testRoot.render(
        <div style={{ position: "relative", width: 400, height: 300 }}>
          <div style={{ appRegion: "drag", width: 400, height: 40 }} />
          <div
            tabIndex={0}
            style={{ width: 100, height: 100 }}
            onMouseDownOutside={() => outside.push("outside")}
          />
        </div>,
      )
      testRoot.renderer.nativeSimulateMouseDown(50, 60)
      testRoot.renderer.nativeSimulateMouseUp(50, 60)
      outside.length = 0

      testRoot.renderer.nativeSimulateMouseDown(300, 20)

      expect(outside).toEqual([])
      expect(testRoot.renderer.getFocusedElementId()).not.toBeNull()
    })

    it("on the second click does what a titlebar does", () => {
      testRoot.render(<Page />)

      testRoot.renderer.nativeSimulateMouseDown(350, 20, 0, undefined, 1)
      testRoot.renderer.nativeSimulateMouseUp(350, 20, 0, undefined, 1)
      testRoot.renderer.nativeSimulateMouseDown(350, 20, 0, undefined, 2)
      testRoot.renderer.nativeSimulateMouseUp(350, 20, 0, undefined, 2)

      expect(log).toEqual([])
      expect(windowActions()).toEqual(["move", doubleClickAction])
    })

    it("goes to the page in a no-drag child, which gets its events as usual", () => {
      testRoot.render(<Page />)

      testRoot.renderer.nativeSimulateMouseDown(30, 20)
      testRoot.renderer.nativeSimulateMouseUp(30, 20)

      expect(windowActions()).toEqual([])
      expect(log.filter((entry) => entry.startsWith("button:"))).toEqual([
        "button:mouseenter",
        "button:pointerdown",
        "button:mousedown",
        "button:mouseup",
        "button:click",
      ])
    })

    it("drags again for a drag region inside a no-drag one", () => {
      testRoot.render(<Page />)

      testRoot.renderer.nativeSimulateMouseDown(5, 5)

      expect(log).toEqual([])
      expect(windowActions()).toEqual(["move"])
    })

    it("still drags under an absolutely positioned overlay that has no appRegion", () => {
      testRoot.render(<Page />)

      // The overlay is what the pointer hits here, and regions are rectangles, not hits.
      testRoot.renderer.nativeSimulateMouseDown(250, 20)

      expect(log).toEqual([])
      expect(windowActions()).toEqual(["move"])
    })

    it("outside every region is the page's, even when the window's earlier press never got its release", () => {
      testRoot.render(<Page />)

      // The OS drag loop can eat the release of the press the window took.
      testRoot.renderer.nativeSimulateMouseDown(350, 20)
      expect(windowActions()).toEqual(["move"])

      testRoot.renderer.nativeSimulateMouseDown(200, 100)
      testRoot.renderer.nativeSimulateMouseUp(200, 100)

      expect(log.filter((entry) => entry.startsWith("body:"))).toEqual([
        "body:mouseenter",
        "body:pointerdown",
        "body:mousedown",
        "body:mouseup",
        "body:click",
      ])
    })

    it("outside every region goes to the page", () => {
      testRoot.render(<Page />)

      testRoot.renderer.nativeSimulateMouseDown(200, 100)

      expect(windowActions()).toEqual([])
      expect(log.filter((entry) => entry.startsWith("body:"))).toEqual([
        "body:mouseenter",
        "body:pointerdown",
        "body:mousedown",
      ])
    })

    it("with another button held goes to the page", () => {
      testRoot.render(<Page />)

      testRoot.renderer.nativeSimulateMouseDown(200, 100, 2)
      log.length = 0
      testRoot.renderer.nativeSimulateMouseDown(350, 20)

      expect(windowActions()).toEqual([])
      expect(log).toContain("bar:mousedown")
    })

    it(
      onMac
        ? "of the secondary button reaches the page, as a context click does"
        : "of the secondary button opens the window menu",
      () => {
        testRoot.render(<Page />)

        testRoot.renderer.nativeSimulateMouseDown(350, 20, 2)

        if (onMac) {
          expect(windowActions()).toEqual([])
          expect(log).toContain("bar:mousedown")
          expect(log).toContain("bar:contextmenu")
        } else {
          expect(windowActions()).toEqual(["showMenu"])
          expect(log).toEqual([])
        }
      },
    )

    it(
      onMac ? "with ctrl reaches the page on macOS" : "with ctrl still moves the window",
      () => {
        testRoot.render(<Page />)

        testRoot.renderer.nativeSimulateMouseDown(350, 20, 0, "ctrl")

        if (onMac) {
          expect(windowActions()).toEqual([])
          expect(log).toContain("bar:mousedown")
        } else {
          expect(windowActions()).toEqual(["move"])
          expect(log).toEqual([])
        }
      },
    )
  })

  describe("the pointer", () => {
    it("leaves the page on entering a drag region and returns on leaving it", () => {
      testRoot.render(<Page />)

      testRoot.renderer.nativeSimulateMouseMove(200, 100)
      expect(log).toContain("body:mouseenter")
      log.length = 0

      testRoot.renderer.nativeSimulateMouseMove(350, 20)
      expect(log).toEqual(["body:mouseleave"])
      log.length = 0

      // No moves inside the region, to the bar or whatever lies over it.
      testRoot.renderer.nativeSimulateMouseMove(360, 25)
      testRoot.renderer.nativeSimulateMouseMove(250, 20)
      expect(log).toEqual([])

      testRoot.renderer.nativeSimulateMouseMove(200, 100)
      expect(log).toEqual(["body:mouseenter", "body:pointermove", "body:mousemove"])
    })

    it("is over the page again in a no-drag child", () => {
      testRoot.render(<Page />)

      testRoot.renderer.nativeSimulateMouseMove(350, 20)
      expect(log).toEqual([])

      testRoot.renderer.nativeSimulateMouseMove(30, 20)
      expect(log).toContain("button:mouseenter")
      expect(log).toContain("button:mousemove")
    })

    it("keeps its moves through a region for a press that started outside", () => {
      testRoot.render(<Page />)

      testRoot.renderer.nativeSimulateMouseDown(200, 100)
      log.length = 0

      testRoot.renderer.nativeSimulateMouseMove(350, 20)
      expect(log).toContain("bar:mousemove")
      expect(windowActions()).toEqual([])

      log.length = 0
      testRoot.renderer.nativeSimulateMouseUp(350, 20)
      expect(log).toContain("bar:mouseup")
    })

    it("does not move over the page while the window holds the press", () => {
      testRoot.render(<Page />)

      testRoot.renderer.nativeSimulateMouseDown(350, 20)
      testRoot.renderer.nativeSimulateMouseMove(200, 100)

      expect(log).toEqual([])
    })

    it("does not see a drag region that appears under it a frame late", () => {
      testRoot.render(
        <div style={{ width: 400, height: 300 }}>
          <div key="body" style={{ width: 400, height: 100 }} {...listeners("body")} />
        </div>,
      )
      testRoot.renderer.nativeSimulateMouseMove(350, 20)
      expect(log).toContain("body:mouseenter")
      log.length = 0

      // The bar is laid out over the still pointer, and is a region from the frame it first paints.
      testRoot.render(
        <div style={{ width: 400, height: 300 }}>
          <div
            key="bar"
            style={{ appRegion: "drag", width: 400, height: 40 }}
            {...listeners("bar")}
          />
          <div key="body" style={{ width: 400, height: 100 }} {...listeners("body")} />
        </div>,
      )
      // The test window delivers what a frame resynced with the next input. A wheel away from the
      // bar and the page changes nothing itself.
      testRoot.renderer.nativeSimulateScrollWheel(390, 250, 0, 10)

      expect(log).toEqual(["body:mouseleave"])
    })

    it("still sends the wheel to what is under it", () => {
      testRoot.render(<Page />)

      testRoot.renderer.nativeSimulateScrollWheel(350, 20, 0, 40)

      expect(log).toContain("bar:wheel")
    })
  })

  describe("inheritance", () => {
    it("inherits through a subtree that sets no value", () => {
      testRoot.render(
        <div style={{ appRegion: "drag", width: 400, height: 40 }}>
          <div style={{ width: 200, height: 20 }}>
            <div style={{ width: 100, height: 10 }} {...listeners("deep")} />
          </div>
        </div>,
      )

      testRoot.renderer.nativeSimulateMouseDown(50, 5)

      expect(windowActions()).toEqual(["move"])
      expect(log).toEqual([])
    })
  })
})
