import { captureHandlerKey } from "./dispatch.js"
import type { AnimationEventHandlerProps, PointerEventHandlerProps } from "./types.js"

/**
 * Pointer, mouse, drag and wheel handler props and the DOM event type each one
 * listens for. Types are the lowercase DOM names.
 */
const POINTER_EVENT_PROPS = [
  ["onClick", "click"],
  ["onAuxClick", "auxclick"],
  ["onDoubleClick", "dblclick"],
  ["onContextMenu", "contextmenu"],
  ["onMouseDown", "mousedown"],
  ["onMouseUp", "mouseup"],
  ["onMouseMove", "mousemove"],
  ["onMouseEnter", "mouseenter"],
  ["onMouseLeave", "mouseleave"],
  ["onMouseOver", "mouseover"],
  ["onMouseOut", "mouseout"],
  ["onPointerDown", "pointerdown"],
  ["onPointerMove", "pointermove"],
  ["onPointerUp", "pointerup"],
  ["onPointerEnter", "pointerenter"],
  ["onPointerLeave", "pointerleave"],
  ["onPointerOver", "pointerover"],
  ["onPointerOut", "pointerout"],
  ["onPointerCancel", "pointercancel"],
  ["onGotPointerCapture", "gotpointercapture"],
  ["onLostPointerCapture", "lostpointercapture"],
  ["onDragStart", "dragstart"],
  ["onDrag", "drag"],
  ["onDragEnd", "dragend"],
  ["onDragEnter", "dragenter"],
  ["onDragOver", "dragover"],
  ["onDragLeave", "dragleave"],
  ["onDrop", "drop"],
  ["onWheel", "wheel"],
] as const satisfies readonly (readonly [keyof PointerEventHandlerProps, string])[]

// Fails to compile when PointerEventHandlerProps declares a prop the table lacks.
type MissingFromTable = Exclude<
  keyof PointerEventHandlerProps,
  (typeof POINTER_EVENT_PROPS)[number][0]
>
const tableCoversEveryProp: [MissingFromTable] extends [never] ? true : never = true
void tableCoversEveryProp

/** CSS transition and animation events. They bubble and cannot be cancelled. */
const ANIMATION_EVENT_PROPS = [
  ["onTransitionRun", "transitionrun"],
  ["onTransitionStart", "transitionstart"],
  ["onTransitionEnd", "transitionend"],
  ["onTransitionCancel", "transitioncancel"],
  ["onAnimationStart", "animationstart"],
  ["onAnimationIteration", "animationiteration"],
  ["onAnimationEnd", "animationend"],
  ["onAnimationCancel", "animationcancel"],
] as const satisfies readonly (readonly [keyof AnimationEventHandlerProps, string])[]

type MissingFromAnimationTable = Exclude<
  keyof AnimationEventHandlerProps,
  (typeof ANIMATION_EVENT_PROPS)[number][0]
>
const animationTableCoversEveryProp: [MissingFromAnimationTable] extends [never] ? true : never = true
void animationTableCoversEveryProp

/** The pointer, mouse, drag and wheel types, lowercase. */
export const POINTER_EVENT_TYPES: ReadonlySet<string> = new Set(
  POINTER_EVENT_PROPS.map(([, eventType]) => eventType)
)

/**
 * The DOM event types that run through `dispatchDomEvent`: native emits them
 * with a propagation path and they are named by their lowercase DOM type.
 */
export const DOM_DISPATCHED_EVENT_TYPES: ReadonlySet<string> = new Set([
  ...POINTER_EVENT_TYPES,
  ...ANIMATION_EVENT_PROPS.map(([, eventType]) => eventType),
])

/**
 * Prop name to handler key, with a capture-phase `on…Capture` entry for each.
 * A capture handler is keyed `captureHandlerKey(type)`; both phases share one
 * native listener on `type`.
 */
export const DOM_DISPATCHED_HANDLER_PROPS: readonly (readonly [string, string])[] = [
  ...POINTER_EVENT_PROPS,
  ...ANIMATION_EVENT_PROPS,
].flatMap(([propName, eventType]) => [
  [propName, eventType] as const,
  [`${propName}Capture`, captureHandlerKey(eventType)] as const,
])
