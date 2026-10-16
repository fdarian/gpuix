/// GPUIX TestRenderer — thin wrapper over the native TestGpuixRenderer.
///
/// All state lives in Rust's RetainedTree. All mutations go directly to
/// the native renderer via napi. Inspection methods (findByType, getAllText,
/// toJSON, etc.) query the Rust tree via napi — no JS-side shadow copy.
///
/// All event simulation goes through the native GPUI pipeline (coordinate-based
/// hit testing, GPUI dispatch, emit_event_full). The nativeSimulate* methods
/// flush the tree, dispatch through GPUI, drain events, and feed them into
/// the injected framework event dispatcher.

import {
  hasTestGpuixRenderer,
  TestGpuixRenderer,
  validateKeyframes,
  type EventPayload,
  type MediaFeatures,
} from "../index.js"
import type {
  DebugFrameOverlayMode,
  DebugFrameOverlayStats,
  ElementBounds,
  HighlightMatch,
  ImagePixelFormat,
  MotionValueHost,
  NativeRenderer,
  PathPromptOptions,
  WindowKeyEventHandlers,
} from "./host.js"
import { bindClipboard } from "./clipboard.js"
import { forgetActiveElementSource } from "./events/window-target.js"
import { matchableText } from "./text-match.js"
import { flushPendingMutations } from "./bounds.js"
import { flushObservations, settleObservations } from "./observers.js"
import { readFontFiles } from "./fonts.js"
import {
  registerKeyframesHost,
  registerKeyframesValidator,
  type KeyframesHost,
} from "./keyframes.js"
import { createRendererState, reportSyncErrors, type SyncDispatchResult } from "./renderer-state.js"
export {
  applyMacCpuThrottleFromEnv,
  MAC_CPU_THROTTLES,
  readMacCpuThrottle,
} from "./cpu-throttle.js"
export type { MacCpuThrottle } from "./cpu-throttle.js"

interface NativeTestRendererApi
  extends Omit<NativeRenderer, keyof MotionValueHost>,
    MotionValueHost,
    KeyframesHost {
  promptForPaths(options?: PathPromptOptions): Promise<string[] | null>
  writeClipboardText(text: string): void
  readClipboardText(): string | null
  flush(): void
  drainEvents(): EventPayload[]
  simulateKeystrokes(keystrokes: string): void
  setSyncEventHandler(handler: ((event: EventPayload) => SyncDispatchResult) | null): void
  dispose(): void
  focusElement(elementId: number): void
  blurElement(elementId: number): void
  matchesFocus(elementId: number, selector: string): boolean
  focusNext(): void
  focusPrevious(): void
  getFocusedElementId(): number | null
  setWindowActive(active: boolean): void
  focusNextWithin(elementId: number): void
  focusPreviousWithin(elementId: number): void
  setWindowKeyEvents(keyDown: boolean, keyUp: boolean, eventId: number): void
  setWindowEventListeners(types: string[]): void
  setWindowSelectionChange(enabled: boolean, eventId: number): void
  setWindowAppearanceChange(enabled: boolean, eventId: number): void
  setAppearance(appearance: string): void
  getAppearance(): string
  setWindowMediaChange(enabled: boolean): void
  getMediaFeatures(): MediaFeatures
  setMediaFeatures(
    reducedMotion?: boolean,
    increasedContrast?: boolean,
    forcedColors?: boolean
  ): void
  simulateKeyDown(keystroke: string, isHeld?: boolean): void
  simulateKeyUp(keystroke: string): void
  simulateClick(x: number, y: number, button?: number, modifiers?: string): void
  simulateScrollWheel(
    x: number,
    y: number,
    deltaX: number,
    deltaY: number,
    modifiers?: string,
    phase?: string
  ): void
  simulateMouseMove(
    x: number,
    y: number,
    pressedButton?: number,
    modifiers?: string,
    movementX?: number,
    movementY?: number
  ): void
  simulateMouseExit(x: number, y: number): void
  simulateMouseDown(
    x: number,
    y: number,
    button: number,
    modifiers?: string,
    clickCount?: number
  ): void
  simulateMouseUp(
    x: number,
    y: number,
    button: number,
    modifiers?: string,
    clickCount?: number
  ): void
  simulateFileDragEnter(x: number, y: number, paths: string[]): void
  simulateFileDragMove(x: number, y: number): void
  simulateFileDragSubmit(x: number, y: number): void
  simulateFileDragExit(): void
  simulateFileDragEnd(): void
  simulateFileDrop(x: number, y: number, paths: string[]): void
  getTreeJson(): string
  getA11yTree(): string
  getAutomationTree(): string
  getRetainedElementCount(): number
  getPaintedFrameCount(): number
  getElementBounds(elementId: number): ElementBounds | null
  getElementsAt(x: number, y: number): number[]
  clockPause(): number
  clockSet(nowMs: number): number
  clockFastForward(deltaMs: number): number
  clockResume(): number
  advanceTime(milliseconds: number): void
  getRootId(): number | null
  getWindowSize(): { width: number; height: number }
  getAllText(): string[]
  scrollTo(elementId: number, x: number, y: number, behavior?: string): void
  scrollToItem(elementId: number, index: number, offsetInItem?: number): void
  scrollIntoView(
    elementId: number,
    block?: string,
    inline?: string,
    behavior?: string
  ): void
  setPointerCapture(elementId: number, pointerId: number): void
  releasePointerCapture(elementId: number, pointerId: number): void
  hasPointerCapture(elementId: number, pointerId: number): boolean
  clickElement(elementId: number): void
  cancelDrag(): void
  requestPointerLock(elementId: number): void
  exitPointerLock(): void
  pointerLockElement(): number | null
  simulatePointerLockGranting(granting: boolean): void
  simulatePointerLockLost(): void
  takeSimulatedWindowActions(): string[]
  getScrollMetrics(elementId: number): number[] | null
  scrollByKey(key: string, shift: boolean): void
  setImage(elementId: number, bytes: Buffer | Uint8Array): void
  selectText(elementId: number): void
  setSelectionRange(elementId: number, start: number, end: number, direction?: string): void
  getTextSelection(elementId: number): { start: number; end: number; direction: string }
  requestSubmit(elementId: number): void
  /** Make a TrueType or OpenType font usable by `fontFamily`. */
  registerFont(bytes: Buffer | Uint8Array): void
  setImagePixels(
    elementId: number,
    width: number,
    height: number,
    pixels: Buffer | Uint8Array,
    format?: ImagePixelFormat
  ): void
  getScrollOffset(elementId: number): number[] | null
  getListScrollTop(elementId: number): number[] | null
  setDebugFrameOverlay(mode: DebugFrameOverlayMode): string
  getDebugFrameOverlay(): string
  cycleDebugFrameOverlay(): string
  resetDebugFrameOverlayStats(): void
  getDebugFrameOverlayStats(): DebugFrameOverlayStats
  dragSelect(x1: number, y1: number, x2: number, y2: number): void
  getSelectedText(): string | null
  getPaintedText(): string[]
  getPaintedHighlights(): HighlightMatch[]
  getSyntaxCacheStats(): number[]
  clearSelection(): void
  captureScreenshot(path: string): void
}

interface NativeTestRendererConstructor {
  new (width?: number, height?: number): NativeTestRendererApi
}

/** Offscreen window size for a test renderer. Defaults to 1280x800 in native. */
export interface TestRendererOptions {
  width?: number
  height?: number
  /** Font files registered right after the renderer is created. See `WindowFontsOption`. */
  fonts?: string[]
  /** Framework scheduling boundary for events drained from native. */
  dispatchEvent?: (event: EventPayload) => void
  /**
   * Framework boundary for the events native waits on: `keydown` and `keyup`,
   * which run inside the simulate call. Must return what the handlers decided.
   */
  dispatchSyncEvent?: (event: EventPayload) => SyncDispatchResult
  /**
   * Receives an error thrown by an event handler, as `onError` does for a live
   * renderer, and the dispatch carries on with the remaining handlers. Without
   * it the error is reported as uncaught (`reportError`), never thrown from the
   * simulate call that delivered the event. A test that expects the error
   * passes `onError`.
   */
  onError?: (error: unknown) => void
}

/** The motion a simulated mouse move reports, in logical pixels. */
export interface MouseMovement {
  x: number
  y: number
}

/** Where a simulated wheel event sits in a gesture. A call that names none starts a
 *  gesture of its own; the scroller that took a gesture's first delta keeps the rest of
 *  it, so continue one with `"moved"`. */
export type ScrollGesturePhase = "started" | "moved" | "ended" | "cancelled"

export type TestWindowOptions = TestRendererOptions & WindowKeyEventHandlers

// The Linux stub has no test methods; the native availability check guards the full API.
function hasNativeTestRendererClass(
  renderer: typeof TestGpuixRenderer
): renderer is typeof TestGpuixRenderer & NativeTestRendererConstructor {
  return hasTestGpuixRenderer()
}

const NativeTestRenderer: NativeTestRendererConstructor | null =
  hasNativeTestRendererClass(TestGpuixRenderer) ? TestGpuixRenderer : null

/** Whether the native TestGpuixRenderer is available (for conditional test registration). */
export const hasNativeTestRenderer = NativeTestRenderer != null

registerKeyframesValidator(validateKeyframes)

// ── Test element tree ────────────────────────────────────────────────

export interface TestElement {
  id: number
  type: string
  style: Record<string, unknown>
  text: string | null
  events: Set<string>
  children: number[]
  parentId: number | null
  testId?: string
  customProps?: Record<string, unknown>
}

/** GPUI `debug_a11y_tree_json` dump. Node keys are ephemeral (`a`, `b`, …). */
export interface A11yTreeNode {
  aria?: {
    role?: string
    label?: string
    description?: string
    value?: string
    author_id?: string
    keyboard_shortcut?: string
    placeholder?: string
    role_description?: string
    braille_label?: string
    braille_role_description?: string
    row_index_text?: string
    column_index_text?: string
    expanded?: boolean
    selected?: boolean
    toggled?: "False" | "True" | "Mixed"
    hidden?: boolean
    disabled?: boolean
    required?: boolean
    read_only?: boolean
    multiselectable?: boolean
    busy?: boolean
    live_atomic?: boolean
    modal?: boolean
    invalid?: "True" | "Grammar" | "Spelling"
    live?: "Off" | "Polite" | "Assertive"
    aria_current?: string
    has_popup?: string
    orientation?: "Horizontal" | "Vertical"
    sort_direction?: "Ascending" | "Descending" | "Other"
    auto_complete?: "Inline" | "List" | "Both"
    numeric_value?: number
    min_numeric_value?: number
    max_numeric_value?: number
    level?: number
    position_in_set?: number
    size_of_set?: number
    row_index?: number
    column_index?: number
    row_count?: number
    column_count?: number
    row_span?: number
    column_span?: number
    /** Relations hold the dump's ephemeral node keys; see `A11yTreeDump.nodes`. */
    labelled_by?: string[]
    described_by?: string[]
    controls?: string[]
    owns?: string[]
    details?: string[]
    flow_to?: string[]
    error_message?: string
    active_descendant?: string
    on_action?: string[]
  }
}

export interface A11yTreeDump {
  root?: string
  /** Null when the window has no focused node. */
  gpui_focus?: string | null
  /** Null when no active descendant is claimed. */
  active_descendant_focus?: string | null
  /** Absent only when the dump is the empty `{}` fallback (no a11y tree yet). */
  frame?: {
    rendered_at: string
    frame_number: number
    window_title: string | null
    node_count: number
    tab_stop_count: number
    viewport_size: { width: number; height: number }
    scale_factor: number
  }
  nodes?: Record<string, A11yTreeNode>
}

// ── TestRenderer ─────────────────────────────────────────────────────

/** The test window is one per thread: making a renderer tears down the one before it. */
let latestRenderer: TestRenderer | undefined

export class TestRenderer implements NativeRenderer {
  /** Native TestGpuixRenderer — all state lives here in Rust's RetainedTree. */
  private native: NativeTestRendererApi
  readonly applyBatch: NativeRenderer["applyBatch"]
  readonly focusNext: () => void
  readonly focusPrevious: () => void
  readonly promptForPaths: NonNullable<NativeRenderer["promptForPaths"]>
  /** The test window's own clipboard. It never touches the system's. */
  readonly writeClipboardText: NonNullable<NativeRenderer["writeClipboardText"]>
  readonly readClipboardText: NonNullable<NativeRenderer["readClipboardText"]>
  readonly setWindowSelectionChange: NonNullable<
    NativeRenderer["setWindowSelectionChange"]
  >
  readonly setWindowAppearanceChange: NonNullable<
    NativeRenderer["setWindowAppearanceChange"]
  >
  readonly setWindowMediaChange: NonNullable<
    NativeRenderer["setWindowMediaChange"]
  >
  readonly getMediaFeatures: NonNullable<NativeRenderer["getMediaFeatures"]>
  readonly setWindowKeyEvents: NonNullable<NativeRenderer["setWindowKeyEvents"]>
  readonly setWindowEventListeners: NonNullable<
    NativeRenderer["setWindowEventListeners"]
  >
  private readonly dispatchEvent: (event: EventPayload) => void
  private readonly dispatchSyncEvent: (event: EventPayload) => SyncDispatchResult
  private readonly onError: ((error: unknown) => void) | undefined

  constructor(options: TestRendererOptions = {}) {
    if (!NativeTestRenderer) {
      throw new Error(
        "TestGpuixRenderer is macOS and Windows only. Linux builds have no test-support because wgpu cannot read a rendered image back yet. GpuixRenderer still works on Linux."
      )
    }
    const fonts = readFontFiles(options.fonts)
    this.native = new NativeTestRenderer(options.width, options.height)
    if (latestRenderer) forgetActiveElementSource(latestRenderer)
    latestRenderer = this
    for (const font of fonts) this.native.registerFont(font)
    registerKeyframesHost(this.native)
    this.applyBatch = this.native.applyBatch.bind(this.native)
    this.focusNext = this.native.focusNext.bind(this.native)
    this.focusPrevious = this.native.focusPrevious.bind(this.native)
    this.promptForPaths = this.native.promptForPaths.bind(this.native)
    this.writeClipboardText = this.native.writeClipboardText.bind(this.native)
    this.readClipboardText = this.native.readClipboardText.bind(this.native)
    this.setWindowKeyEvents = this.native.setWindowKeyEvents.bind(this.native)
    this.setWindowEventListeners = this.native.setWindowEventListeners.bind(
      this.native
    )
    this.setWindowSelectionChange = this.native.setWindowSelectionChange.bind(
      this.native
    )
    this.setWindowAppearanceChange = this.native.setWindowAppearanceChange.bind(
      this.native
    )
    this.onError = options.onError
    this.setWindowMediaChange = this.native.setWindowMediaChange.bind(this.native)
    this.getMediaFeatures = this.native.getMediaFeatures.bind(this.native)
    this.dispatchEvent = options.dispatchEvent ?? ((event) => {
      createRendererState(this).dispatch(event)
    })
    const state = createRendererState(this)
    state.setErrorHandler(options.onError)
    this.dispatchSyncEvent = reportSyncErrors(
      options.dispatchSyncEvent ?? ((event) => state.dispatchSync(event)),
      (error) => state.reportError(error)
    )
    this.native.setSyncEventHandler((event) => this.dispatchSyncEvent(event))
    bindClipboard(this)
  }

  /** Releases the sync event handler, which captures this renderer, and tears the native
   *  window down. Call it after the root is unmounted. Native calls that need the window
   *  throw afterwards; the retained tree stays readable. Without it the window is leaked
   *  when the thread exits. */
  dispose(): void {
    forgetActiveElementSource(this)
    this.native.setSyncEventHandler(null)
    this.native.dispose()
  }

  // ── GPUI pipeline methods ───────────────────────────────────────

  /** Trigger the real GPUI rendering pipeline (GpuixView::render() →
   *  build_element() → apply_styles() → layout). */
  flush(): void {
    this.native.flush()
  }

  /** Drain events collected by the native GPUI event handlers. */
  drainEvents(): EventPayload[] {
    return this.native.drainEvents()
  }

  // ── Native end-to-end simulation ────────────────────────────────
  // These methods go through the full GPUI pipeline:
  //   native simulate → GPUI dispatch → hit test → event handler →
  //   emit_event_full → drainEvents → framework event handler

  /** Drain events from the native GPUI pipeline and feed them into the
   *  framework event registry, triggering state updates synchronously.
   *  Loops until no more events are produced — handles re-entrant events
   *  that may be generated during framework state updates. */
  dispatchNativeEvents(): void {
    // The probe draws, and its reports are what the loop below drains.
    settleObservations(this)
    for (;;) {
      const events = this.native.drainEvents()
      if (events.length === 0) break
      for (const event of events) {
        try {
          this.dispatchEvent(event)
        } catch (error) {
          if (!this.onError) throw error
          this.onError(error)
        }
      }
    }
    // Nothing else would run the observers' batch in a synchronous test.
    flushObservations(this)
  }

  /** End-to-end: focus element → simulate keystrokes through GPUI →
   *  dispatch resulting events to the framework.
   *  @param elementId - element to focus (must have onKeyDown/onKeyUp)
   *  @param keystrokes - space-separated keys, e.g. "a", "enter", "cmd-shift-p"
   */
  /** Send keystrokes to whatever currently holds focus.
   *
   *  Unlike `nativeSimulateKeystrokes`, this focuses nothing first, which is
   *  the only way to test that `autoFocus` (or a click) actually moved focus. */
  simulateKeystrokes(keystrokes: string): void {
    this.native.flush()
    // Events already queued (the focus of an `autoFocus` element) reach JS
    // before a key does, as on a live window. Delivered after it, they would
    // undo what the key's own synchronous focus events did.
    this.dispatchNativeEvents()
    this.native.simulateKeystrokes(keystrokes)
    this.dispatchNativeEvents()
    this.native.flush()
  }

  nativeSimulateKeystrokes(elementId: number, keystrokes: string): void {
    this.native.flush()
    this.native.focusElement(elementId)
    // Focus events are queued but a key is delivered synchronously: let the
    // focus handlers run first, as in a browser.
    this.dispatchNativeEvents()
    this.native.simulateKeystrokes(keystrokes)
    this.dispatchNativeEvents()
  }

  /** End-to-end: focus element → simulate a single key down through GPUI →
   *  dispatch resulting events to the framework. Unlike nativeSimulateKeystrokes,
   *  this dispatches ONLY a KeyDownEvent — no automatic KeyUpEvent follows.
   *  @param elementId - element to focus (must have onKeyDown)
   *  @param keystroke - modifier-key string, e.g. "a", "enter", "cmd-s"
   *  @param isHeld - whether this is a key-repeat event (default: false)
   */
  nativeSimulateKeyDown(elementId: number, keystroke: string, isHeld?: boolean): void {
    this.native.flush()
    this.native.focusElement(elementId)
    // Focus events are queued but a key is delivered synchronously: let the
    // focus handlers run first, as in a browser.
    this.dispatchNativeEvents()
    this.native.simulateKeyDown(keystroke, isHeld)
    this.dispatchNativeEvents()
  }

  /** End-to-end: focus element → simulate a single key up through GPUI →
   *  dispatch resulting events to the framework. Pairs with nativeSimulateKeyDown.
   *  @param elementId - element to focus (must have onKeyUp)
   *  @param keystroke - modifier-key string, e.g. "a", "enter", "cmd-s"
   */
  nativeSimulateKeyUp(elementId: number, keystroke: string): void {
    this.native.flush()
    this.native.focusElement(elementId)
    // Focus events are queued but a key is delivered synchronously: let the
    // focus handlers run first, as in a browser.
    this.dispatchNativeEvents()
    this.native.simulateKeyUp(keystroke)
    this.dispatchNativeEvents()
  }

  /** End-to-end: simulate a click through GPUI hit testing →
   *  dispatch resulting events to the framework. */
  nativeSimulateClick(
    x: number,
    y: number,
    button?: number,
    modifiers?: string
  ): void {
    this.native.flush()
    // Two inputs, not `native.simulateClick`: the press's focus change is queued
    // while the release is delivered synchronously, so the focus handlers must
    // run before it, as they do when a person lets go later.
    this.native.simulateMouseDown(x, y, button ?? 0, modifiers)
    this.dispatchNativeEvents()
    this.native.simulateMouseUp(x, y, button ?? 0, modifiers)
    this.dispatchNativeEvents()
    // Flush again after framework state updates so the Rust RetainedTree
    // is fully rebuilt and GPUI has re-laid-out before any screenshot.
    this.native.flush()
  }

  /** End-to-end: simulate scroll wheel through GPUI →
   *  dispatch resulting events to the framework. Deltas follow GPUI: negative `deltaY`
   *  scrolls down. */
  nativeSimulateScrollWheel(
    x: number,
    y: number,
    deltaX: number,
    deltaY: number,
    modifiers?: string,
    phase?: ScrollGesturePhase
  ): void {
    this.native.flush()
    this.native.simulateScrollWheel(x, y, deltaX, deltaY, modifiers, phase)
    this.dispatchNativeEvents()
  }

  /** Dispatch a wheel without the surrounding flushes, for perf sampling.
   *  Call `flush()` yourself, or the sample is the framework update only and
   *  none of the GPUI build, layout and paint that follows. */
  dispatchScrollWheel(
    x: number,
    y: number,
    deltaX: number,
    deltaY: number,
    modifiers?: string,
    phase?: ScrollGesturePhase
  ): void {
    this.native.simulateScrollWheel(x, y, deltaX, deltaY, modifiers, phase)
    this.dispatchNativeEvents()
  }

  /** Dispatch a move without the surrounding flushes, for perf sampling.
   *  `nativeSimulateMouseMove` flushes before and after, so a drag timed with
   *  it contains two complete paints and cannot be compared to a wheel. */
  dispatchMouseMove(
    x: number,
    y: number,
    pressedButton?: number,
    modifiers?: string,
    movement?: MouseMovement
  ): void {
    this.native.simulateMouseMove(x, y, pressedButton, modifiers, movement?.x, movement?.y)
    this.dispatchNativeEvents()
  }

  /** End-to-end: simulate mouse move through GPUI →
   *  dispatch resulting events to the framework.
   *  @param pressedButton - optional button held during move (0=left, 1=middle, 2=right) for drag simulation
   *  @param movement - what the platform reports as this move's motion, which becomes
   *    `movementX`/`movementY`. A real platform reports the delta from the last position;
   *    the simulation reports `{ x: 0, y: 0 }` unless told otherwise. */
  nativeSimulateMouseMove(
    x: number,
    y: number,
    pressedButton?: number,
    modifiers?: string,
    movement?: MouseMovement
  ): void {
    this.native.flush()
    this.native.simulateMouseMove(x, y, pressedButton, modifiers, movement?.x, movement?.y)
    this.dispatchNativeEvents()
    // Flush again after framework state updates so hover styles are applied
    // and the Rust tree is current before any screenshot.
    this.native.flush()
  }

  /** End-to-end: the pointer leaves the window at (x, y), the way the platform
   *  reports it, with no mouse move after the last position inside. */
  nativeSimulateMouseExit(x: number, y: number): void {
    this.native.flush()
    this.native.simulateMouseExit(x, y)
    this.dispatchNativeEvents()
    this.native.flush()
  }

  /** End-to-end: simulate mouse down through GPUI hit testing →
   *  dispatch resulting events to the framework.
   *  @param button - 0=left (default), 1=middle, 2=right
   *  @param clickCount - the platform's count: 2 for the second press of a double click */
  nativeSimulateMouseDown(
    x: number,
    y: number,
    button?: number,
    modifiers?: string,
    clickCount?: number
  ): void {
    this.native.flush()
    this.native.simulateMouseDown(x, y, button ?? 0, modifiers, clickCount)
    this.dispatchNativeEvents()
    this.native.flush()
  }

  /** End-to-end: simulate mouse up through GPUI hit testing →
   *  dispatch resulting events to the framework.
   *  @param button - 0=left (default), 1=middle, 2=right
   *  @param clickCount - the platform's count: 2 for the release of a double click */
  nativeSimulateMouseUp(
    x: number,
    y: number,
    button?: number,
    modifiers?: string,
    clickCount?: number
  ): void {
    this.native.flush()
    this.native.simulateMouseUp(x, y, button ?? 0, modifiers, clickCount)
    this.dispatchNativeEvents()
    this.native.flush()
  }

  /** End-to-end: files from the OS enter the window at `x`, `y`. */
  nativeSimulateFileDragEnter(x: number, y: number, paths: string[]): void {
    this.native.flush()
    this.native.simulateFileDragEnter(x, y, paths)
    this.dispatchNativeEvents()
    this.native.flush()
  }

  /** End-to-end: the dragged files move to `x`, `y`. */
  nativeSimulateFileDragMove(x: number, y: number): void {
    this.native.flush()
    this.native.simulateFileDragMove(x, y)
    this.dispatchNativeEvents()
    this.native.flush()
  }

  /** End-to-end: the dragged files are released at `x`, `y`. */
  nativeSimulateFileDragSubmit(x: number, y: number): void {
    this.native.flush()
    this.native.simulateFileDragSubmit(x, y)
    this.dispatchNativeEvents()
    this.native.flush()
  }

  /** End-to-end: the dragged files leave the window. */
  nativeSimulateFileDragExit(): void {
    this.native.flush()
    this.native.simulateFileDragExit()
    this.dispatchNativeEvents()
    this.native.flush()
  }

  /** End-to-end: the platform's drag session ends. */
  nativeSimulateFileDragEnd(): void {
    this.native.flush()
    this.native.simulateFileDragEnd()
    this.dispatchNativeEvents()
    this.native.flush()
  }

  /** End-to-end: a whole Finder-style drop at `x`, `y`, as the platform sends it: enter, submit, exit, end. */
  nativeSimulateFileDrop(x: number, y: number, paths: string[]): void {
    this.native.flush()
    this.native.simulateFileDrop(x, y, paths)
    this.dispatchNativeEvents()
    this.native.flush()
  }

  // ── Tree inspection (queries Rust RetainedTree via napi) ────────

  /** Build a flat map of TestElements from the native tree JSON.
   *  One FFI call to get the full tree, then parse into TestElement objects. */
  private buildElementMap(): Map<number, TestElement> {
    const json = JSON.parse(this.native.getTreeJson())
    const map = new Map<number, TestElement>()
    const walk = (node: any, parentId: number | null) => {
      if (!node) return
      map.set(node.id, {
        id: node.id,
        type: node.type,
        style: node.style ?? {},
        text: node.text ?? null,
        events: new Set(node.events ?? []),
        children: (node.children ?? []).map((c: any) => c.id),
        parentId,
        ...(node.testId ? { testId: node.testId } : {}),
        ...(node.customProps ? { customProps: node.customProps } : {}),
      })
      for (const child of node.children ?? []) {
        walk(child, node.id)
      }
    }
    walk(json, null)
    return map
  }

  /** Get the root element. */
  getRoot(): TestElement | undefined {
    const rootId = this.native.getRootId()
    if (rootId == null) return undefined
    return this.buildElementMap().get(rootId)
  }

  /** Get an element by ID. */
  getElement(id: number): TestElement | undefined {
    return this.buildElementMap().get(id)
  }

  /** Find elements by type (e.g. "div", "text"). */
  findByType(type: string): TestElement[] {
    return [...this.buildElementMap().values()].filter((el) => el.type === type)
  }

  /** Find the first text element containing the given string. */
  findByText(text: string): TestElement | undefined {
    const wanted = matchableText(text)
    return [...this.buildElementMap().values()].find(
      (el) => el.text != null && matchableText(el.text).includes(wanted)
    )
  }

  findByTestId(testId: string): TestElement | undefined {
    return [...this.buildElementMap().values()].find((el) => el.testId === testId)
  }

  /** Get all text content in the tree (depth-first). */
  getAllText(): string[] {
    return this.native.getAllText()
  }

  /** Print the tree structure for debugging. Only includes non-empty fields. */
  toJSON(): unknown {
    return JSON.parse(this.native.getTreeJson())
  }

  /** GPUI accessibility dump from the last painted frame. */
  getA11yTree(): A11yTreeDump {
    return JSON.parse(this.native.getA11yTree()) as A11yTreeDump
  }

  getAutomationTree(): string {
    return this.native.getAutomationTree()
  }

  /** Every element the native tree holds, reachable or not. `toJSON()` walks
   *  from the root, so only this can see a node that was detached and leaked. */
  getRetainedElementCount(): number {
    return this.native.getRetainedElementCount()
  }

  /** Frames the window has painted, a layout-only measure pass included. A
   *  test compares it before and after to count the draws a read caused. */
  getPaintedFrameCount(): number {
    return this.native.getPaintedFrameCount()
  }

  getElementBounds(elementId: number): ElementBounds | null {
    flushPendingMutations(this)
    return this.native.getElementBounds(elementId)
  }

  /** Ids of the elements under the window point, topmost first. */
  getElementsAt(x: number, y: number): number[] {
    flushPendingMutations(this)
    return this.native.getElementsAt(x, y)
  }

  clockPause(): number {
    return this.native.clockPause()
  }

  clockSet(nowMs: number): number {
    return this.native.clockSet(nowMs)
  }

  clockFastForward(deltaMs: number): number {
    return this.native.clockFastForward(deltaMs)
  }

  clockResume(): number {
    return this.native.clockResume()
  }

  // ── Motion values ───────────────────────────────────────────────

  createMotionValue(...args: Parameters<MotionValueHost["createMotionValue"]>): void {
    this.native.createMotionValue(...args)
  }

  createMotionTransform(...args: Parameters<MotionValueHost["createMotionTransform"]>): void {
    this.native.createMotionTransform(...args)
  }

  createMotionFollower(...args: Parameters<MotionValueHost["createMotionFollower"]>): void {
    this.native.createMotionFollower(...args)
  }

  createMotionSpring(...args: Parameters<MotionValueHost["createMotionSpring"]>): void {
    this.native.createMotionSpring(...args)
  }

  disposeMotionValue(id: number): boolean {
    return this.native.disposeMotionValue(id)
  }

  getMotionValue(id: number): unknown {
    return this.native.getMotionValue(id)
  }

  getMotionValueVelocity(id: number): number {
    return this.native.getMotionValueVelocity(id)
  }

  isMotionValueAnimating(id: number): boolean {
    return this.native.isMotionValueAnimating(id)
  }

  setMotionValue(id: number, value: unknown): void {
    this.native.setMotionValue(id, value)
  }

  jumpMotionValue(id: number, value: unknown): void {
    this.native.jumpMotionValue(id, value)
  }

  animateMotionValue(id: number, target: unknown, transition?: unknown): void {
    this.native.animateMotionValue(id, target, transition)
  }

  stopMotionValue(id: number): void {
    this.native.stopMotionValue(id)
  }

  setMotionValueListener(id: number, change: boolean, animation: boolean): void {
    this.native.setMotionValueListener(id, change, animation)
  }

  /** Advance GPUI's test dispatcher and run due timers.
   *  This is not `clockFastForward`. That moves the motion clock only.
   *  Use this for caret blink, input drag autoscroll, and list edge scroll. */
  advanceTime(milliseconds: number): void {
    this.native.advanceTime(milliseconds)
    this.dispatchNativeEvents()
  }

  focusElement(elementId: number): void {
    this.native.flush()
    this.native.focusElement(elementId)
    this.dispatchNativeEvents()
  }

  blurElement(elementId: number): void {
    this.native.flush()
    this.native.blurElement(elementId)
    this.dispatchNativeEvents()
  }

  matchesFocus(elementId: number, selector: string): boolean {
    this.native.flush()
    const matches = this.native.matchesFocus(elementId, selector)
    this.dispatchNativeEvents()
    return matches
  }

  getFocusedElementId(): number | null {
    this.native.flush()
    return this.native.getFocusedElementId()
  }

  /** Makes the offscreen window key (`true`, the default) or not, as when the user switches apps. */
  setWindowActive(active: boolean): void {
    this.native.setWindowActive(active)
    this.native.flush()
    this.dispatchNativeEvents()
  }

  focusNextWithin(elementId: number): void {
    this.native.flush()
    this.native.focusNextWithin(elementId)
    this.dispatchNativeEvents()
  }

  focusPreviousWithin(elementId: number): void {
    this.native.flush()
    this.native.focusPreviousWithin(elementId)
    this.dispatchNativeEvents()
  }

  /** The offscreen window size, so `useWindowSize()` works under test. */
  getWindowSize(): { width: number; height: number } {
    return this.native.getWindowSize()
  }

  /** Validates the value and does nothing. The test window is a real offscreen
   *  platform window with no run loop, so it never receives an appearance change. */
  setAppearance(appearance: string): void {
    this.native.setAppearance(appearance)
  }

  /** The offscreen window's appearance, which is the system's. */
  getAppearance(): string {
    return this.native.getAppearance()
  }

  /** Set the OS accessibility preferences this window reports (all start as
   *  "no preference") and deliver the resulting `mediaChange` event. The test
   *  window never hears from the OS, so this is how a test flips reduced motion. */
  setMediaFeatures(features: Partial<MediaFeatures>): void {
    this.native.setMediaFeatures(
      features.reducedMotion,
      features.increasedContrast,
      features.forcedColors
    )
    this.dispatchNativeEvents()
  }

  // ── Scroll API ──────────────────────────────────────────────────

  /** Set the scroll offset of a scrollable element (overflow: "scroll").
   *  x and y are negative pixel values (scroll down = more negative y).
   *  Call flush() internally to apply. */
  scrollTo(elementId: number, x: number, y: number, behavior?: string): void {
    this.native.flush()
    this.native.scrollTo(elementId, x, y, behavior)
    // Flush again to re-render with the new offset
    this.native.flush()
  }

  /** Scroll a child into view by its index in the children list.
   *
   *  `offsetInItem` is in pixels. A negative value anchors the viewport top
   *  above the item, resolved against measured row heights at layout time, so
   *  a row stays pixel-stable while unmeasured rows are spliced in above it. */
  scrollToItem(elementId: number, index: number, offsetInItem?: number): void {
    this.native.flush()
    this.native.scrollToItem(elementId, index, offsetInItem)
    this.dispatchNativeEvents()
    this.native.flush()
  }

  /** The DOM's `scrollIntoView`: every scrollable ancestor scrolls until the element sits
   *  where `block` (default `"start"`) and `inline` (default `"nearest"`) ask. */
  scrollIntoView(
    elementId: number,
    block?: string,
    inline?: string,
    behavior?: string
  ): void {
    this.native.flush()
    this.native.scrollIntoView(elementId, block, inline, behavior)
    this.dispatchNativeEvents()
    this.native.flush()
  }

  /** Applies from the next simulated pointer input. See `PointerCaptureMethods`. */
  setPointerCapture(elementId: number, pointerId: number): void {
    this.native.setPointerCapture(elementId, pointerId)
  }

  releasePointerCapture(elementId: number, pointerId: number): void {
    this.native.releasePointerCapture(elementId, pointerId)
  }

  hasPointerCapture(elementId: number, pointerId: number): boolean {
    return this.native.hasPointerCapture(elementId, pointerId)
  }

  clickElement(elementId: number): void {
    this.native.clickElement(elementId)
  }

  /** Applies from the next simulated pointer input. */
  cancelDrag(): void {
    this.native.cancelDrag()
  }

  /** The frame it asks for is drawn here, so `onPointerLockChange` or
   *  `onPointerLockError` has run when this returns. */
  requestPointerLock(elementId: number): void {
    this.native.requestPointerLock(elementId)
    this.dispatchNativeEvents()
  }

  exitPointerLock(): void {
    this.native.exitPointerLock()
    this.dispatchNativeEvents()
  }

  pointerLockElement(): number | null {
    return this.native.pointerLockElement()
  }

  /** Whether the simulated platform grants lock requests. The offscreen test
   *  window is never key, so a real GPUI lock would always be refused; this
   *  stands in for a platform that can lock, and off, for one that refuses. */
  simulatePointerLockGranting(granting: boolean): void {
    this.native.simulatePointerLockGranting(granting)
  }

  /** The platform drops the lock, as when the window deactivates. */
  simulatePointerLockLost(): void {
    this.native.simulatePointerLockLost()
    this.dispatchNativeEvents()
  }

  /** What the window was asked to do by presses in `appRegion: "drag"` boxes since the last
   *  call, oldest first: `"move"`, `"titlebarDoubleClick"` (macOS), `"zoom"` or `"showMenu"`
   *  (Linux). The offscreen window cannot start a real move, so the test window records
   *  instead. Every test window is frameless, so `appRegion` always applies. */
  takeSimulatedWindowActions(): string[] {
    return this.native.takeSimulatedWindowActions()
  }

  /** The default action of a scrolling key. See `scrollByKey` on the native renderer. */
  scrollByKey(key: string, shift: boolean): void {
    this.native.flush()
    this.native.scrollByKey(key, shift)
    this.dispatchNativeEvents()
    this.native.flush()
  }

  /** `[scrollLeft, scrollTop, scrollWidth, scrollHeight, clientWidth, clientHeight]`, or
   *  null for an element that has not been painted. */
  getScrollMetrics(elementId: number): number[] | null {
    this.native.flush()
    return this.native.getScrollMetrics(elementId)
  }

  setImage(elementId: number, bytes: Buffer | Uint8Array): void {
    this.native.flush()
    this.native.setImage(elementId, bytes as Buffer)
    this.native.flush()
  }

  selectText(elementId: number): void {
    this.native.flush()
    this.native.selectText(elementId)
    this.native.flush()
  }

  setSelectionRange(
    elementId: number,
    start: number,
    end: number,
    direction?: string
  ): void {
    this.native.flush()
    this.native.setSelectionRange(elementId, start, end, direction)
    this.native.flush()
  }

  getTextSelection(elementId: number): { start: number; end: number; direction: string } {
    this.native.flush()
    return this.native.getTextSelection(elementId)
  }

  requestSubmit(elementId: number): void {
    this.native.flush()
    this.native.requestSubmit(elementId)
    this.dispatchNativeEvents()
    this.native.flush()
  }

  registerFont(bytes: Buffer | Uint8Array): void {
    this.native.registerFont(bytes as Buffer)
    this.native.flush()
  }

  setImagePixels(
    elementId: number,
    width: number,
    height: number,
    pixels: Buffer | Uint8Array,
    format?: ImagePixelFormat
  ): void {
    this.native.flush()
    this.native.setImagePixels(elementId, width, height, pixels as Buffer, format)
    this.native.flush()
  }

  /** Get the current scroll offset [x, y] or null if element is not scrollable. */
  getScrollOffset(elementId: number): [number, number] | null {
    this.native.flush()
    const result = this.native.getScrollOffset(elementId)
    if (!result) return null
    return [result[0], result[1]]
  }

  /** The logical scroll anchor of a `<virtual-list>`:
   *  `[itemIndex, offsetInItemPx, viewportHeightPx]`, or null for anything
   *  else. `itemIndex == item count` is gpui's at-end sentinel. Exact even
   *  while row heights are still estimates, because it is the anchor gpui
   *  itself scrolls by. */
  getListScrollTop(elementId: number): [number, number, number] | null {
    this.native.flush()
    const result = this.native.getListScrollTop(elementId)
    if (!result) return null
    return [result[0], result[1], result[2]]
  }

  // ── Selection API ───────────────────────────────────────────────

  /** Drag-select from (x1,y1) to (x2,y2) and return the selected text.
   *
   *  Selection listeners are registered during **paint**, so the native helper
   *  flushes between every step. Calling simulateMouseDown/Move/Up by hand
   *  without those flushes selects nothing. */
  dragSelect(x1: number, y1: number, x2: number, y2: number): string | null {
    this.native.dragSelect(x1, y1, x2, y2)
    return this.native.getSelectedText()
  }

  /** The current selection joined in document order, or null. */
  getSelectedText(): string | null {
    return this.native.getSelectedText()
  }

  /** Every string painted in the last frame, in paint order.
   *
   *  `getAllText()` only sees `<text>` nodes in the retained tree. Native
   *  elements like `<code>` and `<diff>` paint their text inside GPUI, so this
   *  is the only way to assert on what they rendered. */
  getPaintedText(): string[] {
    return this.native.getPaintedText()
  }

  /** Every highlight wash painted in the last frame, in paint order.
   *
   *  A quad never lands in `getPaintedText()`, and a soft-wrapped match must
   *  draw one box per visual row, so each entry carries its `rects`. */
  getPaintedHighlights(): HighlightMatch[] {
    return this.native.getPaintedHighlights()
  }

  /** Syntax-cache counters as `[hits, misses, documents]`. */
  getSyntaxCacheStats(): [number, number, number] {
    const [hits, misses, documents] = this.native.getSyntaxCacheStats()
    return [hits, misses, documents]
  }

  clearSelection(): void {
    this.native.clearSelection()
    this.native.flush()
  }

  setDebugFrameOverlay(mode: DebugFrameOverlayMode): string {
    return this.native.setDebugFrameOverlay(mode)
  }

  getDebugFrameOverlay(): string {
    return this.native.getDebugFrameOverlay()
  }

  cycleDebugFrameOverlay(): string {
    return this.native.cycleDebugFrameOverlay()
  }

  resetDebugFrameOverlayStats(): void {
    this.native.resetDebugFrameOverlayStats()
  }

  getDebugFrameOverlayStats(): DebugFrameOverlayStats {
    return this.native.getDebugFrameOverlayStats()
  }

  /** Capture the current Metal or DirectX frame and save it as a PNG. */
  captureScreenshot(path: string): void {
    this.native.flush()
    this.native.captureScreenshot(path)
  }

  /** Whether the native GPUI test renderer is available. Always true. */
  get hasNative(): boolean {
    return true
  }
}
