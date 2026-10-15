import type { NativeRenderer } from "./host.js"

/** What an adapter hands `requestFormSubmit` for one `<form>`. */
export interface FormHost {
  id: number
  nativeRenderer: NativeRenderer
  /** Apply queued mutations, so the form and its controls exist natively. */
  flushMutations(): void
}

export function requestFormSubmit(host: FormHost): void {
  const renderer = host.nativeRenderer
  if (!renderer.requestSubmit) {
    throw new Error("This GPUIX renderer does not support requestSubmit()")
  }
  host.flushMutations()
  renderer.requestSubmit(host.id)
}
