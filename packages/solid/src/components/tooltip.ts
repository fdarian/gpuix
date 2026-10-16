import {
  createComponent,
  createContext,
  createSignal,
  onCleanup,
  Show,
  useContext,
  type JSX,
} from "solid-js"
import type { DomEvent } from "@gpuix/native/host"
import type { HostMouseEvent, HostPointerEvent, HostProps } from "@gpuix/native/host"
import { jsx } from "../jsx-runtime.js"
import {
  FloatingLayer,
  floatingRootStyle,
  overrideProps,
  renderSlot,
  type FloatingContentProps,
  DismissableLayer,
} from "./floating.js"

interface ProviderState {
  delayDuration: number
  skipDelayDuration: number
  disableHoverableContent: boolean
  lastClosedAt: { current: number }
}
const defaultProvider: ProviderState = {
  delayDuration: 0,
  skipDelayDuration: 300,
  disableHoverableContent: false,
  lastClosedAt: { current: Number.NEGATIVE_INFINITY },
}
const ProviderContext = createContext(defaultProvider)

export interface TooltipProviderProps {
  children: JSX.Element
  delayDuration?: number
  skipDelayDuration?: number
  disableHoverableContent?: boolean
}
export function TooltipProvider(props: TooltipProviderProps): JSX.Element {
  const value: ProviderState = {
    delayDuration: props.delayDuration ?? 0,
    skipDelayDuration: props.skipDelayDuration ?? 300,
    disableHoverableContent: props.disableHoverableContent ?? false,
    lastClosedAt: { current: Number.NEGATIVE_INFINITY },
  }
  return createComponent(ProviderContext.Provider, {
    value,
    get children() { return props.children },
  })
}

interface TooltipState {
  open(): boolean
  hoverable: boolean
  openNow(): void
  scheduleOpen(): void
  scheduleClose(): void
  cancelClose(): void
  close(): void
}
const TooltipContext = createContext<TooltipState>()
function context(name: string): TooltipState {
  const value = useContext(TooltipContext)
  if (!value) throw new Error(`${name} must be used inside Tooltip`)
  return value
}

export interface TooltipProps extends HostProps {
  children?: JSX.Element
  open?: boolean
  defaultOpen?: boolean
  onOpenChange?: (open: boolean) => void
  delayDuration?: number
  disableHoverableContent?: boolean
}
export function Tooltip(props: TooltipProps): JSX.Element {
  const provider = useContext(ProviderContext)
  const [internal, setInternal] = createSignal(props.defaultOpen ?? false)
  const open = () => props.open === undefined ? internal() : props.open
  let openTimer: ReturnType<typeof setTimeout> | undefined
  let closeTimer: ReturnType<typeof setTimeout> | undefined
  const cancelOpen = () => { if (openTimer) clearTimeout(openTimer); openTimer = undefined }
  const cancelClose = () => { if (closeTimer) clearTimeout(closeTimer); closeTimer = undefined }
  const setOpen = (next: boolean) => {
    const previous = open()
    cancelOpen()
    cancelClose()
    if (props.open === undefined) setInternal(next)
    if (previous !== next) props.onOpenChange?.(next)
    if (!next) provider.lastClosedAt.current = Date.now()
  }
  const hoverable = !(props.disableHoverableContent ?? provider.disableHoverableContent)
  const state: TooltipState = {
    open,
    hoverable,
    openNow: () => setOpen(true),
    scheduleOpen() {
      cancelClose()
      const recent = Date.now() - provider.lastClosedAt.current <= provider.skipDelayDuration
      const delay = recent ? 0 : (props.delayDuration ?? provider.delayDuration)
      if (delay <= 0) setOpen(true)
      else openTimer = setTimeout(() => setOpen(true), delay)
    },
    scheduleClose() {
      cancelOpen()
      if (!hoverable) setOpen(false)
      else closeTimer = setTimeout(() => setOpen(false), 80)
    },
    cancelClose,
    close: () => setOpen(false),
  }
  onCleanup(() => { cancelOpen(); cancelClose() })
  return createComponent(TooltipContext.Provider, {
    value: state,
    get children() {
      return jsx("div", overrideProps(props, {
        style: floatingRootStyle(props.style),
        get children() { return props.children },
      }))
    },
  })
}

export interface TooltipTriggerProps extends HostProps { children?: JSX.Element; asChild?: boolean }
export function TooltipTrigger(props: TooltipTriggerProps): JSX.Element {
  const state = context("TooltipTrigger")
  return renderSlot({
    asChild: props.asChild,
    children: props.children,
    props: overrideProps(props, {
      onMouseEnter(event: HostMouseEvent) { props.onMouseEnter?.(event); state.scheduleOpen() },
      onMouseLeave(event: HostMouseEvent) { props.onMouseLeave?.(event); state.scheduleClose() },
      onMouseDown(event: HostMouseEvent) { props.onMouseDown?.(event); state.close() },
      onClick(event: HostPointerEvent) { props.onClick?.(event); state.close() },
      onFocus(event: DomEvent) { props.onFocus?.(event); state.openNow() },
      onBlur(event: DomEvent) { props.onBlur?.(event); state.close() },
    }, ["children"]),
    defaultTabIndex: 0,
  })
}

export interface TooltipContentProps extends FloatingContentProps {}
export function TooltipContent(props: TooltipContentProps): JSX.Element {
  const state = context("TooltipContent")
  return createComponent(Show, {
    get when() { return state.open() },
    keyed: true,
    get children() {
      return createComponent(DismissableLayer, {
        onEscapeKeyDown: state.close,
        get children() {
          return createComponent(FloatingLayer, overrideProps(props, {
            get side() { return props.side ?? "top" },
            get align() { return props.align ?? "center" },
            onMouseEnter(event) {
              props.onMouseEnter?.(event)
              if (state.hoverable) state.cancelClose()
            },
            onMouseLeave(event) {
              props.onMouseLeave?.(event)
              state.scheduleClose()
            },
          }))
        },
      })
    },
  })
}

export {
  Tooltip as Root,
  TooltipContent as Content,
  TooltipProvider as Provider,
  TooltipTrigger as Trigger,
}
