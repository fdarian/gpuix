import {
  children,
  createComponent,
  createContext,
  createRenderEffect,
  createSignal,
  For,
  untrack,
  useContext,
  type Accessor,
  type JSX,
  type Signal,
} from "solid-js"
import type { DomEvent } from "@gpuix/native/host"
import { measurePopLayout } from "@gpuix/native/host"
import type { MotionProps, PresenceMode, StyleDesc } from "@gpuix/native/host"
import { HostElement, setHostProperty } from "../host.js"

export interface PresenceContextValue {
  isPresent: Accessor<boolean>
  initial: false | undefined
  safeToRemove(): void
}

export const PresenceContext = createContext<PresenceContextValue>()

/**
 * What a `motion.*` can see of its `AnimatePresence`. `PresenceContext` cannot
 * reach it: the children resolve before `AnimatePresence` wraps each one, so
 * the exit itself is driven from the outside (see the render effect below).
 */
export interface PresenceGroupContextValue {
  custom: Accessor<unknown>
}

export const PresenceGroupContext = createContext<PresenceGroupContextValue>()

/**
 * Always present outside a `motion.*`'s own `PresenceContext`: a leaving child's
 * owner is disposed when it leaves, which disposes every effect a descendant
 * made, so a descendant could not observe `isPresent` flipping. React's
 * `usePresence` parity would need `AnimatePresence` to own each child's scope.
 */
export function usePresence(): [Accessor<boolean>, () => void] {
  const context = useContext(PresenceContext)
  return context
    ? [context.isPresent, context.safeToRemove]
    : [() => true, () => {}]
}

export function useIsPresent(): Accessor<boolean> {
  return usePresence()[0]
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
  const resolved = children(() =>
    createComponent(PresenceGroupContext.Provider, {
      value: group,
      get children() { return props.children },
    })
  )
  const [rendered, setRendered] = createSignal(normalize(resolved()))
  const presence = new Map<JSX.Element, Signal<boolean>>()
  const unpopped = new Map<HostElement, StyleDesc>()
  let initial = true

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
    if (item instanceof HostElement) unpopped.delete(item)
    setRendered(rendered().filter((candidate) => candidate !== item))
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
      if (entry) entry[1](true)
      else presence.set(item, createSignal(true))
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
      if (item instanceof HostElement && props.mode === "popLayout") pop(item)
      if (!(item instanceof HostElement)) {
        remove(item, entry)
        continue
      }
      const motion = item.props.get("motion") as NativeMotionProps | undefined
      if (!motion?.exit) {
        remove(item, entry)
        continue
      }
      const previous = item.props.get("onMotionComplete") as
        | ((event: DomEvent) => void)
        | undefined
      setHostProperty(item, "motion", {
        ...motion,
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
          remove(item, entry)
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
      const entry = presence.get(item) ?? createSignal(true)
      presence.set(item, entry)
      return createComponent(PresenceContext.Provider, {
        value: {
          isPresent: entry[0],
          initial: initial && props.initial === false ? false : undefined,
          safeToRemove: () => remove(item, entry),
        },
        get children() { return item },
      })
    },
  })
}
