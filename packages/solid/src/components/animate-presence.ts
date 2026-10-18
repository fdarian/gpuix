import {
  children,
  createComponent,
  createContext,
  createRenderEffect,
  createSignal,
  For,
  onCleanup,
  untrack,
  type Accessor,
  type JSX,
  type Signal,
} from "solid-js"
import type { DomEvent } from "@gpuix/native/host"
import { measurePopLayout } from "@gpuix/native/host"
import type { MotionProps, PresenceMode, StyleDesc } from "@gpuix/native/host"
import { HostElement, setHostProperty, type HostNode } from "../host.js"
import {
  anchorHost,
  disposePresenceScope,
  enterPresenceGroup,
  isHostNode,
  isWithin,
  madeUnder,
  ownPresenceGroup,
  registerPresence,
  type PresenceRegistration,
  type PresenceScope,
  type PresenceScopeGroup,
} from "../presence-scope.js"

/**
 * What a `motion.*` can see of its `AnimatePresence`. The children resolve
 * before `AnimatePresence` sees them, so context cannot carry the presence of
 * a child: that is found from where a component's output sits (see
 * `presence-scope.ts`), and the exit itself is driven from the outside (see
 * the render effect below).
 */
export interface PresenceGroupContextValue {
  custom: Accessor<unknown>
}

export const PresenceGroupContext = createContext<PresenceGroupContextValue>()

/**
 * Always present outside an `AnimatePresence`. Inside one it flips to false
 * when the child the caller sits in leaves, and the child stays until
 * `safeToRemove` is called (and until the child's own exit animation, if it has
 * one, is done). Call it from a component: a component's output is what places
 * it in a child.
 */
export function usePresence(): [Accessor<boolean>, () => void] {
  return presenceOf(true)
}

/** Does not hold the child back: only reads whether it is leaving. */
export function useIsPresent(): Accessor<boolean> {
  return presenceOf(false)[0]
}

function presenceOf(waits: boolean): [Accessor<boolean>, () => void] {
  const registration = registerPresence(waits)
  if (!registration) return [() => true, () => {}]
  onCleanup(registration.unregister)
  return [registration.isPresent, registration.safeToRemove]
}

type Consumer = {
  readonly scope: PresenceScope
  readonly waits: boolean
  readonly present: Signal<boolean>
  /** Set while the child the consumer sits in is leaving. */
  leaving: JSX.Element | undefined
  done: boolean
}

export interface AnimatePresenceProps {
  children?: JSX.Element
  initial?: boolean
  onExitComplete?: () => void
  /**
   * `"sync"` (default): enter and exit together. `"wait"`: a new child
   * renders only after every exit is done. `"popLayout"`: a leaving child is
   * taken out of flow at its last box inside its parent, so siblings reflow
   * at once. The child must resolve to a single host element.
   */
  mode?: PresenceMode
  /** Passed to the `exit` function of every child that leaves. */
  custom?: unknown
}

const GESTURE_FIELDS = ["whileHover", "whileTap", "whileFocus", "whileInView", "inView", "inherit", "controls"] as const

type NativeMotionProps = MotionProps & {
  generation?: number
  isExit?: boolean
}

function normalize(value: JSX.Element): JSX.Element[] {
  if (value == null || value === true || value === false) return []
  return Array.isArray(value) ? value.flatMap(normalize) : [value]
}

export function AnimatePresence(props: AnimatePresenceProps): JSX.Element {
  const group: PresenceGroupContextValue = { custom: () => props.custom }
  /** Declared before the children resolve: a child that throws while creating releases its scopes. */
  const [rendered, setRendered] = createSignal<JSX.Element[]>([])
  const consumers = new Set<Consumer>()
  /** Scopes kept alive for a leaving child, disposed when it is removed. */
  const retained = new Map<JSX.Element, Set<PresenceScope>>()
  /** Children whose own exit animation has not settled. */
  const animating = new Set<JSX.Element>()
  let unmounted = false
  const scopeGroup: PresenceScopeGroup = {
    retain(scope, owner) {
      if (unmounted) return false
      const anchor = anchorHost(scope)
      if (!anchor) return false
      const item = untrack(rendered).find(
        (candidate): candidate is HostNode & JSX.Element => isHostNode(candidate) && isWithin(anchor, candidate)
      )
      // Cleaning an owner inside the child leaves the child where it is.
      if (!item || !madeUnder(item, owner)) return false
      const scopes = retained.get(item) ?? new Set()
      scopes.add(scope)
      retained.set(item, scopes)
      return true
    },
    register(scope, waits) {
      const consumer: Consumer = {
        scope,
        waits,
        present: createSignal(true),
        leaving: undefined,
        done: false,
      }
      consumers.add(consumer)
      return {
        isPresent: consumer.present[0],
        safeToRemove() {
          if (consumer.present[0]() || !consumers.has(consumer)) return
          consumer.done = true
          if (consumer.leaving) settle(consumer.leaving)
        },
        unregister() {
          consumers.delete(consumer)
          if (consumer.leaving) settle(consumer.leaving)
        },
      } satisfies PresenceRegistration
    },
  }
  enterPresenceGroup()
  onCleanup(() => {
    unmounted = true
    for (const scopes of retained.values()) for (const scope of scopes) disposePresenceScope(scope)
    retained.clear()
  })
  const resolved = children(() =>
    createComponent(PresenceGroupContext.Provider, {
      value: group,
      get children() {
        ownPresenceGroup(scopeGroup)
        return props.children
      },
    })
  )
  setRendered(normalize(resolved()))
  const presence = new Map<JSX.Element, Signal<boolean>>()
  const unpopped = new Map<HostElement, StyleDesc>()
  let initial = true

  const disposeRetained = (item: JSX.Element) => {
    const scopes = retained.get(item)
    if (!scopes) return
    retained.delete(item)
    for (const scope of scopes) disposePresenceScope(scope)
  }

  const consumersIn = (item: JSX.Element) => {
    if (!isHostNode(item)) return []
    return [...consumers].filter((consumer) => {
      const anchor = anchorHost(consumer.scope)
      return consumer.leaving === undefined && anchor !== undefined && isWithin(anchor, item)
    })
  }

  const blocked = (item: JSX.Element) =>
    animating.has(item) ||
    [...consumers].some((consumer) => consumer.leaving === item && consumer.waits && !consumer.done)

  const settle = (item: JSX.Element) => {
    const entry = presence.get(item)
    if (entry && !blocked(item)) remove(item, entry)
  }

  const pop = (item: HostElement) => {
    if (!item.root || !item.mounted || unpopped.has(item)) return
    const parent = item.parent
    const box = measurePopLayout(
      item.root.nativeRenderer,
      item.id,
      parent?.kind === "element" ? parent.id : undefined
    )
    if (box === undefined) return
    unpopped.set(item, item.style)
    item.style = { ...item.style, ...box }
    item.root.mutations.setStyle(item.id, item.style)
  }

  const unpop = (item: HostElement) => {
    const style = unpopped.get(item)
    if (style === undefined) return
    unpopped.delete(item)
    item.style = style
    if (item.root && item.mounted) item.root.mutations.setStyle(item.id, style)
  }

  const remove = (item: JSX.Element, entry: Signal<boolean>) => {
    if (entry[0]() || !presence.has(item)) return
    presence.delete(item)
    animating.delete(item)
    if (item instanceof HostElement) unpopped.delete(item)
    setRendered(rendered().filter((candidate) => candidate !== item))
    disposeRetained(item)
    if (![...presence.values()].some(([present]) => !present())) {
      if (props.mode === "wait") setRendered(normalize(untrack(resolved)))
      props.onExitComplete?.()
    }
  }

  createRenderEffect(() => {
    const current = normalize(resolved())
    const previous = untrack(rendered)
    const exiting = previous.filter((item) => !current.includes(item))
    for (const item of current) {
      const entry = presence.get(item)
      if (entry) {
        if (!entry[0]()) {
          animating.delete(item)
          for (const consumer of consumers) {
            if (consumer.leaving !== item) continue
            consumer.leaving = undefined
            consumer.done = false
            consumer.present[1](true)
          }
        }
        entry[1](true)
        disposeRetained(item)
      } else presence.set(item, createSignal(true))
      if (item instanceof HostElement) unpop(item)
      if (initial && props.initial === false && item instanceof HostElement) {
        const motion = item.props.get("motion") as MotionProps | undefined
        if (motion) setHostProperty(item, "motion", { ...motion, initial: false }, motion)
      }
    }
    for (const item of exiting) {
      const entry = presence.get(item)
      if (!entry || !entry[0]()) continue
      entry[1](false)
      for (const consumer of consumersIn(item)) {
        consumer.leaving = item
        consumer.present[1](false)
      }
      if (item instanceof HostElement && props.mode === "popLayout") pop(item)
      if (!(item instanceof HostElement)) {
        settle(item)
        continue
      }
      const motion = item.props.get("motion") as NativeMotionProps | undefined
      if (!motion?.exit) {
        settle(item)
        continue
      }
      animating.add(item)
      const previous = item.props.get("onMotionComplete") as
        | ((event: DomEvent) => void)
        | undefined
      // A leaving node answers to no gesture: what they moved goes back as the exit takes over.
      const leaving: NativeMotionProps = { ...motion }
      for (const key of GESTURE_FIELDS) delete leaving[key]
      setHostProperty(item, "motion", {
        ...leaving,
        generation: (motion.generation ?? 0) + 1,
        isExit: true,
        animate: motion.exit,
      }, motion)
      setHostProperty(
        item,
        "onMotionComplete",
        (event: DomEvent) => {
          const active = item.props.get("motion") as NativeMotionProps | undefined
          if (
            active?.generation !== undefined &&
            event.motionGeneration !== active.generation
          ) return
          previous?.(event)
          animating.delete(item)
          settle(item)
        },
        previous
      )
    }
    const waiting = props.mode === "wait" && exiting.length > 0
    const visible = waiting ? current.filter((item) => previous.includes(item)) : current
    // Exiting nodes keep their place: a node that moves is destroyed and
    // created again, which restarts its exit and drops its handlers. Read
    // `rendered` again: a child with no exit was already removed above, and
    // `previous` would bring it back for good.
    const remaining = untrack(rendered)
    const next = [...visible]
    remaining.forEach((item, index) => {
      if (!current.includes(item)) next.splice(index, 0, item)
    })
    if (
      next.length !== remaining.length ||
      next.some((item, index) => item !== remaining[index])
    ) {
      setRendered(next)
    }
    initial = false
  })

  return createComponent(For as any, {
    get each() { return rendered() },
    children(item: JSX.Element) {
      presence.set(item, presence.get(item) ?? createSignal(true))
      return item
    },
  })
}
