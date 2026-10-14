/** Method syntax so a renderer whose `setImage` wants a `Buffer` still fits. */
export type BlobImageHost = {
  setImage?(elementId: number, bytes: Uint8Array): void
}

type BlobImageCallbacks = {
  /** Applies queued mutations, so the `<img>` exists natively before `setImage`. */
  flush(): void
  /**
   * Tells the `<img>` that its `blob:` src could not be read. `ticket` is unique
   * per src assignment: the failure travels as a prop and a prop is applied only
   * when its value changes, so two failures with one message must still differ.
   */
  fail(id: number, message: string, ticket: number): void
}

/**
 * `<img src="blob:...">`. A blob URL only means something in this JS process, so
 * the native layer cannot open it. This fetches the bytes and hands them to
 * `setImage`, the path live images already use. The native element treats a
 * `blob:` src as pending, then fires `load` when `setImage` lands or `error` when
 * `fail` is called.
 *
 * A src that changes while its fetch is in flight wins: the late result is
 * dropped. A revoked URL fails the fetch and reports `error`, as in a browser.
 */
export function createBlobImageLoader(host: BlobImageHost, callbacks: BlobImageCallbacks) {
  const types = new Map<number, string>()
  const latest = new Map<number, number>()
  let counter = 0

  const load = async (id: number, url: string, ticket: number) => {
    const current = () => latest.get(id) === ticket
    try {
      const response = await fetch(url)
      if (!response.ok) throw new Error(`fetch ${url} failed with status ${response.status}`)
      const bytes = new Uint8Array(await response.arrayBuffer())
      if (!current()) return
      if (!host.setImage) throw new Error("this renderer cannot set an image from bytes")
      callbacks.flush()
      host.setImage(id, bytes)
    } catch (error) {
      if (!current()) return
      callbacks.fail(id, error instanceof Error ? error.message : String(error), ticket)
    }
  }

  return {
    created(id: number, elementType: string) {
      types.set(id, elementType)
    },
    destroyed(id: number) {
      types.delete(id)
      latest.delete(id)
    },
    /** Call for every `setCustomProp`, before it is queued. */
    prop(id: number, key: string, value: unknown) {
      if (key !== "src" || types.get(id) !== "img") return
      const ticket = ++counter
      latest.set(id, ticket)
      if (typeof value === "string" && value.trim().startsWith("blob:")) {
        void load(id, value.trim(), ticket)
      }
    },
  }
}
