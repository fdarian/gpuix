import type { RootEvent } from "./dispatch.js"

/** One entry of `DataTransfer.files`. Filled in by native on file drags. */
export type HostFile = {
  path: string
  name: string
  size: number
  /** MIME type guessed from the extension, empty when unknown. */
  type: string
  lastModified: number
}

export type HostDropEffect = "none" | "copy" | "link" | "move"

export type HostEffectAllowed =
  | "none"
  | "copy"
  | "copyLink"
  | "copyMove"
  | "link"
  | "linkMove"
  | "move"
  | "all"
  | "uninitialized"

/**
 * The data of one drag, shared by every event of it. DOM protected mode
 * applies: `getData` returns `""` and `setData` does nothing except in
 * `dragstart` (where both work) and `drop` (where only `getData` does).
 */
export type HostDataTransfer = {
  /**
   * What the drop will do. A `dragover` handler picks it; it must be allowed
   * by `effectAllowed` and not `"none"` for the drop to be delivered. In
   * `dragend` it is what the drop did, `"none"` when nothing was dropped.
   */
  dropEffect: HostDropEffect
  /** What the source allows. Writable in `dragstart` only. */
  effectAllowed: HostEffectAllowed
  readonly files: readonly HostFile[]
  readonly types: readonly string[]
  getData(format: string): string
  setData(format: string, data: string): void
  clearData(format?: string): void
  /** Accepted and ignored: there is no drag image. */
  setDragImage(element: unknown, x: number, y: number): void
}

/**
 * What every dispatched event carries, like the DOM `Event`. `target` is the
 * element the event was aimed at and `currentTarget` the one whose handler is
 * running (the `window` or `document` object during their phases). Both are
 * the object a `ref` gives for the element, `relatedTarget` too; use
 * `elementId` for the numeric id native knows it by.
 */
export interface HostDomEvent extends RootEvent {
  readonly type: string
  /** Never `null` for an element's handler: only `window` and `document` hear an event with no element. */
  readonly target: object
  readonly currentTarget: object | null
  /** The elements on the propagation path, target first. */
  composedPath(): object[]
  /** 1 capturing, 2 at target, 3 bubbling. */
  readonly eventPhase: number
  readonly bubbles: boolean
  readonly cancelable: boolean
  readonly timeStamp: number
  readonly defaultPrevented: boolean
  stopPropagation(): void
  stopImmediatePropagation(): void
  preventDefault(): void
}

/**
 * Coordinates and buttons of a mouse event. `clientX`/`clientY` are window
 * coordinates; `offsetX`/`offsetY` are relative to the target's padding edge.
 * Native fills in the coordinates as the pointer model lands, so each can be
 * absent. `pointerId` and `pointerType` are always there, mouse event or not,
 * as on a DOM `PointerEvent`.
 */
export interface HostMouseEvent extends HostDomEvent {
  clientX?: number
  clientY?: number
  offsetX?: number
  offsetY?: number
  /** Buttons held right now, as a bitmask: 1 primary, 2 secondary, 4 auxiliary. */
  buttons?: number
  /** Consecutive clicks: 1 single, 2 double. */
  detail?: number
  /** 1 for the mouse, -1 for a click no pointer made (VoiceOver, Enter or Space on a button). */
  pointerId: number
  /** `""` for a click no pointer made. */
  pointerType: "mouse" | "pen" | "touch" | ""
}

export interface HostPointerEvent extends HostMouseEvent {}

/**
 * `mousemove`. `movementX`/`movementY` are how far the pointer moved since the
 * previous move, in logical pixels: the real motion while the pointer is locked
 * (`requestPointerLock()`), when `clientX`/`clientY` stand still. `0` on a
 * platform that reports no relative motion (everything but macOS).
 */
export interface HostMouseMoveEvent extends HostMouseEvent {
  movementX: number
  movementY: number
}

/** `pointermove`. See `HostMouseMoveEvent`. */
export interface HostPointerMoveEvent extends HostPointerEvent {
  movementX: number
  movementY: number
}

/**
 * A drag fires `dragenter`, `dragover`, `dragleave` and `drop` at what is under
 * the pointer, and all carry the drag's one `dataTransfer`. `drop` is delivered
 * only after a `dragover` whose handler called `preventDefault()`.
 *
 * Files dragged from the OS fire only those four. Dragging an element that has
 * `draggable` adds `dragstart`, `drag` and `dragend` at the dragged element,
 * the source. No pointer or mouse event fires during either kind of drag.
 */
export interface HostDragEvent extends HostMouseEvent {
  dataTransfer?: HostDataTransfer
}

/**
 * `wheel`: one wheel or trackpad input, aimed at the element under the pointer
 * (pointer capture does not redirect it). It bubbles and `preventDefault()` cancels
 * the scroll that input would cause, but only where native can wait for the handler
 * (macOS and the test renderer).
 *
 * `deltaX` and `deltaY` are pixels, positive for down and right, like the DOM.
 * `deltaMode` is always `0`: a mouse wheel's lines are converted to pixels natively
 * at 20 px a line. `precise` says a trackpad sent it, and `touchPhase` (`"started"`,
 * `"moved"`, `"ended"` or `"cancelled"`) where it sits in a gesture. It fires
 * whether or not anything scrolls.
 */
export interface HostWheelEvent extends HostMouseEvent {
  deltaX: number
  deltaY: number
  deltaMode: 0
  precise: boolean
  touchPhase: "started" | "moved" | "ended" | "cancelled"
}

export interface PointerEventHandlerProps {
  /** Primary button only, like the DOM. Use `onAuxClick` for the others. */
  onClick?: (event: HostPointerEvent) => void
  /** Non-primary click. */
  onAuxClick?: (event: HostPointerEvent) => void
  onDoubleClick?: (event: HostMouseEvent) => void
  onContextMenu?: (event: HostPointerEvent) => void
  onMouseDown?: (event: HostMouseEvent) => void
  onMouseUp?: (event: HostMouseEvent) => void
  onMouseMove?: (event: HostMouseMoveEvent) => void
  /** Does not bubble: fires on each element the pointer enters. */
  onMouseEnter?: (event: HostMouseEvent) => void
  /** Does not bubble: fires on each element the pointer leaves. */
  onMouseLeave?: (event: HostMouseEvent) => void
  onMouseOver?: (event: HostMouseEvent) => void
  onMouseOut?: (event: HostMouseEvent) => void
  onPointerDown?: (event: HostPointerEvent) => void
  onPointerMove?: (event: HostPointerMoveEvent) => void
  onPointerUp?: (event: HostPointerEvent) => void
  onPointerEnter?: (event: HostPointerEvent) => void
  onPointerLeave?: (event: HostPointerEvent) => void
  onPointerOver?: (event: HostPointerEvent) => void
  onPointerOut?: (event: HostPointerEvent) => void
  onPointerCancel?: (event: HostPointerEvent) => void
  onGotPointerCapture?: (event: HostPointerEvent) => void
  onLostPointerCapture?: (event: HostPointerEvent) => void
  /** On a `draggable` source. `preventDefault()` cancels the drag. */
  onDragStart?: (event: HostDragEvent) => void
  /** On the source, on each move. Only fires when something listens. */
  onDrag?: (event: HostDragEvent) => void
  /** On the source. `dataTransfer.dropEffect` is `"none"` if nothing was dropped. */
  onDragEnd?: (event: HostDragEvent) => void
  onDragEnter?: (event: HostDragEvent) => void
  /** `preventDefault()` makes the element a drop target. */
  onDragOver?: (event: HostDragEvent) => void
  onDragLeave?: (event: HostDragEvent) => void
  onDrop?: (event: HostDragEvent) => void
  onWheel?: (event: HostWheelEvent) => void
}

/**
 * `transitionrun`, `transitionstart`, `transitionend` and `transitioncancel`.
 * They bubble from the transitioning element (`target`) and cannot be cancelled.
 * `propertyName` is the CSS name (`background-color`), `elapsedTime` is in
 * seconds and `pseudoElement` is always `""`.
 */
export interface HostTransitionEvent extends HostDomEvent {
  propertyName: string
  elapsedTime: number
  pseudoElement: ""
}

/**
 * `animationstart`, `animationiteration`, `animationend` and `animationcancel`.
 * They bubble from the animating element (`target`) and cannot be cancelled.
 * `animationName` is the name given to `defineKeyframes`, `elapsedTime` is in
 * seconds and `pseudoElement` is always `""`.
 */
export interface HostAnimationEvent extends HostDomEvent {
  animationName: string
  elapsedTime: number
  pseudoElement: ""
}

export interface AnimationEventHandlerProps {
  /** A transition was created, before its delay. */
  onTransitionRun?: (event: HostTransitionEvent) => void
  /** A transition's delay ended and it started animating. */
  onTransitionStart?: (event: HostTransitionEvent) => void
  /** A transition reached its end value. */
  onTransitionEnd?: (event: HostTransitionEvent) => void
  /** A transition was interrupted: its property changed again, left the
   *  transition list, or became owned by `motion`. */
  onTransitionCancel?: (event: HostTransitionEvent) => void
  /** An animation's delay ended and it started playing. */
  onAnimationStart?: (event: HostAnimationEvent) => void
  /** An animation began another iteration. Not fired for the last one. */
  onAnimationIteration?: (event: HostAnimationEvent) => void
  /** An animation finished its last iteration. */
  onAnimationEnd?: (event: HostAnimationEvent) => void
  /** An animation was interrupted before it ended: its name left
   *  `animation-name` or the name changed. */
  onAnimationCancel?: (event: HostAnimationEvent) => void
}

type CaptureProps<Props> = {
  [Name in keyof Props & string as `${Name}Capture`]: Props[Name]
}

/** Each pointer, mouse and drag handler, plus its capture-phase `on…Capture` twin. */
export type PointerEventProps = PointerEventHandlerProps &
  Partial<CaptureProps<PointerEventHandlerProps>>

/** Each transition and animation handler, plus its capture-phase `on…Capture` twin. */
export type AnimationEventProps = AnimationEventHandlerProps &
  Partial<CaptureProps<AnimationEventHandlerProps>>
