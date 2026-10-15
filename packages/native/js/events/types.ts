import type { EventPayload } from "../../index.js"

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

export type HostDataTransfer = {
  dropEffect: HostDropEffect
  effectAllowed: string
  readonly files: readonly HostFile[]
  readonly types: readonly string[]
  getData(format: string): string
  setData(format: string, data: string): void
  clearData(format?: string): void
}

/**
 * What every dispatched event carries, like the DOM `Event`. Ids are element
 * ids, not objects: `target` is the element the event was aimed at and
 * `currentTarget` the one whose handler is running.
 */
export interface HostDomEvent extends EventPayload {
  readonly type: string
  readonly target: number
  readonly currentTarget: number | null
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
  movementX?: number
  movementY?: number
  /** 1 for the mouse, -1 for a click no pointer made (VoiceOver, Enter or Space on a button). */
  pointerId: number
  /** `""` for a click no pointer made. */
  pointerType: "mouse" | "pen" | "touch" | ""
}

export interface HostPointerEvent extends HostMouseEvent {}

export interface HostDragEvent extends HostMouseEvent {
  dataTransfer?: HostDataTransfer
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
  onMouseMove?: (event: HostMouseEvent) => void
  /** Does not bubble: fires on each element the pointer enters. */
  onMouseEnter?: (event: HostMouseEvent) => void
  /** Does not bubble: fires on each element the pointer leaves. */
  onMouseLeave?: (event: HostMouseEvent) => void
  onMouseOver?: (event: HostMouseEvent) => void
  onMouseOut?: (event: HostMouseEvent) => void
  onPointerDown?: (event: HostPointerEvent) => void
  onPointerMove?: (event: HostPointerEvent) => void
  onPointerUp?: (event: HostPointerEvent) => void
  onPointerEnter?: (event: HostPointerEvent) => void
  onPointerLeave?: (event: HostPointerEvent) => void
  onPointerOver?: (event: HostPointerEvent) => void
  onPointerOut?: (event: HostPointerEvent) => void
  onPointerCancel?: (event: HostPointerEvent) => void
  onGotPointerCapture?: (event: HostPointerEvent) => void
  onLostPointerCapture?: (event: HostPointerEvent) => void
  onDragStart?: (event: HostDragEvent) => void
  onDrag?: (event: HostDragEvent) => void
  onDragEnd?: (event: HostDragEvent) => void
  onDragEnter?: (event: HostDragEvent) => void
  onDragOver?: (event: HostDragEvent) => void
  onDragLeave?: (event: HostDragEvent) => void
  onDrop?: (event: HostDragEvent) => void
}

type CaptureProps<Props> = {
  [Name in keyof Props & string as `${Name}Capture`]: Props[Name]
}

/** Each pointer, mouse and drag handler, plus its capture-phase `on…Capture` twin. */
export type PointerEventProps = PointerEventHandlerProps &
  Partial<CaptureProps<PointerEventHandlerProps>>
