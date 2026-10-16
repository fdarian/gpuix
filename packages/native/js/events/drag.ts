import type { EventPayload } from "../../index.js"
import {
  allowsDropEffect,
  createDataTransfer,
  defaultDropEffect,
  type DataTransferMode,
  type DataTransferSession,
} from "./data-transfer.js"
import { dispatchDomEvent, type DomEventRoot } from "./dispatch.js"
import { hasDomListener } from "./route.js"
import type { HostDropEffect } from "./types.js"

const DRAG_TYPES: ReadonlySet<string> = new Set([
  "dragstart",
  "drag",
  "dragend",
  "dragenter",
  "dragover",
  "dragleave",
  "drop",
])

export function isDragEvent(event: EventPayload): boolean {
  return DRAG_TYPES.has(event.eventType)
}

const MODE_BY_TYPE: Record<string, DataTransferMode> = {
  dragstart: "readwrite",
  drop: "readonly",
}

interface Session {
  /** A drag of files ends where it leaves the window; a drag of an element at `dragend`. */
  readonly kind: "files" | "element"
  readonly data: DataTransferSession
  /** Whether the last `dragover` was cancelled with an effect the source allows. */
  accepted: boolean
  /** The effect the accepted `dragover` chose. */
  effect: HostDropEffect
  /** Whether a `drop` was delivered. */
  dropped: boolean
}

function startSession(kind: Session["kind"], data: DataTransferSession): Session {
  return { kind, data, accepted: false, effect: "none", dropped: false }
}

/**
 * Runs the events of drags, one drag at a time, both kinds: files from the OS
 * and an element with `draggable`.
 *
 * Native sends every drag event whether or not anything listens, because the DOM
 * delivers a drop only to a target whose last `dragover` was cancelled, and native
 * cannot wait for JS to say so. A drop that nothing accepted is delivered as the
 * `dragleave` the DOM fires in its place. `cancelDrag` tells native that the
 * handler of `dragstart` cancelled the drag; native may already have sent a
 * move or two, which are dropped here.
 *
 * A `dragover` returns the drop effect it ended with, `"none"` unless a handler
 * cancelled it with an effect the source allows. For a drag of files native
 * answers the OS with it.
 */
export function createDragDispatcher(
  cancelDrag: () => void
): (root: DomEventRoot, event: EventPayload) => HostDropEffect | undefined {
  let session: Session | undefined
  let cancelled = false

  return (root, event) => {
    // A drag of files starts with its files; a drag of an element with dragstart.
    if (event.eventType === "dragstart") {
      session = startSession("element", createDataTransfer())
      cancelled = false
    } else if (event.files) {
      session = startSession("files", createDataTransfer(event.files))
      cancelled = false
    }
    if (cancelled) return
    if (!session) {
      throw new Error(`A "${event.eventType}" drag event arrived before the drag started.`)
    }
    const current = session
    const data = current.data.dataTransfer

    // Leaving the window has no related target; moving to another element does.
    const endsDrag =
      current.kind === "element"
        ? event.eventType === "dragend"
        : event.eventType === "drop" ||
          (event.eventType === "dragleave" && event.relatedTarget == null)

    let eventType = event.eventType
    // What the last target accepted says nothing of the one entered now: it
    // must cancel a dragover of its own before a drop reaches it.
    if (eventType === "dragenter") {
      current.accepted = false
      current.effect = "none"
    }
    if (eventType === "drop" && !current.accepted) eventType = "dragleave"
    if (eventType === "dragenter" || eventType === "dragover") {
      data.dropEffect = defaultDropEffect(data.effectAllowed)
    } else if (eventType === "drop") {
      data.dropEffect = current.effect
    } else if (eventType === "dragend") {
      data.dropEffect = current.dropped ? current.effect : "none"
    }
    current.data.mode = MODE_BY_TYPE[eventType] ?? "protected"

    const dispatched = { ...event, eventType, dataTransfer: data }
    let defaultPrevented = false
    try {
      if (hasDomListener(root.eventHandlers, dispatched)) {
        defaultPrevented = dispatchDomEvent(root, dispatched).defaultPrevented
      }
    } finally {
      current.data.mode = "protected"
      if (eventType === "dragover") {
        current.accepted =
          defaultPrevented &&
          data.dropEffect !== "none" &&
          allowsDropEffect(data.effectAllowed, data.dropEffect)
        current.effect = current.accepted ? data.dropEffect : "none"
      }
      if (eventType === "drop") current.dropped = true
      if (endsDrag) session = undefined
    }

    if (event.eventType === "dragstart" && defaultPrevented) {
      session = undefined
      cancelled = true
      cancelDrag()
    }
    return event.eventType === "dragover" ? current.effect : undefined
  }
}
