import type { EventPayload } from "../../index.js"
import { dispatchDomEvent, type DomEventRoot } from "./dispatch.js"
import { hasDomListener } from "./route.js"
import type { HostDataTransfer, HostFile } from "./types.js"

const FILE_DRAG_TYPES: ReadonlySet<string> = new Set(["dragenter", "dragover", "dragleave", "drop"])

export function isFileDragEvent(event: EventPayload): boolean {
  return FILE_DRAG_TYPES.has(event.eventType)
}

/**
 * The data of a drag from the OS. Like the DOM's protected mode, a handler
 * cannot put anything in it: `setData` and `clearData` are no-ops and
 * `getData` is empty. `dropEffect` is the one thing it can set.
 */
function createFileDataTransfer(files: readonly HostFile[]): HostDataTransfer {
  return {
    dropEffect: "copy",
    effectAllowed: "all",
    files: Object.freeze([...files]),
    types: Object.freeze(["Files"]),
    getData: () => "",
    setData: () => {},
    clearData: () => {},
  }
}

/**
 * Runs the events of OS file drags, one drag at a time.
 *
 * Native sends every `dragenter`, `dragover`, `dragleave` and `drop` whether
 * or not anything listens, because the DOM delivers a drop only to a target
 * whose last `dragover` was cancelled, and native cannot wait for JS to say
 * so. A drop that nothing accepted is delivered as the `dragleave` the DOM
 * fires in its place.
 */
export function createFileDragDispatcher(): (root: DomEventRoot, event: EventPayload) => void {
  let dataTransfer: HostDataTransfer | undefined
  let accepted = false

  return (root, event) => {
    // Only the first event of a drag carries its files.
    if (event.files) dataTransfer = createFileDataTransfer(event.files)
    if (!dataTransfer) {
      throw new Error(`A "${event.eventType}" file drag event arrived before any dragenter.`)
    }
    const session = dataTransfer
    // Leaving the window has no related target; moving to another element does.
    const endsDrag =
      event.eventType === "drop" ||
      (event.eventType === "dragleave" && event.relatedTarget == null)

    let eventType = event.eventType
    if (eventType === "drop" && !accepted) eventType = "dragleave"
    if (eventType === "dragenter" || eventType === "dragover") session.dropEffect = "copy"

    const dispatched = { ...event, eventType, dataTransfer: session }
    let defaultPrevented = false
    try {
      if (hasDomListener(root.eventHandlers, dispatched)) {
        defaultPrevented = dispatchDomEvent(root, dispatched).defaultPrevented
      }
    } finally {
      if (event.eventType === "dragover") {
        accepted = defaultPrevented && session.dropEffect !== "none"
      }
      if (endsDrag) {
        dataTransfer = undefined
        accepted = false
      }
    }
  }
}
