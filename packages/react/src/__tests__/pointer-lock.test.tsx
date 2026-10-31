/// Pointer lock end to end: `requestPointerLock()` on a ref, the document-level
/// events, and where pointer events go while the lock is held.
///
/// The test window is offscreen and never key, so GPUI would always refuse a
/// real lock. These tests run against the renderer's simulated platform, which
/// grants, refuses and drops locks on command; everything on the GPUIX side of
/// that is the real code.

import { describe, it, expect, beforeEach, afterEach } from "vitest"
import React from "react"
import { document, window } from "@ghia/native/host"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("pointer lock", () => {
  type Lockable = { id: number; requestPointerLock(): void }

  let testRoot: ReturnType<typeof createTestRoot>
  /** The lock events at `document`, by type and the element it held then. */
  let log: string[]
  let first: Lockable
  let second: Lockable
  let stopLog: Array<() => void>

  /** Records what `document.pointerLockElement` is while each lock event runs. */
  const recordLockEvents = () => {
    const holder = (element: object | null) =>
      element === first ? "first" : element === second ? "second" : element === null ? "none" : "other"
    const onChange = () => log.push(`change:${holder(document.pointerLockElement)}`)
    const onError = () => log.push(`error:${holder(document.pointerLockElement)}`)
    document.addEventListener("pointerlockchange", onChange as never)
    document.addEventListener("pointerlockerror", onError as never)
    stopLog.push(() => {
      document.removeEventListener("pointerlockchange", onChange as never)
      document.removeEventListener("pointerlockerror", onError as never)
    })
  }

  beforeEach(() => {
    testRoot = createTestRoot()
    log = []
    stopLog = []
    recordLockEvents()
  })

  afterEach(() => {
    for (const stop of stopLog) stop()
    // A root that still holds a lock would be the one `document.pointerLockElement` reads next.
    testRoot.unmount()
  })

  /** `first` at (0..100, 0..100), `second` at (0..100, 100..200). */
  function Boxes(props: { second?: boolean; seen?: Seen[] }) {
    const track = (name: string) => (event: Seen) => {
      props.seen?.push({ name, type: event.type, x: event.clientX, y: event.clientY })
    }
    return (
      <div style={{ width: 400, height: 400 }}>
        <div
          ref={(instance) => {
            if (instance) first = instance as unknown as Lockable
          }}
          style={{ width: 100, height: 100 }}
          onPointerMove={track("first")}
          onMouseMove={track("first")}
          onMouseUp={track("first")}
        />
        {props.second !== false && (
          <div
            ref={(instance) => {
              if (instance) second = instance as unknown as Lockable
            }}
            style={{ width: 100, height: 100 }}
            onPointerMove={track("second")}
            onMouseMove={track("second")}
            onMouseUp={track("second")}
          />
        )}
      </div>
    )
  }

  type Seen = { name: string; type: string; x: number; y: number }

  it("fires pointerlockchange for the element once the frame grants the lock", () => {
    testRoot.render(<Boxes />)
    expect(testRoot.renderer.pointerLockElement()).toBeNull()

    first.requestPointerLock()

    expect(log).toEqual(["change:first"])
    expect(testRoot.renderer.pointerLockElement()).toBe(first.id)
  })

  describe("at document", () => {
    type LockEvent = {
      type: string
      target: object | null
      bubbles: boolean
      cancelable: boolean
      eventPhase: number
      currentTarget: object | null
    }
    let heard: string[]
    let events: LockEvent[]
    let stop: Array<() => void>

    function listen(target: typeof document | typeof window, type: string, name: string) {
      const listener = (event: LockEvent) => {
        heard.push(name)
        events.push({ ...event })
      }
      target.addEventListener(type, listener as never)
      stop.push(() => target.removeEventListener(type, listener as never))
    }

    beforeEach(() => {
      heard = []
      events = []
      stop = []
    })

    afterEach(() => {
      for (const remove of stop) remove()
    })

    it("pointerlockchange targets document, reaches document and then window listeners, and pointerLockElement is set first", () => {
      testRoot.render(<Boxes />)
      const lockSeenByListener: Array<object | null> = []
      listen(document, "pointerlockchange", "document")
      listen(window, "pointerlockchange", "window")
      document.addEventListener("pointerlockchange", (() =>
        lockSeenByListener.push(document.pointerLockElement)) as never)

      first.requestPointerLock()

      expect(heard).toEqual(["document", "window"])
      expect(events[0].type).toBe("pointerlockchange")
      expect(events[0].bubbles).toBe(true)
      expect(events[0].cancelable).toBe(false)
      expect(lockSeenByListener).toEqual([first])
      expect(document.pointerLockElement).toBe(first)
    })

    it("the target is document whichever target the listener sits on, and the phase follows the DOM", () => {
      testRoot.render(<Boxes />)
      const seen: Array<[string, boolean, boolean, number]> = []
      const record = (name: string) => (event: LockEvent) =>
        seen.push([name, event.target === document, event.currentTarget === (name.startsWith("window") ? window : document), event.eventPhase])
      const listeners: Array<[typeof document | typeof window, string, boolean]> = [
        [window, "window-capture", true],
        [document, "document-capture", true],
        [document, "document", false],
        [window, "window", false],
      ]
      for (const [target, name, capture] of listeners) {
        const listener = record(name)
        target.addEventListener("pointerlockchange", listener as never, capture)
        stop.push(() => target.removeEventListener("pointerlockchange", listener as never, capture))
      }

      first.requestPointerLock()

      expect(seen).toEqual([
        ["window-capture", true, true, 1],
        ["document-capture", true, true, 2],
        ["document", true, true, 2],
        ["window", true, true, 3],
      ])
    })

    it("exitPointerLock fires pointerlockchange with no element and clears pointerLockElement", () => {
      testRoot.render(<Boxes />)
      first.requestPointerLock()
      listen(document, "pointerlockchange", "document")

      testRoot.renderer.exitPointerLock()

      expect(heard).toEqual(["document"])
      expect(events[0].target).toBe(document)
      expect(document.pointerLockElement).toBeNull()
    })

    it("a refused request fires pointerlockerror at document and locks nothing", () => {
      testRoot.render(<Boxes />)
      testRoot.renderer.simulatePointerLockGranting(false)
      listen(document, "pointerlockerror", "document")
      listen(document, "pointerlockchange", "change")

      first.requestPointerLock()

      expect(heard).toEqual(["document"])
      expect(events[0].type).toBe("pointerlockerror")
      expect(events[0].target).toBe(document)
      expect(document.pointerLockElement).toBeNull()
    })

    it("a lock the platform drops fires pointerlockchange at document", () => {
      testRoot.render(<Boxes />)
      first.requestPointerLock()
      listen(document, "pointerlockchange", "document")

      testRoot.renderer.simulatePointerLockLost()

      expect(heard).toEqual(["document"])
      expect(document.pointerLockElement).toBeNull()
    })

    it("a capture-phase window listener runs before the document's and can stop it", () => {
      testRoot.render(<Boxes />)
      listen(document, "pointerlockchange", "document")
      const capture = (event: { stopPropagation(): void }) => {
        heard.push("window-capture")
        event.stopPropagation()
      }
      window.addEventListener("pointerlockchange", capture as never, true)
      stop.push(() => window.removeEventListener("pointerlockchange", capture as never, true))

      first.requestPointerLock()

      expect(heard).toEqual(["window-capture"])
    })
  })

  it("fires pointerlockerror when the platform refuses", () => {
    testRoot.render(<Boxes />)
    testRoot.renderer.simulatePointerLockGranting(false)

    first.requestPointerLock()

    expect(log).toEqual(["error:none"])
    expect(testRoot.renderer.pointerLockElement()).toBeNull()
  })

  it("fires pointerlockerror for an element that is not in the tree", () => {
    testRoot.render(<Boxes />)
    const removed = second
    testRoot.render(<Boxes second={false} />)

    removed.requestPointerLock()

    expect(log).toEqual(["error:none"])
    expect(testRoot.renderer.pointerLockElement()).toBeNull()
  })

  it("exitPointerLock releases it with a pointerlockchange that names no element", () => {
    testRoot.render(<Boxes />)
    first.requestPointerLock()
    log.length = 0

    testRoot.renderer.exitPointerLock()

    expect(log).toEqual(["change:none"])
    expect(testRoot.renderer.pointerLockElement()).toBeNull()
  })

  it("exiting with no lock held does nothing", () => {
    testRoot.render(<Boxes />)
    testRoot.renderer.exitPointerLock()
    expect(log).toEqual([])
  })

  it("a lock the platform drops fires pointerlockchange with no element", () => {
    testRoot.render(<Boxes />)
    first.requestPointerLock()
    log.length = 0

    testRoot.renderer.simulatePointerLockLost()

    expect(log).toEqual(["change:none"])
    expect(testRoot.renderer.pointerLockElement()).toBeNull()
  })

  it("locking another element moves the lock", () => {
    testRoot.render(<Boxes />)
    first.requestPointerLock()
    log.length = 0

    second.requestPointerLock()

    expect(log).toEqual(["change:second"])
    expect(testRoot.renderer.pointerLockElement()).toBe(second.id)
  })

  it("removing the locked element exits the lock", () => {
    testRoot.render(<Boxes />)
    second.requestPointerLock()
    log.length = 0

    testRoot.render(<Boxes second={false} />)
    testRoot.renderer.dispatchNativeEvents()

    expect(log).toEqual(["change:none"])
    expect(testRoot.renderer.pointerLockElement()).toBeNull()
  })

  it("sends pointer and mouse events to the lock element with a frozen position", () => {
    const seen: Seen[] = []
    testRoot.render(<Boxes seen={seen} />)
    testRoot.renderer.nativeSimulateMouseMove(30, 40)
    first.requestPointerLock()
    seen.length = 0

    testRoot.renderer.nativeSimulateMouseMove(60, 150)
    testRoot.renderer.nativeSimulateMouseUp(70, 160)

    expect(seen).toEqual([
      { name: "first", type: "pointermove", x: 30, y: 40 },
      { name: "first", type: "mousemove", x: 30, y: 40 },
      { name: "first", type: "mouseup", x: 30, y: 40 },
    ])
  })

  it("presses, clicks and wheel ticks over another element go to the lock element too", () => {
    const seen: string[] = []
    const track = (name: string) => (event: { type: string }) => seen.push(`${name}:${event.type}`)
    const listeners = (name: string) => ({
      onPointerDown: track(name),
      onMouseDown: track(name),
      onPointerUp: track(name),
      onMouseUp: track(name),
      onClick: track(name),
      onWheel: track(name),
    })
    testRoot.render(
      <div style={{ width: 400, height: 400 }}>
        <div
          ref={(instance) => {
            if (instance) first = instance as unknown as Lockable
          }}
          style={{ width: 100, height: 100 }}
          {...listeners("first")}
        />
        <div style={{ width: 100, height: 100 }} {...listeners("second")} />
      </div>,
    )
    first.requestPointerLock()

    testRoot.renderer.nativeSimulateMouseDown(60, 150)
    testRoot.renderer.nativeSimulateMouseUp(60, 150)
    testRoot.renderer.nativeSimulateScrollWheel(60, 150, 0, 10)

    expect(seen).toEqual([
      "first:pointerdown",
      "first:mousedown",
      "first:pointerup",
      "first:mouseup",
      "first:click",
      "first:wheel",
    ])
  })

  it("events go back to what is under the pointer once the lock ends", () => {
    const seen: Seen[] = []
    testRoot.render(<Boxes seen={seen} />)
    testRoot.renderer.nativeSimulateMouseMove(30, 40)
    first.requestPointerLock()
    testRoot.renderer.exitPointerLock()
    seen.length = 0

    testRoot.renderer.nativeSimulateMouseMove(60, 150)

    expect(seen.map((event) => `${event.name}:${event.type}@${event.x},${event.y}`)).toEqual([
      "second:pointermove@60,150",
      "second:mousemove@60,150",
    ])
  })

  describe("from an input handler", () => {
    type Press = { type: string; clientX: number; clientY: number }

    it("a pointerdown handler holds the lock when the call returns, and the next move is aimed at it", () => {
      const seen: Seen[] = []
      let lockInHandler: number | null | undefined
      testRoot.render(
        <div style={{ width: 400, height: 400 }}>
          <div
            ref={(instance) => {
              if (instance) first = instance as unknown as Lockable
            }}
            style={{ width: 100, height: 100 }}
            onPointerMove={(event: Press) =>
              seen.push({ name: "first", type: event.type, x: event.clientX, y: event.clientY })
            }
          />
          <div
            ref={(instance) => {
              if (instance) second = instance as unknown as Lockable
            }}
            style={{ width: 100, height: 100 }}
            onPointerDown={() => {
              second.requestPointerLock()
              lockInHandler = testRoot.renderer.pointerLockElement()
            }}
            onPointerMove={(event: Press) =>
              seen.push({ name: "second", type: event.type, x: event.clientX, y: event.clientY })
            }
          />
        </div>,
      )
      testRoot.renderer.nativeSimulateMouseMove(30, 150)
      seen.length = 0

      testRoot.renderer.nativeSimulateMouseDown(30, 150)
      testRoot.renderer.nativeSimulateMouseMove(60, 40)

      expect(lockInHandler).toBe(second.id)
      expect(log).toEqual(["change:second"])
      expect(seen).toEqual([{ name: "second", type: "pointermove", x: 30, y: 150 }])
    })

    it("exitPointerLock from a handler is over when the call returns", () => {
      let lockIdAfterExit: number | null | undefined
      testRoot.render(
        <div style={{ width: 400, height: 400 }}>
          <div
            ref={(instance) => {
              if (instance) first = instance as unknown as Lockable
            }}
            style={{ width: 100, height: 100 }}
            onPointerDown={() => {
              testRoot.renderer.exitPointerLock()
              lockIdAfterExit = testRoot.renderer.pointerLockElement()
            }}
          />
        </div>,
      )
      first.requestPointerLock()
      log.length = 0

      testRoot.renderer.nativeSimulateMouseDown(30, 40)

      expect(lockIdAfterExit).toBeNull()
      expect(log).toEqual(["change:none"])
    })

    it("a request the platform refuses fires pointerlockerror and leaves no lock", () => {
      let lockId: number | null | undefined
      testRoot.render(
        <div
          ref={(instance) => {
            if (instance) first = instance as unknown as Lockable
          }}
          style={{ width: 100, height: 100 }}
          onPointerDown={() => {
            first.requestPointerLock()
            lockId = testRoot.renderer.pointerLockElement()
          }}
        />,
      )
      testRoot.renderer.simulatePointerLockGranting(false)

      testRoot.renderer.nativeSimulateMouseDown(30, 40)

      expect(lockId).toBeNull()
      expect(log).toEqual(["error:none"])
    })
  })

  describe("movement", () => {
    type Moved = { type: string; movementX: number; movementY: number; clientX: number }

    it("reports the platform's motion on pointermove and mousemove, as clientX stands still", () => {
      const moves: Moved[] = []
      const track = (event: Moved) => moves.push(event)
      testRoot.render(
        <div style={{ width: 200, height: 200 }} onPointerMove={track} onMouseMove={track} />,
      )

      testRoot.renderer.nativeSimulateMouseMove(30, 40, undefined, { x: 4, y: -3 })
      testRoot.renderer.nativeSimulateMouseMove(30, 40)

      expect(moves.map((event) => [event.type, event.movementX, event.movementY])).toEqual([
        ["pointermove", 4, -3],
        ["mousemove", 4, -3],
        ["pointermove", 0, 0],
        ["mousemove", 0, 0],
      ])
    })

    it("reads 0 on every other mouse event, even in the input that moved", () => {
      const seen: Array<[string, number, number]> = []
      const track = (event: Moved) => seen.push([event.type, event.movementX, event.movementY])
      testRoot.render(
        <div
          style={{ width: 200, height: 200 }}
          onPointerOver={track}
          onMouseEnter={track}
          onPointerDown={track}
          onMouseDown={track}
          onPointerUp={track}
          onMouseUp={track}
          onClick={track}
          onWheel={track}
          onPointerMove={track}
        />,
      )

      testRoot.renderer.nativeSimulateMouseMove(30, 40, undefined, { x: 4, y: -3 })
      testRoot.renderer.nativeSimulateMouseDown(30, 40)
      testRoot.renderer.nativeSimulateMouseUp(30, 40)
      testRoot.renderer.nativeSimulateScrollWheel(30, 40, 0, -10)

      const moved = seen.filter(([type]) => type === "pointermove")
      expect(moved).toEqual([["pointermove", 4, -3]])
      const others = seen.filter(([type]) => type !== "pointermove")
      expect(others.map(([type]) => type)).toEqual(
        expect.arrayContaining(["pointerover", "mouseenter", "pointerdown", "mouseup", "click", "wheel"]),
      )
      for (const [, movementX, movementY] of others) expect([movementX, movementY]).toEqual([0, 0])
    })

    it("keeps reporting motion while the pointer is locked and clientX is frozen", () => {
      const moves: Moved[] = []
      testRoot.render(
        <div
          ref={(instance) => {
            if (instance) first = instance as unknown as Lockable
          }}
          style={{ width: 200, height: 200 }}
          onMouseMove={(event: Moved) => moves.push(event)}
        />,
      )
      testRoot.renderer.nativeSimulateMouseMove(30, 40)
      first.requestPointerLock()
      moves.length = 0

      testRoot.renderer.nativeSimulateMouseMove(90, 100, undefined, { x: 12, y: 5 })
      testRoot.renderer.nativeSimulateMouseMove(150, 160, undefined, { x: -9, y: 1 })

      expect(moves.map((event) => [event.clientX, event.movementX, event.movementY])).toEqual([
        [30, 12, 5],
        [30, -9, 1],
      ])
    })
  })
})
