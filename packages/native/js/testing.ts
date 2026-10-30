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
  measureMotionAnimation,
  validateKeyframes,
  type ElementWebAnimationInfo,
  type EventPayload,
  type MediaFeatures,
  type MotionValuePlayback,
  type PatchCounters,
  type WebAnimationEffectInfo,
  type WebAnimationInfo,
  type WebAnimationKeyframeInfo,
  type WebAnimationTimingUpdate,
  type WindowOptions,
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
import { forgetElementSource, forgetWindowSource } from "./events/window-target.js"
import type { HostDropEffect } from "./events/types.js"
import { matchableText } from "./text-match.js"
import { flushPendingMutations } from "./bounds.js"
import { closeWebAnimations } from "./web-animations.js"
import { flushObservations, settleObservations } from "./observers.js"
import { readFontFiles, type WindowFontsOption } from "./fonts.js"
import {
  registerKeyframesHost,
  unregisterKeyframesHost,
  registerKeyframesValidator,
  type KeyframesHost,
} from "./keyframes.js"
import { registerAnimationMeasurer } from "./motion-value.js"
import { createRendererState, reportSyncErrors, type SyncDispatchResult } from "./renderer-state.js"
export {
  applyMacCpuThrottleFromEnv,
  MAC_CPU_THROTTLES,
  readMacCpuThrottle,
} from "./cpu-throttle.js"
export type { MacCpuThrottle } from "./cpu-throttle.js"

// The release index.d.ts has no test-support types, so these mirror the generated
// interfaces of the test-support build (fields documented in src/test_renderer.rs).
// __tests__/testing-types.check.ts fails when either side drifts.
export interface BuildCounters {
  elementsBuilt: number
  elementsSkipped: number
  styleDescClones: number
  inheritedClones: number
  retainableRoots: number
  retainableElements: number
  retainableElementsLarge: number
  retainMismatches: number
  unitsBuilt: number
  unitsOversized: number
  motionsSynced: number
  motionValuesSampled: number
  retainReasons: string
}

export interface FrameWorkCounters {
  prepainted: number
  culledPrepainted: number
  painted: number
  culledPainted: number
  paintedPrimitives: number
  replayedPrimitives: number
  boundsInserts: number
  rasterizedGlyphs: number
  dormantTicks: number
}

export interface LayoutCounters {
  nodesCreated: number
  nodesReused: number
  nodesRemoved: number
  stylesSet: number
  childrenSet: number
  passes: number
  measures: number
  nodesLaidOut: number
  nodesSized: number
  relationVisits: number
  shapes: number
  wrappedLayouts: number
  lineLayoutHits: number
  shapeCacheHits: number
  extentWalks: number
  overflowWalks: number
  unsnappedOverflowWalks: number
}

/** What the page of a `<webview>` was asked to do. */
export interface WebviewCommand {
  /** `loadURL`, `reload`, `stop`, `goBack`, `goForward`, `executeJavaScript` or
   *  `setFirstResponder`. */
  name: string
  /** The URL, the code, or for `setFirstResponder` `"true"`, `"false"`, `"forward"` (Tab) or
   *  `"backward"` (Shift+Tab). */
  argument?: string | null
}

/** Where the last composed frame put the surface of a `<webview>`, in window pixels. */
export interface WebviewSurface {
  /** Painted, and not clipped away entirely. */
  visible: boolean
  /** The whole surface, uncut. */
  x: number
  y: number
  width: number
  height: number
  /** What shows of it: the surface cut by the clip of the content around it. */
  clipX: number
  clipY: number
  clipWidth: number
  clipHeight: number
  opacity: number
  cornerRadius: number
  /** The position among the layers of the frame, bottom to top. */
  layer: number
}

/** A layer of the last composed frame, bottom to top: the window's GPUI layer, native surfaces and
 *  the GPUI layers between and above them. */
export interface CompositionLayer {
  kind: "gpui" | "native"
  /** The window's own GPUI layer. */
  isBase: boolean
  /** GPUI layers: how many primitives of the scene it draws. */
  primitives: number
  /** Native layers: the element of its `<webview>`. */
  webview?: number | null
  visible: boolean
}

/** The container view AppKit holds for a `<webview>` surface, in window points from the top
 *  left: the placement that was applied, not only recorded. */
export interface WebviewNativeView {
  hidden: boolean
  x: number
  y: number
  width: number
  height: number
  /** The content view the page lives in, relative to the container: whole size, cut by it. */
  contentX: number
  contentY: number
  contentWidth: number
  contentHeight: number
  opacity: number
  /** A corner radius is cut into it. */
  masked: boolean
}

export interface RetainedCounters {
  hits: number
  replays: number
  lazyBuilds: number
  builds: number
  layoutFallbacks: number
  layoutSkips: number
  layoutReplays: number
  layoutBuilds: number
  layoutTranslations: number
  layoutRecords: number
  verified: number
  mismatches: number
  nonReusable: string
  relocations: number
  nonRelocatable: string
  relocationMisses: string
}

interface NativeTestRendererApi
  extends Omit<NativeRenderer, keyof MotionValueHost>,
    MotionValueHost,
    KeyframesHost {
  promptForPaths(options?: PathPromptOptions): Promise<string[] | null>
  writeClipboardText(text: string): void
  readClipboardText(): string | null
  flush(): void
  flushIfDirty(): void
  drainEvents(): EventPayload[]
  setFrameWakeCallback(callback?: (() => void) | null): boolean
  requiresTick(): boolean
  tick(): boolean
  postNativeEvent(kind: string, x?: number | null, y?: number | null): void
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
  setScrollbarStyle(style: string): void
  getAppearance(): string
  getWindowTitle(): string
  setWindowMediaChange(enabled: boolean): void
  getMediaFeatures(): MediaFeatures
  setWindowSize(width: number, height: number): void
  setWindowScaleFactor(scaleFactor: number): void
  setMediaFeatures(
    reducedMotion?: boolean,
    increasedContrast?: boolean,
    forcedColors?: boolean
  ): void
  simulateKeyDown(keystroke: string, isHeld?: boolean): void
  simulateKeyUp(keystroke: string): void
  simulateImeKeyDown(keystroke: string, action: string, text?: string): void
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
    modifiers?: string,
    movementX?: number,
    movementY?: number
  ): void
  simulateMouseExit(x: number, y: number): void
  simulateLostMouseUp(button?: number): void
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
  simulateFileDragEnter(x: number, y: number, paths: string[]): string | null
  simulateFileDragMove(x: number, y: number): string | null
  simulateFileDragSubmit(x: number, y: number): void
  simulateFileDragExit(): void
  simulateFileDragEnd(): void
  simulateFileDrop(x: number, y: number, paths: string[]): void
  getTreeJson(): string
  getA11yTree(): string
  getAutomationTree(): string
  getRetainedElementCount(): number
  getBuildCounters(): BuildCounters
  getLayoutCounters(): LayoutCounters
  getRetainedCounters(): RetainedCounters
  getFrameWorkCounters(): FrameWorkCounters
  getRetentionMismatches(): string[]
  resetBuildCounters(): void
  setRetainVerify(verify: boolean): void
  setRetainPlanning(planning: boolean): void
  setRetainEnabled(enabled: boolean): void
  setPatchEnabled(enabled: boolean): void
  getPatchCounters(): PatchCounters
  setRetainLift(names: string[]): void
  getRetainMismatches(): string[]
  getPaintedFrameCount(): number
  getLayoutPassCount(): number
  getCulledSubtreeCount(): number
  setA11yActive(active: boolean): void
  setInlineDraws(enabled: boolean): void
  drawFrame(): void
  getElementBounds(elementId: number): ElementBounds | null
  getBuiltElementBounds(elementId: number): ElementBounds | null
  probeElementBounds(elementId: number): ElementBounds | null
  getElementsAt(x: number, y: number): number[]
  clockPause(): number
  clockSet(nowMs: number): number
  clockFastForward(deltaMs: number): number
  clockAdvance(deltaMs: number): number
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
  getAnimations(elementId: number): WebAnimationInfo[]
  getSubtreeAnimations(elementId: number): ElementWebAnimationInfo[]
  getDocumentAnimations(): ElementWebAnimationInfo[]
  getSvgMarkup(elementId: number): string | null
  getAnimation(animationId: number): WebAnimationInfo
  releaseAnimation(animationId: number): void
  forgetAnimation(animationId: number): void
  getAnimationEffect(animationId: number): WebAnimationEffectInfo
  getAnimationKeyframes(animationId: number): WebAnimationKeyframeInfo[]
  updateAnimationTiming(animationId: number, timing: WebAnimationTimingUpdate): void
  pauseAnimation(animationId: number): void
  playAnimation(animationId: number): void
  cancelAnimation(animationId: number): void
  finishAnimation(animationId: number): void
  reverseAnimation(animationId: number): void
  setAnimationCurrentTime(animationId: number, milliseconds: number): void
  setAnimationPlaybackRate(animationId: number, rate: number): void
  updateAnimationPlaybackRate(animationId: number, rate: number): void
  setAnimationStartTime(animationId: number, startTime: number | null): void
  getAnimationTimelineTime(): number
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
  /** Encoded bytes for the `blob:` URL a `url()` background names. */
  setStyleImage(url: string, bytes: Buffer | Uint8Array): void
  failStyleImage(url: string, message: string): void
  selectText(elementId: number): void
  setSelectionRange(elementId: number, start: number, end: number, direction?: string): void
  getTextSelection(elementId: number): { start: number; end: number; direction: string }
  requestSubmit(elementId: number): void
  webviewCommand(elementId: number, command: string, argument?: string): void
  webviewState(elementId: number): {
    url: string
    title: string
    canGoBack: boolean
    canGoForward: boolean
  }
  webviewExecuteJavascript(elementId: number, code: string): Promise<string | null>
  getWebviewCommands(elementId: number): WebviewCommand[]
  simulateWebviewStartLoading(elementId: number): void
  simulateWebviewStopLoading(elementId: number): void
  simulateWebviewNavigate(elementId: number, url: string): void
  simulateWebviewTabOut(elementId: number, backwards: boolean): void
  simulateWebviewTitle(elementId: number, title: string, explicitSet: boolean): void
  simulateWebviewFail(
    elementId: number,
    errorCode: number,
    errorDescription: string,
    validatedUrl?: string
  ): void
  setWebviewHistory(elementId: number, canGoBack: boolean, canGoForward: boolean): void
  settleWebviewScript(elementId: number, result?: string, error?: string): void
  getWebviewSurface(elementId: number): WebviewSurface | null
  getCompositionLayers(): CompositionLayer[]
  getWebviewNativeView(elementId: number): WebviewNativeView
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
  getCursor(): string
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
  new (options?: WindowOptions, scaleFactor?: number): NativeTestRendererApi
}

/**
 * The options a live window takes (`WindowOptions`), applied to the offscreen test window by
 * the same translation, plus the fonts option and the test hooks below. The size defaults to
 * 1280x800 and `focus` to false, since the window is offscreen and must not take the OS's
 * keyboard. `fullscreen`, `appId`, `appName` and `layerShell` have no effect on it; the README's
 * Testing section lists what each option does there.
 */
export interface TestRendererOptions extends WindowOptions, WindowFontsOption {
  /**
   * Device pixels per CSS px the offscreen test window opens with, `1` when unset (headless
   * Chrome's own), whatever display the machine has. `TestRenderer` only: a live window takes
   * its display's scale.
   */
  scaleFactor?: number
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
/**
 * One key an input method consumes, and what the input method then does with
 * it. `key` is a press in the `simulateKeystrokes` grammar.
 */
export type CompositionStep =
  /** The composition becomes `text`: `compositionstart` on the first, then `compositionupdate`. */
  | { key: string; action: "mark"; text: string }
  /** The composition ends with `text` as the final text: `compositionend`. */
  | { key: string; action: "commit"; text: string }
  /** The composition ends with the text it has: `compositionend`. */
  | { key: string; action: "unmark" }

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
registerAnimationMeasurer((from, target, transition) => {
  const length = measureMotionAnimation(from, target, transition ?? null)
  return { duration: length.duration, iterationDuration: length.iterationDuration }
})

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
  /** In device pixels from the window's content origin: clipped and off-screen
   *  nodes are in the tree, with the box they have. */
  bounds?: { x0: number; y0: number; x1: number; y1: number }
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

/** Throws if a replayed frame of this window differed from a build of it, or if gpuix's own
 *  check found an element judged unchanged that built differently, and prints the counters when
 *  GPUIX_RETAIN_VERIFY_REPORT is set. The gpuix findings are read from the window's list, not
 *  from `retainMismatches`, which is one count per thread that only `resetBuildCounters` clears. */
function reportRetentionMismatches(native: NativeTestRendererApi): void {
  const counters = native.getRetainedCounters()
  const findings = native.getRetainMismatches()
  if (process.env.GPUIX_RETAIN_VERIFY_REPORT !== undefined) {
    console.log(
      `[retain-verify] verified=${counters.verified} mismatches=${counters.mismatches} gpuix-mismatches=${findings.length} replays=${counters.replays} ` +
        `hits=${counters.hits} builds=${counters.builds} lazy=${counters.lazyBuilds} non-reusable=${counters.nonReusable}`
    )
  }
  if (counters.mismatches > 0) {
    throw new Error(`${counters.mismatches} replayed frames differed from a build:\n${native.getRetentionMismatches().join('\n')}`)
  }
  if (findings.length > 0) {
    throw new Error(`${findings.length} elements were judged unchanged and built differently:\n${findings.join('\n')}`)
  }
}

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
    this.native = new NativeTestRenderer(options, options.scaleFactor)
    if (latestRenderer) {
      // The native side keeps one window per thread and this constructor replaced it, so the
      // earlier renderer is as dead as a disposed one: it would reach the new window's state, and
      // throw once that is disposed.
      forgetWindowSource(latestRenderer)
      unregisterKeyframesHost(latestRenderer.native)
      closeWebAnimations(latestRenderer)
    }
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
    // A run under GPUIX_RETAIN_VERIFY fails at the window that showed a replayed frame that a
    // build would have shown differently, and says which test's window it was.
    if (process.env.GPUIX_RETAIN_VERIFY !== undefined) reportRetentionMismatches(this.native)
    forgetElementSource(this)
    unregisterKeyframesHost(this.native)
    closeWebAnimations(this)
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

  // ── Frame loop ──────────────────────────────────────────────────
  // The surface `startFrameLoop` drives, so the wake path can be checked without a live window.
  // The test window draws no frames: a tick delivers the input AppKit has queued for it.

  /** Wakes `callback` when the main run loop has work, as `GpuixRenderer.setFrameWakeCallback`
   *  does. Returns true on macOS. Pass null to clear. */
  setFrameWakeCallback(callback?: (() => void) | null): boolean {
    return this.native.setFrameWakeCallback(callback)
  }

  /** Whether `startFrameLoop` must tick this renderer: true on macOS. */
  requiresTick(): boolean {
    return this.native.requiresTick()
  }

  /** One frame-loop step: hands the input AppKit queued for the test window (see
   *  `postNativeEvent`) to it, then to the JS handlers, as `dispatchNativeEvents` does. Draws
   *  nothing: frames stay `flush` and `drawFrame`. */
  tick(): boolean {
    const running = this.native.tick()
    this.dispatchNativeEvents()
    return running
  }

  /** Posts a real NSEvent for the test window from `com.apple.NSEventThread`, the way AppKit
   *  receives input, so an idle `startFrameLoop(testRenderer)` is woken by it. `kind` is
   *  `"keyDown"` (the key `a`), `"mouseMove"`, `"click"` or `"scroll"` (3 lines down); the last
   *  three need `x` and `y` in logical pixels from the window's top left. The window is neither
   *  made key nor shown, and nothing reaches the OS. `tick()` delivers the event. */
  postNativeEvent(kind: string, x?: number, y?: number): void {
    this.native.postNativeEvent(kind, x, y)
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

  /** Press keys on whatever currently holds focus, in Playwright's
   *  `keyboard.press` grammar. A press is a DOM `key` value (`"Enter"`, `"a"`,
   *  `"A"`, `"ArrowDown"`, `"Escape"`, `"Tab"`, `"F2"`) or a `code` name
   *  (`"Space"`, `"KeyA"`, `"Digit1"`), with modifiers joined by `+`
   *  (`"Shift+Tab"`, `"Meta+a"`, `"Alt+ArrowLeft"`, `"ControlOrMeta+Shift+P"`;
   *  `ControlOrMeta` is Meta on macOS). `keystrokes` is a sequence of presses
   *  separated by spaces: `"a Shift+b Enter"`. Each press is a `keydown` then a
   *  `keyup`, and a typed key inserts its character into a focused input.
   *  Names are case-sensitive and an unknown one throws before anything is
   *  pressed.
   *
   *  Unlike `nativeSimulateKeystrokes`, this focuses nothing first, which is
   *  the only way to test that `autoFocus` (or a click) actually moved focus. */
  simulateKeystrokes(keystrokes: string): void {
    this.native.flushIfDirty()
    // Events already queued reach JS before a key does, as on a live window.
    // Delivered after it, they would undo what the key's own synchronous focus
    // events did.
    this.dispatchNativeEvents()
    this.native.simulateKeystrokes(keystrokes)
    this.dispatchNativeEvents()
    this.native.flushIfDirty()
  }

  /** Focus an element, then press keys on it (the `simulateKeystrokes` grammar).
   *  @param elementId - element to focus
   *  @param keystrokes - space-separated presses, e.g. `"h i Enter Shift+Tab"` */
  nativeSimulateKeystrokes(elementId: number, keystrokes: string): void {
    this.native.flushIfDirty()
    this.native.focusElement(elementId)
    // Focus events are queued but a key is delivered synchronously: let the
    // focus handlers run first, as in a browser.
    this.dispatchNativeEvents()
    this.native.simulateKeystrokes(keystrokes)
    this.dispatchNativeEvents()
  }

  /** Focus an element, then send one `keydown` for one press (the
   *  `simulateKeystrokes` grammar), with no `keyup` after it. Pairs with
   *  `nativeSimulateKeyUp`.
   *  @param elementId - element to focus
   *  @param keystroke - one press, e.g. `"a"`, `"Enter"`, `"Meta+s"`
   *  @param isHeld - whether this is a key-repeat event (default: false)
   */
  nativeSimulateKeyDown(elementId: number, keystroke: string, isHeld?: boolean): void {
    this.native.flushIfDirty()
    this.native.focusElement(elementId)
    // Focus events are queued but a key is delivered synchronously: let the
    // focus handlers run first, as in a browser.
    this.dispatchNativeEvents()
    this.native.simulateKeyDown(keystroke, isHeld)
    this.dispatchNativeEvents()
  }

  /** Focus an element, then send one `keyup` for one press (the
   *  `simulateKeystrokes` grammar). Pairs with `nativeSimulateKeyDown`.
   *  @param elementId - element to focus
   *  @param keystroke - one press, e.g. `"a"`, `"Enter"`, `"Meta+s"`
   */
  nativeSimulateKeyUp(elementId: number, keystroke: string): void {
    this.native.flushIfDirty()
    this.native.focusElement(elementId)
    // Focus events are queued but a key is delivered synchronously: let the
    // focus handlers run first, as in a browser.
    this.dispatchNativeEvents()
    this.native.simulateKeyUp(keystroke)
    this.dispatchNativeEvents()
  }

  /** Focus a text input, then run an input method composition in it, one step
   *  per key. Each step is what macOS does for a key the input method takes: a
   *  `keydown` with `key` `"Process"`, the composition events and the `change`
   *  of the edit, then the `keyup` of that key. A text input is the only element
   *  that composes, so `elementId` must be one.
   *  @param elementId - the `<input>` or `<textarea>` to compose in
   *  @param steps - the keys the input method consumes, in order */
  simulateComposition(elementId: number, steps: readonly CompositionStep[]): void {
    this.native.flushIfDirty()
    this.native.focusElement(elementId)
    // Focus events are queued but a key is delivered synchronously: let the
    // focus handlers run first, as in a browser.
    this.dispatchNativeEvents()
    for (const step of steps) {
      this.native.simulateImeKeyDown(
        step.key,
        step.action,
        step.action === "unmark" ? undefined : step.text
      )
      // The edit's events are queued behind the keydown, and a browser fires
      // them before the keyup.
      this.dispatchNativeEvents()
      this.native.simulateKeyUp(step.key)
      this.dispatchNativeEvents()
    }
    this.native.flushIfDirty()
  }

  /** End-to-end: simulate a click through GPUI hit testing →
   *  dispatch resulting events to the framework. */
  nativeSimulateClick(
    x: number,
    y: number,
    button?: number,
    modifiers?: string
  ): void {
    this.native.flushIfDirty()
    // Two inputs, not `native.simulateClick`: whatever the press queued must
    // reach JS before the release, as it does when a person lets go later.
    this.native.simulateMouseDown(x, y, button ?? 0, modifiers)
    this.dispatchNativeEvents()
    this.native.simulateMouseUp(x, y, button ?? 0, modifiers)
    this.dispatchNativeEvents()
    // Flush again after framework state updates so the Rust RetainedTree
    // is fully rebuilt and GPUI has re-laid-out before any screenshot.
    this.native.flushIfDirty()
  }

  /** End-to-end: simulate scroll wheel through GPUI →
   *  dispatch resulting events to the framework. Deltas have the DOM's sign, like
   *  `WheelEvent.deltaY`: positive `deltaY` scrolls down (the content moves up),
   *  positive `deltaX` scrolls right. */
  nativeSimulateScrollWheel(
    x: number,
    y: number,
    deltaX: number,
    deltaY: number,
    modifiers?: string,
    phase?: ScrollGesturePhase
  ): void {
    this.native.flushIfDirty()
    this.native.simulateScrollWheel(x, y, deltaX, deltaY, modifiers, phase)
    this.dispatchNativeEvents()
  }

  /** Dispatch a wheel without the surrounding flushes, for perf sampling. Deltas
   *  have the DOM's sign: positive `deltaY` scrolls down.
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
   *  `nativeSimulateMouseMove` draws a pending change before and after the move, so a
   *  drag timed with it includes the paint and a wheel timed with `dispatchScrollWheel`
   *  does not. */
  dispatchMouseMove(
    x: number,
    y: number,
    modifiers?: string,
    movement?: MouseMovement
  ): void {
    this.native.simulateMouseMove(x, y, modifiers, movement?.x, movement?.y)
    this.dispatchNativeEvents()
  }

  /** End-to-end: simulate mouse move through GPUI →
   *  dispatch resulting events to the framework. The move reports the buttons held by
   *  `nativeSimulateMouseDown` and not yet released by `nativeSimulateMouseUp`, as a real
   *  OS does on every move, so a drag is a down, moves, an up.
   *  @param movement - what the platform reports as this move's motion, which becomes
   *    `movementX`/`movementY`. A real platform reports the delta from the last position;
   *    the simulation reports `{ x: 0, y: 0 }` unless told otherwise. */
  nativeSimulateMouseMove(
    x: number,
    y: number,
    modifiers?: string,
    movement?: MouseMovement
  ): void {
    this.native.flushIfDirty()
    this.native.simulateMouseMove(x, y, modifiers, movement?.x, movement?.y)
    this.dispatchNativeEvents()
    // Flush again after framework state updates so hover styles are applied
    // and the Rust tree is current before any screenshot.
    this.native.flushIfDirty()
  }

  /** End-to-end: the pointer leaves the window at (x, y), the way the platform
   *  reports it, with no mouse move after the last position inside. */
  nativeSimulateMouseExit(x: number, y: number): void {
    this.native.flushIfDirty()
    this.native.simulateMouseExit(x, y)
    this.dispatchNativeEvents()
    this.native.flushIfDirty()
  }

  /** The OS ate the release of `button` (0=left, the default, 1=middle, 2=right): the
   *  renderer forgets it as held and dispatches nothing. The next move reports no button,
   *  which is how a page sees a release it never got (the pointer let go outside the
   *  window), and the press is cancelled. */
  simulateLostMouseUp(button?: number): void {
    this.native.simulateLostMouseUp(button)
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
    this.native.flushIfDirty()
    this.native.simulateMouseDown(x, y, button ?? 0, modifiers, clickCount)
    this.dispatchNativeEvents()
    this.native.flushIfDirty()
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
    this.native.flushIfDirty()
    this.native.simulateMouseUp(x, y, button ?? 0, modifiers, clickCount)
    this.dispatchNativeEvents()
    this.native.flushIfDirty()
  }

  /**
   * End-to-end: files from the OS enter the window at `x`, `y`. Returns the operation the
   * window answered the OS drag with, as macOS would see it: the `dropEffect` the `dragover`
   * ended with (`"none"` when no handler cancelled it), or `null` when native picked none.
   */
  nativeSimulateFileDragEnter(x: number, y: number, paths: string[]): HostDropEffect | null {
    this.native.flushIfDirty()
    const operation = this.native.simulateFileDragEnter(x, y, paths)
    this.dispatchNativeEvents()
    this.native.flushIfDirty()
    return operation as HostDropEffect | null
  }

  /** End-to-end: the dragged files move to `x`, `y`. Returns the operation, as `nativeSimulateFileDragEnter` does. */
  nativeSimulateFileDragMove(x: number, y: number): HostDropEffect | null {
    this.native.flushIfDirty()
    const operation = this.native.simulateFileDragMove(x, y)
    this.dispatchNativeEvents()
    this.native.flushIfDirty()
    return operation as HostDropEffect | null
  }

  /** End-to-end: the dragged files are released at `x`, `y`. */
  nativeSimulateFileDragSubmit(x: number, y: number): void {
    this.native.flushIfDirty()
    this.native.simulateFileDragSubmit(x, y)
    this.dispatchNativeEvents()
    this.native.flushIfDirty()
  }

  /** End-to-end: the dragged files leave the window. */
  nativeSimulateFileDragExit(): void {
    this.native.flushIfDirty()
    this.native.simulateFileDragExit()
    this.dispatchNativeEvents()
    this.native.flushIfDirty()
  }

  /** End-to-end: the platform's drag session ends. */
  nativeSimulateFileDragEnd(): void {
    this.native.flushIfDirty()
    this.native.simulateFileDragEnd()
    this.dispatchNativeEvents()
    this.native.flushIfDirty()
  }

  /** End-to-end: a whole Finder-style drop at `x`, `y`, as the platform sends it: enter, submit, exit, end. */
  nativeSimulateFileDrop(x: number, y: number, paths: string[]): void {
    this.native.flushIfDirty()
    this.native.simulateFileDrop(x, y, paths)
    this.dispatchNativeEvents()
    this.native.flushIfDirty()
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

  /** What the element build has done since the last `resetBuildCounters()`:
   *  elements built and `StyleDesc` / `Inherited` clones. A draw builds every
   *  element, so divide by the frames drawn for the per-frame figure. */
  getBuildCounters(): BuildCounters {
    return this.native.getBuildCounters()
  }

  /** What the retained subtrees did since the window was made: subtrees not built, replayed,
   *  built late, and frames compared with a whole build under `setRetainVerify`. */
  getRetainedCounters(): RetainedCounters {
    return this.native.getRetainedCounters()
  }

  /** What building and drawing the elements did since the window was made: elements prepainted and
   *  painted (those in a culled subtree apart), and the scene primitives painting inserted against
   *  the ones a replay inserted again. A replayed subtree prepaints and paints none of its
   *  elements; its replays are `getRetainedCounters().replays`. */
  getFrameWorkCounters(): FrameWorkCounters {
    return this.native.getFrameWorkCounters()
  }

  /** How the frames compared under `setRetainVerify` differed from a whole build. */
  getRetentionMismatches(): string[] {
    return this.native.getRetentionMismatches()
  }

  /** Layout nodes made, found again and freed, styles and child lists set, text measured and
   *  shaped, and the subtrees whose extent and overflow were walked again, by the window's layout
   *  engine since the window was made. */
  getLayoutCounters(): LayoutCounters {
    return this.native.getLayoutCounters()
  }

  resetBuildCounters(): void {
    this.native.resetBuildCounters()
  }

  /** Hashes what every element builds, and counts in `retainMismatches` an element the
   *  retain key judged unchanged that built differently since the last build. Costs a
   *  formatted style per element: leave it off to time a frame. */
  setRetainVerify(verify: boolean): void {
    this.native.setRetainVerify(verify)
  }

  /** Turns the skipping of unchanged subtrees on or off (on by default): off, every frame
   *  builds every element, the way `GPUIX_RETAIN_OFF` makes it for the whole run. */
  setRetainPlanning(planning: boolean): void {
    this.native.setRetainPlanning(planning)
  }

  /** Whether a frame records what it builds at all (on by default): off, a frame costs what it did
   *  before retention, as `GPUIX_RETAIN_OFF` makes it for the whole run. Turning it on starts
   *  from a frame that builds everything. */
  setRetainEnabled(enabled: boolean): void {
    this.native.setRetainEnabled(enabled)
  }

  /** Whether a tick of animations that only change what they paint is drawn by painting the roots
   *  they change again in the frame before it (on by default, as `GPUIX_PATCH_OFF` turns off for the
   *  whole run). It is never patched while the retention is off. */
  setPatchEnabled(enabled: boolean): void {
    this.native.setPatchEnabled(enabled)
  }

  /** How ticks were drawn by patching since the window was made: the frames and roots patched, the
   *  ticks gpui drew whole after the patch was tried (`declines`, by reason, as JSON) and the ones
   *  gpuix did not try to patch (`unpatchable`, by kind, as JSON). */
  getPatchCounters(): PatchCounters {
    return this.native.getPatchCounters()
  }

  /** Counts as if these exclusions (`svg`, `overflow-hidden`, `custom-element`, ...) were lifted,
   *  to see which are worth lifting first. Nothing is reused. */
  setRetainLift(names: string[]): void {
    this.native.setRetainLift(names)
  }

  /** The elements counted in `retainMismatches`, one line each. */
  getRetainMismatches(): string[] {
    return this.native.getRetainMismatches()
  }

  /** Frames the window has painted. A test compares it before and after to
   *  count the draws a read caused: a bounds or scroll read paints nothing. */
  getPaintedFrameCount(): number {
    return this.native.getPaintedFrameCount()
  }

  /** Draw a dirty window when an update or a simulated input flushes (the
   *  default), or leave that to `drawFrame`, as a real window leaves it to its
   *  display link. A test that counts draws per frame turns it off. */
  setInlineDraws(enabled: boolean): void {
    this.native.setInlineDraws(enabled)
  }

  /** One display-link frame: deliver the queued frame callbacks, then draw a
   *  pending change once. Pairs with `setInlineDraws(false)`. */
  drawFrame(): void {
    this.native.drawFrame()
  }

  /** Layout-only passes the window has run. A read of bounds or scroll metrics
   *  on a dirty window is one, in place of a draw, and the next frame draws
   *  once regardless. */
  getLayoutPassCount(): number {
    return this.native.getLayoutPassCount()
  }

  /** Pretends a screen reader is connected, as the test window starts out,
   *  or not. Without one no accessibility tree is built. */
  setA11yActive(active: boolean): void {
    this.native.setA11yActive(active)
  }

  /** Subtrees the window has skipped as lying outside their clip, over its
   *  life. Compare before and after a frame. */
  getCulledSubtreeCount(): number {
    return this.native.getCulledSubtreeCount()
  }

  getElementBounds(elementId: number): ElementBounds | null {
    flushPendingMutations(this)
    return this.native.getElementBounds(elementId)
  }

  /** `getElementBounds` for the first report of an observer: it lays out as that does, but not the content
   *  of a skipped `content-visibility` ancestor, and an element inside it has no box (null). */
  probeElementBounds(elementId: number): ElementBounds | null {
    flushPendingMutations(this)
    return this.native.probeElementBounds(elementId)
  }

  /** The box the last layout gave an element, or null if it was not built. Unlike `getElementBounds`
   *  it does not lay out the content of a skipped `content-visibility` ancestor to answer, so it
   *  tells whether that content is built. */
  getBuiltElementBounds(elementId: number): ElementBounds | null {
    flushPendingMutations(this)
    return this.native.getBuiltElementBounds(elementId)
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

  /** `clockFastForward` without telling the window, as time passing does in a live one: the frame
   *  after it is a tick, which patches the frame before it when only paint-only animations ran. */
  clockAdvance(deltaMs: number): number {
    return this.native.clockAdvance(deltaMs)
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

  pauseMotionValue(id: number): void {
    this.native.pauseMotionValue(id)
  }

  playMotionValue(id: number): void {
    this.native.playMotionValue(id)
  }

  seekMotionValue(id: number, seconds: number): void {
    this.native.seekMotionValue(id, seconds)
  }

  setMotionValueSpeed(id: number, speed: number): void {
    this.native.setMotionValueSpeed(id, speed)
  }

  completeMotionValue(id: number): void {
    this.native.completeMotionValue(id)
  }

  detachMotionValueClock(id: number): void {
    this.native.detachMotionValueClock(id)
  }

  getMotionValuePlayback(id: number): MotionValuePlayback | null {
    return this.native.getMotionValuePlayback(id)
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
    this.native.flushIfDirty()
    this.native.focusElement(elementId)
    this.dispatchNativeEvents()
  }

  blurElement(elementId: number): void {
    this.native.flushIfDirty()
    this.native.blurElement(elementId)
    this.dispatchNativeEvents()
  }

  matchesFocus(elementId: number, selector: string): boolean {
    this.native.flushIfDirty()
    const matches = this.native.matchesFocus(elementId, selector)
    this.dispatchNativeEvents()
    return matches
  }

  getFocusedElementId(): number | null {
    this.native.flushIfDirty()
    return this.native.getFocusedElementId()
  }

  /** Makes the offscreen window key (`true`, the default) or not, as when the user switches apps. */
  setWindowActive(active: boolean): void {
    this.native.setWindowActive(active)
    this.native.flushIfDirty()
    this.dispatchNativeEvents()
  }

  focusNextWithin(elementId: number): void {
    this.native.flushIfDirty()
    this.native.focusNextWithin(elementId)
    this.dispatchNativeEvents()
  }

  focusPreviousWithin(elementId: number): void {
    this.native.flushIfDirty()
    this.native.focusPreviousWithin(elementId)
    this.dispatchNativeEvents()
  }

  /** The offscreen window size, so `useWindowSize()` works under test. */
  getWindowSize(): { width: number; height: number } {
    return this.native.getWindowSize()
  }

  /** `"system"` | `"light"` | `"dark"`, as on the live renderer: the window's appearance
   *  follows (`getAppearance`, `light-dark()`, `prefers-color-scheme`) and `appearanceChange`
   *  is delivered. `"system"` returns to the platform's own. */
  setAppearance(appearance: string): void {
    this.native.flush()
    this.native.setAppearance(appearance)
    this.native.flush()
    this.dispatchNativeEvents()
  }

  /** `"overlay"` (macOS "Show scroll bars" Automatically or When scrolling, which the test
   *  window starts with) or `"classic"` (Always, which macOS also picks while a mouse is
   *  attached). Classic bars are always drawn and take 15px (11 with `scrollbarWidth: "thin"`)
   *  out of the container, so its `clientWidth` and content box shrink by it. The test window
   *  never hears from the OS, so this is how a test flips the setting. */
  setScrollbarStyle(style: "overlay" | "classic"): void {
    this.native.flush()
    this.native.setScrollbarStyle(style)
    this.native.flush()
  }

  /** The window's title, as the platform window has it: the `title` option, or `"GPUIX"`. */
  getWindowTitle(): string {
    this.native.flush()
    return this.native.getWindowTitle()
  }

  /** The window's effective appearance, `"light"` or `"dark"`. */
  getAppearance(): string {
    this.native.flush()
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

  /** Resize the offscreen window's content, as dragging its edge would: `getWindowSize()`
   *  and the size queries of `matchMedia` see the new size, and their `change` events are
   *  delivered. */
  setWindowSize(width: number, height: number): void {
    this.native.setWindowSize(width, height)
    this.dispatchNativeEvents()
  }

  /** Give the offscreen window the scale factor of a display of another density (`2` is a
   *  retina one), as a window dragged to it has: the next frame lays out and paints at it, and
   *  a screenshot is the window's size times it. The factor stays until it is set again:
   *  resizing the window does not reset it. The window starts at 1 (`scaleFactor` option). */
  setWindowScaleFactor(scaleFactor: number): void {
    this.native.setWindowScaleFactor(scaleFactor)
  }

  // ── Scroll API ──────────────────────────────────────────────────

  /** Scroll an element (overflow: "scroll", or a `<virtual-list>`) to a DOM scroll
   *  position: `left` and `top` are `scrollLeft` and `scrollTop`, zero or more, so
   *  scrolling down is a larger `top`. Like a browser's, the write forces a layout only if
   *  one is pending, and the window draws the new offset with its next frame. */
  scrollTo(elementId: number, left: number, top: number, behavior?: string): void {
    this.native.scrollTo(elementId, left, top, behavior)
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

  getAnimations(elementId: number): WebAnimationInfo[] {
    return this.native.getAnimations(elementId)
  }

  getSubtreeAnimations(elementId: number): ElementWebAnimationInfo[] {
    return this.native.getSubtreeAnimations(elementId)
  }

  getDocumentAnimations(): ElementWebAnimationInfo[] {
    return this.native.getDocumentAnimations()
  }

  /**
   * The XML the `<svg>` painted from its shape children in the last frame, with
   * animated attributes as that frame had them, or `null` when it painted none.
   */
  getSvgMarkup(elementId: number): string | null {
    return this.native.getSvgMarkup(elementId)
  }

  getAnimation(animationId: number): WebAnimationInfo {
    return this.native.getAnimation(animationId)
  }

  releaseAnimation(animationId: number): void {
    this.native.releaseAnimation(animationId)
  }

  forgetAnimation(animationId: number): void {
    this.native.forgetAnimation(animationId)
  }

  getAnimationEffect(animationId: number): WebAnimationEffectInfo {
    return this.native.getAnimationEffect(animationId)
  }

  getAnimationKeyframes(animationId: number): WebAnimationKeyframeInfo[] {
    return this.native.getAnimationKeyframes(animationId)
  }

  updateAnimationTiming(animationId: number, timing: WebAnimationTimingUpdate): void {
    this.native.updateAnimationTiming(animationId, timing)
  }

  pauseAnimation(animationId: number): void {
    this.native.pauseAnimation(animationId)
  }

  playAnimation(animationId: number): void {
    this.native.playAnimation(animationId)
  }

  setAnimationCurrentTime(animationId: number, milliseconds: number): void {
    this.native.setAnimationCurrentTime(animationId, milliseconds)
  }

  setAnimationPlaybackRate(animationId: number, rate: number): void {
    this.native.setAnimationPlaybackRate(animationId, rate)
  }

  cancelAnimation(animationId: number): void {
    this.native.cancelAnimation(animationId)
  }

  finishAnimation(animationId: number): void {
    this.native.finishAnimation(animationId)
  }

  reverseAnimation(animationId: number): void {
    this.native.reverseAnimation(animationId)
  }

  updateAnimationPlaybackRate(animationId: number, rate: number): void {
    this.native.updateAnimationPlaybackRate(animationId, rate)
  }

  setAnimationStartTime(animationId: number, startTime: number | null): void {
    this.native.setAnimationStartTime(animationId, startTime)
  }

  getAnimationTimelineTime(): number {
    return this.native.getAnimationTimelineTime()
  }

  /** Applies from the next simulated pointer input. */
  cancelDrag(): void {
    this.native.cancelDrag()
  }

  /** The frame it asks for is drawn here, so `pointerlockchange` or
   *  `pointerlockerror` has reached its listeners when this returns. */
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
    return this.native.getScrollMetrics(elementId)
  }

  setImage(elementId: number, bytes: Buffer | Uint8Array): void {
    this.native.flush()
    this.native.setImage(elementId, bytes as Buffer)
    this.native.flush()
  }

  setStyleImage(url: string, bytes: Buffer | Uint8Array): void {
    this.native.flush()
    this.native.setStyleImage(url, bytes as Buffer)
    this.native.flush()
  }

  failStyleImage(url: string, message: string): void {
    this.native.flush()
    this.native.failStyleImage(url, message)
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
    this.native.flushIfDirty()
    this.native.requestSubmit(elementId)
    this.dispatchNativeEvents()
    this.native.flushIfDirty()
  }

  registerFont(bytes: Buffer | Uint8Array): void {
    this.native.registerFont(bytes as Buffer)
    this.native.flush()
  }

  // ── <webview> ───────────────────────────────────────────────────
  // The page of a webview is made when the element is first painted, so these draw first.
  // The engine is a scripted page: it answers from what these set, and reports what they make it.

  webviewCommand(elementId: number, command: string, argument?: string): void {
    this.native.flush()
    this.native.webviewCommand(elementId, command, argument)
  }

  webviewState(elementId: number): {
    url: string
    title: string
    canGoBack: boolean
    canGoForward: boolean
  } {
    this.native.flush()
    return this.native.webviewState(elementId)
  }

  webviewExecuteJavascript(elementId: number, code: string): Promise<string | null> {
    this.native.flush()
    return this.native.webviewExecuteJavascript(elementId, code)
  }

  /** What the page of the `<webview>` was asked to do, in order. */
  getWebviewCommands(elementId: number): WebviewCommand[] {
    this.native.flush()
    return this.native.getWebviewCommands(elementId)
  }

  /** The page starts loading: `did-start-loading`. */
  simulateWebviewStartLoading(elementId: number): void {
    this.native.flush()
    this.native.simulateWebviewStartLoading(elementId)
    this.dispatchNativeEvents()
  }

  /** The page stops loading: `did-stop-loading`. */
  simulateWebviewStopLoading(elementId: number): void {
    this.native.flush()
    this.native.simulateWebviewStopLoading(elementId)
    this.dispatchNativeEvents()
  }

  /** The page commits `url`: `did-navigate`, and `getURL()` answers it. */
  simulateWebviewNavigate(elementId: number, url: string): void {
    this.native.flush()
    this.native.simulateWebviewNavigate(elementId, url)
    this.dispatchNativeEvents()
  }

  /** The page hands the keyboard on, as it does when Tab leaves its last tabbable element
   *  (`backwards`: Shift+Tab leaves its first): focus moves to the next (previous) tab stop. */
  simulateWebviewTabOut(elementId: number, backwards = false): void {
    this.native.flush()
    this.native.simulateWebviewTabOut(elementId, backwards)
    this.dispatchNativeEvents()
  }

  /** The page gets a title: `page-title-updated`, and `getTitle()` answers it. */
  simulateWebviewTitle(elementId: number, title: string, explicitSet = true): void {
    this.native.flush()
    this.native.simulateWebviewTitle(elementId, title, explicitSet)
    this.dispatchNativeEvents()
  }

  /** A load fails: `did-fail-load`, with an `NSURLError` code. */
  simulateWebviewFail(
    elementId: number,
    errorCode: number,
    errorDescription: string,
    validatedUrl?: string
  ): void {
    this.native.flush()
    this.native.simulateWebviewFail(elementId, errorCode, errorDescription, validatedUrl)
    this.dispatchNativeEvents()
  }

  /** What `canGoBack()` and `canGoForward()` answer. */
  setWebviewHistory(elementId: number, canGoBack: boolean, canGoForward: boolean): void {
    this.native.flush()
    this.native.setWebviewHistory(elementId, canGoBack, canGoForward)
  }

  /** Settles the oldest `executeJavaScript` of the `<webview>` that waits: resolves it with
   *  `result` (anything JSON holds; `undefined` for `undefined`), or rejects it with `error`. */
  settleWebviewScript(
    elementId: number,
    outcome: { result?: unknown; error?: string }
  ): void {
    this.native.flush()
    this.native.settleWebviewScript(
      elementId,
      outcome.result === undefined ? undefined : JSON.stringify(outcome.result),
      outcome.error
    )
  }

  /** Where the last frame put the surface of the `<webview>`; null while it has none. */
  getWebviewSurface(elementId: number): WebviewSurface | null {
    this.native.flush()
    return this.native.getWebviewSurface(elementId)
  }

  /** The layers of the last composed frame, bottom to top. */
  getCompositionLayers(): CompositionLayer[] {
    this.native.flush()
    return this.native.getCompositionLayers()
  }

  /** The views AppKit holds for the `<webview>`, with the placement the last frame applied. */
  getWebviewNativeView(elementId: number): WebviewNativeView {
    this.native.flush()
    return this.native.getWebviewNativeView(elementId)
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

  /** The DOM scroll position `[scrollLeft, scrollTop]` (zero or more, like `element.scrollLeft`
   *  and the `onScroll` event), or null if the element is not scrollable. */
  getScrollOffset(elementId: number): [number, number] | null {
    this.native.flush()
    const result = this.native.getScrollOffset(elementId)
    if (!result) return null
    return [result[0], result[1]]
  }

  /** The CSS keyword of the cursor the window asks the platform for where the pointer last
   *  was (`"default"` when nothing asks). A mouse move updates it, a draw keeps it current.
   *  Keywords that name one cursor read back as the first of them: `"pointer"`, `"grabbing"`,
   *  `"nwse-resize"`, `"nesw-resize"`, `"not-allowed"`. */
  getCursor(): string {
    this.native.flush()
    return this.native.getCursor()
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
