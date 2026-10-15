import type { HostDataTransfer, HostDropEffect, HostEffectAllowed, HostFile } from "./types.js"

/**
 * What a handler may do with a `dataTransfer`, as the DOM's drag data store
 * modes: `readwrite` in `dragstart`, `readonly` in `drop` and `protected` in
 * every other drag event, where `getData` is empty and `setData` does nothing.
 */
export type DataTransferMode = "readwrite" | "readonly" | "protected"

export interface DataTransferSession {
  readonly dataTransfer: HostDataTransfer
  /** The drag dispatcher sets this before each event; handlers cannot. */
  mode: DataTransferMode
}

const DROP_EFFECTS: ReadonlySet<string> = new Set(["none", "copy", "link", "move"])

const EFFECT_ALLOWED: ReadonlySet<string> = new Set([
  "none",
  "copy",
  "copyLink",
  "copyMove",
  "link",
  "linkMove",
  "move",
  "all",
  "uninitialized",
])

/** The drop effect a target gets before its handler picks one. */
export function defaultDropEffect(effectAllowed: HostEffectAllowed): HostDropEffect {
  switch (effectAllowed) {
    case "none":
      return "none"
    case "link":
    case "linkMove":
      return "link"
    case "move":
      return "move"
    default:
      return "copy"
  }
}

/** Whether a source that allowed `effectAllowed` lets a target choose `dropEffect`. */
export function allowsDropEffect(
  effectAllowed: HostEffectAllowed,
  dropEffect: HostDropEffect
): boolean {
  if (dropEffect === "none") return true
  if (effectAllowed === "all" || effectAllowed === "uninitialized") return true
  return effectAllowed.toLowerCase().includes(dropEffect)
}

/** `"text"` and `"url"` are the legacy names of two MIME types, and formats ignore case. */
function normalizeFormat(format: string): string {
  const lower = format.toLowerCase()
  if (lower === "text") return "text/plain"
  if (lower === "url") return "text/uri-list"
  return lower
}

/**
 * The data of one drag. A drag of an element fills it in `dragstart`; a drag of
 * files from the OS arrives with `files` and nothing else for a handler to
 * add. `setDragImage` is accepted and ignored: there is no drag image.
 */
export function createDataTransfer(files: readonly HostFile[] = []): DataTransferSession {
  const entries = new Map<string, string>()
  const frozenFiles = Object.freeze([...files])
  let effectAllowed: HostEffectAllowed = files.length > 0 ? "all" : "uninitialized"
  let dropEffect: HostDropEffect = "none"

  const session: DataTransferSession = {
    mode: "protected",
    dataTransfer: {
      get dropEffect() {
        return dropEffect
      },
      set dropEffect(value) {
        if (DROP_EFFECTS.has(value)) dropEffect = value
      },
      get effectAllowed() {
        return effectAllowed
      },
      set effectAllowed(value) {
        if (session.mode === "readwrite" && EFFECT_ALLOWED.has(value)) effectAllowed = value
      },
      files: frozenFiles,
      get types() {
        return Object.freeze(frozenFiles.length > 0 ? [...entries.keys(), "Files"] : [...entries.keys()])
      },
      getData(format) {
        if (session.mode === "protected") return ""
        return entries.get(normalizeFormat(format)) ?? ""
      },
      setData(format, data) {
        if (session.mode === "readwrite") entries.set(normalizeFormat(format), String(data))
      },
      clearData(format) {
        if (session.mode !== "readwrite") return
        if (format === undefined) entries.clear()
        else entries.delete(normalizeFormat(format))
      },
      setDragImage() {},
    },
  }
  return session
}
