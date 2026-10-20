/**
 * Every renderer method a handler can call, called from a pointerdown and a keydown handler. None
 * throws, and a motion value written in the handler reads back as written in the same handler and
 * after it.
 */
// @ts-nocheck

import fs from "fs"
import { describe, expect, it } from "vitest"
import { defineKeyframes } from "../index"
import { createTestRoot, hasNativeTestRenderer, TestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

defineKeyframes("handler-calls-grow", { from: { width: 20 }, to: { width: 50 } })

const SQUARE_FONT = fs.readFileSync(new URL("./fixtures/gpuix-test-square.ttf", import.meta.url))
const PNG_1X1 = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  "base64"
)

/** Methods that start another input or drive the test app itself, which cannot nest in a handler. */
const NOT_CALLABLE_FROM_A_HANDLER = new Set([
  "advanceTime",
  "captureScreenshot",
  "dispose",
  "dispatchEvent",
  "dispatchMouseMove",
  "dispatchNativeEvents",
  "dispatchScrollWheel",
  "dispatchSyncEvent",
  "dragSelect",
  "promptForPaths",
  "setInlineDraws",
  "setWindowSize",
  "simulateComposition",
  "simulateKeystrokes",
  "nativeSimulateClick",
  "nativeSimulateFileDragEnd",
  "nativeSimulateFileDragEnter",
  "nativeSimulateFileDragExit",
  "nativeSimulateFileDragMove",
  "nativeSimulateFileDragSubmit",
  "nativeSimulateFileDrop",
  "nativeSimulateKeyDown",
  "nativeSimulateKeyUp",
  "nativeSimulateKeystrokes",
  "nativeSimulateMouseDown",
  "nativeSimulateMouseExit",
  "nativeSimulateMouseMove",
  "nativeSimulateMouseUp",
  "nativeSimulateScrollWheel",
])

function calls(r, ids) {
  return {
    animateMotionValue: () => r.animateMotionValue(910, 5),
    applyBatch: () => r.applyBatch("[]"),
    blurElement: () => r.blurElement(ids.field),
    buildElementMap: () => r.buildElementMap?.(),
    cancelDrag: () => r.cancelDrag(),
    clearSelection: () => r.clearSelection(),
    clickElement: () => r.clickElement(ids.box),
    clockFastForward: () => r.clockFastForward(5),
    clockPause: () => r.clockPause(),
    clockResume: () => r.clockResume(),
    clockSet: () => r.clockSet(1000),
    createMotionFollower: () => r.createMotionFollower(912, 910, {}),
    createMotionSpring: () => r.createMotionSpring(913, 0, {}),
    createMotionTransform: () => r.createMotionTransform(914, 910, [0, 10], [0, 100]),
    createMotionValue: () => r.createMotionValue(915, 1),
    cycleDebugFrameOverlay: () => r.cycleDebugFrameOverlay(),
    disposeMotionValue: () => r.disposeMotionValue(913),
    drainEvents: () => r.drainEvents(),
    drawFrame: () => r.drawFrame(),
    exitPointerLock: () => r.exitPointerLock(),
    failStyleImage: () => r.failStyleImage("blob:handler-calls-failed", "revoked"),
    findByTestId: () => r.findByTestId("box"),
    findByText: () => r.findByText("hi"),
    findByType: () => r.findByType("div"),
    flush: () => r.flush(),
    focusElement: () => r.focusElement(ids.field),
    focusNext: () => r.focusNext(),
    focusNextWithin: () => r.focusNextWithin(ids.form),
    focusPrevious: () => r.focusPrevious(),
    focusPreviousWithin: () => r.focusPreviousWithin(ids.form),
    getA11yTree: () => r.getA11yTree(),
    getAllText: () => r.getAllText(),
    getAnimation: () => r.getAnimation(r.getAnimations(ids.box)[0].id),
    getAnimations: () => r.getAnimations(ids.box),
    getAppearance: () => r.getAppearance(),
    getAutomationTree: () => r.getAutomationTree(),
    getBuildCounters: () => r.getBuildCounters(),
    getCulledSubtreeCount: () => r.getCulledSubtreeCount(),
    getCursor: () => r.getCursor(),
    getDebugFrameOverlay: () => r.getDebugFrameOverlay(),
    getDebugFrameOverlayStats: () => r.getDebugFrameOverlayStats(),
    getElement: () => r.getElement(ids.box),
    getElementBounds: () => r.getElementBounds(ids.box),
    getElementsAt: () => r.getElementsAt(5, 5),
    getFocusedElementId: () => r.getFocusedElementId(),
    getLayoutCounters: () => r.getLayoutCounters(),
    getRetainedCounters: () => r.getRetainedCounters(),
    getFrameWorkCounters: () => r.getFrameWorkCounters(),
    getRetentionMismatches: () => r.getRetentionMismatches(),
    getLayoutPassCount: () => r.getLayoutPassCount(),
    getListScrollTop: () => r.getListScrollTop(ids.scroller),
    getMediaFeatures: () => r.getMediaFeatures(),
    getMotionValue: () => r.getMotionValue(910),
    getMotionValueVelocity: () => r.getMotionValueVelocity(910),
    getPaintedFrameCount: () => r.getPaintedFrameCount(),
    getPaintedHighlights: () => r.getPaintedHighlights(),
    getPaintedText: () => r.getPaintedText(),
    getRetainMismatches: () => r.getRetainMismatches(),
    getRetainedElementCount: () => r.getRetainedElementCount(),
    getRoot: () => r.getRoot(),
    getScrollMetrics: () => r.getScrollMetrics(ids.scroller),
    getScrollOffset: () => r.getScrollOffset(ids.scroller),
    getSelectedText: () => r.getSelectedText(),
    getSvgMarkup: () => r.getSvgMarkup(ids.box),
    getSyntaxCacheStats: () => r.getSyntaxCacheStats(),
    getTextSelection: () => r.getTextSelection(ids.field),
    getWindowSize: () => r.getWindowSize(),
    getWindowTitle: () => r.getWindowTitle(),
    hasPointerCapture: () => r.hasPointerCapture(ids.box, 1),
    isMotionValueAnimating: () => r.isMotionValueAnimating(910),
    jumpMotionValue: () => r.jumpMotionValue(910, 2),
    pauseAnimation: () => r.pauseAnimation(r.getAnimations(ids.box)[0].id),
    playAnimation: () => r.playAnimation(r.getAnimations(ids.box)[0].id),
    matchesFocus: () => r.matchesFocus(ids.field, ":focus"),
    pointerLockElement: () => r.pointerLockElement(),
    readClipboardText: () => r.readClipboardText(),
    registerFont: () => r.registerFont(SQUARE_FONT),
    releaseAnimation: () => r.releaseAnimation(r.getAnimations(ids.box)[0].id),
    releasePointerCapture: () => r.releasePointerCapture(ids.box, 1),
    requestPointerLock: () => r.requestPointerLock(ids.box),
    requestSubmit: () => r.requestSubmit(ids.form),
    resetBuildCounters: () => r.resetBuildCounters(),
    resetDebugFrameOverlayStats: () => r.resetDebugFrameOverlayStats(),
    scrollByKey: () => r.scrollByKey("down", false),
    scrollIntoView: () => r.scrollIntoView(ids.box),
    scrollTo: () => r.scrollTo(ids.scroller, 0, 10),
    scrollToItem: () => r.scrollToItem(ids.scroller, 0),
    selectText: () => r.selectText(ids.field),
    setA11yActive: () => r.setA11yActive(true),
    setAnimationCurrentTime: () => r.setAnimationCurrentTime(r.getAnimations(ids.box)[0].id, 10),
    setAnimationPlaybackRate: () => r.setAnimationPlaybackRate(r.getAnimations(ids.box)[0].id, 2),
    setAppearance: () => r.setAppearance("dark"),
    setDebugFrameOverlay: () => r.setDebugFrameOverlay("hidden"),
    setImage: () => r.setImage(ids.img, PNG_1X1),
    setImagePixels: () => r.setImagePixels(ids.img, 1, 1, Buffer.from([255, 0, 0, 255])),
    setMediaFeatures: () => r.setMediaFeatures({}),
    setMotionValue: () => r.setMotionValue(910, 3),
    setMotionValueListener: () => r.setMotionValueListener(910, true, true),
    setPointerCapture: () => r.setPointerCapture(ids.box, 1),
    setRetainLift: () => r.setRetainLift(["svg"]),
    setRetainPlanning: () => r.setRetainPlanning(true),
    setRetainEnabled: () => r.setRetainEnabled(true),
    setRetainVerify: () => r.setRetainVerify(false),
    setScrollbarStyle: () => r.setScrollbarStyle("overlay"),
    setSelectionRange: () => r.setSelectionRange(ids.field, 0, 1),
    setStyleImage: () => r.setStyleImage("blob:handler-calls", PNG_1X1),
    setWindowActive: () => r.setWindowActive(true),
    setWindowAppearanceChange: () => r.setWindowAppearanceChange(false, 1),
    setWindowEventListeners: () => r.setWindowEventListeners(["keydown"]),
    setWindowKeyEvents: () => r.setWindowKeyEvents(true, true, 1),
    setWindowMediaChange: () => r.setWindowMediaChange(false),
    setWindowSelectionChange: () => r.setWindowSelectionChange(false, 1),
    simulateLostMouseUp: () => r.simulateLostMouseUp(),
    simulatePointerLockGranting: () => r.simulatePointerLockGranting(true),
    simulatePointerLockLost: () => r.simulatePointerLockLost(),
    stopMotionValue: () => r.stopMotionValue(910),
    takeSimulatedWindowActions: () => r.takeSimulatedWindowActions(),
    toJSON: () => r.toJSON(),
    writeClipboardText: () => r.writeClipboardText("x"),
  }
}

function publicMethods(renderer) {
  const names = new Set()
  for (let o = renderer; o && o !== Object.prototype; o = Object.getPrototypeOf(o)) {
    for (const name of Object.getOwnPropertyNames(o)) {
      if (name !== "constructor" && typeof renderer[name] === "function") names.add(name)
    }
  }
  return [...names]
}

describeNative("renderer calls from a sync handler", () => {
  for (const on of ["pointerdown", "keydown"]) {
    it(`none of them throws inside a ${on} handler`, () => {
      const { render, renderer, unmount } = createTestRoot()
      try {
        const failures: string[] = []
        const ran: string[] = []
        const run = () => {
          const ids = {
            field: renderer.findByTestId("field").id,
            box: renderer.findByTestId("box").id,
            scroller: renderer.findByTestId("scroller").id,
            form: renderer.findByTestId("form").id,
            img: renderer.findByTestId("img").id,
          }
          renderer.createMotionValue(910, 1)
          for (const [name, call] of Object.entries(calls(renderer, ids))) {
            try {
              call()
              ran.push(name)
            } catch (error) {
              failures.push(`${name}: ${error.message}`)
            }
          }
        }
        render(
          <div
            style={{ width: 400, height: 300 }}
            onPointerDown={on === "pointerdown" ? run : undefined}
          >
            <form testId="form">
              <input testId="field" defaultValue="hi" style={{ width: 100, height: 20 }} onKeyDown={on === "keydown" ? run : undefined} />
            </form>
            <img testId="img" style={{ width: 10, height: 10 }} />
            <div testId="box" style={{ width: 50, height: 20, animation: "handler-calls-grow 1s linear forwards" }} />
            <div testId="scroller" style={{ width: 100, height: 50, overflow: "scroll" }}>
              <div style={{ height: 400 }} />
            </div>
          </div>
        )
        if (on === "pointerdown") renderer.nativeSimulateClick(5, 5)
        else {
          renderer.focusElement(renderer.findByTestId("field").id)
          renderer.simulateKeystrokes("a")
        }
        expect(failures).toEqual([])
        expect(ran.length).toBeGreaterThan(80)
      } finally {
        unmount()
      }
    })
  }

  it("covers every public method or says why it cannot be called from a handler", () => {
    const { renderer, unmount } = createTestRoot()
    try {
      const covered = new Set([
        ...Object.keys(calls(renderer, {})),
        ...NOT_CALLABLE_FROM_A_HANDLER,
      ])
      expect(publicMethods(renderer).filter((name) => !covered.has(name))).toEqual([])
    } finally {
      unmount()
    }
  })

  describe("motion values", () => {
    it("a value written in a pointerdown handler reads back as written there and after", () => {
      const { render, renderer, unmount } = createTestRoot()
      try {
        const inside: unknown[] = []
        let stopped = 0
        renderer.createMotionValue(920, 0)
        render(
          <div style={{ width: 100, height: 100 }} onPointerDown={() => {
            renderer.setMotionValue(920, 4)
            inside.push(renderer.getMotionValue(920))
            renderer.jumpMotionValue(920, 8)
            inside.push(renderer.getMotionValue(920))
            renderer.animateMotionValue(920, 20, { type: "tween", duration: 1 })
            inside.push(renderer.isMotionValueAnimating(920))
            renderer.stopMotionValue(920)
            inside.push(renderer.isMotionValueAnimating(920))
            stopped = renderer.getMotionValue(920)
            renderer.createMotionValue(921, 1)
            renderer.setMotionValueListener(921, true, false)
            renderer.setMotionValue(921, 2)
            inside.push(renderer.getMotionValue(921))
            renderer.disposeMotionValue(921)
          }} />
        )
        renderer.nativeSimulateClick(5, 5)
        expect(inside).toEqual([4, 8, true, false, 2])
        expect(stopped).toBeCloseTo(8, 0)
        expect(renderer.getMotionValue(920)).toBe(stopped)
        expect(() => renderer.getMotionValue(921)).toThrow()
      } finally {
        unmount()
      }
    })
  })
})
