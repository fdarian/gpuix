import type { NativeRenderer } from "./host.js"

/** The `navigator.clipboard` text subset. Images and rich items are not supported. */
export interface Clipboard {
  writeText(text: string): Promise<void>
  /** Resolves `""` when the clipboard holds no text, as in the DOM. */
  readText(): Promise<string>
}

export type ClipboardHost = Pick<
  NativeRenderer,
  "writeClipboardText" | "readClipboardText"
>

export function createClipboard(
  host: () => ClipboardHost | undefined
): Clipboard {
  const requireHost = () => {
    const current = host()
    if (!current?.writeClipboardText || !current.readClipboardText) {
      throw new Error("No GPUIX renderer with a clipboard is running")
    }
    return current as Required<ClipboardHost>
  }
  return {
    async writeText(text) {
      requireHost().writeClipboardText(String(text))
    },
    async readText() {
      const text = requireHost().readClipboardText()
      // The DOM has no "empty" state: readText() of a text-less clipboard is "".
      return text === null ? "" : text
    },
  }
}

let activeHost: ClipboardHost | undefined

/** The clipboard of the renderer created last. */
export const clipboard: Clipboard = createClipboard(() => activeHost)

/**
 * Point `clipboard` at `host`, and expose it as `navigator.clipboard` when the
 * runtime has none, so code written for the web runs unchanged.
 */
export function bindClipboard(host: ClipboardHost): void {
  activeHost = host
  const scope = globalThis as { navigator?: { clipboard?: Clipboard } }
  if (scope.navigator === undefined) {
    Object.defineProperty(globalThis, "navigator", {
      value: {},
      configurable: true,
      writable: true,
    })
  }
  const target = scope.navigator as { clipboard?: Clipboard }
  if (target.clipboard === undefined) {
    Object.defineProperty(target, "clipboard", {
      value: clipboard,
      configurable: true,
      enumerable: true,
    })
  }
}
