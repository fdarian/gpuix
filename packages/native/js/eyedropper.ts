import type { NativeRenderer } from "./host.js"

export interface ColorSelectionOptions {
  signal?: AbortSignal
}

export interface ColorSelectionResult {
  /** `#rrggbb`, lowercase. */
  sRGBHex: string
}

export type EyeDropperHost = Pick<
  NativeRenderer,
  "supportsEyeDropper" | "openEyeDropper"
>

export interface EyeDropper {
  /**
   * Show the screen colour picker. Rejects with an `AbortError` when the user
   * cancels, and with the signal's reason when `signal` aborts.
   *
   * macOS cannot dismiss the picker from code, so an aborted picker stays on
   * screen until the user clicks or presses Escape. `open()` rejects with an
   * `InvalidStateError` until then.
   */
  open(options?: ColorSelectionOptions): Promise<ColorSelectionResult>
}

export interface EyeDropperConstructor {
  new (): EyeDropper
}

// DOMException is missing from some embedded runtimes such as Hermes.
function namedError(name: string, message: string): Error {
  if (typeof DOMException === "function") return new DOMException(message, name)
  const error = new Error(message)
  error.name = name
  return error
}

export function createEyeDropper(
  host: () => EyeDropperHost | undefined
): EyeDropperConstructor {
  // One native session at a time; it outlives an aborted open().
  let sessionOpen = false
  return class implements EyeDropper {
    open(options: ColorSelectionOptions = {}): Promise<ColorSelectionResult> {
      const signal = options.signal
      if (signal?.aborted) return Promise.reject(signal.reason)
      const current = host()
      if (!current?.openEyeDropper) {
        return Promise.reject(
          new Error("EyeDropper is not available on this platform")
        )
      }
      if (sessionOpen) {
        return Promise.reject(
          namedError("InvalidStateError", "An EyeDropper is already open")
        )
      }
      sessionOpen = true
      return new Promise((resolve, reject) => {
        let settled = false
        const onAbort = () => {
          settled = true
          reject(signal?.reason)
        }
        signal?.addEventListener("abort", onAbort, { once: true })
        const finish = (settle: () => void) => {
          sessionOpen = false
          signal?.removeEventListener("abort", onAbort)
          if (settled) return
          settled = true
          settle()
        }
        current.openEyeDropper!().then(
          (hex) =>
            finish(() =>
              hex === null
                ? reject(
                    namedError("AbortError", "The user canceled the selection")
                  )
                : resolve({ sRGBHex: hex })
            ),
          (error) => finish(() => reject(error))
        )
      })
    }
  }
}

let activeHost: EyeDropperHost | undefined

/** Opens the picker of the renderer created last. */
export const EyeDropper: EyeDropperConstructor = createEyeDropper(
  () => activeHost
)

/**
 * Point `EyeDropper` at `host`, and define `globalThis.EyeDropper` when the
 * platform can sample colours and the runtime has none. Elsewhere it stays
 * undefined, so `typeof EyeDropper` feature detection works as in a browser.
 */
export function bindEyeDropper(host: EyeDropperHost): void {
  activeHost = host
  const scope = globalThis as { EyeDropper?: EyeDropperConstructor }
  if (scope.EyeDropper === undefined && host.supportsEyeDropper?.() === true) {
    Object.defineProperty(globalThis, "EyeDropper", {
      value: EyeDropper,
      configurable: true,
      writable: true,
    })
  }
}
