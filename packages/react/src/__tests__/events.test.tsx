/// Tests for GPUIX event handling — verifies that React components receive
/// events correctly through the full native GPUI pipeline (end-to-end).
///
/// Every test goes through: React render → Rust RetainedTree → GpuixView::render()
/// → build_element() → GPUI layout → native simulate → GPUI hit test/dispatch →
/// event handler → emit_event_full → drainEvents → handleGpuixEvent → React handler.
///
/// All components use explicit sizes so GPUI can lay them out and hit-test
/// against known coordinates.
///
/// JSX types now resolve to GPUIX's Props via jsxImportSource in tsconfig.

import fs from "fs"
import os from "node:os"
import path from "node:path"
import { spawnSync } from "node:child_process"
import { fileURLToPath } from "node:url"
import { describe, it, expect, beforeEach } from "vitest"
import React, { useState, useRef } from "react"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { startFrameLoop } from "../reconciler/renderer.js"
import { motion } from "../index"
import type { EventPayload, HostDataTransfer, HostDragEvent, HostFile, HostMouseEvent } from "@gpuix/native"
import type { PublicInstance } from "../types/host.js"
import { expectScreenshotsDiffer, expectScreenshotsEqual, SHOTS_DIR } from "./test-utils"

// All tests require the native GPUI test renderer (cargo build with test-support).
const describeNative = hasNativeTestRenderer ? describe : describe.skip

describe("frame loop", () => {
  it.skipIf(process.platform !== "darwin")(
    "drives the frame loop from real display-link wakes without blocking I/O",
    () => {
      const script = [
        'import { readFile } from "node:fs/promises"',
        'import { GpuixRenderer } from "@gpuix/native"',
        'import { startFrameLoop } from "@gpuix/native/runtime"',
        "const renderer = new GpuixRenderer(() => {})",
        "renderer.init({ focus: false })",
        "let wakes = 0",
        "let finish",
        "const frames = new Promise((resolve) => {",
        "  finish = resolve",
        "})",
        "const loop = startFrameLoop({",
        "  requiresTick: () => renderer.requiresTick(),",
        "  tick: () => renderer.tick(),",
        "  setFrameWakeCallback: (callback) => renderer.setFrameWakeCallback(callback && (() => {",
        "    callback()",
        "    if (++wakes === 3) finish()",
        "  })),",
        "})",
        "await Promise.all([frames, readFile('package.json')])",
        "loop.stop()",
        "console.log(JSON.stringify({ wakes }))",
        "process.exit(0)",
      ].join("\n")
      for (const command of ["bun", "node"]) {
        const result = spawnSync(command, ["--input-type=module", "-e", script], {
          encoding: "utf8",
          timeout: 5_000,
        })
        expect(result.status, result.stderr || result.error?.message).toBe(0)
        expect(JSON.parse(result.stdout.trim())).toEqual({ wakes: 3 })
      }
    },
  )

  // The test window is not key: nothing here activates the app or posts to the OS. The events
  // are real NSEvents handed to NSApplication from com.apple.NSEventThread, which is how
  // AppKit receives WindowServer input, and that thread is what the wake path has to see.
  it.skipIf(process.platform !== "darwin")(
    "wakes an idle wake loop for input that AppKit receives, and runs its handler in that tick",
    () => {
      const rendererPath = fileURLToPath(
        new URL("../reconciler/renderer.ts", import.meta.url),
      )
      const script = [
        'import React from "react"',
        `import { createRenderer, render } from ${JSON.stringify(rendererPath)}`,
        'import { startFrameLoop } from "@gpuix/native/runtime"',
        "const renderer = createRenderer()",
        "renderer.init({ focus: false })",
        "const wakes = []",
        "const handled = []",
        "const note = (kind) => () => { handled.push({ kind, at: performance.now() }) }",
        "render(",
        '  React.createElement("div", {',
        '    style: { width: "100%", height: "100%" },',
        "    tabIndex: 0,",
        '    onKeyDown: note("elementKeyDown"),',
        '    onMouseMove: note("mouseMove"),',
        '    onClick: note("click"),',
        '    onWheel: note("scroll"),',
        "  }),",
        // Nothing has focus, so a key goes to the window's handler.
        '  { renderer, onKeyDown: note("keyDown") },',
        ")",
        "startFrameLoop({",
        "  requiresTick: () => renderer.requiresTick(),",
        "  tick: () => renderer.tick(),",
        "  setFrameWakeCallback: (callback) => renderer.setFrameWakeCallback(callback && (() => {",
        "    wakes.push(performance.now())",
        "    callback()",
        "  })),",
        "})",
        "const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))",
        // The launch measurements wake the loop in bursts for up to ~6 s, with gaps of seconds.
        "const launchedAt = performance.now()",
        "const settle = async () => {",
        "  for (;;) {",
        "    const now = performance.now()",
        "    const quiet = now - (wakes.length === 0 ? launchedAt : wakes[wakes.length - 1])",
        "    if (quiet >= 800 && now - launchedAt >= 7000) return quiet",
        '    if (now - launchedAt > 20000) throw new Error("the wake loop did not settle in 20 s")',
        "    await sleep(20)",
        "  }",
        "}",
        "const size = renderer.getWindowSize()",
        "await settle()",
        "const idleFrom = wakes.length",
        "await sleep(2000)",
        "const idleWakes = wakes.length - idleFrom",
        "const kinds = {}",
        'for (const kind of ["keyDown", "mouseMove", "click", "scroll"]) {',
        "  const quietMs = await settle()",
        "  const wakesBefore = wakes.length",
        "  const handledBefore = handled.length",
        "  const postedAt = performance.now()",
        "  renderer.postNativeEvent(kind, size.width / 2, size.height / 2)",
        "  await sleep(400)",
        "  const firstHandled = handled.slice(handledBefore).find((entry) => entry.kind === kind)",
        "  kinds[kind] = {",
        "    quietMs,",
        "    postToWakeMs: wakes.length > wakesBefore ? wakes[wakesBefore] - postedAt : null,",
        "    postToHandlerMs: firstHandled ? firstHandled.at - postedAt : null,",
        "  }",
        "}",
        "console.log(JSON.stringify({ idleWakes, kinds }))",
        "process.exit(0)",
      ].join("\n")
      const result = spawnSync("bun", ["-e", script], {
        encoding: "utf8",
        timeout: 40_000,
      })
      expect(result.status, result.stderr || result.error?.message).toBe(0)
      const report = JSON.parse(result.stdout.trim().split("\n").at(-1) ?? "")
      console.log(`post -> wake -> handler latencies: ${JSON.stringify(report.kinds)}`)

      // #77: an idle window gets no wakes.
      expect(report.idleWakes).toBe(0)
      for (const kind of ["keyDown", "mouseMove", "click", "scroll"]) {
        const measured = report.kinds[kind]
        // No wake between the settle and the post, so the wake is the event's.
        expect(measured.quietMs, kind).toBeGreaterThanOrEqual(800)
        expect(measured.postToWakeMs, `${kind} woke`).not.toBeNull()
        expect(measured.postToWakeMs, `${kind} wake latency`).toBeLessThanOrEqual(20)
        expect(measured.postToHandlerMs, `${kind} handler ran`).not.toBeNull()
        expect(measured.postToHandlerMs, `${kind} handler latency`).toBeLessThanOrEqual(50)
      }
    },
    40_000,
  )

  // The same wake path as the live test above, on the test renderer: its window is not key and
  // not on screen, and `tick()` hands AppKit's queue for it to the window.
  it.skipIf(process.platform !== "darwin" || !hasNativeTestRenderer)(
    "wakes an idle test-renderer wake loop for input that AppKit receives, and runs its handler in that tick",
    async () => {
      const handled: { kind: string; at: number }[] = []
      const note = (kind: string) => () => {
        handled.push({ kind, at: performance.now() })
      }
      const testRoot = createTestRoot({ onKeyDown: note("keyDown") })
      const renderer = testRoot.renderer
      try {
        testRoot.render(
          <div
            style={{ width: "100%", height: "100%" }}
            tabIndex={0}
            onMouseMove={note("mouseMove")}
            onClick={note("click")}
            onWheel={note("scroll")}
          />,
        )
        const wakes: number[] = []
        const loop = startFrameLoop({
          requiresTick: () => renderer.requiresTick(),
          tick: () => renderer.tick(),
          setFrameWakeCallback: (callback) =>
            renderer.setFrameWakeCallback(
              callback &&
                (() => {
                  wakes.push(performance.now())
                  callback()
                }),
            ),
        })
        const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))
        try {
          const size = renderer.getWindowSize()
          // The window's own startup work and the launch measurements wake the loop in bursts for
          // up to ~6 s, with gaps of seconds.
          const launchedAt = performance.now()
          const settle = async (): Promise<number> => {
            for (;;) {
              const now = performance.now()
              const quiet = now - (wakes.length === 0 ? launchedAt : wakes[wakes.length - 1])
              if (quiet >= 800 && now - launchedAt >= 7000) return quiet
              if (now - launchedAt > 20_000) throw new Error("the wake loop did not settle in 20 s")
              await sleep(20)
            }
          }
          await settle()
          const idleFrom = wakes.length
          await sleep(2000)
          // #77: an idle window gets no wakes.
          expect(wakes.length - idleFrom, "wakes over 2 s idle").toBe(0)

          const latencies: Record<string, { postToWakeMs: number; postToHandlerMs: number }> = {}
          for (const kind of ["keyDown", "mouseMove", "click", "scroll"]) {
            // No wake between the settle and the post, so the wake is the event's.
            await settle()
            const wakesBefore = wakes.length
            const handledBefore = handled.length
            const postedAt = performance.now()
            renderer.postNativeEvent(kind, size.width / 2, size.height / 2)
            await sleep(200)
            expect(wakes.length, `${kind} woke`).toBeGreaterThan(wakesBefore)
            const first = handled.slice(handledBefore).find((entry) => entry.kind === kind)
            expect(first, `${kind} handler ran`).toBeDefined()
            latencies[kind] = {
              postToWakeMs: wakes[wakesBefore] - postedAt,
              postToHandlerMs: first!.at - postedAt,
            }
            expect(latencies[kind].postToWakeMs, `${kind} wake latency`).toBeLessThanOrEqual(20)
            expect(latencies[kind].postToHandlerMs, `${kind} handler latency`).toBeLessThanOrEqual(50)
          }
          console.log(`test renderer post -> wake -> handler latencies: ${JSON.stringify(latencies)}`)
        } finally {
          loop.stop()
        }
      } finally {
        testRoot.unmount()
      }
    },
    40_000,
  )

  it.skipIf(process.platform !== "darwin")(
    "arms no timer on an idle wake loop and keeps replacement loops registered",
    () => {
      const script = [
        'import { GpuixRenderer } from "@gpuix/native"',
        'import { startFrameLoop } from "@gpuix/native/runtime"',
        "const renderer = new GpuixRenderer(() => {})",
        "renderer.init({ focus: false, show: false })",
        "let clears = 0",
        "let ticks = 0",
        "let wake",
        "let nextTick",
        "const host = {",
        "  requiresTick: () => renderer.requiresTick(),",
        "  tick: () => {",
        "    const running = renderer.tick()",
        "    ticks++",
        "    nextTick?.()",
        "    return running",
        "  },",
        "  setFrameWakeCallback: (callback) => {",
        "    if (callback === null) clears++",
        "    else wake = callback",
        "    return renderer.setFrameWakeCallback(callback)",
        "  },",
        "}",
        "const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))",
        "const first = startFrameLoop(host)",
        "const ticksAtStart = ticks",
        // The hidden window's startup work wakes the loop for about the first 500 ms; the next wake is seconds away.
        "await sleep(800)",
        "const ticksSettled = ticks",
        "await sleep(400)",
        "const ticksIdle = ticks - ticksSettled",
        "first.stop()",
        "const second = startFrameLoop(host)",
        "first.stop()",
        "const clearsBeforeWake = clears",
        "const ticksBeforeWake = ticks",
        "const woken = new Promise((resolve) => { nextTick = resolve })",
        "wake()",
        "await woken",
        "second.stop()",
        "console.log(JSON.stringify({ ticksAtStart, ticksIdle, clearsBeforeWake, ticksAfterWake: ticks - ticksBeforeWake, clears }))",
        "process.exit(0)",
      ].join("\n")
      const result = spawnSync("bun", ["-e", script], {
        encoding: "utf8",
        timeout: 5_000,
      })
      expect(result.status, result.stderr || result.error?.message).toBe(0)
      expect(JSON.parse(result.stdout.trim())).toEqual({
        ticksAtStart: 1,
        ticksIdle: 0,
        clearsBeforeWake: 1,
        ticksAfterWake: 1,
        clears: 2,
      })
    },
  )

  it.skipIf(process.platform !== "darwin")(
    "returns control to JavaScript while AppKit is idle",
    () => {
      const script = [
        'import { GpuixRenderer } from "@gpuix/native"',
        "const renderer = new GpuixRenderer(() => {})",
        "renderer.init({ focus: false })",
        "renderer.tick()",
        "const startedAt = performance.now()",
        "for (let index = 0; index < 30; index += 1) renderer.tick()",
        "console.log(performance.now() - startedAt)",
        "process.exit(0)",
      ].join("\n")
      const result = spawnSync(
        process.execPath,
        ["-e", script],
        { encoding: "utf8", timeout: 3_000 },
      )

      expect(result.status, result.stderr || result.error?.message).toBe(0)
      const elapsedMs = Number(result.stdout.trim())
      expect(elapsedMs).not.toBeNaN()
      expect(elapsedMs).toBeLessThan(200)
    },
  )

  it("does not tick when the native platform owns its event loop", () => {
    let ticks = 0
    const loop = startFrameLoop({
      requiresTick: () => false,
      tick: () => {
        ticks += 1
      },
    })

    expect(ticks).toBe(0)
    loop.stop()
  })

  it("schedules the next tick immediately after a long tick", async () => {
    let ticks = 0
    const loop = startFrameLoop(
      {
        requiresTick: () => true,
        tick: () => {
          ticks += 1
          if (ticks === 1) {
            const start = Date.now()
            while (Date.now() - start < 25) {}
          }
        },
      },
      { frameMs: 20 },
    )
    await new Promise((resolve) => setTimeout(resolve, 8))
    loop.stop()
    expect(ticks).toBeGreaterThanOrEqual(2)
  })

  it("still waits after a short tick", async () => {
    let ticks = 0
    const loop = startFrameLoop(
      {
        requiresTick: () => true,
        tick: () => {
          ticks += 1
        },
      },
      { frameMs: 40 },
    )
    await new Promise((resolve) => setTimeout(resolve, 15))
    loop.stop()
    expect(ticks).toBe(1)
  })

  it("stops and reports termination when tick returns false", async () => {
    let ticks = 0
    let terminated = 0
    const loop = startFrameLoop(
      {
        requiresTick: () => true,
        tick: () => {
          ticks += 1
          return false
        },
      },
      {
        frameMs: 5,
        onTerminated: () => {
          terminated += 1
        },
      },
    )
    await new Promise((resolve) => setTimeout(resolve, 20))
    expect(ticks).toBe(1)
    expect(terminated).toBe(1)
    loop.stop()
  })

  it("keeps ticking until a later false, then exits once", async () => {
    let ticks = 0
    let terminated = 0
    await new Promise<void>((resolve) => startFrameLoop(
      {
        requiresTick: () => true,
        tick: () => {
          ticks += 1
          return ticks < 3
        },
      },
      {
        frameMs: 5,
        onTerminated: () => {
          terminated += 1
          resolve()
        },
      },
    ))
    expect(ticks).toBe(3)
    expect(terminated).toBe(1)
  })

  it("keeps pumping after tick throws", async () => {
    let ticks = 0
    let terminated = 0
    const loop = startFrameLoop(
      {
        requiresTick: () => true,
        tick: () => {
          ticks += 1
          if (ticks === 1) throw new Error("frame boom")
          return true
        },
      },
      {
        frameMs: 5,
        onTerminated: () => {
          terminated += 1
        },
      },
    )
    await new Promise((resolve) => setTimeout(resolve, 25))
    loop.stop()
    expect(ticks).toBeGreaterThanOrEqual(2)
    expect(terminated).toBe(0)
  })

  it("keeps the process alive after an uncaught exception", () => {
    const rendererPath = fileURLToPath(
      new URL("../reconciler/renderer.ts", import.meta.url),
    )
    const script = [
      `import { installRuntimeErrorHandlers, startFrameLoop } from ${JSON.stringify(rendererPath)}`,
      "installRuntimeErrorHandlers()",
      "let ticks = 0",
      "startFrameLoop({",
      "  requiresTick: () => true,",
      "  tick: () => { ticks += 1; return true },",
      "}, { frameMs: 5 })",
      "setTimeout(() => { throw new Error('boom') }, 10)",
      "setTimeout(() => {",
      "  console.log(`SURVIVED ${ticks}`)",
      "  process.exit(0)",
      "}, 40)",
    ].join("\n")
    const result = spawnSync("bun", ["-e", script], {
      encoding: "utf8",
      timeout: 3_000,
    })
    expect(result.status, result.stderr || result.error?.message).toBe(0)
    expect(result.stdout).toMatch(/SURVIVED [1-9]/)
  })
})

describeNative("events", () => {
  let testRoot: ReturnType<typeof createTestRoot>

  beforeEach(() => {
    testRoot = createTestRoot()
  })

  describe("click events", () => {
    it("should handle onClick and trigger re-render", () => {
      function Counter() {
        const [count, setCount] = useState(0)
        return (
          <div
            style={{ width: 200, height: 50 }}
            onClick={() => setCount((c) => c + 1)}
          >
            <text>{`Count: ${count}`}</text>
          </div>
        )
      }

      testRoot.render(<Counter />)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Count: 0",
        ]
      `)

      // Click within the div bounds (GPUI does hit testing)
      testRoot.renderer.nativeSimulateClick(10, 10)

      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Count: 1",
        ]
      `)

      // Click again
      testRoot.renderer.nativeSimulateClick(10, 10)

      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Count: 2",
        ]
      `)
    })

    it("synthesizes click only for the primary button", () => {
      const received: string[] = []
      testRoot.render(
        <div
          style={{ width: 200, height: 50 }}
          onMouseDown={(event) => received.push(`down:${event.button}`)}
          onMouseUp={(event) => received.push(`up:${event.button}`)}
          onClick={(event) => received.push(`click:${event.button}:${event.isRightClick}`)}
        />,
      )

      for (const button of [1, 2]) {
        testRoot.renderer.nativeSimulateMouseDown(10, 10, button)
        testRoot.renderer.nativeSimulateMouseUp(10, 10, button)
      }
      expect(received).toEqual(["down:1", "up:1", "down:2", "up:2"])

      testRoot.renderer.nativeSimulateMouseDown(10, 10, 0)
      testRoot.renderer.nativeSimulateMouseUp(10, 10, 0)
      expect(received).toEqual([
        "down:1",
        "up:1",
        "down:2",
        "up:2",
        "down:0",
        "up:0",
        "click:0:false",
      ])
    })

    it("dispatches primary clicks from motion elements", () => {
      let clicks = 0
      testRoot.render(
        <motion.div
          initial={false}
          animate={{ width: 200 }}
          style={{ width: 200, height: 50 }}
          onClick={() => {
            clicks += 1
          }}
        />,
      )

      testRoot.renderer.nativeSimulateMouseDown(10, 10, 0)
      testRoot.renderer.nativeSimulateMouseUp(10, 10, 0)
      expect(clicks).toBe(1)
      testRoot.renderer.nativeSimulateMouseDown(10, 10, 2)
      testRoot.renderer.nativeSimulateMouseUp(10, 10, 2)
      expect(clicks).toBe(1)
    })

    it("dispatches primary clicks from native custom elements", () => {
      const clicks: EventPayload[] = []
      testRoot.render(
        <div style={{ display: "flex", padding: 20 }}>
          <code
            code="hello"
            language="ts"
            onClick={(event) => {
              clicks.push(event)
            }}
          />
        </div>,
      )

      testRoot.renderer.nativeSimulateMouseDown(30, 28, 0)
      testRoot.renderer.nativeSimulateMouseUp(30, 28, 0)
      expect(clicks).toHaveLength(1)
      expect(clicks[0]).toMatchObject({ button: 0, isRightClick: false })
      testRoot.renderer.nativeSimulateMouseDown(30, 28, 2)
      testRoot.renderer.nativeSimulateMouseUp(30, 28, 2)
      expect(clicks).toHaveLength(1)
    })
  })

  describe("keyboard events", () => {
    it("should handle onKeyDown and update state", () => {
      function KeyTracker() {
        const [lastKey, setLastKey] = useState("none")
        return (
          <div
            style={{ width: 200, height: 50 }}
            tabIndex={0}
            onKeyDown={(e: EventPayload) => setLastKey(e.key ?? "unknown")}
          >
            <text>{`Key: ${lastKey}`}</text>
          </div>
        )
      }

      testRoot.render(<KeyTracker />)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Key: none",
        ]
      `)

      const div = testRoot.renderer
        .findByType("div")
        .find((d) => d.events.has("keydown"))!

      testRoot.renderer.nativeSimulateKeystrokes(div.id, "ArrowDown")

      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Key: ArrowDown",
        ]
      `)

      testRoot.renderer.nativeSimulateKeystrokes(div.id, "Escape")

      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Key: Escape",
        ]
      `)
    })

    it("should pass modifiers in keyboard events", () => {
      const receivedEvents: EventPayload[] = []

      function ModifierTracker() {
        return (
          <div
            style={{ width: 200, height: 50 }}
            tabIndex={0}
            onKeyDown={(e: EventPayload) => receivedEvents.push(e)}
          />
        )
      }

      testRoot.render(<ModifierTracker />)
      const div = testRoot.renderer
        .findByType("div")
        .find((d) => d.events.has("keydown"))!

      testRoot.renderer.nativeSimulateKeystrokes(div.id, "Meta+s")

      expect(receivedEvents.length).toBeGreaterThanOrEqual(1)
      const event = receivedEvents.find((e) => e.key === "s")
      expect(event).toBeDefined()
      expect(event!.modifiers?.cmd).toBe(true)
    })
  })

  describe("hover events", () => {
    it("should handle mouseEnter and mouseLeave via mouse move", () => {
      function HoverBox() {
        const [hovered, setHovered] = useState(false)
        return (
          <div
            style={{ width: 200, height: 100 }}
            onMouseEnter={() => setHovered(true)}
            onMouseLeave={() => setHovered(false)}
          >
            <text>{hovered ? "hovered" : "not hovered"}</text>
          </div>
        )
      }

      testRoot.render(<HoverBox />)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "not hovered",
        ]
      `)

      // Move mouse into element bounds → triggers on_hover(true) → mouseEnter
      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "hovered",
        ]
      `)

      // Move mouse out of element bounds → triggers on_hover(false) → mouseLeave
      testRoot.renderer.nativeSimulateMouseMove(500, 500)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "not hovered",
        ]
      `)
    })
  })

  describe("pointer leaving the window", () => {
    it("fires mouseLeave without a final mouse move", () => {
      const log: string[] = []
      testRoot.render(
        <div
          style={{ width: 200, height: 100 }}
          onMouseEnter={() => log.push("enter")}
          onMouseLeave={() => log.push("leave")}
        />
      )

      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      expect(log).toEqual(["enter"])

      // The platform reports the exit at the window edge, not a move outside.
      testRoot.renderer.nativeSimulateMouseExit(199, 50)
      expect(log).toEqual(["enter", "leave"])

      testRoot.renderer.nativeSimulateMouseMove(60, 50)
      expect(log).toEqual(["enter", "leave", "enter"])
    })

    it("drops the hover style too", () => {
      testRoot.render(
        <div style={{ width: "100%", height: "100%", backgroundColor: "#11111b" }}>
          <div
            style={{
              width: 200,
              height: 100,
              backgroundColor: "#1e1e2e",
              hover: { backgroundColor: "#f38ba8" },
            }}
          />
        </div>
      )
      fs.mkdirSync(SHOTS_DIR, { recursive: true })
      const idle = `${SHOTS_DIR}/pointer-exit-idle.png`
      const hovered = `${SHOTS_DIR}/pointer-exit-hovered.png`
      const exited = `${SHOTS_DIR}/pointer-exit-exited.png`
      for (const file of [idle, hovered, exited]) fs.rmSync(file, { force: true })

      testRoot.renderer.nativeSimulateMouseMove(1200, 700)
      testRoot.renderer.captureScreenshot(idle)
      testRoot.renderer.nativeSimulateMouseMove(100, 50)
      testRoot.renderer.captureScreenshot(hovered)
      expectScreenshotsDiffer(idle, hovered)

      testRoot.renderer.nativeSimulateMouseExit(199, 50)
      testRoot.renderer.captureScreenshot(exited)
      expectScreenshotsEqual(idle, exited)
    })
  })

  describe("mouseDownOutside", () => {
    it("should handle click outside to close pattern", () => {
      function Dropdown() {
        const [open, setOpen] = useState(false)
        return (
          <div style={{ width: 400, height: 400 }}>
            <div
              style={{ width: 100, height: 30 }}
              onClick={() => setOpen(true)}
            >
              <text>trigger</text>
            </div>
            {open && (
              <div
                style={{ width: 100, height: 100 }}
                onMouseDownOutside={() => setOpen(false)}
              >
                <text>dropdown content</text>
              </div>
            )}
          </div>
        )
      }

      testRoot.render(<Dropdown />)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "trigger",
        ]
      `)

      // Click on the trigger to open (within trigger bounds)
      testRoot.renderer.nativeSimulateClick(10, 10)

      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "trigger",
          "dropdown content",
        ]
      `)

      // Click outside the dropdown — the dispatcher fires mousedownoutside
      testRoot.renderer.nativeSimulateClick(350, 350)

      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "trigger",
        ]
      `)
    })
  })

  describe("mouseDownOutside and the logical tree", () => {
    function Probe({
      log,
      overlay,
      overlayRef,
    }: {
      log: Array<[number, unknown]>
      overlay?: boolean
      overlayRef?: (element: unknown) => void
    }) {
      return (
        <div style={{ width: 600, height: 400, position: "relative" }}>
          <div
            testId="x"
            style={{ width: 100, height: 40 }}
            onMouseDownOutside={(event) => log.push([event.button ?? -1, event.target])}
          >
            <text>x</text>
            <anchored position={{ x: 300, y: 200 }} style={{ width: 120, height: 80 }}>
              <div testId="popup" style={{ width: 120, height: 80 }}>
                <text>popup</text>
              </div>
            </anchored>
          </div>
          {overlay && (
            <div
              ref={overlayRef}
              testId="overlay"
              style={{ position: "absolute", top: 0, left: 0, width: 200, height: 100, backgroundColor: "#222" }}
            />
          )}
        </div>
      )
    }

    it("does not fire for a press inside an anchored popup declared under the element", () => {
      const log: Array<[number, unknown]> = []
      testRoot.render(<Probe log={log} />)
      testRoot.renderer.nativeSimulateClick(10, 10)
      expect(log).toEqual([])
      testRoot.renderer.nativeSimulateClick(340, 240)
      expect(log).toEqual([])
      testRoot.renderer.nativeSimulateClick(500, 350)
      expect(log).toHaveLength(1)
    })

    it("fires for a press on an overlay above the element, with the overlay as target", () => {
      const log: Array<[number, unknown]> = []
      let overlay: unknown
      testRoot.render(<Probe log={log} overlay overlayRef={(element) => (overlay = element)} />)
      testRoot.renderer.nativeSimulateClick(10, 10)
      expect(overlay).toBeDefined()
      expect(log).toHaveLength(1)
      expect(log[0][0]).toBe(0)
      expect(log[0][1]).toBe(overlay)
    })

    it("fires for a non-primary button press", () => {
      const log: Array<[number, unknown]> = []
      testRoot.render(<Probe log={log} />)
      testRoot.renderer.nativeSimulateMouseDown(500, 350, 2)
      expect(log).toEqual([[2, expect.any(Object)]])
    })

    it("fires for every listener, the root's included, when the press hits no element", () => {
      const log: string[] = []
      testRoot.render(
        <div testId="root" style={{ width: 200, height: 100 }} onMouseDownOutside={() => log.push("root")}>
          <div testId="inner" style={{ width: 100, height: 40 }} onMouseDownOutside={() => log.push("inner")} />
        </div>
      )
      testRoot.renderer.nativeSimulateClick(10, 10)
      expect(log).toEqual([])
      testRoot.renderer.nativeSimulateClick(150, 50)
      expect(log).toEqual(["inner"])
      log.length = 0
      testRoot.renderer.nativeSimulateClick(500, 350)
      expect(log.sort()).toEqual(["inner", "root"])
    })
  })

  describe("dialog overlay", () => {
    it("should open a tooltip-like dialog on button click and close on outside click", () => {
      function DialogDemo() {
        const [open, setOpen] = useState(false)

        return (
          <div style={{ width: 420, height: 260, position: "relative" }}>
            <div
              style={{
                width: 120,
                height: 32,
                marginTop: 16,
                marginLeft: 16,
                borderRadius: 8,
                backgroundColor: "#2f4ea3",
              }}
              onClick={() => setOpen(true)}
            >
              <text>Open dialog</text>
            </div>

            {open && (
              <div
                style={{
                  position: "absolute",
                  top: 140,
                  left: 220,
                  width: 170,
                  height: 90,
                  padding: 10,
                  gap: 6,
                  borderRadius: 10,
                  borderWidth: 1,
                  borderStyle: "solid",
                  borderColor: "#3d4660",
                  backgroundColor: "#1c2233",
                }}
                onMouseDownOutside={() => setOpen(false)}
              >
                <text>Tooltip Dialog</text>
                <text>Some content inside</text>
              </div>
            )}
          </div>
        )
      }

      testRoot.render(<DialogDemo />)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Open dialog",
        ]
      `)

      // Open via button click.
      testRoot.renderer.nativeSimulateClick(20, 20)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Open dialog",
          "Tooltip Dialog",
          "Some content inside",
        ]
      `)

      // Click inside dialog bounds (relies on absolute top/left placement).
      testRoot.renderer.nativeSimulateClick(260, 170)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Open dialog",
          "Tooltip Dialog",
          "Some content inside",
        ]
      `)

      // Click outside to close.
      testRoot.renderer.nativeSimulateClick(40, 220)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Open dialog",
        ]
      `)
    })

    it("should capture screenshot changes when the dialog opens", () => {
      function DialogScreenshotProbe() {
        const [open, setOpen] = useState(false)

        return (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              width: "100%",
              height: "100%",
              backgroundColor: "#0f1320",
            }}
          >
            <div
              style={{
                width: 460,
                height: 260,
                position: "relative",
                borderRadius: 18,
                backgroundColor: "#1a2238",
                padding: 20,
              }}
              onClick={() => setOpen(true)}
            >
              <div
                style={{
                  width: 148,
                  height: 36,
                  borderRadius: 10,
                  backgroundColor: "#3a5ecf",
                }}
              >
                <text>Open dialog</text>
              </div>

              {open && (
                <div
                  style={{
                    position: "absolute",
                    top: 84,
                    left: 188,
                    width: 236,
                    height: 130,
                    padding: 12,
                    gap: 8,
                    borderRadius: 12,
                    borderWidth: 1,
                    borderStyle: "solid",
                    borderColor: "#4a5678",
                    backgroundColor: "#0d172b",
                  }}
                >
                  <text>Tooltip Dialog</text>
                  <text>Visual screenshot probe</text>
                </div>
              )}
            </div>
          </div>
        )
      }

      testRoot.render(<DialogScreenshotProbe />)

      const path0 = `${SHOTS_DIR}/gpuix-dialog-0.png`
      const path1 = `${SHOTS_DIR}/gpuix-dialog-1.png`

      if (fs.existsSync(path0)) fs.unlinkSync(path0)
      if (fs.existsSync(path1)) fs.unlinkSync(path1)

      testRoot.renderer.captureScreenshot(path0)
      // Click centered card area to open dialog.
      testRoot.renderer.nativeSimulateClick(640, 400)
      testRoot.renderer.captureScreenshot(path1)

      expect(fs.existsSync(path0)).toBe(true)
      expect(fs.existsSync(path1)).toBe(true)
      expect(fs.statSync(path0).size).toBeGreaterThan(0)
      expect(fs.statSync(path1).size).toBeGreaterThan(0)
      expect(fs.readFileSync(path0).equals(fs.readFileSync(path1))).toBe(false)
    })

    it("should support anchored deferred dialog overlays", () => {
      function AnchoredDialogDemo() {
        const [open, setOpen] = useState(false)

        return (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              width: "100%",
              height: "100%",
              backgroundColor: "#0c1020",
            }}
          >
            <div
              style={{
                width: 320,
                height: 180,
                borderRadius: 14,
                backgroundColor: "#1e2b4f",
                padding: 16,
              }}
              onClick={() => setOpen(true)}
            >
              <text>Open anchored</text>
              {open && (
                <anchored
                  position={{ x: 700, y: 360 }}
                  anchor="topLeft"
                  deferred
                  priority={1}
                >
                  <div
                    style={{
                      width: 190,
                      height: 96,
                      padding: 10,
                      gap: 6,
                      borderRadius: 12,
                      borderWidth: 1,
                      borderStyle: "solid",
                      borderColor: "#4f5b82",
                      backgroundColor: "#131c34",
                    }}
                    onMouseDownOutside={() => setOpen(false)}
                  >
                    <text>Anchored Dialog</text>
                    <text>Deferred popover layer</text>
                  </div>
                </anchored>
              )}
            </div>
          </div>
        )
      }

      testRoot.render(<AnchoredDialogDemo />)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Open anchored",
        ]
      `)

      // Click centered card to open anchored dialog.
      testRoot.renderer.nativeSimulateClick(640, 400)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Open anchored",
          "Anchored Dialog",
          "Deferred popover layer",
        ]
      `)

      // Click inside anchored dialog area (x/y from anchored props).
      testRoot.renderer.nativeSimulateClick(730, 390)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Open anchored",
          "Anchored Dialog",
          "Deferred popover layer",
        ]
      `)

      // Click outside should close via mouseDownOutside.
      testRoot.renderer.nativeSimulateClick(80, 80)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Open anchored",
        ]
      `)
    })
  })

  describe("keyboard navigation", () => {
    it("should support arrow key navigation in a list", () => {
      function SelectableList() {
        const items = ["Apple", "Banana", "Cherry"]
        const [selected, setSelected] = useState(0)

        return (
          <div
            style={{ width: 200, height: 200 }}
            tabIndex={0}
            onKeyDown={(e: EventPayload) => {
              if (e.key === "ArrowDown") {
                setSelected((s) => Math.min(s + 1, items.length - 1))
              } else if (e.key === "ArrowUp") {
                setSelected((s) => Math.max(s - 1, 0))
              }
            }}
          >
            {items.map((item, i) => (
              <div key={item}>
                <text>{`${i === selected ? "> " : "  "}${item}`}</text>
              </div>
            ))}
          </div>
        )
      }

      testRoot.render(<SelectableList />)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "> Apple",
          "  Banana",
          "  Cherry",
        ]
      `)

      const list = testRoot.renderer
        .findByType("div")
        .find((d) => d.events.has("keydown"))!

      // Arrow down through native GPUI pipeline
      testRoot.renderer.nativeSimulateKeystrokes(list.id, "ArrowDown")
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "  Apple",
          "> Banana",
          "  Cherry",
        ]
      `)

      // Arrow down again
      testRoot.renderer.nativeSimulateKeystrokes(list.id, "ArrowDown")
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "  Apple",
          "  Banana",
          "> Cherry",
        ]
      `)

      // Arrow down at bottom — should stay
      testRoot.renderer.nativeSimulateKeystrokes(list.id, "ArrowDown")
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "  Apple",
          "  Banana",
          "> Cherry",
        ]
      `)

      // Arrow up
      testRoot.renderer.nativeSimulateKeystrokes(list.id, "ArrowUp")
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "  Apple",
          "> Banana",
          "  Cherry",
        ]
      `)
    })
  })

  describe("wheel events", () => {
    it("should handle onWheel and receive DOM-signed pixel deltas", () => {
      const receivedEvents: EventPayload[] = []

      function ScrollBox() {
        return (
          <div
            style={{ width: 200, height: 200 }}
            onWheel={(e: EventPayload) => receivedEvents.push(e)}
          >
            <text>scrollable</text>
          </div>
        )
      }

      testRoot.render(<ScrollBox />)
      testRoot.renderer.nativeSimulateScrollWheel(100, 100, 0, 50)

      expect(receivedEvents.length).toBeGreaterThanOrEqual(1)
      const wheelEvent = receivedEvents.find((e) => e.eventType === "wheel")
      expect(wheelEvent).toBeDefined()
      // GPUI reports -50 for a wheel that scrolls down; the DOM reports +50.
      expect(wheelEvent!.deltaX).toBe(0)
      expect(wheelEvent!.deltaY).toBe(50)
      expect(wheelEvent).toMatchObject({
        deltaMode: 0,
        clientX: 100,
        clientY: 100,
        touchPhase: "started",
      })
    })

    it("should update state on wheel", () => {
      function ScrollCounter() {
        const [scrollCount, setScrollCount] = useState(0)
        return (
          <div
            style={{ width: 200, height: 200 }}
            onWheel={() => setScrollCount((c) => c + 1)}
          >
            <text>{`Scrolls: ${scrollCount}`}</text>
          </div>
        )
      }

      testRoot.render(<ScrollCounter />)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Scrolls: 0",
        ]
      `)

      testRoot.renderer.nativeSimulateScrollWheel(100, 100, 0, 30)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Scrolls: 1",
        ]
      `)
    })
  })

  describe("file drag events", () => {
    const dragDir = fs.mkdtempSync(path.join(os.tmpdir(), "gpuix-file-drag-"))
    const textFile = `${dragDir}/notes.txt`
    const imageFile = `${dragDir}/Photo.PNG`
    const bareFile = `${dragDir}/README`
    fs.writeFileSync(textFile, "hello")
    fs.writeFileSync(imageFile, "12345678")
    fs.writeFileSync(bareFile, "")
    fs.utimesSync(textFile, new Date(1_700_000_000_000), new Date(1_700_000_000_000))

    const acceptDrops = (event: HostDragEvent) => event.preventDefault()

    it("delivers enter, over and drop in order with the dropped files", () => {
      const log: string[] = []
      let files: readonly HostFile[] = []

      testRoot.render(
        <div
          style={{ width: 200, height: 200 }}
          onDragEnter={(event) => log.push(event.type)}
          onDragOver={(event) => {
            log.push(event.type)
            event.preventDefault()
          }}
          onDragLeave={(event) => log.push(event.type)}
          onDrop={(event) => {
            log.push(event.type)
            files = event.dataTransfer?.files ?? []
          }}
        />,
      )

      testRoot.renderer.nativeSimulateFileDragEnter(100, 100, [textFile])
      testRoot.renderer.nativeSimulateFileDragMove(110, 100)
      testRoot.renderer.nativeSimulateFileDragSubmit(110, 100)
      testRoot.renderer.nativeSimulateFileDragExit()
      testRoot.renderer.nativeSimulateFileDragEnd()

      expect(log).toEqual(["dragenter", "dragover", "dragover", "drop"])
      expect(files.map((file) => file.path)).toEqual([textFile])
    })

    it("fires dragleave when the files exit the window", () => {
      const log: string[] = []

      testRoot.render(
        <div
          style={{ width: 200, height: 200 }}
          onDragEnter={(event) => log.push(`enter:${event.relatedTarget}`)}
          onDragOver={acceptDrops}
          onDragLeave={(event) => log.push(`leave:${event.relatedTarget}`)}
          onDrop={(event) => log.push(event.type)}
        />,
      )

      testRoot.renderer.nativeSimulateFileDragEnter(100, 100, [textFile])
      testRoot.renderer.nativeSimulateFileDragExit()
      testRoot.renderer.nativeSimulateFileDragEnd()
      // Nothing is left to leave.
      testRoot.renderer.nativeSimulateFileDragExit()

      expect(log).toEqual(["enter:null", "leave:null"])
    })

    it("enters the element dragged onto before leaving the one dragged off", () => {
      const log: string[] = []
      const track = (name: string) => ({
        onDragEnter: () => log.push(`${name}:enter`),
        onDragLeave: () => log.push(`${name}:leave`),
        onDragOver: () => log.push(`${name}:over`),
      })

      testRoot.render(
        <div style={{ width: 400, height: 400, display: "flex", flexDirection: "column" }}>
          <div style={{ width: 100, height: 100 }} {...track("a")} />
          <div style={{ width: 100, height: 100 }} {...track("b")} />
        </div>,
      )

      testRoot.renderer.nativeSimulateFileDragEnter(50, 50, [textFile])
      testRoot.renderer.nativeSimulateFileDragMove(50, 60)
      testRoot.renderer.nativeSimulateFileDragMove(50, 150)

      expect(log).toEqual(["a:enter", "a:over", "a:over", "b:enter", "a:leave", "b:over"])
    })

    it("bubbles drag events to ancestors and the drop to the ancestor of a child", () => {
      let parent: unknown
      let child: unknown
      const log: Array<[unknown, unknown]> = []

      testRoot.render(
        <div
          ref={(element) => (parent = element)}
          style={{ width: 200, height: 200, display: "flex", flexDirection: "column" }}
          onDragOver={acceptDrops}
          onDrop={(event) => log.push([event.target, event.currentTarget])}
        >
          <div ref={(element) => (child = element)} style={{ width: 100, height: 100 }} />
        </div>,
      )

      testRoot.renderer.nativeSimulateFileDrop(40, 40, [textFile])

      expect(log).toHaveLength(1)
      expect(log[0][0]).toBe(child)
      expect(log[0][1]).toBe(parent)
    })

    it("does not deliver a drop whose last dragover was not cancelled", () => {
      const log: string[] = []

      testRoot.render(
        <div
          style={{ width: 200, height: 200 }}
          onDragOver={() => log.push("over")}
          onDragLeave={(event) => log.push(`leave:${event.relatedTarget}`)}
          onDrop={() => log.push("drop")}
        />,
      )

      testRoot.renderer.nativeSimulateFileDrop(100, 100, [textFile])

      expect(log).toEqual(["over", "leave:null"])
    })

    it("does not deliver a drop when a target without dragover follows an accepting one", () => {
      const log: string[] = []

      testRoot.render(
        <div style={{ width: 400, height: 400, display: "flex", flexDirection: "column" }}>
          <div
            style={{ width: 100, height: 100 }}
            onDragOver={acceptDrops}
            onDrop={() => log.push("a:drop")}
          />
          <div style={{ width: 100, height: 100 }} onDrop={() => log.push("b:drop")} />
        </div>,
      )

      testRoot.renderer.nativeSimulateFileDragEnter(50, 50, [textFile])
      testRoot.renderer.nativeSimulateFileDragMove(50, 150)
      testRoot.renderer.nativeSimulateFileDragSubmit(50, 150)

      expect(log).toEqual([])
    })

    it("does not deliver a drop whose dropEffect is none", () => {
      const log: string[] = []

      testRoot.render(
        <div
          style={{ width: 200, height: 200 }}
          onDragOver={(event) => {
            event.preventDefault()
            event.dataTransfer!.dropEffect = "none"
          }}
          onDrop={() => log.push("drop")}
        />,
      )

      testRoot.renderer.nativeSimulateFileDrop(100, 100, [textFile])

      expect(log).toEqual([])
    })

    it("lets the next drag start fresh after one that was refused", () => {
      const log: string[] = []
      let accept = false

      testRoot.render(
        <div
          style={{ width: 200, height: 200 }}
          onDragOver={(event) => {
            if (accept) event.preventDefault()
          }}
          onDrop={() => log.push("drop")}
        />,
      )

      testRoot.renderer.nativeSimulateFileDrop(100, 100, [textFile])
      accept = true
      testRoot.renderer.nativeSimulateFileDrop(100, 100, [textFile])

      expect(log).toEqual(["drop"])
    })

    it("fires no pointer or mouse event during a file drag", () => {
      const log: string[] = []
      const record = (event: HostMouseEvent) => log.push(event.type)

      testRoot.render(
        <div
          style={{ width: 400, height: 400, display: "flex", flexDirection: "column" }}
          onPointerMove={record}
          onMouseMove={record}
          onPointerOver={record}
          onMouseOver={record}
          onPointerEnter={record}
          onMouseEnter={record}
          onPointerOut={record}
          onMouseOut={record}
          onPointerLeave={record}
          onMouseLeave={record}
          onPointerUp={record}
          onMouseUp={record}
          onClick={record}
          onDragOver={acceptDrops}
          onDrop={record}
        >
          <div style={{ width: 100, height: 100 }} />
          <div style={{ width: 100, height: 100 }} />
        </div>,
      )

      testRoot.renderer.nativeSimulateFileDragEnter(50, 50, [textFile])
      testRoot.renderer.nativeSimulateFileDragMove(50, 150)
      testRoot.renderer.nativeSimulateFileDragSubmit(50, 150)
      testRoot.renderer.nativeSimulateFileDragExit()
      testRoot.renderer.nativeSimulateFileDragEnd()

      expect(log).toEqual(["drop"])
    })

    it("gives every file its size, type, name and modification time", () => {
      let files: readonly HostFile[] = []

      testRoot.render(
        <div
          style={{ width: 200, height: 200 }}
          onDragOver={acceptDrops}
          onDrop={(event) => {
            files = event.dataTransfer?.files ?? []
          }}
        />,
      )

      testRoot.renderer.nativeSimulateFileDrop(40, 40, [textFile, imageFile, bareFile])

      expect(files).toEqual([
        {
          path: textFile,
          name: "notes.txt",
          size: 5,
          type: "text/plain",
          lastModified: 1_700_000_000_000,
        },
        {
          path: imageFile,
          name: "Photo.PNG",
          size: 8,
          type: "image/png",
          lastModified: Math.floor(fs.statSync(imageFile).mtimeMs),
        },
        {
          path: bareFile,
          name: "README",
          size: 0,
          type: "",
          lastModified: Math.floor(fs.statSync(bareFile).mtimeMs),
        },
      ])
    })

    it("hands every event of one drag the same dataTransfer, with the files", () => {
      const seen: HostDataTransfer[] = []

      testRoot.render(
        <div
          style={{ width: 200, height: 200 }}
          onDragEnter={(event) => seen.push(event.dataTransfer!)}
          onDragOver={(event) => {
            seen.push(event.dataTransfer!)
            event.preventDefault()
          }}
          onDrop={(event) => seen.push(event.dataTransfer!)}
        />,
      )

      testRoot.renderer.nativeSimulateFileDrop(40, 40, [textFile])

      expect(seen).toHaveLength(3)
      expect(new Set(seen).size).toBe(1)
      expect(seen[0]!.types).toEqual(["Files"])
      expect(seen[0]!.files.map((file) => file.name)).toEqual(["notes.txt"])
      expect(seen[0]!.getData("text/plain")).toBe("")
    })

    it("ignores a drag with no files", () => {
      const log: string[] = []

      testRoot.render(
        <div
          style={{ width: 200, height: 200 }}
          onDragEnter={(event) => log.push(event.type)}
          onDragOver={(event) => log.push(event.type)}
          onDrop={(event) => log.push(event.type)}
        />,
      )

      testRoot.renderer.nativeSimulateFileDrop(40, 40, [])
      testRoot.renderer.nativeSimulateFileDrop(40, 40, [`${dragDir}/deleted.txt`])

      expect(log).toEqual([])
    })

    it("delivers a nested drop to the innermost accepting element, then bubbles", () => {
      const log: string[] = []

      testRoot.render(
        <div
          style={{ width: 400, height: 400, display: "flex", flexDirection: "column" }}
          onDragOver={acceptDrops}
          onDrop={() => log.push("outer")}
        >
          <div
            style={{ width: 100, height: 100 }}
            onDragOver={acceptDrops}
            onDrop={(event) => {
              log.push("inner")
              event.stopPropagation()
            }}
          />
        </div>,
      )

      testRoot.renderer.nativeSimulateFileDrop(50, 50, [textFile])
      expect(log).toEqual(["inner"])

      testRoot.renderer.nativeSimulateFileDrop(200, 200, [textFile])
      expect(log).toEqual(["inner", "outer"])
    })

    describe("the operation answered to the OS", () => {
      it("is the dropEffect of a cancelled dragover", () => {
        testRoot.render(
          <div
            style={{ width: 200, height: 200 }}
            onDragOver={(event) => {
              event.preventDefault()
              if (event.dataTransfer) event.dataTransfer.dropEffect = "link"
            }}
          />,
        )

        expect(testRoot.renderer.nativeSimulateFileDragEnter(100, 100, [textFile])).toBe("link")
        expect(testRoot.renderer.nativeSimulateFileDragMove(110, 100)).toBe("link")
      })

      it("is copy when the dragover is cancelled and picks no effect", () => {
        testRoot.render(<div style={{ width: 200, height: 200 }} onDragOver={acceptDrops} />)

        expect(testRoot.renderer.nativeSimulateFileDragEnter(100, 100, [textFile])).toBe("copy")
      })

      it("is move when the handler picks it", () => {
        testRoot.render(
          <div
            style={{ width: 200, height: 200 }}
            onDragOver={(event) => {
              event.preventDefault()
              if (event.dataTransfer) event.dataTransfer.dropEffect = "move"
            }}
          />,
        )

        expect(testRoot.renderer.nativeSimulateFileDragEnter(100, 100, [textFile])).toBe("move")
      })

      it("refuses the drop when the dragover is not cancelled", () => {
        testRoot.render(
          <div
            style={{ width: 200, height: 200 }}
            onDragOver={(event) => {
              if (event.dataTransfer) event.dataTransfer.dropEffect = "copy"
            }}
          />,
        )

        expect(testRoot.renderer.nativeSimulateFileDragEnter(100, 100, [textFile])).toBe("none")
      })

      it("refuses the drop when the cancelled dragover chose none", () => {
        testRoot.render(
          <div
            style={{ width: 200, height: 200 }}
            onDragOver={(event) => {
              event.preventDefault()
              if (event.dataTransfer) event.dataTransfer.dropEffect = "none"
            }}
          />,
        )

        expect(testRoot.renderer.nativeSimulateFileDragEnter(100, 100, [textFile])).toBe("none")
      })

      it("refuses the drop when only the dragenter was cancelled", () => {
        const log: string[] = []
        testRoot.render(
          <div
            style={{ width: 200, height: 200 }}
            onDragEnter={acceptDrops}
            onDragLeave={() => log.push("leave")}
            onDrop={() => log.push("drop")}
          />,
        )

        expect(testRoot.renderer.nativeSimulateFileDragEnter(100, 100, [textFile])).toBe("none")
        testRoot.renderer.nativeSimulateFileDragSubmit(100, 100)

        expect(log).toEqual(["leave"])
      })

      it("is the answer of the last dragover", () => {
        let accept = true
        testRoot.render(
          <div
            style={{ width: 200, height: 200 }}
            onDragOver={(event) => {
              if (accept) event.preventDefault()
            }}
          />,
        )

        expect(testRoot.renderer.nativeSimulateFileDragEnter(100, 100, [textFile])).toBe("copy")
        accept = false
        expect(testRoot.renderer.nativeSimulateFileDragMove(110, 100)).toBe("none")
        accept = true
        expect(testRoot.renderer.nativeSimulateFileDragMove(120, 100)).toBe("copy")
      })

      it("refuses the drop in a window with nothing rendered", () => {
        expect(testRoot.renderer.nativeSimulateFileDragEnter(100, 100, [textFile])).toBe("none")
      })

      it("refuses the drop over an element nothing listens on", () => {
        testRoot.render(<div style={{ width: 200, height: 200 }} />)

        expect(testRoot.renderer.nativeSimulateFileDragEnter(100, 100, [textFile])).toBe("none")
      })

      it("follows the element under the pointer", () => {
        testRoot.render(
          <div style={{ width: 400, height: 400, display: "flex", flexDirection: "column" }}>
            <div style={{ width: 100, height: 100 }} onDragOver={acceptDrops} />
            <div style={{ width: 100, height: 100 }} />
          </div>,
        )

        expect(testRoot.renderer.nativeSimulateFileDragEnter(50, 50, [textFile])).toBe("copy")
        expect(testRoot.renderer.nativeSimulateFileDragMove(50, 150)).toBe("none")
        expect(testRoot.renderer.nativeSimulateFileDragMove(50, 60)).toBe("copy")
      })

      it("refuses a drag with no readable file", () => {
        testRoot.render(<div style={{ width: 200, height: 200 }} onDragOver={acceptDrops} />)

        expect(testRoot.renderer.nativeSimulateFileDragEnter(40, 40, [])).toBe("none")
        expect(testRoot.renderer.nativeSimulateFileDragMove(40, 40)).toBe("none")
      })

      it("does not carry over to the next drag", () => {
        testRoot.render(<div style={{ width: 200, height: 200 }} onDragOver={acceptDrops} />)

        expect(testRoot.renderer.nativeSimulateFileDragEnter(40, 40, [textFile])).toBe("copy")
        testRoot.renderer.nativeSimulateFileDragExit()
        testRoot.renderer.nativeSimulateFileDragEnd()
        testRoot.render(<div style={{ width: 200, height: 200 }} />)
        expect(testRoot.renderer.nativeSimulateFileDragEnter(40, 40, [textFile])).toBe("none")
      })
    })

    it("delivers the drag events to an img leaf", () => {
      const log: string[] = []
      const svg =
        '<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="#5ca9ff"/></svg>'
      const src = `data:image/svg+xml;base64,${Buffer.from(svg).toString("base64")}`

      testRoot.render(
        <img
          src={src}
          style={{ width: 220, height: 120 }}
          onDragOver={acceptDrops}
          onDrop={(event) => log.push(event.dataTransfer!.files[0]!.name)}
        />,
      )

      testRoot.renderer.nativeSimulateFileDrop(80, 40, [textFile])

      expect(log).toEqual(["notes.txt"])
    })
  })

  describe("keyDown and keyUp events", () => {
    it("should handle onKeyDown via nativeSimulateKeyDown", () => {
      function KeyTracker() {
        const [lastKey, setLastKey] = useState("none")
        return (
          <div
            style={{ width: 200, height: 50 }}
            tabIndex={0}
            onKeyDown={(e: EventPayload) => setLastKey(e.key ?? "unknown")}
          >
            <text>{`Key: ${lastKey}`}</text>
          </div>
        )
      }

      testRoot.render(<KeyTracker />)
      const div = testRoot.renderer
        .findByType("div")
        .find((d) => d.events.has("keydown"))!

      testRoot.renderer.nativeSimulateKeyDown(div.id, "a")

      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Key: a",
        ]
      `)
    })

    it("should handle onKeyUp via nativeSimulateKeyUp", () => {
      const events: string[] = []

      function KeyUpTracker() {
        return (
          <div
            style={{ width: 200, height: 50 }}
            tabIndex={0}
            onKeyDown={(e: EventPayload) => events.push(`down:${e.key}`)}
            onKeyUp={(e: EventPayload) => events.push(`up:${e.key}`)}
          />
        )
      }

      testRoot.render(<KeyUpTracker />)
      const div = testRoot.renderer
        .findByType("div")
        .find((d) => d.events.has("keydown") && d.events.has("keyup"))!

      testRoot.renderer.nativeSimulateKeyDown(div.id, "Enter")
      testRoot.renderer.nativeSimulateKeyUp(div.id, "Enter")

      expect(events).toContain("down:Enter")
      expect(events).toContain("up:Enter")
    })

    it("should handle onKeyUp state update", () => {
      function KeyUpStateTracker() {
        const [lastKey, setLastKey] = useState("none")
        return (
          <div
            style={{ width: 200, height: 50 }}
            tabIndex={0}
            onKeyUp={(e: EventPayload) => setLastKey(e.key ?? "unknown")}
          >
            <text>{`Released: ${lastKey}`}</text>
          </div>
        )
      }

      testRoot.render(<KeyUpStateTracker />)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Released: none",
        ]
      `)

      const div = testRoot.renderer
        .findByType("div")
        .find((d) => d.events.has("keyup"))!

      testRoot.renderer.nativeSimulateKeyUp(div.id, "a")
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Released: a",
        ]
      `)
    })
  })

  describe("mouseDown and mouseUp events", () => {
    it("should handle onMouseDown and onMouseUp", () => {
      function PressTracker() {
        const [pressed, setPressed] = useState(false)
        return (
          <div
            style={{ width: 200, height: 100 }}
            onMouseDown={() => setPressed(true)}
            onMouseUp={() => setPressed(false)}
          >
            <text>{pressed ? "pressed" : "released"}</text>
          </div>
        )
      }

      testRoot.render(<PressTracker />)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "released",
        ]
      `)

      testRoot.renderer.nativeSimulateMouseDown(10, 10)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "pressed",
        ]
      `)

      testRoot.renderer.nativeSimulateMouseUp(10, 10)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "released",
        ]
      `)
    })

    it("should receive correct mouse button in mouseDown payload", () => {
      const receivedEvents: EventPayload[] = []

      function ButtonTracker() {
        return (
          <div
            style={{ width: 200, height: 100 }}
            onMouseDown={(e: EventPayload) => receivedEvents.push(e)}
          />
        )
      }

      testRoot.render(<ButtonTracker />)

      // Left click (button=0)
      testRoot.renderer.nativeSimulateMouseDown(10, 10, 0)
      expect(receivedEvents[0].button).toBe(0)

      // Right click (button=2)
      testRoot.renderer.nativeSimulateMouseDown(10, 10, 2)
      expect(receivedEvents[1].button).toBe(2)

      // Middle click (button=1)
      testRoot.renderer.nativeSimulateMouseDown(10, 10, 1)
      expect(receivedEvents[2].button).toBe(1)
    })
  })

  describe("mouseMove events", () => {
    it("should handle onMouseMove and receive exact position", () => {
      const receivedEvents: EventPayload[] = []

      function MoveTracker() {
        return (
          <div
            style={{ width: 300, height: 300 }}
            onMouseMove={(e: EventPayload) => receivedEvents.push(e)}
          />
        )
      }

      testRoot.render(<MoveTracker />)
      testRoot.renderer.nativeSimulateMouseMove(50, 75)

      expect(receivedEvents.length).toBeGreaterThanOrEqual(1)
      const moveEvent = receivedEvents.find(
        (e) => e.eventType === "mousemove"
      )
      expect(moveEvent).toBeDefined()
      expect(moveEvent!.eventType).toBe("mousemove")
      expect(moveEvent!.x).toBe(50)
      expect(moveEvent!.y).toBe(75)
    })

    it("should receive pressedButton during drag", () => {
      const receivedEvents: EventPayload[] = []

      function DragTracker() {
        return (
          <div
            style={{ width: 300, height: 300 }}
            onMouseMove={(e: EventPayload) => receivedEvents.push(e)}
          />
        )
      }

      testRoot.render(<DragTracker />)

      // Move without button pressed
      testRoot.renderer.nativeSimulateMouseMove(10, 10)
      expect(receivedEvents.length).toBeGreaterThanOrEqual(1)
      const noButtonEvent = receivedEvents.find((e) => e.eventType === "mousemove")!
      expect(noButtonEvent.pressedButton).toBeUndefined()

      // Move with left button pressed (simulating drag)
      receivedEvents.length = 0
      testRoot.renderer.nativeSimulateMouseDown(10, 10)
      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      const dragEvent = receivedEvents.find((e) => e.eventType === "mousemove")!
      expect(dragEvent.pressedButton).toBe(0)
    })

    it("should update state on mouse move", () => {
      function PositionTracker() {
        const [pos, setPos] = useState("0,0")
        return (
          <div
            style={{ width: 300, height: 300 }}
            onMouseMove={(e: EventPayload) =>
              setPos(`${Math.round(e.x ?? 0)},${Math.round(e.y ?? 0)}`)
            }
          >
            <text>{`Position: ${pos}`}</text>
          </div>
        )
      }

      testRoot.render(<PositionTracker />)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Position: 0,0",
        ]
      `)

      testRoot.renderer.nativeSimulateMouseMove(42, 99)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Position: 42,99",
        ]
      `)
    })

    it("keeps mouseMove and mouseUp after the pointer leaves the hitbox when mouseDown captures", () => {
      const received: string[] = []

      function Handle() {
        const handle = useRef<PublicInstance>(null)
        return (
          <div
            ref={handle}
            style={{ width: 80, height: 40, backgroundColor: "#3366ff" }}
            onMouseDown={() => {
              received.push("down")
              handle.current?.setPointerCapture?.(1)
            }}
            onMouseMove={(e: EventPayload) =>
              received.push(`move:${Math.round(e.x ?? 0)},${e.pressedButton}`)
            }
            onMouseUp={() => received.push("up")}
          >
            <text>handle</text>
          </div>
        )
      }

      testRoot.render(<Handle />)
      testRoot.renderer.nativeSimulateMouseDown(20, 20)
      testRoot.renderer.nativeSimulateMouseMove(200, 20)
      testRoot.renderer.nativeSimulateMouseUp(200, 20, 0)

      expect(received).toEqual(["down", "move:200,0", "up"])
    })

    it("does not capture when the element listens for both mouseDown and mouseMove", () => {
      const received: string[] = []

      function Handle() {
        return (
          <div
            style={{ width: 80, height: 40, backgroundColor: "#3366ff" }}
            onMouseDown={() => received.push("down")}
            onMouseMove={() => received.push("move")}
            onMouseUp={() => received.push("up")}
          >
            <text>handle</text>
          </div>
        )
      }

      testRoot.render(<Handle />)
      testRoot.renderer.nativeSimulateMouseDown(20, 20)
      testRoot.renderer.nativeSimulateMouseMove(30, 20)
      testRoot.renderer.nativeSimulateMouseMove(200, 20)
      testRoot.renderer.nativeSimulateMouseUp(200, 20, 0)

      expect(received).toEqual(["down", "move"])
    })

    it("does not swallow a child's click when nothing has captured", () => {
      const received: string[] = []

      function Row() {
        return (
          <div
            style={{ width: 200, height: 60 }}
            onMouseDown={() => received.push("row-down")}
            onMouseMove={() => received.push("row-move")}
            onMouseUp={() => received.push("row-up")}
          >
            <div
              style={{ width: 80, height: 40, backgroundColor: "#3366ff" }}
              onClick={() => received.push("child-click")}
            >
              <text>child</text>
            </div>
          </div>
        )
      }

      testRoot.render(<Row />)
      testRoot.renderer.nativeSimulateMouseDown(20, 20)
      testRoot.renderer.nativeSimulateMouseUp(20, 20, 0)

      expect(received).toEqual(["row-down", "row-up", "child-click"])
    })

    it("does not capture when the element only listens for mouseDown and mouseUp", () => {
      const received: string[] = []

      function PressOnly() {
        return (
          <div
            style={{ width: 80, height: 40, backgroundColor: "#3366ff" }}
            onMouseDown={() => received.push("down")}
            onMouseUp={() => received.push("up")}
          >
            <text>press</text>
          </div>
        )
      }

      testRoot.render(<PressOnly />)
      testRoot.renderer.nativeSimulateMouseDown(20, 20)
      testRoot.renderer.nativeSimulateMouseUp(200, 20, 0)

      expect(received).toEqual(["down"])
    })

    it("still delivers move and up to an overlay mounted on mouseDown", () => {
      const received: string[] = []

      function OverlayDrag() {
        const [dragging, setDragging] = useState(false)
        return (
          <div
            style={{
              width: 400,
              height: 200,
              position: "relative",
              backgroundColor: "#111111",
            }}
          >
            <div
              style={{ width: 80, height: 40, backgroundColor: "#3366ff" }}
              onMouseDown={() => {
                received.push("clip-down")
                setDragging(true)
              }}
            >
              <text>clip</text>
            </div>
            {dragging && (
              <div
                style={{
                  position: "absolute",
                  top: 0,
                  left: 0,
                  width: 400,
                  height: 200,
                  backgroundColor: "#00000001",
                }}
                onMouseMove={() => received.push("overlay-move")}
                onMouseUp={() => received.push("overlay-up")}
              >
                <text>overlay</text>
              </div>
            )}
          </div>
        )
      }

      testRoot.render(<OverlayDrag />)
      testRoot.renderer.nativeSimulateMouseDown(20, 20)
      testRoot.renderer.nativeSimulateMouseMove(200, 20)
      testRoot.renderer.nativeSimulateMouseUp(200, 20, 0)

      expect(received).toEqual(["clip-down", "overlay-move", "overlay-up"])
    })

    it("fires mouseUp once when released inside the captured element", () => {
      const received: string[] = []

      function Handle() {
        const handle = useRef<PublicInstance>(null)
        return (
          <div
            ref={handle}
            style={{ width: 80, height: 40, backgroundColor: "#3366ff" }}
            onMouseDown={() => {
              received.push("down")
              handle.current?.setPointerCapture?.(1)
            }}
            onMouseMove={() => received.push("move")}
            onMouseUp={() => received.push("up")}
          >
            <text>handle</text>
          </div>
        )
      }

      testRoot.render(<Handle />)
      testRoot.renderer.nativeSimulateMouseDown(20, 20)
      testRoot.renderer.nativeSimulateMouseMove(30, 20)
      testRoot.renderer.nativeSimulateMouseUp(30, 20, 0)

      expect(received).toEqual(["down", "move", "up"])
    })

    it("does not deliver captured moves to a sibling", () => {
      const received: string[] = []

      function Pair() {
        const handle = useRef<PublicInstance>(null)
        return (
          <div style={{ width: 400, height: 80, display: "flex" }}>
            <div
              ref={handle}
              style={{ width: 80, height: 40, backgroundColor: "#3366ff" }}
              onMouseDown={() => {
                received.push("handle-down")
                handle.current?.setPointerCapture?.(1)
              }}
              onMouseMove={() => received.push("handle-move")}
              onMouseUp={() => received.push("handle-up")}
            >
              <text>handle</text>
            </div>
            <div
              style={{ width: 200, height: 40, backgroundColor: "#22aa66" }}
              onMouseMove={() => received.push("sibling-move")}
              onMouseUp={() => received.push("sibling-up")}
            >
              <text>sibling</text>
            </div>
          </div>
        )
      }

      testRoot.render(<Pair />)
      testRoot.renderer.nativeSimulateMouseDown(20, 20)
      testRoot.renderer.nativeSimulateMouseMove(160, 20)
      testRoot.renderer.nativeSimulateMouseUp(160, 20, 0)

      expect(received).toEqual(["handle-down", "handle-move", "handle-up"])
    })

    it("releases capture when the captured node is removed", () => {
      const received: string[] = []

      function Drag() {
        const handle = useRef<PublicInstance>(null)
        const [gone, setGone] = useState(false)
        if (gone) {
          return (
            <div
              style={{ width: 400, height: 80, backgroundColor: "#22aa66" }}
              onMouseMove={() => received.push("replacement-move")}
              onMouseUp={() => received.push("replacement-up")}
            >
              <text>replacement</text>
            </div>
          )
        }
        return (
          <div
            ref={handle}
            style={{ width: 80, height: 40, backgroundColor: "#3366ff" }}
            onMouseDown={() => {
              received.push("down")
              handle.current?.setPointerCapture?.(1)
              setGone(true)
            }}
            onMouseMove={() => received.push("handle-move")}
            onMouseUp={() => received.push("handle-up")}
          >
            <text>handle</text>
          </div>
        )
      }

      testRoot.render(<Drag />)
      testRoot.renderer.nativeSimulateMouseDown(20, 20)
      testRoot.renderer.nativeSimulateMouseMove(20, 20)
      testRoot.renderer.nativeSimulateMouseUp(20, 20, 0)

      expect(received).toEqual(["down", "replacement-move", "replacement-up"])
    })

    it("does not hover a sibling while the pointer is captured", () => {
      const received: string[] = []

      function Pair() {
        const handle = useRef<PublicInstance>(null)
        return (
          <div style={{ width: 400, height: 80, display: "flex" }}>
            <div
              ref={handle}
              style={{ width: 80, height: 40, backgroundColor: "#3366ff" }}
              onMouseDown={() => {
                received.push("handle-down")
                handle.current?.setPointerCapture?.(1)
              }}
              onMouseMove={() => received.push("handle-move")}
              onMouseUp={() => received.push("handle-up")}
            >
              <text>handle</text>
            </div>
            <div
              style={{ width: 200, height: 40, backgroundColor: "#22aa66" }}
              onMouseEnter={() => received.push("sibling-enter")}
            >
              <text>sibling</text>
            </div>
          </div>
        )
      }

      testRoot.render(<Pair />)
      testRoot.renderer.nativeSimulateMouseDown(20, 20)
      testRoot.renderer.nativeSimulateMouseMove(160, 20)
      expect(received).toEqual(["handle-down", "handle-move"])
      testRoot.renderer.nativeSimulateMouseUp(160, 20, 0)
      expect(received.slice(0, 3)).toEqual([
        "handle-down",
        "handle-move",
        "handle-up",
      ])
    })
  })

  describe("combined event interactions", () => {
    it("should support keyboard shortcuts with modifiers", () => {
      function ShortcutHandler() {
        const [action, setAction] = useState("none")

        return (
          <div
            style={{ width: 200, height: 50 }}
            tabIndex={0}
            onKeyDown={(e: EventPayload) => {
              const mods = e.modifiers
              if (mods?.cmd && e.key === "s") {
                setAction("save")
              } else if (mods?.cmd && mods?.shift && e.key === "P") {
                setAction("command-palette")
              } else if (e.key === "Escape") {
                setAction("cancel")
              }
            }}
          >
            <text>{`Action: ${action}`}</text>
          </div>
        )
      }

      testRoot.render(<ShortcutHandler />)
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Action: none",
        ]
      `)

      const div = testRoot.renderer
        .findByType("div")
        .find((d) => d.events.has("keydown"))!

      // Cmd+S
      testRoot.renderer.nativeSimulateKeystrokes(div.id, "Meta+s")
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Action: save",
        ]
      `)

      // Cmd+Shift+P
      testRoot.renderer.nativeSimulateKeystrokes(div.id, "Shift+Meta+p")
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Action: command-palette",
        ]
      `)

      // Escape (no modifiers)
      testRoot.renderer.nativeSimulateKeystrokes(div.id, "Escape")
      expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
        [
          "Action: cancel",
        ]
      `)
    })
  })

  describe("screenshot", () => {
    it("should capture screenshot and reflect visual state changes", () => {
      function ScreenshotProbe() {
        const [active, setActive] = useState(false)
        return (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              width: "100%",
              height: "100%",
              backgroundColor: "#0f111a",
            }}
          >
            <div
              style={{
                width: 280,
                height: 120,
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
                justifyContent: "center",
                gap: 10,
                borderRadius: 16,
                backgroundColor: active ? "#f5f7ff" : "#1f2333",
              }}
              onClick={() => setActive((v) => !v)}
            >
              <text style={{ color: active ? "#1f2333" : "#cbd5ff", fontSize: 18 }}>
                {active ? "active" : "idle"}
              </text>
              <text style={{ color: active ? "#525b76" : "#7f8bb3", fontSize: 12 }}>
                click to toggle theme
              </text>
            </div>
          </div>
        )
      }

      testRoot.render(<ScreenshotProbe />)

      // Capture initial state
      const path0 = `${SHOTS_DIR}/gpuix-counter-0.png`
      const path1 = `${SHOTS_DIR}/gpuix-counter-1.png`

      // Clean up from previous runs
      if (fs.existsSync(path0)) fs.unlinkSync(path0)
      if (fs.existsSync(path1)) fs.unlinkSync(path1)

      testRoot.renderer.captureScreenshot(path0)

      // Click and capture again
      testRoot.renderer.nativeSimulateClick(640, 400)
      testRoot.renderer.captureScreenshot(path1)

      expectScreenshotsDiffer(path0, path1)
    })

    it("should capture screenshot changes for keyDown interactions", () => {
      function KeydownScreenshotProbe() {
        const [state, setState] = useState("idle")
        return (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              width: "100%",
              height: "100%",
              backgroundColor: "#10131d",
            }}
          >
            <div
              style={{
                width: 320,
                height: 120,
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
                justifyContent: "center",
                gap: 8,
                borderRadius: 16,
                backgroundColor: state === "idle" ? "#2b324d" : "#1f5a45",
              }}
              tabIndex={0}
              onKeyDown={(e: EventPayload) => {
                if (e.key === "Enter") setState("enter")
              }}
            >
              <text style={{ color: "#e8edff", fontSize: 18 }}>{`State: ${state}`}</text>
              <text style={{ color: "#a8b2d8", fontSize: 12 }}>press Enter to switch</text>
            </div>
          </div>
        )
      }

      testRoot.render(<KeydownScreenshotProbe />)

      const keyTarget = testRoot.renderer
        .findByType("div")
        .find((d) => d.events.has("keydown"))!

      const path0 = `${SHOTS_DIR}/gpuix-keydown-0.png`
      const path1 = `${SHOTS_DIR}/gpuix-keydown-1.png`

      if (fs.existsSync(path0)) fs.unlinkSync(path0)
      if (fs.existsSync(path1)) fs.unlinkSync(path1)

      testRoot.renderer.captureScreenshot(path0)
      testRoot.renderer.nativeSimulateKeyDown(keyTarget.id, "Enter")
      testRoot.renderer.captureScreenshot(path1)

      expectScreenshotsDiffer(path0, path1)
    })

    it("should capture screenshot changes for hover interactions", () => {
      function HoverScreenshotProbe() {
        const [hovered, setHovered] = useState(false)
        return (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              width: "100%",
              height: "100%",
              backgroundColor: "#0f1319",
            }}
          >
            <div
              style={{
                width: 300,
                height: 130,
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
                justifyContent: "center",
                gap: 8,
                borderRadius: 20,
                backgroundColor: hovered ? "#f6d48b" : "#2f3347",
              }}
              onMouseEnter={() => setHovered(true)}
              onMouseLeave={() => setHovered(false)}
            >
              <text style={{ color: hovered ? "#46361e" : "#d5daf2", fontSize: 18 }}>
                {hovered ? "hovered" : "not-hovered"}
              </text>
              <text style={{ color: hovered ? "#7a5e2c" : "#9da6c8", fontSize: 12 }}>
                move cursor over card
              </text>
            </div>
          </div>
        )
      }

      testRoot.render(<HoverScreenshotProbe />)

      const path0 = `${SHOTS_DIR}/gpuix-hover-0.png`
      const path1 = `${SHOTS_DIR}/gpuix-hover-1.png`

      if (fs.existsSync(path0)) fs.unlinkSync(path0)
      if (fs.existsSync(path1)) fs.unlinkSync(path1)

      testRoot.renderer.captureScreenshot(path0)
      testRoot.renderer.nativeSimulateMouseMove(640, 400)
      testRoot.renderer.captureScreenshot(path1)

      expectScreenshotsDiffer(path0, path1)
    })

  })

  describe("tree structure", () => {
    it("should produce correct element tree", () => {
      function App() {
        return (
          <div style={{ display: "flex", gap: 8 }}>
            <text>Hello</text>
            <div onClick={() => {}}>
              <text>Click me</text>
            </div>
          </div>
        )
      }

      testRoot.render(<App />)
      expect(testRoot.renderer.toJSON()).toMatchInlineSnapshot(`
        {
          "children": [
            {
              "children": [
                {
                  "id": 1,
                  "text": "Hello",
                  "type": "text",
                },
              ],
              "id": 2,
              "type": "text",
            },
            {
              "children": [
                {
                  "children": [
                    {
                      "id": 3,
                      "text": "Click me",
                      "type": "text",
                    },
                  ],
                  "id": 4,
                  "type": "text",
                },
              ],
              "events": [
                "click",
              ],
              "id": 5,
              "type": "div",
            },
          ],
          "id": 6,
          "style": {
            "display": "flex",
            "gap": 8,
          },
          "type": "div",
        }
      `)
    })
  })

  describe("scrollable containers", () => {
    it("should scroll content when overflow is scroll", () => {
      function ScrollableList() {
        return (
          <div style={{ width: 300, height: 200, overflow: "scroll" }}>
            <div style={{ height: 100, backgroundColor: "#ff0000" }}>
              <text>Item 1</text>
            </div>
            <div style={{ height: 100, backgroundColor: "#00ff00" }}>
              <text>Item 2</text>
            </div>
            <div style={{ height: 100, backgroundColor: "#0000ff" }}>
              <text>Item 3</text>
            </div>
            <div style={{ height: 100, backgroundColor: "#ffff00" }}>
              <text>Item 4</text>
            </div>
            <div style={{ height: 100, backgroundColor: "#ff00ff" }}>
              <text>Item 5</text>
            </div>
          </div>
        )
      }

      testRoot.render(<ScrollableList />)

      // Find the scrollable container (the one with overflow: scroll)
      const scrollContainer = testRoot.renderer
        .findByType("div")
        .find((d) => d.style.overflow === "scroll")!
      expect(scrollContainer).toBeDefined()

      // Initially scroll offset should be 0,0
      const initialOffset = testRoot.renderer.getScrollOffset(scrollContainer.id)
      expect(initialOffset).toEqual([0, 0])

      // Simulate scrolling down 50px (positive deltaY = scroll down)
      testRoot.renderer.nativeSimulateScrollWheel(150, 100, 0, 50)

      // scrollTop grows as we scroll down
      const afterScrollOffset = testRoot.renderer.getScrollOffset(scrollContainer.id)
      expect(afterScrollOffset).not.toBeNull()
      expect(afterScrollOffset![1]).toBeGreaterThan(0) // scrolled down
    })

    it("does not remap a vertical wheel onto overflow-x", () => {
      function NestedAxisScroll() {
        return (
          <div style={{ width: 240, height: 120, overflowY: "scroll" }}>
            <div style={{ width: 240, height: 80, overflowX: "scroll" }}>
              <div style={{ width: 800, height: 80 }}>
                <text>wide row</text>
              </div>
            </div>
            <div style={{ height: 400 }}>
              <text>below</text>
            </div>
          </div>
        )
      }

      testRoot.render(<NestedAxisScroll />)

      const parent = testRoot.renderer
        .findByType("div")
        .find((d) => d.style.overflowY === "scroll")!
      const inner = testRoot.renderer
        .findByType("div")
        .find((d) => d.style.overflowX === "scroll")!

      expect(testRoot.renderer.getScrollOffset(parent.id)).toEqual([0, 0])
      expect(testRoot.renderer.getScrollOffset(inner.id)).toEqual([0, 0])

      testRoot.renderer.nativeSimulateScrollWheel(80, 40, 0, 60)

      const parentOffset = testRoot.renderer.getScrollOffset(parent.id)
      const innerOffset = testRoot.renderer.getScrollOffset(inner.id)
      expect(parentOffset).not.toBeNull()
      expect(parentOffset![1]).toBeGreaterThan(0)
      expect(innerOffset).toEqual([0, 0])
    })

    it("pans overflow-x when the child is wider than the viewport", () => {
      function WideRow() {
        return (
          <div style={{ width: 240, height: 80 }}>
            <div style={{ width: "100%", height: 80, overflowX: "scroll" }}>
              <div style={{ width: 800, height: 80, flexShrink: 0 }}>
                <text>wide row</text>
              </div>
            </div>
          </div>
        )
      }

      testRoot.render(<WideRow />)

      const scroller = testRoot.renderer
        .findByType("div")
        .find((d) => d.style.overflowX === "scroll")!
      expect(testRoot.renderer.getScrollOffset(scroller.id)).toEqual([0, 0])

      testRoot.renderer.nativeSimulateScrollWheel(80, 40, 80, 0)
      const offset = testRoot.renderer.getScrollOffset(scroller.id)
      expect(offset).not.toBeNull()
      expect(offset![0]).toBeGreaterThan(0)
    })

    it("lets a parent scroller take a vertical wheel over a filled child", () => {
      function FilledColumn() {
        return (
          <div style={{ width: 240, height: 120, overflowY: "scroll" }}>
            <div style={{ height: 80, width: "100%", backgroundColor: "#1e1e2e" }}>
              <text>card</text>
            </div>
            <div style={{ height: 400 }}>
              <text>below</text>
            </div>
          </div>
        )
      }

      testRoot.render(<FilledColumn />)
      const parent = testRoot.renderer
        .findByType("div")
        .find((d) => d.style.overflowY === "scroll")!
      testRoot.renderer.nativeSimulateScrollWheel(80, 40, 0, 80)
      const offset = testRoot.renderer.getScrollOffset(parent.id)
      expect(offset).not.toBeNull()
      expect(offset![1]).toBeGreaterThan(0)
    })

    it("lets an ancestor take the wheel over an absolutely placed child", () => {
      // A pannable canvas places every item absolutely: a timeline clip, a
      // graph node. If absolute stole the wheel, the pan listener never ran.
      const deltas: number[] = []

      function Canvas() {
        return (
          <div
            style={{ width: 320, height: 200, position: "relative" }}
            onWheel={(e: EventPayload) => deltas.push(e.deltaY ?? 0)}
          >
            <div
              style={{
                position: "absolute",
                left: 40,
                top: 40,
                width: 120,
                height: 40,
                backgroundColor: "#3366ff",
              }}
            >
              <text>clip</text>
            </div>
          </div>
        )
      }

      testRoot.render(<Canvas />)
      testRoot.renderer.nativeSimulateScrollWheel(100, 60, 0, 80)

      expect(deltas).toEqual([80])
    })

    it("does not pass the wheel to a scroller behind an unrelated overlay", () => {
      // The wheel goes to the target's scrolling ancestors, as in the DOM. The
      // overlay is the target and the scroller is not its ancestor. An overlay
      // that should let the wheel through sets `pointerEvents: "none"`.
      function OverlayOverScroller({ passThrough }: { passThrough: boolean }) {
        return (
          <div style={{ width: 320, height: 200, position: "relative" }}>
            <div style={{ width: 320, height: 200, overflowY: "scroll" }}>
              <div style={{ height: 900, backgroundColor: "#1e1e2e" }}>
                <text>tall</text>
              </div>
            </div>
            <div
              style={{
                position: "absolute",
                left: 0,
                top: 0,
                width: 320,
                height: 200,
                backgroundColor: "#101010",
                pointerEvents: passThrough ? "none" : undefined,
              }}
            >
              <text>card</text>
            </div>
          </div>
        )
      }

      testRoot.render(<OverlayOverScroller passThrough={false} />)
      const scroller = testRoot.renderer
        .findByType("div")
        .find((d) => d.style.overflowY === "scroll")!
      testRoot.renderer.nativeSimulateScrollWheel(160, 100, 0, 80)
      expect(testRoot.renderer.getScrollOffset(scroller.id)![1]).toBe(0)

      testRoot.render(<OverlayOverScroller passThrough />)
      testRoot.renderer.nativeSimulateScrollWheel(160, 100, 0, 80)
      expect(testRoot.renderer.getScrollOffset(scroller.id)![1]).toBeGreaterThan(0)
    })

    it("does not stop the wheel at a child with pointerEvents auto", () => {
      const deltas: number[] = []

      function ModalOverCanvas() {
        return (
          <div
            style={{ width: 320, height: 200, position: "relative" }}
            onWheel={(e: EventPayload) => deltas.push(e.deltaY ?? 0)}
          >
            <div
              style={{
                position: "absolute",
                left: 0,
                top: 0,
                width: 320,
                height: 200,
                backgroundColor: "#101010",
                pointerEvents: "auto",
              }}
            >
              <text>modal</text>
            </div>
          </div>
        )
      }

      testRoot.render(<ModalOverCanvas />)
      testRoot.renderer.nativeSimulateScrollWheel(100, 60, 0, 80)

      // The DOM sign: content moving up is a positive deltaY.
      expect(deltas).toEqual([80])
    })

    it("reports the modifiers held during a simulated wheel", () => {
      const held: Array<boolean | undefined> = []

      function ZoomSurface() {
        return (
          <div
            style={{ width: 200, height: 200, backgroundColor: "#101010" }}
            onWheel={(e: EventPayload) => held.push(e.modifiers?.cmd)}
          >
            <text>surface</text>
          </div>
        )
      }

      testRoot.render(<ZoomSurface />)
      testRoot.renderer.nativeSimulateScrollWheel(100, 100, 0, 40)
      testRoot.renderer.nativeSimulateScrollWheel(100, 100, 0, 40, "cmd")

      expect(held).toEqual([false, true])
    })

    it("should support overflow-y scroll only", () => {
      function VerticalScroll() {
        return (
          <div style={{ width: 300, height: 100, overflowY: "scroll" }}>
            <div style={{ height: 500 }}>
              <text>Tall content</text>
            </div>
          </div>
        )
      }

      testRoot.render(<VerticalScroll />)

      const container = testRoot.renderer
        .findByType("div")
        .find((d) => d.style.overflowY === "scroll")!
      expect(container).toBeDefined()

      const initialOffset = testRoot.renderer.getScrollOffset(container.id)
      expect(initialOffset).toEqual([0, 0])

      // Scroll down
      testRoot.renderer.nativeSimulateScrollWheel(150, 50, 0, 80)
      const offset = testRoot.renderer.getScrollOffset(container.id)
      expect(offset).not.toBeNull()
      expect(offset![1]).toBeGreaterThan(0) // scrolled down vertically
    })

    it("should support programmatic scrollTo", () => {
      function ScrollableBox() {
        return (
          <div style={{ width: 200, height: 100, overflow: "scroll" }}>
            <div style={{ height: 500 }}>
              <text>Very tall content</text>
            </div>
          </div>
        )
      }

      testRoot.render(<ScrollableBox />)

      const container = testRoot.renderer
        .findByType("div")
        .find((d) => d.style.overflow === "scroll")!

      // Initially at 0,0
      expect(testRoot.renderer.getScrollOffset(container.id)).toEqual([0, 0])

      // Scroll programmatically to scrollTop 100
      testRoot.renderer.scrollTo(container.id, 0, 100)

      const offset = testRoot.renderer.getScrollOffset(container.id)
      expect(offset).not.toBeNull()
      expect(offset![1]).toBe(100)
    })

    it("should support programmatic scrollToItem", () => {
      function ItemList() {
        return (
          <div style={{ width: 200, height: 100, overflow: "scroll" }}>
            <div style={{ height: 80 }}>
              <text>Item A</text>
            </div>
            <div style={{ height: 80 }}>
              <text>Item B</text>
            </div>
            <div style={{ height: 80 }}>
              <text>Item C</text>
            </div>
            <div style={{ height: 80 }}>
              <text>Item D</text>
            </div>
          </div>
        )
      }

      testRoot.render(<ItemList />)

      const container = testRoot.renderer
        .findByType("div")
        .find((d) => d.style.overflow === "scroll")!

      // Initially at top
      expect(testRoot.renderer.getScrollOffset(container.id)).toEqual([0, 0])

      // Scroll to item 3 (index 3, the 4th child "Item D")
      testRoot.renderer.scrollToItem(container.id, 3)

      // After scrolling to item, offset should have changed
      const offset = testRoot.renderer.getScrollOffset(container.id)
      expect(offset).not.toBeNull()
      expect(offset![1]).toBeGreaterThan(0) // scrolled down to reveal item
    })

    it("should render scrollable container with visible screenshot diff", () => {
      function ScreenshotScroller() {
        return (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              width: "100%",
              height: "100%",
              backgroundColor: "#1a1a2e",
            }}
          >
            <div
              style={{
                width: 300,
                height: 200,
                overflow: "scroll",
                backgroundColor: "#16213e",
                borderRadius: 8,
                borderWidth: 2,
                borderStyle: "solid",
                borderColor: "#0f3460",
              }}
            >
              <div style={{ height: 80, backgroundColor: "#e94560", padding: 16 }}>
                <text style={{ color: "#ffffff", fontSize: 20 }}>Section 1 (Red)</text>
              </div>
              <div style={{ height: 80, backgroundColor: "#0f3460", padding: 16 }}>
                <text style={{ color: "#ffffff", fontSize: 20 }}>Section 2 (Blue)</text>
              </div>
              <div style={{ height: 80, backgroundColor: "#533483", padding: 16 }}>
                <text style={{ color: "#ffffff", fontSize: 20 }}>Section 3 (Purple)</text>
              </div>
              <div style={{ height: 80, backgroundColor: "#e94560", padding: 16 }}>
                <text style={{ color: "#ffffff", fontSize: 20 }}>Section 4 (Red)</text>
              </div>
              <div style={{ height: 80, backgroundColor: "#0f3460", padding: 16 }}>
                <text style={{ color: "#ffffff", fontSize: 20 }}>Section 5 (Blue)</text>
              </div>
            </div>
          </div>
        )
      }

      testRoot.render(<ScreenshotScroller />)

      const path0 = `${SHOTS_DIR}/gpuix-scroll-before.png`
      const path1 = `${SHOTS_DIR}/gpuix-scroll-after.png`

      if (fs.existsSync(path0)) fs.unlinkSync(path0)
      if (fs.existsSync(path1)) fs.unlinkSync(path1)

      // Screenshot before scrolling
      testRoot.renderer.captureScreenshot(path0)

      // Scroll down 150px inside the scrollable container
      testRoot.renderer.nativeSimulateScrollWheel(640, 400, 0, 150)

      // Screenshot after scrolling
      testRoot.renderer.captureScreenshot(path1)

      // Both screenshots should exist and be different (content shifted)
      expect(fs.existsSync(path0)).toBe(true)
      expect(fs.existsSync(path1)).toBe(true)
      expect(fs.statSync(path0).size).toBeGreaterThan(0)
      expect(fs.statSync(path1).size).toBeGreaterThan(0)
      // Before and after scroll should produce different pixels
      expect(fs.readFileSync(path0).equals(fs.readFileSync(path1))).toBe(false)
    })

    it("should combine onWheel with overflow scroll", () => {
      const receivedScrollEvents: EventPayload[] = []

      function ScrollWithEvent() {
        return (
          <div
            style={{ width: 300, height: 100, overflow: "scroll" }}
            onWheel={(e: EventPayload) => receivedScrollEvents.push(e)}
          >
            <div style={{ height: 500 }}>
              <text>Scrollable with events</text>
            </div>
          </div>
        )
      }

      testRoot.render(<ScrollWithEvent />)

      // The wheel should fire onWheel AND move the content
      testRoot.renderer.nativeSimulateScrollWheel(150, 50, 0, 40)

      // Event should have fired
      expect(receivedScrollEvents.length).toBeGreaterThanOrEqual(1)
      const scrollEvent = receivedScrollEvents.find(
        (e) => e.eventType === "wheel"
      )
      expect(scrollEvent).toBeDefined()
      expect(scrollEvent!.deltaY).toBe(40)

      // Content should have scrolled
      const container = testRoot.renderer
        .findByType("div")
        .find((d) => d.style.overflow === "scroll")!
      const offset = testRoot.renderer.getScrollOffset(container.id)
      expect(offset).not.toBeNull()
      expect(offset![1]).toBeGreaterThan(0)
    })

    it("should expose element id via ref for programmatic scroll", () => {
      let capturedRef: any = null

      function RefScroller() {
        const scrollRef = useRef<any>(null)
        // Capture the ref after render so the test can access it
        capturedRef = scrollRef

        return (
          <div
            ref={scrollRef}
            style={{ width: 200, height: 100, overflow: "scroll" }}
          >
            <div style={{ height: 80 }}>
              <text>Item A</text>
            </div>
            <div style={{ height: 80 }}>
              <text>Item B</text>
            </div>
            <div style={{ height: 80 }}>
              <text>Item C</text>
            </div>
          </div>
        )
      }

      testRoot.render(<RefScroller />)

      // ref.current should be the Instance with a numeric id
      expect(capturedRef).not.toBeNull()
      expect(capturedRef.current).not.toBeNull()
      expect(typeof capturedRef.current.id).toBe("number")
      expect(capturedRef.current.id).toBeGreaterThan(0)

      // Use the ref's id with the scroll API
      const elementId = capturedRef.current.id

      // Initially at 0,0
      expect(testRoot.renderer.getScrollOffset(elementId)).toEqual([0, 0])

      // Programmatic scroll via ref id
      testRoot.renderer.scrollTo(elementId, 0, 60)
      const offset = testRoot.renderer.getScrollOffset(elementId)
      expect(offset).not.toBeNull()
      expect(offset![1]).toBe(60)
    })

    it("scrolls a child into view from the host ref", () => {
      let childRef: { id: number; scrollIntoView: () => void } | null = null

      function Scroller() {
        return (
          <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }}>
            <div style={{ height: 80 }}>
              <text>Item A</text>
            </div>
            <div style={{ height: 80 }}>
              <text>Item B</text>
            </div>
            <div
              ref={(instance) => {
                childRef = instance as { id: number; scrollIntoView: () => void } | null
              }}
              testId="item-d"
              style={{ height: 80 }}
            >
              <text>Item D</text>
            </div>
          </div>
        )
      }

      testRoot.render(<Scroller />)

      const scroller = testRoot.renderer.findByTestId("scroller")!
      expect(testRoot.renderer.getScrollOffset(scroller.id)).toEqual([0, 0])
      expect(childRef).not.toBeNull()
      expect(typeof childRef!.scrollIntoView).toBe("function")

      childRef!.scrollIntoView?.()
      const offset = testRoot.renderer.getScrollOffset(scroller.id)
      expect(offset).not.toBeNull()
      expect(offset![1]).toBeGreaterThan(0)
    })
  })
})
