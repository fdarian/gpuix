import type { DomEvent } from "./events/dispatch.js"
import { registerEventListener, unregisterEventListener } from "./events/listeners.js"
import type { EventHandlerMap, HostEventHandler, MutationRenderer } from "./host.js"

const ATTACH = "attach"
const DETACH = "detach"

/**
 * What one `<nativeview>` has been told, kept per element object so it dies
 * with the element and needs no cleanup when one is destroyed.
 */
interface Pairing {
  /** `onAttach` ran, so the library holds a view that `onDetach` must end. */
  attachCalled: boolean
  /** The surface is gone. */
  detached: boolean
}

const pairings = new WeakMap<object, Pairing>()

function pairingOf(event: DomEvent): Pairing {
  if (event.target === null) {
    throw new Error(`The "${event.type}" event of a <nativeview> has no target element.`)
  }
  let pairing = pairings.get(event.target)
  if (!pairing) {
    pairing = { attachCalled: false, detached: false }
    pairings.set(event.target, pairing)
  }
  return pairing
}

function viewOf(event: DomEvent): bigint {
  const handle = event.nativeHandle
  if (handle === undefined || !/^\d+$/.test(handle)) {
    throw new Error(
      `The "attach" event of a <nativeview> carries no native handle in decimal digits: ${JSON.stringify(handle)}.`
    )
  }
  return BigInt(handle)
}

function asHandler<Handler extends (...args: never[]) => void>(
  prop: string,
  value: unknown
): Handler | undefined {
  if (value === undefined || value === null || value === false) return undefined
  if (typeof value !== "function") {
    throw new TypeError(`${prop} of a <nativeview> must be a function, got ${typeof value}.`)
  }
  return value as Handler
}

/**
 * The two native listeners for a pair of user handlers. Both are always made,
 * whichever handler is present: native sends `detach` only after an `attach` it
 * emitted, and the state that pairs them has to see both.
 *
 * `attach` comes through the event queue and `detach` synchronously, so a
 * teardown can overtake the queued `attach`. Such an `attach` is dropped:
 * handing out the pointer of a surface that is already gone would be a crash.
 * `onDetach` runs only when `onAttach` ran, or always when there is no `onAttach`.
 */
function listeners(
  onAttach: ((view: bigint) => void) | undefined,
  onDetach: (() => void) | undefined
): { attach: HostEventHandler; detach: HostEventHandler } {
  return {
    attach(event) {
      const pairing = pairingOf(event)
      if (pairing.detached) return
      if (pairing.attachCalled) {
        throw new Error(`<nativeview> ${event.elementId} was attached twice.`)
      }
      if (!onAttach) return
      const view = viewOf(event)
      // Before the call: a throw in `onAttach` still leaves a library that may hold the view.
      pairing.attachCalled = true
      onAttach(view)
    },
    detach(event) {
      const pairing = pairingOf(event)
      if (pairing.detached) {
        throw new Error(`<nativeview> ${event.elementId} was detached twice.`)
      }
      pairing.detached = true
      if (!onDetach) return
      if (onAttach && !pairing.attachCalled) return
      onDetach()
    },
  }
}

/**
 * Registers the native listeners for a `<nativeview>`'s `onAttach` and `onDetach`,
 * or removes them when it has neither. Call it on mount and whenever either prop
 * changed; a new function replaces the old one without a second native listener.
 */
export function syncNativeviewListeners(
  eventHandlers: EventHandlerMap,
  renderer: Pick<MutationRenderer, "setEventListener">,
  elementId: number,
  onAttach: unknown,
  onDetach: unknown
): void {
  const attach = asHandler<(view: bigint) => void>("onAttach", onAttach)
  const detach = asHandler<() => void>("onDetach", onDetach)
  if (!attach && !detach) {
    unregisterEventListener(eventHandlers, renderer, elementId, ATTACH)
    unregisterEventListener(eventHandlers, renderer, elementId, DETACH)
    return
  }
  const pair = listeners(attach, detach)
  registerEventListener(eventHandlers, renderer, elementId, ATTACH, pair.attach)
  registerEventListener(eventHandlers, renderer, elementId, DETACH, pair.detach)
}

/** `onAttach` and `onDetach`: the adapters register these through `syncNativeviewListeners`, not as DOM handlers. */
export function isNativeviewHandlerProp(name: string): boolean {
  return name === "onAttach" || name === "onDetach"
}
