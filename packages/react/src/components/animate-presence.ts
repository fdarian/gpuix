import {
  Children,
  cloneElement,
  createContext,
  createElement,
  Fragment,
  isValidElement,
  useCallback,
  useContext,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react"
import type { ReactElement, ReactNode, Ref } from "react"
import { measurePopLayout } from "@ghia/native/host"
import type { PopLayoutStyle, PresenceMode } from "@ghia/native/host"
import { useGpuix } from "../hooks/use-gpuix.js"
import type { Instance } from "../types/host.js"
export type ComponentKey = string | number

export interface PresenceContextValue {
  id: string
  isPresent: boolean
  initial: false | undefined
  /** The `custom` of the `AnimatePresence` the child is in, for an `exit` function. */
  custom: unknown
  onExitComplete: (childId: string) => void
  register: (childId: string) => () => void
}

export const PresenceContext = createContext<PresenceContextValue | null>(null)

export type SafeToRemove = () => void

export function usePresence(): [true] | [true, null] | [false, SafeToRemove] {
  const context = useContext(PresenceContext)
  const id = useId()
  const register = context?.register
  useLayoutEffect(() => {
    if (!register) return
    return register(id)
  }, [id, register])

  const onExitComplete = context?.onExitComplete
  const safeToRemove = useCallback(() => {
    onExitComplete?.(id)
  }, [id, onExitComplete])

  if (context === null) return [true, null]
  return !context.isPresent ? [false, safeToRemove] : [true]
}

export function useIsPresent(): boolean {
  const context = useContext(PresenceContext)
  return context === null ? true : context.isPresent
}

export interface AnimatePresenceProps {
  children?: ReactNode
  /** Skip enter animations for children present on the first render. */
  initial?: boolean
  /** Fires when every exiting child has finished. */
  onExitComplete?: () => void
  /**
   * `"sync"` (default): enter and exit together. `"wait"`: a new child
   * renders only after every exit is done. `"popLayout"`: a leaving child is
   * taken out of flow at its last box inside its parent, so siblings reflow
   * at once; the parent must be positioned, as in CSS. The child must take `style` and `ref` (a `motion.*` or a host
   * element); `getElementBounds` supplies the box.
   */
  mode?: PresenceMode
  /** Passed to the `exit` function of every child that leaves, in place of the `custom` it last rendered with. */
  custom?: unknown
}

function getChildKey(child: ReactElement): ComponentKey {
  return child.key ?? ""
}

function onlyElements(children: ReactNode): ReactElement[] {
  const filtered: ReactElement[] = []
  Children.forEach(children, (child) => {
    if (isValidElement(child)) filtered.push(child)
  })
  return filtered
}

function setRef<Value>(ref: Ref<Value> | undefined, value: Value | null): void {
  if (typeof ref === "function") ref(value)
  else if (ref) ref.current = value
}

type PoppableProps = { ref?: Ref<Instance>; style?: object }

type PresenceChildProps = {
  children: ReactElement
  isPresent: boolean
  initial: false | undefined
  custom: unknown
  popLayout: boolean
  onExitComplete?: () => void
}

/** The child with its box pinned while it leaves; the box is read before the commit that takes it out of flow. */
function usePoppedChild(props: PresenceChildProps): ReactElement {
  const child = props.children as ReactElement<PoppableProps>
  const gpuix = useGpuix()
  const instance = useRef<Instance | null>(null)
  const box = useRef<PopLayoutStyle | undefined>(undefined)
  const ownRef = child.props.ref
  const ref = useCallback(
    (node: Instance | null) => {
      instance.current = node
      setRef(ownRef, node)
    },
    [ownRef]
  )

  if (!props.popLayout) return child
  if (props.isPresent) {
    box.current = undefined
  } else if (box.current === undefined && instance.current !== null) {
    if (!gpuix.renderer) throw new Error('AnimatePresence mode="popLayout" needs a GPUIX root')
    box.current = measurePopLayout(gpuix.renderer, instance.current.id, instance.current.parentId)
  }
  const style = child.props.style
  return cloneElement(
    child,
    box.current === undefined ? { ref } : { ref, style: { ...style, ...box.current } }
  )
}

function PresenceChild(props: PresenceChildProps) {
  const children = usePoppedChild(props)
  const isPresent = props.isPresent
  const initial = props.initial
  const custom = props.custom
  const onExitComplete = props.onExitComplete
  const presenceChildren = useRef(
    new Map<string, PresenceContextValue["onExitComplete"] | null>()
  ).current
  const id = useId()
  const isPresentRef = useRef(isPresent)
  const onExitCompleteRef = useRef(onExitComplete)

  useLayoutEffect(() => {
    isPresentRef.current = isPresent
    onExitCompleteRef.current = onExitComplete
  }, [isPresent, onExitComplete])

  const register = useCallback((childId: string) => {
    presenceChildren.set(childId, null)
    return () => {
      presenceChildren.delete(childId)
      queueMicrotask(() => {
        if (!isPresentRef.current && presenceChildren.size === 0) {
          onExitCompleteRef.current?.()
        }
      })
    }
  }, [presenceChildren])

  const completeCycle = useMemo(() => {
    const complete: PresenceContextValue["onExitComplete"] = (childId) => {
      if (!presenceChildren.has(childId)) return
      presenceChildren.set(childId, complete)
      for (const completedBy of presenceChildren.values()) {
        if (completedBy !== complete) return
      }
      onExitCompleteRef.current?.()
    }
    return complete
  }, [isPresent, presenceChildren])

  const context = useMemo(
    (): PresenceContextValue => ({
      id,
      initial,
      custom,
      isPresent,
      onExitComplete: completeCycle,
      register,
    }),
    [completeCycle, custom, id, initial, isPresent, register]
  )

  useEffect(() => {
    if (!isPresent && presenceChildren.size === 0) onExitCompleteRef.current?.()
  }, [isPresent, presenceChildren])

  return createElement(PresenceContext.Provider, { value: context }, children)
}

export function AnimatePresence({
  children,
  initial = true,
  onExitComplete,
  mode = "sync",
  custom,
}: AnimatePresenceProps): ReactElement {
  const presentChildren = useMemo(() => onlyElements(children), [children])
  const presentKeys = new Set(presentChildren.map(getChildKey))
  const isInitialRender = useRef(true)
  const exitComplete = useRef(new Map<ComponentKey, boolean>()).current
  const [renderedChildren, setRenderedChildren] = useState(presentChildren)

  useLayoutEffect(() => {
    isInitialRender.current = false
    for (const child of renderedChildren) {
      const key = getChildKey(child)
      if (!presentKeys.has(key)) {
        if (exitComplete.get(key) !== true) exitComplete.set(key, false)
      } else {
        exitComplete.delete(key)
      }
    }
  }, [exitComplete, presentChildren, renderedChildren])

  const renderedKeys = new Set(renderedChildren.map(getChildKey))
  const isWaiting = mode === "wait" && renderedChildren.some((child) => !presentKeys.has(getChildKey(child)))
  const nextChildren = isWaiting
    ? presentChildren.filter((child) => renderedKeys.has(getChildKey(child)))
    : [...presentChildren]
  for (let i = 0; i < renderedChildren.length; i++) {
    const child = renderedChildren[i]
    const key = getChildKey(child)
    if (!presentKeys.has(key)) nextChildren.splice(i, 0, child)
  }

  useLayoutEffect(() => {
    if (
      nextChildren.length !== renderedChildren.length ||
      nextChildren.some((child, index) => child !== renderedChildren[index])
    ) {
      setRenderedChildren(nextChildren)
    }
  }, [nextChildren, renderedChildren])

  return createElement(
    Fragment,
    null,
    ...nextChildren.map((child) => {
      const key = getChildKey(child)
      const isPresent = presentKeys.has(key)
      const onExit = () => {
        if (exitComplete.get(key) === true) return
        exitComplete.set(key, true)
        let isEveryExitComplete = true
        exitComplete.forEach((isExitComplete) => {
          if (!isExitComplete) isEveryExitComplete = false
        })
        if (!isEveryExitComplete) return
        exitComplete.clear()
        setRenderedChildren(presentChildren)
        onExitComplete?.()
      }
      return createElement(PresenceChild, {
        key,
        isPresent,
        custom,
        popLayout: mode === "popLayout",
        initial: isInitialRender.current && initial === false ? false : undefined,
        onExitComplete: isPresent ? undefined : onExit,
        children: child,
      })
    })
  )
}
