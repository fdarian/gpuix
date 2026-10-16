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

type Seen = { type: string; target: object; currentTarget: object | null; phase: number }

describeNative("pointer events", () => {
  let testRoot: ReturnType<typeof createTestRoot>
  let log: string[]

  beforeEach(() => {
    testRoot = createTestRoot()
    log = []
  })

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

    it("reports the target element and the path, target first", () => {
      let inner: unknown
      let outer: unknown
      const seen: { target: object; path: object[] }[] = []
      testRoot.render(
        <div
          ref={(element) => (outer = element)}
          style={{ width: 400, height: 300 }}
          onClick={(event: { target: object; composedPath(): object[] }) =>
            seen.push({ target: event.target, path: event.composedPath() })
          }
        >
          <div ref={(element) => (inner = element)} style={{ width: 100, height: 100 }} />
        </div>,
      )

      testRoot.renderer.nativeSimulateClick(50, 50)

      expect(seen).toHaveLength(1)
      expect(seen[0].target).toBe(inner)
      expect(seen[0].path).toEqual([inner, outer])
      expect(seen[0].path[0]).toBe(inner)
      expect(seen[0].path[1]).toBe(outer)
    })

    it("keeps target fixed and moves currentTarget up the path while a click bubbles", () => {
      let inner: unknown
      let outer: unknown
      const seen: Array<{ at: string; target: unknown; currentTarget: unknown; phase: number }> = []
      const record = (at: string) => (event: { target: unknown; currentTarget: unknown; eventPhase: number }) =>
        seen.push({ at, target: event.target, currentTarget: event.currentTarget, phase: event.eventPhase })
      testRoot.render(
        <div ref={(element) => (outer = element)} style={{ width: 400, height: 300 }} onClick={record("outer")}>
          <div ref={(element) => (inner = element)} style={{ width: 100, height: 100 }} onClick={record("inner")} />
        </div>,
      )

      testRoot.renderer.nativeSimulateClick(50, 50)

      expect(seen.map((entry) => [entry.at, entry.phase])).toEqual([
        ["inner", 2],
        ["outer", 3],
      ])
      expect(seen[0].target).toBe(inner)
      expect(seen[0].currentTarget).toBe(inner)
      expect(seen[1].target).toBe(inner)
      expect(seen[1].currentTarget).toBe(outer)
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

    it("targets a virtual-list where no row is, and bubbles from it", () => {
      let list: unknown
      let root: unknown
      const seen: { target: object; path: object[] }[] = []
      testRoot.render(
        <div
          ref={(element) => (root = element)}
          style={{ width: 400, height: 300 }}
          onClick={(event: { target: object; composedPath(): object[] }) =>
            seen.push({ target: event.target, path: event.composedPath() })
          }
        >
          <virtual-list ref={(element) => (list = element)} estimatedItemHeight={20} style={{ width: 200, height: 160 }}>
            <div style={{ width: 200, height: 20, flexShrink: 0 }} />
            <div style={{ width: 200, height: 20, flexShrink: 0 }} />
          </virtual-list>
        </div>,
      )

      testRoot.renderer.nativeSimulateClick(100, 120)

      expect(seen).toHaveLength(1)
      expect(seen[0].target).toBe(list)
      expect(seen[0].path).toEqual([list, root])
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
      let content: unknown
      let trigger: unknown
      let anchored: unknown
      let root: unknown
      const seen: { target: object; path: object[] }[] = []
      testRoot.render(
        <div
          ref={(element) => (root = element)}
          style={{ width: 600, height: 400 }}
          onClick={(event: { target: object; composedPath(): object[] }) =>
            seen.push({ target: event.target, path: event.composedPath() })
          }
        >
          <div ref={(element) => (trigger = element)} style={{ width: 100, height: 100 }}>
            <anchored
              ref={(element) => (anchored = element)}
              position={{ x: 400, y: 250 }}
              anchor="topLeft"
              deferred
              priority={1}
            >
              <div ref={(element) => (content = element)} style={{ width: 120, height: 60 }} />
            </anchored>
          </div>
        </div>,
      )

      // Far from the trigger, over the layer.
      testRoot.renderer.nativeSimulateClick(430, 270)

      expect(seen).toHaveLength(1)
      expect(seen[0].target).toBe(content)
      expect(seen[0].path).toEqual([content, anchored, trigger, root])
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

    it("sets relatedTarget to the element the pointer came from, and null when it came from outside", () => {
      let first: unknown
      let second: unknown
      const related: Array<[unknown, unknown]> = []
      testRoot.render(
        <div style={{ width: 600, height: 400 }}>
          <div ref={(element) => (first = element)} style={{ width: 200, height: 100 }} />
          <div
            ref={(element) => (second = element)}
            style={{ width: 300, height: 100 }}
            onMouseEnter={(event: { target: unknown; relatedTarget: unknown }) =>
              related.push([event.target, event.relatedTarget])
            }
          />
        </div>,
      )

      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      testRoot.renderer.nativeSimulateMouseMove(50, 150)

      expect(related).toHaveLength(1)
      expect(related[0][0]).toBe(second)
      expect(related[0][1]).toBe(first)
    })

    it("gives pointerover and pointerout the element on the other side as relatedTarget", () => {
      let first: unknown
      let second: unknown
      const seen: Array<[string, unknown, unknown]> = []
      const record = (event: { type: string; target: unknown; relatedTarget: unknown }) =>
        seen.push([event.type, event.target, event.relatedTarget])
      testRoot.render(
        <div style={{ width: 600, height: 400 }}>
          <div
            ref={(element) => (first = element)}
            style={{ width: 200, height: 100 }}
            onPointerOver={record}
            onPointerOut={record}
          />
          <div
            ref={(element) => (second = element)}
            style={{ width: 300, height: 100 }}
            onPointerOver={record}
            onPointerOut={record}
          />
        </div>,
      )

      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      testRoot.renderer.nativeSimulateMouseMove(50, 150)

      expect(seen.map((entry) => entry[0])).toEqual(["pointerover", "pointerout", "pointerover"])
      expect(seen[0][1]).toBe(first)
      expect(seen[0][2]).toBeNull()
      expect(seen[1][1]).toBe(first)
      expect(seen[1][2]).toBe(second)
      expect(seen[2][1]).toBe(second)
      expect(seen[2][2]).toBe(first)
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

    it("clicks from a host ref like HTMLElement.click(): detail 0, no pointer, bubbles", () => {
      const seen: Array<{ at: string; detail: number; pointerId: number; pointerType: string; target: unknown }> = []
      const record = (at: string) => (event: { detail: number; pointerId: number; pointerType: string; target: unknown }) =>
        seen.push({ at, detail: event.detail, pointerId: event.pointerId, pointerType: event.pointerType, target: event.target })
      let row: { id: number; click(): void } | undefined
      testRoot.render(
        <div style={{ width: 400, height: 200 }} onClick={record("list")}>
          <div
            ref={(instance) => {
              if (instance) row = instance as unknown as { id: number; click(): void }
            }}
            style={{ width: 400, height: 100 }}
            onClick={record("row")}
          />
        </div>,
      )

      testRoot.renderer.flush()
      row!.click()
      testRoot.renderer.dispatchNativeEvents()

      expect(seen.map((entry) => [entry.at, entry.detail, entry.pointerId, entry.pointerType])).toEqual([
        ["row", 0, -1, ""],
        ["list", 0, -1, ""],
      ])
      expect(seen[0].target).toBe(row)
      expect(seen[1].target).toBe(row)
    })

    it("does nothing from click() on a disabled form control, and clicks it again once enabled", () => {
      const seen: string[] = []
      let field: { id: number; click(): void } | undefined
      const render = (disabled: boolean) =>
        testRoot.render(
          <div style={{ width: 400, height: 200 }} onClick={() => seen.push("parent")}>
            <input
              ref={(instance) => {
                if (instance) field = instance as unknown as { id: number; click(): void }
              }}
              value=""
              disabled={disabled}
              onClick={() => seen.push("field")}
              style={{ width: 200, height: 30 }}
            />
          </div>,
        )

      render(true)
      testRoot.renderer.flush()
      field!.click()
      testRoot.renderer.dispatchNativeEvents()
      expect(seen).toEqual([])

      render(false)
      testRoot.renderer.flush()
      field!.click()
      testRoot.renderer.dispatchNativeEvents()
      expect(seen).toEqual(["field", "parent"])
    })

    it("clicks from click() on an element inside a disabled ancestor that is not a form control", () => {
      const seen: string[] = []
      let row: { click(): void } | undefined
      testRoot.render(
        <div aria-disabled style={{ width: 400, height: 200 }} onClick={() => seen.push("parent")}>
          <div
            ref={(instance) => {
              if (instance) row = instance as unknown as { click(): void }
            }}
            style={{ width: 400, height: 100 }}
          />
        </div>,
      )
      testRoot.renderer.flush()
      row!.click()
      testRoot.renderer.dispatchNativeEvents()
      expect(seen).toEqual(["parent"])
    })

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

    it.skipIf(process.platform !== "darwin")(
      "fires contextmenu on a ctrl+primary press on macOS, with button 0",
      () => {
        const seen: string[] = []
        testRoot.render(
          <div
            style={{ width: 200, height: 100 }}
            onContextMenu={(event: { button: number; modifiers: { ctrl: boolean } }) =>
              seen.push(`${event.button}:${event.modifiers.ctrl}`)
            }
          />,
        )

        testRoot.renderer.nativeSimulateMouseDown(50, 50, 0, "ctrl")
        expect(seen).toEqual(["0:true"])
        testRoot.renderer.nativeSimulateMouseUp(50, 50, 0, "ctrl")
      },
    )

    it("fires no contextmenu for a plain primary press", () => {
      testRoot.render(
        <div style={{ width: 200, height: 100 }} onContextMenu={record("box")} />,
      )

      testRoot.renderer.nativeSimulateMouseDown(50, 50, 0)
      expect(log).toEqual([])
      testRoot.renderer.nativeSimulateMouseUp(50, 50, 0)
    })

    it("fires pointerdown and pointerup for the first and last button only; the others are pointermove", () => {
      const seen: string[] = []
      const note = (name: string) => (event: { button: number; buttons: number }) =>
        seen.push(`${name}:${event.button}:${event.buttons}`)
      testRoot.render(
        <div
          style={{ width: 200, height: 100 }}
          onPointerDown={note("pointerdown")}
          onPointerMove={note("pointermove")}
          onPointerUp={note("pointerup")}
          onMouseDown={note("mousedown")}
          onMouseUp={note("mouseup")}
        />,
      )

      testRoot.renderer.nativeSimulateMouseDown(50, 50, 0)
      testRoot.renderer.nativeSimulateMouseDown(50, 50, 2)
      testRoot.renderer.nativeSimulateMouseUp(50, 50, 0)
      testRoot.renderer.nativeSimulateMouseUp(50, 50, 2)

      expect(seen).toEqual([
        "pointerdown:0:1",
        "mousedown:0:1",
        "pointermove:2:3",
        "mousedown:2:3",
        "pointermove:0:2",
        "mouseup:0:2",
        "pointerup:2:0",
        "mouseup:2:0",
      ])
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

  describe("pointer capture", () => {
    type Capturable = {
      id: number
      setPointerCapture(pointerId: number): void
      releasePointerCapture(pointerId: number): void
      hasPointerCapture(pointerId: number): boolean
    }

    let first: Capturable
    let second: Capturable

    /** Box `first` at (0..100, 0..100) above box `second` at (0..200, 100..150). */
    function Boxes(props: { second?: boolean; onFirstDown?: () => void }) {
      const all = (name: string) => ({
        onPointerOver: record(name),
        onPointerOut: record(name),
        onPointerMove: record(name),
        onPointerUp: record(name),
        onPointerCancel: record(name),
        onMouseUp: record(name),
        onGotPointerCapture: record(name),
        onLostPointerCapture: record(name),
      })
      return (
        <div style={{ width: 600, height: 400 }}>
          <div
            ref={(instance) => {
              if (instance) first = instance as unknown as Capturable
            }}
            style={{ width: 100, height: 100 }}
            onPointerDown={props.onFirstDown}
            {...all("first")}
          />
          {props.second !== false && (
            <div
              ref={(instance) => {
                if (instance) second = instance as unknown as Capturable
              }}
              style={{ width: 200, height: 50 }}
              {...all("second")}
            />
          )}
        </div>
      )
    }

    it("sends pointer events to the capturing element wherever the pointer is", () => {
      const targets: object[] = []
      testRoot.render(
        <div
          style={{ width: 600, height: 400 }}
          onPointerMoveCapture={(event: { target: object }) => targets.push(event.target)}
        >
          <Boxes onFirstDown={() => second.setPointerCapture(1)} />
        </div>,
      )

      testRoot.renderer.nativeSimulateMouseDown(50, 50)
      testRoot.renderer.nativeSimulateMouseMove(50, 20)

      expect(targets).toHaveLength(1)
      expect(targets[0]).toBe(second)
    })

    it("keeps delivering to the capturer on moves that name no button", () => {
      const moves: { target: object; pressedButton?: number }[] = []
      testRoot.render(
        <div
          style={{ width: 600, height: 400 }}
          onPointerMoveCapture={(event: { target: object; pressedButton?: number }) =>
            moves.push({ target: event.target, pressedButton: event.pressedButton })
          }
        >
          <Boxes onFirstDown={() => first.setPointerCapture(1)} />
        </div>,
      )

      testRoot.renderer.nativeSimulateMouseDown(50, 50)
      testRoot.renderer.nativeSimulateMouseMove(300, 300)
      testRoot.renderer.nativeSimulateMouseMove(500, 380)

      expect(moves.map((move) => move.pressedButton)).toEqual([0, 0])
      for (const move of moves) expect(move.target).toBe(first)
    })

    it("ends the press and the capture when the release was lost", () => {
      testRoot.render(<Boxes onFirstDown={() => first.setPointerCapture(1)} />)
      testRoot.renderer.nativeSimulateMouseDown(50, 50)
      testRoot.renderer.nativeSimulateMouseMove(50, 125)
      log.length = 0

      testRoot.renderer.simulateLostMouseUp()
      expect(log).toEqual([])
      testRoot.renderer.nativeSimulateMouseMove(60, 125)

      expect(log).toEqual([
        "first:pointercancel",
        "first:lostpointercapture",
        "first:pointerout",
        "second:pointerover",
        "second:pointermove",
      ])
      expect(first.hasPointerCapture(1)).toBe(false)
    })

    it("applies from the next native event: got first, boundary events, then the move", () => {
      testRoot.render(<Boxes onFirstDown={() => first.setPointerCapture(1)} />)

      testRoot.renderer.nativeSimulateMouseDown(50, 50)
      expect(log).toEqual(["first:pointerover"])
      expect(first.hasPointerCapture(1)).toBe(true)

      log.length = 0
      testRoot.renderer.nativeSimulateMouseMove(50, 125)
      expect(log).toEqual(["first:gotpointercapture", "first:pointermove"])
    })

    it("moves hover to the capture target: the old chain gets out, the target gets over", () => {
      testRoot.render(<Boxes onFirstDown={() => second.setPointerCapture(1)} />)
      testRoot.renderer.nativeSimulateMouseDown(50, 50)
      log.length = 0

      testRoot.renderer.nativeSimulateMouseMove(50, 20)
      expect(log).toEqual([
        "second:gotpointercapture",
        "first:pointerout",
        "second:pointerover",
        "second:pointermove",
      ])

      log.length = 0
      testRoot.renderer.nativeSimulateMouseMove(50, 70)
      expect(log).toEqual(["second:pointermove"])
    })

    it("releases after pointerup: lostpointercapture comes before mouseup", () => {
      testRoot.render(<Boxes onFirstDown={() => first.setPointerCapture(1)} />)
      testRoot.renderer.nativeSimulateMouseDown(50, 50)
      testRoot.renderer.nativeSimulateMouseMove(50, 125)
      log.length = 0

      testRoot.renderer.nativeSimulateMouseUp(50, 125)
      expect(log).toEqual([
        "first:pointerup",
        "first:lostpointercapture",
        "first:mouseup",
        "first:pointerout",
        "second:pointerover",
      ])
      expect(first.hasPointerCapture(1)).toBe(false)

      log.length = 0
      testRoot.renderer.nativeSimulateMouseMove(60, 125)
      expect(log).toEqual(["second:pointermove"])
    })

    it("releases from JS at the next native event", () => {
      testRoot.render(<Boxes onFirstDown={() => first.setPointerCapture(1)} />)
      testRoot.renderer.nativeSimulateMouseDown(50, 50)
      testRoot.renderer.nativeSimulateMouseMove(50, 125)
      log.length = 0

      first.releasePointerCapture(1)
      expect(first.hasPointerCapture(1)).toBe(false)
      testRoot.renderer.nativeSimulateMouseMove(60, 125)
      expect(log).toEqual([
        "first:lostpointercapture",
        "first:pointerout",
        "second:pointerover",
        "second:pointermove",
      ])
    })

    it("releases without an event when the capturing element is removed", () => {
      testRoot.render(<Boxes onFirstDown={() => second.setPointerCapture(1)} />)
      testRoot.renderer.nativeSimulateMouseDown(50, 50)
      testRoot.renderer.nativeSimulateMouseMove(50, 20)
      log.length = 0

      testRoot.render(<Boxes second={false} />)
      testRoot.renderer.nativeSimulateMouseMove(50, 30)

      expect(log).not.toContain("second:lostpointercapture")
      expect(log).toEqual(["first:pointerover", "first:pointermove"])
    })

    it("reports who holds the capture", () => {
      testRoot.render(<Boxes onFirstDown={() => first.setPointerCapture(1)} />)
      expect(first.hasPointerCapture(1)).toBe(false)
      testRoot.renderer.nativeSimulateMouseDown(50, 50)

      expect(first.hasPointerCapture(1)).toBe(true)
      expect(second.hasPointerCapture(1)).toBe(false)
    })

    it("ignores a capture requested while no button is down", () => {
      testRoot.render(<Boxes />)

      first.setPointerCapture(1)

      expect(first.hasPointerCapture(1)).toBe(false)
    })

    it("throws for a pointerId that does not exist", () => {
      testRoot.render(<Boxes />)

      expect(() => first.setPointerCapture(2)).toThrow(/pointerId/)
      expect(() => first.releasePointerCapture(0)).toThrow(/pointerId/)
      expect(() => first.hasPointerCapture(7)).toThrow(/pointerId/)
    })

    it("throws from setPointerCapture only, for an element that is not connected", () => {
      testRoot.render(<Boxes />)
      const { renderer } = testRoot

      expect(() => renderer.setPointerCapture(987654, 1)).toThrow(/not connected/)
      expect(() => renderer.releasePointerCapture(987654, 1)).not.toThrow()
      expect(renderer.hasPointerCapture(987654, 1)).toBe(false)
    })

    it("still throws for an unknown pointerId on an element that is not connected", () => {
      testRoot.render(<Boxes />)
      const { renderer } = testRoot

      expect(() => renderer.releasePointerCapture(987654, 7)).toThrow(/pointerId/)
      expect(() => renderer.hasPointerCapture(987654, 7)).toThrow(/pointerId/)
    })

    it("reports no capture on an element once it is removed", () => {
      testRoot.render(<Boxes onFirstDown={() => second.setPointerCapture(1)} />)
      testRoot.renderer.nativeSimulateMouseDown(50, 50)
      expect(second.hasPointerCapture(1)).toBe(true)

      testRoot.render(<Boxes second={false} />)

      expect(second.hasPointerCapture(1)).toBe(false)
      expect(() => second.releasePointerCapture(1)).not.toThrow()
    })
  })
})
