/** Method syntax so a renderer whose methods want a `Buffer` still fits. */
export type StyleImageHost = {
  setStyleImage?(url: string, bytes: Uint8Array): void
  failStyleImage?(url: string, message: string): void
}

const BLOB_URL =
  /url\(\s*(?:"(blob:(?:[^"\\]|\\.)*)"|'(blob:(?:[^'\\]|\\.)*)'|(blob:[^\s)"'\\]*))\s*\)/gi

/** Calls `visit` with each `blob:` address that `url()` names in the strings of a style, at any depth. */
export function forEachBlobUrl(style: unknown, visit: (url: string) => void): void {
  if (typeof style === "string") {
    if (!style.includes("blob:")) return
    for (const match of style.matchAll(BLOB_URL)) {
      const quoted = match[1] ?? match[2]
      visit(quoted === undefined ? match[3]! : quoted.replace(/\\(.)/g, "$1"))
    }
  } else if (Array.isArray(style)) {
    for (const item of style) forEachBlobUrl(item, visit)
  } else if (typeof style === "object" && style !== null) {
    for (const value of Object.values(style)) forEachBlobUrl(value, visit)
  }
}

/**
 * `background: url(blob:...)`. A blob URL only means something in this JS
 * process, so the native layer cannot open it. This fetches the bytes once per
 * distinct URL a style names, base style and state styles alike, and hands them
 * to `setStyleImage`; native keeps the image until a layer that names the URL
 * asks for it. A URL that cannot be read (revoked) is reported through
 * `failStyleImage`, and the layer paints nothing.
 */
export function createStyleImageLoader(host: StyleImageHost) {
  const requested = new Set<string>()

  const load = async (url: string) => {
    try {
      const response = await fetch(url)
      if (!response.ok) throw new Error(`fetch ${url} failed with status ${response.status}`)
      const bytes = new Uint8Array(await response.arrayBuffer())
      if (!host.setStyleImage) throw new Error("this renderer cannot set a style image from bytes")
      host.setStyleImage(url, bytes)
    } catch (error) {
      host.failStyleImage?.(url, error instanceof Error ? error.message : String(error))
    }
  }

  return {
    /** Call for every style a mutation carries, before it is queued. */
    style(style: unknown) {
      forEachBlobUrl(style, (url) => {
        if (requested.has(url)) return
        requested.add(url)
        void load(url)
      })
    },
  }
}
