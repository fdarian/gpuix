/// Pointer lock end to end: `requestPointerLock()` on a ref, the document-level
/// events, and where pointer events go while the lock is held.
///
/// The test window is offscreen and never key, so GPUI would always refuse a
/// real lock. These tests run against the renderer's simulated platform, which
/// grants, refuses and drops locks on command; everything on the GPUIX side of
/// that is the real code.

import { describe, it, expect, beforeEach } from "vitest"
import React from "react"
import type { EventPayload } from "@gpuix/native"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("pointer lock", () => {
  type Lockable = { id: number; requestPointerLock(): void }

  let testRoot: ReturnType<typeof createTestRoot>
  let log: string[]
  let first: Lockable
  let second: Lockable

  /** Names the lock events by type, with the element they name. */
  const lockEvents = () => ({
    onPointerLockChange: (event: EventPayload) =>
      log.push(`change:${event.target ?? "none"}`),
    onPointerLockError: (event: EventPayload) => log.push(`error:${event.target}`),
  })

  beforeEach(() => {
    testRoot = createTestRoot(lockEvents())
    log = []
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

    expect(log).toEqual([`change:${first.id}`])
    expect(testRoot.renderer.pointerLockElement()).toBe(first.id)
  })

  it("fires pointerlockerror when the platform refuses", () => {
    testRoot.render(<Boxes />)
    testRoot.renderer.simulatePointerLockGranting(false)

    first.requestPointerLock()

    expect(log).toEqual([`error:${first.id}`])
    expect(testRoot.renderer.pointerLockElement()).toBeNull()
  })

  it("fires pointerlockerror for an element that is not in the tree", () => {
    testRoot.render(<Boxes />)
    const removed = second
    testRoot.render(<Boxes second={false} />)

    removed.requestPointerLock()

    expect(log).toEqual([`error:${removed.id}`])
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

    expect(log).toEqual([`change:${second.id}`])
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
})
