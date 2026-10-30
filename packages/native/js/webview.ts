import type { NativeRenderer } from "./host.js"

/**
 * The methods a ref to a `<webview>` carries, shaped after Electron's `<webview>` tag. The
 * page is made when the element is first painted: `loadURL` before that waits for it, and every
 * other method throws until then.
 */
export interface WebviewMethods {
  /**
   * Load a full URL. A `file:` URL may read the directory it is in; anything else is requested.
   * A URL without a scheme throws. Setting `src` does the same.
   */
  loadURL(url: string): void
  reload(): void
  stop(): void
  goBack(): void
  goForward(): void
  canGoBack(): boolean
  canGoForward(): boolean
  /** The URL of the page, or `""` before the first load commits. */
  getURL(): string
  /** The title of the page, or `""` when it has none. */
  getTitle(): string
  /**
   * Run `code` in the page and resolve with the value of its last expression, as JSON: numbers,
   * strings, booleans, `null`, arrays and objects. `undefined` resolves `undefined`. Rejects with
   * the message of an error the script throws, and for a result that is not JSON (a function,
   * a promise). A promise is not awaited.
   */
  executeJavaScript(code: string): Promise<unknown>
}

/** What an adapter hands the helper below for one `<webview>`. */
export interface WebviewHost {
  id: number
  nativeRenderer: NativeRenderer
  /** Apply queued mutations, so the element and its latest `src` exist natively. */
  flushMutations(): void
}

type WebviewNative = Required<
  Pick<NativeRenderer, "webviewCommand" | "webviewState" | "webviewExecuteJavascript">
>

function native(host: WebviewHost): WebviewNative {
  const renderer = host.nativeRenderer
  if (!renderer.webviewCommand || !renderer.webviewState || !renderer.webviewExecuteJavascript) {
    throw new Error("This renderer does not support <webview>")
  }
  host.flushMutations()
  return renderer as WebviewNative
}

export function createWebviewMethods(host: WebviewHost): WebviewMethods {
  return {
    loadURL: (url) => native(host).webviewCommand(host.id, "loadURL", url),
    reload: () => native(host).webviewCommand(host.id, "reload"),
    stop: () => native(host).webviewCommand(host.id, "stop"),
    goBack: () => native(host).webviewCommand(host.id, "goBack"),
    goForward: () => native(host).webviewCommand(host.id, "goForward"),
    canGoBack: () => native(host).webviewState(host.id).canGoBack,
    canGoForward: () => native(host).webviewState(host.id).canGoForward,
    getURL: () => native(host).webviewState(host.id).url,
    getTitle: () => native(host).webviewState(host.id).title,
    executeJavaScript: async (code) => {
      const json = await native(host).webviewExecuteJavascript(host.id, code)
      return json === null ? undefined : JSON.parse(json)
    },
  }
}
