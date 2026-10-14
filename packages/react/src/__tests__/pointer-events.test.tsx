/// The DOM pointer event model end to end: the target the pointer hits, the
/// path an event takes, boundary events, the click family and the payload.
///
/// Every test drives the real native dispatcher through the GPU test renderer.

import fs from "fs"
import { describe, it, expect, beforeEach } from "vitest"
import React from "react"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { expectScreenshotsDiffer, expectScreenshotsEqual, SHOTS_DIR } from "./test-utils"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Seen = { type: string; target: number; currentTarget: number | null; phase: number }

describeNative("pointer events", () => {
  let testRoot: ReturnType<typeof createTestRoot>
  let log: string[]

  beforeEach(() => {
    testRoot = createTestRoot()
    log = []
  })

  function idOfWidth(width: number): number {
    const found = testRoot.renderer
      .findByType("div")
      .find((element) => element.style.width === width)
    if (!found) throw new Error(`No div with width ${width}`)
    return found.id
  }

  /** Records `name:type` for each event, so a test reads as an ordered list. */
  function record(name: string) {
    return (event: Seen) => {
      log.push(`${name}:${event.type}`)
    }
  }

  describe("target and path", () => {
    it("runs capture, target and bubble handlers along the target's ancestors", () => {
      testRoot.render(
        <div
          style={{ width: 400, height: 300 }}
          onClickCapture={record("outer-capture")}
          onClick={record("outer")}
        >
          <div style={{ width: 300, height: 200 }} onClick={record("middle")}>
            <div style={{ width: 100, height: 100 }} onClick={record("inner")} />
          </div>
        </div>,
      )

      testRoot.renderer.nativeSimulateClick(50, 50)

      expect(log).toEqual([
        "outer-capture:click",
        "inner:click",
        "middle:click",
        "outer:click",
      ])
    })

    it("reports the target and the path, target first", () => {
      const seen: { target: number; path: number[] | undefined }[] = []
      testRoot.render(
        <div
          style={{ width: 400, height: 300 }}
          onClick={(event: { target: number; path?: number[] }) =>
            seen.push({ target: event.target, path: event.path })
          }
        >
          <div style={{ width: 100, height: 100 }} />
        </div>,
      )
      const inner = idOfWidth(100)
      const outer = idOfWidth(400)

      testRoot.renderer.nativeSimulateClick(50, 50)

      expect(seen).toEqual([{ target: inner, path: [inner, outer] }])
    })

    it("stops propagation at the handler that asks", () => {
      testRoot.render(
        <div style={{ width: 400, height: 300 }} onClick={record("outer")}>
          <div
            style={{ width: 100, height: 100 }}
            onClick={(event: { stopPropagation(): void }) => {
              log.push("inner")
              event.stopPropagation()
            }}
          />
        </div>,
      )

      testRoot.renderer.nativeSimulateClick(50, 50)

      expect(log).toEqual(["inner"])
    })

    it("hits an element with no listener and no fill, and bubbles from it", () => {
      testRoot.render(
        <div style={{ width: 400, height: 300 }} onClick={record("outer")}>
          <div style={{ width: 100, height: 100 }} />
        </div>,
      )

      testRoot.renderer.nativeSimulateClick(50, 50)

      expect(log).toEqual(["outer:click"])
    })
  })

  describe("pointerEvents", () => {
    function Scene() {
      return (
        <div style={{ width: 600, height: 400, position: "relative" }}>
          <div style={{ position: "absolute", left: 0, top: 0, width: 200, height: 200 }}>
            <div
              style={{ width: 200, height: 200 }}
              onClick={record("behind")}
            />
          </div>
          <div
            style={{
              position: "absolute",
              left: 0,
              top: 0,
              width: 300,
              height: 300,
              pointerEvents: "none",
            }}
            onClick={record("none-root")}
          >
            <div style={{ width: 250, height: 250 }} onClick={record("inherits-none")}>
              <div
                style={{ width: 100, height: 100, pointerEvents: "auto" }}
                onClick={record("auto")}
              />
            </div>
          </div>
        </div>
      )
    }

    it("lets the pointer through a none subtree to what is under it", () => {
      testRoot.render(<Scene />)

      testRoot.renderer.nativeSimulateClick(150, 150)

      expect(log).toEqual(["behind:click"])
    })

    it("hits an auto descendant of a none subtree and bubbles through the none elements", () => {
      testRoot.render(<Scene />)

      testRoot.renderer.nativeSimulateClick(50, 50)

      expect(log).toEqual(["auto:click", "inherits-none:click", "none-root:click"])
    })
  })

  describe("anchored layers", () => {
    it("bubbles from an anchored layer to the element it is declared under", () => {
      const seen: { target: number; path: number[] }[] = []
      testRoot.render(
        <div
          style={{ width: 600, height: 400 }}
          onClick={(event: { target: number; path: number[] }) =>
            seen.push({ target: event.target, path: event.path })
          }
        >
          <div style={{ width: 100, height: 100 }}>
            <anchored position={{ x: 400, y: 250 }} anchor="topLeft" deferred priority={1}>
              <div style={{ width: 120, height: 60 }} />
            </anchored>
          </div>
        </div>,
      )
      const content = idOfWidth(120)
      const trigger = idOfWidth(100)
      const anchored = testRoot.renderer.findByType("anchored")[0]!.id
      const root = idOfWidth(600)

      // Far from the trigger, over the layer.
      testRoot.renderer.nativeSimulateClick(430, 270)

      expect(seen).toEqual([
        { target: content, path: [content, anchored, trigger, root] },
      ])
    })
  })

  describe("hover", () => {
    it("applies a hover style to the target and each of its ancestors", () => {
      function Card() {
        return (
          <div style={{ width: 600, height: 400, backgroundColor: "#0f1319" }}>
            <div
              style={{
                width: 300,
                height: 200,
                backgroundColor: "#2f3347",
                hover: { backgroundColor: "#f6d48b" },
              }}
            >
              <div style={{ width: 100, height: 100, backgroundColor: "#445" }} />
            </div>
          </div>
        )
      }
      testRoot.render(<Card />)
      const rest = `${SHOTS_DIR}/pointer-hover-rest.png`
      const onChild = `${SHOTS_DIR}/pointer-hover-child.png`
      const away = `${SHOTS_DIR}/pointer-hover-away.png`
      for (const file of [rest, onChild, away]) if (fs.existsSync(file)) fs.unlinkSync(file)

      testRoot.renderer.captureScreenshot(rest)
      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      testRoot.renderer.captureScreenshot(onChild)
      testRoot.renderer.nativeSimulateMouseMove(550, 350)
      testRoot.renderer.captureScreenshot(away)

      expectScreenshotsDiffer(rest, onChild)
      expectScreenshotsEqual(rest, away)
    })

    it("does not let an absolute child hide the pointer from its parent", () => {
      testRoot.render(
        <div
          style={{ width: 400, height: 300, position: "relative" }}
          onMouseEnter={record("parent")}
          onMouseLeave={record("parent")}
        >
          <div
            style={{
              position: "absolute",
              left: 50,
              top: 50,
              width: 100,
              height: 100,
              backgroundColor: "#336",
            }}
            onMouseEnter={record("child")}
          />
        </div>,
      )

      testRoot.renderer.nativeSimulateMouseMove(100, 100)

      expect(log).toEqual(["parent:mouseenter", "child:mouseenter"])
      testRoot.renderer.nativeSimulateMouseMove(300, 200)
      expect(log).toEqual(["parent:mouseenter", "child:mouseenter"])
    })
  })

  describe("boundary events", () => {
    function Nested() {
      const both = (name: string) => ({
        [`onMouseEnter`]: record(`${name}`),
        [`onMouseLeave`]: record(`${name}`),
        [`onMouseOver`]: record(`${name}`),
        [`onMouseOut`]: record(`${name}`),
      })
      return (
        <div style={{ width: 600, height: 400 }}>
          <div style={{ width: 300, height: 300 }} {...both("a")}>
            <div style={{ width: 150, height: 150 }} {...both("b")} />
          </div>
          <div style={{ width: 200, height: 50 }} {...both("c")} />
        </div>
      )
    }

    it("enters outermost first and leaves innermost first, with over and out bubbling", () => {
      testRoot.render(<Nested />)

      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      expect(log).toEqual([
        "b:mouseover",
        "a:mouseover",
        "a:mouseenter",
        "b:mouseenter",
      ])

      log.length = 0
      testRoot.renderer.nativeSimulateMouseMove(100, 320)
      expect(log).toEqual([
        "b:mouseout",
        "a:mouseout",
        "b:mouseleave",
        "a:mouseleave",
        "c:mouseover",
        "c:mouseenter",
      ])
    })

    it("sets relatedTarget to the element the pointer came from", () => {
      const related: (number | undefined)[] = []
      testRoot.render(
        <div style={{ width: 600, height: 400 }}>
          <div style={{ width: 200, height: 100 }} />
          <div
            style={{ width: 300, height: 100 }}
            onMouseEnter={(event: { relatedTarget?: number }) => related.push(event.relatedTarget)}
          />
        </div>,
      )
      const first = idOfWidth(200)

      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      testRoot.renderer.nativeSimulateMouseMove(50, 150)

      expect(related).toEqual([first])
    })
  })

  describe("click family", () => {
    function Rows() {
      return (
        <div style={{ width: 400, height: 200 }} onClick={record("list")}>
          <div style={{ width: 400, height: 100 }} onClick={record("row-a")} />
          <div style={{ width: 300, height: 100 }} onClick={record("row-b")} />
        </div>
      )
    }

    it("clicks the common ancestor when the press and the release are on different rows", () => {
      testRoot.render(<Rows />)

      testRoot.renderer.nativeSimulateMouseDown(50, 50)
      testRoot.renderer.nativeSimulateMouseUp(50, 150)

      expect(log).toEqual(["list:click"])
    })

    it("clicks the row when the press and release are on it", () => {
      testRoot.render(<Rows />)

      testRoot.renderer.nativeSimulateMouseDown(50, 150)
      testRoot.renderer.nativeSimulateMouseUp(60, 160)

      expect(log).toEqual(["row-b:click", "list:click"])
    })

    it("fires auxclick for the middle button and not click", () => {
      testRoot.render(
        <div
          style={{ width: 200, height: 100 }}
          onClick={record("box")}
          onAuxClick={record("box")}
        />,
      )

      testRoot.renderer.nativeSimulateClick(50, 50, 1)

      expect(log).toEqual(["box:auxclick"])
    })

    it("fires contextmenu on the secondary press, before the release", () => {
      testRoot.render(
        <div
          style={{ width: 200, height: 100 }}
          onMouseDown={record("box")}
          onContextMenu={record("box")}
          onMouseUp={record("box")}
        />,
      )

      testRoot.renderer.nativeSimulateMouseDown(50, 50, 2)
      expect(log).toEqual(["box:mousedown", "box:contextmenu"])
      testRoot.renderer.nativeSimulateMouseUp(50, 50, 2)
    })

    it("counts clicks in detail and fires dblclick after the second click", () => {
      const details: string[] = []
      testRoot.render(
        <div
          style={{ width: 200, height: 100 }}
          onClick={(event: { detail?: number }) => details.push(`click:${event.detail}`)}
          onDoubleClick={(event: { detail?: number }) =>
            details.push(`dblclick:${event.detail}`)
          }
        />,
      )

      testRoot.renderer.nativeSimulateMouseDown(50, 50, 0, undefined, 1)
      testRoot.renderer.nativeSimulateMouseUp(50, 50, 0, undefined, 1)
      testRoot.renderer.nativeSimulateMouseDown(50, 50, 0, undefined, 2)
      testRoot.renderer.nativeSimulateMouseUp(50, 50, 0, undefined, 2)

      expect(details).toEqual(["click:1", "click:2", "dblclick:2"])
    })

    it("fires the pointer event before the mouse event", () => {
      testRoot.render(
        <div
          style={{ width: 200, height: 100 }}
          onPointerDown={record("box")}
          onMouseDown={record("box")}
          onPointerUp={record("box")}
          onMouseUp={record("box")}
        />,
      )

      testRoot.renderer.nativeSimulateClick(50, 50)

      expect(log).toEqual([
        "box:pointerdown",
        "box:mousedown",
        "box:pointerup",
        "box:mouseup",
      ])
    })
  })

  describe("payload", () => {
    it("reports client and offset coordinates from the target's padding edge", () => {
      const seen: Record<string, unknown>[] = []
      testRoot.render(
        <div style={{ width: 600, height: 400 }}>
          <div
            style={{
              position: "absolute",
              left: 100,
              top: 80,
              width: 200,
              height: 100,
              borderWidth: 4,
              borderColor: "#fff",
            }}
            onMouseMove={(event: Record<string, unknown>) => seen.push(event)}
          />
        </div>,
      )

      testRoot.renderer.nativeSimulateMouseMove(130, 110)

      expect(seen).toHaveLength(1)
      expect(seen[0]).toMatchObject({
        clientX: 130,
        clientY: 110,
        x: 130,
        y: 110,
        // 130 - 100 left - 4 border
        offsetX: 26,
        offsetY: 26,
        pointerId: 1,
        pointerType: "mouse",
        buttons: 0,
      })
    })

    it("reports the held buttons as a bitmask", () => {
      const buttons: (number | undefined)[] = []
      testRoot.render(
        <div
          style={{ width: 200, height: 100 }}
          onMouseDown={(event: { buttons?: number }) => buttons.push(event.buttons)}
        />,
      )

      testRoot.renderer.nativeSimulateMouseDown(50, 50, 0)
      testRoot.renderer.nativeSimulateMouseDown(50, 50, 2)

      expect(buttons).toEqual([1, 3])
    })
  })
})
