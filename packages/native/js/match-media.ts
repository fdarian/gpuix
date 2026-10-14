/// `matchMedia` for a GPUIX window: `MediaQueryList` and `MediaQueryListEvent`
/// with the DOM shape, backed by the renderer that owns the window.
///
/// The environment is read live from the renderer (`getWindowSize`,
/// `getAppearance`, `getMediaFeatures`). Changes arrive as the renderer's
/// `mediaChange` event, plus a size poll while a size-dependent query has a
/// listener: the renderer has no resize event.

import type { MediaFeatures } from "../index.js"
import { toAppearance } from "./host-runtime.js"
import type { NativeRenderer } from "./host.js"
import { parseMediaQueryList } from "./media-query.js"
import type { MediaEnvironment, ParsedMediaQueryList } from "./media-query.js"
import { createRendererState, mountedRenderer } from "./renderer-state.js"

export type { MediaEnvironment } from "./media-query.js"
export type { MediaFeatures }

/** How often window size is sampled while a size query has a change listener. */
export const MEDIA_SIZE_POLL_MS = 100

/** Platforms with no OS read have no preference set. */
const NO_PREFERENCES: MediaFeatures = {
  reducedMotion: false,
  increasedContrast: false,
  forcedColors: false,
}

export type MatchMedia = (query: string) => MediaQueryList

function lazy<Value>(read: () => Value): () => Value {
  let cached: { value: Value } | undefined
  return () => {
    cached ??= { value: read() }
    return cached.value
  }
}

/** A snapshot that reads each part of the window the first time a query asks. */
function readEnvironment(host: NativeRenderer): MediaEnvironment {
  const size = lazy(() => {
    if (!host.getWindowSize) {
      throw new Error("This renderer does not report the window size, so size media queries cannot be evaluated")
    }
    return host.getWindowSize()
  })
  const colorScheme = lazy(() => {
    if (!host.getAppearance) {
      throw new Error("This renderer does not report the window appearance, so prefers-color-scheme cannot be evaluated")
    }
    return toAppearance(host.getAppearance())
  })
  const features = lazy(() => host.getMediaFeatures?.() ?? NO_PREFERENCES)
  return {
    get width() {
      return size().width
    },
    get height() {
      return size().height
    },
    get colorScheme() {
      return colorScheme()
    },
    get reducedMotion() {
      return features().reducedMotion
    },
    get increasedContrast() {
      return features().increasedContrast
    },
    get forcedColors() {
      return features().forcedColors
    },
  }
}

export interface MediaSourceEntry {
  notify(): void
  usesSize: boolean
}

/** Shared by every `MediaQueryList` of one window. */
export interface MediaQueryListSource {
  environment(): MediaEnvironment
  subscribe(entry: MediaSourceEntry): () => void
}

function createMediaQueryListSource(host: NativeRenderer): MediaQueryListSource {
  const entries = new Set<MediaSourceEntry>()
  let unsubscribeHost: (() => void) | undefined
  let timer: ReturnType<typeof setInterval> | undefined
  let lastSize: { width: number; height: number } | undefined

  const notify = (filter: (entry: MediaSourceEntry) => boolean) => {
    for (const entry of [...entries]) {
      if (entries.has(entry) && filter(entry)) entry.notify()
    }
  }

  const pollSize = () => {
    const environment = readEnvironment(host)
    const next = { width: environment.width, height: environment.height }
    if (lastSize && lastSize.width === next.width && lastSize.height === next.height) return
    lastSize = next
    notify((entry) => entry.usesSize)
  }

  const syncPoll = () => {
    const polling = [...entries].some((entry) => entry.usesSize)
    if (polling && !timer) {
      const environment = readEnvironment(host)
      lastSize = { width: environment.width, height: environment.height }
      const interval = setInterval(pollSize, MEDIA_SIZE_POLL_MS)
      // A pending poll must not keep the process alive.
      ;(interval as { unref?: () => void }).unref?.()
      timer = interval
    } else if (!polling && timer) {
      clearInterval(timer)
      timer = undefined
      lastSize = undefined
    }
  }

  const sync = () => {
    // The poll is settled in `finally`: the renderer can throw while a window
    // subscription changes, and that must not leave a timer running.
    try {
      if (entries.size > 0 && !unsubscribeHost) {
        unsubscribeHost = createRendererState(host).subscribeMedia(() => notify(() => true))
      } else if (entries.size === 0 && unsubscribeHost) {
        const unsubscribe = unsubscribeHost
        unsubscribeHost = undefined
        unsubscribe()
      }
    } finally {
      syncPoll()
    }
  }

  return {
    environment: () => readEnvironment(host),
    subscribe(entry) {
      entries.add(entry)
      try {
        sync()
      } catch (error) {
        // The caller never gets the unsubscribe function, so nothing else would remove the entry.
        entries.delete(entry)
        syncPoll()
        throw error
      }
      return () => {
        if (!entries.delete(entry)) return
        sync()
      }
    },
  }
}

// ── DOM types ────────────────────────────────────────────────────────

export interface MediaQueryListEventInit extends EventInit {
  matches?: boolean
  media?: string
}

export class MediaQueryListEvent extends Event {
  readonly matches: boolean
  readonly media: string

  constructor(type: string, init: MediaQueryListEventInit = {}) {
    super(type, init)
    this.matches = init.matches ?? false
    this.media = init.media ?? ""
  }
}

type ChangeListener = EventListenerOrEventListenerObject

interface ListenerEntry {
  /** `null` is the `onchange` slot, which keeps its place in the order. */
  callback: ChangeListener | null
  capture: boolean
  once: boolean
}

export class MediaQueryList {
  readonly #parsed: ParsedMediaQueryList
  readonly #source: MediaQueryListSource
  readonly #reevaluate: MediaSourceEntry
  #listeners: ListenerEntry[] = []
  #onchange: ((this: MediaQueryList, event: MediaQueryListEvent) => unknown) | null = null
  #lastMatches: boolean | undefined
  #unsubscribe: (() => void) | undefined

  /** Use `matchMedia`; a list is bound to the window that created it. */
  constructor(query: string, source: MediaQueryListSource) {
    this.#parsed = parseMediaQueryList(query)
    this.#source = source
    this.#reevaluate = {
      usesSize: this.#parsed.usesSize,
      notify: () => this.#notify(),
    }
  }

  get media(): string {
    return this.#parsed.media
  }

  /** Evaluated against the window now, not when the list was created. */
  get matches(): boolean {
    return this.#parsed.matches(this.#source.environment())
  }

  get onchange(): ((this: MediaQueryList, event: MediaQueryListEvent) => unknown) | null {
    return this.#onchange
  }

  set onchange(handler: ((this: MediaQueryList, event: MediaQueryListEvent) => unknown) | null) {
    const typed = typeof handler === "function" ? handler : null
    this.#onchange = typed
    const slot = this.#listeners.find((entry) => entry.callback === null)
    if (typed && !slot) {
      this.#listeners.push({ callback: null, capture: false, once: false })
    } else if (!typed && slot) {
      this.#listeners = this.#listeners.filter((entry) => entry !== slot)
    }
    this.#sync()
  }

  /** Only `"change"` is ever dispatched; other types are accepted and never fire. */
  addEventListener(
    type: string,
    callback: ChangeListener | null,
    options?: boolean | AddEventListenerOptions
  ): void {
    if (type !== "change" || callback === null) return
    const settings = typeof options === "boolean" ? { capture: options } : (options ?? {})
    if (settings.signal?.aborted) return
    const capture = settings.capture ?? false
    if (this.#listeners.some((entry) => entry.callback === callback && entry.capture === capture)) {
      return
    }
    this.#listeners.push({ callback, capture, once: settings.once ?? false })
    settings.signal?.addEventListener("abort", () => this.removeEventListener(type, callback, capture), {
      once: true,
    })
    this.#sync()
  }

  removeEventListener(
    type: string,
    callback: ChangeListener | null,
    options?: boolean | EventListenerOptions
  ): void {
    if (type !== "change" || callback === null) return
    const capture = (typeof options === "boolean" ? options : options?.capture) ?? false
    this.#listeners = this.#listeners.filter(
      (entry) => !(entry.callback === callback && entry.capture === capture)
    )
    this.#sync()
  }

  /** Deprecated alias of `addEventListener("change", callback)`. */
  addListener(callback: ChangeListener | null): void {
    this.addEventListener("change", callback)
  }

  /** Deprecated alias of `removeEventListener("change", callback)`. */
  removeListener(callback: ChangeListener | null): void {
    this.removeEventListener("change", callback)
  }

  dispatchEvent(event: Event): boolean {
    Object.defineProperty(event, "target", { value: this, configurable: true })
    Object.defineProperty(event, "currentTarget", { value: this, configurable: true })
    if (event.type === "change") this.#invoke(event)
    return !event.defaultPrevented
  }

  #notify(): void {
    const matches = this.matches
    if (matches === this.#lastMatches) return
    this.#lastMatches = matches
    this.dispatchEvent(new MediaQueryListEvent("change", { matches, media: this.media }))
  }

  #invoke(event: Event): void {
    for (const entry of [...this.#listeners]) {
      if (!this.#listeners.includes(entry)) continue
      if (entry.once) this.#listeners = this.#listeners.filter((other) => other !== entry)
      try {
        if (entry.callback === null) {
          this.#onchange?.call(this, event as MediaQueryListEvent)
        } else if (typeof entry.callback === "function") {
          entry.callback.call(this, event)
        } else {
          entry.callback.handleEvent(event)
        }
      } catch (error) {
        // DOM reports a listener's error and still runs the others.
        queueMicrotask(() => {
          throw error
        })
      }
    }
    this.#sync()
  }

  /** Subscribed to the window only while something listens. */
  #sync(): void {
    if (this.#listeners.length > 0 && !this.#unsubscribe) {
      this.#lastMatches = this.matches
      this.#unsubscribe = this.#source.subscribe(this.#reevaluate)
    } else if (this.#listeners.length === 0 && this.#unsubscribe) {
      this.#unsubscribe()
      this.#unsubscribe = undefined
    }
  }
}

// ── Entry points ─────────────────────────────────────────────────────

const matchers = new WeakMap<object, MatchMedia>()

/**
 * `matchMedia` bound to `host`'s window. The same function comes back for the
 * same renderer, so it is safe in dependency lists.
 */
export function matchMediaFor(host: NativeRenderer): MatchMedia {
  const existing = matchers.get(host)
  if (existing) return existing
  const source = createMediaQueryListSource(host)
  const matcher: MatchMedia = (query) => new MediaQueryList(String(query), source)
  matchers.set(host, matcher)
  return matcher
}

/**
 * `window.matchMedia` for code with no component to ask: the window of the
 * most recently mounted root. Throws before any root is mounted.
 */
export function matchMedia(query: string): MediaQueryList {
  const host = mountedRenderer()
  if (!host) {
    throw new Error("matchMedia needs a mounted GPUIX root. Call it after render(), or use useMatchMedia() / createMatchMedia() inside a component.")
  }
  return matchMediaFor(host)(query)
}

/**
 * Call `callback` with the current result now and again whenever it flips.
 * Returns the stop function.
 */
export function observeMediaQuery(
  host: NativeRenderer,
  query: string,
  callback: (matches: boolean) => void
): () => void {
  const list = matchMediaFor(host)(query)
  const listener = (event: Event) => callback((event as MediaQueryListEvent).matches)
  callback(list.matches)
  list.addEventListener("change", listener)
  return () => list.removeEventListener("change", listener)
}

export const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)"

/** The OS "reduce motion" setting, now and on every change. */
export function observeReducedMotion(
  host: NativeRenderer,
  callback: (reduced: boolean) => void
): () => void {
  return observeMediaQuery(host, REDUCED_MOTION_QUERY, callback)
}

export function readReducedMotion(host: NativeRenderer): boolean {
  return matchMediaFor(host)(REDUCED_MOTION_QUERY).matches
}
